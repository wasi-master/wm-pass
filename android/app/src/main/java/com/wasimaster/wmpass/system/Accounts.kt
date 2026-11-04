package com.wasimaster.wmpass.system

import android.app.Application
import android.content.Context
import android.net.Uri
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/** A vault that lives somewhere else, known to this phone. Holds no secret. */
@Serializable
data class AccountRef(
    val id: String,
    val kind: String,
    val label: String,
    val server: String = "",
    val region: String = "",
    val email: String = "",
    val uri: String = "",
    /** False until the first sign-in goes through, so a setup that never worked can be fixed or dropped. */
    val opened: Boolean = true,
)

object Accounts {
    private val mutableList = MutableStateFlow<List<AccountRef>>(emptyList())
    val list: StateFlow<List<AccountRef>> = mutableList

    /** How the last trip to a server went: null for fine, or words to show. */
    val syncProblem = MutableStateFlow<String?>(null)

    /** The account just added, for the door to have ready the next time it shows. */
    private var signInNext: String? = null

    fun signInNext(id: String) {
        signInNext = id
    }

    fun takeSignInNext(): String? = signInNext.also { signInNext = null }

    private val serializer = ListSerializer(AccountRef.serializer())

    private fun store(context: Context) = context.getSharedPreferences("accounts", Context.MODE_PRIVATE)

    fun load(context: Context) {
        val text = store(context).getString("list", null) ?: return
        mutableList.value = runCatching { Json.decodeFromString(serializer, text) }.getOrDefault(emptyList())
    }

    private fun save(context: Context, value: List<AccountRef>) {
        mutableList.value = value
        store(context).edit().putString("list", Json.encodeToString(serializer, value)).apply()
    }

    fun add(context: Context, ref: AccountRef) = save(context, mutableList.value.filter { it.id != ref.id } + ref)

    /**
     * Changes an account's details in place. A new server, email or file is another account to
     * the server, so the offline copy of the old one goes.
     */
    fun update(context: Context, ref: AccountRef) {
        val old = mutableList.value.firstOrNull { it.id == ref.id } ?: return add(context, ref)
        if (old.server != ref.server || old.region != ref.region || old.email != ref.email || old.uri != ref.uri) {
            cacheFile(context, ref.id).delete()
        }
        save(context, mutableList.value.map { if (it.id == ref.id) ref else it })
    }

    private fun markOpened(context: Context, id: String) {
        if (mutableList.value.none { it.id == id && !it.opened }) return
        save(context, mutableList.value.map { if (it.id == id) it.copy(opened = true) else it })
    }

    /**
     * Whether a Bitwarden-compatible server answers at this address, before the account is kept.
     * Null when it does, or the words to show. It cannot tell whether the email has an account.
     */
    suspend fun checkServer(context: Context, server: String, region: String, email: String): String? = runCatching {
        Engine.net("bitwarden.check", online = online(context)) {
            put("server", server)
            put("region", region)
            put("email", email)
        }
    }.exceptionOrNull()?.let { failure ->
        (failure.message ?: "The server could not be reached").replaceFirstChar { it.uppercase() }.trimEnd('.') + "."
    }

    fun remove(context: Context, id: String) {
        save(context, mutableList.value.filter { it.id != id })
        cacheFile(context, id).delete()
        bakFile(context, id).delete()
    }

    /** On a wipe: no sign of them remains, offline copies included. */
    fun forget(context: Context) {
        mutableList.value = emptyList()
        store(context).edit().clear().apply()
        folder(context).deleteRecursively()
    }

    private fun folder(context: Context) = File(context.filesDir, "accounts").apply { mkdirs() }
    private fun cacheFile(context: Context, id: String) = File(folder(context), "$id.cache")
    private fun bakFile(context: Context, id: String) = File(folder(context), "$id.bak")

    private fun deviceId(context: Context): String {
        val prefs = store(context)
        prefs.getString("device", null)?.let { return it }
        val made = java.util.UUID.randomUUID().toString()
        prefs.edit().putString("device", made).apply()
        return made
    }

    private fun online(context: Context) = NetAccess.allowed(context)

    /**
     * Opens a KeePass file in place. It stays where it is and is written back on each save.
     * [answer] has a YubiKey answer the file's challenge, for a database locked with one.
     */
    suspend fun openKdbx(
        context: Context,
        ref: AccountRef,
        password: ByteArray,
        keyFile: ByteArray? = null,
        answer: (suspend (ByteArray) -> ByteArray)? = null,
    ) {
        val uri = Uri.parse(ref.uri)
        val data = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: throw NativeFailure("file\nThat file could not be read.")
        val response = answer?.invoke(data)
        val reply = try {
            Engine.call(
                "kdbx.open",
                args = {
                    put("name", ref.label)
                    put("response", response != null)
                },
                secret = password,
                blobs = listOfNotNull(data, keyFile, response),
            )
        } finally {
            response?.fill(0)
        }
        Pass.adoptAccount(ref.id, reply) { bytes ->
            // The last good copy stays here, encrypted as the file itself is, in case the
            // write below is cut off halfway.
            bakFile(context, ref.id).writeBytes(data)
            context.contentResolver.openOutputStream(uri, "wt")?.use { it.write(bytes) }
        }
        markOpened(context, ref.id)
        // KDBX 3 is read but written as KDBX 4, which apps from before 2017 cannot open.
        val upgrade = (reply.json as? kotlinx.serialization.json.JsonObject)?.get("kdbx_upgrade")
        if (upgrade is kotlinx.serialization.json.JsonPrimitive && upgrade.content == "true") {
            kotlinx.coroutines.withContext(Dispatchers.Main) {
                android.widget.Toast.makeText(
                    context,
                    "This file is in the older KDBX 3 format. The first change saved rewrites it as KDBX 4, which KeePass 2.35 or later, KeePassXC 2.3 or later, KeePassDX and Strongbox open. WM Pass keeps a copy of the file as it was.",
                    android.widget.Toast.LENGTH_LONG,
                ).show()
            }
        }
    }

    /**
     * Signs in to a Bitwarden-compatible server, or opens the offline copy if there is one. [code]
     * is a two-step code, or with [deviceCode] the one Bitwarden emails to confirm a new device.
     */
    suspend fun openBitwarden(context: Context, ref: AccountRef, password: ByteArray, code: String? = null, deviceCode: Boolean = false) {
        val cache = cacheFile(context, ref.id)
        val reply = if (cache.exists() && code == null) {
            Engine.net("bitwarden.open_cached", online = online(context), secret = password) {
                put("server", ref.server)
                put("region", ref.region)
                put("cache", android.util.Base64.encodeToString(cache.readBytes(), android.util.Base64.NO_WRAP))
            }
        } else {
            Engine.net("bitwarden.login", online = online(context), secret = password) {
                put("server", ref.server)
                put("region", ref.region)
                put("email", ref.email)
                put("device", deviceId(context))
                code?.let { put(if (deviceCode) "device_code" else "code", it) }
            }
        }
        val scope = CoroutineScope(Dispatchers.IO)
        markOpened(context, ref.id)
        Pass.adoptAccount(ref.id, reply) { bytes -> cache.writeBytes(bytes) }
        Pass.afterAccountSave = {
            scope.launch { syncNow(context) }
        }
        if (cache.exists()) scope.launch { syncNow(context) } else Pass.flush()
    }

    /** Takes local changes to the server and brings its changes back. Failing leaves them queued. */
    suspend fun syncNow(context: Context) {
        val problem = runCatching {
            Pass.net("remote.sync", online(context))
            Pass.reload()
        }.exceptionOrNull()
        syncProblem.value = problem?.message
    }

    suspend fun open(context: Context, ref: AccountRef, password: ByteArray) {
        when (ref.kind) {
            "kdbx" -> openKdbx(context, ref, password)
            else -> openBitwarden(context, ref, password)
        }
    }

    fun newId(): String = java.util.UUID.randomUUID().toString().take(12)
}

/** Where an account lives, as a person would name it. */
fun accountHost(ref: AccountRef): String = when {
    ref.kind == "kdbx" -> "KeePass file"
    ref.server.isNotEmpty() -> runCatching { android.net.Uri.parse(ref.server).host }.getOrNull() ?: ref.server
    ref.region == "eu" -> "bitwarden.eu"
    else -> "bitwarden.com"
}

/** Whether anything may use the network right now. */
object NetAccess {
    fun allowed(context: Context): Boolean =
        !com.wasimaster.wmpass.BuildConfig.OFFLINE && Prefs.of(context).networkAllowed
}

/** A device paired for sync on the local network, and how the last trip to it went. */
@Serializable
data class LanPeer(
    val key: String,
    /** The name it gave itself when pairing. */
    val name: String,
    /** What the person calls it on this phone, or blank for [name]. */
    val nickname: String = "",
    /** When the last sync or merge with it ended, or 0 for never. */
    val lastAt: Long = 0,
    val lastOk: Boolean = true,
    /** Words for the last result: counts after a success, the reason after a failure. */
    val lastText: String = "",
    /** "synced" or "merged" after a success, the engine's failure code after a failure. */
    val lastCode: String = "",
) {
    /** The name to show for it. */
    val shown: String get() = nickname.ifBlank { name }
}

/** How one paired device fared in a sync or merge. */
data class LanResult(val peer: LanPeer, val ok: Boolean, val text: String, val code: String = "", val received: Int = 0)

/** What waiting for a paired device came to. */
sealed interface LanServed {
    data class Done(val result: LanResult) : LanServed

    /** The device wants to merge vaults; answer with [Lan.mergeAnswer]. */
    data class MergeAsked(val peer: LanPeer, val theyKeep: Boolean) : LanServed
}

/** How a merge of two vaults ended. */
sealed interface LanMerged {
    val peer: LanPeer

    /** This phone kept its vault and took the other device's items. */
    data class Kept(override val peer: LanPeer, val added: Int, val matched: Int) : LanMerged

    /** This phone gave its items and now holds the other device's vault, locked. */
    data class Replaced(override val peer: LanPeer) : LanMerged

    data class Failed(override val peer: LanPeer, val text: String, val code: String) : LanMerged
}

/** This phone's identity for the local network, and who it trusts there. */
object Lan {
    private const val IDENTITY = "lan_identity"
    private const val PEERS = "lan_peers"
    private val peerList = ListSerializer(LanPeer.serializer())

    /** Lists stored before nicknames and statuses existed still read. */
    private val json = Json { ignoreUnknownKeys = true }

    /** Rises on every change to the paired devices or their statuses, for screens that show them. */
    private val mutableRevision = MutableStateFlow(0)
    val revision: StateFlow<Int> = mutableRevision

    /** One trip on the network at a time: a sync, a wait or a merge. */
    private val busy = Mutex()

    /** Set by Stop, so a wait that saw a stranger does not start waiting again. */
    @Volatile
    private var stopAsked = false

    /** The caller wipes what comes back. */
    suspend fun identity(context: Context): ByteArray {
        Sealed.read(context, IDENTITY)?.let { return it }
        val made = Engine.net("lan.new_identity", online = false).secret!!.consume { it.copyOf() }
        Sealed.write(context, IDENTITY, made)
        return made
    }

    fun peers(context: Context): List<LanPeer> =
        Sealed.read(context, PEERS)?.let { runCatching { json.decodeFromString(peerList, it.decodeToString()) }.getOrNull() }.orEmpty()

    private fun write(context: Context, all: List<LanPeer>) {
        Sealed.write(context, PEERS, json.encodeToString(peerList, all).encodeToByteArray())
        mutableRevision.value++
    }

    private fun edit(context: Context, change: (List<LanPeer>) -> List<LanPeer>) = synchronized(this) {
        write(context, change(peers(context)))
    }

    /** Pairing again with a device already known keeps the name it was given here. */
    fun addPeer(context: Context, peer: LanPeer) = edit(context) { all ->
        val old = all.firstOrNull { it.key == peer.key }
        all.filter { it.key != peer.key } + peer.copy(nickname = peer.nickname.ifBlank { old?.nickname.orEmpty() })
    }

    fun removePeer(context: Context, key: String) = edit(context) { all -> all.filter { it.key != key } }

    fun rename(context: Context, key: String, nickname: String) = edit(context) { all ->
        all.map { if (it.key == key) it.copy(nickname = nickname.trim()) else it }
    }

    private fun note(context: Context, key: String, ok: Boolean, text: String, code: String) = edit(context) { all ->
        all.map { if (it.key == key) it.copy(lastAt = System.currentTimeMillis(), lastOk = ok, lastText = text, lastCode = code) else it }
    }

    /** What to say about a failure, by its code. */
    fun failureWords(code: String, message: String): String = when (code) {
        "not_this_vault" -> "Has a different vault. Merge the two to sync them."
        "io" -> "The connection was lost."
        "version" -> "Update WM Pass on both devices."
        "declined" -> "Declined on that device."
        else -> message.ifBlank { "Something went wrong" }.replaceFirstChar { it.uppercase() }.trimEnd('.') + "."
    }

    /** One line for the screen about a whole sync. */
    fun summary(results: List<LanResult>): String = results.joinToString(" ") { result ->
        if (result.ok) "Synced with ${result.peer.shown}: ${result.text}." else "${result.peer.shown}: ${result.text}"
    }

    private fun JsonObjectBuilder.peerArgs(context: Context, only: List<LanPeer>? = null) {
        putJsonArray("peers") {
            (only ?: peers(context)).forEach { peer ->
                addJsonObject {
                    put("key", peer.key)
                    put("name", peer.name)
                }
            }
        }
        Pass.blobFolder()?.let { put("blob_dir", it.absolutePath) }
    }

    // ---- a new device ----

    /** Starts waiting as a phone with no vault, and gives the code to show. */
    suspend fun joinListen(address: String?): String =
        Engine.net("lan.join_listen", online = false) { put("address", address.orEmpty()) }.secret!!.consumeText()

    /** Takes the next connection: the vault itself, or six digits to compare first. */
    suspend fun joinWait(context: Context, name: String, blobs: File): com.wasimaster.wmpass.core.EngineReply {
        val identity = identity(context)
        try {
            return Engine.net("lan.join_wait", online = false, secret = identity) {
                put("name", name)
                put("blob_dir", blobs.absolutePath)
            }
        } finally {
            identity.fill(0)
        }
    }

    suspend fun joinFinish(accepted: Boolean, blobs: File): com.wasimaster.wmpass.core.EngineReply =
        Engine.net("lan.join_finish", online = false) {
            put("accepted", accepted)
            put("blob_dir", blobs.absolutePath)
        }

    // ---- sending this vault to a new device ----

    suspend fun joinFind(): List<String> =
        Engine.net("lan.join_find", online = false) { put("ms", 4000) }.json.jsonArray.map { it.jsonPrimitive.content }

    suspend fun joinScan(context: Context, text: String, name: String): LanPeer {
        Pass.flush()
        val identity = identity(context)
        try {
            val reply = Pass.net("lan.join_scan", online = false, secret = identity) {
                put("text", text)
                put("name", name)
                peerArgs(context)
            }
            return peerOf(reply.obj["peer"]!!.jsonObject).also { addPeer(context, it) }
        } finally {
            identity.fill(0)
        }
    }

    suspend fun joinConnect(context: Context, address: String, name: String): com.wasimaster.wmpass.core.EngineReply {
        val identity = identity(context)
        try {
            return Engine.net("lan.join_connect", online = false, secret = identity) {
                put("address", address)
                put("name", name)
            }
        } finally {
            identity.fill(0)
        }
    }

    suspend fun joinSend(context: Context, accepted: Boolean): LanPeer {
        Pass.flush()
        val reply = Pass.net("lan.join_send", online = false) {
            put("accepted", accepted)
            peerArgs(context)
        }
        return peerOf(reply.obj["peer"]!!.jsonObject).also { addPeer(context, it) }
    }

    fun peerOf(obj: JsonObject) = LanPeer(obj["key"]!!.jsonPrimitive.content, obj["name"]!!.jsonPrimitive.content)

    /** The paired device behind [obj], with the name it has here. */
    private fun known(context: Context, obj: JsonObject?): LanPeer? {
        val key = obj?.get("key")?.jsonPrimitive?.contentOrNull ?: return null
        return peers(context).firstOrNull { it.key == key } ?: obj["name"]?.jsonPrimitive?.contentOrNull?.let { LanPeer(key, it) }
    }

    suspend fun browse(): List<String> =
        Engine.net("lan.browse", online = false) { put("ms", 4000) }.json.jsonArray.map { it.jsonPrimitive.content }

    private fun JsonObject.number(name: String) = get(name)?.jsonPrimitive?.intOrNull ?: 0

    private fun syncedText(sent: Int, received: Int) = if (sent == 0 && received == 0) "nothing to change" else "$sent sent, $received received"

    /** A sync report or failure as one device's result, noted against it. Null for a device not asked for. */
    private suspend fun resultOf(context: Context, reply: JsonObject, wanted: List<LanPeer>): LanResult? {
        reply["failed"]?.jsonObject?.let { failed ->
            val code = failed["code"]?.jsonPrimitive?.contentOrNull.orEmpty()
            val key = failed["key"]?.jsonPrimitive?.contentOrNull
            // A device this phone does not know, or one it was not looking for, or a vault busy
            // with another sync: the next try settles it.
            if (code == "unknown_peer" || code == "busy") return null
            val peer = wanted.firstOrNull { it.key == key } ?: return null
            val text = failureWords(code, failed["message"]?.jsonPrimitive?.contentOrNull.orEmpty())
            note(context, peer.key, false, text, code)
            return LanResult(peer, false, text, code)
        }
        val key = reply["key"]?.jsonPrimitive?.contentOrNull
        val peer = wanted.firstOrNull { it.key == key } ?: return null
        val received = reply.number("received") + reply.number("blobs_received")
        withContext(NonCancellable) {
            Pass.reload()
            ownFlush(received > 0)
        }
        val text = syncedText(reply.number("sent"), reply.number("received"))
        note(context, peer.key, true, text, "synced")
        return LanResult(peer, true, text, "synced", received)
    }

    private fun notFound(only: LanPeer?) = NativeFailure(
        "lan\n" + (if (only != null) "${only.shown} did not answer on this network." else "No paired device answered on this network.") +
            " On a phone, tap Wait for one. A computer answers while WM Pass is unlocked on it.",
    )

    /**
     * Finds paired devices on this network and syncs with each, or only with [onlyKey]. The key
     * exchange decides who each one really is. A device that answers but fails has its reason
     * noted against it; throws only when none of them answered at all.
     */
    suspend fun sync(context: Context, onlyKey: String? = null): List<LanResult> = busy.withLock { syncLocked(context, onlyKey) }

    /** Syncs with one paired device. */
    suspend fun syncWith(context: Context, peerKey: String): LanResult = sync(context, peerKey).first()

    private suspend fun syncLocked(context: Context, onlyKey: String?): List<LanResult> {
        val wanted = peers(context).filter { onlyKey == null || it.key == onlyKey }
        if (wanted.isEmpty()) throw NativeFailure("lan\nNo device is paired with this phone yet.")
        val only = wanted.singleOrNull()?.takeIf { onlyKey != null }
        ownFlush(false)
        // Not this phone's own background listener.
        val found = browse().filterNot(::isOwnAddress)
        if (found.isEmpty()) throw notFound(only)
        val identity = identity(context)
        val results = mutableListOf<LanResult>()
        try {
            for (address in found) {
                val left = wanted.filter { peer -> results.none { it.peer.key == peer.key } }
                if (left.isEmpty()) break
                // Only the devices still to reach: any other that answers is turned away at once.
                val reply = try {
                    Pass.net("lan.sync", online = false, secret = identity) {
                        put("address", address)
                        peerArgs(context, left)
                    }
                } catch (_: NativeFailure) {
                    // Could not reach that address at all.
                    continue
                }
                resultOf(context, reply.obj, left)?.let { results += it }
            }
        } finally {
            identity.fill(0)
        }
        if (results.isEmpty()) throw notFound(only)
        return results
    }

    /** Ends a wait on this network, from the screen's Stop or as it closes. */
    fun stopWaiting() {
        stopAsked = true
        CoroutineScope(Dispatchers.IO).launch { runCatching { Engine.net("lan.stop", online = false) } }
    }

    /**
     * Waits on this network for a paired device: to sync, or to ask to merge vaults. A device that
     * connects and leaves without a word was looking for another one, and is ignored.
     */
    suspend fun serve(context: Context): LanServed = busy.withLock {
        stopAsked = false
        val identity = identity(context)
        val until = System.currentTimeMillis() + 115_000
        try {
            val port = Engine.net("lan.listen", online = false).obj["port"]!!.jsonPrimitive.content.toInt()
            Engine.net("lan.advertise", online = false) { put("port", port) }
            var outcome: LanServed? = null
            while (outcome == null) {
                val wanted = peers(context)
                val reply = Pass.net("lan.serve", online = false, secret = identity) {
                    peerArgs(context, wanted)
                    put("background", false)
                }.obj
                val asked = reply["merge_asked"]?.jsonObject
                outcome = if (asked != null) {
                    val peer = known(context, asked["peer"]?.jsonObject) ?: throw NativeFailure("lan\nThe other device did not say who it is.")
                    LanServed.MergeAsked(peer, asked["they_keep"]?.jsonPrimitive?.booleanOrNull == true)
                } else {
                    resultOf(context, reply, wanted)?.let { LanServed.Done(it) }
                }
                if (outcome == null && (stopAsked || System.currentTimeMillis() > until)) throw NativeFailure("cancelled\nstopped waiting")
            }
            checkNotNull(outcome)
        } finally {
            identity.fill(0)
            runCatching { Engine.net("lan.stop", online = false) }
        }
    }

    /**
     * Answers a device that asked to merge while this phone waited. Null when declined. Runs to
     * the end even if the screen goes, since this phone's vault may be replaced on the way.
     */
    suspend fun mergeAnswer(context: Context, asker: LanPeer, accepted: Boolean): LanMerged? = withContext(NonCancellable) {
        busy.withLock {
            val name = Pass.freshVaultName()
            val newBlobs = Pass.blobFolderFor(name)
            val reply = try {
                Pass.net("lan.merge_answer", online = false) {
                    put("accepted", accepted)
                    Pass.blobFolder()?.let { put("blob_dir", it.absolutePath) }
                    put("new_blob_dir", newBlobs.absolutePath)
                }
            } catch (failure: NativeFailure) {
                newBlobs.deleteRecursively()
                throw failure
            }
            if (!accepted) {
                newBlobs.deleteRecursively()
                return@withLock null
            }
            finishMerge(context, reply, asker, name, newBlobs)
        }
    }

    /**
     * Merges this vault with [peerKey]'s, which must be waiting for it. With [keepMine] the
     * other device's items come here and it takes this vault; without, this phone's items go
     * there and this phone takes that vault. Waits up to three minutes for the person there.
     */
    suspend fun merge(context: Context, peerKey: String, keepMine: Boolean): LanMerged = withContext(NonCancellable) {
        busy.withLock {
            val peer = peers(context).firstOrNull { it.key == peerKey } ?: throw NativeFailure("lan\nThat device is no longer paired.")
            if (!Pass.isFolderVault) throw NativeFailure("lan\nOnly the vault kept on this phone can be merged.")
            ownFlush(false)
            val found = browse().filterNot(::isOwnAddress)
            val name = Pass.freshVaultName()
            val newBlobs = Pass.blobFolderFor(name)
            val identity = identity(context)
            try {
                for (address in found) {
                    val reply = try {
                        Pass.net("lan.merge", online = false, secret = identity) {
                            put("address", address)
                            peerArgs(context, listOf(peer))
                            put("keep_mine", keepMine)
                            put("new_blob_dir", newBlobs.absolutePath)
                        }
                    } catch (_: NativeFailure) {
                        continue
                    }
                    val failed = reply.obj["failed"]?.jsonObject
                    if (failed != null) {
                        val code = failed["code"]?.jsonPrimitive?.contentOrNull
                        val key = failed["key"]?.jsonPrimitive?.contentOrNull
                        // Some other device answered at that address.
                        if (code == "unknown_peer" || key != peer.key) continue
                    }
                    return@withLock finishMerge(context, reply, peer, name, newBlobs)
                }
            } finally {
                identity.fill(0)
            }
            newBlobs.deleteRecursively()
            throw NativeFailure("lan\n${peer.shown} did not answer on this network. On it, tap Wait for one first.")
        }
    }

    /** Puts a merge's outcome in place: the vault reloaded, or replaced by the one kept. */
    private suspend fun finishMerge(context: Context, reply: com.wasimaster.wmpass.core.EngineReply, fallback: LanPeer, name: String, newBlobs: File): LanMerged {
        val obj = reply.obj
        val peer = known(context, obj["peer"]?.jsonObject) ?: fallback
        obj["failed"]?.jsonObject?.let { failed ->
            newBlobs.deleteRecursively()
            val code = failed["code"]?.jsonPrimitive?.contentOrNull.orEmpty()
            val text = failureWords(code, failed["message"]?.jsonPrimitive?.contentOrNull.orEmpty())
            note(context, peer.key, false, text, code)
            return LanMerged.Failed(peer, text, code)
        }
        return when (obj["merged"]?.jsonPrimitive?.contentOrNull) {
            "kept" -> {
                if (newBlobs.list().isNullOrEmpty()) newBlobs.deleteRecursively()
                Pass.reload()
                ownFlush(true)
                val added = obj.number("added")
                val matched = obj.number("matched")
                note(context, peer.key, true, "$added added, $matched already here", "merged")
                LanMerged.Kept(peer, added, matched)
            }
            "replaced" -> {
                val bytes = reply.blobs.firstOrNull() ?: throw NativeFailure("lan\nThe other device's vault did not arrive.")
                note(context, peer.key, true, "this phone now uses its vault", "merged")
                Pass.replaceWith(bytes, name)
                withContext(Dispatchers.Main) {
                    android.widget.Toast.makeText(
                        context.applicationContext,
                        "This phone now uses ${peer.shown}'s vault. Unlock it with that vault's master password.",
                        android.widget.Toast.LENGTH_LONG,
                    ).show()
                }
                LanMerged.Replaced(peer)
            }
            else -> {
                newBlobs.deleteRecursively()
                throw NativeFailure("lan\nThe other device gave an answer this version does not know. Update WM Pass on both devices.")
            }
        }
    }

    // ---- syncing by itself ----

    private val autoScope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private lateinit var app: Application
    private var lastAuto = 0L
    private var changeJob: Job? = null

    /** How often an open vault reaches its paired devices, with no edit to set it off. */
    private const val AUTO_SYNC_MS = 5 * 60_000L

    @Volatile
    private var foreground = false

    /** Screen flows that use the network themselves: pairing, joining, Wait for one, a merge. */
    @Volatile
    private var pauses = 0

    /** Set while a sync here saves what it did, so that save does not start another sync. */
    @Volatile
    private var ownSave = false

    @Volatile
    private var ownSaveReceived = false

    /** Whether the save under way is one a sync on this network made itself. */
    val savingOwn: Boolean get() = ownSave

    /** Whether that save brought something from another device, which the sync places should get. */
    val savingReceived: Boolean get() = ownSave && ownSaveReceived

    /** A device asking to merge while this phone waited in the background. The vault's shell asks the person. */
    val mergeAsk = MutableStateFlow<LanServed.MergeAsked?>(null)

    private var backgroundJob: Job? = null

    /** Starting and stopping the background listener, one at a time. */
    private val backgroundCalls = Mutex()

    /** Rises each time the background listener starts or stops, so a loop left over knows to end. */
    @Volatile
    private var generation = 0

    @Volatile
    private var backgroundPort = 0

    private suspend fun ownFlush(received: Boolean) {
        ownSave = true
        ownSaveReceived = received
        try {
            Pass.flush()
        } finally {
            ownSave = false
            ownSaveReceived = false
        }
    }

    /** Whether a browsed address is this phone's own background listener. */
    private fun isOwnAddress(address: String): Boolean {
        val port = backgroundPort.takeIf { it != 0 } ?: return false
        if (!address.endsWith(":$port")) return false
        val host = address.substringBeforeLast(':').removePrefix("[").removeSuffix("]").substringBefore('%')
        return runCatching {
            java.net.NetworkInterface.getNetworkInterfaces().toList().flatMap { it.inetAddresses.toList() }
                .any { it.hostAddress?.substringBefore('%') == host }
        }.getOrDefault(false)
    }

    /**
     * Keeps paired devices together with no one asking. While the app is in front with the vault
     * open, this phone waits for them in the background, as a computer does, and it reaches out
     * to them itself: as the vault unlocks, as the app comes back, about every five minutes, and
     * twenty seconds after the last change.
     */
    fun startAuto(application: Application) {
        app = application
        autoScope.launch {
            Pass.state.collect { state ->
                refreshBackground()
                if (state is LockState.Unlocked) {
                    delay(1_500)
                    autoSync(app)
                } else {
                    changeJob?.cancel()
                }
            }
        }
        autoScope.launch { revision.collect { refreshBackground() } }
        autoScope.launch {
            while (true) {
                // Spread out, so two devices on the same timer do not keep meeting each other.
                delay((AUTO_SYNC_MS * (0.8 + Math.random() * 0.4)).toLong())
                refreshBackground()
                if (foreground) autoSync(app, force = true)
            }
        }
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) {
                    foreground = true
                    refreshBackground()
                    autoScope.launch { autoSync(app) }
                }

                override fun onStop(owner: LifecycleOwner) {
                    foreground = false
                    refreshBackground()
                }
            },
        )
    }

    /** A little after the last change saved here, so a burst of edits makes one sync. */
    fun soon(context: Context) {
        if (ownSave || FileSync.savingOwn) return
        autoScope.launch {
            changeJob?.cancel()
            changeJob = autoScope.launch {
                delay(20_000)
                autoSync(context, force = true)
            }
        }
    }

    /** While a screen uses the network itself, the background listener steps aside. Main thread only. */
    fun pause(on: Boolean) {
        pauses = (pauses + if (on) 1 else -1).coerceAtLeast(0)
        refreshBackground()
    }

    private fun backgroundWanted(): Boolean {
        val state = Pass.state.value
        return foreground && pauses == 0 && state is LockState.Unlocked && !state.readOnly && Pass.isFolderVault &&
            NetAccess.allowed(app) && peers(app).isNotEmpty()
    }

    /** Starts or stops the background listener, by whether it should run now. */
    fun refreshBackground() {
        if (!::app.isInitialized) return
        autoScope.launch {
            val want = backgroundWanted()
            backgroundCalls.withLock {
                val running = backgroundJob?.isActive == true
                if (want && !running) {
                    val mine = ++generation
                    backgroundJob = autoScope.launch(Dispatchers.IO) { backgroundLoop(mine) }
                } else if (!want && backgroundJob != null) {
                    generation++
                    backgroundJob?.cancel()
                    backgroundJob = null
                    backgroundPort = 0
                    mergeAsk.value = null
                    withContext(Dispatchers.IO) { runCatching { Engine.net("lan.stop_background", online = false) } }
                }
            }
        }
    }

    private suspend fun backgroundLoop(mine: Int) {
        val wifi = app.getSystemService(android.net.wifi.WifiManager::class.java)
        val lock = wifi?.createMulticastLock("wmpass-lan-background")?.apply { setReferenceCounted(false) }
        runCatching { lock?.acquire() }
        try {
            var attempts = 0
            while (generation == mine) {
                try {
                    val ready = backgroundCalls.withLock {
                        if (generation != mine) return@withLock false
                        // Once per open vault; asking again while it runs changes nothing.
                        backgroundPort = Engine.net("lan.listen_background", online = false).obj["port"]?.jsonPrimitive?.intOrNull ?: 0
                        true
                    }
                    if (!ready) break
                    // Run to the end once a device has connected: a sync half taken in is no use.
                    withContext(NonCancellable) {
                        val wanted = peers(app)
                        val identity = identity(app)
                        val reply = try {
                            Pass.net("lan.serve", online = false, secret = identity) {
                                peerArgs(app, wanted)
                                put("background", true)
                            }
                        } finally {
                            identity.fill(0)
                        }
                        backgroundReply(reply.obj, wanted)
                    }
                    attempts = 0
                } catch (failure: NativeFailure) {
                    if (failure.code == "cancelled" || generation != mine) break
                    attempts++
                    delay(minOf(2_000L * attempts, 60_000L))
                }
            }
        } finally {
            runCatching { lock?.release() }
        }
    }

    private suspend fun backgroundReply(reply: JsonObject, wanted: List<LanPeer>) {
        reply["merge_asked"]?.jsonObject?.let { asked ->
            val peer = known(app, asked["peer"]?.jsonObject) ?: return
            mergeAsk.value = LanServed.MergeAsked(peer, asked["they_keep"]?.jsonPrimitive?.booleanOrNull == true)
            // The next device waits until the person has answered this one.
            mergeAsk.first { it == null }
            return
        }
        val result = resultOf(app, reply, wanted) ?: return
        if (result.ok && result.received > 0) withContext(Dispatchers.Main) { cameIn(listOf(result)) }
    }

    private fun cameIn(results: List<LanResult>) {
        val came = results.filter { it.ok && it.received > 0 }
        if (came.isEmpty()) return
        android.widget.Toast.makeText(
            app,
            "Synced with ${came.joinToString(" and ") { it.peer.shown }}. Changes came in.",
            android.widget.Toast.LENGTH_SHORT,
        ).show()
    }

    /** One quiet sync with every paired device. Errors only change the devices' statuses. */
    private suspend fun autoSync(context: Context, force: Boolean = false) {
        val state = Pass.state.value
        if (state !is LockState.Unlocked || state.readOnly || !Pass.isFolderVault) return
        if (!NetAccess.allowed(context) || pauses > 0) return
        val now = System.currentTimeMillis()
        if (!force && now - lastAuto < 60_000) return
        if (peers(context).isEmpty()) return
        // Something on the network already under way, by hand: this one is not needed.
        if (!busy.tryLock()) return
        lastAuto = now
        val wifi = context.applicationContext.getSystemService(android.net.wifi.WifiManager::class.java)
        val lock = wifi?.createMulticastLock("wmpass-lan-auto")?.apply { setReferenceCounted(false) }
        val results = try {
            runCatching { lock?.acquire() }
            withContext(Dispatchers.IO) { runCatching { syncLocked(context, null) }.getOrNull() }
        } finally {
            runCatching { lock?.release() }
            busy.unlock()
        }
        cameIn(results.orEmpty())
    }
}

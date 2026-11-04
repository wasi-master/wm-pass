package com.wasimaster.wmpass.system

import android.content.Context
import android.net.Uri
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import java.io.File
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.addJsonObject
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

    /** Opens a KeePass file in place. It stays where it is and is written back on each save. */
    suspend fun openKdbx(context: Context, ref: AccountRef, password: ByteArray, keyFile: ByteArray? = null) {
        val uri = Uri.parse(ref.uri)
        val data = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: throw NativeFailure("file\nThat file could not be read.")
        val reply = Engine.call(
            "kdbx.open",
            args = { put("name", ref.label) },
            secret = password,
            blobs = listOfNotNull(data, keyFile),
        )
        Pass.adoptAccount(ref.id, reply) { bytes ->
            // The last good copy stays here, encrypted as the file itself is, in case the
            // write below is cut off halfway.
            bakFile(context, ref.id).writeBytes(data)
            context.contentResolver.openOutputStream(uri, "wt")?.use { it.write(bytes) }
        }
        markOpened(context, ref.id)
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

/** A device paired for sync on the local network. */
@Serializable
data class LanPeer(val key: String, val name: String)

/** This phone's identity for the local network, and who it trusts there. */
object Lan {
    private const val IDENTITY = "lan_identity"
    private const val PEERS = "lan_peers"
    private val peerList = ListSerializer(LanPeer.serializer())

    /** The caller wipes what comes back. */
    suspend fun identity(context: Context): ByteArray {
        Sealed.read(context, IDENTITY)?.let { return it }
        val made = Engine.net("lan.new_identity", online = false).secret!!.consume { it.copyOf() }
        Sealed.write(context, IDENTITY, made)
        return made
    }

    fun peers(context: Context): List<LanPeer> =
        Sealed.read(context, PEERS)?.let { runCatching { Json.decodeFromString(peerList, it.decodeToString()) }.getOrNull() }.orEmpty()

    fun addPeer(context: Context, peer: LanPeer) {
        val all = peers(context).filter { it.key != peer.key } + peer
        Sealed.write(context, PEERS, Json.encodeToString(peerList, all).encodeToByteArray())
    }

    fun removePeer(context: Context, key: String) {
        Sealed.write(context, PEERS, Json.encodeToString(peerList, peers(context).filter { it.key != key }).encodeToByteArray())
    }

    private fun JsonObjectBuilder.peerArgs(context: Context) {
        putJsonArray("peers") {
            peers(context).forEach { peer ->
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

    fun peerOf(obj: kotlinx.serialization.json.JsonObject) = LanPeer(obj["key"]!!.jsonPrimitive.content, obj["name"]!!.jsonPrimitive.content)

    suspend fun browse(): List<String> =
        Engine.net("lan.browse", online = false) { put("ms", 4000) }.json.jsonArray.map { it.jsonPrimitive.content }

    /** Finds a paired device and syncs. The key exchange decides who it really is. */
    suspend fun sync(context: Context): com.wasimaster.wmpass.core.EngineReply {
        val found = browse()
        if (found.isEmpty()) throw NativeFailure("lan\nNo paired device is waiting on this network. Tap Wait for one on it.")
        val identity = identity(context)
        try {
            var last: NativeFailure? = null
            for (address in found) {
                try {
                    val reply = Pass.net("lan.sync", online = false, secret = identity) {
                        put("address", address)
                        peerArgs(context)
                    }
                    Pass.reload()
                    Pass.flush()
                    return reply
                } catch (failure: NativeFailure) {
                    last = failure
                }
            }
            throw last!!
        } finally {
            identity.fill(0)
        }
    }

    /** Waits on this network for a paired device to sync with. */
    suspend fun serve(context: Context): com.wasimaster.wmpass.core.EngineReply {
        val identity = identity(context)
        try {
            val port = Engine.net("lan.listen", online = false).obj["port"]!!.jsonPrimitive.content.toInt()
            Engine.net("lan.advertise", online = false) { put("port", port) }
            val reply = Pass.net("lan.serve", online = false, secret = identity) { peerArgs(context) }
            Pass.reload()
            Pass.flush()
            return reply
        } finally {
            identity.fill(0)
            runCatching { Engine.net("lan.stop", online = false) }
        }
    }
}

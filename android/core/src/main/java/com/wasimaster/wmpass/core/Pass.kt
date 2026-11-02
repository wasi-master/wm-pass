package com.wasimaster.wmpass.core

import android.annotation.SuppressLint
import android.app.ActivityManager
import android.content.Context
import android.content.SharedPreferences
import java.io.File
import java.security.SecureRandom
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.int
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.add
import kotlinx.serialization.json.contentOrNull

/** An arbitrary fixed time for every file in the vault folder. */
private const val FILE_TIME_MS = 1_600_000_000_000L

/** Where the door stands. */
sealed interface LockState {
    /** Nothing on this device yet: first run, or after a wipe. */
    data object NoVault : LockState

    data object Locked : LockState

    data class Unlocked(val name: String, val file: String, val readOnly: Boolean) : LockState
}

/** What a typed password led to. */
sealed interface UnlockResult {
    data object Opened : UnlockResult

    /** Not right. A wipe looks exactly like this from the outside, on purpose. */
    data class Wrong(val retryAtMs: Long) : UnlockResult

    /** Asked to wait: too many tries. */
    data class Waiting(val retryAtMs: Long) : UnlockResult

    /** Right so far, and a key file or hardware key is also needed. */
    data class NeedsFactor(val factors: List<NeededFactor>) : UnlockResult

    /** A duress password asking for a hard lock. */
    data object HardLocked : UnlockResult
}

data class NeededFactor(val kind: String, val credentialId: String? = null, val salt: String? = null, val challenge: String? = null, val slot: Int? = null)

/**
 * The vaults on this device, for every part of the app: screens, autofill, passkeys,
 * tiles. They all share one process and so one unlocked state.
 */
@SuppressLint("StaticFieldLeak")
object Pass {
    private lateinit var context: Context
    private lateinit var prefs: SharedPreferences
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private val writing = Mutex()
    private var saveJob: Job? = null

    private val mutableState = MutableStateFlow<LockState>(LockState.NoVault)
    val state: StateFlow<LockState> = mutableState.asStateFlow()

    private val mutableEntries = MutableStateFlow<List<Summary>>(emptyList())

    /** Everything visible: not trashed and not on the shelf, unless the shelf is open. */
    val entries: StateFlow<List<Summary>> = mutableEntries.asStateFlow()

    private val mutableFolders = MutableStateFlow<List<FolderView>>(emptyList())
    val folders: StateFlow<List<FolderView>> = mutableFolders.asStateFlow()

    /** Rises on every change, for screens that hold a detail view. */
    private val mutableRevision = MutableStateFlow(0L)
    val revision: StateFlow<Long> = mutableRevision.asStateFlow()

    private var handle = 0L
    private var file: File? = null

    private val vaultDir: File get() = File(context.filesDir, "vaults")
    private fun blobDir(name: String) = File(File(context.filesDir, "blobs"), name)
    val handleForTests: Long get() = handle

    fun init(appContext: Context) {
        if (::context.isInitialized) return
        context = appContext.applicationContext
        prefs = context.getSharedPreferences("door", Context.MODE_PRIVATE)
        var device = prefs.getInt("device", 0)
        if (device == 0) {
            device = SecureRandom().nextInt().let { if (it == 0) 1 else it }
            prefs.edit().putInt("device", device).apply()
        }
        Engine.start(device)
        vaultDir.mkdirs()
        mutableState.value = if (vaultFiles().isEmpty()) LockState.NoVault else LockState.Locked
    }

    private fun vaultFiles(): List<File> =
        vaultDir.listFiles { f -> f.isFile && !f.name.startsWith(".") }?.sortedBy { it.name }.orEmpty()

    /** Memory for the password hash, by what this phone can spare. */
    private fun kdfMemoryKib(): Int {
        val info = ActivityManager.MemoryInfo()
        context.getSystemService(ActivityManager::class.java)?.getMemoryInfo(info)
        val gib = info.totalMem / (1L shl 30)
        return when {
            gib >= 6 -> 256 * 1024
            gib >= 4 -> 128 * 1024
            else -> 64 * 1024
        }
    }

    // ---- the door ----

    /** Makes the first vault, with chaff files beside it so the folder gives nothing away. */
    suspend fun create(name: String, password: ByteArray): Unit = writing.withLock {
        val memory = kdfMemoryKib()
        val reply = Engine.call(
            "vault.create",
            args = {
                put("name", name)
                put("kdf", kotlinx.serialization.json.buildJsonObject { put("m_kib", memory) })
            },
            secret = password,
        )
        adopt(reply.obj["handle"]!!.jsonPrimitive.long, File(vaultDir, randomHex(16)), name, false)
        saveNow()
        // One decoy that never changes, so that a hidden vault's unchanging file is nothing
        // unusual. Its partner is made by saveNow and keeps pace with this vault.
        writeChaff(File(vaultDir, randomHex(16)), file!!.length())
        refresh()
    }

    /** Adds a vault with its own password. The door shows no sign of it. */
    suspend fun createHidden(name: String, password: ByteArray): Unit = writing.withLock {
        val current = handle
        val reply = Engine.call(
            "vault.create",
            args = {
                put("name", name)
                put("same_kdf_as", current)
            },
            secret = password,
        )
        val saved = Engine.call("vault.save", handle = reply.obj["handle"]!!.jsonPrimitive.long)
        val hidden = File(vaultDir, randomHex(16))
        hidden.writeAtomically(saved.blobs[0])
        Engine.call("vault.lock", handle = reply.obj["handle"]!!.jsonPrimitive.long)
        hidden.setLastModified(FILE_TIME_MS)
    }

    private suspend fun writeChaff(target: File, fileLength: Long) {
        val reply = Engine.call(
            "vault.chaff",
            handle,
            {
                put("file_len", fileLength)
                put("same_kdf_as", handle)
            },
        )
        target.writeAtomically(reply.blobs[0])
    }

    /**
     * Keeps the folder from saying which file is the vault. The decoy matches the vault's size
     * as it grows, and every file carries the same time.
     */
    private suspend fun coverTracks(saved: File) {
        val name = Engine.call("vault.chaff_name", handle).obj["name"]!!.jsonPrimitive.content
        val decoy = File(vaultDir, name)
        if (decoy.length() != saved.length()) writeChaff(decoy, saved.length())
        vaultFiles().forEach { it.setLastModified(FILE_TIME_MS) }
    }

    private suspend fun adopt(newHandle: Long, target: File, name: String, readOnly: Boolean) {
        handle = newHandle
        file = target
        blobDir(target.name).mkdirs()
        mutableState.value = LockState.Unlocked(name, target.name, readOnly)
    }

    /** When typing a password next becomes allowed, or 0. */
    fun retryAt(): Long = prefs.getLong("retry_at", 0).takeIf { it > System.currentTimeMillis() } ?: 0

    private fun noteFailure(): Long {
        val fails = prefs.getInt("fails", 0) + 1
        // Five free tries, then thirty seconds doubling up to fifteen minutes.
        val wait = if (fails <= 5) 0L else minOf(30_000L shl minOf(fails - 6, 5), 900_000L)
        val at = if (wait == 0L) 0L else System.currentTimeMillis() + wait
        prefs.edit().putInt("fails", fails).putLong("retry_at", at).apply()
        return at
    }

    private fun noteSuccess() {
        prefs.edit().remove("fails").remove("retry_at").apply()
    }

    suspend fun unlock(password: ByteArray, factors: List<Pair<String, ByteArray>> = emptyList()): UnlockResult {
        retryAt().takeIf { it != 0L }?.let { return UnlockResult.Waiting(it) }
        val files = vaultFiles()
        val blobs = factors.map { it.second } + files.map { it.readBytes() }
        val reply = Engine.call(
            "vault.open",
            args = { putJsonArray("factors") { factors.forEach { add(it.first) } } },
            secret = password,
            blobs = blobs,
        )
        val obj = reply.obj
        return when (obj["outcome"]?.jsonPrimitive?.content) {
            "opened" -> {
                noteSuccess()
                val index = obj["index"]!!.jsonPrimitive.int
                val target = files[index]
                adopt(
                    obj["handle"]!!.jsonPrimitive.long,
                    target,
                    obj["name"]!!.jsonPrimitive.content,
                    obj["read_only"]?.jsonPrimitive?.boolean == true,
                )
                afterOpen(target.name, obj["generation"]!!.jsonPrimitive.long)
                UnlockResult.Opened
            }
            "duress" -> when (obj["action"]?.jsonPrimitive?.content) {
                "wipe" -> {
                    wipeEverything()
                    // Indistinguishable from a mistyped password.
                    UnlockResult.Wrong(0)
                }
                else -> {
                    prefs.edit().putLong("retry_at", System.currentTimeMillis() + 3_600_000L).apply()
                    UnlockResult.HardLocked
                }
            }
            "needs_factor" -> UnlockResult.NeedsFactor(
                obj["factors"]!!.jsonArray.map {
                    val f = it as kotlinx.serialization.json.JsonObject
                    NeededFactor(
                        f["kind"]!!.jsonPrimitive.content,
                        f["credential_id"]?.jsonPrimitive?.contentOrNull,
                        f["salt"]?.jsonPrimitive?.contentOrNull,
                        f["challenge"]?.jsonPrimitive?.contentOrNull,
                        f["slot"]?.jsonPrimitive?.int,
                    )
                },
            )
            else -> UnlockResult.Wrong(noteFailure())
        }
    }

    /** Opens with a key the phone's secure hardware just gave back. */
    suspend fun unlockWithKey(key: ByteArray, fileName: String) {
        val target = File(vaultDir, fileName)
        val reply = Engine.call("vault.open_key", secret = key, blobs = listOf(target.readBytes()))
        noteSuccess()
        val obj = reply.obj
        adopt(
            obj["handle"]!!.jsonPrimitive.long,
            target,
            obj["name"]!!.jsonPrimitive.content,
            obj["read_only"]?.jsonPrimitive?.boolean == true,
        )
        afterOpen(target.name, obj["generation"]!!.jsonPrimitive.long)
    }

    private suspend fun afterOpen(name: String, generation: Long) {
        val seen = prefs.getLong("seen_$name", 0)
        rollbackWarning.value = generation < seen
        if (generation > seen) prefs.edit().putLong("seen_$name", generation).apply()
        val swept = Engine.call("vault.sweep", handle).obj["delete_blobs"]?.jsonArray.orEmpty()
        swept.forEach { deleteBlob(it.jsonPrimitive.content) }
        if (swept.isNotEmpty()) scheduleSave()
        refresh()
    }

    /** True when the file on disk is older than one this phone has already seen. */
    val rollbackWarning = MutableStateFlow(false)

    /** Saves anything pending and forgets every key. */
    suspend fun lock() {
        flush()
        Engine.lockAll()
        handle = 0
        file = null
        mutableEntries.value = emptyList()
        mutableFolders.value = emptyList()
        mutableState.value = if (vaultFiles().isEmpty()) LockState.NoVault else LockState.Locked
    }

    private suspend fun wipeEverything() {
        Engine.lockAll()
        vaultDir.deleteRecursively()
        File(context.filesDir, "blobs").deleteRecursively()
        File(context.filesDir, "quick").deleteRecursively()
        vaultDir.mkdirs()
        prefs.edit().clear().apply()
        handle = 0
        file = null
        mutableEntries.value = emptyList()
        mutableState.value = LockState.NoVault
    }

    // ---- saving ----

    private fun scheduleSave() {
        saveJob?.cancel()
        saveJob = scope.launch {
            delay(350)
            writing.withLock { saveNow() }
        }
    }

    private suspend fun saveNow() {
        val target = file ?: return
        val active = handle.takeIf { it != 0L } ?: return
        val reply = Engine.call("vault.save", active)
        target.writeAtomically(reply.blobs[0])
        prefs.edit().putLong("seen_${target.name}", reply.obj["generation"]!!.jsonPrimitive.long).apply()
        coverTracks(target)
    }

    /** Writes pending changes now, before the process may go away. */
    suspend fun flush() {
        saveJob?.cancel()
        saveJob = null
        writing.withLock { saveNow() }
    }

    private suspend fun refresh() {
        val active = handle.takeIf { it != 0L } ?: return
        mutableEntries.value = Engine.view(
            "entry.list",
            ListSerializer(Summary.serializer()),
            active,
            { put("sort", "title") },
        )
        mutableFolders.value = Engine.view("folder.list", ListSerializer(FolderView.serializer()), active)
        mutableRevision.value++
    }

    // ---- reading ----

    suspend fun search(query: String, sort: String = "title"): List<Summary> = Engine.view(
        "entry.list",
        ListSerializer(Summary.serializer()),
        handle,
        {
            put("query", query)
            put("sort", sort)
        },
    )

    suspend fun detail(id: String): Detail = Engine.view("entry.get", Detail.serializer(), handle, { put("id", id) })

    /** A secret field's value. The caller wipes it. */
    suspend fun reveal(id: String, field: String): SecretBytes = Engine.call(
        "entry.reveal",
        handle,
        {
            put("id", id)
            put("field", field)
        },
    ).secret!!

    suspend fun revealCustom(id: String, index: Int): SecretBytes = Engine.call(
        "entry.reveal_custom",
        handle,
        {
            put("id", id)
            put("index", index)
        },
    ).secret!!

    suspend fun revealHistory(id: String, index: Int): SecretBytes = Engine.call(
        "entry.reveal_history",
        handle,
        {
            put("id", id)
            put("index", index)
        },
    ).secret!!

    suspend fun codes(ids: List<String>? = null): List<OtpRow> = Engine.view(
        "otp.codes",
        ListSerializer(OtpRow.serializer()),
        handle,
        { if (ids != null) putJsonArray("ids") { ids.forEach { add(it) } } },
    )

    suspend fun code(id: String): SecretBytes = Engine.call("otp.code", handle, { put("id", id) }).secret!!

    suspend fun health(breached: Map<String, Long> = emptyMap()): HealthView = Engine.view(
        "health.report",
        HealthView.serializer(),
        handle,
        {
            put(
                "breached",
                kotlinx.serialization.json.buildJsonObject { breached.forEach { (k, v) -> put(k, v) } },
            )
        },
    )

    suspend fun presets(): List<PresetView> = Engine.view("gen.presets", ListSerializer(PresetView.serializer()))

    /** Trashed entries, newest first. */
    suspend fun trashed(): List<Summary> = search("trashed", "modified")

    suspend fun kinds(): List<KindSpec> = Engine.view("kinds", ListSerializer(KindSpec.serializer()))

    suspend fun info(): VaultInfo = Engine.view("vault.info", VaultInfo.serializer(), handle)

    suspend fun slots(): List<SlotView> = Engine.view("slot.list", ListSerializer(SlotView.serializer()), handle)

    suspend fun matchWeb(url: String): List<MatchRow> =
        Engine.view("match.web", ListSerializer(MatchRow.serializer()), handle, { put("url", url) })

    suspend fun matchApp(pkg: String, certSha256: String): List<MatchRow> = Engine.view(
        "match.app",
        ListSerializer(MatchRow.serializer()),
        handle,
        {
            put("package", pkg)
            put("cert_sha256", certSha256)
        },
    )

    suspend fun strength(password: ByteArray, context: List<String> = emptyList()): StrengthView = Engine.view(
        "strength.assess",
        StrengthView.serializer(),
        args = { putJsonArray("context") { context.forEach { add(it) } } },
        secret = password,
    )

    suspend fun compare(first: ByteArray, second: ByteArray): CompareView =
        Engine.view("strength.compare", CompareView.serializer(), blobs = listOf(first, second))

    // ---- writing ----

    /** Any call that changes the vault. Refreshes the lists and schedules a save. */
    suspend fun change(
        method: String,
        secret: ByteArray? = null,
        blobs: List<ByteArray> = emptyList(),
        args: JsonObjectBuilder.() -> Unit = {},
    ): EngineReply {
        val reply = Engine.call(method, handle, args, secret, blobs)
        reply.obj["delete_blobs"]?.jsonArray?.forEach { deleteBlob(it.jsonPrimitive.content) }
        refresh()
        scheduleSave()
        return reply
    }

    private fun deleteBlob(name: String) {
        val folder = file?.name ?: return
        File(blobDir(folder), name).delete()
    }

    suspend fun addAttachment(id: String, name: String, mime: String, plain: ByteArray) {
        val reply = change("attachment.add", blobs = listOf(plain)) {
            put("id", id)
            put("name", name)
            put("mime", mime)
        }
        val blob = reply.obj["blob"]!!.jsonPrimitive.content
        File(blobDir(file!!.name), blob).writeAtomically(reply.blobs[0])
    }

    suspend fun openAttachment(id: String, blob: String): SecretBytes {
        val sealed = File(blobDir(file!!.name), blob).readBytes()
        return Engine.call(
            "attachment.open",
            handle,
            {
                put("id", id)
                put("blob", blob)
            },
            blobs = listOf(sealed),
        ).secret!!
    }

    /** The saved file, for a backup or a sync. It is ciphertext. */
    suspend fun sealedFile(): ByteArray {
        flush()
        return file!!.readBytes()
    }

    suspend fun verify(password: ByteArray): Boolean =
        Engine.call("vault.verify", handle, secret = password).obj["ok"]?.jsonPrimitive?.boolean == true

    /** The key, right after an unlock, for wrapping in the phone's secure hardware. */
    suspend fun keyForWrapping(): SecretBytes = Engine.call("vault.key_for_wrapping", handle).secret!!

    val currentFileName: String? get() = file?.name

    /** A new key for the vault. Its decoy's name follows the key, so the old decoy goes. */
    suspend fun rekey(password: ByteArray) {
        val before = Engine.call("vault.chaff_name", handle).obj["name"]!!.jsonPrimitive.content
        change("vault.rekey", secret = password)
        flush()
        File(vaultDir, before).delete()
    }

    /** Reads an arbitrary view-returning call on the open vault. */
    suspend fun plainCall(
        method: String,
        secret: ByteArray? = null,
        blobs: List<ByteArray> = emptyList(),
        args: JsonObjectBuilder.() -> Unit = {},
    ): EngineReply = Engine.call(method, handle, args, secret, blobs)
}

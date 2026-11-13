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
import kotlinx.coroutines.NonCancellable
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.builtins.MapSerializer
import kotlinx.serialization.builtins.serializer
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.int
import kotlinx.serialization.json.intOrNull
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

/** What the current password led to, before a change of password. */
sealed interface SlotCheck {
    data class Opens(val slot: Int) : SlotCheck

    /** Right so far, and this slot also needs its key file or security key. */
    data class NeedsFactor(val factors: List<NeededFactor>) : SlotCheck

    data object Wrong : SlotCheck
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

    /** The glyph picked for each tag, by name. Tags with none are left out. */
    private val mutableTagIcons = MutableStateFlow<Map<String, String>>(emptyMap())
    val tagIcons: StateFlow<Map<String, String>> = mutableTagIcons.asStateFlow()

    /** Rises on every change, for screens that hold a detail view. */
    private val mutableRevision = MutableStateFlow(0L)
    val revision: StateFlow<Long> = mutableRevision.asStateFlow()

    private var handle = 0L
    private var file: File? = null

    /** For an account held elsewhere: where its bytes go when it saves. */
    private var accountWriter: (suspend (ByteArray) -> Unit)? = null

    /** The account behind what is open, if it is not a vault in this folder. */
    private val mutableAccount = MutableStateFlow<String?>(null)
    val account: StateFlow<String?> = mutableAccount.asStateFlow()

    /** Called after an account has saved, to take its changes to a server if it has one. */
    var afterAccountSave: (suspend () -> Unit)? = null

    /** Called after the vault in this folder has saved, for a sync that follows it. */
    var afterSave: (() -> Unit)? = null

    /** Called after the lists reload, which follows every change and every merge. */
    var afterRefresh: (() -> Unit)? = null

    /** Run on a wipe, so that what the app keeps beside the vault goes with it. */
    val onWipe = mutableListOf<() -> Unit>()

    /** The vault this one was opened from, while inside a nested or shared one. */
    private class Outer(val handle: Long, val file: File?, val state: LockState)

    private var outer: Outer? = null
    private var childWriter: (suspend (ByteArray) -> Unit)? = null
    private var childBefore: (suspend () -> Unit)? = null

    /** The name of the nested or shared vault in use, or null. */
    private val mutableChild = MutableStateFlow<String?>(null)
    val child: StateFlow<String?> = mutableChild.asStateFlow()

    private val vaultDir: File get() = File(context.filesDir, "vaults")
    private val linkedDir: File get() = File(context.filesDir, "linked")

    /**
     * Vault files restored from a backup while vaults were already here. They wait beside the
     * folder, never over a file in it, until a password shows what they are: a copy that merges
     * into the vault it opens, or the only one a password still opens.
     */
    private val heldDir: File get() = File(context.filesDir, "restored")
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
        Engine.onInternal = ::internalError
        Engine.start(device)
        vaultDir.mkdirs()
        mutableState.value = if (vaultFiles().isEmpty()) LockState.NoVault else LockState.Locked
    }

    private fun vaultFiles(): List<File> =
        vaultDir.listFiles { f -> f.isFile && !f.name.startsWith(".") }?.sortedBy { it.name }.orEmpty()

    private fun heldFiles(): List<File> =
        heldDir.listFiles { f -> f.isFile && !f.name.startsWith(".") }?.sortedBy { it.name }.orEmpty()

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

    /**
     * An unlock is a fresh start after an internal error: one asked for from autofill, say, while
     * the app's own screens still show the message about it.
     */
    private fun resumeForDoor() {
        if (mutableState.value !is LockState.Unlocked) Engine.resume()
    }

    /** Makes the first vault, with chaff files beside it so the folder gives nothing away. */
    suspend fun create(name: String, password: ByteArray): Unit = writing.withLock {
        resumeForDoor()
        // Finished even if the caller goes away: a vault that opened on screen but never
        // reached the disk would go with the process.
        withContext(NonCancellable) {
            val memory = kdfMemoryKib()
            val reply = Engine.call(
                "vault.create",
                args = {
                    put("name", name)
                    put("kdf", kotlinx.serialization.json.buildJsonObject { put("m_kib", memory) })
                },
                secret = password,
            )
            val target = File(vaultDir, randomHex(16))
            adopt(reply.obj["handle"]!!.jsonPrimitive.long, target)
            saveNow()
            // One decoy that never changes, so that a hidden vault's unchanging file is nothing
            // unusual. Its partner is made by saveNow and keeps pace with this vault.
            writeChaff(File(vaultDir, randomHex(16)), target.length())
            refresh()
            showOpen(name, target, false)
        }
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
        // A fresh time would mark the decoy written last, until the next save covered it.
        target.setLastModified(FILE_TIME_MS)
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

    private fun adopt(newHandle: Long, target: File) {
        handle = newHandle
        file = target
        blobDir(target.name).mkdirs()
    }

    /**
     * Always the last step of opening. The door leaves the screen on this change and cancels
     * the coroutine that opened the vault, so nothing after it can be relied on to run.
     */
    private fun showOpen(name: String, target: File, readOnly: Boolean) {
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
        resumeForDoor()
        retryAt().takeIf { it != 0L }?.let { return UnlockResult.Waiting(it) }
        val local = vaultFiles()
        val held = heldFiles()
        suspend fun open(files: List<File>) = Engine.call(
            "vault.open",
            args = { putJsonArray("factors") { factors.forEach { add(it.first) } } },
            secret = password,
            blobs = factors.map { it.second } + files.map { it.readBytes() },
        )
        // The files here come first, so a restored copy opens only when the password opens none
        // of them: a password changed since the backup, or a local file that is damaged.
        var files = local + held
        val reply = try {
            open(files)
        } catch (failure: Exception) {
            if (held.isEmpty() || failure is kotlinx.coroutines.CancellationException) throw failure
            files = held
            open(held)
        }
        val obj = reply.obj
        return when (obj["outcome"]?.jsonPrimitive?.content) {
            "opened" -> {
                noteSuccess()
                val index = obj["index"]!!.jsonPrimitive.int
                val target = files[index].let { if (it in held) takeInHeld(it) else it }
                val readOnly = obj["read_only"]?.jsonPrimitive?.boolean == true
                adopt(obj["handle"]!!.jsonPrimitive.long, target)
                afterOpen(target.name, obj["generation"]!!.jsonPrimitive.long, readOnly)
                showOpen(obj["name"]!!.jsonPrimitive.content, target, readOnly)
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
            "needs_factor" -> UnlockResult.NeedsFactor(neededFactors(obj["factors"]!!.jsonArray))
            else -> UnlockResult.Wrong(noteFailure())
        }
    }

    /** Opens with a key the phone's secure hardware just gave back. */
    suspend fun unlockWithKey(key: ByteArray, fileName: String) {
        resumeForDoor()
        // A panic or a lockout password refuses every way in for its hour, and a finger is easier
        // to force than a password. A wait after wrong passwords holds the fingerprint back too.
        check(retryAt() == 0L) { "Wait until the vault can be opened again." }
        val target = File(vaultDir, fileName)
        val reply = Engine.call("vault.open_key", secret = key, blobs = listOf(target.readBytes()))
        noteSuccess()
        val obj = reply.obj
        val readOnly = obj["read_only"]?.jsonPrimitive?.boolean == true
        adopt(obj["handle"]!!.jsonPrimitive.long, target)
        afterOpen(target.name, obj["generation"]!!.jsonPrimitive.long, readOnly)
        showOpen(obj["name"]!!.jsonPrimitive.content, target, readOnly)
    }

    private suspend fun afterOpen(name: String, generation: Long, readOnly: Boolean) {
        val seen = prefs.getLong("seen_$name", 0)
        rollbackWarning.value = generation < seen
        if (generation > seen) prefs.edit().putLong("seen_$name", generation).apply()
        val swept = Engine.call("vault.sweep", handle).obj["delete_blobs"]?.jsonArray.orEmpty()
        swept.forEach { deleteBlob(it.jsonPrimitive.content) }
        if (swept.isNotEmpty()) scheduleSave()
        // A restored copy that cannot be dealt with now waits for the next unlock; it never keeps
        // this vault from opening.
        if (!readOnly) runCatching { absorbHeld() }
        // This phone's name, so a warning on another device about its clock can name it.
        if (!readOnly) {
            val name = listOf(android.os.Build.MANUFACTURER, android.os.Build.MODEL)
                .filter { !it.isNullOrBlank() && it != "unknown" }
                .distinct()
                .joinToString(" ") { it.replaceFirstChar(Char::uppercaseChar) }
                .ifEmpty { "Android phone" }
            val named = runCatching { Engine.call("vault.device_name", handle, { put("name", name) }) }.getOrNull()
            if (named?.obj?.get("changed")?.jsonPrimitive?.boolean == true) scheduleSave()
        }
        refresh()
    }

    /**
     * A restored copy the password opened when it opened nothing here: moved into the folder
     * under a new name, so a local file that shares its name is left as it was.
     */
    private fun takeInHeld(copy: File): File {
        val target = File(vaultDir, randomHex(16))
        check(copy.renameTo(target)) { "The restored vault could not be moved into place." }
        target.setLastModified(FILE_TIME_MS)
        // Its attachments go with it. A folder a local vault also uses is copied, not taken.
        val blobs = blobDir(copy.name)
        if (blobs.isDirectory) {
            if (File(vaultDir, copy.name).exists() || !blobs.renameTo(blobDir(target.name))) carryBlobs(blobs, blobDir(target.name))
        }
        return target
    }

    /** Copies the attachments in [from] that [to] lacks. */
    private fun carryBlobs(from: File, to: File) {
        from.listFiles()?.filter { it.isFile && !it.name.startsWith(".") }?.forEach { blob ->
            val target = File(to, blob.name)
            if (!target.exists()) target.writeAtomically(blob.readBytes())
        }
    }

    /**
     * Deals with each restored copy once a vault here is open. A copy of this vault merges in
     * and goes, as does one that is this vault's decoy or the same as a file here. Any other
     * copy stays held: moved into the folder, an older copy could open before the newer file it
     * shares a password with. It leaves only by merging into its own vault when that one opens,
     * or by being the one a password opens when it opens nothing here.
     */
    private suspend fun absorbHeld() {
        val held = heldFiles()
        if (held.isEmpty()) return
        val open = file ?: return
        val local = vaultFiles().map { it.name }.toSet()
        // This vault's own decoy: the backup's has the same name, and the one here replaces it.
        val decoy = runCatching { Engine.call("vault.chaff_name", handle).obj["name"]!!.jsonPrimitive.content }.getOrNull()
        val done = mutableListOf<File>()
        var merged = false
        held.forEach { copy ->
            val bytes = copy.readBytes()
            val twin = File(vaultDir, copy.name)
            if (twin.isFile && twin.length() == bytes.size.toLong() && twin.readBytes().contentEquals(bytes)) {
                done += copy
            } else if (runCatching { Engine.call("vault.merge", handle, blobs = listOf(bytes)) }.isSuccess) {
                merged = true
                if (copy.name != open.name) {
                    val blobs = blobDir(copy.name)
                    if (blobs.isDirectory) {
                        carryBlobs(blobs, blobDir(open.name))
                        if (copy.name !in local) blobs.deleteRecursively()
                    }
                }
                done += copy
            } else if (copy.name == decoy) {
                done += copy
            }
        }
        // Saved before the copies go, so that what they brought is on disk first.
        if (merged) flush()
        done.forEach { it.delete() }
    }

    /** True when the file on disk is older than one this phone has already seen. */
    val rollbackWarning = MutableStateFlow(false)

    private val mutableNotices = MutableStateFlow(VaultNotices())

    /** What the open vault has to tell its owner, read again after every change and merge. */
    val notices: StateFlow<VaultNotices> = mutableNotices.asStateFlow()

    private suspend fun readNotices(active: Long) {
        val obj = runCatching { Engine.call("vault.notices", active).obj }.getOrNull() ?: return
        if (handle != active) return
        val conflict = obj["password_conflict"] as? JsonObject
        val clock = obj["clock_warning"] as? JsonObject
        mutableNotices.value = VaultNotices(
            passwordConflict = conflict?.get("message")?.jsonPrimitive?.contentOrNull,
            bothOpen = conflict?.get("both_open")?.jsonPrimitive?.boolean == true,
            clockWarning = clock?.get("message")?.jsonPrimitive?.contentOrNull,
        )
    }

    /** Marks a lost change of password, which the owner has now read about, as told on every device. */
    suspend fun passwordLossSeen() {
        change("slot.conflict_seen")
    }

    /** Saves anything pending and forgets every key. */
    // Not cancellable: a lock cut off part way (the app coming back, its caller's scope ending)
    // would leave the vault open, or the engine locked behind a screen that still shows it.
    suspend fun lock(): Unit = withContext(NonCancellable) {
        // A save that fails here (a shared vault's server out of reach, a KeePass file whose
        // provider is gone) must not leave the vault open: it locks all the same.
        // A nested vault that could not be left failed its own save; the one behind it was saved on
        // the way in, so a second try would only wait on the same failure.
        if (runCatching { leaveChild() }.isSuccess) runCatching { flush() }
        Engine.lockAll()
        forgetOpen()
        mutableEntries.value = emptyList()
        mutableNotices.value = VaultNotices()
        mutableFolders.value = emptyList()
        mutableTagIcons.value = emptyMap()
        mutableState.value = if (vaultFiles().isEmpty()) LockState.NoVault else LockState.Locked
    }

    // ---- an internal error ----

    private val mutableHeld = MutableStateFlow<LockState?>(null)

    /**
     * After an internal error, what the app's screens were showing, which they keep showing under
     * a message until the person has read it and goes to the door ([leaveInternal]). Null
     * otherwise. [state] is already locked by then, for everything else: widgets, tiles, autofill.
     */
    val internalHeld: StateFlow<LockState?> = mutableHeld.asStateFlow()

    /**
     * A bug in the core stopped a call, whoever made it (see [Engine.onInternal]). The core has
     * already thrown its session away, wiping every key, so every vault is locked: this forgets
     * the handles too. Nothing is saved, as what was open may be half changed; the files on disk
     * are whole, since a save replaces a file at once.
     */
    private fun internalError() {
        val was = mutableState.value
        if (was is LockState.Unlocked) mutableHeld.value = was
        saveJob?.cancel()
        saveJob = null
        forgetOpen()
        // The lists stay for the screens kept under the message; leaving it empties them.
        mutableState.value = if (vaultFiles().isEmpty()) LockState.NoVault else LockState.Locked
    }

    /** From the message about an internal error to the door. */
    fun leaveInternal() {
        mutableHeld.value = null
        mutableEntries.value = emptyList()
        mutableNotices.value = VaultNotices()
        mutableFolders.value = emptyList()
        mutableTagIcons.value = emptyMap()
        Engine.resume()
    }

    /** What every vault had open, for a lock. */
    private fun forgetOpen() {
        handle = 0
        file = null
        outer = null
        childWriter = null
        childBefore = null
        mutableChild.value = null
        accountWriter = null
        afterAccountSave = null
        mutableAccount.value = null
        mutableSaveProblem.value = null
    }

    /** A panic: locked at once, and every password refused for an hour. */
    suspend fun panic() {
        lock()
        // A panic takes a copied password off the clipboard too. An ordinary lock leaves it, since
        // locking on leaving the app would otherwise wipe it before it could be pasted.
        SecureClipboard.wipe(context, everything = true)
        prefs.edit().putLong("retry_at", System.currentTimeMillis() + 3_600_000L).apply()
    }

    suspend fun wipe() = wipeEverything()

    private suspend fun wipeEverything() {
        SecureClipboard.wipe(context, everything = true)
        Engine.lockAll()
        vaultDir.deleteRecursively()
        File(context.filesDir, "blobs").deleteRecursively()
        File(context.filesDir, "quick").deleteRecursively()
        linkedDir.deleteRecursively()
        heldDir.deleteRecursively()
        vaultDir.mkdirs()
        prefs.edit().clear().apply()
        onWipe.forEach { runCatching { it() } }
        handle = 0
        file = null
        outer = null
        childWriter = null
        childBefore = null
        accountWriter = null
        mutableChild.value = null
        mutableAccount.value = null
        mutableEntries.value = emptyList()
        mutableNotices.value = VaultNotices()
        // The folder names went with the vault too.
        mutableFolders.value = emptyList()
        mutableTagIcons.value = emptyMap()
        mutableSaveProblem.value = null
        mutableState.value = LockState.NoVault
    }

    // ---- saving ----

    /**
     * Why the last save in the background failed, until a save works. What changed stays in
     * memory, and the next save, or the one on leaving the app, takes it along.
     */
    private val mutableSaveProblem = MutableStateFlow<String?>(null)
    val saveProblem: StateFlow<String?> = mutableSaveProblem.asStateFlow()

    private fun scheduleSave() {
        saveJob?.cancel()
        saveJob = scope.launch {
            delay(350)
            // A place out of reach, such as a shared vault's server while offline or a KeePass
            // file whose app has gone, must not take the whole process down with it.
            try {
                writing.withLock { saveNow() }
                mutableSaveProblem.value = null
            } catch (cancelled: kotlinx.coroutines.CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                mutableSaveProblem.value = failure.message ?: "The vault could not be saved."
            }
        }
    }

    private suspend fun saveNow() {
        childWriter?.let { writer ->
            val active = handle.takeIf { it != 0L } ?: return
            childBefore?.let { runCatching { it() } }
            writer(Engine.call("vault.save", active).blobs[0])
            refresh()
            return
        }
        accountWriter?.let { writer ->
            val active = handle.takeIf { it != 0L } ?: return
            val reply = Engine.call("remote.save", active)
            writer(reply.blobs[0])
            afterAccountSave?.invoke()
            return
        }
        val target = file ?: return
        val active = handle.takeIf { it != 0L } ?: return
        val reply = Engine.call("vault.save", active)
        target.writeAtomically(reply.blobs[0])
        prefs.edit().putLong("seen_${target.name}", reply.obj["generation"]!!.jsonPrimitive.long).apply()
        coverTracks(target)
        afterSave?.invoke()
    }

    /** Writes pending changes now, before the process may go away. */
    suspend fun flush() {
        saveJob?.cancel()
        saveJob = null
        writing.withLock { saveNow() }
        mutableSaveProblem.value = null
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
        mutableTagIcons.value = Engine.view("tags.icons", MapSerializer(String.serializer(), String.serializer()), active)
        mutableRevision.value++
        afterRefresh?.invoke()
        readNotices(active)
    }

    // ---- settings that follow the vault ----

    /**
     * Whether the vault in this folder is open, the only one that carries the app's settings: not
     * an account held elsewhere, and not a vault opened from inside another.
     */
    val carriesSettings: Boolean get() = handle != 0L && file != null && accountWriter == null && childWriter == null

    /** The settings other devices left in the vault, by key. */
    suspend fun syncedSettings(): JsonObject = Engine.view("prefs.synced", JsonObject.serializer(), handle)

    /** Leaves settings in the vault for the other devices, and saves soon if any changed. */
    suspend fun syncSettings(values: JsonObject) {
        val reply = Engine.call("prefs.sync", handle, { put("values", values) })
        if (reply.obj["changed"]!!.jsonPrimitive.int > 0) scheduleSave()
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

    suspend fun health(
        breached: Map<String, Long> = emptyMap(),
        ignored: Map<String, Set<String>> = emptyMap(),
        skipLocal: Boolean = false,
    ): HealthView = Engine.view(
        "health.report",
        HealthView.serializer(),
        handle,
        {
            put(
                "breached",
                kotlinx.serialization.json.buildJsonObject { breached.forEach { (k, v) -> put(k, v) } },
            )
            put(
                "ignored",
                kotlinx.serialization.json.buildJsonObject {
                    ignored.forEach { (check, ids) -> put(check, kotlinx.serialization.json.JsonArray(ids.map { kotlinx.serialization.json.JsonPrimitive(it) })) }
                },
            )
            put("skip_local", skipLocal)
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

    /** Adds the import just read, and writes the attachments it brought along. */
    suspend fun applyImport(): EngineReply {
        val reply = change("import.apply")
        val names = reply.obj["blob_names"]?.jsonArray?.map { it.jsonPrimitive.content }.orEmpty()
        val folder = file?.let { blobDir(it.name).apply { mkdirs() } }
        if (folder != null) names.zip(reply.blobs).forEach { (name, sealed) -> File(folder, name).writeAtomically(sealed) }
        return reply
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

    /**
     * Which slot this password opens, with the key file or security key outputs in [factors] that
     * slot also needs. A change then replaces that slot and keeps its factor; the engine allows it
     * only right after this check succeeds.
     */
    suspend fun passwordSlot(password: ByteArray, factors: List<Pair<String, ByteArray>> = emptyList()): SlotCheck {
        val reply = Engine.call(
            "vault.verify",
            handle,
            { putJsonArray("factors") { factors.forEach { add(it.first) } } },
            secret = password,
            blobs = factors.map { it.second },
        ).obj
        val slot = reply["slot"]?.jsonPrimitive?.intOrNull
        if (reply["ok"]?.jsonPrimitive?.boolean == true && slot != null) return SlotCheck.Opens(slot)
        val needed = reply["needs_factor"]?.jsonArray?.let(::neededFactors).orEmpty()
        return if (needed.isNotEmpty()) SlotCheck.NeedsFactor(needed) else SlotCheck.Wrong
    }

    private fun neededFactors(array: kotlinx.serialization.json.JsonArray): List<NeededFactor> = array.map {
        val f = it as JsonObject
        NeededFactor(
            f["kind"]!!.jsonPrimitive.content,
            f["credential_id"]?.jsonPrimitive?.contentOrNull,
            f["salt"]?.jsonPrimitive?.contentOrNull,
            f["challenge"]?.jsonPrimitive?.contentOrNull,
            f["slot"]?.jsonPrimitive?.intOrNull,
        )
    }

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

    /** Takes a vault file from elsewhere as this phone's vault. */
    suspend fun adoptFile(bytes: ByteArray, name: String = randomHex(16)) {
        File(vaultDir, name).writeAtomically(bytes)
        // A chaff file of its own, or a lone file would say which one is the vault.
        val chaff = Engine.call(
            "vault.chaff",
            args = {
                put("file_len", bytes.size)
                put("kdf", kotlinx.serialization.json.buildJsonObject { put("m_kib", kdfMemoryKib()) })
            },
        )
        File(vaultDir, randomHex(16)).writeAtomically(chaff.blobs[0])
        vaultFiles().forEach { it.setLastModified(FILE_TIME_MS) }
        if (handle == 0L) mutableState.value = LockState.Locked
    }

    /**
     * Puts a vault file from elsewhere in place of the open one, as a merge on the local network
     * ends when the other device's vault is kept. The open vault locks first; then its file, its
     * decoy and its attachments go, and so does fingerprint unlock set up for it. [name] is the
     * name whose attachment folder already holds the new vault's attachments. Any other vault
     * in the folder, hidden ones included, stays as it is.
     */
    suspend fun replaceWith(bytes: ByteArray, name: String) {
        val old = file ?: throw IllegalStateException("no vault in this folder is open")
        // The decoy's name follows the key, so it can only be asked while the vault is open.
        val decoy = Engine.call("vault.chaff_name", handle).obj["name"]!!.jsonPrimitive.content
        lock()
        adoptFile(bytes, name)
        File(vaultDir, old.name).delete()
        File(vaultDir, decoy).delete()
        blobDir(old.name).deleteRecursively()
        prefs.edit().remove("seen_${old.name}").apply()
        QuickUnlock(context).let { if (it.enrolledFile() == old.name) it.disable() }
        vaultFiles().forEach { it.setLastModified(FILE_TIME_MS) }
    }

    /**
     * Shows an account through the same screens as a vault. [writer] keeps what it saves: the
     * KeePass file itself, or the account's sealed offline copy.
     */
    suspend fun adoptAccount(accountId: String, reply: EngineReply, writer: suspend (ByteArray) -> Unit) {
        val obj = reply.obj
        handle = obj["handle"]!!.jsonPrimitive.long
        file = null
        accountWriter = writer
        mutableAccount.value = accountId
        noteSuccess()
        refresh()
        // Last, as in showOpen.
        mutableState.value = LockState.Unlocked(obj["name"]!!.jsonPrimitive.content, accountId, false)
    }

    // ---- nested and shared vaults ----

    /** A nested vault's file in this phone's storage. */
    fun linkedFile(id: String) = File(linkedDir, id)

    suspend fun writeLinked(id: String, bytes: ByteArray) = linkedFile(id).writeAtomically(bytes)

    /**
     * Shows a nested or shared vault through the same screens. The vault it was opened from
     * stays open behind it. [writer] takes each save.
     */
    suspend fun enterChild(
        id: String,
        reply: EngineReply,
        before: (suspend () -> Unit)? = null,
        writer: suspend (ByteArray) -> Unit,
    ) {
        flush()
        val obj = reply.obj
        outer = outer ?: Outer(handle, file, mutableState.value)
        handle = obj["handle"]!!.jsonPrimitive.long
        file = linkedFile(id)
        childWriter = writer
        childBefore = before
        blobDir(id).mkdirs()
        val name = obj["name"]!!.jsonPrimitive.content
        mutableChild.value = name
        refresh()
        // Last, as in showOpen.
        mutableState.value = LockState.Unlocked(name, id, false)
    }

    /** Back to the vault this one was opened from. */
    suspend fun leaveChild() {
        val back = outer ?: return
        flush()
        Engine.call("vault.lock", handle)
        outer = null
        childWriter = null
        childBefore = null
        handle = back.handle
        file = back.file
        mutableChild.value = null
        mutableState.value = back.state
        refresh()
    }

    // ---- files coming and going ----

    private val bundled get() = listOf("vaults" to vaultDir, "linked" to linkedDir)

    /**
     * Every vault file and sealed attachment as one file, for a backup. All of it is
     * ciphertext and none of it needs a key, so this runs while the vault is locked. Which
     * file is the real vault stays as unclear in the backup as it is in the folder.
     */
    suspend fun bundle(): ByteArray {
        if (handle != 0L) flush()
        val out = java.io.ByteArrayOutputStream()
        java.util.zip.ZipOutputStream(out).use { zip ->
            zip.setLevel(0)
            fun add(name: String, source: File) {
                val entry = java.util.zip.ZipEntry(name)
                entry.time = FILE_TIME_MS
                zip.putNextEntry(entry)
                source.inputStream().use { it.copyTo(zip) }
                zip.closeEntry()
            }
            bundled.forEach { (prefix, folder) ->
                folder.listFiles()?.filter { it.isFile && !it.name.startsWith(".") }?.sortedBy { it.name }?.forEach { add("$prefix/${it.name}", it) }
            }
            File(context.filesDir, "blobs").listFiles()?.sortedBy { it.name }?.forEach { group ->
                group.listFiles()?.filter { it.isFile }?.sortedBy { it.name }?.forEach { add("blobs/${group.name}/${it.name}", it) }
            }
        }
        return out.toByteArray()
    }

    /** Whether [bytes] holds at least one vault file. */
    fun bundleLooksRight(bytes: ByteArray): Boolean = unbundle(bytes).any { it.first.startsWith("vaults/") }

    /**
     * The files in a bundle, with hostile names refused: a backup comes from somewhere else,
     * and an entry named `../../x` must not reach beyond these folders.
     */
    private fun unbundle(bytes: ByteArray): List<Pair<String, ByteArray>> {
        val out = mutableListOf<Pair<String, ByteArray>>()
        runCatching {
            java.util.zip.ZipInputStream(bytes.inputStream()).use { zip ->
                while (true) {
                    val entry = zip.nextEntry ?: break
                    val parts = entry.name.split('/')
                    val fine = parts.none { it.isEmpty() || it == "." || it == ".." || it.startsWith(".") || '\\' in it } &&
                        when (parts.first()) {
                            "vaults", "linked" -> parts.size == 2
                            "blobs" -> parts.size == 3
                            else -> false
                        }
                    if (fine && !entry.isDirectory) out += entry.name to zip.readBytes()
                }
            }
        }
        return out
    }

    private fun isVaultFile(bytes: ByteArray) =
        bytes.size > 4 && bytes[0] == 'W'.code.toByte() && bytes[1] == 'M'.code.toByte() && bytes[2] == 'P'.code.toByte() && bytes[3] == 'V'.code.toByte()

    /**
     * Puts a vault file or a backup in place at the door. The password decides what opens.
     *
     * With vaults already here, nothing here is written over, since an older backup must not
     * take the place of newer files. Its vault files wait in [heldDir] for the next unlock,
     * which merges each into its own vault, or opens it when the password opens nothing here.
     * Only attachments and nested vaults this phone lacks are written. Returns true when the
     * backup waits so.
     */
    suspend fun restore(bytes: ByteArray): Boolean {
        val hold = vaultFiles().isNotEmpty()
        if (isVaultFile(bytes)) {
            if (!hold) {
                adoptFile(bytes)
                return false
            }
            File(heldDir, randomHex(16)).writeAtomically(bytes)
            return true
        }
        val files = unbundle(bytes)
        require(files.any { it.first.startsWith("vaults/") }) { "That is not a WM Pass backup." }
        // A copy still waiting from an earlier restore keeps its place, and this one takes a new
        // name, with its attachments under that name too.
        val renamed = mutableMapOf<String, String>()
        if (hold) {
            files.filter { it.first.startsWith("vaults/") }.forEach { (name, data) ->
                val waiting = File(heldDir, name.substringAfter('/'))
                if (waiting.exists() && !waiting.readBytes().contentEquals(data)) renamed[waiting.name] = randomHex(16)
            }
        }
        files.forEach { (name, data) ->
            val parts = name.split('/')
            val target = when (parts[0]) {
                "vaults" -> if (hold) File(heldDir, renamed[parts[1]] ?: parts[1]) else File(vaultDir, parts[1])
                "linked" -> File(linkedDir, parts[1])
                else -> File(blobDir(renamed[parts[1]] ?: parts[1]), parts[2])
            }
            if (!hold || parts[0] == "vaults" || !target.exists()) target.writeAtomically(data)
        }
        vaultFiles().forEach { it.setLastModified(FILE_TIME_MS) }
        if (handle == 0L) mutableState.value = LockState.Locked
        return hold
    }

    /**
     * Merges a vault file or a backup into the open vault, and reports how many of its files
     * were copies of this vault. Nothing here is lost: a value that loses moves to history.
     */
    suspend fun mergeIn(bytes: ByteArray): Int {
        val files = if (isVaultFile(bytes)) listOf("vaults/x" to bytes) else unbundle(bytes)
        var merged = 0
        files.filter { it.first.startsWith("vaults/") }.forEach { (_, data) ->
            val fine = runCatching { Engine.call("vault.merge", handle, blobs = listOf(data)) }.isSuccess
            if (fine) merged++
        }
        if (merged > 0) {
            val folder = blobFolder()
            files.filter { it.first.startsWith("blobs/") }.forEach { (name, data) ->
                val target = folder?.let { File(it, name.substringAfterLast('/')) }
                if (target != null && !target.exists()) target.writeAtomically(data)
            }
            refresh()
            flush()
        }
        return merged
    }

    /** Merges one saved copy of the open vault, as a sync brings it in. */
    suspend fun mergeCopy(bytes: ByteArray): Boolean {
        val seen = currentFileName?.let { prefs.getLong("seen_$it", 0) } ?: 0
        val reply = Engine.call("vault.merge", handle, { put("seen_generation", seen) }, blobs = listOf(bytes))
        if (reply.obj["rollback"]?.jsonPrimitive?.boolean == true) rollbackWarning.value = true
        refresh()
        return true
    }

    /** Merges a copy in during a save, when the lists are about to be refreshed anyway. */
    suspend fun absorb(bytes: ByteArray) {
        Engine.call("vault.merge", handle, blobs = listOf(bytes))
    }

    /** What is in the open vault's folder of sealed attachments. */
    fun blobNames(): List<String> = blobFolder()?.list()?.filterNot { it.startsWith(".") }.orEmpty()

    fun readBlob(name: String): ByteArray = File(blobFolder(), name).readBytes()

    fun writeBlob(name: String, bytes: ByteArray) {
        require('/' !in name && !name.startsWith(".")) { "bad name" }
        File(blobFolder(), name).writeAtomically(bytes)
    }

    /** Opens with emergency shares and a vault file, sets a new password and keeps the file. */
    suspend fun recover(file: ByteArray, shares: List<String>, password: ByteArray) = withContext(NonCancellable) {
        // Finished even if the caller goes away, as in create.
        val reply = Engine.call(
            "emergency.recover",
            args = { putJsonArray("shares") { shares.forEach { add(it) } } },
            blobs = listOf(file),
        )
        val obj = reply.obj
        val target = File(vaultDir, randomHex(16))
        adopt(obj["handle"]!!.jsonPrimitive.long, target)
        Engine.call("vault.change_password", handle, { put("index", 0) }, secret = password)
        saveNow()
        writeChaff(File(vaultDir, randomHex(16)), target.length())
        refresh()
        showOpen(obj["name"]!!.jsonPrimitive.content, target, false)
    }

    /** Reloads the lists, after something changed the vault from outside these calls. */
    suspend fun reload() = refresh()

    /** A failed account sign-in counts against the same throttle as a wrong password. */
    fun noteWrongAccountPassword(): Long = noteFailure()

    /** A call that waits on a network, with the open vault's handle. */
    suspend fun net(method: String, online: Boolean, secret: ByteArray? = null, args: JsonObjectBuilder.() -> Unit = {}): EngineReply =
        Engine.net(method, handle, online, secret, args)

    /** A name for a vault file about to arrive, so its attachments can arrive first. */
    fun freshVaultName(): String = randomHex(16)

    /** Where the attachments of the vault file [name] live, made if need be. */
    fun blobFolderFor(name: String): File = blobDir(name).apply { mkdirs() }

    /** Whether the open vault is the one in this folder: the only one that goes to a new device. */
    val isFolderVault: Boolean get() = carriesSettings

    /** Where attachments live, for a sync that carries them. */
    fun blobFolder(): File? = file?.let { blobDir(it.name) }

    /** Reads an arbitrary view-returning call on the open vault. */
    suspend fun plainCall(
        method: String,
        secret: ByteArray? = null,
        blobs: List<ByteArray> = emptyList(),
        args: JsonObjectBuilder.() -> Unit = {},
    ): EngineReply = Engine.call(method, handle, args, secret, blobs)
}

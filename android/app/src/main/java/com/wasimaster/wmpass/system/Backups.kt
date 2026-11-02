package com.wasimaster.wmpass.system

import android.content.Context
import android.net.Uri
import androidx.work.Constraints
import androidx.work.CoroutineWorker
import androidx.work.ExistingPeriodicWorkPolicy
import androidx.work.NetworkType
import androidx.work.PeriodicWorkRequestBuilder
import androidx.work.WorkManager
import androidx.work.WorkerParameters
import com.wasimaster.wmkeyboard.core.settings.S3Config
import com.wasimaster.wmkeyboard.core.settings.SftpConfig
import com.wasimaster.wmkeyboard.core.settings.sink.BackupSink
import com.wasimaster.wmkeyboard.core.settings.sink.S3Sink
import com.wasimaster.wmkeyboard.core.settings.sink.SafFolderSink
import com.wasimaster.wmkeyboard.core.settings.sink.SftpSink
import com.wasimaster.wmkeyboard.core.settings.sink.SinkEntry
import com.wasimaster.wmkeyboard.core.settings.sink.SinkError
import com.wasimaster.wmkeyboard.core.settings.sink.WebDavSink
import com.wasimaster.wmkeyboard.core.settings.sink.sinkErrorOf
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmsuite.sinks.BackupPipeline
import java.text.SimpleDateFormat
import java.util.Date
import java.util.Locale
import java.util.TimeZone
import java.util.concurrent.TimeUnit
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.Serializable
import kotlinx.serialization.builtins.ListSerializer
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * Somewhere outside this phone. Sealed on disk, since it holds that place's own password. It can
 * reach only ciphertext: what is written there is the vault file, encrypted as it always is.
 */
@Serializable
data class Destination(
    val id: String,
    val kind: String,
    val label: String,
    val uri: String = "",
    val url: String = "",
    val user: String = "",
    val password: String = "",
    val endpoint: String = "",
    val region: String = "us-east-1",
    val bucket: String = "",
    val prefix: String = "",
    val accessKeyId: String = "",
    val secretAccessKey: String = "",
    val pathStyle: Boolean = false,
    val host: String = "",
    val port: Int = 22,
    val privateKey: String = "",
    val keyPassphrase: String = "",
    val path: String = "",
    val hostKey: String = "",
    val backup: Boolean = true,
    val sync: Boolean = false,
) {
    val remote: Boolean get() = kind != "folder"
}

/** How a place's last run went. */
@Serializable
data class RunResult(val atMs: Long, val problem: String? = null)

object Destinations {
    private const val FILE = "destinations"
    private val serializer = ListSerializer(Destination.serializer())
    private val mutable = MutableStateFlow<List<Destination>>(emptyList())
    val list: StateFlow<List<Destination>> = mutable

    fun load(context: Context) {
        val raw = Sealed.read(context, FILE) ?: return
        mutable.value = runCatching { Json.decodeFromString(serializer, raw.decodeToString()) }.getOrDefault(emptyList())
    }

    private fun save(context: Context, value: List<Destination>) {
        mutable.value = value
        Sealed.write(context, FILE, Json.encodeToString(serializer, value).encodeToByteArray())
    }

    fun put(context: Context, destination: Destination) {
        val rest = mutable.value.filter { it.id != destination.id }
        save(context, (rest + destination).sortedBy { it.label.lowercase() })
    }

    fun remove(context: Context, id: String) {
        save(context, mutable.value.filter { it.id != id })
        if (id.isNotEmpty()) context.getSharedPreferences("runs", Context.MODE_PRIVATE).edit().remove(id).apply()
    }

    fun forget() {
        mutable.value = emptyList()
    }

    fun sink(context: Context, d: Destination): BackupSink = when (d.kind) {
        "folder" -> SafFolderSink(context, Uri.parse(d.uri))
        "webdav" -> WebDavSink(d.url, d.user, d.password)
        "s3" -> S3Sink(
            S3Config(
                endpoint = d.endpoint,
                region = d.region,
                bucket = d.bucket,
                prefix = d.prefix,
                accessKeyId = d.accessKeyId,
                secretAccessKey = d.secretAccessKey,
                pathStyle = d.pathStyle,
            ),
        )
        "sftp" -> SftpSink(
            SftpConfig(
                host = d.host,
                port = d.port,
                user = d.user,
                password = d.password,
                privateKey = d.privateKey,
                keyPassphrase = d.keyPassphrase,
                path = d.path,
                hostKey = d.hostKey,
            ),
        ) { key -> put(context, d.copy(hostKey = key)) }
        else -> error("unknown destination")
    }

    fun lastRun(context: Context, id: String): RunResult? =
        context.getSharedPreferences("runs", Context.MODE_PRIVATE).getString(id, null)?.let {
            runCatching { Json.decodeFromString(RunResult.serializer(), it) }.getOrNull()
        }

    fun noteRun(context: Context, id: String, result: RunResult) {
        context.getSharedPreferences("runs", Context.MODE_PRIVATE).edit()
            .putString(id, Json.encodeToString(RunResult.serializer(), result)).apply()
    }
}

/** Words for a failure at a destination. */
fun sinkProblem(failure: Throwable): String = when (sinkErrorOf(failure)) {
    SinkError.NOT_CONFIGURED -> "This place is not set up yet."
    SinkError.PERMISSION_LOST -> "Access was refused. Check the login, or choose the folder again."
    SinkError.TARGET_MISSING -> "That folder or bucket is not there."
    SinkError.OUT_OF_SPACE -> "There is no space left there."
    SinkError.OUT_OF_MEMORY -> "This phone ran out of memory building the backup."
    SinkError.UNSAFE -> "This server is not safe to use: its identity changed, or it will not encrypt the connection."
    SinkError.IO -> failure.message?.takeIf { it.isNotBlank() && it != "IO" } ?: "The connection failed. WM Pass will try again."
}

object Backups {
    private const val PREFIX = "wmpass-backup-"
    private const val EXTENSION = ".wmbackup"
    private const val WORK = "wmpass-backup"
    private val pipeline = BackupPipeline(PREFIX, EXTENSION)

    /** The names WM Pass keeps in a destination, so a listing shows nothing else there. */
    fun isOurs(name: String) =
        (name.startsWith(PREFIX) && name.endsWith(EXTENSION)) ||
            name.endsWith(SYNC_EXTENSION) ||
            name.endsWith(BLOB_EXTENSION) ||
            name.endsWith(SHARED_EXTENSION) ||
            name.endsWith(MANIFEST_EXTENSION)

    const val SYNC_EXTENSION = ".wmsync"
    const val BLOB_EXTENSION = ".wmblob"
    const val SHARED_EXTENSION = ".wmshared"
    const val MANIFEST_EXTENSION = ".wmmanifest"

    private fun stamp(): String =
        SimpleDateFormat("yyyyMMdd-HHmmss", Locale.US).apply { timeZone = TimeZone.getTimeZone("UTC") }.format(Date())

    private fun prefs(context: Context) = context.getSharedPreferences("backup", Context.MODE_PRIVATE)

    fun everyDays(context: Context): Int = prefs(context).getInt("every_days", 0)
    fun keep(context: Context): Int = prefs(context).getInt("keep", 7)

    fun setKeep(context: Context, value: Int) = prefs(context).edit().putInt("keep", value).apply()

    /** Turns scheduled backups on, off, or to a new interval. */
    fun schedule(context: Context, days: Int) {
        prefs(context).edit().putInt("every_days", days).apply()
        val work = WorkManager.getInstance(context)
        if (days <= 0) {
            work.cancelUniqueWork(WORK)
            return
        }
        val request = PeriodicWorkRequestBuilder<BackupWorker>(days.toLong(), TimeUnit.DAYS)
            .setConstraints(Constraints.Builder().setRequiresBatteryNotLow(true).build())
            .build()
        work.enqueueUniquePeriodicWork(WORK, ExistingPeriodicWorkPolicy.UPDATE, request)
    }

    /** One backup to one place. Fails with words a person can act on. */
    suspend fun to(context: Context, destination: Destination): Result<Unit> = withContext(Dispatchers.IO) {
        val outcome = runCatching {
            if (destination.remote && !NetAccess.allowed(context)) error("Network features are turned off.")
            val payload = Pass.bundle()
            pipeline.run(
                Destinations.sink(context, destination),
                payload,
                stamp(),
                keep(context),
                verify = Pass::bundleLooksRight,
            ).getOrThrow()
            Unit
        }
        val problem = outcome.exceptionOrNull()?.let(::sinkProblem)
        Destinations.noteRun(context, destination.id, RunResult(System.currentTimeMillis(), problem))
        problem?.let { Result.failure(IllegalStateException(it)) } ?: Result.success(Unit)
    }

    /** The copies at a place, newest first. */
    suspend fun copies(context: Context, destination: Destination): Result<List<SinkEntry>> = withContext(Dispatchers.IO) {
        Destinations.sink(context, destination).list().map { all ->
            all.filter { it.name.startsWith(PREFIX) && it.name.endsWith(EXTENSION) }.sortedByDescending { it.modifiedAtMs.takeIf { t -> t > 0 } ?: 0 }
        }
    }

    suspend fun read(context: Context, destination: Destination, entry: SinkEntry): ByteArray = withContext(Dispatchers.IO) {
        Destinations.sink(context, destination).read(entry).getOrThrow().use { it.readBytes() }
    }
}

/** Runs while the vault is locked, which is safe because it only ever moves ciphertext. */
class BackupWorker(context: Context, params: WorkerParameters) : CoroutineWorker(context, params) {
    override suspend fun doWork(): Result {
        Pass.init(applicationContext)
        Destinations.load(applicationContext)
        val places = Destinations.list.value.filter { it.backup }
        if (places.isEmpty()) return Result.success()
        val outcomes = places.map { Backups.to(applicationContext, it) }
        return if (outcomes.all { it.isSuccess }) Result.success() else Result.retry()
    }
}

/**
 * The open vault's own copy at each place set to sync, one file per device. Each device merges
 * the others' files, which cannot lose a value, and writes its own.
 */
object FileSync {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)
    private var pending: Job? = null
    val problem = MutableStateFlow<String?>(null)

    /** A little after the last change, so that a burst of edits makes one trip. */
    fun soon(context: Context) {
        if (Destinations.list.value.none { it.sync } || !NetAccess.allowed(context)) return
        pending?.cancel()
        pending = scope.launch {
            delay(20_000)
            now(context)
        }
    }

    suspend fun now(context: Context): Int = withContext(Dispatchers.IO) {
        var trips = 0
        if (Pass.state.value !is LockState.Unlocked || Pass.account.value != null || Pass.child.value != null) return@withContext 0
        var failure: String? = null
        Destinations.list.value.filter { it.sync }.forEach { place ->
            val done = runCatching { one(context, place) }
            done.exceptionOrNull()?.let { failure = sinkProblem(it) }
            Destinations.noteRun(context, place.id, RunResult(System.currentTimeMillis(), done.exceptionOrNull()?.let(::sinkProblem)))
            if (done.isSuccess) trips++
        }
        problem.value = failure
        trips
    }

    private suspend fun one(context: Context, place: Destination) {
        if (place.remote && !NetAccess.allowed(context)) error("Network features are turned off.")
        val sink = Destinations.sink(context, place)
        val mine = Pass.plainCall("vault.sync_name").obj["name"]!!.jsonPrimitive.content
        val listing = sink.list().getOrThrow()
        // Every other device's copy. A file under another key fails to open and is left alone.
        listing.filter { it.name.endsWith(Backups.SYNC_EXTENSION) && it.name != "$mine${Backups.SYNC_EXTENSION}" }.forEach { other ->
            val bytes = sink.read(other).getOrThrow().use { it.readBytes() }
            runCatching { Pass.mergeCopy(bytes) }
        }
        Pass.flush()
        val group = Pass.plainCall("vault.chaff_name").obj["name"]!!.jsonPrimitive.content.take(16)
        // Attachments are sealed already. Each goes up once and stays, named under this vault.
        val blobPrefix = "$group-"
        val have = listing.filter { it.name.startsWith(blobPrefix) && it.name.endsWith(Backups.BLOB_EXTENSION) }
        val haveNames = have.map { it.name.removePrefix(blobPrefix).removeSuffix(Backups.BLOB_EXTENSION) }.toSet()
        val local = Pass.blobNames().toSet()
        (local - haveNames).forEach { name ->
            val bytes = Pass.readBlob(name)
            sink.write("$blobPrefix$name${Backups.BLOB_EXTENSION}", "application/octet-stream") { it.write(bytes) }.getOrThrow()
        }
        have.filter { it.name.removePrefix(blobPrefix).removeSuffix(Backups.BLOB_EXTENSION) !in local }.forEach { entry ->
            val bytes = sink.read(entry).getOrThrow().use { it.readBytes() }
            Pass.writeBlob(entry.name.removePrefix(blobPrefix).removeSuffix(Backups.BLOB_EXTENSION), bytes)
        }
        val file = Pass.sealedFile()
        sink.write("$mine${Backups.SYNC_EXTENSION}", "application/octet-stream") { it.write(file) }.getOrThrow()
    }
}

/** A vault shared with other people, kept at a destination. */
object SharedVaults {
    private fun vaultName(file: String) = "$file${Backups.SHARED_EXTENSION}"
    private fun manifestName(file: String) = "$file${Backups.MANIFEST_EXTENSION}"

    private fun folderPrefs(context: Context) = context.getSharedPreferences("shared_places", Context.MODE_PRIVATE)

    /** Which destination holds a shared vault on this device. */
    fun placeOf(context: Context, file: String): Destination? {
        val id = folderPrefs(context).getString(file, null) ?: return null
        return Destinations.list.value.firstOrNull { it.id == id }
    }

    fun setPlace(context: Context, file: String, destination: Destination) =
        folderPrefs(context).edit().putString(file, destination.id).apply()

    private suspend fun read(context: Context, place: Destination, name: String): ByteArray {
        val sink = Destinations.sink(context, place)
        val entry = sink.list().getOrThrow().firstOrNull { it.name == name } ?: error("That file is not at ${place.label} yet.")
        return sink.read(entry).getOrThrow().use { it.readBytes() }
    }

    private suspend fun write(context: Context, place: Destination, name: String, bytes: ByteArray) {
        Destinations.sink(context, place).write(name, "application/octet-stream") { it.write(bytes) }.getOrThrow()
    }

    suspend fun create(context: Context, place: Destination, name: String, myName: String): String = withContext(Dispatchers.IO) {
        val reply = Pass.change("shared.create") {
            put("name", name)
            put("my_name", myName)
        }
        val file = reply.obj["file"]!!.jsonPrimitive.content
        write(context, place, vaultName(file), reply.blobs[0])
        write(context, place, manifestName(file), reply.blobs[1])
        setPlace(context, file, place)
        Pass.flush()
        file
    }

    suspend fun manifest(context: Context, place: Destination, file: String): ByteArray =
        withContext(Dispatchers.IO) { read(context, place, manifestName(file)) }

    suspend fun addMember(context: Context, place: Destination, file: String, code: String): String = withContext(Dispatchers.IO) {
        val current = read(context, place, manifestName(file))
        val reply = Pass.plainCall("shared.add_member", blobs = listOf(current)) {
            put("file", file)
            put("code", code)
        }
        write(context, place, manifestName(file), reply.blobs[0])
        reply.obj["invite"]!!.jsonPrimitive.content
    }

    suspend fun removeMember(context: Context, place: Destination, file: String, key: String) = withContext(Dispatchers.IO) {
        val reply = Pass.change(
            "shared.remove_member",
            blobs = listOf(read(context, place, manifestName(file)), read(context, place, vaultName(file))),
        ) {
            put("file", file)
            put("key", key)
        }
        // The vault first: a manifest pointing at a key the file does not use yet would lock
        // everyone out if this stopped halfway.
        write(context, place, vaultName(file), reply.blobs[1])
        write(context, place, manifestName(file), reply.blobs[0])
        Pass.flush()
    }

    /** Opens it through the normal screens. Saving merges with the copy at the destination first. */
    suspend fun open(context: Context, place: Destination, file: String) = withContext(Dispatchers.IO) {
        val manifest = read(context, place, manifestName(file))
        val sealed = read(context, place, vaultName(file))
        val reply = Pass.change("shared.open", blobs = listOf(manifest, sealed)) {
            put("file", file)
        }
        Pass.enterChild(
            file,
            reply,
            before = { Pass.absorb(read(context, place, vaultName(file))) },
        ) { bytes -> write(context, place, vaultName(file), bytes) }
    }

    /** Takes in what others have written since, then leaves the merged copy there. */
    suspend fun refresh(context: Context, place: Destination, file: String) = withContext(Dispatchers.IO) {
        Pass.mergeCopy(read(context, place, vaultName(file)))
        Pass.flush()
    }
}

package com.wasimaster.wmpass.core

import kotlinx.coroutines.CoroutineDispatcher
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ExperimentalCoroutinesApi
import kotlinx.coroutines.withContext
import kotlinx.serialization.DeserializationStrategy
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonObject

/**
 * A secret on the Kotlin side: bytes, wiped as soon as they have been used.
 *
 * Whatever draws one on screen has to make a String of it, and a String cannot be wiped.
 * So a secret is turned into text only for the moment it is on show, and the view holding
 * that text is dropped when it is hidden. That window is the honest limit of what a JVM
 * language can do here; the docs say so.
 */
class SecretBytes(private val bytes: ByteArray) {
    private var wiped = false

    /** Runs [use] on the bytes, then wipes them. */
    inline fun <R> consume(use: (ByteArray) -> R): R = try {
        use(peek())
    } finally {
        wipe()
    }

    @PublishedApi
    internal fun peek(): ByteArray = bytes

    fun wipe() {
        if (!wiped) {
            bytes.fill(0)
            wiped = true
        }
    }

    /** As text, for showing it. See the note on the class. */
    fun consumeText(): String = consume { String(it, Charsets.UTF_8) }
}

/** What came back from one call. */
class EngineReply(val json: JsonElement, val secret: SecretBytes?, val blobs: List<ByteArray>) {
    val obj: JsonObject get() = json.jsonObject
}

/**
 * Every call into the core, on one thread. The core holds one lock anyway, and a single
 * thread here means a slow unlock queues the next call instead of pinning another worker.
 */
@OptIn(ExperimentalCoroutinesApi::class)
object Engine {
    val json = Json {
        ignoreUnknownKeys = true
        explicitNulls = false
        coerceInputValues = true
    }
    private val thread: CoroutineDispatcher = Dispatchers.IO.limitedParallelism(1)

    fun start(device: Int) = Native.load(device)

    suspend fun call(
        method: String,
        handle: Long = 0,
        args: JsonObjectBuilder.() -> Unit = {},
        secret: ByteArray? = null,
        blobs: List<ByteArray> = emptyList(),
    ): EngineReply {
        val text = buildJsonObject(args).toString()
        return withContext(thread) {
            val reply = Native.call(
                method,
                handle,
                text,
                secret,
                blobs.takeIf { it.isNotEmpty() }?.toTypedArray(),
                System.currentTimeMillis(),
            )
            val parsed = json.parseToJsonElement(reply[0] as String)
            val out = reply[1] as ByteArray?
            @Suppress("UNCHECKED_CAST")
            val files = (reply[2] as Array<ByteArray>).toList()
            EngineReply(parsed, out?.let(::SecretBytes), files)
        }
    }

    suspend fun <T> view(
        method: String,
        serializer: DeserializationStrategy<T>,
        handle: Long = 0,
        args: JsonObjectBuilder.() -> Unit = {},
        secret: ByteArray? = null,
        blobs: List<ByteArray> = emptyList(),
    ): T = json.decodeFromJsonElement(serializer, call(method, handle, args, secret, blobs).json)

    suspend fun lockAll() = withContext(thread) { Native.lockAll() }
}

package com.wasimaster.wmpass.core

/** The Rust library. One call, with bytes for anything secret. See `wmpass-session`. */
internal object Native {
    private var loaded = false

    @Synchronized
    fun load(device: Int) {
        if (!loaded) {
            System.loadLibrary("wmpass_jni")
            loaded = true
        }
        init(device)
    }

    private external fun init(device: Int)

    external fun lockAll()

    /** Returns `{String json, ByteArray? secret, Array<ByteArray> blobs}`. */
    external fun call(
        method: String,
        handle: Long,
        args: String,
        secret: ByteArray?,
        blobs: Array<ByteArray>?,
        nowMs: Long,
    ): Array<Any?>
}

/** A refusal from the Rust side. The message is safe to show and never holds a secret. */
class NativeFailure(text: String) : Exception(text.substringAfter('\n')) {
    val code: String = text.substringBefore('\n')
}

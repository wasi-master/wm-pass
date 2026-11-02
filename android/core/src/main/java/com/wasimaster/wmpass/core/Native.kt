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

    /** Anything that waits on a network. Runs outside the core's lock. */
    external fun net(
        method: String,
        handle: Long,
        args: String,
        secret: ByteArray?,
        online: Boolean,
        nowMs: Long,
    ): Array<Any?>

    /** A hardware key operation, over [device]. Blocks until the key has answered. */
    external fun hardware(method: String, factor: String, device: HwDevice): Array<Any?>

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

/** A hardware key as Rust talks to it. Each side implements only what it can carry. */
interface HwDevice {
    fun transceive(apdu: ByteArray): ByteArray = throw UnsupportedOperationException()
    fun hidWrite(report: ByteArray): ByteArray = throw UnsupportedOperationException()
    fun hidRead(ignored: ByteArray): ByteArray = throw UnsupportedOperationException()
    fun otpGet(ignored: ByteArray): ByteArray = throw UnsupportedOperationException()
    fun otpSet(report: ByteArray): ByteArray = throw UnsupportedOperationException()
}

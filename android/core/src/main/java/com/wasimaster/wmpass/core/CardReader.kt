package com.wasimaster.wmpass.core

/**
 * Reads a payment card's number, and its expiry when it can, from camera frames, in the Rust
 * core (`wmpass-cardscan`). Frames are read and dropped; nothing is kept or sent anywhere.
 * One scanner at a time, and every call from the same thread.
 */
object CardReader {
    /** What the frames agreed on: the number's digits, and the expiry as `MM/YYYY` if seen. */
    class Read(val number: String, val expiry: String?)

    /** Loads the model from its bytes and forgets any earlier reads. False when it will not load. */
    fun open(model: ByteArray): Boolean = Native.cardOpen(model)

    /**
     * Reads one RGBA frame, [rotation] degrees clockwise from upright, looking only inside
     * [crop] (left, top, right, bottom of the upright frame, as fractions). Null until two
     * frames agree on a number.
     */
    fun frame(pixels: ByteArray, width: Int, height: Int, rowStride: Int, rotation: Int, crop: FloatArray): Read? {
        val reply = Native.cardFrame(pixels, width, height, rowStride, rotation, crop, System.currentTimeMillis()) ?: return null
        val digits = reply[0] as ByteArray
        val number = String(digits, Charsets.US_ASCII)
        digits.fill(0)
        return Read(number, reply[1] as String?)
    }

    /** Lets go of the model and wipes what it read. */
    fun close() = Native.cardClose()
}

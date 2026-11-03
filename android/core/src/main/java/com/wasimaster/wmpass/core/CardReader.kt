package com.wasimaster.wmpass.core

/**
 * Reads a payment card from camera frames, in the Rust core (`wmpass-cardscan`). Either the
 * core runs Stripe's digit detector on each frame ([frame]), or a text recognizer on the
 * Kotlin side reads the frame and the core makes sense of its lines ([text]). Frames are read
 * and dropped; nothing is kept or sent anywhere. One scanner at a time, every call from the
 * same thread.
 */
object CardReader {
    /** What three frames agreed on: the number's digits, and the expiry (`MM/YYYY`) and name if seen. */
    class Read(val number: String, val expiry: String?, val name: String?)

    /**
     * Starts a scan and forgets any earlier reads. [model] is the detector's bytes, or null
     * when a text recognizer reads the frames. False when the model will not load.
     */
    fun open(model: ByteArray?): Boolean = Native.cardOpen(model)

    /**
     * Reads one RGBA frame, [rotation] degrees clockwise from upright, looking only inside
     * [crop] (left, top, right, bottom of the upright frame, as fractions). Null until three
     * frames agree on a number.
     */
    fun frame(pixels: ByteArray, width: Int, height: Int, rowStride: Int, rotation: Int, crop: FloatArray): Read? =
        reply(Native.cardFrame(pixels, width, height, rowStride, rotation, crop, System.currentTimeMillis()))

    /**
     * One frame as a text recognizer read it: its [lines], and in [boxes] each line's left and
     * top as fractions of the card's window. Null until three frames agree on a number.
     */
    fun text(lines: Array<String>, boxes: FloatArray): Read? =
        reply(Native.cardText(lines, boxes, System.currentTimeMillis()))

    /** Lets go of the model and wipes what it read. */
    fun close() = Native.cardClose()

    private fun reply(reply: Array<Any?>?): Read? {
        if (reply == null) return null
        val digits = reply[0] as ByteArray
        val number = String(digits, Charsets.US_ASCII)
        digits.fill(0)
        return Read(number, reply[1] as String?, reply[2] as String?)
    }
}

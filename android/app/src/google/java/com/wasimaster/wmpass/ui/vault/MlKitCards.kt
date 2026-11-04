package com.wasimaster.wmpass.ui.vault

import android.content.Context
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Rect
import androidx.camera.core.ImageProxy
import com.google.android.gms.common.moduleinstall.ModuleInstall
import com.google.android.gms.common.moduleinstall.ModuleInstallRequest
import com.google.android.gms.tasks.Tasks
import com.google.mlkit.vision.common.InputImage
import com.google.mlkit.vision.text.TextRecognition
import com.google.mlkit.vision.text.TextRecognizer
import com.google.mlkit.vision.text.latin.TextRecognizerOptions
import com.wasimaster.wmpass.core.CardReader
import java.util.concurrent.TimeUnit

/**
 * Google's text reader for the card scanner: ML Kit's Latin recognizer, from Play services.
 * In builds with Google's SDKs only; src/nogoogle has a stand-in with no engine.
 */
internal object MlKitCards {
    fun engine(): CardEngine? = MlKitCardEngine()

    /**
     * What ML Kit alone reads in an upright picture: its lines, and each line's left and top as
     * fractions of the picture. For measuring it against the detector; the scanner uses [engine].
     * Null when the model is not on this phone yet.
     */
    fun lines(context: Context, picture: Bitmap): Pair<Array<String>, FloatArray>? {
        val client = TextRecognition.getClient(TextRecognizerOptions.DEFAULT_OPTIONS)
        try {
            val ready = runCatching { Tasks.await(ModuleInstall.getClient(context).areModulesAvailable(client), 10, TimeUnit.SECONDS).areModulesAvailable() }
                .getOrDefault(false)
            if (!ready) return null
            val text = runCatching { Tasks.await(client.process(InputImage.fromBitmap(picture, 0))) }.getOrNull() ?: return null
            val lines = text.textBlocks.flatMap { it.lines }
            val boxes = FloatArray(lines.size * 2)
            lines.forEachIndexed { i, line ->
                boxes[i * 2] = (line.boundingBox?.left ?: 0) / picture.width.toFloat()
                boxes[i * 2 + 1] = (line.boundingBox?.top ?: 0) / picture.height.toFloat()
            }
            return Array(lines.size) { lines[it].text } to boxes
        } finally {
            client.close()
        }
    }
}

/**
 * Runs Stripe's digit detector and ML Kit on the card's part of each frame, on this phone, into
 * one tally in the Rust core. ML Kit is a general text reader: on embossed and foil digits it
 * misreads or misses the number that the detector, trained on card digits, reads cleanly. So the
 * number comes from whichever agrees first, and ML Kit adds the expiry and the name, which the
 * detector cannot read.
 */
private class MlKitCardEngine : CardEngine {
    private var recognizer: TextRecognizer? = null
    private var stripe: StripeFrames? = null

    override fun open(context: Context): Boolean {
        val client = TextRecognition.getClient(TextRecognizerOptions.DEFAULT_OPTIONS)
        val modules = ModuleInstall.getClient(context)
        val ready = runCatching { Tasks.await(modules.areModulesAvailable(client), 10, TimeUnit.SECONDS).areModulesAvailable() }
            .getOrDefault(false)
        if (!ready) {
            // Play services fetches the model for next time; this scan falls back to WM Pass's own reader.
            runCatching { modules.installModules(ModuleInstallRequest.Builder().addApi(client).build()) }
            client.close()
            return false
        }
        // Without the model, ML Kit reads alone.
        val digits = StripeFrames().takeIf { it.open(context) }
        if (digits == null && !CardReader.open(null)) {
            client.close()
            return false
        }
        stripe = digits
        recognizer = client
        return true
    }

    override fun read(image: ImageProxy, crop: FloatArray): CardReader.Read? {
        val recognizer = recognizer ?: return null
        val digits = stripe?.read(image, crop)
        val rotation = image.imageInfo.rotationDegrees
        val whole = image.toBitmap()
        val cut = sensorRect(crop, whole.width, whole.height, rotation)
        if (cut == null) {
            whole.eraseColor(0)
            whole.recycle()
            return digits
        }
        // Only the card's window goes to ML Kit.
        val card = Bitmap.createBitmap(cut.width(), cut.height(), Bitmap.Config.ARGB_8888)
        Canvas(card).drawBitmap(whole, cut, Rect(0, 0, cut.width(), cut.height()), null)
        whole.eraseColor(0)
        whole.recycle()
        val text = runCatching { Tasks.await(recognizer.process(InputImage.fromBitmap(card, rotation))) }.getOrNull()
        card.eraseColor(0)
        card.recycle()
        if (text == null) return digits
        // ML Kit's boxes are in the upright picture of the window.
        val width = (if (rotation % 180 == 90) cut.height() else cut.width()).toFloat()
        val height = (if (rotation % 180 == 90) cut.width() else cut.height()).toFloat()
        val lines = text.textBlocks.flatMap { it.lines }
        val boxes = FloatArray(lines.size * 2)
        lines.forEachIndexed { i, line ->
            val box = line.boundingBox
            boxes[i * 2] = (box?.left ?: 0) / width
            boxes[i * 2 + 1] = (box?.top ?: 0) / height
        }
        return CardReader.text(Array(lines.size) { lines[it].text }, boxes) ?: digits
    }

    override fun readPicture(picture: Bitmap): CardReader.Read? {
        val recognizer = recognizer ?: return null
        val text = runCatching { Tasks.await(recognizer.process(InputImage.fromBitmap(picture, 0))) }.getOrNull()
        val lines = text?.textBlocks?.flatMap { it.lines }.orEmpty()
        val boxes = FloatArray(lines.size * 2)
        lines.forEachIndexed { i, line ->
            boxes[i * 2] = (line.boundingBox?.left ?: 0) / picture.width.toFloat()
            boxes[i * 2 + 1] = (line.boundingBox?.top ?: 0) / picture.height.toFloat()
        }
        val words = Array(lines.size) { lines[it].text }
        var read: CardReader.Read? = null
        repeat(3) {
            read = stripe?.readPicture(picture) ?: read
            read = CardReader.text(words, boxes) ?: read
        }
        return read
    }

    override fun close() {
        recognizer?.close()
        recognizer = null
        val digits = stripe
        stripe = null
        if (digits != null) digits.close() else CardReader.close()
    }
}

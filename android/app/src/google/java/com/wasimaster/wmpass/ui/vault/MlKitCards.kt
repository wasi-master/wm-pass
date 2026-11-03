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
}

/**
 * Runs ML Kit on the card's part of each frame, on this phone, and hands its lines to the Rust
 * core, which finds the number, expiry and name in them as it does for WM Pass's own reader.
 */
private class MlKitCardEngine : CardEngine {
    private var recognizer: TextRecognizer? = null

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
        if (!CardReader.open(null)) {
            client.close()
            return false
        }
        recognizer = client
        return true
    }

    override fun read(image: ImageProxy, crop: FloatArray): CardReader.Read? {
        val recognizer = recognizer ?: return null
        val rotation = image.imageInfo.rotationDegrees
        val whole = image.toBitmap()
        val cut = sensorRect(crop, whole.width, whole.height, rotation)
        if (cut == null) {
            whole.eraseColor(0)
            whole.recycle()
            return null
        }
        // Only the card's window goes to ML Kit.
        val card = Bitmap.createBitmap(cut.width(), cut.height(), Bitmap.Config.ARGB_8888)
        Canvas(card).drawBitmap(whole, cut, Rect(0, 0, cut.width(), cut.height()), null)
        whole.eraseColor(0)
        whole.recycle()
        val text = runCatching { Tasks.await(recognizer.process(InputImage.fromBitmap(card, rotation))) }.getOrNull()
        card.eraseColor(0)
        card.recycle()
        if (text == null) return null
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
        return CardReader.text(Array(lines.size) { lines[it].text }, boxes)
    }

    override fun close() {
        recognizer?.close()
        recognizer = null
        CardReader.close()
    }
}

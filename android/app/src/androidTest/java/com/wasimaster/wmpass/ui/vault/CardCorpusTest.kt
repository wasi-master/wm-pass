package com.wasimaster.wmpass.ui.vault

import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.os.ParcelFileDescriptor
import android.util.Log
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.wasimaster.wmpass.core.CardReader
import org.json.JSONObject
import org.junit.Test
import org.junit.runner.RunWith

/**
 * The card readers side by side on a folder of card pictures, on a phone: Stripe's detector
 * alone, ML Kit alone, and the two together as Google's reader now runs them. Pictures and
 * `truth.json` (as `tools/cardscan/make_cards.py` writes it) go in `/data/local/tmp/wmpass-cards`
 * by `adb push`; the test reads them through the shell, so they never enter the app's storage.
 * Results go to logcat under `CardCorpus`. Nothing passes or fails on them: this measures.
 *
 * Run with `am instrument`, never Gradle's connected test task, which uninstalls the app (and
 * its vault) when it ends:
 * `adb shell am instrument -w -e class com.wasimaster.wmpass.ui.vault.CardCorpusTest
 * com.wasimaster.wmpass.test/androidx.test.runner.AndroidJUnitRunner`
 */
@RunWith(AndroidJUnit4::class)
class CardCorpusTest {
    private val instrumentation = InstrumentationRegistry.getInstrumentation()

    private fun shell(command: String): ByteArray =
        ParcelFileDescriptor.AutoCloseInputStream(instrumentation.uiAutomation.executeShellCommand(command)).use { it.readBytes() }

    @Test
    fun measuresTheReaders() {
        val folder = "/data/local/tmp/wmpass-cards"
        val truthText = String(shell("cat $folder/truth.json"))
        if (truthText.isBlank()) {
            Log.i(TAG, "no pictures in $folder")
            return
        }
        val truth = JSONObject(truthText)
        val context = instrumentation.targetContext
        val tally = mutableMapOf("stripe" to 0, "mlkit" to 0, "both" to 0, "expiry" to 0, "name" to 0)
        val names = truth.keys().asSequence().sorted().toList()
        for (file in names) {
            val facts = truth.getJSONObject(file)
            val want = facts.getString("number")
            val bytes = shell("cat $folder/$file")
            val picture = BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.copy(Bitmap.Config.ARGB_8888, false) ?: continue

            val stripe = StripeFrames()
            val alone = if (stripe.open(context)) {
                var read: CardReader.Read? = null
                repeat(3) { read = stripe.readPicture(picture) ?: read }
                read
            } else {
                null
            }
            stripe.close()

            val lines = MlKitCards.lines(context, picture)
            val text = if (lines != null && CardReader.open(null)) {
                var read: CardReader.Read? = null
                repeat(3) { read = CardReader.text(lines.first, lines.second) ?: read }
                CardReader.close()
                read
            } else {
                null
            }

            val engine = MlKitCards.engine()
            val both = if (engine != null && engine.open(context)) engine.readPicture(picture).also { engine.close() } else null

            fun mark(read: CardReader.Read?) = when (read?.number) {
                null -> "none "
                want -> "right"
                else -> "WRONG"
            }
            if (alone?.number == want) tally["stripe"] = tally.getValue("stripe") + 1
            if (text?.number == want) tally["mlkit"] = tally.getValue("mlkit") + 1
            if (both?.number == want) tally["both"] = tally.getValue("both") + 1
            if (facts.has("expiry") && both?.expiry == facts.getString("expiry")) tally["expiry"] = tally.getValue("expiry") + 1
            if (facts.has("name") && both?.name == facts.getString("name")) tally["name"] = tally.getValue("name") + 1
            Log.i(
                TAG,
                "$file stripe=${mark(alone)} mlkit=${mark(text)} both=${mark(both)} expiry=${both?.expiry} name=${both?.name} " +
                    "style=${facts.optString("style")} font=${facts.optString("font")} lines=${lines?.first?.joinToString(" | ")}",
            )
            picture.recycle()
        }
        Log.i(TAG, "of ${names.size}: $tally")
    }

    private companion object {
        const val TAG = "CardCorpus"
    }
}

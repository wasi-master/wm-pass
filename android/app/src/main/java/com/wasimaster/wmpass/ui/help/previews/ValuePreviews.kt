package com.wasimaster.wmpass.ui.help.previews

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.parts.colouredSecret
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.GenState
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.intOrNull
import kotlin.math.roundToInt

/** One code, stopped at a moment with eighteen seconds left. A tap shows a hidden one. */
@Composable
internal fun CodePreview(prefs: Prefs, showing: Showing) {
    val colors = palette
    val size = showing.string("code_size", prefs.codeSize.value)
    val groups = showing.flag("code_groups", prefs.codeGroups.value)
    val next = showing.flag("code_next", prefs.codeNext.value)
    val ring = showing.flag("code_ring", prefs.codeRing.value)
    val hiddenUntilTapped = showing.flag("code_hidden", prefs.codeHidden.value)
    var revealed by remember { mutableStateOf(false) }
    val hidden = hiddenUntilTapped && !revealed
    val scale = when (size) {
        "small" -> 0.8f
        "large" -> 1.25f
        else -> 1f
    }
    fun show(code: String) = if (groups) "${code.take(3)} ${code.drop(3)}" else code
    Row(
        Modifier.fillMaxWidth().clickable { revealed = !revealed }.padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SiteTile("GitHub", "github.com", "otp", size = 40.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text("GitHub", color = colors.text, style = MaterialTheme.typography.bodyMedium)
            Text("sam@example.com", color = colors.textMid, style = MaterialTheme.typography.bodySmall)
            Spacer(Modifier.size(4.dp))
            val style = PassType.code.copy(fontSize = PassType.code.fontSize * scale)
            if (hidden) {
                Text("••• •••", color = colors.textLow, style = style)
            } else {
                Text(show("482913"), color = colors.text, style = style)
                if (next) Text("Next  ${show("105377")}", color = colors.textMid, style = PassType.secretSmall)
            }
        }
        if (ring) {
            Spacer(Modifier.width(12.dp))
            // The ring at a fixed moment: a preview is not a clock.
            Canvas(Modifier.size(26.dp)) {
                val px = 3.dp.toPx()
                val inset = px / 2
                val arc = Size(this.size.width - px, this.size.height - px)
                drawArc(colors.hairline, 0f, 360f, false, Offset(inset, inset), arc, style = Stroke(px))
                drawArc(colors.primary, -90f, 360f * 18f / 30f, false, Offset(inset, inset), arc, style = Stroke(px, cap = StrokeCap.Round))
            }
        }
    }
    PreviewCaption(if (hiddenUntilTapped) "A sample code. Tap it to show and hide it." else "A sample code.")
}

/** A real password from the generator, made with the choice being explained. Nothing is saved. */
@Composable
internal fun GeneratorPreview(prefs: Prefs, pref: String?, value: JsonPrimitive?) {
    val colors = palette
    val state = remember { GenState.fromPrefs(prefs) }
    LaunchedEffect(pref, value) {
        state.mode = if (pref == "gen_mode" && value?.isString == true) value.content else prefs.genMode.value
        val length = if (pref == "gen_length") value?.intOrNull else null
        state.length = (length ?: prefs.genLength.value).coerceIn(8, 64)
        state.words = if (pref == "gen_words") value?.intOrNull ?: prefs.genWords.value else prefs.genWords.value
        state.symbols = if (pref == "gen_symbols") value?.booleanOrNull ?: prefs.genSymbols.value else prefs.genSymbols.value
        state.noAmbiguous = if (pref == "gen_no_ambiguous") value?.booleanOrNull ?: prefs.genNoAmbiguous.value else prefs.genNoAmbiguous.value
        runCatching { state.roll() }
    }
    val bits = if (pref == "gen_show_bits") value?.booleanOrNull ?: prefs.genShowBits.value else prefs.genShowBits.value
    val shown = state.value.ifEmpty { " " }
    Row(verticalAlignment = Alignment.Bottom) {
        Text(
            if (prefs.colourSecrets.value) colouredSecret(shown, colors) else androidx.compose.ui.text.AnnotatedString(shown),
            color = colors.text,
            style = PassType.secret.copy(fontSize = 18.sp),
            modifier = Modifier.weight(1f),
        )
        if (bits && state.value.isNotEmpty()) {
            Text("${state.bits.roundToInt()} bits", color = colors.textLow, style = MaterialTheme.typography.bodySmall.merge(PassType.numeric), modifier = Modifier.padding(start = 8.dp))
        }
    }
    PreviewCaption("Made just now by the generator, and not saved anywhere. Choose another option above for a new one.")
}

/** A sample password, with digits and symbols in their own colours or not. */
@Composable
internal fun SecretPreview(prefs: Prefs, showing: Showing) {
    val colors = palette
    val colour = showing.flag("colour_secrets", prefs.colourSecrets.value)
    val sample = "Tr0ub4dor&3-Ox8!lI"
    Text(
        if (colour) colouredSecret(sample, colors) else androidx.compose.ui.text.AnnotatedString(sample),
        color = colors.text,
        style = PassType.secret.copy(fontSize = 20.sp),
    )
    PreviewCaption("A sample password. Colored digits and symbols are easier to read aloud and to type on another device.")
}

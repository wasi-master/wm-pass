package com.wasimaster.wmpass.ui.help.previews

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.theme.palette
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.intOrNull

// Live parts of the real interface for the guide's sheet. Each is given the setting it explains
// and the choice to show, and reads the rest of the settings as they are, so the sample looks the
// way this phone would. Nothing in them moves until it is touched.

/** The previews the phone has. A topic whose preview is not here shows its figure, or nothing. */
private val Drawn = setOf(
    "preview.row",
    "preview.code",
    "preview.generator",
    "preview.secret",
    "preview.site_icon",
    "preview.settings_icon",
    "preview.icons",
    "preview.dark",
    "preview.sidebar",
)

fun hasPreview(id: String?): Boolean = id in Drawn

/** The choice being shown for [key]: the one picked in the sheet when it is about [key], else the setting as it is. */
internal class Showing(private val pref: String?, private val value: JsonPrimitive?) {
    fun string(key: String, current: String): String = if (pref == key && value != null && value.isString) value.content else current
    fun flag(key: String, current: Boolean): Boolean = if (pref == key) value?.booleanOrNull ?: current else current
    fun whole(key: String, current: Int): Int = if (pref == key) value?.intOrNull ?: current else current
}

/** [id]'s preview in its frame: a quiet box with a hairline, never a card inside the sheet's card. */
@Composable
fun HelpPreview(id: String, pref: String?, value: JsonPrimitive?, modifier: Modifier = Modifier) {
    if (!hasPreview(id)) return
    val colors = palette
    val prefs = Prefs.of(LocalContext.current)
    val showing = Showing(pref, value)
    val shape = RoundedCornerShape(14.dp)
    Column(
        modifier
            .fillMaxWidth()
            .border(1.dp, colors.hairline, shape)
            .background(colors.ground, shape)
            .padding(12.dp),
    ) {
        when (id) {
            "preview.row" -> RowPreview(prefs, showing)
            "preview.code" -> CodePreview(prefs, showing)
            "preview.generator" -> GeneratorPreview(prefs, pref, value)
            "preview.secret" -> SecretPreview(prefs, showing)
            "preview.site_icon" -> SiteIconPreview(prefs, showing)
            "preview.settings_icon" -> SettingsIconPreview(prefs, showing)
            "preview.icons" -> AppIconPreview(prefs, showing)
            "preview.dark" -> DarkPreview(prefs, showing)
            "preview.sidebar" -> NavBarPreview(prefs, showing)
        }
    }
}

@Composable
internal fun PreviewCaption(text: String) {
    Text(text, color = palette.textLow, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(top = 10.dp))
}

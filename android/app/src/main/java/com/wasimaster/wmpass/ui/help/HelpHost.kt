package com.wasimaster.wmpass.ui.help

import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlassCircle
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.LocalShell
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.withContext

/**
 * The guide's layers over the open vault: the sheet, the welcome and a running tour. Drawn last in
 * the vault's root, so a tour's dim covers every page and the capsule.
 */
@Composable
fun HelpHost(prefs: Prefs) {
    val shell = LocalShell.current
    LaunchedEffect(Unit) {
        // Read the guide off the main thread while the vault settles, so the first settings page
        // does not wait for it.
        withContext(Dispatchers.Default) { Help.file }
        // The first vault on this phone gets the welcome once, after it has had a moment to appear.
        if (!prefs.helpWelcomeSeen.value) {
            delay(700)
            Guide.welcome = true
        }
    }
    // A tour points at the vault's own pages, so locking ends it rather than leaving it for later.
    DisposableEffect(Unit) {
        onDispose { if (Guide.tour != null) Guide.endTour(prefs, false) }
    }
    if (Guide.stack.isNotEmpty()) HelpSheet(prefs, shell)
    if (Guide.welcome && Guide.tour == null) WelcomeSheet(prefs)
    Coach(prefs)
}

/**
 * The "?" in a screen's bar, opening [id]. Draws nothing when the phone has no such topic, or
 * when the "?" buttons are turned off under Settings, Guide.
 */
@Composable
fun HelpAction(id: String, label: String, modifier: Modifier = Modifier, tint: Color = palette.textMid) {
    val prefs = Prefs.of(LocalContext.current)
    if (prefs.helpButtons.value == "off" || Help.topic(id) == null) return
    if (palette.flat) {
        IconButton(onClick = { Guide.open(id) }, modifier = modifier) { Icon(PassIcons.Help, "About $label", tint = tint) }
    } else {
        GlassCircle(PassIcons.Help, "About $label", modifier, tint = tint) { Guide.open(id) }
    }
}

/**
 * A small "?" for the end of a heading the app draws itself, such as the backup page's, where the
 * settings kit's own "?" is not there to use.
 */
@Composable
fun HelpMark(id: String, label: String) {
    val prefs = Prefs.of(LocalContext.current)
    if (prefs.helpButtons.value == "off" || Help.topic(id) == null) return
    IconButton(onClick = { Guide.open(id) }, modifier = Modifier.size(32.dp)) {
        Icon(PassIcons.Help, "About $label", tint = palette.textLow, modifier = Modifier.size(18.dp))
    }
}

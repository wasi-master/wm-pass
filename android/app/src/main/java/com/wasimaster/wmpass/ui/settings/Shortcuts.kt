package com.wasimaster.wmpass.ui.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.focusable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.shape.RoundedCornerShape
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
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.input.key.KeyEvent
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.isAltPressed
import androidx.compose.ui.input.key.isCtrlPressed
import androidx.compose.ui.input.key.isMetaPressed
import androidx.compose.ui.input.key.isShiftPressed
import androidx.compose.ui.input.key.key
import androidx.compose.ui.input.key.nativeKeyCode
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.ConfirmSheet
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmsuite.settings.WmScreen

/** A key press as stored and matched. Null for a key alone that nothing could mean: a modifier. */
fun chordOf(event: KeyEvent): String? {
    val name = android.view.KeyEvent.keyCodeToString(event.key.nativeKeyCode).removePrefix("KEYCODE_")
    if (name.contains("CTRL") || name.contains("SHIFT") || name.contains("ALT") || name.contains("META")) return null
    val parts = buildList {
        if (event.isCtrlPressed) add("Ctrl")
        if (event.isAltPressed) add("Alt")
        if (event.isShiftPressed) add("Shift")
        if (event.isMetaPressed) add("Meta")
        add(name.lowercase().replaceFirstChar { it.uppercase() })
    }
    return parts.joinToString("+")
}

val ShortcutNames = listOf(
    "search" to "Search",
    "add" to "New item",
    "lock" to "Lock",
    "generator" to "Password generator",
    "codes" to "Codes",
    "health" to "Health",
    "settings" to "Settings",
)

/** For a keyboard plugged in or paired. Tap a row, then press what you want. */
@Composable
fun ShortcutsScreen() {
    val colors = palette
    val shell = LocalShell.current
    val prefs = shell.prefs
    var editing by remember { mutableStateOf<String?>(null) }
    var resetting by remember { mutableStateOf(false) }
    WmScreen("Keyboard shortcuts", onBack = { shell.nav.popBackStack() }) {
        Column(Modifier.padding(horizontal = 16.dp)) {
            Text(
                "These work with a keyboard plugged in or paired. Tap one to change it.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.padding(horizontal = 6.dp, vertical = 8.dp),
            )
            GroupCard {
                ShortcutNames.forEachIndexed { index, (id, name) ->
                    if (index > 0) Hairline()
                    Row(
                        Modifier.fillMaxWidth().clickable { editing = id }.padding(horizontal = 18.dp, vertical = 14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(name, color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                        Text(
                            prefs.shortcuts[id] ?: "None",
                            color = colors.textMid,
                            style = PassType.secretSmall,
                            modifier = Modifier.clip(RoundedCornerShape(8.dp)).background(colors.surface2).padding(horizontal = 10.dp, vertical = 4.dp),
                        )
                    }
                }
            }
            Spacer(Modifier.height(14.dp))
            com.wasimaster.wmpass.ui.parts.QuietButton("Back to the usual keys", Modifier.fillMaxWidth(), PassIcons.History, height = 48.dp) { resetting = true }
            Spacer(Modifier.height(40.dp))
        }
    }
    editing?.let { id ->
        val focus = remember { FocusRequester() }
        LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
        PassSheet({ editing = null }, ShortcutNames.first { it.first == id }.second) {
            Box(
                Modifier
                    .fillMaxWidth()
                    .focusRequester(focus)
                    .focusable()
                    .onPreviewKeyEvent { event ->
                        if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                        val chord = chordOf(event) ?: return@onPreviewKeyEvent false
                        prefs.updateShortcut(id, chord)
                        editing = null
                        true
                    }
                    .padding(vertical = 26.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text("Press the keys you want", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
            }
            com.wasimaster.wmpass.ui.parts.QuietButton("No shortcut", Modifier.fillMaxWidth(), height = 48.dp) {
                prefs.updateShortcut(id, null)
                editing = null
            }
        }
    }
    if (resetting) {
        ConfirmSheet("Back to the usual keys?", "Every shortcut you changed goes back to how it started.", "Reset", danger = false, onDismiss = { resetting = false }) { prefs.resetShortcuts() }
    }
}

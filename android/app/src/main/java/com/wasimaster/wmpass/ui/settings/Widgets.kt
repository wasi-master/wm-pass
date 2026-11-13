package com.wasimaster.wmpass.ui.settings

import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.material3.Checkbox
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.widgets.WidgetFeed
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmsuite.settings.ChoiceSetting
import com.wasimaster.wmsuite.settings.NavRow
import com.wasimaster.wmsuite.settings.ReorderableColumn
import com.wasimaster.wmsuite.settings.SettingsGroup
import com.wasimaster.wmsuite.settings.ToggleSetting
import com.wasimaster.wmsuite.settings.WmScreen

private val ButtonNames = mapOf(
    "search" to "Search",
    "add" to "Add",
    "codes" to "Codes",
    "generator" to "Generator",
    "health" to "Health",
    "lock" to "Lock",
)

/** How the home screen widgets look, the quick bar's buttons, and what the codes widget shows. */
@Composable
internal fun WidgetSettingsScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val context = LocalContext.current
    var picking by rememberSaveable { mutableStateOf(false) }
    val changed = { WidgetFeed.redraw(context) }
    WmScreen("Widgets", route = "settings/widgets", onBack = { shell.nav.popBackStack() }) {
        SettingsGroup("Look") {
            item {
                ChoiceSetting(
                    "Colors",
                    info = infoFor("widget_colours"),
                    options = listOf("app" to "App theme", "system" to "Wallpaper colors", "dark" to "Dark"),
                    selected = prefs.widgetColours.value,
                    icon = PassIcons.Palette,
                    detail = choiceDetail("widget_colours"),
                ) { prefs.widgetColours.set(it); changed() }
            }
            item {
                ChoiceSetting(
                    "Background",
                    info = infoFor("widget_background"),
                    options = listOf("solid" to "Solid", "translucent" to "See-through", "none" to "None"),
                    selected = prefs.widgetBackground.value,
                    icon = PassIcons.Layers,
                    detail = choiceDetail("widget_background"),
                ) { prefs.widgetBackground.set(it); changed() }
            }
        }
        SettingsGroup("Quick bar", info = infoFor("widget_buttons")) {
            item {
                ReorderableColumn(
                    // The rows run to their edges; the card around them wants the usual inset, as on the other pages.
                    modifier = Modifier.padding(horizontal = 16.dp),
                    items = prefs.widgetButtons,
                    label = { ButtonNames[it] ?: it },
                    onReorder = { prefs.updateWidgetButtons(it); changed() },
                    onDelete = { prefs.updateWidgetButtons(prefs.widgetButtons - it); changed() },
                )
            }
            Prefs.WIDGET_BUTTONS.filter { it !in prefs.widgetButtons }.forEach { id ->
                item { NavRow("Add ${ButtonNames[id] ?: id}", icon = PassIcons.Add) { prefs.updateWidgetButtons(prefs.widgetButtons + id); changed() } }
            }
        }
        SettingsGroup("Buttons") {
            item {
                ChoiceSetting(
                    "Lock button",
                    info = infoFor("widget_lock"),
                    subtitle = if (prefs.widgetLock.value == "panic") "Locks and refuses every password for an hour." else "Locks the vault.",
                    options = listOf("lock" to "Lock", "panic" to "Panic lock"),
                    selected = prefs.widgetLock.value,
                    icon = PassIcons.Lock,
                    detail = choiceDetail("widget_lock"),
                ) { prefs.widgetLock.set(it); changed() }
            }
        }
        SettingsGroup("Codes and favorites") {
            item {
                ToggleSetting(
                    "Show on widgets",
                    "While the vault is open, the Codes and Favorites widgets show codes and the names of favorites. Locked, they show nothing.",
                    prefs.widgetAccess.value,
                    info = infoFor("widget_access"),
                    icon = PassIcons.Shield,
                ) { prefs.widgetAccess.set(it); changed() }
            }
            if (prefs.widgetAccess.value) {
                item {
                    val chosen = prefs.widgetCodes.size
                    NavRow(
                        "Codes on widgets",
                        if (chosen == 0) "Favorites that have a code" else if (chosen == 1) "1 chosen" else "$chosen chosen",
                        icon = PassIcons.Pin,
                        info = infoFor("widget_codes"),
                    ) { picking = true }
                }
            }
        }
    }
    if (picking) CodesPickSheet(prefs, onDone = { picking = false; changed() })
}

/** The items with codes, to tick the ones the Codes widget shows. None ticked means favorites. */
@Composable
private fun CodesPickSheet(prefs: Prefs, onDone: () -> Unit) {
    val colors = palette
    val entries by Pass.entries.collectAsStateWithLifecycle()
    val withCodes = remember(entries) { entries.filter { it.hasOtp && !it.trashed }.sortedBy { it.title.lowercase() } }
    var chosen by remember { mutableStateOf(prefs.widgetCodes.toSet()) }
    PassSheet(onDismiss = onDone, title = "Codes on widgets") {
        Text(
            "Tick the ones to show, up to eight, in this order. With none ticked, the widget shows your favorites that have a code.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(bottom = 8.dp),
        )
        if (withCodes.isEmpty()) {
            Text("No item has a code yet.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        }
        LazyColumn(Modifier.fillMaxWidth().heightIn(max = 420.dp)) {
            items(withCodes, key = { it.id }) { entry ->
                val on = entry.id in chosen
                // A tick box to a screen reader too, and greyed once eight are ticked rather than doing nothing.
                Row(
                    Modifier
                        .fillMaxWidth()
                        .toggleable(on, enabled = on || chosen.size < 8, role = androidx.compose.ui.semantics.Role.Checkbox) {
                            chosen = if (on) chosen - entry.id else chosen + entry.id
                        }
                        .padding(vertical = 4.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Checkbox(checked = on, onCheckedChange = null, enabled = on || chosen.size < 8)
                    Text(entry.title, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(start = 8.dp))
                }
            }
        }
        PrimaryButton("Save", Modifier.fillMaxWidth().padding(top = 12.dp)) {
            // Kept in the order they appear here, which is the order the widget draws them.
            prefs.updateWidgetCodes(withCodes.map { it.id }.filter { it in chosen })
            onDone()
        }
    }
}

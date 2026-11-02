package com.wasimaster.wmpass.ui.settings

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.LocalSiteIcons
import com.wasimaster.wmpass.ui.parts.SiteIconLook
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.parts.kindIcon
import com.wasimaster.wmpass.ui.parts.kindName
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.AllTabs
import com.wasimaster.wmpass.ui.vault.ConfirmSheet
import com.wasimaster.wmpass.ui.vault.KindOrder
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmsuite.settings.AdvancedFolds
import com.wasimaster.wmsuite.settings.ChoiceSetting
import com.wasimaster.wmsuite.settings.LocalAdvancedFolds
import com.wasimaster.wmsuite.settings.LocalIconReveal
import com.wasimaster.wmsuite.settings.NavRow
import com.wasimaster.wmsuite.settings.ReorderableColumn
import com.wasimaster.wmsuite.settings.SettingsGroup
import com.wasimaster.wmsuite.settings.SettingsHighlight
import com.wasimaster.wmsuite.settings.SettingsTables
import com.wasimaster.wmsuite.settings.ToggleSetting
import com.wasimaster.wmsuite.settings.WmScreen
import com.wasimaster.wmsuite.settings.rememberIconReveal
import kotlinx.serialization.json.put

/** Each settings page's colour. Its rows wear it, and so does the row that leads to it. */
private val RouteColours: Map<String, Color> = mapOf(
    "settings/security" to Color(0xFF10B981),
    "settings/shelf" to Color(0xFF7C4DFF),
    "settings/breaches" to Color(0xFFF4511E),
    "settings/folders" to Color(0xFFFFB300),
    "settings/trash" to Color(0xFFE53935),
    "settings/import" to Color(0xFF1E88E5),
    "settings/export" to Color(0xFF00ACC1),
    "settings/backup" to Color(0xFF3949AB),
    "settings/sync" to Color(0xFF00897B),
    "settings/vaults" to Color(0xFF5C6BC0),
    "settings/hardware" to Color(0xFF43A047),
    "settings/receive" to Color(0xFF26A69A),
    "settings/aliases" to Color(0xFF8E24AA),
    "settings/inbox" to Color(0xFF039BE5),
    "settings/usernames" to Color(0xFF5E35B1),
    "settings/compare" to Color(0xFF8D6E63),
    "settings/look" to Color(0xFFD81B60),
    "settings/themes" to Color(0xFFD81B60),
    "settings/icons" to Color(0xFFFB8C00),
    "settings/list" to Color(0xFF43A047),
    "settings/navigation" to Color(0xFF3F51B5),
    "settings/add" to Color(0xFF00BFA5),
    "settings/editor" to Color(0xFF7CB342),
    "settings/entry" to Color(0xFF26A69A),
    "settings/codes" to Color(0xFF5C6BC0),
    "settings/generator" to Color(0xFFAB47BC),
    "settings/health" to Color(0xFFEC407A),
    "settings/shortcuts" to Color(0xFF78909C),
    "settings/file" to Color(0xFF8D6E63),
    "settings/about" to Color(0xFF90A4AE),
)

/** What the settings kit asks of the app it is in. */
@Composable
fun ProvideSettingsKit(prefs: Prefs, content: @Composable () -> Unit) {
    SettingsTables.routeAccent = { route -> if (prefs.settingsIcons.value == "coloured") RouteColours[route] else null }
    val folds = remember(prefs) {
        object : AdvancedFolds {
            override val toggle: (String, Boolean) -> Unit = { id, open -> prefs.setFoldOpen(id, open) }

            @Composable
            override fun isOpen(id: String): Boolean = id in prefs.foldsOpen
        }
    }
    val reveal = rememberIconReveal(prefs.settingsIcons.value != "off", LocalReduceMotion.current)
    CompositionLocalProvider(LocalAdvancedFolds provides folds, LocalIconReveal provides reveal, content = content)
}

@Composable
private fun SwitchRow(title: String, subtitle: String?, opt: Prefs.Opt<Boolean>, icon: ImageVector) {
    ToggleSetting(title, subtitle, opt.value, icon = icon) { opt.set(it) }
}

@Composable
private fun ChoiceRow(title: String, opt: Prefs.Opt<String>, icon: ImageVector, subtitle: String? = null, options: List<Pair<String, String>>) {
    ChoiceSetting(title, subtitle = subtitle, options = options, selected = opt.value, icon = icon) { opt.set(it) }
}

@Composable
private fun back(): () -> Unit {
    val shell = LocalShell.current
    return { shell.nav.popBackStack() }
}

// ---- icons ----

@Composable
private fun PickRow(title: String, subtitle: String, selected: Boolean, onClick: () -> Unit, preview: @Composable () -> Unit) {
    val colors = palette
    Column(Modifier.fillMaxWidth().clickable(onClick = onClick).padding(horizontal = 16.dp, vertical = 12.dp)) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge)
                Text(subtitle, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            }
            if (selected) Icon(PassIcons.Check, "In use", tint = colors.primary)
        }
        Spacer(Modifier.height(10.dp))
        preview()
    }
}

private val IconStyles = listOf(
    Triple("outlined", "Material Symbols", "The standard Android look"),
    Triple("rounded", "Material Symbols Rounded", "Softer corners"),
    Triple("sharp", "Material Symbols Sharp", "Square corners"),
    Triple("light", "Rounded light", "Thin strokes and soft corners"),
)

private val SiteStyles = listOf(
    Triple("brand", "Brand colours", "Each site's mark on its own colour"),
    Triple("tinted", "Coloured marks", "Each mark in its colour, on one background"),
    Triple("uniform", "One colour", "Every mark the same, on one background"),
    Triple("letters", "Letters", "First letters only, no logos"),
)

@Composable
private fun SiteSamples() {
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        SiteTile("GitHub", "github.com", "login", size = 40.dp)
        SiteTile("Discord", "discord.com", "login", size = 40.dp)
        SiteTile("Amazon", "amazon.com", "login", size = 40.dp)
        SiteTile("Figma", "figma.com", "login", size = 40.dp)
        SiteTile("My bank", null, "login", size = 40.dp)
        SiteTile("Everyday Visa", null, "card", size = 40.dp)
    }
}

@Composable
internal fun IconsScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val colors = palette
    WmScreen("Icons", route = "settings/icons", onBack = back()) {
        SettingsGroup("App icons") {
            IconStyles.forEach { (id, name, about) ->
                item {
                    PickRow(name, about, prefs.iconStyle.value == id, { prefs.updateIconStyle(id) }) {
                        Row(horizontalArrangement = Arrangement.spacedBy(14.dp)) {
                            PassIcons.sample(id).forEach { Icon(it, null, tint = colors.textMid, modifier = Modifier.size(24.dp)) }
                        }
                    }
                }
            }
        }
        SettingsGroup("Site icons") {
            SiteStyles.forEach { (id, name, about) ->
                item {
                    PickRow(name, about, prefs.siteIconStyle.value == id, { prefs.siteIconStyle.set(id) }) {
                        CompositionLocalProvider(LocalSiteIcons provides SiteIconLook(id, prefs.siteIconShape.value)) { SiteSamples() }
                    }
                }
            }
            item {
                ChoiceRow(
                    "Shape",
                    prefs.siteIconShape,
                    PassIcons.Image,
                    options = listOf("circle" to "Circle", "rounded" to "Rounded", "square" to "Square", "none" to "No background"),
                )
            }
        }
        SettingsGroup("Settings icons") {
            item {
                ChoiceRow(
                    "Style",
                    prefs.settingsIcons,
                    PassIcons.Palette,
                    options = listOf("coloured" to "Coloured", "mono" to "One colour", "bare" to "No background", "off" to "None"),
                )
            }
            item {
                SwitchRow("Cards", "Draw each group of settings on a card.", prefs.settingsCards, PassIcons.ViewAgenda)
            }
        }
    }
}

// ---- vault list ----

private val RowActions = listOf(
    "password" to "Copy password",
    "username" to "Copy username",
    "code" to "Copy code",
)

@Composable
internal fun ListSettingsScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    WmScreen("Vault list", route = "settings/list", onBack = back()) {
        SettingsGroup("Layout") {
            item {
                ChoiceSetting(
                    "Layout",
                    options = listOf("list" to "List", "compact" to "Compact", "grid" to "Grid"),
                    selected = prefs.layout,
                    icon = PassIcons.ViewAgenda,
                ) { prefs.updateLayout(it) }
            }
            item {
                ChoiceSetting(
                    "Grouping",
                    options = listOf("smart" to "By site", "folder" to "By folder"),
                    selected = prefs.grouping,
                    icon = PassIcons.Layers,
                ) { prefs.updateGrouping(it) }
            }
            item {
                ChoiceRow("Sort by", prefs.sort, PassIcons.Sort, options = listOf("name" to "Name", "changed" to "Last changed", "kind" to "Type", "site" to "Website"))
            }
            item { SwitchRow("Pinned items first", "Keep pinned items above the rest.", prefs.pinnedOnTop, PassIcons.Keep) }
            item { SwitchRow("Fold logins on one site", "Show several logins for a site as one row.", prefs.groupSites, PassIcons.Layers) }
            item { SwitchRow("Section headings", null, prefs.listHeadings, PassIcons.FormatListBulleted) }
            item { SwitchRow("Filter chips", "All, Favourites and the types you have.", prefs.filterChips, PassIcons.FilterList) }
            item { SwitchRow("Search at the bottom", "Within reach of your thumb.", prefs.searchAtBottom, PassIcons.Search) }
        }
        SettingsGroup("Rows") {
            item { SwitchRow("Icons", null, prefs.rowIcons, PassIcons.Image) }
            item { SwitchRow("Second line", "The username or other detail under the name.", prefs.rowSubtitle, PassIcons.TextFields) }
            item { SwitchRow("Small marks", "Favourite, one-time code, passkey and attachment.", prefs.rowBadges, PassIcons.Star) }
            item {
                ChoiceRow("Button on the right", prefs.rowButton, PassIcons.ContentCopy, options = RowActions + ("none" to "None"))
            }
        }
        SettingsGroup("Gestures") {
            item {
                ChoiceRow("Tap", prefs.tapAction, PassIcons.TouchApp, options = listOf("open" to "Open") + RowActions)
            }
            item {
                ChoiceRow("Swipe right", prefs.swipeRight, PassIcons.Swipe, options = RowActions + listOf("favourite" to "Favourite", "none" to "Nothing"))
            }
            item {
                ChoiceRow("Swipe left", prefs.swipeLeft, PassIcons.Swipe, options = listOf("trash" to "Move to trash", "favourite" to "Favourite", "password" to "Copy password", "none" to "Nothing"))
            }
        }
        SettingsGroup("Above the list") {
            val all = listOf("favourites" to "Favourites", "recent" to "Recently changed", "health" to "Health", "codes" to "Codes")
            val names = all.toMap()
            item {
                ReorderableColumn(
                    items = prefs.widgets,
                    label = { names[it] ?: it },
                    onReorder = { prefs.updateWidgets(it) },
                    onDelete = { prefs.updateWidgets(prefs.widgets - it) },
                    keepLast = false,
                )
            }
            all.filter { it.first !in prefs.widgets }.forEach { (id, name) ->
                item { NavRow("Add $name", icon = PassIcons.Dashboard) { prefs.updateWidgets(prefs.widgets + id) } }
            }
        }
    }
}

// ---- navigation ----

@Composable
internal fun NavigationSettingsScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    WmScreen("Navigation", route = "settings/navigation", onBack = back()) {
        SettingsGroup("Navigation bar") {
            val names = AllTabs.associate { it.id to it.label }
            item {
                ReorderableColumn(
                    items = prefs.tabs,
                    label = { names[it] ?: it },
                    onReorder = { prefs.updateTabs(it) },
                    onDelete = { tab -> if (tab != "vault") prefs.updateTabs(prefs.tabs - tab) },
                )
            }
            AllTabs.filter { it.id !in prefs.tabs }.forEach { tab ->
                item { NavRow("Add ${tab.label}", icon = tab.icon) { prefs.updateTabs(prefs.tabs + tab.id) } }
            }
            item {
                ChoiceRow("Labels", prefs.navLabels, PassIcons.TextFields, options = listOf("all" to "All", "selected" to "Selected", "none" to "None"))
            }
            item {
                ChoiceRow("Open on", prefs.startTab, PassIcons.History, options = listOf("first" to "First page", "last" to "Last page used"))
            }
        }
        SettingsGroup("Buttons") {
            item { SwitchRow("Add button", null, prefs.showFab, PassIcons.Add) }
            item {
                ChoiceRow(
                    "Add button opens",
                    prefs.fabAction,
                    PassIcons.TouchApp,
                    options = listOf("menu" to "Add menu") + KindOrder.map { it to "New ${kindName(it).lowercase()}" },
                )
            }
            item { SwitchRow("Lock button", "Show the padlock at the top of each page.", prefs.showLock, PassIcons.Lock) }
        }
        SettingsGroup("Hand") {
            item {
                ToggleSetting("Left-handed", "Move search and add to the other side. Applies to Glass.", prefs.leftHanded, icon = PassIcons.TouchApp) { prefs.updateLeftHanded(it) }
            }
        }
    }
}

// ---- add menu ----

@Composable
internal fun AddMenuScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    WmScreen("Add menu", route = "settings/add", onBack = back()) {
        SettingsGroup("Shown") {
            item {
                ReorderableColumn(
                    items = prefs.addKinds,
                    label = { kindName(it) },
                    onReorder = { prefs.updateAddKinds(it) },
                    onDelete = { prefs.updateAddKinds(prefs.addKinds - it) },
                )
            }
        }
        val hidden = KindOrder.filter { it !in prefs.addKinds }
        if (hidden.isNotEmpty()) {
            SettingsGroup("Hidden") {
                hidden.forEach { kind ->
                    item { NavRow("Add ${kindName(kind)}", icon = kindIcon(kind)) { prefs.updateAddKinds(prefs.addKinds + kind) } }
                }
            }
        }
        SettingsGroup {
            item { NavRow("Show all in the usual order", icon = PassIcons.Refresh) { prefs.updateAddKinds(Prefs.DEFAULT_ADD) } }
        }
    }
}

// ---- editor ----

@Composable
internal fun EditorSettingsScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    WmScreen("Editor", route = "settings/editor", onBack = back()) {
        SettingsGroup("Sections open at the start") {
            listOf(
                Triple("sites", "Websites", PassIcons.Language),
                Triple("otp", "One-time code", PassIcons.Pin),
                Triple("notes", "Notes", PassIcons.StickyNote2),
                Triple("custom", "More fields", PassIcons.Tag),
                Triple("questions", "Security questions", PassIcons.Help),
                Triple("organise", "Organise", PassIcons.Folder),
                Triple("details", "More details", PassIcons.Tune),
            ).forEach { (key, name, icon) ->
                item { ToggleSetting(name, null, key in prefs.editorOpen, icon = icon) { prefs.setEditorOpen(key, it) } }
            }
        }
        SettingsGroup("Behaviour") {
            item { SwitchRow("Remember what I open", "A section you open or close stays that way next time.", prefs.editorRemember, PassIcons.History) }
            item { SwitchRow("Fold less used fields", "Such as a card's PIN or a database's connection string.", prefs.editorFoldDetails, PassIcons.ExpandMore) }
            item { SwitchRow("Start logins with a password", "Fill in a generated password on every new login.", prefs.editorGenerate, PassIcons.Casino) }
        }
    }
}

// ---- entry page ----

@Composable
internal fun EntrySettingsScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    var limit by remember { mutableStateOf<Int?>(null) }
    androidx.compose.runtime.LaunchedEffect(Unit) { limit = runCatching { Pass.info().historyLimit }.getOrNull() }
    WmScreen("Item page", route = "settings/entry", onBack = back()) {
        SettingsGroup("Buttons at the top") {
            item { SwitchRow("Show them", null, prefs.entryActions, PassIcons.TouchApp) }
            item { SwitchRow("Copy username", null, prefs.entryActionUsername, PassIcons.Person) }
            item { SwitchRow("Open site", null, prefs.entryActionOpen, PassIcons.OpenInNew) }
        }
        SettingsGroup("Fields") {
            item { SwitchRow("Tap a field to copy it", null, prefs.tapCopies, PassIcons.ContentCopy) }
            item {
                ChoiceSetting(
                    "Hide shown secrets after",
                    options = listOf(15 to "15 s", 45 to "45 s", 120 to "2 min", 0 to "Leaving"),
                    selected = prefs.revealSeconds.value,
                    icon = PassIcons.Timer,
                ) { prefs.revealSeconds.set(it) }
            }
            item { SwitchRow("Colour digits and symbols", "Makes a password easier to read aloud or retype.", prefs.colourSecrets, PassIcons.Palette) }
        }
        SettingsGroup("History") {
            limit?.let { current ->
                item {
                    ChoiceSetting(
                        "Old passwords to keep",
                        options = (listOf(5, 10, 20, 50) + current).distinct().sorted().map { it to it.toString() },
                        selected = current,
                        icon = PassIcons.History,
                    ) { value ->
                        limit = value
                        shell.launch { Pass.change("vault.prefs") { put("history_limit", value) } }
                    }
                }
            }
        }
    }
}

// ---- codes ----

@Composable
internal fun CodesSettingsScreen() {
    val prefs = LocalShell.current.prefs
    WmScreen("Codes", route = "settings/codes", onBack = back()) {
        SettingsGroup("Display") {
            item { ChoiceRow("Size", prefs.codeSize, PassIcons.MatchCase, options = listOf("small" to "Small", "normal" to "Normal", "large" to "Large")) }
            item { SwitchRow("Split in two groups", "Shows 123 456 rather than 123456.", prefs.codeGroups, PassIcons.Numbers) }
            item { SwitchRow("Show the next code", "Useful when a code is about to change.", prefs.codeNext, PassIcons.Schedule) }
            item { SwitchRow("Time ring", "How long the code has left.", prefs.codeRing, PassIcons.Timer) }
            item { ChoiceRow("Order", prefs.codeSort, PassIcons.Sort, options = listOf("default" to "As in the vault", "name" to "By name")) }
        }
        SettingsGroup("Privacy") {
            item { SwitchRow("Hide codes until tapped", "A tap shows the code for 15 seconds and copies it.", prefs.codeHidden, PassIcons.VisibilityOff) }
        }
    }
}

// ---- generator ----

@Composable
internal fun GeneratorSettingsScreen() {
    val prefs = LocalShell.current.prefs
    WmScreen("Generator", route = "settings/generator", onBack = back()) {
        SettingsGroup("What it starts with") {
            item {
                ChoiceRow(
                    "Type",
                    prefs.genMode,
                    PassIcons.Casino,
                    options = listOf("password" to "Random", "passphrase" to "Words", "pronounceable" to "Pronounceable", "pin" to "PIN", "pattern" to "Pattern"),
                )
            }
            item {
                ChoiceSetting(
                    "Length",
                    options = listOf(12, 16, 20, 24, 32, 48, 64).map { it to it.toString() },
                    selected = prefs.genLength.value,
                    icon = PassIcons.Numbers,
                ) { prefs.genLength.set(it) }
            }
            item {
                ChoiceSetting(
                    "Words",
                    options = listOf(3, 4, 5, 6, 7, 8).map { it to it.toString() },
                    selected = prefs.genWords.value,
                    icon = PassIcons.Abc,
                ) { prefs.genWords.set(it) }
            }
            item { SwitchRow("Symbols", null, prefs.genSymbols, PassIcons.Tag) }
            item { SwitchRow("Avoid look-alikes", "Leaves out characters like 0 and O.", prefs.genNoAmbiguous, PassIcons.Visibility) }
        }
        SettingsGroup("Display") {
            item { SwitchRow("Show strength in bits", null, prefs.genShowBits, PassIcons.Balance) }
        }
    }
}

// ---- health ----

@Composable
internal fun HealthSettingsScreen() {
    val prefs = LocalShell.current.prefs
    WmScreen("Health checks", route = "settings/health", onBack = back()) {
        SettingsGroup("Page") {
            item { SwitchRow("Show the score", null, prefs.healthScore, PassIcons.EcgHeart) }
        }
        SettingsGroup("Problems to list") {
            listOf(
                Triple("breached", "In a known breach", PassIcons.Dangerous),
                Triple("reused", "Used more than once", PassIcons.SwapHoriz),
                Triple("weak", "Easy to guess", PassIcons.GppBad),
                Triple("insecure", "Unencrypted sites", PassIcons.Warning),
                Triple("expiring", "Expiring soon", PassIcons.Schedule),
                Triple("old", "Not changed in a year", PassIcons.History),
                Triple("twofactor", "No second factor", PassIcons.Pin),
                Triple("duplicates", "Duplicate items", PassIcons.Layers),
            ).forEach { (key, name, icon) ->
                item { ToggleSetting(name, null, key !in prefs.healthHidden, icon = icon) { prefs.setHealthShown(key, it) } }
            }
        }
    }
}

// ---- settings file ----

@Composable
internal fun SettingsFileScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val colors = palette
    val context = LocalContext.current
    var importing by remember { mutableStateOf(false) }
    var resetting by remember { mutableStateOf(false) }
    var result by remember { mutableStateOf<String?>(null) }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri != null) context.contentResolver.openOutputStream(uri)?.use { it.write(prefs.exportText().toByteArray()) }
    }
    WmScreen("Settings file", route = "settings/file", onBack = back()) {
        Text(
            "Carry your look and layout to another device, or share them. Nothing from your vault is in this file, and neither are lock timers, screenshot blocking or other protection settings.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp),
        )
        result?.let { Text(it, color = colors.text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) }
        SettingsGroup {
            item { NavRow("Save to a file", icon = PassIcons.Download) { save.launch("wm-pass-settings.json") } }
            item {
                NavRow("Send to an app", icon = PassIcons.Share) {
                    val intent = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, prefs.exportText())
                    context.startActivity(Intent.createChooser(intent, "Share settings"))
                }
            }
            item { NavRow("Import", icon = PassIcons.Upload) { importing = true } }
            item { NavRow("Reset all customisation", "Keeps your own themes and protection settings", icon = PassIcons.Refresh) { resetting = true } }
        }
    }
    if (importing) {
        val text = rememberTextFieldState()
        var problem by remember { mutableStateOf<String?>(null) }
        fun take(raw: String) {
            try {
                val changed = prefs.importText(raw)
                result = if (changed == 1) "1 setting changed." else "$changed settings changed."
                importing = false
            } catch (failure: IllegalArgumentException) {
                problem = failure.message
            }
        }
        val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
            if (uri != null) context.contentResolver.openInputStream(uri)?.use { take(it.readBytes().decodeToString()) }
        }
        PassSheet({ importing = false }, "Import settings") {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                SheetField(text, "Paste settings")
                problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodySmall) }
                QuietButton("Choose a file", Modifier.fillMaxWidth(), PassIcons.Folder) { pick.launch(arrayOf("*/*")) }
                PrimaryButton("Import", Modifier.fillMaxWidth(), enabled = text.text.isNotBlank()) { take(text.text.toString()) }
            }
        }
    }
    if (resetting) {
        ConfirmSheet("Reset all customisation?", "Style, icons, layout and gestures go back to how they started.", "Reset", onDismiss = { resetting = false }) {
            prefs.resetCustomisations()
            result = "Everything is back to how it started."
        }
    }
}

// ---- search ----

private class Found(val title: String, val where: String, val route: String, val words: String = "")

private val SearchIndex = listOf(
    Found("Style", "Appearance", "settings/look", "standard glass interface"),
    Found("Theme", "Appearance", "settings/look", "dracula catppuccin nord solarized gruvbox colours colors dark"),
    Found("Dark mode", "Appearance", "settings/look", "light black amoled system"),
    Found("Animations", "Appearance", "settings/look", "motion reduce"),
    Found("Vibration", "Appearance", "settings/look", "haptics feedback"),
    Found("App icons", "Icons", "settings/icons", "material symbols rounded sharp light glyphs"),
    Found("Site icons", "Icons", "settings/icons", "brand logo simple icons favicon letters"),
    Found("Shape", "Icons", "settings/icons", "circle square rounded"),
    Found("Settings icons", "Icons", "settings/icons", "tile coloured background"),
    Found("Cards", "Icons", "settings/icons", "settings groups"),
    Found("Layout", "Vault list", "settings/list", "grid compact list density"),
    Found("Grouping", "Vault list", "settings/list", "folder site"),
    Found("Sort by", "Vault list", "settings/list", "order name changed type"),
    Found("Pinned items first", "Vault list", "settings/list"),
    Found("Fold logins on one site", "Vault list", "settings/list", "multiple accounts"),
    Found("Section headings", "Vault list", "settings/list"),
    Found("Filter chips", "Vault list", "settings/list"),
    Found("Search at the bottom", "Vault list", "settings/list", "thumb reach"),
    Found("Second line", "Vault list", "settings/list", "username subtitle"),
    Found("Small marks", "Vault list", "settings/list", "badges star"),
    Found("Button on the right", "Vault list", "settings/list", "copy row"),
    Found("Tap", "Vault list", "settings/list", "gesture click"),
    Found("Swipe right", "Vault list", "settings/list", "gesture"),
    Found("Swipe left", "Vault list", "settings/list", "gesture"),
    Found("Above the list", "Vault list", "settings/list", "dashboard widgets favourites recent"),
    Found("Navigation bar", "Navigation", "settings/navigation", "tabs dock order"),
    Found("Labels", "Navigation", "settings/navigation", "tab text"),
    Found("Open on", "Navigation", "settings/navigation", "start page last"),
    Found("Add button", "Navigation", "settings/navigation", "fab"),
    Found("Add button opens", "Navigation", "settings/navigation", "fab new login"),
    Found("Lock button", "Navigation", "settings/navigation", "padlock"),
    Found("Left-handed", "Navigation", "settings/navigation"),
    Found("Add menu", "Add menu", "settings/add", "types kinds new item order hide"),
    Found("Sections open at the start", "Editor", "settings/editor", "fold collapse minimise"),
    Found("Remember what I open", "Editor", "settings/editor"),
    Found("Fold less used fields", "Editor", "settings/editor", "details"),
    Found("Start logins with a password", "Editor", "settings/editor", "generate"),
    Found("Tap a field to copy it", "Item page", "settings/entry"),
    Found("Hide shown secrets after", "Item page", "settings/entry", "reveal timeout"),
    Found("Colour digits and symbols", "Item page", "settings/entry", "password colors"),
    Found("Old passwords to keep", "Item page", "settings/entry", "history"),
    Found("Size", "Codes", "settings/codes", "totp digits big"),
    Found("Split in two groups", "Codes", "settings/codes", "totp"),
    Found("Show the next code", "Codes", "settings/codes", "totp"),
    Found("Time ring", "Codes", "settings/codes", "totp timer"),
    Found("Hide codes until tapped", "Codes", "settings/codes", "privacy totp"),
    Found("Type", "Generator", "settings/generator", "password words pin passphrase default"),
    Found("Length", "Generator", "settings/generator", "default"),
    Found("Avoid look-alikes", "Generator", "settings/generator", "ambiguous"),
    Found("Show strength in bits", "Generator", "settings/generator", "entropy"),
    Found("Problems to list", "Health checks", "settings/health", "weak reused breach second factor duplicates"),
    Found("Show the score", "Health checks", "settings/health"),
    Found("Lock after leaving the app", "Security", "settings/security", "auto-lock timeout"),
    Found("Lock when the screen goes off", "Security", "settings/security"),
    Found("Unlock with fingerprint", "Security", "settings/security", "biometric face"),
    Found("Block screenshots", "Security", "settings/security", "flag secure recents"),
    Found("Clear the clipboard after", "Security", "settings/security", "copy timeout"),
    Found("Change it", "Security", "settings/security", "master password"),
    Found("Add a security key", "Security", "settings/security", "yubikey fido hardware"),
    Found("Wipe password", "Security", "settings/security", "duress panic"),
    Found("Decoy vault", "Security", "settings/security", "duress hidden"),
    Found("Emergency kit", "Security", "settings/security", "shamir trusted"),
    Found("Autofill service", "Security", "settings/security"),
    Found("Inline suggestions", "Security", "settings/security", "autofill keyboard chips"),
    Found("Offer to save passwords", "Security", "settings/security", "autofill"),
    Found("Backups", "Backups", "settings/backup", "webdav s3 sftp restore schedule"),
    Found("Sync and accounts", "Sync", "settings/sync", "bitwarden keepass lan server"),
    Found("Keyboard shortcuts", "Shortcuts", "settings/shortcuts", "hotkey chord"),
    Found("Settings file", "Settings file", "settings/file", "export import reset backup"),
    Found("Email aliases", "Tools", "settings/aliases", "simplelogin addy relay duckduckgo"),
    Found("Breaches", "Tools", "settings/breaches", "hibp leak"),
    Found("Import", "Your vault", "settings/import", "1password bitwarden chrome keepass csv"),
    Found("Export", "Your vault", "settings/export", "csv json kdbx"),
    Found("Keep deleted items for", "Trash", "settings/trash", "retention"),
)

@Composable
internal fun SettingsSearchScreen() {
    val shell = LocalShell.current
    val colors = palette
    val field = rememberTextFieldState()
    val query = field.text.toString().trim().lowercase()
    val found = if (query.isEmpty()) {
        emptyList()
    } else {
        val words = query.split(' ').filter { it.isNotEmpty() }
        SearchIndex.filter { item ->
            val text = "${item.title} ${item.where} ${item.words}".lowercase()
            words.all { it in text }
        }.sortedBy { if (it.title.lowercase().startsWith(query)) 0 else if (query in it.title.lowercase()) 1 else 2 }
    }
    WmScreen("Search settings", onBack = back()) {
        Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) { SheetField(field, "Search settings") }
        if (query.isNotEmpty() && found.isEmpty()) {
            Text("No settings match.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(16.dp))
        }
        found.forEach { item ->
            Column(
                Modifier
                    .fillMaxWidth()
                    .clickable {
                        SettingsHighlight.requestTitle(item.title)
                        shell.nav.navigate(item.route)
                    }
                    .padding(horizontal = 16.dp, vertical = 12.dp),
            ) {
                Text(item.title, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                Text(item.where, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            }
        }
    }
}

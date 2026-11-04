package com.wasimaster.wmpass.ui.settings

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.animateContentSize
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
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.help.Guide
import com.wasimaster.wmpass.ui.help.Help
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.LocalSiteIcons
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.SiteIconLook
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.parts.kindIcon
import com.wasimaster.wmpass.ui.parts.kindName
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.Springs
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.AllTabs
import com.wasimaster.wmpass.ui.vault.CardReaderTradeOffs
import com.wasimaster.wmpass.ui.vault.CardReaders
import com.wasimaster.wmpass.ui.vault.ConfirmSheet
import com.wasimaster.wmpass.ui.vault.KindOrder
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmpass.ui.vault.TradeOff
import com.wasimaster.wmsuite.settings.AdvancedFolds
import com.wasimaster.wmsuite.settings.ChoiceDetail
import com.wasimaster.wmsuite.settings.ChoiceSetting
import com.wasimaster.wmsuite.settings.LocalAdvancedFolds
import com.wasimaster.wmsuite.settings.LocalIconReveal
import com.wasimaster.wmsuite.settings.LocalInfoButtons
import com.wasimaster.wmsuite.settings.LocalInfoIcon
import com.wasimaster.wmsuite.settings.LocalInfoOpener
import com.wasimaster.wmsuite.settings.LocalScreenExtras
import com.wasimaster.wmsuite.settings.LocalScreenRoute
import com.wasimaster.wmsuite.settings.NavRow
import com.wasimaster.wmsuite.settings.ReorderableColumn
import com.wasimaster.wmsuite.settings.ScreenExtras
import com.wasimaster.wmsuite.settings.SettingsGroup
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
    "settings/tools" to Color(0xFF8E24AA),
    "settings/appearance" to Color(0xFFD81B60),
    "settings/editing" to Color(0xFF7CB342),
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
    "settings/guide" to Color(0xFF039BE5),
)

/** A settings page's own colour, for drawings of settings rows elsewhere, such as the guide's. */
internal fun routeColour(route: String): Color? = RouteColours[route]

/** What the settings kit asks of the app it is in. */
@Composable
fun ProvideSettingsKit(prefs: Prefs, content: @Composable () -> Unit) {
    SettingsTables.routeAccent = { route -> if (prefs.settingsIcons.value == "coloured") RouteColours[route] else null }
    Help.attach(LocalContext.current)
    val folds = remember(prefs) {
        object : AdvancedFolds {
            override val toggle: (String, Boolean) -> Unit = { id, open -> prefs.setFoldOpen(id, open) }

            @Composable
            override fun isOpen(id: String): Boolean = id in prefs.foldsOpen
        }
    }
    val reduce = LocalReduceMotion.current
    val reveal = rememberIconReveal(prefs.settingsIcons.value != "off", reduce)
    val shell = LocalShell.current
    // A row's "?" opens the guide's page for that row, found by its title on the page it is on.
    val opener: (String, String) -> Unit = remember(shell) {
        { title, detail -> Guide.openRow(shell.nav.currentBackStackEntry?.destination?.route, title, detail) }
    }
    CompositionLocalProvider(
        LocalAdvancedFolds provides folds,
        LocalIconReveal provides reveal,
        // Settings motion follows Animations like the rest of the app, not only the system's scale.
        com.wasimaster.wmsuite.settings.LocalReduceMotion provides reduce,
        LocalInfoOpener provides opener,
        LocalInfoIcon provides PassIcons.Help,
        LocalInfoButtons provides (prefs.helpButtons.value != "off"),
        LocalScreenExtras provides GuideExtras,
        content = content,
    )
}

/** What every settings page gets from the app: a "?" in its bar, opening the guide's page about it. */
private object GuideExtras : ScreenExtras {
    @Composable
    override fun fabs(route: String?, hasDock: Boolean, hasOwnFab: Boolean): List<@Composable () -> Unit> = emptyList()

    @Composable
    override fun bottomBar(route: String?, hasDock: Boolean): (@Composable () -> Unit)? = null

    @Composable
    override fun barAction(title: String) {
        val prefs = Prefs.of(LocalContext.current)
        val topic = Help.forRoute(LocalScreenRoute.current) ?: return
        if (prefs.helpButtons.value == "off") return
        // The kit's bar is Material's in either style, so this is a plain bar button in both.
        androidx.compose.material3.IconButton(onClick = { Guide.open(topic.id) }) {
            Icon(PassIcons.Help, "About $title", tint = MaterialTheme.colorScheme.onSurfaceVariant)
        }
    }
}

@Composable
private fun SwitchRow(title: String, subtitle: String?, opt: Prefs.Opt<Boolean>, icon: ImageVector) {
    ToggleSetting(title, subtitle, opt.value, info = Help.forPref(opt.key)?.plain, icon = icon) { opt.set(it) }
}

@Composable
private fun ChoiceRow(title: String, opt: Prefs.Opt<String>, icon: ImageVector, subtitle: String? = null, options: List<Pair<String, String>>) {
    ChoiceSetting(
        title,
        subtitle = subtitle,
        info = Help.forPref(opt.key)?.plain,
        options = options,
        selected = opt.value,
        icon = icon,
        detail = choiceDetail(opt.key),
    ) { opt.set(it) }
}

/** Each option's line in a choice sheet, from what the guide says that choice does. */
internal fun <T> choiceDetail(key: String): @Composable (T) -> ChoiceDetail? = { value -> Help.effectDetail(key, value)?.let { ChoiceDetail(description = it) } }

/** The guide's words for a setting, for its row's "?". */
internal fun infoFor(key: String): String? = Help.forPref(key)?.plain

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
    Triple("phosphor", "Phosphor", "Even lines and friendly, rounded shapes"),
    Triple("phosphor_bold", "Phosphor Bold", "The same shapes in heavier lines"),
    Triple("phosphor_light", "Phosphor Light", "The same shapes in fine lines"),
    Triple("lucide", "Lucide", "Plain line drawings, as on many websites"),
)

/** Who drew the app's symbols in a style, for the icon picker. */
internal fun iconCredit(style: String): String = when {
    style.startsWith("phosphor") -> "Phosphor Icons, MIT License"
    style == "lucide" -> "Lucide, ISC License"
    else -> "Material Symbols by Google, Apache License 2.0"
}

private val SiteStyles = listOf(
    Triple("brand", "Brand colors", "Each site's mark on its own color"),
    Triple("tinted", "Colored marks", "Each mark in its color, on one background"),
    Triple("uniform", "One color", "Every mark the same, on one background"),
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
    val context = androidx.compose.ui.platform.LocalContext.current
    // Glass keeps its own layout for these, so the rows only show where they do something.
    val flat = palette.flat
    val colors = palette
    WmScreen("Icons", route = "settings/icons", onBack = back()) {
        SettingsGroup("App icons", info = infoFor("icon_style")) {
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
        SettingsGroup("Site icons", info = infoFor("site_icon_style")) {
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
        SettingsGroup("Pictures, in order", info = infoFor("icon_sources"), modifier = Modifier.animateContentSize(Springs.glide())) {
            val names = mapOf("brand" to "Brand marks", "app" to "App icons", "site" to "Website icons")
            val about = mapOf(
                "brand" to "Marks for well-known sites, built into the app",
                "app" to "The icon of the app a login is for, when it is installed",
                "site" to "Fetched once from each site or DuckDuckGo. Needs network features",
            )
            item {
                ReorderableColumn(
                    // The rows run to their edges; the card around them wants the usual inset.
                    modifier = Modifier.padding(horizontal = 16.dp),
                    items = prefs.iconSources,
                    label = { names[it] ?: it },
                    onReorder = { prefs.updateIconSources(it) },
                    onDelete = { source ->
                        prefs.updateIconSources(prefs.iconSources - source)
                        if (source == "site") com.wasimaster.wmpass.system.ItemIcons.forgetSites(context)
                    },
                    keepLast = false,
                )
            }
            com.wasimaster.wmpass.app.IconSources.filter { it !in prefs.iconSources }.forEach { source ->
                item {
                    NavRow(
                        "Add ${names[source]!!.lowercase()}",
                        about[source],
                        icon = when (source) {
                            "app" -> PassIcons.Smartphone
                            "site" -> PassIcons.Language
                            else -> PassIcons.Star
                        },
                    ) { prefs.updateIconSources(prefs.iconSources + source) }
                }
            }
            if ("site" in prefs.iconSources) item {
                ChoiceRow("Website icons from", prefs.siteIconFrom, PassIcons.Public, options = listOf("site" to "Each site", "duckduckgo" to "DuckDuckGo"))
            }
        }
        SettingsGroup("Settings icons") {
            item {
                ChoiceRow(
                    "Style",
                    prefs.settingsIcons,
                    PassIcons.Palette,
                    options = listOf("coloured" to "Colored", "mono" to "One color", "bare" to "No background", "off" to "None"),
                )
            }
            if (flat) item {
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
    // Glass keeps its own layout for these, so the rows only show where they do something.
    val flat = palette.flat
    WmScreen("Vault list", route = "settings/list", onBack = back()) {
        SettingsGroup("Layout") {
            item {
                ChoiceSetting(
                    "Layout",
                    info = infoFor("layout"),
                    options = listOf("list" to "List", "compact" to "Compact", "grid" to "Grid"),
                    selected = prefs.layout,
                    icon = PassIcons.ViewAgenda,
                    detail = choiceDetail("layout"),
                ) { prefs.updateLayout(it) }
            }
            item {
                ChoiceSetting(
                    "Grouping",
                    info = infoFor("grouping"),
                    options = listOf("types" to "Types, then folders", "smart" to "By site", "folder" to "By folder"),
                    selected = prefs.grouping,
                    icon = PassIcons.Layers,
                    detail = choiceDetail("grouping"),
                ) { prefs.updateGrouping(it) }
            }
            item {
                ChoiceRow("Sort by", prefs.sort, PassIcons.Sort, options = listOf("name" to "Name", "changed" to "Last changed", "kind" to "Type", "site" to "Website"))
            }
            item { SwitchRow("Pinned items first", "Keep pinned items above the rest.", prefs.pinnedOnTop, PassIcons.Keep) }
            item { SwitchRow("Fold logins on one site", "Show several logins for a site as one row.", prefs.groupSites, PassIcons.Layers) }
            item { SwitchRow("Section headings", null, prefs.listHeadings, PassIcons.FormatListBulleted) }
            item { SwitchRow("Filter chips", "All, Favorites and the types you have.", prefs.filterChips, PassIcons.FilterList) }
            if (flat) item { SwitchRow("Search at the bottom", "Within reach of your thumb.", prefs.searchAtBottom, PassIcons.Search) }
        }
        SettingsGroup("Rows") {
            item { SwitchRow("Icons", null, prefs.rowIcons, PassIcons.Image) }
            item { SwitchRow("Second line", "The username or other detail under the name.", prefs.rowSubtitle, PassIcons.TextFields) }
            item { SwitchRow("Small marks", "Favorite, one-time code, passkey and attachment.", prefs.rowBadges, PassIcons.Star) }
            item {
                ChoiceRow("Button on the right", prefs.rowButton, PassIcons.ContentCopy, options = RowActions + ("none" to "None"))
            }
        }
        SettingsGroup("Gestures") {
            item {
                ChoiceRow("Tap", prefs.tapAction, PassIcons.TouchApp, options = listOf("open" to "Open") + RowActions)
            }
            item {
                ChoiceRow("Swipe right", prefs.swipeRight, PassIcons.Swipe, options = RowActions + listOf("favourite" to "Favorite", "none" to "Nothing"))
            }
            item {
                ChoiceRow("Swipe left", prefs.swipeLeft, PassIcons.Swipe, options = listOf("trash" to "Move to trash", "favourite" to "Favorite", "password" to "Copy password", "none" to "Nothing"))
            }
        }
        SettingsGroup("Above the list", info = infoFor("widgets"), modifier = Modifier.animateContentSize(Springs.glide())) {
            val all = listOf("favourites" to "Favorites", "recent" to "Recently changed", "health" to "Health", "codes" to "Codes")
            val names = all.toMap()
            item {
                ReorderableColumn(
                    // The rows run to their edges; the card around them wants the usual inset.
                    modifier = Modifier.padding(horizontal = 16.dp),
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
    // Glass keeps its own layout for these, so the rows only show where they do something.
    val flat = palette.flat
    WmScreen("Navigation", route = "settings/navigation", onBack = back()) {
        SettingsGroup("Navigation bar", info = infoFor("tabs"), modifier = Modifier.animateContentSize(Springs.glide())) {
            val names = AllTabs.associate { it.id to it.label }
            item {
                ReorderableColumn(
                    // The rows run to their edges; the card around them wants the usual inset.
                    modifier = Modifier.padding(horizontal = 16.dp),
                    items = prefs.tabs,
                    label = { names[it] ?: it },
                    onReorder = { prefs.updateTabs(it) },
                    onDelete = { tab -> if (tab != "vault") prefs.updateTabs(prefs.tabs - tab) },
                )
            }
            AllTabs.filter { it.id !in prefs.tabs }.forEach { tab ->
                item { NavRow("Add ${tab.label}", icon = tab.icon) { prefs.updateTabs(prefs.tabs + tab.id) } }
            }
            if (flat) item {
                ChoiceRow("Labels", prefs.navLabels, PassIcons.TextFields, options = listOf("all" to "All", "selected" to "Selected", "none" to "None"))
            }
            item {
                ChoiceRow("Open on", prefs.startTab, PassIcons.History, options = listOf("first" to "First page", "last" to "Last page used"))
            }
        }
        if (flat) SettingsGroup("Buttons") {
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
        // Only Glass puts search and add beside the tabs, so the other style has nothing to move.
        if (!flat) SettingsGroup("Hand") {
            item {
                ToggleSetting("Left-handed", "Move search and add to the other side.", prefs.leftHanded, info = infoFor("left_handed"), icon = PassIcons.TouchApp) { prefs.updateLeftHanded(it) }
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
        SettingsGroup("Shown", info = infoFor("add_kinds"), modifier = Modifier.animateContentSize(Springs.glide())) {
            item {
                ReorderableColumn(
                    // The rows run to their edges; the card around them wants the usual inset.
                    modifier = Modifier.padding(horizontal = 16.dp),
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
            item { NavRow("Show all in the usual order", icon = PassIcons.Refresh, info = Help.info("action.add_show_all")) { prefs.updateAddKinds(Prefs.DEFAULT_ADD) } }
        }
    }
}

// ---- editor ----

@Composable
internal fun EditorSettingsScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    WmScreen("Editor", route = "settings/editor", onBack = back()) {
        SettingsGroup("Shown and open at the start", info = infoFor("editor_open")) {
            listOf(
                Triple("sites", "Websites and apps", PassIcons.Language),
                Triple("otp", "One-time code", PassIcons.Pin),
                Triple("notes", "Notes", PassIcons.StickyNote2),
                Triple("custom", "Custom fields", PassIcons.Tag),
                Triple("questions", "Security questions", PassIcons.Help),
                Triple("organise", "Organize", PassIcons.Folder),
                Triple("details", "Other details", PassIcons.Tune),
            ).forEach { (key, name, icon) ->
                item { ToggleSetting(name, null, key in prefs.editorOpen, icon = icon) { prefs.setEditorOpen(key, it) } }
            }
        }
        SettingsGroup("Behavior") {
            item { SwitchRow("Remember what I open", "A section you open or close stays that way next time.", prefs.editorRemember, PassIcons.History) }
            item { SwitchRow("Fold less used fields", "Such as a card's PIN or a database's connection string.", prefs.editorFoldDetails, PassIcons.ExpandMore) }
            item { SwitchRow("Start logins with a password", "Fill in a generated password on every new login.", prefs.editorGenerate, PassIcons.Casino) }
            item { SwitchRow("Tag new items", "A new item without tags gets the ones its site suggests, such as Banking or Social.", prefs.autoTag, PassIcons.Sell) }
        }
        // Only builds with Google's SDKs have a second card reader to choose.
        if (com.wasimaster.wmpass.BuildConfig.GOOGLE_SDKS) {
            SettingsGroup("Card scanner") {
                item {
                    ChoiceRow(
                        "Reader",
                        prefs.cardReader,
                        PassIcons.PhotoCamera,
                        subtitle = "Both read the card on this phone.",
                        options = listOf("" to "Ask next time", CardReaders.Google to "Google (ML Kit)", CardReaders.Own to "WM Pass"),
                    )
                }
                item {
                    Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) {
                        Text("Google (ML Kit)", style = MaterialTheme.typography.titleSmall, color = palette.text)
                        CardReaderTradeOffs.google.forEach { (good, text) -> TradeOff(if (good) PassIcons.Check else PassIcons.Info, if (good) palette.primary else palette.warn, text) }
                        Text("WM Pass", style = MaterialTheme.typography.titleSmall, color = palette.text, modifier = Modifier.padding(top = 14.dp))
                        CardReaderTradeOffs.own.forEach { (good, text) -> TradeOff(if (good) PassIcons.Check else PassIcons.Info, if (good) palette.primary else palette.warn, text) }
                    }
                }
            }
        }
    }
}

// ---- entry page ----

@Composable
internal fun EntrySettingsScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    // Glass keeps its own layout for these, so the rows only show where they do something.
    val flat = palette.flat
    var limit by remember { mutableStateOf<Int?>(null) }
    androidx.compose.runtime.LaunchedEffect(Unit) { limit = runCatching { Pass.info().historyLimit }.getOrNull() }
    WmScreen("Item page", route = "settings/entry", onBack = back()) {
        if (flat) SettingsGroup("Buttons at the top") {
            item { SwitchRow("Show them", null, prefs.entryActions, PassIcons.TouchApp) }
            item { SwitchRow("Copy username", null, prefs.entryActionUsername, PassIcons.Person) }
            item { SwitchRow("Open site", null, prefs.entryActionOpen, PassIcons.OpenInNew) }
        }
        SettingsGroup("Fields") {
            item { SwitchRow("Tap a field to copy it", null, prefs.tapCopies, PassIcons.ContentCopy) }
            item {
                ChoiceSetting(
                    "Hide shown secrets after",
                    info = infoFor("reveal_seconds"),
                    options = listOf(15 to "15 s", 45 to "45 s", 120 to "2 min", 0 to "Leaving"),
                    selected = prefs.revealSeconds.value,
                    icon = PassIcons.Timer,
                    detail = choiceDetail("reveal_seconds"),
                ) { prefs.revealSeconds.set(it) }
            }
            item { SwitchRow("Color digits and symbols", "Makes a password easier to read aloud or retype.", prefs.colourSecrets, PassIcons.Palette) }
        }
        SettingsGroup("History") {
            limit?.let { current ->
                item {
                    ChoiceSetting(
                        "Old passwords to keep",
                        info = Help.info("action.history_keep"),
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
                    info = infoFor("gen_length"),
                    options = listOf(12, 16, 20, 24, 32, 48, 64).map { it to it.toString() },
                    selected = prefs.genLength.value,
                    icon = PassIcons.Numbers,
                    detail = choiceDetail("gen_length"),
                ) { prefs.genLength.set(it) }
            }
            item {
                ChoiceSetting(
                    "Words",
                    info = infoFor("gen_words"),
                    options = listOf(3, 4, 5, 6, 7, 8).map { it to it.toString() },
                    selected = prefs.genWords.value,
                    icon = PassIcons.Abc,
                    detail = choiceDetail("gen_words"),
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
            item { SwitchRow("Leave out local addresses", "Logins only for your router, NAS or other devices at home.", prefs.healthSkipLocal, PassIcons.Router) }
        }
        SettingsGroup("Problems to list", info = Help.info("group.health.problems")) {
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
                item { ToggleSetting(name, null, key !in prefs.healthHidden, info = Help.info("action.health_check_$key"), icon = icon) { prefs.setHealthShown(key, it) } }
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
            item { NavRow("Save to a file", icon = PassIcons.Download, info = Help.info("action.settings_save_file")) { save.launch("wm-pass-settings.json") } }
            item {
                NavRow("Send to an app", icon = PassIcons.Share, info = Help.info("action.settings_copy_text")) {
                    val intent = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, prefs.exportText())
                    context.startActivity(Intent.createChooser(intent, "Share settings"))
                }
            }
            item { NavRow("Import", icon = PassIcons.Upload, info = Help.info("action.settings_import")) { importing = true } }
            item { NavRow("Reset all customization", "Keeps your own themes and protection settings", icon = PassIcons.Refresh, info = Help.info("action.settings_reset")) { resetting = true } }
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
        ConfirmSheet("Reset all customization?", "Style, icons, layout and gestures go back to how they started.", "Reset", onDismiss = { resetting = false }) {
            prefs.resetCustomisations()
            result = "Everything is back to how it started."
        }
    }
}

// ---- search ----

internal class Found(val title: String, val where: String, val route: String, val words: String = "")

internal val SearchIndex = listOf(
    Found("Style", "Appearance › Style and theme", "settings/look", "standard glass interface"),
    Found("Theme", "Appearance › Style and theme", "settings/look", "dracula catppuccin nord solarized gruvbox colours colors dark colors colors"),
    Found("Dark mode", "Appearance › Style and theme", "settings/look", "light black amoled system"),
    Found("Animations", "Appearance › Style and theme", "settings/look", "motion reduce"),
    Found("Vibration", "Appearance › Style and theme", "settings/look", "haptics feedback"),
    Found("App icons", "Appearance › Icons", "settings/icons", "material symbols rounded sharp light phosphor lucide glyphs"),
    Found("Pictures, in order", "Appearance › Icons", "settings/icons", "favicon app icon website logo fallback"),
    Found("Website icons from", "Appearance › Icons", "settings/icons", "favicon duckduckgo"),
    Found("Leave out local addresses", "Health checks", "settings/health", "lan router nas intranet private ip firewall"),
    Found("Site icons", "Appearance › Icons", "settings/icons", "brand logo simple icons favicon letters"),
    Found("Shape", "Appearance › Icons", "settings/icons", "circle square rounded"),
    Found("Settings icons", "Appearance › Icons", "settings/icons", "tile coloured background colored colored"),
    Found("Cards", "Appearance › Icons", "settings/icons", "settings groups"),
    Found("Layout", "Appearance › Vault list", "settings/list", "grid compact list density"),
    Found("Grouping", "Appearance › Vault list", "settings/list", "folder site type types bitwarden"),
    Found("Sort by", "Appearance › Vault list", "settings/list", "order name changed type"),
    Found("Pinned items first", "Appearance › Vault list", "settings/list"),
    Found("Fold logins on one site", "Appearance › Vault list", "settings/list", "multiple accounts"),
    Found("Section headings", "Appearance › Vault list", "settings/list"),
    Found("Filter chips", "Appearance › Vault list", "settings/list"),
    Found("Search at the bottom", "Appearance › Vault list", "settings/list", "thumb reach"),
    Found("Second line", "Appearance › Vault list", "settings/list", "username subtitle"),
    Found("Small marks", "Appearance › Vault list", "settings/list", "badges star"),
    Found("Button on the right", "Appearance › Vault list", "settings/list", "copy row"),
    Found("Tap", "Appearance › Vault list", "settings/list", "gesture click"),
    Found("Swipe right", "Appearance › Vault list", "settings/list", "gesture"),
    Found("Swipe left", "Appearance › Vault list", "settings/list", "gesture"),
    Found("Above the list", "Appearance › Vault list", "settings/list", "dashboard widgets favourites recent favorites favorites"),
    Found("Navigation bar", "Appearance › Navigation", "settings/navigation", "tabs dock order"),
    Found("Labels", "Appearance › Navigation", "settings/navigation", "tab text"),
    Found("Open on", "Appearance › Navigation", "settings/navigation", "start page last"),
    Found("Add button", "Appearance › Navigation", "settings/navigation", "fab"),
    Found("Add button opens", "Appearance › Navigation", "settings/navigation", "fab new login"),
    Found("Lock button", "Appearance › Navigation", "settings/navigation", "padlock"),
    Found("Left-handed", "Appearance › Navigation", "settings/navigation"),
    Found("Add menu", "Adding and editing › Add menu", "settings/add", "types kinds new item order hide"),
    Found("Shown and open at the start", "Adding and editing › Editor", "settings/editor", "fold collapse minimise minimize minimize"),
    Found("Remember what I open", "Adding and editing › Editor", "settings/editor"),
    Found("Fold less used fields", "Adding and editing › Editor", "settings/editor", "details"),
    Found("Start logins with a password", "Adding and editing › Editor", "settings/editor", "generate"),
    Found("Tag new items", "Adding and editing › Editor", "settings/editor", "auto tag category"),
    Found("Card scanner", "Adding and editing › Editor", "settings/editor", "reader camera scan credit debit ml kit google"),
    Found("Tap a field to copy it", "Appearance › Item page", "settings/entry"),
    Found("Hide shown secrets after", "Appearance › Item page", "settings/entry", "reveal timeout"),
    Found("Color digits and symbols", "Appearance › Item page", "settings/entry", "password colors"),
    Found("Old passwords to keep", "Appearance › Item page", "settings/entry", "history"),
    Found("Size", "Appearance › Codes", "settings/codes", "totp digits big"),
    Found("Split in two groups", "Appearance › Codes", "settings/codes", "totp"),
    Found("Show the next code", "Appearance › Codes", "settings/codes", "totp"),
    Found("Time ring", "Appearance › Codes", "settings/codes", "totp timer"),
    Found("Hide codes until tapped", "Appearance › Codes", "settings/codes", "privacy totp"),
    Found("Type", "Adding and editing › Generator", "settings/generator", "password words pin passphrase default"),
    Found("Length", "Adding and editing › Generator", "settings/generator", "default"),
    Found("Avoid look-alikes", "Adding and editing › Generator", "settings/generator", "ambiguous"),
    Found("Show strength in bits", "Adding and editing › Generator", "settings/generator", "entropy"),
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
    Found("Email aliases", "Tools › Email aliases", "settings/aliases", "simplelogin addy relay duckduckgo"),
    Found("Breaches", "Breaches", "settings/breaches", "hibp leak"),
    Found("Import", "Your vault", "settings/import", "1password bitwarden chrome keepass csv"),
    Found("Export", "Your vault", "settings/export", "csv json kdbx"),
    Found("Keep deleted items for", "Trash", "settings/trash", "retention"),
    Found("Appearance", "Settings", "settings/appearance", "look theme icons list customise customize customize"),
    Found("Disposable inbox", "Tools › Disposable inbox", "settings/inbox", "throwaway email temporary"),
    Found("Username checker", "Tools › Username checker", "settings/usernames", "handle available"),
    Found("Compare two passwords", "Tools › Compare two passwords", "settings/compare", "same match"),
    Found("Guide", "Settings", "settings/guide", "help manual tour tutorial explain question what is this"),
    Found("Help buttons", "Guide", "settings/guide", "question mark explain"),
    Found("Tips on new pages", "Guide", "settings/guide", "hints first time"),
)

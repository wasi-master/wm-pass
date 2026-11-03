package com.wasimaster.wmpass.ui.help

import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.remember
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmsuite.settings.ChoiceSetting
import com.wasimaster.wmsuite.settings.ChoiceDetail
import com.wasimaster.wmsuite.settings.NavRow
import com.wasimaster.wmsuite.settings.SettingsGroup
import com.wasimaster.wmsuite.settings.ToggleSetting
import com.wasimaster.wmsuite.settings.WmScreen

/** The settings pages, grouped as the settings root groups them, for the guide's own list. */
private val Sections = listOf(
    "Protection" to listOf("security", "hardware", "shelf", "breaches", "health"),
    "Your vault" to listOf("folders", "trash", "import", "export"),
    "Backup and sync" to listOf("backup", "sync", "vaults", "receive"),
    "Tools" to listOf("tools", "aliases", "inbox", "usernames", "compare"),
    "Appearance" to listOf("appearance", "look", "themes", "icons", "list", "navigation", "entry", "codes"),
    "Adding and editing" to listOf("editing", "add", "editor", "generator", "shortcuts"),
    "This app" to listOf("file", "about", "guide"),
)

/**
 * Settings, Guide: every topic in one place. A search first, then the welcome and the tours, the
 * screens, how things work, each settings page with what is on it, and the guide's own two
 * settings at the end.
 */
@Composable
fun GuideScreen() {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val colors = palette
    val field = rememberTextFieldState()
    val query = field.text.toString()
    val open = remember { mutableStateListOf<String>() }
    WmScreen("Guide", route = "settings/guide", onBack = { shell.nav.popBackStack() }) {
        Text(
            "What every part of WM Pass does, and why. The same pages open from the ? beside a setting or a page title.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(horizontal = 32.dp, vertical = 4.dp),
        )
        Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) { SheetField(field, "Search the guide") }
        if (query.isNotBlank()) {
            val found = Help.search(query)
            SettingsGroup(if (found.isEmpty()) "Nothing found" else if (found.size == 1) "1 found" else "${found.size} found") {
                found.forEach { topic -> item(topic.id) { NavRow(topic.title, Help.where(topic)) { Guide.open(topic.id) } } }
            }
            return@WmScreen
        }
        val topics = Help.topics
        SettingsGroup("Start here") {
            item { NavRow("Show the welcome again", "Where the everyday things are.", icon = PassIcons.SentimentSatisfied) { Guide.welcome = true } }
            if (Guide.hasTour("tour.home")) {
                item {
                    val done = "tour.home" in prefs.helpToursDone
                    NavRow("Take the tour", if (done) "Done" else "The parts of the app, one at a time.", icon = if (done) PassIcons.CheckCircle else PassIcons.TouchApp) { Guide.startTour("tour.home") }
                }
            }
        }
        val tours = Help.file.tours.filter { it.id != "tour.home" }
        if (tours.isNotEmpty()) {
            SettingsGroup("Tours") {
                tours.forEach { tour ->
                    item(tour.id) {
                        val done = tour.id in prefs.helpToursDone
                        NavRow(
                            tour.title,
                            if (done) "Done" else if (tour.steps.size == 1) "1 step" else "${tour.steps.size} steps",
                            icon = if (done) PassIcons.CheckCircle else PassIcons.TouchApp,
                        ) { Guide.startTour(tour.id) }
                    }
                }
            }
        }
        val screens = topics.filter { it.kind == "screen" }
        if (screens.isNotEmpty()) {
            SettingsGroup("Screens") {
                screens.forEach { topic -> item(topic.id) { NavRow(topic.title) { Guide.open(topic.id) } } }
            }
        }
        val ideas = topics.filter { (it.kind == "feature" || it.kind == "concept") && it.page == null }
        if (ideas.isNotEmpty()) {
            SettingsGroup("How WM Pass works") {
                ideas.forEach { topic -> item(topic.id) { NavRow(topic.title) { Guide.open(topic.id) } } }
            }
        }
        for ((section, pages) in Sections) {
            val shown = pages.filter { page -> Help.topic("page.$page") != null || topics.any { it.page == page } }
            if (shown.isEmpty()) continue
            SettingsGroup("Settings, $section") {
                for (page in shown) {
                    val about = Help.topic("page.$page")
                    val rows = topics.filter { it.page == page && it.kind != "page" && it.kind != "screen" }
                    val expanded = page in open
                    item("page:$page") {
                        NavRow(
                            about?.title ?: page,
                            if (rows.size == 1) "1 topic" else "${rows.size} topics",
                            icon = if (expanded) PassIcons.ExpandMore else PassIcons.ChevronRight,
                        ) { if (expanded) open.remove(page) else open.add(page) }
                    }
                    if (about != null) {
                        item(visible = expanded, key = "about:$page") { NavRow("About the page", icon = PassIcons.Info) { Guide.open(about.id) } }
                    }
                    rows.forEach { topic ->
                        item(visible = expanded, key = topic.id) {
                            NavRow(topic.title, if (topic.kind == "action") "Button" else null) { Guide.open(topic.id) }
                        }
                    }
                }
            }
        }
        SettingsGroup("The guide itself") {
            item {
                ChoiceSetting(
                    "Help buttons",
                    info = Help.forPref("help_buttons")?.plain,
                    options = listOf("always" to "Always", "off" to "Off"),
                    selected = prefs.helpButtons.value,
                    icon = PassIcons.Help,
                    default = "always",
                    detail = { value -> Help.effectDetail("help_buttons", value)?.let { ChoiceDetail(description = it) } },
                ) { prefs.helpButtons.set(it) }
            }
            item {
                ToggleSetting(
                    "Tips on new pages",
                    "A line about a page the first time you open it.",
                    prefs.helpHints.value,
                    info = Help.forPref("help_hints")?.plain,
                    icon = PassIcons.Info,
                ) { prefs.helpHints.set(it) }
            }
        }
    }
}

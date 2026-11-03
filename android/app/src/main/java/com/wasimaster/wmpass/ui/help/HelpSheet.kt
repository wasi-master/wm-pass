package com.wasimaster.wmpass.ui.help

import android.content.Intent
import android.net.Uri
import androidx.activity.compose.BackHandler
import androidx.compose.animation.togetherWith
import androidx.compose.animation.using
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.selection.selectable
import androidx.compose.foundation.selection.selectableGroup
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.navigation.compose.currentBackStackEntryAsState
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.help.previews.HelpPreview
import com.wasimaster.wmpass.ui.help.previews.hasPreview
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.Shell
import com.wasimaster.wmsuite.settings.SettingsHighlight
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.intOrNull

/**
 * The guide's page about one thing, in the app's own sheet: what it is, why, how, when, what each
 * choice does, a picture or a live preview, and what it does not do. Links open the next topic in
 * the same sheet, with a way back. [shell] is null at the door, where there is nowhere to go yet,
 * so the sheet only explains.
 */
@Composable
fun HelpSheet(prefs: Prefs, shell: Shell?) {
    val id = Guide.current ?: return
    val scroll = androidx.compose.foundation.rememberScrollState()
    // Each topic starts at its top, whichever way it was reached.
    androidx.compose.runtime.LaunchedEffect(id) { scroll.scrollTo(0) }
    PassSheet({ Guide.close() }, scroll = scroll) {
        // Inside the sheet, so it is asked before the sheet's own back, which would close it.
        BackHandler(Guide.stack.size > 1) { Guide.back() }
        // A link slides the next topic in from the side, and Back slides it out again, while
        // the sheet eases to the new page's height.
        androidx.compose.animation.AnimatedContent(
            id to Guide.stack.size,
            transitionSpec = {
                val deeper = targetState.second >= initialState.second
                val reach = { width: Int -> width / 5 }
                (
                    androidx.compose.animation.slideInHorizontally(androidx.compose.animation.core.tween(260)) { if (deeper) reach(it) else -reach(it) } +
                        androidx.compose.animation.fadeIn(androidx.compose.animation.core.tween(200, 60))
                    ) togetherWith (
                    androidx.compose.animation.slideOutHorizontally(androidx.compose.animation.core.tween(200)) { if (deeper) -reach(it) else reach(it) } +
                        androidx.compose.animation.fadeOut(androidx.compose.animation.core.tween(120))
                    ) using androidx.compose.animation.SizeTransform(clip = false)
            },
            contentKey = { it.first },
            label = "topic",
        ) { (topic, _) ->
            androidx.compose.foundation.layout.Column { HelpSheetContent(topic, prefs, shell) }
        }
    }
}

/** What the sheet holds for [id], drawn into the column around it. */
@Composable
fun HelpSheetContent(id: String, prefs: Prefs, shell: Shell?) {
    val loose = Guide.looseText(id)
    val topic = Help.topic(id)
    when {
        topic != null -> TopicPage(topic, prefs, shell)
        loose != null -> LoosePage(loose.first, loose.second)
    }
}

@Composable
private fun Head(where: String?, title: String) {
    val colors = palette
    Row(Modifier.fillMaxWidth().padding(top = 2.dp, bottom = 10.dp), verticalAlignment = Alignment.Top) {
        if (Guide.stack.size > 1) {
            IconButton(onClick = { Guide.back() }, modifier = Modifier.padding(end = 4.dp)) { Icon(PassIcons.ArrowBack, "Back", tint = colors.text) }
        }
        Column(Modifier.weight(1f).padding(top = 6.dp)) {
            if (where != null) Text(where, color = colors.textMid, style = MaterialTheme.typography.labelMedium)
            Text(title, color = colors.text, style = MaterialTheme.typography.titleLarge)
        }
        IconButton(onClick = { Guide.close() }) { Icon(PassIcons.Close, "Close", tint = colors.textMid) }
    }
}

@Composable
private fun LoosePage(title: String, detail: String) {
    Head(null, title)
    Text(detail, color = palette.text, style = MaterialTheme.typography.bodyLarge)
    Spacer(Modifier.height(20.dp))
    PrimaryButton("Close", Modifier.fillMaxWidth()) { Guide.close() }
}

@Composable
private fun Section(title: String, content: @Composable () -> Unit) {
    Column(Modifier.padding(top = 18.dp)) {
        Text(title, color = palette.primary, style = MaterialTheme.typography.titleSmall, modifier = Modifier.padding(bottom = 6.dp))
        content()
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun TopicPage(topic: HelpTopic, prefs: Prefs, shell: Shell?) {
    val colors = palette
    val context = LocalContext.current
    // Only for the settings this phone has: the Now and Default marks, and Use this.
    val opt = topic.pref?.let { prefs.opt(it) }
    val now = opt?.value
    val usual = opt?.default
    var picked by remember(topic.id) { mutableStateOf<JsonPrimitive?>(null) }
    val preview = topic.preview.takeIf { hasPreview(it) }
    val showing = picked ?: topic.effects?.firstOrNull { now != null && Help.matches(it.value, now) }?.value
    val route = shell?.nav?.currentBackStackEntryAsState()?.value?.destination?.route

    Head(Help.where(topic), topic.title)
    RichText(topic.what, style = MaterialTheme.typography.bodyLarge, color = colors.text)
    if (topic.figure != null && preview == null) {
        Spacer(Modifier.height(16.dp))
        FigureView(topic.figure)
    }
    topic.why?.let { Section("Why it is there") { RichText(it) } }
    topic.how?.let { Section("How it works") { RichText(it) } }
    topic.whenTo?.let { Section("When to use it") { RichText(it) } }

    val effects = topic.effects.orEmpty()
    if (effects.isNotEmpty()) {
        Section("What each choice does") {
            if (preview != null) {
                HelpPreview(preview, topic.pref, showing, Modifier.padding(bottom = 10.dp))
            }
            Column(if (opt != null) Modifier.selectableGroup() else Modifier) {
                effects.forEach { effect ->
                    val current = now != null && Help.matches(effect.value, now)
                    val isDefault = usual != null && Help.matches(effect.value, usual)
                    val chosen = showing == effect.value
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .then(
                                if (opt != null) {
                                    Modifier.selectable(chosen, role = Role.RadioButton) { picked = effect.value }
                                } else {
                                    Modifier
                                },
                            )
                            .padding(vertical = 8.dp),
                        verticalAlignment = Alignment.Top,
                    ) {
                        // The setting as it is now is ticked; the one being previewed is in the accent.
                        Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
                            if (current) Icon(PassIcons.Check, "Now", tint = colors.primary, modifier = Modifier.size(20.dp))
                        }
                        Spacer(Modifier.width(10.dp))
                        Column(Modifier.weight(1f)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                Text(
                                    effect.label,
                                    color = if (chosen && opt != null && preview != null) colors.primary else colors.text,
                                    style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium),
                                )
                                if (isDefault) Text("  ·  Default", color = colors.textLow, style = MaterialTheme.typography.bodySmall)
                            }
                            RichText(effect.text)
                        }
                    }
                }
            }
            val choice = picked
            if (opt != null && choice != null && !(now != null && Help.matches(choice, now)) && canApply(topic.pref)) {
                Row(Modifier.fillMaxWidth().padding(top = 8.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    PrimaryButton("Use this", Modifier.weight(1f)) {
                        if (apply(prefs, topic.pref, choice)) picked = null
                    }
                    QuietButton("Keep it as it is", Modifier.weight(1f)) { picked = null }
                }
            }
        }
    } else if (preview != null) {
        Spacer(Modifier.height(16.dp))
        HelpPreview(preview, topic.pref, null)
    }
    if (topic.figure != null && preview != null) {
        Spacer(Modifier.height(16.dp))
        FigureView(topic.figure)
    }
    topic.caveats?.let { Section("Limits") { RichText(it) } }

    val related = topic.related.mapNotNull { Help.topic(it) }
    if (related.isNotEmpty()) {
        Section("Related") {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                related.forEach { other -> Pill(other.title, false) { Guide.open(other.id) } }
            }
        }
    }
    topic.docs?.takeIf { shell != null }?.let { path ->
        TextButton(
            onClick = { runCatching { context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse("https://wmpass.pages.dev$path")).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) } },
            modifier = Modifier.padding(top = 10.dp),
        ) { Text("More on the WM Pass website") }
    }

    // The way out: to the setting itself, through a tour, or simply closed.
    Spacer(Modifier.height(20.dp))
    val page = topic.page?.takeIf { shell != null && Help.hasPage(it) }
    val here = page != null && route == "settings/$page"
    val tour = topic.tour?.takeIf { shell != null && Guide.hasTour(it) }
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if (tour != null) {
            QuietButton("Show me", Modifier.fillMaxWidth(), PassIcons.TouchApp) { Guide.startTour(tour) }
        }
        if (page != null && !here && shell != null) {
            QuietButton(if (topic.kind == "page") "Open the page" else "Open the setting", Modifier.fillMaxWidth()) {
                Guide.close()
                if (topic.kind != "page") SettingsHighlight.requestTitle(topic.title)
                shell.nav.navigate("settings/$page") { launchSingleTop = true }
            }
        }
        PrimaryButton("Close", Modifier.fillMaxWidth()) { Guide.close() }
    }
}

/** Whether **Use this** can set [key] on its own. A panic wipe needs an app named first, on its page. */
private fun canApply(key: String?): Boolean = key != null && key != "panic_action" && key != "panic_app"

/** Sets [key] to [value] the way its own row would, side effects included. */
internal fun apply(prefs: Prefs, key: String?, value: JsonPrimitive): Boolean {
    key ?: return false
    when (key) {
        "icon_style" -> if (value.isString) prefs.updateIconStyle(value.content) else return false
        "clipboard_seconds" -> prefs.updateClipboard(value.intOrNull ?: return false)
        else -> {
            val opt = prefs.opt(key) ?: return false
            @Suppress("UNCHECKED_CAST")
            when (opt.default) {
                is Boolean -> (opt as Prefs.Opt<Boolean>).set(value.booleanOrNull ?: return false)
                is Int -> (opt as Prefs.Opt<Int>).set(value.intOrNull ?: return false)
                is String -> if (value.isString) (opt as Prefs.Opt<String>).set(value.content) else return false
                else -> return false
            }
        }
    }
    return true
}

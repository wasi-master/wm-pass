package com.wasimaster.wmpass.ui.settings

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextField
import androidx.compose.material3.TextFieldDefaults
import androidx.compose.material3.TopAppBar
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalSoftwareKeyboardController
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.unit.dp
import com.wasimaster.wmkeyboard.core.ui.RailBucket
import com.wasimaster.wmkeyboard.core.ui.ScrollRailBox
import com.wasimaster.wmkeyboard.core.ui.rememberScrollRailState
import com.wasimaster.wmpass.ui.help.Guide
import com.wasimaster.wmpass.ui.help.Help
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmsuite.settings.LocalIconReveal
import com.wasimaster.wmsuite.settings.SettingsHighlight
import com.wasimaster.wmsuite.settings.WmIconTile
import com.wasimaster.wmsuite.settings.WmRow
import com.wasimaster.wmsuite.settings.routeAccent

/** The glyph of the page a result opens: the same tile its row wears on the way there. */
private fun routeIcon(route: String): ImageVector = when (route) {
    "settings/about" -> PassIcons.Info
    "settings/add" -> PassIcons.Add
    "settings/aliases" -> PassIcons.AlternateEmail
    "settings/appearance", "settings/look" -> PassIcons.Palette
    "settings/backup" -> PassIcons.Backup
    "settings/breaches" -> PassIcons.LocalFireDepartment
    "settings/codes" -> PassIcons.Pin
    "settings/compare" -> PassIcons.Balance
    "settings/editing", "settings/editor" -> PassIcons.Edit
    "settings/tags" -> PassIcons.Sell
    "settings/entry" -> PassIcons.Description
    "settings/export" -> PassIcons.Upload
    "settings/file" -> PassIcons.ImportExport
    "settings/folders" -> PassIcons.Folder
    "settings/generator" -> PassIcons.Casino
    "settings/guide" -> PassIcons.Help
    "settings/health" -> PassIcons.EcgHeart
    "settings/icons" -> PassIcons.Image
    "settings/import" -> PassIcons.Download
    "settings/inbox" -> PassIcons.Inbox
    "settings/list" -> PassIcons.ViewAgenda
    "settings/navigation" -> PassIcons.Dashboard
    "settings/security" -> PassIcons.Shield
    "settings/shelf" -> PassIcons.VisibilityLock
    "settings/shortcuts" -> PassIcons.Keyboard
    "settings/sync" -> PassIcons.Sync
    "settings/tools" -> PassIcons.Widgets
    "settings/trash" -> PassIcons.Delete
    "settings/usernames" -> PassIcons.PersonSearch
    else -> PassIcons.Search
}

/** One line of the results: a setting, or a guide page on the same words. */
private class Hit(val key: String, val title: String, val where: String, val route: String?, val open: () -> Unit)

/** The deep link's screen: the same search, on a page of its own. */
@Composable
internal fun SettingsSearchScreen() {
    val shell = LocalShell.current
    SettingsSearchPane { shell.nav.popBackStack() }
}

/**
 * Search, in the settings bar itself: the field takes the title's place, and the results are
 * cards that wear the tile of the page each one opens. What was typed is kept, so backing out
 * of a result that was not the one comes back to the same list.
 */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
internal fun SettingsSearchPane(onClose: () -> Unit) {
    val shell = LocalShell.current
    var query by rememberSaveable { mutableStateOf("") }
    val keyboard = LocalSoftwareKeyboardController.current
    val focus = remember { FocusRequester() }
    // The caret only on a fresh search: back from a result, the list is there to be read.
    var focused by rememberSaveable { mutableStateOf(false) }
    BackHandler(onBack = onClose)
    val text = query.trim().lowercase()
    val words = text.split(' ').filter { it.isNotEmpty() }
    val hits = remember(text) {
        if (text.isEmpty()) return@remember emptyList<Hit>()
        val found = SearchIndex.filter { item ->
            val haystack = "${item.title} ${item.where} ${item.words}".lowercase()
            words.all { it in haystack }
        }.sortedBy { if (it.title.lowercase().startsWith(text)) 0 else if (text in it.title.lowercase()) 1 else 2 }
        // What the guide has on the same words: screens, buttons and ideas the rows do not name.
        // A plain setting is left out, since its row is already listed.
        val guide = Help.search(text).filter { topic -> topic.kind != "setting" && found.none { it.title == topic.title } }
        found.map { item ->
            Hit("s:${item.route}:${item.title}", item.title, item.where, item.route) {
                SettingsHighlight.requestTitle(item.title)
                shell.nav.navigate(item.route)
            }
        } + guide.map { topic ->
            Hit("g:${topic.id}", topic.title, "Guide, ${Help.where(topic)}", null) { Guide.open(topic.id) }
        }
    }
    Scaffold(
        topBar = {
            TopAppBar(
                title = {
                    TextField(
                        value = query,
                        onValueChange = { query = it },
                        placeholder = { Text("Search settings") },
                        singleLine = true,
                        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Search),
                        keyboardActions = KeyboardActions(onSearch = { keyboard?.hide() }),
                        trailingIcon = {
                            if (query.isNotEmpty()) {
                                IconButton(onClick = { query = "" }) { Icon(PassIcons.Close, "Clear") }
                            }
                        },
                        // The field is the bar, so it draws no box of its own.
                        colors = TextFieldDefaults.colors(
                            focusedContainerColor = Color.Transparent,
                            unfocusedContainerColor = Color.Transparent,
                            focusedIndicatorColor = Color.Transparent,
                            unfocusedIndicatorColor = Color.Transparent,
                        ),
                        modifier = Modifier
                            .fillMaxWidth()
                            .focusRequester(focus)
                            .onGloballyPositioned {
                                if (!focused) {
                                    focused = true
                                    runCatching { focus.requestFocus() }
                                }
                            },
                    )
                },
                navigationIcon = {
                    IconButton(onClick = onClose) { Icon(PassIcons.ArrowBack, "Close search") }
                },
            )
        },
    ) { padding ->
        Column(Modifier.padding(padding).fillMaxSize()) {
            if (text.isNotEmpty() && hits.isEmpty()) {
                Text(
                    "No settings match “${query.trim()}”.",
                    color = MaterialTheme.colorScheme.onSurfaceVariant,
                    style = MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.padding(20.dp),
                )
            }
            val list = rememberLazyListState()
            // Two stops when the guide adds its own: what is a setting, and what is only explained.
            val firstGuide = hits.indexOfFirst { it.route == null }
            val stops = if (firstGuide > 0) listOf(RailBucket("Settings", 0), RailBucket("Guide", firstGuide)) else emptyList()
            ScrollRailBox(
                rememberScrollRailState(list),
                Modifier.fillMaxSize(),
                fadeColor = MaterialTheme.colorScheme.background,
                buckets = stops,
                peek = false,
            ) { rows ->
                LazyColumn(
                    state = list,
                    modifier = rows.fillMaxSize(),
                    contentPadding = PaddingValues(horizontal = 16.dp, vertical = 8.dp),
                    verticalArrangement = Arrangement.spacedBy(3.dp),
                ) {
                    items(hits, key = { it.key }) { hit ->
                        HitCard(hit, words, Modifier.animateItem()) {
                            keyboard?.hide()
                            hit.open()
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun HitCard(hit: Hit, words: List<String>, modifier: Modifier = Modifier, onClick: () -> Unit) {
    val title = remember(hit.title, words) { bolded(hit.title, words) }
    val route = hit.route
    Surface(shape = RoundedCornerShape(12.dp), color = MaterialTheme.colorScheme.surfaceContainer, modifier = modifier.fillMaxWidth()) {
        WmRow(
            title = hit.title,
            titleContent = { Text(title) },
            // Gone with the rows' own tiles when settings icons are off.
            leading = if (!LocalIconReveal.current.present) {
                null
            } else {
                {
                    if (route != null) {
                        WmIconTile(routeIcon(route), accent = routeAccent(route))
                    } else {
                        WmIconTile(PassIcons.Help, accent = routeAccent("settings/guide"))
                    }
                }
            },
            supporting = {
                Text(hit.where, style = MaterialTheme.typography.labelMedium, color = MaterialTheme.colorScheme.primary)
            },
            onClick = onClick,
        )
    }
}

/** The title with the typed words in bold, so a result says why it is here. */
private fun bolded(title: String, words: List<String>): AnnotatedString {
    val lower = title.lowercase()
    val bold = BooleanArray(title.length)
    for (word in words) {
        if (word.length < 2) continue
        var from = lower.indexOf(word)
        while (from >= 0) {
            for (i in from until minOf(from + word.length, title.length)) bold[i] = true
            from = lower.indexOf(word, from + word.length)
        }
    }
    return buildAnnotatedString {
        title.forEachIndexed { i, c ->
            if (bold[i]) pushStyle(SpanStyle(fontWeight = FontWeight.Bold))
            append(c)
            if (bold[i]) pop()
        }
    }
}

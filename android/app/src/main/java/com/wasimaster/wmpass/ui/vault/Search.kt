package com.wasimaster.wmpass.ui.vault

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.KeyboardActionHandler
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.ui.fx.glass
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.parts.kindIcon
import com.wasimaster.wmpass.ui.parts.kindName
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.Springs
import com.wasimaster.wmpass.ui.theme.arrive
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.pressScale
import kotlin.math.hypot
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest

/** A circle growing from a corner: the button becoming the page behind it. */
private class ReachShape(private val fraction: Float, private val fromEnd: Boolean) : Shape {
    override fun createOutline(size: Size2, layoutDirection: LayoutDirection, density: Density): Outline {
        val anchor = with(density) {
            Offset(if (fromEnd) size.width - 46.dp.toPx() else 46.dp.toPx(), size.height - 56.dp.toPx())
        }
        val radius = hypot(size.width.toDouble(), size.height.toDouble()).toFloat() * fraction
        return Outline.Generic(
            Path().apply {
                addOval(Rect(anchor, radius))
            },
        )
    }
}

private typealias Size2 = androidx.compose.ui.geometry.Size

/** How far the layer has grown, driven by a boolean. Null while it is fully away. */
@Composable
private fun rememberReach(open: Boolean): Float? {
    val reduce = LocalReduceMotion.current
    val reach = remember { Animatable(0f) }
    var shown by remember { mutableStateOf(open) }
    LaunchedEffect(open) {
        if (open) {
            shown = true
            if (reduce) reach.snapTo(1f) else reach.animateTo(1f, tween(340, easing = androidx.compose.animation.core.FastOutSlowInEasing))
        } else {
            if (reduce) reach.snapTo(0f) else reach.animateTo(0f, tween(220))
            shown = false
        }
    }
    return if (shown) reach.value else null
}

/**
 * Search grows out of its button and fills the page, with the box staying where the thumb
 * already is. Results read upward from the box, so the best one sits nearest the finger.
 */
@Composable
fun SearchLayer() {
    val shell = LocalShell.current
    val colors = palette
    if (colors.flat) {
        StandardSearch()
        return
    }
    val reach = rememberReach(shell.searching) ?: return
    val all by Pass.entries.collectAsStateWithLifecycle()
    val focus = remember { FocusRequester() }
    var results by remember { mutableStateOf<List<Summary>>(emptyList()) }
    var query by remember { mutableStateOf(shell.searchField.text.toString()) }
    LaunchedEffect(Unit) {
        snapshotFlow { shell.searchField.text.toString() }.collectLatest { text ->
            query = text
            if (text.isBlank()) {
                results = emptyList()
            } else {
                delay(40)
                results = runCatching { Pass.search(text.trim(), "relevance") }.getOrDefault(emptyList())
            }
        }
    }
    LaunchedEffect(Unit) {
        delay(120)
        runCatching { focus.requestFocus() }
    }
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val shape = remember(reach, shell.prefs.leftHanded) { ReachShape(reach, !shell.prefs.leftHanded) }
    Box(
        Modifier
            .fillMaxSize()
            .clip(shape)
            .background(colors.ground),
    ) {
        com.wasimaster.wmpass.ui.fx.Aurora(Modifier.fillMaxWidth().height(380.dp), strength = if (colors.dark) 0.42f else 0.6f)
        Box(
            Modifier
                .fillMaxWidth()
                .height(380.dp)
                .background(androidx.compose.ui.graphics.Brush.verticalGradient(0.3f to androidx.compose.ui.graphics.Color.Transparent, 1f to colors.ground)),
        )
        Column(Modifier.fillMaxSize().imePadding().navigationBarsPadding()) {
            val lead = query.trim()
            AnimatedContent(
                targetState = when {
                    lead.isEmpty() -> "start"
                    results.isEmpty() -> "none"
                    else -> "hits"
                },
                modifier = Modifier.weight(1f).graphicsLayer { alpha = ((reach - 0.5f) * 2f).coerceIn(0f, 1f) },
                transitionSpec = { fadeIn(tween(160)) togetherWith fadeOut(tween(90)) },
                label = "search",
            ) { mode ->
                when (mode) {
                    "start" -> SearchStart(all, top)
                    "none" -> NoMatch(lead)
                    else -> LazyColumn(
                        Modifier.fillMaxSize(),
                        reverseLayout = true,
                        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = top + 16.dp, bottom = 12.dp),
                        verticalArrangement = Arrangement.spacedBy(8.dp, Alignment.Bottom),
                    ) {
                        items(results, key = { it.id }) { entry ->
                            SearchRow(entry, lead, Modifier.animateItem(fadeInSpec = tween(120), fadeOutSpec = tween(80)))
                        }
                    }
                }
            }
            Row(
                Modifier.padding(start = 14.dp, end = 14.dp, bottom = 10.dp, top = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Row(
                    Modifier
                        .weight(1f)
                        .height(60.dp)
                        .clip(CircleShape)
                        .background(colors.surface2)
                        .rim(CircleShape, colors)
                        .padding(start = 20.dp, end = 6.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(PassIcons.Search, null, tint = colors.primary, modifier = Modifier.size(22.dp))
                    Spacer(Modifier.width(12.dp))
                    Box(Modifier.weight(1f)) {
                        if (query.isEmpty()) {
                            Text("Search everything", color = colors.textLow, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
                        }
                        BasicTextField(
                            state = shell.searchField,
                            modifier = Modifier.fillMaxWidth().focusRequester(focus),
                            textStyle = TextStyle(color = colors.text, fontSize = 17.sp),
                            cursorBrush = SolidColor(colors.primary),
                            lineLimits = TextFieldLineLimits.SingleLine,
                            keyboardOptions = KeyboardOptions(
                                keyboardType = KeyboardType.Text,
                                autoCorrectEnabled = false,
                                imeAction = ImeAction.Go,
                            ),
                            onKeyboardAction = KeyboardActionHandler {
                                results.firstOrNull()?.let { shell.open(it.id) }
                                    ?: lead.takeIf { it.isNotEmpty() }?.let { title ->
                                        shell.nav.navigate("new/login?title=${android.net.Uri.encode(title)}")
                                    }
                            },
                        )
                    }
                    if (query.isNotEmpty()) {
                        BarIcon(PassIcons.Close, "Clear", colors.textMid) { shell.searchField.clearText() }
                    }
                }
                Spacer(Modifier.width(10.dp))
                val source = remember { MutableInteractionSource() }
                Box(
                    Modifier
                        .pressScale(source, 0.9f)
                        .size(52.dp)
                        .clip(CircleShape)
                        .background(colors.surface2)
                        .rim(CircleShape, colors)
                        .clickable(source, androidx.compose.material3.ripple(), onClickLabel = "Close search") {
                            shell.haptics.tick()
                            shell.searching = false
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(PassIcons.ExpandMore, "Close search", tint = colors.text)
                }
            }
        }
    }
}

@Composable
internal fun SearchRow(entry: Summary, query: String, modifier: Modifier = Modifier) {
    val colors = palette
    val shell = LocalShell.current
    val source = remember { MutableInteractionSource() }
    if (colors.flat) {
        Row(
            modifier
                .fillMaxWidth()
                .clickable(source, androidx.compose.material3.ripple()) { shell.open(entry.id) }
                .padding(start = 16.dp, end = 4.dp, top = 10.dp, bottom = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SiteTile(entry.title, entry.site, entry.kind, Modifier.sharedEntry("tile-${entry.id}", inSearch = true), 40.dp, entry.color, entry.icon)
            Spacer(Modifier.width(16.dp))
            Column(Modifier.weight(1f)) {
                Text(highlighted(entry.title.ifEmpty { "Untitled" }, query, colors.primary), color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.sharedEntry("title-${entry.id}", inSearch = true))
                if (entry.subtitle.isNotEmpty()) {
                    Text(highlighted(entry.subtitle, query, colors.primary), color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.sharedEntry("subtitle-${entry.id}", inSearch = true))
                }
            }
            if (entry.hasPassword) RowIconButton(PassIcons.ContentCopy, "Copy password for ${entry.title}") { shell.copyPassword(entry.id) }
        }
        return
    }
    Row(
        modifier
            .pressScale(source)
            .fillMaxWidth()
            .clip(Shapes.cardSmall)
            .background(colors.surface1)
            .rim(Shapes.cardSmall, colors)
            .clickable(source, androidx.compose.material3.ripple()) { shell.open(entry.id) }
            .padding(start = 12.dp, end = 10.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SiteTile(entry.title, entry.site, entry.kind, Modifier.sharedEntry("tile-${entry.id}", inSearch = true), 44.dp, entry.color, entry.icon)
        Spacer(Modifier.width(13.dp))
        Column(Modifier.weight(1f)) {
            Text(
                highlighted(entry.title.ifEmpty { "Untitled" }, query, colors.primary),
                color = colors.text,
                style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.sharedEntry("title-${entry.id}", inSearch = true),
            )
            if (entry.subtitle.isNotEmpty()) {
                Text(
                    highlighted(entry.subtitle, query, colors.primary),
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.sharedEntry("subtitle-${entry.id}", inSearch = true),
                )
            }
        }
        if (entry.hasPassword) {
            Spacer(Modifier.width(8.dp))
            RowIconButton(PassIcons.ContentCopy, "Copy password for ${entry.title}") { shell.copyPassword(entry.id) }
        }
    }
}

/** Marks where the words typed appear in a line. Query words like `kind:` mark nothing. */
private fun highlighted(text: String, query: String, color: androidx.compose.ui.graphics.Color) = buildAnnotatedString {
    append(text)
    query.split(' ').filter { it.isNotEmpty() && ':' !in it && '>' !in it }.forEach { word ->
        var from = text.indexOf(word, ignoreCase = true)
        while (from >= 0) {
            addStyle(SpanStyle(color = color), from, from + word.length)
            from = text.indexOf(word, from + word.length, ignoreCase = true)
        }
    }
}

internal val QueryChips = listOf(
    Triple("Weak", "weak", PassIcons.GppMaybe),
    Triple("Reused", "reused", PassIcons.SwapHoriz),
    Triple("Has code", "has:totp", PassIcons.Pin),
    Triple("Favorites", "favourite", PassIcons.StarFilled),
    Triple("Pinned", "pinned", PassIcons.KeepFilled),
    Triple("Changed recently", "modified<30d", PassIcons.Schedule),
    Triple("Trash", "trashed", PassIcons.Delete),
)

/**
 * Before anything is typed: what you are most likely to want, nearest the thumb first, then
 * the words that narrow a search down.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun SearchStart(all: List<Summary>, top: androidx.compose.ui.unit.Dp) {
    val shell = LocalShell.current
    val recent = shell.recents.mapNotNull { id -> all.firstOrNull { it.id == id } }.take(5)
    val favourites = all.filter { it.favourite && it.id !in shell.recents }
    LazyColumn(
        Modifier.fillMaxSize(),
        reverseLayout = true,
        contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = top + 16.dp, bottom = 8.dp),
        verticalArrangement = Arrangement.spacedBy(8.dp, Alignment.Bottom),
    ) {
        item("chips") {
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
            ) {
                QueryChips.forEach { (label, query, icon) ->
                    Pill(label, false, icon = icon) { shell.searchField.setTextAndPlaceCursorAtEnd(query) }
                }
                all.map { it.kind }.distinct().forEach { kind ->
                    Pill(kindName(kind), false, icon = kindIcon(kind)) { shell.searchField.setTextAndPlaceCursorAtEnd("kind:$kind") }
                }
            }
        }
        item("chips-title") { Eyebrow("Look for") }
        if (recent.isNotEmpty()) {
            items(recent, key = { "r" + it.id }) { SearchRow(it, "") }
            item("recent-title") { Eyebrow("Just opened") }
        }
        if (favourites.isNotEmpty()) {
            items(favourites, key = { "f" + it.id }) { SearchRow(it, "") }
            item("fav-title") { Eyebrow("Favorites") }
        }
    }
}

/** Searching for something that is not there is the quickest way to add it. */
@Composable
private fun NoMatch(query: String) {
    val colors = palette
    val shell = LocalShell.current
    val source = remember { MutableInteractionSource() }
    Column(Modifier.fillMaxSize().padding(horizontal = 16.dp, vertical = 12.dp), verticalArrangement = Arrangement.Bottom) {
        Text("Nothing called “$query”", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 6.dp, bottom = 10.dp))
        if (':' !in query && '>' !in query && '<' !in query) {
            Row(
                Modifier
                    .pressScale(source)
                    .fillMaxWidth()
                    .clip(Shapes.cardSmall)
                    .background(colors.surface1)
                    .rim(Shapes.cardSmall, colors)
                    .clickable(source, androidx.compose.material3.ripple()) {
                        shell.nav.navigate("new/login?title=${android.net.Uri.encode(query)}")
                    }
                    .padding(14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                GlyphTile(PassIcons.Add, colors.primary, size = 44.dp)
                Spacer(Modifier.width(13.dp))
                Text(
                    "Add “$query” as a new login",
                    color = colors.text,
                    style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                    maxLines = 2,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

// ---- adding ----

internal val KindOrder = listOf("login", "otp", "note", "card", "identity", "recovery", "wifi", "ssh", "api", "database", "licence", "document")

/** The small glass button that opens the choice of what to add. */
@Composable
fun AddButton(modifier: Modifier = Modifier) {
    val shell = LocalShell.current
    com.wasimaster.wmpass.ui.parts.GlassCircle(PassIcons.Add, "Add", modifier, size = 52.dp) {
        if (shell.tab == "codes") shell.nav.navigate("new/otp") else shell.adding = true
    }
}

/** Every kind of thing a vault holds, opening out of the add button. */
@Composable
fun AddLayer() {
    val shell = LocalShell.current
    val colors = palette
    if (colors.flat) {
        if (shell.adding) {
            var showAll by androidx.compose.runtime.saveable.rememberSaveable { mutableStateOf(false) }
            val chosen = shell.prefs.addKinds.filter { it in KindOrder }
            val others = KindOrder.filter { it !in chosen }
            PassSheet({ shell.adding = false }, "Add") {
                (if (showAll) chosen + others else chosen).forEach { kind ->
                    SheetRow(kindIcon(kind), kindName(kind)) {
                        shell.adding = false
                        shell.nav.navigate("new/$kind")
                    }
                }
                if (others.isNotEmpty() && !showAll) {
                    SheetRow(PassIcons.ExpandMore, "More types") { showAll = true }
                }
            }
        }
        return
    }
    val reach = rememberReach(shell.adding) ?: return
    BackHandler(shell.adding) { shell.adding = false }
    Box(
        Modifier
            .fillMaxSize()
            .background(colors.ground.copy(alpha = 0.72f * reach))
            .clickable(remember { MutableInteractionSource() }, null) { shell.adding = false },
    )
    val leftHanded = shell.prefs.leftHanded
    Box(Modifier.fillMaxSize().navigationBarsPadding().padding(start = 14.dp, end = 14.dp, bottom = 10.dp), contentAlignment = Alignment.BottomCenter) {
        Column(
            Modifier
                .fillMaxWidth()
                .graphicsLayer {
                    val pop = 0.72f + 0.28f * reach
                    scaleX = pop
                    scaleY = pop
                    alpha = reach
                    transformOrigin = androidx.compose.ui.graphics.TransformOrigin(if (leftHanded) 0.08f else 0.92f, 1f)
                }
                .glass(Shapes.sheet, tint = colors.surface2.copy(alpha = 0.92f))
                .padding(14.dp),
        ) {
            Text("Add to the vault", color = colors.text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(start = 8.dp, top = 4.dp, bottom = 12.dp))
            (shell.prefs.addKinds.filter { it in KindOrder }.ifEmpty { KindOrder }).chunked(4).forEachIndexed { rowIndex, row ->
                Row(Modifier.fillMaxWidth()) {
                    row.forEachIndexed { index, kind ->
                        val source = remember { MutableInteractionSource() }
                        Column(
                            Modifier
                                .weight(1f)
                                .arrive(rowIndex * 4 + index)
                                .pressScale(source, 0.92f)
                                .clip(Shapes.tile)
                                .clickable(source, androidx.compose.material3.ripple()) {
                                    shell.haptics.tick()
                                    shell.adding = false
                                    shell.nav.navigate("new/$kind")
                                }
                                .padding(vertical = 9.dp),
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            SiteTile("", null, kind, size = 52.dp, label = KindHue[kind])
                            Spacer(Modifier.height(7.dp))
                            Text(kindName(kind), color = colors.textMid, style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Medium), maxLines = 1)
                        }
                    }
                }
            }
        }
    }
}

private val KindHue = mapOf(
    "login" to "165", "otp" to "195", "note" to "85", "card" to "265", "identity" to "235", "recovery" to "25",
    "wifi" to "215", "ssh" to "145", "api" to "305", "database" to "340", "licence" to "55", "document" to "120",
)

/** A full page with an ordinary search field at the top. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun StandardSearch() {
    val shell = LocalShell.current
    val colors = palette
    if (!shell.searching) return
    val all by Pass.entries.collectAsStateWithLifecycle()
    val focus = remember { FocusRequester() }
    var results by remember { mutableStateOf<List<Summary>>(emptyList()) }
    var query by remember { mutableStateOf(shell.searchField.text.toString()) }
    LaunchedEffect(Unit) {
        snapshotFlow { shell.searchField.text.toString() }.collectLatest { text ->
            query = text
            results = if (text.isBlank()) emptyList() else runCatching { Pass.search(text.trim(), "relevance") }.getOrDefault(emptyList())
        }
    }
    LaunchedEffect(Unit) { runCatching { focus.requestFocus() } }
    androidx.compose.foundation.layout.Column(
        Modifier
            .fillMaxSize()
            .background(colors.ground)
            .clickable(remember { MutableInteractionSource() }, null) {}
            .statusBarsPadding()
            .imePadding()
            .navigationBarsPadding(),
    ) {
        Row(Modifier.fillMaxWidth().height(64.dp).padding(horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
            BarIcon(PassIcons.ArrowBack, "Back") {
                shell.searching = false
                shell.searchField.clearText()
            }
            Box(Modifier.weight(1f).padding(horizontal = 4.dp)) {
                if (query.isEmpty()) Text("Search vault", color = colors.textMid, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
                BasicTextField(
                    state = shell.searchField,
                    modifier = Modifier.fillMaxWidth().focusRequester(focus),
                    textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.text),
                    cursorBrush = SolidColor(colors.primary),
                    lineLimits = TextFieldLineLimits.SingleLine,
                    keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Text, autoCorrectEnabled = false, imeAction = ImeAction.Search),
                    onKeyboardAction = KeyboardActionHandler { results.firstOrNull()?.let { shell.open(it.id) } },
                )
            }
            if (query.isNotEmpty()) BarIcon(PassIcons.Close, "Clear", colors.textMid) { shell.searchField.clearText() }
        }
        androidx.compose.material3.HorizontalDivider(color = colors.hairline)
        val lead = query.trim()
        LazyColumn(Modifier.fillMaxSize()) {
            when {
                lead.isEmpty() -> {
                    item("chips") {
                        FlowRow(
                            Modifier.padding(16.dp),
                            horizontalArrangement = Arrangement.spacedBy(8.dp),
                            verticalArrangement = Arrangement.spacedBy(8.dp),
                        ) {
                            QueryChips.forEach { (label, text, _) ->
                                Pill(label, false) { shell.searchField.setTextAndPlaceCursorAtEnd(text) }
                            }
                        }
                    }
                    val recent = shell.recents.mapNotNull { id -> all.firstOrNull { it.id == id } }.take(5)
                    if (recent.isNotEmpty()) {
                        item("recent") { Eyebrow("Recent", Modifier.padding(horizontal = 16.dp)) }
                        items(recent, key = { "r" + it.id }) { SearchRow(it, "") }
                    }
                }
                results.isEmpty() -> item("none") {
                    Column(Modifier.fillMaxWidth().padding(16.dp)) {
                        Text("No results for “$lead”", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                        if (':' !in lead && '>' !in lead && '<' !in lead) {
                            Spacer(Modifier.height(12.dp))
                            com.wasimaster.wmpass.ui.parts.QuietButton("Add “$lead” as a login", icon = PassIcons.Add) {
                                shell.nav.navigate("new/login?title=${android.net.Uri.encode(lead)}")
                            }
                        }
                    }
                }
                else -> items(results, key = { it.id }) { entry -> SearchRow(entry, lead) }
            }
        }
    }
}

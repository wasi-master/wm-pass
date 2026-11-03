package com.wasimaster.wmpass.ui.vault

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.LazyRow
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.runtime.snapshotFlow
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.HealthView
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.OtpRow
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.ui.fx.Aurora
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlassCircle
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.LifeRing
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.bleed
import com.wasimaster.wmpass.ui.parts.fadeEdges
import com.wasimaster.wmpass.ui.parts.RollingCode
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.parts.kindIcon
import com.wasimaster.wmpass.ui.parts.kindName
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.arrive
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.pressScale
import kotlinx.coroutines.delay

/** What a list row is: a section title, one entry, or logins grouped by site. */
private sealed interface Item {
    val key: String

    data class Heading(val text: String) : Item {
        override val key = "h:$text"
    }

    data object Gap : Item {
        override val key = "gap"
    }

    data class One(val entry: Summary, val first: Boolean, val last: Boolean) : Item {
        override val key = entry.id
    }

    data class Stack(val site: String, val entries: List<Summary>, val first: Boolean, val last: Boolean) : Item {
        override val key = "s:$site"
    }
}

private fun filtered(entries: List<Summary>, filter: String): List<Summary> = when (filter) {
    "all" -> entries
    "favourite" -> entries.filter { it.favourite }
    else -> entries.filter { it.kind == filter }
}

/** Pinned first, then the rest by name, with several logins on one site folded together. */
private fun arrange(
    unsorted: List<Summary>,
    grouping: String,
    open: Set<String>,
    flat: Boolean,
    sort: String,
    pinnedFirst: Boolean,
    headings: Boolean,
    foldSites: Boolean,
): List<Item> {
    val entries = when (sort) {
        "changed" -> unsorted.sortedByDescending { it.modifiedMs }
        "kind" -> unsorted.sortedWith(compareBy({ com.wasimaster.wmpass.ui.parts.kindName(it.kind) }, { it.title.lowercase() }))
        "site" -> unsorted.sortedWith(compareBy({ it.site.orEmpty().ifEmpty { "~" } }, { it.title.lowercase() }))
        else -> unsorted
    }
    fun card(run: List<Any>, into: MutableList<Item>) {
        run.forEachIndexed { index, value ->
            val first = index == 0
            val last = index == run.lastIndex
            when (value) {
                is Summary -> into += Item.One(value, first, last)
                is Pair<*, *> -> {
                    @Suppress("UNCHECKED_CAST")
                    val group = value as Pair<String, List<Summary>>
                    into += Item.Stack(group.first, group.second, first, last)
                }
            }
        }
    }
    val out = mutableListOf<Item>()
    val (pinned, rest) = if (pinnedFirst) entries.partition { it.pinned } else emptyList<Summary>() to entries
    if (pinned.isNotEmpty()) {
        if (headings) out += Item.Heading("Pinned")
        card(pinned, out)
    }
    if (rest.isEmpty()) return out
    if (grouping == "folder") {
        val names = Pass.folders.value.associate { it.id to it.name }
        rest.groupBy { it.folder?.let(names::get) ?: "No folder" }.toSortedMap().forEach { (name, group) ->
            if (headings) out += Item.Heading(name)
            card(group, out)
        }
        return out
    }
    val bySite = if (foldSites) rest.filter { it.kind == "login" && it.site != null }.groupBy { it.site!! } else emptyMap()
    val row = mutableListOf<Any>()
    val done = mutableSetOf<String>()
    for (entry in rest) {
        val site = entry.site
        val group = site?.let(bySite::get)
        if (group != null && group.size > 1 && entry.kind == "login") {
            if (site !in done) {
                done += site
                row += site to group
                if (site in open) row.addAll(group)
            }
        } else {
            row += entry
        }
    }
    if (pinned.isNotEmpty()) out += if (flat && headings) Item.Heading("All items") else Item.Gap
    card(row, out)
    return out
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun VaultTab() {
    val colors = palette
    val shell = LocalShell.current
    val prefs = shell.prefs
    val entries by Pass.entries.collectAsStateWithLifecycle()
    val state by Pass.state.collectAsStateWithLifecycle()
    var filter by rememberSaveable { mutableStateOf("all") }
    val openSites = remember { mutableStateListOf<String>() }
    val shown = remember(entries, filter) { filtered(entries, filter) }
    fun arranged(list: List<Summary>) =
        arrange(list, prefs.grouping, openSites.toSet(), colors.flat, prefs.sort.value, prefs.pinnedOnTop.value, prefs.listHeadings.value, prefs.groupSites.value)
    val items = remember(shown, prefs.grouping, openSites.toList(), colors.flat, prefs.sort.value, prefs.pinnedOnTop.value, prefs.listHeadings.value, prefs.groupSites.value) {
        arranged(shown)
    }
    val listState = rememberLazyListState()
    // Space at the foot of the list that stands in for rows a filter just took away. Scrolled to
    // the end, a shorter list would otherwise snap the header and chips down over the row still
    // fading out; held, the rows below slide up into its place as they do at the top. It gives
    // the space back only as far as the list reaches past the screen, so nothing ever jumps.
    var hold by remember { mutableIntStateOf(0) }
    LaunchedEffect(listState) {
        snapshotFlow { listState.layoutInfo }.collect { info ->
            if (hold == 0) return@collect
            val end = info.visibleItemsInfo.lastOrNull()?.takeIf { it.key == "hold" }
            val past = if (end == null) hold else end.offset + end.size + info.afterContentPadding - info.viewportEndOffset
            if (past > 0) hold = (hold - past).coerceAtLeast(0)
        }
    }
    val density = androidx.compose.ui.platform.LocalDensity.current
    fun pick(next: String) {
        if (next == filter) return
        val gone = if (prefs.layout == "grid") {
            // The grid is one item of rows; only the rows it loses count.
            val rows = { list: List<Summary> -> (list.size + 1) / 2 }
            val lost = rows(shown) - rows(filtered(entries, next))
            val visible = listState.layoutInfo.visibleItemsInfo.any { it.key == "grid" }
            if (visible && lost > 0) with(density) { (146.dp * lost).roundToPx() } else 0
        } else {
            val kept = arranged(filtered(entries, next)).mapTo(HashSet()) { it.key }
            val old = items.mapTo(HashSet()) { it.key }
            listState.layoutInfo.visibleItemsInfo.filter { it.key in old && it.key !in kept }.sumOf { it.size }
        }
        hold += gone
        filter = next
    }
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val scrolled by remember {
        derivedStateOf { listState.firstVisibleItemIndex > 0 || listState.firstVisibleItemScrollOffset > 500 }
    }
    // Rows arrive one after another the first time the vault opens, and never again: after
    // that a list is a tool, and a tool does not perform.
    var arrived by rememberSaveable { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        delay(900)
        arrived = true
    }
    Box(Modifier.fillMaxSize()) {
        // The aurora sits behind the header and slides away with it; once it is out of sight
        // it stops drawing altogether.
        Aurora(
            Modifier
                .fillMaxWidth()
                .height(420.dp)
                .graphicsLayer {
                    translationY = if (listState.firstVisibleItemIndex == 0) -listState.firstVisibleItemScrollOffset * 0.6f else -10_000f
                    alpha = if (listState.firstVisibleItemIndex == 0) (1f - listState.firstVisibleItemScrollOffset / 700f).coerceIn(0f, 1f) else 0f
                },
            moving = !scrolled,
            strength = if (colors.dark) 0.62f else 0.8f,
        )
        // It has to melt into the page, not end in an edge.
        Box(
            Modifier
                .fillMaxWidth()
                .height(420.dp)
                .background(Brush.verticalGradient(0.45f to Color.Transparent, 1f to colors.ground)),
        )
        LazyColumn(
            state = listState,
            modifier = Modifier.fillMaxSize(),
            contentPadding = if (colors.flat) {
                PaddingValues(bottom = 96.dp)
            } else {
                PaddingValues(top = top + 8.dp, bottom = 190.dp, start = 16.dp, end = 16.dp)
            },
        ) {
            if (!colors.flat) {
                item("header") {
                    Header((state as? LockState.Unlocked)?.name.orEmpty(), entries.size)
                }
            }
            item("widgets") {
                Dashboard(entries)
            }
            if (entries.isNotEmpty()) {
                item("autofill") { AutofillNudge() }
            }
            if (prefs.filterChips.value) {
                item("filters") {
                    Filters(entries, filter) { pick(it) }
                }
            }
            if (entries.isEmpty()) {
                item("empty") { EmptyVault() }
            }
            if (prefs.layout == "grid") {
                item("grid") {
                    EntryGrid(shown, arrived)
                }
            } else {
                itemsIndexedStable(items, arrived, openSites, prefs.layout == "compact")
            }
            item("hold") {
                Spacer(Modifier.height(with(density) { hold.toDp() }))
            }
        }
    }
}

private fun androidx.compose.foundation.lazy.LazyListScope.itemsIndexedStable(
    items: List<Item>,
    arrived: Boolean,
    openSites: MutableList<String>,
    compact: Boolean,
) {
    items.forEachIndexed { index, item ->
        item(item.key) {
            val shell = LocalShell.current
            val modifier = Modifier.animateItem(fadeInSpec = tween(180), fadeOutSpec = tween(120)).arrive(index, !arrived)
            when (item) {
                is Item.Heading -> Eyebrow(item.text, if (palette.flat) modifier.padding(horizontal = 16.dp) else modifier)
                Item.Gap -> Spacer(modifier.height(12.dp))
                is Item.One -> {
                    val selected = item.entry.id in shell.selection
                    Column(modifier) {
                        EntryRow(
                            item.entry,
                            if (palette.flat) androidx.compose.ui.graphics.RectangleShape else remember(item.first, item.last) { SegmentShape(28.dp, item.first, item.last) },
                            compact,
                            shell.selecting,
                            selected,
                            onClick = {
                                if (shell.selecting) shell.toggleSelected(item.entry.id) else shell.tapped(item.entry)
                            },
                            onLongClick = { shell.toggleSelected(item.entry.id) },
                        )
                        if (!item.last) Hairline(Modifier.background(palette.surface1), inset = 74.dp)
                    }
                }
                is Item.Stack -> Column(modifier) {
                    val open = item.site in openSites
                    SiteStackRow(
                        item.site,
                        item.entries,
                        if (palette.flat) androidx.compose.ui.graphics.RectangleShape else remember(item.first, item.last) { SegmentShape(28.dp, item.first, item.last) },
                        open,
                    ) { if (open) openSites.remove(item.site) else openSites.add(item.site) }
                    if (!item.last) Hairline(Modifier.background(palette.surface1), inset = 74.dp)
                }
            }
        }
    }
}

@Composable
private fun EntryGrid(entries: List<Summary>, arrived: Boolean) {
    val shell = LocalShell.current
    // A plain two-column flow, because this already sits inside the page's own list.
    Column(if (palette.flat) Modifier.padding(horizontal = 16.dp) else Modifier, verticalArrangement = Arrangement.spacedBy(10.dp)) {
        entries.chunked(2).forEachIndexed { rowIndex, row ->
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                row.forEachIndexed { index, entry ->
                    EntryCard(
                        entry,
                        entry.id in shell.selection,
                        shell.selecting,
                        Modifier.weight(1f).arrive(rowIndex * 2 + index, !arrived),
                        onClick = { if (shell.selecting) shell.toggleSelected(entry.id) else shell.tapped(entry) },
                        onLongClick = { shell.toggleSelected(entry.id) },
                    )
                }
                if (row.size == 1) Spacer(Modifier.weight(1f))
            }
        }
    }
}

@Composable
private fun Header(name: String, count: Int) {
    val colors = palette
    val shell = LocalShell.current
    Row(Modifier.fillMaxWidth().padding(start = 6.dp, top = 12.dp, bottom = 20.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(name, color = colors.text, style = MaterialTheme.typography.headlineLarge.copy(fontSize = 32.sp), maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                if (count == 1) "1 item, encrypted" else "$count items, encrypted",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        GlassCircle(PassIcons.Settings, "Settings") { shell.nav.navigate("settings") }
        Spacer(Modifier.width(10.dp))
        GlassCircle(PassIcons.LockFilled, "Lock now", tint = colors.primary) { shell.lockNow() }
    }
}

/** The owner's choice of cards above the list. */
@Composable
private fun Dashboard(entries: List<Summary>) {
    val shell = LocalShell.current
    val prefs = shell.prefs
    if (entries.isEmpty()) return
    if (palette.flat) {
        FlatDashboard(entries)
        return
    }
    Column {
        val wide = prefs.widgets.filter { it == "health" || it == "codes" }
        if (wide.isNotEmpty()) {
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                wide.forEach { widget ->
                    when (widget) {
                        "health" -> HealthWidget(Modifier.weight(1f), entries)
                        "codes" -> CodesWidget(Modifier.weight(1f), entries)
                    }
                }
            }
        }
        for (widget in prefs.widgets) {
            when (widget) {
                "favourites" -> TileShelf("Favorites", entries.filter { it.favourite })
                "recent" -> TileShelf("Recently changed", entries.sortedByDescending { it.modifiedMs }.take(10))
            }
        }
    }
}

/** The same choices as plain rows: a number in a list needs no card to be read. */
@Composable
private fun FlatDashboard(entries: List<Summary>) {
    val colors = palette
    val shell = LocalShell.current
    var health by remember { mutableStateOf<HealthView?>(null) }
    LaunchedEffect(entries.size, entries.maxOfOrNull { it.modifiedMs }) {
        if ("health" in shell.prefs.widgets) {
            delay(600)
            health = runCatching { Pass.health() }.getOrNull()
        }
    }
    for (widget in shell.prefs.widgets) {
        when (widget) {
            "health" -> health?.let { h ->
                val issues = h.weak.size + h.reused.sumOf { it.size } + h.breached.size
                if (issues > 0) {
                    com.wasimaster.wmpass.ui.parts.NavRow(
                        if (issues == 1) "1 password needs attention" else "$issues passwords need attention",
                        "Password health",
                        icon = PassIcons.EcgHeart,
                        tint = colors.warn,
                    ) { shell.tab = "health" }
                }
            }
            "codes" -> {
                val count = entries.count { it.hasOtp }
                if (count > 0) {
                    com.wasimaster.wmpass.ui.parts.NavRow(if (count == 1) "1 code" else "$count codes", "Authenticator", icon = PassIcons.Pin) { shell.tab = "codes" }
                }
            }
            "favourites" -> TileShelf("Favorites", entries.filter { it.favourite })
            "recent" -> TileShelf("Recently changed", entries.sortedByDescending { it.modifiedMs }.take(10))
        }
    }
}

@Composable
private fun WidgetCard(modifier: Modifier, onClick: () -> Unit, content: @Composable () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val source = remember { MutableInteractionSource() }
    Box(
        modifier
            .pressScale(source)
            .height(100.dp)
            .clip(Shapes.card)
            .background(colors.surface1)
            .rim(Shapes.card, colors)
            .clickable(source, androidx.compose.material3.ripple()) {
                shell.haptics.tick()
                onClick()
            }
            .padding(14.dp),
    ) { content() }
}

@Composable
private fun HealthWidget(modifier: Modifier, entries: List<Summary>) {
    val colors = palette
    val shell = LocalShell.current
    var health by remember { mutableStateOf<HealthView?>(null) }
    LaunchedEffect(entries.size, entries.maxOfOrNull { it.modifiedMs }) {
        delay(600)
        health = runCatching { Pass.health() }.getOrNull()
    }
    WidgetCard(modifier, { shell.tab = "health" }) {
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.SpaceBetween) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(PassIcons.EcgHeart, null, tint = colors.primary, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text("Health", color = colors.textMid, style = MaterialTheme.typography.labelMedium)
            }
            val h = health
            AnimatedContent(h?.score, label = "score", transitionSpec = { fadeIn(tween(200)) togetherWith fadeOut(tween(120)) }) { score ->
                Row(verticalAlignment = Alignment.Bottom) {
                    Text(score?.toString() ?: "–", color = colors.text, style = MaterialTheme.typography.headlineLarge.copy(fontSize = 34.sp, lineHeight = 36.sp))
                    Spacer(Modifier.width(6.dp))
                    if (h != null) {
                        val issues = h.weak.size + h.reused.sumOf { it.size } + h.breached.size
                        Text(
                            if (issues == 0) "all good" else "$issues to fix",
                            color = if (issues == 0) colors.primary else colors.warn,
                            style = MaterialTheme.typography.labelMedium,
                            modifier = Modifier.padding(bottom = 7.dp),
                        )
                    }
                }
            }
        }
    }
}

@Composable
private fun CodesWidget(modifier: Modifier, entries: List<Summary>) {
    val colors = palette
    val shell = LocalShell.current
    var row by remember { mutableStateOf<OtpRow?>(null) }
    val count = entries.count { it.hasOtp }
    LaunchedEffect(entries.size, count) {
        while (true) {
            row = runCatching { Pass.codes().firstOrNull() }.getOrNull()
            val period = (row?.otp?.period ?: 30) * 1000L
            delay(period - System.currentTimeMillis() % period + 30)
        }
    }
    WidgetCard(modifier, { shell.tab = "codes" }) {
        val current = row
        Column(Modifier.fillMaxSize(), verticalArrangement = Arrangement.SpaceBetween) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Icon(PassIcons.Pin, null, tint = colors.primary, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(6.dp))
                Text(
                    current?.title ?: "Codes",
                    color = colors.textMid,
                    style = MaterialTheme.typography.labelMedium,
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                    modifier = Modifier.weight(1f),
                )
                if (current?.otp?.period != null) LifeRing(current.otp.period!!, size = 18.dp, stroke = 2.5.dp)
            }
            if (current?.code != null) {
                RollingCode(current.code!!, style = com.wasimaster.wmpass.ui.theme.PassType.code.copy(fontSize = 25.sp))
            } else {
                Text(if (count == 0) "None yet" else "$count codes", color = colors.text, style = MaterialTheme.typography.titleLarge)
            }
        }
    }
}

@Composable
private fun TileShelf(title: String, entries: List<Summary>) {
    val colors = palette
    val shell = LocalShell.current
    if (entries.isEmpty()) return
    Eyebrow(title, if (colors.flat) Modifier.padding(horizontal = 16.dp) else Modifier)
    LazyRow(
        if (colors.flat) Modifier else Modifier.bleed(16.dp).fadeEdges(),
        horizontalArrangement = Arrangement.spacedBy(10.dp),
        contentPadding = PaddingValues(horizontal = if (colors.flat) 16.dp else 18.dp),
    ) {
        items(entries, key = { it.id }) { entry ->
            val source = remember { MutableInteractionSource() }
            Column(
                Modifier
                    .width(68.dp)
                    .pressScale(source, 0.93f)
                    .clickable(source, androidx.compose.material3.ripple()) { shell.open(entry.id) },
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                SiteTile(entry.title, entry.site, entry.kind, size = 56.dp, label = entry.color, icon = entry.icon)
                Spacer(Modifier.height(6.dp))
                Text(
                    entry.title,
                    color = colors.textMid,
                    style = MaterialTheme.typography.labelSmall.copy(fontWeight = FontWeight.Medium),
                    maxLines = 1,
                    overflow = TextOverflow.Ellipsis,
                )
            }
        }
    }
}

@Composable
private fun Filters(entries: List<Summary>, selected: String, onSelect: (String) -> Unit) {
    val shell = LocalShell.current
    val prefs = shell.prefs
    val kinds = remember(entries) { entries.groupingBy { it.kind }.eachCount().toList().sortedByDescending { it.second } }
    if (entries.isEmpty()) return
    if (palette.flat) {
        LazyRow(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            contentPadding = PaddingValues(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 4.dp),
        ) {
            item { Pill("All", selected == "all") { onSelect("all") } }
            if (entries.any { it.favourite }) {
                item { Pill("Favorites", selected == "favourite") { onSelect("favourite") } }
            }
            items(kinds, key = { it.first }) { (kind, _) ->
                Pill(kindName(kind), selected == kind) { onSelect(kind) }
            }
        }
        return
    }
    Column {
        val next = when (prefs.layout) {
            "list" -> "compact"
            "compact" -> "grid"
            else -> "list"
        }
        Eyebrow("All items") {
            Box(
                Modifier
                    .size(32.dp)
                    .clip(CircleShape)
                    .clickable(onClickLabel = "Change layout") {
                        shell.haptics.tick()
                        prefs.updateLayout(next)
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(
                    when (prefs.layout) {
                        "grid" -> PassIcons.GridView
                        "compact" -> PassIcons.ViewList
                        else -> PassIcons.ViewAgenda
                    },
                    "Change layout",
                    tint = palette.textMid,
                    modifier = Modifier.size(20.dp),
                )
            }
        }
        LazyRow(
            Modifier.bleed(16.dp).fadeEdges(),
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            contentPadding = PaddingValues(horizontal = 16.dp),
        ) {
            item { Pill("All", selected == "all") { onSelect("all") } }
            if (entries.any { it.favourite }) {
                item { Pill("Favorites", selected == "favourite", icon = PassIcons.StarFilled) { onSelect("favourite") } }
            }
            items(kinds, key = { it.first }) { (kind, count) ->
                Pill("${kindName(kind)}  $count", selected == kind, icon = kindIcon(kind)) { onSelect(kind) }
            }
        }
        Spacer(Modifier.height(14.dp))
    }
}

@Composable
private fun EmptyVault() {
    val colors = palette
    val shell = LocalShell.current
    if (colors.flat) {
        Column(Modifier.fillMaxWidth().padding(top = 12.dp)) {
            Text("No items yet", color = colors.text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(horizontal = 16.dp))
            Spacer(Modifier.height(4.dp))
            Text("Import from another app, or add your first login.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 16.dp))
            Spacer(Modifier.height(12.dp))
            com.wasimaster.wmpass.ui.parts.NavRow("Import", "1Password, Bitwarden, LastPass, Chrome, KeePass and others", icon = PassIcons.Download) { shell.nav.navigate("settings/import") }
            com.wasimaster.wmpass.ui.parts.NavRow("Add a login", icon = PassIcons.Key) { shell.nav.navigate("new/login") }
            com.wasimaster.wmpass.ui.parts.NavRow("Add a two-factor code", icon = PassIcons.QrCodeScanner) { shell.nav.navigate("new/otp") }
        }
        return
    }
    Column(Modifier.fillMaxWidth().padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Text("Nothing here yet", color = colors.text, style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(start = 6.dp, top = 12.dp))
        Text(
            "Bring your passwords over from another app, or start with your first login.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(start = 6.dp, end = 20.dp, bottom = 8.dp),
        )
        StartCard(PassIcons.Download, "Import from another app", "1Password, Bitwarden, LastPass, Chrome, KeePass and more") {
            shell.nav.navigate("settings/import")
        }
        StartCard(PassIcons.Key, "Add your first login", "Or a card, a note, a recovery phrase") {
            shell.nav.navigate("new/login")
        }
        StartCard(PassIcons.QrCodeScanner, "Add a two-factor code", "Scan the QR code a site shows you") {
            shell.nav.navigate("new/otp")
        }
    }
}

/**
 * Asks once, until answered, to be the phone's autofill service: without it the vault only fills
 * by copy and paste. It goes as soon as WM Pass is chosen, or for good on "Not now".
 */
@Composable
private fun AutofillNudge() {
    val colors = palette
    val shell = LocalShell.current
    val context = androidx.compose.ui.platform.LocalContext.current
    val manager = remember { context.getSystemService(android.view.autofill.AutofillManager::class.java) }
    var chosen by remember { mutableStateOf(true) }
    androidx.lifecycle.compose.LifecycleResumeEffect(Unit) {
        chosen = manager == null || !manager.isAutofillSupported || manager.hasEnabledAutofillServices()
        onPauseOrDispose { }
    }
    androidx.compose.animation.AnimatedVisibility(!chosen && shell.prefs.autofillNudge.value) {
        val turnOn = {
            runCatching {
                context.startActivity(
                    android.content.Intent(android.provider.Settings.ACTION_REQUEST_SET_AUTOFILL_SERVICE)
                        .setData(android.net.Uri.parse("package:${context.packageName}")),
                )
            }
            Unit
        }
        val title = "Fill passwords in other apps"
        val body = "Make WM Pass your autofill service and it offers your logins in apps and browsers, with nothing to copy."
        if (colors.flat) {
            androidx.compose.material3.OutlinedCard(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 8.dp)) {
                Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 16.dp, bottom = 4.dp)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Icon(PassIcons.TextFields, null, tint = MaterialTheme.colorScheme.primary, modifier = Modifier.size(24.dp))
                        Spacer(Modifier.width(12.dp))
                        Text(title, style = MaterialTheme.typography.titleMedium)
                    }
                    Spacer(Modifier.height(6.dp))
                    Text(body, color = MaterialTheme.colorScheme.onSurfaceVariant, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(end = 8.dp))
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                        androidx.compose.material3.TextButton(onClick = { shell.prefs.autofillNudge.set(false) }) { Text("Not now") }
                        androidx.compose.material3.TextButton(onClick = turnOn) { Text("Turn on") }
                    }
                }
            }
        } else {
            Column(
                Modifier
                    .padding(vertical = 8.dp)
                    .fillMaxWidth()
                    .clip(Shapes.cardSmall)
                    .background(colors.surface1)
                    .rim(Shapes.cardSmall, colors)
                    .padding(16.dp),
            ) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    com.wasimaster.wmpass.ui.parts.GlyphTile(PassIcons.TextFields, colors.primary, size = 44.dp)
                    Spacer(Modifier.width(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold))
                        Text(body, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                    }
                }
                Spacer(Modifier.height(12.dp))
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    com.wasimaster.wmpass.ui.parts.QuietButton("Not now", Modifier.weight(1f), height = 44.dp) { shell.prefs.autofillNudge.set(false) }
                    com.wasimaster.wmpass.ui.parts.PrimaryButton("Turn on", Modifier.weight(1f), onClick = turnOn)
                }
            }
        }
    }
}

@Composable
private fun StartCard(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, body: String, onClick: () -> Unit) {
    val colors = palette
    val source = remember { MutableInteractionSource() }
    Row(
        Modifier
            .pressScale(source)
            .fillMaxWidth()
            .clip(Shapes.cardSmall)
            .background(colors.surface1)
            .rim(Shapes.cardSmall, colors)
            .clickable(source, androidx.compose.material3.ripple(), onClick = onClick)
            .padding(16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        com.wasimaster.wmpass.ui.parts.GlyphTile(icon, colors.primary, size = 44.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold))
            Text(body, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

package com.wasimaster.wmpass.ui.vault

import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.heading
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.heightIn
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
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
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
import androidx.lifecycle.repeatOnLifecycle
import com.wasimaster.wmkeyboard.core.ui.RailBucket
import com.wasimaster.wmkeyboard.core.ui.ScrollRailBox
import com.wasimaster.wmkeyboard.core.ui.rememberScrollRailState
import com.wasimaster.wmkeyboard.core.ui.scrollRailColors
import com.wasimaster.wmpass.core.HealthView
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.OtpRow
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.ui.fx.Aurora
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.help.HelpAction
import com.wasimaster.wmpass.ui.help.helpAnchor
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlassCircle
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.LifeRing
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.RollingCode
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.parts.bleed
import com.wasimaster.wmpass.ui.parts.fadeEdges
import com.wasimaster.wmpass.ui.parts.kindIcon
import com.wasimaster.wmpass.ui.parts.kindName
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.arrive
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.pressScale
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** What a list row is: a section title, one entry, or logins grouped by site. */
private sealed interface Item {
    val key: String

    data class Heading(val text: String) : Item {
        override val key = "h:$text"
    }

    /**
     * Space before the run of rows named [before]. A list can hold more than one, as after the
     * pinned items and again before the folders, so each is keyed by what it comes before: two
     * rows with one key would close the app.
     */
    data class Gap(val before: String) : Item {
        override val key = "gap:$before"
    }

    /**
     * [under] is the open row an item is listed under, a folder or a type. One item can show
     * under both at once, so the row it is under is part of its key.
     */
    data class One(val entry: Summary, val first: Boolean, val last: Boolean, val under: String? = null) : Item {
        override val key = if (under == null) entry.id else "$under/${entry.id}"
        val inside: Boolean get() = under != null
    }

    /** A row that opens in place: a folder, or in the types view a type. [kind] is null for a folder. */
    data class Folder(val id: String, val name: String, val entries: List<Summary>, val first: Boolean, val last: Boolean, val kind: String? = null) : Item {
        override val key = "f:$id"
    }

    data class Stack(val site: String, val entries: List<Summary>, val first: Boolean, val last: Boolean) : Item {
        override val key = "s:$site"
    }
}

/** A folder's or a type's row in a run of rows, and an item listed under it while it is open. */
private class FolderRun(val id: String, val name: String, val entries: List<Summary>, val kind: String? = null)

private class Inside(val entry: Summary, val under: String)

/** Where an open folder is kept among the open sites, which can never look like this. */
private fun folderOpenKey(id: String) = "folder:$id"

/** The same for a type in the types view. */
private fun kindOpenKey(kind: String) = "kind:$kind"

/** A folder id no folder has, for the row of items in none. */
private const val NoFolder = "~none"

/** A type's name as a heading over many of them. */
internal fun kindPlural(kind: String): String = when (kind) {
    "identity" -> "Identities"
    "recovery" -> "Recovery phrases"
    "wifi" -> "Wi-Fi networks"
    "otp" -> "Codes"
    else -> com.wasimaster.wmpass.ui.parts.kindName(kind) + "s"
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
                is Inside -> into += Item.One(value.entry, first, last, under = value.under)
                is FolderRun -> into += Item.Folder(value.id, value.name, value.entries, first, last, value.kind)
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
    if (grouping == "types") {
        // Each type that has items, then each folder, as rows that open in place: the shape
        // Bitwarden's vault opens on, which reads a large vault in a screen.
        val byKind = rest.groupBy { it.kind }
        val kinds = KindOrder.filter { it in byKind } + byKind.keys.filter { it !in KindOrder }
        val types = mutableListOf<Any>()
        kinds.forEach { kind ->
            val inside = byKind.getValue(kind)
            types += FolderRun(kindOpenKey(kind), kindPlural(kind), inside, kind)
            if (kindOpenKey(kind) in open) inside.mapTo(types) { Inside(it, kindOpenKey(kind)) }
        }
        if (headings) out += Item.Heading("Types") else if (out.isNotEmpty()) out += Item.Gap("types")
        card(types, out)
        val folders = Pass.folders.value.sortedBy { it.name.lowercase() }
        if (folders.isNotEmpty()) {
            val byFolder = rest.groupBy { it.folder }
            val known = folders.mapTo(HashSet()) { it.id }
            val loose = rest.filter { it.folder == null || it.folder !in known }
            val run = mutableListOf<Any>()
            (folders.map { it.id to it.name } + (NoFolder to "No folder")).forEach { (id, name) ->
                val inside = if (id == NoFolder) loose else byFolder[id].orEmpty()
                if (inside.isEmpty()) return@forEach
                run += FolderRun(id, name, inside)
                if (folderOpenKey(id) in open) inside.mapTo(run) { Inside(it, folderOpenKey(id)) }
            }
            if (run.isNotEmpty()) {
                out += if (headings) Item.Heading("Folders") else Item.Gap("folders")
                card(run, out)
            }
        }
        return out
    }
    if (grouping == "folder") {
        // Each folder is one row that opens in place, with the items in no folder after them.
        val folders = Pass.folders.value.sortedBy { it.name.lowercase() }
        val byFolder = rest.groupBy { it.folder }
        val run = mutableListOf<Any>()
        folders.forEach { folder ->
            val inside = byFolder[folder.id] ?: return@forEach
            run += FolderRun(folder.id, folder.name, inside)
            if (folderOpenKey(folder.id) in open) inside.mapTo(run) { Inside(it, folderOpenKey(folder.id)) }
        }
        val known = folders.mapTo(HashSet()) { it.id }
        val loose = rest.filter { it.folder == null || it.folder !in known }
        if (run.isNotEmpty()) {
            if (pinned.isNotEmpty()) out += if (flat && headings) Item.Heading("Folders") else Item.Gap("folders")
            card(run, out)
        }
        if (loose.isNotEmpty()) {
            if (out.isNotEmpty()) out += if (flat && headings) Item.Heading("No folder") else Item.Gap("loose")
            card(loose, out)
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
                if (site in open) group.mapTo(row) { Inside(it, "site:$site") }
            }
        } else {
            row += entry
        }
    }
    if (pinned.isNotEmpty()) out += if (flat && headings) Item.Heading("All items") else Item.Gap("rest")
    card(row, out)
    return out
}

/**
 * Where the list was left: its scroll, the groups open in it and the chip picked. Kept for the
 * life of the process, so opening an item, another tab or a sheet and coming back finds the list
 * as it was rather than at the top with every group shut.
 */
private object VaultMemory {
    val open = mutableStateListOf<String>()
    var index = 0
    var offset = 0
    var filter = "all"
    var arrived = false

    /**
     * The unlock these belong to. Each unlock brings a new shell, and the list starts afresh with
     * it: the sites opened and the place scrolled to in one vault say nothing in the next, such as
     * a decoy opened under pressure.
     */
    private var owner = java.lang.ref.WeakReference<Shell>(null)

    fun belongTo(shell: Shell) {
        if (owner.get() === shell) return
        owner = java.lang.ref.WeakReference(shell)
        open.clear()
        index = 0
        offset = 0
        filter = "all"
    }
}

@OptIn(ExperimentalFoundationApi::class)
@Composable
fun VaultTab() {
    val colors = palette
    val shell = LocalShell.current
    val prefs = shell.prefs
    val entries by Pass.entries.collectAsStateWithLifecycle()
    val state by Pass.state.collectAsStateWithLifecycle()
    remember(shell) { VaultMemory.belongTo(shell) }
    var filter by remember { mutableStateOf(VaultMemory.filter) }
    val openSites = VaultMemory.open
    val shown = remember(entries, filter) { filtered(entries, filter) }
    // A type chip already narrows the list to one type, so the types view lists it plainly.
    fun groupingFor(chip: String) = if (prefs.grouping == "types" && chip != "all") "smart" else prefs.grouping
    fun arranged(list: List<Summary>, chip: String = filter) =
        arrange(list, groupingFor(chip), openSites.toSet(), colors.flat, prefs.sort.value, prefs.pinnedOnTop.value, prefs.listHeadings.value, prefs.groupSites.value)
    // The folders are read inside arrange(), so they are a key too: renaming one leaves every
    // entry as it was, and the list would go on showing the old name.
    val folders by Pass.folders.collectAsStateWithLifecycle()
    val items = remember(shown, filter, prefs.grouping, openSites.toList(), colors.flat, prefs.sort.value, prefs.pinnedOnTop.value, prefs.listHeadings.value, prefs.groupSites.value, folders) {
        arranged(shown)
    }
    val listState = rememberLazyListState(VaultMemory.index, VaultMemory.offset)
    DisposableEffect(listState) {
        onDispose {
            VaultMemory.index = listState.firstVisibleItemIndex
            VaultMemory.offset = listState.firstVisibleItemScrollOffset
        }
    }
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
    // As many cards to a row as the width holds: two on a phone, more on a tablet.
    var gridColumns by remember { mutableIntStateOf(2) }
    fun pick(next: String) {
        if (next == filter) return
        val gone = if (prefs.layout == "grid") {
            // The grid is one item of rows; only the rows it loses count.
            val rows = { list: List<Summary> -> (list.size + gridColumns - 1) / gridColumns }
            val lost = rows(shown) - rows(filtered(entries, next))
            val visible = listState.layoutInfo.visibleItemsInfo.any { it.key == "grid" }
            if (visible && lost > 0) with(density) { (146.dp * lost).roundToPx() } else 0
        } else {
            val kept = arranged(filtered(entries, next), next).mapTo(HashSet()) { it.key }
            val old = items.mapTo(HashSet()) { it.key }
            listState.layoutInfo.visibleItemsInfo.filter { it.key in old && it.key !in kept }.sumOf { it.size }
        }
        hold += gone
        filter = next
        VaultMemory.filter = next
    }
    // Un-starring the last favorite, or trashing the only card, takes away the chip that was
    // chosen: the list goes back to everything rather than sitting empty with nothing chosen.
    LaunchedEffect(shown.isEmpty(), filter) {
        if (filter != "all" && entries.isNotEmpty() && shown.isEmpty()) {
            filter = "all"
            VaultMemory.filter = "all"
        }
    }
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val scrolled by remember {
        derivedStateOf { listState.firstVisibleItemIndex > 0 || listState.firstVisibleItemScrollOffset > 500 }
    }
    // Rows arrive one after another the first time the vault opens, and never again: after
    // that a list is a tool, and a tool does not perform.
    var arrived by remember { mutableStateOf(VaultMemory.arrived) }
    LaunchedEffect(Unit) {
        if (arrived) return@LaunchedEffect
        delay(900)
        arrived = true
        VaultMemory.arrived = true
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
        // The rows before the list's own, so the rail's stops land on the right ones.
        val lead = (if (colors.flat) 0 else 1) + 1 + (if (entries.isNotEmpty()) 1 else 0) + (if (prefs.filterChips.value) 1 else 0) + (if (entries.isEmpty()) 1 else 0)
        val letters = groupingFor(filter) == "smart" && prefs.sort.value == "name"
        val stops = remember(items, lead, letters) { railStops(items, lead, letters, prefs.pinnedOnTop.value) }
        ScrollRailBox(
            rememberScrollRailState(listState),
            Modifier.fillMaxSize(),
            fadeColor = colors.ground,
            colors = scrollRailColors(track = colors.textLow.copy(alpha = 0.25f), live = colors.primary.copy(alpha = 0.32f), thumb = colors.primary, pill = colors.surface3, onPill = colors.text),
            buckets = if (prefs.layout == "grid") emptyList() else stops,
            peek = false,
            reserveGutter = colors.flat,
        ) { railModifier ->
            LazyColumn(
                state = listState,
                modifier = railModifier.fillMaxSize(),
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
                        EntryGrid(shown, arrived, Modifier.helpAnchor("list.rows")) { gridColumns = it }
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
}

/**
 * The rail's stops: each heading, and with the list sorted by name each first letter, so a drag
 * down the rail says where it is. [lead] is how many rows come before the list's own.
 */
private fun railStops(items: List<Item>, lead: Int, letters: Boolean, pinnedFirst: Boolean): List<RailBucket> {
    val stops = mutableListOf<RailBucket>()
    var last: String? = null
    fun initial(text: String): String = text.firstOrNull()?.let { if (it.isLetter()) it.uppercaseChar().toString() else "#" } ?: "#"
    items.forEachIndexed { index, item ->
        val label = when (item) {
            is Item.Heading -> item.text
            is Item.One -> if (letters && !item.inside && !(pinnedFirst && item.entry.pinned)) initial(item.entry.title) else null
            is Item.Stack -> if (letters) initial(item.entries.map { it.title }.distinct().singleOrNull()?.takeIf { it.isNotEmpty() } ?: item.site) else null
            else -> null
        }
        if (label != null && label != last) {
            stops += RailBucket(label, lead + index)
            last = label
        }
    }
    return stops
}

private fun androidx.compose.foundation.lazy.LazyListScope.itemsIndexedStable(
    items: List<Item>,
    arrived: Boolean,
    openSites: MutableList<String>,
    compact: Boolean,
) {
    // The first row stands for the list when a tour points at it.
    val firstRow = items.indexOfFirst { it is Item.One || it is Item.Stack || it is Item.Folder }
    items.forEachIndexed { index, item ->
        item(item.key) {
            val shell = LocalShell.current
            val placed = Modifier.animateItem(fadeInSpec = tween(180), fadeOutSpec = tween(120)).arrive(index, !arrived)
            val modifier = if (index == firstRow) placed.helpAnchor("list.rows") else placed
            when (item) {
                is Item.Heading -> Eyebrow(item.text, if (palette.flat) modifier.padding(horizontal = 16.dp) else modifier)
                is Item.Gap -> Spacer(modifier.height(12.dp))
                is Item.One -> {
                    val selected = item.entry.id in shell.selection
                    val indent = if (item.inside) FolderIndent else 0.dp
                    Column(modifier) {
                        EntryRow(
                            item.entry,
                            if (palette.flat) androidx.compose.ui.graphics.RectangleShape else remember(item.first, item.last) { SegmentShape(28.dp, item.first, item.last) },
                            compact,
                            shell.selecting,
                            selected,
                            indent = indent,
                            onClick = {
                                if (shell.selecting) shell.toggleSelected(item.entry.id) else shell.tapped(item.entry)
                            },
                            onLongClick = { shell.toggleSelected(item.entry.id) },
                        )
                        if (!item.last) Hairline(Modifier.background(palette.surface1), inset = 74.dp + indent)
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
                is Item.Folder -> Column(modifier) {
                    val key = if (item.kind != null) item.id else folderOpenKey(item.id)
                    val open = key in openSites
                    FolderRow(
                        item.name,
                        item.entries.size,
                        if (palette.flat) androidx.compose.ui.graphics.RectangleShape else remember(item.first, item.last) { SegmentShape(28.dp, item.first, item.last) },
                        open,
                        icon = item.kind?.let(::kindIcon),
                    ) { if (open) openSites.remove(key) else openSites.add(key) }
                    if (!item.last) Hairline(Modifier.background(palette.surface1), inset = 74.dp)
                }
            }
        }
    }
}

@Composable
private fun EntryGrid(entries: List<Summary>, arrived: Boolean, modifier: Modifier = Modifier, onColumns: (Int) -> Unit = {}) {
    val shell = LocalShell.current
    // A plain flow of rows, because this already sits inside the page's own list. Two cards to a
    // row on a phone; a wider window gets another column for every card's width it has to spare,
    // rather than two cards stretched into banners.
    androidx.compose.foundation.layout.BoxWithConstraints(modifier.then(if (palette.flat) Modifier.padding(horizontal = 16.dp) else Modifier)) {
        val columns = (maxWidth / 180.dp).toInt().coerceIn(2, 6)
        androidx.compose.runtime.SideEffect { onColumns(columns) }
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            entries.chunked(columns).forEachIndexed { rowIndex, row ->
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    row.forEachIndexed { index, entry ->
                        EntryCard(
                            entry,
                            entry.id in shell.selection,
                            shell.selecting,
                            Modifier.weight(1f).arrive(rowIndex * columns + index, !arrived),
                            onClick = { if (shell.selecting) shell.toggleSelected(entry.id) else shell.tapped(entry) },
                            onLongClick = { shell.toggleSelected(entry.id) },
                        )
                    }
                    repeat(columns - row.size) { Spacer(Modifier.weight(1f)) }
                }
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
            Text(name, color = colors.text, style = MaterialTheme.typography.headlineLarge.copy(fontSize = 32.sp), maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.semantics { heading() })
            Text(
                if (count == 1) "1 item, encrypted" else "$count items, encrypted",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
            )
        }
        HelpAction("screen.window", "the vault", Modifier.padding(end = 10.dp), tint = colors.text)
        GlassCircle(PassIcons.Settings, "Settings", Modifier.helpAnchor("sidebar.settings")) { shell.nav.navigate("settings") }
        Spacer(Modifier.width(10.dp))
        GlassCircle(PassIcons.LockFilled, "Lock now", Modifier.helpAnchor("sidebar.lock"), tint = colors.primary) { shell.lockNow() }
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
            health = runCatching { healthNow(shell.prefs) }.getOrNull()
        }
    }
    var putAway by remember { mutableStateOf(false) }
    for (widget in shell.prefs.widgets) {
        when (widget) {
            "health" -> health?.let { h ->
                val issues = h.weak.size + h.reused.sumOf { it.size } + h.breached.size
                val seen = shell.prefs.healthBannerSeen
                // Fewer problems than when it was put away: the next new one brings it back.
                LaunchedEffect(issues) { if (seen.value >= 0 && issues < seen.value) seen.set(issues) }
                if (issues > 0 && issues > seen.value) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        com.wasimaster.wmpass.ui.parts.NavRow(
                            if (issues == 1) "1 password needs attention" else "$issues passwords need attention",
                            "Password health",
                            icon = PassIcons.EcgHeart,
                            tint = colors.warn,
                            modifier = Modifier.weight(1f),
                        ) { shell.tab = "health" }
                        com.wasimaster.wmpass.ui.parts.RowIconButton(PassIcons.Close, "Put this away", Modifier.padding(end = 8.dp)) { putAway = true }
                    }
                    if (putAway) {
                        PassSheet({ putAway = false }, "Put this away?") {
                            SheetRow(PassIcons.VisibilityOff, "Until something new needs attention", "Comes back when a problem is added, not for these $issues") {
                                seen.set(issues)
                                putAway = false
                            }
                            SheetRow(PassIcons.Close, "For good", "Health stays a tab away. Above the list, in Settings, brings this back") {
                                shell.prefs.updateWidgets(shell.prefs.widgets - "health")
                                putAway = false
                            }
                        }
                    }
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
            // A floor: at a large font size the card grows to hold its number rather than clipping it.
            .heightIn(min = 100.dp)
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
        health = runCatching { healthNow(shell.prefs) }.getOrNull()
    }
    WidgetCard(modifier, { shell.tab = "health" }) {
        Column(Modifier.fillMaxWidth().heightIn(min = 72.dp), verticalArrangement = Arrangement.SpaceBetween) {
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
    // Read only while the app is in view, as on the codes tab: in the background there is no one
    // to read a code, and each read asks the vault.
    val lifecycle = androidx.lifecycle.compose.LocalLifecycleOwner.current.lifecycle
    LaunchedEffect(entries.size, count, lifecycle) {
        lifecycle.repeatOnLifecycle(androidx.lifecycle.Lifecycle.State.STARTED) {
            while (true) {
                row = runCatching { Pass.codes().firstOrNull() }.getOrNull()
                val period = (row?.otp?.period ?: 30) * 1000L
                delay(period - System.currentTimeMillis() % period + 30)
            }
        }
    }
    WidgetCard(modifier, { shell.tab = "codes" }) {
        val current = row
        Column(Modifier.fillMaxWidth().heightIn(min = 72.dp), verticalArrangement = Arrangement.SpaceBetween) {
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
                Text(if (count == 0) "None yet" else if (count == 1) "1 code" else "$count codes", color = colors.text, style = MaterialTheme.typography.titleLarge)
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
                SiteTile(entry.title, entry.site, entry.kind, size = 56.dp, label = entry.color, icon = entry.icon, app = entry.app)
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
                    .clickable(onClickLabel = "Change layout", role = androidx.compose.ui.semantics.Role.Button) {
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
                    // Says which layout is on now; the tap's label says what it does.
                    "Layout: ${prefs.layout}",
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
        Text("Nothing here yet", color = colors.text, style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() }.padding(start = 6.dp, top = 12.dp))
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
 * by copy and paste. It goes as soon as WM Pass is chosen, or for good on "Not now". Once chosen,
 * it asks the same once about a browser that still fills from its own password manager, since
 * WM Pass never sees that browser's pages.
 */
@Composable
private fun AutofillNudge() {
    val colors = palette
    val shell = LocalShell.current
    val context = androidx.compose.ui.platform.LocalContext.current
    val manager = remember { context.getSystemService(android.view.autofill.AutofillManager::class.java) }
    var chosen by remember { mutableStateOf(true) }
    var lagging by remember { mutableStateOf<com.wasimaster.wmpass.system.BrowserAutofill.Browser?>(null) }
    val scope = androidx.compose.runtime.rememberCoroutineScope()
    androidx.lifecycle.compose.LifecycleResumeEffect(Unit) {
        chosen = manager == null || !manager.isAutofillSupported || manager.hasEnabledAutofillServices()
        if (chosen && shell.prefs.browserNudge.value) {
            // A package query that fails leaves the card unasked rather than closing the app.
            scope.launch {
                lagging = runCatching { com.wasimaster.wmpass.system.BrowserAutofill.browsers(context).firstOrNull { !it.usesAutofill } }.getOrNull()
            }
        }
        onPauseOrDispose { }
    }
    val browser = lagging
    val askService = !chosen && shell.prefs.autofillNudge.value
    val askBrowser = chosen && browser != null && shell.prefs.browserNudge.value
    androidx.compose.animation.AnimatedVisibility(askService || askBrowser) {
        val turnOn = {
            if (askBrowser && browser != null) {
                // A browser that does not open the page for the setting itself: the browser at
                // least, where it is under Settings, rather than a tap that does nothing.
                if (!com.wasimaster.wmpass.system.BrowserAutofill.openSetting(context, browser.pkg)) {
                    runCatching { context.packageManager.getLaunchIntentForPackage(browser.pkg)?.let(context::startActivity) }
                }
            } else {
                runCatching {
                    context.startActivity(
                        android.content.Intent(android.provider.Settings.ACTION_REQUEST_SET_AUTOFILL_SERVICE)
                            .setData(android.net.Uri.parse("package:${context.packageName}")),
                    )
                }
            }
            Unit
        }
        val notNow = { if (askBrowser) shell.prefs.browserNudge.set(false) else shell.prefs.autofillNudge.set(false) }
        val title = if (askBrowser) "Fill passwords in ${browser?.label}" else "Fill passwords in other apps"
        val body = if (askBrowser) {
            "${browser?.label} fills from its own password manager, so WM Pass never sees its pages. In its settings, turn on Autofill using another service."
        } else {
            "Make WM Pass your autofill service and it offers your logins in apps and browsers, with nothing to copy."
        }
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
                        androidx.compose.material3.TextButton(onClick = notNow) { Text("Not now") }
                        androidx.compose.material3.TextButton(onClick = turnOn) { Text(if (askBrowser) "Open settings" else "Turn on") }
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
                    com.wasimaster.wmpass.ui.parts.QuietButton("Not now", Modifier.weight(1f), height = 44.dp, onClick = notNow)
                    com.wasimaster.wmpass.ui.parts.PrimaryButton(if (askBrowser) "Open settings" else "Turn on", Modifier.weight(1f), onClick = turnOn)
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

package com.wasimaster.wmpass.ui.vault

import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.ModalBottomSheet
import androidx.compose.material3.Text
import androidx.compose.material3.rememberModalBottomSheetState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlyphChoices
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.glyphName
import com.wasimaster.wmpass.ui.parts.oklch
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.Springs
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.pressScale
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/** The app's bottom sheet: dragged away, or dismissed by tapping outside it. */
@OptIn(ExperimentalMaterial3Api::class)
@Composable
fun PassSheet(
    onDismiss: () -> Unit,
    title: String? = null,
    scroll: androidx.compose.foundation.ScrollState = rememberScrollState(),
    content: @Composable ColumnScope.() -> Unit,
) {
    val colors = palette
    val state = rememberModalBottomSheetState(skipPartiallyExpanded = true)
    ModalBottomSheet(
        onDismissRequest = onDismiss,
        sheetState = state,
        containerColor = if (colors.flat) androidx.compose.material3.MaterialTheme.colorScheme.surfaceContainerLow else colors.surface1,
        contentColor = colors.text,
        shape = if (colors.flat) androidx.compose.material3.BottomSheetDefaults.ExpandedShape else Shapes.sheet,
        dragHandle = {
            if (colors.flat) {
                androidx.compose.material3.BottomSheetDefaults.DragHandle()
            } else {
                Box(Modifier.padding(top = 10.dp, bottom = 6.dp).size(36.dp, 4.dp).clip(CircleShape).background(colors.hairline))
            }
        },
    ) {
        Column(
            Modifier
                .fillMaxWidth()
                .imePadding()
                .navigationBarsPadding()
                .verticalScroll(scroll)
                .padding(start = 18.dp, end = 18.dp, bottom = 16.dp),
        ) {
            if (title != null) {
                Text(title, color = colors.text, style = MaterialTheme.typography.titleLarge, modifier = Modifier.padding(start = 2.dp, top = 6.dp, bottom = 14.dp))
            }
            content()
        }
    }
}

/** A row in a sheet. */
@Composable
fun SheetRow(
    icon: ImageVector,
    title: String,
    subtitle: String? = null,
    tint: Color = palette.primary,
    selected: Boolean = false,
    // Off greys the row and lets the subtitle say why.
    enabled: Boolean = true,
    onClick: () -> Unit,
) {
    val colors = palette
    val shell = LocalShell.current
    val source = remember { MutableInteractionSource() }
    Row(
        Modifier
            .pressScale(source, 0.98f)
            .fillMaxWidth()
            .clip(Shapes.tile)
            .then(if (enabled) Modifier else Modifier.alpha(0.45f))
            .clickable(source, androidx.compose.material3.ripple(), enabled = enabled) {
                shell.haptics.tick()
                onClick()
            }
            .padding(vertical = 10.dp, horizontal = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GlyphTile(icon, tint, size = 42.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge)
            if (subtitle != null) Text(subtitle, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        }
        if (selected) Icon(PassIcons.Check, "Selected", tint = colors.primary, modifier = Modifier.size(22.dp))
    }
}

/** Asks before something that cannot be taken back. */
@Composable
fun ConfirmSheet(
    title: String,
    body: String,
    action: String,
    danger: Boolean = true,
    onDismiss: () -> Unit,
    onConfirm: () -> Unit,
) {
    val colors = palette
    if (colors.flat) {
        androidx.compose.material3.AlertDialog(
            onDismissRequest = onDismiss,
            title = { Text(title) },
            text = { Text(body) },
            confirmButton = {
                androidx.compose.material3.TextButton(
                    onClick = {
                        onDismiss()
                        onConfirm()
                    },
                    colors = androidx.compose.material3.ButtonDefaults.textButtonColors(contentColor = if (danger) colors.danger else colors.primary),
                ) { Text(action) }
            },
            dismissButton = { androidx.compose.material3.TextButton(onClick = onDismiss) { Text("Cancel") } },
        )
        return
    }
    PassSheet(onDismiss, title) {
        Text(body, color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
        Spacer(Modifier.height(20.dp))
        if (danger) {
            QuietButton(action, Modifier.fillMaxWidth(), PassIcons.Delete, colors.danger) {
                onDismiss()
                onConfirm()
            }
        } else {
            PrimaryButton(action, Modifier.fillMaxWidth()) {
                onDismiss()
                onConfirm()
            }
        }
        Spacer(Modifier.height(10.dp))
        QuietButton("Keep it as it is", Modifier.fillMaxWidth(), onClick = onDismiss)
    }
}

/** A short line of text typed into a sheet. */
@Composable
fun SheetField(state: androidx.compose.foundation.text.input.TextFieldState, placeholder: String, modifier: Modifier = Modifier) {
    val colors = palette
    if (colors.flat) {
        androidx.compose.material3.OutlinedTextField(
            state = state,
            modifier = modifier.fillMaxWidth(),
            label = { Text(placeholder) },
            lineLimits = androidx.compose.foundation.text.input.TextFieldLineLimits.SingleLine,
        )
        return
    }
    Box(
        modifier
            .fillMaxWidth()
            .height(54.dp)
            .clip(CircleShape)
            .background(colors.surface2)
            .padding(horizontal = 20.dp),
        contentAlignment = Alignment.CenterStart,
    ) {
        if (state.text.isEmpty()) Text(placeholder, color = colors.textLow, style = MaterialTheme.typography.bodyLarge)
        BasicTextField(
            state,
            Modifier.fillMaxWidth(),
            textStyle = TextStyle(color = colors.text, fontSize = 16.sp),
            cursorBrush = SolidColor(colors.primary),
            lineLimits = TextFieldLineLimits.SingleLine,
        )
    }
}

/** Where something goes: a folder, a new one, or none. */
@Composable
fun FolderSheet(current: String?, onDismiss: () -> Unit, onPick: (String?) -> Unit) {
    val folders by Pass.folders.collectAsStateWithLifecycle()
    val shell = LocalShell.current
    val name = rememberTextFieldState()
    PassSheet(onDismiss, "Move to") {
        SheetRow(PassIcons.Inventory2, "No folder", selected = current == null) {
            onPick(null)
            onDismiss()
        }
        folders.forEach { folder ->
            SheetRow(PassIcons.Folder, folder.name, selected = folder.id == current) {
                onPick(folder.id)
                onDismiss()
            }
        }
        Spacer(Modifier.height(12.dp))
        Row(verticalAlignment = Alignment.CenterVertically) {
            SheetField(name, "New folder", Modifier.weight(1f))
            Spacer(Modifier.width(10.dp))
            QuietButton("Add", tint = palette.primary, enabled = name.text.isNotBlank()) {
                val typed = name.text.toString().trim()
                shell.launch {
                    val reply = Pass.change("folder.add") { put("name", typed) }
                    onPick(reply.obj["id"]?.let { (it as? JsonPrimitive)?.content })
                    onDismiss()
                }
            }
        }
    }
}

/** Twelve hues of equal brightness, so no colour shouts over another. */
val LabelHues = listOf(25, 55, 85, 120, 150, 180, 210, 240, 270, 300, 330, 355)

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun ColourSheet(current: String?, onDismiss: () -> Unit, natural: com.wasimaster.wmpass.ui.parts.TileFace? = null, onPick: (String?) -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    PassSheet(onDismiss, "Color") {
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(12.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            maxItemsInEachRow = 6,
            modifier = Modifier.fillMaxWidth(),
        ) {
            LabelHues.forEach { hue ->
                val selected = current == hue.toString()
                val source = remember { MutableInteractionSource() }
                Box(
                    Modifier
                        .pressScale(source, 0.88f)
                        .size(46.dp)
                        .clip(CircleShape)
                        .background(Brush.linearGradient(listOf(oklch(0.70f, 0.14f, hue.toFloat()), oklch(0.52f, 0.15f, hue + 28f))))
                        .then(if (selected) Modifier.border(3.dp, colors.text, CircleShape) else Modifier)
                        .clickable(source, androidx.compose.material3.ripple(), onClickLabel = "Color") {
                            shell.haptics.tick()
                            onPick(hue.toString())
                            onDismiss()
                        },
                )
            }
        }
        Spacer(Modifier.height(16.dp))
        // What the item goes back to: its brand's colour, or one drawn from its name.
        Row(
            Modifier
                .fillMaxWidth()
                .clip(CircleShape)
                .background(colors.surface2)
                .then(if (current == null) Modifier.border(2.dp, colors.primary, CircleShape) else Modifier)
                .clickable {
                    shell.haptics.tick()
                    onPick(null)
                    onDismiss()
                }
                .padding(horizontal = 8.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (natural != null) {
                Box(Modifier.size(36.dp).clip(CircleShape).background(Brush.linearGradient(listOf(natural.from, natural.to))))
                Spacer(Modifier.width(12.dp))
            }
            Text("Use the site's own color", color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            if (current == null) Icon(PassIcons.Check, "In use", tint = colors.primary, modifier = Modifier.padding(end = 8.dp).size(20.dp))
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun TagSheet(present: List<String>, onDismiss: () -> Unit, suggested: List<String> = emptyList(), onToggle: (String, Boolean) -> Unit) {
    val colors = palette
    val entries by Pass.entries.collectAsStateWithLifecycle()
    // Most used first, so the few that matter are in reach without scrolling past the rest.
    val all = remember(entries) { entries.flatMap { it.tags }.groupingBy { it }.eachCount().entries.sortedWith(compareBy({ -it.value }, { it.key.lowercase() })).map { it.key } }
    val typed = rememberTextFieldState()
    var managing by remember { mutableStateOf<String?>(null) }
    var everything by remember { mutableStateOf(false) }
    PassSheet(onDismiss, "Tags") {
        val offered = suggested.filter { it !in present && it !in all }
        if (offered.isNotEmpty()) {
            Text("Suggested", color = colors.textMid, style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(start = 2.dp, bottom = 8.dp))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp), itemVerticalAlignment = Alignment.CenterVertically) {
                offered.forEach { tag -> Pill(tag, false, icon = PassIcons.Add) { onToggle(tag, true) } }
            }
            Spacer(Modifier.height(16.dp))
        }
        if (all.isNotEmpty()) {
            Text("Tap to add or take off. Hold one to rename or delete it.", color = colors.textMid, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(start = 2.dp, bottom = 8.dp))
            val search = typed.text.toString().trim()
            // The item's own tags always show; past those, the most used, until asked for the rest.
            val matching = all.filter { search.isEmpty() || it.contains(search, ignoreCase = true) }
            val shown = if (everything || search.isNotEmpty()) matching else (present.filter { it in matching } + matching.filter { it !in present }.take(TagsAtFirst)).distinct()
            FlowRow(
                horizontalArrangement = Arrangement.spacedBy(8.dp),
                verticalArrangement = Arrangement.spacedBy(8.dp),
                itemVerticalAlignment = Alignment.CenterVertically,
                modifier = Modifier.animateContentSize(Springs.glide()),
            ) {
                shown.forEach { tag ->
                    val on = tag in present
                    TagChip(tag, on, onRemove = if (on) ({ onToggle(tag, false) }) else null, onLongClick = { managing = tag }) { onToggle(tag, !on) }
                }
                val hidden = matching.size - shown.size
                if (hidden > 0) Pill("$hidden more", false, icon = PassIcons.ExpandMore) { everything = true }
            }
            Spacer(Modifier.height(14.dp))
        }
        Row(verticalAlignment = Alignment.CenterVertically) {
            SheetField(typed, "New tag", Modifier.weight(1f))
            Spacer(Modifier.width(10.dp))
            QuietButton("Add", tint = palette.primary, enabled = typed.text.isNotBlank()) {
                onToggle(typed.text.toString().trim(), true)
                onDismiss()
            }
        }
    }
    managing?.let { tag -> TagManageSheet(tag) { managing = null } }
}

/** How many tags the tag sheet shows before its "more" chip, past the item's own. */
private const val TagsAtFirst = 12

/**
 * A tag as a chip. Selected, it carries a cross that takes it off; held, it opens the sheet that
 * renames or deletes it on every item.
 */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
fun TagChip(tag: String, selected: Boolean, onRemove: (() -> Unit)?, onLongClick: () -> Unit, onClick: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    // The standard look follows Material's input chip: 32dp, small corners, outlined until chosen.
    val shape = if (colors.flat) androidx.compose.foundation.shape.RoundedCornerShape(8.dp) else CircleShape
    val scheme = MaterialTheme.colorScheme
    Row(
        Modifier
            .height(if (colors.flat) 32.dp else 36.dp)
            .clip(shape)
            .then(
                when {
                    colors.flat && selected -> Modifier.background(scheme.secondaryContainer)
                    colors.flat -> Modifier.border(1.dp, scheme.outlineVariant, shape)
                    selected -> Modifier.background(colors.primary.copy(alpha = if (colors.dark) 0.22f else 0.15f))
                    else -> Modifier.background(colors.surface2)
                },
            )
            .combinedClickable(
                onClick = {
                    shell.haptics.tick()
                    onClick()
                },
                onLongClick = {
                    shell.haptics.click()
                    onLongClick()
                },
                onLongClickLabel = "Rename or delete $tag",
            )
            .padding(start = 12.dp, end = if (onRemove != null) 4.dp else 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val picked by Pass.tagIcons.collectAsStateWithLifecycle()
        val ink = when {
            colors.flat && selected -> scheme.onSecondaryContainer
            colors.flat -> scheme.onSurfaceVariant
            selected -> colors.primary
            else -> colors.text
        }
        Icon(TagIcons.of(tag, picked), null, tint = if (selected || colors.flat) ink else colors.textMid, modifier = Modifier.size(if (colors.flat) 18.dp else 16.dp))
        Spacer(Modifier.width(if (colors.flat) 8.dp else 6.dp))
        if (colors.flat) {
            Text(tag, color = ink, style = MaterialTheme.typography.labelLarge, maxLines = 1)
        } else {
            Text(tag, color = ink, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
        }
        if (onRemove != null) {
            Spacer(Modifier.width(2.dp))
            Box(
                Modifier
                    .size(28.dp)
                    .clip(CircleShape)
                    .clickable(onClickLabel = "Take off $tag") {
                        shell.haptics.tick()
                        onRemove()
                    },
                contentAlignment = Alignment.Center,
            ) {
                Icon(PassIcons.Close, "Take off $tag", tint = if (colors.flat) scheme.onSurfaceVariant else colors.textMid, modifier = Modifier.size(if (colors.flat) 18.dp else 16.dp))
            }
        }
    }
}

/** Renames a tag, deletes it, or gives it an icon, on every item that has it. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun TagManageSheet(tag: String, onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val entries by Pass.entries.collectAsStateWithLifecycle()
    val picked by Pass.tagIcons.collectAsStateWithLifecycle()
    val holders = remember(entries, tag) { entries.filter { tag in it.tags }.map { it.id } }
    val name = remember { androidx.compose.foundation.text.input.TextFieldState(tag) }
    var deleting by remember { mutableStateOf(false) }
    suspend fun bulk(op: String, to: String? = null) = Pass.change("entries.bulk") {
        putJsonArray("ids") { holders.forEach { add(it) } }
        put("op", op)
        put("tag", tag)
        if (to != null) put("to", to)
    }
    PassSheet(onDismiss, tag) {
        Text(
            if (holders.size == 1) "On 1 item." else "On ${holders.size} items.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(start = 2.dp, bottom = 12.dp),
        )
        Row(verticalAlignment = Alignment.CenterVertically) {
            SheetField(name, "Name", Modifier.weight(1f))
            Spacer(Modifier.width(10.dp))
            val renamed = name.text.toString().trim()
            QuietButton("Rename", tint = colors.primary, enabled = renamed.isNotEmpty() && renamed != tag) {
                shell.launch {
                    bulk("tag_rename", renamed)
                    TagIcons.rename(tag, renamed)
                    onDismiss()
                }
            }
        }
        Spacer(Modifier.height(16.dp))
        Text("Icon", color = colors.textMid, style = MaterialTheme.typography.labelLarge, modifier = Modifier.padding(start = 2.dp, bottom = 8.dp))
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            val chosen = picked[tag]
            val glyphs = GlyphChoices
            // Automatic first, drawn as what it would pick, then the short list.
            (listOf<String?>(null) + TagIcons.choices).forEach { id ->
                val glyph = if (id == null) TagIcons.automatic(tag) else glyphs[id] ?: return@forEach
                val on = chosen == id
                val name = if (id == null) "Automatic" else glyphName(id)
                Box(
                    Modifier
                        .size(42.dp)
                        .clip(CircleShape)
                        .background(if (on) colors.primary.copy(alpha = 0.25f) else colors.surface2)
                        .then(if (on) Modifier.border(2.dp, colors.primary, CircleShape) else Modifier)
                        .clickable(onClickLabel = name) {
                            shell.haptics.tick()
                            shell.launch { TagIcons.set(tag, id) }
                        },
                    contentAlignment = Alignment.Center,
                ) { Icon(glyph, name, tint = if (on) colors.primary else colors.text, modifier = Modifier.size(20.dp)) }
            }
        }
        Spacer(Modifier.height(16.dp))
        QuietButton("Delete from every item", Modifier.fillMaxWidth(), PassIcons.Delete, colors.danger) { deleting = true }
    }
    if (deleting) {
        ConfirmSheet("Delete “$tag”?", "It comes off every item that has it. The items stay.", "Delete tag", onDismiss = { deleting = false }) {
            shell.launch {
                bulk("tag_remove")
                if (picked[tag] != null) TagIcons.set(tag, null)
                onDismiss()
            }
        }
    }
}

/**
 * Automatic, one of the app's glyphs, or a known brand: for authenticators above all. One grid
 * fills the sheet's width whatever the screen, and the brands load as they scroll into view, so all
 * of them are there without searching. Holding one says what it is and where it comes from.
 */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
@Composable
fun IconSheet(
    current: String?,
    onDismiss: () -> Unit,
    title: String = "",
    site: String? = null,
    kind: String = "login",
    color: String? = null,
    onPick: (String?) -> Unit,
) {
    val colors = palette
    val shell = LocalShell.current
    val context = androidx.compose.ui.platform.LocalContext.current
    val typed = rememberTextFieldState()
    val brands = remember { com.wasimaster.wmpass.ui.parts.Brands.all(context) }
    var about by remember { mutableStateOf<Pair<String, String>?>(null) }
    fun choose(id: String?) {
        shell.haptics.tick()
        onPick(id)
        onDismiss()
    }
    PassSheet(onDismiss, "Icon") {
        // Automatic, drawn as it would come out, so choosing it is not a guess.
        Row(
            Modifier
                .fillMaxWidth()
                .clip(Shapes.tile)
                .background(colors.surface2)
                .then(if (current == null) Modifier.border(2.dp, colors.primary, Shapes.tile) else Modifier)
                .clickable { choose(null) }
                .padding(10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            com.wasimaster.wmpass.ui.parts.SiteTile(title, site, kind, size = 44.dp, label = color, icon = null)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text("Automatic", color = colors.text, style = MaterialTheme.typography.bodyLarge)
                Text("The site's own mark when it is known, or a letter", color = colors.textMid, style = MaterialTheme.typography.bodySmall)
            }
            if (current == null) Icon(PassIcons.Check, "In use", tint = colors.primary, modifier = Modifier.size(22.dp))
        }
        Spacer(Modifier.height(12.dp))
        SheetField(typed, "Search icons and ${brands.size} brands")
        Spacer(Modifier.height(8.dp))
        val query = typed.text.toString().trim().lowercase()
        val glyphs = GlyphChoices.entries.filter { (id, _) -> query.isEmpty() || query in glyphName(id).lowercase() || query in id }
        val marks = remember(query, brands) {
            brands.filter { brand -> query.isEmpty() || query in brand.name.lowercase() || brand.otherNames.any { query in it.lowercase() } }
        }
        androidx.compose.foundation.lazy.grid.LazyVerticalGrid(
            columns = androidx.compose.foundation.lazy.grid.GridCells.Adaptive(54.dp),
            modifier = Modifier.fillMaxWidth().heightIn(max = 520.dp),
            horizontalArrangement = Arrangement.spacedBy(6.dp),
            verticalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            val full: androidx.compose.foundation.lazy.grid.LazyGridItemSpanScope.() -> androidx.compose.foundation.lazy.grid.GridItemSpan =
                { androidx.compose.foundation.lazy.grid.GridItemSpan(maxLineSpan) }
            if (glyphs.isNotEmpty()) {
                item(span = full, key = "glyphs") { GridHeading("Symbols", glyphs.size) }
                items(glyphs.size, key = { "g:" + glyphs[it].key }) { index ->
                    val (name, glyph) = glyphs[index]
                    val id = "glyph:$name"
                    val label = glyphName(name)
                    Box(
                        Modifier
                            .aspectRatio(1f)
                            .clip(Shapes.tileSmall)
                            .background(if (current == id) colors.primary.copy(alpha = 0.25f) else colors.surface2)
                            .combinedClickable(
                                onClickLabel = label,
                                onLongClick = {
                                    shell.haptics.click()
                                    about = label to com.wasimaster.wmpass.ui.settings.iconCredit(shell.prefs.iconStyle.value)
                                },
                            ) { choose(id) },
                        contentAlignment = Alignment.Center,
                    ) {
                        Icon(glyph, label, tint = if (current == id) colors.primary else colors.text, modifier = Modifier.size(24.dp))
                    }
                }
            }
            if (marks.isNotEmpty()) {
                item(span = full, key = "brands") { GridHeading("Brands", marks.size) }
                items(marks.size, key = { "b:" + marks[it].slug }) { index ->
                    val brand = marks[index]
                    val id = "brand:${brand.slug}"
                    Box(
                        Modifier
                            .aspectRatio(1f)
                            .clip(Shapes.tileSmall)
                            .then(if (current == id) Modifier.border(2.dp, colors.primary, Shapes.tileSmall) else Modifier)
                            .combinedClickable(
                                onClickLabel = brand.name,
                                onLongClick = {
                                    shell.haptics.click()
                                    about = brand.name to "Simple Icons, CC0. The mark belongs to its owner."
                                },
                            ) { choose(id) },
                        contentAlignment = Alignment.Center,
                    ) {
                        com.wasimaster.wmpass.ui.parts.SiteTile(brand.name, null, "login", size = 46.dp, icon = id)
                    }
                }
            }
            if (glyphs.isEmpty() && marks.isEmpty()) {
                item(span = full, key = "none") {
                    Text("Nothing called “$query”.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(12.dp))
                }
            }
        }
        Spacer(Modifier.height(10.dp))
        // Held icons say their name here, where the thumb is not covering it.
        androidx.compose.animation.AnimatedContent(about, label = "about") { shown ->
            if (shown == null) {
                Text("Hold an icon to see its name.", color = colors.textLow, style = MaterialTheme.typography.bodySmall, modifier = Modifier.fillMaxWidth().padding(horizontal = 4.dp))
            } else {
                Column(Modifier.fillMaxWidth().padding(horizontal = 4.dp)) {
                    Text(shown.first, color = colors.text, style = MaterialTheme.typography.titleSmall)
                    Text(shown.second, color = colors.textMid, style = MaterialTheme.typography.bodySmall)
                }
            }
        }
    }
}

@Composable
private fun GridHeading(text: String, count: Int) {
    val colors = palette
    Row(Modifier.fillMaxWidth().padding(start = 2.dp, top = 10.dp, bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(text, color = colors.textMid, style = MaterialTheme.typography.labelLarge, modifier = Modifier.weight(1f))
        Text("$count", color = colors.textLow, style = MaterialTheme.typography.labelMedium)
    }
}

// ---- acting on a selection ----

@Composable
fun BulkActions() {
    val shell = LocalShell.current
    val colors = palette
    var sheet by remember { mutableStateOf<String?>(null) }
    fun bulk(op: String, undoLabel: String? = null, undoOp: String? = null, args: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit = {}) {
        val ids = shell.selection.toList()
        shell.launch {
            Pass.change("entries.bulk") {
                putJsonArray("ids") { ids.forEach { add(it) } }
                put("op", op)
                args()
            }
            if (undoLabel != null && undoOp != null) {
                shell.undo = UndoOffer(undoLabel) {
                    Pass.change("entries.bulk") {
                        putJsonArray("ids") { ids.forEach { add(it) } }
                        put("op", undoOp)
                    }
                }
            }
        }
    }
    // One item held: the same as opening it and tapping Edit.
    if (shell.selection.size == 1) {
        BarIcon(PassIcons.Edit, "Edit") {
            val id = shell.selection.first()
            shell.selection.clear()
            shell.nav.navigate("edit/$id")
        }
    }
    // Four at most beside the count, so the bar never squeezes it; the rest wait under More.
    BarIcon(PassIcons.Sell, "Tag") { sheet = "tag" }
    BarIcon(PassIcons.MoreHoriz, "More") { sheet = "more" }
    BarIcon(PassIcons.Delete, "Move to trash", colors.danger) {
        val count = shell.selection.size
        bulk("trash", if (count == 1) "1 item moved to the trash" else "$count items moved to the trash", "restore")
        shell.selection.clear()
    }
    when (sheet) {
        "folder" -> FolderSheet(null, { sheet = null }) { folder ->
            bulk("folder") {
                if (folder == null) put("folder", JsonNull) else put("folder", folder)
            }
            shell.selection.clear()
        }
        "colour" -> ColourSheet(null, { sheet = null }) { hue ->
            bulk("color") {
                if (hue == null) put("color", JsonNull) else put("color", hue)
            }
            shell.selection.clear()
        }
        "tag" -> TagSheet(emptyList(), { sheet = null }) { tag, on ->
            bulk(if (on) "tag_add" else "tag_remove") { put("tag", tag) }
        }
        "merge" -> MergeSheet(shell.selection.toList(), { sheet = null }) { shell.selection.clear() }
        "more" -> {
            val entries by Pass.entries.collectAsStateWithLifecycle()
            val chosen = remember(entries, shell.selection.toList()) { entries.filter { it.id in shell.selection } }
            PassSheet({ sheet = null }, "${shell.selection.size} selected") {
                SheetRow(PassIcons.Folder, "Move to a folder") { sheet = "folder" }
                Hairline(inset = 60.dp)
                SheetRow(PassIcons.Palette, "Color") { sheet = "colour" }
                Hairline(inset = 60.dp)
                val allFav = chosen.all { it.favourite }
                SheetRow(PassIcons.StarFilled, if (allFav) "Remove from favorites" else "Add to favorites", tint = colors.warn) {
                    bulk("favourite") { put("flag", !allFav) }
                    sheet = null
                }
                Hairline(inset = 60.dp)
                val allPin = chosen.all { it.pinned }
                SheetRow(PassIcons.KeepFilled, if (allPin) "Unpin" else "Pin to the top") {
                    bulk("pin") { put("flag", !allPin) }
                    sheet = null
                }
                if (chosen.size > 1) {
                    Hairline(inset = 60.dp)
                    val oneKind = chosen.map { it.kind }.distinct().size == 1
                    SheetRow(
                        PassIcons.Layers,
                        "Merge into one",
                        if (oneKind) "You pick which stays, and which value wins where they differ" else "Only items of one kind merge",
                        enabled = oneKind,
                    ) { sheet = "merge" }
                    Hairline(inset = 60.dp)
                    SheetRow(PassIcons.Link, "Link together", "Each one lists the others under Linked") {
                        val ids = chosen.map { it.id }
                        shell.launch {
                            suspend fun setLinks(id: String, links: List<String>) = Pass.change("entry.common") {
                                put("id", id)
                                putJsonArray("links") { links.forEach { add(it) } }
                            }
                            val before = ids.associateWith { Pass.detail(it).links }
                            before.forEach { (id, have) ->
                                val next = (have + ids.filter { it != id }).distinct()
                                if (next.size != have.size) setLinks(id, next)
                            }
                            shell.undo = UndoOffer(if (ids.size == 2) "Linked the two" else "Linked all ${ids.size}") {
                                before.forEach { (id, have) -> setLinks(id, have) }
                            }
                        }
                        shell.selection.clear()
                        sheet = null
                    }
                }
                Hairline(inset = 60.dp)
                SheetRow(PassIcons.VisibilityLock, "Put on the hidden shelf", "Out of lists and autofill until you open the shelf") {
                    bulk("hide", "Moved to the hidden shelf", null) { put("flag", true) }
                    shell.selection.clear()
                    sheet = null
                }
                Hairline(inset = 60.dp)
                SheetRow(PassIcons.DoneAll, "Select everything") {
                    entries.forEach { if (it.id !in shell.selection) shell.selection.add(it.id) }
                    sheet = null
                }
            }
        }
    }
}

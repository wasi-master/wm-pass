package com.wasimaster.wmpass.ui.vault

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
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
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.oklch
import com.wasimaster.wmpass.ui.theme.Shapes
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
fun PassSheet(onDismiss: () -> Unit, title: String? = null, content: @Composable ColumnScope.() -> Unit) {
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
                .verticalScroll(rememberScrollState())
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
            .clickable(source, androidx.compose.material3.ripple()) {
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
fun ColourSheet(current: String?, onDismiss: () -> Unit, onPick: (String?) -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    PassSheet(onDismiss, "Colour") {
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
                        .clickable(source, androidx.compose.material3.ripple(), onClickLabel = "Colour") {
                            shell.haptics.tick()
                            onPick(hue.toString())
                            onDismiss()
                        },
                )
            }
        }
        Spacer(Modifier.height(16.dp))
        QuietButton("Use the site's own colour", Modifier.fillMaxWidth()) {
            onPick(null)
            onDismiss()
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun TagSheet(present: List<String>, onDismiss: () -> Unit, onToggle: (String, Boolean) -> Unit) {
    val entries by Pass.entries.collectAsStateWithLifecycle()
    val all = remember(entries) { entries.flatMap { it.tags }.distinct().sorted() }
    val typed = rememberTextFieldState()
    PassSheet(onDismiss, "Tags") {
        if (all.isNotEmpty()) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                all.forEach { tag ->
                    val on = tag in present
                    Pill(tag, on, icon = PassIcons.Sell) { onToggle(tag, !on) }
                }
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
}

/** Automatic, one of the app's glyphs, or a known brand: for authenticators above all. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun IconSheet(current: String?, onDismiss: () -> Unit, onPick: (String?) -> Unit) {
    val colors = palette
    val context = androidx.compose.ui.platform.LocalContext.current
    val typed = rememberTextFieldState()
    val brands = remember { com.wasimaster.wmpass.ui.parts.Brands.all(context) }
    PassSheet(onDismiss, "Icon") {
        QuietButton("Automatic", Modifier.fillMaxWidth(), height = 48.dp) {
            onPick(null)
            onDismiss()
        }
        Spacer(Modifier.height(16.dp))
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
            maxItemsInEachRow = 6,
        ) {
            com.wasimaster.wmpass.ui.parts.GlyphChoices.forEach { (name, glyph) ->
                val id = "glyph:$name"
                Box(
                    Modifier
                        .size(46.dp)
                        .clip(Shapes.tileSmall)
                        .background(if (current == id) colors.primary.copy(alpha = 0.25f) else colors.surface2)
                        .clickable {
                            onPick(id)
                            onDismiss()
                        },
                    contentAlignment = Alignment.Center,
                ) {
                    Icon(glyph, name, tint = colors.text, modifier = Modifier.size(22.dp))
                }
            }
        }
        Spacer(Modifier.height(18.dp))
        SheetField(typed, "Search brands")
        Spacer(Modifier.height(12.dp))
        val query = typed.text.toString().trim().lowercase()
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(10.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
            maxItemsInEachRow = 6,
        ) {
            brands.filter { query.isEmpty() || query in it.name.lowercase() }.take(48).forEach { brand ->
                com.wasimaster.wmpass.ui.parts.SiteTile(
                    brand.name,
                    null,
                    "login",
                    Modifier.clickable {
                        onPick("brand:${brand.slug}")
                        onDismiss()
                    },
                    46.dp,
                    icon = "brand:${brand.slug}",
                )
            }
        }
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
    BarIcon(PassIcons.Folder, "Move to a folder") { sheet = "folder" }
    BarIcon(PassIcons.Sell, "Tag") { sheet = "tag" }
    BarIcon(PassIcons.Palette, "Colour") { sheet = "colour" }
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
        "more" -> {
            val entries by Pass.entries.collectAsStateWithLifecycle()
            val chosen = remember(entries, shell.selection.toList()) { entries.filter { it.id in shell.selection } }
            PassSheet({ sheet = null }, "${shell.selection.size} selected") {
                val allFav = chosen.all { it.favourite }
                SheetRow(PassIcons.StarFilled, if (allFav) "Remove from favourites" else "Add to favourites", tint = colors.warn) {
                    bulk("favourite") { put("flag", !allFav) }
                    sheet = null
                }
                Hairline(inset = 60.dp)
                val allPin = chosen.all { it.pinned }
                SheetRow(PassIcons.KeepFilled, if (allPin) "Unpin" else "Pin to the top") {
                    bulk("pin") { put("flag", !allPin) }
                    sheet = null
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

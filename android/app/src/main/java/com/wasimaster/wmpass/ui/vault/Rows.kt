package com.wasimaster.wmpass.ui.vault

import androidx.compose.animation.BoundsTransform
import androidx.compose.animation.ExperimentalSharedTransitionApi
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.absoluteOffset
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.CustomAccessibilityAction
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.customActions
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathOperation
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.boundsInWindow
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.theme.Springs
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.pressScale
import com.wasimaster.wmpass.ui.theme.squirclePath
import kotlin.math.abs
import kotlin.math.roundToInt
import kotlinx.coroutines.launch

/** One piece of a stacked card: round corners only where the stack ends. */
class SegmentShape(private val radius: Dp, private val top: Boolean, private val bottom: Boolean) : Shape {
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline {
        if (!top && !bottom) return Outline.Rectangle(Rect(Offset.Zero, size))
        val r = with(density) { radius.toPx() }
        val extra = if (top && bottom) 0f else r * 3
        val full = Size(size.width, size.height + extra)
        val squircle = squirclePath(full, r)
        if (!bottom && top) {
            val rect = Path().apply { addRect(Rect(Offset.Zero, size)) }
            return Outline.Generic(Path.combine(PathOperation.Intersect, squircle, rect))
        }
        if (bottom && !top) {
            squircle.translate(Offset(0f, -extra))
            val rect = Path().apply { addRect(Rect(Offset.Zero, size)) }
            return Outline.Generic(Path.combine(PathOperation.Intersect, squircle, rect))
        }
        return Outline.Generic(squircle)
    }

    override fun equals(other: Any?) =
        other is SegmentShape && other.radius == radius && other.top == top && other.bottom == bottom

    override fun hashCode() = radius.hashCode() * 31 + top.hashCode() * 2 + bottom.hashCode()
}

/**
 * The tile, title and subtitle fly with the entry to its page. Search rows lie over the same entries in
 * the list beneath them, so only whichever list is on top takes part.
 */
@OptIn(ExperimentalSharedTransitionApi::class)
@Composable
fun Modifier.sharedEntry(key: String, inSearch: Boolean = false): Modifier {
    val shared = LocalShared.current
    val route = LocalRouteScope.current
    val shell = LocalShell.current
    if (shared == null || route == null || shell.searching != inSearch) return this
    return with(shared) {
        this@sharedEntry.sharedBounds(
            rememberSharedContentState(key),
            route,
            boundsTransform = BoundsTransform { _, _ -> Springs.glide() },
        )
    }
}

/**
 * [sharedEntry] for a page that scrolls: once the part has scrolled fully out of view, it stops
 * taking part, so going back does not fly it in from above the top of the screen.
 */
@Composable
fun Modifier.sharedEntryWhileShown(key: String, inSearch: Boolean = false): Modifier {
    var shown by remember { mutableStateOf(true) }
    val tracked = this.onGloballyPositioned { coordinates ->
        val bounds = coordinates.boundsInWindow()
        shown = bounds.width > 0f && bounds.height > 0f
    }
    return if (shown) tracked.sharedEntry(key, inSearch) else tracked
}

/**
 * Swipe a row to act on it. Right copies the password, left trashes. The action shows under the
 * row and locks in with a tick at the point of no return; letting go short of it does nothing.
 */
@Composable
fun SwipeRow(
    modifier: Modifier = Modifier,
    shape: Shape,
    enabled: Boolean = true,
    onCopy: (() -> Unit)? = null,
    onTrash: (() -> Unit)? = null,
    rightIcon: ImageVector = PassIcons.ContentCopy,
    leftIcon: ImageVector = PassIcons.Delete,
    // What each swipe does, in words; the icon's meaning when not given.
    rightLabel: String? = null,
    leftLabel: String? = null,
    content: @Composable BoxScope.() -> Unit,
) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val offset = remember { Animatable(0f) }
    var armed by remember { mutableStateOf(false) }
    val threshold = with(LocalDensity.current) { 92.dp.toPx() }
    val reach = with(LocalDensity.current) { 180.dp.toPx() }
    // A screen reader cannot swipe a row, so it is offered the same two actions by name. They are
    // handed to the row's own clickable surface through [LocalSwipeActions], so the row stays one
    // focus stop: on this outer box they would make a second, unnamed stop around it.
    val spoken = if (enabled) {
        listOfNotNull(
            onCopy?.let { act -> CustomAccessibilityAction(rightLabel ?: swipeLabel(rightIcon)) { act(); true } },
            onTrash?.let { act -> CustomAccessibilityAction(leftLabel ?: swipeLabel(leftIcon)) { act(); true } },
        )
    } else {
        emptyList()
    }
    Box(modifier.fillMaxWidth().clip(shape)) {
        val value = offset.value
        if (value != 0f) {
            val copying = value > 0
            val tint = if ((if (copying) rightIcon else leftIcon) == PassIcons.Delete) colors.danger else colors.primary
            val lit by animateFloatAsState(if (armed) 1f else 0.45f, Springs.snap(), label = "armed")
            Box(
                Modifier
                    .matchParentSize()
                    .background(tint.copy(alpha = 0.10f + 0.20f * lit)),
                // The drag is measured on the screen, left to right, so the icon is placed the same way,
                // in a right-to-left language too.
                contentAlignment = if (copying) androidx.compose.ui.AbsoluteAlignment.CenterLeft else androidx.compose.ui.AbsoluteAlignment.CenterRight,
            ) {
                val pop by animateFloatAsState(if (armed) 1.25f else 1f, Springs.lively(), label = "pop")
                Icon(
                    if (copying) rightIcon else leftIcon,
                    null,
                    tint = tint,
                    modifier = Modifier
                        .padding(horizontal = 28.dp)
                        .size(24.dp)
                        .graphicsLayer {
                            scaleX = pop
                            scaleY = pop
                            alpha = (abs(value) / threshold).coerceIn(0f, 1f)
                        },
                )
            }
        }
        Box(
            Modifier
                .fillMaxWidth()
                // Not mirrored: offset would flip the row against the finger in a right-to-left layout.
                .absoluteOffset { IntOffset(offset.value.roundToInt(), 0) }
                .then(
                    if (enabled) {
                        Modifier.pointerInput(onCopy, onTrash) {
                            detectHorizontalDragGestures(
                                onDragEnd = {
                                    val fired = armed
                                    val forward = offset.value > 0
                                    armed = false
                                    scope.launch {
                                        if (fired && !forward && leftIcon == PassIcons.Delete) {
                                            offset.animateTo(-size.width.toFloat(), Springs.glide())
                                            onTrash?.invoke()
                                            offset.snapTo(0f)
                                        } else if (fired && !forward) {
                                            onTrash?.invoke()
                                            offset.animateTo(0f, Springs.lively())
                                        } else {
                                            if (fired) onCopy?.invoke()
                                            offset.animateTo(0f, Springs.lively())
                                        }
                                    }
                                },
                                onDragCancel = {
                                    armed = false
                                    scope.launch { offset.animateTo(0f, Springs.lively()) }
                                },
                            ) { change, drag ->
                                val wants = offset.value + drag
                                val allowed = when {
                                    wants > 0 && onCopy == null -> 0f
                                    wants < 0 && onTrash == null -> 0f
                                    else -> wants
                                }
                                // Past the threshold the row resists, as if on a spring.
                                val resisted = if (abs(allowed) > reach) offset.value + drag * 0.2f else allowed
                                change.consume()
                                scope.launch { offset.snapTo(resisted) }
                                val nowArmed = abs(resisted) >= threshold
                                if (nowArmed != armed) {
                                    armed = nowArmed
                                    if (nowArmed) shell.haptics.confirm() else shell.haptics.tick()
                                }
                            }
                        }
                    } else {
                        Modifier
                    },
                ),
        ) {
            androidx.compose.runtime.CompositionLocalProvider(LocalSwipeActions provides spoken) { content() }
        }
    }
}

/** The swipe actions of the [SwipeRow] around, for the row's clickable surface to offer by name. */
val LocalSwipeActions = androidx.compose.runtime.compositionLocalOf { emptyList<CustomAccessibilityAction>() }

/** What [action], as a swipe, tap or row button, does to [entry], in words. */
fun actionLabel(action: String, entry: Summary): String = when (action) {
    "password" -> "Copy password"
    "username" -> "Copy username"
    "code" -> "Copy code"
    "trash" -> "Move to trash"
    "favourite" -> if (entry.favourite) "Remove from favorites" else "Add to favorites"
    "edit" -> "Edit"
    else -> "Open"
}

/** What a swipe that shows [icon] does, in words. */
private fun swipeLabel(icon: ImageVector): String = when (icon) {
    PassIcons.Delete -> "Move to trash"
    PassIcons.Edit -> "Edit"
    PassIcons.Star -> "Favorite"
    PassIcons.Pin -> "Copy code"
    else -> "Copy"
}

private fun swipeIcon(action: String): ImageVector = when (action) {
    "trash" -> PassIcons.Delete
    "favourite" -> PassIcons.Star
    "code" -> PassIcons.Pin
    "edit" -> PassIcons.Edit
    else -> PassIcons.ContentCopy
}

/** An entry's row. Tap opens it, the copy button copies the password, holding selects. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun EntryRow(
    entry: Summary,
    shape: Shape,
    compact: Boolean,
    selecting: Boolean,
    selected: Boolean,
    modifier: Modifier = Modifier,
    showCopy: Boolean = true,
    // How far in the row starts, for an item listed under its folder.
    indent: Dp = 0.dp,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
) {
    val colors = palette
    val shell = LocalShell.current
    val source = remember { MutableInteractionSource() }
    // Compact fits a row in the 48dp a touch needs anyway: a small tile, and the subtitle on the
    // same line as the name, so more of the vault shows at once without losing what each row says.
    val tileSize = if (compact) 28.dp else if (colors.flat) 40.dp else 46.dp
    val prefs = shell.prefs
    val right = shell.effective(prefs.swipeRight.value, entry)
    val left = shell.effective(prefs.swipeLeft.value, entry)
    val showing = entry.id == shell.paneEntry && com.wasimaster.wmpass.ui.theme.LocalWindowShape.current.panes
    SwipeRow(
        modifier,
        shape,
        enabled = !selecting,
        onCopy = right?.let { action -> { shell.perform(action, entry) } },
        onTrash = left?.let { action -> { shell.perform(action, entry) } },
        rightIcon = swipeIcon(right.orEmpty()),
        leftIcon = swipeIcon(left.orEmpty()),
        rightLabel = right?.let { actionLabel(it, entry) },
        leftLabel = left?.let { actionLabel(it, entry) },
    ) {
        val button = shell.effective(prefs.rowButton.value, entry)
        val tapCopies = prefs.tapAction.value != "open"
        val buttonShown = showCopy && !selecting && (tapCopies || button != null)
        // One focus stop for the whole row: its button and its swipes are offered as actions on it,
        // so a screen reader does not stop on the row and then again on the small button beside it.
        val swipes = LocalSwipeActions.current
        val spoken = buildList {
            if (buttonShown) {
                if (tapCopies) {
                    add(CustomAccessibilityAction("Open") { shell.open(entry.id); true })
                } else if (button != null) {
                    add(CustomAccessibilityAction(actionLabel(button, entry)) { shell.perform(button, entry); true })
                }
            }
            swipes.forEach { swipe -> if (none { it.label == swipe.label }) add(swipe) }
        }
        val tapLabel = when {
            selecting -> if (selected) "Deselect" else "Select"
            else -> shell.effective(prefs.tapAction.value, entry)?.takeIf { tapCopies }?.let { actionLabel(it, entry) } ?: "Open"
        }
        Row(
            Modifier
                .fillMaxWidth()
                .background(
                    when {
                        selected -> colors.surface3
                        // The row whose item is open beside the list, so the eye can find its place.
                        showing -> colors.primary.copy(alpha = if (colors.dark) 0.16f else 0.11f)
                        colors.flat -> colors.ground
                        else -> colors.surface1
                    },
                )
                .semantics {
                    customActions = spoken
                    if (selecting) this.selected = selected
                }
                .combinedClickable(
                    source,
                    androidx.compose.material3.ripple(),
                    onClickLabel = tapLabel,
                    onClick = onClick,
                    onLongClick = onLongClick,
                    onLongClickLabel = if (selected) "Deselect" else "Select",
                )
                .padding(
                    start = (if (colors.flat) 16.dp else 14.dp) + indent,
                    end = if (colors.flat) 4.dp else 10.dp,
                    top = if (compact) 2.dp else 12.dp,
                    bottom = if (compact) 2.dp else 12.dp,
                ),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (prefs.rowIcons.value || selected) {
                FlipTile(entry, selected, tileSize)
                Spacer(Modifier.width(if (compact) 12.dp else 14.dp))
            }
            Column(Modifier.weight(1f).then(if (compact) Modifier.heightIn(min = 44.dp) else Modifier), verticalArrangement = Arrangement.Center) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        entry.title.ifEmpty { "Untitled" },
                        color = colors.text,
                        style = if (colors.flat) MaterialTheme.typography.bodyLarge else MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f, fill = false).sharedEntry("title-${entry.id}"),
                    )
                    if (compact && prefs.rowSubtitle.value && entry.subtitle.isNotEmpty()) {
                        Text(
                            entry.subtitle,
                            color = colors.textMid,
                            style = MaterialTheme.typography.bodyMedium,
                            maxLines = 1,
                            overflow = TextOverflow.Ellipsis,
                            modifier = Modifier.padding(start = 8.dp).weight(1f, fill = false).sharedEntry("subtitle-${entry.id}"),
                        )
                    }
                    if (prefs.rowBadges.value) Badges(entry)
                }
                if (!compact && prefs.rowSubtitle.value && entry.subtitle.isNotEmpty()) {
                    Text(
                        entry.subtitle,
                        color = colors.textMid,
                        style = MaterialTheme.typography.bodyMedium,
                        maxLines = 1,
                        overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.sharedEntry("subtitle-${entry.id}"),
                    )
                }
            }
            if (showCopy && !selecting) {
                // Kept out of the screen reader's way: the row offers what it does as an action.
                val quiet = Modifier.clearAndSetSemantics { }
                if (tapCopies) {
                    // A tap copies, so this is now the way to the entry's page.
                    Spacer(Modifier.width(8.dp))
                    RowIconButton(PassIcons.ChevronRight, "Open ${entry.title}", quiet) { shell.open(entry.id) }
                } else if (button != null) {
                    Spacer(Modifier.width(8.dp))
                    // Says what it copies, since a row set to copy passwords falls back to the code or username.
                    val what = when (button) {
                        "username" -> "username"
                        "code" -> "code"
                        else -> "password"
                    }
                    RowIconButton(PassIcons.ContentCopy, "Copy $what for ${entry.title}", quiet) { shell.perform(button, entry) }
                }
            }
        }
    }
}

@Composable
private fun Badges(entry: Summary) {
    val colors = palette
    // Named, so a screen reader reading the row hears what the marks show.
    val icons = buildList<Triple<ImageVector, Color, String>> {
        // Pinned rows sit under their own heading in the standard look, so the mark says nothing new.
        if (entry.pinned && !colors.flat) add(Triple(PassIcons.KeepFilled, colors.textLow, "Pinned"))
        if (entry.favourite) add(Triple(PassIcons.StarFilled, colors.warn, "Favorite"))
        if (entry.hasOtp) add(Triple(PassIcons.Pin, colors.textLow, "One-time code"))
        if (entry.hasPasskey) add(Triple(PassIcons.KeyVertical, colors.textLow, "Passkey"))
        if (entry.attachments > 0) add(Triple(PassIcons.AttachFile, colors.textLow, "Attachments"))
        if (entry.destroyAtMs != null) add(Triple(PassIcons.Timer, colors.danger, "Deletes itself"))
    }
    icons.forEach { (icon, tint, name) ->
        Spacer(Modifier.width(5.dp))
        Icon(icon, name, tint = tint, modifier = Modifier.size(14.dp))
    }
}

/** Holding a row turns its tile over to a tick. */
@Composable
private fun FlipTile(entry: Summary, selected: Boolean, size: Dp) {
    val colors = palette
    val turn by animateFloatAsState(if (selected) 180f else 0f, Springs.glide(), label = "flip")
    Box(
        Modifier
            .size(size)
            .graphicsLayer {
                rotationY = turn
                cameraDistance = 14f * density
            },
    ) {
        if (turn < 90f) {
            SiteTile(entry.title, entry.site, entry.kind, Modifier.sharedEntry("tile-${entry.id}"), size, entry.color, entry.icon, app = entry.app)
        } else {
            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer { rotationY = 180f }
                    .clip(if (colors.flat) androidx.compose.foundation.shape.CircleShape else com.wasimaster.wmpass.ui.theme.SquircleShape(size * 0.30f))
                    .background(colors.primary),
                contentAlignment = Alignment.Center,
            ) {
                Icon(PassIcons.Check, "Selected", tint = colors.onPrimary, modifier = Modifier.size(size * 0.54f))
            }
        }
    }
}

/** Several logins on one site, as one row that opens in place. */
@Composable
fun SiteStackRow(
    site: String,
    entries: List<Summary>,
    shape: Shape,
    expanded: Boolean,
    onToggle: () -> Unit,
) {
    val colors = palette
    val shell = LocalShell.current
    val source = remember { MutableInteractionSource() }
    val turn by animateFloatAsState(if (expanded) 180f else 0f, Springs.glide(), label = "chevron")
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(if (colors.flat) colors.ground else colors.surface1)
            // Read as open or closed, and what a tap will do, rather than as a chevron's name.
            .semantics { stateDescription = if (expanded) "Expanded" else "Collapsed" }
            .clickable(source, androidx.compose.material3.ripple(), onClickLabel = if (expanded) "Collapse" else "Expand") {
                shell.haptics.tick()
                onToggle()
            }
            .padding(start = if (colors.flat) 16.dp else 14.dp, end = 18.dp, top = 12.dp, bottom = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (colors.flat) {
            SiteTile(site, site, "login", size = 40.dp)
        } else {
            Box(Modifier.size(46.dp)) {
                // Two tiles fanned, like a small deck of the same card.
                SiteTile(site, site, "login", Modifier.align(Alignment.TopEnd).graphicsLayer { rotationZ = 7f; alpha = 0.72f }, 38.dp)
                SiteTile(site, site, "login", Modifier.align(Alignment.BottomStart), 40.dp)
            }
        }
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            // Everyone calls it Google, not google.com.
            val name = entries.map { it.title }.distinct().singleOrNull()?.takeIf { it.isNotEmpty() } ?: site
            Text(name, color = colors.text, style = if (colors.flat) MaterialTheme.typography.bodyLarge else MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold), maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text("${entries.size} accounts", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        }
        Icon(PassIcons.ExpandMore, null, tint = colors.textLow, modifier = Modifier.graphicsLayer { rotationZ = turn })
    }
}

/** How far in the items under an open folder start, so they read as inside it. */
val FolderIndent = 24.dp

/** A folder in the list, or a type in the types view, as one row that opens in place to show what is in it. */
@Composable
fun FolderRow(
    name: String,
    count: Int,
    shape: Shape,
    expanded: Boolean,
    // A type's glyph; a folder shows a folder, open or shut.
    icon: androidx.compose.ui.graphics.vector.ImageVector? = null,
    onToggle: () -> Unit,
) {
    val colors = palette
    val shell = LocalShell.current
    val source = remember { MutableInteractionSource() }
    val turn by animateFloatAsState(if (expanded) 180f else 0f, Springs.glide(), label = "chevron")
    val tile = if (colors.flat) 40.dp else 46.dp
    Row(
        Modifier
            .fillMaxWidth()
            .clip(shape)
            .background(if (colors.flat) colors.ground else colors.surface1)
            .semantics { stateDescription = if (expanded) "Expanded" else "Collapsed" }
            .clickable(source, androidx.compose.material3.ripple(), onClickLabel = if (expanded) "Close" else "Open") {
                shell.haptics.tick()
                onToggle()
            }
            .padding(start = if (colors.flat) 16.dp else 14.dp, end = 18.dp, top = 12.dp, bottom = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.size(tile), contentAlignment = Alignment.Center) {
            com.wasimaster.wmpass.ui.parts.GlyphTile(icon ?: if (expanded) PassIcons.FolderOpen else PassIcons.Folder, colors.primary, size = tile)
        }
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(
                name,
                color = colors.text,
                style = if (colors.flat) MaterialTheme.typography.bodyLarge else MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            Text(if (count == 1) "1 item" else "$count items", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        }
        Icon(PassIcons.ExpandMore, null, tint = colors.textLow, modifier = Modifier.graphicsLayer { rotationZ = turn })
    }
}

/** A square card for the grid layout. */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun EntryCard(
    entry: Summary,
    selected: Boolean,
    selecting: Boolean,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
    onLongClick: () -> Unit,
) {
    val colors = palette
    val shell = LocalShell.current
    val source = remember { MutableInteractionSource() }
    val shape = com.wasimaster.wmpass.ui.theme.Shapes.cardSmall
    val copies = entry.hasPassword && !selecting
    val tapCopies = shell.prefs.tapAction.value != "open"
    Column(
        modifier
            .pressScale(source)
            .fillMaxWidth()
            // A floor: at a large font size the card grows to hold its title rather than clipping it.
            .heightIn(min = 136.dp)
            .clip(shape)
            .background(if (selected) colors.surface3 else colors.surface1)
            // One focus stop: the copy button is offered as an action on the card.
            .semantics {
                if (copies) customActions = listOf(CustomAccessibilityAction("Copy password") { shell.copyPassword(entry.id); true })
                if (selecting) this.selected = selected
            }
            .combinedClickable(
                source,
                androidx.compose.material3.ripple(),
                onClickLabel = when {
                    selecting -> if (selected) "Deselect" else "Select"
                    tapCopies -> shell.effective(shell.prefs.tapAction.value, entry)?.let { actionLabel(it, entry) } ?: "Open"
                    else -> "Open"
                },
                onClick = onClick,
                onLongClick = onLongClick,
                onLongClickLabel = if (selected) "Deselect" else "Select",
            )
            .padding(14.dp),
        verticalArrangement = Arrangement.SpaceBetween,
    ) {
        Row(verticalAlignment = Alignment.Top) {
            FlipTile(entry, selected, 44.dp)
            Spacer(Modifier.weight(1f))
            if (copies) {
                RowIconButton(PassIcons.ContentCopy, "Copy password for ${entry.title}", Modifier.offset(x = 4.dp, y = (-4).dp).clearAndSetSemantics { }) {
                    shell.copyPassword(entry.id)
                }
            }
        }
        Column {
            Text(
                entry.title.ifEmpty { "Untitled" },
                color = colors.text,
                style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.sharedEntry("title-${entry.id}"),
            )
            Text(
                entry.subtitle.ifEmpty { " " },
                color = colors.textMid,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = if (entry.subtitle.isEmpty()) Modifier else Modifier.sharedEntry("subtitle-${entry.id}"),
            )
        }
    }
}


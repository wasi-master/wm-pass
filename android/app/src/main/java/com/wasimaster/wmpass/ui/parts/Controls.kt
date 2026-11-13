package com.wasimaster.wmpass.ui.parts

import androidx.compose.ui.semantics.text
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsFocusedAsState
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.foundation.combinedClickable
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.layout
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.heading
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.toggleableState
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.wasimaster.wmpass.ui.fx.glass
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.onBrand
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.pressScale
import com.wasimaster.wmpass.ui.theme.rememberHaptics
import kotlin.math.hypot
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch

/** The main action: the brand gradient, lit from inside. */
@Composable
fun PrimaryButton(
    text: String,
    modifier: Modifier = Modifier,
    enabled: Boolean = true,
    icon: ImageVector? = null,
    onClick: () -> Unit,
) {
    val colors = palette
    val haptics = rememberHaptics()
    if (colors.flat) {
        androidx.compose.material3.Button(
            onClick = {
                haptics.click()
                onClick()
            },
            // A floor, not a height: at a large font size the label wraps to a second line and the
            // button grows to hold it, rather than clipping it or cutting it short.
            modifier = modifier.heightIn(min = 48.dp),
            enabled = enabled,
        ) {
            if (icon != null) {
                Icon(icon, null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
            }
            Text(text, maxLines = 2, overflow = TextOverflow.Ellipsis, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
        }
        return
    }
    val source = remember { MutableInteractionSource() }
    val fade by animateFloatAsState(if (enabled) 1f else 0.38f, tween(180), label = "enabled")
    Row(
        modifier
            .pressScale(source)
            .alpha(fade)
            .heightIn(min = 56.dp)
            .clip(CircleShape)
            .background(Brush.linearGradient(listOf(colors.brandFrom, colors.brandTo)))
            .rim(CircleShape, colors)
            .clickable(source, androidx.compose.material3.ripple(), enabled, role = Role.Button) {
                haptics.click()
                onClick()
            }
            .padding(horizontal = 26.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            Icon(icon, null, tint = colors.onBrand, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(10.dp))
        }
        // The pill's height is a floor: a long label at a large font size wraps to a second line and
        // the pill grows to hold it, rather than ending in an ellipsis.
        Text(
            text,
            color = colors.onBrand,
            style = MaterialTheme.typography.titleMedium.copy(fontWeight = FontWeight.SemiBold),
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            modifier = Modifier.padding(vertical = 4.dp),
        )
    }
}

/** A quiet action. */
@Composable
fun QuietButton(
    text: String,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    tint: Color = palette.text,
    enabled: Boolean = true,
    height: Dp = 56.dp,
    onClick: () -> Unit,
) {
    val colors = palette
    val haptics = rememberHaptics()
    if (colors.flat) {
        androidx.compose.material3.OutlinedButton(
            onClick = {
                haptics.click()
                onClick()
            },
            modifier = modifier.heightIn(min = 48.dp),
            enabled = enabled,
            colors = androidx.compose.material3.ButtonDefaults.outlinedButtonColors(
                contentColor = if (tint == colors.text) colors.primary else tint,
            ),
        ) {
            if (icon != null) {
                Icon(icon, null, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(8.dp))
            }
            Text(text, maxLines = 2, overflow = TextOverflow.Ellipsis, textAlign = androidx.compose.ui.text.style.TextAlign.Center)
        }
        return
    }
    val source = remember { MutableInteractionSource() }
    Row(
        modifier
            .pressScale(source)
            .alpha(if (enabled) 1f else 0.4f)
            .heightIn(min = height)
            .clip(CircleShape)
            .background(colors.surface2)
            .rim(CircleShape, colors)
            .clickable(source, androidx.compose.material3.ripple(), enabled, role = Role.Button) {
                haptics.click()
                onClick()
            }
            .padding(horizontal = 22.dp),
        horizontalArrangement = Arrangement.Center,
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            Icon(icon, null, tint = tint, modifier = Modifier.size(19.dp))
            Spacer(Modifier.width(9.dp))
        }
        Text(
            text,
            color = tint,
            style = MaterialTheme.typography.labelLarge,
            maxLines = 2,
            overflow = TextOverflow.Ellipsis,
            textAlign = androidx.compose.ui.text.style.TextAlign.Center,
            modifier = Modifier.padding(vertical = 4.dp),
        )
    }
}

/** A round button floating on glass. */
@Composable
fun GlassCircle(
    icon: ImageVector,
    label: String,
    modifier: Modifier = Modifier,
    size: Dp = 46.dp,
    tint: Color = palette.text,
    onClick: () -> Unit,
) {
    val haptics = rememberHaptics()
    if (palette.flat) {
        androidx.compose.material3.IconButton(onClick = onClick, modifier = modifier) { Icon(icon, label, tint = tint) }
        return
    }
    val source = remember { MutableInteractionSource() }
    Box(
        modifier
            .pressScale(source, 0.92f)
            .size(size)
            .glass(CircleShape)
            .clickable(source, androidx.compose.material3.ripple(), role = Role.Button, onClickLabel = label) {
                haptics.click()
                onClick()
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, label, tint = tint, modifier = Modifier.size(size * 0.46f))
    }
}

/** A round tinted icon button for rows: copy, reveal. */
@Composable
fun RowIconButton(
    icon: ImageVector,
    label: String,
    modifier: Modifier = Modifier,
    tint: Color = palette.textMid,
    onClick: () -> Unit,
) {
    val colors = palette
    val haptics = rememberHaptics()
    if (colors.flat) {
        androidx.compose.material3.IconButton(
            onClick = {
                haptics.click()
                onClick()
            },
            modifier = modifier,
        ) {
            Icon(icon, label, tint = tint, modifier = Modifier.size(22.dp))
        }
        return
    }
    val source = remember { MutableInteractionSource() }
    Box(
        modifier
            .pressScale(source, 0.88f)
            .size(40.dp)
            .clip(CircleShape)
            .background(colors.surface2)
            .clickable(source, androidx.compose.material3.ripple(), role = Role.Button, onClickLabel = label) {
                haptics.click()
                onClick()
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, label, tint = tint, modifier = Modifier.size(19.dp))
    }
}

/** A filter or a tag. Selecting one slides its colour in. */
@Composable
fun Pill(
    text: String,
    selected: Boolean,
    modifier: Modifier = Modifier,
    icon: ImageVector? = null,
    // An on and off switch, read as a checkbox, rather than one choice of several.
    toggle: Boolean = false,
    // The name a screen reader says, when the text on the pill does not read well aloud.
    description: String? = null,
    onClick: () -> Unit,
) {
    val colors = palette
    val haptics = rememberHaptics()
    if (colors.flat) {
        androidx.compose.material3.FilterChip(
            selected = selected,
            onClick = {
                haptics.tick()
                onClick()
            },
            label = { Text(text, maxLines = 1, overflow = TextOverflow.Ellipsis) },
            modifier = if (description != null) modifier.semantics { contentDescription = description } else modifier,
            leadingIcon = if (icon != null) ({ Icon(icon, null, modifier = Modifier.size(18.dp)) }) else null,
        )
        return
    }
    val source = remember { MutableInteractionSource() }
    val on by animateFloatAsState(if (selected) 1f else 0f, tween(180), label = "pill")
    Row(
        modifier
            .pressScale(source, 0.94f)
            .heightIn(min = 36.dp)
            .clip(CircleShape)
            .background(colors.surface2)
            .drawWithContent {
                if (on > 0f) drawRect(colors.primary.copy(alpha = on * if (colors.dark) 0.20f else 0.14f))
                drawContent()
            }
            .rim(CircleShape, colors)
            // Whether it is on, read out as the standard look's chip reads it.
            .semantics {
                if (toggle) {
                    toggleableState = androidx.compose.ui.state.ToggleableState(selected)
                } else {
                    this.selected = selected
                }
                if (description != null) contentDescription = description
            }
            .clickable(source, androidx.compose.material3.ripple(), role = if (toggle) Role.Checkbox else Role.Tab) {
                haptics.tick()
                onClick()
            }
            .padding(horizontal = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val tint = if (selected) colors.primary else colors.textMid
        if (icon != null) {
            Icon(icon, null, tint = tint, modifier = Modifier.size(16.dp))
            Spacer(Modifier.width(6.dp))
        }
        Text(text, color = if (selected) colors.primary else colors.text, fontSize = 13.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

/** Small capitals over a group. */
@Composable
fun Eyebrow(text: String, modifier: Modifier = Modifier, trailing: (@Composable RowScope.() -> Unit)? = null) {
    val colors = palette
    if (colors.flat) {
        Row(modifier.fillMaxWidth().padding(top = 20.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(text, style = MaterialTheme.typography.titleSmall, color = colors.primary, modifier = Modifier.weight(1f).semantics { heading() })
            trailing?.invoke(this)
        }
        return
    }
    Row(modifier.fillMaxWidth().padding(start = 6.dp, end = 6.dp, top = 22.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        // A heading to a screen reader, read in its own case: in capitals a short word can be spelt out letter by letter.
        Text(
            text.uppercase(),
            style = PassType.eyebrow,
            color = colors.textLow,
            modifier = Modifier.weight(1f).clearAndSetSemantics {
                heading()
                contentDescription = text
            },
        )
        trailing?.invoke(this)
    }
}

/** A card of grouped rows. */
@Composable
fun GroupCard(
    modifier: Modifier = Modifier,
    shape: Shape = Shapes.card,
    // In the standard look, whether the rows go without a card: Settings follows its Cards switch.
    plain: Boolean = com.wasimaster.wmsuite.settings.LocalWmPlainGroups.current,
    content: @Composable () -> Unit,
) {
    val colors = palette
    if (colors.flat) {
        // Rows run edge to edge, past the page's margin.
        if (plain) {
            Column(modifier.bleed(16.dp).fillMaxWidth()) { content() }
            return
        }
        // The card a settings group sits on, so these pages match the rest of Settings.
        androidx.compose.material3.Surface(
            shape = androidx.compose.foundation.shape.RoundedCornerShape(24.dp),
            color = MaterialTheme.colorScheme.surfaceContainer,
            modifier = modifier.fillMaxWidth(),
        ) { Column { content() } }
        return
    }
    Column(
        modifier
            .fillMaxWidth()
            .clip(shape)
            .background(colors.surface1)
            .rim(shape, colors),
    ) { content() }
}

@Composable
fun Hairline(modifier: Modifier = Modifier, inset: Dp = 18.dp) {
    val colors = palette
    if (colors.flat) return
    Box(modifier.fillMaxWidth().padding(start = inset).height(1.dp).background(colors.hairline))
}

/** Tile with a glyph: the small lit square at the start of a settings row. */
@Composable
fun GlyphTile(icon: ImageVector, tint: Color, modifier: Modifier = Modifier, size: Dp = 38.dp) {
    if (palette.flat) {
        // A plain icon: a tinted square on every row is one of the surest signs of a template.
        Icon(icon, null, tint = if (tint == palette.primary) palette.textMid else tint, modifier = modifier.size(24.dp))
        return
    }
    Box(
        modifier
            .size(size)
            .clip(Shapes.tileSmall)
            .background(tint.copy(alpha = 0.16f)),
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, null, tint = tint, modifier = Modifier.size(size * 0.54f))
    }
}

/** A plain navigating row. */
@Composable
fun NavRow(
    title: String,
    subtitle: String? = null,
    icon: ImageVector? = null,
    tint: Color = palette.primary,
    trailing: String? = null,
    /** The guide's topic for what the row does, drawn as a "?" before the chevron. */
    help: String? = null,
    modifier: Modifier = Modifier,
    onClick: () -> Unit,
) {
    val colors = palette
    val haptics = rememberHaptics()
    Row(
        modifier
            .fillMaxWidth()
            .clickable(role = Role.Button) {
                haptics.tick()
                onClick()
            }
            .padding(horizontal = 16.dp, vertical = 13.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        if (icon != null) {
            GlyphTile(icon, tint)
            Spacer(Modifier.width(14.dp))
        }
        Column(Modifier.weight(1f)) {
            // Two lines, so a title at a large font size is not cut down to its first word or two.
            Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (subtitle != null) {
                Text(subtitle, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
        }
        if (trailing != null) {
            Spacer(Modifier.width(10.dp))
            // Measured before the title, which takes what is left: a long value must not take it all.
            Text(
                trailing,
                color = colors.textLow,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.widthIn(max = 160.dp),
            )
        }
        if (help != null) com.wasimaster.wmpass.ui.help.HelpMark(help, title)
        Spacer(Modifier.width(6.dp))
        Icon(com.wasimaster.wmpass.ui.icons.PassIcons.ChevronRight, null, tint = colors.textLow, modifier = Modifier.size(20.dp))
    }
}

/** Where ink starts, and how far it has spread. See [inkSurface]. */
class InkState(private val scope: CoroutineScope) {
    internal val spread = Animatable(0f)
    internal val fade = Animatable(0f)
    internal var from = Offset.Zero

    fun splash(at: Offset) {
        from = at
        scope.launch {
            fade.snapTo(1f)
            spread.snapTo(0f)
            launch { spread.animateTo(1f, tween(420)) }
            kotlinx.coroutines.delay(620)
            fade.animateTo(0f, tween(420))
        }
    }
}

/** Ink spreading from a point across a surface: a copy you can see land. */
fun Modifier.inkSurface(state: InkState, color: Color): Modifier = drawWithContent {
    if (state.fade.value > 0f) {
        val reach = hypot(size.width.toDouble(), size.height.toDouble()).toFloat()
        drawCircle(
            color.copy(alpha = 0.22f * state.fade.value),
            radius = reach * state.spread.value,
            center = state.from,
        )
    }
    drawContent()
}

/**
 * Tap and long-press with the finger's position, for ink. A real click underneath, so a hardware
 * keyboard or D-pad can reach it (Enter taps, from the middle), a ring shows where focus is, and a
 * screen reader hears [tapLabel] and [longPressLabel]. With [enabled] off it takes no focus and
 * says it does nothing, rather than offering a tap that does nothing.
 */
@OptIn(androidx.compose.foundation.ExperimentalFoundationApi::class)
fun Modifier.tapAt(
    onLongPress: (() -> Unit)? = null,
    tapLabel: String? = null,
    longPressLabel: String? = null,
    enabled: Boolean = true,
    onTap: (Offset) -> Unit,
): Modifier = composed {
    val source = remember { MutableInteractionSource() }
    val at = remember { androidx.compose.runtime.mutableStateOf(Offset.Unspecified) }
    val size = remember { androidx.compose.runtime.mutableStateOf(androidx.compose.ui.unit.IntSize.Zero) }
    androidx.compose.runtime.LaunchedEffect(source) {
        source.interactions.collect { if (it is androidx.compose.foundation.interaction.PressInteraction.Press) at.value = it.pressPosition }
    }
    val focused by source.collectIsFocusedAsState()
    val ring = palette.primary
    this
        .onSizeChanged { size.value = it }
        .drawWithContent {
            drawContent()
            if (focused) drawRect(ring, style = androidx.compose.ui.graphics.drawscope.Stroke(2.dp.toPx()))
        }
        .combinedClickable(
            interactionSource = source,
            indication = null,
            enabled = enabled,
            onClickLabel = tapLabel,
            onLongClickLabel = longPressLabel,
            onLongClick = onLongPress,
        ) {
            val from = at.value.takeIf { it != Offset.Unspecified && it != Offset.Zero }
                ?: Offset(size.value.width / 2f, size.value.height / 2f)
            at.value = Offset.Unspecified
            onTap(from)
        }
}

/**
 * Text that appears to say something went wrong, or what happened: a screen reader says it as
 * it appears or changes, rather than leaving it for someone to find.
 */
fun Modifier.announced(): Modifier = semantics { liveRegion = androidx.compose.ui.semantics.LiveRegionMode.Polite }

/** Lets a row run to the screen's edge from inside a page's margins. */
fun Modifier.bleed(margin: Dp): Modifier = layout { measurable, constraints ->
    val extra = margin.roundToPx() * 2
    val placeable = measurable.measure(
        constraints.copy(minWidth = constraints.minWidth + extra, maxWidth = constraints.maxWidth + extra),
    )
    layout(constraints.maxWidth, placeable.height) { placeable.place(-extra / 2, 0) }
}

/** A scrolling row melts away at its ends instead of being cut off. */
fun Modifier.fadeEdges(width: Dp = 20.dp): Modifier = graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
    .drawWithContent {
        drawContent()
        val edge = (width.toPx() / size.width).coerceAtMost(0.4f)
        drawRect(
            Brush.horizontalGradient(0f to Color.Transparent, edge to Color.Black, 1f - edge to Color.Black, 1f to Color.Transparent),
            blendMode = BlendMode.DstIn,
        )
    }

/**
 * [placeholderDecorator] with the field's name drawn above it, inside the field, so a screen
 * reader names the field by it even once something is typed, as it does a Material field's label.
 * Outside the field, the name would be a stray line beside a box that says only what is in it.
 */
fun labelledDecorator(
    state: androidx.compose.foundation.text.input.TextFieldState,
    label: @Composable () -> Unit,
    minHeight: Dp,
    placeholder: @Composable () -> Unit,
): androidx.compose.foundation.text.input.TextFieldDecorator =
    androidx.compose.foundation.text.input.TextFieldDecorator { inner ->
        Column {
            label()
            Box(Modifier.heightIn(min = minHeight), contentAlignment = Alignment.CenterStart) {
                Box(Modifier.fillMaxWidth(), contentAlignment = Alignment.CenterStart, propagateMinConstraints = true) {
                    if (state.text.isEmpty()) placeholder()
                    inner()
                }
            }
        }
    }

/**
 * A field's placeholder drawn inside the field, so a screen reader takes it as the field's own
 * name, as it does a Material field's, rather than as a stray line beside an unnamed box.
 */
fun placeholderDecorator(
    state: androidx.compose.foundation.text.input.TextFieldState,
    // The field's name once something is typed and the placeholder has gone, for a field with no
    // label on show: a screen reader would otherwise say only what is in it.
    name: String? = null,
    placeholder: @Composable () -> Unit,
): androidx.compose.foundation.text.input.TextFieldDecorator =
    androidx.compose.foundation.text.input.TextFieldDecorator { inner ->
        // Both take the field's whole width, so a centred title and its placeholder line up.
        Box(contentAlignment = Alignment.CenterStart, propagateMinConstraints = true) {
            if (state.text.isEmpty()) {
                placeholder()
            } else if (name != null) {
                // Draws nothing; it only names the field.
                Box(Modifier.semantics { text = androidx.compose.ui.text.AnnotatedString(name) })
            }
            inner()
        }
    }

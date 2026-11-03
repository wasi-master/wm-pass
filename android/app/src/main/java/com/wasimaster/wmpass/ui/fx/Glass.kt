package com.wasimaster.wmpass.ui.fx

import android.os.Build
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Stable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.BlurEffect
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.graphics.TileMode
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.layer.GraphicsLayer
import androidx.compose.ui.graphics.layer.drawLayer
import androidx.compose.ui.graphics.rememberGraphicsLayer
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.theme.Palette
import com.wasimaster.wmpass.ui.theme.palette

/**
 * What glass sees through it. The page draws into this layer and glass above the page draws
 * the same layer blurred. It works without redrawing the glass each frame because a layer is
 * a display list: when the page changes, everything that refers to it shows the change.
 */
@Stable
class Backdrop(val layer: GraphicsLayer) {
    var origin by mutableStateOf(Offset.Zero)
}

val LocalBackdrop = compositionLocalOf<Backdrop?> { null }

@Composable
fun rememberBackdrop(): Backdrop? {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.S || !FxSupport.blur) return null
    val layer = rememberGraphicsLayer()
    return remember(layer) { Backdrop(layer) }
}

/**
 * Marks [content] as what the glass above it blurs. Glass inside [content] gets no backdrop and
 * shows its tint: a pane drawn into the layer it blurs would draw itself, and the render thread
 * recurses until it runs out of stack.
 */
@Composable
fun BackdropSource(backdrop: Backdrop?, modifier: Modifier = Modifier, content: @Composable () -> Unit) {
    Box(modifier.backdropSource(backdrop)) {
        CompositionLocalProvider(LocalBackdrop provides null, content = content)
    }
}

private fun Modifier.backdropSource(backdrop: Backdrop?): Modifier {
    if (backdrop == null) return this
    return onGloballyPositioned { backdrop.origin = it.positionInRoot() }
        .drawWithContent {
            backdrop.layer.record { this@drawWithContent.drawContent() }
            drawLayer(backdrop.layer)
        }
}

/**
 * Frosted glass with a lit rim. Falls back to a solid tint below Android 12, where the same
 * shape and rim still read as a floating pane.
 */
fun Modifier.glass(shape: Shape, blur: Dp = 28.dp, tint: Color? = null, rim: Boolean = true): Modifier = composed {
    val colors = palette
    if (colors.flat) return@composed clip(shape).background(colors.surface2)
    val backdrop = LocalBackdrop.current
    val blurPx = with(LocalDensity.current) { blur.toPx() }
    val blurLayer = if (backdrop != null) rememberGraphicsLayer() else null
    var at by remember { mutableStateOf(Offset.Zero) }
    val paint = tint ?: colors.glassTint
    this
        .onGloballyPositioned { at = it.positionInRoot() }
        .clip(shape)
        .drawBehind {
            if (backdrop != null && blurLayer != null) {
                blurLayer.renderEffect = BlurEffect(blurPx, blurPx, TileMode.Clamp)
                blurLayer.record(size = androidx.compose.ui.unit.IntSize(size.width.toInt(), size.height.toInt())) {
                    translate(backdrop.origin.x - at.x, backdrop.origin.y - at.y) {
                        drawLayer(backdrop.layer)
                    }
                }
                drawLayer(blurLayer)
                drawRect(paint)
            } else {
                // No blur to hide the page through: the tint carries the whole pane.
                drawRect(paint.copy(alpha = 1f))
            }
        }
        .then(if (rim) Modifier.rim(shape, colors) else Modifier)
}

/** The hairline lit from above that gives every pane its edge. */
fun Modifier.rim(shape: Shape, colors: Palette, width: Dp = 1.dp): Modifier = if (colors.flat) {
    this
} else {
    border(width, Brush.verticalGradient(listOf(colors.rimTop, colors.rimBottom)), shape)
}

/** A solid card on the page. */
@Composable
fun Modifier.card(shape: Shape, fill: Color = palette.surface1): Modifier {
    val colors = palette
    return clip(shape).background(fill).rim(shape, colors)
}

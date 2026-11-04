package com.wasimaster.wmpass.ui.theme

import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentWidth
import androidx.compose.material3.adaptive.currentWindowAdaptiveInfo
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalWindowInfo
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.window.core.layout.WindowSizeClass

/**
 * What the window the app is drawn in allows: a phone held upright, a phone on its side, an
 * unfolded book-style foldable, a tablet, or a free-form window on a desktop. Measured from the
 * window, never the screen, so split screen and a resized window get the layout that fits them.
 */
@Immutable
data class WindowShape(
    /** The window's width. */
    val width: Dp,
    /** The window's height. */
    val height: Dp,
    /**
     * At least Material's medium width (600dp): the tabs move from the bottom to a rail at the
     * side, where a thumb holding a tablet or an open foldable reaches them.
     */
    val rail: Boolean,
    /**
     * At least Material's expanded width (840dp): the list and the item it opened sit side by
     * side, as on a tablet held sideways or a book-style foldable unfolded. Not on a window as
     * short as a phone on its side: each half would be a sliver of a phone's screen.
     */
    val panes: Boolean,
    /**
     * A fold or hinge running top to bottom that splits the window in two, from its left edge
     * to its right, in dp from the window's left. Nothing is laid across it.
     */
    val hinge: ClosedFloatingPointRange<Float>?,
) {
    /** Shorter than a phone is wide: a phone on its side, or a short window. */
    val short: Boolean get() = height < ShortHeight

    companion object {
        /** A phone held upright, for anything drawn before the window has been measured. */
        val Phone = WindowShape(393.dp, 852.dp, rail = false, panes = false, hinge = null)
    }
}

val LocalWindowShape = staticCompositionLocalOf { WindowShape.Phone }

/** The window's shape now. Read through [LocalWindowShape]; [PassTheme] provides it. */
@Composable
fun rememberWindowShape(): WindowShape {
    val info = currentWindowAdaptiveInfo()
    val density = LocalDensity.current
    val size = LocalWindowInfo.current.containerSize
    val classes = info.windowSizeClass
    val hinges = info.windowPosture.hingeList
    return remember(size, density, classes, hinges) {
        val hinge = hinges.firstOrNull { it.isSeparating && it.isVertical }?.let { h ->
            with(density) { h.bounds.left.toDp().value..h.bounds.right.toDp().value }
        }
        with(density) {
            WindowShape(
                width = size.width.toDp(),
                height = size.height.toDp(),
                rail = classes.isWidthAtLeastBreakpoint(WindowSizeClass.WIDTH_DP_MEDIUM_LOWER_BOUND),
                panes = classes.isWidthAtLeastBreakpoint(WindowSizeClass.WIDTH_DP_EXPANDED_LOWER_BOUND) && size.height.toDp() >= ShortHeight,
                hinge = hinge,
            )
        }
    }
}

/**
 * How wide a page of reading or form filling gets. Past this a line of text is too long to
 * follow back to its start, and a field's label sits a hand's width from what it labels. The
 * same width the settings pages stop at.
 */
val ReadableWidth = 720.dp

/** Fills the width up to [max], centred, so a page on a tablet is a column and not a banner. */
fun Modifier.readableWidth(max: Dp = ReadableWidth): Modifier =
    fillMaxWidth().wrapContentWidth(Alignment.CenterHorizontally).widthIn(max = max)

/** Below this a window is short: a phone on its side, or a window squeezed flat. */
private val ShortHeight = 480.dp

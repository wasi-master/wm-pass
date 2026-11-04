package com.wasimaster.wmpass.ui.vault

import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.layout.positionInWindow
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.coerceIn
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.LocalWindowShape
import com.wasimaster.wmpass.ui.theme.palette

/** Whether the page showing now has the item it opens beside it, for the window it is in. */
@Composable
internal fun showsPanes(): Boolean {
    val shell = LocalShell.current
    return LocalWindowShape.current.panes && shell.tab in PaneTabs
}

/**
 * A list with the item it opened beside it, for a window wide enough for both (Material's
 * list-detail layout). The list keeps a phone's width, which its rows are drawn for, and the
 * item takes the rest. When the window is folded down the middle, the split follows the fold, so
 * no row or field is drawn across it.
 *
 * With [panes] off, [list] has the whole width and nothing else is drawn.
 */
@Composable
internal fun ListAndItem(panes: Boolean, modifier: Modifier = Modifier, list: @Composable () -> Unit) {
    val shell = LocalShell.current
    val colors = palette
    if (!panes) {
        Box(modifier) { list() }
        return
    }
    // Back closes the item first; search and a selection, drawn over both, close before it.
    BackHandler(shell.paneEntry != null && !shell.searching && !shell.selecting && !shell.adding) { shell.paneEntry = null }
    val shape = LocalWindowShape.current
    val density = LocalDensity.current
    // Where this row starts in the window: a hinge is measured from the window's edge, and a
    // rail or a cutout may come before this row.
    var start by remember { mutableFloatStateOf(0f) }
    BoxWithConstraints(modifier.onPlaced { start = with(density) { it.positionInWindow().x.toDp().value } }) {
        val hinge = shape.hinge
        val (listWidth, gap) = if (hinge != null && hinge.start - start > 280f && maxWidth.value - (hinge.endInclusive - start) > 280f) {
            (hinge.start - start).dp to (hinge.endInclusive - hinge.start).dp
        } else {
            (maxWidth * 0.4f).coerceIn(360.dp, 440.dp) to 0.dp
        }
        Row(Modifier.fillMaxSize()) {
            Box(Modifier.width(listWidth).fillMaxHeight()) { list() }
            if (gap > 0.dp) {
                Box(Modifier.width(gap).fillMaxHeight().background(colors.ground))
            } else {
                Box(Modifier.width(1.dp).fillMaxHeight().background(colors.hairline))
            }
            Box(Modifier.weight(1f).fillMaxHeight().background(colors.ground)) {
                ItemPane()
            }
        }
    }
}

/** The side of [ListAndItem] that shows the item, or says how to pick one. */
@Composable
private fun ItemPane() {
    val shell = LocalShell.current
    val reduce = LocalReduceMotion.current
    AnimatedContent(
        targetState = shell.paneEntry,
        transitionSpec = {
            if (reduce) {
                fadeIn(tween(0)) togetherWith fadeOut(tween(0))
            } else {
                fadeIn(tween(220, 60)) togetherWith fadeOut(tween(120))
            }
        },
        label = "pane",
    ) { id ->
        // Nothing flies between the row and the pane: both stay on screen, so there is no page
        // for the row's tile to travel to.
        CompositionLocalProvider(LocalRouteScope provides null) {
            if (id == null) NothingPicked() else EntryScreen(id, inPane = true)
        }
    }
}

@Composable
private fun NothingPicked() {
    val colors = palette
    Column(
        Modifier.fillMaxSize().padding(32.dp),
        verticalArrangement = Arrangement.Center,
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Icon(PassIcons.Shield, null, tint = colors.textLow, modifier = Modifier.size(56.dp))
        Text(
            "Pick an item to see it here",
            color = colors.textMid,
            style = MaterialTheme.typography.titleMedium,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 16.dp).widthIn(max = 320.dp),
        )
        Text(
            "Its fields, codes and history open beside the list, and the list stays where it was.",
            color = colors.textLow,
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
            modifier = Modifier.padding(top = 6.dp).widthIn(max = 320.dp),
        )
    }
}

/** The width of the side rail's column, rail and margins included. */
internal val RailWidth: Dp = 92.dp

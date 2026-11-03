package com.wasimaster.wmpass.ui.help

import androidx.activity.compose.BackHandler
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.VectorConverter
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.material3.Button
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameNanos
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.boundsInRoot
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalLayoutDirection
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.toSize
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.AllTabs
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.Shell
import kotlin.math.max
import kotlin.math.min
import kotlin.math.roundToInt

/**
 * Places on screen a tour can point at, by the ids the guide's tours name. Each is the layout's
 * own coordinates, read only while a tour is running, so marking a place costs nothing the rest of
 * the time.
 */
object HelpAnchors {
    class Spot(val requester: BringIntoViewRequester) {
        var coordinates: LayoutCoordinates? = null
    }

    private val spots = HashMap<String, ArrayList<Spot>>()

    internal fun add(id: String, spot: Spot) {
        spots.getOrPut(id) { ArrayList() }.add(spot)
    }

    internal fun remove(id: String, spot: Spot) {
        val list = spots[id] ?: return
        list.remove(spot)
        if (list.isEmpty()) spots.remove(id)
    }

    /** The newest place marked [id] that is drawn now. During a page change that is the page arriving. */
    fun find(id: String): Spot? = spots[id]?.lastOrNull { spot ->
        spot.coordinates?.let { it.isAttached && it.size.width > 0 && it.size.height > 0 } == true
    }
}

/** Marks this part of the screen as [id], for a tour to point at and scroll to. */
@Composable
fun Modifier.helpAnchor(id: String): Modifier {
    val requester = remember { BringIntoViewRequester() }
    val spot = remember(requester) { HelpAnchors.Spot(requester) }
    DisposableEffect(id, spot) {
        HelpAnchors.add(id, spot)
        onDispose { HelpAnchors.remove(id, spot) }
    }
    return bringIntoViewRequester(requester).onPlaced { spot.coordinates = it }
}

/** Where a step's part lives, for a step that does not say: the page to bring up before looking. */
private fun homeOf(anchor: String): String? = when {
    anchor.startsWith("group:") -> "settings/" + anchor.removePrefix("group:").substringBefore('.')
    anchor.startsWith("list.") || anchor.startsWith("sidebar.") || anchor == "narrow.tabs" -> "vault"
    anchor.startsWith("codes.") -> "codes"
    anchor.startsWith("generator.") -> "generator"
    anchor.startsWith("health.") -> "health"
    anchor.startsWith("settings.") -> "settings"
    else -> null
}

/** Brings up a page of the vault: a settings page by its route, or one of the home tabs. */
private fun go(shell: Shell, where: String) {
    val route = shell.nav.currentBackStackEntry?.destination?.route
    if (where.startsWith("settings")) {
        if (route != where) shell.nav.navigate(where) { launchSingleTop = true }
        return
    }
    val tab = if (where == "all") "vault" else where
    if (AllTabs.none { it.id == tab }) return
    if (route != "home") shell.nav.popBackStack("home", inclusive = false)
    shell.searching = false
    shell.adding = false
    shell.tab = tab
}

/**
 * A tour: the screen dims except for one part of it, and a note beside that part says what it is.
 * The dim is one layer with a hole cleared out of it, and four bands around the hole take the taps,
 * so on a step meant to be tried the part in the hole still answers. A step whose part is not on
 * screen here, such as the lock button someone has hidden, is skipped. Nothing moves except the
 * hole, from one part to the next, and not even that with animations off.
 */
@Composable
fun Coach(prefs: Prefs) {
    val tour = Guide.tour ?: return
    val shell = LocalShell.current
    val colors = palette
    val reduce = LocalReduceMotion.current
    val density = LocalDensity.current
    val index = Guide.step
    val step = tour.steps.getOrNull(index)
    if (step == null) {
        LaunchedEffect(tour.id) { Guide.endTour(prefs, true) }
        return
    }
    val last = index >= tour.steps.lastIndex
    var forward by remember(tour.id) { mutableStateOf(true) }
    var origin by remember { mutableStateOf(Offset.Zero) }
    var view by remember { mutableStateOf(Size.Zero) }
    var found by remember(tour.id, index) { mutableStateOf<HelpAnchors.Spot?>(null) }
    var target by remember(tour.id, index) { mutableStateOf<Rect?>(null) }
    val hole = remember(tour.id) { Animatable(Rect.Zero, Rect.VectorConverter) }
    var shown by remember(tour.id) { mutableStateOf(false) }

    fun finish(done: Boolean) = Guide.endTour(prefs, done)
    fun move(by: Int) {
        val to = index + by
        forward = by > 0
        when {
            to < 0 -> Unit
            to > tour.steps.lastIndex -> finish(true)
            else -> Guide.step = to
        }
    }

    BackHandler { finish(false) }

    // Each step: bring up its page, wait for its part to be drawn there, and scroll it into view.
    LaunchedEffect(tour.id, index) {
        val where = step.before?.go ?: homeOf(step.anchor).takeIf { HelpAnchors.find(step.anchor) == null }
        if (where != null) go(shell, where)
        if (step.before?.open == "sample" && shell.nav.currentBackStackEntry?.destination?.route?.startsWith("entry/") != true) {
            Pass.entries.value.firstOrNull()?.let { shell.open(it.id) }
        }
        if (step.before?.open == "editor" && shell.nav.currentBackStackEntry?.destination?.route?.startsWith("new/") != true) {
            shell.nav.navigate("new/login")
        }
        // A page change takes a third of a second, and the groups below the fold on a settings
        // page are drawn a frame at a time after it, so the wait is longer than the desktop's.
        val until = System.nanoTime() + 1_500_000_000L
        var spot: HelpAnchors.Spot? = null
        while (spot == null && System.nanoTime() < until) {
            withFrameNanos { }
            spot = HelpAnchors.find(step.anchor)
        }
        if (spot == null) {
            // Not on this screen. Keep going the way the person was going.
            if (forward) move(1) else if (index > 0) move(-1) else move(1)
            return@LaunchedEffect
        }
        runCatching { spot.requester.bringIntoView() }
        found = spot
    }

    // Follows the part while the step lasts: a list scrolling under it, a bar folding away.
    LaunchedEffect(found) {
        val spot = found ?: return@LaunchedEffect
        while (true) {
            withFrameNanos { }
            val placed = spot.coordinates?.takeIf { it.isAttached } ?: continue
            val box = placed.boundsInRoot().translate(-origin)
            if (box != target) target = box
        }
    }

    // From one part to the next the hole slides; while one part moves, it simply follows.
    var holeStep by remember(tour.id) { mutableStateOf(-1) }
    LaunchedEffect(target) {
        val box = target ?: return@LaunchedEffect
        val pad = with(density) { 6.dp.toPx() }
        val padded = Rect(box.left - pad, box.top - pad, box.right + pad, box.bottom + pad)
        if (!shown || reduce || holeStep == index) {
            hole.snapTo(padded)
        } else {
            hole.animateTo(padded, tween(240))
        }
        holeStep = index
        shown = true
    }

    val scrim = Color.Black.copy(alpha = if (colors.dark) 0.62f else 0.5f)
    val ring = colors.primary
    val corner = with(density) { 12.dp.toPx() }
    val ready = shown && target != null && found != null
    Box(
        Modifier
            .fillMaxSize()
            .onGloballyPositioned {
                origin = it.positionInRoot()
                view = it.size.toSize()
            },
    ) {
        val rect = if (ready) hole.value else null
        Box(
            Modifier
                .fillMaxSize()
                .graphicsLayer(compositingStrategy = CompositingStrategy.Offscreen)
                .drawBehind {
                    drawRect(scrim)
                    if (rect != null) {
                        drawRoundRect(Color.Black, rect.topLeft, rect.size, CornerRadius(corner), blendMode = BlendMode.Clear)
                        drawRoundRect(ring, rect.topLeft, rect.size, CornerRadius(corner), style = Stroke(2.dp.toPx()))
                    }
                },
        )
        if (rect == null) {
            Blocker(Rect(Offset.Zero, view))
        } else {
            val clear = rect.intersect(Rect(Offset.Zero, view))
            Blocker(Rect(0f, 0f, view.width, clear.top))
            Blocker(Rect(0f, clear.bottom, view.width, view.height))
            Blocker(Rect(0f, clear.top, clear.left, clear.bottom))
            Blocker(Rect(clear.right, clear.top, view.width, clear.bottom))
            if (!step.tryIt) Blocker(clear)
            CardPlacer(rect, step.placement) {
                CoachCard(tour, step, index, last, onBack = { move(-1) }, onNext = { move(1) }, onEnd = { finish(false) })
            }
        }
    }
}

/** Takes every touch in [area], so the dimmed screen behind it cannot be used during a step. */
@Composable
private fun Blocker(area: Rect) {
    if (area.width <= 0f || area.height <= 0f) return
    val density = LocalDensity.current
    Box(
        Modifier
            .offset { IntOffset(area.left.roundToInt(), area.top.roundToInt()) }
            .size(with(density) { area.width.toDp() }, with(density) { area.height.toDp() })
            .pointerInput(Unit) {
                awaitEachGesture {
                    awaitFirstDown(requireUnconsumed = false).consume()
                    do {
                        val event = awaitPointerEvent()
                        event.changes.forEach { it.consume() }
                    } while (event.changes.any { it.pressed })
                }
            },
    )
}

/** Puts the note beside the hole, on whichever side has room, inside the screen's safe area. */
@Composable
private fun CardPlacer(hole: Rect, placement: String?, card: @Composable () -> Unit) {
    val density = LocalDensity.current
    val direction = LocalLayoutDirection.current
    val insets = WindowInsets.safeDrawing
    Layout(card, Modifier.fillMaxSize()) { measurables, constraints ->
        val margin = 12.dp.roundToPx()
        val gap = 12.dp.toPx()
        val width = min(constraints.maxWidth - margin * 2, 360.dp.roundToPx())
        val placeable = measurables.first().measure(Constraints(maxWidth = width.coerceAtLeast(0)))
        val w = placeable.width.toFloat()
        val h = placeable.height.toFloat()
        val viewW = constraints.maxWidth.toFloat()
        val viewH = constraints.maxHeight.toFloat()
        val safeTop = (insets.getTop(density) + margin).toFloat()
        val safeBottom = viewH - insets.getBottom(density) - margin
        val safeLeft = insets.getLeft(density, direction) + margin.toFloat()
        val safeRight = viewW - insets.getRight(density, direction) - margin
        val room = mapOf(
            "bottom" to safeBottom - (hole.bottom + gap),
            "top" to (hole.top - gap) - safeTop,
            "right" to safeRight - (hole.right + gap),
            "left" to (hole.left - gap) - safeLeft,
        )
        fun fits(side: String) = if (side == "top" || side == "bottom") room.getValue(side) >= h else room.getValue(side) >= w
        val side = listOf(placement ?: "bottom", "bottom", "top", "right", "left").firstOrNull(::fits)
        fun clampX(x: Float) = x.coerceIn(safeLeft, max(safeLeft, safeRight - w))
        fun clampY(y: Float) = y.coerceIn(safeTop, max(safeTop, safeBottom - h))
        val at = when (side) {
            "bottom" -> Offset(clampX(hole.left), clampY(hole.bottom + gap))
            "top" -> Offset(clampX(hole.left), clampY(hole.top - gap - h))
            "right" -> Offset(clampX(hole.right + gap), clampY(hole.top))
            "left" -> Offset(clampX(hole.left - gap - w), clampY(hole.top))
            // A part taller than the screen leaves no side free: the note goes over its foot.
            else -> Offset(clampX(hole.left), clampY(safeBottom - h))
        }
        layout(constraints.maxWidth, constraints.maxHeight) { placeable.place(at.x.roundToInt(), at.y.roundToInt()) }
    }
}

@Composable
private fun CoachCard(tour: HelpTour, step: HelpStep, index: Int, last: Boolean, onBack: () -> Unit, onNext: () -> Unit, onEnd: () -> Unit) {
    val colors = palette
    Surface(
        color = if (colors.flat) MaterialTheme.colorScheme.surfaceContainerHigh else colors.surface2,
        contentColor = colors.text,
        shape = if (colors.flat) MaterialTheme.shapes.large else Shapes.card,
        shadowElevation = if (colors.flat) 6.dp else 0.dp,
        modifier = Modifier.semantics {
            paneTitle = tour.title
            liveRegion = LiveRegionMode.Polite
        },
    ) {
        Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 12.dp, bottom = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Row(verticalAlignment = Alignment.Top) {
                Column(Modifier.weight(1f).padding(top = 4.dp)) {
                    Text("${tour.title} · ${index + 1} of ${tour.steps.size}", color = colors.textMid, style = MaterialTheme.typography.labelMedium)
                    Text(step.title, color = colors.text, style = MaterialTheme.typography.titleMedium, modifier = Modifier.padding(top = 2.dp))
                }
                IconButton(onClick = onEnd) { Icon(PassIcons.Close, "End the tour", tint = colors.textMid, modifier = Modifier.size(20.dp)) }
            }
            RichText(step.body, Modifier.padding(end = 8.dp))
            if (step.tryIt) {
                Text("Go ahead and try it. The tour waits.", color = colors.textLow, style = MaterialTheme.typography.bodySmall)
            }
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End, verticalAlignment = Alignment.CenterVertically) {
                if (index > 0) TextButton(onClick = onBack) { Text("Back") }
                Spacer(Modifier.width(4.dp))
                Button(onClick = onNext) { Text(if (last) "Done" else "Next") }
            }
        }
    }
}

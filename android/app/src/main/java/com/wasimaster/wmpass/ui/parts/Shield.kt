package com.wasimaster.wmpass.ui.parts

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.Matrix
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathMeasure
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.PathParser
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.palette
import kotlin.math.max

/** The logo's geometry, in its 108-unit box. Same numbers as `tools/icon/gen_icon.py`. */
object ShieldGeometry {
    private const val SHIELD = "M31.4,45 H76.6 A6.4,6.4 0 0 1 83,51.4 V73 C83,85 67,93 54,99 C41,93 25,85 25,73 V51.4 A6.4,6.4 0 0 1 31.4,45 Z"
    private const val KEYHOLE = "M52.7,80.36 A3.6,3.6 0 1 1 55.3,80.36 L56,86.6 H52 Z"
    private const val PIN = "M43,17.5 H65 A9.5,9.5 0 0 1 74.5,27 V41 A9.5,9.5 0 0 1 65,50.5 H58.9 L54,60 L49.1,50.5 H43 A9.5,9.5 0 0 1 33.5,41 V27 A9.5,9.5 0 0 1 43,17.5 Z"
    private const val PIN_HALO = "M43,15.9 H65 A11.1,11.1 0 0 1 76.1,27 V41 A11.1,11.1 0 0 1 65,52.1 H60.58 L54,63.2 L47.42,52.1 H43 A11.1,11.1 0 0 1 31.9,41 V27 A11.1,11.1 0 0 1 43,15.9 Z"
    private const val WM = "M45.63,39.30 L40.06,35.51V26.13L42.81 28.36V34.05L45.51 35.86V28.50L48.33 26.51V35.89L51.13 34.02V28.28L53.95 26.09V35.45L48.26 39.30Z M54.79,27.09H66.77Q67.94 27.38 67.94 28.59V36.68L64.55 38.76V29.58H63.13V36.55L59.60 38.90V29.58H58.18V38.49L54.79 40.71Z"

    private fun parse(data: String): Path = PathParser().parsePathString(data).toPath()

    val shield: Path by lazy { parse(SHIELD) }
    val keyhole: Path by lazy { parse(KEYHOLE) }
    val pin: Path by lazy { parse(PIN) }
    val pinHalo: Path by lazy { parse(PIN_HALO) }
    val monogram: Path by lazy { parse(WM) }

    /** The whole mark's box, for centring it. */
    const val LEFT = 25f
    const val TOP = 15.9f
    const val RIGHT = 83f
    const val BOTTOM = 99f

    /** The shield alone, centred on the origin and one unit across at scale 1. */
    fun maskPath(centre: Offset, width: Float): Path {
        val scale = width / (RIGHT - LEFT)
        val path = Path()
        path.addPath(shield)
        path.transform(
            Matrix().apply {
                translate(centre.x, centre.y)
                scale(scale, scale)
                translate(-54f, -72f)
            },
        )
        return path
    }
}

/** What the door is doing. */
enum class ShieldState { IDLE, WORKING, OPENED, REFUSED }

/**
 * The logo as a living thing. Idle, it breathes. While the key derives, a comet runs round its
 * outline, which is a true reading: it runs for as long as the hash does. It fills when the
 * vault opens, and it shakes red when it does not.
 */
@Composable
fun ShieldMark(state: ShieldState, modifier: Modifier = Modifier, typed: Int = 0) {
    val colors = palette
    val reduce = LocalReduceMotion.current
    val flat = colors.flat
    // The standard look's logo is the logo: its own colours, still until there is work to show.
    val markFrom = if (flat) Color(0xFF10B981) else colors.brandFrom
    val markTo = if (flat) Color(0xFF0D9488) else colors.brandTo
    val loop = rememberInfiniteTransition(label = "shield")
    val breathe by if (flat) {
        remember { androidx.compose.runtime.mutableFloatStateOf(0f) }
    } else {
        loop.animateFloat(0f, 1f, infiniteRepeatable(tween(3600, easing = FastOutSlowInEasing), RepeatMode.Reverse), label = "breathe")
    }
    val orbit by if (flat && state != ShieldState.WORKING) {
        remember { androidx.compose.runtime.mutableFloatStateOf(0f) }
    } else {
        loop.animateFloat(0f, 1f, infiniteRepeatable(tween(1100, easing = LinearEasing)), label = "orbit")
    }

    val shake = remember { Animatable(0f) }
    val fill = remember { Animatable(0f) }
    val refuse = remember { Animatable(0f) }
    val pulse = remember { Animatable(0f) }
    LaunchedEffect(state) {
        when (state) {
            ShieldState.REFUSED -> {
                refuse.snapTo(1f)
                if (!reduce) {
                    for (step in listOf(-1f, 0.9f, -0.7f, 0.5f, -0.3f, 0.15f, 0f)) {
                        shake.animateTo(step, tween(55, easing = FastOutSlowInEasing))
                    }
                }
                refuse.animateTo(0f, tween(700))
            }
            ShieldState.OPENED -> fill.animateTo(1f, tween(260))
            else -> {
                refuse.snapTo(0f)
                fill.snapTo(0f)
            }
        }
    }
    // Each keystroke sends one soft ring outwards. It says "heard you" without counting.
    LaunchedEffect(typed) {
        if (typed > 0 && !reduce) {
            pulse.snapTo(1f)
            pulse.animateTo(0f, tween(420))
        }
    }

    val measure = remember { PathMeasure().apply { setPath(ShieldGeometry.shield, false) } }
    val segment = remember { Path() }
    val shakePx = shake
    Box(modifier, contentAlignment = Alignment.Center) {
        // The light under the mark. Its own canvas, because the mark's offscreen layer clips
        // to its bounds and a glow has to reach well past them.
        if (!flat) Canvas(Modifier.fillMaxSize()) {
            val glow = 0.34f + 0.12f * breathe + 0.45f * pulse.value + 0.45f * fill.value
            val tone = if (refuse.value > 0f) colors.danger else colors.brandFrom
            val centre = Offset(size.width / 2, size.height * 0.68f)
            val reach = size.width * 1.25f
            drawCircle(
                Brush.radialGradient(
                    0f to tone.copy(alpha = glow.coerceAtMost(1f)),
                    0.45f to tone.copy(alpha = glow.coerceAtMost(1f) * 0.28f),
                    1f to Color.Transparent,
                    center = centre,
                    radius = reach,
                ),
                radius = reach,
                center = centre,
            )
        }
        Canvas(
            Modifier
                .fillMaxSize()
                .graphicsLayer {
                    compositingStrategy = CompositingStrategy.Offscreen
                    translationX = shakePx.value * 12f * density
                },
        ) {
            val scale = max(size.width / 108f, 0f)
        withTransform({ scale(scale, scale, Offset.Zero) }) {
            val body = if (refuse.value > 0.01f) {
                Brush.linearGradient(
                    listOf(colors.danger, colors.danger.copy(alpha = 0.75f)),
                    Offset(54f, 45f),
                    Offset(54f, 99f),
                )
            } else {
                Brush.linearGradient(listOf(markFrom, markTo), Offset(54f, 45f), Offset(54f, 99f))
            }
            drawPath(ShieldGeometry.shield, body)
            drawPath(ShieldGeometry.keyhole, Color.Black, blendMode = BlendMode.Clear)
            drawPath(ShieldGeometry.pinHalo, Color.Black, blendMode = BlendMode.Clear)
            drawPath(ShieldGeometry.pin, Color.White)
            drawPath(ShieldGeometry.monogram, Color.Black, blendMode = BlendMode.Clear)
            if (fill.value > 0f) {
                drawPath(ShieldGeometry.shield, Color.White.copy(alpha = fill.value * 0.9f))
            }
            if (state == ShieldState.WORKING) {
                comet(measure, segment, orbit, colors.text, glow = !flat)
            }
        }
    }
    }
}

private fun DrawScope.comet(measure: PathMeasure, segment: Path, phase: Float, color: Color, glow: Boolean) {
    val length = measure.length
    val tail = length * 0.28f
    val head = phase * length
    // The tail is drawn as a run of short pieces, fading towards its end, with two wider
    // passes under them for the glow.
    val pieces = 14
    for (i in 0 until pieces) {
        val from = head - tail * (i + 1) / pieces
        val to = head - tail * i / pieces
        val alpha = (1f - i.toFloat() / pieces)
        segment.reset()
        if (from < 0f) {
            measure.getSegment(from + length, length, segment, true)
            measure.getSegment(0f, to.coerceAtLeast(0f), segment, true)
        } else {
            measure.getSegment(from, to, segment, true)
        }
        if (glow) drawPath(segment, color.copy(alpha = alpha * 0.16f), style = Stroke(3.2f, cap = StrokeCap.Round))
        drawPath(segment, color.copy(alpha = alpha), style = Stroke(1.1f, cap = StrokeCap.Round))
    }
}

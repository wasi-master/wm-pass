package com.wasimaster.wmpass.ui.parts

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableLongStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameMillis
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.Palette
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.palette
import kotlin.random.Random
import kotlinx.coroutines.delay

private const val MASK_LENGTH = 12
private const val MASK_DOT = '•'
private const val POOL = "abcdefghjkmnpqrstuvwxyzABCDEFGHJKLMNPQRSTUVWXYZ23456789#%&*+=?@!$"

/** Letters plain, digits and symbols in their own colours, so a password can be read aloud. */
fun colouredSecret(text: String, colors: Palette, base: Color = colors.text): AnnotatedString = buildAnnotatedString {
    for (ch in text) {
        val color = when {
            ch == MASK_DOT -> colors.textLow
            ch.isDigit() -> colors.digit
            ch.isLetter() -> base
            ch.isWhitespace() -> base
            else -> colors.symbol
        }
        withStyle(SpanStyle(color = color)) { append(ch) }
    }
}

/**
 * A secret on show or under dots. Changing between the two scrambles through random glyphs and
 * settles left to right, so the eye is led along the value. The dots are always the same
 * length: how long the secret is stays private until it is shown.
 */
@Composable
fun SecretText(value: String?, modifier: Modifier = Modifier, style: TextStyle = PassType.secret, maxLines: Int = Int.MAX_VALUE) {
    val colors = palette
    val reduce = LocalReduceMotion.current
    val mask = remember { MASK_DOT.toString().repeat(MASK_LENGTH) }
    val target = value ?: mask
    var shown by remember { mutableStateOf(target) }
    LaunchedEffect(target) {
        if (shown == target) return@LaunchedEffect
        if (reduce || colors.flat) {
            shown = target
            return@LaunchedEffect
        }
        val from = shown
        val start = withFrameMillis { it }
        val stepMs = 22L
        val settleBase = 110L
        val total = settleBase + stepMs * target.length
        var lastSwap = -100L
        while (true) {
            val elapsed = withFrameMillis { it } - start
            if (elapsed >= total) break
            if (elapsed - lastSwap >= 38) {
                lastSwap = elapsed
                val length = (from.length + (target.length - from.length) * (elapsed.toFloat() / total)).toInt()
                    .coerceAtLeast(1)
                shown = buildString {
                    for (i in 0 until length) {
                        val settled = i < target.length && elapsed > settleBase + stepMs * i
                        append(
                            when {
                                settled -> target[i]
                                value == null -> if (Random.nextFloat() < 0.5f) MASK_DOT else POOL.random()
                                else -> POOL.random()
                            },
                        )
                    }
                }
            }
        }
        shown = target
    }
    val prefs = com.wasimaster.wmpass.app.Prefs.of(androidx.compose.ui.platform.LocalContext.current)
    if (prefs.colourSecrets.value) {
        Text(colouredSecret(shown, colors), modifier, style = style, maxLines = maxLines)
    } else {
        Text(shown, modifier, color = colors.text, style = style, maxLines = maxLines)
    }
}

/**
 * A one-time code. When it changes, each digit rolls on its own, a little after the one before,
 * like an odometer.
 */
@Composable
fun RollingCode(code: String, modifier: Modifier = Modifier, style: TextStyle = PassType.code, color: Color = palette.text) {
    val reduce = LocalReduceMotion.current
    val prefs = com.wasimaster.wmpass.app.Prefs.of(androidx.compose.ui.platform.LocalContext.current)
    val group = if (!prefs.codeGroups.value) 0 else if (code.length % 3 == 0) 3 else code.length / 2
    val scale = when (prefs.codeSize.value) {
        "small" -> 0.8f
        "large" -> 1.25f
        else -> 1f
    }
    val sized = style.copy(fontSize = style.fontSize * scale)
    Row(modifier, verticalAlignment = Alignment.CenterVertically) {
        code.forEachIndexed { index, digit ->
            if (index > 0 && group > 0 && index % group == 0) Spacer(Modifier.width(sized.fontSize.value.dp * 0.32f))
            AnimatedContent(
                targetState = digit,
                modifier = Modifier.clipToBounds(),
                transitionSpec = {
                    val delayMs = if (reduce) 0 else index * 34
                    val duration = if (reduce) 0 else 320
                    (
                        slideInVertically(tween(duration, delayMs)) { it } + fadeIn(tween(duration / 2, delayMs))
                        ) togetherWith (
                        slideOutVertically(tween(duration, delayMs)) { -it } + fadeOut(tween(duration / 2, delayMs))
                        )
                },
                label = "digit",
            ) { shown -> Text(shown.toString(), style = sized, color = color) }
        }
    }
}

/**
 * How long a code has left. Its true lifetime, read from the clock on every frame and only
 * redrawn, not recomposed. Amber for the last five seconds.
 */
@Composable
fun LifeRing(periodSeconds: Int, modifier: Modifier = Modifier, size: Dp = 26.dp, stroke: Dp = 3.dp) {
    val colors = palette
    var left by remember { mutableFloatStateOf(1f) }
    LaunchedEffect(periodSeconds) {
        val period = periodSeconds.coerceAtLeast(1) * 1000L
        while (true) {
            withFrameMillis { }
            val now = System.currentTimeMillis()
            left = (period - now % period).toFloat() / period
            delay(33)
        }
    }
    Canvas(modifier.then(Modifier.size(size))) {
        val px = stroke.toPx()
        val inset = px / 2
        val arc = Size(this.size.width - px, this.size.height - px)
        val warn = left * periodSeconds <= 5f
        drawArc(colors.hairline, 0f, 360f, false, Offset(inset, inset), arc, style = Stroke(px))
        drawArc(
            if (warn) colors.warn else colors.primary,
            -90f,
            360f * left,
            false,
            Offset(inset, inset),
            arc,
            style = Stroke(px, cap = StrokeCap.Round),
        )
    }
}

/** The strength bar: five lit segments and how long the password would hold. */
@Composable
fun StrengthBar(score: Int, modifier: Modifier = Modifier, active: Boolean = true) {
    val colors = palette
    val color = when (score) {
        0, 1 -> colors.danger
        2 -> colors.warn
        else -> colors.primary
    }
    Row(modifier, horizontalArrangement = Arrangement.spacedBy(5.dp)) {
        repeat(5) { index ->
            val lit by animateFloatAsState(
                if (active && index <= score) 1f else 0f,
                tween(240, index * 30),
                label = "seg",
            )
            Canvas(Modifier.weight(1f).height(5.dp)) {
                val radius = androidx.compose.ui.geometry.CornerRadius(size.height / 2)
                drawRoundRect(colors.hairline, cornerRadius = radius)
                if (lit > 0f) {
                    drawRoundRect(
                        if (colors.flat) androidx.compose.ui.graphics.SolidColor(color) else Brush.horizontalGradient(listOf(color.copy(alpha = 0.75f), color)),
                        size = Size(size.width * lit, size.height),
                        cornerRadius = radius,
                    )
                }
            }
        }
    }
}

/** The moment a code changes, for rows that fetch their next one. */
@Composable
fun rememberCodeTick(periodSeconds: Int): Long {
    var slot by remember { mutableLongStateOf(0L) }
    LaunchedEffect(periodSeconds) {
        val period = periodSeconds.coerceAtLeast(1) * 1000L
        while (true) {
            val now = System.currentTimeMillis()
            slot = now / period
            delay(period - now % period + 20)
        }
    }
    return slot
}

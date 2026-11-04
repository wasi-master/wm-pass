package com.wasimaster.wmpass.ui.vault

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.palette

/** The dots of a lock pattern written as `2-6-7-1`, as 0 to 8, or empty when it is not one. */
internal fun lockPath(value: String): List<Int> {
    val dots = value.split('-').map { it.trim().toIntOrNull()?.minus(1) }
    if (dots.size < 2 || dots.any { it == null || it !in 0..8 }) return emptyList()
    return dots.filterNotNull().takeIf { it.distinct().size == it.size }.orEmpty()
}

private val DotNames = listOf(
    "top left", "top middle", "top right",
    "middle left", "centre", "middle right",
    "bottom left", "bottom middle", "bottom right",
)

/** The dot a straight line from [a] to [b] passes over, if any. */
private fun between(a: Int, b: Int): Int? {
    val rows = a / 3 + b / 3
    val cols = a % 3 + b % 3
    return if (rows % 2 == 0 && cols % 2 == 0 && a != b) rows / 2 * 3 + cols / 2 else null
}

/** The pattern in words, one stroke a line, so it can be drawn without looking back up. */
internal fun lockSteps(path: List<Int>): List<String> {
    if (path.isEmpty()) return emptyList()
    val steps = mutableListOf("Put your finger on the ${DotNames[path.first()]} dot")
    path.zipWithNext().forEachIndexed { index, (from, to) ->
        val over = between(from, to)?.let { ", passing over the ${DotNames[it]}" }.orEmpty()
        val last = index == path.size - 2
        steps += (if (last) "Finish on the ${DotNames[to]}" else "Slide to the ${DotNames[to]}") + over
    }
    steps += "Lift your finger"
    return steps
}

/**
 * A lock pattern on its grid of nine dots, drawn the way a finger would draw it: the line runs
 * from dot to dot in order, and each dot it reaches carries its number. Tapping replays it.
 */
@Composable
fun LockPatternGrid(value: String, modifier: Modifier = Modifier, size: Dp = 220.dp) {
    val colors = palette
    val path = remember(value) { lockPath(value) }
    val reduce = LocalReduceMotion.current
    var replays by remember { mutableIntStateOf(0) }
    val progress = remember { Animatable(0f) }
    LaunchedEffect(path, replays) {
        val end = (path.size - 1).coerceAtLeast(0).toFloat()
        if (reduce || path.size < 2) {
            progress.snapTo(end)
        } else {
            progress.snapTo(0f)
            // A short pause on the first dot, then an even pace, so the eye can follow the turns.
            kotlinx.coroutines.delay(260)
            progress.animateTo(end, tween(durationMillis = 380 * path.size, easing = LinearEasing))
        }
    }
    val measurer = rememberTextMeasurer()
    val labelStyle = TextStyle(fontSize = 11.sp, fontWeight = FontWeight.Bold, color = colors.ground)
    val description = remember(path) { lockSteps(path).joinToString(". ") }
    Box(
        modifier
            .widthIn(max = size)
            .fillMaxWidth()
            .aspectRatio(1f)
            .clip(androidx.compose.foundation.shape.RoundedCornerShape(24.dp))
            .clickable(onClickLabel = "Replay the pattern") { replays++ }
            .semantics { contentDescription = "Lock pattern. $description" },
    ) {
        Canvas(Modifier.matchParentSize().padding(14.dp)) {
            val cell = this.size.width / 3f
            fun centre(dot: Int) = Offset((dot % 3 + 0.5f) * cell, (dot / 3 + 0.5f) * cell)
            val shown = progress.value
            val reached = if (path.isEmpty()) 0 else shown.toInt() + 1
            val line = colors.primary.copy(alpha = 0.78f)
            val width = 7.dp.toPx()
            // The strokes so far, the last one only part of the way.
            for (i in 0 until path.size - 1) {
                val part = (shown - i).coerceIn(0f, 1f)
                if (part <= 0f) break
                val a = centre(path[i])
                val b = centre(path[i + 1])
                drawLine(line, a, a + (b - a) * part, width, StrokeCap.Round)
            }
            for (dot in 0..8) {
                val order = path.indexOf(dot)
                val on = order in 0 until reached
                val c = centre(dot)
                if (on) {
                    drawCircle(colors.primary.copy(alpha = 0.18f), radius = cell * 0.3f, center = c)
                    drawCircle(colors.primary, radius = 12.dp.toPx(), center = c)
                    val label = measurer.measure((order + 1).toString(), labelStyle)
                    drawText(label, topLeft = c - Offset(label.size.width / 2f, label.size.height / 2f))
                } else {
                    drawCircle(colors.textLow, radius = 5.dp.toPx(), center = c)
                }
                if (order == 0) {
                    drawCircle(colors.primary, radius = cell * 0.3f, center = c, style = Stroke(2.dp.toPx()))
                }
            }
            // The fingertip, while it is still moving.
            if (path.size >= 2 && shown < path.size - 1) {
                val i = shown.toInt().coerceAtMost(path.size - 2)
                val a = centre(path[i])
                val b = centre(path[i + 1])
                drawCircle(colors.text.copy(alpha = 0.22f), radius = 18.dp.toPx(), center = a + (b - a) * (shown - i))
            }
        }
    }
}

/** How to draw it, as numbered steps, and how to put it on the phone. */
@Composable
fun LockPatternGuide(value: String, modifier: Modifier = Modifier) {
    val colors = palette
    val steps = remember(value) { lockSteps(lockPath(value)) }
    if (steps.isEmpty()) return
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("How to draw it", color = colors.text, style = MaterialTheme.typography.titleSmall)
        steps.forEachIndexed { index, step ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(24.dp), contentAlignment = Alignment.Center) {
                    Text("${index + 1}", color = colors.primary, style = MaterialTheme.typography.labelLarge)
                }
                Spacer(Modifier.width(10.dp))
                Text(step, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            }
        }
        Row(Modifier.padding(top = 6.dp), verticalAlignment = Alignment.Top) {
            Icon(PassIcons.Info, null, tint = colors.textLow, modifier = Modifier.padding(top = 2.dp).size(16.dp))
            Spacer(Modifier.width(8.dp))
            Text(
                "To use it, open your phone's screen lock settings, choose Pattern, and draw it twice. Tap the grid to watch it again.",
                color = colors.textLow,
                style = MaterialTheme.typography.bodySmall,
            )
        }
    }
}

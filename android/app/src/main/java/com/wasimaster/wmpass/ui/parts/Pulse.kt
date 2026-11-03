package com.wasimaster.wmpass.ui.parts

import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.RepeatMode
import androidx.compose.animation.core.animateFloat
import androidx.compose.animation.core.infiniteRepeatable
import androidx.compose.animation.core.rememberInfiniteTransition
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import com.google.zxing.BarcodeFormat
import com.google.zxing.EncodeHintType
import com.google.zxing.qrcode.QRCodeWriter
import com.google.zxing.qrcode.decoder.ErrorCorrectionLevel
import com.wasimaster.wmpass.ui.theme.onBrand
import com.wasimaster.wmpass.ui.theme.palette
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Size
import kotlin.math.hypot

/**
 * Something listening: rings that leave a lit centre and fade. Used while waiting for a key to be
 * tapped or for another device to appear.
 */
@Composable
fun Listening(icon: ImageVector, modifier: Modifier = Modifier, size: Dp = 200.dp) {
    val colors = palette
    val time = rememberInfiniteTransition(label = "listening")
    val phase by time.animateFloat(0f, 1f, infiniteRepeatable(tween(2600, easing = LinearEasing), RepeatMode.Restart), label = "phase")
    Box(modifier.size(size), contentAlignment = Alignment.Center) {
        Canvas(Modifier.size(size)) {
            val centre = Offset(this.size.width / 2, this.size.height / 2)
            val inner = 44.dp.toPx()
            val outer = this.size.minDimension / 2
            repeat(3) { index ->
                val t = (phase + index / 3f) % 1f
                drawCircle(
                    colors.primary.copy(alpha = (1f - t) * (1f - t) * 0.6f),
                    inner + (outer - inner) * t,
                    centre,
                    style = Stroke((2.5f - 1.5f * t).dp.toPx()),
                )
            }
            drawCircle(
                Brush.radialGradient(listOf(colors.primary.copy(alpha = 0.25f), Color.Transparent), centre, inner * 1.8f),
                inner * 1.8f,
                centre,
            )
        }
        Box(
            Modifier
                .size(76.dp)
                .clip(CircleShape)
                .background(Brush.linearGradient(listOf(colors.brandFrom, colors.brandTo))),
            contentAlignment = Alignment.Center,
        ) {
            Icon(icon, null, tint = colors.onBrand, modifier = Modifier.size(34.dp))
        }
    }
}

private class QrModules(val size: Int, val on: BooleanArray)

private fun qrOf(text: String): QrModules? = runCatching {
    val hints = mapOf(EncodeHintType.ERROR_CORRECTION to ErrorCorrectionLevel.L, EncodeHintType.MARGIN to 0)
    val matrix = QRCodeWriter().encode(text, BarcodeFormat.QR_CODE, 0, 0, hints)
    QrModules(matrix.width, BooleanArray(matrix.width * matrix.width) { matrix.get(it % matrix.width, it / matrix.width) })
}.getOrNull()

/** Whether [text] fits in a code a phone can read. */
fun qrFits(text: String): Boolean = qrOf(text) != null

/**
 * A QR code that prints itself outward from the centre. Its modules are round and carry the
 * brand's gradient, on a white card, because a scanner needs dark on light in any theme.
 */
@Composable
fun QrCode(text: String, modifier: Modifier = Modifier) {
    val modules = remember(text) { qrOf(text) } ?: return
    val printed = remember(text) { Animatable(0f) }
    LaunchedEffect(text) { printed.animateTo(1f, tween(760, easing = FastOutSlowInEasing)) }
    val dark = Color(0xFF0B3B33)
    val light = Color(0xFF0B6B5C)
    Box(
        modifier
            .fillMaxWidth()
            .aspectRatio(1f)
            .clip(com.wasimaster.wmpass.ui.theme.Shapes.card)
            .background(Color.White)
            .padding(18.dp),
    ) {
        Canvas(Modifier.fillMaxWidth().aspectRatio(1f)) {
            val n = modules.size
            val cell = this.size.width / n
            val middle = (n - 1) / 2f
            val reach = hypot(middle, middle)
            val shader = Brush.linearGradient(listOf(dark, light), Offset.Zero, Offset(this.size.width, this.size.height))
            fun isFinder(x: Int, y: Int) = (x < 7 && y < 7) || (x >= n - 7 && y < 7) || (x < 7 && y >= n - 7)
            for (y in 0 until n) {
                for (x in 0 until n) {
                    if (!modules.on[y * n + x] || isFinder(x, y)) continue
                    val distance = hypot(x - middle, y - middle) / reach
                    val grown = ((printed.value * 1.5f - distance) / 0.5f).coerceIn(0f, 1f)
                    if (grown > 0f) {
                        drawCircle(shader, cell * 0.47f * grown, Offset((x + 0.5f) * cell, (y + 0.5f) * cell))
                    }
                }
            }
            // The three corner marks, drawn whole so that a scanner finds them quickly.
            val fade = (printed.value * 3f).coerceIn(0f, 1f)
            listOf(0 to 0, n - 7 to 0, 0 to n - 7).forEach { (cx, cy) ->
                val at = Offset(cx * cell, cy * cell)
                drawRoundRect(
                    shader,
                    at + Offset(cell / 2, cell / 2),
                    Size(cell * 6, cell * 6),
                    CornerRadius(cell * 1.6f),
                    style = Stroke(cell),
                    alpha = fade,
                )
                drawRoundRect(
                    shader,
                    at + Offset(cell * 2, cell * 2),
                    Size(cell * 3, cell * 3),
                    CornerRadius(cell * 0.9f),
                    alpha = fade,
                )
            }
        }
    }
}

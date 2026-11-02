package com.wasimaster.wmpass.ui.vault

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.SecureClipboard
import com.wasimaster.wmpass.ui.fx.glass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.palette
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

private data class CapsuleContent(val label: String, val startMs: Long, val endMs: Long?)

/**
 * Drops from the top after a copy. For a secret its ring empties until the clipboard is wiped
 * for real, and a tap wipes it now. For a plain value it says so and goes.
 */
@Composable
fun CopyCapsule(modifier: Modifier = Modifier) {
    val colors = palette
    val context = LocalContext.current
    val shell = LocalShell.current
    val copy by SecureClipboard.current.collectAsStateWithLifecycle()
    var plain by remember { mutableStateOf<CapsuleContent?>(null) }
    LaunchedEffect(shell.plainCopied) {
        val copied = shell.plainCopied ?: return@LaunchedEffect
        plain = CapsuleContent(copied.first, copied.second, null)
        delay(1600)
        plain = null
    }
    val secret = copy?.let { CapsuleContent(it.label, it.startedAtMs, it.wipeAtMs) }
    val content = secret ?: plain
    var remembered by remember { mutableStateOf(content) }
    if (content != null) remembered = content
    AnimatedVisibility(
        visible = content != null,
        modifier = modifier.padding(top = 8.dp),
        enter = slideInVertically(com.wasimaster.wmpass.ui.theme.Springs.lively()) { -it * 2 } +
            scaleIn(com.wasimaster.wmpass.ui.theme.Springs.lively(), 0.7f) + fadeIn(tween(120)),
        exit = slideOutVertically(tween(220)) { -it * 2 } + scaleOut(tween(220), 0.8f) + fadeOut(tween(180)),
    ) {
        val shown = remembered ?: return@AnimatedVisibility
        Row(
            Modifier
                .height(44.dp)
                .glass(CircleShape, tint = colors.surface3.copy(alpha = 0.82f))
                .clickable(enabled = shown.endMs != null) { SecureClipboard.wipe(context) }
                .padding(start = 14.dp, end = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (shown.endMs != null) {
                WipeRing(shown.startMs, shown.endMs)
            } else {
                Icon(PassIcons.CheckCircleFilled, null, tint = colors.primary, modifier = Modifier.size(20.dp))
            }
            Spacer(Modifier.width(9.dp))
            Text("${shown.label} copied", color = colors.text, style = MaterialTheme.typography.labelLarge)
            if (shown.endMs != null) {
                Spacer(Modifier.width(10.dp))
                Countdown(shown.endMs)
            }
        }
    }
}

@Composable
private fun WipeRing(startMs: Long, endMs: Long) {
    val colors = palette
    var left by remember { mutableFloatStateOf(1f) }
    LaunchedEffect(startMs, endMs) {
        while (true) {
            left = ((endMs - System.currentTimeMillis()).toFloat() / (endMs - startMs)).coerceIn(0f, 1f)
            delay(33)
        }
    }
    Canvas(Modifier.size(20.dp)) {
        val stroke = 2.4.dp.toPx()
        val box = Size(size.width - stroke, size.height - stroke)
        val at = Offset(stroke / 2, stroke / 2)
        drawArc(colors.hairline, 0f, 360f, false, at, box, style = Stroke(stroke))
        drawArc(colors.primary, -90f, 360f * left, false, at, box, style = Stroke(stroke, cap = StrokeCap.Round))
    }
}

@Composable
private fun Countdown(endMs: Long) {
    val colors = palette
    var seconds by remember { mutableStateOf(0L) }
    LaunchedEffect(endMs) {
        while (true) {
            seconds = ((endMs - System.currentTimeMillis() + 999) / 1000).coerceAtLeast(0)
            delay(200)
        }
    }
    Text("${seconds}s", color = colors.textMid, style = MaterialTheme.typography.labelMedium.merge(PassType.numeric))
}

/** The last change, still reversible for a few seconds. Its bottom edge shows how long. */
@Composable
fun UndoBar(modifier: Modifier = Modifier) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val offer = shell.undo
    var remembered by remember { mutableStateOf(offer) }
    if (offer != null) remembered = offer
    LaunchedEffect(offer) {
        if (offer != null) {
            delay(6000)
            if (shell.undo === offer) shell.undo = null
        }
    }
    AnimatedVisibility(
        visible = offer != null,
        modifier = modifier,
        enter = slideInVertically(com.wasimaster.wmpass.ui.theme.Springs.glide()) { it } + fadeIn(tween(140)),
        exit = slideOutVertically(tween(200)) { it } + fadeOut(tween(160)),
    ) {
        val shown = remembered ?: return@AnimatedVisibility
        Row(
            Modifier
                .fillMaxWidth()
                .height(52.dp)
                .glass(CircleShape, tint = colors.surface3.copy(alpha = 0.86f))
                .padding(start = 20.dp, end = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                shown.message,
                color = colors.text,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 1,
                modifier = Modifier.weight(1f),
            )
            Box(
                Modifier
                    .height(40.dp)
                    .clickable {
                        shell.haptics.click()
                        val action = shown.action
                        shell.undo = null
                        scope.launch { action() }
                    }
                    .padding(horizontal = 16.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text("Undo", color = colors.primary, style = MaterialTheme.typography.labelLarge.copy(fontWeight = FontWeight.Bold))
            }
        }
    }
}

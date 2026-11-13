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
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
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
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.clearAndSetSemantics
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.SecureClipboard
import com.wasimaster.wmpass.ui.fx.glass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.palette
import kotlinx.coroutines.delay

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
    if (colors.flat) {
        AnimatedVisibility(
            visible = content != null,
            modifier = modifier.padding(horizontal = 16.dp),
            enter = fadeIn(tween(120)) + slideInVertically(tween(180)) { it / 3 },
            exit = fadeOut(tween(120)),
        ) {
            val shown = remembered ?: return@AnimatedVisibility
            // Counted from the first frame, so it never opens on "Clears in 0s".
            var seconds by remember(shown.endMs) { mutableStateOf(secondsUntil(shown.endMs ?: 0L)) }
            LaunchedEffect(shown.endMs) {
                val end = shown.endMs ?: return@LaunchedEffect
                while (true) {
                    seconds = secondsUntil(end)
                    delay(250)
                }
            }
            androidx.compose.material3.Snackbar(
                action = if (shown.endMs != null) {
                    { androidx.compose.material3.TextButton(onClick = { SecureClipboard.wipe(context) }) { Text("Clear now") } }
                } else {
                    null
                },
            ) {
                // Said once when it appears; the seconds counting down are left out so TalkBack does not chatter.
                Text(
                    if (shown.endMs != null) "${shown.label} copied. Clears in ${seconds}s." else "${shown.label} copied.",
                    Modifier.clearAndSetSemantics {
                        contentDescription = "${shown.label} copied"
                        liveRegion = LiveRegionMode.Polite
                    },
                )
            }
        }
        return
    }
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
                // A floor, not a height: at large text the label grows the capsule rather than being clipped.
                .heightIn(min = 44.dp)
                .glass(CircleShape, tint = colors.surface3.copy(alpha = 0.82f))
                // A screen reader says what the tap does: the capsule shows no button for it.
                .clickable(enabled = shown.endMs != null, onClickLabel = "Clear the clipboard now") { SecureClipboard.wipe(context) }
                .padding(start = 14.dp, end = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            if (shown.endMs != null) {
                WipeRing(shown.startMs, shown.endMs)
            } else {
                Icon(PassIcons.CheckCircleFilled, null, tint = colors.primary, modifier = Modifier.size(20.dp))
            }
            Spacer(Modifier.width(9.dp))
            // Announced once as it appears; the countdown beside it is not live, so it does not chatter.
            Text(
                "${shown.label} copied",
                Modifier.semantics { liveRegion = LiveRegionMode.Polite },
                color = colors.text,
                style = MaterialTheme.typography.labelLarge,
            )
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
    var seconds by remember(endMs) { mutableStateOf(secondsUntil(endMs)) }
    LaunchedEffect(endMs) {
        while (true) {
            seconds = secondsUntil(endMs)
            delay(200)
        }
    }
    // Kept out of the capsule's spoken text: the capsule is announced as it appears, and a number
    // in it that changes every second would be announced every second.
    Text(
        "${seconds}s",
        Modifier.clearAndSetSemantics { },
        color = colors.textMid,
        style = MaterialTheme.typography.labelMedium.merge(PassType.numeric),
    )
}

/** Whole seconds left until [endMs], rounded up. */
private fun secondsUntil(endMs: Long): Long = ((endMs - System.currentTimeMillis() + 999) / 1000).coerceAtLeast(0)

/** The last change, still reversible for a few seconds. Its bottom edge shows how long. */
@Composable
fun UndoBar(modifier: Modifier = Modifier) {
    val colors = palette
    val shell = LocalShell.current
    val offer = shell.undo
    var remembered by remember { mutableStateOf(offer) }
    if (offer != null) remembered = offer
    // A screen reader user needs longer to find and reach Undo: the system's recommended timeout
    // (Settings, Accessibility, Time to take action) stretches the six seconds.
    val a11y = androidx.compose.ui.platform.LocalAccessibilityManager.current
    LaunchedEffect(offer) {
        if (offer != null) {
            delay(a11y?.calculateRecommendedTimeoutMillis(6000, containsIcons = false, containsText = true, containsControls = offer.action != null) ?: 6000L)
            if (shell.undo === offer) shell.undo = null
        }
    }
    if (colors.flat) {
        AnimatedVisibility(offer != null, modifier.padding(horizontal = 16.dp), enter = fadeIn(tween(120)), exit = fadeOut(tween(120))) {
            val shown = remembered ?: return@AnimatedVisibility
            val action = shown.action
            androidx.compose.material3.Snackbar(
                action = if (action != null) {
                    {
                        androidx.compose.material3.TextButton(onClick = {
                            shell.undo = null
                            shell.launch { action() }
                        }) { Text("Undo") }
                    }
                } else {
                    null
                },
                modifier = Modifier.semantics { liveRegion = LiveRegionMode.Polite },
            ) { Text(shown.message, maxLines = 2) }
        }
        return
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
                .heightIn(min = 52.dp)
                .glass(CircleShape, tint = colors.surface3.copy(alpha = 0.86f))
                // Said as it appears, so a screen reader hears what changed and that Undo is offered.
                .semantics { liveRegion = LiveRegionMode.Polite }
                .padding(start = 20.dp, end = 6.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            // Two lines, as the standard look's bar allows: a long title or a save problem is not cut off mid-word.
            Text(
                shown.message,
                color = colors.text,
                style = MaterialTheme.typography.bodyMedium,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.weight(1f),
            )
            val action = shown.action
            if (action != null) {
                Box(
                    Modifier
                        .heightIn(min = 48.dp)
                        .clickable(role = androidx.compose.ui.semantics.Role.Button) {
                            shell.haptics.click()
                            shell.undo = null
                            shell.launch { action() }
                        }
                        .padding(horizontal = 16.dp),
                    contentAlignment = Alignment.Center,
                ) {
                    Text("Undo", color = colors.primary, style = MaterialTheme.typography.labelLarge.copy(fontWeight = FontWeight.Bold))
                }
            } else {
                Spacer(Modifier.width(14.dp))
            }
        }
    }
}

/** While inside a nested or shared vault: which one, and the way back out. */
@Composable
fun ChildPill(modifier: Modifier = Modifier) {
    val colors = palette
    val shell = LocalShell.current
    val child by com.wasimaster.wmpass.core.Pass.child.collectAsStateWithLifecycle()
    var remembered by remember { mutableStateOf(child) }
    if (child != null) remembered = child
    AnimatedVisibility(
        visible = child != null,
        modifier = modifier,
        enter = slideInVertically(com.wasimaster.wmpass.ui.theme.Springs.lively()) { it } + fadeIn(tween(140)),
        exit = slideOutVertically(tween(200)) { it } + fadeOut(tween(160)),
    ) {
        Row(
            Modifier
                // A 48dp floor: a full touch target, and room for the name at large text sizes.
                .heightIn(min = 48.dp)
                .glass(CircleShape, tint = colors.surface3.copy(alpha = 0.86f))
                .clickable(role = androidx.compose.ui.semantics.Role.Button) {
                    shell.haptics.click()
                    shell.launch { com.wasimaster.wmpass.core.Pass.leaveChild() }
                }
                .padding(start = 12.dp, end = 18.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(PassIcons.ArrowBack, null, tint = colors.primary, modifier = Modifier.size(18.dp))
            Spacer(Modifier.width(8.dp))
            // One line: a long vault name wrapped in a pill one line high would be cut through.
            Text(
                "Close ${remembered.orEmpty()}",
                color = colors.text,
                style = MaterialTheme.typography.labelLarge,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier.widthIn(max = 280.dp),
            )
        }
    }
}

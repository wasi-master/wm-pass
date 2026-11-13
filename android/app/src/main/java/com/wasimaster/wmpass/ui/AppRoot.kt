package com.wasimaster.wmpass.ui

import android.view.inputmethod.EditorInfo
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.tween
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.isSpecified
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.InterceptPlatformTextInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.PlatformTextInputMethodRequest
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.door.DoorScreen
import com.wasimaster.wmpass.ui.parts.ShieldGeometry
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.rememberHaptics
import com.wasimaster.wmpass.ui.vault.VaultShell
import kotlin.math.hypot

private val OpenEase = CubicBezierEasing(0.65f, 0f, 0.15f, 1f)

/**
 * The door and the vault, and the moment between them.
 *
 * Opening: the vault is drawn on top of the door, clipped to the shield, and the shield grows
 * until the whole screen is inside it. Locking runs the same film backwards, so the vault
 * folds into the mark that guards it.
 */
@Composable
fun AppRoot(prefs: Prefs) {
    val live by Pass.state.collectAsStateWithLifecycle()
    // After an internal error the vault is locked already, but the screens it showed stay, with
    // whatever was typed in them, under a message until the person goes to the door.
    val held by Pass.internalHeld.collectAsStateWithLifecycle()
    val stopped by Engine.stopped.collectAsStateWithLifecycle()
    val state = held ?: live
    Box(Modifier.fillMaxSize()) {
        if (palette.flat) FlatDoor(state, prefs) else ShieldDoor(state, prefs)
        if (stopped || held != null) InternalErrorNotice()
    }
}

@Composable
private fun ShieldDoor(state: LockState, prefs: Prefs) {
    val colors = palette
    val reduce = LocalReduceMotion.current
    val haptics = rememberHaptics()
    val unlocked = state is LockState.Unlocked
    // 0 is the door, 1 is the vault. The vault stays composed while this is above 0.
    val open = remember { Animatable(if (unlocked) 1f else 0f) }
    var vaultShown by remember { mutableStateOf(unlocked) }
    LaunchedEffect(unlocked) {
        if (unlocked) {
            vaultShown = true
            if (reduce) open.snapTo(1f) else open.animateTo(1f, tween(920, easing = OpenEase))
        } else {
            if (vaultShown) haptics.latch()
            if (reduce) open.snapTo(0f) else open.animateTo(0f, tween(520, easing = OpenEase))
            vaultShown = false
        }
    }
    val shieldCentreY = with(LocalDensity.current) { DoorShieldCentreDp * density }
    Box(Modifier.fillMaxSize().background(colors.ground)) {
        if (open.value < 1f) {
            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        // The door drifts back as the vault comes through it.
                        val p = open.value
                        scaleX = 1f - 0.1f * p
                        scaleY = 1f - 0.1f * p
                        alpha = 1f - p * p
                    },
            ) {
                // The door's own fields too: an account's email and server, a new vault's name.
                NoLearningInput { DoorScreen(state, prefs) }
            }
        }
        if (vaultShown) {
            Box(
                Modifier
                    .fillMaxSize()
                    .drawWithCache {
                        val p = open.value
                        val placed = DoorShieldCentre.floatValue
                        val centre = Offset(size.width / 2f, if (placed.isNaN()) shieldCentreY.coerceAtMost(size.height / 2f) else placed)
                        // Wide enough that the shield's narrow tip still clears the far corner.
                        val reach = hypot(size.width, size.height) * 3.2f
                        val start = 96f * density
                        // Cubed, so the shield is seen widening before it fills the screen.
                        val width = start + (reach - start) * p * p * p
                        val mask = ShieldGeometry.maskPath(centre, width)
                        val rim = Stroke(width = 3f * density)
                        onDrawWithContent {
                            if (p >= 1f) {
                                drawContent()
                            } else {
                                clipPath(mask, ClipOp.Intersect) { this@onDrawWithContent.drawContent() }
                                // A lit edge on the opening, fading as it passes the screen's edges.
                                drawPath(mask, colors.primary.copy(alpha = 0.85f * (1f - p)), style = rim)
                            }
                        }
                    }
                    .graphicsLayer {
                        val p = open.value
                        val s = 1.12f - 0.12f * p
                        scaleX = s
                        scaleY = s
                    },
            ) {
                NoLearningInput { VaultShell(prefs) }
            }
        }
    }
}

/**
 * Tells every keyboard not to learn from what is typed in here: names, notes and searches
 * included, not only the password fields, which a keyboard already leaves alone.
 */
@OptIn(ExperimentalComposeUiApi::class)
@Composable
fun NoLearningInput(content: @Composable () -> Unit) {
    InterceptPlatformTextInput(
        interceptor = { request, next ->
            next.startInputMethod(
                PlatformTextInputMethodRequest { outAttributes ->
                    val connection = request.createInputConnection(outAttributes)
                    outAttributes.imeOptions = outAttributes.imeOptions or EditorInfo.IME_FLAG_NO_PERSONALIZED_LEARNING
                    connection
                },
            )
        },
        content = content,
    )
}

/**
 * The standard look's opening: the vault grows out of the Unlock button as a circle, in
 * Material's emphasized easing, while the door falls back. Locking runs it the other way.
 */
@Composable
private fun FlatDoor(state: LockState, prefs: Prefs) {
    val colors = palette
    val reduce = LocalReduceMotion.current
    val haptics = rememberHaptics()
    val unlocked = state is LockState.Unlocked
    val open = remember { Animatable(if (unlocked) 1f else 0f) }
    var vaultShown by remember { mutableStateOf(unlocked) }
    LaunchedEffect(unlocked) {
        if (unlocked) {
            vaultShown = true
            if (reduce) open.snapTo(1f) else open.animateTo(1f, tween(640, easing = Emphasized))
        } else {
            if (vaultShown) haptics.latch()
            if (reduce) open.snapTo(0f) else open.animateTo(0f, tween(380, easing = EmphasizedAccelerate))
            vaultShown = false
        }
    }
    Box(Modifier.fillMaxSize().background(colors.ground)) {
        if (open.value < 1f) {
            Box(
                Modifier
                    .fillMaxSize()
                    .graphicsLayer {
                        val p = open.value
                        scaleX = 1f - 0.06f * p
                        scaleY = 1f - 0.06f * p
                        alpha = 1f - p
                    },
            ) { NoLearningInput { DoorScreen(state, prefs) } }
        }
        if (vaultShown) {
            Box(
                Modifier
                    .fillMaxSize()
                    .drawWithCache {
                        val p = open.value
                        val from = DoorRevealFrom.value.takeIf { it.isSpecified } ?: Offset(size.width / 2f, size.height * 0.7f)
                        // Far enough to clear the corner furthest from where it starts.
                        val far = maxOf(
                            hypot(from.x, from.y),
                            hypot(size.width - from.x, from.y),
                            hypot(from.x, size.height - from.y),
                            hypot(size.width - from.x, size.height - from.y),
                        )
                        val circle = androidx.compose.ui.graphics.Path().apply {
                            addOval(androidx.compose.ui.geometry.Rect(from, (28f * density) + (far - 28f * density) * p))
                        }
                        onDrawWithContent {
                            if (p >= 1f) drawContent() else clipPath(circle, ClipOp.Intersect) { this@onDrawWithContent.drawContent() }
                        }
                    }
                    .graphicsLayer {
                        val p = open.value
                        alpha = (p * 4f).coerceAtMost(1f)
                        val s = 1.04f - 0.04f * p
                        scaleX = s
                        scaleY = s
                    },
            ) {
                NoLearningInput { VaultShell(prefs) }
            }
        }
    }
}

private val Emphasized = CubicBezierEasing(0.2f, 0f, 0f, 1f)
private val EmphasizedAccelerate = CubicBezierEasing(0.3f, 0f, 0.8f, 0.15f)

/** Where the standard door's Unlock button sits, in root pixels, for the opening to start from. */
internal val DoorRevealFrom = mutableStateOf(Offset.Unspecified)

/** Where the door draws its shield's centre while nothing is in the way. */
const val DoorShieldCentreDp = 262f

/**
 * Where the door's shield is centred, in pixels from the top, as last placed: the keyboard lifts
 * it. The opening starts exactly from here. Not a number until the door has drawn.
 */
internal val DoorShieldCentre = mutableFloatStateOf(Float.NaN)


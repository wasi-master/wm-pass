package com.wasimaster.wmpass.ui

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
import android.view.inputmethod.EditorInfo
import androidx.compose.ui.ExperimentalComposeUiApi
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.InterceptPlatformTextInput
import androidx.compose.ui.platform.PlatformTextInputMethodRequest
import androidx.compose.ui.draw.drawWithCache
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.ClipOp
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalDensity
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.app.Prefs
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
    val colors = palette
    val state by Pass.state.collectAsStateWithLifecycle()
    if (colors.flat) {
        // The standard look opens the way any app moves between screens.
        androidx.compose.animation.Crossfade(state is LockState.Unlocked, Modifier.fillMaxSize().background(colors.ground), tween(180), label = "door") { open ->
            if (open) NoLearningInput { VaultShell(prefs) } else DoorScreen(state, prefs)
        }
        return
    }
    val reduce = LocalReduceMotion.current
    val haptics = rememberHaptics()
    val unlocked = state is LockState.Unlocked
    // 0 is the door, 1 is the vault. The vault stays composed while this is above 0.
    val open = remember { Animatable(if (unlocked) 1f else 0f) }
    var vaultShown by remember { mutableStateOf(unlocked) }
    LaunchedEffect(unlocked) {
        if (unlocked) {
            vaultShown = true
            if (reduce) open.snapTo(1f) else open.animateTo(1f, tween(760, easing = OpenEase))
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
                        scaleX = 1f - 0.06f * p
                        scaleY = 1f - 0.06f * p
                        alpha = 1f - p * p
                    },
            ) {
                DoorScreen(state, prefs)
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
                        val width = start + (reach - start) * p * p
                        val mask = ShieldGeometry.maskPath(centre, width)
                        onDrawWithContent {
                            if (p >= 1f) {
                                drawContent()
                            } else {
                                clipPath(mask, ClipOp.Intersect) { this@onDrawWithContent.drawContent() }
                            }
                        }
                    }
                    .graphicsLayer {
                        val p = open.value
                        val s = 1.06f - 0.06f * p
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

/** Where the door draws its shield's centre while nothing is in the way. */
const val DoorShieldCentreDp = 262f

/**
 * Where the door's shield is centred, in pixels from the top, as last placed: the keyboard lifts
 * it. The opening starts exactly from here. Not a number until the door has drawn.
 */
internal val DoorShieldCentre = mutableFloatStateOf(Float.NaN)


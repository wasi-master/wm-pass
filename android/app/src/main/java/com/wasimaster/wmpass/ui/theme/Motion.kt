package com.wasimaster.wmpass.ui.theme

import android.content.Context
import android.os.Build
import android.os.VibrationEffect
import android.os.Vibrator
import android.provider.Settings
import android.view.HapticFeedbackConstants
import android.view.View
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.Spring
import androidx.compose.animation.core.SpringSpec
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.spring
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.interaction.collectIsPressedAsState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** The three springs. Under a finger, changing place, arriving with some joy. */
object Springs {
    fun <T> snap(): SpringSpec<T> = spring(dampingRatio = 0.86f, stiffness = 900f)
    fun <T> glide(): SpringSpec<T> = spring(dampingRatio = 0.92f, stiffness = 320f)
    fun <T> lively(): SpringSpec<T> = spring(dampingRatio = 0.62f, stiffness = 420f)
    fun <T> slow(): SpringSpec<T> = spring(dampingRatio = Spring.DampingRatioNoBouncy, stiffness = 90f)
}

/** True when the system's animator scale is zero: everything snaps and nothing is lost. */
val LocalReduceMotion = staticCompositionLocalOf { false }

fun Context.systemAnimationsOff(): Boolean =
    Settings.Global.getFloat(contentResolver, Settings.Global.ANIMATOR_DURATION_SCALE, 1f) == 0f

/** Shrinks a little under the finger, and springs back. */
@Composable
fun Modifier.pressScale(source: MutableInteractionSource, pressed: Float = 0.97f): Modifier {
    val isPressed by source.collectIsPressedAsState()
    val scale by animateFloatAsState(if (isPressed) pressed else 1f, Springs.snap(), label = "press")
    return graphicsLayer {
        scaleX = scale
        scaleY = scale
    }
}

/**
 * The first arrival of a list row: up and in, one after another. Capped, so a row far down
 * a long list does not wait behind ones nobody can see.
 */
@Composable
fun Modifier.arrive(index: Int, enabled: Boolean = true, rise: Dp = 18.dp): Modifier {
    val reduce = LocalReduceMotion.current
    if (!enabled || reduce) return this
    val progress = remember { Animatable(0f) }
    LaunchedEffect(Unit) {
        delay(minOf(index, 8) * 34L)
        launch { progress.animateTo(1f, Springs.glide()) }
    }
    val px = with(LocalDensity.current) { rise.toPx() }
    return graphicsLayer {
        alpha = progress.value.coerceIn(0f, 1f)
        translationY = (1f - progress.value) * px
    }
}

/**
 * Touch feedback, composed where the hardware can: raw vibration feels mushy because it has
 * no braking, while a click primitive stops dead.
 */
@Stable
class Haptics(private val view: View, context: Context) {
    private val vibrator: Vibrator? = context.getSystemService(Vibrator::class.java)

    private fun composed(primitive: Int, scale: Float, fallback: Int) {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
            vibrator?.areAllPrimitivesSupported(primitive) == true
        ) {
            vibrator.vibrate(VibrationEffect.startComposition().addPrimitive(primitive, scale).compose())
        } else {
            view.performHapticFeedback(fallback)
        }
    }

    /** One step of something: a slider, a digit. */
    fun tick() = composed(primitiveTick(), 0.5f, HapticFeedbackConstants.CLOCK_TICK)

    /** A press that did something. */
    fun click() = composed(primitiveClick(), 0.7f, HapticFeedbackConstants.KEYBOARD_TAP)

    /** It worked. */
    fun confirm() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
            vibrator?.areAllPrimitivesSupported(primitiveClick(), primitiveThud()) == true
        ) {
            vibrator.vibrate(
                VibrationEffect.startComposition()
                    .addPrimitive(primitiveClick(), 0.5f)
                    .addPrimitive(primitiveThud(), 0.6f, 70)
                    .compose(),
            )
        } else {
            view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
        }
    }

    /** It did not. */
    fun reject() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R &&
            vibrator?.areAllPrimitivesSupported(primitiveClick()) == true
        ) {
            vibrator.vibrate(
                VibrationEffect.startComposition()
                    .addPrimitive(primitiveClick(), 0.9f)
                    .addPrimitive(primitiveClick(), 0.9f, 90)
                    .compose(),
            )
        } else {
            view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
        }
    }

    /** The vault latching shut. */
    fun latch() = composed(primitiveThud(), 0.9f, HapticFeedbackConstants.LONG_PRESS)

    private fun primitiveTick() = VibrationEffect.Composition.PRIMITIVE_TICK
    private fun primitiveClick() = VibrationEffect.Composition.PRIMITIVE_CLICK
    private fun primitiveThud() = VibrationEffect.Composition.PRIMITIVE_THUD
}

@Composable
fun rememberHaptics(): Haptics {
    val view = LocalView.current
    val context = LocalContext.current
    return remember(view, context) { Haptics(view, context) }
}

package com.wasimaster.wmpass.ui.fx

import android.graphics.RuntimeShader
import android.os.Build
import androidx.annotation.RequiresApi
import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameMillis
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ShaderBrush
import androidx.compose.ui.graphics.drawscope.DrawScope
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.Palette
import com.wasimaster.wmpass.ui.theme.palette

private const val AURORA = """
uniform float2 size;
uniform float time;
uniform float strength;
layout(color) uniform half4 ground;
layout(color) uniform half4 a;
layout(color) uniform half4 b;
layout(color) uniform half4 c;

float blob(float2 p, float2 at, float radius) {
    float d = length(p - at) / radius;
    return exp(-d * d * 1.7);
}

half4 main(float2 here) {
    float aspect = size.x / size.y;
    float2 p = here / size.y;
    float2 one = float2(aspect * (0.28 + 0.20 * sin(time * 0.11)), 0.02 + 0.07 * cos(time * 0.17));
    float2 two = float2(aspect * (0.82 + 0.16 * cos(time * 0.09 + 1.7)), 0.16 + 0.10 * sin(time * 0.13 + 0.6));
    float2 three = float2(aspect * (0.50 + 0.30 * sin(time * 0.07 + 3.1)), 0.34 + 0.08 * cos(time * 0.10 + 2.2));
    half3 color = ground.rgb;
    color = mix(color, a.rgb, half(blob(p, one, 0.46) * strength));
    color = mix(color, b.rgb, half(blob(p, two, 0.40) * strength));
    color = mix(color, c.rgb, half(blob(p, three, 0.34) * strength * 0.8));
    // Fine grain, about a level of 8-bit colour: dark gradients band without it.
    float grain = fract(sin(dot(here, float2(12.9898, 78.233))) * 43758.5453);
    color += half3((grain - 0.5) * 0.012);
    return half4(color, 1.0);
}
"""

/**
 * The light behind the door and the top of the vault: three slow glows drifting over the
 * ground. It runs only while [moving], and it stops for reduced motion. Below Android 13
 * there is no shader to run, so the same glows stand still as gradients.
 */
@Composable
fun Aurora(modifier: Modifier = Modifier, moving: Boolean = true, strength: Float = 1f) {
    val colors = palette
    if (colors.flat) return
    val reduce = LocalReduceMotion.current
    var time by remember { mutableFloatStateOf(37f) }
    LaunchedEffect(moving, reduce) {
        if (!moving || reduce) return@LaunchedEffect
        var last = withFrameMillis { it }
        while (true) {
            withFrameMillis { now ->
                time += (now - last).coerceAtMost(64L) / 1000f
                last = now
            }
        }
    }
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU && FxSupport.shaders) {
        val shader = remember { AuroraShader() }
        Canvas(modifier) { shader.draw(this, time, strength, colors) }
    } else {
        Canvas(modifier) { drawStillAurora(strength, colors) }
    }
}

@RequiresApi(Build.VERSION_CODES.TIRAMISU)
private class AuroraShader {
    private val shader = RuntimeShader(AURORA)
    private val brush = ShaderBrush(shader)

    fun draw(scope: DrawScope, time: Float, strength: Float, colors: Palette) = with(scope) {
        shader.setFloatUniform("size", size.width, size.height)
        shader.setFloatUniform("time", time)
        shader.setFloatUniform("strength", strength)
        shader.setColorUniform("ground", colors.ground.toLong())
        shader.setColorUniform("a", colors.auroraA.toLong())
        shader.setColorUniform("b", colors.auroraB.toLong())
        shader.setColorUniform("c", colors.auroraC.toLong())
        drawRect(brush)
    }
}

private fun Color.toLong(): Long = value.toLong()

private fun DrawScope.drawStillAurora(strength: Float, colors: Palette) {
    drawRect(colors.ground)
    val r = size.height * 0.5f
    fun glow(at: Offset, color: Color, radius: Float) = drawRect(
        Brush.radialGradient(listOf(color.copy(alpha = 0.9f * strength), Color.Transparent), at, radius),
    )
    glow(Offset(size.width * 0.28f, size.height * 0.04f), colors.auroraA, r)
    glow(Offset(size.width * 0.84f, size.height * 0.18f), colors.auroraB, r * 0.9f)
    glow(Offset(size.width * 0.5f, size.height * 0.36f), colors.auroraC, r * 0.7f)
}

/** What this device can draw. Tests turn the GPU-only parts off. */
object FxSupport {
    @Volatile
    var shaders: Boolean = true

    @Volatile
    var blur: Boolean = true
}

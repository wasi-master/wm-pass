package com.wasimaster.wmpass.ui.theme

import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Outline
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shape
import androidx.compose.ui.unit.Density
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.LayoutDirection
import androidx.compose.ui.unit.dp

// A continuous corner: curvature eases in along the edge rather than starting at once, which
// is why it reads as one object where a circular corner reads as a box with its corners cut.
// These ratios are the well known cubic fit to a superellipse corner.
private const val A = 1.528665f
private const val B = 1.088492f
private const val C = 0.868407f
private const val D = 0.631493f
private const val E = 0.074911f
private const val F = 0.169060f
private const val G = 0.372824f

class SquircleShape(private val radius: Dp) : Shape {
    override fun createOutline(size: Size, layoutDirection: LayoutDirection, density: Density): Outline =
        Outline.Generic(squirclePath(size, with(density) { radius.toPx() }))

    override fun equals(other: Any?) = other is SquircleShape && other.radius == radius

    override fun hashCode() = radius.hashCode()

    override fun toString() = "SquircleShape($radius)"
}

fun squirclePath(size: Size, requested: Float): Path {
    val w = size.width
    val h = size.height
    val r = minOf(requested, minOf(w, h) / 2f / A).coerceAtLeast(0f)
    return Path().apply {
        moveTo(A * r, 0f)
        lineTo(w - A * r, 0f)
        cubicTo(w - B * r, 0f, w - C * r, 0f, w - D * r, E * r)
        cubicTo(w - G * r, F * r, w - F * r, G * r, w - E * r, D * r)
        cubicTo(w, C * r, w, B * r, w, A * r)
        lineTo(w, h - A * r)
        cubicTo(w, h - B * r, w, h - C * r, w - E * r, h - D * r)
        cubicTo(w - F * r, h - G * r, w - G * r, h - F * r, w - D * r, h - E * r)
        cubicTo(w - C * r, h, w - B * r, h, w - A * r, h)
        lineTo(A * r, h)
        cubicTo(B * r, h, C * r, h, D * r, h - E * r)
        cubicTo(G * r, h - F * r, F * r, h - G * r, E * r, h - D * r)
        cubicTo(0f, h - C * r, 0f, h - B * r, 0f, h - A * r)
        lineTo(0f, A * r)
        cubicTo(0f, B * r, 0f, C * r, E * r, D * r)
        cubicTo(F * r, G * r, G * r, F * r, D * r, E * r)
        cubicTo(C * r, 0f, B * r, 0f, A * r, 0f)
        close()
    }
}

/** The app's corners. */
object Shapes {
    val card = SquircleShape(28.dp)
    val cardSmall = SquircleShape(22.dp)
    val tile = SquircleShape(16.dp)
    val tileSmall = SquircleShape(12.dp)
    val sheet = SquircleShape(34.dp)
}

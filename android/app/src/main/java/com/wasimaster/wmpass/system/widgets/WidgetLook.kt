package com.wasimaster.wmpass.system.widgets

import android.content.Context
import android.os.Build
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.TextUnit
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.ColorFilter
import androidx.glance.GlanceModifier
import androidx.glance.Image
import androidx.glance.ImageProvider
import androidx.glance.action.Action
import androidx.glance.action.clickable
import androidx.glance.appwidget.appWidgetBackground
import androidx.glance.appwidget.cornerRadius
import androidx.glance.background
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Column
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.padding
import androidx.glance.layout.size
import androidx.glance.semantics.contentDescription
import androidx.glance.semantics.semantics
import androidx.glance.text.FontFamily
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextStyle
import androidx.glance.unit.ColorProvider
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.theme.BuiltInThemes
import com.wasimaster.wmpass.ui.theme.Look
import com.wasimaster.wmpass.ui.theme.Palette
import com.wasimaster.wmpass.ui.theme.resolveTheme
import com.wasimaster.wmpass.ui.theme.toPalette
import androidx.glance.color.ColorProvider as DayNight

/** The colours a widget draws with, each one for the day and for the night. */
class WidgetColors(
    val background: ColorProvider,
    val button: ColorProvider,
    val buttonActive: ColorProvider,
    val onButtonActive: ColorProvider,
    val text: ColorProvider,
    val textMid: ColorProvider,
    val primary: ColorProvider,
    val warn: ColorProvider,
    val danger: ColorProvider,
    /** No background: the text sits on a soft dark halo instead, see [Halo]. */
    val halo: Boolean = false,
)

/** One side, light or dark, of what a widget draws with. */
private class Roles(val palette: Palette, val scheme: ColorScheme)

object WidgetLook {
    /** The widget colours for the owner's choice under Settings, Appearance, Widgets. */
    fun colors(context: Context): WidgetColors {
        val prefs = Prefs.of(context)
        val halo = prefs.widgetBackground.value == "none"
        // On a bare wallpaper the text sits on a dark halo, so it is the dark side's light text,
        // and the buttons go with it, day or night.
        val (day, night) = sides(context, prefs).let { if (halo) it.second to it.second else it }
        val alpha = when (prefs.widgetBackground.value) {
            "translucent" -> 0.78f
            "none" -> 0f
            else -> 1f
        }
        fun pick(alphaOf: Float = 1f, role: (Roles) -> Color): ColorProvider {
            val d = role(day)
            val n = role(night)
            return DayNight(d.copy(alpha = d.alpha * alphaOf), n.copy(alpha = n.alpha * alphaOf))
        }
        return WidgetColors(
            background = pick(alpha) { it.palette.ground },
            button = pick { it.palette.surface2 },
            buttonActive = pick { it.scheme.primaryContainer },
            onButtonActive = pick { it.scheme.onPrimaryContainer },
            text = pick { it.palette.text },
            textMid = pick { it.palette.textMid },
            primary = pick { it.palette.primary },
            warn = pick { it.palette.warn },
            danger = pick { it.palette.danger },
            halo = halo,
        )
    }

    /** The light and the dark side, following the app's theme or the wallpaper's colours. */
    private fun sides(context: Context, prefs: Prefs): Pair<Roles, Roles> {
        val choice = prefs.widgetColours.value
        if (choice == "system" && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            val light = dynamicLightColorScheme(context)
            val dark = dynamicDarkColorScheme(context)
            return Roles(light.toPalette(false, false), light) to Roles(dark.toPalette(true, false), dark)
        }
        val look = prefs.look
        val black = look == Look.OBSIDIAN
        val flat = prefs.uiStyle != "glass"
        val theme = (BuiltInThemes.all + prefs.customThemes).firstOrNull { it.id == prefs.themeId }
        fun side(dark: Boolean) = resolveTheme(context, dark, black && dark, flat, prefs.accent, prefs.themeId, theme).let { Roles(it.first, it.second) }
        return when {
            choice == "dark" -> side(true).let { it to it }
            look == Look.LIGHT -> side(false).let { it to it }
            look == Look.DARK || look == Look.OBSIDIAN -> side(true).let { it to it }
            else -> side(false) to side(true)
        }
    }
}

object WidgetText {
    fun style(color: ColorProvider, size: TextUnit = 13.sp, weight: FontWeight = FontWeight.Normal, mono: Boolean = false) =
        TextStyle(color = color, fontSize = size, fontWeight = weight, fontFamily = if (mono) FontFamily.Monospace else null)
}

/**
 * The widget's own surface: the system's corner radius, and the background the owner chose. With
 * no background, the padding gives way to the halos' own, so the text stays where it was.
 */
@Composable
fun WidgetFrame(colors: WidgetColors, modifier: GlanceModifier = GlanceModifier, onClick: Action? = null, padding: Dp = 12.dp, content: @Composable () -> Unit) {
    var outer = GlanceModifier
        .fillMaxSize()
        .appWidgetBackground()
        .background(colors.background)
        .cornerRadius(android.R.dimen.system_app_widget_background_radius)
    if (onClick != null) outer = outer.clickable(onClick)
    val inner = if (colors.halo) {
        GlanceModifier.padding(horizontal = (padding - HaloPadX).coerceAtLeast(0.dp), vertical = (padding - HaloPadY).coerceAtLeast(0.dp))
    } else {
        GlanceModifier.padding(padding)
    }
    Box(outer.then(modifier).then(inner), contentAlignment = Alignment.TopStart) { content() }
}

/**
 * A round button with a glyph, read out by its [label]. With no [onClick] it is only drawn: the
 * thing around it takes the tap and carries the name, so a screen reader stops once, not twice.
 */
@Composable
fun RoundButton(icon: Int, label: String?, colors: WidgetColors, onClick: Action?, active: Boolean = false, size: Dp = 48.dp) {
    var look = GlanceModifier
        .size(size)
        .background(if (active) colors.buttonActive else colors.button)
        .cornerRadius(size / 2)
    if (onClick != null) look = look.clickable(onClick)
    if (onClick != null && label != null) look = look.semantics { contentDescription = label }
    Box(
        look,
        contentAlignment = Alignment.Center,
    ) {
        Glyph(icon, null, if (active) colors.onButtonActive else colors.text, size / 2)
    }
}

private val HaloPadX = 6.dp
private val HaloPadY = 2.dp

/**
 * A run of text, a line or a few lines together, on a soft dark halo when the widget has no
 * background (Glance has no text shadow). Otherwise it is only [content]. [modifier] is for the
 * place the run takes, a weight or a margin, and is kept off the halo so the halo hugs the text.
 * The halo is only drawn: it takes no tap and no name, so what a screen reader hears is unchanged.
 */
@Composable
fun Halo(colors: WidgetColors, modifier: GlanceModifier = GlanceModifier, content: @Composable () -> Unit) {
    if (!colors.halo) {
        if (modifier == GlanceModifier) content() else Box(modifier) { content() }
        return
    }
    val glow = @Composable {
        Box(GlanceModifier.background(ImageProvider(R.drawable.widget_halo)).padding(horizontal = HaloPadX, vertical = HaloPadY)) { content() }
    }
    if (modifier == GlanceModifier) glow() else Box(modifier) { glow() }
}

/** One [Text], on a [Halo] when the widget has no background. */
@Composable
fun WidgetLabel(text: String, colors: WidgetColors, style: TextStyle, modifier: GlanceModifier = GlanceModifier, maxLines: Int = 1) {
    if (!colors.halo) {
        Text(text, modifier, style = style, maxLines = maxLines)
        return
    }
    Halo(colors, modifier) { Text(text, style = style, maxLines = maxLines) }
}

@Composable
fun Glyph(icon: Int, label: String?, tint: ColorProvider, size: Dp = 20.dp, modifier: GlanceModifier = GlanceModifier) {
    Image(ImageProvider(icon), label, modifier.size(size), colorFilter = ColorFilter.tint(tint))
}

/** What a widget shows when it has nothing it may show: a glyph and one plain sentence. */
@Composable
fun Placeholder(colors: WidgetColors, icon: Int, text: String) {
    Column(GlanceModifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically, horizontalAlignment = Alignment.CenterHorizontally) {
        Glyph(icon, null, colors.textMid, 22.dp)
        WidgetLabel(text, colors, WidgetText.style(colors.textMid, 12.sp).copy(textAlign = androidx.glance.text.TextAlign.Center), GlanceModifier.padding(top = 6.dp), maxLines = 3)
    }
}

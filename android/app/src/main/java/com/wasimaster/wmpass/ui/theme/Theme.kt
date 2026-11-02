package com.wasimaster.wmpass.ui.theme

import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.em
import androidx.compose.ui.unit.sp
import com.wasimaster.wmpass.R
import com.wasimaster.wmsuite.theme.WmShapes
import com.wasimaster.wmsuite.theme.WmTypography

/** One colour story. See android/DESIGN.md. */
@Immutable
data class Palette(
    val dark: Boolean,
    val ground: Color,
    val surface1: Color,
    val surface2: Color,
    val surface3: Color,
    val rimTop: Color,
    val rimBottom: Color,
    val hairline: Color,
    val text: Color,
    val textMid: Color,
    val textLow: Color,
    val primary: Color,
    val onPrimary: Color,
    val brandFrom: Color,
    val brandTo: Color,
    val auroraA: Color,
    val auroraB: Color,
    val auroraC: Color,
    val digit: Color,
    val symbol: Color,
    val warn: Color,
    val danger: Color,
    val info: Color,
    val glassTint: Color,
) {
    val brand: Brush get() = Brush.linearGradient(listOf(brandFrom, brandTo))
}

/** The accent a person can choose. Emerald matches the logo. */
enum class Accent(val label: String, val from: Color, val to: Color, val bright: Color, val deep: Color) {
    EMERALD("Emerald", Color(0xFF10B981), Color(0xFF0D9488), Color(0xFF34D399), Color(0xFF059669)),
    OCEAN("Ocean", Color(0xFF0EA5E9), Color(0xFF2563EB), Color(0xFF38BDF8), Color(0xFF0369A1)),
    IRIS("Iris", Color(0xFF8B5CF6), Color(0xFF6366F1), Color(0xFFA78BFA), Color(0xFF6D28D9)),
    ROSE("Rose", Color(0xFFF43F5E), Color(0xFFDB2777), Color(0xFFFB7185), Color(0xFFBE123C)),
    AMBER("Amber", Color(0xFFF59E0B), Color(0xFFEA580C), Color(0xFFFBBF24), Color(0xFFB45309)),
    GRAPHITE("Graphite", Color(0xFF94A3B8), Color(0xFF475569), Color(0xFFCBD5E1), Color(0xFF334155)),
}

enum class Look { SYSTEM, DARK, LIGHT, OBSIDIAN }

private fun blend(a: Color, b: Color, amount: Float) = Color(
    red = a.red + (b.red - a.red) * amount,
    green = a.green + (b.green - a.green) * amount,
    blue = a.blue + (b.blue - a.blue) * amount,
    alpha = 1f,
)

private fun darkPalette(accent: Accent, obsidian: Boolean): Palette {
    val base = Color(0xFF061311)
    // Neutrals lean towards the accent, so the whole app sits in one light.
    val tint = accent.to
    val surface = { lift: Float -> blend(base, blend(Color(0xFF32524A), tint, 0.28f), lift) }
    return Palette(
        dark = true,
        ground = if (obsidian) Color.Black else blend(base, tint, 0.02f),
        surface1 = if (obsidian) Color(0xFF0A0F0E) else surface(0.17f),
        surface2 = if (obsidian) Color(0xFF111816) else surface(0.27f),
        surface3 = if (obsidian) Color(0xFF19221F) else surface(0.39f),
        rimTop = Color.White.copy(alpha = 0.13f),
        rimBottom = Color.White.copy(alpha = 0.025f),
        hairline = Color.White.copy(alpha = 0.07f),
        text = Color(0xFFEAFBF4),
        textMid = Color(0xFFA3BDB5),
        textLow = Color(0xFF6C8A81),
        primary = accent.bright,
        onPrimary = Color(0xFF04201A),
        brandFrom = accent.from,
        brandTo = accent.to,
        auroraA = blend(accent.from, base, 0.42f),
        auroraB = blend(accent.to, base, 0.55f),
        auroraC = blend(Color(0xFF0E7490), base, 0.70f),
        digit = Color(0xFF5EEAD4),
        symbol = Color(0xFFFBBF24),
        warn = Color(0xFFFBBF24),
        danger = Color(0xFFFB7185),
        info = Color(0xFF7DD3FC),
        glassTint = (if (obsidian) Color(0xFF0C1211) else surface(0.30f)).copy(alpha = 0.62f),
    )
}

private fun lightPalette(accent: Accent): Palette {
    val base = Color(0xFFF4F9F7)
    return Palette(
        dark = false,
        ground = blend(base, accent.bright, 0.015f),
        surface1 = Color.White,
        surface2 = blend(Color.White, accent.to, 0.045f),
        surface3 = blend(Color.White, accent.to, 0.09f),
        rimTop = Color.White,
        rimBottom = Color(0xFF0A1F1A).copy(alpha = 0.05f),
        hairline = Color(0xFF0A1F1A).copy(alpha = 0.07f),
        text = Color(0xFF0A1F1A),
        textMid = Color(0xFF46605A),
        textLow = Color(0xFF75908A),
        primary = accent.deep,
        onPrimary = Color.White,
        brandFrom = accent.from,
        brandTo = accent.to,
        auroraA = blend(accent.bright, Color.White, 0.55f),
        auroraB = blend(accent.from, Color.White, 0.70f),
        auroraC = blend(Color(0xFF67E8F9), Color.White, 0.70f),
        digit = Color(0xFF0D9488),
        symbol = Color(0xFFB45309),
        warn = Color(0xFFB45309),
        danger = Color(0xFFE11D48),
        info = Color(0xFF0369A1),
        glassTint = Color.White.copy(alpha = 0.66f),
    )
}

private fun Palette.scheme(): ColorScheme = if (dark) {
    darkColorScheme(
        primary = primary,
        onPrimary = onPrimary,
        primaryContainer = blend(surface2, primary, 0.22f),
        onPrimaryContainer = text,
        secondary = primary,
        onSecondary = onPrimary,
        secondaryContainer = surface3,
        onSecondaryContainer = text,
        tertiary = info,
        onTertiary = onPrimary,
        background = ground,
        onBackground = text,
        surface = ground,
        onSurface = text,
        surfaceVariant = surface2,
        onSurfaceVariant = textMid,
        surfaceContainerLowest = ground,
        surfaceContainerLow = surface1,
        surfaceContainer = surface1,
        surfaceContainerHigh = surface2,
        surfaceContainerHighest = surface3,
        outline = textLow,
        outlineVariant = hairline,
        error = danger,
        onError = onPrimary,
    )
} else {
    lightColorScheme(
        primary = primary,
        onPrimary = onPrimary,
        primaryContainer = blend(Color.White, primary, 0.14f),
        onPrimaryContainer = text,
        secondary = primary,
        onSecondary = onPrimary,
        secondaryContainer = surface3,
        onSecondaryContainer = text,
        tertiary = info,
        background = ground,
        onBackground = text,
        surface = ground,
        onSurface = text,
        surfaceVariant = surface2,
        onSurfaceVariant = textMid,
        surfaceContainerLowest = Color.White,
        surfaceContainerLow = surface1,
        surfaceContainer = surface1,
        surfaceContainerHigh = surface2,
        surfaceContainerHighest = surface3,
        outline = textLow,
        outlineVariant = hairline,
        error = danger,
    )
}

val LocalPalette = staticCompositionLocalOf { darkPalette(Accent.EMERALD, false) }

val MonoFamily = FontFamily(
    Font(R.font.jetbrainsmono_regular, FontWeight.Normal),
    Font(R.font.jetbrainsmono_medium, FontWeight.Medium),
)

/** Styles the Material roles do not cover. */
object PassType {
    /** A secret, large. */
    val secret = TextStyle(fontFamily = MonoFamily, fontWeight = FontWeight.Medium, fontSize = 19.sp, lineHeight = 27.sp)

    /** A secret on a row. */
    val secretSmall = TextStyle(fontFamily = MonoFamily, fontWeight = FontWeight.Normal, fontSize = 15.sp, lineHeight = 22.sp)

    /** A code you read and type. */
    val code = TextStyle(fontFamily = MonoFamily, fontWeight = FontWeight.Medium, fontSize = 30.sp, letterSpacing = 0.02.em)

    /** Numbers that change: tabular so they do not jitter. */
    val numeric = TextStyle(fontFeatureSettings = "tnum")

    /** A section's small heading. */
    val eyebrow = TextStyle(fontWeight = FontWeight.SemiBold, fontSize = 12.sp, letterSpacing = 0.06.em)
}

@Composable
fun PassTheme(look: Look = Look.DARK, accent: Accent = Accent.EMERALD, content: @Composable () -> Unit) {
    val systemDark = isSystemInDarkTheme()
    val dark = when (look) {
        Look.SYSTEM -> systemDark
        Look.DARK, Look.OBSIDIAN -> true
        Look.LIGHT -> false
    }
    val palette = remember(dark, look, accent) {
        if (dark) darkPalette(accent, look == Look.OBSIDIAN) else lightPalette(accent)
    }
    val scheme = remember(palette) { palette.scheme() }
    CompositionLocalProvider(LocalPalette provides palette) {
        MaterialTheme(colorScheme = scheme, typography = WmTypography, shapes = WmShapes, content = content)
    }
}

val palette: Palette
    @Composable get() = LocalPalette.current

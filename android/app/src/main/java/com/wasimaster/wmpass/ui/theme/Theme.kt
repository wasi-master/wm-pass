package com.wasimaster.wmpass.ui.theme

import android.os.Build
import androidx.compose.foundation.isSystemInDarkTheme
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.ColorScheme
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Shapes as MaterialShapes
import androidx.compose.material3.Typography
import androidx.compose.material3.darkColorScheme
import androidx.compose.material3.dynamicDarkColorScheme
import androidx.compose.material3.dynamicLightColorScheme
import androidx.compose.material3.lightColorScheme
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.Immutable
import androidx.compose.runtime.remember
import androidx.compose.runtime.staticCompositionLocalOf
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.Font
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
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
    /**
     * True for the standard look: solid surfaces, no lit rims, no gradients. Read by the shared
     * pieces, so a page built from them follows without knowing which look is on.
     */
    val flat: Boolean = false,
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

private fun String?.orThemeColor(fallback: Color) = if (isNullOrEmpty()) fallback else toThemeColor()

/** A palette from a theme's colours, for the standard look or for Glass. */
fun ThemeColors.toPalette(dark: Boolean, black: Boolean, flat: Boolean): Palette {
    val c = readableOn()
    val bg = if (black && dark) Color.Black else c.background.toThemeColor()
    val low = if (black && dark) Color(0xFF0B0B0B) else c.surface.toThemeColor()
    val high = if (black && dark) Color(0xFF161616) else c.surfaceHigh.toThemeColor()
    val highest = if (black && dark) Color(0xFF222222) else c.surfaceHighest.toThemeColor()
    val text = c.text.toThemeColor()
    val primary = c.primary.toThemeColor()
    val on = when {
        c.onPrimary.isNotEmpty() -> c.onPrimary.toThemeColor()
        contrast(c.background.toThemeColor(), primary) >= 4.5 -> c.background.toThemeColor()
        contrast(Color.White, primary) >= contrast(Color.Black, primary) -> Color.White
        else -> Color.Black
    }
    val hair = mixColors(bg, text, 0.16f)
    return Palette(
        dark = dark,
        ground = bg,
        surface1 = low,
        surface2 = high,
        surface3 = highest,
        rimTop = if (flat) Color.Transparent else text.copy(alpha = 0.13f),
        rimBottom = if (flat) Color.Transparent else text.copy(alpha = 0.025f),
        hairline = hair,
        text = text,
        textMid = c.textMid.toThemeColor(),
        textLow = c.textLow.toThemeColor(),
        primary = primary,
        onPrimary = on,
        brandFrom = primary,
        brandTo = mixColors(primary, bg, 0.18f),
        auroraA = mixColors(primary, bg, 0.5f),
        auroraB = mixColors(primary, bg, 0.65f),
        auroraC = mixColors(c.info.toThemeColor(), bg, 0.72f),
        digit = c.digit.orThemeColor(c.info.toThemeColor()),
        symbol = c.symbol.orThemeColor(c.warn.toThemeColor()),
        warn = c.warn.toThemeColor(),
        danger = c.danger.toThemeColor(),
        info = c.info.toThemeColor(),
        glassTint = high.copy(alpha = 0.62f),
        flat = flat,
    )
}

/** Material's roles from a theme, as exactly as it specifies them. */
fun Palette.flatScheme(source: ThemeColors?): ColorScheme {
    val container = source?.primaryContainer.orThemeColor(mixColors(surface2, primary, if (dark) 0.28f else 0.20f))
    val second = source?.secondaryContainer.orThemeColor(mixColors(surface2, primary, if (dark) 0.18f else 0.14f))
    val outlineColor = source?.outline.orThemeColor(textLow)
    val base = if (dark) darkColorScheme() else lightColorScheme()
    return base.copy(
        primary = primary,
        onPrimary = onPrimary,
        primaryContainer = container,
        onPrimaryContainer = text,
        secondary = primary,
        onSecondary = onPrimary,
        secondaryContainer = second,
        onSecondaryContainer = text,
        tertiary = info,
        onTertiary = onPrimary,
        tertiaryContainer = mixColors(surface2, info, 0.2f),
        onTertiaryContainer = text,
        background = ground,
        onBackground = text,
        surface = ground,
        onSurface = text,
        surfaceVariant = surface2,
        onSurfaceVariant = textMid,
        surfaceTint = primary,
        inverseSurface = text,
        inverseOnSurface = ground,
        inversePrimary = mixColors(primary, ground, 0.4f),
        surfaceContainerLowest = ground,
        surfaceContainerLow = surface1,
        surfaceContainer = mixColors(surface1, surface2, 0.5f),
        surfaceContainerHigh = surface2,
        surfaceContainerHighest = surface3,
        surfaceBright = surface3,
        surfaceDim = ground,
        outline = outlineColor,
        outlineVariant = hairline,
        error = danger,
        onError = if (contrast(Color.White, danger) >= contrast(Color.Black, danger)) Color.White else Color.Black,
        errorContainer = mixColors(surface2, danger, 0.22f),
        onErrorContainer = text,
    )
}

/** The phone's own colours, seen through the app's palette. */
internal fun ColorScheme.toPalette(dark: Boolean, black: Boolean): Palette {
    val bg = if (black && dark) Color.Black else surface
    return Palette(
        dark = dark,
        ground = bg,
        surface1 = surfaceContainerLow,
        surface2 = surfaceContainerHigh,
        surface3 = surfaceContainerHighest,
        rimTop = Color.Transparent,
        rimBottom = Color.Transparent,
        hairline = outlineVariant,
        text = onSurface,
        textMid = onSurfaceVariant,
        textLow = outline,
        primary = primary,
        onPrimary = onPrimary,
        brandFrom = primary,
        brandTo = primary,
        auroraA = primaryContainer,
        auroraB = secondaryContainer,
        auroraC = tertiaryContainer,
        digit = tertiary,
        symbol = secondary,
        warn = if (dark) Color(0xFFFFB95C) else Color(0xFF8A5300),
        danger = error,
        info = tertiary,
        glassTint = surfaceContainerHigh.copy(alpha = 0.62f),
        flat = true,
    )
}

private fun Typography.inFamily(family: FontFamily) = Typography(
    displayLarge.copy(fontFamily = family), displayMedium.copy(fontFamily = family), displaySmall.copy(fontFamily = family),
    headlineLarge.copy(fontFamily = family), headlineMedium.copy(fontFamily = family), headlineSmall.copy(fontFamily = family),
    titleLarge.copy(fontFamily = family), titleMedium.copy(fontFamily = family), titleSmall.copy(fontFamily = family),
    bodyLarge.copy(fontFamily = family), bodyMedium.copy(fontFamily = family), bodySmall.copy(fontFamily = family),
    labelLarge.copy(fontFamily = family), labelMedium.copy(fontFamily = family), labelSmall.copy(fontFamily = family),
)

/**
 * Font files the owner added for their themes, kept in the app's own storage. A theme names one
 * as `file:<name>`; on a device without that file the theme falls back to the system font.
 */
object ThemeFonts {
    private const val LIMIT = 12L * 1024 * 1024

    fun dir(context: android.content.Context): java.io.File = java.io.File(context.filesDir, "fonts").apply { mkdirs() }

    fun saved(context: android.content.Context): List<String> =
        dir(context).listFiles()?.filter { it.isFile }?.sortedBy { it.name.lowercase() }?.map { "file:${it.name}" }.orEmpty()

    fun label(key: String): String = key.removePrefix("file:").substringBeforeLast('.')

    fun family(context: android.content.Context, key: String): FontFamily? {
        if (!key.startsWith("file:")) return null
        val file = java.io.File(dir(context), key.removePrefix("file:"))
        return if (file.isFile) runCatching { FontFamily(Font(file)) }.getOrNull() else null
    }

    /** Copies a picked font file in and checks Android can draw with it. The key, or an error. */
    fun add(context: android.content.Context, uri: android.net.Uri): Result<String> = runCatching {
        val resolver = context.contentResolver
        val shown = resolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)
            ?.use { if (it.moveToFirst()) it.getString(0) else null } ?: "font.ttf"
        val name = shown.filter { it.isLetterOrDigit() || it in " ._-" }.trim().ifEmpty { "font.ttf" }
        require(name.substringAfterLast('.').lowercase() in setOf("ttf", "otf")) { "Choose a .ttf or .otf font file." }
        val target = java.io.File(dir(context), name)
        resolver.openInputStream(uri)?.use { input ->
            target.outputStream().use { out ->
                val copied = input.copyTo(out)
                require(copied in 1..LIMIT) { "That font file is too large." }
            }
        } ?: error("That file could not be read.")
        val ok = runCatching { android.graphics.Typeface.createFromFile(target) != android.graphics.Typeface.DEFAULT }.getOrDefault(false)
        if (!ok) {
            target.delete()
            throw IllegalArgumentException("Android could not read that font.")
        }
        "file:$name"
    }
}

/** The type a flat theme draws with: a built-in family, or a font file the owner added. */
fun typeFor(font: String, context: android.content.Context? = null): Typography = when {
    font.startsWith("file:") -> context?.let { ThemeFonts.family(it, font) }?.let { Typography().inFamily(it) } ?: typeFor("system")
    else -> builtInType(font)
}

private fun builtInType(font: String): Typography = when (font) {
    "inter" -> WmTypography
    "serif" -> Typography().inFamily(FontFamily.Serif)
    "mono" -> Typography().inFamily(FontFamily.Monospace)
    else -> Typography().let { type ->
        // Material's tracking was set for Roboto at low density. Tighter reads better on today's
        // screens and in the fonts phones ship with now.
        type.copy(
            bodyLarge = type.bodyLarge.copy(letterSpacing = 0.15.sp),
            bodyMedium = type.bodyMedium.copy(letterSpacing = 0.1.sp),
            bodySmall = type.bodySmall.copy(letterSpacing = 0.2.sp),
            labelLarge = type.labelLarge.copy(letterSpacing = 0.05.sp),
            labelMedium = type.labelMedium.copy(letterSpacing = 0.2.sp),
        )
    }
}

private fun shapesFor(corners: Int): MaterialShapes = MaterialShapes(
    extraSmall = RoundedCornerShape((corners / 3).dp),
    small = RoundedCornerShape((corners * 2 / 3).dp),
    medium = RoundedCornerShape(corners.dp),
    large = RoundedCornerShape((corners * 4 / 3).dp),
    extraLarge = RoundedCornerShape((corners * 7 / 3).dp),
)

/**
 * The palette and Material roles for one set of choices. Plain code rather than a composable, so
 * the home screen widgets draw in the same colours as the app.
 */
fun resolveTheme(
    context: android.content.Context,
    dark: Boolean,
    black: Boolean,
    flat: Boolean,
    accent: Accent,
    themeId: String,
    theme: ThemeFile?,
): Pair<Palette, ColorScheme> {
    val colors = theme?.let { if (dark) it.dark ?: it.light else it.light ?: it.dark }
    return when {
        themeId == BuiltInThemes.SYSTEM && Build.VERSION.SDK_INT >= Build.VERSION_CODES.S -> {
            val scheme = if (dark) dynamicDarkColorScheme(context) else dynamicLightColorScheme(context)
            val made = scheme.toPalette(dark, black)
            val finished = if (black && dark) scheme.copy(surface = Color.Black, background = Color.Black) else scheme
            (if (flat) made else made.copy(flat = false)) to finished
        }
        // Glass with no theme chosen keeps the colours it was drawn in.
        !flat && (themeId == BuiltInThemes.SYSTEM || themeId == BuiltInThemes.DEFAULT) -> {
            val made = if (dark) darkPalette(accent, black) else lightPalette(accent)
            made to made.scheme()
        }
        else -> {
            val source = colors ?: if (dark) BuiltInThemes.all.first().dark!! else BuiltInThemes.all.first().light!!
            // A theme with one side, on a phone asking for the other, stays what it is.
            val made = source.toPalette(if (colors != null) colors === theme?.dark else dark, black, flat)
            made to if (flat) made.flatScheme(source) else made.scheme()
        }
    }
}

/** The app's colours and type. [style] is `standard` or `glass`; see DESIGN.md for why. */
@Composable
fun PassTheme(
    look: Look = Look.SYSTEM,
    accent: Accent = Accent.EMERALD,
    themeId: String = BuiltInThemes.DEFAULT,
    style: String = "standard",
    custom: List<ThemeFile> = emptyList(),
    content: @Composable () -> Unit,
) {
    val systemDark = isSystemInDarkTheme()
    val context = LocalContext.current
    val dark = when (look) {
        Look.SYSTEM -> systemDark
        Look.DARK, Look.OBSIDIAN -> true
        Look.LIGHT -> false
    }
    val black = look == Look.OBSIDIAN
    val flat = style != "glass"
    val theme = remember(themeId, custom) { (BuiltInThemes.all + custom).firstOrNull { it.id == themeId } }
    val resolved = remember(dark, black, flat, accent, themeId, theme) { resolveTheme(context, dark, black, flat, accent, themeId, theme) }
    val type = remember(flat, theme) { if (flat) typeFor(theme?.font ?: "system", context) else WmTypography }
    val shapes = remember(flat, theme) { if (flat) shapesFor(theme?.corners ?: 12) else WmShapes }
    val prefs = com.wasimaster.wmpass.app.Prefs.of(context)
    CompositionLocalProvider(
        LocalPalette provides resolved.first,
        com.wasimaster.wmpass.ui.parts.LocalSiteIcons provides com.wasimaster.wmpass.ui.parts.SiteIconLook(prefs.siteIconStyle.value, prefs.siteIconShape.value, prefs.iconSources, prefs.siteIconFrom.value),
        com.wasimaster.wmsuite.settings.LocalWmPlainGroups provides (flat && !prefs.settingsCards.value),
        com.wasimaster.wmsuite.settings.LocalWmBareTiles provides (prefs.settingsIcons.value == "bare"),
    ) {
        MaterialTheme(colorScheme = resolved.second, typography = type, shapes = shapes, content = content)
    }
}

val palette: Palette
    @Composable get() = LocalPalette.current

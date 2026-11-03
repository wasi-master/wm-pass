package com.wasimaster.wmpass.ui.theme

import android.util.Base64
import androidx.compose.ui.graphics.Color
import kotlin.math.max
import kotlin.math.min
import kotlin.math.pow
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json

/** One side of a theme, light or dark. Colours are `#RRGGBB`. Anything left empty is worked out. */
@Serializable
data class ThemeColors(
    val background: String,
    val surface: String,
    val surfaceHigh: String,
    val surfaceHighest: String,
    val text: String,
    val textMid: String,
    val textLow: String,
    val primary: String,
    val danger: String,
    val warn: String,
    val info: String,
    val onPrimary: String = "",
    val primaryContainer: String = "",
    val secondaryContainer: String = "",
    val outline: String = "",
    val digit: String = "",
    val symbol: String = "",
)

/** A theme a person can pick, edit and pass on. Holds colours and a few choices, never code. */
@Serializable
data class ThemeFile(
    val id: String,
    val name: String,
    val light: ThemeColors? = null,
    val dark: ThemeColors? = null,
    /** The radius of a medium shape, in dp. Everything else scales from it. */
    val corners: Int = 12,
    /** `system`, `inter`, `serif` or `mono`. */
    val font: String = "system",
    val format: String = "wmpass-theme",
    val version: Int = 1,
)

private fun tc(
    bg: String,
    s1: String,
    s2: String,
    s3: String,
    text: String,
    mid: String,
    low: String,
    primary: String,
    danger: String,
    warn: String,
    info: String,
    onPrimary: String = "",
    primaryContainer: String = "",
    secondaryContainer: String = "",
    outline: String = "",
) = ThemeColors(bg, s1, s2, s3, text, mid, low, primary, danger, warn, info, onPrimary, primaryContainer, secondaryContainer, outline)

/** Themes that come with the app. Colours are each project's published palette. */
object BuiltInThemes {
    /** The phone's own wallpaper colours, on Android 12 and later. Has no colours of its own. */
    const val SYSTEM = "system"
    const val DEFAULT = "wmpass"

    private val wmLight = tc(
        "#F4FBF8", "#EEF5F2", "#E3EAE7", "#DDE4E1", "#171D1B", "#3F4947", "#6F7977",
        "#006A60", "#BA1A1A", "#8A5300", "#0B5FA5",
        onPrimary = "#FFFFFF", primaryContainer = "#BDEBE1", secondaryContainer = "#CCE8E2", outline = "#6F7977",
    )
    private val wmDark = tc(
        "#0E1513", "#171D1B", "#252B2A", "#303635", "#DDE4E1", "#BEC9C6", "#899391",
        "#82D5C8", "#FFB4AB", "#FFB95C", "#9CCBFB",
        onPrimary = "#003731", primaryContainer = "#005048", secondaryContainer = "#334B47", outline = "#899391",
    )

    val all: List<ThemeFile> = listOf(
        ThemeFile(DEFAULT, "WM Pass", wmLight, wmDark),
        ThemeFile(
            "high_contrast", "High contrast",
            tc("#FFFFFF", "#F5F5F5", "#E8E8E8", "#DCDCDC", "#000000", "#1F1F1F", "#3D3D3D", "#00504A", "#930006", "#5C3700", "#003F7A"),
            tc("#000000", "#101010", "#1E1E1E", "#2B2B2B", "#FFFFFF", "#E8E8E8", "#BDBDBD", "#7FF5E3", "#FFB4AB", "#FFD08A", "#A9D3FF"),
        ),
        ThemeFile(
            "catppuccin", "Catppuccin",
            tc("#EFF1F5", "#E6E9EF", "#DCE0E8", "#CCD0DA", "#4C4F69", "#5C5F77", "#6C6F85", "#8839EF", "#D20F39", "#DF8E1D", "#1E66F5"),
            tc("#1E1E2E", "#181825", "#313244", "#45475A", "#CDD6F4", "#BAC2DE", "#A6ADC8", "#CBA6F7", "#F38BA8", "#F9E2AF", "#89B4FA"),
        ),
        ThemeFile(
            "catppuccin_frappe", "Catppuccin Frappé", null,
            tc("#303446", "#292C3C", "#414559", "#51576D", "#C6D0F5", "#B5BFE2", "#A5ADCE", "#CA9EE6", "#E78284", "#E5C890", "#8CAAEE"),
        ),
        ThemeFile(
            "catppuccin_macchiato", "Catppuccin Macchiato", null,
            tc("#24273A", "#1E2030", "#363A4F", "#494D64", "#CAD3F5", "#B8C0E0", "#A5ADCB", "#C6A0F6", "#ED8796", "#EED49F", "#8AADF4"),
        ),
        ThemeFile(
            "dracula", "Dracula", null,
            tc("#282A36", "#21222C", "#343746", "#44475A", "#F8F8F2", "#D5D6DB", "#9AA1C2", "#BD93F9", "#FF5555", "#FFB86C", "#8BE9FD"),
        ),
        ThemeFile(
            "nord", "Nord",
            tc("#ECEFF4", "#E5E9F0", "#D8DEE9", "#CBD3E1", "#2E3440", "#3B4252", "#4C566A", "#5E81AC", "#BF616A", "#B5763A", "#4C7A99"),
            tc("#2E3440", "#3B4252", "#434C5E", "#4C566A", "#ECEFF4", "#D8DEE9", "#AEB7C7", "#88C0D0", "#BF616A", "#EBCB8B", "#81A1C1"),
        ),
        ThemeFile(
            "solarized", "Solarized",
            tc("#FDF6E3", "#F6EFD8", "#EEE8D5", "#E4DDC8", "#073642", "#586E75", "#657B83", "#268BD2", "#DC322F", "#B58900", "#2AA198"),
            tc("#002B36", "#073642", "#0E404D", "#154A58", "#EEE8D5", "#93A1A1", "#839496", "#268BD2", "#DC322F", "#B58900", "#2AA198"),
        ),
        ThemeFile(
            "gruvbox", "Gruvbox",
            tc("#FBF1C7", "#F2E5BC", "#EBDBB2", "#D5C4A1", "#3C3836", "#504945", "#7C6F64", "#AF3A03", "#9D0006", "#B57614", "#076678"),
            tc("#282828", "#32302F", "#3C3836", "#504945", "#EBDBB2", "#D5C4A1", "#A89984", "#FABD2F", "#FB4934", "#FE8019", "#83A598"),
        ),
        ThemeFile(
            "tokyo_night", "Tokyo Night",
            tc("#E1E2E7", "#D5D6DB", "#CBCCD1", "#C1C2C7", "#343B58", "#4C505E", "#6172B0", "#2E7DE9", "#F52A65", "#8C6C3E", "#007197"),
            tc("#1A1B26", "#16161E", "#24283B", "#292E42", "#C0CAF5", "#A9B1D6", "#7A83AD", "#7AA2F7", "#F7768E", "#E0AF68", "#7DCFFF"),
        ),
        ThemeFile(
            "one", "One",
            tc("#FAFAFA", "#F0F0F1", "#E5E5E6", "#DBDBDC", "#383A42", "#4F525D", "#696C77", "#4078F2", "#E45649", "#C18401", "#0184BC"),
            tc("#282C34", "#21252B", "#2C313A", "#3E4451", "#DCDFE4", "#ABB2BF", "#7F848E", "#61AFEF", "#E06C75", "#E5C07B", "#56B6C2"),
        ),
        ThemeFile(
            "rose_pine", "Rosé Pine",
            tc("#FAF4ED", "#FFFAF3", "#F2E9E1", "#E4DFDE", "#575279", "#797593", "#9893A5", "#907AA9", "#B4637A", "#EA9D34", "#286983"),
            tc("#191724", "#1F1D2E", "#26233A", "#403D52", "#E0DEF4", "#C4C1DB", "#908CAA", "#C4A7E7", "#EB6F92", "#F6C177", "#9CCFD8"),
        ),
        ThemeFile(
            "rose_pine_moon", "Rosé Pine Moon", null,
            tc("#232136", "#2A273F", "#393552", "#44415A", "#E0DEF4", "#C4C1DB", "#908CAA", "#C4A7E7", "#EB6F92", "#F6C177", "#9CCFD8"),
        ),
        ThemeFile(
            "everforest", "Everforest",
            tc("#FDF6E3", "#F4F0D9", "#EFEBD4", "#E6E2CC", "#5C6A72", "#708089", "#829181", "#8DA101", "#F85552", "#DFA000", "#3A94C5"),
            tc("#2D353B", "#343F44", "#3D484D", "#475258", "#D3C6AA", "#B9B29A", "#9DA9A0", "#A7C080", "#E67E80", "#DBBC7F", "#7FBBB3"),
        ),
        ThemeFile(
            "ayu", "Ayu",
            tc("#FCFCFC", "#F3F4F5", "#E7E8E9", "#DCDDDE", "#5C6166", "#6B7076", "#8A9199", "#FF9940", "#E65050", "#F2AE49", "#399EE6"),
            tc("#0B0E14", "#0F131A", "#131721", "#1F2430", "#BFBDB6", "#ACB6BF", "#7E8A96", "#E6B450", "#D95757", "#FF8F40", "#39BAE6"),
        ),
        ThemeFile(
            "ayu_mirage", "Ayu Mirage", null,
            tc("#1F2430", "#1C212B", "#242936", "#2D3340", "#CCCAC2", "#B8CFE6", "#8A9199", "#FFCC66", "#FF6666", "#FFAD66", "#5CCFE6"),
        ),
        ThemeFile(
            "kanagawa", "Kanagawa", null,
            tc("#1F1F28", "#16161D", "#2A2A37", "#363646", "#DCD7BA", "#C8C093", "#9A9984", "#7E9CD8", "#E46876", "#E6C384", "#7FB4CA"),
        ),
        ThemeFile(
            "monokai", "Monokai", null,
            tc("#272822", "#1E1F1C", "#3E3D32", "#49483E", "#F8F8F2", "#CFCFC2", "#9D9B86", "#A6E22E", "#F92672", "#FD971F", "#66D9EF"),
        ),
        ThemeFile(
            "github", "GitHub",
            tc("#FFFFFF", "#F6F8FA", "#EAEEF2", "#D8DEE4", "#1F2328", "#57606A", "#6E7781", "#0969DA", "#CF222E", "#9A6700", "#1A7F37"),
            tc("#0D1117", "#161B22", "#21262D", "#30363D", "#E6EDF3", "#B1BAC4", "#8B949E", "#58A6FF", "#F85149", "#D29922", "#3FB950"),
        ),
    )
}

// ---- colour arithmetic ----

private val HEX = Regex("^#[0-9a-fA-F]{6}$")

fun String.toThemeColor(): Color = Color(("FF" + removePrefix("#")).toLong(16))

fun Color.toThemeHex(): String {
    fun part(v: Float) = (v * 255f + 0.5f).toInt().coerceIn(0, 255).toString(16).padStart(2, '0')
    return "#" + part(red) + part(green) + part(blue)
}

private fun linear(v: Float): Double = if (v <= 0.03928f) v / 12.92 else ((v + 0.055) / 1.055).pow(2.4)

fun Color.relativeLuminance(): Double = 0.2126 * linear(red) + 0.7152 * linear(green) + 0.0722 * linear(blue)

fun contrast(a: Color, b: Color): Double {
    val la = a.relativeLuminance()
    val lb = b.relativeLuminance()
    return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)
}

fun mixColors(a: Color, b: Color, amount: Float) = Color(
    a.red + (b.red - a.red) * amount,
    a.green + (b.green - a.green) * amount,
    a.blue + (b.blue - a.blue) * amount,
)

/**
 * Ink for words and icons on the brand gradient, judged where a label sits on it. White alone
 * failed on a pale brand: a theme with a light blue primary had white on it at 1.7:1. The palette's
 * own colour for the purpose wins when it reads; otherwise whichever of white or black reads best.
 */
val Palette.onBrand: Color
    get() {
        val under = listOf(0.3f, 0.5f, 0.7f).map { mixColors(brandFrom, brandTo, it) }
        fun reads(ink: Color) = under.minOf { contrast(ink, it) }
        if (reads(onPrimary) >= 4.5) return onPrimary
        return listOf(onPrimary, Color.White, Color.Black).maxBy { reads(it) }
    }

/** Moves [color] towards black or white until it reads on [ground]. */
private fun readable(color: Color, ground: Color, ratio: Double): Color {
    if (contrast(color, ground) >= ratio) return color
    val target = if (ground.relativeLuminance() > 0.18) Color.Black else Color.White
    var step = 0.1f
    while (step <= 1f) {
        val tried = mixColors(color, target, step)
        if (contrast(tried, ground) >= ratio) return tried
        step += 0.1f
    }
    return target
}

/**
 * A theme someone made may be careless, or unkind: text the colour of its background would make
 * a warning unreadable. So every side passes through this, built-in or not.
 */
fun ThemeColors.readableOn(): ThemeColors {
    val bg = background.toThemeColor()
    val high = surfaceHighest.toThemeColor()
    // Checked against the lightest or darkest surface it can land on, whichever is worst.
    fun worst(color: String, ratio: Double): String {
        val first = readable(color.toThemeColor(), bg, ratio)
        return readable(first, high, ratio - 0.5).toThemeHex()
    }
    return copy(
        text = worst(text, 7.0),
        textMid = worst(textMid, 4.5),
        textLow = worst(textLow, 3.0),
        primary = worst(primary, 3.0),
        danger = worst(danger, 3.0),
        warn = worst(warn, 3.0),
        info = worst(info, 3.0),
    )
}

// ---- sharing ----

object ThemeCodec {
    private const val PREFIX = "wmpass-theme:"
    private const val LIMIT = 8 * 1024
    private val json = Json {
        ignoreUnknownKeys = true
        encodeDefaults = true
    }

    /** The file form, for saving and for the share sheet. */
    fun toText(theme: ThemeFile): String = json.encodeToString(ThemeFile.serializer(), theme)

    /** Short enough for a QR code. */
    fun toCode(theme: ThemeFile): String =
        PREFIX + Base64.encodeToString(toText(theme).toByteArray(), Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)

    private fun checked(colors: ThemeColors?): ThemeColors? {
        colors ?: return null
        val all = listOf(
            colors.background, colors.surface, colors.surfaceHigh, colors.surfaceHighest, colors.text, colors.textMid,
            colors.textLow, colors.primary, colors.danger, colors.warn, colors.info,
        )
        require(all.all(HEX::matches)) { "That theme has a colour that is not valid." }
        val optional = listOf(colors.onPrimary, colors.primaryContainer, colors.secondaryContainer, colors.outline, colors.digit, colors.symbol)
        require(optional.all { it.isEmpty() || HEX.matches(it) }) { "That theme has a colour that is not valid." }
        return colors.readableOn()
    }

    /**
     * Reads a theme file or code from anywhere. It comes from a stranger, so it gets a new id,
     * a name with nothing odd in it, numbers within range, and readable text.
     */
    fun read(text: String): ThemeFile {
        val trimmed = text.trim()
        require(trimmed.length <= LIMIT * 2) { "That is too long to be a theme." }
        val body = if (trimmed.startsWith(PREFIX)) {
            String(Base64.decode(trimmed.removePrefix(PREFIX), Base64.URL_SAFE))
        } else {
            trimmed
        }
        require(body.length <= LIMIT) { "That is too long to be a theme." }
        val parsed = runCatching { json.decodeFromString(ThemeFile.serializer(), body) }
            .getOrElse { throw IllegalArgumentException("That is not a WM Pass theme.") }
        require(parsed.format == "wmpass-theme") { "That is not a WM Pass theme." }
        require(parsed.version <= 1) { "This theme needs a newer version of WM Pass." }
        require(parsed.light != null || parsed.dark != null) { "That theme has no colours." }
        return parsed.copy(
            id = "custom_" + java.util.UUID.randomUUID().toString().take(8),
            name = parsed.name.filter { !it.isISOControl() }.trim().take(40).ifEmpty { "Shared theme" },
            light = checked(parsed.light),
            dark = checked(parsed.dark),
            corners = parsed.corners.coerceIn(0, 28),
            font = parsed.font.takeIf { it in setOf("system", "inter", "serif", "mono") } ?: "system",
        )
    }

    fun readList(text: String?): List<ThemeFile> =
        if (text.isNullOrEmpty()) {
            emptyList()
        } else {
            runCatching { json.decodeFromString(kotlinx.serialization.builtins.ListSerializer(ThemeFile.serializer()), text) }.getOrDefault(emptyList())
        }

    fun writeList(list: List<ThemeFile>): String =
        json.encodeToString(kotlinx.serialization.builtins.ListSerializer(ThemeFile.serializer()), list)
}

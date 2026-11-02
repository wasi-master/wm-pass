package com.wasimaster.wmpass.ui.parts

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.size
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.remember
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.SquircleShape
import com.wasimaster.wmpass.ui.theme.palette
import kotlin.math.abs
import kotlin.math.cos
import kotlin.math.pow
import kotlin.math.sin

/** An OKLCH colour as sRGB, clamped. Equal lightness and chroma look equally bright at any hue. */
fun oklch(l: Float, c: Float, hueDegrees: Float): Color {
    val h = Math.toRadians(hueDegrees.toDouble())
    val a = c * cos(h)
    val b = c * sin(h)
    val lp = l + 0.3963377774 * a + 0.2158037573 * b
    val mp = l - 0.1055613458 * a - 0.0638541728 * b
    val sp = l - 0.0894841775 * a - 1.2914855480 * b
    val ll = lp * lp * lp
    val mm = mp * mp * mp
    val ss = sp * sp * sp
    fun gamma(v: Double): Float {
        val clipped = v.coerceIn(0.0, 1.0)
        return (if (clipped <= 0.0031308) 12.92 * clipped else 1.055 * clipped.pow(1 / 2.4) - 0.055).toFloat()
    }
    return Color(
        gamma(4.0767416621 * ll - 3.3077115913 * mm + 0.2309699292 * ss),
        gamma(-1.2684380046 * ll + 2.6097574011 * mm - 0.3413193965 * ss),
        gamma(-0.0041960863 * ll - 0.7034186147 * mm + 1.7076147010 * ss),
    )
}

/**
 * The standard look's icon: a flat circle, as in Contacts. A known brand keeps its colour, a
 * label its hue, and everything else sits on the theme's own tone rather than a random one.
 */
@Composable
private fun FlatTile(face: TileFace, label: String?, modifier: Modifier, size: Dp) {
    val scheme = androidx.compose.material3.MaterialTheme.colorScheme
    val coloured = com.wasimaster.wmpass.ui.theme.LocalColouredIcons.current
    val hue = label?.toFloatOrNull()
    // The two ends sit either side of the brand's own colour, so their middle is close to it.
    val brandFill = mix(face.from, face.to, 0.5f)
    val showsBrand = face.brand && coloured && com.wasimaster.wmpass.ui.theme.contrast(brandFill, palette.ground) >= 1.3f
    val fill = when {
        hue != null -> oklch(0.58f, 0.11f, hue)
        showsBrand -> brandFill
        else -> scheme.secondaryContainer
    }
    val ink = when {
        hue != null -> Color.White
        showsBrand -> face.glyphTint
        else -> scheme.onSecondaryContainer
    }
    Box(modifier.size(size).clip(androidx.compose.foundation.shape.CircleShape).background(fill), contentAlignment = Alignment.Center) {
        if (face.glyph != null) {
            Icon(face.glyph, null, tint = ink, modifier = Modifier.size(size * 0.52f))
        } else {
            Text(
                face.letter.orEmpty(),
                color = ink,
                style = TextStyle(fontSize = (size.value * 0.42f).sp, fontWeight = FontWeight.Medium),
            )
        }
    }
}

/**
 * A site or title's own hue: always the same for the same name. Yellow-greens are left out,
 * because at this brightness they come out as olive and tint a whole page the colour of mud.
 */
fun hueOf(seed: String): Float {
    val raw = (abs(seed.lowercase().hashCode()) % 360).toFloat()
    return when (raw) {
        in 70f..100f -> raw - 30f
        in 100f..135f -> raw + 35f
        else -> raw
    }
}

/** The two ends of an entry's colour, from its label or its name. */
fun tileColors(seed: String, label: String?): Pair<Color, Color> {
    val hue = label?.toFloatOrNull() ?: hueOf(seed)
    return oklch(0.70f, 0.14f, hue) to oklch(0.52f, 0.15f, hue + 28f)
}

private fun luminance(color: Color) = 0.2126f * color.red + 0.7152f * color.green + 0.0722f * color.blue

private fun mix(a: Color, b: Color, amount: Float) = Color(
    a.red + (b.red - a.red) * amount,
    a.green + (b.green - a.green) * amount,
    a.blue + (b.blue - a.blue) * amount,
)

/**
 * The two ends of a brand's colour. A nearly black brand is lifted to graphite, so that it still
 * reads as a tile on a dark page, and a page tinted with it still has some light in it.
 */
fun brandColors(brand: Color): Pair<Color, Color> {
    val base = if (luminance(brand) < 0.16f) Color(0xFF3C4552) else brand
    return mix(base, Color.White, 0.14f) to mix(base, Color.Black, 0.16f)
}

/** What an entry looks like: its colours, and a mark or a letter. */
class TileFace(val from: Color, val to: Color, val glyph: ImageVector?, val letter: String?, val glyphTint: Color = Color.White, val brand: Boolean = false)

/** Everything an entry's look depends on, worked out once. */
@Composable
fun rememberFace(title: String, site: String?, kind: String, label: String? = null, icon: String? = null): TileFace {
    val context = LocalContext.current
    return remember(title, site, kind, label, icon) {
        val chosenBrand = icon?.removePrefix("brand:")?.takeIf { icon.startsWith("brand:") }?.let { Brands.bySlug(context, it) }
        val chosenGlyph = icon?.removePrefix("glyph:")?.takeIf { icon.startsWith("glyph:") }?.let { GlyphChoices[it] }
        val brand = chosenBrand ?: if (chosenGlyph == null) Brands.find(context, site, title) else null
        val seed = site ?: title.ifEmpty { kind }
        val colors = when {
            label?.toFloatOrNull() != null -> tileColors(seed, label)
            brand != null -> brandColors(brand.color)
            else -> tileColors(seed, null)
        }
        val tint = if (luminance(colors.second) > 0.62f) Color(0xFF16181C) else Color.White
        when {
            chosenGlyph != null -> TileFace(colors.first, colors.second, chosenGlyph, null, tint)
            brand != null -> TileFace(colors.first, colors.second, brand.glyph, null, tint, brand = true)
            kind != "login" && site == null -> TileFace(colors.first, colors.second, kindIcon(kind), null, tint)
            else -> {
                val letter = (site ?: title).firstOrNull { it.isLetterOrDigit() }?.uppercaseChar()?.toString()
                TileFace(colors.first, colors.second, if (letter == null) kindIcon(kind) else null, letter, tint)
            }
        }
    }
}

/** Glyphs a person can give an entry instead of its automatic face. */
val GlyphChoices: Map<String, ImageVector> by lazy {
    mapOf(
        "key" to PassIcons.Key,
        "shield" to PassIcons.ShieldFilled,
        "lock" to PassIcons.LockFilled,
        "star" to PassIcons.StarFilled,
        "heart" to PassIcons.FavoriteFilled,
        "bolt" to PassIcons.BoltFilled,
        "fire" to PassIcons.LocalFireDepartmentFilled,
        "person" to PassIcons.Person,
        "family" to PassIcons.FamilyRestroom,
        "mail" to PassIcons.Mail,
        "card" to PassIcons.CreditCardFilled,
        "folder" to PassIcons.FolderFilled,
        "note" to PassIcons.StickyNote2,
        "terminal" to PassIcons.Terminal,
        "database" to PassIcons.Database,
        "router" to PassIcons.Router,
        "wifi" to PassIcons.Wifi,
        "devices" to PassIcons.Devices,
        "globe" to PassIcons.Language,
        "contract" to PassIcons.Contract,
        "badge" to PassIcons.Badge,
        "sparkle" to PassIcons.AutoAwesomeFilled,
        "home" to PassIcons.HomeStorage,
        "hub" to PassIcons.Hub,
    )
}

fun kindIcon(kind: String): ImageVector = when (kind) {
    "login" -> PassIcons.Key
    "card" -> PassIcons.CreditCard
    "identity" -> PassIcons.Badge
    "note" -> PassIcons.StickyNote2
    "document" -> PassIcons.Description
    "ssh" -> PassIcons.Terminal
    "api" -> PassIcons.Api
    "database" -> PassIcons.Database
    "licence" -> PassIcons.License
    "recovery" -> PassIcons.EmergencyHome
    "wifi" -> PassIcons.Wifi
    "otp" -> PassIcons.Pin
    else -> PassIcons.Key
}

fun kindName(kind: String): String = when (kind) {
    "login" -> "Login"
    "card" -> "Card"
    "identity" -> "Identity"
    "note" -> "Note"
    "document" -> "Document"
    "ssh" -> "SSH key"
    "api" -> "API key"
    "database" -> "Database"
    "licence" -> "Licence"
    "recovery" -> "Recovery"
    "wifi" -> "Wi-Fi"
    "otp" -> "Code"
    else -> kind
}

/**
 * An entry's face: a continuous-cornered tile in its site's own colour, with the site's mark if
 * it is one we know, the first letter for anything else that has a name, and the kind's glyph
 * for anything that does not.
 */
@Composable
fun SiteTile(
    title: String,
    site: String?,
    kind: String,
    modifier: Modifier = Modifier,
    size: Dp = 46.dp,
    label: String? = null,
    icon: String? = null,
) {
    val colors = palette
    val face = rememberFace(title, site, kind, label, icon)
    if (colors.flat) {
        FlatTile(face, label, modifier, size)
        return
    }
    val shape = remember(size) { SquircleShape(size * 0.30f) }
    Box(
        modifier
            .size(size)
            .clip(shape)
            .background(Brush.linearGradient(listOf(face.from, face.to)))
            .rim(shape, colors),
        contentAlignment = Alignment.Center,
    ) {
        if (face.glyph != null) {
            Icon(face.glyph, null, tint = face.glyphTint, modifier = Modifier.size(size * 0.54f))
        } else {
            Text(
                face.letter.orEmpty(),
                color = face.glyphTint,
                style = TextStyle(fontSize = (size.value * 0.44f).sp, fontWeight = FontWeight.Bold),
            )
        }
    }
}

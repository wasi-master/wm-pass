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

/** How entries' icons are drawn. See Settings, Icons. */
@androidx.compose.runtime.Immutable
data class SiteIconLook(val style: String = "brand", val shape: String = "circle")

val LocalSiteIcons = androidx.compose.runtime.staticCompositionLocalOf { SiteIconLook() }

/**
 * The standard look's icon. By default a flat circle, as in Contacts, where a known brand keeps
 * its colour. A label's hue always wins, because someone chose it for that one entry.
 */
@Composable
private fun FlatTile(face: TileFace, name: String, label: String?, modifier: Modifier, size: Dp) {
    val scheme = androidx.compose.material3.MaterialTheme.colorScheme
    val colors = palette
    val look = LocalSiteIcons.current
    val hue = label?.toFloatOrNull()
    // The two ends sit either side of the brand's own colour, so their middle is close to it.
    val brandColour = mix(face.from, face.to, 0.5f)
    val bare = look.shape == "none"
    val glyph = if (look.style == "letters" && face.brand) null else face.glyph
    val letter = face.letter ?: name.firstOrNull { it.isLetterOrDigit() }?.uppercaseChar()?.toString().orEmpty()
    val brandShown = face.brand && hue == null && look.style != "uniform" && look.style != "letters"
    val neutral = scheme.surfaceContainerHighest
    val fill = when {
        bare -> Color.Transparent
        hue != null -> oklch(0.58f, 0.11f, hue)
        brandShown && look.style == "brand" && com.wasimaster.wmpass.ui.theme.contrast(brandColour, colors.ground) >= 1.3f -> brandColour
        brandShown && look.style == "tinted" -> neutral
        else -> scheme.secondaryContainer
    }
    val ink = when {
        hue != null -> if (bare) oklch(0.62f, 0.14f, hue) else Color.White
        brandShown && !bare && fill == brandColour -> face.glyphTint
        brandShown && com.wasimaster.wmpass.ui.theme.contrast(brandColour, if (bare) colors.ground else neutral) >= 2.2f -> brandColour
        bare -> scheme.onSurfaceVariant
        fill == neutral -> scheme.onSurface
        else -> scheme.onSecondaryContainer
    }
    val shape = when (look.shape) {
        "rounded" -> androidx.compose.foundation.shape.RoundedCornerShape(size * 0.28f)
        "square" -> androidx.compose.foundation.shape.RoundedCornerShape(size * 0.12f)
        else -> androidx.compose.foundation.shape.CircleShape
    }
    Box(modifier.size(size).clip(shape).background(fill), contentAlignment = Alignment.Center) {
        if (glyph != null) {
            Icon(glyph, null, tint = ink, modifier = Modifier.size(size * if (bare) 0.72f else 0.52f))
        } else {
            Text(
                letter,
                color = ink,
                style = TextStyle(fontSize = (size.value * if (bare) 0.56f else 0.42f).sp, fontWeight = FontWeight.Medium),
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
    return remember(title, site, kind, label, icon, PassIcons.style) {
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
/**
 * Every glyph an item or a tag can wear, by the id saved for it. The first ones were the original
 * set and keep their short ids; the rest are named after their Material Symbol.
 */
val GlyphChoices: Map<String, ImageVector>
    get() = linkedMapOf(
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
        "shopping_cart" to PassIcons.ShoppingCart,
        "storefront" to PassIcons.Storefront,
        "shopping_bag" to PassIcons.ShoppingBag,
        "restaurant" to PassIcons.Restaurant,
        "local_cafe" to PassIcons.LocalCafe,
        "flight" to PassIcons.Flight,
        "hotel" to PassIcons.Hotel,
        "luggage" to PassIcons.Luggage,
        "directions_car" to PassIcons.DirectionsCar,
        "train" to PassIcons.Train,
        "directions_bus" to PassIcons.DirectionsBus,
        "local_shipping" to PassIcons.LocalShipping,
        "local_gas_station" to PassIcons.LocalGasStation,
        "school" to PassIcons.School,
        "work" to PassIcons.Work,
        "account_balance" to PassIcons.AccountBalance,
        "savings" to PassIcons.Savings,
        "payments" to PassIcons.Payments,
        "wallet" to PassIcons.Wallet,
        "currency_bitcoin" to PassIcons.CurrencyBitcoin,
        "toll" to PassIcons.Toll,
        "sports_esports" to PassIcons.SportsEsports,
        "movie" to PassIcons.Movie,
        "music_note" to PassIcons.MusicNote,
        "headphones" to PassIcons.Headphones,
        "live_tv" to PassIcons.LiveTv,
        "podcasts" to PassIcons.Podcasts,
        "subscriptions" to PassIcons.Subscriptions,
        "photo_camera" to PassIcons.PhotoCamera,
        "videocam" to PassIcons.Videocam,
        "mic" to PassIcons.Mic,
        "call" to PassIcons.Call,
        "smartphone" to PassIcons.Smartphone,
        "laptop_mac" to PassIcons.LaptopMac,
        "desktop_windows" to PassIcons.DesktopWindows,
        "watch" to PassIcons.Watch,
        "cast" to PassIcons.Cast,
        "chat" to PassIcons.Chat,
        "forum" to PassIcons.Forum,
        "contacts" to PassIcons.Contacts,
        "public" to PassIcons.Public,
        "map" to PassIcons.Map,
        "location_on" to PassIcons.LocationOn,
        "home" to PassIcons.Home,
        "apartment" to PassIcons.Apartment,
        "local_hospital" to PassIcons.LocalHospital,
        "medication" to PassIcons.Medication,
        "fitness_center" to PassIcons.FitnessCenter,
        "pets" to PassIcons.Pets,
        "child_care" to PassIcons.ChildCare,
        "local_grocery_store" to PassIcons.LocalGroceryStore,
        "sports_soccer" to PassIcons.SportsSoccer,
        "menu_book" to PassIcons.MenuBook,
        "newspaper" to PassIcons.Newspaper,
        "science" to PassIcons.Science,
        "code" to PassIcons.Code,
        "bug_report" to PassIcons.BugReport,
        "memory" to PassIcons.Memory,
        "storage" to PassIcons.Storage,
        "cloud" to PassIcons.Cloud,
        "hard_drive" to PassIcons.HardDrive,
        "bluetooth" to PassIcons.Bluetooth,
        "print" to PassIcons.Print,
        "gavel" to PassIcons.Gavel,
        "policy" to PassIcons.Policy,
        "verified_user" to PassIcons.VerifiedUser,
        "admin_panel_settings" to PassIcons.AdminPanelSettings,
        "support_agent" to PassIcons.SupportAgent,
        "volunteer_activism" to PassIcons.VolunteerActivism,
        "celebration" to PassIcons.Celebration,
        "cake" to PassIcons.Cake,
        "redeem" to PassIcons.Redeem,
        "rocket_launch" to PassIcons.RocketLaunch,
        "construction" to PassIcons.Construction,
        "water_drop" to PassIcons.WaterDrop,
        "eco" to PassIcons.Eco,
        "park" to PassIcons.Park,
        "beach_access" to PassIcons.BeachAccess,
        "flag" to PassIcons.Flag,
        "translate" to PassIcons.Translate,
        "brush" to PassIcons.Brush,
        "photo_library" to PassIcons.PhotoLibrary,
        "theater_comedy" to PassIcons.TheaterComedy,
        "smart_toy" to PassIcons.SmartToy,
        "face" to PassIcons.Face,
        "calendar_month" to PassIcons.CalendarMonth,
        "alarm" to PassIcons.Alarm,
        "hiking" to PassIcons.Hiking,
        "kitchen" to PassIcons.Kitchen,
        "bed" to PassIcons.Bed,
        "power" to PassIcons.Power,
        "electric_car" to PassIcons.ElectricCar,
        "casino" to PassIcons.Casino,
        "ecg_heart" to PassIcons.EcgHeart,
        "widgets" to PassIcons.Widgets,
        "key_vertical" to PassIcons.KeyVertical,
        "password" to PassIcons.Password,
        "lock_open" to PassIcons.LockOpen,
        "fingerprint" to PassIcons.Fingerprint,
        "description" to PassIcons.Description,
        "api" to PassIcons.Api,
        "license" to PassIcons.License,
        "qr_code_2" to PassIcons.QrCode2,
        "qr_code_scanner" to PassIcons.QrCodeScanner,
        "link" to PassIcons.Link,
        "alternate_email" to PassIcons.AlternateEmail,
        "group" to PassIcons.Group,
        "cloud_upload" to PassIcons.CloudUpload,
        "palette" to PassIcons.Palette,
        "inventory_2" to PassIcons.Inventory2,
        "backup" to PassIcons.Backup,
        "attach_file" to PassIcons.AttachFile,
        "gpp_good" to PassIcons.GppGood,
        "gpp_bad" to PassIcons.GppBad,
        "gpp_maybe" to PassIcons.GppMaybe,
        "encrypted" to PassIcons.Encrypted,
        "account_circle" to PassIcons.AccountCircle,
        "nfc" to PassIcons.Nfc,
        "usb" to PassIcons.Usb,
        "radar" to PassIcons.Radar,
        "visibility_lock" to PassIcons.VisibilityLock,
        "phishing" to PassIcons.Phishing,
        "dns" to PassIcons.Dns,
        "vpn_key" to PassIcons.VpnKey,
        "security_key" to PassIcons.SecurityKey,
        "receipt_long" to PassIcons.ReceiptLong,
        "emergency_home" to PassIcons.EmergencyHome,
        "cloud_sync" to PassIcons.CloudSync,
        "contactless" to PassIcons.Contactless,
        "keyboard" to PassIcons.Keyboard,
        "numbers" to PassIcons.Numbers,
        "check_circle" to PassIcons.CheckCircle,
        "balance" to PassIcons.Balance,
        "mobile_lock_portrait" to PassIcons.MobileLockPortrait,
        "screen_lock_portrait" to PassIcons.ScreenLockPortrait,
        "folder_open" to PassIcons.FolderOpen,
        "qr_code" to PassIcons.QrCode,
        "password_2" to PassIcons.Password2,
        "lock_reset" to PassIcons.LockReset,
        "lock_clock" to PassIcons.LockClock,
        "person_search" to PassIcons.PersonSearch,
        "mark_email_read" to PassIcons.MarkEmailRead,
        "database_upload" to PassIcons.DatabaseUpload,
        "sentiment_satisfied" to PassIcons.SentimentSatisfied,
    )

/** What a glyph is called, for the long press in the picker. */
fun glyphName(id: String): String = GlyphNames[id] ?: id.replace('_', ' ').replaceFirstChar { it.uppercase() }

private val GlyphNames = mapOf(
    "shopping_cart" to "Cart",
    "storefront" to "Shop",
    "shopping_bag" to "Bag",
    "local_cafe" to "Cafe",
    "directions_car" to "Car",
    "directions_bus" to "Bus",
    "local_shipping" to "Delivery",
    "local_gas_station" to "Fuel",
    "account_balance" to "Bank",
    "currency_bitcoin" to "Bitcoin",
    "sports_esports" to "Games",
    "live_tv" to "TV",
    "photo_camera" to "Camera",
    "laptop_mac" to "Laptop",
    "desktop_windows" to "Desktop",
    "location_on" to "Place",
    "local_hospital" to "Hospital",
    "fitness_center" to "Fitness",
    "child_care" to "Child",
    "local_grocery_store" to "Groceries",
    "sports_soccer" to "Soccer",
    "menu_book" to "Book",
    "bug_report" to "Bug",
    "hard_drive" to "Drive",
    "verified_user" to "Verified",
    "admin_panel_settings" to "Admin",
    "support_agent" to "Support",
    "volunteer_activism" to "Charity",
    "rocket_launch" to "Rocket",
    "water_drop" to "Water",
    "beach_access" to "Beach",
    "photo_library" to "Photos",
    "theater_comedy" to "Theater",
    "smart_toy" to "Robot",
    "calendar_month" to "Calendar",
    "casino" to "Dice",
    "ecg_heart" to "Health",
    "api" to "API",
    "qr_code_2" to "QR code",
    "qr_code_scanner" to "QR scanner",
    "alternate_email" to "At sign",
    "cloud_upload" to "Cloud upload",
    "inventory_2" to "Box",
    "gpp_good" to "Protected",
    "gpp_bad" to "Unprotected",
    "gpp_maybe" to "Unsure",
    "account_circle" to "Account",
    "nfc" to "NFC",
    "usb" to "USB",
    "dns" to "Server",
    "vpn_key" to "VPN key",
    "security_key" to "Security key",
    "receipt_long" to "Receipt",
    "emergency_home" to "Emergency",
    "cloud_sync" to "Cloud sync",
    "mobile_lock_portrait" to "Phone lock",
    "screen_lock_portrait" to "Screen lock",
    "qr_code" to "QR code (small)",
    "password_2" to "Password (dots)",
    "mark_email_read" to "Mail read",
    "database_upload" to "Database upload",
    "sentiment_satisfied" to "Smile",
)

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
    "licence" -> "License"
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
        FlatTile(face, title, label, modifier, size)
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

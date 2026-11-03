package com.wasimaster.wmpass.ui.vault

import androidx.compose.ui.graphics.vector.ImageVector
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlyphChoices
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.put

/**
 * What a tag is drawn with: the glyph picked for it, or one that suits its name. The picks live in
 * the vault, since a tag's name says something about what is in it.
 */
object TagIcons {
    fun of(tag: String, picked: Map<String, String>): ImageVector =
        picked[tag]?.let { GlyphChoices[it] } ?: automatic(tag)

    /** For the names the app suggests, and the ones people tend to type. */
    fun automatic(tag: String): ImageVector = when (tag.trim().lowercase()) {
        "authenticator", "2fa", "codes", "otp" -> PassIcons.Pin
        "social", "friends", "chat" -> PassIcons.Group
        "banking", "bank", "finance", "money" -> PassIcons.AccountBalance
        "payment cards", "cards" -> PassIcons.CreditCard
        "shopping", "shop", "stores" -> PassIcons.ShoppingCart
        "developer", "dev", "code", "coding" -> PassIcons.Code
        "email", "mail" -> PassIcons.Mail
        "work", "job", "office" -> PassIcons.Work
        "home", "house" -> PassIcons.HomeStorage
        "family", "kids" -> PassIcons.FamilyRestroom
        "gaming", "games" -> PassIcons.SportsEsports
        "entertainment", "streaming", "media", "tv" -> PassIcons.LiveTv
        "music" -> PassIcons.MusicNote
        "travel", "trips" -> PassIcons.Flight
        "health", "medical" -> PassIcons.EcgHeart
        "education", "school", "university" -> PassIcons.School
        "government", "tax", "taxes" -> PassIcons.Gavel
        "utilities", "bills" -> PassIcons.ReceiptLong
        "crypto", "bitcoin" -> PassIcons.CurrencyBitcoin
        "cloud", "hosting" -> PassIcons.Cloud
        "network", "wi-fi", "wifi" -> PassIcons.Wifi
        "servers", "server" -> PassIcons.Dns
        "databases" -> PassIcons.Database
        "ssh keys", "ssh" -> PassIcons.Terminal
        "api keys", "api" -> PassIcons.Api
        "identities", "id" -> PassIcons.Badge
        "documents", "docs" -> PassIcons.Description
        "notes" -> PassIcons.StickyNote2
        "licenses", "licences", "software" -> PassIcons.License
        "recovery", "backup codes" -> PassIcons.EmergencyHome
        "personal", "me" -> PassIcons.Person
        "shared" -> PassIcons.Group
        "important", "favorites" -> PassIcons.Star
        else -> PassIcons.Sell
    }

    /** A short pick for tags. The item picker has the rest. */
    val choices: List<String> = listOf(
        "star", "heart", "work", "home", "family", "person", "group", "mail", "chat", "account_balance", "card",
        "shopping_cart", "code", "terminal", "cloud", "database", "wifi", "school", "flight", "sports_esports", "live_tv",
        "music_note", "ecg_heart", "gavel", "receipt_long", "currency_bitcoin", "lock", "shield", "key", "bolt", "fire",
    )

    suspend fun set(tag: String, glyph: String?) {
        Pass.change("tags.set_icon") {
            put("tag", tag)
            if (glyph == null) put("icon", JsonNull) else put("icon", glyph)
        }
    }

    /** Carries a tag's glyph over to its new name. */
    suspend fun rename(from: String, to: String) {
        val glyph = Pass.tagIcons.value[from] ?: return
        set(to, glyph)
        set(from, null)
    }
}

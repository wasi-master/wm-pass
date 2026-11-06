package com.wasimaster.wmpass.system

import com.wasimaster.wmpass.core.Detail
import com.wasimaster.wmpass.core.FieldView
import com.wasimaster.wmpass.core.Summary
import java.time.Instant
import java.time.ZoneOffset
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonObjectBuilder
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/**
 * What [KeyboardProvider] says to WM Keyboard's WM Pass tool, and what the tool may ask. WM
 * Keyboard reads the same format in its own `VaultWire`. A change here is a change there too.
 *
 * Every call is `ContentResolver.call` on `content://com.wasimaster.wmpass.keyboard`, and every
 * reply carries [STATE]: `open`, `locked`, `none` (no vault yet) or `off` (Let WM Keyboard fill is
 * off). Only an `open` reply carries anything else. The calls the tool makes:
 *
 * - [STATUS]: nothing but [STATE], to learn whether the vault is open.
 * - [ITEMS], arg the package of the app in front, extra [QUERY]: [ROWS], a JSON array of the
 *   vault's items, every kind. Those saved for the app in front come first, then the rest by name,
 *   or in search order when [QUERY] is not empty. Each row is `id`, `kind`, `title`, `subtitle`,
 *   `matches`, `has_password`, `has_code` and `favourite`. No secret.
 * - [ITEM], arg an item's id: [PAGE], one JSON object, `id`, `kind`, `title`, `subtitle`,
 *   `fields`, and `changes` when the item has a password [SET_PASSWORD] can change. Each field is
 *   `key`, `label`, `secret`, and `value` only when it is not secret. Besides the kind's own
 *   fields, the keys [SITE] plus an index (a website), [CUSTOM] plus an index (a field the person
 *   added), [CODE] and [NOTES] name the rest of what the item holds.
 * - [VALUE], arg an id, extra [FIELD]: the one value, as [VALUE] in the reply, for the keyboard to
 *   type. It is the only call that hands a secret to the keyboard, and it comes only when the
 *   person taps to type that one field.
 * - [COPY], arg an id, extra [FIELD]: WM Pass copies the value itself, marked sensitive and wiped
 *   after the time set here, so a copied secret never passes through the keyboard. [COPIED] says
 *   whether it did.
 * - [ADD_LOGIN], extras [TITLE], [USERNAME], [PASSWORD], [WEBSITE] and [APP]: a new login, its id
 *   as [ID]. [APP], the app in front, is linked by its signing certificate unless it is a
 *   browser, where one app is every site.
 * - [SET_PASSWORD], arg an id, extra [PASSWORD]: the item's new password; the old one stays in its
 *   history. [SAVED] says whether it was kept.
 * - [GENERATE]: a new password from the generator as set up here, as [VALUE].
 *
 * A `locked` reply to [STATUS] or [ITEMS] also carries [FINGERPRINT], whether the fingerprint can
 * open the vault now, and [RETRY_AT], when a password may next be tried (0 for now). An `open`
 * one carries [VAULT], the open vault's name. Three calls work on a locked vault:
 *
 * - [UNLOCK_PROMPT]: [INTENT], a pending intent that asks for the fingerprint over the app in
 *   front, or the door when the fingerprint is not set up. The keyboard only starts it.
 * - [UNLOCK], extra [SECRET], the master password as UTF-8 bytes, wiped once read: [OUTCOME] is
 *   `opened`, `wrong`, `waiting` (with [RETRY_AT]), `factor` (a key file or hardware key is
 *   needed, so the door it is) or `failed`.
 * - [LOCK]: locks the vault.
 *
 * Opening an item in WM Pass is not a call: the keyboard starts WM Pass with the extra
 * [ROUTE_EXTRA] set to [openRoute], and the item opens once the vault is.
 */
object VaultWire {
    const val VERSION = 1

    const val STATE = "state"
    const val STATUS = "status"
    const val ITEMS = "items"
    const val ITEM = "item"
    const val VALUE = "value"
    const val COPY = "copy"
    const val ADD_LOGIN = "add_login"
    const val SET_PASSWORD = "set_password"
    const val GENERATE = "generate"
    const val UNLOCK_PROMPT = "unlock_prompt"
    const val UNLOCK = "unlock"
    const val LOCK = "lock"

    const val QUERY = "query"
    const val FIELD = "field"
    const val TITLE = "title"
    const val USERNAME = "username"
    const val PASSWORD = "password"
    const val WEBSITE = "website"
    const val APP = "app"

    const val ROWS = "rows"
    const val PAGE = "item"
    const val COPIED = "copied"
    const val ID = "id"
    const val SAVED = "saved"
    const val FINGERPRINT = "fingerprint"
    const val RETRY_AT = "retry_at"
    const val VAULT = "vault"
    const val INTENT = "intent"
    const val SECRET = "secret"
    const val OUTCOME = "outcome"

    const val CODE = "code"
    const val NOTES = "notes"
    const val SITE = "site:"
    const val CUSTOM = "custom:"

    const val ROUTE_EXTRA = "route"

    fun openRoute(id: String): String = "item:$id"

    /** [entries] as the rows of [ITEMS], each marked when its id is in [matched]. */
    fun rows(entries: List<Summary>, matched: Set<String>): String = buildJsonArray {
        entries.forEach { entry ->
            addJsonObject {
                put("id", entry.id)
                put("kind", entry.kind)
                put("title", entry.title)
                put("subtitle", entry.subtitle)
                put("matches", entry.id in matched)
                put("has_password", entry.hasPassword)
                put("has_code", entry.hasOtp)
                put("favourite", entry.favourite)
            }
        }
    }.toString()

    /** [detail] as the page of [ITEM]. [hasCode] comes from the item's summary, since the detail's code is a view of its own. */
    fun page(detail: Detail, hasCode: Boolean): String = buildJsonObject {
        put("v", VERSION)
        put("id", detail.id)
        put("kind", detail.kind)
        put("title", detail.title)
        put("subtitle", detail.subtitle)
        putJsonArray("fields") {
            detail.fields.forEach { field ->
                if (field.secret) {
                    if (field.hasValue) addJsonObject { entry(field.key, field.label, secret = true) }
                } else {
                    val text = plain(field)
                    if (text != null) addJsonObject { entry(field.key, field.label, secret = false, text) }
                }
            }
            detail.sites.forEachIndexed { index, site ->
                siteUrl(site)?.let { url -> addJsonObject { entry("$SITE$index", "Website", secret = false, url) } }
            }
            detail.customFields.forEach { custom ->
                // A hidden field's value stays in the vault until asked for, as in the editor.
                val secret = custom.kind == "hidden" || custom.value == null
                if (secret) {
                    if (custom.hasValue) addJsonObject { entry("$CUSTOM${custom.index}", custom.name, secret = true) }
                } else if (!custom.value.isNullOrEmpty()) {
                    addJsonObject { entry("$CUSTOM${custom.index}", custom.name, secret = false, custom.value) }
                }
            }
            if (hasCode) addJsonObject { entry(CODE, "One-time code", secret = true) }
            if (detail.hasNotes) addJsonObject { entry(NOTES, "Notes", secret = true) }
        }
        if (detail.fields.any { it.key == PASSWORD && it.secret }) put("changes", PASSWORD)
    }.toString()

    private fun JsonObjectBuilder.entry(key: String, label: String, secret: Boolean, value: String? = null) {
        put("key", key)
        put("label", label)
        put("secret", secret)
        if (!secret && value != null) put("value", value)
    }

    /** A plain field as text, or null when it holds nothing worth typing. A switch is not typed. */
    fun plain(field: FieldView): String? = when (field.input) {
        "toggle" -> null
        "date" -> field.number?.let { Instant.ofEpochMilli(it).atZone(ZoneOffset.UTC).toLocalDate().toString() }
        "number" -> field.number?.toString() ?: field.text.takeIf { it.isNotEmpty() }
        else -> field.text.takeIf { it.isNotEmpty() }
    }

    /** A website an item is for, as its address; null for an app, a pattern or anything else. */
    fun siteUrl(site: JsonElement): String? {
        val (kind, value) = (site as? JsonObject)?.entries?.firstOrNull() ?: return null
        return when (kind) {
            "Url", "Domain", "Host" -> runCatching { value.jsonPrimitive.contentOrNull }.getOrNull()?.takeIf { it.isNotEmpty() }
            else -> null
        }
    }

    /** A name for a login made from [website], such as "Github" for github.com; empty when it has none. */
    fun titleFromSite(website: String): String {
        val host = runCatching { java.net.URI(if ("://" in website) website else "https://$website").host }.getOrNull() ?: return ""
        val parts = host.removePrefix("www.").split('.')
        val name = if (parts.size >= 2) parts[parts.size - 2] else parts.firstOrNull().orEmpty()
        return name.replaceFirstChar { it.uppercase() }
    }
}

package com.wasimaster.wmpass.system

import android.content.ContentProvider
import android.content.ContentValues
import android.database.Cursor
import android.net.Uri
import android.os.Binder
import android.os.Bundle
import android.os.Process
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.Summary
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import org.json.JSONArray
import org.json.JSONObject

/**
 * How WM Keyboard reaches an open vault. It can only ever ask, never unlock, and it asks only after
 * the person has turned this on here. A password that goes this way passes through the keyboard's
 * process, so every call first proves who is calling by its signing certificate. A package name
 * alone proves nothing, and a manifest permission would lock out Android 11 and below, where
 * `knownSigner` does not exist.
 *
 * The one write is an API key: a key field in the keyboard's settings can keep what it holds as an
 * API key item here, when the person presses Save in WM Pass beside it.
 */
class KeyboardProvider : ContentProvider() {
    override fun onCreate() = true

    override fun call(method: String, arg: String?, extras: Bundle?): Bundle {
        val context = context ?: return Bundle()
        if (!callerIsKeyboard()) throw SecurityException("Only WM Keyboard may ask.")
        val prefs = Prefs(context)
        prefs.switchOnFirstTime(prefs.keyboardAccess)
        if (!prefs.keyboardAccess.value) return status("off")
        if (Pass.state.value == LockState.NoVault) return status("none")
        if (Pass.state.value !is LockState.Unlocked) return status("locked")
        return runBlocking {
            when (method) {
                "status" -> status("open")
                "logins" -> status("open").apply { putString("rows", logins(arg.orEmpty())) }
                "value" -> status("open").apply { value(arg.orEmpty(), extras?.getString("field").orEmpty())?.let { putString("value", it) } }
                "code" -> status("open").apply { code(arg.orEmpty(), this) }
                "api_keys" -> status("open").apply { putString("rows", apiKeys(arg.orEmpty())) }
                "save_api_key" -> status("open").apply { saveApiKey(extras, this) }
                else -> throw IllegalArgumentException("Unknown call.")
            }
        }
    }

    private fun status(state: String) = Bundle().apply { putString("state", state) }

    private fun callerIsKeyboard(): Boolean {
        val context = context ?: return false
        val uid = Binder.getCallingUid()
        if (uid == Process.myUid()) return false
        val packages = context.packageManager.getPackagesForUid(uid) ?: return false
        val own = Trust.certOf(context, context.packageName)
        return packages.any { name ->
            if (name !in KEYBOARD_PACKAGES) return@any false
            val cert = Trust.certOf(context, name) ?: return@any false
            cert in KEYBOARD_CERTS || cert == own
        }
    }

    /** What matches the app in front, or every login when nothing does, as in a browser. */
    private suspend fun logins(focused: String): String {
        val context = context ?: return "[]"
        val cert = focused.takeIf { it.isNotEmpty() }?.let { Trust.certOf(context, it) }
        val matched = if (cert != null) runCatching { Pass.matchApp(focused, cert).map { it.entry } }.getOrDefault(emptyList()) else emptyList()
        val rows = JSONArray()
        val logins = Pass.entries.value.filter { it.kind == "login" }.sortedBy { it.title.lowercase() }
        (matched + logins.filter { login -> matched.none { it.id == login.id } }).take(MOST_ROWS).forEach { rows.put(row(it, it in matched)) }
        return rows.toString()
    }

    private fun row(entry: Summary, matches: Boolean) = JSONObject()
        .put("id", entry.id)
        .put("title", entry.title)
        .put("username", entry.subtitle)
        .put("has_password", entry.hasPassword)
        .put("has_code", entry.hasOtp)
        .put("matches", matches)

    private suspend fun value(id: String, field: String): String? = when (field) {
        "username" -> Pass.entries.value.firstOrNull { it.id == id && it.kind == "login" }?.subtitle
        "password" -> Pass.reveal(id, "password").consume { String(it) }
        "code" -> Pass.code(id).consume { String(it) }
        API_KEY, API_SECRET -> if (Pass.entries.value.any { it.id == id && it.kind == "api" }) {
            runCatching { Pass.reveal(id, field).consume { String(it) } }.getOrNull()?.takeIf { it.isNotEmpty() }
        } else {
            null
        }
        else -> null
    }

    /**
     * The vault's API key items for a key field in the keyboard's settings. The ones whose title or
     * provider names [service] come first and say so; the rest follow by title.
     */
    private fun apiKeys(service: String): String {
        val wanted = service.trim().lowercase()
        fun names(entry: Summary): Boolean {
            if (wanted.isEmpty()) return false
            val provider = entry.subtitle.trim().lowercase()
            return wanted in entry.title.lowercase() || (provider.isNotEmpty() && (wanted in provider || provider in wanted))
        }
        val keys = Pass.entries.value.filter { it.kind == "api" && !it.trashed && !it.hidden }.sortedBy { it.title.lowercase() }
        val (named, rest) = keys.partition(::names)
        val rows = JSONArray()
        (named + rest).take(MOST_ROWS).forEachIndexed { index, entry ->
            rows.put(
                JSONObject()
                    .put("id", entry.id)
                    .put("title", entry.title)
                    .put("provider", entry.subtitle)
                    .put("matches", index < named.size),
            )
        }
        return rows.toString()
    }

    /**
     * Keeps a key from the keyboard's settings as one field of an API key item. An item already
     * called the same takes the new value, so saving again, or saving a service's key and then its
     * secret, leaves one item rather than two. The value it held before stays in the item's history.
     */
    private suspend fun saveApiKey(extras: Bundle?, out: Bundle) {
        val title = extras?.getString("title")?.trim().orEmpty()
        val field = extras?.getString("field")?.takeIf { it == API_KEY || it == API_SECRET }
        val value = extras?.getString("value")?.trim().orEmpty()
        if (title.isEmpty() || value.isEmpty() || field == null) return
        val existing = Pass.entries.value.firstOrNull { it.kind == "api" && !it.trashed && it.title == title }
        val id = existing?.id ?: Pass.change("entry.create") {
            put("kind", "api")
            put("title", title)
            extras?.getString("provider")?.trim()?.takeIf { it.isNotEmpty() }?.let { provider ->
                put("fields", buildJsonObject { put("provider", provider) })
            }
        }.obj["id"]!!.jsonPrimitive.content
        val bytes = value.toByteArray()
        try {
            Pass.change("entry.set_secret", secret = bytes) {
                put("id", id)
                put("field", field)
            }
        } finally {
            bytes.fill(0)
        }
        Pass.flush()
        out.putString("saved", if (existing == null) "created" else "updated")
    }

    /** The code for the app in front, for the chip on the keyboard's strip. Never for a guess. */
    private suspend fun code(focused: String, out: Bundle) {
        val context = context ?: return
        val cert = Trust.certOf(context, focused) ?: return
        val entry = Pass.matchApp(focused, cert).map { it.entry }.firstOrNull { it.hasOtp } ?: return
        val row = Pass.codes(listOf(entry.id)).firstOrNull() ?: return
        out.putString("code", Pass.code(entry.id).consume { String(it) })
        out.putInt("seconds_left", row.secondsLeft ?: 30)
    }

    override fun query(uri: Uri, projection: Array<out String>?, selection: String?, selectionArgs: Array<out String>?, sortOrder: String?): Cursor? = null
    override fun getType(uri: Uri): String? = null
    override fun insert(uri: Uri, values: ContentValues?): Uri? = null
    override fun delete(uri: Uri, selection: String?, selectionArgs: Array<out String>?) = 0
    override fun update(uri: Uri, values: ContentValues?, selection: String?, selectionArgs: Array<out String>?) = 0

    internal companion object {
        private const val MOST_ROWS = 200
        private const val API_KEY = "key"
        private const val API_SECRET = "secret"
        val KEYBOARD_PACKAGES = setOf("com.wasimaster.wmkeyboard", "com.wasimaster.wmkeyboard.debug", "com.wasimaster.wmkeyboard.fast")

        /** WM Keyboard's signers, from its published assetlinks.json. */
        val KEYBOARD_CERTS = setOf(
            "d6c1f30a6116d33e4e778b372f335faa91dad35f657a84e4e7077b29641e7cad",
            "a9b43a363119f1c6b747be2f7b942f71b4dab8d76bd1b8e488d94774f6aa0981",
            "dc4493107b16cedd0c0ab51275d7f5c3217caf9d8cd246a4a8a9f59eab1acf3e",
            "f9ee53b27ae388701b68e3268eb5b8eb26964d83c00755d8cb73a8eca222d69a",
            "3a97c7f20270addb101763ffd7e1c6e3eeab9f4cb24c7f7fcddca807f0f63f09",
            "1ac715ba5bfe36bcac1ee31a514c492211c750888ff5207118203626622aa904",
        )

        /** Whether the real WM Keyboard is on this phone: one of its packages, signed by one of its keys. */
        fun installed(context: android.content.Context): Boolean = installedPackage(context) != null

        /** The real WM Keyboard's package on this phone, or null. */
        fun installedPackage(context: android.content.Context): String? = KEYBOARD_PACKAGES.firstOrNull { isKeyboard(context, it) }

        /**
         * Whether [packageName], say an app that started one of WM Pass's screens for a result, is the
         * real WM Keyboard: one of its packages, signed by one of its keys.
         */
        fun isKeyboard(context: android.content.Context, packageName: String?): Boolean {
            if (packageName == null || packageName !in KEYBOARD_PACKAGES) return false
            val cert = Trust.certOf(context, packageName) ?: return false
            return cert in KEYBOARD_CERTS || cert == Trust.certOf(context, context.packageName)
        }
    }
}

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
import com.wasimaster.wmpass.core.SecretBytes
import com.wasimaster.wmpass.core.SecureClipboard
import com.wasimaster.wmpass.core.Summary
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.runBlocking
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonArray
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import org.json.JSONArray
import org.json.JSONObject

/**
 * How WM Keyboard reaches the vault. It asks only after the person has turned this on here, and
 * it opens a locked vault only with the person: their fingerprint, asked for by this app over the
 * app they are in, or their master password, typed into the keyboard's own box ([KeyboardUnlock]). A password that goes this way passes through the keyboard's
 * process, so every call first proves who is calling by its signing certificate. A package name
 * alone proves nothing, and a manifest permission would lock out Android 11 and below, where
 * `knownSigner` does not exist.
 *
 * The keyboard's WM Pass tool browses the vault, types or copies one field at a time, adds a login
 * and changes a password; [VaultWire] sets out each call. A key field in the keyboard's settings
 * can keep what it holds as an API key item here, when the person presses Save in WM Pass beside it.
 *
 * One call needs no vault access at all: [FieldWire.ACTION_CALL] presses a button on the screen
 * the person is typing on here, which the keyboard offers as a chip on its strip.
 */
class KeyboardProvider : ContentProvider() {
    override fun onCreate() = true

    override fun call(method: String, arg: String?, extras: Bundle?): Bundle {
        val context = context ?: return Bundle()
        if (!callerIsKeyboard()) throw SecurityException("Only WM Keyboard may ask.")
        // A button on one of this app's own screens: nothing leaves the vault, and a screen with
        // fields to tag shows only while the vault is open.
        if (method == FieldWire.ACTION_CALL) {
            val done = Pass.state.value is LockState.Unlocked && FieldActions.run(extras?.getString(FieldWire.TOKEN).orEmpty(), arg.orEmpty())
            return Bundle().apply { putBoolean(FieldWire.DONE, done) }
        }
        val prefs = Prefs(context)
        prefs.switchOnFirstTime(prefs.keyboardAccess)
        if (!prefs.keyboardAccess.value) return status("off")
        // Opening the vault from the keyboard, and locking it: the calls that work while it is locked.
        runBlocking { KeyboardUnlock.call(context, method, extras) }?.let { return it }
        if (Pass.state.value == LockState.NoVault) return status("none")
        if (Pass.state.value !is LockState.Unlocked) return status("locked").also { KeyboardUnlock.describeLocked(context, it) }
        return runBlocking {
            when (method) {
                VaultWire.STATUS -> status("open").also(KeyboardUnlock::describeOpen)
                "logins" -> status("open").apply { putString("rows", logins(arg.orEmpty())) }
                VaultWire.ITEMS -> status("open").also(KeyboardUnlock::describeOpen).apply { putString(VaultWire.ROWS, items(arg.orEmpty(), extras?.getString(VaultWire.QUERY).orEmpty())) }
                VaultWire.ITEM -> status("open").apply { page(arg.orEmpty())?.let { putString(VaultWire.PAGE, it) } }
                VaultWire.VALUE -> status("open").apply {
                    value(arg.orEmpty(), extras?.getString(VaultWire.FIELD).orEmpty())?.let { putString(VaultWire.VALUE, it.text()) }
                }
                VaultWire.COPY -> status("open").apply { putBoolean(VaultWire.COPIED, copy(arg.orEmpty(), extras?.getString(VaultWire.FIELD).orEmpty())) }
                VaultWire.ADD_LOGIN -> status("open").apply { addLogin(extras)?.let { putString(VaultWire.ID, it) } }
                VaultWire.SET_PASSWORD -> status("open").apply { putBoolean(VaultWire.SAVED, setPassword(arg.orEmpty(), extras?.getString(VaultWire.PASSWORD).orEmpty())) }
                VaultWire.GENERATE -> status("open").apply { putString(VaultWire.VALUE, generate(prefs)) }
                "code" -> status("open").apply { code(arg.orEmpty(), this) }
                "api_keys" -> status("open").apply { putString("rows", apiKeys(arg.orEmpty())) }
                "save_api_key" -> status("open").apply { saveApiKey(extras, this) }
                else -> throw IllegalArgumentException("Unknown call.")
            }
        }
    }

    private fun status(state: String) = Bundle().apply { putString(VaultWire.STATE, state) }

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

    /** An item the keyboard may see: not in the trash, and not on the hidden shelf. */
    private fun visible(id: String): Summary? = Pass.entries.value.firstOrNull { it.id == id && !it.trashed && !it.hidden }

    /** The ids of what is saved for the app in front, best first; none for a guess. */
    private suspend fun matched(focused: String): List<String> {
        val context = context ?: return emptyList()
        val cert = focused.takeIf { it.isNotEmpty() }?.let { Trust.certOf(context, it) } ?: return emptyList()
        return runCatching { Pass.matchApp(focused, cert).map { it.entry.id } }.getOrDefault(emptyList())
    }

    /** What matches the app in front, or every login when nothing does, as in a browser. Kept for keyboards from before [VaultWire.ITEMS]. */
    private suspend fun logins(focused: String): String {
        val matched = matched(focused)
        val rows = JSONArray()
        val logins = Pass.entries.value.filter { it.kind == "login" && !it.trashed && !it.hidden }.sortedBy { it.title.lowercase() }
        val (first, rest) = logins.partition { it.id in matched }
        (first.sortedBy { matched.indexOf(it.id) } + rest).take(MOST_ROWS).forEach { rows.put(row(it, it.id in matched)) }
        return rows.toString()
    }

    private fun row(entry: Summary, matches: Boolean) = JSONObject()
        .put("id", entry.id)
        .put("title", entry.title)
        .put("username", entry.subtitle)
        .put("has_password", entry.hasPassword)
        .put("has_code", entry.hasOtp)
        .put("matches", matches)

    /** Every kind of item, what matches the app in front first, narrowed by [query] when there is one. */
    private suspend fun items(focused: String, query: String): String {
        val matched = matched(focused)
        val shown = if (query.isBlank()) {
            Pass.entries.value.sortedBy { it.title.lowercase() }
        } else {
            runCatching { Pass.search(query.trim()) }.getOrDefault(emptyList())
        }.filter { !it.trashed && !it.hidden }
        val (first, rest) = shown.partition { it.id in matched }
        return VaultWire.rows((first.sortedBy { matched.indexOf(it.id) } + rest).take(MOST_ROWS), matched.toSet())
    }

    private suspend fun page(id: String): String? {
        val summary = visible(id) ?: return null
        return VaultWire.page(Pass.detail(id), summary.hasOtp)
    }

    /** One value of one item, as the keyboard names it in [VaultWire.FIELD]. */
    private sealed interface Found {
        fun text(): String

        class Plain(val label: String, val value: String) : Found {
            override fun text() = value
        }

        class Secret(val label: String, val bytes: SecretBytes) : Found {
            override fun text() = bytes.consumeText()
        }
    }

    private suspend fun value(id: String, field: String): Found? {
        val summary = visible(id) ?: return null
        return runCatching {
            when {
                field == VaultWire.CODE -> if (summary.hasOtp) Found.Secret("Code", Pass.code(id)) else null
                field == VaultWire.NOTES -> if (Pass.detail(id).hasNotes) Found.Secret("Notes", Pass.reveal(id, VaultWire.NOTES)) else null
                field.startsWith(VaultWire.SITE) -> {
                    val index = field.removePrefix(VaultWire.SITE).toIntOrNull() ?: return null
                    Pass.detail(id).sites.getOrNull(index)?.let(VaultWire::siteUrl)?.let { Found.Plain("Website", it) }
                }
                field.startsWith(VaultWire.CUSTOM) -> {
                    val index = field.removePrefix(VaultWire.CUSTOM).toIntOrNull() ?: return null
                    val custom = Pass.detail(id).customFields.firstOrNull { it.index == index } ?: return null
                    val plain = custom.value
                    if (custom.kind != "hidden" && plain != null) Found.Plain(custom.name, plain) else Found.Secret(custom.name, Pass.revealCustom(id, index))
                }
                else -> {
                    val spec = Pass.detail(id).fields.firstOrNull { it.key == field } ?: return null
                    if (spec.secret) {
                        if (spec.hasValue) Found.Secret(spec.label, Pass.reveal(id, field)) else null
                    } else {
                        VaultWire.plain(spec)?.let { Found.Plain(spec.label, it) }
                    }
                }
            }
        }.getOrNull()
    }

    /** Copies one value here, the way a copy in WM Pass is made: a secret marked sensitive and wiped after a while. */
    private suspend fun copy(id: String, field: String): Boolean {
        val context = context ?: return false
        val found = value(id, field) ?: return false
        withContext(Dispatchers.Main) {
            when (found) {
                is Found.Plain -> SecureClipboard.copyPlain(context, found.label, found.value)
                is Found.Secret -> SecureClipboard.copy(context, found.label, found.bytes)
            }
        }
        return true
    }

    /**
     * A new login from the keyboard's form. The app in front is linked by its certificate, so the
     * login comes first there next time, unless it is a browser, which shows every site, or one of
     * the two apps themselves.
     */
    private suspend fun addLogin(extras: Bundle?): String? {
        val context = context ?: return null
        val title = extras?.getString(VaultWire.TITLE)?.trim().orEmpty()
        val username = extras?.getString(VaultWire.USERNAME)?.trim().orEmpty()
        val password = extras?.getString(VaultWire.PASSWORD).orEmpty()
        val website = extras?.getString(VaultWire.WEBSITE)?.trim().orEmpty()
        val app = extras?.getString(VaultWire.APP)?.trim().orEmpty()
        if (title.isEmpty() && username.isEmpty() && password.isEmpty() && website.isEmpty()) return null
        val linked = app.takeIf {
            it.isNotEmpty() && it != context.packageName && it !in KEYBOARD_PACKAGES && !Trust.isVerifiedBrowser(context, it)
        }
        val cert = linked?.let { Trust.certOf(context, it) }
        val appName = if (linked != null && cert != null) Trust.labelOf(context, linked) else ""
        val name = title.ifEmpty { VaultWire.titleFromSite(website) }.ifEmpty { appName }.ifEmpty { "Untitled" }
        val id = Pass.change("entry.create") {
            put("kind", "login")
            put("title", name)
            put("fields", buildJsonObject { if (username.isNotEmpty()) put("username", username) })
            put(
                "sites",
                buildJsonArray {
                    if (website.isNotEmpty()) addJsonObject { put("Url", if ("://" in website) website else "https://$website") }
                    if (linked != null && cert != null) {
                        addJsonObject {
                            put(
                                "AndroidApp",
                                buildJsonObject {
                                    put("package", linked)
                                    put("cert_sha256", cert)
                                },
                            )
                        }
                    }
                },
            )
        }.obj["id"]!!.jsonPrimitive.content
        if (password.isNotEmpty()) {
            val bytes = password.toByteArray()
            try {
                Pass.change("entry.set_secret", secret = bytes) {
                    put("id", id)
                    put("field", VaultWire.PASSWORD)
                }
            } finally {
                bytes.fill(0)
            }
        }
        Pass.flush()
        return id
    }

    /** Puts [password] on an item that holds one. The one it had stays in the item's history. */
    private suspend fun setPassword(id: String, password: String): Boolean {
        if (password.isEmpty() || visible(id) == null) return false
        if (Pass.detail(id).fields.none { it.key == VaultWire.PASSWORD && it.secret }) return false
        val bytes = password.toByteArray()
        try {
            Pass.change("entry.set_secret", secret = bytes) {
                put("id", id)
                put("field", VaultWire.PASSWORD)
            }
        } finally {
            bytes.fill(0)
        }
        Pass.flush()
        return true
    }

    /** A password from the generator, made the way the person set it up here. */
    private suspend fun generate(prefs: Prefs): String {
        val made = com.wasimaster.wmpass.ui.vault.GenState.fromPrefs(prefs)
        made.roll()
        return made.value
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

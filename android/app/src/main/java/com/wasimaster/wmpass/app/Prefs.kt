package com.wasimaster.wmpass.app

import android.content.Context
import android.content.SharedPreferences
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.Accent
import com.wasimaster.wmpass.ui.theme.BuiltInThemes
import com.wasimaster.wmpass.ui.theme.Look
import com.wasimaster.wmpass.ui.theme.ThemeCodec
import com.wasimaster.wmpass.ui.theme.ThemeFile
import org.json.JSONObject

/**
 * Preferences that are not secret. They live outside the vault so the door can be drawn in
 * the right colours before it opens. Nothing here names a site or an account.
 *
 * Each one is an [Opt], so all of them can be written to a file and read back, or reset, without
 * a list to keep up to date. The few that weaken protection are not [Opt.portable]: a settings
 * file passed around by someone else must not be able to turn off screenshot blocking.
 */
class Prefs(context: Context) {
    private val store: SharedPreferences = context.getSharedPreferences("prefs", Context.MODE_PRIVATE)
    private val all = ArrayList<Opt<*>>()

    abstract inner class Opt<T>(val key: String, val default: T, val portable: Boolean) {
        var value: T by mutableStateOf(default)
            private set

        protected abstract fun read(): T
        protected abstract fun write(editor: SharedPreferences.Editor, next: T)
        abstract fun parse(raw: Any?): T?

        fun load() {
            value = read()
        }

        fun set(next: T) {
            put(next)
            onSet?.invoke(this)
        }

        /** Writes a value without telling anyone, for one that came from the vault. */
        internal fun put(next: T) {
            value = next
            store.edit().also { write(it, next) }.apply()
        }

        fun reset() = set(default)

        init {
            all += this
        }
    }

    private inner class Flag(key: String, default: Boolean, portable: Boolean) : Opt<Boolean>(key, default, portable) {
        override fun read() = store.getBoolean(key, default)
        override fun write(editor: SharedPreferences.Editor, next: Boolean) { editor.putBoolean(key, next) }
        override fun parse(raw: Any?) = raw as? Boolean
    }

    private inner class Whole(key: String, default: Int, portable: Boolean) : Opt<Int>(key, default, portable) {
        override fun read() = store.getInt(key, default)
        override fun write(editor: SharedPreferences.Editor, next: Int) { editor.putInt(key, next) }
        override fun parse(raw: Any?) = (raw as? Number)?.toInt()
    }

    /** A text value, limited to [allowed] when the app only understands certain words. */
    private inner class Word(key: String, default: String, portable: Boolean, val allowed: Set<String>?) : Opt<String>(key, default, portable) {
        override fun read() = store.getString(key, default) ?: default
        override fun write(editor: SharedPreferences.Editor, next: String) { editor.putString(key, next) }
        override fun parse(raw: Any?) = (raw as? String)?.takeIf { allowed == null || it in allowed }
    }

    private fun flag(key: String, default: Boolean, portable: Boolean = true): Opt<Boolean> = Flag(key, default, portable).also { it.load() }
    private fun whole(key: String, default: Int, portable: Boolean = true): Opt<Int> = Whole(key, default, portable).also { it.load() }
    private fun word(key: String, default: String, vararg allowed: String): Opt<String> =
        Word(key, default, true, allowed.toSet().ifEmpty { null }).also { it.load() }
    private fun local(key: String, default: String): Opt<String> = Word(key, default, false, null).also { it.load() }

    // ---- style ----

    val lookOpt = word("look", Look.SYSTEM.name, *Look.entries.map { it.name }.toTypedArray())
    val look: Look get() = runCatching { Look.valueOf(lookOpt.value) }.getOrDefault(Look.SYSTEM)
    val accentOpt = word("accent", Accent.EMERALD.name, *Accent.entries.map { it.name }.toTypedArray())
    val accent: Accent get() = runCatching { Accent.valueOf(accentOpt.value) }.getOrDefault(Accent.EMERALD)

    /** `standard`, or `glass` for the expressive look. */
    val uiStyleOpt = word("ui_style", "standard", "standard", "glass")
    val uiStyle: String get() = uiStyleOpt.value
    val themeIdOpt = word("theme_id", defaultTheme())
    val themeId: String get() = themeIdOpt.value
    private val customThemesOpt = word("custom_themes", "")
    val customThemes: List<ThemeFile> get() = ThemeCodec.readList(customThemesOpt.value.ifEmpty { null })
    val richMotion = flag("rich_motion", true)
    val haptics = flag("haptics", true)
    val colourSecrets = flag("colour_secrets", true)

    // ---- icons ----

    /** The app's own glyphs: `outlined`, `rounded`, `sharp` or `light`. */
    val iconStyle = word("icon_style", "rounded", "outlined", "rounded", "sharp", "light")

    /** Entries: `brand`, `tinted`, `uniform` or `letters`. */
    val siteIconStyle = word("site_icon_style", if (store.getBoolean("coloured_icons", true)) "brand" else "uniform", "brand", "tinted", "uniform", "letters")
    val siteIconShape = word("site_icon_shape", "circle", "circle", "rounded", "square", "none")

    /** Settings rows: `coloured`, `mono`, `bare` or `off`. */
    val settingsIcons = word("settings_icons", "coloured", "coloured", "mono", "bare", "off")
    val settingsCards = flag("settings_cards", true)

    // ---- vault list ----

    val layoutOpt = word("layout", "list", "list", "compact", "grid")
    val layout: String get() = layoutOpt.value
    val groupingOpt = word("grouping", "smart", "smart", "folder")
    val grouping: String get() = groupingOpt.value
    val sort = word("sort", "name", "name", "changed", "kind", "site")
    val groupSites = flag("group_sites", true)
    val pinnedOnTop = flag("pinned_on_top", true)
    val listHeadings = flag("list_headings", true)
    val rowIcons = flag("row_icons", true)
    val rowSubtitle = flag("row_subtitle", true)
    val rowBadges = flag("row_badges", true)
    val rowButton = word("row_button", "password", "password", "username", "code", "none")
    val filterChips = flag("filter_chips", true)
    val searchAtBottom = flag("search_at_bottom", false)
    val tapAction = word("tap_action", "open", "open", "password", "username", "code")
    val swipeRight = word("swipe_right", "password", "password", "username", "code", "favourite", "none")
    val swipeLeft = word("swipe_left", "trash", "trash", "favourite", "none", "password")
    val leftHandedOpt = flag("left_handed", false)
    val leftHanded: Boolean get() = leftHandedOpt.value
    private val widgetsOpt = word("widgets", "health")
    val widgets: List<String> get() = widgetsOpt.value.split(",").filter { it.isNotEmpty() }

    // ---- navigation ----

    private val tabsOpt = word("tabs", DEFAULT_TABS.joinToString(","))
    val tabs: List<String> get() = tabsOpt.value.split(",").filter { it.isNotEmpty() }
    val navLabels = word("nav_labels", "all", "all", "selected", "none")
    val startTab = word("start_tab", "first", "first", "last")
    val lastTab = word("last_tab", "")
    val showFab = flag("show_fab", true)
    val fabAction = word("fab_action", "menu")
    val showLock = flag("show_lock", true)

    // ---- adding and editing ----

    private val addKindsOpt = word("add_kinds", DEFAULT_ADD.joinToString(","))
    val addKinds: List<String> get() = addKindsOpt.value.split(",").filter { it.isNotEmpty() }
    private val editorOpenOpt = word("editor_open", "sites")
    val editorOpen: Set<String> get() = editorOpenOpt.value.split(",").filter { it.isNotEmpty() }.toSet()
    val editorRemember = flag("editor_remember", true)
    val editorFoldDetails = flag("editor_fold_details", true)
    val editorGenerate = flag("editor_generate", false)
    val autoTag = flag("auto_tag", true)
    /**
     * Which reader the card scanner uses, in builds with Google's SDKs: `google` (ML Kit) or
     * `own`. Empty until the first scan asks. Not portable: a settings file from someone else
     * must not pick the reader that talks to Google.
     */
    val cardReader: Opt<String> = Word("card_reader", "", false, setOf("", "google", "own")).also { it.load() }

    // ---- entry page ----

    val entryActions = flag("entry_actions", true)
    val entryActionUsername = flag("entry_action_username", true)
    val entryActionOpen = flag("entry_action_open", true)
    val tapCopies = flag("tap_copies", true)
    val revealSeconds = whole("reveal_seconds", 45)

    // ---- codes ----

    val codeGroups = flag("code_groups", true)
    val codeSize = word("code_size", "normal", "small", "normal", "large")
    val codeNext = flag("code_next", true)
    val codeRing = flag("code_ring", true)
    val codeHidden = flag("code_hidden", false)
    val codeSort = word("code_sort", "default", "default", "name")

    // ---- generator ----

    val genMode = word("gen_mode", "password", "password", "passphrase", "pronounceable", "pin", "pattern")
    val genLength = whole("gen_length", 20)
    val genWords = whole("gen_words", 5)
    val genSymbols = flag("gen_symbols", true)
    val genNoAmbiguous = flag("gen_no_ambiguous", false)
    val genShowBits = flag("gen_show_bits", true)

    // ---- health ----

    private val healthHiddenOpt = word("health_hidden", "")
    val healthHidden: Set<String> get() = healthHiddenOpt.value.split(",").filter { it.isNotEmpty() }.toSet()
    val healthScore = flag("health_score", true)

    // ---- home screen widgets: they describe this phone's home screen, so they stay on it ----

    /** `app` (the theme chosen here), `system` (the wallpaper's colours) or `dark`. */
    val widgetColours = word("widget_colours", "app", "app", "system", "dark")

    /** `solid`, `translucent` or `none`. */
    val widgetBackground = word("widget_background", "solid", "solid", "translucent", "none")
    private val widgetButtonsOpt = word("widget_buttons", DEFAULT_WIDGET_BUTTONS.joinToString(","))

    /** The quick bar's buttons, in order. */
    val widgetButtons: List<String> get() = widgetButtonsOpt.value.split(",").filter { it in WIDGET_BUTTONS }

    /**
     * What a widget's lock button does: `lock`, or `panic`, which also refuses every password for
     * an hour. Not portable: a settings file from someone else must not turn a lock into a lockout.
     */
    val widgetLock: Opt<String> = Word("widget_lock", "lock", false, setOf("lock", "panic")).also { it.load() }

    /** Lets widgets show codes and favorites while the vault is open. Off until turned on. */
    val widgetAccess = flag("widget_access", false, portable = false)
    private val widgetCodesOpt = local("widget_codes", "")

    /** The items whose codes widgets show, by id. Empty means the favorites that have a code. */
    val widgetCodes: List<String> get() = widgetCodesOpt.value.split(",").filter { it.isNotEmpty() }

    fun updateWidgetButtons(value: List<String>) = widgetButtonsOpt.set(value.joinToString(","))
    fun updateWidgetCodes(value: List<String>) = widgetCodesOpt.set(value.joinToString(","))

    // ---- settings pages ----

    private val foldsOpt = word("folds_open", "")
    val foldsOpen: Set<String> get() = foldsOpt.value.split(",").filter { it.isNotEmpty() }.toSet()

    // ---- protection: never taken from a settings file, never reset with the look ----

    /** Seconds in the background before locking. Zero is at once, -1 never. */
    private val lockAfterOpt = whole("lock_after", 60, portable = false)
    val lockAfter: Int get() = lockAfterOpt.value
    val lockScreenOff = flag("lock_screen_off", true, portable = false)
    private val clipboardOpt = whole("clipboard_seconds", 30, portable = false)
    val clipboardSeconds: Int get() = clipboardOpt.value
    private val screenOpt = flag("screen_protection", true, portable = false)
    val screenProtection: Boolean get() = screenOpt.value
    private val networkOpt = flag("network_allowed", false, portable = false)
    val networkAllowed: Boolean get() = networkOpt.value
    val autofillInline = flag("autofill_inline", true, portable = false)
    val autofillSave = flag("autofill_save", true, portable = false)
    val keyboardAccess = flag("keyboard_access", false, portable = false)
    val wearAccess = flag("wear_access", false, portable = false)
    val autofillNudge = flag("autofill_nudge", true)

    /**
     * Turns a companion's access on the first time the real one shows up: WM Keyboard installed,
     * or a watch with WM Pass on it asking. Once the person has set it either way, that stands.
     */
    fun switchOnFirstTime(opt: Opt<Boolean>) {
        if (!store.contains(opt.key)) opt.set(true) else opt.load()
    }
    private val panicActionOpt = local("panic_action", "lock")
    val panicAction: String get() = panicActionOpt.value
    private val panicAppOpt = local("panic_app", "")
    val panicTrustedApp: String? get() = panicAppOpt.value.ifEmpty { null }

    // ---- the guide: what it has shown on this phone stays on this phone ----

    /**
     * Whether the welcome has been shown, or put away, on this phone. An install from before the
     * guide, whose settings file already holds something, counts as having seen it: its owner
     * knows their way round, and the guide is in Settings when they want it.
     */
    val helpWelcomeSeen = flag("help_welcome_seen", false, portable = false).also { opt ->
        if (!store.contains(opt.key)) opt.set(store.all.isNotEmpty())
    }

    /** A line about Codes, Health or the shelf the first time each one opens. */
    val helpHints = flag("help_hints", true)

    /** `always`, or `off` for no "?" at all. A touch screen has no hover. */
    val helpButtons = word("help_buttons", "always", "always", "off")

    /** The hints already shown or followed, by name. */
    private val helpSeenOpt = local("help_seen", "")
    val helpSeen: Set<String> get() = helpSeenOpt.value.split(",").filter { it.isNotEmpty() }.toSet()

    /** The tours someone has gone through to the end. */
    private val helpToursDoneOpt = local("help_tours_done", "")
    val helpToursDone: Set<String> get() = helpToursDoneOpt.value.split(",").filter { it.isNotEmpty() }.toSet()

    fun markHelpSeen(key: String) {
        if (key !in helpSeen) helpSeenOpt.set((helpSeen + key).joinToString(","))
    }

    fun markTourDone(id: String) {
        if (id !in helpToursDone) helpToursDoneOpt.set((helpToursDone + id).joinToString(","))
    }

    // ---- settings sync: each phone decides for itself ----

    val syncSettings = flag("sync_settings", true, portable = false)
    private val syncSkipOpt = local("sync_skip", "")
    val syncSkip: Set<String> get() = syncSkipOpt.value.split(",").filter { it.isNotEmpty() }.toSet()

    fun setSyncSkipped(key: String, skipped: Boolean) {
        val next = if (skipped) syncSkip + key else syncSkip - key
        syncSkipOpt.set(next.joinToString(","))
    }

    /** Key chords for a physical keyboard, by what they do. */
    private val shortcutsOpt = word("shortcuts", "")
    val shortcuts: Map<String, String?> get() = DEFAULT_SHORTCUTS + shortcutsOpt.value.split('\n').filter { '=' in it }.associate {
        val (key, value) = it.split('=', limit = 2)
        key to value.takeIf { chord -> chord.isNotEmpty() }
    }

    private fun defaultTheme() =
        if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S) BuiltInThemes.SYSTEM else BuiltInThemes.DEFAULT

    init {
        PassIcons.style = iconStyle.value
    }

    fun updateIconStyle(value: String) {
        iconStyle.set(value)
        PassIcons.style = value
    }

    fun updateUiStyle(value: String) = uiStyleOpt.set(value)
    fun updateTheme(value: String) = themeIdOpt.set(value)

    fun saveCustomTheme(theme: ThemeFile) {
        customThemesOpt.set(ThemeCodec.writeList(customThemes.filter { it.id != theme.id } + theme))
    }

    fun deleteCustomTheme(id: String) {
        customThemesOpt.set(ThemeCodec.writeList(customThemes.filter { it.id != id }))
        if (themeId == id) updateTheme(defaultTheme())
    }

    fun updateShortcut(action: String, chord: String?) {
        // One chord does one thing, so taking it away from anything else that had it.
        val freed = shortcuts.mapValues { (_, value) -> if (value == chord) null else value }
        val next = freed + (action to chord)
        shortcutsOpt.set(next.entries.joinToString("\n") { "${it.key}=${it.value.orEmpty()}" })
    }

    fun resetShortcuts() = shortcutsOpt.reset()

    /** What a panic signal does: "lock", or "wipe" if it comes from the app named below. */
    fun updatePanic(action: String, app: String?) {
        panicActionOpt.set(action)
        panicAppOpt.set(app.orEmpty())
    }

    fun updateLook(value: Look) = lookOpt.set(value.name)
    fun updateAccent(value: Accent) = accentOpt.set(value.name)
    fun updateLockAfter(value: Int) = lockAfterOpt.set(value)
    fun updateClipboard(value: Int) {
        clipboardOpt.set(value)
        com.wasimaster.wmpass.core.SecureClipboard.clearAfterSeconds = value
    }
    fun updateScreenProtection(value: Boolean) = screenOpt.set(value)
    fun updateLayout(value: String) = layoutOpt.set(value)
    fun updateLeftHanded(value: Boolean) = leftHandedOpt.set(value)
    fun updateTabs(value: List<String>) = tabsOpt.set(value.joinToString(","))
    fun updateWidgets(value: List<String>) = widgetsOpt.set(value.joinToString(","))
    fun updateGrouping(value: String) = groupingOpt.set(value)
    fun updateRichMotion(value: Boolean) = richMotion.set(value)
    fun updateNetworkAllowed(value: Boolean) = networkOpt.set(value)
    fun updateAddKinds(value: List<String>) = addKindsOpt.set(value.joinToString(","))

    fun setEditorOpen(section: String, open: Boolean) {
        val next = if (open) editorOpen + section else editorOpen - section
        editorOpenOpt.set(next.joinToString(","))
    }

    fun setHealthShown(check: String, shown: Boolean) {
        val next = if (shown) healthHidden - check else healthHidden + check
        healthHiddenOpt.set(next.joinToString(","))
    }

    fun setFoldOpen(id: String, open: Boolean) {
        val next = if (open) foldsOpen + id else foldsOpen - id
        foldsOpt.set(next.joinToString(","))
    }

    /** Every setting's key, in the order they are declared, for a test that each one is explained. */
    val keys: List<String> get() = all.map { it.key }

    /** The setting stored under [key], for the guide's "Now" and "Default" marks and its **Use this**. */
    fun opt(key: String): Opt<*>? = all.firstOrNull { it.key == key }

    // ---- settings that follow the vault ----

    /** Told about every setting someone changes, for the vault to carry it to other devices. */
    var onSet: ((Opt<*>) -> Unit)? = null

    /** Every setting that can follow the vault, whether or not this phone lets it. */
    val syncable: List<Opt<*>> get() = all.filter { it.portable && it.key !in DEVICE_ONLY }

    /** Whether [opt] goes to and comes from the vault on this phone. */
    fun syncs(opt: Opt<*>): Boolean = syncSettings.value && opt.portable && opt.key !in DEVICE_ONLY && opt.key !in syncSkip

    /**
     * Takes settings another device left in the vault. Each one gets the checks a settings file
     * gets, and own themes keep their ids so the theme chosen with them is still found. Returns how
     * many changed. Nothing here is sent back.
     */
    fun applySynced(values: Map<String, Any?>): Int {
        var changed = 0
        for ((key, raw) in values) {
            val opt = opt(key) ?: continue
            if (!syncs(opt)) continue
            if (opt === themeIdOpt && raw == BuiltInThemes.SYSTEM && defaultTheme() != BuiltInThemes.SYSTEM) continue
            val incoming = if (opt === customThemesOpt) cleanThemes(raw) else raw
            @Suppress("UNCHECKED_CAST")
            val typed = opt as Opt<Any?>
            val parsed = typed.parse(incoming) ?: continue
            if (parsed != typed.value) {
                typed.put(parsed)
                changed++
            }
        }
        if (changed > 0) PassIcons.style = iconStyle.value
        return changed
    }

    private fun cleanThemes(raw: Any?): String? {
        val text = raw as? String ?: return null
        if (text.isEmpty()) return ""
        val clean = ThemeCodec.readList(text).mapNotNull { theme ->
            runCatching { ThemeCodec.read(ThemeCodec.toText(theme)) }.getOrNull()?.let { checked ->
                if (Regex("custom_[\\w-]{1,40}").matches(theme.id)) checked.copy(id = theme.id) else checked
            }
        }
        return ThemeCodec.writeList(clean)
    }

    /** Every customisation as a file. Protection settings are left out on purpose. */
    fun exportText(): String = JSONObject().apply {
        put("format", "wmpass-settings")
        put("version", 1)
        all.filter { it.portable }.forEach { put(it.key, it.value) }
    }.toString(2)

    /**
     * Applies a settings file, which may have come from anyone. Unknown keys and values the app
     * does not understand are skipped. Returns how many settings changed.
     */
    fun importText(text: String): Int {
        require(text.length <= IMPORT_LIMIT) { "That file is too long to be settings." }
        val json = runCatching { JSONObject(text) }.getOrElse { throw IllegalArgumentException("That is not a WM Pass settings file.") }
        require(json.optString("format") == "wmpass-settings") { "That is not a WM Pass settings file." }
        var changed = 0
        // Themes in the file get the same checks as a theme imported on its own, including a new
        // id, so a theme the file chooses has to be found under that new id.
        val renamed = HashMap<String, String>()
        if (json.has(customThemesOpt.key)) {
            val incoming = ThemeCodec.readList(json.optString(customThemesOpt.key)).mapNotNull { theme ->
                runCatching { ThemeCodec.read(ThemeCodec.toText(theme)) }.getOrNull()?.also { renamed[theme.id] = it.id }
            }
            json.put(customThemesOpt.key, ThemeCodec.writeList(customThemes.filter { mine -> incoming.none { it.name == mine.name } } + incoming))
        }
        renamed[json.optString(themeIdOpt.key)]?.let { json.put(themeIdOpt.key, it) }
        all.filter { it.portable }.forEach { opt ->
            if (json.has(opt.key)) {
                @Suppress("UNCHECKED_CAST")
                val typed = opt as Opt<Any?>
                val parsed = typed.parse(json.get(opt.key))
                if (parsed != null && parsed != typed.value) {
                    typed.set(parsed)
                    changed++
                }
            }
        }
        PassIcons.style = iconStyle.value
        return changed
    }

    /**
     * Puts back how the app is drawn: style, theme, motion, icons and the list layout, for a
     * launch that keeps crashing as soon as it draws. Dark mode, accent and the rest stay.
     */
    fun resetLook() {
        listOf(uiStyleOpt, themeIdOpt, richMotion, iconStyle, siteIconStyle, siteIconShape, settingsIcons, settingsCards, layoutOpt).forEach { it.reset() }
        PassIcons.style = iconStyle.value
    }

    /** Puts every customisation back. The theme stays on device colours where that is the default. */
    fun resetCustomisations() {
        all.filter { it.portable && it !== customThemesOpt }.forEach { it.reset() }
        PassIcons.style = iconStyle.value
    }

    companion object {
        val DEFAULT_TABS = listOf("vault", "codes", "generator", "health")
        val DEFAULT_ADD = listOf("login", "otp", "note", "card", "identity", "recovery", "wifi", "ssh", "api", "database", "licence", "document")
        private const val IMPORT_LIMIT = 200_000

        /**
         * Customisations that describe this phone's screens rather than the person, or mean
         * something different on a computer under the same name. They never travel with the vault.
         */
        private val DEVICE_ONLY = setOf("tabs", "last_tab", "folds_open", "editor_open", "widget_colours", "widget_background", "widget_buttons")

        /** What the quick bar widget can hold, in the order the settings page offers them. */
        val WIDGET_BUTTONS = listOf("search", "add", "codes", "generator", "health", "lock")
        val DEFAULT_WIDGET_BUTTONS = listOf("search", "add", "codes", "lock")
        val DEFAULT_SHORTCUTS: Map<String, String?> = mapOf(
            "search" to "Ctrl+F",
            "add" to "Ctrl+N",
            "lock" to "Ctrl+L",
            "generator" to "Ctrl+G",
            "codes" to "Ctrl+O",
            "health" to "Ctrl+H",
            "settings" to "Ctrl+Comma",
        )

        @Volatile
        private var instance: Prefs? = null

        fun of(context: Context): Prefs = instance ?: synchronized(this) {
            instance ?: Prefs(context.applicationContext).also { instance = it }
        }
    }
}

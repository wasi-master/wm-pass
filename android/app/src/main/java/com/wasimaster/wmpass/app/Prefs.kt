package com.wasimaster.wmpass.app

import android.content.Context
import android.content.SharedPreferences
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.wasimaster.wmpass.ui.theme.Accent
import com.wasimaster.wmpass.ui.theme.Look

/**
 * Preferences that are not secret. They live outside the vault so the door can be drawn in
 * the right colours before it opens. Nothing here names a site or an account.
 */
class Prefs(context: Context) {
    private val store: SharedPreferences = context.getSharedPreferences("prefs", Context.MODE_PRIVATE)

    private fun edit(block: SharedPreferences.Editor.() -> Unit) = store.edit().apply(block).apply()

    var look by mutableStateOf(runCatching { Look.valueOf(store.getString("look", null) ?: "") }.getOrDefault(Look.DARK))
        private set
    var accent by mutableStateOf(runCatching { Accent.valueOf(store.getString("accent", null) ?: "") }.getOrDefault(Accent.EMERALD))
        private set

    /** Seconds in the background before locking. Zero is at once, -1 never. */
    var lockAfter by mutableIntStateOf(store.getInt("lock_after", 60))
        private set
    var clipboardSeconds by mutableIntStateOf(store.getInt("clipboard_seconds", 30))
        private set
    var screenProtection by mutableStateOf(store.getBoolean("screen_protection", true))
        private set
    var layout by mutableStateOf(store.getString("layout", "list") ?: "list")
        private set
    var leftHanded by mutableStateOf(store.getBoolean("left_handed", false))
        private set
    var tabs by mutableStateOf(store.getString("tabs", null)?.split(",") ?: DEFAULT_TABS)
        private set
    var widgets by mutableStateOf(store.getString("widgets", null)?.split(",")?.filter { it.isNotEmpty() } ?: DEFAULT_WIDGETS)
        private set
    var grouping by mutableStateOf(store.getString("grouping", "smart") ?: "smart")
        private set
    var richMotion by mutableStateOf(store.getBoolean("rich_motion", true))
        private set
    var networkAllowed by mutableStateOf(store.getBoolean("network_allowed", false))
        private set

    /** Key chords for a physical keyboard, by what they do. */
    var shortcuts by mutableStateOf(DEFAULT_SHORTCUTS + readShortcuts())
        private set

    private fun readShortcuts(): Map<String, String?> =
        store.getString("shortcuts", null).orEmpty().split('\n').filter { '=' in it }.associate {
            val (key, value) = it.split('=', limit = 2)
            key to value.takeIf { chord -> chord.isNotEmpty() }
        }

    fun updateShortcut(action: String, chord: String?) {
        // One chord does one thing, so taking it away from anything else that had it.
        val freed = shortcuts.mapValues { (_, value) -> if (value == chord) null else value }
        shortcuts = freed + (action to chord)
        edit { putString("shortcuts", shortcuts.entries.joinToString("\n") { "${it.key}=${it.value.orEmpty()}" }) }
    }

    fun resetShortcuts() {
        shortcuts = DEFAULT_SHORTCUTS
        edit { remove("shortcuts") }
    }

    /** What a panic signal does: "lock", or "wipe" if it comes from the app named below. */
    var panicAction by mutableStateOf(store.getString("panic_action", "lock") ?: "lock")
        private set
    var panicTrustedApp by mutableStateOf(store.getString("panic_app", null))
        private set

    fun updatePanic(action: String, app: String?) {
        panicAction = action
        panicTrustedApp = app
        edit {
            putString("panic_action", action)
            putString("panic_app", app)
        }
    }

    fun updateLook(value: Look) { look = value; edit { putString("look", value.name) } }
    fun updateAccent(value: Accent) { accent = value; edit { putString("accent", value.name) } }
    fun updateLockAfter(value: Int) { lockAfter = value; edit { putInt("lock_after", value) } }
    fun updateClipboard(value: Int) { clipboardSeconds = value; edit { putInt("clipboard_seconds", value) } }
    fun updateScreenProtection(value: Boolean) { screenProtection = value; edit { putBoolean("screen_protection", value) } }
    fun updateLayout(value: String) { layout = value; edit { putString("layout", value) } }
    fun updateLeftHanded(value: Boolean) { leftHanded = value; edit { putBoolean("left_handed", value) } }
    fun updateTabs(value: List<String>) { tabs = value; edit { putString("tabs", value.joinToString(",")) } }
    fun updateWidgets(value: List<String>) { widgets = value; edit { putString("widgets", value.joinToString(",")) } }
    fun updateGrouping(value: String) { grouping = value; edit { putString("grouping", value) } }
    fun updateRichMotion(value: Boolean) { richMotion = value; edit { putBoolean("rich_motion", value) } }
    fun updateNetworkAllowed(value: Boolean) { networkAllowed = value; edit { putBoolean("network_allowed", value) } }

    companion object {
        val DEFAULT_TABS = listOf("vault", "codes", "generator", "health")
        val DEFAULT_WIDGETS = listOf("health", "codes", "favourites")
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

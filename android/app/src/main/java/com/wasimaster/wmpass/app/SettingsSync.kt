package com.wasimaster.wmpass.app

import com.wasimaster.wmpass.core.Pass
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.intOrNull

/**
 * Settings that follow the vault. They travel inside it, so every way the vault syncs (the local
 * network, a folder or server, a WM Pass server) carries them, and a copy of the file says nothing
 * about them. Each value is written one key at a time and the later write wins, the same as an
 * item's fields. Only the vault in this phone's folder carries them.
 */
object SettingsSync {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    // Both only touched on the main thread.
    private var running = false
    private var again = false

    /** Hooks into the vault's reloads and the settings' writes. Call once, after [Pass.init]. */
    fun start(prefs: Prefs) {
        prefs.onSet = { opt ->
            if (opt === prefs.syncSettings) {
                if (prefs.syncSettings.value) pull(prefs)
            } else if (prefs.syncs(opt)) {
                push(prefs, JsonObject(mapOf(opt.key to toJson(opt.value))))
            }
        }
        Pass.afterRefresh = { pull(prefs) }
    }

    /**
     * Takes what other devices left, then leaves anything this phone has set that the vault has
     * never held, so the first device to sync brings its settings with it.
     */
    private fun pull(prefs: Prefs) {
        if (!Pass.carriesSettings || !prefs.syncSettings.value) return
        scope.launch {
            // A pull already running reads the vault again when it finishes.
            if (running) {
                again = true
                return@launch
            }
            running = true
            try {
                do {
                    again = false
                    runCatching {
                        val held = Pass.syncedSettings()
                        prefs.applySynced(held.mapValues { (_, value) -> fromJson(value) })
                        val missing = prefs.syncable
                            .filter { prefs.syncs(it) && it.key !in held && it.value != it.default }
                            .associate { it.key to toJson(it.value) }
                        if (missing.isNotEmpty()) Pass.syncSettings(JsonObject(missing))
                    }
                } while (again && Pass.carriesSettings)
            } finally {
                running = false
            }
        }
    }

    private fun push(prefs: Prefs, values: JsonObject) {
        if (!Pass.carriesSettings || !prefs.syncSettings.value) return
        scope.launch {
            // Read-only, or locked meanwhile: the next pull leaves it if it still differs.
            runCatching { Pass.syncSettings(values) }
        }
    }

    private fun toJson(value: Any?): JsonElement = when (value) {
        is Boolean -> JsonPrimitive(value)
        is Number -> JsonPrimitive(value)
        is String -> JsonPrimitive(value)
        else -> JsonNull
    }

    /** As a settings file would give it: a yes or no, a whole number, or text. */
    private fun fromJson(value: JsonElement): Any? {
        val primitive = value as? JsonPrimitive ?: return null
        if (primitive.isString) return primitive.content
        return primitive.booleanOrNull ?: primitive.intOrNull
    }
}

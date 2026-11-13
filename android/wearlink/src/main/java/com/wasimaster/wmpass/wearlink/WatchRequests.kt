package com.wasimaster.wmpass.wearlink

import android.content.Context
import com.google.android.gms.tasks.Task
import com.google.android.gms.wearable.WearableListenerService
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.async
import kotlinx.coroutines.tasks.asTask
import kotlinx.coroutines.withContext
import org.json.JSONArray
import org.json.JSONObject

/**
 * Answers the paired watch. The watch keeps nothing: it asks each time, and gets an answer only
 * while the vault on this phone is open and the person has turned this on. The Wearable API already
 * limits callers to the same package signed the same way on a paired watch.
 */
class WatchRequests : WearableListenerService() {
    companion object {
        /**
         * Asked before every answer, on the main thread: the app locks a vault whose background
         * time ran out while the phone slept, as autofill and the keyboard do, and says whether that
         * worked. False sends the watch nothing. Set by the app when it starts; unset, nothing is held back.
         */
        @Volatile
        var beforeServe: (suspend () -> Boolean)? = null

        /**
         * Whether the person lets the watch see the vault, turning it on the first time a watch
         * asks. Set by the app, on the main thread, so its own settings screen shows the switch as
         * it is; unset, the stored setting is read here.
         */
        @Volatile
        var access: (suspend () -> Boolean)? = null

        private const val MOST_CODES = 20
        private const val MOST_LOGINS = 20
    }

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onRequest(nodeId: String, path: String, request: ByteArray): Task<ByteArray>? {
        if (!path.startsWith("/wmpass/")) return null
        // A request that fails here (an item gone since the watch listed it) answers as the vault
        // stands, rather than as a failure the watch can only read as the phone being out of reach.
        return scope.async {
            runCatching { answer(path, request) }.getOrElse {
                JSONObject().put("state", if (Pass.state.value is LockState.Unlocked) "open" else "locked")
            }.toString().toByteArray()
        }.asTask()
    }

    private suspend fun answer(path: String, request: ByteArray): JSONObject {
        // A watch asking is a paired watch with WM Pass on it, so sharing starts on, as the setting
        // promises. Turned off in the app, it stays off.
        val check = access
        val allowed = if (check != null) {
            runCatching { withContext(Dispatchers.Main) { check() } }.getOrDefault(false)
        } else {
            val prefs = getSharedPreferences("prefs", Context.MODE_PRIVATE)
            if (!prefs.contains("wear_access")) prefs.edit().putBoolean("wear_access", true).apply()
            prefs.getBoolean("wear_access", false)
        }
        if (!allowed) return JSONObject().put("state", "off")
        // A lock that fails answers as locked, rather than serving a vault meant to be shut.
        val gate = beforeServe
        if (gate != null && !runCatching { withContext(Dispatchers.Main) { gate() } }.getOrDefault(false)) {
            return JSONObject().put("state", "locked")
        }
        if (Pass.state.value !is LockState.Unlocked) return JSONObject().put("state", "locked")
        val out = JSONObject().put("state", "open")
        when (path) {
            "/wmpass/items" -> items(out)
            // Only a login the watch was shown: not one in the trash or on the hidden shelf, whatever id is sent.
            "/wmpass/password" -> {
                val id = String(request)
                if (Pass.entries.value.any { it.id == id && it.kind == "login" && it.hasPassword && !it.trashed && !it.hidden }) {
                    out.put("password", Pass.reveal(id, "password").consume { String(it) })
                }
            }
            else -> out.put("state", "off")
        }
        return out
    }

    private suspend fun items(out: JSONObject) {
        // Never an item in the trash or on the hidden shelf: a wrist is for anyone nearby to see, as a home screen is.
        val entries = Pass.entries.value.filter { !it.trashed && !it.hidden }
        val favourite = entries.filter { it.favourite || it.pinned }
        val codes = JSONArray()
        val withCodes = (favourite + entries).filter { it.hasOtp }.distinctBy { it.id }.take(MOST_CODES)
        // Counter codes have no time left to count, and a row at zero seconds would have the
        // watch ask again every second.
        val rows = if (withCodes.isEmpty()) emptyList() else Pass.codes(withCodes.map { it.id }).filter { it.otp.kind != "hotp" && it.code != null }
        rows.forEach { row ->
            codes.put(
                JSONObject()
                    .put("id", row.id)
                    .put("title", row.title)
                    .put("code", row.code.orEmpty())
                    .put("seconds", row.secondsLeft ?: 0),
            )
        }
        out.put("codes", codes)
        val logins = JSONArray()
        favourite.filter { it.kind == "login" }.take(MOST_LOGINS).forEach {
            logins.put(JSONObject().put("id", it.id).put("title", it.title).put("username", it.subtitle).put("has_password", it.hasPassword))
        }
        out.put("favourites", logins)
    }

}

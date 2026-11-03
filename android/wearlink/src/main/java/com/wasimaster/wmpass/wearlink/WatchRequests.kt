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
import org.json.JSONArray
import org.json.JSONObject

/**
 * Answers the paired watch. The watch keeps nothing: it asks each time, and gets an answer only
 * while the vault on this phone is open and the person has turned this on. The Wearable API already
 * limits callers to the same package signed the same way on a paired watch.
 */
class WatchRequests : WearableListenerService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onRequest(nodeId: String, path: String, request: ByteArray): Task<ByteArray>? {
        if (!path.startsWith("/wmpass/")) return null
        return scope.async { answer(path, request).toString().toByteArray() }.asTask()
    }

    private suspend fun answer(path: String, request: ByteArray): JSONObject {
        val prefs = getSharedPreferences("prefs", Context.MODE_PRIVATE)
        // A watch asking is a paired watch with WM Pass on it, so sharing starts on, as the setting
        // promises. Turned off in the app, it stays off.
        if (!prefs.contains("wear_access")) prefs.edit().putBoolean("wear_access", true).apply()
        val allowed = prefs.getBoolean("wear_access", false)
        if (!allowed) return JSONObject().put("state", "off")
        if (Pass.state.value !is LockState.Unlocked) return JSONObject().put("state", "locked")
        val out = JSONObject().put("state", "open")
        when (path) {
            "/wmpass/items" -> items(out)
            "/wmpass/password" -> out.put("password", Pass.reveal(String(request), "password").consume { String(it) })
            else -> out.put("state", "off")
        }
        return out
    }

    private suspend fun items(out: JSONObject) {
        val entries = Pass.entries.value
        val favourite = entries.filter { it.favourite || it.pinned }
        val codes = JSONArray()
        val withCodes = (favourite + entries).filter { it.hasOtp }.distinctBy { it.id }.take(MOST_CODES)
        val rows = if (withCodes.isEmpty()) emptyList() else Pass.codes(withCodes.map { it.id })
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

    private companion object {
        const val MOST_CODES = 20
        const val MOST_LOGINS = 20
    }
}

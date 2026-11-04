package com.wasimaster.wmpass.system

import android.os.Handler
import android.os.Looper
import java.security.SecureRandom
import java.util.Base64
import java.util.concurrent.ConcurrentHashMap

/**
 * The screens whose fields offer WM Keyboard their buttons, each under the token its fields carry
 * in [FieldWire]. A screen adds itself while it shows and leaves when it goes, so a tap for a
 * screen that has closed, or one that never was, finds nobody and does nothing.
 */
object FieldActions {
    private val screens = ConcurrentHashMap<String, (String) -> Unit>()
    private val random = SecureRandom()

    /** A token no other screen has: 12 random bytes, which is 16 characters of base64url. */
    fun newToken(): String {
        val bytes = ByteArray(12).also(random::nextBytes)
        return Base64.getUrlEncoder().withoutPadding().encodeToString(bytes)
    }

    fun register(token: String, onAction: (String) -> Unit) {
        screens[token] = onAction
    }

    fun unregister(token: String) {
        screens.remove(token)
    }

    /**
     * Hands [action] to the screen called [token], on the main thread, where its own button would
     * have run it. False when no screen has that token or the action is not one WM Pass knows.
     */
    fun run(token: String, action: String): Boolean {
        if (action !in FieldWire.ACTIONS) return false
        val onAction = screens[token] ?: return false
        Handler(Looper.getMainLooper()).post { onAction(action) }
        return true
    }
}

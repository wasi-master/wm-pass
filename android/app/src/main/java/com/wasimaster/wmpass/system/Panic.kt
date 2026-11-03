package com.wasimaster.wmpass.system

import android.app.Activity
import android.os.Bundle
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * Answers panic signals from apps such as Ripple, once the owner has said what a signal does.
 * Only the apps they chose are listened to, so no other app can lock the vault over and over or
 * erase it. With Lock chosen and no app named, any app may ask for a lock, which costs nothing
 * but a retype. A wipe always needs a named app, since one we cannot identify must not be able to
 * erase a vault.
 */
class PanicActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val prefs = Prefs.of(this)
        val caller = callingActivity?.packageName
        val chosen = caller != null && caller in prefs.panicApps
        val act = when (prefs.panicAction) {
            "wipe" -> if (chosen) "wipe" else null
            "lock" -> if (chosen || prefs.panicApps.isEmpty()) "lock" else null
            else -> null
        }
        CoroutineScope(Dispatchers.Main.immediate).launch {
            if (intent?.action == ACTION_TRIGGER) {
                when (act) {
                    "wipe" -> Pass.wipe()
                    "lock" -> Pass.panic()
                }
            }
            finish()
        }
    }

    companion object {
        const val ACTION_TRIGGER = "info.guardianproject.panic.action.TRIGGER"

        /**
         * Apps known to send PanicKit's signal, put first when choosing: Guardian Project's
         * Ripple and Amnesty International's Panic Button.
         */
        val KNOWN = mapOf(
            "info.guardianproject.ripple" to "Ripple",
            "org.iilab.pb" to "Panic Button",
        )
    }
}

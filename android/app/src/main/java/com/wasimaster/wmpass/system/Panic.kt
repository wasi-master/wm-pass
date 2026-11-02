package com.wasimaster.wmpass.system

import android.app.Activity
import android.os.Bundle
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * Answers panic signals from apps such as Ripple. Any app may ask for a lock, which costs
 * nothing but a retype. Wiping happens only when the owner chose it and named that app here,
 * since an app we cannot identify must not be able to erase a vault.
 */
class PanicActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val prefs = Prefs.of(this)
        val caller = callingActivity?.packageName
        val wipe = prefs.panicAction == "wipe" && caller != null && caller == prefs.panicTrustedApp
        CoroutineScope(Dispatchers.Main.immediate).launch {
            if (intent?.action == ACTION_TRIGGER) {
                if (wipe) Pass.wipe() else Pass.panic()
            }
            finish()
        }
    }

    companion object {
        const val ACTION_TRIGGER = "info.guardianproject.panic.action.TRIGGER"
    }
}

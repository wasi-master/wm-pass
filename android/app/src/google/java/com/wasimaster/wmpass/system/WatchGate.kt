package com.wasimaster.wmpass.system

import android.content.Context
import com.wasimaster.wmpass.app.AutoLock
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.wearlink.WatchRequests

/**
 * The watch's check before each answer: a vault whose background time ran out while the phone
 * slept is locked first. In builds with Google's SDKs only, where the watch link is; src/nogoogle
 * has the same name doing nothing.
 */
internal object WatchGate {
    fun install(context: Context) {
        val app = context.applicationContext
        WatchRequests.beforeServe = {
            AutoLock.lockIfOverdue()
            true
        }
        // Through the app's own settings, so the switch in Settings shows on once a watch has
        // turned it on, rather than off while the watch is being answered.
        WatchRequests.access = {
            val prefs = Prefs.of(app)
            prefs.switchOnFirstTime(prefs.wearAccess)
            prefs.wearAccess.value
        }
    }
}

package com.wasimaster.wmpass.app

import android.app.Application
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import androidx.lifecycle.DefaultLifecycleObserver
import androidx.lifecycle.LifecycleOwner
import androidx.lifecycle.ProcessLifecycleOwner
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.SecureClipboard
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

/** Locks after a while in the background, and at once when the screen goes off. */
object AutoLock {
    private val scope = CoroutineScope(Dispatchers.Main.immediate)
    private var pending: Job? = null

    fun install(app: Application) {
        val prefs = Prefs.of(app)
        SecureClipboard.clearAfterSeconds = prefs.clipboardSeconds
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) {
                    pending?.cancel()
                }

                override fun onStop(owner: LifecycleOwner) {
                    scope.launch { Pass.flush() }
                    val after = prefs.lockAfter
                    if (after < 0) return
                    pending = scope.launch {
                        delay(after * 1000L)
                        Pass.lock()
                    }
                }
            },
        )
        app.registerReceiver(
            object : BroadcastReceiver() {
                override fun onReceive(context: Context, intent: Intent) {
                    if (prefs.lockAfter >= 0 && prefs.lockScreenOff.value) scope.launch { Pass.lock() }
                }
            },
            IntentFilter(Intent.ACTION_SCREEN_OFF),
        )
    }
}

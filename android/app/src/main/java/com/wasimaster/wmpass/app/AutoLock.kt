package com.wasimaster.wmpass.app

import android.app.Application
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.SystemClock
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

    /**
     * Until when, by the clock that counts deep sleep, the next move to the background counts as
     * going back to WM Keyboard: the vault was just opened from its WM Pass tool, and the person
     * is still typing there.
     */
    @Volatile
    private var keyboardUntil = 0L

    /** The shortest time the vault stays open after it was opened from WM Keyboard. */
    private const val KEYBOARD_GRACE_SECONDS = 60

    /**
     * The vault was opened from WM Keyboard: by the fingerprint over the app the person is in, or
     * by the master password typed into the keyboard. Either way the person stays in that other
     * app, so the background timer runs as if they had just left this one, though for at least
     * [KEYBOARD_GRACE_SECONDS]. "At once" means once the person leaves WM Pass, and they never
     * came in. Opened by the password, this app was never on screen and nothing else starts the
     * timer, so it starts here. Call on the main thread.
     */
    fun openedFromKeyboard(context: Context) {
        val after = Prefs.of(context).lockAfter
        if (after < 0) return
        val started = ProcessLifecycleOwner.get().lifecycle.currentState.isAtLeast(androidx.lifecycle.Lifecycle.State.STARTED)
        if (started) {
            // The fingerprint's screen is still up: its stop starts the timer, with the grace.
            keyboardUntil = android.os.SystemClock.elapsedRealtime() + KEYBOARD_HANDOFF_MS
            return
        }
        val wait = maxOf(after, KEYBOARD_GRACE_SECONDS)
        pending?.cancel()
        // As on leaving the app: the timer below stops while the phone sleeps, and the keyboard,
        // autofill and widgets check this one before they show anything.
        lockAt = SystemClock.elapsedRealtime() + wait * 1000L
        pending = scope.launch {
            delay(wait * 1000L)
            Pass.lock()
        }
    }

    /** [after], or the keyboard's grace when the stop is the fingerprint's screen closing. */
    private fun keyboardGrace(after: Int): Int {
        val handoff = keyboardUntil != 0L && android.os.SystemClock.elapsedRealtime() < keyboardUntil
        keyboardUntil = 0L
        return if (handoff && after >= 0) maxOf(after, KEYBOARD_GRACE_SECONDS) else after
    }

    /** How long after the fingerprint opens the vault its screen has to close. */
    private const val KEYBOARD_HANDOFF_MS = 10_000L

    /**
     * While the app waits on a system picker it opened (a file to attach or import, a backup
     * folder), it is in the background only because it asked to be. Locking then, at once with
     * the "At once" setting, would drop the editor and leave the picked file nowhere to go.
     */
    @Volatile
    private var awaitingResult = false

    /** The longest a picker may stay open before the vault locks after all. */
    private const val PICKER_GRACE_SECONDS = 300

    fun awaitingResult(waiting: Boolean) {
        awaitingResult = waiting
    }

    /**
     * When the background timer is due, by the clock that counts deep sleep, or zero. The timer
     * itself waits on the main thread's clock, which stops while the phone sleeps, so with the
     * screen-off lock turned off it would come due long after the time chosen.
     */
    @Volatile
    private var lockAt = 0L

    /** Locks now if the vault has been in the background longer than the setting allows. */
    suspend fun lockIfOverdue() {
        val at = lockAt
        if (at == 0L || SystemClock.elapsedRealtime() < at) return
        lockAt = 0L
        Pass.lock()
    }

    fun install(app: Application) {
        val prefs = Prefs.of(app)
        SecureClipboard.clearAfterSeconds = prefs.clipboardSeconds
        ProcessLifecycleOwner.get().lifecycle.addObserver(
            object : DefaultLifecycleObserver {
                override fun onStart(owner: LifecycleOwner) {
                    pending?.cancel()
                    // Back on screen, so any picker is done, even one whose result never came.
                    awaitingResult = false
                    // Past due while the phone slept: lock before anything is shown open.
                    val overdue = lockAt != 0L && SystemClock.elapsedRealtime() >= lockAt
                    lockAt = 0L
                    if (overdue) scope.launch { Pass.lock() }
                }

                override fun onStop(owner: LifecycleOwner) {
                    // A save that fails here is retried with the next change; it must not crash the app.
                    scope.launch { runCatching { Pass.flush() } }
                    val after = keyboardGrace(prefs.lockAfter)
                    if (after < 0) {
                        lockAt = 0L
                        return
                    }
                    val wait = if (awaitingResult) maxOf(after, PICKER_GRACE_SECONDS) else after
                    lockAt = SystemClock.elapsedRealtime() + wait * 1000L
                    pending = scope.launch {
                        delay(wait * 1000L)
                        Pass.lock()
                    }
                }
            },
        )
        app.registerReceiver(
            object : BroadcastReceiver() {
                override fun onReceive(context: Context, intent: Intent) {
                    // Its own switch, whatever the background timer says: with that at Never, the
                    // screen going off is the one thing left that locks by itself.
                    if (prefs.lockScreenOff.value) scope.launch { Pass.lock() }
                }
            },
            IntentFilter(Intent.ACTION_SCREEN_OFF),
        )
    }
}

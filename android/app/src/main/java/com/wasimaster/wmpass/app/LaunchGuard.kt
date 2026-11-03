package com.wasimaster.wmpass.app

import android.app.Application
import android.content.Context
import android.content.SharedPreferences
import android.os.Handler
import android.os.Looper

/**
 * Gets the owner out of a crash that a saved setting causes as soon as the app draws, without
 * clearing its data, which would take the vault with it.
 *
 * A crash on the render thread is native and cannot be caught, so the guard counts instead. The
 * door and the vault mark the launch as unsettled just before they draw, and clear the mark after
 * a few seconds on screen or as soon as the app leaves the screen. A process that finds the mark
 * still set knows the one before it died in those seconds. Two such launches in a row put the
 * look back to standard.
 *
 * Leaving the app clears the mark before anything can end the process, so a swipe from recents,
 * the screen going off, a lock or the system freeing memory in the background never counts.
 */
object LaunchGuard {
    /** How long something has to stay on screen before it counts as drawn safely. */
    private const val STEADY_MS = 5_000L

    const val NOTICE = "WM Pass closed twice right after opening, so Appearance is back to the standard style."

    private var store: SharedPreferences? = null
    private var marks = LaunchMarks()
    private val main = Handler(Looper.getMainLooper())
    private val settleLater = Runnable { settle() }

    /** Once per process, before anything is drawn. Runs [resetLook] after two short launches. */
    fun start(app: Application, resetLook: () -> Unit) {
        val prefs = app.getSharedPreferences("launches", Context.MODE_PRIVATE)
        store = prefs
        marks = LaunchMarks(
            armed = prefs.getBoolean("armed", false),
            shown = prefs.getBoolean("shown", false),
            strikes = prefs.getInt("strikes", 0),
            notice = prefs.getBoolean("notice", false),
        )
        val (next, reset) = marks.started()
        save(next)
        if (reset) resetLook()
    }

    /** The door or the vault is about to draw. */
    fun arm() {
        main.removeCallbacks(settleLater)
        save(marks.armed())
        main.postDelayed(settleLater, STEADY_MS)
    }

    /** What was drawn has lasted, or the app has left the screen. */
    fun settle() {
        main.removeCallbacks(settleLater)
        save(marks.settled())
    }

    /** [NOTICE] the first time this is asked after a reset, otherwise null. */
    fun takeNotice(): String? {
        if (!marks.notice) return null
        save(marks.copy(notice = false))
        return NOTICE
    }

    private fun save(next: LaunchMarks) {
        if (next == marks) return
        marks = next
        // On disk before returning: the crash this is for can come within the next frame.
        store?.edit()
            ?.putBoolean("armed", next.armed)
            ?.putBoolean("shown", next.shown)
            ?.putInt("strikes", next.strikes)
            ?.putBoolean("notice", next.notice)
            ?.commit()
    }
}

/** What [LaunchGuard] keeps from one process to the next. */
internal data class LaunchMarks(
    /** Something was drawn and has not lasted long enough yet. */
    val armed: Boolean = false,
    /** This process drew the door or the vault at all. */
    val shown: Boolean = false,
    /** Launches in a row that died while armed. */
    val strikes: Int = 0,
    /** The look was put back and nothing has said so yet. */
    val notice: Boolean = false,
) {
    /**
     * At the start of a process: counts the last one if it died while armed, and forgives the
     * count if it drew and settled. One that never drew, started for autofill say, changes
     * nothing. The second value is true when the look has to go back to standard.
     */
    fun started(): Pair<LaunchMarks, Boolean> {
        val count = when {
            armed -> strikes + 1
            shown -> 0
            else -> strikes
        }
        if (count >= STRIKES) return LaunchMarks(notice = true) to true
        return LaunchMarks(strikes = count, notice = notice) to false
    }

    fun armed() = copy(armed = true, shown = true)

    fun settled() = copy(armed = false)

    companion object {
        const val STRIKES = 2
    }
}

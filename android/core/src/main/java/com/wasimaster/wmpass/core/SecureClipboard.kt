package com.wasimaster.wmpass.core

import android.content.ClipData
import android.content.ClipDescription
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.PersistableBundle
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch

/** A copy waiting to be wiped. */
data class CopyInFlight(val label: String, val startedAtMs: Long, val wipeAtMs: Long)

/**
 * Copies a secret marked sensitive, so the system keeps it out of previews and clipboard
 * history where it honours the flag, and wipes it after a delay unless something else has
 * been copied since.
 */
object SecureClipboard {
    private val scope = CoroutineScope(Dispatchers.Main.immediate)
    private var wipeJob: Job? = null
    private var listener: ClipboardManager.OnPrimaryClipChangedListener? = null
    private val mutable = MutableStateFlow<CopyInFlight?>(null)

    /**
     * Whether the clip is still a secret copied here, timed or not: a panic takes it off the
     * clipboard even when the timer is off or the copy was a generated password.
     */
    @Volatile
    private var ownsClip = false

    /** The copy under way, for the capsule that counts it down. */
    val current: StateFlow<CopyInFlight?> = mutable.asStateFlow()

    /** Seconds before a copy is wiped. Zero leaves it. */
    @Volatile
    var clearAfterSeconds: Int = 60

    fun copy(context: Context, label: String, secret: SecretBytes) {
        copyText(context, label, secret.consumeText(), sensitive = true)
    }

    /**
     * A password just made by the generator and saved nowhere yet. Marked sensitive like any
     * secret, but never wiped: it is on its way to a site's sign-up or change-password form,
     * and wiping it there, before it is saved here, would lose the only copy.
     */
    fun copyGenerated(context: Context, label: String, secret: SecretBytes) {
        copyText(context, label, secret.consumeText(), sensitive = true, wipe = false)
    }

    fun copyPlain(context: Context, label: String, text: String) {
        copyText(context, label, text, sensitive = false)
    }

    private fun copyText(context: Context, label: String, text: String, sensitive: Boolean, wipe: Boolean = true) {
        val manager = context.applicationContext.getSystemService(ClipboardManager::class.java) ?: return
        val clip = ClipData.newPlainText(label, text)
        if (sensitive) {
            clip.description.extras = PersistableBundle().apply {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    putBoolean(ClipDescription.EXTRA_IS_SENSITIVE, true)
                }
                // What Android 12 and older read, by its literal name.
                putBoolean("android.content.extra.IS_SENSITIVE", true)
            }
        }
        listener?.let(manager::removePrimaryClipChangedListener)
        listener = null
        manager.setPrimaryClip(clip)
        wipeJob?.cancel()
        ownsClip = sensitive
        if (!sensitive) {
            mutable.value = null
            return
        }
        val onChanged = ClipboardManager.OnPrimaryClipChangedListener {
            // Something else was copied: the secret is gone and the new clip is not ours.
            wipeJob?.cancel()
            ownsClip = false
            mutable.value = null
            listener?.let(manager::removePrimaryClipChangedListener)
            listener = null
        }
        listener = onChanged
        manager.addPrimaryClipChangedListener(onChanged)
        if (!wipe || clearAfterSeconds == 0) {
            mutable.value = null
            return
        }
        val now = System.currentTimeMillis()
        val delayMs = clearAfterSeconds * 1000L
        mutable.value = CopyInFlight(label, now, now + delayMs)
        // The application, not the screen that copied: an activity would be held until the wipe.
        val app = context.applicationContext
        wipeJob = scope.launch {
            delay(delayMs)
            wipe(app)
        }
    }

    /**
     * Wipes now: a timed copy, or with [everything], any secret copied here that is still on the
     * clipboard, as a panic wants.
     */
    fun wipe(context: Context, everything: Boolean = false) {
        val manager = context.applicationContext.getSystemService(ClipboardManager::class.java) ?: return
        wipeJob?.cancel()
        listener?.let(manager::removePrimaryClipChangedListener)
        listener = null
        if (mutable.value != null || (everything && ownsClip)) {
            runCatching {
                // clearPrimaryClip is Android 9 and up. Before it, an empty clip takes the secret's place.
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                    manager.clearPrimaryClip()
                } else {
                    manager.setPrimaryClip(ClipData.newPlainText("", ""))
                }
            }
        }
        ownsClip = false
        mutable.value = null
    }
}

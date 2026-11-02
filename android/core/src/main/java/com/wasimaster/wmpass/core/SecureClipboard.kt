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

    /** The copy under way, for the capsule that counts it down. */
    val current: StateFlow<CopyInFlight?> = mutable.asStateFlow()

    /** Seconds before a copy is wiped. Zero leaves it. */
    @Volatile
    var clearAfterSeconds: Int = 30

    fun copy(context: Context, label: String, secret: SecretBytes) {
        copyText(context, label, secret.consumeText(), sensitive = true)
    }

    fun copyPlain(context: Context, label: String, text: String) {
        copyText(context, label, text, sensitive = false)
    }

    private fun copyText(context: Context, label: String, text: String, sensitive: Boolean) {
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
        manager.setPrimaryClip(clip)
        wipeJob?.cancel()
        if (!sensitive || clearAfterSeconds == 0) {
            mutable.value = null
            return
        }
        val now = System.currentTimeMillis()
        val delayMs = clearAfterSeconds * 1000L
        mutable.value = CopyInFlight(label, now, now + delayMs)
        val onChanged = ClipboardManager.OnPrimaryClipChangedListener {
            // Something else was copied: the secret is gone and the new clip is not ours.
            wipeJob?.cancel()
            mutable.value = null
            listener?.let(manager::removePrimaryClipChangedListener)
            listener = null
        }
        listener = onChanged
        manager.addPrimaryClipChangedListener(onChanged)
        wipeJob = scope.launch {
            delay(delayMs)
            wipe(context)
        }
    }

    /** Wipes now. */
    fun wipe(context: Context) {
        val manager = context.applicationContext.getSystemService(ClipboardManager::class.java) ?: return
        wipeJob?.cancel()
        listener?.let(manager::removePrimaryClipChangedListener)
        listener = null
        if (mutable.value != null) {
            runCatching { manager.clearPrimaryClip() }
        }
        mutable.value = null
    }
}

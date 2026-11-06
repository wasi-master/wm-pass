package com.wasimaster.wmpass.system

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Bundle
import androidx.biometric.BiometricManager
import com.wasimaster.wmpass.app.AutoLock
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.QuickUnlock
import com.wasimaster.wmpass.core.UnlockResult
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * The calls of [KeyboardProvider] that work on a locked vault: opening it from WM Keyboard's WM
 * Pass tool, and locking it again.
 *
 * Nothing here opens the vault without the person. [VaultWire.UNLOCK_PROMPT] hands the keyboard a
 * way to start [UnlockActivity], which asks for the fingerprint over the app the person is in, as
 * autofill does; the keyboard can start it but cannot answer it. [VaultWire.UNLOCK] takes the
 * master password the person typed into the keyboard's own box. That password went through the
 * keyboard either way, as the door's field is typed with it too, and it meets the same count of
 * wrong tries, the same waits and the same duress handling as at the door. A vault that also
 * needs a key file or a hardware key is opened at the door.
 */
internal object KeyboardUnlock {
    /** The request code of the prompt's pending intent, so a new one replaces the last. */
    private const val PROMPT_REQUEST = 7301

    /**
     * Answers [method] when it is one of the calls this object handles, null otherwise. Runs on
     * the provider's binder thread, inside its `runBlocking`.
     */
    suspend fun call(context: Context, method: String, extras: Bundle?): Bundle? = when (method) {
        VaultWire.UNLOCK_PROMPT -> state().apply {
            if (Pass.state.value == LockState.Locked) putParcelable(VaultWire.INTENT, prompt(context))
        }
        VaultWire.UNLOCK -> unlock(context, extras?.getByteArray(VaultWire.SECRET))
        VaultWire.LOCK -> {
            withContext(Dispatchers.Main) { Pass.lock() }
            state()
        }
        else -> null
    }

    /** What the tool shows while the vault is locked: whether to offer the fingerprint, and any wait. */
    fun describeLocked(context: Context, out: Bundle) {
        val retryAt = Pass.retryAt()
        out.putLong(VaultWire.RETRY_AT, retryAt)
        out.putBoolean(VaultWire.FINGERPRINT, retryAt == 0L && fingerprintReady(context))
    }

    /** The open vault's name, for the tool's footer. */
    fun describeOpen(out: Bundle) {
        (Pass.state.value as? LockState.Unlocked)?.let { out.putString(VaultWire.VAULT, it.name) }
    }

    private fun state(): Bundle = Bundle().apply {
        putString(
            VaultWire.STATE,
            when (Pass.state.value) {
                is LockState.Unlocked -> "open"
                LockState.Locked -> "locked"
                LockState.NoVault -> "none"
            },
        )
    }

    /** The same test the door and [SecureActivity] make before they offer the fingerprint. */
    private fun fingerprintReady(context: Context): Boolean {
        val quick = runCatching { QuickUnlock(context) }.getOrNull() ?: return false
        return quick.enrolledFile() != null &&
            BiometricManager.from(context).canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG) == BiometricManager.BIOMETRIC_SUCCESS
    }

    /**
     * Starts [UnlockActivity] over the app in front. Only this app can make it, and only the
     * keyboard, proved by its certificate, is given it.
     */
    private fun prompt(context: Context): PendingIntent {
        val intent = Intent(context, UnlockActivity::class.java)
            .putExtra(UnlockActivity.FOR_KEYBOARD, true)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_NO_ANIMATION)
        return PendingIntent.getActivity(context, PROMPT_REQUEST, intent, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT)
    }

    private suspend fun unlock(context: Context, secret: ByteArray?): Bundle {
        if (secret == null || secret.isEmpty()) return state().apply { putString(VaultWire.OUTCOME, "wrong") }
        try {
            if (Pass.state.value != LockState.Locked) return state()
            // On the main thread, where the door opens the vault and everything that watches the
            // vault's state expects the change.
            val result = withContext(Dispatchers.Main) { runCatching { Pass.unlock(secret) }.getOrNull() }
            val out = state()
            when (result) {
                UnlockResult.Opened -> {
                    withContext(Dispatchers.Main) { AutoLock.openedFromKeyboard(context) }
                    out.putString(VaultWire.OUTCOME, "opened")
                    describeOpen(out)
                }
                is UnlockResult.Wrong -> out.putString(VaultWire.OUTCOME, "wrong").also { out.putLong(VaultWire.RETRY_AT, result.retryAtMs) }
                is UnlockResult.Waiting -> out.putString(VaultWire.OUTCOME, "waiting").also { out.putLong(VaultWire.RETRY_AT, result.retryAtMs) }
                // The key file or hardware key is asked for at the door, which has the pickers for them.
                is UnlockResult.NeedsFactor -> out.putString(VaultWire.OUTCOME, "factor")
                // A duress password's hard lock looks like a wait, as it does at the door.
                UnlockResult.HardLocked -> out.putString(VaultWire.OUTCOME, "waiting").also { out.putLong(VaultWire.RETRY_AT, Pass.retryAt()) }
                null -> out.putString(VaultWire.OUTCOME, "failed")
            }
            return out
        } finally {
            secret.fill(0)
        }
    }
}

package com.wasimaster.wmpass.system

import android.os.Bundle
import android.os.SystemClock
import android.view.WindowManager
import android.widget.Toast
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.lifecycle.lifecycleScope
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.QuickUnlock
import com.wasimaster.wmpass.ui.NoLearningInput
import com.wasimaster.wmpass.ui.door.DoorScreen
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.PassTheme
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.rememberSystemAnimationsOff
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch

/**
 * Base for every screen that shows or asks for a secret: no screenshots, no overlays.
 *
 * A screen started from another app (a form to fill, a passkey, a widget) sets [overlayUnlock]
 * and runs with a see-through window. Locked, with fingerprint unlock set up, it then shows only
 * the system's fingerprint prompt over the app the person was in. The door comes up only when
 * they ask for the password instead, or the fingerprint cannot be used.
 */
abstract class SecureActivity : FragmentActivity() {
    /** Whether this screen tries the fingerprint over the app behind before showing the door. */
    protected open val overlayUnlock: Boolean = false

    private var showDoor by mutableStateOf(true)
    private var doorAsksFingerprint by mutableStateOf(true)
    private var fingerprintAt = 0L
    private var restoring = false

    /** Whether the fingerprint opened the vault here a moment ago, so a second check would only repeat it. */
    protected val justVerified: Boolean
        get() = fingerprintAt != 0L && SystemClock.elapsedRealtime() - fingerprintAt < VERIFIED_FOR_MS

    override fun onCreate(savedInstanceState: Bundle?) {
        restoring = savedInstanceState != null
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        window.decorView.filterTouchesWhenObscured = true
        // On in every build unless the owner turns it off: a debug build is still somebody's vault.
        if (Prefs.of(this).screenProtection) {
            window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        }
    }

    /** The door until the vault is open, then [content]. */
    protected fun gated(content: @Composable () -> Unit) {
        val prefs = Prefs.of(this)
        // A bug in the core while this screen is up has locked the vault: the screen goes, saying
        // so in a line, back to the app it was asked from. An error from before it opened is
        // nothing to it: the door is a fresh start.
        lifecycleScope.launch {
            Engine.stopped.drop(1).first { it }
            Toast.makeText(this@SecureActivity, "WM Pass hit an internal error and locked the vault. Try again.", Toast.LENGTH_LONG).show()
            finish()
        }
        // Only on a first start: after a rotation the prompt, or the door, is already up.
        // Not while every way in is refused, after a panic or too many tries: the door says how long.
        if (overlayUnlock && Pass.state.value == LockState.Locked && !restoring && Pass.retryAt() == 0L) {
            val quick = runCatching { QuickUnlock(this) }.getOrNull()
            val cipher = quick?.takeIf { fingerprintReady(it) }?.let { runCatching { it.cipherForUnlocking() }.getOrNull() }
            if (quick != null && cipher != null) {
                showDoor = false
                window.decorView.post { askFingerprint(quick, cipher) }
            }
        }
        setContent {
            PassTheme(prefs.look, prefs.accent, prefs.themeId, prefs.uiStyle, prefs.customThemes) {
                // Followed live: turning the system's animations off while this screen is up applies at once.
                CompositionLocalProvider(LocalReduceMotion provides (rememberSystemAnimationsOff() || !prefs.richMotion.value)) {
                    val state by Pass.state.collectAsStateWithLifecycle()
                    // As in the app itself, no keyboard learns the usernames, sites and searches typed here.
                    NoLearningInput {
                        when {
                            state is LockState.Unlocked -> content()
                            // The window may be see-through, so the door brings its own ground.
                            showDoor -> Box(Modifier.fillMaxSize().background(palette.ground)) {
                                DoorScreen(state, prefs, askFingerprint = doorAsksFingerprint)
                            }
                            // Nothing of this app's while the fingerprint prompt is up over the other one.
                            else -> Unit
                        }
                    }
                }
            }
        }
    }

    private fun fingerprintReady(quick: QuickUnlock): Boolean =
        quick.enrolledFile() != null &&
            BiometricManager.from(this).canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG) == BiometricManager.BIOMETRIC_SUCCESS

    private fun askFingerprint(quick: QuickUnlock, cipher: javax.crypto.Cipher) {
        if (isFinishing || isDestroyed) return
        val prompt = BiometricPrompt(
            this,
            ContextCompat.getMainExecutor(this),
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) {
                    val authorised = result.cryptoObject?.cipher ?: return toDoor()
                    lifecycleScope.launch {
                        val key = runCatching { quick.unwrap(authorised) }.getOrNull() ?: return@launch toDoor()
                        val file = quick.enrolledFile()
                        val done = file != null && runCatching { Pass.unlockWithKey(key, file) }.isSuccess
                        key.fill(0)
                        if (done) fingerprintAt = SystemClock.elapsedRealtime() else toDoor()
                    }
                }

                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                    when (errorCode) {
                        // Waved away: back to the app it came from, as if nothing was tapped.
                        BiometricPrompt.ERROR_USER_CANCELED, BiometricPrompt.ERROR_CANCELED -> finish()
                        // Use password, a lockout, or a sensor that stopped answering.
                        else -> toDoor()
                    }
                }
            },
        )
        prompt.authenticate(
            BiometricPrompt.PromptInfo.Builder()
                .setTitle("Unlock WM Pass")
                .setNegativeButtonText("Use password")
                .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
                .build(),
            BiometricPrompt.CryptoObject(cipher),
        )
    }

    private fun toDoor() {
        doorAsksFingerprint = false
        showDoor = true
    }

    private companion object {
        const val VERIFIED_FOR_MS = 60_000L
    }
}

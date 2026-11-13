package com.wasimaster.wmpass.app

import android.content.Intent
import android.content.IntentSender
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.view.WindowManager
import androidx.activity.compose.setContent
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import com.wasimaster.wmpass.system.SecureActivity
import com.wasimaster.wmpass.ui.AppRoot
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.PassTheme
import com.wasimaster.wmpass.ui.theme.rememberSystemAnimationsOff
import kotlinx.coroutines.flow.MutableStateFlow

/** Something arriving from outside the app, waiting for the vault to be open. */
sealed interface Incoming {
    data class Route(val name: String) : Incoming
    data class SharedText(val text: String) : Incoming
    data class SharedFile(val uri: Uri, val mime: String) : Incoming
    data class OtpLink(val uri: String) : Incoming
}

object Inbox {
    val pending = MutableStateFlow<Incoming?>(null)
}

class MainActivity : SecureActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState == null) receive(intent)
        val prefs = Prefs.of(this)
        setContent {
            // Off only by the owner's hand.
            LaunchedEffect(prefs.screenProtection) {
                if (prefs.screenProtection) {
                    window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
                } else {
                    window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
                }
            }
            PassTheme(prefs.look, prefs.accent, prefs.themeId, prefs.uiStyle, prefs.customThemes) {
                // Followed live: turning the system's animations off while the app is open applies at once.
                CompositionLocalProvider(LocalReduceMotion provides (rememberSystemAnimationsOff() || !prefs.richMotion.value)) {
                    AppRoot(prefs)
                }
            }
        }
    }

    // The door, or a vault left open, draws once the app is back on screen.
    override fun onDestroy() {
        // Backing out of the door gives up what was shared: it must not open hours later on the
        // next unlock, and a shared file's permission ends with this activity anyway.
        if (isFinishing) Inbox.pending.value = null
        super.onDestroy()
    }

    override fun onStart() {
        super.onStart()
        LaunchGuard.arm()
    }

    // Off screen, nothing drawn here can take the process down, so whatever ends it now
    // (a swipe from recents, the system freeing memory) is not a crash.
    override fun onStop() {
        LaunchGuard.settle()
        super.onStop()
    }

    // Every picker launched from these screens goes out through here and back through
    // onActivityResult, whichever result API asked for it: the vault waits for it to come back.
    @Deprecated("Called by the result APIs; kept only to see pickers go and come back.")
    override fun startActivityForResult(intent: Intent, requestCode: Int, options: Bundle?) {
        AutoLock.awaitingResult(true)
        super.startActivityForResult(intent, requestCode, options)
    }

    @Deprecated("Called by the result APIs; kept only to see pickers go and come back.")
    override fun startIntentSenderForResult(
        intent: IntentSender,
        requestCode: Int,
        fillInIntent: Intent?,
        flagsMask: Int,
        flagsValues: Int,
        extraFlags: Int,
        options: Bundle?,
    ) {
        AutoLock.awaitingResult(true)
        super.startIntentSenderForResult(intent, requestCode, fillInIntent, flagsMask, flagsValues, extraFlags, options)
    }

    @Deprecated("Called by the result APIs; kept only to see pickers go and come back.")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        AutoLock.awaitingResult(false)
        super.onActivityResult(requestCode, resultCode, data)
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        receive(intent)
    }

    private fun receive(intent: Intent?) {
        intent ?: return
        // Opened again from Recents after the app was closed, Android hands back the intent that
        // first started it: a text shared, a widget's route or a code link already dealt with.
        if (intent.flags and Intent.FLAG_ACTIVITY_LAUNCHED_FROM_HISTORY != 0) return
        // Only something new takes the place of what is waiting: opening the app from the launcher
        // or a widget while a share waits on the door must not drop it.
        Inbox.pending.value = when (intent.action) {
            Intent.ACTION_SEND -> {
                val text = intent.getStringExtra(Intent.EXTRA_TEXT)
                val file = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra(Intent.EXTRA_STREAM)
                }
                // Only a file another app hands over. A file: address, or one of this app's own,
                // would be read with this app's rights: any app could have this one's private
                // files, such as its settings, put into the vault and carried off by its next sync.
                val shared = file?.takeIf { it.scheme == "content" && it.authority?.startsWith(packageName) != true }
                when {
                    shared != null -> Incoming.SharedFile(shared, intent.type.orEmpty())
                    text != null -> Incoming.SharedText(text)
                    else -> null
                }
            }
            Intent.ACTION_VIEW -> {
                val data = intent.dataString
                val route = intent.getStringExtra(EXTRA_ROUTE)
                when {
                    data != null && data.startsWith("otpauth://") -> Incoming.OtpLink(data)
                    route != null -> Incoming.Route(route)
                    else -> null
                }
            }
            else -> null
        } ?: return
    }

    companion object {
        const val EXTRA_ROUTE = "route"
    }
}

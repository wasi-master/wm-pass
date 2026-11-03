package com.wasimaster.wmpass.app

import android.content.Intent
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
import com.wasimaster.wmpass.ui.theme.systemAnimationsOff
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
                CompositionLocalProvider(LocalReduceMotion provides (systemAnimationsOff() || !prefs.richMotion.value)) {
                    AppRoot(prefs)
                }
            }
        }
    }

    // The door, or a vault left open, draws once the app is back on screen.
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

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        setIntent(intent)
        receive(intent)
    }

    private fun receive(intent: Intent?) {
        intent ?: return
        Inbox.pending.value = when (intent.action) {
            Intent.ACTION_SEND -> {
                val text = intent.getStringExtra(Intent.EXTRA_TEXT)
                val file = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
                    intent.getParcelableExtra(Intent.EXTRA_STREAM, Uri::class.java)
                } else {
                    @Suppress("DEPRECATION")
                    intent.getParcelableExtra(Intent.EXTRA_STREAM)
                }
                when {
                    file != null -> Incoming.SharedFile(file, intent.type.orEmpty())
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
        }
    }

    companion object {
        const val EXTRA_ROUTE = "route"
    }
}

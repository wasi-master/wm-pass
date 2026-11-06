package com.wasimaster.wmpass.system

import android.os.Bundle
import androidx.compose.runtime.LaunchedEffect
import com.wasimaster.wmpass.app.AutoLock
import com.wasimaster.wmpass.system.widgets.WidgetFeed

/**
 * A widget's unlock, or WM Keyboard's: over the home screen or the app being typed in, gone
 * again once the vault is open. The keyboard reaches it only through the pending intent
 * [KeyboardUnlock] hands it, which sets [FOR_KEYBOARD].
 */
class UnlockActivity : SecureActivity() {
    override val overlayUnlock = true

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val forKeyboard = intent.getBooleanExtra(FOR_KEYBOARD, false)
        gated {
            LaunchedEffect(Unit) {
                // The person goes on typing in the app behind, so leaving this screen is not
                // leaving WM Pass: the background timer gets the keyboard's grace.
                if (forKeyboard) AutoLock.openedFromKeyboard(this@UnlockActivity)
                WidgetFeed.redraw(this@UnlockActivity)
                finish()
            }
        }
    }

    companion object {
        const val FOR_KEYBOARD = "for_keyboard"
    }
}

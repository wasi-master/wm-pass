package com.wasimaster.wmpass.app

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.LaunchedEffect
import androidx.fragment.app.FragmentActivity
import com.wasimaster.wmpass.BuildConfig
import com.wasimaster.wmpass.ui.AppRoot
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.PassTheme
import com.wasimaster.wmpass.ui.theme.systemAnimationsOff

class MainActivity : FragmentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        // A touch through another app's overlay is refused, so nothing can lure a tap on
        // "Copy password" through a window drawn on top of this one.
        window.decorView.filterTouchesWhenObscured = true
        val prefs = Prefs.of(this)
        setContent {
            // Off only by the owner's hand, and debug builds so screens can be inspected.
            LaunchedEffect(prefs.screenProtection) {
                if (prefs.screenProtection && !BuildConfig.DEBUG) {
                    window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
                } else {
                    window.clearFlags(WindowManager.LayoutParams.FLAG_SECURE)
                }
            }
            PassTheme(prefs.look, prefs.accent) {
                CompositionLocalProvider(LocalReduceMotion provides (systemAnimationsOff() || !prefs.richMotion)) {
                    AppRoot(prefs)
                }
            }
        }
    }
}

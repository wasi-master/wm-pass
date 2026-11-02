package com.wasimaster.wmpass.system

import android.os.Bundle
import android.view.WindowManager
import androidx.activity.compose.setContent
import androidx.activity.enableEdgeToEdge
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.getValue
import androidx.fragment.app.FragmentActivity
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.BuildConfig
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.door.DoorScreen
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.PassTheme
import com.wasimaster.wmpass.ui.theme.systemAnimationsOff

/** Base for every screen that shows or asks for a secret: no screenshots, no overlays. */
abstract class SecureActivity : FragmentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        enableEdgeToEdge()
        super.onCreate(savedInstanceState)
        window.decorView.filterTouchesWhenObscured = true
        if (!BuildConfig.DEBUG) {
            window.setFlags(WindowManager.LayoutParams.FLAG_SECURE, WindowManager.LayoutParams.FLAG_SECURE)
        }
    }

    /** The door until the vault is open, then [content]. */
    protected fun gated(content: @Composable () -> Unit) {
        val prefs = Prefs.of(this)
        setContent {
            PassTheme(prefs.look, prefs.accent, prefs.themeId, prefs.uiStyle, prefs.customThemes) {
                CompositionLocalProvider(LocalReduceMotion provides (systemAnimationsOff() || !prefs.richMotion.value)) {
                    val state by Pass.state.collectAsStateWithLifecycle()
                    if (state is LockState.Unlocked) content() else DoorScreen(state, prefs)
                }
            }
        }
    }
}

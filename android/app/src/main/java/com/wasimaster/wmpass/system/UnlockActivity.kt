package com.wasimaster.wmpass.system

import android.os.Bundle
import androidx.compose.runtime.LaunchedEffect
import com.wasimaster.wmpass.system.widgets.WidgetFeed

/** A widget's unlock: over the home screen, gone again once the vault is open. */
class UnlockActivity : SecureActivity() {
    override val overlayUnlock = true

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        gated {
            LaunchedEffect(Unit) {
                WidgetFeed.redraw(this@UnlockActivity)
                finish()
            }
        }
    }
}

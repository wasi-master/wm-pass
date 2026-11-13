package com.wasimaster.wmpass.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.safeDrawing
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.windowInsetsPadding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.paneTitle
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.palette

const val INTERNAL_ERROR_TITLE = "Something went wrong"
const val INTERNAL_ERROR_BODY =
    "WM Pass hit an internal error and locked the vault to keep it safe. Anything you were typing is still on this " +
        "screen: copy it if you need it, then unlock again. Saved items are not affected."

/**
 * The message after an internal error (Engine.stopped). It sits over the top of the screen without
 * taking it away, so whatever was being typed can still be selected and copied: a tap outside it
 * reaches the screen and never closes it. Only its button, or Back, goes on to the door.
 */
@Composable
fun InternalErrorNotice() {
    val colors = palette
    BackHandler { Pass.leaveInternal() }
    Box(Modifier.fillMaxSize().windowInsetsPadding(WindowInsets.safeDrawing).padding(16.dp), contentAlignment = Alignment.TopCenter) {
        Column(
            Modifier
                .widthIn(max = 480.dp)
                .fillMaxWidth()
                .shadow(16.dp, Shapes.card)
                .background(colors.surface2, Shapes.card)
                .border(1.dp, colors.hairline, Shapes.card)
                .semantics {
                    paneTitle = INTERNAL_ERROR_TITLE
                    liveRegion = LiveRegionMode.Assertive
                }
                .padding(20.dp),
        ) {
            Text(INTERNAL_ERROR_TITLE, color = colors.text, style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(8.dp))
            Text(INTERNAL_ERROR_BODY, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            Spacer(Modifier.height(16.dp))
            PrimaryButton("Go to unlock", Modifier.fillMaxWidth()) { Pass.leaveInternal() }
        }
    }
}

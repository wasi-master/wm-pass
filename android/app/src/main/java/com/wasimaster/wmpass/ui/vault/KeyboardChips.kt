package com.wasimaster.wmpass.ui.vault

import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.text.input.PlatformImeOptions
import com.wasimaster.wmpass.system.FieldActions
import com.wasimaster.wmpass.system.FieldWire

/**
 * This screen's token for WM Keyboard's chips, with [onAction] taking the buttons the keyboard
 * presses for the person. The screen answers only while it is showing.
 */
@Composable
fun rememberKeyboardActions(onAction: (String) -> Unit): String {
    val latest by rememberUpdatedState(onAction)
    val token = remember { FieldActions.newToken() }
    DisposableEffect(token) {
        FieldActions.register(token) { latest(it) }
        onDispose { FieldActions.unregister(token) }
    }
    return token
}

/** These options, with the field telling WM Keyboard which of the screen's [actions] it can offer. */
fun KeyboardOptions.offering(token: String, actions: List<String>): KeyboardOptions {
    val tag = FieldWire.tag(token, actions) ?: return this
    return copy(platformImeOptions = PlatformImeOptions(tag))
}

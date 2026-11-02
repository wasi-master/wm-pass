package com.wasimaster.wmpass.ui.vault

import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import com.wasimaster.wmpass.ui.settings.ToolsBody
import com.wasimaster.wmsuite.settings.WmScreen

/** The tools, for people who keep them in the dock. */
@Composable
fun ToolsTab() {
    Box(Modifier.fillMaxSize()) {
        WmScreen("Tools") { ToolsBody() }
    }
}


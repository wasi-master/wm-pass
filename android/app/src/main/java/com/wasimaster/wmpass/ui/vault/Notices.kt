package com.wasimaster.wmpass.ui.vault

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.semantics.LiveRegionMode
import androidx.compose.ui.semantics.liveRegion
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.palette

/**
 * What the vault has to tell its owner, at the top of the vault: a master password changed on two
 * devices at once, with the way to Unlock methods, and a device's clock that looks wrong. Each is
 * said again on every unlock until it is dealt with; Dismiss puts it away until then.
 */
@Composable
fun VaultNoticeBanner(modifier: Modifier = Modifier) {
    val shell = LocalShell.current
    val notices by Pass.notices.collectAsStateWithLifecycle()
    var dismissed by remember { mutableStateOf(emptySet<String>()) }
    val shown = listOfNotNull(notices.passwordConflict, notices.clockWarning).filter { it !in dismissed }
    if (shown.isEmpty()) return
    val colors = palette
    Column(modifier.widthIn(max = 560.dp).fillMaxWidth().padding(horizontal = 14.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        shown.forEach { message ->
            val conflict = message == notices.passwordConflict
            Surface(
                shape = RoundedCornerShape(20.dp),
                color = colors.surface3,
                tonalElevation = 3.dp,
                shadowElevation = 4.dp,
                modifier = Modifier.fillMaxWidth().semantics { liveRegion = LiveRegionMode.Polite },
            ) {
                Column(Modifier.padding(start = 16.dp, end = 8.dp, top = 14.dp, bottom = 4.dp)) {
                    Row(verticalAlignment = Alignment.Top) {
                        Icon(PassIcons.Warning, contentDescription = null, tint = colors.warn, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(12.dp))
                        Text(message, color = colors.text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(end = 8.dp))
                    }
                    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.End) {
                        TextButton(onClick = { dismissed = dismissed + message }) { Text("Dismiss") }
                        if (conflict) {
                            TextButton(onClick = {
                                dismissed = dismissed + message
                                shell.nav.navigate("settings/security")
                                // Only the later change stayed: there is nothing to remove, and having looked is enough.
                                if (!notices.bothOpen) shell.launch { runCatching { Pass.passwordLossSeen() } }
                            }) { Text("Go to Unlock methods") }
                        }
                    }
                }
            }
        }
    }
}

package com.wasimaster.wmpass.ui.parts

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.palette

/**
 * A YubiKey as part of a KeePass database's key, as KeePassXC sets one up: its HMAC-SHA1
 * challenge-response slot, 2 unless the key was set up otherwise. Zero is no key.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SecurityKeySlot(slot: Int, modifier: Modifier = Modifier, onChange: (Int) -> Unit) {
    val colors = palette
    if (slot == 0) {
        QuietButton("Use security key", modifier.fillMaxWidth(), PassIcons.SecurityKey, colors.primary, height = 52.dp) { onChange(2) }
        return
    }
    Column(modifier.fillMaxWidth(), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Text("Security key, challenge-response slot", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
            Pill("Slot 1", slot == 1) { onChange(1) }
            Pill("Slot 2", slot == 2) { onChange(2) }
            Pill("No security key", false) { onChange(0) }
        }
        Text(
            "When it is time, hold the key to the back of the phone, or plug it in and touch it.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

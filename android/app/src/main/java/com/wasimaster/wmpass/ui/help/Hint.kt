package com.wasimaster.wmpass.ui.help

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.IconButton
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.palette

/**
 * One line at the top of a page the first time it opens: what the page is for, and a way to read
 * more. Following it or closing it means it does not come back on this phone. Drawn like the
 * settings kit's state banner, in the theme's quiet container, so it reads as a note and not as
 * a warning.
 */
@Composable
fun Hint(prefs: Prefs, name: String, topicId: String, modifier: Modifier = Modifier) {
    val key = "hint:$name"
    if (!prefs.helpHints.value || key in prefs.helpSeen) return
    val topic = Help.topic(topicId) ?: return
    val words = Help.plain(topic.what, prefs).lineSequence().firstOrNull().orEmpty()
    if (words.isEmpty()) return
    val colors = palette
    val flat = colors.flat
    val container = if (flat) MaterialTheme.colorScheme.secondaryContainer else colors.surface2
    val content = if (flat) MaterialTheme.colorScheme.onSecondaryContainer else colors.text
    Row(
        modifier
            .fillMaxWidth()
            .clip(if (flat) RoundedCornerShape(12.dp) else Shapes.cardSmall)
            .background(container)
            .padding(start = 14.dp, top = 6.dp, bottom = 6.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(PassIcons.Info, null, tint = content, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(12.dp))
        Text(words, color = content, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f).padding(vertical = 6.dp))
        TextButton(onClick = {
            prefs.markHelpSeen(key)
            Guide.open(topic.id)
        }) { Text("More") }
        IconButton(onClick = { prefs.markHelpSeen(key) }) {
            Icon(PassIcons.Close, "Do not show this again", tint = content, modifier = Modifier.size(18.dp))
        }
    }
}

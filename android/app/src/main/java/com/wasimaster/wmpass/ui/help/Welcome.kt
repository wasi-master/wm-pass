package com.wasimaster.wmpass.ui.help

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.PassSheet

/**
 * Once, after the first vault opens on this phone: where the everyday things are, and a tour. Both
 * buttons, and pulling the sheet away, mean it is not shown again; Settings, Guide brings it back.
 */
@Composable
fun WelcomeSheet(prefs: Prefs) {
    val colors = palette
    val rows = listOf(
        Triple(PassIcons.Search, "Find anything", "Search sits at the top of the vault, or at the bottom if you put it there."),
        Triple(PassIcons.Add, "Add an item", "The **+** button adds a login, a card, a note, a one-time code and more."),
        Triple(PassIcons.ContentCopy, "Copy without showing it", "Tap an item to see it. Copy buttons put a secret on the clipboard, and it is cleared again after a while."),
        Triple(PassIcons.Help, "Ask what anything does", "Every setting has a **?** beside it, and Settings, Guide has the rest."),
    )
    PassSheet({ Guide.dismissWelcome(prefs) }, "Your vault is ready") {
        Text("A minute on where things are. You can come back to this in Settings, Guide.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.height(16.dp))
        Column(verticalArrangement = Arrangement.spacedBy(16.dp)) {
            rows.forEach { (icon, title, text) -> WelcomeRow(icon, title, text) }
        }
        Spacer(Modifier.height(24.dp))
        PrimaryButton("Show me around", Modifier.fillMaxWidth(), icon = PassIcons.TouchApp) {
            Guide.dismissWelcome(prefs)
            Guide.startTour("tour.home")
        }
        Spacer(Modifier.height(10.dp))
        QuietButton("Not now", Modifier.fillMaxWidth()) { Guide.dismissWelcome(prefs) }
    }
}

@Composable
private fun WelcomeRow(icon: ImageVector, title: String, text: String) {
    val colors = palette
    Row(verticalAlignment = Alignment.Top) {
        Icon(icon, null, tint = colors.textMid, modifier = Modifier.padding(top = 2.dp).size(22.dp))
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge)
            // The same markup as the guide, so **+** and **?** read as the buttons they name.
            Text(annotated(listOf(HelpRun("text", text)).flatMap(::bold), colors, null) {}, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

/** Splits **words** out of a plain run, the one piece of the guide's markup these rows use. */
private fun bold(run: HelpRun): List<HelpRun> {
    val text = run.s.orEmpty()
    val parts = text.split("**")
    return parts.mapIndexedNotNull { i, part -> if (part.isEmpty()) null else HelpRun(if (i % 2 == 1) "strong" else "text", part) }
}

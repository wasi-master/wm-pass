package com.wasimaster.wmpass.ui.help.previews

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.theme.palette

/** Made-up items for the previews. Nothing here comes from a vault. */
internal class SampleRow(
    val title: String,
    val site: String?,
    val kind: String,
    val subtitle: String,
    val favourite: Boolean = false,
    val code: Boolean = false,
    val attachment: Boolean = false,
)

internal val SampleRows = listOf(
    SampleRow("GitHub", "github.com", "login", "sam@example.com", favourite = true, code = true),
    SampleRow("GitHub", "github.com", "login", "sam-work"),
    SampleRow("Netflix", "netflix.com", "login", "sam@example.com"),
    SampleRow("Home Wi-Fi", null, "wifi", "Garden-5G", attachment = true),
)

/** The vault list as this phone draws it, with the one setting being explained set to the choice shown. */
@Composable
internal fun RowPreview(prefs: Prefs, showing: Showing) {
    val colors = palette
    val layout = showing.string("layout", prefs.layout)
    val icons = showing.flag("row_icons", prefs.rowIcons.value)
    val second = showing.flag("row_subtitle", prefs.rowSubtitle.value)
    val marks = showing.flag("row_badges", prefs.rowBadges.value)
    val button = showing.string("row_button", prefs.rowButton.value)
    val fold = showing.flag("group_sites", prefs.groupSites.value)
    val compact = layout == "compact"
    if (layout == "grid") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            SampleRows.chunked(2).forEach { pair ->
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    pair.forEach { row ->
                        Column(Modifier.weight(1f).padding(vertical = 6.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                            if (icons) SiteTile(row.title, row.site, row.kind, size = 40.dp)
                            Spacer(Modifier.size(6.dp))
                            Text(row.title, color = colors.text, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis, textAlign = TextAlign.Center)
                        }
                    }
                }
            }
        }
        PreviewCaption("Sample items.")
        return
    }
    // Folded, the two GitHub logins are one row that opens to show both.
    val rows: List<Pair<SampleRow, Int>> = if (fold) listOf(SampleRows[0] to 2) + SampleRows.drop(2).map { it to 1 } else SampleRows.map { it to 1 }
    Column {
        rows.forEachIndexed { index, (row, count) ->
            if (index > 0) Hairline(inset = if (icons) 54.dp else 0.dp)
            Row(Modifier.fillMaxWidth().padding(vertical = if (compact) 6.dp else 10.dp), verticalAlignment = Alignment.CenterVertically) {
                if (icons) {
                    SiteTile(row.title, row.site, row.kind, size = if (compact) 30.dp else 38.dp)
                    Spacer(Modifier.width(14.dp))
                }
                Column(Modifier.weight(1f)) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            row.title,
                            color = colors.text,
                            style = if (colors.flat) MaterialTheme.typography.bodyLarge else MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                            maxLines = 1,
                        )
                        if (marks && count == 1) {
                            if (row.favourite) Mark(PassIcons.StarFilled, colors.warn)
                            if (row.code) Mark(PassIcons.Pin, colors.textLow)
                            if (row.attachment) Mark(PassIcons.AttachFile, colors.textLow)
                        }
                    }
                    if (!compact && second) {
                        Text(if (count > 1) "$count logins" else row.subtitle, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 1)
                    }
                }
                if (count > 1) {
                    Icon(PassIcons.ExpandMore, null, tint = colors.textMid)
                } else if (canCopy(button, row)) {
                    Icon(PassIcons.ContentCopy, null, tint = colors.textMid, modifier = Modifier.size(20.dp))
                }
            }
        }
    }
    PreviewCaption("Sample items.")
}

private fun canCopy(button: String, row: SampleRow): Boolean = when (button) {
    "password" -> true
    "username" -> row.kind == "login"
    "code" -> row.code
    else -> false
}

@Composable
private fun Mark(icon: androidx.compose.ui.graphics.vector.ImageVector, tint: androidx.compose.ui.graphics.Color) {
    Spacer(Modifier.width(5.dp))
    Icon(icon, null, tint = tint, modifier = Modifier.size(14.dp))
}

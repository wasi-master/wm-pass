package com.wasimaster.wmpass.ui.help.previews

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.LocalSiteIcons
import com.wasimaster.wmpass.ui.parts.SiteIconLook
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.settings.routeColour
import com.wasimaster.wmpass.ui.theme.Look
import com.wasimaster.wmpass.ui.theme.PassTheme
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.AllTabs
import com.wasimaster.wmsuite.settings.LocalWmBareTiles
import com.wasimaster.wmsuite.settings.WmIconTile

/** Sites with a known mark, and items without one, in the icon style and shape shown. */
@Composable
internal fun SiteIconPreview(prefs: Prefs, showing: Showing) {
    val style = showing.string("site_icon_style", prefs.siteIconStyle.value)
    val shape = showing.string("site_icon_shape", prefs.siteIconShape.value)
    val sites = listOf(
        Triple("GitHub", "github.com", "login"),
        Triple("Spotify", "spotify.com", "login"),
        Triple("Mozilla", "mozilla.org", "login"),
        Triple("Corner shop", null, "login"),
        Triple("Visa card", null, "card"),
    )
    CompositionLocalProvider(LocalSiteIcons provides SiteIconLook(style, shape)) {
        Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
            sites.forEach { (title, site, kind) ->
                Column(Modifier.widthIn(max = 60.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    SiteTile(title, site, kind, size = 38.dp)
                    Spacer(Modifier.height(4.dp))
                    Text(title, color = palette.textMid, style = MaterialTheme.typography.labelSmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                }
            }
        }
    }
    PreviewCaption("Sites with a known mark, and items without one.")
}

/** Three settings rows with their tiles in the style shown. */
@Composable
internal fun SettingsIconPreview(prefs: Prefs, showing: Showing) {
    val colors = palette
    val style = showing.string("settings_icons", prefs.settingsIcons.value)
    val rows = listOf(
        Triple("settings/security", "Security", PassIcons.Shield),
        Triple("settings/backup", "Backups", PassIcons.Backup),
        Triple("settings/appearance", "Appearance", PassIcons.Palette),
    )
    Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
        rows.forEach { (route, title, icon) ->
            Row(verticalAlignment = Alignment.CenterVertically) {
                when (style) {
                    "coloured" -> WmIconTile(icon, routeColour(route) ?: colors.primary)
                    "mono" -> WmIconTile(icon, MaterialTheme.colorScheme.primary)
                    "bare" -> CompositionLocalProvider(LocalWmBareTiles provides true) { WmIconTile(icon, colors.primary) }
                }
                if (style != "off") Spacer(Modifier.width(16.dp))
                Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge)
            }
        }
    }
}

/** The app's own glyphs in the style shown. */
@Composable
internal fun AppIconPreview(prefs: Prefs, showing: Showing) {
    val style = showing.string("icon_style", prefs.iconStyle.value)
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.SpaceBetween) {
        PassIcons.sample(style).forEach { Icon(it, null, tint = palette.textMid, modifier = Modifier.size(24.dp)) }
    }
}

/** The vault in this phone's theme, light or dark as shown, without changing the real one. */
@Composable
internal fun DarkPreview(prefs: Prefs, showing: Showing) {
    val chosen = showing.string("look", prefs.lookOpt.value)
    val look = runCatching { Look.valueOf(chosen) }.getOrDefault(Look.SYSTEM)
    PassTheme(look, prefs.accent, prefs.themeId, prefs.uiStyle, prefs.customThemes) {
        val colors = palette
        Column(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(12.dp))
                .background(colors.ground)
                .padding(12.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            Row(
                Modifier.fillMaxWidth().height(40.dp).clip(CircleShape).background(if (colors.flat) MaterialTheme.colorScheme.surfaceContainerHigh else colors.surface2).padding(horizontal = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Icon(PassIcons.Search, null, tint = colors.textMid, modifier = Modifier.size(18.dp))
                Spacer(Modifier.width(10.dp))
                Text("Search vault", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            }
            SampleRows.take(2).forEach { row ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    SiteTile(row.title, row.site, row.kind, size = 32.dp)
                    Spacer(Modifier.width(12.dp))
                    Column {
                        Text(row.title, color = colors.text, style = MaterialTheme.typography.bodyMedium)
                        Text(row.subtitle, color = colors.textMid, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
        }
    }
    PreviewCaption(
        when (look) {
            Look.SYSTEM -> "Your theme, following the phone."
            Look.LIGHT -> "Your theme, light."
            Look.DARK -> "Your theme, dark."
            Look.OBSIDIAN -> "Your theme, with black behind it."
        },
    )
}

/** The navigation bar with the pages and labels shown. */
@Composable
internal fun NavBarPreview(prefs: Prefs, showing: Showing) {
    val colors = palette
    val labels = showing.string("nav_labels", prefs.navLabels.value)
    val tabs = prefs.tabs.mapNotNull { id -> AllTabs.firstOrNull { it.id == id } }
    Row(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(if (colors.flat) MaterialTheme.colorScheme.surfaceContainer else colors.surface1).padding(vertical = 10.dp),
        horizontalArrangement = Arrangement.SpaceEvenly,
    ) {
        tabs.forEachIndexed { index, tab ->
            val selected = index == 0
            Column(horizontalAlignment = Alignment.CenterHorizontally) {
                Box(
                    Modifier
                        .clip(CircleShape)
                        .background(if (selected) colors.primary.copy(alpha = 0.16f) else Color.Transparent)
                        .padding(horizontal = 16.dp, vertical = 4.dp),
                ) {
                    Icon(tabIcon(tab.icon, tab.filled, selected), null, tint = if (selected) colors.primary else colors.textMid, modifier = Modifier.size(22.dp))
                }
                if (labels == "all" || (labels == "selected" && selected)) {
                    Text(tab.label, color = if (selected) colors.text else colors.textMid, style = MaterialTheme.typography.labelSmall, modifier = Modifier.padding(top = 4.dp))
                }
            }
        }
    }
    PreviewCaption("Your pages, in your order. The first is selected here.")
}

private fun tabIcon(outline: ImageVector, filled: ImageVector, selected: Boolean) = if (selected) filled else outline

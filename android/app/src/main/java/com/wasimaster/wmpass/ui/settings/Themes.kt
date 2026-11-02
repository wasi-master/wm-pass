package com.wasimaster.wmpass.ui.settings

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Slider
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.NavRow
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QrCode
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.parts.qrFits
import com.wasimaster.wmpass.ui.theme.BuiltInThemes
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.ThemeCodec
import com.wasimaster.wmpass.ui.theme.ThemeColors
import com.wasimaster.wmpass.ui.theme.ThemeFile
import com.wasimaster.wmpass.ui.theme.contrast
import com.wasimaster.wmpass.ui.theme.flatScheme
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.toPalette
import com.wasimaster.wmpass.ui.theme.toThemeColor
import com.wasimaster.wmpass.ui.theme.toThemeHex
import com.wasimaster.wmpass.ui.vault.ConfirmSheet
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmpass.ui.vault.SheetRow
import com.wasimaster.wmsuite.settings.WmScreen

/** A theme's colours at a glance: its ground with the text and accent on it. */
@Composable
private fun Swatch(colors: ThemeColors?, modifier: Modifier = Modifier) {
    val c = colors ?: return
    Row(
        modifier
            .clip(RoundedCornerShape(10.dp))
            .background(c.background.toThemeColor())
            .border(1.dp, palette.hairline, RoundedCornerShape(10.dp))
            .padding(horizontal = 7.dp, vertical = 9.dp),
        horizontalArrangement = Arrangement.spacedBy(3.dp),
    ) {
        listOf(c.text, c.primary, c.danger, c.info).forEach { hex ->
            Box(Modifier.size(9.dp).clip(CircleShape).background(hex.toThemeColor()))
        }
    }
}

@Composable
private fun ThemeRow(theme: ThemeFile, selected: Boolean, darkFirst: Boolean, onClick: () -> Unit, trailing: (@Composable () -> Unit)? = null) {
    val colors = palette
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(start = 16.dp, end = 8.dp, top = 12.dp, bottom = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Swatch(if (darkFirst) theme.dark ?: theme.light else theme.light ?: theme.dark)
        Spacer(Modifier.width(16.dp))
        Column(Modifier.weight(1f)) {
            Text(theme.name, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                when {
                    theme.light != null && theme.dark != null -> "Light and dark"
                    theme.dark != null -> "Dark only"
                    else -> "Light only"
                },
                color = colors.textMid,
                style = MaterialTheme.typography.bodySmall,
            )
        }
        if (selected) Icon(PassIcons.Check, "In use", tint = colors.primary, modifier = Modifier.padding(horizontal = 12.dp))
        trailing?.invoke()
    }
}

/** Every theme, built in and made, and the ways to bring one in or pass one on. */
@Composable
fun ThemesScreen() {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val prefs = shell.prefs
    var importing by remember { mutableStateOf(false) }
    var sharing by remember { mutableStateOf<ThemeFile?>(null) }
    var acting by remember { mutableStateOf<ThemeFile?>(null) }
    var deleting by remember { mutableStateOf<ThemeFile?>(null) }
    val dark = colors.dark
    WmScreen("Theme", route = "settings/themes", onBack = { shell.nav.popBackStack() }) {
        Column(Modifier.padding(horizontal = 16.dp)) {
            if (android.os.Build.VERSION.SDK_INT >= android.os.Build.VERSION_CODES.S) {
                GroupCard {
                    Row(
                        Modifier.fillMaxWidth().clickable { prefs.updateTheme(BuiltInThemes.SYSTEM) }.padding(horizontal = 16.dp, vertical = 14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(Modifier.weight(1f)) {
                            Text("Device colours", color = colors.text, style = MaterialTheme.typography.bodyLarge)
                            Text("Follows your wallpaper", color = colors.textMid, style = MaterialTheme.typography.bodySmall)
                        }
                        if (prefs.themeId == BuiltInThemes.SYSTEM) Icon(PassIcons.Check, "In use", tint = colors.primary)
                    }
                }
            }
            if (prefs.customThemes.isNotEmpty()) {
                Eyebrow("Yours")
                GroupCard {
                    prefs.customThemes.forEach { theme ->
                        ThemeRow(theme, prefs.themeId == theme.id, dark, { prefs.updateTheme(theme.id) }) {
                            RowIconButton(PassIcons.MoreVert, "More for ${theme.name}") { acting = theme }
                        }
                    }
                }
            }
            Eyebrow("Themes")
            GroupCard {
                BuiltInThemes.all.forEach { theme ->
                    ThemeRow(theme, prefs.themeId == theme.id, dark, { prefs.updateTheme(theme.id) }) {
                        RowIconButton(PassIcons.MoreVert, "More for ${theme.name}") { acting = theme }
                    }
                }
            }
            Spacer(Modifier.height(20.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                QuietButton("Import", Modifier.weight(1f), PassIcons.Download, height = 48.dp) { importing = true }
                PrimaryButton("New theme", Modifier.weight(1f), icon = PassIcons.Add) { shell.nav.navigate("settings/theme_edit/new") }
            }
            Spacer(Modifier.height(32.dp))
        }
    }
    acting?.let { theme ->
        val mine = prefs.customThemes.any { it.id == theme.id }
        PassSheet({ acting = null }, theme.name) {
            if (mine) {
                SheetRow(PassIcons.Edit, "Edit") {
                    acting = null
                    shell.nav.navigate("settings/theme_edit/${theme.id}")
                }
            } else {
                SheetRow(PassIcons.Layers, "Customise a copy") {
                    acting = null
                    shell.nav.navigate("settings/theme_edit/${theme.id}")
                }
            }
            SheetRow(PassIcons.Upload, "Share") {
                acting = null
                sharing = theme
            }
            if (mine) {
                SheetRow(PassIcons.Delete, "Delete", tint = colors.danger) {
                    acting = null
                    deleting = theme
                }
            }
        }
    }
    sharing?.let { theme -> ShareThemeSheet(theme) { sharing = null } }
    deleting?.let { theme ->
        ConfirmSheet("Delete ${theme.name}?", "This removes the theme from this device.", "Delete", onDismiss = { deleting = null }) {
            prefs.deleteCustomTheme(theme.id)
        }
    }
    if (importing) ImportThemeSheet { importing = false }
}

@Composable
private fun ShareThemeSheet(theme: ThemeFile, onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val code = remember(theme) { ThemeCodec.toCode(theme) }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/json")) { uri ->
        if (uri != null) context.contentResolver.openOutputStream(uri)?.use { it.write(ThemeCodec.toText(theme).toByteArray()) }
    }
    PassSheet(onDismiss, "Share ${theme.name}") {
        Text("A theme holds colours and a few settings. Nothing from your vault is in it.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        Spacer(Modifier.height(14.dp))
        if (qrFits(code)) {
            QrCode(code, Modifier.padding(horizontal = 40.dp))
            Spacer(Modifier.height(14.dp))
        }
        SheetRow(PassIcons.Upload, "Send to an app") {
            val intent = Intent(Intent.ACTION_SEND).setType("text/plain").putExtra(Intent.EXTRA_TEXT, code)
            context.startActivity(Intent.createChooser(intent, "Share theme"))
        }
        SheetRow(PassIcons.ContentCopy, "Copy") { shell.copyPlain("Theme", code) }
        SheetRow(PassIcons.Download, "Save as a file") { save.launch(theme.name.replace(Regex("[^A-Za-z0-9]+"), "-").lowercase() + ".wmtheme") }
    }
}

@Composable
private fun ImportThemeSheet(onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val text = rememberTextFieldState()
    var problem by remember { mutableStateOf<String?>(null) }
    fun take(raw: String) {
        try {
            val theme = ThemeCodec.read(raw)
            shell.prefs.saveCustomTheme(theme)
            shell.prefs.updateTheme(theme.id)
            onDismiss()
        } catch (failure: IllegalArgumentException) {
            problem = failure.message
        }
    }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            val raw = context.contentResolver.openInputStream(uri)?.use { it.readBytes().decodeToString() }
            if (raw != null) take(raw)
        }
    }
    PassSheet(onDismiss, "Import a theme") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            SheetField(text, "Paste a theme code")
            problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodySmall) }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                QuietButton("Scan", Modifier.weight(1f), PassIcons.QrCodeScanner) {
                    shell.scanTarget = { take(it) }
                    shell.nav.navigate("scan")
                }
                QuietButton("File", Modifier.weight(1f), PassIcons.Folder) { pick.launch(arrayOf("*/*")) }
            }
            PrimaryButton("Import", Modifier.fillMaxWidth(), enabled = text.text.isNotBlank()) { take(text.text.toString()) }
        }
    }
}

private data class ColourSlot(val label: String, val get: (ThemeColors) -> String, val set: (ThemeColors, String) -> ThemeColors)

private val Slots = listOf(
    ColourSlot("Accent", { it.primary }, { c, v -> c.copy(primary = v, primaryContainer = "", secondaryContainer = "", onPrimary = "") }),
    ColourSlot("Background", { it.background }, { c, v -> c.copy(background = v) }),
    ColourSlot("Surface", { it.surface }, { c, v -> c.copy(surface = v) }),
    ColourSlot("Raised surface", { it.surfaceHigh }, { c, v -> c.copy(surfaceHigh = v) }),
    ColourSlot("Highest surface", { it.surfaceHighest }, { c, v -> c.copy(surfaceHighest = v) }),
    ColourSlot("Text", { it.text }, { c, v -> c.copy(text = v) }),
    ColourSlot("Secondary text", { it.textMid }, { c, v -> c.copy(textMid = v) }),
    ColourSlot("Faint text", { it.textLow }, { c, v -> c.copy(textLow = v) }),
    ColourSlot("Danger", { it.danger }, { c, v -> c.copy(danger = v) }),
    ColourSlot("Warning", { it.warn }, { c, v -> c.copy(warn = v) }),
    ColourSlot("Information", { it.info }, { c, v -> c.copy(info = v) }),
)

/** A small piece of the app in the colours being edited, so a change shows at once. */
@Composable
private fun ThemePreview(colors: ThemeColors, dark: Boolean, corners: Int) {
    val p = colors.toPalette(dark, black = false, flat = true)
    val scheme = p.flatScheme(colors)
    MaterialTheme(colorScheme = scheme, typography = MaterialTheme.typography) {
        Column(
            Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(corners.dp))
                .background(p.ground)
                .border(1.dp, palette.hairline, RoundedCornerShape(corners.dp))
                .padding(16.dp),
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(40.dp).clip(CircleShape).background(scheme.secondaryContainer), contentAlignment = Alignment.Center) {
                    Text("G", color = scheme.onSecondaryContainer, style = MaterialTheme.typography.titleMedium)
                }
                Spacer(Modifier.width(16.dp))
                Column(Modifier.weight(1f)) {
                    Text("GitHub", color = p.text, style = MaterialTheme.typography.bodyLarge)
                    Text("name@example.com", color = p.textMid, style = MaterialTheme.typography.bodyMedium)
                }
                Text("3 days", color = p.textLow, style = MaterialTheme.typography.bodySmall)
            }
            Spacer(Modifier.height(12.dp))
            Text(
                com.wasimaster.wmpass.ui.parts.colouredSecret("hT7#kQ2!mZ", p),
                style = PassType.secretSmall,
            )
            Spacer(Modifier.height(12.dp))
            Row(verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.clip(RoundedCornerShape(50)).background(p.primary).padding(horizontal = 16.dp, vertical = 8.dp)) {
                    Text("Copy", color = p.onPrimary, style = MaterialTheme.typography.labelLarge)
                }
                Spacer(Modifier.width(12.dp))
                Text("Weak", color = p.warn, style = MaterialTheme.typography.labelLarge)
                Spacer(Modifier.width(12.dp))
                Text("Breached", color = p.danger, style = MaterialTheme.typography.labelLarge)
            }
        }
    }
}

@Composable
private fun ColourSheet(label: String, value: String, ground: String, onDismiss: () -> Unit, onChange: (String) -> Unit) {
    val colors = palette
    val start = remember { android.graphics.Color.parseColor(value).let { c -> FloatArray(3).also { android.graphics.Color.colorToHSV(c, it) } } }
    var hue by remember { mutableFloatStateOf(start[0]) }
    var sat by remember { mutableFloatStateOf(start[1]) }
    var bright by remember { mutableFloatStateOf(start[2]) }
    val hex = rememberTextFieldState(value)
    fun push() {
        val made = Color(android.graphics.Color.HSVToColor(floatArrayOf(hue, sat, bright))).toThemeHex()
        hex.setTextAndPlaceCursorAtEnd(made)
        onChange(made)
    }
    LaunchedEffect(hex.text) {
        val typed = hex.text.toString().trim()
        if (Regex("^#[0-9a-fA-F]{6}$").matches(typed) && typed != value) {
            onChange(typed.lowercase())
            val out = FloatArray(3)
            android.graphics.Color.colorToHSV(android.graphics.Color.parseColor(typed), out)
            hue = out[0]
            sat = out[1]
            bright = out[2]
        }
    }
    PassSheet(onDismiss, label) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.size(52.dp).clip(RoundedCornerShape(12.dp)).background(value.toThemeColor()).border(1.dp, colors.hairline, RoundedCornerShape(12.dp)))
            Spacer(Modifier.width(14.dp))
            SheetField(hex, "#RRGGBB", Modifier.weight(1f))
        }
        Spacer(Modifier.height(8.dp))
        val ratio = contrast(value.toThemeColor(), ground.toThemeColor())
        Text(
            "Contrast against the background: %.1f to 1".format(ratio),
            color = colors.textMid,
            style = MaterialTheme.typography.bodySmall,
        )
        Spacer(Modifier.height(10.dp))
        Text("Hue", color = colors.textMid, style = MaterialTheme.typography.labelMedium)
        Slider(hue, { hue = it; push() }, valueRange = 0f..360f)
        Text("Saturation", color = colors.textMid, style = MaterialTheme.typography.labelMedium)
        Slider(sat, { sat = it; push() })
        Text("Brightness", color = colors.textMid, style = MaterialTheme.typography.labelMedium)
        Slider(bright, { bright = it; push() })
    }
}

/** Starts from any theme. Saving a built-in one makes a copy, so the original stays as it was. */
@Composable
fun ThemeEditor(id: String) {
    val colors = palette
    val shell = LocalShell.current
    val prefs = shell.prefs
    val source = remember(id) { (BuiltInThemes.all + prefs.customThemes).firstOrNull { it.id == id } }
    val isMine = remember(id) { prefs.customThemes.any { it.id == id } }
    val base = source ?: BuiltInThemes.all.first()
    val name = rememberTextFieldState(if (isMine) base.name else if (source == null) "My theme" else "${base.name} copy")
    var light by remember { mutableStateOf(base.light) }
    var dark by remember { mutableStateOf(base.dark) }
    var editingDark by remember { mutableStateOf(if (colors.dark) base.dark != null else base.light == null) }
    var corners by remember { mutableIntStateOf(base.corners) }
    var font by remember { mutableStateOf(base.font) }
    var slot by remember { mutableStateOf<ColourSlot?>(null) }
    val current = if (editingDark) dark else light
    fun update(next: ThemeColors) {
        if (editingDark) dark = next else light = next
    }
    WmScreen(if (isMine) "Edit theme" else "New theme", onBack = { shell.nav.popBackStack() }) {
        Column(Modifier.padding(horizontal = 16.dp)) {
            SheetField(name, "Name")
            Spacer(Modifier.height(12.dp))
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                Pill("Light", !editingDark) {
                    if (light == null) light = dark
                    editingDark = false
                }
                Pill("Dark", editingDark) {
                    if (dark == null) dark = light
                    editingDark = true
                }
            }
            Spacer(Modifier.height(12.dp))
            current?.let { ThemePreview(it, editingDark, corners) }
            Eyebrow("Colours")
            GroupCard {
                Slots.forEach { item ->
                    val hex = current?.let(item.get).orEmpty()
                    Row(
                        Modifier.fillMaxWidth().clickable { slot = item }.padding(horizontal = 16.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(item.label, color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                        Text(hex.uppercase(), color = colors.textMid, style = PassType.secretSmall)
                        Spacer(Modifier.width(12.dp))
                        if (hex.isNotEmpty()) {
                            Box(Modifier.size(28.dp).clip(CircleShape).background(hex.toThemeColor()).border(1.dp, colors.hairline, CircleShape))
                        }
                    }
                }
            }
            Eyebrow("Corners")
            Row(verticalAlignment = Alignment.CenterVertically) {
                Slider(corners.toFloat(), { corners = it.toInt() }, valueRange = 0f..28f, modifier = Modifier.weight(1f))
                Spacer(Modifier.width(12.dp))
                Text("$corners dp", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            }
            Eyebrow("Font")
            Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf("system" to "System", "inter" to "Inter", "serif" to "Serif", "mono" to "Mono").forEach { (key, label) ->
                    Pill(label, font == key) { font = key }
                }
            }
            Spacer(Modifier.height(24.dp))
            PrimaryButton("Save and use", Modifier.fillMaxWidth(), enabled = name.text.isNotBlank()) {
                val saved = ThemeFile(
                    id = if (isMine) base.id else "custom_" + java.util.UUID.randomUUID().toString().take(8),
                    name = name.text.toString().trim().take(40),
                    light = light,
                    dark = dark,
                    corners = corners,
                    font = font,
                )
                prefs.saveCustomTheme(saved)
                prefs.updateTheme(saved.id)
                shell.nav.popBackStack()
            }
            Spacer(Modifier.height(32.dp))
        }
    }
    slot?.let { item ->
        val now = current ?: return@let
        ColourSheet(item.label, item.get(now), now.background, { slot = null }) { update(item.set(now, it)) }
    }
}

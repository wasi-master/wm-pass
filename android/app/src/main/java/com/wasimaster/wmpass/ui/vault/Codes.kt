package com.wasimaster.wmpass.ui.vault

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.lazy.rememberLazyListState
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.OtpRow
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.help.HelpAction
import com.wasimaster.wmpass.ui.help.Hint
import com.wasimaster.wmpass.ui.help.helpAnchor
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.InkState
import com.wasimaster.wmpass.ui.parts.LifeRing
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.RollingCode
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.parts.inkSurface
import com.wasimaster.wmpass.ui.parts.tapAt
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.arrive
import com.wasimaster.wmpass.ui.theme.palette
import kotlinx.coroutines.delay
import kotlinx.serialization.json.put

/** The authenticator. Codes are large enough to read off the phone while typing on a laptop. */
@Composable
fun CodesTab() {
    val colors = palette
    val shell = LocalShell.current
    val revision by Pass.revision.collectAsStateWithLifecycle()
    var rows by remember { mutableStateOf<List<OtpRow>?>(null) }
    LaunchedEffect(revision) {
        while (true) {
            rows = runCatching { Pass.codes() }.getOrDefault(emptyList())
            delay(1000 - System.currentTimeMillis() % 1000 + 10)
        }
    }
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val list = rows?.let { all -> if (shell.prefs.codeSort.value == "name") all.sortedBy { it.otp.issuer.ifEmpty { it.title }.lowercase() } else all }
    LazyColumn(
        state = rememberLazyListState(),
        modifier = Modifier.fillMaxSize(),
        contentPadding = if (colors.flat) {
            PaddingValues(bottom = 96.dp)
        } else {
            PaddingValues(top = top + 8.dp, bottom = 190.dp, start = 16.dp, end = 16.dp)
        },
        verticalArrangement = Arrangement.spacedBy(if (colors.flat) 0.dp else 10.dp),
    ) {
        if (!colors.flat) {
            item("title") {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Text(
                        "Codes",
                        color = colors.text,
                        style = MaterialTheme.typography.headlineLarge.copy(fontSize = 32.sp),
                        modifier = Modifier.weight(1f).padding(start = 6.dp, top = 12.dp, bottom = 10.dp),
                    )
                    HelpAction("screen.codes", "Codes", tint = colors.text)
                }
            }
        }
        item("hint") {
            Hint(shell.prefs, "codes", "screen.codes", Modifier.padding(horizontal = if (colors.flat) 16.dp else 0.dp, vertical = 4.dp))
        }
        if (list != null && list.isEmpty()) {
            item("empty") {
                Column(Modifier.fillMaxWidth().padding(top = 8.dp, start = if (colors.flat) 16.dp else 0.dp, end = if (colors.flat) 16.dp else 0.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Text(
                        "No codes yet. When a site offers two-factor sign-in, scan its QR code or enter its setup key here.",
                        color = colors.textMid,
                        style = MaterialTheme.typography.bodyLarge,
                        modifier = Modifier.padding(start = 6.dp, end = 12.dp, bottom = 10.dp),
                    )
                    PrimaryButton("Add a code", Modifier.fillMaxWidth(), icon = PassIcons.QrCodeScanner) { shell.nav.navigate("new/otp") }
                    QuietButton("Bring them from another app", Modifier.fillMaxWidth(), PassIcons.Download) { shell.nav.navigate("settings/import") }
                }
            }
        }
        if (list != null) {
            itemsIndexed(list, key = { _, row -> row.id }) { index, row ->
                val placed = Modifier.animateItem().arrive(index)
                // The first code stands for the list when a tour points at it.
                CodeCard(row, if (index == 0) placed.helpAnchor("codes.list") else placed)
            }
        }
    }
}

@Composable
private fun CodeCard(row: OtpRow, modifier: Modifier = Modifier) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val ink = remember { InkState(scope) }
    var hotp by remember { mutableStateOf<String?>(null) }
    val code = row.code ?: hotp
    val prefs = shell.prefs
    // Hidden codes show for a short while after a tap, then go again.
    var shownAt by remember { mutableStateOf(0L) }
    val hidden = prefs.codeHidden.value && shownAt == 0L
    LaunchedEffect(shownAt) {
        if (shownAt != 0L) {
            delay(15_000)
            shownAt = 0L
        }
    }
    // Right copies the code and left opens the editor, as rows in the vault list do.
    SwipeRow(
        modifier,
        if (colors.flat) androidx.compose.ui.graphics.RectangleShape else Shapes.card,
        onCopy = if (code != null) ({ shell.copySecret("Code") { Pass.code(row.id) } }) else null,
        onTrash = { shell.nav.navigate("edit/${row.id}") },
        rightIcon = PassIcons.ContentCopy,
        leftIcon = PassIcons.Edit,
    ) {
    Row(
        Modifier
            .fillMaxWidth()
            .then(if (colors.flat) Modifier.background(colors.ground) else Modifier.clip(Shapes.card).background(colors.surface1).rim(Shapes.card, colors))
            .inkSurface(ink, colors.primary)
            .tapAt(onLongPress = { shell.open(row.id) }) { at ->
                if (code != null) {
                    ink.splash(at)
                    shownAt = System.currentTimeMillis()
                    shell.copySecret("Code") { Pass.code(row.id) }
                }
            }
            .padding(start = 16.dp, end = 18.dp, top = 15.dp, bottom = 15.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(Modifier.padding(end = 14.dp)) {
            SiteTile(row.otp.issuer.ifEmpty { row.title }, row.site, "otp", size = if (colors.flat) 40.dp else 46.dp, icon = row.icon)
        }
        Column(Modifier.weight(1f)) {
            val account = row.otp.account
            Text(
                row.otp.issuer.ifEmpty { row.title }.ifEmpty { "Code" },
                color = colors.text,
                style = if (colors.flat) MaterialTheme.typography.bodyMedium else MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold),
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
            )
            if (account.isNotEmpty()) {
                Text(account, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.height(4.dp))
            if (code != null && hidden) {
                Text("••• •••", color = colors.textLow, style = PassType.code)
            } else if (code != null) {
                RollingCode(code)
            } else {
                QuietButton("Show the next code", height = 40.dp, tint = colors.primary) {
                    shell.launch {
                        val reply = Pass.change("otp.hotp_next") { put("id", row.id) }
                        hotp = reply.secret?.consumeText()
                    }
                }
            }
            AnimatedVisibility(row.nextCode != null && prefs.codeNext.value && !hidden) {
                Text("Next  ${grouped(row.nextCode.orEmpty())}", color = colors.textMid, style = PassType.secretSmall)
            }
        }
        if (row.otp.period != null && prefs.codeRing.value) {
            Spacer(Modifier.width(12.dp))
            LifeRing(row.otp.period!!, size = if (colors.flat) 26.dp else 34.dp, stroke = if (colors.flat) 3.dp else 3.5.dp)
        }
    }
    }
}

/** A code in the same groups as the big one above it. */
fun grouped(code: String): String {
    val group = if (code.length % 3 == 0) 3 else code.length / 2
    return if (group == 0) code else code.chunked(group).joinToString(" ")
}

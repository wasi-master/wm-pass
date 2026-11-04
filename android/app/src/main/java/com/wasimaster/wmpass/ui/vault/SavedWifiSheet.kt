package com.wasimaster.wmpass.ui.vault

import android.content.Intent
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.SavedWifi
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.palette
import kotlinx.coroutines.launch

private fun securityName(security: String): String = when (security) {
    "sae" -> "WPA3"
    "wpa" -> "WPA"
    "wep" -> "WEP"
    "eap" -> "Enterprise"
    else -> "Open"
}

/** How an add-all run went, for the line the sheet ends on. */
class WifiAdded(val added: Int, val updated: Int, val leftAlone: Int, val failed: Int = 0)

/** What happened to one network in an add-all run. A [failed] one says why in [text]. */
private class WifiOutcome(val text: String, val failed: Boolean)

/**
 * The networks this phone has saved, read through Shizuku. With [onPick] a tap fills one item;
 * with [onAdd] several are ticked and saved at once. A network already in the vault says so and
 * starts unticked; ticking it updates that item instead of adding a copy. The sheet stays open while
 * they are saved, counting, and ends on what it did to each network by name: added, updated, left
 * as it was, or not saved and why. Passwords are never shown here.
 */
@Composable
fun SavedWifiSheet(
    onDismiss: () -> Unit,
    onPick: ((SavedWifi.Network) -> Unit)? = null,
    /** Saves one network: a new item when [existing] is null, else an update of that item. */
    onAdd: (suspend (network: SavedWifi.Network, existing: String?) -> Unit)? = null,
    onFinished: ((WifiAdded) -> Unit)? = null,
) {
    val colors = palette
    val context = LocalContext.current
    val links = LocalUriHandler.current
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf<SavedWifi.State?>(null) }
    var networks by remember { mutableStateOf<List<SavedWifi.Network>?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var tries by remember { mutableStateOf(0) }
    // Shizuku can take a few seconds to wake, to show its prompt, and to hand the list over.
    var asking by remember { mutableStateOf(false) }
    val entries by Pass.entries.collectAsStateWithLifecycle()
    val chosen = remember { mutableStateListOf<String>() }
    // A network's item in the vault, by name, when it has one already.
    val inVault = remember(entries) {
        entries.filter { it.kind == "wifi" && !it.trashed }.flatMap { entry -> listOf(entry.title to entry.id, entry.subtitle to entry.id) }
            .filter { it.first.isNotEmpty() }.toMap()
    }
    var saving by remember { mutableStateOf<Pair<Int, Int>?>(null) }
    var done by remember { mutableStateOf<WifiAdded?>(null) }
    val outcomes = remember { mutableStateMapOf<String, WifiOutcome>() }
    LaunchedEffect(tries) {
        problem = null
        val now = SavedWifi.state(context)
        state = now
        if (now == SavedWifi.State.READY) {
            runCatching { SavedWifi.list() }
                .onSuccess { found ->
                    networks = found
                    chosen.clear()
                    chosen.addAll(found.map { it.ssid }.filter { it !in inVault })
                }
                .onFailure { problem = "Android did not hand over the saved networks: ${it.cause?.message ?: it.message}" }
        }
    }
    PassSheet(onDismiss, "Saved on this phone") {
        val explain: @Composable (String) -> Unit = { text ->
            Text(text, color = colors.textMid, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 4.dp, end = 4.dp, bottom = 14.dp))
        }
        val waiting = state == null || asking || (state == SavedWifi.State.READY && networks == null && problem == null)
        androidx.compose.animation.AnimatedVisibility(waiting) {
            androidx.compose.material3.LinearProgressIndicator(
                color = colors.primary,
                trackColor = colors.surface3,
                modifier = Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, bottom = 14.dp).clip(androidx.compose.foundation.shape.CircleShape),
            )
        }
        when (state) {
            null -> explain("Looking for Shizuku…")
            SavedWifi.State.MISSING -> {
                explain(
                    "Android keeps saved Wi-Fi passwords from apps. Shizuku, a free app, can pass WM Pass the same access a computer has over USB debugging, for as long as you allow it. Install it, start it, then come back.",
                )
                PrimaryButton("Get Shizuku", Modifier.fillMaxWidth(), icon = PassIcons.OpenInNew) { runCatching { links.openUri("https://shizuku.rikka.app/download/") } }
                Spacer(Modifier.height(10.dp))
                QuietButton("Try again", Modifier.fillMaxWidth()) { tries++ }
            }
            SavedWifi.State.STOPPED -> {
                explain("Shizuku is installed but not running. Start it in the Shizuku app, then try again.")
                PrimaryButton("Open Shizuku", Modifier.fillMaxWidth(), icon = PassIcons.OpenInNew) {
                    context.packageManager.getLaunchIntentForPackage(SavedWifi.SHIZUKU_PACKAGE)?.let { runCatching { context.startActivity(it.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)) } }
                }
                Spacer(Modifier.height(10.dp))
                QuietButton("Try again", Modifier.fillMaxWidth()) { tries++ }
            }
            SavedWifi.State.ASK -> {
                explain("Shizuku asks once whether WM Pass may use it. WM Pass uses it only to read saved networks, and only when you open this.")
                PrimaryButton(if (asking) "Waiting for Shizuku…" else "Allow in Shizuku", Modifier.fillMaxWidth(), enabled = !asking) {
                    asking = true
                    scope.launch {
                        try {
                            if (SavedWifi.ask()) tries++ else problem = "Shizuku said no. You can allow WM Pass later in the Shizuku app's list."
                        } finally {
                            asking = false
                        }
                    }
                }
            }
            SavedWifi.State.REFUSED -> {
                explain("WM Pass was refused in Shizuku before. Allow it in the Shizuku app's list of apps, then try again.")
                QuietButton("Try again", Modifier.fillMaxWidth()) { tries++ }
            }
            SavedWifi.State.UNSUPPORTED -> explain("Shizuku on this phone cannot read saved passwords. On Android 10 it has to run as root, and it needs version 11 or newer.")
            SavedWifi.State.READY -> {
                val found = networks
                when {
                    problem != null -> Unit
                    found == null -> explain("Reading saved networks…")
                    found.isEmpty() -> explain("This phone has no saved networks.")
                    done != null -> {
                        val result = done!!
                        val lines = listOfNotNull(
                            when (result.added) {
                                0 -> null
                                1 -> "Added 1 network."
                                else -> "Added ${result.added} networks."
                            },
                            when (result.updated) {
                                0 -> null
                                1 -> "Updated 1 that was already in your vault."
                                else -> "Updated ${result.updated} that were already in your vault."
                            },
                            when (result.failed) {
                                0 -> null
                                1 -> "1 could not be saved."
                                else -> "${result.failed} could not be saved."
                            },
                        )
                        explain(lines.joinToString(" ").ifEmpty { "Nothing was added." })
                        found.forEach { network ->
                            outcomes[network.ssid]?.let { ResultRow(network.ssid, it) }
                        }
                        Spacer(Modifier.height(12.dp))
                        PrimaryButton("Done", Modifier.fillMaxWidth()) {
                            onFinished?.invoke(result)
                            onDismiss()
                        }
                    }
                    onAdd != null -> {
                        val progress = saving
                        if (progress != null) {
                            explain("Saving ${progress.first + 1} of ${progress.second}…")
                            androidx.compose.material3.LinearProgressIndicator(
                                progress = { progress.first / progress.second.toFloat() },
                                color = colors.primary,
                                trackColor = colors.surface3,
                                modifier = Modifier.fillMaxWidth().padding(start = 4.dp, end = 4.dp, bottom = 14.dp).clip(androidx.compose.foundation.shape.CircleShape),
                            )
                        } else {
                            val already = found.count { it.ssid in inVault }
                            explain(
                                if (already == 0) {
                                    "Ticked ones become Wi-Fi items."
                                } else {
                                    "Ticked ones become Wi-Fi items. The ones marked In your vault start unticked; tick one to update its item instead of adding a copy."
                                },
                            )
                            found.forEach { network ->
                                val on = network.ssid in chosen
                                val sub = securityName(network.security) + if (network.ssid in inVault) ", in your vault" else ""
                                SheetRow(PassIcons.Wifi, network.ssid, sub, selected = on) {
                                    if (!chosen.remove(network.ssid)) chosen.add(network.ssid)
                                }
                            }
                            Spacer(Modifier.height(12.dp))
                            val picked = found.filter { it.ssid in chosen }
                            val updates = picked.count { it.ssid in inVault }
                            val adds = picked.size - updates
                            val label = when {
                                picked.isEmpty() -> "Add networks"
                                updates == 0 -> if (adds == 1) "Add 1 network" else "Add $adds networks"
                                adds == 0 -> if (updates == 1) "Update 1 network" else "Update $updates networks"
                                else -> "Add $adds, update $updates"
                            }
                            PrimaryButton(label, Modifier.fillMaxWidth(), enabled = picked.isNotEmpty()) {
                                scope.launch {
                                    var added = 0
                                    var updated = 0
                                    var failed = 0
                                    outcomes.clear()
                                    picked.forEachIndexed { index, network ->
                                        saving = index to picked.size
                                        val existing = inVault[network.ssid]
                                        runCatching { onAdd(network, existing) }
                                            .onSuccess {
                                                if (existing == null) added++ else updated++
                                                outcomes[network.ssid] = WifiOutcome(if (existing == null) "Added" else "Updated its item", false)
                                            }
                                            .onFailure {
                                                failed++
                                                outcomes[network.ssid] = WifiOutcome("Not saved: ${it.message ?: "no reason was given"}", true)
                                            }
                                    }
                                    val leftAlone = found.filter { it.ssid in inVault && it.ssid !in chosen }
                                    leftAlone.forEach { outcomes[it.ssid] = WifiOutcome("Already in your vault, left as it was", false) }
                                    saving = null
                                    done = WifiAdded(added, updated, leftAlone.size, failed)
                                }
                            }
                        }
                    }
                    else -> found.forEach { network ->
                        SheetRow(PassIcons.Wifi, network.ssid, securityName(network.security)) {
                            onPick?.invoke(network)
                            onDismiss()
                        }
                    }
                }
            }
        }
        problem?.let {
            Spacer(Modifier.height(10.dp))
            Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 4.dp))
        }
    }
}

/** A network after an add-all run, with what happened to it. One that failed is in red. */
@Composable
private fun ResultRow(ssid: String, outcome: WifiOutcome) {
    val colors = palette
    Row(Modifier.fillMaxWidth().padding(vertical = 10.dp, horizontal = 4.dp), verticalAlignment = Alignment.CenterVertically) {
        GlyphTile(PassIcons.Wifi, if (outcome.failed) colors.danger else colors.primary, size = 42.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(ssid, color = colors.text, style = MaterialTheme.typography.bodyLarge)
            Text(outcome.text, color = if (outcome.failed) colors.danger else colors.textMid, style = MaterialTheme.typography.bodyMedium)
        }
    }
}

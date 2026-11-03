package com.wasimaster.wmpass.ui.vault

import android.content.Intent
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.SavedWifi
import com.wasimaster.wmpass.ui.icons.PassIcons
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

/**
 * The networks this phone has saved, read through Shizuku. With [onPick] a tap fills one item;
 * with [onAddAll] several are ticked and saved at once, the ones already in the vault left unticked.
 * Passwords are never shown here.
 */
@Composable
fun SavedWifiSheet(
    onDismiss: () -> Unit,
    onPick: ((SavedWifi.Network) -> Unit)? = null,
    onAddAll: ((List<SavedWifi.Network>) -> Unit)? = null,
) {
    val colors = palette
    val context = LocalContext.current
    val links = LocalUriHandler.current
    val scope = rememberCoroutineScope()
    var state by remember { mutableStateOf<SavedWifi.State?>(null) }
    var networks by remember { mutableStateOf<List<SavedWifi.Network>?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var tries by remember { mutableStateOf(0) }
    val entries by Pass.entries.collectAsStateWithLifecycle()
    val chosen = remember { mutableStateListOf<String>() }
    LaunchedEffect(tries) {
        problem = null
        val now = SavedWifi.state(context)
        state = now
        if (now == SavedWifi.State.READY) {
            runCatching { SavedWifi.list() }
                .onSuccess { found ->
                    networks = found
                    val saved = entries.filter { it.kind == "wifi" }.flatMap { listOf(it.title, it.subtitle) }.toSet()
                    chosen.clear()
                    chosen.addAll(found.map { it.ssid }.filter { it !in saved })
                }
                .onFailure { problem = "Android did not hand over the saved networks: ${it.cause?.message ?: it.message}" }
        }
    }
    PassSheet(onDismiss, "Saved on this phone") {
        val explain: @Composable (String) -> Unit = { text ->
            Text(text, color = colors.textMid, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(start = 4.dp, end = 4.dp, bottom = 14.dp))
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
                PrimaryButton("Allow in Shizuku", Modifier.fillMaxWidth()) {
                    scope.launch {
                        if (SavedWifi.ask()) tries++ else problem = "Shizuku said no. You can allow WM Pass later in the Shizuku app's list."
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
                    onAddAll != null -> {
                        explain("Ticked ones become Wi-Fi items. Those already in your vault start unticked.")
                        found.forEach { network ->
                            val on = network.ssid in chosen
                            SheetRow(PassIcons.Wifi, network.ssid, securityName(network.security), selected = on) {
                                if (!chosen.remove(network.ssid)) chosen.add(network.ssid)
                            }
                        }
                        Spacer(Modifier.height(12.dp))
                        val picked = found.filter { it.ssid in chosen }
                        PrimaryButton(if (picked.size == 1) "Add 1 network" else "Add ${picked.size} networks", Modifier.fillMaxWidth(), enabled = picked.isNotEmpty()) {
                            onAddAll(picked)
                            onDismiss()
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

package com.wasimaster.wmpass.ui.vault

import android.app.Activity
import android.content.Intent
import android.net.wifi.WifiNetworkSuggestion
import android.os.Build
import android.provider.Settings
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.annotation.RequiresApi
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.core.Detail
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.parts.QrCode
import com.wasimaster.wmpass.ui.theme.palette

/** What a Wi-Fi item's page can do with its network, beside copying the password. */
internal class WifiActions(val canShow: Boolean, val canAdd: Boolean, val show: () -> Unit, val add: () -> Unit)

private class WifiNetwork(val ssid: String, val security: String, val password: String, val hidden: Boolean)

/**
 * The code Android's own Wi-Fi settings show to share a network, which a camera app reads to join
 * it. Enterprise networks have no such code.
 */
private fun wifiCode(network: WifiNetwork): String? {
    val type = when (network.security) {
        "wpa" -> "WPA"
        "sae" -> "SAE"
        "wep" -> "WEP"
        "open" -> "nopass"
        else -> return null
    }
    fun escaped(value: String) = value.replace(Regex("""([\\;,:"])"""), """\\$1""")
    return buildString {
        append("WIFI:T:").append(type).append(";S:").append(escaped(network.ssid)).append(';')
        if (type != "nopass") append("P:").append(escaped(network.password)).append(';')
        if (network.hidden) append("H:true;")
        append(';')
    }
}

/**
 * Android's own "Save this network?" page, from Android 11. Android asks the person, saves the
 * network beside the ones they typed in themselves, and joins it when it is in range. It takes no
 * WEP or enterprise network, nor a passphrase it thinks malformed, such as a 64-digit hex key.
 */
@RequiresApi(Build.VERSION_CODES.R)
private fun addIntent(network: WifiNetwork): Intent? {
    val builder = WifiNetworkSuggestion.Builder().setSsid(network.ssid).setIsHiddenSsid(network.hidden)
    when (network.security) {
        "wpa" -> builder.setWpa2Passphrase(network.password)
        "sae" -> builder.setWpa3Passphrase(network.password)
        "open" -> Unit
        else -> return null
    }
    return Intent(Settings.ACTION_WIFI_ADD_NETWORKS).putParcelableArrayListExtra(Settings.EXTRA_WIFI_NETWORK_LIST, arrayListOf(builder.build()))
}

private fun securityOf(entry: Detail): String {
    val chosen = entry.fields.firstOrNull { it.key == "security" }?.text.orEmpty()
    return chosen.ifEmpty { if (entry.hasPassword) "wpa" else "open" }
}

/** Null for anything but a Wi-Fi item with a network name. */
@Composable
internal fun rememberWifiActions(entry: Detail): WifiActions? {
    if (entry.kind != "wifi") return null
    val shell = LocalShell.current
    val ssid = entry.fields.firstOrNull { it.key == "ssid" }?.text.orEmpty()
    if (ssid.isEmpty()) return null
    val security = securityOf(entry)
    val hidden = entry.fields.firstOrNull { it.key == "hidden_ssid" }?.flag == true
    var code by remember(entry.id) { mutableStateOf<String?>(null) }
    val saving = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        if (result.resultCode != Activity.RESULT_OK) return@rememberLauncherForActivityResult
        val outcome = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            result.data?.getIntegerArrayListExtra(Settings.EXTRA_WIFI_NETWORK_RESULT_LIST)?.firstOrNull()
        } else {
            null
        }
        shell.notice(
            when (outcome) {
                Settings.ADD_WIFI_RESULT_ALREADY_EXISTS -> "This phone already has $ssid"
                Settings.ADD_WIFI_RESULT_ADD_OR_UPDATE_FAILED -> "Android could not save $ssid"
                else -> "Saved $ssid on this phone"
            },
        )
    }
    suspend fun network(): WifiNetwork {
        val password = if (entry.hasPassword) Pass.reveal(entry.id, "password").consumeText() else ""
        return WifiNetwork(ssid, security, password, hidden)
    }
    code?.let { shown -> WifiCodeSheet(shown, ssid) { code = null } }
    return WifiActions(
        canShow = security != "eap",
        canAdd = Build.VERSION.SDK_INT >= Build.VERSION_CODES.R && security in setOf("wpa", "sae", "open"),
        show = { shell.launch { code = wifiCode(network()) } },
        add = {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                shell.launch {
                    val intent = runCatching { addIntent(network()) }.getOrNull()
                    if (intent == null) {
                        shell.notice("Android takes this password only through its own Wi-Fi settings")
                    } else {
                        runCatching { saving.launch(intent) }.onFailure { shell.notice("This phone has no page for saving networks") }
                    }
                }
            }
        },
    )
}

@Composable
private fun WifiCodeSheet(code: String, ssid: String, onDismiss: () -> Unit) {
    val colors = palette
    PassSheet(onDismiss, ssid) {
        QrCode(code, Modifier.padding(horizontal = 30.dp))
        Spacer(Modifier.height(14.dp))
        Text(
            "Point another phone's camera at this to join the network. The password is in the code, so anyone who can see it can read it.",
            color = colors.textMid,
            style = MaterialTheme.typography.bodyMedium,
            textAlign = TextAlign.Center,
            modifier = Modifier.fillMaxWidth(),
        )
    }
}

package com.wasimaster.wmpass.ui.door

import android.os.Build
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.EngineReply
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.Lan
import com.wasimaster.wmpass.ui.parts.Listening
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QrCode
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.settings.HoldMulticast
import com.wasimaster.wmpass.ui.settings.localAddress
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.PassSheet
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

private enum class JoinPhase { STARTING, SHOWING, COMPARE, RECEIVING, DONE }

/**
 * A phone with no vault waits here for a device that has it. That device connects, the person
 * vouches for the connection with the QR code or six digits, and the sealed vault comes over.
 * It still opens only with its master password.
 */
@Composable
internal fun JoinSheet(onDismiss: () -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var phase by remember { mutableStateOf(JoinPhase.STARTING) }
    var qrText by remember { mutableStateOf<String?>(null) }
    var code by remember { mutableStateOf("") }
    var from by remember { mutableStateOf("") }
    var problem by remember { mutableStateOf<String?>(null) }
    var attempt by remember { mutableIntStateOf(0) }
    val name = remember { Pass.freshVaultName() }
    val blobs = remember { Pass.blobFolderFor(name) }

    suspend fun arrived(reply: EngineReply) {
        val obj = reply.obj
        val peer = Lan.peerOf(obj["peer"]!!.jsonObject)
        if (obj["done"]?.jsonPrimitive?.content != "true") {
            code = obj["code"]!!.jsonPrimitive.content
            from = peer.name
            phase = JoinPhase.COMPARE
            return
        }
        phase = JoinPhase.RECEIVING
        Pass.adoptFile(reply.blobs[0], name)
        Lan.addPeer(context, peer)
        from = peer.name
        phase = JoinPhase.DONE
    }

    fun stop() {
        scope.launch(Dispatchers.IO) { runCatching { Engine.net("lan.stop", online = false) } }
    }

    HoldMulticast()
    DisposableEffect(Unit) { onDispose { if (phase != JoinPhase.DONE) stop() } }
    LaunchedEffect(attempt) {
        problem = null
        phase = JoinPhase.STARTING
        try {
            qrText = Lan.joinListen(withContext(Dispatchers.IO) { localAddress(context) })
            phase = JoinPhase.SHOWING
            arrived(Lan.joinWait(context, Build.MODEL, blobs))
        } catch (failure: NativeFailure) {
            if (failure.code != "cancelled") problem = failure.message
        }
    }

    PassSheet(onDismiss, "Get it from another device") {
        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally, verticalArrangement = Arrangement.spacedBy(10.dp)) {
            val text = problem
            when {
                text != null -> {
                    Text(text, color = colors.danger, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center)
                    PrimaryButton("Try again", Modifier.fillMaxWidth()) {
                        stop()
                        attempt++
                    }
                }
                phase == JoinPhase.STARTING -> Listening(PassIcons.Devices)
                phase == JoinPhase.SHOWING -> {
                    Text(
                        "On the device that has your vault, open Settings, then Sync and accounts, then Add a new device.",
                        color = colors.text,
                        style = MaterialTheme.typography.bodyLarge,
                        textAlign = TextAlign.Center,
                    )
                    qrText?.takeIf { it.isNotEmpty() }?.let { QrCode(it, Modifier.padding(horizontal = 30.dp)) }
                    Text(
                        "Another phone scans this code. A computer or the terminal app finds this phone on the network instead, and you compare six digits. Both need the same Wi-Fi.",
                        color = colors.textMid,
                        style = MaterialTheme.typography.bodyMedium,
                        textAlign = TextAlign.Center,
                    )
                }
                phase == JoinPhase.COMPARE -> {
                    Text("Do both screens show this?", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
                    Text(code, color = colors.text, style = PassType.secret.copy(fontSize = 44.sp, lineHeight = 52.sp, fontWeight = FontWeight.Bold))
                    Text(from, color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                        fun answer(accepted: Boolean) {
                            phase = JoinPhase.RECEIVING
                            scope.launch {
                                try {
                                    arrived(Lan.joinFinish(accepted, blobs))
                                } catch (failure: NativeFailure) {
                                    problem = if (accepted) failure.message else "Stopped. Nothing was sent."
                                }
                            }
                        }
                        QuietButton("They differ", Modifier.weight(1f), tint = colors.danger) { answer(false) }
                        PrimaryButton("They match", Modifier.weight(1f)) { answer(true) }
                    }
                }
                phase == JoinPhase.RECEIVING -> {
                    Listening(PassIcons.Devices)
                    Text("Receiving the vault…", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                }
                else -> {
                    Text("Your vault is here", color = colors.text, style = MaterialTheme.typography.titleMedium)
                    Text(
                        "It came from $from. Unlock it with its master password. The two are paired, so they can sync on this Wi-Fi.",
                        color = colors.textMid,
                        style = MaterialTheme.typography.bodyMedium,
                        textAlign = TextAlign.Center,
                    )
                    PrimaryButton("Unlock it", Modifier.fillMaxWidth(), onClick = onDismiss)
                }
            }
            Spacer(Modifier.height(4.dp))
        }
    }
}

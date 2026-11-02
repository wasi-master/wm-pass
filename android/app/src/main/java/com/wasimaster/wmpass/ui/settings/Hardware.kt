package com.wasimaster.wmpass.ui.settings

import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.fragment.app.FragmentActivity
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.HardwareKeys
import com.wasimaster.wmpass.ui.door.PasswordPill
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Listening
import com.wasimaster.wmpass.ui.parts.NavRow
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmsuite.settings.WmScreen
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.put

private enum class KeyStep { CHOOSE, DETAILS, TAP, DONE }

/** Adds a hardware key as a second lock: from now on, this password opens the vault only with it. */
@Composable
fun HardwareScreen() {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val activity = context as? FragmentActivity
    var step by remember { mutableStateOf(KeyStep.CHOOSE) }
    var fido by remember { mutableStateOf(true) }
    val label = rememberTextFieldState()
    val password = rememberTextFieldState()
    var problem by remember { mutableStateOf<String?>(null) }
    WmScreen("Hardware key", onBack = { shell.nav.popBackStack() }) {
        AnimatedContent(step, label = "step", transitionSpec = { fadeIn(tween(220)) togetherWith fadeOut(tween(140)) }) { current ->
            Column(Modifier.fillMaxWidth().padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                when (current) {
                    KeyStep.CHOOSE -> {
                        Text(
                            "A hardware key adds something you hold to something you know. Someone who learns the password still gets nowhere without the key. Keep a second way in, in case you lose it.",
                            color = colors.textMid,
                            style = MaterialTheme.typography.bodyLarge,
                            modifier = Modifier.padding(horizontal = 6.dp, vertical = 8.dp),
                        )
                        Eyebrow("What kind of key")
                        GroupCard {
                            NavRow("A FIDO2 security key", "YubiKey 5, SoloKey, Nitrokey and others, by NFC or USB", icon = PassIcons.SecurityKey) {
                                fido = true
                                step = KeyStep.DETAILS
                            }
                            Hairline(inset = 70.dp)
                            NavRow("A YubiKey with challenge-response", "Uses slot 2, set up beforehand in Yubico's own tool", icon = PassIcons.Key) {
                                fido = false
                                step = KeyStep.DETAILS
                            }
                        }
                    }
                    KeyStep.DETAILS -> {
                        Text("Choose the password that goes with this key. It can be a new one, used only with the key.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(horizontal = 6.dp, vertical = 8.dp))
                        SheetField(label, "A name for the key")
                        PasswordPill(password, "Password to use with it")
                        Spacer(Modifier.height(6.dp))
                        PrimaryButton("Next", Modifier.fillMaxWidth(), enabled = password.text.isNotEmpty()) { step = KeyStep.TAP }
                    }
                    KeyStep.TAP -> {
                        LaunchedEffect(Unit) {
                            problem = null
                            val host = activity ?: return@LaunchedEffect
                            try {
                                val (factor, output) = HardwareKeys.tap(host, fido, "")
                                val secret = password.text.toString().toByteArray()
                                try {
                                    Pass.change("slot.add", secret = secret, blobs = listOf(output)) {
                                        put("label", label.text.toString().ifBlank { if (fido) "Security key" else "YubiKey" })
                                        put("action", "open")
                                        put("factor", Json.parseToJsonElement(factor).jsonObject)
                                    }
                                } finally {
                                    secret.fill(0)
                                    output.fill(0)
                                }
                                Pass.flush()
                                password.clearText()
                                shell.haptics.confirm()
                                step = KeyStep.DONE
                            } catch (failure: NativeFailure) {
                                shell.haptics.reject()
                                problem = failure.message
                            }
                        }
                        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                            Spacer(Modifier.height(36.dp))
                            Listening(PassIcons.SecurityKey)
                            Spacer(Modifier.height(24.dp))
                            Text("Hold your key to the back of the phone", color = colors.text, style = MaterialTheme.typography.titleLarge, textAlign = TextAlign.Center)
                            Spacer(Modifier.height(8.dp))
                            Text("Or plug it in and touch it. Keep it there until the phone buzzes.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge, textAlign = TextAlign.Center)
                            problem?.let {
                                Spacer(Modifier.height(14.dp))
                                Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center)
                                Spacer(Modifier.height(14.dp))
                                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                                    QuietButton("Back", height = 48.dp) { step = KeyStep.DETAILS }
                                }
                            }
                        }
                    }
                    KeyStep.DONE -> {
                        Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                            Spacer(Modifier.height(50.dp))
                            androidx.compose.animation.AnimatedVisibility(true, enter = scaleIn(com.wasimaster.wmpass.ui.theme.Springs.lively(), 0.4f) + fadeIn()) {
                                Icon(PassIcons.CheckCircleFilled, null, tint = colors.primary, modifier = Modifier.size(84.dp))
                            }
                            Spacer(Modifier.height(20.dp))
                            Text("The key is added", color = colors.text, style = MaterialTheme.typography.titleLarge)
                            Spacer(Modifier.height(8.dp))
                            Text("Next time, type this password and tap the key when asked.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge, textAlign = TextAlign.Center)
                            Spacer(Modifier.height(24.dp))
                            PrimaryButton("Done", Modifier.fillMaxWidth()) { shell.nav.popBackStack() }
                        }
                    }
                }
            }
        }
    }
}

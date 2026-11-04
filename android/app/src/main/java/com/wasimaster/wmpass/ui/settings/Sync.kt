package com.wasimaster.wmpass.ui.settings

import android.content.Context
import android.content.Intent
import android.net.ConnectivityManager
import android.net.Uri
import android.net.wifi.WifiManager
import android.os.Build
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.togetherWith
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.CircularProgressIndicator
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.AccountRef
import com.wasimaster.wmpass.system.Accounts
import com.wasimaster.wmpass.system.Destinations
import com.wasimaster.wmpass.system.FileSync
import com.wasimaster.wmpass.system.Lan
import com.wasimaster.wmpass.system.LanPeer
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.help.Help
import com.wasimaster.wmpass.ui.help.HelpMark
import com.wasimaster.wmpass.ui.help.helpAnchor
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Listening
import com.wasimaster.wmpass.ui.parts.NavRow
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QrCode
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.ConfirmSheet
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.PassSwitch
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmpass.ui.vault.SheetRow
import com.wasimaster.wmsuite.settings.WmScreen
import kotlinx.coroutines.launch
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/** Keeps multicast working on Wi-Fi for as long as this is on screen. */
@Composable
internal fun HoldMulticast() {
    val context = LocalContext.current
    DisposableEffect(Unit) {
        val wifi = context.applicationContext.getSystemService(WifiManager::class.java)
        val lock = wifi?.createMulticastLock("wmpass-lan")?.apply { setReferenceCounted(false) }
        runCatching { lock?.acquire() }
        onDispose {
            runCatching { lock?.release() }
            scopeStop()
        }
    }
}

private fun scopeStop() {
    kotlinx.coroutines.CoroutineScope(kotlinx.coroutines.Dispatchers.IO).launch {
        runCatching { Engine.net("lan.stop", online = false) }
    }
}

internal fun localAddress(context: Context): String? {
    val net = context.getSystemService(ConnectivityManager::class.java) ?: return null
    val active = net.activeNetwork ?: return null
    val address = net.getLinkProperties(active)?.linkAddresses?.map { it.address }?.firstOrNull { it is java.net.Inet4Address && !it.isLoopbackAddress }
    return address?.hostAddress
}

private enum class LanMode { IDLE, WAITING, SHOW_QR, CODE_WAIT, CODE_COMPARE }

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun SyncScreen() {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val places by Destinations.list.collectAsStateWithLifecycle()
    val accounts by Accounts.list.collectAsStateWithLifecycle()
    val account by Pass.account.collectAsStateWithLifecycle()
    val problem by Accounts.syncProblem.collectAsStateWithLifecycle()
    var refreshing by remember { mutableStateOf(false) }
    var addingAccount by remember { mutableStateOf(false) }
    var removing by remember { mutableStateOf<AccountRef?>(null) }
    var fileSyncing by remember { mutableStateOf(false) }
    var addingPlace by remember { mutableStateOf(false) }
    var fileMessage by remember { mutableStateOf<String?>(null) }
    WmScreen("Sync and accounts", route = "settings/sync", onBack = { shell.nav.popBackStack() }) {
        Column(Modifier.padding(horizontal = 16.dp)) {
            account?.let { id ->
                val current = accounts.firstOrNull { it.id == id }
                if (current?.kind != "kdbx") {
                    Eyebrow("This account") { HelpMark("group.sync.account", "this account") }
                    GroupCard {
                        Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                            GlyphTile(PassIcons.Sync, if (problem != null) colors.danger else colors.primary, size = 40.dp)
                            Spacer(Modifier.width(14.dp))
                            Column(Modifier.weight(1f)) {
                                Text(current?.label ?: "Server", color = colors.text, style = MaterialTheme.typography.bodyLarge)
                                Text(problem ?: "Changes go up after each edit. If the server cannot be reached they wait here.", color = if (problem != null) colors.danger else colors.textMid, style = MaterialTheme.typography.bodySmall)
                            }
                            Spacer(Modifier.width(8.dp))
                            if (refreshing) {
                                CircularProgressIndicator(Modifier.size(22.dp), color = colors.primary, strokeWidth = 2.dp)
                            } else {
                                RowIconButton(PassIcons.Sync, "Sync now", tint = colors.primary) {
                                    refreshing = true
                                    scope.launch {
                                        Accounts.syncNow(context)
                                        refreshing = false
                                    }
                                }
                            }
                        }
                    }
                }
            }
            LanSection()
            Column(Modifier.helpAnchor("group:sync.places")) {
            Eyebrow("Through a folder or server") { HelpMark("group.sync.files", "syncing through a folder or server") }
            Text(
                "Each device leaves its own encrypted copy there and merges the others. Pick any place you set up for backups.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(start = 6.dp, end = 6.dp, bottom = 10.dp),
            )
            GroupCard {
                places.forEachIndexed { index, place ->
                    if (index > 0) Hairline(inset = 70.dp)
                    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 12.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        GlyphTile(placeIcon(place.kind), colors.primary, size = 40.dp)
                        Spacer(Modifier.width(14.dp))
                        Text(place.label, color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                        PassSwitch(place.sync) { on -> Destinations.put(context, place.copy(sync = on)) }
                    }
                }
                if (places.isNotEmpty()) Hairline()
                NavRow("Add a place", icon = PassIcons.Add, help = "action.add_place") { addingPlace = true }
            }
            }
            if (places.any { it.sync }) {
                Spacer(Modifier.height(10.dp))
                QuietButton(if (fileSyncing) "Syncing…" else "Sync now", Modifier.fillMaxWidth(), PassIcons.Sync, colors.primary, enabled = !fileSyncing, height = 50.dp) {
                    fileSyncing = true
                    scope.launch {
                        FileSync.now(context)
                        fileMessage = FileSync.problem.value ?: "Up to date."
                        fileSyncing = false
                    }
                }
                fileMessage?.let { Text(it, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 6.dp, top = 10.dp)) }
            }
            SettingsSyncSection()
            Column(Modifier.helpAnchor("group:sync.accounts")) {
            Eyebrow("Accounts held elsewhere") { HelpMark("group.sync.accounts", "accounts held elsewhere") }
            Text(
                "Open a KeePass file where it is, or sign in to Bitwarden, Vaultwarden or a WM Pass server. They appear on the door beside this vault.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(start = 6.dp, end = 6.dp, bottom = 10.dp),
            )
            GroupCard {
                accounts.forEachIndexed { index, item ->
                    if (index > 0) Hairline(inset = 70.dp)
                    val where = com.wasimaster.wmpass.system.accountHost(item)
                    NavRow(item.label, if (item.kind == "kdbx") where else listOf(item.email, where).filter { it.isNotEmpty() }.joinToString(" on "), icon = if (item.kind == "kdbx") PassIcons.Key else PassIcons.Sync) { removing = item }
                }
                if (accounts.isNotEmpty()) Hairline()
                NavRow("Add an account", icon = PassIcons.Add, help = "action.add_account") { addingAccount = true }
            }
            }
            Eyebrow("Vaults inside this one")
            GroupCard {
                NavRow("Nested and shared vaults", "Separate spaces, and vaults you share with family", icon = PassIcons.FamilyRestroom) { shell.nav.navigate("settings/vaults") }
            }
            Eyebrow("Hand over one item")
            GroupCard {
                NavRow("Receive an item", "From WM Pass on another device, by QR code", icon = PassIcons.QrCodeScanner) { shell.nav.navigate("settings/receive") }
            }
            Spacer(Modifier.height(40.dp))
        }
    }
    if (addingAccount) AddAccountSheet { addingAccount = false }
    if (addingPlace) DestinationSheet(null, { addingPlace = false }) { addingPlace = false }
    removing?.let { item ->
        ConfirmSheet(
            "Remove ${item.label}?",
            "WM Pass forgets this account on this phone, with its offline copy. Nothing on the server or in the file changes.",
            "Remove",
            onDismiss = { removing = null },
        ) { Accounts.remove(context, item.id) }
    }
}

/**
 * Adding an account stores only where it is. Signing in happens at the door, since an account opens
 * in place of the vault: added from Settings, it offers to lock this vault and go there.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AddAccountSheet(onDismiss: () -> Unit) {
    val colors = palette
    val context = LocalContext.current
    var added by remember { mutableStateOf<AccountRef?>(null) }
    added?.let { made ->
        PassSheet(onDismiss, "Sign in to ${made.label}") {
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(
                    "An account opens in place of this vault. Signing in locks this one, and the door asks for the account's password.",
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(horizontal = 6.dp),
                )
                PrimaryButton("Sign in now", Modifier.fillMaxWidth()) {
                    Accounts.signInNext(made.id)
                    onDismiss()
                    // Not this sheet's scope, which goes with it: a lock cut short would leave saves behind.
                    kotlinx.coroutines.MainScope().launch { Pass.lock() }
                }
                QuietButton("Later", Modifier.fillMaxWidth(), onClick = onDismiss)
            }
        }
        return
    }
    var kind by remember { mutableStateOf("bitwarden") }
    var region by remember { mutableStateOf("") }
    val server = rememberTextFieldState()
    val email = rememberTextFieldState()
    val label = rememberTextFieldState()
    var uri by remember { mutableStateOf<Uri?>(null) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { picked ->
        if (picked != null) {
            context.contentResolver.takePersistableUriPermission(picked, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            uri = picked
        }
    }
    PassSheet(onDismiss, "Add an account") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                Pill("Bitwarden", kind == "bitwarden" && region != "eu") {
                    kind = "bitwarden"
                    region = ""
                }
                Pill("Bitwarden EU", region == "eu") {
                    kind = "bitwarden"
                    region = "eu"
                }
                Pill("Own server", kind == "server") {
                    kind = "server"
                    region = ""
                }
                Pill("KeePass file", kind == "kdbx") { kind = "kdbx" }
            }
            when (kind) {
                "kdbx" -> {
                    Text(
                        "The file stays where it is and WM Pass writes back to it, keeping one earlier copy beside its own files. Syncthing or a cloud app can carry it.",
                        color = colors.textMid,
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.padding(horizontal = 6.dp),
                    )
                    QuietButton(if (uri == null) "Choose the file" else "File chosen", Modifier.fillMaxWidth(), PassIcons.Folder, colors.primary) { pick.launch(arrayOf("*/*")) }
                }
                else -> {
                    if (kind == "server") SheetField(server, "https://vault.example.com")
                    SheetField(email, "Email")
                    Text(
                        "This needs the network. Your password is never sent: a key derived from it proves who you are, as with any Bitwarden client.",
                        color = colors.textMid,
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.padding(horizontal = 6.dp),
                    )
                }
            }
            SheetField(label, "A name for the door, such as Work")
            Spacer(Modifier.height(4.dp))
            val ready = if (kind == "kdbx") uri != null else email.text.isNotBlank() && (kind != "server" || server.text.isNotBlank())
            PrimaryButton("Add", Modifier.fillMaxWidth(), enabled = ready) {
                val fallback = when {
                    kind == "kdbx" -> "KeePass"
                    kind == "server" -> runCatching { Uri.parse(server.text.toString()).host }.getOrNull() ?: "Server"
                    else -> "Bitwarden"
                }
                if (kind != "kdbx") com.wasimaster.wmpass.app.Prefs.of(context).updateNetworkAllowed(true)
                val ref = AccountRef(
                    id = Accounts.newId(),
                    kind = if (kind == "kdbx") "kdbx" else "bitwarden",
                    label = label.text.toString().trim().ifEmpty { fallback },
                    server = if (kind == "server") server.text.toString().trim() else "",
                    region = region,
                    email = email.text.toString().trim(),
                    uri = uri?.toString().orEmpty(),
                )
                Accounts.add(context, ref)
                if (Pass.state.value is com.wasimaster.wmpass.core.LockState.Unlocked) {
                    added = ref
                } else {
                    // Added from the door itself: it opens with this account chosen.
                    Accounts.signInNext(ref.id)
                    onDismiss()
                }
            }
        }
    }
}

/** Whether customisations follow the vault, and which ones stay on this phone. */
@Composable
private fun SettingsSyncSection() {
    val prefs = Prefs.of(LocalContext.current)
    var choosing by remember { mutableStateOf(false) }
    Eyebrow("Settings") { HelpMark("setting.sync_settings", "syncing settings") }
    GroupCard {
        SyncSwitchRow("Sync settings with the vault", "Your customisations follow this vault to your other devices. Protection settings stay here.", prefs.syncSettings.value) {
            prefs.syncSettings.set(it)
        }
        if (prefs.syncSettings.value) {
            Hairline()
            val skipped = prefs.syncSkip
            NavRow(
                "Settings that stay on this phone",
                if (skipped.isEmpty()) "None: everything syncs" else "${skipped.size} kept here",
                icon = PassIcons.Devices,
                help = "setting.sync_skip",
            ) { choosing = !choosing }
            if (choosing) {
                // Named as the guide names them, grouped by the page each one is on.
                val rows = remember {
                    prefs.syncable
                        .mapNotNull { opt -> Help.forPref(opt.key)?.let { Triple(opt.key, it.title, Help.where(it).removePrefix("Settings, ")) } }
                        .sortedWith(compareBy({ it.third }, { it.second }))
                }
                rows.forEach { (key, title, where) ->
                    Hairline()
                    SyncSwitchRow(title, where, key !in skipped) { prefs.setSyncSkipped(key, !it) }
                }
            }
        }
    }
}

@Composable
private fun SyncSwitchRow(title: String, subtitle: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    val colors = palette
    Row(Modifier.fillMaxWidth().padding(start = 18.dp, end = 12.dp, top = 10.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge)
            Text(subtitle, color = colors.textMid, style = MaterialTheme.typography.bodySmall)
        }
        Spacer(Modifier.width(8.dp))
        PassSwitch(checked) { onChange(it) }
    }
}

@Composable
private fun LanSection() {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var peers by remember { mutableStateOf(Lan.peers(context)) }
    var mode by remember { mutableStateOf(LanMode.IDLE) }
    var qrText by remember { mutableStateOf<String?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
    var syncing by remember { mutableStateOf<String?>(null) }
    var adding by remember { mutableStateOf(false) }
    var codeText by remember { mutableStateOf<String?>(null) }
    var codePeer by remember { mutableStateOf<String?>(null) }
    var forgetting by remember { mutableStateOf<LanPeer?>(null) }
    var joining by remember { mutableStateOf(false) }
    var pastingJoin by remember { mutableStateOf(false) }
    /** Set while the digits on show are for sending this vault, not for pairing. */
    var sendingVault by remember { mutableStateOf(false) }
    val name = remember { Build.MODEL }

    fun sentTo(peer: LanPeer) {
        peers = Lan.peers(context)
        message = "Sent to ${peer.name}. Unlock it there with this vault's master password. The two are paired, so Sync now keeps them together."
        shell.haptics.confirm()
    }

    fun failed(failure: Throwable) {
        mode = LanMode.IDLE
        if (failure is NativeFailure && failure.code == "cancelled") return
        message = failure.message
        shell.haptics.reject()
    }

    fun reportOf(reply: com.wasimaster.wmpass.core.EngineReply): String {
        val o = reply.obj
        val sent = o["sent"]?.jsonPrimitive?.content ?: "0"
        val received = o["received"]?.jsonPrimitive?.content ?: "0"
        return "Done with ${o["peer"]?.jsonPrimitive?.content}: $sent sent and $received received."
    }

    fun syncWith(peer: LanPeer?) {
        syncing = peer?.key ?: "any"
        message = null
        scope.launch {
            try {
                val reply = Lan.sync(context)
                message = reportOf(reply)
                shell.haptics.confirm()
            } catch (failure: NativeFailure) {
                failed(failure)
            }
            syncing = null
        }
    }

    Column(Modifier.helpAnchor("group:sync.lan")) {
    Eyebrow("Your devices, on this network") { HelpMark("group.sync.lan", "syncing on this network") }
    NetGate("Sync talks directly to your own devices on this Wi-Fi network, encrypted end to end, with nothing in between. Android files this under internet access, so it needs your yes like the rest.") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            HoldMulticast()
            AnimatedContent(mode, label = "lan", transitionSpec = { fadeIn(tween(220)) togetherWith fadeOut(tween(160)) }) { current ->
                when (current) {
                    LanMode.IDLE -> Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                        GroupCard {
                            peers.forEachIndexed { index, peer ->
                                if (index > 0) Hairline(inset = 70.dp)
                                Row(
                                    Modifier.fillMaxWidth().clickable { forgetting = peer }.padding(horizontal = 16.dp, vertical = 12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    GlyphTile(PassIcons.Devices, colors.primary, size = 40.dp)
                                    Spacer(Modifier.width(14.dp))
                                    Text(peer.name, color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f), maxLines = 1, overflow = TextOverflow.Ellipsis)
                                }
                            }
                            if (peers.isNotEmpty()) Hairline()
                            if (Pass.isFolderVault) {
                                NavRow("Add a new device", "Send this vault to a phone or computer that has none yet", icon = PassIcons.Devices, help = "action.lan_join") { joining = true }
                                Hairline()
                            }
                            NavRow("Pair a device", "This phone and a desktop, or two phones", icon = PassIcons.Add, help = "action.lan_pair") { adding = true }
                        }
                        if (peers.isNotEmpty()) {
                            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                                PrimaryButton(if (syncing != null) "Looking…" else "Sync now", Modifier.weight(1f), enabled = syncing == null, icon = PassIcons.Sync) { syncWith(null) }
                                QuietButton("Wait for one", Modifier.weight(1f)) {
                                    mode = LanMode.WAITING
                                    message = null
                                    scope.launch {
                                        try {
                                            message = reportOf(Lan.serve(context))
                                            shell.haptics.confirm()
                                        } catch (failure: NativeFailure) {
                                            failed(failure)
                                        }
                                        mode = LanMode.IDLE
                                    }
                                }
                            }
                        }
                    }
                    LanMode.WAITING -> Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                        Listening(PassIcons.Devices)
                        Spacer(Modifier.height(12.dp))
                        Text("Waiting for your other device", color = colors.text, style = MaterialTheme.typography.titleMedium)
                        Spacer(Modifier.height(4.dp))
                        Text("Tap Sync now on it. Stay on this page.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                        Spacer(Modifier.height(14.dp))
                        QuietButton("Stop", height = 46.dp) { scopeStop() }
                    }
                    LanMode.SHOW_QR -> Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                        qrText?.let { QrCode(it, Modifier.padding(horizontal = 30.dp)) }
                        Spacer(Modifier.height(14.dp))
                        Text("Scan this with your other device", color = colors.text, style = MaterialTheme.typography.titleMedium)
                        Spacer(Modifier.height(4.dp))
                        Text("It works once and stops in two minutes.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                        Spacer(Modifier.height(14.dp))
                        QuietButton("Cancel", height = 46.dp) { scopeStop() }
                    }
                    LanMode.CODE_WAIT -> Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                        Listening(PassIcons.Devices)
                        Spacer(Modifier.height(12.dp))
                        Text("Waiting for the other device to connect", color = colors.text, style = MaterialTheme.typography.titleMedium, textAlign = TextAlign.Center)
                        Spacer(Modifier.height(14.dp))
                        QuietButton("Cancel", height = 46.dp) { scopeStop() }
                    }
                    LanMode.CODE_COMPARE -> Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                        Text("Do both screens show this?", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
                        Spacer(Modifier.height(10.dp))
                        Text(codeText.orEmpty(), color = colors.text, style = PassType.secret.copy(fontSize = 44.sp, lineHeight = 52.sp, fontWeight = FontWeight.Bold))
                        Spacer(Modifier.height(6.dp))
                        Text(codePeer.orEmpty(), color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                        Spacer(Modifier.height(18.dp))
                        Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                            fun finish(accepted: Boolean) {
                                if (sendingVault) {
                                    sendingVault = false
                                    scope.launch {
                                        try {
                                            sentTo(Lan.joinSend(context, accepted))
                                        } catch (failure: NativeFailure) {
                                            failed(failure)
                                        }
                                        mode = LanMode.IDLE
                                        scopeStop()
                                    }
                                    return
                                }
                                scope.launch {
                                    try {
                                        val reply = Engine.net("lan.code_finish", online = false) { put("accepted", accepted) }
                                        if (accepted) {
                                            Lan.addPeer(context, LanPeer(reply.obj["key"]!!.jsonPrimitive.content, reply.obj["name"]!!.jsonPrimitive.content))
                                            peers = Lan.peers(context)
                                            message = "Paired. Tap Sync now to bring them together."
                                            shell.haptics.confirm()
                                        }
                                    } catch (failure: NativeFailure) {
                                        failed(failure)
                                    }
                                    mode = LanMode.IDLE
                                    scopeStop()
                                }
                            }
                            QuietButton("They differ", Modifier.weight(1f), tint = colors.danger) { finish(false) }
                            PrimaryButton("They match", Modifier.weight(1f)) { finish(true) }
                        }
                    }
                }
            }
            message?.let { Text(it, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp)) }
        }
    }
    }
    if (adding) {
        PassSheet({ adding = false }, "Pair a device") {
            Text(
                "On the other device, open WM Pass and go to Settings, then Sync and accounts, then Pair a device. Both need to be on the same Wi-Fi.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(start = 4.dp, end = 4.dp, bottom = 10.dp),
            )
            SheetRow(PassIcons.QrCodeScanner, "Show a code", "The other device scans it") {
                adding = false
                val address = localAddress(context)
                if (address == null) {
                    message = "This phone is not on a network."
                    return@SheetRow
                }
                scope.launch {
                    try {
                        val port = Engine.net("lan.listen", online = false).obj["port"]!!.jsonPrimitive.content
                        qrText = Engine.net("lan.pair_show", online = false) { put("address", "$address:$port") }.secret!!.consumeText()
                        mode = LanMode.SHOW_QR
                        val identity = Lan.identity(context)
                        val reply = Engine.net("lan.pair_wait", online = false, secret = identity) { put("name", name) }
                        identity.fill(0)
                        Lan.addPeer(context, LanPeer(reply.obj["key"]!!.jsonPrimitive.content, reply.obj["name"]!!.jsonPrimitive.content))
                        peers = Lan.peers(context)
                        message = "Paired. Tap Sync now to bring them together."
                        shell.haptics.confirm()
                    } catch (failure: NativeFailure) {
                        failed(failure)
                    }
                    mode = LanMode.IDLE
                    scopeStop()
                }
            }
            SheetRow(PassIcons.QrCodeScanner, "Scan a code", "The other device shows it") {
                adding = false
                shell.scanTarget = { text ->
                    scope.launch {
                        try {
                            val identity = Lan.identity(context)
                            val reply = Engine.net("lan.pair_scan", online = false, secret = identity) {
                                put("text", text)
                                put("name", name)
                            }
                            identity.fill(0)
                            val peer = reply.obj["peer"]!!.jsonObject
                            Lan.addPeer(context, LanPeer(peer["key"]!!.jsonPrimitive.content, peer["name"]!!.jsonPrimitive.content))
                            peers = Lan.peers(context)
                            message = "Paired. Tap Sync now to bring them together."
                            shell.haptics.confirm()
                        } catch (failure: NativeFailure) {
                            failed(failure)
                        }
                    }
                }
            }
            SheetRow(PassIcons.Pin, "Compare six digits", "When a camera will not do. This side waits") {
                adding = false
                scope.launch {
                    try {
                        val port = Engine.net("lan.listen", online = false).obj["port"]!!.jsonPrimitive.content.toInt()
                        Engine.net("lan.advertise", online = false) { put("port", port) }
                        mode = LanMode.CODE_WAIT
                        val identity = Lan.identity(context)
                        val reply = Engine.net("lan.code_wait", online = false, secret = identity) { put("name", name) }
                        identity.fill(0)
                        codeText = reply.obj["code"]!!.jsonPrimitive.content
                        codePeer = reply.obj["peer"]!!.jsonObject["name"]!!.jsonPrimitive.content
                        mode = LanMode.CODE_COMPARE
                    } catch (failure: NativeFailure) {
                        failed(failure)
                    }
                }
            }
            SheetRow(PassIcons.Pin, "Compare six digits, finding the other", "The other device is already waiting") {
                adding = false
                scope.launch {
                    try {
                        val found = Lan.browse()
                        val address = found.firstOrNull() ?: throw NativeFailure("lan\nNo device is waiting on this network.")
                        val identity = Lan.identity(context)
                        val reply = Engine.net("lan.code_connect", online = false, secret = identity) {
                            put("address", address)
                            put("name", name)
                        }
                        identity.fill(0)
                        codeText = reply.obj["code"]!!.jsonPrimitive.content
                        codePeer = reply.obj["peer"]!!.jsonObject["name"]!!.jsonPrimitive.content
                        mode = LanMode.CODE_COMPARE
                    } catch (failure: NativeFailure) {
                        failed(failure)
                    }
                }
            }
        }
    }
    if (joining) {
        PassSheet({ joining = false }, "Add a new device") {
            Text(
                "On the new device, tap I already have a vault, then Get it from another device. It shows a code and waits. Both need to be on the same Wi-Fi.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(start = 4.dp, end = 4.dp, bottom = 10.dp),
            )
            SheetRow(PassIcons.QrCodeScanner, "Scan its code", "The code on the new phone, computer or terminal") {
                joining = false
                shell.scanTarget = { text ->
                    scope.launch {
                        try {
                            sentTo(Lan.joinScan(context, text, name))
                        } catch (failure: NativeFailure) {
                            failed(failure)
                        }
                    }
                }
            }
            SheetRow(PassIcons.ContentPaste, "Paste its code", "The text under the code on a computer or in the terminal app") {
                joining = false
                pastingJoin = true
            }
            SheetRow(PassIcons.Pin, "Find it on this network", "Then compare six digits on both screens") {
                joining = false
                message = null
                scope.launch {
                    try {
                        mode = LanMode.CODE_WAIT
                        val address = Lan.joinFind().firstOrNull()
                            ?: throw NativeFailure("lan\nNo new device is waiting on this network. On it, tap I already have a vault, then Get it from another device.")
                        val reply = Lan.joinConnect(context, address, name)
                        codeText = reply.obj["code"]!!.jsonPrimitive.content
                        codePeer = reply.obj["peer"]!!.jsonObject["name"]!!.jsonPrimitive.content
                        sendingVault = true
                        mode = LanMode.CODE_COMPARE
                    } catch (failure: NativeFailure) {
                        failed(failure)
                    }
                }
            }
        }
    }
    if (pastingJoin) {
        val pasted = rememberTextFieldState()
        PassSheet({ pastingJoin = false }, "Paste its code") {
            Text(
                "Copy the code's text on the new device, then paste it here. It starts with wmpass-pair and works for two minutes.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(start = 4.dp, end = 4.dp, bottom = 10.dp),
            )
            SheetField(pasted, "wmpass-pair:1:…")
            Spacer(Modifier.height(12.dp))
            PrimaryButton("Send the vault", Modifier.fillMaxWidth(), enabled = pasted.text.isNotBlank()) {
                val text = pasted.text.toString().trim()
                pastingJoin = false
                message = null
                scope.launch {
                    try {
                        sentTo(Lan.joinScan(context, text, name))
                    } catch (failure: NativeFailure) {
                        failed(failure)
                    }
                }
            }
        }
    }
    forgetting?.let { peer ->
        ConfirmSheet(
            "Forget ${peer.name}?",
            "It will need pairing again before it can sync with this phone.",
            "Forget",
            onDismiss = { forgetting = null },
        ) {
            Lan.removePeer(context, peer.key)
            peers = Lan.peers(context)
        }
    }
}

/** The other half of a one-time handover: show a code, then read the answer. */
@Composable
fun ReceiveScreen() {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    var token by remember { mutableStateOf<String?>(null) }
    var offer by remember { mutableStateOf<String?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var tries by remember { mutableIntStateOf(0) }
    LaunchedEffect(tries) {
        runCatching {
            val reply = Engine.net("share.offer", online = false)
            token = reply.obj["token"]!!.jsonPrimitive.content
            offer = reply.obj["text"]!!.jsonPrimitive.content
        }.onFailure { problem = it.message }
    }
    WmScreen("Receive an item", route = "settings/receive", onBack = { shell.nav.popBackStack() }) {
        Column(Modifier.fillMaxWidth().padding(horizontal = 24.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            Spacer(Modifier.height(10.dp))
            offer?.let { QrCode(it, Modifier.padding(horizontal = 12.dp)) }
            Spacer(Modifier.height(18.dp))
            Text("On the other device", color = colors.text, style = MaterialTheme.typography.titleMedium)
            Spacer(Modifier.height(4.dp))
            Text(
                "Open WM Pass there, open the item, choose Send to a device, and scan this. Then scan the code it shows you. This works once and stops in two minutes, and nothing travels over a network.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyLarge,
                textAlign = TextAlign.Center,
            )
            problem?.let {
                Spacer(Modifier.height(10.dp))
                Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center)
            }
            Spacer(Modifier.height(20.dp))
            PrimaryButton("Scan its answer", Modifier.fillMaxWidth(), icon = PassIcons.QrCodeScanner, enabled = offer != null) {
                shell.scanTarget = { text ->
                    scope.launch {
                        try {
                            val id = Pass.net("share.open", online = false) {
                                put("token", token.orEmpty())
                                put("text", text)
                            }.obj["id"]!!.jsonPrimitive.content
                            Pass.reload()
                            Pass.flush()
                            shell.haptics.confirm()
                            shell.nav.popBackStack()
                            shell.open(id)
                        } catch (failure: NativeFailure) {
                            problem = failure.message
                            shell.haptics.reject()
                            tries++
                        }
                    }
                }
            }
            Spacer(Modifier.height(30.dp))
        }
    }
}

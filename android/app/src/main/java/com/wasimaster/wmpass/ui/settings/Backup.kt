package com.wasimaster.wmpass.ui.settings

import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
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
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmkeyboard.core.settings.sink.SinkEntry
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.Backups
import com.wasimaster.wmpass.system.Destination
import com.wasimaster.wmpass.system.Destinations
import com.wasimaster.wmpass.system.sinkProblem
import com.wasimaster.wmpass.ui.door.PasswordPill
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.ConfirmSheet
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmsuite.settings.WmScreen
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.launch

private fun kindLabel(kind: String) = when (kind) {
    "folder" -> "Folder on this phone or a cloud app"
    "webdav" -> "WebDAV, such as Nextcloud"
    "s3" -> "S3, such as Backblaze or MinIO"
    "sftp" -> "SFTP server"
    else -> kind
}

private fun kindIcon(kind: String) = when (kind) {
    "folder" -> PassIcons.Folder
    "webdav" -> PassIcons.Language
    "s3" -> PassIcons.Database
    else -> PassIcons.Terminal
}

fun whenShort(ms: Long): String = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(ms))

/**
 * Add or change a place to keep copies. Works at the door too, where no vault is open yet, so
 * a phone can be restored from a place that was set up on another one.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun DestinationSheet(existing: Destination?, onDismiss: () -> Unit, onSaved: (Destination) -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val prefs = remember { Prefs.of(context) }
    var kind by remember { mutableStateOf(existing?.kind ?: "folder") }
    val label = rememberTextFieldState(existing?.label.orEmpty())
    val url = rememberTextFieldState(existing?.url.orEmpty())
    val user = rememberTextFieldState(existing?.user.orEmpty())
    val password = rememberTextFieldState(existing?.password.orEmpty())
    val endpoint = rememberTextFieldState(existing?.endpoint.orEmpty())
    val region = rememberTextFieldState(existing?.region ?: "us-east-1")
    val bucket = rememberTextFieldState(existing?.bucket.orEmpty())
    val keyId = rememberTextFieldState(existing?.accessKeyId.orEmpty())
    val keySecret = rememberTextFieldState(existing?.secretAccessKey.orEmpty())
    val prefix = rememberTextFieldState(existing?.prefix.orEmpty())
    val host = rememberTextFieldState(existing?.host.orEmpty())
    val port = rememberTextFieldState((existing?.port ?: 22).toString())
    val path = rememberTextFieldState(existing?.path.orEmpty())
    var folderUri by remember { mutableStateOf(existing?.uri.orEmpty()) }
    var folderName by remember { mutableStateOf(if (existing?.uri.isNullOrEmpty()) null else "Chosen") }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri != null) {
            context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            folderUri = uri.toString()
            folderName = uri.lastPathSegment?.substringAfterLast(':')?.ifEmpty { null } ?: "Chosen"
        }
    }
    PassSheet(onDismiss, if (existing == null) "A place for copies" else existing.label) {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (existing == null) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    listOf("folder" to "Folder", "webdav" to "WebDAV", "s3" to "S3", "sftp" to "SFTP").forEach { (id, name) ->
                        Pill(name, kind == id) { kind = id }
                    }
                }
            }
            Text(
                if (kind == "folder") {
                    "Pick a folder. Syncthing, Nextcloud or a cloud provider's own app can carry it from there."
                } else {
                    "Only the encrypted file goes there, so the server cannot read it. Its login is kept on this phone, sealed by the phone's secure hardware."
                },
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
            SheetField(label, "A name for it")
            when (kind) {
                "folder" -> QuietButton(folderName?.let { "Folder: $it" } ?: "Choose a folder", Modifier.fillMaxWidth(), PassIcons.Folder, colors.primary) { pick.launch(null) }
                "webdav" -> {
                    SheetField(url, "https://example.com/remote.php/dav/files/you/")
                    SheetField(user, "User name")
                    PasswordPill(password, "Password")
                }
                "s3" -> {
                    SheetField(endpoint, "Endpoint, empty for AWS")
                    SheetField(region, "Region")
                    SheetField(bucket, "Bucket")
                    SheetField(prefix, "Folder inside it, if any")
                    SheetField(keyId, "Access key")
                    PasswordPill(keySecret, "Secret key")
                }
                else -> {
                    SheetField(host, "Host")
                    SheetField(port, "Port")
                    SheetField(user, "User name")
                    PasswordPill(password, "Password")
                    SheetField(path, "Folder on the server")
                    Text(
                        "The server's key is remembered on the first connection. If it ever changes, WM Pass refuses to connect.",
                        color = colors.textLow,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(horizontal = 6.dp),
                    )
                }
            }
            val ready = when (kind) {
                "folder" -> folderUri.isNotEmpty()
                "webdav" -> url.text.isNotBlank() && user.text.isNotBlank()
                "s3" -> bucket.text.isNotBlank() && keyId.text.isNotBlank() && keySecret.text.isNotBlank()
                else -> host.text.isNotBlank() && user.text.isNotBlank()
            }
            Spacer(Modifier.height(6.dp))
            PrimaryButton("Save", Modifier.fillMaxWidth(), enabled = ready) {
                val saved = (existing ?: Destination(id = java.util.UUID.randomUUID().toString(), kind = kind, label = "")).copy(
                    kind = kind,
                    label = label.text.toString().trim().ifEmpty { kindLabel(kind).substringBefore(',') },
                    uri = folderUri,
                    url = url.text.toString().trim(),
                    user = user.text.toString().trim(),
                    password = password.text.toString(),
                    endpoint = endpoint.text.toString().trim(),
                    region = region.text.toString().trim().ifEmpty { "us-east-1" },
                    bucket = bucket.text.toString().trim(),
                    prefix = prefix.text.toString().trim(),
                    accessKeyId = keyId.text.toString().trim(),
                    secretAccessKey = keySecret.text.toString(),
                    host = host.text.toString().trim(),
                    port = port.text.toString().toIntOrNull() ?: 22,
                    path = path.text.toString().trim(),
                )
                // Setting up a remote place is the owner's yes to reaching it.
                if (saved.remote) prefs.updateNetworkAllowed(true)
                Destinations.put(context, saved)
                onSaved(saved)
            }
        }
    }
}

@Composable
private fun DestinationRow(place: Destination, tick: Int, busy: Boolean, onOpen: () -> Unit, onBackup: () -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val last = remember(place.id, tick) { Destinations.lastRun(context, place.id) }
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onOpen).padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GlyphTile(kindIcon(place.kind), if (last?.problem != null) colors.danger else colors.primary, size = 40.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(place.label, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                last?.problem ?: last?.let { "Last copy ${whenShort(it.atMs)}" } ?: kindLabel(place.kind),
                color = if (last?.problem != null) colors.danger else colors.textMid,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 2,
                overflow = TextOverflow.Ellipsis,
            )
        }
        Spacer(Modifier.width(8.dp))
        if (busy) {
            CircularProgressIndicator(Modifier.size(22.dp), color = colors.primary, strokeWidth = 2.dp)
        } else {
            com.wasimaster.wmpass.ui.parts.RowIconButton(PassIcons.Backup, "Back up now", tint = colors.primary, onClick = onBackup)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun BackupScreen() {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val places by Destinations.list.collectAsStateWithLifecycle()
    var editing by remember { mutableStateOf<Destination?>(null) }
    var adding by remember { mutableStateOf(false) }
    var busy by remember { mutableStateOf<String?>(null) }
    var tick by remember { mutableIntStateOf(0) }
    var every by remember { mutableIntStateOf(Backups.everyDays(context)) }
    var keep by remember { mutableIntStateOf(Backups.keep(context)) }
    var restoring by remember { mutableStateOf<Destination?>(null) }
    var removing by remember { mutableStateOf<Destination?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
    val saveFile = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri != null) {
            scope.launch {
                val bytes = Pass.bundle()
                context.contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                shell.haptics.confirm()
                message = "The backup is saved. It opens with your vault's password."
            }
        }
    }
    val openFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            scope.launch {
                val bytes = context.contentResolver.openInputStream(uri)?.use { it.readBytes() } ?: return@launch
                val merged = Pass.mergeIn(bytes)
                message = if (merged > 0) "Merged into this vault. Anything newer here was kept, and older values moved to history." else "That file is under another key, so it holds some other vault."
                if (merged > 0) shell.haptics.confirm() else shell.haptics.reject()
            }
        }
    }
    WmScreen("Backups", route = "settings/backup", onBack = { shell.nav.popBackStack() }) {
        Column(Modifier.padding(horizontal = 16.dp)) {
            Text(
                "A backup is the encrypted file itself, so it can sit anywhere and only your password opens it. It includes every vault on this phone, without showing how many there are.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.padding(horizontal = 6.dp, vertical = 8.dp),
            )
            Eyebrow("Where they go")
            GroupCard {
                places.forEachIndexed { index, place ->
                    if (index > 0) Hairline(inset = 70.dp)
                    DestinationRow(
                        place,
                        tick,
                        busy == place.id,
                        onOpen = { editing = place },
                        onBackup = {
                            busy = place.id
                            scope.launch {
                                Backups.to(context, place)
                                    .onSuccess { shell.haptics.confirm() }
                                    .onFailure { shell.haptics.reject() }
                                busy = null
                                tick++
                            }
                        },
                    )
                }
                if (places.isNotEmpty()) Hairline()
                com.wasimaster.wmpass.ui.parts.NavRow("Add a place", icon = PassIcons.Add) { adding = true }
            }
            Eyebrow("How often")
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(0 to "By hand", 1 to "Every day", 7 to "Every week").forEach { (days, name) ->
                    Pill(name, every == days) {
                        every = days
                        Backups.schedule(context, days)
                    }
                }
            }
            AnimatedVisibility(every > 0) {
                Text(
                    "Scheduled copies run even while the vault is locked, since they only move ciphertext. The newest copy is read back and checked before any older one is removed.",
                    color = colors.textLow,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(start = 6.dp, end = 6.dp, top = 10.dp),
                )
            }
            Eyebrow("How many to keep")
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(3, 7, 14, 30).forEach { count ->
                    Pill(count.toString(), keep == count) {
                        keep = count
                        Backups.setKeep(context, count)
                    }
                }
            }
            Eyebrow("By hand")
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                QuietButton("Save a file", Modifier.weight(1f), PassIcons.Upload, height = 50.dp) { saveFile.launch("wm-pass.wmbackup") }
                QuietButton("Merge a file", Modifier.weight(1f), PassIcons.Download, height = 50.dp) { openFile.launch(arrayOf("*/*")) }
            }
            message?.let {
                Text(it, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 6.dp, end = 6.dp, top = 12.dp))
            }
            Spacer(Modifier.height(40.dp))
        }
    }
    if (adding) {
        DestinationSheet(null, { adding = false }) { place ->
            adding = false
            busy = place.id
            scope.launch {
                Backups.to(context, place)
                busy = null
                tick++
            }
        }
    }
    editing?.let { place ->
        PlaceSheet(
            place,
            onDismiss = { editing = null },
            onRestore = {
                editing = null
                restoring = place
            },
            onRemove = {
                editing = null
                removing = place
            },
        )
    }
    restoring?.let { place ->
        CopiesSheet(place, { restoring = null }) { bytes ->
            val merged = Pass.mergeIn(bytes)
            restoring = null
            message = if (merged > 0) "Merged into this vault. Anything newer here was kept, and older values moved to history." else "That copy is under another key, so it holds some other vault."
        }
    }
    removing?.let { place ->
        ConfirmSheet(
            "Stop using ${place.label}?",
            "WM Pass forgets this place and its login. Copies already there stay where they are.",
            "Stop using it",
            onDismiss = { removing = null },
        ) { Destinations.remove(context, place.id) }
    }
}

@Composable
private fun PlaceSheet(place: Destination, onDismiss: () -> Unit, onRestore: () -> Unit, onRemove: () -> Unit) {
    var editing by remember { mutableStateOf(false) }
    if (editing) {
        DestinationSheet(place, onDismiss) { onDismiss() }
        return
    }
    PassSheet(onDismiss, place.label) {
        com.wasimaster.wmpass.ui.vault.SheetRow(PassIcons.History, "Bring back a copy", "Merge an older copy into this vault", onClick = onRestore)
        com.wasimaster.wmpass.ui.vault.SheetRow(PassIcons.Edit, "Change the details") { editing = true }
        com.wasimaster.wmpass.ui.vault.SheetRow(PassIcons.Delete, "Stop using it", tint = palette.danger, onClick = onRemove)
    }
}

/** The copies at a place, newest first, to bring one back. */
@Composable
fun CopiesSheet(place: Destination, onDismiss: () -> Unit, onPicked: suspend (ByteArray) -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var copies by remember { mutableStateOf<List<SinkEntry>?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    androidx.compose.runtime.LaunchedEffect(place.id) {
        Backups.copies(context, place).onSuccess { copies = it }.onFailure { problem = sinkProblem(it) }
    }
    PassSheet(onDismiss, "Copies at ${place.label}") {
        problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
        val shown = copies
        when {
            shown == null && problem == null -> CircularProgressIndicator(Modifier.size(26.dp), color = colors.primary, strokeWidth = 2.dp)
            shown != null && shown.isEmpty() -> Text("There are no copies here yet.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
            shown != null -> shown.take(30).forEach { entry ->
                Row(
                    Modifier.fillMaxWidth().clickable(enabled = !busy) {
                        busy = true
                        scope.launch {
                            val bytes = runCatching { Backups.read(context, place, entry) }
                            bytes.onSuccess { onPicked(it) }.onFailure { problem = sinkProblem(it) }
                            busy = false
                        }
                    }.padding(vertical = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Icon(PassIcons.History, null, tint = colors.primary, modifier = Modifier.size(20.dp))
                    Spacer(Modifier.width(14.dp))
                    Text(
                        if (entry.modifiedAtMs > 0) whenShort(entry.modifiedAtMs) else entry.name,
                        color = colors.text,
                        style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium),
                    )
                }
            }
        }
    }
}

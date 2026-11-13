package com.wasimaster.wmpass.ui.settings

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
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.Backups
import com.wasimaster.wmpass.system.Destination
import com.wasimaster.wmpass.system.Destinations
import com.wasimaster.wmpass.system.sinkProblem
import com.wasimaster.wmpass.ui.parts.announced
import com.wasimaster.wmpass.ui.help.HelpMark
import com.wasimaster.wmpass.ui.help.helpAnchor
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.ConfirmSheet
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmsuite.settings.WmScreen
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.launch

fun whenShort(ms: Long): String = DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(ms))

@Composable
private fun DestinationRow(place: Destination, tick: Int, busy: Boolean, onOpen: () -> Unit, onBackup: () -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val last = remember(place.id, tick) { Destinations.lastRun(context, place.id) }
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onOpen).padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        GlyphTile(placeIcon(place.kind), if (last?.problem != null) colors.danger else colors.primary, size = 40.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(place.label, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(
                last?.problem ?: last?.let { "Last copy ${whenShort(it.atMs)}" } ?: placeLabel(place.kind),
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
    // Every place with a copy on its way: two at once each keep their spinner, and neither runs twice.
    var busy by remember { mutableStateOf(setOf<String>()) }
    var tick by remember { mutableIntStateOf(0) }
    var every by remember { mutableIntStateOf(Backups.everyDays(context)) }
    var keep by remember { mutableIntStateOf(Backups.keep(context)) }
    var restoring by remember { mutableStateOf<Destination?>(null) }
    var removing by remember { mutableStateOf<Destination?>(null) }
    var message by remember { mutableStateOf<String?>(null) }
    val saveFile = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        if (uri != null) {
            scope.launch {
                // This page's scope has no handler of its own: a failure is said here, not a crash.
                try {
                    val bytes = Pass.bundle()
                    // Off the main thread: the picker's provider may be slow or remote, as a cloud drive's is.
                    kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                        context.contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                            ?: throw java.io.IOException("The backup could not be written there.")
                    }
                    shell.haptics.confirm()
                    message = "The backup is saved. It opens with your vault's password."
                } catch (failure: Exception) {
                    if (failure is kotlinx.coroutines.CancellationException) throw failure
                    shell.haptics.reject()
                    message = failure.message ?: "The backup could not be saved."
                }
            }
        }
    }
    val openFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            scope.launch {
                try {
                    val bytes = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                        context.contentResolver.openInputStream(uri)?.use { it.readBytes() }
                    } ?: error("That file could not be read.")
                    val merged = Pass.mergeIn(bytes)
                    message = if (merged > 0) "Merged into this vault. Anything newer here was kept, and older values moved to history." else "That file is under another key, so it holds some other vault."
                    if (merged > 0) shell.haptics.confirm() else shell.haptics.reject()
                } catch (failure: Exception) {
                    if (failure is kotlinx.coroutines.CancellationException) throw failure
                    shell.haptics.reject()
                    message = failure.message ?: "That file could not be merged."
                }
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
            Column(Modifier.helpAnchor("group:backup.places")) {
            Eyebrow("Where they go") { HelpMark("group.backup.places", "where backups go") }
            GroupCard {
                places.forEachIndexed { index, place ->
                    if (index > 0) Hairline(inset = 70.dp)
                    DestinationRow(
                        place,
                        tick,
                        place.id in busy,
                        onOpen = { editing = place },
                        onBackup = {
                            if (place.id in busy) return@DestinationRow
                            busy = busy + place.id
                            scope.launch {
                                Backups.to(context, place)
                                    .onSuccess { shell.haptics.confirm() }
                                    .onFailure { shell.haptics.reject() }
                                busy = busy - place.id
                                tick++
                            }
                        },
                    )
                }
                if (places.isNotEmpty()) Hairline()
                com.wasimaster.wmpass.ui.parts.NavRow("Add a place", icon = PassIcons.Add, help = "action.add_place") { adding = true }
            }
            }
            Column(Modifier.helpAnchor("group:backup.schedule")) {
            Eyebrow("How often") { HelpMark("action.backup_schedule", "how often") }
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
            Eyebrow("How many to keep") { HelpMark("action.backup_keep", "how many to keep") }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                listOf(3, 7, 14, 30).forEach { count ->
                    Pill(count.toString(), keep == count) {
                        keep = count
                        Backups.setKeep(context, count)
                    }
                }
            }
            }
            Eyebrow("By hand")
            Row(Modifier.helpAnchor("group:backup.manual"), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
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
            busy = busy + place.id
            scope.launch {
                Backups.to(context, place)
                busy = busy - place.id
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
            // A copy of some other vault is said under that copy, and the sheet stays open.
            check(Pass.mergeIn(bytes) > 0) { "This copy is under another key, so it holds some other vault." }
            restoring = null
            message = "The copy from ${place.label} is merged into this vault. Anything newer here was kept, and older values moved to history."
        }
    }
    removing?.let { place ->
        ConfirmSheet(
            "Stop using ${place.label}?",
            "WM Pass forgets this place and its login. Copies already there stay where they are.",
            "Stop using it",
            icon = PassIcons.Close,
            onDismiss = { removing = null },
        ) { Destinations.remove(context, place.id) }
    }
}

/** What can be done with a place: its copies, its details, or letting it go. Sync has no copies to bring back. */
@Composable
internal fun PlaceSheet(place: Destination, onDismiss: () -> Unit, onRestore: (() -> Unit)?, onRemove: () -> Unit) {
    var editing by remember { mutableStateOf(false) }
    if (editing) {
        DestinationSheet(place, onDismiss) { onDismiss() }
        return
    }
    PassSheet(onDismiss, place.label) {
        Text(placeLabel(place.kind), color = palette.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 4.dp, bottom = 6.dp))
        if (onRestore != null) com.wasimaster.wmpass.ui.vault.SheetRow(PassIcons.History, "Bring back a copy", "Merge an older copy into this vault", onClick = onRestore)
        com.wasimaster.wmpass.ui.vault.SheetRow(PassIcons.Edit, "Change the details", "The address, login or folder") { editing = true }
        com.wasimaster.wmpass.ui.vault.SheetRow(PassIcons.Delete, "Stop using it", "For backups and for sync", tint = palette.danger, onClick = onRemove)
    }
}

/**
 * The copies at a place, newest first, to bring one back. A copy that cannot be read, or that
 * [onPicked] refuses by throwing, says why under that copy, and the sheet stays open.
 */
@Composable
fun CopiesSheet(place: Destination, onDismiss: () -> Unit, onPicked: suspend (ByteArray) -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var copies by remember { mutableStateOf<List<SinkEntry>?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    // The copy that last failed, by id, and why.
    var failed by remember { mutableStateOf<Pair<String, String>?>(null) }
    androidx.compose.runtime.LaunchedEffect(place.id) {
        Backups.copies(context, place).onSuccess { copies = it }.onFailure { problem = sinkProblem(it) }
    }
    PassSheet(onDismiss, "Copies at ${place.label}") {
        problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.announced()) }
        val shown = copies
        when {
            shown == null && problem == null -> CircularProgressIndicator(Modifier.size(26.dp), color = colors.primary, strokeWidth = 2.dp)
            shown != null && shown.isEmpty() -> Text("There are no copies here yet.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
            shown != null -> shown.take(30).forEach { entry ->
                Row(
                    Modifier.fillMaxWidth().clickable(enabled = !busy) {
                        busy = true
                        failed = null
                        scope.launch {
                            runCatching { Backups.read(context, place, entry) }
                                .onSuccess { bytes ->
                                    runCatching { onPicked(bytes) }.onFailure {
                                        if (it is kotlinx.coroutines.CancellationException) throw it
                                        failed = entry.id to (it.message ?: "This copy could not be used.")
                                    }
                                }
                                .onFailure { failed = entry.id to sinkProblem(it) }
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
                failed?.takeIf { it.first == entry.id }?.let { (_, why) ->
                    Text(why, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.announced().padding(start = 34.dp, bottom = 12.dp))
                }
            }
        }
    }
}

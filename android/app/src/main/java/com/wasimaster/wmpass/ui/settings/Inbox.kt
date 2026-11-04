package com.wasimaster.wmpass.ui.settings

import android.net.Uri
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.selection.SelectionContainer
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalUriHandler
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.lifecycle.Lifecycle
import androidx.lifecycle.compose.LocalLifecycleOwner
import androidx.lifecycle.repeatOnLifecycle
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.NetAccess
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmpass.ui.vault.SheetRow
import com.wasimaster.wmsuite.settings.WmScreen
import java.io.ByteArrayInputStream
import java.text.DateFormat
import java.util.Calendar
import java.util.Date
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put

/** A throwaway address the vault keeps until it is closed, like a chat kept in a list. */
private class KeptInbox(val id: String, val provider: String, val base: String, val address: String, val label: String, val createdMs: Long) {
    val title: String get() = label.ifEmpty { address }
}

private class MailRow(val id: String, val from: String, val subject: String, val preview: String, val atMs: Long)

private class Mail(val from: String, val to: String, val subject: String, val text: String, val html: String?, val atMs: Long)

private class Provider(val id: String, val name: String, val about: String, val own: Boolean = false)

private val Providers = listOf(
    Provider("mailtm", "mail.tm", "Free and public."),
    Provider("mailgw", "mail.gw", "Free and public, on other domains than mail.tm."),
    Provider("guerrilla", "Guerrilla Mail", "Free and public. It deletes mail after an hour."),
    Provider("custom", "Your own mail.tm server", "Any server that speaks the mail.tm API, at the address you give.", own = true),
    Provider("inbucket", "Inbucket", "A test mail server you run. Every address on its domain lands there.", own = true),
)

private fun providerName(id: String): String = Providers.firstOrNull { it.id == id }?.name ?: id

private const val InboxWarning =
    "Makes throwaway addresses on a mail service, for sign-ups that only need a confirmation mail. The service, and possibly others, can read what arrives. Never use one for an account you want to keep."

private fun JsonObject.text(key: String): String = this[key]?.jsonPrimitive?.contentOrNull.orEmpty()

private fun JsonObject.number(key: String): Long = this[key]?.jsonPrimitive?.longOrNull ?: 0

private suspend fun keptInboxes(): List<KeptInbox> =
    (Pass.net("inbox.sessions", online = false).json as JsonArray).map {
        val row = it.jsonObject
        KeptInbox(row.text("id"), row.text("provider"), row.text("base"), row.text("address"), row.text("label"), row.number("created_ms"))
    }

/** Today's mail by the clock, older mail by the date. */
private fun mailTime(ms: Long): String {
    if (ms <= 0) return ""
    val then = Calendar.getInstance().apply { timeInMillis = ms }
    val now = Calendar.getInstance()
    val today = then.get(Calendar.YEAR) == now.get(Calendar.YEAR) && then.get(Calendar.DAY_OF_YEAR) == now.get(Calendar.DAY_OF_YEAR)
    return if (today) DateFormat.getTimeInstance(DateFormat.SHORT).format(Date(ms)) else DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(ms))
}

/** Numbers that look like a sign-up code: four to eight digits standing alone, and not a year. */
private fun codesIn(mail: Mail): List<String> {
    val pattern = Regex("(?<![\\d.,:/-])\\d{4,8}(?![\\d.,:/-])")
    return (pattern.findAll(mail.subject) + pattern.findAll(mail.text))
        .map { it.value }
        .filterNot { it.length == 4 && it.toInt() in 1900..2100 }
        .distinct()
        .take(3)
        .toList()
}

// ---- the list of addresses ----

@Composable
fun InboxScreen() {
    val colors = palette
    val shell = LocalShell.current
    var kept by remember { mutableStateOf<List<KeptInbox>?>(null) }
    var making by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) { kept = runCatching { keptInboxes() }.getOrDefault(emptyList()) }
    Page("Disposable inbox") {
        NetGate(InboxWarning) {
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Text(
                    "Each address stays here, with its mail, until you close it. Good for one-off sign-ups, nothing more.",
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.padding(horizontal = 6.dp, vertical = 4.dp),
                )
                PrimaryButton("New address", Modifier.fillMaxWidth(), icon = PassIcons.Add) { making = true }
                val list = kept
                when {
                    list == null -> Unit
                    list.isEmpty() -> Text(
                        "No addresses yet.",
                        color = colors.textLow,
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.padding(horizontal = 6.dp, vertical = 8.dp),
                    )
                    else -> {
                        Eyebrow(if (list.size == 1) "1 address" else "${list.size} addresses")
                        GroupCard {
                            list.forEachIndexed { index, inbox ->
                                if (index > 0) Hairline(inset = 70.dp)
                                Row(
                                    Modifier.fillMaxWidth().clickable { shell.nav.navigate("settings/inbox/${inbox.id}") }.padding(start = 16.dp, end = 12.dp, top = 12.dp, bottom = 12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    GlyphTile(if (Providers.firstOrNull { it.id == inbox.provider }?.own == true) PassIcons.Dns else PassIcons.Inbox, colors.primary)
                                    Spacer(Modifier.width(14.dp))
                                    Column(Modifier.weight(1f)) {
                                        Text(inbox.title, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                                        Text(
                                            listOfNotNull(inbox.address.takeIf { inbox.label.isNotEmpty() }, providerName(inbox.provider), mailTime(inbox.createdMs).takeIf { it.isNotEmpty() }).joinToString("  ·  "),
                                            color = colors.textLow,
                                            style = MaterialTheme.typography.bodySmall,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                        )
                                    }
                                    Icon(PassIcons.ChevronRight, null, tint = colors.textLow, modifier = Modifier.size(20.dp))
                                }
                            }
                        }
                    }
                }
                Spacer(Modifier.height(30.dp))
            }
        }
    }
    if (making) {
        NewInboxSheet({ making = false }) { id ->
            making = false
            shell.nav.navigate("settings/inbox/$id")
        }
    }
}

@Composable
private fun NewInboxSheet(onDismiss: () -> Unit, onMade: (String) -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var provider by remember { mutableStateOf("mailtm") }
    val label = rememberTextFieldState()
    val base = rememberTextFieldState()
    val domain = rememberTextFieldState()
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    // What the service said shows under the chosen service, or under the server for one's own.
    PassSheet(onDismiss, "New address") {
        Providers.forEach { choice ->
            SheetRow(if (choice.own) PassIcons.Dns else PassIcons.Inbox, choice.name, choice.about, selected = provider == choice.id) {
                provider = choice.id
                problem = null
            }
            if (!choice.own && provider == choice.id) {
                problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 60.dp, bottom = 6.dp)) }
            }
        }
        val own = Providers.first { it.id == provider }.own
        AnimatedVisibility(own) {
            Column(Modifier.padding(top = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                SheetField(base, "Server, like https://mail.example.com")
                if (provider == "inbucket") SheetField(domain, "The domain it takes mail for")
                problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 4.dp)) }
                Text(
                    "Plain http works only for this phone and Tailscale addresses.",
                    color = colors.textLow,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 4.dp),
                )
            }
        }
        Spacer(Modifier.height(8.dp))
        SheetField(label, "What it is for (optional)")
        Spacer(Modifier.height(14.dp))
        val ready = !own || (base.text.isNotBlank() && (provider != "inbucket" || domain.text.isNotBlank()))
        PrimaryButton(if (busy) "Making the address…" else "Make the address", Modifier.fillMaxWidth(), enabled = ready && !busy, icon = PassIcons.Inbox) {
            busy = true
            problem = null
            scope.launch {
                try {
                    val reply = Pass.net("inbox.open", online = NetAccess.allowed(context)) {
                        put("provider", provider)
                        put("base", base.text.toString().trim().trimEnd('/'))
                        put("domain", domain.text.toString().trim())
                        put("label", label.text.toString().trim())
                    }
                    Pass.flush()
                    onMade(reply.obj.text("id"))
                } catch (failure: NativeFailure) {
                    problem = failure.message
                } finally {
                    busy = false
                }
            }
        }
    }
}

// ---- one address ----

@Composable
fun InboxSessionScreen(id: String) {
    val shell = LocalShell.current
    var inbox by remember { mutableStateOf<KeptInbox?>(null) }
    var renaming by remember { mutableStateOf(false) }
    var closing by remember { mutableStateOf(false) }
    var reload by remember { mutableIntStateOf(0) }
    LaunchedEffect(id, reload) { inbox = runCatching { keptInboxes().firstOrNull { it.id == id } }.getOrNull() }
    WmScreen(
        inbox?.title ?: "Inbox",
        route = null,
        onBack = { shell.nav.popBackStack() },
        actions = {
            if (inbox != null) {
                RowIconButton(PassIcons.Edit, "Name this address") { renaming = true }
                RowIconButton(PassIcons.Delete, "Close this address") { closing = true }
            }
        },
    ) {
        NetGate(InboxWarning) {
            inbox?.let { Mailbox(it) }
        }
    }
    val current = inbox
    if (renaming && current != null) {
        RenameInboxSheet(current, { renaming = false }) {
            renaming = false
            reload++
        }
    }
    if (closing && current != null) {
        CloseInboxSheet(current, { closing = false }) {
            closing = false
            shell.nav.popBackStack()
        }
    }
}

@Composable
private fun Mailbox(inbox: KeptInbox) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val lifecycle = LocalLifecycleOwner.current.lifecycle
    var mail by remember { mutableStateOf<List<MailRow>?>(null) }
    // What was there when the page opened, so what comes in afterwards can be marked new.
    var seen by remember { mutableStateOf<Set<String>?>(null) }
    var looking by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    var again by remember { mutableIntStateOf(0) }
    LaunchedEffect(inbox.id, again) {
        lifecycle.repeatOnLifecycle(Lifecycle.State.STARTED) {
            while (true) {
                looking = true
                try {
                    val rows = (Pass.net("inbox.list", online = NetAccess.allowed(context)) { put("id", inbox.id) }.json as JsonArray).map {
                        val row = it.jsonObject
                        MailRow(row.text("id"), row.text("from"), row.text("subject"), row.text("preview"), row.number("at_ms"))
                    }
                    if (seen == null) seen = rows.map { it.id }.toSet()
                    mail = rows
                    problem = null
                } catch (failure: NativeFailure) {
                    problem = failure.message
                }
                looking = false
                delay(8000)
            }
        }
    }
    Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Row(
            Modifier.fillMaxWidth().clip(Shapes.cardSmall).background(colors.surface1).rim(Shapes.cardSmall, colors).padding(start = 18.dp, end = 10.dp, top = 10.dp, bottom = 10.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(inbox.address, color = colors.text, style = PassType.secretSmall)
                Text(providerName(inbox.provider), color = colors.textLow, style = MaterialTheme.typography.bodySmall)
            }
            RowIconButton(PassIcons.ContentCopy, "Copy the address") { shell.copyPlain("Address", inbox.address) }
        }
        Eyebrow("Arrived") {
            com.wasimaster.wmpass.ui.help.HelpMark("group.inbox.arrived", "mail that arrived")
            Box(Modifier.size(40.dp), contentAlignment = Alignment.Center) {
                if (looking) {
                    CircularProgressIndicator(Modifier.size(18.dp), color = colors.primary, strokeWidth = 2.dp)
                } else {
                    RowIconButton(PassIcons.Refresh, "Look for new mail") { again++ }
                }
            }
        }
        problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp)) }
        val rows = mail
        when {
            rows == null -> Unit
            rows.isEmpty() -> Text(
                "Nothing yet. This page looks for new mail every few seconds while it is open.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
            else -> GroupCard(Modifier.animateContentSize()) {
                rows.forEachIndexed { index, row ->
                    if (index > 0) Hairline()
                    val fresh = seen?.contains(row.id) == false
                    Row(
                        Modifier.fillMaxWidth().clickable { shell.nav.navigate("settings/inbox/${inbox.id}/${Uri.encode(row.id)}") }.padding(horizontal = 18.dp, vertical = 12.dp),
                        verticalAlignment = Alignment.Top,
                    ) {
                        Column(Modifier.weight(1f)) {
                            Row(verticalAlignment = Alignment.CenterVertically) {
                                if (fresh) {
                                    Box(Modifier.size(8.dp).clip(CircleShape).background(colors.primary))
                                    Spacer(Modifier.width(8.dp))
                                }
                                Text(row.from.ifEmpty { "Unknown sender" }, color = colors.text, style = MaterialTheme.typography.bodyMedium.copy(fontWeight = FontWeight.SemiBold), maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                                Text(mailTime(row.atMs), color = colors.textLow, style = MaterialTheme.typography.labelMedium)
                            }
                            Text(row.subject.ifEmpty { "No subject" }, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (row.preview.isNotBlank()) {
                                Text(row.preview, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis)
                            }
                        }
                    }
                }
            }
        }
        Spacer(Modifier.height(30.dp))
    }
}

@Composable
private fun RenameInboxSheet(inbox: KeptInbox, onDismiss: () -> Unit, onDone: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val label = rememberTextFieldState(inbox.label)
    PassSheet(onDismiss, "Name this address") {
        SheetField(label, "What it is for")
        Spacer(Modifier.height(14.dp))
        PrimaryButton("Save", Modifier.fillMaxWidth()) {
            scope.launch {
                runCatching {
                    Pass.net("inbox.rename", online = NetAccess.allowed(context)) {
                        put("id", inbox.id)
                        put("label", label.text.toString().trim())
                    }
                    Pass.flush()
                }
                onDone()
            }
        }
    }
}

@Composable
private fun CloseInboxSheet(inbox: KeptInbox, onDismiss: () -> Unit, onClosed: () -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var busy by remember { mutableStateOf(false) }
    fun close(delete: Boolean) {
        busy = true
        scope.launch {
            runCatching {
                Pass.net("inbox.close", online = NetAccess.allowed(context)) {
                    put("id", inbox.id)
                    put("delete", delete)
                }
                Pass.flush()
            }
            busy = false
            onClosed()
        }
    }
    PassSheet(onDismiss, "Close this address?") {
        val deletes = inbox.provider != "guerrilla"
        Text(
            if (deletes) {
                "WM Pass forgets ${inbox.address} and its mail. Anything saved to the vault stays."
            } else {
                "WM Pass forgets ${inbox.address}. Guerrilla Mail clears it on its own within the hour. Anything saved to the vault stays."
            },
            color = colors.textMid,
            style = MaterialTheme.typography.bodyLarge,
            modifier = Modifier.padding(start = 4.dp, end = 4.dp, bottom = 14.dp),
        )
        if (deletes) {
            PrimaryButton("Close and delete it on ${providerName(inbox.provider)}", Modifier.fillMaxWidth(), enabled = !busy, icon = PassIcons.Delete) { close(true) }
            Spacer(Modifier.height(10.dp))
            QuietButton("Only forget it here", Modifier.fillMaxWidth()) { if (!busy) close(false) }
        } else {
            PrimaryButton("Close", Modifier.fillMaxWidth(), enabled = !busy, icon = PassIcons.Delete) { close(false) }
        }
    }
}

// ---- one mail ----

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun MailScreen(id: String, message: String) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    var mail by remember { mutableStateOf<Mail?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var pictures by remember { mutableStateOf(false) }
    // A mail with HTML is saved as a page that opens in any browser, the rest as plain text.
    val saveFile = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.CreateDocument("*/*")) { uri ->
        val mail = mail
        if (uri != null && mail != null) {
            runCatching {
                context.contentResolver.openOutputStream(uri)?.use { out -> out.write(mailFile(mail).toByteArray()) }
                shell.notice("Saved")
            }.onFailure { shell.notice("That file could not be written") }
        }
    }
    LaunchedEffect(id, message) {
        try {
            val body = Pass.net("inbox.read", online = NetAccess.allowed(context)) {
                put("id", id)
                put("message", message)
            }.obj
            mail = Mail(body.text("from"), body.text("to"), body.text("subject"), body.text("text"), body["html"]?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotBlank() }, body.number("at_ms"))
        } catch (failure: NativeFailure) {
            problem = failure.message
        }
    }
    val open = mail
    WmScreen(
        open?.subject?.ifEmpty { "No subject" } ?: "Mail",
        route = null,
        onBack = { shell.nav.popBackStack() },
        actions = {
            if (open != null) {
                RowIconButton(PassIcons.Share, "Forward or share") {
                    // Android's share sheet hands it to a mail app, or anywhere else, as a forward.
                    val send = android.content.Intent(android.content.Intent.ACTION_SEND)
                        .setType("text/plain")
                        .putExtra(android.content.Intent.EXTRA_SUBJECT, "Fwd: " + open.subject)
                        .putExtra(android.content.Intent.EXTRA_TEXT, mailAsText(open))
                    open.html?.let { send.putExtra(android.content.Intent.EXTRA_HTML_TEXT, it) }
                    runCatching { context.startActivity(android.content.Intent.createChooser(send, "Forward this mail")) }
                }
                Box {
                    var menu by remember { mutableStateOf(false) }
                    RowIconButton(PassIcons.MoreVert, "More") { menu = true }
                    androidx.compose.material3.DropdownMenu(menu, { menu = false }) {
                        androidx.compose.material3.DropdownMenuItem(
                            text = { Text("Copy the text") },
                            leadingIcon = { Icon(PassIcons.ContentCopy, null) },
                            onClick = {
                                menu = false
                                shell.copyPlain("Mail", open.text)
                            },
                        )
                        androidx.compose.material3.DropdownMenuItem(
                            text = { Text("Save as a file") },
                            leadingIcon = { Icon(PassIcons.Download, null) },
                            onClick = {
                                menu = false
                                saveFile.launch(mailFileName(open))
                            },
                        )
                    }
                }
                RowIconButton(PassIcons.StickyNote2, "Save to the vault as a note") {
                    shell.pendingTitle = open.subject.ifEmpty { "Mail from ${open.from}" }
                    shell.pendingNote = mailAsText(open)
                    shell.nav.navigate("new/note")
                }
            }
        },
    ) {
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp)) }
            if (open == null) {
                if (problem == null) CircularProgressIndicator(Modifier.padding(24.dp).size(24.dp), color = colors.primary, strokeWidth = 2.dp)
                return@Column
            }
            Column(Modifier.padding(horizontal = 6.dp)) {
                Text(open.from.ifEmpty { "Unknown sender" }, color = colors.text, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold))
                if (open.to.isNotEmpty()) Text("To ${open.to}", color = colors.textMid, style = MaterialTheme.typography.bodySmall)
                if (open.atMs > 0) {
                    Text(DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(open.atMs)), color = colors.textLow, style = MaterialTheme.typography.bodySmall)
                }
            }
            val codes = remember(open) { codesIn(open) }
            if (codes.isNotEmpty()) {
                FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    codes.forEach { code ->
                        QuietButton("Copy $code", icon = PassIcons.ContentCopy) { shell.copyPlain("Code", code) }
                    }
                }
            }
            val html = open.html
            if (html == null) {
                SelectionContainer {
                    Text(open.text.ifEmpty { "This mail has no text." }, color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(horizontal = 6.dp))
                }
            } else {
                if (!pictures) {
                    Row(Modifier.fillMaxWidth().padding(horizontal = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            "Pictures are not loaded, so the sender cannot see that you opened this.",
                            color = colors.textLow,
                            style = MaterialTheme.typography.bodySmall,
                            modifier = Modifier.weight(1f),
                        )
                        QuietButton("Load", icon = PassIcons.Image) { pictures = true }
                    }
                }
                MailBody(html, pictures)
            }
            Spacer(Modifier.height(30.dp))
        }
    }
}

private fun mailDate(mail: Mail): String =
    if (mail.atMs > 0) DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(mail.atMs)) else ""

/** Headers, then the text: for the vault, the clipboard and a forward. */
private fun mailAsText(mail: Mail): String = buildString {
    appendLine("From: ${mail.from}")
    if (mail.to.isNotEmpty()) appendLine("To: ${mail.to}")
    if (mail.atMs > 0) appendLine("Date: ${mailDate(mail)}")
    if (mail.subject.isNotEmpty()) appendLine("Subject: ${mail.subject}")
    appendLine()
    append(mail.text)
}

private fun mailFileName(mail: Mail): String {
    val stem = mail.subject.filter { it.isLetterOrDigit() || it in " -_" }.trim().take(60).ifEmpty { "mail" }
    return if (mail.html != null) "$stem.html" else "$stem.txt"
}

private fun escapeHtml(text: String) = text.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\"", "&quot;")

/** The file a mail is saved as: its HTML under a header block, or its text. */
private fun mailFile(mail: Mail): String {
    val html = mail.html ?: return mailAsText(mail)
    val head = listOfNotNull(
        "From" to mail.from,
        ("To" to mail.to).takeIf { mail.to.isNotEmpty() },
        ("Date" to mailDate(mail)).takeIf { mail.atMs > 0 },
        "Subject" to mail.subject,
    ).joinToString("") { (name, value) -> "<div><b>$name:</b> ${escapeHtml(value)}</div>" }
    return "<!doctype html><meta charset=\"utf-8\"><title>${escapeHtml(mail.subject)}</title>" +
        "<div style=\"font:14px sans-serif;border-bottom:1px solid #ccc;padding:8px 0;margin-bottom:12px\">$head</div>$html"
}

/**
 * Mail is written for a desktop's width. These make it read on a phone the way a mail app shows
 * it: the page takes the screen's width, and pictures and tables shrink to fit inside it.
 */
private const val PhoneFit =
    "<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">" +
        "<style>html,body{margin:0;padding:8px;word-wrap:break-word;overflow-wrap:anywhere}" +
        "img{max-width:100% !important;height:auto !important}" +
        "table{max-width:100% !important}td,th{word-break:break-word}pre{white-space:pre-wrap}</style>"

private fun fitted(html: String): String {
    val head = Regex("<head[^>]*>", RegexOption.IGNORE_CASE).find(html)
    return if (head != null) html.substring(0, head.range.last + 1) + PhoneFit + html.substring(head.range.last + 1) else PhoneFit + html
}

/**
 * A mail's HTML, drawn the way a mail app draws it, minus what lets a sender act: no script runs,
 * nothing on the phone can be reached, and nothing loads from the network until [pictures].
 * A tapped link opens in the browser.
 */
@Composable
private fun MailBody(html: String, pictures: Boolean) {
    val links = LocalUriHandler.current
    val client = remember { MailClient { url -> runCatching { links.openUri(url) } } }
    AndroidView(
        factory = { context ->
            WebView(context).apply {
                setBackgroundColor(android.graphics.Color.WHITE)
                isVerticalScrollBarEnabled = false
                settings.apply {
                    javaScriptEnabled = false
                    allowFileAccess = false
                    allowContentAccess = false
                    domStorageEnabled = false
                    useWideViewPort = true
                    loadWithOverviewMode = true
                    builtInZoomControls = true
                    displayZoomControls = false
                    blockNetworkLoads = true
                }
                webViewClient = client
            }
        },
        update = { web ->
            val key = html.hashCode() to pictures
            if (web.tag != key) {
                web.tag = key
                client.pictures = pictures
                runCatching {
                    web.settings.blockNetworkLoads = !pictures
                    web.settings.blockNetworkImage = !pictures
                }
                web.loadDataWithBaseURL(null, fitted(html), "text/html", "utf-8", null)
            }
        },
        onRelease = { web -> web.destroy() },
        modifier = Modifier.fillMaxWidth().clip(Shapes.cardSmall).background(Color.White),
    )
}

/** Sends taps on links to the browser and lets nothing load but https pictures and styles, once allowed. */
private class MailClient(private val open: (String) -> Unit) : WebViewClient() {
    // Read on WebView's own loading thread, where its settings may not be touched.
    @Volatile
    var pictures = false

    override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean {
        val url = request.url
        if (url.scheme in setOf("https", "http", "mailto")) open(url.toString())
        return true
    }

    override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
        if (pictures && request.url.scheme == "https") null else WebResourceResponse("text/plain", "utf-8", ByteArrayInputStream(ByteArray(0)))
}

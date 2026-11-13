package com.wasimaster.wmpass.ui.settings

import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.heading
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.foundation.background
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.clearText
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.BuildConfig
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.system.NetAccess
import com.wasimaster.wmpass.ui.parts.announced
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.BreachCounts
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmsuite.settings.WmScreen
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put

/**
 * In front of any feature that leaves the phone: what it sends and to whom, and a choice. Until
 * it is made, [content] does not run and nothing goes anywhere.
 */
@Composable
fun NetGate(what: String, content: @Composable () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    if (BuildConfig.OFFLINE) {
        Text("This is the offline build. It has no way to reach a network, so this feature is not available.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.padding(20.dp))
        return
    }
    if (shell.prefs.networkAllowed) {
        content()
        return
    }
    Column(Modifier.padding(horizontal = 20.dp, vertical = 12.dp), verticalArrangement = Arrangement.spacedBy(14.dp)) {
        GlyphTile(PassIcons.Language, colors.primary, size = 52.dp)
        Text("This needs the network", color = colors.text, style = MaterialTheme.typography.titleLarge, modifier = Modifier.semantics { heading() })
        Text(what, color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
        Text("WM Pass stays offline until you allow this. You can turn it off again in Settings.", color = colors.textLow, style = MaterialTheme.typography.bodyMedium)
        PrimaryButton("Allow network features", Modifier.fillMaxWidth()) { shell.prefs.updateNetworkAllowed(true) }
    }
}

@Composable
internal fun Page(title: String, content: @Composable () -> Unit) {
    val shell = LocalShell.current
    val route = when (title) {
        "Breaches" -> "settings/breaches"
        "Email aliases" -> "settings/aliases"
        "Disposable inbox" -> "settings/inbox"
        "Username checker" -> "settings/usernames"
        else -> null
    }
    WmScreen(title, route = route, onBack = { shell.nav.popBackStack() }) { content() }
}

// ---- breaches ----

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun BreachScreen() {
    val colors = palette
    val context = LocalContext.current
    val entries by Pass.entries.collectAsStateWithLifecycle()
    val progress by com.wasimaster.wmpass.system.BreachCheck.progress.collectAsStateWithLifecycle()
    val found by BreachCounts.counts.collectAsStateWithLifecycle()
    val running = progress.running
    var source by androidx.compose.runtime.saveable.rememberSaveable { mutableStateOf("hibp") }
    var byCount by androidx.compose.runtime.saveable.rememberSaveable { mutableStateOf(true) }
    // Asked once, as the first check starts; the check runs whatever the answer.
    val askNotify = rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.RequestPermission()) { }
    Page("Breaches") {
        NetGate("Checks your passwords against Have I Been Pwned or XposedOrNot. Only the start of each password's hash is sent: five characters of SHA-1, or ten of Keccak-512. Neither service can tell which password you asked about. Checking an email address sends that address.") {
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Eyebrow("Your passwords")
                val checkable = entries.filter { it.hasPassword }
                val last = remember(progress.running, progress.done) { com.wasimaster.wmpass.system.BreachCheck.last(context) }
                Text(
                    when {
                        checkable.isEmpty() -> "No item in this vault holds a password yet, so there is nothing to check."
                        running -> "Checked ${progress.done} of ${progress.total} with ${com.wasimaster.wmpass.system.BreachCheck.sourceName(progress.source)}. You can leave this page; the notification shows how far it has got."
                        found.size == 1 -> "One of them has appeared in leaks. It now leads the list on the Health page."
                        found.isNotEmpty() -> "${found.size} of them have appeared in leaks. They now lead the list on the Health page."
                        else -> "See whether any password in this vault has appeared in a known leak."
                    },
                    color = if (found.isNotEmpty() && !running) colors.danger else colors.textMid,
                    style = MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.padding(horizontal = 6.dp),
                )
                if (last != null && !running) {
                    Text(
                        "Last checked ${com.wasimaster.wmpass.ui.vault.whenText(last.atMs)} at ${java.text.DateFormat.getTimeInstance(java.text.DateFormat.SHORT).format(java.util.Date(last.atMs))} with ${com.wasimaster.wmpass.system.BreachCheck.sourceName(last.source)}: ${last.found} of ${last.checked} found. What it found is kept only until WM Pass closes.",
                        color = colors.textLow,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(horizontal = 6.dp),
                    )
                }
                AnimatedVisibility(running) {
                    LinearProgressIndicator(
                        progress = { if (progress.total == 0) 0f else progress.done.toFloat() / progress.total },
                        color = colors.primary,
                        trackColor = colors.surface3,
                        modifier = Modifier.fillMaxWidth().clip(CircleShape).padding(vertical = 4.dp),
                    )
                }
                if (checkable.isNotEmpty() && !running) {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        Pill("Have I Been Pwned", source == "hibp") { source = "hibp" }
                        Pill("XposedOrNot", source == "xon") { source = "xon" }
                    }
                    PrimaryButton(if (last == null) "Check every password" else "Check again", Modifier.fillMaxWidth(), icon = PassIcons.GppGood) {
                        if (android.os.Build.VERSION.SDK_INT >= 33 &&
                            context.checkSelfPermission(android.Manifest.permission.POST_NOTIFICATIONS) != android.content.pm.PackageManager.PERMISSION_GRANTED
                        ) {
                            askNotify.launch(android.Manifest.permission.POST_NOTIFICATIONS)
                        }
                        com.wasimaster.wmpass.system.BreachCheck.start(context, checkable.map { it.id }, source)
                    }
                }
                if (running) QuietButton("Stop", Modifier.fillMaxWidth(), PassIcons.Close) { com.wasimaster.wmpass.system.BreachCheck.stop() }
                progress.problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.announced()) }
                val breached = entries.filter { it.id in found }
                if (breached.isNotEmpty()) {
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Pill("Most seen", byCount, icon = PassIcons.Sort) { byCount = true }
                        Pill("Name", !byCount, icon = PassIcons.Abc) { byCount = false }
                    }
                    val ordered = if (byCount) breached.sortedByDescending { found[it.id] ?: 0 } else breached.sortedBy { it.title.lowercase() }
                    GroupCard {
                        ordered.forEachIndexed { index, entry ->
                            if (index > 0) Hairline(inset = 70.dp)
                            EntryLineCounted(entry, found[entry.id] ?: 0)
                        }
                    }
                }
                EmailChecks(entries)
                Spacer(Modifier.height(30.dp))
            }
        }
    }
}

@Composable
private fun EntryLineCounted(entry: Summary, count: Long) {
    val colors = palette
    val shell = LocalShell.current
    Row(
        Modifier.fillMaxWidth().clickable { shell.open(entry.id) }.padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SiteTile(entry.title, entry.site, entry.kind, size = 40.dp, label = entry.color, icon = entry.icon, app = entry.app)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(entry.title, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
            Text(if (count == 1L) "Seen once" else "Seen %,d times".format(count), color = colors.danger, style = MaterialTheme.typography.bodySmall)
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun EmailChecks(entries: List<Summary>) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val emails = remember(entries) { entries.map { it.subtitle.trim().lowercase() }.filter { '@' in it }.distinct() }
    var source by remember { mutableStateOf("leakcheck") }
    // Not saved with the activity's state, and typed with no suggestions: an API key is a secret.
    val key = remember { TextFieldState() }
    // By address: two checks can run at once, and each row keeps its own spinner until its own ends.
    val busy = remember { androidx.compose.runtime.mutableStateListOf<String>() }
    val results = remember { mutableStateMapOf<String, String>() }
    // Two checks run at once with a typed key keep it in the vault once, not twice.
    var keySaving by remember { mutableStateOf(false) }
    if (emails.isEmpty()) return
    Eyebrow("Your email addresses") { com.wasimaster.wmpass.ui.help.HelpMark("group.breaches.emails", "your email addresses") }
    Text(
        when (source) {
            "hibp" -> "Have I Been Pwned needs an API key of your own."
            "comb" -> "Asks Proxynova for your address, then compares your own passwords here on the phone. Only yes or no comes out: leaked text is never shown or kept."
            else -> "LeakCheck's free check names the services an address appeared in."
        },
        color = colors.textMid,
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.padding(horizontal = 6.dp),
    )
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        Pill("LeakCheck", source == "leakcheck") { source = "leakcheck" }
        Pill("Have I Been Pwned", source == "hibp") { source = "hibp" }
        Pill("Passwords in COMB", source == "comb") { source = "comb" }
    }
    val all by Pass.entries.collectAsStateWithLifecycle()
    val hibpKept = all.any { it.kind == "api" && it.title == HIBP_KEY }
    if (source == "hibp") {
        if (hibpKept) {
            Text("Using the key kept in the vault as “$HIBP_KEY”.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp))
        } else {
            SheetField(key, "API key", keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(keyboardType = androidx.compose.ui.text.input.KeyboardType.Password, autoCorrectEnabled = false))
            Text("After the first check the key is kept in the vault, so your other devices have it too.", color = colors.textMid, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 6.dp))
        }
    }
    GroupCard {
        emails.forEachIndexed { index, email ->
            if (index > 0) Hairline()
            Row(Modifier.fillMaxWidth().padding(start = 18.dp, end = 12.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(email, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
                    results[email]?.let { Text(it, color = colors.textMid, style = MaterialTheme.typography.bodySmall) }
                }
                if (email in busy) {
                    CircularProgressIndicator(Modifier.size(22.dp), color = colors.primary, strokeWidth = 2.dp)
                } else {
                    RowIconButton(PassIcons.Search, "Check $email") {
                        if (email in busy) return@RowIconButton
                        busy += email
                        scope.launch {
                            val online = NetAccess.allowed(context)
                            results[email] = try {
                                if (source == "comb") {
                                    val reply = Pass.net("breach.comb", online) { put("email", email) }
                                    val matched = reply.obj["matched"]!!.jsonArray.size
                                    val lines = reply.obj["lines"]!!.jsonPrimitive.long
                                    when {
                                        matched == 1 -> "One of your passwords for it is in this leak. Change it."
                                        matched > 0 -> "$matched of your passwords for it are in this leak. Change them."
                                        lines > 0 -> "The address is listed, with passwords you no longer use."
                                        else -> "Not listed."
                                    }
                                } else {
                                    val typed = key.text.toString().trim()
                                    val secret = when {
                                        source != "hibp" -> null
                                        hibpKept -> serviceKey(HIBP_KEY)?.toByteArray()
                                        else -> typed.toByteArray()
                                    }
                                    val rows = Engine.net("breach.account", online = online, secret = secret) {
                                        put("source", source)
                                        put("email", email)
                                    }.json as JsonArray
                                    secret?.fill(0)
                                    if (source == "hibp" && !hibpKept && typed.isNotEmpty() && !keySaving) {
                                        keySaving = true
                                        try {
                                            saveServiceKey(HIBP_KEY, typed)
                                        } catch (failure: Exception) {
                                            keySaving = false
                                            throw failure
                                        }
                                        key.clearText()
                                    }
                                    if (rows.isEmpty()) "Not listed." else "In ${rows.size}: " + rows.take(5).joinToString(", ") { it.jsonObject["name"]!!.jsonPrimitive.content }
                                }
                            } catch (failure: Exception) {
                                // This page's scope has no handler: a key that will not open, or an
                                // answer missing a field, is said on the row rather than closing the app.
                                if (failure is kotlinx.coroutines.CancellationException) throw failure
                                failure.message ?: "The check did not work."
                            }
                            busy -= email
                        }
                    }
                }
            }
        }
    }
}

/** The key is an item in the vault, so it follows the vault to every device. */
private const val HIBP_KEY = "Have I Been Pwned API key"

// ---- aliases ----

private data class AliasService(val id: String, val name: String, val note: String, val keyTitle: String)

private val AliasServices = listOf(
    AliasService("simplelogin", "SimpleLogin", "Create an API key under API Keys in your SimpleLogin account.", "SimpleLogin API key"),
    AliasService("addy", "addy.io", "Create an API key in addy.io's settings.", "addy.io API key"),
    AliasService("firefox-relay", "Firefox Relay", "Copy the API key from your Firefox Relay settings.", "Firefox Relay API key"),
    AliasService("duckduckgo", "DuckDuckGo", "Experimental. DuckDuckGo has no public API, so this uses the private one its extension calls. Copy the token from your Email Protection dashboard. It may stop working.", "DuckDuckGo Email token"),
)

/** Finds the key for a service in the vault itself: it is a secret like any other. */
private suspend fun serviceKey(title: String): String? {
    val entry = Pass.entries.value.firstOrNull { it.kind == "api" && it.title == title } ?: return null
    return Pass.reveal(entry.id, "key").consumeText()
}

private suspend fun saveServiceKey(title: String, value: String) {
    val id = Pass.change("entry.create") {
        put("kind", "api")
        put("title", title)
    }.obj["id"]!!.jsonPrimitive.content
    val bytes = value.toByteArray()
    Pass.change("entry.set_secret", secret = bytes) {
        put("id", id)
        put("field", "key")
    }
    bytes.fill(0)
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
fun AliasScreen() {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var service by remember { mutableStateOf(AliasServices.first()) }
    var hasKey by remember { mutableStateOf(false) }
    // Not saved with the activity's state, and typed with no suggestions: an API key is a secret.
    val keyField = remember { TextFieldState() }
    val note = rememberTextFieldState()
    var aliases by remember { mutableStateOf<List<JsonObject>>(emptyList()) }
    var problem by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val entries by Pass.entries.collectAsStateWithLifecycle()
    suspend fun refresh() {
        problem = null
        // Run from an effect with no handler: a key that will not open is said here, not a crash.
        val key = try {
            serviceKey(service.keyTitle)
        } catch (failure: Exception) {
            if (failure is kotlinx.coroutines.CancellationException) throw failure
            problem = failure.message ?: "The key could not be read from the vault."
            null
        }
        hasKey = key != null
        aliases = emptyList()
        if (key != null && service.id != "duckduckgo") {
            val bytes = key.toByteArray()
            try {
                aliases = (
                    Engine.net("alias.list", online = NetAccess.allowed(context), secret = bytes) { put("provider", service.id) }.json as JsonArray
                    ).map { it.jsonObject }
            } catch (failure: Exception) {
                // Run from an effect with no handler: any failure is said here, not a crash.
                if (failure is kotlinx.coroutines.CancellationException) throw failure
                problem = failure.message ?: "The aliases could not be listed."
            } finally {
                bytes.fill(0)
            }
        }
    }
    LaunchedEffect(service, entries.size) { if (shell.prefs.networkAllowed) refresh() }
    Page("Email aliases") {
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            AddressTricks()
            NetGate("Alias services create addresses that forward to you, so a site never sees your real one. The service you pick receives your API key and the note you write.") {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Eyebrow("Forwarding services") { com.wasimaster.wmpass.ui.help.HelpMark("group.aliases.services", "forwarding services") }
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        AliasServices.forEach { option -> Pill(option.name, option == service) { service = option } }
                    }
                    if (!hasKey) {
                        Text(service.note, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp))
                        SheetField(keyField, "API key", keyboardOptions = androidx.compose.foundation.text.KeyboardOptions(keyboardType = androidx.compose.ui.text.input.KeyboardType.Password, autoCorrectEnabled = false))
                        PrimaryButton("Keep this key in the vault", Modifier.fillMaxWidth(), enabled = keyField.text.isNotBlank() && !busy) {
                            // Once: a second tap while the first saves would keep the key twice.
                            if (busy) return@PrimaryButton
                            busy = true
                            // A save that fails is said at the bottom of the screen, rather than closing the app.
                            shell.launch {
                                try {
                                    saveServiceKey(service.keyTitle, keyField.text.toString().trim())
                                    // Kept in the vault now, so not on screen as well.
                                    keyField.clearText()
                                    refresh()
                                } finally {
                                    busy = false
                                }
                            }
                        }
                    } else {
                        SheetField(note, "What is it for?")
                        PrimaryButton(if (busy) "Making it…" else "Make a new alias", Modifier.fillMaxWidth(), enabled = !busy, icon = PassIcons.AlternateEmail) {
                            busy = true
                            scope.launch {
                                try {
                                    val key = serviceKey(service.keyTitle).orEmpty().toByteArray()
                                    val made = Engine.net("alias.create", online = NetAccess.allowed(context), secret = key) {
                                        put("provider", service.id)
                                        put("note", note.text.toString())
                                    }.obj
                                    key.fill(0)
                                    shell.copyPlain("Alias", made["address"]!!.jsonPrimitive.content)
                                    refresh()
                                } catch (failure: Exception) {
                                    if (failure is kotlinx.coroutines.CancellationException) throw failure
                                    problem = failure.message ?: "The alias could not be made."
                                }
                                busy = false
                            }
                        }
                    }
                    problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.announced()) }
                    if (aliases.isNotEmpty()) {
                        GroupCard {
                            aliases.forEachIndexed { index, alias ->
                                if (index > 0) Hairline()
                                AliasRow(alias, service.id) { scope.launch { refresh() } }
                            }
                        }
                    }
                }
            }
            Spacer(Modifier.height(30.dp))
        }
    }
}

@Composable
private fun AliasRow(alias: JsonObject, provider: String, onChange: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val title = AliasServices.first { it.id == provider }.keyTitle
    val address = alias["address"]!!.jsonPrimitive.content
    val id = alias["id"]!!.jsonPrimitive.content
    val enabled = alias["enabled"]!!.jsonPrimitive.content == "true"
    Row(Modifier.fillMaxWidth().padding(start = 18.dp, end = 10.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(address, color = if (enabled) colors.text else colors.textLow, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
            val note = alias["note"]!!.jsonPrimitive.content
            if (note.isNotEmpty()) Text(note, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
        }
        RowIconButton(PassIcons.ContentCopy, "Copy $address") { shell.copyPlain("Alias", address) }
        Spacer(Modifier.width(6.dp))
        // Named, as it stands apart from the address it turns on and off.
        com.wasimaster.wmpass.ui.vault.PassSwitch(enabled, modifier = Modifier.semantics { contentDescription = "Receive mail at $address" }, onChange = { on ->
            scope.launch {
                runCatching {
                    Engine.net("alias.set_enabled", online = NetAccess.allowed(context), secret = serviceKey(title).orEmpty().toByteArray()) {
                        put("provider", provider)
                        put("id", id)
                        put("enabled", on)
                    }
                }.onFailure {
                    if (it is kotlinx.coroutines.CancellationException) throw it
                    // The switch goes back as the list reloads; this says why.
                    shell.notice(it.message ?: "The alias could not be changed")
                }
                onChange()
            }
        })
    }
}

/** From the editor: a new alias for the item being edited, with its name as the note. */
@Composable
fun AliasPickerSheet(note: String, onDismiss: () -> Unit, onPicked: (String) -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val entries by Pass.entries.collectAsStateWithLifecycle()
    val ready = remember(entries) { AliasServices.filter { service -> entries.any { it.kind == "api" && it.title == service.keyTitle } } }
    var busy by remember { mutableStateOf<String?>(null) }
    // The service that last refused, by id, and why. It shows under that service's row.
    var failed by remember { mutableStateOf<Pair<String, String>?>(null) }
    com.wasimaster.wmpass.ui.vault.PassSheet(onDismiss, "Use an alias") {
        if (ready.isEmpty()) {
            Text(
                "Set up a forwarding service first. Then each new login can have an address of its own.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyLarge,
            )
            Spacer(Modifier.height(14.dp))
            PrimaryButton("Set one up", Modifier.fillMaxWidth()) {
                onDismiss()
                shell.nav.navigate("settings/aliases")
            }
        } else {
            ready.forEach { service ->
                com.wasimaster.wmpass.ui.vault.SheetRow(
                    PassIcons.AlternateEmail,
                    if (busy == service.id) "Making it…" else service.name,
                    "A new address that forwards to you",
                ) {
                    // One at a time: a second tap while the first is made would make a second address.
                    if (busy != null) return@SheetRow
                    busy = service.id
                    failed = null
                    scope.launch {
                        try {
                            val key = serviceKey(service.keyTitle).orEmpty().toByteArray()
                            val made = Engine.net("alias.create", online = NetAccess.allowed(context), secret = key) {
                                put("provider", service.id)
                                put("note", note)
                            }.obj
                            key.fill(0)
                            onPicked(made["address"]!!.jsonPrimitive.content)
                        } catch (failure: Exception) {
                            if (failure is kotlinx.coroutines.CancellationException) throw failure
                            failed = service.id to (failure.message ?: "${service.name} did not make an address.")
                        }
                        busy = null
                    }
                }
                failed?.takeIf { it.first == service.id }?.let { (_, why) ->
                    Text(why, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.announced().padding(start = 60.dp, bottom = 6.dp))
                }
            }
        }
    }
}

/** Gmail and Fastmail address tricks need no service at all, and work offline. */
@Composable
private fun AddressTricks() {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val address = rememberTextFieldState()
    val tag = rememberTextFieldState()
    var result by remember { mutableStateOf<String?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    Eyebrow("With your own address") { com.wasimaster.wmpass.ui.help.HelpMark("group.aliases.own", "aliases with your own address") }
    Text(
        "A plus tag works with Gmail, Fastmail and many others. Dots work with Gmail. Neither hides your real address from a site that strips them.",
        color = colors.textMid,
        style = MaterialTheme.typography.bodyMedium,
        modifier = Modifier.padding(horizontal = 6.dp),
    )
    SheetField(address, "you@gmail.com")
    SheetField(tag, "Tag, such as the site's name")
    Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        fun run(method: String) {
            scope.launch {
                try {
                    result = Engine.net(method, online = false) {
                        put("address", address.text.toString().trim())
                        put("tag", tag.text.toString().trim())
                    }.obj["address"]!!.jsonPrimitive.content
                    problem = null
                } catch (failure: Exception) {
                    // This scope has no handler: an answer missing its field is said here, not a crash.
                    if (failure is kotlinx.coroutines.CancellationException) throw failure
                    problem = failure.message ?: "That address could not be made."
                }
            }
        }
        QuietButton("Plus tag", Modifier.weight(1f), height = 48.dp) { run("alias.plus") }
        QuietButton("Gmail dots", Modifier.weight(1f), height = 48.dp) { run("alias.dots") }
    }
    problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.announced()) }
    result?.let { value ->
        Row(
            Modifier.fillMaxWidth().clip(Shapes.cardSmall).background(colors.surface1).rim(Shapes.cardSmall, colors).padding(start = 18.dp, end = 10.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(value, color = colors.text, style = PassType.secretSmall, modifier = Modifier.weight(1f))
            RowIconButton(PassIcons.ContentCopy, "Copy") { shell.copyPlain("Address", value) }
        }
    }
}

package com.wasimaster.wmpass.ui.settings

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
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.BuildConfig
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.system.NetAccess
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
        Text("This needs the network", color = colors.text, style = MaterialTheme.typography.titleLarge)
        Text(what, color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
        Text("WM Pass stays offline until you allow this. You can turn it off again in Settings.", color = colors.textLow, style = MaterialTheme.typography.bodyMedium)
        PrimaryButton("Allow network features", Modifier.fillMaxWidth()) { shell.prefs.updateNetworkAllowed(true) }
    }
}

@Composable
private fun Page(title: String, content: @Composable () -> Unit) {
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
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val entries by Pass.entries.collectAsStateWithLifecycle()
    var running by remember { mutableStateOf(false) }
    var done by remember { mutableStateOf(0) }
    var total by remember { mutableStateOf(0) }
    val found by BreachCounts.counts.collectAsStateWithLifecycle()
    var problem by remember { mutableStateOf<String?>(null) }
    Page("Breaches") {
        NetGate("Checks your passwords against Have I Been Pwned. Only the first five characters of each password's SHA-1 hash are sent, so the service cannot tell which password you asked about. Checking an email address sends that address.") {
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                Eyebrow("Your passwords")
                Text(
                    if (found.isEmpty() && !running) {
                        "See whether any password in this vault has appeared in a known leak."
                    } else if (running) {
                        "Checked $done of $total."
                    } else {
                        "${found.size} of them have appeared in leaks. They now lead the list on the Health page."
                    },
                    color = if (found.isNotEmpty() && !running) colors.danger else colors.textMid,
                    style = MaterialTheme.typography.bodyLarge,
                    modifier = Modifier.padding(horizontal = 6.dp),
                )
                AnimatedVisibility(running) {
                    LinearProgressIndicator(
                        progress = { if (total == 0) 0f else done.toFloat() / total },
                        color = colors.primary,
                        trackColor = colors.surface3,
                        modifier = Modifier.fillMaxWidth().clip(CircleShape).padding(vertical = 4.dp),
                    )
                }
                PrimaryButton(if (running) "Checking…" else "Check every password", Modifier.fillMaxWidth(), enabled = !running, icon = PassIcons.GppGood) {
                    running = true
                    problem = null
                    scope.launch {
                        val targets = entries.filter { it.hasPassword }
                        total = targets.size
                        done = 0
                        val counts = mutableMapOf<String, Long>()
                        try {
                            for (entry in targets) {
                                val secret = Pass.reveal(entry.id, "password")
                                val reply = secret.consume { bytes ->
                                    Engine.net("breach.password", online = NetAccess.allowed(context), secret = bytes)
                                }
                                val count = reply.obj["count"]?.jsonPrimitive?.long ?: 0
                                if (count > 0) counts[entry.id] = count
                                done++
                            }
                            BreachCounts.counts.value = counts
                            shell.haptics.confirm()
                        } catch (failure: NativeFailure) {
                            problem = failure.message
                        }
                        running = false
                    }
                }
                problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
                val breached = entries.filter { it.id in found }
                if (breached.isNotEmpty()) {
                    GroupCard {
                        breached.forEachIndexed { index, entry ->
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
        SiteTile(entry.title, entry.site, entry.kind, size = 40.dp, label = entry.color, icon = entry.icon)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(entry.title, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
            Text("Seen %,d times".format(count), color = colors.danger, style = MaterialTheme.typography.bodySmall)
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
    val key = rememberTextFieldState()
    var busy by remember { mutableStateOf<String?>(null) }
    val results = remember { mutableStateMapOf<String, String>() }
    if (emails.isEmpty()) return
    Eyebrow("Your email addresses")
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
    if (source == "hibp") SheetField(key, "API key")
    GroupCard {
        emails.forEachIndexed { index, email ->
            if (index > 0) Hairline()
            Row(Modifier.fillMaxWidth().padding(start = 18.dp, end = 12.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(email, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
                    results[email]?.let { Text(it, color = colors.textMid, style = MaterialTheme.typography.bodySmall) }
                }
                if (busy == email) {
                    CircularProgressIndicator(Modifier.size(22.dp), color = colors.primary, strokeWidth = 2.dp)
                } else {
                    RowIconButton(PassIcons.Search, "Check $email") {
                        busy = email
                        scope.launch {
                            val online = NetAccess.allowed(context)
                            results[email] = try {
                                if (source == "comb") {
                                    val reply = Pass.net("breach.comb", online) { put("email", email) }
                                    val matched = reply.obj["matched"]!!.jsonArray.size
                                    val lines = reply.obj["lines"]!!.jsonPrimitive.long
                                    when {
                                        matched > 0 -> "$matched of your passwords for it are in this leak. Change them."
                                        lines > 0 -> "The address is listed, with passwords you no longer use."
                                        else -> "Not listed."
                                    }
                                } else {
                                    val secret = if (source == "hibp") key.text.toString().toByteArray() else null
                                    val rows = Engine.net("breach.account", online = online, secret = secret) {
                                        put("source", source)
                                        put("email", email)
                                    }.json as JsonArray
                                    if (rows.isEmpty()) "Not listed." else "In ${rows.size}: " + rows.take(5).joinToString(", ") { it.jsonObject["name"]!!.jsonPrimitive.content }
                                }
                            } catch (failure: NativeFailure) {
                                failure.message.orEmpty()
                            }
                            busy = null
                        }
                    }
                }
            }
        }
    }
}

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
    val keyField = rememberTextFieldState()
    val note = rememberTextFieldState()
    var aliases by remember { mutableStateOf<List<JsonObject>>(emptyList()) }
    var problem by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    val entries by Pass.entries.collectAsStateWithLifecycle()
    suspend fun refresh() {
        problem = null
        val key = serviceKey(service.keyTitle)
        hasKey = key != null
        aliases = emptyList()
        if (key != null && service.id != "duckduckgo") {
            try {
                val bytes = key.toByteArray()
                aliases = (
                    Engine.net("alias.list", online = NetAccess.allowed(context), secret = bytes) { put("provider", service.id) }.json as JsonArray
                    ).map { it.jsonObject }
            } catch (failure: NativeFailure) {
                problem = failure.message
            }
        }
    }
    LaunchedEffect(service, entries.size) { if (shell.prefs.networkAllowed) refresh() }
    Page("Email aliases") {
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            AddressTricks()
            NetGate("Alias services create addresses that forward to you, so a site never sees your real one. The service you pick receives your API key and the note you write.") {
                Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    Eyebrow("Forwarding services")
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        AliasServices.forEach { option -> Pill(option.name, option == service) { service = option } }
                    }
                    if (!hasKey) {
                        Text(service.note, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp))
                        SheetField(keyField, "API key")
                        PrimaryButton("Keep this key in the vault", Modifier.fillMaxWidth(), enabled = keyField.text.isNotBlank()) {
                            scope.launch {
                                saveServiceKey(service.keyTitle, keyField.text.toString().trim())
                                refresh()
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
                                    shell.copyPlain("Alias", made["address"]!!.jsonPrimitive.content)
                                    refresh()
                                } catch (failure: NativeFailure) {
                                    problem = failure.message
                                }
                                busy = false
                            }
                        }
                    }
                    problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
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
            Text(address, color = if (enabled) colors.text else colors.textLow, style = MaterialTheme.typography.bodyMedium, maxLines = 1)
            val note = alias["note"]!!.jsonPrimitive.content
            if (note.isNotEmpty()) Text(note, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1)
        }
        RowIconButton(PassIcons.ContentCopy, "Copy") { shell.copyPlain("Alias", address) }
        Spacer(Modifier.width(6.dp))
        com.wasimaster.wmpass.ui.vault.PassSwitch(enabled) { on ->
            scope.launch {
                runCatching {
                    Engine.net("alias.set_enabled", online = NetAccess.allowed(context), secret = serviceKey(title).orEmpty().toByteArray()) {
                        put("provider", provider)
                        put("id", id)
                        put("enabled", on)
                    }
                }
                onChange()
            }
        }
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
    var problem by remember { mutableStateOf<String?>(null) }
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
                    busy = service.id
                    problem = null
                    scope.launch {
                        try {
                            val key = serviceKey(service.keyTitle).orEmpty().toByteArray()
                            val made = Engine.net("alias.create", online = NetAccess.allowed(context), secret = key) {
                                put("provider", service.id)
                                put("note", note)
                            }.obj
                            key.fill(0)
                            onPicked(made["address"]!!.jsonPrimitive.content)
                        } catch (failure: NativeFailure) {
                            problem = failure.message
                        }
                        busy = null
                    }
                }
            }
            problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
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
    Eyebrow("With your own address")
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
                } catch (failure: NativeFailure) {
                    problem = failure.message
                }
            }
        }
        QuietButton("Plus tag", Modifier.weight(1f), height = 48.dp) { run("alias.plus") }
        QuietButton("Gmail dots", Modifier.weight(1f), height = 48.dp) { run("alias.dots") }
    }
    problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
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

// ---- disposable inbox ----

@Composable
fun InboxScreen() {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var address by remember { mutableStateOf<String?>(null) }
    var token by remember { mutableStateOf<ByteArray?>(null) }
    var service by remember { mutableStateOf("mailtm") }
    var messages by remember { mutableStateOf<List<JsonObject>>(emptyList()) }
    var opened by remember { mutableStateOf<Pair<String, String>?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(address) {
        while (address != null) {
            runCatching {
                messages = (
                    Engine.net("inbox.messages", online = NetAccess.allowed(context), secret = token) {
                        put("service", service)
                        put("address", address.orEmpty())
                    }.json as JsonArray
                    ).map { it.jsonObject }
            }
            kotlinx.coroutines.delay(8000)
        }
    }
    Page("Disposable inbox") {
        NetGate("Makes a throwaway address on a public mail service, for a sign-up that only needs one confirmation mail. The service, and possibly others, can read what arrives. Never use one for an account you want to keep.") {
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                val current = address
                if (current == null) {
                    Text(
                        "Anyone could read these mailboxes in principle, and the address is gone when you leave this page. Good for a one-off sign-up, nothing more.",
                        color = colors.textMid,
                        style = MaterialTheme.typography.bodyLarge,
                        modifier = Modifier.padding(horizontal = 6.dp, vertical = 8.dp),
                    )
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        Pill("mail.tm", service == "mailtm") { service = "mailtm" }
                        Pill("Guerrilla Mail", service == "guerrilla") { service = "guerrilla" }
                    }
                    PrimaryButton("Make an address", Modifier.fillMaxWidth(), icon = PassIcons.Inbox) {
                        scope.launch {
                            try {
                                val reply = Engine.net("inbox.new", online = NetAccess.allowed(context)) { put("service", service) }
                                token = reply.secret?.consume { it.copyOf() }
                                address = reply.obj["address"]!!.jsonPrimitive.content
                                problem = null
                            } catch (failure: NativeFailure) {
                                problem = failure.message
                            }
                        }
                    }
                } else {
                    Row(
                        Modifier.fillMaxWidth().clip(Shapes.cardSmall).background(colors.surface1).rim(Shapes.cardSmall, colors).padding(start = 18.dp, end = 10.dp, top = 8.dp, bottom = 8.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Text(current, color = colors.text, style = PassType.secretSmall, modifier = Modifier.weight(1f))
                        RowIconButton(PassIcons.ContentCopy, "Copy") { shell.copyPlain("Address", current) }
                    }
                    Eyebrow("Arrived")
                    if (messages.isEmpty()) {
                        Text("Nothing yet. This page checks every few seconds.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp))
                    } else {
                        GroupCard {
                            messages.forEachIndexed { index, message ->
                                if (index > 0) Hairline()
                                val subject = message["subject"]!!.jsonPrimitive.content
                                val preview = message["preview"]!!.jsonPrimitive.content
                                Column(
                                    Modifier.fillMaxWidth().clickable {
                                        if (service == "mailtm") {
                                            scope.launch {
                                                val body = runCatching {
                                                    Engine.net("inbox.body", online = NetAccess.allowed(context), secret = token) { put("id", message["id"]!!.jsonPrimitive.content) }
                                                        .obj["text"]!!.jsonPrimitive.content
                                                }.getOrDefault(preview)
                                                opened = subject to body
                                            }
                                        } else {
                                            opened = subject to preview
                                        }
                                    }.padding(horizontal = 18.dp, vertical = 12.dp),
                                ) {
                                    Text(subject.ifEmpty { "No subject" }, color = colors.text, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold), maxLines = 1)
                                    Text(message["from"]!!.jsonPrimitive.content, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1)
                                    Text(preview, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 2)
                                }
                            }
                        }
                    }
                    opened?.let { (subject, body) ->
                        Eyebrow(subject)
                        // Plain text only, so no tracking pixel or script in a mail ever runs here.
                        Text(body, color = colors.text, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp))
                    }
                }
                problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
                Spacer(Modifier.height(30.dp))
            }
        }
    }
}

// ---- usernames ----

@Composable
fun UsernameScreen() {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val name = rememberTextFieldState()
    var sites by remember { mutableStateOf<List<String>>(emptyList()) }
    val answers = remember { mutableStateMapOf<String, String>() }
    var running by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(Unit) {
        sites = runCatching { (Engine.net("usernames.sites", online = false).json as JsonArray).map { it.jsonPrimitive.content } }.getOrDefault(emptyList())
    }
    Page("Username checker") {
        NetGate("Asks each site whether a name is taken, one at a time. Every site you check learns the name you looked up. X, Instagram, npm and PyPI are left out, because they refuse or give the same answer for every name.") {
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                SheetField(name, "A username")
                PrimaryButton(if (running) "Checking…" else "Check", Modifier.fillMaxWidth(), enabled = !running && name.text.isNotBlank(), icon = PassIcons.PersonSearch) {
                    running = true
                    problem = null
                    answers.clear()
                    scope.launch {
                        for (site in sites) {
                            answers[site] = "…"
                            try {
                                answers[site] = Engine.net("usernames.check", online = NetAccess.allowed(context)) {
                                    put("site", site)
                                    put("name", name.text.toString().trim())
                                }.obj["answer"]!!.jsonPrimitive.content
                            } catch (failure: NativeFailure) {
                                answers[site] = "unknown"
                                problem = failure.message
                            }
                        }
                        running = false
                    }
                }
                problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
                if (answers.isNotEmpty()) {
                    GroupCard {
                        sites.filter { it in answers }.forEachIndexed { index, site ->
                            if (index > 0) Hairline(inset = 70.dp)
                            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 10.dp), verticalAlignment = Alignment.CenterVertically) {
                                SiteTile(site, null, "login", size = 38.dp)
                                Spacer(Modifier.width(14.dp))
                                Text(site, color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                                val answer = answers[site].orEmpty()
                                Text(
                                    when (answer) {
                                        "free" -> "Free"
                                        "taken" -> "Taken"
                                        "unknown" -> "No clear answer"
                                        else -> "…"
                                    },
                                    color = when (answer) {
                                        "free" -> colors.primary
                                        "taken" -> colors.textMid
                                        else -> colors.textLow
                                    },
                                    style = MaterialTheme.typography.labelLarge,
                                )
                            }
                        }
                    }
                }
                Spacer(Modifier.height(30.dp))
            }
        }
    }
}

package com.wasimaster.wmpass.ui.settings

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
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
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.Destination
import com.wasimaster.wmpass.system.Destinations
import com.wasimaster.wmpass.system.SharedVaults
import com.wasimaster.wmpass.system.sinkProblem
import com.wasimaster.wmpass.ui.door.PasswordPill
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.NavRow
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QrCode
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.parts.qrFits
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.ConfirmSheet
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.SheetField
import com.wasimaster.wmpass.ui.vault.SheetRow
import com.wasimaster.wmsuite.settings.WmScreen
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.boolean
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/** A row of places to choose from, for where a shared vault lives. */
@Composable
private fun PlacePicker(chosen: Destination?, onPick: (Destination) -> Unit) {
    val colors = palette
    val places by Destinations.list.collectAsStateWithLifecycle()
    var adding by remember { mutableStateOf(false) }
    Text("Where it lives. Everyone sharing needs access to this place.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp))
    androidx.compose.foundation.layout.FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        places.forEach { place -> Pill(place.label, chosen?.id == place.id) { onPick(place) } }
        Pill("Add a place", false, icon = PassIcons.Add) { adding = true }
    }
    if (adding) {
        DestinationSheet(null, { adding = false }) { place ->
            adding = false
            onPick(place)
        }
    }
}

/** A code on screen, as a QR and as text to send by any means. */
@Composable
private fun CodeCard(code: String, caption: String) {
    val colors = palette
    val shell = LocalShell.current
    if (qrFits(code)) QrCode(code, Modifier.padding(horizontal = 30.dp))
    Spacer(Modifier.height(10.dp))
    Text(caption, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center, modifier = Modifier.fillMaxWidth())
    Spacer(Modifier.height(10.dp))
    QuietButton("Copy it instead", Modifier.fillMaxWidth(), PassIcons.ContentCopy, height = 48.dp) { shell.copyPlain("Code", code) }
}

@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun VaultsScreen() {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val child by Pass.child.collectAsStateWithLifecycle()
    var links by remember { mutableStateOf<List<JsonObject>>(emptyList()) }
    var tick by remember { mutableIntStateOf(0) }
    var sheet by remember { mutableStateOf<String?>(null) }
    var opening by remember { mutableStateOf<String?>(null) }
    // The vault that last failed to open, by its file, with the words for why. They show under its row.
    var problem by remember { mutableStateOf<Pair<String, String>?>(null) }
    var chosen by remember { mutableStateOf<JsonObject?>(null) }
    LaunchedEffect(tick, child) {
        links = runCatching { (Pass.plainCall("linked.list").json as JsonArray).map { it.jsonObject } }.getOrDefault(emptyList())
    }
    fun open(link: JsonObject) {
        val file = link["file"]!!.jsonPrimitive.content
        opening = file
        problem = null
        scope.launch {
            try {
                if (link["shared"]!!.jsonPrimitive.boolean) {
                    val place = SharedVaults.placeOf(context, file)
                    if (place == null) {
                        chosen = link
                        sheet = "place"
                    } else {
                        SharedVaults.open(context, place, file)
                        shell.nav.popBackStack("home", false)
                    }
                } else {
                    val sealed = Pass.linkedFile(file).readBytes()
                    val reply = Pass.plainCall("linked.open", blobs = listOf(sealed)) { put("file", file) }
                    Pass.enterChild(file, reply) { bytes -> Pass.writeLinked(file, bytes) }
                    shell.nav.popBackStack("home", false)
                }
            } catch (failure: Exception) {
                problem = file to ((failure as? NativeFailure)?.message ?: sinkProblem(failure))
                shell.haptics.reject()
            }
            opening = null
        }
    }
    WmScreen("Nested and shared vaults", route = "settings/vaults", onBack = { shell.nav.popBackStack() }) {
        Column(Modifier.padding(horizontal = 16.dp)) {
            child?.let { name ->
                Eyebrow("You are inside")
                GroupCard { NavRow(name, "Back to the vault it lives in", icon = PassIcons.ArrowBack) { scope.launch { Pass.leaveChild() } } }
            }
            Text(
                "A nested vault is its own encrypted file, opened from inside this one. A shared vault is held at a place you and others can reach, and each person has their own key to it.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyLarge,
                modifier = Modifier.padding(horizontal = 6.dp, vertical = 8.dp),
            )
            if (links.isNotEmpty()) {
                Eyebrow("Yours")
                GroupCard {
                    links.forEachIndexed { index, link ->
                        if (index > 0) Hairline(inset = 70.dp)
                        val shared = link["shared"]!!.jsonPrimitive.boolean
                        val pending = link["pending"]!!.jsonPrimitive.boolean
                        val owner = link["owner"]!!.jsonPrimitive.boolean
                        val file = link["file"]!!.jsonPrimitive.content
                        Row(verticalAlignment = Alignment.CenterVertically) {
                            Column(Modifier.weight(1f)) {
                                NavRow(
                                    link["name"]!!.jsonPrimitive.content,
                                    when {
                                        pending -> "Waiting for the owner's invitation"
                                        owner -> "Shared, and yours to manage"
                                        shared -> "Shared with you"
                                        else -> "Nested"
                                    },
                                    icon = if (shared) PassIcons.FamilyRestroom else PassIcons.Layers,
                                ) {
                                    if (pending) {
                                        chosen = link
                                        sheet = "accept"
                                    } else {
                                        open(link)
                                    }
                                }
                            }
                            if (opening == file) {
                                CircularProgressIndicator(Modifier.size(22.dp), color = colors.primary, strokeWidth = 2.dp)
                                Spacer(Modifier.width(14.dp))
                            } else if (shared && !pending) {
                                RowIconButton(PassIcons.Person, "People") {
                                    chosen = link
                                    sheet = "people"
                                }
                                Spacer(Modifier.width(12.dp))
                            }
                        }
                        problem?.takeIf { it.first == file }?.let { (_, words) ->
                            Text(words, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(start = 70.dp, end = 16.dp, bottom = 12.dp))
                        }
                    }
                }
            }
            Eyebrow("Add")
            GroupCard {
                NavRow("A nested vault", "Its own encrypted file, kept apart from this one", icon = PassIcons.Layers, help = "action.nested_vault") { sheet = "nested" }
                Hairline(inset = 70.dp)
                NavRow("A shared vault", "For family or a team, without a server", icon = PassIcons.FamilyRestroom, help = "action.shared_vault") { sheet = "shared" }
                Hairline(inset = 70.dp)
                NavRow("Join someone's shared vault", "Make a code to send them", icon = PassIcons.Add, help = "action.join_shared_vault") { sheet = "join" }
            }
            Spacer(Modifier.height(40.dp))
        }
    }
    when (sheet) {
        "nested" -> NestedSheet({ sheet = null }) { tick++ }
        "shared" -> CreateSharedSheet({ sheet = null }) { tick++ }
        "join" -> JoinSheet({ sheet = null }) { tick++ }
        "place" -> chosen?.let { link ->
            PassSheet({ sheet = null }, "Where is ${link["name"]!!.jsonPrimitive.content}?") {
                PlacePicker(null) { place ->
                    SharedVaults.setPlace(context, link["file"]!!.jsonPrimitive.content, place)
                    sheet = null
                    open(link)
                }
            }
        }
        "accept" -> chosen?.let { link -> AcceptSheet(link, { sheet = null }) { tick++ } }
        "people" -> chosen?.let { link -> PeopleSheet(link) { sheet = null } }
    }
}

@Composable
private fun NestedSheet(onDismiss: () -> Unit, onMade: () -> Unit) {
    val colors = palette
    val scope = rememberCoroutineScope()
    val name = rememberTextFieldState()
    val password = rememberTextFieldState()
    var problem by remember { mutableStateOf<String?>(null) }
    PassSheet(onDismiss, "A nested vault") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(
                "It opens from here without asking again, so anyone inside this vault can reach it. Its own password opens it only through a recovery, or if you later keep the file elsewhere.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
            SheetField(name, "Its name, such as Work")
            PasswordPill(password, "Its own password")
            problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
            PrimaryButton("Make it", Modifier.fillMaxWidth(), enabled = name.text.isNotBlank() && password.text.isNotEmpty()) {
                scope.launch {
                    val secret = password.text.toString().toByteArray()
                    try {
                        val reply = Pass.change("linked.create", secret = secret) {
                            put("name", name.text.toString().trim())
                            put("kdf", kotlinx.serialization.json.buildJsonObject { put("m_kib", 64 * 1024) })
                        }
                        Pass.writeLinked(reply.obj["file"]!!.jsonPrimitive.content, reply.blobs[0])
                        Pass.flush()
                        onMade()
                        onDismiss()
                    } catch (failure: NativeFailure) {
                        problem = failure.message
                    } finally {
                        secret.fill(0)
                    }
                }
            }
        }
    }
}

@Composable
private fun CreateSharedSheet(onDismiss: () -> Unit, onMade: () -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val name = rememberTextFieldState()
    val me = rememberTextFieldState()
    var place by remember { mutableStateOf<Destination?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }
    PassSheet(onDismiss, "A shared vault") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            SheetField(name, "Its name, such as Family")
            SheetField(me, "Your name, as the others will see it")
            PlacePicker(place) { place = it }
            problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
            Text(
                "Everyone with the key can read and change what is in it. Removing someone moves it to a new key, but they keep any copy they already have.",
                color = colors.textLow,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
            PrimaryButton(if (busy) "Making it…" else "Make it", Modifier.fillMaxWidth(), enabled = !busy && name.text.isNotBlank() && me.text.isNotBlank() && place != null) {
                busy = true
                scope.launch {
                    runCatching { SharedVaults.create(context, place!!, name.text.toString().trim(), me.text.toString().trim()) }
                        .onSuccess {
                            onMade()
                            onDismiss()
                        }
                        .onFailure { problem = it.message?.let { _ -> sinkProblem(it) } }
                    busy = false
                }
            }
        }
    }
}

@Composable
private fun JoinSheet(onDismiss: () -> Unit, onMade: () -> Unit) {
    val colors = palette
    val me = rememberTextFieldState()
    val scope = rememberCoroutineScope()
    var code by remember { mutableStateOf<String?>(null) }
    var fingerprint by remember { mutableStateOf("") }
    PassSheet(onDismiss, "Join a shared vault") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp), horizontalAlignment = Alignment.CenterHorizontally) {
            val shown = code
            if (shown == null) {
                SheetField(me, "Your name, as the others will see it")
                PrimaryButton("Make my code", Modifier.fillMaxWidth(), enabled = me.text.isNotBlank()) {
                    scope.launch {
                        val reply = Pass.change("shared.join_code") { put("my_name", me.text.toString().trim()) }
                        code = reply.obj["code"]!!.jsonPrimitive.content
                        fingerprint = reply.obj["fingerprint"]!!.jsonPrimitive.content
                        onMade()
                    }
                }
            } else {
                CodeCard(shown, "Show or send this to the vault's owner. It holds no secret.")
                Text("Read this out to them, to check nobody swapped the code", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, textAlign = TextAlign.Center)
                Text(fingerprint, color = colors.text, style = PassType.secret, textAlign = TextAlign.Center)
                Text("Their invitation comes back to this same list, under Waiting.", color = colors.textLow, style = MaterialTheme.typography.bodySmall, textAlign = TextAlign.Center)
            }
        }
    }
}

@Composable
private fun AcceptSheet(link: JsonObject, onDismiss: () -> Unit, onDone: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val invite = rememberTextFieldState()
    var place by remember { mutableStateOf<Destination?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    fun accept(text: String) {
        val where = place ?: return
        scope.launch {
            try {
                val reply = Pass.change("shared.accept") {
                    put("file", link["file"]!!.jsonPrimitive.content)
                    put("code", text.trim())
                }
                SharedVaults.setPlace(context, reply.obj["file"]!!.jsonPrimitive.content, where)
                Pass.flush()
                onDone()
                onDismiss()
            } catch (failure: NativeFailure) {
                problem = failure.message
            }
        }
    }
    PassSheet(onDismiss, "Your invitation") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            PlacePicker(place) { place = it }
            SheetField(invite, "Paste the invitation")
            problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                QuietButton("Scan it", Modifier.weight(1f), PassIcons.QrCodeScanner, enabled = place != null) {
                    shell.scanTarget = { accept(it) }
                }
                PrimaryButton("Accept", Modifier.weight(1f), enabled = place != null && invite.text.isNotBlank()) { accept(invite.text.toString()) }
            }
        }
    }
}

@Composable
private fun PeopleSheet(link: JsonObject, onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val file = link["file"]!!.jsonPrimitive.content
    val owner = link["owner"]!!.jsonPrimitive.boolean
    val place = remember(file) { SharedVaults.placeOf(context, file) }
    var members by remember { mutableStateOf<List<JsonObject>>(emptyList()) }
    var tick by remember { mutableIntStateOf(0) }
    var problem by remember { mutableStateOf<String?>(null) }
    val code = rememberTextFieldState()
    var reading by remember { mutableStateOf<Pair<String, String>?>(null) }
    var invite by remember { mutableStateOf<String?>(null) }
    var removing by remember { mutableStateOf<JsonObject?>(null) }
    // A code that could not be read or added. It shows under the field or check it concerns.
    var codeProblem by remember { mutableStateOf<String?>(null) }
    // The member whose removal failed, by their key, with the words for why. They show under that person.
    var removeProblem by remember { mutableStateOf<Pair<String, String>?>(null) }
    LaunchedEffect(tick) {
        if (place == null) {
            problem = "This phone does not know where the vault lives yet. Open it once first."
            return@LaunchedEffect
        }
        try {
            val manifest = SharedVaults.manifest(context, place, file)
            members = (Pass.plainCall("shared.members", blobs = listOf(manifest)) { put("file", file) }.json as JsonArray).map { it.jsonObject }
        } catch (failure: Exception) {
            problem = if (failure is NativeFailure) failure.message else sinkProblem(failure)
        }
    }
    fun read(text: String) {
        codeProblem = null
        scope.launch {
            try {
                val reply = Engine.call("shared.read_code", Pass.handleForTests, { put("code", text.trim()) })
                reading = text.trim() to "${reply.obj["name"]!!.jsonPrimitive.content}\n${reply.obj["fingerprint"]!!.jsonPrimitive.content}"
            } catch (failure: NativeFailure) {
                codeProblem = failure.message
            }
        }
    }
    PassSheet(onDismiss, link["name"]!!.jsonPrimitive.content) {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            members.forEach { member ->
                Row(verticalAlignment = Alignment.CenterVertically) {
                    Column(Modifier.weight(1f)) {
                        Text(member["name"]!!.jsonPrimitive.content, color = colors.text, style = MaterialTheme.typography.bodyLarge)
                        Text(member["fingerprint"]!!.jsonPrimitive.content, color = colors.textMid, style = PassType.secretSmall)
                    }
                    if (owner && !member["owner"]!!.jsonPrimitive.boolean) {
                        RowIconButton(PassIcons.Delete, "Remove", tint = colors.danger) { removing = member }
                    }
                }
                removeProblem?.takeIf { it.first == member["key"]?.jsonPrimitive?.content }?.let { (_, words) ->
                    Text(words, color = colors.danger, style = MaterialTheme.typography.bodyMedium)
                }
            }
            problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
            val offered = invite
            val check = reading
            if (offered != null) {
                CodeCard(offered, "They scan or paste this. Anyone can see it, but only the person you added can use it.")
            } else if (owner && check != null) {
                val (name, print) = check.second.split('\n', limit = 2).let { it[0] to it[1] }
                Text("Is this $name?", color = colors.text, style = MaterialTheme.typography.titleMedium)
                Text("Ask them to read their code out. A different one means someone swapped it, and adding it would hand that person the vault.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                Text(print, color = colors.text, style = PassType.secret)
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    QuietButton("Not them", Modifier.weight(1f), tint = colors.danger) {
                        reading = null
                        codeProblem = null
                    }
                    PrimaryButton("It matches", Modifier.weight(1f)) {
                        codeProblem = null
                        scope.launch {
                            try {
                                invite = SharedVaults.addMember(context, place!!, file, check.first)
                                reading = null
                                tick++
                            } catch (failure: Exception) {
                                codeProblem = if (failure is NativeFailure) failure.message else sinkProblem(failure)
                            }
                        }
                    }
                }
                codeProblem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
            } else if (owner && place != null) {
                Eyebrow("Add someone")
                SheetField(code, "Paste their code")
                codeProblem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium) }
                Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                    QuietButton("Scan it", Modifier.weight(1f), PassIcons.QrCodeScanner) {
                        shell.scanTarget = { read(it) }
                    }
                    PrimaryButton("Check it", Modifier.weight(1f), enabled = code.text.isNotBlank()) { read(code.text.toString()) }
                }
            }
            Spacer(Modifier.height(10.dp))
        }
    }
    removing?.let { member ->
        ConfirmSheet(
            "Remove ${member["name"]!!.jsonPrimitive.content}?",
            "The vault moves to a new key, so they cannot open anything written from now on. Change any password they could see: they may have kept a copy.",
            "Remove them",
            onDismiss = { removing = null },
        ) {
            val key = member["key"]!!.jsonPrimitive.content
            removeProblem = null
            scope.launch {
                try {
                    SharedVaults.removeMember(context, place!!, file, key)
                    tick++
                } catch (failure: Exception) {
                    removeProblem = key to ((failure as? NativeFailure)?.message ?: sinkProblem(failure))
                }
            }
        }
    }
}

@file:OptIn(androidx.compose.material3.ExperimentalMaterial3Api::class)

package com.wasimaster.wmpass.ui.vault

import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.semantics.stateDescription
import androidx.compose.ui.semantics.contentDescription
import com.wasimaster.wmpass.ui.parts.announced
import com.wasimaster.wmpass.ui.theme.readableWidth
import androidx.activity.compose.BackHandler
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.relocation.BringIntoViewRequester
import androidx.compose.foundation.relocation.bringIntoViewRequester
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.BasicSecureTextField
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.text.input.TextFieldLineLimits
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.TextObfuscationMode
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.DatePicker
import androidx.compose.material3.DatePickerDialog
import androidx.compose.material3.ExperimentalMaterial3Api
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Switch
import androidx.compose.material3.SwitchDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.material3.rememberDatePickerState
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.KeyboardCapitalization
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.graphics.drawable.toBitmap
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.BuildConfig
import com.wasimaster.wmpass.core.CustomFieldView
import com.wasimaster.wmpass.core.Detail
import com.wasimaster.wmpass.core.FieldView
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.StrengthView
import com.wasimaster.wmpass.system.FieldWire
import com.wasimaster.wmpass.ui.help.HelpAction
import com.wasimaster.wmpass.ui.help.helpAnchor
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlassCircle
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.parts.StrengthBar
import com.wasimaster.wmpass.ui.parts.kindInSentence
import com.wasimaster.wmpass.ui.parts.kindName
import com.wasimaster.wmpass.ui.parts.oklch
import com.wasimaster.wmpass.ui.parts.secretSemantics
import com.wasimaster.wmpass.ui.theme.MonoFamily
import com.wasimaster.wmpass.ui.theme.Springs
import com.wasimaster.wmpass.ui.theme.palette
import java.net.URI
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.collectLatest
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/** One field being edited. An existing secret stays in the vault until asked for. */
@Stable
private class EditField(val spec: FieldView) {
    val text = TextFieldState(if (spec.secret) "" else spec.text)
    var flag by mutableStateOf(spec.flag)
    var millis by mutableStateOf(spec.number)
    var loaded by mutableStateOf(!spec.hasValue)
    var original: String = spec.text
    var showing by mutableStateOf(false)
    /** A choice filled in for a new item, such as a Wi-Fi network's security. Not an edit of its own. */
    var preset: String? = null

    val changed: Boolean
        get() = when {
            spec.input == "toggle" -> flag != spec.flag
            spec.input == "date" -> millis != spec.number
            spec.secret -> loaded && text.text.toString() != original
            spec.input == "number" -> text.text.toString() != (spec.number?.toString().orEmpty())
            else -> text.text.toString() != spec.text
        }
}

@Stable
private class EditCustom(val existing: CustomFieldView?, hidden: Boolean) {
    val name = TextFieldState(existing?.name.orEmpty())
    val value = TextFieldState(existing?.value.orEmpty())
    // Switchable either way: the value stays as it is, only how it shows changes.
    var hidden by mutableStateOf(hidden)
    val wasHidden = hidden
    var loaded by mutableStateOf(existing == null || existing.value != null)
    var remove by mutableStateOf(false)
    var original: String = existing?.value.orEmpty()

    /** The kind to save: hidden, or what it was before, such as a link, when it is plain. */
    val kind: String get() = if (hidden) "hidden" else existing?.kind?.takeIf { it != "hidden" } ?: "text"

    val valueChanged: Boolean get() = loaded && value.text.toString() != original
    val metaChanged: Boolean get() = existing != null && (hidden != wasHidden || name.text.toString().trim() != existing.name)
}

@Stable
private class EditQuestion(val index: Int?, question: String) {
    val question = TextFieldState(question)
    val answer = TextFieldState("")
    /** The wording saved, to tell a reworded question from one left alone. */
    val original = question
    var remove by mutableStateOf(false)
    var showing by mutableStateOf(false)

    val reworded: Boolean get() = index != null && question.text.toString().trim() != original.trim()
}

/** A file chosen before saving. It is read and sealed into the vault only when the item is saved. */
private class PendingFile(val uri: android.net.Uri, val name: String, val mime: String)

private fun isApp(site: JsonElement): Boolean = (site as? kotlinx.serialization.json.JsonObject)?.containsKey("AndroidApp") == true

private fun appPackage(site: JsonElement): String =
    ((site as? kotlinx.serialization.json.JsonObject)?.get("AndroidApp") as? kotlinx.serialization.json.JsonObject)?.get("package")?.jsonPrimitive?.contentOrNull.orEmpty()

/**
 * Puts what the card scanner read into the form: the number, shown so it can be checked
 * against the card, and the expiry and name when the scanner read them.
 */
private fun applyCard(read: com.wasimaster.wmpass.core.CardReader.Read, fields: List<EditField>) {
    fields.firstOrNull { it.spec.key == "number" }?.let { field ->
        field.text.setTextAndPlaceCursorAtEnd(read.number)
        field.loaded = true
        field.showing = true
    }
    read.expiry?.let { expiry -> fields.firstOrNull { it.spec.key == "expiry" }?.text?.setTextAndPlaceCursorAtEnd(expiry) }
    read.name?.let { name -> fields.firstOrNull { it.spec.key == "holder" }?.text?.setTextAndPlaceCursorAtEnd(name) }
}

/**
 * Reads a `WIFI:` code, the kind Android's own Wi-Fi settings show to share a network, into the
 * form. Returns false when the text is not one.
 */
private fun applyWifi(text: String, fields: List<EditField>, title: TextFieldState): Boolean {
    if (!text.startsWith("WIFI:", ignoreCase = true)) return false
    val parts = HashMap<String, String>()
    var key = StringBuilder()
    var value = StringBuilder()
    var inValue = false
    var escaped = false
    for (c in text.substring(5)) {
        when {
            escaped -> {
                (if (inValue) value else key).append(c)
                escaped = false
            }
            c == '\\' -> escaped = true
            c == ':' && !inValue -> inValue = true
            c == ';' -> {
                if (key.isNotEmpty()) parts[key.toString().uppercase()] = value.toString()
                key = StringBuilder()
                value = StringBuilder()
                inValue = false
            }
            else -> (if (inValue) value else key).append(c)
        }
    }
    val security = when (parts["T"]?.uppercase()) {
        "WPA", "WPA2" -> "wpa"
        "SAE", "WPA3" -> "sae"
        "WEP" -> "wep"
        "WPA2-EAP", "EAP" -> "eap"
        else -> if (parts["P"].isNullOrEmpty()) "open" else "wpa"
    }
    fillWifi(fields, title, parts["S"].orEmpty(), security, parts["P"].orEmpty(), parts["H"].equals("true", ignoreCase = true))
    return true
}

private fun fillWifi(fields: List<EditField>, title: TextFieldState, ssid: String, security: String, password: String, hidden: Boolean) {
    fields.firstOrNull { it.spec.key == "ssid" }?.text?.setTextAndPlaceCursorAtEnd(ssid)
    fields.firstOrNull { it.spec.key == "security" }?.text?.setTextAndPlaceCursorAtEnd(security)
    fields.firstOrNull { it.spec.key == "password" }?.let { field ->
        field.text.setTextAndPlaceCursorAtEnd(password)
        field.loaded = true
    }
    fields.firstOrNull { it.spec.key == "hidden_ssid" }?.let { it.flag = hidden }
    if (title.text.isEmpty() && ssid.isNotEmpty()) title.setTextAndPlaceCursorAtEnd(ssid)
}


private fun titleFromSite(url: String): String {
    val host = runCatching { URI(if ("://" in url) url else "https://$url").host }.getOrNull() ?: return ""
    val parts = host.removePrefix("www.").split('.')
    val name = if (parts.size >= 2) parts[parts.size - 2] else parts.firstOrNull().orEmpty()
    return name.replaceFirstChar { it.uppercase() }
}

/** Adding and changing, drawn from the core's own table of what each kind holds. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun EditorScreen(entryId: String?, newKind: String?, presetTitle: String = "") {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    var kind by remember { mutableStateOf(newKind.orEmpty()) }
    var detail by remember { mutableStateOf<Detail?>(null) }
    val fields = remember { mutableStateListOf<EditField>() }
    val title = remember { TextFieldState(presetTitle) }
    val sites = remember { mutableStateListOf<Pair<TextFieldState, JsonElement?>>() }
    val customs = remember { mutableStateListOf<EditCustom>() }
    val questions = remember { mutableStateListOf<EditQuestion>() }
    val notes = remember { TextFieldState() }
    var notesLoaded by remember { mutableStateOf(entryId == null) }
    var notesOriginal by remember { mutableStateOf("") }
    val otpSeed = remember { TextFieldState() }
    var otpOpen by remember { mutableStateOf(newKind == "otp") }
    var color by remember { mutableStateOf<String?>(null) }
    var icon by remember { mutableStateOf<String?>(null) }
    var folder by remember { mutableStateOf<String?>(null) }
    var favourite by remember { mutableStateOf(false) }
    var tags by remember { mutableStateOf(emptyList<String>()) }
    val apps = remember { mutableStateListOf<JsonElement>() }
    val files = remember { mutableStateListOf<PendingFile>() }
    val links = remember { mutableStateListOf<String>() }
    var remindDays by remember { mutableStateOf<Int?>(null) }
    /** Parts of the form brought in from "Add more" this time. */
    val added = remember { mutableStateListOf<String>() }
    val context = androidx.compose.ui.platform.LocalContext.current
    val pickFile = androidx.activity.compose.rememberLauncherForActivityResult(androidx.activity.result.contract.ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            val resolver = context.contentResolver
            val name = runCatching {
                resolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use { if (it.moveToFirst()) it.getString(0) else null }
            }.getOrNull() ?: "Attachment"
            files.add(PendingFile(uri, name, resolver.getType(uri).orEmpty()))
            added.add("files")
        }
    }
    var ready by remember { mutableStateOf(false) }
    LaunchedEffect(ready) {
        if (ready && entryId == null && shell.prefs.editorGenerate.value) {
            val field = fields.firstOrNull { it.spec.input == "password" }
            if (field != null && field.text.text.isEmpty()) {
                val made = GenState.fromPrefs(shell.prefs)
                // A courtesy: a generator that fails leaves the field empty to type in, not a crash.
                if (runCatching { made.roll() }.isFailure || made.value.isEmpty()) return@LaunchedEffect
                field.text.setTextAndPlaceCursorAtEnd(made.value)
                field.loaded = true
                // Filled in, not typed: backing out of an otherwise empty form asks nothing.
                field.preset = made.value
            }
        }
    }
    var saving by remember { mutableStateOf(false) }
    var error by remember { mutableStateOf<String?>(null) }
    /**
     * The part of the form a failed save was about: "otp", "notes", or the field, custom field,
     * question or file itself. Its error shows under it. Null means the error is about the whole
     * item, and it shows by Save.
     */
    var errorAt by remember { mutableStateOf<Any?>(null) }
    /** The new item a save made before a later step failed. Saving again finishes it, not a second one. */
    var createdId by remember { mutableStateOf<String?>(null) }
    /** Custom fields, questions and files a failed save already wrote, so saving again does not add them twice. */
    val written = remember { mutableSetOf<Any>() }
    /**
     * Whether a failed save already removed a custom field. Until then a field that was there
     * before is written by its index, which is safe to write again with what it says now.
     */
    var removedAny by remember { mutableStateOf(false) }
    var sheet by remember { mutableStateOf<String?>(null) }
    var generating by remember { mutableStateOf<EditField?>(null) }
    var aliasFor by remember { mutableStateOf<EditField?>(null) }
    var askDiscard by remember { mutableStateOf(false) }
    /** The private key field a new key would replace, waiting on a yes. */
    var askNewKey by remember { mutableStateOf<EditField?>(null) }
    val titleFocus = remember { FocusRequester() }

    // The buttons that fill a card or a network from elsewhere. WM Keyboard offers the same on its
    // strip while one of these fields has the keys, and a tap there runs exactly these.
    fun scanCard() {
        shell.cardTarget = { read -> applyCard(read, fields) }
    }
    fun wifiFromCode() {
        shell.scanTarget = { text ->
            if (!applyWifi(text, fields, title)) shell.notice("That code is not a Wi-Fi network")
        }
    }
    fun wifiFromPhone() {
        sheet = "savedwifi"
    }
    val keyboardToken = rememberKeyboardActions { action ->
        when {
            action == FieldWire.CARD_SCAN && kind == "card" -> scanCard()
            action == FieldWire.WIFI_QR && kind == "wifi" -> wifiFromCode()
            action == FieldWire.WIFI_SAVED && kind == "wifi" -> wifiFromPhone()
        }
    }
    /** What the keyboard may offer in the field [key]. Only Google's reader reads a card's name and expiry. */
    fun keyboardActions(key: String?): List<String> = when (kind) {
        "card" -> when (key) {
            "number" -> listOf(FieldWire.CARD_SCAN)
            "holder", "expiry" ->
                if (BuildConfig.GOOGLE_SDKS && shell.prefs.cardReader.value == CardReaders.Google) listOf(FieldWire.CARD_SCAN) else emptyList()
            else -> emptyList()
        }
        "wifi" -> listOf(FieldWire.WIFI_QR, FieldWire.WIFI_SAVED)
        else -> emptyList()
    }

    LaunchedEffect(entryId, newKind) {
        if (entryId != null) {
            // Gone since the edit began, as an item another device deleted: back, not a crash.
            val d = try {
                Pass.detail(entryId)
            } catch (_: NativeFailure) {
                shell.notice("That item is no longer in this vault")
                shell.nav.popBackStack()
                return@LaunchedEffect
            }
            detail = d
            kind = d.kind
            title.setTextAndPlaceCursorAtEnd(d.title)
            fields.addAll(d.fields.map(::EditField))
            d.sites.forEach { raw ->
                if (isApp(raw)) {
                    apps.add(raw)
                } else {
                    val line = siteLine(raw)
                    if (line != null) sites.add(TextFieldState(line.label) to raw)
                }
            }
            links.addAll(d.links)
            remindDays = d.reminders.changeEveryDays
            d.customFields.forEach { customs.add(EditCustom(it, it.value == null)) }
            d.questions.forEachIndexed { index, q -> questions.add(EditQuestion(index, q)) }
            color = d.color
            icon = d.icon
            folder = d.folder
            favourite = d.favourite
            tags = d.tags
            otpOpen = d.otp != null
            // A note's body is what you came to edit, so it opens straight away.
            // One that cannot be read, as when the vault locks meanwhile, is said under it and left to a tap.
            fields.firstOrNull { it.spec.key == "body" && it.spec.hasValue }?.let { body ->
                try {
                    val text = Pass.reveal(entryId, "body").consumeText()
                    body.text.setTextAndPlaceCursorAtEnd(text)
                    body.original = text
                    body.loaded = true
                } catch (cancelled: kotlinx.coroutines.CancellationException) {
                    throw cancelled
                } catch (failure: Exception) {
                    error = failure.message ?: "Could not read this field"
                    errorAt = body
                }
            }
        } else {
            // Without the kind's table there is no form to fill: back, with a word, rather than a crash.
            val spec = try {
                Pass.kinds().firstOrNull { it.kind == newKind }
            } catch (cancelled: kotlinx.coroutines.CancellationException) {
                throw cancelled
            } catch (_: Exception) {
                shell.notice("Could not start a new item")
                shell.nav.popBackStack()
                return@LaunchedEffect
            }
            fields.addAll(
                spec?.fields.orEmpty().map { f ->
                    EditField(FieldView(f.key, f.label, f.input, f.secret, f.choices, f.suggestions)).also { field ->
                        if (f.input == "choice" && f.choices.isNotEmpty()) {
                            field.text.setTextAndPlaceCursorAtEnd(f.choices.first())
                            field.preset = f.choices.first()
                        }
                    }
                },
            )
            if (newKind == "login") sites.add(TextFieldState() to null)
            shell.pendingOtp?.let { link ->
                otpSeed.setTextAndPlaceCursorAtEnd(link)
                otpOpen = true
                shell.pendingOtp = null
            }
            shell.pendingNote?.let { text ->
                fields.firstOrNull { it.spec.key == "body" }?.text?.setTextAndPlaceCursorAtEnd(text)
                shell.pendingNote = null
            }
            shell.pendingTitle?.let { text ->
                title.setTextAndPlaceCursorAtEnd(text)
                shell.pendingTitle = null
            }
            shell.pendingPassword?.let { generated ->
                fields.firstOrNull { it.spec.input == "password" }?.text?.setTextAndPlaceCursorAtEnd(generated)
                shell.pendingPassword = null
            }
            shell.pendingWifi?.let { code ->
                applyWifi(code, fields, title)
                shell.pendingWifi = null
            }
        }
        ready = true
        if (entryId == null && presetTitle.isEmpty()) {
            delay(240)
            runCatching { titleFocus.requestFocus() }
        }
    }

    LaunchedEffect(ready, kind) {
        if (!ready || kind != "wifi") return@LaunchedEffect
        val password = fields.firstOrNull { it.spec.key == "password" } ?: return@LaunchedEffect
        val security = fields.firstOrNull { it.spec.key == "security" } ?: return@LaunchedEffect
        // Follows the password both ways: typing one means a key is needed, clearing it means none is.
        // The first value is what was saved, so an item opened for editing keeps what it had.
        androidx.compose.runtime.snapshotFlow { password.text.text.isNotEmpty() }.drop(1).collect { typed ->
            val now = security.text.text.toString()
            if (typed && now == "open") security.text.setTextAndPlaceCursorAtEnd("wpa")
            if (!typed && now in setOf("wpa", "sae", "wep")) security.text.setTextAndPlaceCursorAtEnd("open")
        }
    }

    /** A new SSH key pair in place of the fields' key, public half and fingerprint. */
    fun newKey(field: EditField) {
        shell.launch {
            val reply = com.wasimaster.wmpass.core.Engine.call("ssh.new", args = { put("comment", title.text.toString()) })
            field.text.setTextAndPlaceCursorAtEnd(reply.secret!!.consumeText())
            field.loaded = true
            fields.firstOrNull { it.spec.key == "public_key" }?.text?.setTextAndPlaceCursorAtEnd(reply.obj["public_key"]!!.jsonPrimitive.content)
            fields.firstOrNull { it.spec.key == "fingerprint" }?.text?.setTextAndPlaceCursorAtEnd(reply.obj["fingerprint"]!!.jsonPrimitive.content)
            shell.haptics.confirm()
        }
    }

    /** A code that wants a username and password too becomes a login, keeping its code. */
    fun becomeLogin() {
        scope.launch {
            // A table that cannot be read leaves the form as it is, rather than closing the app.
            val spec = runCatching { Pass.kinds() }.getOrNull()?.firstOrNull { it.kind == "login" } ?: return@launch
            fields.clear()
            fields.addAll(spec.fields.map { f -> EditField(FieldView(f.key, f.label, f.input, f.secret, f.choices, f.suggestions)) })
            if (sites.isEmpty()) sites.add(TextFieldState() to null)
            otpOpen = true
            kind = "login"
        }
    }

    val firstSite = sites.firstOrNull()?.first?.text?.toString().orEmpty()
    val derived = remember(firstSite) { titleFromSite(firstSite) }

    // The name, websites and apps suggest each other: Facebook brings messenger.com and the
    // Facebook, Lite and Messenger apps that are installed. Worked out on the phone, a moment
    // after typing stops.
    var related by remember { mutableStateOf<com.wasimaster.wmpass.system.Related.Found?>(null) }
    LaunchedEffect(ready, kind) {
        if (!ready || kind != "login") return@LaunchedEffect
        androidx.compose.runtime.snapshotFlow { Triple(title.text.toString(), sites.map { it.first.text.toString() }, apps.map(::appPackage)) }
            .collectLatest { (name, typed, chosen) ->
                delay(300)
                related = runCatching { com.wasimaster.wmpass.system.Related.find(context, name, typed, chosen) }.getOrNull()
            }
    }
    fun addApp(pkg: String, label: String) {
        if (apps.none { appPackage(it) == pkg }) {
            val cert = com.wasimaster.wmpass.system.Trust.certOf(context, pkg)
            apps.add(
                buildJsonObject {
                    put(
                        "AndroidApp",
                        buildJsonObject {
                            put("package", pkg)
                            if (cert != null) put("cert_sha256", cert) else put("cert_sha256", JsonNull)
                        },
                    )
                },
            )
        }
        if (title.text.isEmpty()) title.setTextAndPlaceCursorAtEnd(label)
    }
    fun addSite(site: String) {
        val blank = sites.firstOrNull { it.second == null && it.first.text.isBlank() }
        if (blank != null) blank.first.setTextAndPlaceCursorAtEnd(site) else sites.add(TextFieldState(site) to null)
    }

    val dirty = fields.any { it.changed && (it.preset == null || it.text.text.toString() != it.preset) } ||
        title.text.toString() != (detail?.title ?: presetTitle) ||
        sites.any { it.second == null && it.first.text.isNotBlank() } ||
        otpSeed.text.isNotEmpty() ||
        (notesLoaded && notes.text.toString() != notesOriginal) ||
        customs.any { it.existing == null || it.remove || it.valueChanged || it.metaChanged } ||
        questions.any { it.answer.text.isNotEmpty() || it.remove || it.reworded } ||
        color != detail?.color || icon != detail?.icon || folder != detail?.folder || favourite != (detail?.favourite ?: false) || tags != detail?.tags.orEmpty() ||
        apps.toList() != detail?.sites.orEmpty().filter(::isApp) || files.isNotEmpty() || links.toList() != detail?.links.orEmpty() ||
        remindDays != detail?.reminders?.changeEveryDays || (detail != null && kind != detail?.kind)

    fun leave() {
        // A save under way pops the editor itself when it is done; leaving as well would pop twice.
        if (saving) return
        if (dirty) askDiscard = true else shell.nav.popBackStack()
    }
    BackHandler(dirty || saving) { if (!saving) askDiscard = true }

    fun save() {
        if (saving) return
        saving = true
        error = null
        errorAt = null
        scope.launch {
            // The part of the form the step running now writes, so a failure can point at it.
            var step: Any? = null
            try {
                val finalTitle = title.text.toString().trim().ifEmpty { derived }.ifEmpty { "Untitled" }
                val plain = fields.filter { !it.spec.secret && it.changed }
                val newSites = buildList<JsonElement> {
                    sites.forEach { (state, raw) ->
                        val typed = state.text.toString().trim()
                        when {
                            typed.isEmpty() -> Unit
                            raw != null && siteLine(raw)?.label == typed -> add(raw)
                            else -> add(
                                buildJsonObject {
                                    put("Url", if ("://" in typed) typed else "https://$typed")
                                },
                            )
                        }
                    }
                    apps.forEach { add(it) }
                }
                val made = createdId
                val id = entryId ?: made ?: Pass.change("entry.create") {
                    put("kind", kind)
                    put("title", finalTitle)
                    color?.let { put("color", it) }
                    folder?.let { put("folder", it) }
                    put(
                        "fields",
                        buildJsonObject { plain.forEach { put(it.spec.key, jsonValue(it)) } },
                    )
                    putJsonArray("sites") { newSites.forEach { add(it) } }
                }.obj["id"]!!.jsonPrimitive.content.also { createdId = it }
                if (entryId != null && detail?.kind == "otp" && kind == "login") {
                    Pass.change("entry.convert") {
                        put("id", id)
                        put("kind", "login")
                    }
                }
                if ((entryId != null || made != null) && plain.isNotEmpty()) {
                    Pass.change("entry.set") {
                        put("id", id)
                        put("fields", buildJsonObject { plain.forEach { put(it.spec.key, jsonValue(it)) } })
                    }
                }
                fields.filter { it.spec.secret && it.changed }.forEach { field ->
                    step = field
                    val bytes = field.text.text.toString().toByteArray()
                    Pass.change("entry.set_secret", secret = bytes) {
                        put("id", id)
                        put("field", field.spec.key)
                    }
                    bytes.fill(0)
                }
                if (notesLoaded && notes.text.toString() != notesOriginal) {
                    step = "notes"
                    val bytes = notes.text.toString().toByteArray()
                    Pass.change("entry.set_secret", secret = bytes) {
                        put("id", id)
                        put("field", "notes")
                    }
                    bytes.fill(0)
                }
                if (otpSeed.text.isNotBlank()) {
                    step = "otp"
                    val bytes = otpSeed.text.toString().toByteArray()
                    Pass.change("entry.otp_set", secret = bytes) {
                        put("id", id)
                        if (kind == "otp") put("issuer", finalTitle)
                    }
                    bytes.fill(0)
                }
                // Changed before any is removed, while every index still points where it did.
                customs.filter { !it.remove && it.name.text.isNotBlank() && !(it in written && (it.existing == null || removedAny)) }.forEach { custom ->
                    step = custom
                    val existing = custom.existing
                    when {
                        existing == null && custom.value.text.isEmpty() -> Unit
                        existing == null || custom.valueChanged -> {
                            val bytes = custom.value.text.toString().toByteArray()
                            Pass.change("entry.custom_set", secret = bytes) {
                                put("id", id)
                                existing?.let { put("index", it.index) }
                                put("name", custom.name.text.toString().trim())
                                put("kind", custom.kind)
                            }
                            bytes.fill(0)
                            written.add(custom)
                        }
                        // Only its name or whether it is secret: the value stays in the vault untouched.
                        custom.metaChanged -> {
                            Pass.change("entry.custom_meta") {
                                put("id", id)
                                put("index", existing.index)
                                put("name", custom.name.text.toString().trim())
                                put("kind", custom.kind)
                            }
                            written.add(custom)
                        }
                    }
                }
                // Removed from the end first, so earlier indexes stay right.
                customs.filter { it.remove && it.existing != null && it !in written }.sortedByDescending { it.existing!!.index }.forEach { custom ->
                    step = null
                    Pass.change("entry.custom_remove") {
                        put("id", id)
                        put("index", custom.existing!!.index)
                    }
                    written.add(custom)
                    removedAny = true
                }
                questions.filter { !it.remove && it.question.text.isNotBlank() && it !in written }.forEach { q ->
                    step = q
                    if (q.answer.text.isNotEmpty()) {
                        val bytes = q.answer.text.toString().toByteArray()
                        Pass.change("entry.question_set", secret = bytes) {
                            put("id", id)
                            q.index?.let { put("index", it) }
                            put("question", q.question.text.toString().trim())
                        }
                        bytes.fill(0)
                        written.add(q)
                    } else if (q.reworded) {
                        // New wording only: the saved answer stays as it is.
                        Pass.change("entry.question_set") {
                            put("id", id)
                            put("index", q.index!!)
                            put("question", q.question.text.toString().trim())
                            put("keep_answer", true)
                        }
                        written.add(q)
                    }
                }
                // Removed last and from the end, so the indexes written above still pointed at the right ones.
                questions.filter { it.remove && it.index != null && it !in written }.sortedByDescending { it.index!! }.forEach { q ->
                    step = null
                    Pass.change("entry.question_remove") {
                        put("id", id)
                        put("index", q.index!!)
                    }
                    written.add(q)
                }
                step = null
                Pass.change("entry.common") {
                    put("id", id)
                    put("title", finalTitle)
                    if (color == null) put("color", JsonNull) else put("color", color)
                    if (icon == null) put("icon", JsonNull) else put("icon", icon)
                    if (folder == null) put("folder", JsonNull) else put("folder", folder)
                    put("favourite", favourite)
                    putJsonArray("tags") { tags.forEach { add(it) } }
                    if (kind == "login") putJsonArray("sites") { newSites.forEach { add(it) } }
                    if (links.toList() != detail?.links.orEmpty()) putJsonArray("links") { links.forEach { add(it) } }
                    if (remindDays != detail?.reminders?.changeEveryDays) {
                        put(
                            "reminders",
                            buildJsonObject {
                                remindDays?.let { put("change_every_days", it) }
                                detail?.reminders?.expiresMs?.let { put("expires_ms", it) }
                                put("warn_days", detail?.reminders?.warnDays ?: 0)
                            },
                        )
                    }
                }
                files.filter { it !in written }.forEach { file ->
                    step = file
                    // A file that opens to nothing, as one deleted since it was chosen, is an error
                    // under it, not an attachment quietly left out.
                    val bytes = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
                        context.contentResolver.openInputStream(file.uri)?.use { it.readBytes() }
                    } ?: throw java.io.FileNotFoundException(file.name)
                    Pass.addAttachment(id, file.name, file.mime, bytes)
                    bytes.fill(0)
                    written.add(file)
                }
                step = null
                // Tagged by its site, such as Banking or Social; the kind already says it is a card or a code.
                // A guess only when asked for: a wrong tag costs the owner a hunt to undo it.
                if (entryId == null && tags.isEmpty() && shell.prefs.autoTag.value) {
                    val site = runCatching {
                        val found = Pass.plainCall("categorise.suggest") { put("id", id) }.obj
                        val sure = found["site_source"]?.jsonPrimitive?.contentOrNull != "guess" || shell.prefs.autoTagGuess.value
                        found["site"]?.jsonPrimitive?.contentOrNull?.takeIf { sure }
                    }.getOrNull()
                    if (site != null) {
                        Pass.change("entry.common") {
                            put("id", id)
                            putJsonArray("tags") { add(site) }
                        }
                    }
                }
                shell.haptics.confirm()
                shell.nav.popBackStack()
                if (entryId == null) shell.open(id)
            } catch (failure: NativeFailure) {
                shell.haptics.reject()
                error = failure.message
                errorAt = step
                saving = false
            } catch (cancelled: kotlinx.coroutines.CancellationException) {
                throw cancelled
            } catch (failure: Exception) {
                // A file that can no longer be read, or a vault that locked meanwhile, says so instead of
                // closing the app with the form half saved.
                shell.haptics.reject()
                error = (step as? PendingFile)?.let { "Could not read ${it.name}" } ?: failure.message ?: "Could not save this item"
                errorAt = step
                saving = false
            }
        }
    }

    Box(Modifier.fillMaxSize().background(colors.ground)) {
        Column(Modifier.fillMaxSize().imePadding()) {
            if (colors.flat) {
                androidx.compose.material3.TopAppBar(
                    title = { Text(if (entryId == null) "New ${kindInSentence(kind)}" else "Edit ${kindInSentence(kind)}") },
                    navigationIcon = {
                        androidx.compose.material3.IconButton(onClick = { leave() }) { Icon(PassIcons.Close, "Close") }
                    },
                    actions = {
                        HelpAction("screen.editor", "the editor")
                        androidx.compose.material3.TextButton(onClick = { save() }, enabled = ready && !saving && (dirty || entryId == null), modifier = Modifier.helpAnchor("editor.save")) { Text("Save") }
                    },
                    colors = androidx.compose.material3.TopAppBarDefaults.topAppBarColors(containerColor = colors.ground),
                )
            }
            Column(
                Modifier
                    .weight(1f)
                    .verticalScroll(rememberScrollState())
                    .readableWidth()
                    .then(if (colors.flat) Modifier else Modifier.statusBarsPadding())
                    .padding(horizontal = 16.dp),
            ) {
                if (colors.flat) {
                    androidx.compose.material3.OutlinedTextField(
                        state = title,
                        modifier = Modifier.fillMaxWidth().focusRequester(titleFocus),
                        label = { Text("Name") },
                        placeholder = if (derived.isNotEmpty()) ({ Text(derived) }) else null,
                        lineLimits = TextFieldLineLimits.SingleLine,
                        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Words).offering(keyboardToken, keyboardActions(null)),
                    )
                    related?.name?.takeIf { !it.equals(derived, ignoreCase = true) }?.let { FieldSuggestions(title, listOf(it)) }
                } else {
                Spacer(Modifier.height(62.dp))
                Column(Modifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                    val seedSite = firstSite.trim().takeIf { it.isNotEmpty() }?.let { s -> runCatching { URI(if ("://" in s) s else "https://$s").host }.getOrNull() }
                    SiteTile(title.text.toString().ifEmpty { derived }, seedSite, kind, size = 72.dp, label = color, icon = icon)
                    Spacer(Modifier.height(14.dp))
                    Box(Modifier.fillMaxWidth().padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
                        BasicTextField(
                            title,
                            Modifier.fillMaxWidth().focusRequester(titleFocus),
                            decorator = com.wasimaster.wmpass.ui.parts.placeholderDecorator(title, name = "Name") {
                                Text(
                                    derived.ifEmpty { "Name" },
                                    color = colors.textLow,
                                    style = MaterialTheme.typography.headlineMedium,
                                    textAlign = TextAlign.Center,
                                )
                            },
                            textStyle = MaterialTheme.typography.headlineMedium.copy(color = colors.text, textAlign = TextAlign.Center),
                            cursorBrush = SolidColor(colors.primary),
                            lineLimits = TextFieldLineLimits.SingleLine,
                            keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Words).offering(keyboardToken, keyboardActions(null)),
                        )
                    }
                    Text(
                        if (entryId == null) "New ${kindInSentence(kind)}" else kindName(kind),
                        color = colors.textMid,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                    related?.name?.takeIf { !it.equals(derived, ignoreCase = true) }?.let { FieldSuggestions(title, listOf(it), center = true) }
                }
                }
                if (ready) {
                    val mainKeys = MainFields[kind].takeIf { shell.prefs.editorFoldDetails.value }
                    val fieldEditor: @Composable (EditField) -> Unit = { field ->
                                FieldEditor(
                                    entryId,
                                    field,
                                    context = listOfNotNull(title.text.toString().takeIf { it.isNotEmpty() }),
                                    onAlias = if (field.spec.key == "username") ({ aliasFor = field }) else null,
                                    onSsh = if (field.spec.key == "private_key") {
                                        {
                                            // A key already there, saved or typed, is asked about first: one tap
                                            // would otherwise replace it, and its public half, unseen.
                                            if ((field.spec.hasValue && !field.loaded) || field.text.text.isNotEmpty()) askNewKey = field else newKey(field)
                                        }
                                    } else {
                                        null
                                    },
                                    tag = { options -> options.offering(keyboardToken, keyboardActions(field.spec.key)) },
                                ) { generating = field }
                                FieldError(error.takeIf { errorAt === field })
                    }
                    val mainRows = fields.filter { mainKeys == null || it.spec.key in mainKeys }
                    val detailRows = fields.filter { mainKeys != null && it.spec.key !in mainKeys }
                    if (mainRows.isNotEmpty()) {
                        Spacer(Modifier.height(if (colors.flat) 4.dp else 18.dp))
                        GroupCard(plain = true) {
                            mainRows.forEachIndexed { index, field ->
                                if (index > 0) Hairline()
                                fieldEditor(field)
                            }
                            if (kind == "wifi") {
                                Hairline()
                                AddLine("Fill in from a QR code", PassIcons.QrCodeScanner) { wifiFromCode() }
                                Hairline()
                                AddLine("Pick a network saved on this phone", PassIcons.Wifi) { wifiFromPhone() }
                            }
                            if (kind == "card") {
                                Hairline()
                                AddLine("Scan the card", PassIcons.PhotoCamera) { scanCard() }
                            }
                        }
                    }
                    if (colors.flat) Spacer(Modifier.height(8.dp))
                    if (detailRows.isNotEmpty()) {
                        EditorFold("details", "Other details", detailRows.joinToString(", ") { it.spec.label }, false, opensFor = detailRows.any { it === errorAt }) {
                            detailRows.forEachIndexed { index, field ->
                                if (index > 0) Hairline()
                                fieldEditor(field)
                            }
                        }
                    }
                    val opened = shell.prefs.editorOpen
                    if (kind == "login") {
                        val firstWebsite = sites.firstOrNull()?.first?.text?.toString().orEmpty()
                        val appNames = apps.map { com.wasimaster.wmpass.system.Trust.labelOf(context, appPackage(it)) }
                        val offered = related?.let { it.sites + it.apps.map { app -> app.label } }.orEmpty()
                        val summary = (listOf(firstWebsite).filter { it.isNotEmpty() } + appNames).joinToString(", ").ifEmpty { null }
                            ?: offered.take(3).joinToString(", ").takeIf { it.isNotEmpty() }?.let { "Suggests $it" }
                        EditorFold("sites", "Websites and apps", summary, false) {
                            sites.forEachIndexed { index, (state, _) ->
                                if (index > 0) Hairline()
                                if (colors.flat) {
                                    androidx.compose.material3.OutlinedTextField(
                                        state = state,
                                        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                                        label = { Text("Website") },
                                        placeholder = { Text("example.com") },
                                        lineLimits = TextFieldLineLimits.SingleLine,
                                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false),
                                        trailingIcon = if (sites.size > 1 || state.text.isNotEmpty()) {
                                            { RowIconButton(PassIcons.Close, "Remove website") { sites.removeAt(index) } }
                                        } else {
                                            null
                                        },
                                    )
                                } else
                                Row(Modifier.padding(start = 18.dp, end = 8.dp, top = 6.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                                    Icon(PassIcons.Language, null, tint = colors.textMid, modifier = Modifier.size(20.dp))
                                    Spacer(Modifier.width(14.dp))
                                    Box(Modifier.weight(1f).heightIn(min = 40.dp), contentAlignment = Alignment.CenterStart) {
                                        BasicTextField(
                                            state,
                                            Modifier.fillMaxWidth(),
                                            decorator = com.wasimaster.wmpass.ui.parts.placeholderDecorator(state) { Text("example.com", color = colors.textLow, style = MaterialTheme.typography.bodyLarge) },
                                            textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.text),
                                            cursorBrush = SolidColor(colors.primary),
                                            lineLimits = TextFieldLineLimits.SingleLine,
                                            keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false),
                                        )
                                    }
                                    if (sites.size > 1 || state.text.isNotEmpty()) {
                                        RowIconButton(PassIcons.Close, "Remove website") { sites.removeAt(index) }
                                    }
                                }
                            }
                            if (sites.isNotEmpty()) Hairline()
                            apps.toList().forEach { raw ->
                                AppRow(appPackage(raw)) { apps.remove(raw) }
                                Hairline()
                            }
                            AddLine("Add a website") { sites.add(TextFieldState() to null) }
                            Hairline()
                            AddLine("Add an app") { sheet = "app" }
                            val offer = related
                            AnimatedVisibility(offer != null && (offer.sites.isNotEmpty() || offer.apps.isNotEmpty())) {
                                Column(Modifier.padding(start = 16.dp, end = 16.dp, top = 4.dp, bottom = 14.dp)) {
                                    Row(Modifier.padding(bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                                        Icon(PassIcons.AutoAwesome, null, tint = colors.textMid, modifier = Modifier.size(16.dp))
                                        Spacer(Modifier.width(6.dp))
                                        Text("Goes with this login", color = colors.textMid, style = MaterialTheme.typography.labelLarge)
                                    }
                                    FlowRow(
                                        horizontalArrangement = Arrangement.spacedBy(8.dp),
                                        verticalArrangement = Arrangement.spacedBy(8.dp),
                                        itemVerticalAlignment = Alignment.CenterVertically,
                                    ) {
                                        offer?.sites?.forEach { site -> Pill(site, false, icon = PassIcons.Language) { addSite(site) } }
                                        offer?.apps?.forEach { app -> Pill(app.label, false, icon = PassIcons.Smartphone) { addApp(app.pkg, app.label) } }
                                    }
                                }
                            }
                        }
                    }
                    // Passkeys are made by the site, so they are shown here but cannot be typed in.
                    if (kind == "login") {
                        val keys = detail?.passkeys.orEmpty()
                        EditorFold("passkeys", "Passkeys", if (keys.isEmpty()) "None" else "${keys.size}", false) {
                            Column(Modifier.padding(horizontal = 16.dp, vertical = 8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                                keys.forEach { key ->
                                    Row(verticalAlignment = Alignment.CenterVertically) {
                                        Icon(PassIcons.KeyVertical, null, tint = colors.textMid, modifier = Modifier.size(20.dp))
                                        Spacer(Modifier.width(12.dp))
                                        Column(Modifier.weight(1f)) {
                                            Text(key.rpId, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
                                            Text(listOf(key.userName, if (key.createdMs > 0) "made ${whenText(key.createdMs)}" else "").filter { it.isNotEmpty() }.joinToString(", "), color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1)
                                        }
                                    }
                                }
                                Text(
                                    "A site adds a passkey when you create one there with WM Pass as your passkey provider, on Android 14 and up. A passkey cannot be typed in, copied or changed, since its private half never leaves WM Pass.",
                                    color = colors.textLow,
                                    style = MaterialTheme.typography.bodySmall,
                                )
                            }
                        }
                    }
                    val otpShown = kind == "otp" || otpOpen || detail?.otp != null || "otp" in added || "otp" in opened
                    if (kind == "login" || kind == "otp") Appear(otpShown) {
                        EditorFold("otp", "One-time code", if (detail?.otp != null) "Saved" else null, kind == "otp" || otpOpen || "otp" in added, opensFor = errorAt == "otp") {
                            Box {
                                if (colors.flat) {
                                    androidx.compose.material3.OutlinedTextField(
                                        state = otpSeed,
                                        modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                                        label = { Text("Setup key or otpauth link") },
                                        placeholder = { Text(if (detail?.otp != null) "Saved. Type to replace" else "JBSWY3DPEHPK3PXP") },
                                        textStyle = TextStyle(fontFamily = MonoFamily, fontSize = 16.sp),
                                        lineLimits = TextFieldLineLimits.SingleLine,
                                        keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
                                        trailingIcon = {
                                            RowIconButton(PassIcons.QrCodeScanner, "Scan a QR code", tint = colors.primary) {
                                                shell.scanTarget = { uri -> otpSeed.setTextAndPlaceCursorAtEnd(uri) }
                                            }
                                        },
                                    )
                                } else
                                Column(Modifier.padding(start = 18.dp, end = 12.dp, top = 12.dp, bottom = 12.dp)) {
                                    Text("Setup key or otpauth link", color = colors.textLow, style = MaterialTheme.typography.labelMedium)
                                    Row(verticalAlignment = Alignment.CenterVertically) {
                                        Box(Modifier.weight(1f).heightIn(min = 44.dp), contentAlignment = Alignment.CenterStart) {
                                            BasicTextField(
                                                otpSeed,
                                                Modifier.fillMaxWidth(),
                                                decorator = com.wasimaster.wmpass.ui.parts.placeholderDecorator(otpSeed) {
                                                    Text(
                                                        if (detail?.otp != null) "Saved. Type to replace it" else "JBSWY3DPEHPK3PXP",
                                                        color = colors.textLow,
                                                        style = MaterialTheme.typography.bodyLarge,
                                                    )
                                                },
                                                textStyle = TextStyle(fontFamily = MonoFamily, fontSize = 16.sp, color = colors.text),
                                                cursorBrush = SolidColor(colors.primary),
                                                lineLimits = TextFieldLineLimits.SingleLine,
                                                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false),
                                            )
                                        }
                                        Spacer(Modifier.width(8.dp))
                                        RowIconButton(PassIcons.QrCodeScanner, "Scan a QR code", tint = colors.primary) {
                                            shell.scanTarget = { uri -> otpSeed.setTextAndPlaceCursorAtEnd(uri) }
                                        }
                                    }
                                }
                            }
                            FieldError(error.takeIf { errorAt == "otp" })
                        }
                    }
                    val notesShown = detail?.hasNotes == true || notes.text.isNotEmpty() || "notes" in added || "notes" in opened
                    if (kind != "note") Appear(notesShown) {
                        EditorFold("notes", "Notes", if (detail?.hasNotes == true) "Saved" else null, "notes" in added, opensFor = errorAt == "notes") {
                            if ((notesLoaded || detail?.hasNotes != true) && colors.flat) {
                                if (!notesLoaded) notesLoaded = true
                                androidx.compose.material3.OutlinedTextField(
                                    state = notes,
                                    modifier = Modifier.fillMaxWidth().padding(horizontal = 16.dp, vertical = 4.dp),
                                    label = { Text("Notes") },
                                    lineLimits = TextFieldLineLimits.MultiLine(minHeightInLines = 3),
                                    keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                                )
                            } else if (notesLoaded) {
                                Box(Modifier.padding(horizontal = 18.dp, vertical = 14.dp).heightIn(min = 54.dp)) {
                                    BasicTextField(
                                        notes,
                                        Modifier.fillMaxWidth(),
                                        decorator = com.wasimaster.wmpass.ui.parts.placeholderDecorator(notes) { Text("Anything worth remembering", color = colors.textLow, style = MaterialTheme.typography.bodyLarge) },
                                        textStyle = MaterialTheme.typography.bodyLarge.copy(color = colors.text),
                                        cursorBrush = SolidColor(colors.primary),
                                        keyboardOptions = KeyboardOptions(capitalization = KeyboardCapitalization.Sentences),
                                    )
                                }
                            } else {
                                AddLine(if (detail?.hasNotes == true) "Edit notes" else "Add notes", if (detail?.hasNotes == true) PassIcons.Edit else PassIcons.Add) {
                                    val existing = detail
                                    scope.launch {
                                        try {
                                            if (existing?.hasNotes == true) {
                                                val text = Pass.reveal(existing.id, "notes").consumeText()
                                                notes.setTextAndPlaceCursorAtEnd(text)
                                                notesOriginal = text
                                            }
                                            notesLoaded = true
                                            if (errorAt == "notes") error = null
                                        } catch (cancelled: kotlinx.coroutines.CancellationException) {
                                            throw cancelled
                                        } catch (failure: Exception) {
                                            error = failure.message ?: "Could not read the notes"
                                            errorAt = "notes"
                                        }
                                    }
                                }
                            }
                            FieldError(error.takeIf { errorAt == "notes" })
                        }
                    }
                    val customCount = customs.count { !it.remove }
                    Appear(customCount > 0 || "custom" in added || "custom" in opened) {
                        EditorFold("custom", "Custom fields", if (customCount == 0) null else "$customCount", "custom" in added, opensFor = customs.any { it === errorAt }) {
                            val first = customs.indexOfFirst { !it.remove }
                            customs.forEachIndexed { index, custom ->
                                // New ones grow in and removed ones fold away, rather than jumping.
                                val shown = remember(custom) { androidx.compose.animation.core.MutableTransitionState(custom.existing != null) }
                                shown.targetState = !custom.remove
                                AnimatedVisibility(
                                    shown,
                                    enter = androidx.compose.animation.expandVertically(Springs.glide()) + androidx.compose.animation.fadeIn(),
                                    exit = androidx.compose.animation.shrinkVertically(Springs.glide()) + androidx.compose.animation.fadeOut(),
                                ) {
                                    Column {
                                        if (index > first) Hairline()
                                        CustomEditor(entryId, custom)
                                        FieldError(error.takeIf { errorAt === custom })
                                    }
                                }
                            }
                            if (customCount > 0) Hairline()
                            AddLine("Add a text field", PassIcons.TextFields) { customs.add(EditCustom(null, false)) }
                            Hairline()
                            AddLine("Add a secret field", PassIcons.Password) { customs.add(EditCustom(null, true)) }
                        }
                    }
                    val questionCount = questions.count { !it.remove }
                    if (kind == "login") Appear(questionCount > 0 || "questions" in added || "questions" in opened) {
                        EditorFold("questions", "Security questions", if (questionCount == 0) null else "$questionCount", "questions" in added, opensFor = questions.any { it === errorAt }) {
                            val first = questions.indexOfFirst { !it.remove }
                            questions.forEachIndexed { index, q ->
                                if (q.remove) return@forEachIndexed
                                if (index > first) Hairline()
                                Row(Modifier.padding(start = 18.dp, end = 6.dp, top = 12.dp, bottom = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                                    Column(Modifier.weight(1f)) {
                                        PlainField(q.question, "Question")
                                        Spacer(Modifier.height(8.dp))
                                        // An answer is as good as a password to whoever reads it over a shoulder.
                                        SecretLine(q.answer, if (q.index != null) "Saved. Type to replace the answer" else "Answer", q.showing)
                                    }
                                    Column {
                                        RowIconButton(PassIcons.Close, "Remove question") {
                                            if (q.index == null) questions.remove(q) else q.remove = true
                                        }
                                        RowIconButton(if (q.showing) PassIcons.VisibilityOff else PassIcons.Visibility, if (q.showing) "Hide answer" else "Show answer") { q.showing = !q.showing }
                                    }
                                }
                                FieldError(error.takeIf { errorAt === q })
                            }
                            if (questionCount > 0) Hairline()
                            AddLine("Add a question") { questions.add(EditQuestion(null, "")) }
                        }
                    }
                    val hasPasswordField = fields.any { it.spec.input == "password" }
                    Appear(hasPasswordField && (remindDays != null || "reminder" in added)) {
                        EditorFold("reminder", "Change reminder", remindDays?.let { if (it == 1) "Every day" else "Every $it days" } ?: "Off", "reminder" in added) {
                            FlowRow(
                                Modifier.padding(horizontal = 16.dp, vertical = 12.dp),
                                horizontalArrangement = Arrangement.spacedBy(8.dp),
                                verticalArrangement = Arrangement.spacedBy(8.dp),
                            ) {
                                Pill("Off", remindDays == null) { remindDays = null }
                                (listOf(30, 90, 180, 365) + listOfNotNull(detail?.reminders?.changeEveryDays)).distinct().sorted().forEach { days -> Pill(if (days == 1) "Every day" else "Every $days days", remindDays == days) { remindDays = days } }
                            }
                        }
                    }
                    Appear(links.isNotEmpty() || "links" in added) {
                        val everything by Pass.entries.collectAsStateWithLifecycle()
                        EditorFold("links", "Linked items", if (links.isEmpty()) null else "${links.size}", "links" in added) {
                            links.toList().forEach { linked ->
                                val other = everything.firstOrNull { it.id == linked }
                                Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 6.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                                    SiteTile(other?.title.orEmpty(), other?.site, other?.kind.orEmpty(), size = 34.dp, label = other?.color, icon = other?.icon)
                                    Spacer(Modifier.width(14.dp))
                                    Text(other?.title ?: "An item no longer here", color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                                    RowIconButton(PassIcons.Close, "Unlink") { links.remove(linked) }
                                }
                                Hairline()
                            }
                            AddLine("Link another item") { sheet = "link" }
                        }
                    }
                    Appear(files.isNotEmpty()) {
                        EditorFold("files", "Files to attach", "${files.size}", true, opensFor = files.any { it === errorAt }) {
                            files.toList().forEach { file ->
                                Row(Modifier.fillMaxWidth().padding(start = 18.dp, end = 8.dp, top = 6.dp, bottom = 6.dp), verticalAlignment = Alignment.CenterVertically) {
                                    Icon(if (file.mime.startsWith("image/")) PassIcons.Image else PassIcons.AttachFile, null, tint = colors.textMid, modifier = Modifier.size(20.dp))
                                    Spacer(Modifier.width(14.dp))
                                    Text(file.name, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, modifier = Modifier.weight(1f))
                                    RowIconButton(PassIcons.Close, "Leave out ${file.name}") { files.remove(file) }
                                }
                                FieldError(error.takeIf { errorAt === file })
                                Hairline()
                            }
                            AddLine("Attach another file") { pickFile.launch(arrayOf("*/*")) }
                        }
                    }
                    EditorFold("organise", "Organize", "Folder, tags, icon, color and favorite", false) {
                        val folders by Pass.folders.collectAsStateWithLifecycle()
                        OptionLine(PassIcons.Folder, "Folder", folders.firstOrNull { it.id == folder }?.name ?: "None") { sheet = "folder" }
                        Hairline(inset = 58.dp)
                        OptionLine(PassIcons.Sell, "Tags", if (tags.isEmpty()) "None" else tags.joinToString(", ")) { sheet = "tags" }
                        Hairline(inset = 58.dp)
                        OptionLine(PassIcons.Image, "Icon", if (icon == null) "Automatic" else "Your choice") { sheet = "icon" }
                        Hairline(inset = 58.dp)
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .clickable { sheet = "colour" }
                                .padding(horizontal = 18.dp, vertical = 14.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(PassIcons.Palette, null, tint = colors.textMid, modifier = Modifier.size(22.dp))
                            Spacer(Modifier.width(18.dp))
                            Text("Color", color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                            val hue = color?.toFloatOrNull()
                            if (hue != null) {
                                Box(
                                    Modifier
                                        .size(22.dp)
                                        .clip(CircleShape)
                                        .background(Brush.linearGradient(listOf(oklch(0.70f, 0.14f, hue), oklch(0.52f, 0.15f, hue + 28f)))),
                                )
                            } else {
                                Text("From the site", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                            }
                        }
                        Hairline(inset = 58.dp)
                        // One switch to a screen reader: the row toggles, and the switch only shows it.
                        Row(
                            Modifier
                                .fillMaxWidth()
                                .toggleable(favourite, role = androidx.compose.ui.semantics.Role.Switch) { favourite = it }
                                .padding(start = 18.dp, end = 14.dp, top = 8.dp, bottom = 8.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Icon(if (favourite) PassIcons.StarFilled else PassIcons.Star, null, tint = if (favourite) colors.warn else colors.textMid, modifier = Modifier.size(22.dp))
                            Spacer(Modifier.width(18.dp))
                            Text("Favorite", color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                            PassSwitch(favourite, null)
                        }
                    }
                    // Everything an item can also hold waits behind one line instead of a stack of empty sections.
                    if (!colors.flat) Spacer(Modifier.height(10.dp)) else Spacer(Modifier.height(8.dp))
                    GroupCard(plain = true) { AddLine("Add more", PassIcons.Add) { sheet = "more" } }
                }
                // The standard look has no bar below the form, so its end clears the navigation bar
                // itself; the error line, when there is one, already does.
                Spacer((if (colors.flat && !(error != null && errorAt == null)) Modifier.navigationBarsPadding() else Modifier).height(24.dp))
            }
            // Saving stays under the thumb, above the keyboard when there is one.
            if (colors.flat && error != null && errorAt == null) {
                Text(error.orEmpty(), color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.announced().navigationBarsPadding().padding(16.dp))
            }
            if (!colors.flat) Column(Modifier.navigationBarsPadding().padding(start = 16.dp, end = 16.dp, top = 6.dp, bottom = 10.dp)) {
                AnimatedVisibility(error != null && errorAt == null) {
                    Text(
                        error.orEmpty(),
                        color = colors.danger,
                        style = MaterialTheme.typography.bodyMedium,
                        modifier = Modifier.fillMaxWidth().announced().padding(start = 6.dp, end = 6.dp, bottom = 8.dp),
                    )
                }
                PrimaryButton(if (entryId == null) "Save" else "Save changes", Modifier.fillMaxWidth().helpAnchor("editor.save"), enabled = ready && !saving && (dirty || entryId == null)) { save() }
            }
        }
        if (!colors.flat) {
            Box(Modifier.fillMaxWidth().statusBarsPadding().padding(horizontal = 14.dp, vertical = 8.dp)) {
                GlassCircle(PassIcons.Close, "Close") { leave() }
                HelpAction("screen.editor", "the editor", Modifier.align(Alignment.CenterEnd), tint = colors.text)
            }
        }
    }

    when (sheet) {
        "more" -> PassSheet({ sheet = null }, "Add to this item") {
            fun pick(action: () -> Unit) {
                sheet = null
                action()
            }
            if (kind == "otp") {
                SheetRow(PassIcons.Person, "Username and password", "Keeps the code, and adds the login it belongs to") { pick { becomeLogin() } }
            }
            if (kind == "login" && !(otpOpen || detail?.otp != null || "otp" in added)) {
                SheetRow(PassIcons.Pin, "One-time code", "A setup key or QR code from the site") { pick { added.add("otp") } }
            }
            if (kind != "note" && !(detail?.hasNotes == true || notes.text.isNotEmpty() || "notes" in added)) {
                SheetRow(PassIcons.StickyNote2, "Notes", "Anything worth remembering, kept encrypted") { pick { added.add("notes") } }
            }
            SheetRow(PassIcons.TextFields, "Text field", "A name and a value you can see") {
                pick {
                    customs.add(EditCustom(null, false))
                    added.add("custom")
                }
            }
            SheetRow(PassIcons.Password, "Secret field", "Hidden until you tap to show it") {
                pick {
                    customs.add(EditCustom(null, true))
                    added.add("custom")
                }
            }
            if (kind == "login") {
                SheetRow(PassIcons.Help, "Security question", "A question the site asks, and your answer") {
                    pick {
                        questions.add(EditQuestion(null, ""))
                        added.add("questions")
                    }
                }
                SheetRow(PassIcons.Widgets, "App", "Fill this login in an Android app too") { sheet = "app" }
            }
            SheetRow(PassIcons.AttachFile, "File", "Kept encrypted with the item") { pick { pickFile.launch(arrayOf("*/*")) } }
            SheetRow(PassIcons.Link, "Link to another item", "Such as a card to the bank's login") {
                added.add("links")
                sheet = "link"
            }
            if (fields.any { it.spec.input == "password" } && remindDays == null && "reminder" !in added) {
                SheetRow(PassIcons.Snooze, "Change reminder", "A nudge to change the password now and then") { pick { added.add("reminder") } }
            }
        }
        "app" -> AppPickerSheet({ sheet = null }) { pkg, label -> addApp(pkg, label) }
        "link" -> PassSheet({ sheet = null }, "Link to") {
            val everything by Pass.entries.collectAsStateWithLifecycle()
            // Searched, since only the first few dozen are listed: any item must be reachable.
            val query = androidx.compose.foundation.text.input.rememberTextFieldState()
            SheetField(query, "Search items")
            Spacer(Modifier.height(8.dp))
            val typed = query.text.toString().trim()
            val matching = everything.filter { it.id != entryId && (typed.isEmpty() || it.title.contains(typed, true) || it.subtitle.contains(typed, true)) }
            // An empty list says why, rather than leaving a blank sheet under the search.
            if (matching.isEmpty()) {
                Text(
                    if (typed.isEmpty()) "There are no other items to link to yet." else "No item matches “$typed”.",
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(8.dp),
                )
            }
            matching.take(80).forEach { other ->
                SheetRow(com.wasimaster.wmpass.ui.parts.kindIcon(other.kind), other.title, other.subtitle.ifEmpty { null }, selected = other.id in links) {
                    if (!links.remove(other.id)) links.add(other.id)
                }
            }
        }
        "savedwifi" -> SavedWifiSheet({ sheet = null }, onPick = { network ->
            fillWifi(fields, title, network.ssid, network.security, network.password, network.hidden)
        })
        "folder" -> FolderSheet(folder, { sheet = null }) { folder = it }
        "colour" -> ColourSheet(color, { sheet = null }) { color = it }
        "icon" -> {
            val seed = firstSite.trim().takeIf { it.isNotEmpty() }?.let { typed -> runCatching { URI(if ("://" in typed) typed else "https://$typed").host }.getOrNull() }
            IconSheet(icon, { sheet = null }, title.text.toString().ifEmpty { derived }, seed, kind, color) { icon = it }
        }
        "tags" -> TagSheet(tags, { sheet = null }) { tag, on -> tags = if (on) (tags + tag).distinct() else tags - tag }
    }
    aliasFor?.let { field ->
        com.wasimaster.wmpass.ui.settings.AliasPickerSheet(title.text.toString(), { aliasFor = null }) { address ->
            field.text.setTextAndPlaceCursorAtEnd(address)
            aliasFor = null
        }
    }
    generating?.let { field ->
        GeneratorSheet(
            onDismiss = { generating = null },
        ) { value ->
            field.loaded = true
            field.text.setTextAndPlaceCursorAtEnd(value)
            field.showing = true
            generating = null
        }
    }
    askNewKey?.let { field ->
        ConfirmSheet(
            "Replace this key?",
            "A new key pair takes the place of the one here once you save. Servers that know the old public key will not accept the new one until you add it there.",
            "Make a new key",
            icon = PassIcons.Key,
            onDismiss = { askNewKey = null },
        ) {
            askNewKey = null
            newKey(field)
        }
    }
    if (askDiscard) {
        ConfirmSheet("Discard changes?", if (entryId == null) "This new item has not been saved and will be lost." else "Your edits to this item will be lost.", "Discard", icon = PassIcons.Close, onDismiss = { askDiscard = false }) {
            shell.nav.popBackStack()
        }
    }
}

/** Each kind's everyday fields. The rest wait under "More details". */
private val MainFields = mapOf(
    "card" to setOf("holder", "number", "expiry", "cvv"),
    "identity" to setOf("full_name", "email", "phone"),
    "ssh" to setOf("private_key", "public_key"),
    "api" to setOf("provider", "key"),
    "database" to setOf("engine", "host", "port", "name", "user", "password"),
    "licence" to setOf("product", "key"),
    "recovery" to setOf("form", "phrase"),
    "wifi" to setOf("ssid", "password"),
)

private val EditField.hasContent: Boolean
    get() = !loaded || text.text.isNotEmpty() || flag || millis != null

/**
 * A part of the form that stays out of the way until it is wanted. Every fold starts closed, with
 * its line saying what is inside, unless it was just asked for under Add more ([startOpen]) or
 * the owner keeps it open. It also opens when a save failed on something inside it ([opensFor]),
 * so the error under that field can be seen.
 */
@Composable
private fun EditorFold(key: String, title: String, summary: String?, startOpen: Boolean, opensFor: Boolean = false, content: @Composable () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val prefs = shell.prefs
    var open by androidx.compose.runtime.saveable.rememberSaveable(key) { mutableStateOf(startOpen || key in prefs.editorOpen) }
    LaunchedEffect(opensFor) { if (opensFor) open = true }
    val turn by androidx.compose.animation.core.animateFloatAsState(if (open) 180f else 0f, Springs.glide(), label = "fold")
    if (!colors.flat) Spacer(Modifier.height(10.dp))
    GroupCard(plain = true) {
        Row(
            Modifier
                .fillMaxWidth()
                // Read as open or folded, and what a tap will do, rather than through the chevron's name.
                .semantics { stateDescription = if (open) "Expanded" else "Collapsed" }
                .clickable(onClickLabel = if (open) "Fold" else "Open") {
                    shell.haptics.tick()
                    open = !open
                    if (prefs.editorRemember.value) prefs.setEditorOpen(key, open)
                }
                .padding(start = 16.dp, end = 12.dp, top = 14.dp, bottom = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge)
                if (!open && summary != null) {
                    Text(summary, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = androidx.compose.ui.text.style.TextOverflow.Ellipsis)
                }
            }
            Icon(
                PassIcons.ExpandMore,
                null,
                tint = colors.textMid,
                modifier = Modifier.graphicsLayer { rotationZ = turn },
            )
        }
        AnimatedVisibility(
            open,
            enter = androidx.compose.animation.expandVertically(Springs.glide()) + androidx.compose.animation.fadeIn(),
            exit = androidx.compose.animation.shrinkVertically(Springs.glide()) + androidx.compose.animation.fadeOut(),
        ) {
            Column {
                content()
                Spacer(Modifier.height(4.dp))
            }
        }
    }
}

/**
 * Why a save failed, under the field it failed on, such as a setup key that is not a real key.
 * It scrolls into sight, since Save may be far below the field.
 */
@Composable
private fun FieldError(message: String?) {
    val colors = palette
    val requester = remember { BringIntoViewRequester() }
    LaunchedEffect(message) {
        if (message != null) {
            // Waits for its fold to open first.
            delay(320)
            runCatching { requester.bringIntoView() }
        }
    }
    AnimatedVisibility(message != null) {
        Text(
            message.orEmpty(),
            color = colors.danger,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.fillMaxWidth().announced().bringIntoViewRequester(requester).padding(start = 18.dp, end = 16.dp, top = 2.dp, bottom = 10.dp),
        )
    }
}

/** A part of the form that slides in when it is first asked for. */
@Composable
private fun Appear(visible: Boolean, content: @Composable () -> Unit) {
    AnimatedVisibility(
        visible,
        enter = androidx.compose.animation.expandVertically(Springs.glide()) + androidx.compose.animation.fadeIn(),
        exit = androidx.compose.animation.shrinkVertically(Springs.glide()) + androidx.compose.animation.fadeOut(),
    ) { Column { content() } }
}

/** An app this login fills, with its own icon. */
@Composable
private fun AppRow(pkg: String, onRemove: () -> Unit) {
    val colors = palette
    val context = androidx.compose.ui.platform.LocalContext.current
    val label = remember(pkg) { com.wasimaster.wmpass.system.Trust.labelOf(context, pkg) }
    Row(Modifier.fillMaxWidth().padding(start = 16.dp, end = 8.dp, top = 8.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        AppIcon(pkg, 30.dp)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(label, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
            if (label != pkg) Text(pkg, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1)
        }
        RowIconButton(PassIcons.Close, "Remove $label", onClick = onRemove)
    }
}

@Composable
private fun AppIcon(pkg: String, size: androidx.compose.ui.unit.Dp) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val icon by androidx.compose.runtime.produceState<androidx.compose.ui.graphics.ImageBitmap?>(null, pkg) {
        value = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            runCatching {
                context.packageManager.getApplicationIcon(pkg).toBitmap(96, 96).asImageBitmap()
            }.getOrNull()
        }
    }
    Box(Modifier.size(size), contentAlignment = Alignment.Center) {
        val drawn = icon
        if (drawn != null) {
            androidx.compose.foundation.Image(drawn, null, Modifier.fillMaxSize())
        } else {
            Icon(PassIcons.Widgets, null, tint = palette.textMid, modifier = Modifier.size(size * 0.7f))
        }
    }
}

/** The apps on this phone, to pick the one a login is for. */
@Composable
private fun AppPickerSheet(onDismiss: () -> Unit, onPick: (String, String) -> Unit) {
    val colors = palette
    val context = androidx.compose.ui.platform.LocalContext.current
    var all by remember { mutableStateOf<List<Pair<String, String>>?>(null) }
    val query = androidx.compose.foundation.text.input.rememberTextFieldState()
    LaunchedEffect(Unit) {
        all = kotlinx.coroutines.withContext(kotlinx.coroutines.Dispatchers.IO) {
            val pm = context.packageManager
            val launcher = android.content.Intent(android.content.Intent.ACTION_MAIN).addCategory(android.content.Intent.CATEGORY_LAUNCHER)
            pm.queryIntentActivities(launcher, 0)
                .map { it.activityInfo.applicationInfo }
                .distinctBy { it.packageName }
                .filter { it.packageName != context.packageName }
                .map { it.packageName to it.loadLabel(pm).toString() }
                .sortedBy { it.second.lowercase() }
        }
    }
    PassSheet(onDismiss, "Choose an app") {
        SheetField(query, "Search apps")
        Spacer(Modifier.height(10.dp))
        val typed = query.text.toString().trim()
        val shown = all?.filter { (pkg, label) -> typed.isEmpty() || label.contains(typed, true) || pkg.contains(typed, true) }
        when {
            shown == null -> Text("Looking at your apps…", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(8.dp))
            shown.isEmpty() -> Text("No app matches “$typed”.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(8.dp))
            else -> androidx.compose.foundation.lazy.LazyColumn(Modifier.fillMaxWidth().heightIn(max = 480.dp)) {
                items(shown.size, key = { shown[it].first }) { index ->
                    val (pkg, label) = shown[index]
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clip(com.wasimaster.wmpass.ui.theme.Shapes.tile)
                            .clickable {
                                onPick(pkg, label)
                                onDismiss()
                            }
                            .padding(vertical = 8.dp, horizontal = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        AppIcon(pkg, 40.dp)
                        Spacer(Modifier.width(14.dp))
                        Column(Modifier.weight(1f)) {
                            Text(label, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
                            Text(pkg, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1)
                        }
                    }
                }
            }
        }
    }
}

/** Common values for a field, as chips under it, narrowed by what has been typed. */
@Composable
private fun FieldSuggestions(state: TextFieldState, options: List<String>, center: Boolean = false) {
    val typed = state.text.toString().trim()
    val shown = remember(typed) {
        if (options.any { it.equals(typed, ignoreCase = true) }) emptyList() else options.filter { typed.isEmpty() || it.contains(typed, ignoreCase = true) }
    }
    AnimatedVisibility(shown.isNotEmpty()) {
        Row(
            Modifier
                .fillMaxWidth()
                .horizontalScroll(rememberScrollState())
                .padding(top = 8.dp, bottom = 4.dp),
            horizontalArrangement = Arrangement.spacedBy(8.dp, if (center) Alignment.CenterHorizontally else Alignment.Start),
        ) {
            shown.forEach { option -> Pill(option, false) { state.setTextAndPlaceCursorAtEnd(option) } }
        }
    }
}

private fun jsonValue(field: EditField): JsonElement = when (field.spec.input) {
    "toggle" -> JsonPrimitive(field.flag)
    "date" -> field.millis?.let { JsonPrimitive(it) } ?: JsonNull
    "number" -> field.text.text.toString().trim().toLongOrNull()?.let { JsonPrimitive(it) } ?: JsonNull
    else -> JsonPrimitive(field.text.text.toString())
}

@Composable
private fun AddLine(text: String, icon: androidx.compose.ui.graphics.vector.ImageVector = PassIcons.Add, onClick: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    Row(
        Modifier
            .fillMaxWidth()
            .clickable {
                shell.haptics.tick()
                onClick()
            }
            .padding(horizontal = 18.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = colors.primary, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(14.dp))
        Text(text, color = colors.primary, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.Medium))
    }
}

@Composable
private fun OptionLine(icon: androidx.compose.ui.graphics.vector.ImageVector, title: String, value: String, onClick: () -> Unit) {
    val colors = palette
    Row(
        Modifier
            .fillMaxWidth()
            .clickable(onClick = onClick)
            .padding(horizontal = 18.dp, vertical = 14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = colors.textMid, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(18.dp))
        Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge)
        Spacer(Modifier.width(12.dp))
        Text(value, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 1, textAlign = TextAlign.End, modifier = Modifier.weight(1f))
    }
}

@Composable
fun PassSwitch(checked: Boolean, onChange: ((Boolean) -> Unit)?, modifier: Modifier = Modifier) {
    val colors = palette
    Switch(
        checked,
        onChange,
        modifier = modifier,
        colors = SwitchDefaults.colors(
            checkedTrackColor = colors.primary,
            checkedThumbColor = colors.onPrimary,
            uncheckedTrackColor = colors.surface3,
            uncheckedBorderColor = Color.Transparent,
            uncheckedThumbColor = colors.textMid,
        ),
    )
}

@Composable
private fun PlainField(state: TextFieldState, hint: String, mono: Boolean = false, keyboardOptions: KeyboardOptions = KeyboardOptions.Default) {
    val colors = palette
    if (colors.flat) {
        androidx.compose.material3.OutlinedTextField(
            state = state,
            modifier = Modifier.fillMaxWidth(),
            label = { Text(hint) },
            textStyle = if (mono) TextStyle(fontFamily = MonoFamily, fontSize = 16.sp) else MaterialTheme.typography.bodyLarge,
            keyboardOptions = keyboardOptions,
        )
        return
    }
    Box(Modifier.fillMaxWidth().heightIn(min = 28.dp), contentAlignment = Alignment.CenterStart) {
        BasicTextField(
            state,
            Modifier.fillMaxWidth(),
            decorator = com.wasimaster.wmpass.ui.parts.placeholderDecorator(state) { Text(hint, color = colors.textLow, style = MaterialTheme.typography.bodyLarge) },
            textStyle = if (mono) {
                TextStyle(fontFamily = MonoFamily, fontSize = 16.sp, color = colors.text)
            } else {
                MaterialTheme.typography.bodyLarge.copy(color = colors.text)
            },
            cursorBrush = SolidColor(colors.primary),
            keyboardOptions = keyboardOptions,
        )
    }
}

/** A one-line secret typed into the form, such as a security answer: hidden unless [showing]. */
@Composable
private fun SecretLine(state: TextFieldState, hint: String, showing: Boolean) {
    val colors = palette
    val mode = if (showing) TextObfuscationMode.Visible else TextObfuscationMode.RevealLastTyped
    val keys = KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false)
    if (colors.flat) {
        androidx.compose.material3.OutlinedSecureTextField(
            state = state,
            modifier = Modifier.fillMaxWidth().secretSemantics(state, showing),
            label = { Text(hint) },
            textStyle = TextStyle(fontFamily = MonoFamily, fontSize = 16.sp),
            textObfuscationMode = mode,
            keyboardOptions = keys,
        )
        return
    }
    Box(Modifier.fillMaxWidth().heightIn(min = 28.dp), contentAlignment = Alignment.CenterStart) {
        BasicSecureTextField(
            state,
            Modifier.fillMaxWidth().secretSemantics(state, showing),
            decorator = com.wasimaster.wmpass.ui.parts.placeholderDecorator(state) { Text(hint, color = colors.textLow, style = MaterialTheme.typography.bodyLarge) },
            textStyle = TextStyle(fontFamily = MonoFamily, fontSize = 16.sp, color = colors.text),
            cursorBrush = SolidColor(colors.primary),
            textObfuscationMode = mode,
            keyboardOptions = keys,
        )
    }
}

@OptIn(ExperimentalLayoutApi::class, ExperimentalMaterial3Api::class)
@Composable
private fun FieldEditor(
    entryId: String?,
    field: EditField,
    context: List<String>,
    onAlias: (() -> Unit)? = null,
    onSsh: (() -> Unit)? = null,
    /** Adds what WM Keyboard may offer in this field to its keyboard options. */
    tag: (KeyboardOptions) -> KeyboardOptions = { it },
    onGenerate: () -> Unit,
) {
    val colors = palette
    val shell = LocalShell.current
    val spec = field.spec
    val scope = rememberCoroutineScope()
    when (spec.input) {
        "toggle" -> Row(
            Modifier
                .fillMaxWidth()
                .toggleable(field.flag, role = androidx.compose.ui.semantics.Role.Switch) { field.flag = it }
                .padding(start = 18.dp, end = 14.dp, top = 8.dp, bottom = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(spec.label, color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
            PassSwitch(field.flag, null)
        }
        "choice" -> Column(Modifier.padding(horizontal = 18.dp, vertical = 12.dp)) {
            Text(spec.label, color = colors.textLow, style = MaterialTheme.typography.labelMedium)
            Spacer(Modifier.height(8.dp))
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                spec.choices.forEach { choice ->
                    Pill(choice.uppercase().takeIf { it.length <= 4 } ?: choice.replaceFirstChar { it.uppercase() }, field.text.text.toString() == choice) {
                        field.text.setTextAndPlaceCursorAtEnd(choice)
                    }
                }
            }
        }
        "date" -> {
            var picking by remember { mutableStateOf(false) }
            Row(
                Modifier
                    .fillMaxWidth()
                    .clickable { picking = true }
                    .padding(horizontal = 18.dp, vertical = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(spec.label, color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                Text(field.millis?.let(::dayText) ?: "Not set", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            }
            if (picking) {
                val state = rememberDatePickerState(initialSelectedDateMillis = field.millis)
                DatePickerDialog(
                    onDismissRequest = { picking = false },
                    confirmButton = {
                        TextButton({
                            field.millis = state.selectedDateMillis
                            picking = false
                        }) { Text("Set") }
                    },
                    dismissButton = {
                        TextButton({
                            field.millis = null
                            picking = false
                        }) { Text("Clear") }
                    },
                ) { DatePicker(state) }
            }
        }
        else -> if (colors.flat) {
            val isSecret = spec.secret
            val multiline = spec.input == "multiline"
            Column(Modifier.padding(horizontal = 16.dp, vertical = 4.dp)) {
                val actions: @Composable () -> Unit = {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        if (isSecret && !multiline && field.loaded && field.text.text.isNotEmpty()) {
                            RowIconButton(if (field.showing) PassIcons.VisibilityOff else PassIcons.Visibility, if (field.showing) "Hide ${spec.label}" else "Show ${spec.label}") { field.showing = !field.showing }
                        }
                        if (spec.input == "password") RowIconButton(PassIcons.Casino, "Generate", tint = colors.primary, onClick = onGenerate)
                        if (onAlias != null) RowIconButton(PassIcons.AlternateEmail, "Use an alias", tint = colors.primary, onClick = onAlias)
                        if (onSsh != null) RowIconButton(PassIcons.Casino, "Generate a key", tint = colors.primary, onClick = onSsh)
                    }
                }
                when {
                    isSecret && !field.loaded -> androidx.compose.material3.OutlinedCard(
                        onClick = {
                            scope.launch {
                                revealing(shell) {
                                    if (entryId != null) {
                                        val text = Pass.reveal(entryId, spec.key).consumeText()
                                        field.text.setTextAndPlaceCursorAtEnd(text)
                                        field.original = text
                                    }
                                    field.loaded = true
                                }
                            }
                        },
                        modifier = Modifier.fillMaxWidth().padding(top = 8.dp),
                        shape = androidx.compose.material3.OutlinedTextFieldDefaults.shape,
                        border = androidx.compose.foundation.BorderStroke(1.dp, MaterialTheme.colorScheme.outline),
                    ) {
                        Column(Modifier.padding(horizontal = 16.dp, vertical = 10.dp)) {
                            Text(spec.label, color = colors.textMid, style = MaterialTheme.typography.bodySmall)
                            Text("Saved. Tap to change", color = colors.text, style = MaterialTheme.typography.bodyLarge)
                        }
                    }
                    isSecret && !multiline -> androidx.compose.material3.OutlinedSecureTextField(
                        state = field.text,
                        modifier = Modifier.fillMaxWidth().secretSemantics(field.text, field.showing),
                        label = { Text(spec.label) },
                        textStyle = TextStyle(fontFamily = MonoFamily, fontSize = 16.sp),
                        trailingIcon = actions,
                        textObfuscationMode = if (field.showing) TextObfuscationMode.Visible else TextObfuscationMode.RevealLastTyped,
                        keyboardOptions = tag(KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false)),
                    )
                    else -> androidx.compose.material3.OutlinedTextField(
                        state = field.text,
                        modifier = Modifier.fillMaxWidth(),
                        label = { Text(spec.label) },
                        placeholder = { Text(hintOf(spec)) },
                        textStyle = if (isSecret) TextStyle(fontFamily = MonoFamily, fontSize = 15.sp) else MaterialTheme.typography.bodyLarge,
                        trailingIcon = if (onAlias != null || onSsh != null) actions else null,
                        lineLimits = if (multiline) TextFieldLineLimits.MultiLine(minHeightInLines = 3) else TextFieldLineLimits.SingleLine,
                        keyboardOptions = tag(keyboardFor(spec)),
                    )
                }
                if (spec.input == "password" && field.loaded && field.text.text.isNotEmpty()) {
                    LiveStrength(field.text, context)
                }
                spec.suggestions.takeIf { it.isNotEmpty() && !isSecret }?.let { FieldSuggestions(field.text, it) }
            }
        } else {
            val isSecret = spec.secret
            val multiline = spec.input == "multiline"
            Column(
                Modifier
                    .animateContentSize(Springs.glide())
                    .padding(start = 18.dp, end = 10.dp, top = 11.dp, bottom = 11.dp),
            ) {
                val label: @Composable () -> Unit = {
                    Text(spec.label, color = colors.textLow, style = MaterialTheme.typography.labelMedium)
                    Spacer(Modifier.height(2.dp))
                }
                // A field carries its name inside it, so a screen reader says it with the value;
                // a saved secret not yet opened is no field, and keeps it above.
                val inField = !(isSecret && !field.loaded)
                if (!inField) label()
                Row(verticalAlignment = if (inField) Alignment.Bottom else Alignment.CenterVertically) {
                    Box(Modifier.weight(1f).heightIn(min = 42.dp), contentAlignment = Alignment.CenterStart) {
                        if (isSecret && !field.loaded) {
                            Text(
                                "••••••••••••  Tap to change",
                                color = colors.textLow,
                                style = TextStyle(fontFamily = MonoFamily, fontSize = 15.sp),
                                // Read as what it is, not as twelve bullets.
                                modifier = Modifier.fillMaxWidth().semantics { contentDescription = "${spec.label}, saved" }.clickable(onClickLabel = "Change it") {
                                    scope.launch {
                                        revealing(shell) {
                                            if (entryId != null) {
                                                val text = Pass.reveal(entryId, spec.key).consumeText()
                                                field.text.setTextAndPlaceCursorAtEnd(text)
                                                field.original = text
                                            }
                                            field.loaded = true
                                        }
                                    }
                                },
                            )
                        } else if (isSecret && !multiline) {
                            BasicSecureTextField(
                                field.text,
                                Modifier.fillMaxWidth().secretSemantics(field.text, field.showing),
                                decorator = com.wasimaster.wmpass.ui.parts.labelledDecorator(field.text, label, 42.dp) { Text("Not set", color = colors.textLow, style = MaterialTheme.typography.bodyLarge) },
                                textStyle = TextStyle(fontFamily = MonoFamily, fontSize = 16.sp, color = colors.text),
                                cursorBrush = SolidColor(colors.primary),
                                textObfuscationMode = if (field.showing) TextObfuscationMode.Visible else TextObfuscationMode.RevealLastTyped,
                                keyboardOptions = tag(KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false)),
                            )
                        } else {
                            BasicTextField(
                                field.text,
                                Modifier.fillMaxWidth(),
                                decorator = com.wasimaster.wmpass.ui.parts.labelledDecorator(field.text, label, 42.dp) { Text(hintOf(spec), color = colors.textLow, style = MaterialTheme.typography.bodyLarge) },
                                textStyle = if (isSecret) {
                                    TextStyle(fontFamily = MonoFamily, fontSize = 15.sp, color = colors.text)
                                } else {
                                    MaterialTheme.typography.bodyLarge.copy(color = colors.text)
                                },
                                cursorBrush = SolidColor(colors.primary),
                                lineLimits = if (multiline) TextFieldLineLimits.MultiLine(minHeightInLines = 3) else TextFieldLineLimits.SingleLine,
                                keyboardOptions = tag(keyboardFor(spec)),
                            )
                        }
                    }
                    if (isSecret && !multiline && field.loaded && field.text.text.isNotEmpty()) {
                        Spacer(Modifier.width(6.dp))
                        RowIconButton(
                            if (field.showing) PassIcons.VisibilityOff else PassIcons.Visibility,
                            if (field.showing) "Hide ${spec.label}" else "Show ${spec.label}",
                        ) { field.showing = !field.showing }
                    }
                    if (spec.input == "password") {
                        Spacer(Modifier.width(6.dp))
                        RowIconButton(PassIcons.Casino, "Generate", tint = colors.primary) {
                            shell.haptics.click()
                            onGenerate()
                        }
                    }
                    if (onAlias != null) {
                        Spacer(Modifier.width(6.dp))
                        RowIconButton(PassIcons.AlternateEmail, "Use an alias", tint = colors.primary, onClick = onAlias)
                    }
                    if (onSsh != null) {
                        Spacer(Modifier.width(6.dp))
                        RowIconButton(PassIcons.Casino, "Make a new key", tint = colors.primary, onClick = onSsh)
                    }
                }
                if (spec.input == "password" && field.loaded && field.text.text.isNotEmpty()) {
                    LiveStrength(field.text, context)
                }
                spec.suggestions.takeIf { it.isNotEmpty() && !isSecret }?.let { FieldSuggestions(field.text, it) }
            }
        }
    }
}

private fun hintOf(spec: FieldView): String = when (spec.key) {
    "username" -> "Email or username"
    "email" -> "name@example.com"
    "phone" -> "+1 555 0100"
    "host" -> "db.example.com"
    "port" -> "5432"
    "ssid" -> "Network name"
    else -> "Not set"
}

private fun keyboardFor(spec: FieldView): KeyboardOptions = when (spec.input) {
    "email" -> KeyboardOptions(keyboardType = KeyboardType.Email, autoCorrectEnabled = false)
    "url" -> KeyboardOptions(keyboardType = KeyboardType.Uri, autoCorrectEnabled = false)
    "phone" -> KeyboardOptions(keyboardType = KeyboardType.Phone)
    "number" -> KeyboardOptions(keyboardType = KeyboardType.Number)
    else -> if (spec.key == "username") {
        KeyboardOptions(keyboardType = KeyboardType.Email, autoCorrectEnabled = false)
    } else if (spec.secret && spec.key != "body") {
        // A private key or recovery phrase typed out: a corrected word is a wrong secret.
        KeyboardOptions(autoCorrectEnabled = false)
    } else {
        KeyboardOptions.Default
    }
}

/** Grades the password as it is typed, using the core's own estimate. */
@Composable
private fun LiveStrength(field: TextFieldState, context: List<String>) {
    val colors = palette
    var strength by remember { mutableStateOf<StrengthView?>(null) }
    LaunchedEffect(field.text) {
        delay(120)
        val bytes = field.text.toString().toByteArray()
        strength = runCatching { Pass.strength(bytes, context) }.getOrNull()
        bytes.fill(0)
    }
    val s = strength ?: return
    Column(Modifier.padding(top = 6.dp, end = 8.dp)) {
        StrengthBar(s.score, Modifier.fillMaxWidth())
        Spacer(Modifier.height(6.dp))
        Text(
            s.warning?.takeIf { it.isNotEmpty() } ?: "Time to crack: ${s.crackSlowHash}",
            color = if (s.score < 2) colors.danger else colors.textMid,
            style = MaterialTheme.typography.bodySmall,
        )
    }
}

/** A saved secret read for editing. One that fails, as when the vault locks meanwhile, is said rather than closing the app. */
private suspend fun revealing(shell: Shell, block: suspend () -> Unit) {
    try {
        block()
    } catch (cancelled: kotlinx.coroutines.CancellationException) {
        throw cancelled
    } catch (failure: Exception) {
        shell.notice(failure.message ?: "Could not read this field")
    }
}

@Composable
private fun CustomEditor(entryId: String?, custom: EditCustom) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    Row(Modifier.padding(start = 18.dp, end = 10.dp, top = 10.dp, bottom = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            PlainField(custom.name, "Name")
            Spacer(Modifier.height(4.dp))
            if (custom.hidden && !custom.loaded) {
                Text(
                    "••••••••  Tap to change",
                    color = colors.textLow,
                    style = TextStyle(fontFamily = MonoFamily, fontSize = 15.sp),
                    modifier = Modifier.fillMaxWidth().clickable {
                        scope.launch {
                            revealing(shell) {
                                val existing = custom.existing
                                if (entryId != null && existing != null) {
                                    val text = Pass.revealCustom(entryId, existing.index).consumeText()
                                    custom.value.setTextAndPlaceCursorAtEnd(text)
                                    custom.original = text
                                }
                                custom.loaded = true
                            }
                        }
                    },
                )
            } else {
                // A secret value is still a secret while it is typed: the keyboard is told so.
                PlainField(
                    custom.value,
                    "Value",
                    mono = custom.hidden,
                    keyboardOptions = if (custom.hidden) KeyboardOptions(keyboardType = KeyboardType.Password, autoCorrectEnabled = false) else KeyboardOptions.Default,
                )
            }
        }
        RowIconButton(
            if (custom.hidden) PassIcons.Lock else PassIcons.LockOpen,
            if (custom.hidden) "Secret field. Tap to show it in the clear" else "Plain field. Tap to make it secret",
            tint = if (custom.hidden) colors.primary else colors.textMid,
        ) {
            val existing = custom.existing
            if (custom.hidden && !custom.loaded && entryId != null && existing != null) {
                // Made plain, it shows its value, so the value is read first.
                scope.launch {
                    revealing(shell) {
                        val text = Pass.revealCustom(entryId, existing.index).consumeText()
                        custom.value.setTextAndPlaceCursorAtEnd(text)
                        custom.original = text
                        custom.loaded = true
                        custom.hidden = false
                    }
                }
            } else {
                custom.hidden = !custom.hidden
            }
        }
        RowIconButton(PassIcons.Close, "Remove field") { custom.remove = true }
    }
}

/** The generator, opened from the field it is for. */
@Composable
private fun GeneratorSheet(onDismiss: () -> Unit, onUse: (String) -> Unit) {
    val shellPrefs = LocalShell.current.prefs
    val state = remember { GenState.fromPrefs(shellPrefs) }
    val scope = rememberCoroutineScope()
    LaunchedEffect(Unit) { runCatching { state.roll() } }
    PassSheet(onDismiss, "Generate password") {
        GeneratedValue(state, large = false)
        GeneratedStrength(state, Modifier.padding(horizontal = 4.dp))
        Spacer(Modifier.height(16.dp))
        GeneratorControls(state)
        Spacer(Modifier.height(18.dp))
        Row(verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
            // Not before the first value is made: an empty password would be put in the field.
            PrimaryButton("Use this", Modifier.weight(1f), enabled = state.value.isNotEmpty()) { onUse(state.value) }
            if (!palette.flat) RollButton({ scope.launch { runCatching { state.roll() } } })
        }
    }
}

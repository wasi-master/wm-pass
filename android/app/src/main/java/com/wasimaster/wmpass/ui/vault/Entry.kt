package com.wasimaster.wmpass.ui.vault

import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.slideInVertically
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.interaction.MutableInteractionSource
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
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.derivedStateOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.AttachmentView
import com.wasimaster.wmpass.core.Detail
import com.wasimaster.wmpass.core.FieldView
import com.wasimaster.wmpass.core.OtpRow
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.SecretBytes
import com.wasimaster.wmpass.ui.fx.glass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GlassCircle
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.InkState
import com.wasimaster.wmpass.ui.parts.LifeRing
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.RollingCode
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.parts.SecretText
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.parts.StrengthBar
import com.wasimaster.wmpass.ui.parts.inkSurface
import com.wasimaster.wmpass.ui.parts.kindName
import com.wasimaster.wmpass.ui.parts.tapAt
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.Springs
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.pressScale
import java.text.DateFormat
import java.util.Date
import kotlinx.coroutines.delay
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/** What a site matcher says, in a form fit for a row. */
data class SiteLine(val label: String, val openable: String?)

fun siteLine(element: JsonElement): SiteLine? {
    val obj = element as? JsonObject
    if (obj == null) return null
    val (kind, value) = obj.entries.firstOrNull() ?: return null
    return when (kind) {
        "Domain", "Host" -> value.jsonPrimitive.contentOrNull?.let { SiteLine(it, it) }
        "Url" -> value.jsonPrimitive.contentOrNull?.let { SiteLine(it.removePrefix("https://").removeSuffix("/"), it) }
        "AndroidApp" -> (value as? JsonObject)?.get("package")?.jsonPrimitive?.contentOrNull?.let { SiteLine(it, null) }
        "Regex" -> value.jsonPrimitive.contentOrNull?.let { SiteLine(it, null) }
        else -> null
    }
}

fun whenText(ms: Long): String {
    if (ms <= 0) return "never"
    val days = (System.currentTimeMillis() - ms) / 86_400_000L
    return when {
        days < 0 -> DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(ms))
        days == 0L -> "today"
        days == 1L -> "yesterday"
        days < 30 -> "$days days ago"
        else -> DateFormat.getDateInstance(DateFormat.MEDIUM).format(Date(ms))
    }
}

/** An entry's page: lit in the colour of its site. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun EntryScreen(id: String) {
    val colors = palette
    val shell = LocalShell.current
    val revision by Pass.revision.collectAsStateWithLifecycle()
    var detail by remember { mutableStateOf<Detail?>(null) }
    LaunchedEffect(id, revision) {
        val loaded = runCatching { Pass.detail(id) }.getOrNull()
        if (loaded != null) detail = loaded else if (detail != null) shell.nav.popBackStack()
    }
    val revealed = remember { mutableStateMapOf<String, String>() }
    DisposableEffect(id) { onDispose { revealed.clear() } }
    // Nothing stays on show by accident: 45 seconds after the last reveal, all of it goes back.
    LaunchedEffect(revealed.size) {
        if (revealed.isNotEmpty()) {
            delay(45_000)
            revealed.clear()
        }
    }
    val scroll = rememberScrollState()
    val entry = detail
    val face = com.wasimaster.wmpass.ui.parts.rememberFace(entry?.title.orEmpty(), entry?.site, entry?.kind.orEmpty(), entry?.color, entry?.icon)
    val from = face.from
    val to = face.to
    Box(Modifier.fillMaxSize().background(colors.ground)) {
        // The site's own light, falling away with the scroll.
        Box(
            Modifier
                .fillMaxWidth()
                .height(420.dp)
                .graphicsLayer {
                    translationY = -scroll.value * 0.45f
                    alpha = (1f - scroll.value / 900f).coerceIn(0f, 1f)
                }
                .background(
                    Brush.verticalGradient(
                        0f to Color(from.red, from.green, from.blue, if (colors.dark) 0.46f else 0.30f),
                        0.55f to Color(to.red, to.green, to.blue, if (colors.dark) 0.16f else 0.10f),
                        1f to Color.Transparent,
                    ),
                ),
        )
        if (entry != null) {
            Column(
                Modifier
                    .fillMaxSize()
                    .verticalScroll(scroll)
                    .statusBarsPadding()
                    .navigationBarsPadding()
                    .padding(horizontal = 16.dp),
                horizontalAlignment = Alignment.CenterHorizontally,
            ) {
                Spacer(Modifier.height(66.dp))
                val inSearch = shell.searching
                SiteTile(entry.title, entry.site, entry.kind, Modifier.sharedEntry("tile-$id", inSearch), 84.dp, entry.color, entry.icon)
                Spacer(Modifier.height(14.dp))
                Text(
                    entry.title.ifEmpty { "Untitled" },
                    color = colors.text,
                    style = MaterialTheme.typography.headlineMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.sharedEntry("title-$id", inSearch),
                )
                val under = entry.subtitle.ifEmpty { kindName(entry.kind) }
                Text(under, color = colors.textMid, style = MaterialTheme.typography.bodyLarge, textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis)
                if (entry.trashed) {
                    TrashedActions(entry)
                } else {
                    QuickActions(entry)
                }
                AnimatedVisibility(
                    visible = true,
                    enter = fadeIn(tween(260, 120)) + slideInVertically(Springs.glide()) { it / 12 },
                ) {
                    Column {
                        entry.destroyAtMs?.let { at -> DestructBanner(at) }
                        Sections(entry, revealed)
                        Spacer(Modifier.height(120.dp))
                    }
                }
            }
        }
        TopBar(entry, scroll.value)
    }
}

@Composable
private fun TopBar(entry: Detail?, scrolled: Int) {
    val colors = palette
    val shell = LocalShell.current
    val solid by remember(scrolled) { derivedStateOf { (scrolled / 240f).coerceIn(0f, 1f) } }
    Box(Modifier.fillMaxWidth().statusBarsPadding().padding(horizontal = 14.dp, vertical = 8.dp)) {
        GlassCircle(PassIcons.ArrowBack, "Back", Modifier.align(Alignment.CenterStart)) { shell.nav.popBackStack() }
        if (entry != null) {
            Text(
                entry.title,
                color = colors.text,
                style = MaterialTheme.typography.titleMedium,
                maxLines = 1,
                overflow = TextOverflow.Ellipsis,
                modifier = Modifier
                    .align(Alignment.Center)
                    .padding(horizontal = 112.dp)
                    .graphicsLayer {
                        alpha = solid
                        translationY = (1f - solid) * 12f
                    },
            )
            Row(Modifier.align(Alignment.CenterEnd), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                GlassCircle(
                    if (entry.favourite) PassIcons.StarFilled else PassIcons.Star,
                    if (entry.favourite) "Remove from favourites" else "Add to favourites",
                    tint = if (entry.favourite) colors.warn else colors.text,
                ) {
                    shell.launch {
                        Pass.change("entry.common") {
                            put("id", entry.id)
                            put("favourite", !entry.favourite)
                        }
                    }
                }
                if (!entry.trashed) {
                    GlassCircle(PassIcons.Edit, "Edit") { shell.nav.navigate("edit/${entry.id}") }
                }
            }
        }
    }
}

/** What most people came here for, first and largest. */
@Composable
private fun QuickActions(entry: Detail) {
    val shell = LocalShell.current
    val username = entry.fields.firstOrNull { it.key == "username" || it.key == "user" }?.text.orEmpty()
    val url = entry.sites.firstNotNullOfOrNull { siteLine(it)?.openable }
    Row(Modifier.fillMaxWidth().padding(top = 20.dp), verticalAlignment = Alignment.CenterVertically, horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        if (entry.hasPassword) {
            PrimaryButton("Copy password", Modifier.weight(1f), icon = PassIcons.ContentCopy) { shell.copyPassword(entry.id) }
        }
        if (username.isNotEmpty()) {
            RoundAction(PassIcons.Person, "Copy username") { shell.copyPlain("Username", username) }
        }
        if (url != null) {
            RoundAction(PassIcons.OpenInNew, "Open site") { shell.openUrl(url) }
        }
    }
}

@Composable
private fun RoundAction(icon: ImageVector, label: String, onClick: () -> Unit) {
    val shell = LocalShell.current
    val colors = palette
    val source = remember { MutableInteractionSource() }
    Box(
        Modifier
            .pressScale(source, 0.9f)
            .size(56.dp)
            .glass(CircleShape)
            .clickable(source, null, onClickLabel = label) {
                shell.haptics.click()
                onClick()
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, label, tint = colors.text, modifier = Modifier.size(22.dp))
    }
}

@Composable
private fun TrashedActions(entry: Detail) {
    val shell = LocalShell.current
    val colors = palette
    var confirm by remember { mutableStateOf(false) }
    Row(Modifier.fillMaxWidth().padding(top = 20.dp), horizontalArrangement = Arrangement.spacedBy(10.dp)) {
        PrimaryButton("Restore", Modifier.weight(1f), icon = PassIcons.RestoreFromTrash) {
            shell.launch {
                Pass.change("entry.restore") { put("id", entry.id) }
            }
        }
        QuietButton("Delete", tint = colors.danger, icon = PassIcons.Delete) { confirm = true }
    }
    if (confirm) {
        ConfirmSheet(
            "Delete “${entry.title}” for good?",
            "Its password, history and attachments go with it. A backup made before now would still hold them.",
            "Delete for good",
            onDismiss = { confirm = false },
        ) {
            shell.launch {
                Pass.change("entry.purge") { put("id", entry.id) }
                shell.nav.popBackStack()
            }
        }
    }
}

@Composable
private fun DestructBanner(at: Long) {
    val colors = palette
    Row(
        Modifier
            .padding(top = 16.dp)
            .fillMaxWidth()
            .clip(Shapes.cardSmall)
            .background(colors.danger.copy(alpha = 0.13f))
            .padding(14.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(PassIcons.Timer, null, tint = colors.danger, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(10.dp))
        Text(
            "Destroys itself on ${DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(at))}",
            color = colors.text,
            style = MaterialTheme.typography.bodyMedium,
        )
    }
}

/** Loads a secret into the page's reveal map, or takes it back out. */
private fun Shell.toggle(map: MutableMap<String, String>, key: String, fetch: suspend () -> SecretBytes) {
    if (map.remove(key) != null) return
    launch { map[key] = fetch().consumeText() }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun Sections(entry: Detail, revealed: MutableMap<String, String>) {
    val shell = LocalShell.current
    val colors = palette
    var sheet by remember { mutableStateOf<String?>(null) }
    var linking by remember { mutableStateOf(false) }
    val pickAttachment = rememberAttachmentPicker(entry.id)

    if (entry.otp != null) OtpCard(entry)

    val shownFields = entry.fields.filter { it.hasValue || it.flag }
    if (shownFields.isNotEmpty()) {
        Eyebrow(kindName(entry.kind))
        GroupCard {
            shownFields.forEachIndexed { index, field ->
                if (index > 0) Hairline()
                FieldRow(entry, field, revealed)
            }
        }
    }

    val sites = entry.sites.mapNotNull(::siteLine)
    if (sites.isNotEmpty()) {
        Eyebrow(if (sites.size == 1) "Website" else "Websites")
        GroupCard {
            sites.forEachIndexed { index, site ->
                if (index > 0) Hairline()
                PlainRow(site.label, PassIcons.Language, site.openable?.let { url -> { shell.openUrl(url) } })
            }
        }
    }

    if (entry.passkeys.isNotEmpty()) {
        Eyebrow("Passkeys")
        GroupCard {
            entry.passkeys.forEachIndexed { index, key ->
                if (index > 0) Hairline()
                PlainRow("${key.rpId}  ${key.userName}", PassIcons.KeyVertical, null, "Made ${whenText(key.createdMs)}")
            }
        }
    }

    if (entry.customFields.isNotEmpty()) {
        Eyebrow("More fields")
        GroupCard {
            entry.customFields.forEachIndexed { index, field ->
                if (index > 0) Hairline()
                if (field.value != null) {
                    TextRow(field.name, field.value!!, field.name)
                } else {
                    SecretRow(
                        field.name,
                        revealed["custom:${field.index}"],
                        onToggle = { shell.toggle(revealed, "custom:${field.index}") { Pass.revealCustom(entry.id, field.index) } },
                        onCopy = { shell.copySecret(field.name) { Pass.revealCustom(entry.id, field.index) } },
                    )
                }
            }
        }
    }

    if (entry.questions.isNotEmpty()) {
        Eyebrow("Security questions")
        GroupCard {
            entry.questions.forEachIndexed { index, question ->
                if (index > 0) Hairline()
                suspend fun fetch() = Pass.plainCall("entry.reveal_question") {
                    put("id", entry.id)
                    put("index", index)
                }.secret!!
                SecretRow(
                    question,
                    revealed["question:$index"],
                    onToggle = { shell.toggle(revealed, "question:$index") { fetch() } },
                    onCopy = { shell.copySecret("Answer") { fetch() } },
                )
            }
        }
    }

    if (entry.hasNotes) {
        Eyebrow("Notes")
        GroupCard {
            SecretRow(
                "Notes",
                revealed["notes"],
                style = MaterialTheme.typography.bodyLarge,
                onToggle = { shell.toggle(revealed, "notes") { Pass.reveal(entry.id, "notes") } },
                onCopy = { shell.copySecret("Notes") { Pass.reveal(entry.id, "notes") } },
            )
        }
    }

    Attachments(entry)

    if (entry.tags.isNotEmpty()) {
        Eyebrow("Tags")
        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(horizontal = 2.dp)) {
            entry.tags.forEach { tag -> Pill(tag, false, icon = PassIcons.Sell) { sheet = "tags" } }
            Pill("Add", false, icon = PassIcons.Add) { sheet = "tags" }
        }
    }

    Linked(entry, linking) { linking = it }
    History(entry, revealed)

    Eyebrow("Reminders and more")
    GroupCard {
        val remind = entry.reminders
        val reminderText = when {
            remind.changeEveryDays != null -> "Every ${remind.changeEveryDays} days"
            remind.expiresMs != null -> "Expires ${whenText(remind.expiresMs!!)}"
            else -> "Off"
        }
        if (entry.tags.isEmpty()) {
            ActionLine(PassIcons.Sell, "Add a tag") { sheet = "tags" }
            Hairline(inset = 58.dp)
        }
        ActionLine(PassIcons.AttachFile, "Attach a file") { pickAttachment.launch(arrayOf("*/*")) }
        Hairline(inset = 58.dp)
        ActionLine(PassIcons.Link, "Link another item") { linking = true }
        Hairline(inset = 58.dp)
        ActionLine(PassIcons.Snooze, "Remind me to change it", reminderText) { sheet = "reminders" }
        Hairline(inset = 58.dp)
        val folderName = Pass.folders.collectAsStateWithLifecycle().value.firstOrNull { it.id == entry.folder }?.name
        ActionLine(PassIcons.Folder, "Folder", folderName ?: "None") { sheet = "folder" }
        Hairline(inset = 58.dp)
        ActionLine(PassIcons.Palette, "Colour", if (entry.color != null) "Your choice" else "From the site") { sheet = "colour" }
        Hairline(inset = 58.dp)
        ActionLine(if (entry.pinned) PassIcons.KeepFilled else PassIcons.Keep, if (entry.pinned) "Unpin" else "Pin to the top") {
            shell.launch {
                Pass.change("entry.common") {
                    put("id", entry.id)
                    put("pinned", !entry.pinned)
                }
            }
        }
        Hairline(inset = 58.dp)
        ActionLine(PassIcons.VisibilityLock, if (entry.hidden) "Take off the hidden shelf" else "Put on the hidden shelf") {
            shell.launch {
                Pass.change("entry.common") {
                    put("id", entry.id)
                    put("hidden", !entry.hidden)
                }
            }
        }
        Hairline(inset = 58.dp)
        ActionLine(PassIcons.Timer, "Destroy itself", entry.destroyAtMs?.let { "Set" } ?: "Never") { sheet = "destruct" }
        Hairline(inset = 58.dp)
        ActionLine(PassIcons.Layers, "Make a copy") {
            shell.launch {
                val made = Pass.change("entry.duplicate") { put("id", entry.id) }
                made.obj["id"]?.jsonPrimitive?.content?.let { copy ->
                    shell.nav.popBackStack()
                    shell.open(copy)
                }
            }
        }
        if (!entry.trashed) {
            Hairline(inset = 58.dp)
            ActionLine(PassIcons.Delete, "Move to trash", tint = colors.danger) {
                shell.nav.popBackStack()
                shell.trash(entry.id, entry.title)
            }
        }
    }
    Text(
        "Made ${whenText(entry.createdMs)}. Changed ${whenText(entry.modifiedMs)}.",
        color = colors.textLow,
        style = MaterialTheme.typography.bodySmall,
        textAlign = TextAlign.Center,
        modifier = Modifier.fillMaxWidth().padding(top = 16.dp),
    )

    when (sheet) {
        "tags" -> TagSheet(entry.tags, { sheet = null }) { tag, on ->
            val next = if (on) entry.tags + tag else entry.tags - tag
            shell.launch {
                Pass.change("entry.common") {
                    put("id", entry.id)
                    putJsonArray("tags") { next.distinct().forEach { add(it) } }
                }
            }
        }
        "folder" -> FolderSheet(entry.folder, { sheet = null }) { folder ->
            shell.launch {
                Pass.change("entry.common") {
                    put("id", entry.id)
                    if (folder == null) put("folder", JsonNull) else put("folder", folder)
                }
            }
        }
        "colour" -> ColourSheet(entry.color, { sheet = null }) { hue ->
            shell.launch {
                Pass.change("entry.common") {
                    put("id", entry.id)
                    if (hue == null) put("color", JsonNull) else put("color", hue)
                }
            }
        }
        "reminders" -> RemindersSheet(entry) { sheet = null }
        "destruct" -> DestructSheet(entry) { sheet = null }
    }
}

/** A one-time code with its true lifetime beside it, and the next one when time is short. */
@Composable
private fun OtpCard(entry: Detail) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val ink = remember { InkState(scope) }
    var row by remember { mutableStateOf<OtpRow?>(null) }
    var hotp by remember { mutableStateOf<String?>(null) }
    LaunchedEffect(entry.id, entry.otp) {
        while (true) {
            row = runCatching { Pass.codes(listOf(entry.id)).firstOrNull() }.getOrNull()
            delay(1000 - System.currentTimeMillis() % 1000 + 10)
        }
    }
    val otp = entry.otp ?: return
    Eyebrow("One-time code")
    GroupCard {
        val current = row?.code ?: hotp
        Row(
            Modifier
                .fillMaxWidth()
                .inkSurface(ink, colors.primary)
                .tapAt { at ->
                    if (current != null) {
                        ink.splash(at)
                        shell.copySecret("Code") { Pass.code(entry.id) }
                    }
                }
                .padding(horizontal = 18.dp, vertical = 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                if (current != null) {
                    RollingCode(current)
                } else {
                    Text("Counter-based", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
                }
                val next = row?.nextCode
                AnimatedVisibility(next != null) {
                    Text("Next  ${grouped(next.orEmpty())}", color = colors.textMid, style = PassType.secretSmall)
                }
            }
            if (otp.period != null) {
                LifeRing(otp.period!!, size = 34.dp, stroke = 3.5.dp)
            } else {
                QuietButton("New code", height = 42.dp, tint = colors.primary) {
                    shell.launch {
                        val reply = Pass.change("otp.hotp_next") { put("id", entry.id) }
                        hotp = reply.secret?.consumeText()
                    }
                }
            }
        }
    }
}

@Composable
private fun FieldRow(entry: Detail, field: FieldView, revealed: MutableMap<String, String>) {
    val shell = LocalShell.current
    when {
        field.input == "toggle" -> PlainRow(field.label, PassIcons.CheckCircle, null, if (field.flag) "On" else "Off")
        field.secret -> {
            val shown = revealed["field:${field.key}"]
            SecretRow(
                field.label,
                shown,
                onToggle = { shell.toggle(revealed, "field:${field.key}") { Pass.reveal(entry.id, field.key) } },
                onCopy = { shell.copySecret(field.label) { Pass.reveal(entry.id, field.key) } },
                footer = if (field.input == "password" && shown != null) {
                    { HoldsFor(shown) }
                } else {
                    null
                },
            )
        }
        field.input == "date" -> field.number?.let { TextRow(field.label, whenText(it), field.label) }
        field.number != null -> TextRow(field.label, field.number.toString(), field.label)
        else -> TextRow(field.label, field.text, field.label)
    }
}

/** Revealing a password also tells you how long it would hold. */
@Composable
private fun HoldsFor(password: String) {
    val colors = palette
    var strength by remember { mutableStateOf<com.wasimaster.wmpass.core.StrengthView?>(null) }
    LaunchedEffect(password) {
        val bytes = password.toByteArray()
        strength = runCatching { Pass.strength(bytes) }.getOrNull()
        bytes.fill(0)
    }
    val s = strength ?: return
    Column(Modifier.padding(top = 10.dp)) {
        StrengthBar(s.score, Modifier.fillMaxWidth())
        Spacer(Modifier.height(6.dp))
        Text("Would hold for ${s.crackSlowHash}", color = colors.textMid, style = MaterialTheme.typography.bodySmall)
    }
}

/** A plain value: tap anywhere on it and ink spreads as it copies. */
@Composable
private fun TextRow(label: String, value: String, copyAs: String) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val ink = remember { InkState(scope) }
    Column(
        Modifier
            .fillMaxWidth()
            .inkSurface(ink, colors.primary)
            .tapAt { at ->
                ink.splash(at)
                shell.copyPlain(copyAs, value)
            }
            .padding(horizontal = 18.dp, vertical = 13.dp),
    ) {
        Text(label, color = colors.textLow, style = MaterialTheme.typography.labelMedium)
        Spacer(Modifier.height(3.dp))
        Text(value, color = colors.text, style = MaterialTheme.typography.bodyLarge)
    }
}

@Composable
private fun SecretRow(
    label: String,
    shown: String?,
    style: androidx.compose.ui.text.TextStyle = PassType.secret,
    onToggle: () -> Unit,
    onCopy: () -> Unit,
    footer: (@Composable () -> Unit)? = null,
) {
    val colors = palette
    val scope = rememberCoroutineScope()
    val ink = remember { InkState(scope) }
    Column(
        Modifier
            .fillMaxWidth()
            .animateContentSize(Springs.glide())
            .inkSurface(ink, colors.primary)
            .tapAt { at ->
                ink.splash(at)
                onCopy()
            }
            .padding(start = 18.dp, end = 10.dp, top = 12.dp, bottom = 12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(label, color = colors.textLow, style = MaterialTheme.typography.labelMedium)
                Spacer(Modifier.height(3.dp))
                SecretText(shown, style = style)
            }
            Spacer(Modifier.width(10.dp))
            RowIconButton(
                if (shown != null) PassIcons.VisibilityOff else PassIcons.Visibility,
                if (shown != null) "Hide $label" else "Show $label",
                onClick = onToggle,
            )
            Spacer(Modifier.width(6.dp))
            RowIconButton(PassIcons.ContentCopy, "Copy $label", onClick = onCopy)
        }
        footer?.let { Box(Modifier.padding(end = 8.dp)) { it() } }
    }
}

@Composable
private fun PlainRow(text: String, icon: ImageVector, onClick: (() -> Unit)?, sub: String? = null) {
    val colors = palette
    Row(
        Modifier
            .fillMaxWidth()
            .then(if (onClick != null) Modifier.clickable(onClick = onClick) else Modifier)
            .padding(horizontal = 18.dp, vertical = 13.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(icon, null, tint = colors.textMid, modifier = Modifier.size(20.dp))
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(text, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
            if (sub != null) Text(sub, color = colors.textMid, style = MaterialTheme.typography.bodySmall)
        }
        if (onClick != null) Icon(PassIcons.OpenInNew, null, tint = colors.textLow, modifier = Modifier.size(18.dp))
    }
}

@Composable
private fun ActionLine(icon: ImageVector, title: String, value: String? = null, tint: Color = palette.text, onClick: () -> Unit) {
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
        Icon(icon, null, tint = if (tint == colors.text) colors.textMid else tint, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(18.dp))
        Text(title, color = tint, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
        if (value != null) Text(value, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 1)
    }
}

@Composable
private fun rememberAttachmentPicker(id: String) = run {
    val shell = LocalShell.current
    val context = LocalContext.current
    rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) {
            shell.launch {
                val resolver = context.contentResolver
                val name = resolver.query(uri, arrayOf(android.provider.OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                    if (it.moveToFirst()) it.getString(0) else null
                } ?: "Attachment"
                val mime = resolver.getType(uri).orEmpty()
                val bytes = resolver.openInputStream(uri)?.use { it.readBytes() } ?: return@launch
                Pass.addAttachment(id, name, mime, bytes)
                bytes.fill(0)
                shell.haptics.confirm()
            }
        }
    }
}

@Composable
private fun Attachments(entry: Detail) {
    var opened by remember { mutableStateOf<AttachmentView?>(null) }
    if (entry.attachmentList.isNotEmpty()) {
        Eyebrow("Attachments")
        GroupCard {
            entry.attachmentList.forEachIndexed { index, file ->
                if (index > 0) Hairline()
                PlainRow(file.name, if (file.mime.startsWith("image/")) PassIcons.Image else PassIcons.AttachFile, { opened = file }, sizeText(file.size))
            }
        }
    }
    opened?.let { file -> AttachmentSheet(entry.id, file) { opened = null } }
}

fun sizeText(bytes: Long): String = when {
    bytes < 1024 -> "$bytes bytes"
    bytes < 1024 * 1024 -> "${bytes / 1024} KB"
    else -> "%.1f MB".format(bytes / (1024.0 * 1024.0))
}

@Composable
private fun AttachmentSheet(id: String, file: AttachmentView, onDismiss: () -> Unit) {
    val shell = LocalShell.current
    val colors = palette
    val context = LocalContext.current
    var image by remember { mutableStateOf<androidx.compose.ui.graphics.ImageBitmap?>(null) }
    LaunchedEffect(file.blob) {
        if (file.mime.startsWith("image/")) {
            image = runCatching {
                Pass.openAttachment(id, file.blob).consume { bytes ->
                    BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap()
                }
            }.getOrNull()
        }
    }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument(file.mime.ifEmpty { "application/octet-stream" })) { uri ->
        if (uri != null) {
            shell.launch {
                Pass.openAttachment(id, file.blob).consume { bytes ->
                    context.contentResolver.openOutputStream(uri)?.use { it.write(bytes) }
                }
                shell.haptics.confirm()
                onDismiss()
            }
        }
    }
    var confirm by remember { mutableStateOf(false) }
    PassSheet(onDismiss, file.name) {
        image?.let { bitmap ->
            Image(
                bitmap,
                file.name,
                contentScale = ContentScale.Fit,
                modifier = Modifier.fillMaxWidth().heightIn(max = 420.dp).clip(Shapes.cardSmall),
            )
            Spacer(Modifier.height(14.dp))
        }
        SheetRow(PassIcons.Download, "Save a copy", "The copy will not be encrypted") { save.launch(file.name) }
        Hairline(inset = 60.dp)
        SheetRow(PassIcons.Delete, "Remove", tint = colors.danger) { confirm = true }
    }
    if (confirm) {
        ConfirmSheet("Remove “${file.name}”?", "The file is deleted from this vault.", "Remove", onDismiss = { confirm = false }) {
            shell.launch {
                Pass.change("attachment.remove") {
                    put("id", id)
                    put("blob", file.blob)
                }
                onDismiss()
            }
        }
    }
}

@Composable
private fun Linked(entry: Detail, picking: Boolean, onPicking: (Boolean) -> Unit) {
    val shell = LocalShell.current
    val all by Pass.entries.collectAsStateWithLifecycle()
    val others = remember(all, entry.links) { entry.links.mapNotNull { id -> all.firstOrNull { it.id == id } } }
    val colors = palette
    if (others.isNotEmpty()) {
        Eyebrow("Linked")
        GroupCard {
            others.forEachIndexed { index, other ->
                if (index > 0) Hairline(inset = 70.dp)
                Row(
                    Modifier.fillMaxWidth().clickable { shell.open(other.id) }.padding(horizontal = 16.dp, vertical = 10.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    SiteTile(other.title, other.site, other.kind, size = 38.dp, label = other.color, icon = other.icon)
                    Spacer(Modifier.width(14.dp))
                    Column(Modifier.weight(1f)) {
                        Text(other.title, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                        if (other.subtitle.isNotEmpty()) Text(other.subtitle, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1)
                    }
                }
            }
        }
    }
    if (picking) {
        PassSheet({ onPicking(false) }, "Link to") {
            all.filter { it.id != entry.id }.take(60).forEach { other ->
                val on = other.id in entry.links
                SheetRow(com.wasimaster.wmpass.ui.parts.kindIcon(other.kind), other.title, other.subtitle.ifEmpty { null }, selected = on) {
                    val next = if (on) entry.links - other.id else entry.links + other.id
                    shell.launch {
                        Pass.change("entry.common") {
                            put("id", entry.id)
                            putJsonArray("links") { next.forEach { add(it) } }
                        }
                    }
                }
            }
        }
    }
}

/** Every value this entry has had: a timeline you can read back from. */
@Composable
private fun History(entry: Detail, revealed: MutableMap<String, String>) {
    val colors = palette
    val shell = LocalShell.current
    if (entry.history.isEmpty()) return
    var open by remember { mutableStateOf(false) }
    Eyebrow("History") {
        Text(
            if (open) "Hide" else "${entry.history.size} earlier",
            color = colors.primary,
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.clickable { open = !open }.padding(horizontal = 4.dp),
        )
    }
    AnimatedVisibility(open) {
        GroupCard {
            entry.history.sortedByDescending { it.atMs }.forEachIndexed { index, revision ->
                if (index > 0) Hairline()
                val key = "history:${revision.index}"
                SecretRow(
                    "${revision.field.replaceFirstChar { it.uppercase() }}, ${whenText(revision.atMs)}",
                    revealed[key],
                    style = PassType.secretSmall,
                    onToggle = { shell.toggle(revealed, key) { Pass.revealHistory(entry.id, revision.index) } },
                    onCopy = { shell.copySecret("Old value") { Pass.revealHistory(entry.id, revision.index) } },
                )
            }
        }
    }
}

@Composable
private fun RemindersSheet(entry: Detail, onDismiss: () -> Unit) {
    val shell = LocalShell.current
    fun set(days: Int?) {
        shell.launch {
            Pass.change("entry.common") {
                put("id", entry.id)
                put(
                    "reminders",
                    buildJsonObject {
                        if (days != null) put("change_every_days", days)
                        entry.reminders.expiresMs?.let { put("expires_ms", it) }
                        put("warn_days", entry.reminders.warnDays)
                    },
                )
            }
            onDismiss()
        }
    }
    PassSheet(onDismiss, "Remind me to change it") {
        val current = entry.reminders.changeEveryDays
        SheetRow(PassIcons.Close, "Never", selected = current == null) { set(null) }
        listOf(30, 90, 180, 365).forEach { days ->
            SheetRow(PassIcons.Snooze, "Every $days days", selected = current == days) { set(days) }
        }
    }
}

@Composable
private fun DestructSheet(entry: Detail, onDismiss: () -> Unit) {
    val shell = LocalShell.current
    fun set(after: Long?) {
        shell.launch {
            Pass.change("entry.common") {
                put("id", entry.id)
                if (after == null) put("destroy_at_ms", JsonNull) else put("destroy_at_ms", System.currentTimeMillis() + after)
            }
            onDismiss()
        }
    }
    PassSheet(onDismiss, "Destroy itself") {
        Text(
            "On the day, its secrets and attachments are erased on every device that syncs. A copy taken before then still holds them.",
            color = palette.textMid,
            style = MaterialTheme.typography.bodyMedium,
            modifier = Modifier.padding(bottom = 10.dp),
        )
        if (entry.destroyAtMs != null) SheetRow(PassIcons.Close, "Keep it after all") { set(null) }
        val hour = 3_600_000L
        SheetRow(PassIcons.Timer, "In an hour") { set(hour) }
        SheetRow(PassIcons.Timer, "Tomorrow") { set(24 * hour) }
        SheetRow(PassIcons.Timer, "In a week") { set(7 * 24 * hour) }
        SheetRow(PassIcons.Timer, "In a month") { set(30 * 24 * hour) }
    }
}

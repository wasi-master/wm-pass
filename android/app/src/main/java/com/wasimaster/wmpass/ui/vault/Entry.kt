package com.wasimaster.wmpass.ui.vault

import android.graphics.BitmapFactory
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.togetherWith
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
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.AttachmentView
import com.wasimaster.wmpass.core.Detail
import com.wasimaster.wmpass.core.FieldView
import com.wasimaster.wmpass.core.OtpRow
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.SecretBytes
import com.wasimaster.wmpass.ui.fx.glass
import com.wasimaster.wmpass.ui.help.HelpAction
import com.wasimaster.wmpass.ui.help.helpAnchor
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
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/** What a site matcher says, in a form fit for a row. */
data class SiteLine(val label: String, val openable: String?, val app: String? = null)

/**
 * An address in its parts, so a long one reads at a glance: the scheme faint, the site's own
 * domain strong, the rest of the host plain, the path and query quieter. Breaks are allowed after
 * each slash, question mark and ampersand, so a long path wraps where it reads naturally.
 */
fun addressText(address: String, colors: com.wasimaster.wmpass.ui.theme.Palette): androidx.compose.ui.text.AnnotatedString =
    androidx.compose.ui.text.buildAnnotatedString {
        fun part(text: String, color: Color, weight: FontWeight? = null) {
            if (text.isEmpty()) return
            pushStyle(androidx.compose.ui.text.SpanStyle(color = color, fontWeight = weight))
            append(text.replace("/", "/\u200B").replace("?", "?\u200B").replace("&", "&\u200B"))
            pop()
        }
        val scheme = address.substringBefore("://", "").takeIf { it.isNotEmpty() && address.contains("://") }
        val rest = if (scheme != null) address.substringAfter("://") else address
        val cut = rest.indexOfFirst { it == '/' || it == '?' || it == '#' }.let { if (it < 0) rest.length else it }
        val hostPort = rest.substring(0, cut)
        val tail = rest.substring(cut)
        val host = hostPort.substringBefore(':')
        val port = hostPort.removePrefix(host)
        val domain = com.wasimaster.wmpass.system.Related.registrable(host)?.takeIf { host.endsWith(it) } ?: host
        // An app address names a package, not a place on the web, so it is set apart in the accent.
        val appScheme = scheme != null && scheme.lowercase().startsWith("android")
        if (scheme != null) part("$scheme://", if (appScheme) colors.primary else colors.textLow)
        part(host.removeSuffix(domain), colors.textMid)
        part(domain, if (appScheme) colors.primary else colors.text, FontWeight.SemiBold)
        part(port, colors.textLow)
        val query = tail.indexOfFirst { it == '?' || it == '#' }.let { if (it < 0) tail.length else it }
        part(tail.substring(0, query), colors.textMid)
        part(tail.substring(query), colors.textLow)
    }

fun siteLine(element: JsonElement): SiteLine? {
    val obj = element as? JsonObject
    if (obj == null) return null
    val (kind, value) = obj.entries.firstOrNull() ?: return null
    return when (kind) {
        "Domain", "Host" -> value.jsonPrimitive.contentOrNull?.let { SiteLine(it, it) }
        "Url" -> value.jsonPrimitive.contentOrNull?.let { SiteLine(it.removePrefix("https://").removeSuffix("/"), it) }
        "AndroidApp" -> (value as? JsonObject)?.get("package")?.jsonPrimitive?.contentOrNull?.let { SiteLine(it, null, app = it) }
        "Regex" -> value.jsonPrimitive.contentOrNull?.let { SiteLine(it, null) }
        else -> null
    }
}

/** The package of the first app an item is for, for that app's icon. */
val Detail.app: String?
    get() = sites.firstNotNullOfOrNull { site ->
        ((site as? JsonObject)?.get("AndroidApp") as? JsonObject)?.get("package")?.jsonPrimitive?.contentOrNull
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
            val seconds = shell.prefs.revealSeconds.value
            if (seconds > 0) {
                delay(seconds * 1000L)
                revealed.clear()
            }
        }
    }
    val scroll = rememberScrollState()
    val entry = detail
    val face = com.wasimaster.wmpass.ui.parts.rememberFace(entry?.title.orEmpty(), entry?.site, entry?.kind.orEmpty(), entry?.color, entry?.icon)
    val from = face.from
    val to = face.to
    if (colors.flat) {
        StandardEntry(id, entry, revealed, scroll)
        return
    }
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
                SiteTile(entry.title, entry.site, entry.kind, Modifier.sharedEntryWhileShown("tile-$id", inSearch), 84.dp, entry.color, entry.icon, app = entry.app)
                Spacer(Modifier.height(14.dp))
                Text(
                    entry.title.ifEmpty { "Untitled" },
                    color = colors.text,
                    style = MaterialTheme.typography.headlineMedium,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.sharedEntryWhileShown("title-$id", inSearch),
                )
                val under = entry.subtitle.ifEmpty { kindName(entry.kind) }
                Text(under, color = colors.textMid, style = MaterialTheme.typography.bodyLarge, textAlign = TextAlign.Center, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.sharedEntryWhileShown("subtitle-$id", inSearch))
                if (entry.trashed) {
                    TrashedActions(entry)
                } else {
                    Box(Modifier.helpAnchor("detail.actions")) { QuickActions(entry) }
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
        // What this page's own buttons report, such as a network already saved, says so here
        // rather than waiting for the vault list.
        UndoBar(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(horizontal = 14.dp, vertical = 10.dp))
    }
}

/** An ordinary detail page: an app bar, a heading, and rows. */
@OptIn(ExperimentalLayoutApi::class, androidx.compose.material3.ExperimentalMaterial3Api::class)
@Composable
private fun StandardEntry(id: String, entry: Detail?, revealed: MutableMap<String, String>, scroll: androidx.compose.foundation.ScrollState) {
    val colors = palette
    val shell = LocalShell.current
    androidx.compose.material3.Scaffold(
        containerColor = colors.ground,
        snackbarHost = { UndoBar() },
        topBar = {
            androidx.compose.material3.TopAppBar(
                title = {
                    if (scroll.value > 180 && entry != null) {
                        Text(entry.title, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                },
                navigationIcon = {
                    androidx.compose.material3.IconButton(onClick = { shell.nav.popBackStack() }) { Icon(PassIcons.ArrowBack, "Back") }
                },
                actions = {
                    HelpAction("screen.item", "the item page")
                    if (entry != null) {
                        androidx.compose.material3.IconButton(onClick = {
                            shell.launch {
                                Pass.change("entry.common") {
                                    put("id", entry.id)
                                    put("favourite", !entry.favourite)
                                }
                            }
                        }) {
                            Icon(
                                if (entry.favourite) PassIcons.StarFilled else PassIcons.Star,
                                if (entry.favourite) "Remove from favorites" else "Add to favorites",
                                tint = if (entry.favourite) colors.warn else colors.textMid,
                            )
                        }
                        if (!entry.trashed) {
                            androidx.compose.material3.IconButton(onClick = { shell.nav.navigate("edit/${entry.id}") }) { Icon(PassIcons.Edit, "Edit") }
                        }
                    }
                },
                colors = androidx.compose.material3.TopAppBarDefaults.topAppBarColors(containerColor = colors.ground),
            )
        },
    ) { inner ->
        if (entry != null) {
            Column(
                Modifier
                    .padding(inner)
                    .fillMaxSize()
                    .verticalScroll(scroll)
                    .padding(horizontal = 16.dp),
            ) {
                Row(Modifier.fillMaxWidth().padding(top = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                    val inSearch = shell.searching
                    SiteTile(entry.title, entry.site, entry.kind, Modifier.sharedEntryWhileShown("tile-$id", inSearch), 56.dp, entry.color, entry.icon, app = entry.app)
                    Spacer(Modifier.width(16.dp))
                    Column(Modifier.weight(1f)) {
                        Text(entry.title.ifEmpty { "Untitled" }, color = colors.text, style = MaterialTheme.typography.headlineSmall, modifier = Modifier.sharedEntryWhileShown("title-$id", inSearch))
                        Text(entry.subtitle.ifEmpty { kindName(entry.kind) }, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 2, overflow = TextOverflow.Ellipsis, modifier = Modifier.sharedEntryWhileShown("subtitle-$id", inSearch))
                    }
                }
                if (entry.trashed) TrashedActions(entry) else Box(Modifier.helpAnchor("detail.actions")) { QuickActions(entry) }
                entry.destroyAtMs?.let { at -> DestructBanner(at) }
                Sections(entry, revealed)
                Spacer(Modifier.height(32.dp))
            }
        }
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
                HelpAction("screen.item", "the item page", tint = colors.text)
                GlassCircle(
                    if (entry.favourite) PassIcons.StarFilled else PassIcons.Star,
                    if (entry.favourite) "Remove from favorites" else "Add to favorites",
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
    val wifi = rememberWifiActions(entry)
    if (palette.flat) {
        val prefs = shell.prefs
        if (!prefs.entryActions.value) {
            Spacer(Modifier.height(12.dp))
            return
        }
        // As in Contacts: every action says what it does, and the likeliest one is filled.
        Row(Modifier.fillMaxWidth().padding(top = 20.dp, bottom = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            if (entry.hasPassword) {
                LabelledAction(PassIcons.ContentCopy, "Copy password", filled = true, confirms = true, Modifier.weight(1f)) { shell.copyPassword(entry.id) }
            }
            if (username.isNotEmpty() && prefs.entryActionUsername.value) {
                LabelledAction(PassIcons.Person, "Copy username", filled = !entry.hasPassword, confirms = true, Modifier.weight(1f)) { shell.copyPlain("Username", username) }
            }
            if (url != null && prefs.entryActionOpen.value) {
                LabelledAction(PassIcons.OpenInNew, "Open site", filled = false, confirms = false, Modifier.weight(1f)) { shell.openUrl(url) }
            }
            if (wifi?.canShow == true) {
                LabelledAction(PassIcons.QrCode, "QR code", filled = !entry.hasPassword, confirms = false, Modifier.weight(1f), wifi.show)
            }
            if (wifi?.canAdd == true) {
                LabelledAction(PassIcons.Wifi, "Add to phone", filled = false, confirms = false, Modifier.weight(1f), wifi.add)
            }
        }
        return
    }
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
        if (wifi?.canShow == true) RoundAction(PassIcons.QrCode, "Show QR code", wifi.show)
        if (wifi?.canAdd == true) RoundAction(PassIcons.Wifi, "Add to this phone", wifi.add)
    }
}

/** The glyph turns to a tick for a moment after copying, so the tap has an answer on the spot. */
@Composable
private fun LabelledAction(icon: ImageVector, label: String, filled: Boolean, confirms: Boolean, modifier: Modifier, onClick: () -> Unit) {
    val colors = palette
    val scheme = MaterialTheme.colorScheme
    val shell = LocalShell.current
    var done by remember { mutableStateOf(false) }
    LaunchedEffect(done) {
        if (done) {
            delay(1400)
            done = false
        }
    }
    Column(modifier, horizontalAlignment = Alignment.CenterHorizontally) {
        androidx.compose.material3.Surface(
            onClick = {
                shell.haptics.click()
                onClick()
                if (confirms) done = true
            },
            shape = CircleShape,
            color = if (filled) scheme.primary else scheme.secondaryContainer,
            contentColor = if (filled) scheme.onPrimary else scheme.onSecondaryContainer,
            modifier = Modifier.fillMaxWidth().height(48.dp),
        ) {
            Box(contentAlignment = Alignment.Center) {
                androidx.compose.animation.AnimatedContent(
                    done,
                    transitionSpec = {
                        (fadeIn(tween(150)) + androidx.compose.animation.scaleIn(tween(220), 0.6f)) togetherWith androidx.compose.animation.fadeOut(tween(90))
                    },
                    label = "copied",
                ) { copied ->
                    Icon(if (copied) PassIcons.Check else icon, label, modifier = Modifier.size(22.dp))
                }
            }
        }
        Spacer(Modifier.height(6.dp))
        Text(if (done) "Copied" else label, color = colors.textMid, style = MaterialTheme.typography.labelMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
    }
}

@Composable
private fun RoundAction(icon: ImageVector, label: String, onClick: () -> Unit) {
    val shell = LocalShell.current
    val colors = palette
    val source = remember { MutableInteractionSource() }
    if (colors.flat) {
        androidx.compose.material3.FilledTonalIconButton(
            onClick = {
                shell.haptics.click()
                onClick()
            },
            modifier = Modifier.size(48.dp),
        ) { Icon(icon, label) }
        return
    }
    Box(
        Modifier
            .pressScale(source, 0.9f)
            .size(56.dp)
            .glass(CircleShape)
            .clickable(source, androidx.compose.material3.ripple(), onClickLabel = label) {
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
            "Deletes itself on ${DateFormat.getDateTimeInstance(DateFormat.MEDIUM, DateFormat.SHORT).format(Date(at))}",
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

    if (entry.otp != null && !colors.flat) OtpCard(entry)

    val shownFields = entry.fields.filter { it.hasValue || it.flag }
    if (shownFields.isNotEmpty()) {
        if (!colors.flat) Eyebrow(kindName(entry.kind))
        GroupCard(plain = true) {
            shownFields.forEachIndexed { index, field ->
                if (index > 0) Hairline()
                FieldRow(entry, field, revealed)
            }
        }
    }
    if (entry.otp != null && colors.flat) OtpCard(entry)

    val sites = entry.sites.mapNotNull(::siteLine)
    val webs = sites.filter { it.app == null }
    val apps = sites.mapNotNull { it.app }
    val context = androidx.compose.ui.platform.LocalContext.current
    val change = remember(webs.firstOrNull()?.label) { com.wasimaster.wmpass.system.ChangePassword.url(context, webs.firstOrNull { it.openable != null }?.openable) }
    val heading = when {
        webs.isNotEmpty() && apps.isNotEmpty() -> "Websites and apps"
        apps.isNotEmpty() -> if (apps.size == 1) "App" else "Apps"
        else -> if (webs.size == 1) "Website" else "Websites"
    }
    if (sites.isNotEmpty() && colors.flat) {
        GroupCard(plain = true) {
            webs.forEach { site ->
                val open = site.openable?.let { url -> { shell.openUrl(url) } }
                Row(
                    Modifier
                        .fillMaxWidth()
                        .then(if (open != null) Modifier.clickable(onClick = open) else Modifier)
                        .padding(start = 16.dp, end = 4.dp, top = 12.dp, bottom = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Column(Modifier.weight(1f)) {
                        Text("Website", color = colors.textMid, style = MaterialTheme.typography.bodySmall)
                        Spacer(Modifier.height(2.dp))
                        Text(remember(site.label, colors) { addressText(site.label, colors) }, style = MaterialTheme.typography.bodyLarge)
                    }
                    if (open != null) RowIconButton(PassIcons.OpenInNew, "Open ${site.label}", onClick = open)
                }
            }
            apps.forEach { pkg -> AppLine(pkg) }
            if (change != null && entry.kind == "login") ChangePasswordLine(change)
        }
    } else if (sites.isNotEmpty()) {
        Eyebrow(heading)
        GroupCard(plain = true) {
            webs.forEachIndexed { index, site ->
                if (index > 0) Hairline()
                PlainRow(site.label, PassIcons.Language, site.openable?.let { url -> { shell.openUrl(url) } }, styled = remember(site.label, colors) { addressText(site.label, colors) })
            }
            apps.forEachIndexed { index, pkg ->
                if (index > 0 || webs.isNotEmpty()) Hairline()
                AppLine(pkg)
            }
            if (change != null && entry.kind == "login") {
                Hairline()
                ChangePasswordLine(change)
            }
        }
    }

    if (entry.passkeys.isNotEmpty()) {
        Eyebrow("Passkeys")
        GroupCard(plain = true) {
            entry.passkeys.forEachIndexed { index, key ->
                if (index > 0) Hairline()
                PlainRow("${key.rpId}  ${key.userName}", PassIcons.KeyVertical, null, "Made ${whenText(key.createdMs)}")
            }
        }
    }

    if (entry.customFields.isNotEmpty()) {
        Eyebrow("Custom fields")
        GroupCard(plain = true) {
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
        GroupCard(plain = true) {
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
        GroupCard(plain = true) {
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

    var managing by remember { mutableStateOf<String?>(null) }
    var suggested by remember(entry.id) { mutableStateOf<List<String>>(emptyList()) }
    LaunchedEffect(entry.id, entry.title, entry.sites) {
        suggested = runCatching {
            Pass.plainCall("categorise.suggest") { put("id", entry.id) }.obj["categories"]?.jsonArray?.mapNotNull { it.jsonPrimitive.contentOrNull }
        }.getOrNull().orEmpty()
    }
    fun setTags(next: List<String>) = shell.launch {
        Pass.change("entry.common") {
            put("id", entry.id)
            putJsonArray("tags") { next.distinct().forEach { add(it) } }
        }
    }
    if (!entry.trashed || entry.tags.isNotEmpty()) {
        Eyebrow("Tags")
        // Material's chip keeps a 48dp touch height around a 32dp chip, so the row lines them up by their middles.
        FlowRow(
            horizontalArrangement = Arrangement.spacedBy(8.dp),
            verticalArrangement = Arrangement.spacedBy(8.dp),
            itemVerticalAlignment = Alignment.CenterVertically,
            modifier = Modifier.padding(horizontal = 2.dp).animateContentSize(Springs.glide()),
        ) {
            entry.tags.forEach { tag ->
                TagChip(tag, true, onRemove = { setTags(entry.tags - tag) }, onLongClick = { managing = tag }) { managing = tag }
            }
            if (!entry.trashed) Pill(if (entry.tags.isEmpty()) "Add a tag" else "Add", false, icon = PassIcons.Add) { sheet = "tags" }
        }
        val offered = suggested.filter { it !in entry.tags }
        if (offered.isNotEmpty() && !entry.trashed) {
            Row(Modifier.padding(start = 4.dp, top = 12.dp, bottom = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                Icon(PassIcons.AutoAwesome, null, tint = colors.textMid, modifier = Modifier.size(16.dp))
                Spacer(Modifier.width(6.dp))
                Text("Suggested", color = colors.textMid, style = MaterialTheme.typography.labelLarge)
            }
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp), modifier = Modifier.padding(horizontal = 2.dp)) {
                offered.forEach { tag -> Pill(tag, false, icon = PassIcons.Add) { setTags(entry.tags + tag) } }
            }
        }
    }
    managing?.let { tag -> TagManageSheet(tag) { managing = null } }

    Linked(entry, linking) { linking = it }
    History(entry, revealed)

    Eyebrow("More")
    GroupCard(plain = true) {
        val remind = entry.reminders
        val reminderText = when {
            remind.changeEveryDays != null -> "Every ${remind.changeEveryDays} days"
            remind.expiresMs != null -> "Expires ${whenText(remind.expiresMs!!)}"
            else -> "Off"
        }
        ActionLine(PassIcons.AttachFile, "Attach a file") { pickAttachment.launch(arrayOf("*/*")) }
        Hairline(inset = 58.dp)
        ActionLine(PassIcons.Link, "Link another item") { linking = true }
        Hairline(inset = 58.dp)
        // A reminder to change a password means nothing on an item that has none.
        if (entry.hasPassword || remind.changeEveryDays != null || remind.expiresMs != null) {
            ActionLine(PassIcons.Snooze, "Change reminder", reminderText) { sheet = "reminders" }
            Hairline(inset = 58.dp)
        }
        val folderName = Pass.folders.collectAsStateWithLifecycle().value.firstOrNull { it.id == entry.folder }?.name
        ActionLine(PassIcons.Folder, "Folder", folderName ?: "None") { sheet = "folder" }
        Hairline(inset = 58.dp)
        val shown = com.wasimaster.wmpass.ui.parts.rememberFace(entry.title, entry.site, entry.kind, entry.color, entry.icon)
        ActionLine(PassIcons.Palette, "Color", if (entry.color != null) "Your choice" else "From the site", trailing = { Swatch(shown) }) {
            sheet = "colour"
        }
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
        ActionLine(PassIcons.QrCodeScanner, "Send to a device") { sheet = "send" }
        // The rarely used ones wait behind one line, unless one of them is already in use.
        var rare by remember(entry.id) { mutableStateOf(entry.hidden || entry.destroyAtMs != null) }
        Hairline(inset = 58.dp)
        AnimatedVisibility(rare) {
            Column {
                ActionLine(PassIcons.VisibilityLock, if (entry.hidden) "Remove from hidden shelf" else "Move to hidden shelf") {
                    shell.launch {
                        Pass.change("entry.common") {
                            put("id", entry.id)
                            put("hidden", !entry.hidden)
                        }
                    }
                }
                Hairline(inset = 58.dp)
                ActionLine(PassIcons.Timer, "Self-destruct", entry.destroyAtMs?.let { "Set" } ?: "Never") { sheet = "destruct" }
                Hairline(inset = 58.dp)
                ActionLine(PassIcons.Layers, "Duplicate") {
                    shell.launch {
                        val made = Pass.change("entry.duplicate") { put("id", entry.id) }
                        made.obj["id"]?.jsonPrimitive?.content?.let { copy ->
                            shell.nav.popBackStack()
                            shell.open(copy)
                        }
                    }
                }
                Hairline(inset = 58.dp)
            }
        }
        if (!rare) {
            ActionLine(PassIcons.MoreHoriz, "More options", "Hidden shelf, self-destruct, duplicate") { rare = true }
        }
        if (!entry.trashed) {
            if (!rare) Hairline(inset = 58.dp)
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
        "tags" -> TagSheet(entry.tags, { sheet = null }, suggested) { tag, on ->
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
        "colour" -> ColourSheet(entry.color, { sheet = null }, com.wasimaster.wmpass.ui.parts.rememberFace(entry.title, entry.site, entry.kind, null, entry.icon)) { hue ->
            shell.launch {
                Pass.change("entry.common") {
                    put("id", entry.id)
                    if (hue == null) put("color", JsonNull) else put("color", hue)
                }
            }
        }
        "reminders" -> RemindersSheet(entry) { sheet = null }
        "destruct" -> DestructSheet(entry) { sheet = null }
        "send" -> SendSheet(entry) { sheet = null }
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
    if (!colors.flat) Eyebrow("One-time code")
    GroupCard(plain = true) {
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
                .padding(horizontal = if (colors.flat) 16.dp else 18.dp, vertical = if (colors.flat) 12.dp else 16.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Column(Modifier.weight(1f)) {
                if (colors.flat) {
                    Text("One-time code", color = colors.textMid, style = MaterialTheme.typography.bodySmall)
                    Spacer(Modifier.height(2.dp))
                }
                if (current != null) {
                    RollingCode(current)
                } else {
                    Text("Counter-based", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
                }
                val next = row?.nextCode
                AnimatedVisibility(next != null && shell.prefs.codeNext.value) {
                    Text("Next  ${grouped(next.orEmpty())}", color = colors.textMid, style = PassType.secretSmall)
                }
            }
            if (otp.period != null) {
                if (shell.prefs.codeRing.value) LifeRing(otp.period!!, size = 34.dp, stroke = 3.5.dp)
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
        Text("Time to crack: ${s.crackSlowHash}", color = colors.textMid, style = MaterialTheme.typography.bodySmall)
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
                if (shell.prefs.tapCopies.value) {
                    ink.splash(at)
                    shell.copyPlain(copyAs, value)
                }
            }
            .padding(start = if (colors.flat) 16.dp else 18.dp, end = if (colors.flat) 4.dp else 18.dp, top = if (colors.flat) 12.dp else 13.dp, bottom = if (colors.flat) 12.dp else 13.dp),
    ) {
        if (colors.flat) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                Column(Modifier.weight(1f)) {
                    Text(label, color = colors.textMid, style = MaterialTheme.typography.bodySmall)
                    Spacer(Modifier.height(2.dp))
                    Text(value, color = colors.text, style = MaterialTheme.typography.bodyLarge)
                }
                RowIconButton(PassIcons.ContentCopy, "Copy $label") { shell.copyPlain(copyAs, value) }
            }
        } else {
            Text(label, color = colors.textLow, style = MaterialTheme.typography.labelMedium)
            Spacer(Modifier.height(3.dp))
            Text(value, color = colors.text, style = MaterialTheme.typography.bodyLarge)
        }
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
    val appContext = androidx.compose.ui.platform.LocalContext.current
    Column(
        Modifier
            .fillMaxWidth()
            .animateContentSize(Springs.glide())
            .inkSurface(ink, colors.primary)
            .tapAt { at ->
                if (Prefs.of(appContext).tapCopies.value) {
                    ink.splash(at)
                    onCopy()
                }
            }
            .padding(start = if (colors.flat) 16.dp else 18.dp, end = if (colors.flat) 4.dp else 10.dp, top = 12.dp, bottom = 12.dp),
    ) {
        Row(verticalAlignment = Alignment.CenterVertically) {
            Column(Modifier.weight(1f)) {
                Text(
                    label,
                    color = if (colors.flat) colors.textMid else colors.textLow,
                    style = if (colors.flat) MaterialTheme.typography.bodySmall else MaterialTheme.typography.labelMedium,
                )
                Spacer(Modifier.height(if (colors.flat) 2.dp else 3.dp))
                SecretText(shown, style = if (colors.flat && style == PassType.secret) PassType.secretSmall.copy(fontSize = 16.sp) else style)
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

/** An app a login is for: its name and icon when it is installed here, its package either way. */
@Composable
private fun AppLine(pkg: String) {
    val colors = palette
    val context = androidx.compose.ui.platform.LocalContext.current
    val label = remember(pkg) {
        runCatching { context.packageManager.run { getApplicationLabel(getApplicationInfo(pkg, 0)).toString() } }.getOrNull()
    }
    val launch = remember(pkg) { context.packageManager.getLaunchIntentForPackage(pkg) }
    var icon by remember(pkg) { mutableStateOf(com.wasimaster.wmpass.system.ItemIcons.remembered(com.wasimaster.wmpass.system.ItemIcons.appKey(pkg))) }
    LaunchedEffect(pkg) { if (icon == null) icon = com.wasimaster.wmpass.system.ItemIcons.app(context, pkg) }
    Row(
        Modifier
            .fillMaxWidth()
            .then(if (launch != null) Modifier.clickable { runCatching { context.startActivity(launch) } } else Modifier)
            .padding(horizontal = 16.dp, vertical = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        val picture = icon
        if (picture != null) {
            androidx.compose.foundation.Image(picture, null, Modifier.size(28.dp))
        } else {
            Icon(PassIcons.Smartphone, null, tint = colors.textMid, modifier = Modifier.size(24.dp))
        }
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(if (palette.flat) "App" else label ?: pkg, color = if (palette.flat) colors.textMid else colors.text, style = if (palette.flat) MaterialTheme.typography.bodySmall else MaterialTheme.typography.bodyLarge, maxLines = 1)
            if (palette.flat) Text(label ?: pkg, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
            Text(if (label == null) "Not installed on this phone" else pkg, color = colors.textLow, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (launch != null) Icon(PassIcons.OpenInNew, "Open ${label ?: pkg}", tint = colors.textLow, modifier = Modifier.size(18.dp))
    }
}

/** The site's own page for a new password, from Apple's list or the standard address. */
@Composable
private fun ChangePasswordLine(url: String) {
    val colors = palette
    val shell = LocalShell.current
    Row(
        Modifier
            .fillMaxWidth()
            .clickable { shell.openUrl(url) }
            .padding(horizontal = 16.dp, vertical = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(PassIcons.LockReset, null, tint = colors.primary, modifier = Modifier.size(22.dp))
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text("Change the password on the site", color = colors.primary, style = MaterialTheme.typography.bodyLarge)
            Text(url.removePrefix("https://"), color = colors.textLow, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
    }
}

@Composable
private fun PlainRow(text: String, icon: ImageVector, onClick: (() -> Unit)?, sub: String? = null, styled: androidx.compose.ui.text.AnnotatedString? = null) {
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
            // An address shows whole, however long; anything else keeps to two lines.
            if (styled != null) {
                Text(styled, style = MaterialTheme.typography.bodyLarge)
            } else {
                Text(text, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 2, overflow = TextOverflow.Ellipsis)
            }
            if (sub != null) Text(sub, color = colors.textMid, style = MaterialTheme.typography.bodySmall)
        }
        if (onClick != null) Icon(PassIcons.OpenInNew, null, tint = colors.textLow, modifier = Modifier.size(18.dp))
    }
}

@Composable
private fun ActionLine(
    icon: ImageVector,
    title: String,
    value: String? = null,
    tint: Color = palette.text,
    trailing: (@Composable () -> Unit)? = null,
    onClick: () -> Unit,
) {
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
        Column(Modifier.weight(1f)) {
            Text(title, color = tint, style = MaterialTheme.typography.bodyLarge)
            // Long explanations go under the title; short states sit at the end.
            if (value != null && value.length > 18) Text(value, color = colors.textMid, style = MaterialTheme.typography.bodySmall)
        }
        if (value != null && value.length <= 18) Text(value, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 1)
        if (trailing != null) {
            Spacer(Modifier.width(12.dp))
            trailing()
        }
    }
}

/** The colour an item's tile is drawn in, small. */
@Composable
private fun Swatch(face: com.wasimaster.wmpass.ui.parts.TileFace) {
    Box(Modifier.size(22.dp).clip(CircleShape).background(Brush.linearGradient(listOf(face.from, face.to))))
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
        GroupCard(plain = true) {
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
        GroupCard(plain = true) {
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
        GroupCard(plain = true) {
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

/**
 * Hands one item to a device in the same room, by QR codes alone. The other side shows a code
 * with a one-use key in it, this side seals the item to that key and shows the result.
 */
@Composable
private fun SendSheet(entry: Detail, onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    var sealed by remember { mutableStateOf<String?>(null) }
    var problem by remember { mutableStateOf<String?>(null) }
    PassSheet(onDismiss, "Send to a device") {
        val shown = sealed
        if (shown != null) {
            com.wasimaster.wmpass.ui.parts.QrCode(shown, Modifier.padding(horizontal = 30.dp))
            Spacer(Modifier.height(14.dp))
            Text("Scan this with the other device. It can be opened only there, once.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, textAlign = androidx.compose.ui.text.style.TextAlign.Center, modifier = Modifier.fillMaxWidth())
        } else {
            Text(
                "Open WM Pass on the other device and go to Settings, then Sync and accounts, then Receive an item. It shows a code for you to scan. Nothing travels over a network.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyLarge,
            )
            problem?.let {
                Spacer(Modifier.height(10.dp))
                Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium)
            }
            Spacer(Modifier.height(16.dp))
            PrimaryButton("Scan its code", Modifier.fillMaxWidth(), icon = PassIcons.QrCodeScanner) {
                shell.scanTarget = { text ->
                    shell.launch {
                        try {
                            val made = Pass.net("share.seal", online = false) {
                                put("id", entry.id)
                                put("text", text)
                            }.obj["text"]!!.jsonPrimitive.content
                            if (com.wasimaster.wmpass.ui.parts.qrFits(made)) {
                                sealed = made
                                shell.haptics.confirm()
                            } else {
                                problem = "This item is too large for a code. Remove its long notes or use sync."
                            }
                        } catch (failure: com.wasimaster.wmpass.core.NativeFailure) {
                            problem = failure.message
                        }
                    }
                }
            }
        }
    }
}

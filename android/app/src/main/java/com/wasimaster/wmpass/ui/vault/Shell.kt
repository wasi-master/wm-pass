package com.wasimaster.wmpass.ui.vault

import android.content.Context
import android.content.Intent
import android.net.Uri
import android.provider.OpenableColumns
import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.ExperimentalSharedTransitionApi
import androidx.compose.animation.SharedTransitionLayout
import androidx.compose.animation.SharedTransitionScope
import androidx.compose.animation.core.CubicBezierEasing
import androidx.compose.animation.core.tween
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.scaleOut
import androidx.compose.animation.slideInHorizontally
import androidx.compose.animation.slideInVertically
import androidx.compose.animation.slideOutHorizontally
import androidx.compose.animation.slideOutVertically
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.compositionLocalOf
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.input.key.KeyEventType
import androidx.compose.ui.input.key.onPreviewKeyEvent
import androidx.compose.ui.input.key.type
import androidx.compose.ui.platform.LocalContext
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import androidx.navigation.NavHostController
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.wasimaster.wmpass.app.Inbox
import com.wasimaster.wmpass.app.Incoming
import com.wasimaster.wmpass.app.LaunchGuard
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.SecretBytes
import com.wasimaster.wmpass.core.SecureClipboard
import com.wasimaster.wmpass.ui.fx.BackdropSource
import com.wasimaster.wmpass.ui.fx.LocalBackdrop
import com.wasimaster.wmpass.ui.fx.rememberBackdrop
import com.wasimaster.wmpass.ui.settings.settingsGraph
import com.wasimaster.wmpass.ui.theme.Haptics
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.rememberHaptics
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/** A change that can still be taken back. */
/** A line at the bottom of the screen, with a way to take back what it reports when there is one. */
/**
 * A tab coming in from the side its button sits on, as pages are pushed: the new one slides the
 * whole way across, the old one drifts a third of the way back and fades under it.
 */
fun tabSlide(forward: Boolean): androidx.compose.animation.ContentTransform {
    val shift = if (forward) 1 else -1
    val motion = androidx.compose.animation.core.tween<androidx.compose.ui.unit.IntOffset>(
        340,
        easing = androidx.compose.animation.core.CubicBezierEasing(0.05f, 0.7f, 0.1f, 1f),
    )
    return androidx.compose.animation.ContentTransform(
        androidx.compose.animation.slideInHorizontally(motion) { shift * it },
        androidx.compose.animation.slideOutHorizontally(motion) { -shift * it / 3 } + androidx.compose.animation.fadeOut(androidx.compose.animation.core.tween(260)),
    )
}

class UndoOffer(val message: String, val action: (suspend () -> Unit)? = null)

/** What every screen in the vault can ask of the shell around it. */
@Stable
class Shell(
    val nav: NavHostController,
    val prefs: Prefs,
    private val scope: CoroutineScope,
    private val context: Context,
    val haptics: Haptics,
) {
    var undo by mutableStateOf<UndoOffer?>(null)
    var tab by mutableStateOf(
        prefs.lastTab.value.takeIf { prefs.startTab.value == "last" && it in prefs.tabs } ?: prefs.tabs.firstOrNull() ?: "vault",
    )
    var searching by mutableStateOf(false)
    var adding by mutableStateOf(false)

    /** What the search box holds. It outlives a trip to an entry and back. */
    val searchField = TextFieldState()

    /** Opened this session, newest first. Forgotten on lock, since this shell goes with it. */
    val recents = mutableStateListOf<String>()

    /** A password just generated, waiting for the editor it was meant for. */
    var pendingPassword: String? = null

    /** An otpauth link or note waiting for the editor it was meant for. */
    var pendingOtp: String? = null
    var pendingNote: String? = null
    var pendingTitle: String? = null

    /** A shared `WIFI:` network waiting for the Wi-Fi editor. */
    var pendingWifi: String? = null

    /** Who gets the next QR code scanned. Setting it opens the scanner over whatever is showing. */
    var scanTarget by mutableStateOf<((String) -> Unit)?>(null)

    /** Who gets the next payment card read. Setting it opens the card scanner the same way. */
    var cardTarget by mutableStateOf<((com.wasimaster.wmpass.core.CardReader.Read) -> Unit)?>(null)
    val selection = mutableStateListOf<String>()

    /** The label of the last plain value copied, for the capsule. */
    var plainCopied by mutableStateOf<Pair<String, Long>?>(null)

    val selecting: Boolean get() = selection.isNotEmpty()

    fun toggleSelected(id: String) {
        haptics.tick()
        if (!selection.remove(id)) selection.add(id)
    }

    fun open(id: String) {
        recents.remove(id)
        recents.add(0, id)
        if (recents.size > 8) recents.removeAt(recents.lastIndex)
        nav.navigate("entry/$id")
    }

    fun launch(block: suspend CoroutineScope.() -> Unit) {
        scope.launch(block = block)
    }

    fun copySecret(label: String, fetch: suspend () -> SecretBytes) {
        haptics.confirm()
        scope.launch { SecureClipboard.copy(context, label, fetch()) }
    }

    fun copyPassword(id: String) = copySecret("Password") { Pass.reveal(id, "password") }

    private fun copyUsername(id: String) {
        scope.launch {
            val name = Pass.detail(id).fields.firstOrNull { it.key == "username" || it.key == "user" }?.text.orEmpty()
            if (name.isNotEmpty()) copyPlain("Username", name) else notice("This item has no username to copy")
        }
    }

    /** Whether a chosen row action means anything for this entry. */
    fun canDo(action: String, entry: com.wasimaster.wmpass.core.Summary): Boolean = when (action) {
        "password" -> entry.hasPassword
        "username" -> entry.kind == "login" || entry.kind == "database"
        "code" -> entry.hasOtp
        "none" -> false
        else -> true
    }

    /** Does what a tap, swipe or row button has been set to do. */
    fun perform(action: String, entry: com.wasimaster.wmpass.core.Summary) {
        when (action) {
            "password" -> copyPassword(entry.id)
            "username" -> copyUsername(entry.id)
            "code" -> copySecret("Code") { Pass.code(entry.id) }
            "trash" -> trash(entry.id, entry.title)
            "favourite" -> {
                haptics.click()
                scope.launch {
                    Pass.change("entry.common") {
                        put("id", entry.id)
                        put("favourite", !entry.favourite)
                    }
                }
            }
            "open" -> open(entry.id)
        }
    }

    /**
     * What a swipe, tap or row button set to [action] really does to this entry. Copying falls
     * back from the password to the code to the username, so an imported code still answers a
     * swipe set to copy passwords. Null when nothing fits.
     */
    fun effective(action: String, entry: com.wasimaster.wmpass.core.Summary): String? {
        if (canDo(action, entry)) return action
        if (action != "password" && action != "code" && action != "username") return null
        return listOf("password", "code", "username").firstOrNull { canDo(it, entry) }
    }

    /** Says something at the bottom of the screen, with nothing to undo. */
    fun notice(message: String) {
        undo = UndoOffer(message)
    }

    /** A tap on a row. Something that cannot be copied still opens. */
    fun tapped(entry: com.wasimaster.wmpass.core.Summary) {
        val action = prefs.tapAction.value
        val doing = if (action == "open") null else effective(action, entry)
        if (doing != null) perform(doing, entry) else open(entry.id)
    }

    fun copyPlain(label: String, text: String) {
        haptics.confirm()
        SecureClipboard.copyPlain(context, label, text)
        plainCopied = label to System.currentTimeMillis()
    }

    fun trash(id: String, title: String) {
        haptics.click()
        scope.launch {
            Pass.change("entry.trash") { put("id", id) }
            undo = UndoOffer("“$title” moved to the trash") {
                Pass.change("entry.restore") { put("id", id) }
            }
        }
    }

    fun openUrl(url: String) {
        val target = if (url.contains("://")) url else "https://$url"
        runCatching {
            context.startActivity(Intent(Intent.ACTION_VIEW, Uri.parse(target)).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK))
        }
    }

    fun lockNow() {
        scope.launch { Pass.lock() }
    }
}

/** Acts on whatever arrived from outside: a shortcut, something shared, a code's link. */
@Composable
private fun ReceiveIncoming() {
    val shell = LocalShell.current
    val context = LocalContext.current
    val incoming by Inbox.pending.collectAsStateWithLifecycle()
    LaunchedEffect(incoming) {
        val what = incoming ?: return@LaunchedEffect
        Inbox.pending.value = null
        when (what) {
            is Incoming.Route -> when {
                what.name == "search" -> shell.searching = true
                what.name == "add" -> shell.adding = true
                // From the Backups widget.
                what.name == "backups" -> shell.nav.navigate("settings/backup")
                // From the Favorites widget: an item id, which names nothing by itself.
                what.name.startsWith("item:") -> shell.open(what.name.removePrefix("item:"))
                else -> shell.tab = what.name
            }
            is Incoming.OtpLink -> {
                shell.pendingOtp = what.uri
                shell.nav.navigate("new/otp")
            }
            is Incoming.SharedText -> {
                val text = what.text.trim()
                when {
                    text.startsWith("otpauth://") -> {
                        shell.pendingOtp = text
                        shell.nav.navigate("new/otp")
                    }
                    text.startsWith("WIFI:", ignoreCase = true) -> {
                        shell.pendingWifi = text
                        shell.nav.navigate("new/wifi")
                    }
                    text.startsWith("http://") || text.startsWith("https://") -> {
                        // A link: show what is saved for it, and search offers to add it if nothing is.
                        val host = runCatching { Uri.parse(text).host }.getOrNull().orEmpty().removePrefix("www.")
                        shell.searchField.setTextAndPlaceCursorAtEnd(host)
                        shell.searching = true
                    }
                    else -> {
                        shell.pendingNote = text
                        shell.nav.navigate("new/note")
                    }
                }
            }
            is Incoming.SharedFile -> {
                val resolver = context.contentResolver
                val name = resolver.query(what.uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use {
                    if (it.moveToFirst()) it.getString(0) else null
                } ?: "Shared file"
                val bytes = withContext(Dispatchers.IO) { resolver.openInputStream(what.uri)?.use { it.readBytes() } }
                if (bytes != null) {
                    val id = Pass.change("entry.create") {
                        put("kind", "document")
                        put("title", name)
                    }.obj["id"]!!.jsonPrimitive.content
                    Pass.addAttachment(id, name, what.mime, bytes)
                    bytes.fill(0)
                    shell.open(id)
                }
            }
        }
    }
}

val LocalShell = compositionLocalOf<Shell> { error("no shell") }

@OptIn(ExperimentalSharedTransitionApi::class)
val LocalShared = compositionLocalOf<SharedTransitionScope?> { null }
val LocalRouteScope = compositionLocalOf<AnimatedVisibilityScope?> { null }

/** The vault, once it is open. */
@OptIn(ExperimentalSharedTransitionApi::class)
@Composable
fun VaultShell(prefs: Prefs) {
    val colors = palette
    val context = LocalContext.current
    val nav = rememberNavController()
    val scope = rememberCoroutineScope()
    val haptics = rememberHaptics()
    val shell = remember(nav) { Shell(nav, prefs, scope, context, haptics) }
    val backdrop = rememberBackdrop()
    // WM Keyboard's push, collapsed to a cut when motion is reduced.
    val push = tween<IntOffset>(if (LocalReduceMotion.current) 0 else 340, easing = CubicBezierEasing(0.05f, 0.7f, 0.1f, 1f))
    // Runs as the vault's first frame is put together, before the render thread gets it.
    DisposableEffect(Unit) {
        LaunchGuard.arm()
        onDispose {}
    }
    CompositionLocalProvider(LocalShell provides shell, LocalBackdrop provides backdrop) {
        Box(
            Modifier
                .fillMaxSize()
                .background(colors.ground)
                .onPreviewKeyEvent { event ->
                    if (event.type != KeyEventType.KeyDown) return@onPreviewKeyEvent false
                    val chord = com.wasimaster.wmpass.ui.settings.chordOf(event) ?: return@onPreviewKeyEvent false
                    val action = prefs.shortcuts.entries.firstOrNull { it.value == chord }?.key ?: return@onPreviewKeyEvent false
                    when (action) {
                        "search" -> shell.searching = true
                        "add" -> shell.adding = true
                        "lock" -> shell.lockNow()
                        "settings" -> nav.navigate("settings")
                        else -> shell.tab = action
                    }
                    true
                },
        ) {
            ReceiveIncoming()
            BackdropSource(backdrop, Modifier.fillMaxSize()) {
            SharedTransitionLayout(Modifier.fillMaxSize()) {
                CompositionLocalProvider(LocalShared provides this) {
                    com.wasimaster.wmpass.ui.settings.ProvideSettingsKit(prefs) {
                    NavHost(
                        nav,
                        startDestination = "home",
                        // A push, as in WM Keyboard: the new page comes the whole way in from the
                        // right while the one behind drifts a third of the way left. Pages are opaque,
                        // so nothing fades. A back gesture plays the same pair in reverse; left to
                        // itself, navigation-compose shrinks the page from every side instead.
                        enterTransition = { slideInHorizontally(push) { it } },
                        exitTransition = { slideOutHorizontally(push) { -it / 3 } },
                        popEnterTransition = { slideInHorizontally(push) { -it / 3 } },
                        popExitTransition = { slideOutHorizontally(push) { it } },
                        predictivePopEnterTransition = { slideInHorizontally(push) { -it / 3 } },
                        predictivePopExitTransition = { slideOutHorizontally(push) { it } },
                    ) {
                        composable("home") {
                            CompositionLocalProvider(LocalRouteScope provides this) { HomeScreen() }
                        }
                        composable(
                            "entry/{id}",
                            listOf(navArgument("id") { type = NavType.StringType }),
                            enterTransition = if (colors.flat) null else ({ fadeIn(tween(220, 40)) + scaleIn(tween(340), 0.97f) + slideInVertically(tween(340)) { it / 24 } }),
                            exitTransition = if (colors.flat) null else ({ fadeOut(tween(140)) }),
                            popEnterTransition = if (colors.flat) null else ({ fadeIn(tween(200, 40)) }),
                            popExitTransition = if (colors.flat) null else ({ fadeOut(tween(180)) + scaleOut(tween(280), 0.97f) + slideOutVertically(tween(280)) { it / 24 } }),
                        ) { entry ->
                            CompositionLocalProvider(LocalRouteScope provides this) {
                                EntryScreen(entry.arguments?.getString("id").orEmpty())
                            }
                        }
                        composable(
                            "edit/{id}",
                            listOf(navArgument("id") { type = NavType.StringType }),
                            enterTransition = if (colors.flat) ({ fadeIn(tween(210)) + slideInVertically(tween(300)) { it / 12 } }) else null,
                            popExitTransition = if (colors.flat) ({ fadeOut(tween(150)) + slideOutVertically(tween(220)) { it / 12 } }) else null,
                        ) { entry -> EditorScreen(entryId = entry.arguments?.getString("id").orEmpty(), newKind = null) }
                        composable(
                            "new/{kind}?title={title}",
                            listOf(
                                navArgument("kind") { type = NavType.StringType },
                                navArgument("title") {
                                    type = NavType.StringType
                                    defaultValue = ""
                                },
                            ),
                            enterTransition = if (colors.flat) ({ fadeIn(tween(210)) + slideInVertically(tween(300)) { it / 12 } }) else null,
                            popExitTransition = if (colors.flat) ({ fadeOut(tween(150)) + slideOutVertically(tween(220)) { it / 12 } }) else null,
                        ) { entry ->
                            EditorScreen(
                                entryId = null,
                                newKind = entry.arguments?.getString("kind").orEmpty(),
                                presetTitle = entry.arguments?.getString("title").orEmpty(),
                            )
                        }
                        settingsGraph()
                    }
                    }
                }
            }
            }
            // Above every page: the capsule that counts a copied secret down.
            if (colors.flat) {
                CopyCapsule(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 88.dp))
            } else {
                CopyCapsule(Modifier.align(Alignment.TopCenter).statusBarsPadding())
            }
            ChildPill(Modifier.align(Alignment.BottomCenter).navigationBarsPadding().padding(bottom = 96.dp))
            shell.scanTarget?.let { target ->
                ScanDialog(
                    onCode = { text ->
                        shell.scanTarget = null
                        target(text)
                    },
                    onClose = { shell.scanTarget = null },
                )
            }
            shell.cardTarget?.let { target ->
                CardScanDialog(
                    onCard = { read ->
                        shell.cardTarget = null
                        target(read)
                    },
                    onClose = { shell.cardTarget = null },
                )
            }
            // The guide: its sheet, the first-time welcome and a tour's spotlight, over everything.
            com.wasimaster.wmpass.ui.help.HelpHost(prefs)
        }
    }
}

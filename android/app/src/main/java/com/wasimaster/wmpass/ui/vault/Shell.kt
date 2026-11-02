package com.wasimaster.wmpass.ui.vault

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.compose.animation.AnimatedVisibilityScope
import androidx.compose.animation.ExperimentalSharedTransitionApi
import androidx.compose.animation.SharedTransitionLayout
import androidx.compose.animation.SharedTransitionScope
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
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
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
import androidx.compose.ui.platform.LocalContext
import androidx.navigation.NavHostController
import androidx.navigation.NavType
import androidx.navigation.compose.NavHost
import androidx.navigation.compose.composable
import androidx.navigation.compose.rememberNavController
import androidx.navigation.navArgument
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.SecretBytes
import com.wasimaster.wmpass.core.SecureClipboard
import com.wasimaster.wmpass.ui.fx.LocalBackdrop
import com.wasimaster.wmpass.ui.fx.backdropSource
import com.wasimaster.wmpass.ui.fx.rememberBackdrop
import com.wasimaster.wmpass.ui.settings.settingsGraph
import com.wasimaster.wmpass.ui.theme.Haptics
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.theme.rememberHaptics
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.serialization.json.put

/** A change that can still be taken back. */
class UndoOffer(val message: String, val action: suspend () -> Unit)

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
    var tab by mutableStateOf(prefs.tabs.firstOrNull() ?: "vault")
    var searching by mutableStateOf(false)
    var adding by mutableStateOf(false)

    /** What the search box holds. It outlives a trip to an entry and back. */
    val searchField = TextFieldState()

    /** Opened this session, newest first. Forgotten on lock, since this shell goes with it. */
    val recents = mutableStateListOf<String>()

    /** A password just generated, waiting for the editor it was meant for. */
    var pendingPassword: String? = null

    /** Who gets the next QR code scanned. */
    var scanTarget: ((String) -> Unit)? = null
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
    CompositionLocalProvider(LocalShell provides shell, LocalBackdrop provides backdrop) {
        Box(Modifier.fillMaxSize().background(colors.ground)) {
            SharedTransitionLayout(Modifier.fillMaxSize().backdropSource(backdrop)) {
                CompositionLocalProvider(LocalShared provides this) {
                    NavHost(
                        nav,
                        startDestination = "home",
                        enterTransition = { fadeIn(tween(260, 60)) + slideInHorizontally(tween(340)) { it / 7 } },
                        exitTransition = { fadeOut(tween(160)) + slideOutHorizontally(tween(340)) { -it / 12 } },
                        popEnterTransition = { fadeIn(tween(240, 40)) + slideInHorizontally(tween(320)) { -it / 12 } },
                        popExitTransition = { fadeOut(tween(160)) + slideOutHorizontally(tween(320)) { it / 7 } },
                    ) {
                        composable("home") {
                            CompositionLocalProvider(LocalRouteScope provides this) { HomeScreen() }
                        }
                        composable(
                            "entry/{id}",
                            listOf(navArgument("id") { type = NavType.StringType }),
                            enterTransition = { fadeIn(tween(220, 40)) + scaleIn(tween(340), 0.97f) + slideInVertically(tween(340)) { it / 24 } },
                            exitTransition = { fadeOut(tween(140)) },
                            popEnterTransition = { fadeIn(tween(200, 40)) },
                            popExitTransition = { fadeOut(tween(180)) + scaleOut(tween(280), 0.97f) + slideOutVertically(tween(280)) { it / 24 } },
                        ) { entry ->
                            CompositionLocalProvider(LocalRouteScope provides this) {
                                EntryScreen(entry.arguments?.getString("id").orEmpty())
                            }
                        }
                        composable(
                            "edit/{id}",
                            listOf(navArgument("id") { type = NavType.StringType }),
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
            // Above every page: the capsule that counts a copied secret down.
            CopyCapsule(Modifier.align(Alignment.TopCenter).statusBarsPadding())
        }
    }
}

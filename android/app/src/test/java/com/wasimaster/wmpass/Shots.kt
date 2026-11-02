package com.wasimaster.wmpass

import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.navigation.compose.rememberNavController
import com.github.takahirom.roborazzi.captureRoboImage
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.door.DoorScreen
import com.wasimaster.wmpass.ui.fx.FxSupport
import com.wasimaster.wmpass.ui.theme.Look
import com.wasimaster.wmpass.ui.theme.PassTheme
import com.wasimaster.wmpass.ui.theme.rememberHaptics
import com.wasimaster.wmpass.ui.vault.EditorScreen
import com.wasimaster.wmpass.ui.vault.EntryScreen
import com.wasimaster.wmpass.ui.vault.HomeScreen
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.Shell
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.JsonNull
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/** Stills of the app for checking how it looks, drawn on this computer. */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "w393dp-h852dp-xxhdpi")
class Shots {
    @get:Rule val rule = createComposeRule()

    private val prefs get() = Prefs.of(RuntimeEnvironment.getApplication())

    @Before
    fun setUp() {
        FxSupport.shaders = false
        FxSupport.blur = false
        Pass.init(RuntimeEnvironment.getApplication())
        prefs.updateTabs(Prefs.DEFAULT_TABS)
    }

    private fun shot(name: String, look: Look = Look.DARK, advance: Long = 1600, content: @Composable () -> Unit) {
        // The clock only moves by hand: the door has animations that never end, so the
        // app is never "idle" in the sense a test usually waits for.
        rule.mainClock.autoAdvance = false
        rule.setContent { PassTheme(look) { content() } }
        rule.mainClock.advanceTimeBy(advance)
        // Engine calls finish on another thread; let them land, then let the UI catch up.
        repeat(6) {
            Thread.sleep(120)
            rule.mainClock.advanceTimeBy(400)
        }
        rule.onRoot().captureRoboImage("build/shots/$name.png")
    }

    /** A vault that looks like someone's real one, with fake data. */
    private fun seed(): Map<String, String> = runBlocking {
        Pass.lock()
        val ids = mutableMapOf<String, String>()
        if (Pass.state.value is LockState.NoVault) {
            Pass.create("Personal", "correct horse battery staple".toByteArray())
        } else {
            Pass.unlock("correct horse battery staple".toByteArray())
        }
        Pass.entries.value.forEach { e -> Pass.change("entry.purge") { put("id", e.id) } }
        suspend fun login(title: String, user: String, password: String, site: String, otp: String? = null, fav: Boolean = false, pin: Boolean = false): String {
            val id = Pass.change("entry.create") {
                put("kind", "login")
                put("title", title)
                put("fields", buildJsonObject { put("username", user) })
                putJsonArray("sites") { addJsonObject { put("Url", "https://$site") } }
            }.obj["id"]!!.jsonPrimitive.content
            Pass.change("entry.set_secret", secret = password.toByteArray()) {
                put("id", id)
                put("field", "password")
            }
            if (otp != null) {
                Pass.change("entry.otp_set", secret = otp.toByteArray()) {
                    put("id", id)
                    put("issuer", title)
                    put("account", user)
                }
            }
            Pass.change("entry.common") {
                put("id", id)
                put("favourite", fav)
                put("pinned", pin)
                put("color", JsonNull)
            }
            ids[title] = id
            return id
        }
        val github = login("GitHub", "wasi@example.com", "h7#Lq2!vZp9\$Rt4xK", "github.com", "JBSWY3DPEHPK3PXP", fav = true, pin = true)
        login("Proton Mail", "wasi@proton.me", "Tr0ub4dor&3xtra-long-one", "proton.me", "GEZDGNBVGY3TQOJQ", fav = true)
        login("Amazon", "wasi@example.com", "password123", "amazon.com", fav = true)
        login("Netflix", "family@example.com", "password123", "netflix.com")
        login("Google", "wasi@gmail.com", "b3!pZk9@mXq2#vLw", "google.com", "MFRGGZDFMZTWQ2LK", fav = true)
        login("Google", "work@company.com", "Yn5%tGh8^jK1&wE", "google.com")
        login("Figma", "wasi@example.com", "c9*Vb6(nM3)xQz", "figma.com")
        login("Discord", "wasimaster", "dR4+fT7=yU2-iO", "discord.com", "KRSXG5CTMVRXEZLU")
        login("Steam", "wasimaster", "sJ8_kL1~zX5`cV", "store.steampowered.com")
        suspend fun other(kind: String, title: String, fields: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit = {}): String {
            val id = Pass.change("entry.create") {
                put("kind", kind)
                put("title", title)
                put("fields", buildJsonObject(fields))
            }.obj["id"]!!.jsonPrimitive.content
            ids[title] = id
            return id
        }
        other("card", "Everyday Visa") {
            put("holder", "Wasi Master")
            put("issuer", "City Bank")
            put("expiry", "09/29")
        }
        other("wifi", "Home Wi-Fi") { put("ssid", "Bananas_5G") }
        other("note", "Passport notes")
        other("api", "OpenRouter") { put("provider", "OpenRouter") }
        ids
    }

    @Test
    fun doorHello() = shot("door-hello") { DoorScreen(LockState.NoVault, prefs) }

    @Test
    fun doorLocked() = shot("door-locked") { DoorScreen(LockState.Locked, prefs) }

    @Composable
    private fun WithShell(tab: String? = null, configure: (Shell) -> Unit = {}, content: @Composable () -> Unit) {
        val context = LocalContext.current
        val nav = rememberNavController()
        val scope = rememberCoroutineScope()
        val haptics = rememberHaptics()
        val shell = remember {
            Shell(nav, prefs, scope, context, haptics).also {
                if (tab != null) it.tab = tab
                configure(it)
            }
        }
        CompositionLocalProvider(LocalShell provides shell) { content() }
    }

    @Test
    fun vaultDark() {
        seed()
        shot("vault-dark") { WithShell { HomeScreen() } }
    }

    @Test
    fun vaultLight() {
        seed()
        shot("vault-light", Look.LIGHT) { WithShell { HomeScreen() } }
    }

    @Test
    fun entry() {
        val ids = seed()
        shot("entry-dark") { WithShell { EntryScreen(ids.getValue("GitHub")) } }
    }

    @Test
    fun entryLight() {
        val ids = seed()
        shot("entry-light", Look.LIGHT) { WithShell { EntryScreen(ids.getValue("GitHub")) } }
    }

    @Test
    fun editor() {
        val ids = seed()
        shot("editor-dark") { WithShell { EditorScreen(ids.getValue("GitHub"), null) } }
    }

    @Test
    fun editorNew() {
        seed()
        shot("editor-new") { WithShell { EditorScreen(null, "login") } }
    }

    @Test
    fun codes() {
        seed()
        shot("codes-dark") { WithShell("codes") { HomeScreen() } }
    }

    @Test
    fun generator() {
        seed()
        shot("generator-dark") { WithShell("generator") { HomeScreen() } }
    }

    @Test
    fun health() {
        seed()
        shot("health-dark") { WithShell("health") { HomeScreen() } }
    }

    @Test
    fun healthLight() {
        seed()
        shot("health-light", Look.LIGHT) { WithShell("health") { HomeScreen() } }
    }

    @Test
    fun search() {
        seed()
        shot("search-start") { WithShell(configure = { it.searching = true }) { HomeScreen() } }
    }

    @Test
    fun add() {
        seed()
        shot("add") { WithShell(configure = { it.adding = true }) { HomeScreen() } }
    }

    @Test
    fun selecting() {
        val ids = seed()
        shot("select") {
            WithShell(configure = {
                it.selection.add(ids.getValue("Amazon"))
                it.selection.add(ids.getValue("Figma"))
            }) { HomeScreen() }
        }
    }

    @Test
    fun settings() {
        seed()
        shot("settings-root") { WithShell { com.wasimaster.wmpass.ui.settings.SettingsRoot() } }
    }
}

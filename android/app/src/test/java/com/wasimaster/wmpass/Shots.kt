package com.wasimaster.wmpass

import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
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
import com.wasimaster.wmpass.ui.theme.palette
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

    private fun shot(
        name: String,
        look: Look = Look.DARK,
        advance: Long = 1600,
        theme: String = com.wasimaster.wmpass.ui.theme.BuiltInThemes.DEFAULT,
        style: String = "standard",
        content: @Composable () -> Unit,
    ) {
        // The clock only moves by hand: the door has animations that never end, so the
        // app is never "idle" in the sense a test usually waits for.
        rule.mainClock.autoAdvance = false
        rule.setContent { PassTheme(look, themeId = theme, style = style) { content() } }
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
        CompositionLocalProvider(LocalShell provides shell) { com.wasimaster.wmpass.ui.settings.ProvideSettingsKit(prefs) { content() } }
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

    @Test
    fun dracula() {
        val ids = seed()
        shot("theme-dracula", theme = "dracula") { WithShell { HomeScreen() } }
    }

    @Test
    fun nordEntry() {
        val ids = seed()
        shot("theme-nord-entry", theme = "nord") { WithShell { EntryScreen(ids.getValue("GitHub")) } }
    }

    @Test
    fun catppuccinLight() {
        seed()
        shot("theme-catppuccin-light", Look.LIGHT, theme = "catppuccin") { WithShell { HomeScreen() } }
    }

    @Test
    fun solarizedLight() {
        val ids = seed()
        shot("theme-solarized-light", Look.LIGHT, theme = "solarized") { WithShell { EntryScreen(ids.getValue("GitHub")) } }
    }

    @Test
    fun glass() {
        seed()
        shot("glass-vault", style = "glass") { WithShell { HomeScreen() } }
    }

    @Test
    fun themes() {
        seed()
        shot("settings-themes") { WithShell { com.wasimaster.wmpass.ui.settings.ThemesScreen() } }
    }

    @Test
    fun themeEditor() {
        seed()
        shot("settings-theme-edit") { WithShell { com.wasimaster.wmpass.ui.settings.ThemeEditor("dracula") } }
    }

    @Test
    fun look() {
        seed()
        shot("settings-look") { WithShell { com.wasimaster.wmpass.ui.settings.LookScreen() } }
    }

    @Test
    fun arcFlagsAreSpacedOut() {
        val spaced = com.wasimaster.wmpass.ui.parts.spaceArcFlags("M1 2a1 1 0 00-1 2.5.5.5 0 01.5.5l3 4")
        org.junit.Assert.assertEquals("M1 2a1 1 0 0 0 -1 2.5 .5 .5 0 0 1 .5 .5 l3 4", spaced)
    }

    @Test
    fun iconsPage() {
        seed()
        shot("settings-icons") { WithShell { com.wasimaster.wmpass.ui.settings.IconsScreen() } }
    }

    @Test
    fun listPage() {
        seed()
        shot("settings-list") { WithShell { com.wasimaster.wmpass.ui.settings.ListSettingsScreen() } }
    }

    @Test
    fun uniformIcons() {
        seed()
        prefs.siteIconStyle.set("uniform")
        try {
            shot("vault-uniform") { WithShell { HomeScreen() } }
        } finally {
            prefs.siteIconStyle.reset()
        }
    }

    @Test
    fun settingsAreSharedWithoutProtectionSettings() {
        val text = prefs.exportText()
        org.junit.Assert.assertFalse(text.contains("screen_protection"))
        org.junit.Assert.assertFalse(text.contains("lock_after"))
        org.junit.Assert.assertTrue(text.contains("icon_style"))
        val changed = prefs.importText("""{"format":"wmpass-settings","icon_style":"nonsense","layout":"grid","screen_protection":false}""")
        org.junit.Assert.assertEquals(1, changed)
        org.junit.Assert.assertEquals("grid", prefs.layout)
        org.junit.Assert.assertTrue(prefs.screenProtection)
        prefs.updateLayout("list")
    }

    /** The guide's sheet as it opens over a page, its body alone since a sheet is a window of its own. */
    @Composable
    private fun HelpPage(id: String) {
        val shell = LocalShell.current
        Column(
            Modifier
                .fillMaxSize()
                .background(palette.surface1)
                .verticalScroll(rememberScrollState())
                .padding(horizontal = 18.dp, vertical = 16.dp),
        ) { com.wasimaster.wmpass.ui.help.HelpSheetContent(id, prefs, shell) }
    }

    @Test
    fun helpSheet() {
        seed()
        com.wasimaster.wmpass.ui.help.Help.attach(RuntimeEnvironment.getApplication())
        shot("help-sheet") { WithShell { HelpPage("setting.layout") } }
    }

    @Test
    fun helpSheetGlass() {
        seed()
        com.wasimaster.wmpass.ui.help.Help.attach(RuntimeEnvironment.getApplication())
        shot("help-sheet-glass", style = "glass") { WithShell { HelpPage("action.emergency_kit") } }
    }

    @Test
    fun guide() {
        seed()
        shot("settings-guide") { WithShell { com.wasimaster.wmpass.ui.help.GuideScreen() } }
    }

    @Test
    fun guideGlass() {
        seed()
        shot("settings-guide-glass", style = "glass") { WithShell { com.wasimaster.wmpass.ui.help.GuideScreen() } }
    }

    @Test
    fun addListFollowsSettings() {
        seed()
        prefs.updateAddKinds(listOf("login", "note", "card"))
        try {
            shot("add-short") { WithShell(configure = { it.adding = true }) { HomeScreen() } }
        } finally {
            prefs.updateAddKinds(Prefs.DEFAULT_ADD)
        }
    }

    // ---- large screens: a tablet both ways round, a book-style foldable open, and a phone on its side ----

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletVault() {
        seed()
        shot("large-tablet-vault") { WithShell { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletVaultGlass() {
        seed()
        shot("large-tablet-vault-glass", style = "glass") { WithShell { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = TABLET_PORTRAIT)
    fun tabletPortraitVault() {
        seed()
        shot("large-tablet-portrait-vault") { WithShell { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = FOLDABLE_OPEN)
    fun foldableVault() {
        seed()
        shot("large-foldable-vault") { WithShell { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = FOLDABLE_OPEN)
    fun foldableVaultGlass() {
        seed()
        shot("large-foldable-vault-glass", style = "glass") { WithShell { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = PHONE_LANDSCAPE)
    fun phoneLandscapeVault() {
        seed()
        shot("large-phone-landscape-vault") { WithShell { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletEntry() {
        val ids = seed()
        shot("large-tablet-entry") { WithShell { EntryScreen(ids.getValue("GitHub")) } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletEntryGlass() {
        val ids = seed()
        shot("large-tablet-entry-glass", style = "glass") { WithShell { EntryScreen(ids.getValue("GitHub")) } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletEditor() {
        val ids = seed()
        shot("large-tablet-editor") { WithShell { EditorScreen(ids.getValue("GitHub"), null) } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletGenerator() {
        seed()
        shot("large-tablet-generator") { WithShell("generator") { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletHealth() {
        seed()
        shot("large-tablet-health") { WithShell("health") { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletSettings() {
        seed()
        shot("large-tablet-settings") { WithShell { com.wasimaster.wmpass.ui.settings.SettingsRoot() } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletDoor() = shot("large-tablet-door") { DoorScreen(LockState.Locked, prefs) }

    @Test
    @Config(qualifiers = PHONE_LANDSCAPE)
    fun phoneLandscapeDoor() = shot("large-phone-landscape-door") { DoorScreen(LockState.Locked, prefs) }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletSearch() {
        seed()
        shot("large-tablet-search") { WithShell(configure = { it.searching = true }) { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletAdd() {
        seed()
        shot("large-tablet-add") { WithShell(configure = { it.adding = true }) { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletVaultWithItem() {
        val ids = seed()
        shot("large-tablet-vault-item") { WithShell(configure = { it.paneEntry = ids.getValue("GitHub") }) { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = TABLET_LANDSCAPE)
    fun tabletVaultWithItemGlass() {
        val ids = seed()
        shot("large-tablet-vault-item-glass", style = "glass") { WithShell(configure = { it.paneEntry = ids.getValue("GitHub") }) { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = FOLDABLE_OPEN)
    fun foldableVaultWithItem() {
        val ids = seed()
        shot("large-foldable-vault-item") { WithShell(configure = { it.paneEntry = ids.getValue("GitHub") }) { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = FOLDABLE_TALL)
    fun foldableTallVault() {
        seed()
        shot("large-foldable-tall-vault") { WithShell { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = FOLDABLE_TALL)
    fun foldableTallVaultGlass() {
        seed()
        shot("large-foldable-tall-vault-glass", style = "glass") { WithShell { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = PHONE_LANDSCAPE)
    fun phoneLandscapeVaultGlass() {
        seed()
        shot("large-phone-landscape-vault-glass", style = "glass") { WithShell { HomeScreen() } }
    }

    @Test
    @Config(qualifiers = TABLET_PORTRAIT)
    fun tabletGrid() {
        seed()
        prefs.updateLayout("grid")
        try {
            shot("large-tablet-grid", style = "glass") { WithShell(configure = { it.tab = "vault" }) { HomeScreen() } }
        } finally {
            prefs.updateLayout("list")
        }
    }

    private companion object {
        /** A book-style foldable open with its fold upright, like a Galaxy Z Fold. */
        const val FOLDABLE_TALL = "w673dp-h841dp-port-xhdpi"
        const val TABLET_LANDSCAPE = "w1280dp-h800dp-land-xhdpi"
        const val TABLET_PORTRAIT = "w800dp-h1280dp-port-xhdpi"
        const val FOLDABLE_OPEN = "w841dp-h701dp-land-xhdpi"
        const val PHONE_LANDSCAPE = "w852dp-h393dp-land-xxhdpi"
    }
}

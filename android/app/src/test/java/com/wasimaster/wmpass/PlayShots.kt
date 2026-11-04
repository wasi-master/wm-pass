package com.wasimaster.wmpass

import android.content.pm.ApplicationInfo
import android.content.pm.PackageInfo
import android.graphics.Bitmap
import android.graphics.Canvas
import android.graphics.Matrix
import android.graphics.Paint
import android.graphics.drawable.BitmapDrawable
import androidx.compose.runtime.Composable
import androidx.compose.runtime.CompositionLocalProvider
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.test.junit4.createComposeRule
import androidx.compose.ui.test.onRoot
import androidx.core.graphics.PathParser
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
import kotlinx.serialization.json.putJsonObject
import org.junit.Before
import org.junit.Rule
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The raw screens for the Play Store slides, at 1440x2560 (9:16, which Play takes as is).
 * play/slides.py frames them. The demo vault leads with a site most people know, and the
 * first-run hints are off so no screen carries a tip.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "w360dp-h640dp-xxxhdpi")
class PlayShots {
    @get:Rule val rule = createComposeRule()

    private val app get() = RuntimeEnvironment.getApplication()
    private val prefs get() = Prefs.of(app)

    @Before
    fun setUp() {
        FxSupport.shaders = false
        FxSupport.blur = false
        Pass.init(app)
        prefs.updateTabs(Prefs.DEFAULT_TABS)
        prefs.helpHints.set(false)
        installFacebook()
    }

    /** Facebook as if it were installed, so the editor can show it among a login's apps. */
    private fun installFacebook() {
        val pkg = "com.facebook.katana"
        val info = PackageInfo().apply {
            packageName = pkg
            applicationInfo = ApplicationInfo().apply {
                packageName = pkg
                nonLocalizedLabel = "Facebook"
            }
        }
        val pm = shadowOf(app.packageManager)
        pm.installPackage(info)
        // The app's own Facebook mark (Simple Icons, CC0), drawn as a launcher icon.
        val mark = "M9.101 23.691v-7.98H6.627v-3.667h2.474v-1.58c0-4.085 1.848-5.978 5.858-5.978.401 0 .955.042 1.468.103a8.68 8.68 0 0 1 1.141.195v3.325a8.623 8.623 0 0 0-.653-.036 26.805 26.805 0 0 0-.733-.009c-.707 0-1.259.096-1.675.309a1.686 1.686 0 0 0-.679.622c-.258.42-.374.995-.374 1.752v1.297h3.919l-.386 2.103-.287 1.564h-3.246v8.245C19.396 23.238 24 18.179 24 12.044c0-6.627-5.373-12-12-12s-12 5.373-12 12c0 5.628 3.874 10.35 9.101 11.647Z"
        val size = 192
        val bitmap = Bitmap.createBitmap(size, size, Bitmap.Config.ARGB_8888)
        val canvas = Canvas(bitmap)
        canvas.drawCircle(size / 2f, size / 2f, size / 2f - 8, Paint(Paint.ANTI_ALIAS_FLAG).apply { color = 0xFFFFFFFF.toInt() })
        val path = PathParser.createPathFromPathData(mark)
        path.transform(Matrix().apply { setScale(size / 24f * 0.92f, size / 24f * 0.92f); postTranslate(size * 0.04f, size * 0.04f) })
        canvas.drawPath(path, Paint(Paint.ANTI_ALIAS_FLAG).apply { color = 0xFF0866FF.toInt() })
        pm.setApplicationIcon(pkg, BitmapDrawable(app.resources, bitmap))
    }

    private fun shot(name: String, look: Look = Look.DARK, style: String = "standard", content: @Composable () -> Unit) {
        rule.mainClock.autoAdvance = false
        rule.setContent { PassTheme(look, themeId = com.wasimaster.wmpass.ui.theme.BuiltInThemes.DEFAULT, style = style) { content() } }
        rule.mainClock.advanceTimeBy(1600)
        repeat(6) {
            Thread.sleep(120)
            rule.mainClock.advanceTimeBy(400)
        }
        rule.onRoot().captureRoboImage("build/play-shots/$name.png")
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
        suspend fun login(
            title: String, user: String, password: String, site: String,
            otp: String? = null, fav: Boolean = false, pin: Boolean = false, app: String? = null,
        ): String {
            val id = Pass.change("entry.create") {
                put("kind", "login")
                put("title", title)
                put("fields", buildJsonObject { put("username", user) })
                putJsonArray("sites") {
                    addJsonObject { put("Url", "https://$site") }
                    if (app != null) addJsonObject { putJsonObject("AndroidApp") { put("package", app); put("cert_sha256", JsonNull) } }
                }
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
        login("Facebook", "wasi@example.com", "h7#Lq2!vZp9\$Rt4xK", "facebook.com", "JBSWY3DPEHPK3PXP", fav = true, pin = true, app = "com.facebook.katana")
        login("Proton Mail", "wasi@proton.me", "Tr0ub4dor&3xtra-long-one", "proton.me", "GEZDGNBVGY3TQOJQ", fav = true)
        login("Amazon", "wasi@example.com", "password123", "amazon.com", fav = true)
        login("Netflix", "family@example.com", "password123", "netflix.com")
        login("Google", "wasi@gmail.com", "b3!pZk9@mXq2#vLw", "google.com", "MFRGGZDFMZTWQ2LK", fav = true)
        login("Google", "work@company.com", "Yn5%tGh8^jK1&wE", "google.com")
        login("Instagram", "wasi.master", "c9*Vb6(nM3)xQz", "instagram.com", "ONSWG4TFOQYTEMZU")
        login("Discord", "wasimaster", "dR4+fT7=yU2-iO", "discord.com", "KRSXG5CTMVRXEZLU")
        login("Spotify", "wasi@example.com", "sJ8_kL1~zX5`cV", "spotify.com")
        suspend fun other(kind: String, title: String, fields: kotlinx.serialization.json.JsonObjectBuilder.() -> Unit = {}) {
            val id = Pass.change("entry.create") {
                put("kind", kind)
                put("title", title)
                put("fields", buildJsonObject(fields))
            }.obj["id"]!!.jsonPrimitive.content
            ids[title] = id
        }
        other("card", "Everyday Visa") {
            put("holder", "Wasi Master")
            put("issuer", "City Bank")
            put("expiry", "09/29")
        }
        other("wifi", "Home Wi-Fi") { put("ssid", "Bananas_5G") }
        other("note", "Passport notes")
        ids
    }

    @Composable
    private fun WithShell(tab: String? = null, content: @Composable () -> Unit) {
        val context = LocalContext.current
        val nav = rememberNavController()
        val scope = rememberCoroutineScope()
        val haptics = rememberHaptics()
        val shell = remember { Shell(nav, prefs, scope, context, haptics).also { if (tab != null) it.tab = tab } }
        CompositionLocalProvider(LocalShell provides shell) { com.wasimaster.wmpass.ui.settings.ProvideSettingsKit(prefs) { content() } }
    }

    @Test
    fun vault() {
        seed()
        shot("vault") { WithShell { HomeScreen() } }
    }

    @Test
    fun entry() {
        val ids = seed()
        shot("entry") { WithShell { EntryScreen(ids.getValue("Facebook")) } }
    }

    @Test
    fun editor() {
        val ids = seed()
        prefs.setEditorOpen("sites", true)
        shot("editor") { WithShell { EditorScreen(ids.getValue("Facebook"), null) } }
    }

    @Test
    fun codes() {
        seed()
        shot("codes") { WithShell("codes") { HomeScreen() } }
    }

    @Test
    fun generator() {
        seed()
        shot("generator") { WithShell("generator") { HomeScreen() } }
    }

    @Test
    fun health() {
        seed()
        shot("health") { WithShell("health") { HomeScreen() } }
    }

    @Test
    fun glass() {
        seed()
        shot("glass", style = "glass") { WithShell { HomeScreen() } }
    }

    @Test
    fun themes() {
        seed()
        shot("themes") { WithShell { com.wasimaster.wmpass.ui.settings.ThemesScreen() } }
    }

    @Test
    fun light() {
        seed()
        shot("light", Look.LIGHT) { WithShell { HomeScreen() } }
    }

    @Test
    fun locked() {
        shot("locked") { DoorScreen(LockState.Locked, prefs) }
    }
}

package com.wasimaster.wmpass

import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.help.Help
import com.wasimaster.wmpass.ui.help.figureIcon
import com.wasimaster.wmpass.ui.help.previews.hasPreview
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment
import org.robolectric.annotation.Config
import org.robolectric.annotation.GraphicsMode

/**
 * The guide as the phone reads it: the file is there and readable, and it covers the phone's
 * settings. Run the way [Shots] is run, so both share one sandbox: the native library behind the
 * vault loads once per test process, and a second sandbox could not load it again.
 */
@RunWith(RobolectricTestRunner::class)
@GraphicsMode(GraphicsMode.Mode.NATIVE)
@Config(qualifiers = "w393dp-h852dp-xxhdpi")
class HelpTest {
    @Before
    fun setUp() {
        Help.attach(RuntimeEnvironment.getApplication())
    }

    @Test
    fun theGuideFileParses() {
        val file = Help.file
        assertEquals("wmpass-help", file.format)
        assertTrue("the guide has no topics", file.topics.isNotEmpty())
        assertTrue("the guide has no tours", file.tours.isNotEmpty())
        assertTrue("the guide has no figures", file.figures.isNotEmpty())
        // Every topic the phone links to or lists as related is one the phone has.
        val ids = file.topics.map { it.id }.toSet()
        val broken = file.topics.flatMap { topic -> topic.related.filter { it !in ids }.map { "${topic.id} -> $it" } }
        assertTrue("related topics missing: $broken", broken.isEmpty())
        // The welcome's tour is the one the phone starts.
        assertTrue("tour.home is missing", file.tours.any { it.id == "tour.home" })
    }

    /**
     * Every setting someone can change on the phone has a page in the guide. This is the phone's
     * counterpart of the desktop's type check: add a pref without a `setting.<key>` topic for
     * Android and this fails.
     */
    @Test
    fun everySettingIsExplained() {
        val prefs = Prefs.of(RuntimeEnvironment.getApplication())
        val missing = prefs.keys.filter { it !in NotSettings && Help.forPref(it) == null }
        assertTrue("These settings have no setting.<key> topic for Android: $missing", missing.isEmpty())
    }

    @Test
    fun everyFigureIconHasAGlyph() {
        val missing = Help.file.figures.flatMap { figure -> figure.rows.flatten().mapNotNull { box -> box.icon?.takeIf { figureIcon(it) == null }?.let { "${figure.id}: $it" } } }
        assertTrue("Figure icons with no PassIcons glyph: $missing", missing.isEmpty())
    }

    @Test
    fun aRowIsFoundByItsTitleOnItsPage() {
        assertEquals("setting.lock_after", Help.forTitle("settings/security", "Lock after leaving the app")?.id)
        assertEquals("setting.settings_icons", Help.forTitle("settings/icons", "Style")?.id)
        assertEquals("page.security", Help.forRoute("settings/security")?.id)
        assertTrue(Help.effectDetail("lock_after", 60)?.isNotEmpty() == true)
    }

    @Test
    fun previewsTheGuideNamesAreDrawnOrLeftToTheFigure() {
        // A preview the phone does not draw is fine, but one it claims to must be a real id.
        val named = Help.file.topics.mapNotNull { it.preview }.toSet()
        assertTrue(named.any { hasPreview(it) })
    }

    companion object {
        /**
         * Keys that are the app's own memory rather than something a person sets, or that have no
         * row on the phone. Each says where its explanation lives instead.
         */
        private val NotSettings = setOf(
            // Which theme is chosen, and the ones someone made: Settings, Appearance, Theme, under action.theme_*.
            "theme_id",
            "custom_themes",
            // The page the vault last showed, for Open on, Last page used.
            "last_tab",
            // Which checks Health lists: each is its own action.health_check_* topic.
            "health_hidden",
            // Which folds on a settings page were left open.
            "folds_open",
            // No switch on the phone: a page that needs the network asks, and feature.network explains.
            "network_allowed",
            // Whether the vault list still offers, once, to be the autofill service.
            "autofill_nudge",
            // What the guide itself has shown on this phone.
            "help_welcome_seen",
            "help_seen",
            "help_tours_done",
            // Items left out of one Health check, taken back under Ignored on the Health page.
            "health_ignored",
            // How many problems the vault banner was put away at, so a new one brings it back.
            "health_banner_seen",
            // When the last breach check ran, shown on the Breaches page.
            "breach_last",
        )
    }
}

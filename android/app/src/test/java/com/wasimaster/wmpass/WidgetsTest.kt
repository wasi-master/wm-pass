package com.wasimaster.wmpass

import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.system.widgets.BackupRow
import com.wasimaster.wmpass.system.widgets.CodeRow
import com.wasimaster.wmpass.system.widgets.WidgetFeed
import com.wasimaster.wmpass.system.widgets.hiddenCode
import com.wasimaster.wmpass.system.widgets.overdue
import com.wasimaster.wmpass.system.widgets.spacedCode
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WidgetsTest {
    private val day = 86_400_000L
    private val now = 1_800_000_000_000L

    private fun code(endsIn: Long) = CodeRow("a", "A", "123456", "654321", now + endsIn)

    @Test
    fun `codes are drawn again as they run out`() {
        assertEquals(25_050L, WidgetFeed.nextWake(listOf(code(25_000)), showNext = false, now = now))
    }

    @Test
    fun `and ten seconds before, to show the next one`() {
        assertEquals(15_000L, WidgetFeed.nextWake(listOf(code(25_000)), showNext = true, now = now))
        // Inside the last ten seconds, only the end is left to wait for.
        assertEquals(5_050L, WidgetFeed.nextWake(listOf(code(5_000)), showNext = true, now = now))
    }

    @Test
    fun `the earliest of several codes wins, within a second and half a minute`() {
        assertEquals(3_050L, WidgetFeed.nextWake(listOf(code(28_000), code(3_000)), showNext = false, now = now))
        assertEquals(30_000L, WidgetFeed.nextWake(emptyList(), showNext = true, now = now))
        assertEquals(1_000L, WidgetFeed.nextWake(listOf(code(-5_000)), showNext = false, now = now))
    }

    @Test
    fun `a backup is late a day after its schedule`() {
        val row = { last: Long? -> BackupRow("a", "Home", last, false) }
        assertFalse(overdue(row(now - day), 1, now))
        assertFalse(overdue(row(now - 2 * day + 1000), 1, now))
        assertTrue(overdue(row(now - 2 * day - 1000), 1, now))
        assertTrue(overdue(row(null), 7, now))
        // With no schedule nothing is late.
        assertFalse(overdue(row(null), 0, now))
    }

    @Test
    fun `codes are split the way the app splits them`() {
        assertEquals("284 913", spacedCode("284913"))
        assertEquals("2849 1307", spacedCode("28491307"))
        assertEquals("12345", spacedCode("12345"))
        assertEquals("••• •••", hiddenCode("284913"))
    }

    @Test
    fun `the quick bar offers every button, and starts with the old four`() {
        assertEquals(listOf("search", "add", "codes", "lock"), Prefs.DEFAULT_WIDGET_BUTTONS)
        assertTrue(Prefs.WIDGET_BUTTONS.containsAll(Prefs.DEFAULT_WIDGET_BUTTONS))
    }
}

package com.wasimaster.wmpass.system

import com.wasimaster.wmpass.system.PanicPins.Signers
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class PanicPinsTest {
    private val ripple = "aa".repeat(32)
    private val squatter = "bb".repeat(32)
    private val rotated = "cc".repeat(32)

    @Test
    fun theSameSignerMatches() {
        assertTrue(PanicPins.matches(setOf(ripple), Signers(setOf(ripple), setOf(ripple))))
    }

    @Test
    fun anotherAppUnderTheSameNameDoesNot() {
        assertFalse(PanicPins.matches(setOf(ripple), Signers(setOf(squatter), setOf(squatter))))
    }

    @Test
    fun noPinOrNoSignersNeverMatches() {
        assertFalse(PanicPins.matches(emptySet(), Signers(setOf(ripple), setOf(ripple))))
        assertFalse(PanicPins.matches(setOf(ripple), null))
        assertFalse(PanicPins.matches(setOf(ripple), Signers(emptySet(), setOf(ripple))))
    }

    @Test
    fun aKeyRotatedFromThePinnedOneMatches() {
        assertTrue(PanicPins.matches(setOf(ripple), Signers(setOf(rotated), setOf(ripple, rotated))))
        // A lineage that never held the pinned key is another app.
        assertFalse(PanicPins.matches(setOf(ripple), Signers(setOf(rotated), setOf(squatter, rotated))))
    }

    @Test
    fun severalSignersMustAllBeThere() {
        assertTrue(PanicPins.matches(setOf(ripple, rotated), Signers(setOf(rotated, ripple), setOf(rotated, ripple))))
        assertFalse(PanicPins.matches(setOf(ripple, rotated), Signers(setOf(ripple), setOf(ripple))))
        assertFalse(PanicPins.matches(setOf(ripple), Signers(setOf(ripple, squatter), setOf(ripple, squatter))))
    }

    @Test
    fun pinsReadBackAsWritten() {
        val pins = mapOf("info.guardianproject.ripple" to setOf(ripple), "org.iilab.pb" to setOf(rotated, squatter))
        val text = PanicPins.encode(pins)
        assertEquals("info.guardianproject.ripple=$ripple;org.iilab.pb=$squatter|$rotated", text)
        assertEquals(pins, PanicPins.decode(text))
        assertEquals(emptyMap<String, Set<String>>(), PanicPins.decode(""))
        assertEquals(mapOf("a.b" to setOf(ripple)), PanicPins.decode("a.b=$ripple;broken;=x;c.d="))
    }
}

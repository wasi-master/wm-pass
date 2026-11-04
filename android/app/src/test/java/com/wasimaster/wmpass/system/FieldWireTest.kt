package com.wasimaster.wmpass.system

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class FieldWireTest {
    @Test
    fun aTagSaysTheScreenAndItsButtons() {
        val tag = FieldWire.tag("Kx3m9QaZ0bLw7RtE", listOf(FieldWire.WIFI_QR, FieldWire.WIFI_SAVED))
        // The same line WM Keyboard's FieldWireTest reads.
        assertEquals("com.wasimaster.suite.field=1;token=Kx3m9QaZ0bLw7RtE;actions=wifi_qr+wifi_saved", tag)
        assertEquals(FieldWire.Tag("Kx3m9QaZ0bLw7RtE", listOf("wifi_qr", "wifi_saved")), FieldWire.parse(tag))
    }

    @Test
    fun itSharesTheOptionsWithOtherApps() {
        val raw = "nm,com.wasimaster.suite.field=1;token=abcdefgh12;actions=card_scan, com.example.other=1"
        assertEquals(FieldWire.Tag("abcdefgh12", listOf(FieldWire.CARD_SCAN)), FieldWire.parse(raw))
    }

    @Test
    fun unknownButtonsAndKeysAreSkipped() {
        val later = "com.wasimaster.suite.field=2;token=abcdefgh12;colour=teal;actions=card_scan+print_card+card_scan"
        assertEquals(FieldWire.Tag("abcdefgh12", listOf(FieldWire.CARD_SCAN)), FieldWire.parse(later))
        assertNull(FieldWire.parse("com.wasimaster.suite.field=1;token=abcdefgh12;actions=print_card"))
        assertNull(FieldWire.tag("abcdefgh12", listOf("print_card")))
    }

    @Test
    fun whatIsNotATagReadsAsNone() {
        assertNull(FieldWire.parse(null))
        assertNull(FieldWire.parse(""))
        assertNull(FieldWire.parse("com.wasimaster.suite.field=x;token=abcdefgh12;actions=card_scan"))
        // A token too short to be one, or with a separator in it.
        assertNull(FieldWire.parse("com.wasimaster.suite.field=1;token=abc;actions=card_scan"))
        assertNull(FieldWire.tag("has space in it", listOf(FieldWire.CARD_SCAN)))
        assertNull(FieldWire.parse("com.wasimaster.suite.field=1;actions=card_scan"))
    }

    @Test
    fun aNewTokenFitsTheTag() {
        val token = FieldActions.newToken()
        assertEquals(16, token.length)
        assertEquals(token, FieldWire.parse(FieldWire.tag(token, listOf(FieldWire.CARD_SCAN)))?.token)
    }
}

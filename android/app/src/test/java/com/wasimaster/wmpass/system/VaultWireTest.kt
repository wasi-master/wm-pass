package com.wasimaster.wmpass.system

import com.wasimaster.wmpass.core.CustomFieldView
import com.wasimaster.wmpass.core.Detail
import com.wasimaster.wmpass.core.FieldView
import com.wasimaster.wmpass.core.Summary
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.put
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Test

class VaultWireTest {
    private val login = Detail(
        id = "a1",
        kind = "login",
        title = "GitHub",
        subtitle = "me@example.com",
        fields = listOf(
            FieldView("username", "Username", "text", hasValue = true, value = JsonPrimitive("me@example.com")),
            FieldView("password", "Password", "password", secret = true, hasValue = true),
        ),
        sites = listOf(
            buildJsonObject { put("Url", "https://github.com") },
            buildJsonObject {
                put(
                    "AndroidApp",
                    buildJsonObject {
                        put("package", "com.github.android")
                        put("cert_sha256", "ab")
                    },
                )
            },
        ),
        customFields = listOf(
            CustomFieldView(0, "Recovery email", "text", hasValue = true, value = "backup@example.com"),
            CustomFieldView(1, "PIN", "hidden", hasValue = true, value = null),
        ),
        hasNotes = true,
    )

    @Test
    fun anItemPageNamesItsSecretsButHoldsNone() {
        // The same page WM Keyboard's VaultWireTest reads.
        val expected = """{"v":1,"id":"a1","kind":"login","title":"GitHub","subtitle":"me@example.com","fields":[""" +
            """{"key":"username","label":"Username","secret":false,"value":"me@example.com"},""" +
            """{"key":"password","label":"Password","secret":true},""" +
            """{"key":"site:0","label":"Website","secret":false,"value":"https://github.com"},""" +
            """{"key":"custom:0","label":"Recovery email","secret":false,"value":"backup@example.com"},""" +
            """{"key":"custom:1","label":"PIN","secret":true},""" +
            """{"key":"code","label":"One-time code","secret":true},""" +
            """{"key":"notes","label":"Notes","secret":true}],"changes":"password"}"""
        assertEquals(expected, VaultWire.page(login, hasCode = true))
    }

    @Test
    fun aSecretWithAValueBesideItStillStaysOut() {
        val card = Detail(
            id = "c1",
            kind = "card",
            title = "Visa",
            fields = listOf(
                FieldView("holder", "Name on card", "text", hasValue = true, value = JsonPrimitive("A Person")),
                FieldView("number", "Card number", "hidden", secret = true, hasValue = true, value = JsonPrimitive("SECRET-4111")),
                FieldView("cvv", "Security code", "hidden", secret = true, hasValue = false),
            ),
        )
        val page = VaultWire.page(card, hasCode = false)
        assertFalse(page, page.contains("SECRET"))
        assertFalse(page, page.contains("cvv"))
        assertFalse(page, page.contains("changes"))
    }

    @Test
    fun rowsSayWhatMatchesTheAppInFront() {
        val rows = VaultWire.rows(
            listOf(
                Summary(id = "a1", kind = "login", title = "GitHub", subtitle = "me@example.com", hasPassword = true, hasOtp = true),
                Summary(id = "w1", kind = "wifi", title = "Home", favourite = true),
            ),
            matched = setOf("a1"),
        )
        // The same rows WM Keyboard's VaultWireTest reads.
        val expected = """[{"id":"a1","kind":"login","title":"GitHub","subtitle":"me@example.com","matches":true,"has_password":true,"has_code":true,"favourite":false},""" +
            """{"id":"w1","kind":"wifi","title":"Home","subtitle":"","matches":false,"has_password":false,"has_code":false,"favourite":true}]"""
        assertEquals(expected, rows)
    }

    @Test
    fun plainFieldsAreTypedAsText() {
        assertNull(VaultWire.plain(FieldView("hidden_ssid", "Hidden network", "toggle", hasValue = true, value = JsonPrimitive(true))))
        assertEquals("2030-01-31", VaultWire.plain(FieldView("expires", "Expires", "date", hasValue = true, value = JsonPrimitive(1_896_048_000_000L))))
        assertEquals("5432", VaultWire.plain(FieldView("port", "Port", "number", hasValue = true, value = JsonPrimitive(5432))))
        assertNull(VaultWire.plain(FieldView("notes", "Notes", "text")))
    }

    @Test
    fun aNewLoginIsNamedForItsSite() {
        assertEquals("Github", VaultWire.titleFromSite("https://www.github.com/login"))
        assertEquals("Example", VaultWire.titleFromSite("example.co"))
        assertEquals("", VaultWire.titleFromSite(""))
        assertEquals("item:a1", VaultWire.openRoute("a1"))
    }
}

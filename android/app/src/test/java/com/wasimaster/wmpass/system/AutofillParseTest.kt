package com.wasimaster.wmpass.system

import android.app.assist.AssistStructure
import android.text.InputType
import android.view.View
import android.view.autofill.AutofillId
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.annotation.Config

/**
 * A plain [android.app.Application], not the app's own: its start loads the vault's native
 * library, which loads only once per test process, and this test runs in a sandbox of its own
 * beside the one the stills ([com.wasimaster.wmpass.Shots]) load it in.
 */
@RunWith(RobolectricTestRunner::class)
@Config(application = android.app.Application::class)
class AutofillParseTest {
    // AssistStructure has no public way to build one, so the test fills its fields directly.
    private fun <T> blank(type: Class<T>): T {
        val unsafeType = Class.forName("sun.misc.Unsafe")
        val unsafe = unsafeType.getDeclaredField("theUnsafe").apply { isAccessible = true }.get(null)
        return type.cast(unsafeType.getMethod("allocateInstance", Class::class.java).invoke(unsafe, type))
    }

    private fun Any.put(name: String, value: Any?) {
        var type: Class<*>? = javaClass
        while (type != null) {
            type.declaredFields.firstOrNull { it.name == name }?.let {
                it.isAccessible = true
                it.set(this, value)
                return
            }
            type = type.superclass
        }
        error("${javaClass.name} has no field $name")
    }

    private fun id(n: Int): AutofillId =
        AutofillId::class.java.getDeclaredConstructor(Int::class.javaPrimitiveType).apply { isAccessible = true }.newInstance(n)

    private fun field(n: Int, hints: Array<String?>?, inputType: Int) = blank(AssistStructure.ViewNode::class.java).apply {
        put("mAutofillId", id(n))
        put("mAutofillHints", hints)
        put("mAutofillType", View.AUTOFILL_TYPE_TEXT)
        put("mInputType", inputType)
    }

    private fun screen(vararg fields: AssistStructure.ViewNode): AssistStructure {
        val root = blank(AssistStructure.ViewNode::class.java).apply { put("mChildren", fields) }
        val window = blank(AssistStructure.WindowNode::class.java).apply { put("mRoot", root) }
        return blank(AssistStructure::class.java).apply {
            put("mHaveData", true)
            put("mWindowNodes", arrayListOf(window))
        }
    }

    @Test
    fun `a null among an app's autofill hints is passed over`() {
        val form = parseForm(
            screen(
                field(1, arrayOf(null, "emailAddress"), InputType.TYPE_CLASS_TEXT),
                field(2, arrayOf(null), InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD),
            ),
        )
        assertEquals(id(1), form.username)
        assertEquals(id(2), form.password)
    }

    private val textBox = InputType.TYPE_CLASS_TEXT
    private val passwordBox = InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD

    @Test
    fun `an email address box is the username, not a postal address`() {
        val form = parseForm(
            screen(
                field(1, null, textBox).apply { put("mIdEntry", "email_address") },
                field(2, null, passwordBox),
            ),
        )
        assertEquals(id(1), form.username)
        assertEquals(id(2), form.password)
    }

    @Test
    fun `a box that says it takes a one-time code gets no password, even as a PIN box`() {
        val form = parseForm(screen(field(1, arrayOf("smsOTPCode"), InputType.TYPE_CLASS_NUMBER or InputType.TYPE_NUMBER_VARIATION_PASSWORD)))
        assertEquals(id(1), form.otp)
        assertNull(form.password)
    }

    @Test
    fun `a code box before the password is not taken as its username`() {
        val form = parseForm(
            screen(
                field(1, null, textBox).apply { put("mIdEntry", "otp_code") },
                field(2, null, passwordBox),
            ),
        )
        assertEquals(id(1), form.otp)
        assertNull(form.username)
        assertEquals(id(2), form.password)
    }

    @Test
    fun `a change-password form watches every password box after the first`() {
        val form = parseForm(screen(field(1, null, passwordBox), field(2, null, passwordBox), field(3, null, passwordBox)))
        assertEquals(id(1), form.password)
        assertEquals(listOf(id(2), id(3)), form.morePasswords)
    }

    private fun sent(username: String?, password: String?, domain: String? = "example.com", app: String = "com.browser") =
        ParsedForm(app, domain, null, null, null, username, password)

    @Test
    fun `a password screen saves the username typed on the screen before it`() {
        val form = sent(null, "hunter2").withEarlierUsername(sequenceOf(sent("sam@example.com", null)))
        assertEquals("sam@example.com", form.usernameValue)
        assertEquals("hunter2", form.passwordValue)
    }

    @Test
    fun `a username from another site or app, or one on the screen itself, is not replaced`() {
        assertNull(sent(null, "hunter2").withEarlierUsername(sequenceOf(sent("sam", null, domain = "other.com"))).usernameValue)
        assertNull(sent(null, "hunter2").withEarlierUsername(sequenceOf(sent("sam", null, app = "com.other"))).usernameValue)
        assertEquals("kim", sent("kim", "hunter2").withEarlierUsername(sequenceOf(sent("sam", null))).usernameValue)
        // A screen that had a password of its own was another form, not this login's first step.
        assertNull(sent(null, "hunter2").withEarlierUsername(sequenceOf(sent("555-0100", "1234"))).usernameValue)
        // The newest earlier screen wins.
        assertEquals("new", sent(null, "hunter2").withEarlierUsername(sequenceOf(sent("new", null), sent("old", null))).usernameValue)
    }

    /** A web page's input box, as a browser reports it. */
    private fun input(n: Int, type: String?, name: String) = field(n, null, 0).apply {
        val html = blank(Class.forName("android.app.assist.AssistStructure\$HtmlInfoNode"))
        val names = listOfNotNull("name", type?.let { "type" }).toTypedArray()
        val values = listOfNotNull(name, type).toTypedArray()
        html.put("mTag", "input")
        html.put("mNames", names)
        html.put("mValues", values)
        put("mHtmlInfo", html)
    }

    @Test
    fun `a search box above a login is not taken as its username`() {
        val form = parseForm(screen(input(1, "search", "q"), input(2, "password", "pass")))
        assertNull(form.username)
        assertEquals(id(2), form.password)
    }

    @Test
    fun `a field with hints and no input type, as Compose gives, is still read`() {
        val form = parseForm(screen(field(1, arrayOf("username"), 0), field(2, arrayOf("password"), 0)))
        assertEquals(id(1), form.username)
        assertEquals(id(2), form.password)
    }

    @Test
    fun `the focused box is known, for a fill asked for by hand`() {
        val focusedFlag = AssistStructure.ViewNode::class.java.getDeclaredField("FLAGS_FOCUSED").apply { isAccessible = true }.getInt(null)
        val form = parseForm(screen(field(1, null, textBox), field(2, null, textBox).apply { put("mFlags", focusedFlag) }))
        assertEquals(false, form.isLogin)
        assertEquals(id(2), form.focused)
        val asked = form.withUsernameField(id(2))
        assertEquals(id(2), asked.username)
        assertEquals(true, asked.isLogin)
    }

    @Test
    fun `a username screen on its own has no password, so its value is handed on`() {
        val first = parseForm(screen(field(1, arrayOf("emailAddress"), textBox)))
        assertEquals(id(1), first.username)
        assertNull(first.password)
    }

    // ---- a browser's address bar, in compatibility mode ----

    private fun addressBar(n: Int, idEntry: String, text: String, domain: String? = null) = field(n, null, textBox).apply {
        put("mIdEntry", idEntry)
        put("mAutofillValue", android.view.autofill.AutofillValue.forText(text))
        if (domain != null) put("mWebDomain", domain)
    }

    @Test
    fun `a browser's address bar by its id is not taken as the username`() {
        val form = parseForm(screen(addressBar(1, "url_bar", "example.com/login"), field(2, null, passwordBox)))
        assertNull(form.username)
        assertEquals(id(2), form.password)
    }

    @Test
    fun `a box showing the page's own address is not taken as the username, whatever its id`() {
        val form = parseForm(screen(addressBar(1, "toolbar_input_42", "https://www.example.com/login?next=1", domain = "example.com"), field(2, null, passwordBox)))
        assertNull(form.username)
        assertEquals("example.com", form.webDomain)
        assertEquals(id(2), form.password)
    }

    @Test
    fun `an ordinary box before the password is still its username`() {
        val form = parseForm(screen(addressBar(1, "login_name", "sam", domain = "example.com"), field(2, null, passwordBox)))
        assertEquals(id(1), form.username)
    }

    @Test
    fun `an address is matched by its host alone`() {
        assertEquals(true, isAddressOf("https://www.Example.com/a/b", "example.com"))
        assertEquals(true, isAddressOf("example.com", "www.example.com"))
        assertEquals(false, isAddressOf("example.com.evil.net", "example.com"))
        assertEquals(false, isAddressOf("sam@example.com", "example.com"))
        assertEquals(false, isAddressOf("", "example.com"))
    }

    // ---- which apps may say what website a form is on ----

    @Test
    fun `only a browser may name the website its form is on`() {
        val compat = setOf("com.android.chrome", "mark.via")
        val verified = setOf("org.mozilla.firefox")
        fun trusted(pkg: String) = Trust.domainTrusted(pkg, compat) { it in verified }
        assertEquals(true, trusted("com.android.chrome"))
        assertEquals(true, trusted("mark.via"))
        assertEquals(true, trusted("org.mozilla.firefox"))
        // An app with a WebView that says it shows bank.com is matched by the app alone.
        assertEquals(false, trusted("com.example.flashlight"))
    }

    @Test
    fun `the compatibility-mode browsers are read from the autofill service's own list`() {
        val compat = Trust.compatBrowsers(org.robolectric.RuntimeEnvironment.getApplication())
        assertEquals(true, "com.android.chrome" in compat)
        assertEquals(true, "com.sec.android.app.sbrowser" in compat)
        assertEquals(false, "com.example.flashlight" in compat)
    }
}

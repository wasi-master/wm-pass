package com.wasimaster.wmpass.system

import android.app.assist.AssistStructure
import android.text.InputType
import android.view.View
import android.view.autofill.AutofillId

/** The fields of one screen, as far as can be told from its structure. */
class ParsedForm(
    val packageName: String,
    val webDomain: String?,
    val username: AutofillId?,
    val password: AutofillId?,
    val otp: AutofillId?,
    val usernameValue: String?,
    val passwordValue: String?,
    /**
     * The password boxes after the first, as on a change-password form. Never filled, but watched
     * for a save: the system brings a box's value up to date only if the response names it.
     */
    val morePasswords: List<AutofillId> = emptyList(),
    /** The text box that has the focus, whatever it is for: a fill asked for by hand goes there. */
    val focused: AutofillId? = null,
) {
    val ids: Array<AutofillId> get() = listOfNotNull(username, password, otp).toTypedArray()
    val isLogin: Boolean get() = username != null || password != null || otp != null
}

private val UserWords = listOf("user", "email", "e-mail", "login", "account", "phone", "mobile", "identifier")
private val OtpWords = listOf("otp", "one-time", "onetime", "2fa", "two-factor", "totp", "verification", "authcode", "auth_code")
private val NotLogin = listOf("search", "captcha", "comment", "message", "subject", "address", "coupon", "promo")

/** Input types that take no typed text, or text that is never a login's. */
private val NotText = setOf("hidden", "checkbox", "radio", "submit", "button", "reset", "file", "image", "search", "range", "color", "date", "datetime-local", "month", "week", "time")

/**
 * The ids of browsers' address bars. Filled in compatibility mode, a browser's page comes through
 * accessibility with its address bar beside it, an editable text box like any other: on a page
 * with a password and nothing before it, the bar would be taken for the username.
 */
private val AddressBarIds = setOf(
    "url_bar", "url_bar_title", "url_field", "url_edit", "url_edittext", "url_text", "urlbar", "url",
    "location_bar_edit_text", "mozac_browser_toolbar_url_view", "mozac_browser_toolbar_edit_url_view",
    "omnibarTextInput", "omnibox", "omnibox_text", "bro_omnibar_address_title_text", "search_box",
    "search_box_text", "address_bar_edit_text", "addressbar_url", "addressbar_edit", "edit_text_url",
    "et_search_url", "toolbar_url", "search_fragment_input_view", "searchbox", "search_edit_text",
)

/** Whether [text] is the address of a page on [domain], as an address bar shows it. */
internal fun isAddressOf(text: String, domain: String): Boolean {
    val host = text.trim().lowercase()
        .substringAfter("://")
        .substringBefore('/').substringBefore('?').substringBefore('#')
        .removePrefix("www.")
    return host.isNotEmpty() && host == domain.lowercase().removePrefix("www.")
}

/** "Email address" is a username, not a postal address. */
private val EmailAddress = Regex("e-?mail[ _-]?address")

private fun String.notLogin(): Boolean {
    val plain = replace(EmailAddress, "email")
    return NotLogin.any { it in plain }
}

private fun AssistStructure.ViewNode.attr(name: String): String? =
    htmlInfo?.attributes?.firstOrNull { it.first.equals(name, ignoreCase = true) }?.second

private fun AssistStructure.ViewNode.words(): String = buildString {
    append(idEntry.orEmpty())
    append(' ')
    append(hint.orEmpty())
    append(' ')
    append(attr("name").orEmpty())
    append(' ')
    append(attr("id").orEmpty())
    append(' ')
    append(attr("placeholder").orEmpty())
    append(' ')
    append(attr("aria-label").orEmpty())
}.lowercase()

/** Reads a screen's structure into the fields we would fill. */
fun parseForm(structure: AssistStructure): ParsedForm {
    var username: AssistStructure.ViewNode? = null
    var password: AssistStructure.ViewNode? = null
    // Every password box, for the one a change-password form keeps the new password in.
    val passwords = mutableListOf<AssistStructure.ViewNode>()
    var newPassword: AssistStructure.ViewNode? = null
    var otp: AssistStructure.ViewNode? = null
    var domain: String? = null
    var lastText: AssistStructure.ViewNode? = null
    var focused: AssistStructure.ViewNode? = null
    // The page each field is on: a frame from another site inside the page says so on its own node.
    val domains = HashMap<AssistStructure.ViewNode, String>()

    fun visit(node: AssistStructure.ViewNode, inherited: String?) {
        val here = node.webDomain?.takeIf { it.isNotEmpty() } ?: inherited
        if (domain == null) domain = here
        if (node.visibility == View.VISIBLE && node.autofillId != null) {
            // Some apps leave nulls in the list.
            val hints = node.autofillHints.orEmpty().mapNotNull { it?.lowercase() }
            val tag = node.htmlInfo?.tag?.lowercase()
            val htmlType = node.attr("type")?.lowercase()
            val complete = node.attr("autocomplete")?.lowercase().orEmpty()
            // The attribute is a list of tokens, as in "section-login username" or "username webauthn".
            val tokens = complete.split(' ', '\t', '\n').filter { it.isNotEmpty() }
            val variation = node.inputType and InputType.TYPE_MASK_VARIATION
            val klass = node.inputType and InputType.TYPE_MASK_CLASS
            val words = node.words()
            // A search box above a login is not its username, and a Compose field may give no input
            // type at all while its hints say what it is for.
            val isTextInput = (tag == "input" && htmlType !in NotText) ||
                (tag == null && node.autofillType == View.AUTOFILL_TYPE_TEXT && (klass != 0 || hints.isNotEmpty()))
            // A browser's address bar, by its id, by taking addresses, or by showing the page's own.
            val shown = node.autofillValue?.takeIf { it.isText }?.textValue?.toString() ?: node.text?.toString()
            val pageDomain = here ?: domain
            val isAddressBar = tag == null && (
                node.idEntry in AddressBarIds ||
                    (klass == InputType.TYPE_CLASS_TEXT && variation == InputType.TYPE_TEXT_VARIATION_URI) ||
                    (pageDomain != null && shown != null && isAddressOf(shown, pageDomain))
                )
            if (isTextInput && !isAddressBar) {
                if (node.isFocused && focused == null) focused = node
                if (here != null) domains[node] = here
                // A box that says outright it takes a one-time code is one, even when it hides its
                // digits as a PIN box does: the login's password must not land in it.
                val saysOtp = hints.any { "otp" in it } || "one-time-code" in tokens
                val isPassword = !saysOtp && (
                    hints.any { "password" in it } || htmlType == "password" ||
                        "password" in complete ||
                        (klass == InputType.TYPE_CLASS_TEXT && variation in setOf(InputType.TYPE_TEXT_VARIATION_PASSWORD, InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD, InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD)) ||
                        (klass == InputType.TYPE_CLASS_NUMBER && variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD)
                    )
                val isOtp = !isPassword && (saysOtp || OtpWords.any { it in words })
                val isUser = !isPassword && !isOtp && (
                    hints.any { "username" in it || "email" in it || "phone" in it } ||
                        tokens.any { it == "username" || it == "email" || it == "tel" } || htmlType == "email" ||
                        (klass == InputType.TYPE_CLASS_TEXT && variation in setOf(InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS, InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS)) ||
                        (!words.notLogin() && UserWords.any { it in words })
                    )
                if (isPassword) {
                    passwords += node
                    val saysNew = hints.any { "newpassword" in it } || "new-password" in complete || "new" in words
                    if (saysNew && newPassword == null) newPassword = node
                }
                when {
                    isPassword && password == null -> {
                        password = node
                        // The text box just before a password is its username, whatever it calls itself.
                        if (username == null && lastText != null) username = lastText
                    }
                    isOtp && otp == null -> otp = node
                    isUser && username == null -> username = node
                }
                // Not a code box either: a code asked for before the password is not its username.
                if (!isPassword && !isOtp && !words.notLogin()) lastText = node
            }
        }
        for (index in 0 until node.childCount) visit(node.getChildAt(index), here)
    }
    for (index in 0 until structure.windowNodeCount) visit(structure.getWindowNodeAt(index).rootViewNode, null)

    fun AssistStructure.ViewNode?.text(): String? =
        this?.autofillValue?.takeIf { it.isText }?.textValue?.toString()?.takeIf { it.isNotEmpty() }

    // What is saved after a send. Filling still goes to the first box, but on a form with the
    // current password and a new one, the first holds the old password: the new one is in the box
    // that says so, or else the last, where it is typed again to confirm it.
    val saved = newPassword ?: passwords.takeIf { it.size >= 2 }?.last() ?: password
    // The site the fields belong to, not merely the first one named: a login inside another
    // site's frame must be matched against that site.
    val fieldDomain = listOfNotNull(password, username, otp).firstNotNullOfOrNull { domains[it] } ?: domain
    return ParsedForm(
        structure.activityComponent?.packageName.orEmpty(),
        fieldDomain,
        username?.autofillId,
        password?.autofillId,
        otp?.autofillId,
        username.text(),
        saved.text() ?: password.text(),
        passwords.drop(1).mapNotNull { it.autofillId },
        focused?.autofillId,
    )
}

/** This form with [field] taken as its username, for a fill asked for by hand on a box that says nothing. */
fun ParsedForm.withUsernameField(field: AutofillId): ParsedForm =
    ParsedForm(packageName, webDomain, field, password, otp, usernameValue, passwordValue, morePasswords, focused)

/**
 * A login sent over more than one screen of one session, the username first and the password
 * after: the username typed on an earlier screen, when this one has none. [earlier] is newest
 * first, and only a screen of the same app and the same site counts.
 */
fun ParsedForm.withEarlierUsername(earlier: Sequence<ParsedForm>): ParsedForm {
    if (usernameValue != null) return this
    // A screen with a password of its own was a login, or a form, of its own: only a username
    // screen with no password hands its value on.
    val found = earlier.firstOrNull {
        it.usernameValue != null && it.password == null && it.passwordValue == null && it.packageName == packageName && it.webDomain == webDomain
    } ?: return this
    return ParsedForm(packageName, webDomain, username, password, otp, found.usernameValue, passwordValue, morePasswords, focused)
}

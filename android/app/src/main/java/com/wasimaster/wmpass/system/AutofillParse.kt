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
) {
    val ids: Array<AutofillId> get() = listOfNotNull(username, password, otp).toTypedArray()
    val isLogin: Boolean get() = username != null || password != null || otp != null
}

private val UserWords = listOf("user", "email", "e-mail", "login", "account", "phone", "mobile", "identifier")
private val OtpWords = listOf("otp", "one-time", "onetime", "2fa", "two-factor", "totp", "verification", "authcode", "auth_code")
private val NotLogin = listOf("search", "captcha", "comment", "message", "subject", "address", "coupon", "promo")

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
    var otp: AssistStructure.ViewNode? = null
    var domain: String? = null
    var lastText: AssistStructure.ViewNode? = null

    fun visit(node: AssistStructure.ViewNode) {
        if (domain == null) node.webDomain?.takeIf { it.isNotEmpty() }?.let { domain = it }
        if (node.visibility == View.VISIBLE && node.autofillId != null) {
            // Some apps leave nulls in the list.
            val hints = node.autofillHints.orEmpty().mapNotNull { it?.lowercase() }
            val tag = node.htmlInfo?.tag?.lowercase()
            val htmlType = node.attr("type")?.lowercase()
            val complete = node.attr("autocomplete")?.lowercase().orEmpty()
            val variation = node.inputType and InputType.TYPE_MASK_VARIATION
            val klass = node.inputType and InputType.TYPE_MASK_CLASS
            val words = node.words()
            val isTextInput = (tag == "input" && htmlType !in setOf("hidden", "checkbox", "radio", "submit", "button", "file", "image")) ||
                (tag == null && node.autofillType == View.AUTOFILL_TYPE_TEXT && klass != 0)
            if (isTextInput) {
                val isPassword = hints.any { "password" in it } || htmlType == "password" ||
                    "password" in complete ||
                    (klass == InputType.TYPE_CLASS_TEXT && variation in setOf(InputType.TYPE_TEXT_VARIATION_PASSWORD, InputType.TYPE_TEXT_VARIATION_WEB_PASSWORD, InputType.TYPE_TEXT_VARIATION_VISIBLE_PASSWORD)) ||
                    (klass == InputType.TYPE_CLASS_NUMBER && variation == InputType.TYPE_NUMBER_VARIATION_PASSWORD)
                val isOtp = !isPassword && (
                    hints.any { "otp" in it } || complete == "one-time-code" || OtpWords.any { it in words }
                    )
                val isUser = !isPassword && !isOtp && (
                    hints.any { "username" in it || "email" in it || "phone" in it } ||
                        complete in setOf("username", "email", "tel") || htmlType == "email" ||
                        (klass == InputType.TYPE_CLASS_TEXT && variation in setOf(InputType.TYPE_TEXT_VARIATION_EMAIL_ADDRESS, InputType.TYPE_TEXT_VARIATION_WEB_EMAIL_ADDRESS)) ||
                        (NotLogin.none { it in words } && UserWords.any { it in words })
                    )
                when {
                    isPassword && password == null -> {
                        password = node
                        // The text box just before a password is its username, whatever it calls itself.
                        if (username == null && lastText != null) username = lastText
                    }
                    isOtp && otp == null -> otp = node
                    isUser && username == null -> username = node
                }
                if (!isPassword && NotLogin.none { it in words }) lastText = node
            }
        }
        for (index in 0 until node.childCount) visit(node.getChildAt(index))
    }
    for (index in 0 until structure.windowNodeCount) visit(structure.getWindowNodeAt(index).rootViewNode)

    fun AssistStructure.ViewNode?.text(): String? =
        this?.autofillValue?.takeIf { it.isText }?.textValue?.toString()?.takeIf { it.isNotEmpty() }

    return ParsedForm(
        structure.activityComponent?.packageName.orEmpty(),
        domain,
        username?.autofillId,
        password?.autofillId,
        otp?.autofillId,
        username.text(),
        password.text(),
    )
}

package com.wasimaster.wmpass.system

import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Whether each browser on the phone hands its forms to Android's autofill, and so to WM Pass.
 *
 * Since Chrome 135, Chrome fills from its own password manager unless the person turns on
 * **Autofill using another service** in Chrome's settings, and WM Pass is never asked about a
 * page. Chrome says which way it is set through a content provider, and opens that setting for
 * an intent. Browsers built on Chromium carry the same code under their own package name, so each
 * one installed is asked the same way; one that does not answer either predates the setting or
 * left it out, and is not listed. Firefox and the browsers built on it use Android's autofill
 * already and need nothing.
 */
object BrowserAutofill {
    class Browser(val pkg: String, val label: String, val usesAutofill: Boolean)

    /** Chromium browsers that may carry the setting: Chrome's channels, then the rest by name. */
    val Chromium = listOf(
        "com.android.chrome",
        "com.chrome.beta",
        "com.chrome.dev",
        "com.chrome.canary",
        "com.brave.browser",
        "com.brave.browser_beta",
        "com.brave.browser_nightly",
        "com.microsoft.emmx",
        "com.vivaldi.browser",
        "com.opera.browser",
        "com.opera.gx",
        "com.sec.android.app.sbrowser",
        "com.kiwibrowser.browser",
        "org.cromite.cromite",
        "org.chromium.chrome",
        "com.duckduckgo.mobile.android",
        "com.yandex.browser",
    )

    private const val PROVIDER = ".AutofillThirdPartyModeContentProvider"
    private const val COLUMN = "autofill_third_party_state"
    private const val PATH = "autofill_third_party_mode"

    /** The browsers here that answer, each with which way it is set. */
    suspend fun browsers(context: Context): List<Browser> = withContext(Dispatchers.IO) {
        Chromium.mapNotNull { pkg ->
            val label = runCatching { Trust.labelOf(context, pkg).takeIf { context.packageManager.getPackageInfo(pkg, 0) != null } }.getOrNull()
                ?: return@mapNotNull null
            val state = runCatching {
                val uri = Uri.Builder().scheme(ContentResolver.SCHEME_CONTENT).authority(pkg + PROVIDER).path(PATH).build()
                context.contentResolver.query(uri, arrayOf(COLUMN), null, null, null)?.use { cursor ->
                    if (cursor.moveToFirst()) cursor.getInt(cursor.getColumnIndexOrThrow(COLUMN)) else null
                }
            }.getOrNull() ?: return@mapNotNull null
            Browser(pkg, label, state == 1)
        }
    }

    /** Opens the browser's own page for the setting. False when it has none to open. */
    fun openSetting(context: Context, pkg: String): Boolean = runCatching {
        context.startActivity(
            Intent(Intent.ACTION_APPLICATION_PREFERENCES)
                .addCategory(Intent.CATEGORY_DEFAULT)
                .addCategory(Intent.CATEGORY_APP_BROWSER)
                .addCategory(Intent.CATEGORY_PREFERENCE)
                .setPackage(pkg)
                .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
        )
        true
    }.getOrDefault(false)
}

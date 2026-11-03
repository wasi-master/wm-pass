package com.wasimaster.wmpass.system

import android.annotation.SuppressLint
import android.content.Context
import android.net.Uri
import android.webkit.CookieManager
import android.webkit.WebResourceRequest
import android.webkit.WebStorage
import android.webkit.WebView
import android.webkit.WebViewClient
import com.wasimaster.wmpass.core.Engine
import kotlin.coroutines.resume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * Username pages that only show their answer once their own scripts have run. Each loads in a
 * WebView that is never shown, has no bridge to the app, and may not leave the site's domain.
 * Cookies, storage and cache are wiped once it is done. The engine reads the page's text.
 */
object PageCheck {
    private const val WAIT_MS = 15_000L
    private const val READ = "document.body ? document.body.innerText.slice(0, 20000) : ''"

    /** "free", "taken" or "unknown". */
    suspend fun check(context: Context, site: String, name: String): String {
        val page = Engine.net("usernames.page", online = false) {
            put("site", site)
            put("name", name)
        }.obj
        val url = page["url"]!!.jsonPrimitive.content
        val domain = page["domain"]!!.jsonPrimitive.content
        return withContext(Dispatchers.Main) {
            val view = open(context, domain)
            try {
                view.loadUrl(url)
                withTimeoutOrNull(WAIT_MS) {
                    var answer = "waiting"
                    while (answer == "waiting") {
                        delay(500)
                        val text = view.read()
                        answer = Engine.net("usernames.read_page", online = false) {
                            put("site", site)
                            put("text", text)
                        }.obj["answer"]!!.jsonPrimitive.content
                    }
                    answer
                } ?: "unknown"
            } finally {
                wipe(view)
            }
        }
    }

    @SuppressLint("SetJavaScriptEnabled")
    private fun open(context: Context, domain: String): WebView {
        val view = WebView(context.applicationContext)
        view.settings.apply {
            javaScriptEnabled = true
            domStorageEnabled = true
            allowFileAccess = false
            allowContentAccess = false
            javaScriptCanOpenWindowsAutomatically = false
            setSupportMultipleWindows(false)
        }
        view.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean =
                !mayVisit(request.url, domain)
        }
        // Never attached to a window, so it needs a size of its own to lay the page out.
        view.layout(0, 0, 1080, 2340)
        return view
    }

    /** The same rule as the engine's: https on the site's own domain, and nothing else. */
    private fun mayVisit(url: Uri, domain: String): Boolean {
        if (url.toString() == "about:blank") return true
        val host = url.host?.lowercase() ?: return false
        return url.scheme == "https" && url.userInfo == null && (host == domain || host.endsWith(".$domain"))
    }

    private suspend fun WebView.read(): String = suspendCancellableCoroutine { done ->
        evaluateJavascript(READ) { result ->
            done.resume(runCatching { Json.parseToJsonElement(result).jsonPrimitive.contentOrNull }.getOrNull().orEmpty())
        }
    }

    private fun wipe(view: WebView) {
        view.stopLoading()
        view.clearHistory()
        view.clearCache(true)
        view.destroy()
        CookieManager.getInstance().removeAllCookies(null)
        WebStorage.getInstance().deleteAllData()
    }
}

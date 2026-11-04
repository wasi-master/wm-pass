package com.wasimaster.wmpass.system

import android.content.Context
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * Where a site lets you change your password. Apple's list of known pages comes first; any
 * other site gets its `/.well-known/change-password` address, which sites that follow the W3C
 * proposal send on to the right page and the rest answer with their home or a missing page.
 */
object ChangePassword {
    @Volatile
    private var known: Map<String, String>? = null

    private fun known(context: Context): Map<String, String> = known ?: runCatching {
        val text = context.assets.open("change-password-urls.json").bufferedReader().use { it.readText() }
        Json.parseToJsonElement(text).jsonObject.mapValues { it.value.jsonPrimitive.content }
    }.getOrDefault(emptyMap()).also { known = it }

    /** The page for a saved address, or null when it names no website. */
    fun url(context: Context, site: String?): String? {
        if (site.isNullOrBlank() || Related.app(site) != null) return null
        val host = site.trim().lowercase().substringAfter("://").substringBefore('/').substringBefore(':').removePrefix("www.")
        if (host.isEmpty() || '.' !in host) return null
        val domain = Related.registrable(host) ?: host
        val list = known(context)
        return list[host] ?: list[domain] ?: "https://$domain/.well-known/change-password"
    }
}

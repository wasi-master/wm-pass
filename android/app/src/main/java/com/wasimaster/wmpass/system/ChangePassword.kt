package com.wasimaster.wmpass.system

import android.content.Context
import com.wasimaster.wmpass.core.Engine
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * Where a site lets you change your password, and only for a site that has such a page.
 *
 * Apple's list of 805 known pages answers first, with no network. Any other site is asked once
 * whether its `/.well-known/change-password` address leads anywhere, the way the W3C proposal
 * says, after checking that it does not answer every address alike. Most sites have no such
 * page, so most items show no link. The answer is kept sealed on this phone for a month, a no
 * for two weeks, so the site is not asked on every visit.
 */
object ChangePassword {
    @Volatile
    private var known: Map<String, String>? = null

    /** Answers this run, a blank string for a site with no page. */
    private val asked = ConcurrentHashMap<String, String>()

    private const val FOUND_MS = 30L * 86_400_000
    private const val NONE_MS = 14L * 86_400_000

    private fun known(context: Context): Map<String, String> = known ?: runCatching {
        val text = context.assets.open("change-password-urls.json").bufferedReader().use { it.readText() }
        Json.parseToJsonElement(text).jsonObject.mapValues { it.value.jsonPrimitive.content }
    }.getOrDefault(emptyMap()).also { known = it }

    /** The host a saved address names, or null for an app or anything that is not a website. */
    private fun hostOf(site: String?): String? {
        if (site.isNullOrBlank() || Related.app(site) != null) return null
        val host = site.trim().lowercase().substringAfter("://").substringBefore('/').substringBefore('?').substringBefore(':').removePrefix("www.")
        return host.takeIf { it.isNotEmpty() && '.' in it }
    }

    /** The page from the bundled list alone: no network, no wait. */
    fun listed(context: Context, site: String?): String? {
        val host = hostOf(site) ?: return null
        val list = known(context)
        return list[host] ?: list[Related.registrable(host) ?: host]
    }

    /** The answer kept from an earlier visit, without asking again. */
    fun remembered(site: String?): String? = hostOf(site)?.let { asked[it] }?.ifEmpty { null }

    /**
     * The page, from the list, an earlier answer, or by asking the site when the network may be
     * used. Null for a site with no page, or one that could not be asked right now.
     */
    suspend fun find(context: Context, site: String?): String? {
        listed(context, site)?.let { return it }
        val host = hostOf(site) ?: return null
        asked[host]?.let { return it.ifEmpty { null } }
        val name = "cpw-" + MessageDigest.getInstance("SHA-256").digest(host.toByteArray()).joinToString("") { "%02x".format(it) }
        return withContext(Dispatchers.IO) {
            Sealed.read(context, name)?.decodeToString()?.let { kept ->
                val at = kept.substringBefore('\n').toLongOrNull() ?: 0L
                val url = kept.substringAfter('\n', "")
                if (System.currentTimeMillis() - at < if (url.isEmpty()) NONE_MS else FOUND_MS) {
                    asked[host] = url
                    return@withContext url.ifEmpty { null }
                }
            }
            if (!NetAccess.allowed(context)) return@withContext null
            // Asked for the site's own domain: a login page on a subdomain changes nothing about where accounts are run.
            val domain = Related.registrable(host) ?: host
            val url = runCatching {
                Engine.net("site.change_password", online = true) { put("host", domain) }.obj["url"]?.jsonPrimitive?.contentOrNull
            }.getOrElse { return@withContext null }.orEmpty()
            asked[host] = url
            Sealed.write(context, name, "${System.currentTimeMillis()}\n$url".encodeToByteArray())
            url.ifEmpty { null }
        }
    }

    /** Forgets every answer, as when the owner clears what WM Pass fetched. */
    fun forget(context: Context) {
        asked.clear()
        java.io.File(context.filesDir, "sealed").listFiles { file -> file.name.startsWith("cpw-") }?.forEach { it.delete() }
    }
}

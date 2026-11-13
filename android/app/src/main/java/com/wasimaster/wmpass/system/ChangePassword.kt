package com.wasimaster.wmpass.system

import android.content.Context
import com.wasimaster.wmpass.core.Engine
import java.security.MessageDigest
import java.util.concurrent.ConcurrentHashMap
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * Where a site lets you change your password, and only for a site that has such a page.
 *
 * The core answers first, with no network, so every app agrees: Apple's list of known pages.
 * For a website off the list it names the domain to ask, and that site is asked once whether
 * its `/.well-known/change-password` address leads anywhere, the way the W3C proposal says,
 * after checking that it does not answer every address alike. Most sites have no such page, so
 * most items show no link. The answer is kept sealed on this phone for a month, a no for two
 * weeks, so the site is not asked on every visit.
 */
object ChangePassword {
    /** Answers this run by saved address, a blank string for a site with no page. */
    private val answers = ConcurrentHashMap<String, String>()

    /** Answers this run by domain asked, so two addresses on one site ask it once. */
    private val asked = ConcurrentHashMap<String, String>()

    private const val FOUND_MS = 30L * 86_400_000
    private const val NONE_MS = 14L * 86_400_000

    /** The answer kept from earlier this run, without asking again. */
    fun remembered(site: String?): String? = site?.let { answers[it] }?.ifEmpty { null }

    /**
     * The page, from the list, an earlier answer, or by asking the site when the network may be
     * used. Null for a site with no page, or one that could not be asked right now.
     */
    suspend fun find(context: Context, site: String?): String? {
        if (site.isNullOrBlank()) return null
        answers[site]?.let { return it.ifEmpty { null } }
        val core = runCatching { Engine.call("site.change_password", args = { put("site", site) }).obj }.getOrNull() ?: return null
        core["url"]?.jsonPrimitive?.contentOrNull?.let { url ->
            answers[site] = url
            return url
        }
        val domain = core["ask"]?.jsonPrimitive?.contentOrNull
        if (domain == null) {
            answers[site] = ""
            return null
        }
        val url = ask(context, domain) ?: return null
        answers[site] = url
        return url.ifEmpty { null }
    }

    /** What [domain] answered, a blank string for no page, or null when it could not be asked. */
    private suspend fun ask(context: Context, domain: String): String? {
        asked[domain]?.let { return it }
        val name = "cpw-" + MessageDigest.getInstance("SHA-256").digest(domain.toByteArray()).joinToString("") { "%02x".format(it) }
        return withContext(Dispatchers.IO) {
            Sealed.read(context, name)?.decodeToString()?.let { kept ->
                val at = kept.substringBefore('\n').toLongOrNull() ?: 0L
                val url = kept.substringAfter('\n', "")
                if (System.currentTimeMillis() - at < if (url.isEmpty()) NONE_MS else FOUND_MS) {
                    asked[domain] = url
                    return@withContext url
                }
            }
            if (!NetAccess.allowed(context)) return@withContext null
            val url = runCatching {
                Engine.net("site.probe_change_password", online = true) { put("host", domain) }.obj["url"]?.jsonPrimitive?.contentOrNull
            }.getOrElse { return@withContext null }.orEmpty()
            asked[domain] = url
            // Only a note to save asking again: a keystore that will not seal it costs a later probe, not this answer.
            runCatching { Sealed.write(context, name, "${System.currentTimeMillis()}\n$url".encodeToByteArray()) }
            url
        }
    }

    /** Forgets every answer, as when the owner clears what WM Pass fetched. */
    fun forget(context: Context) {
        answers.clear()
        asked.clear()
        java.io.File(context.filesDir, "sealed").listFiles { file -> file.name.startsWith("cpw-") }?.forEach { it.delete() }
    }
}

package com.wasimaster.wmpass.system

import android.content.Context
import android.content.Intent
import com.wasimaster.wmpass.ui.parts.Brands
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/**
 * What goes with a login's name, websites and apps, so each can suggest the others: Facebook
 * brings facebook.com, messenger.com and the Facebook, Lite and Messenger apps that are
 * installed. Families of sites come from `data/related`; apps are matched to them by the company
 * in their package name and by their own name. Everything is worked out on the phone.
 */
object Related {
    class App(val pkg: String, val label: String)

    class Found(val sites: List<String>, val apps: List<App>, val name: String?)

    @Volatile private var families: List<Set<String>>? = null

    @Volatile private var launchable: List<App>? = null

    private fun families(context: Context): List<Set<String>> = families ?: synchronized(this) {
        families ?: buildList {
            runCatching {
                context.assets.open("accounts.txt").bufferedReader().useLines { lines ->
                    lines.map { it.trim() }.filter { it.isNotEmpty() && !it.startsWith("#") }.forEach { add(it.split(' ').filter { d -> d.isNotEmpty() }.toSet()) }
                }
            }
            runCatching {
                val groups = Json.parseToJsonElement(context.assets.open("shared-credentials.json").bufferedReader().use { it.readText() }).jsonArray
                groups.forEach { group ->
                    val fields = group.jsonObject
                    val domains = listOf("shared", "from", "to").flatMap { key -> fields[key]?.jsonArray?.mapNotNull { it.jsonPrimitive.contentOrNull }.orEmpty() }
                    if (domains.size > 1) add(domains.toSet())
                }
            }
        }.also { families = it }
    }

    /** The apps a launcher shows, read once. */
    suspend fun installed(context: Context): List<App> = launchable ?: withContext(Dispatchers.IO) {
        val pm = context.packageManager
        val launcher = Intent(Intent.ACTION_MAIN).addCategory(Intent.CATEGORY_LAUNCHER)
        pm.queryIntentActivities(launcher, 0)
            .map { it.activityInfo.applicationInfo }
            .distinctBy { it.packageName }
            .filter { it.packageName != context.packageName }
            .map { App(it.packageName, it.loadLabel(pm).toString()) }
            .sortedBy { it.label.lowercase() }
            .also { launchable = it }
    }

    /** The part of an address a sign-in belongs to: `m.facebook.com/login` is facebook.com. */
    fun registrable(address: String): String? {
        val host = address.trim().lowercase().substringAfter("://").substringBefore('/').substringBefore(':').removePrefix("www.")
        val labels = host.split('.').filter { it.isNotEmpty() }
        if (labels.size < 2) return null
        // co.uk, com.au and the like keep a third label.
        val keep = if (labels.size >= 3 && labels[labels.size - 2] in setOf("co", "com", "org", "net", "ac", "gov", "ne", "or")) 3 else 2
        return labels.takeLast(keep).joinToString(".")
    }

    private fun squeeze(text: String) = text.lowercase().filter { it.isLetterOrDigit() }

    /**
     * Suggestions for what is not there yet. [sites] and [apps] are what the login already has;
     * nothing already added comes back, and an app or site is suggested only when it is installed
     * or in a known family.
     */
    suspend fun find(context: Context, title: String, sites: List<String>, apps: List<String>): Found = withContext(Dispatchers.Default) {
        val have = sites.mapNotNull(::registrable).toSet()
        val seeds = LinkedHashSet<String>(have)
        val brandNames = LinkedHashSet<String>()
        Brands.find(context, null, title)?.let { brand ->
            seeds += brand.domains.mapNotNull(::registrable)
            brandNames += brand.name
        }
        apps.forEach { pkg ->
            // com.facebook.katana belongs to facebook.com.
            val parts = pkg.split('.')
            if (parts.size >= 2 && parts[0] in setOf("com", "org", "net", "io")) seeds += "${parts[1]}.${parts[0]}"
        }
        have.forEach { domain -> Brands.find(context, domain, "")?.let { brandNames += it.name } }
        val family = LinkedHashSet<String>(seeds)
        families(context).filter { group -> group.any { it in seeds } }.forEach { family += it }
        val siteOffers = family.filter { it !in have && registrable(it) == it }.take(6)
        // Each site's own word, and each brand's name, to know its apps by.
        val words = (family.map { it.substringBefore('.') } + brandNames.map(::squeeze)).filter { it.length >= 3 }.toSet()
        val installed = installed(context)
        val appOffers = installed.filter { app ->
            if (app.pkg in apps) return@filter false
            val parts = app.pkg.split('.')
            val company = parts.getOrNull(1).orEmpty()
            val label = squeeze(app.label)
            company in words || words.any { label.startsWith(it) }
        }.sortedByDescending { app -> words.any { squeeze(app.label).startsWith(it) } }.take(6)
        val name = if (title.isNotBlank()) {
            null
        } else {
            brandNames.firstOrNull()
                ?: apps.firstNotNullOfOrNull { pkg -> installed.firstOrNull { it.pkg == pkg }?.label }
                ?: have.firstOrNull()?.substringBefore('.')?.replaceFirstChar { it.uppercase() }
        }
        Found(siteOffers, appOffers.map { App(it.pkg, it.label) }, name)
    }
}

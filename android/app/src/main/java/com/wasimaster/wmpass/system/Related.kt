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
    /** First labels of package names as people write them: a top-level domain, reversed. */
    private val PackageRoots = setOf(
        "com", "org", "net", "io", "app", "dev", "me", "co", "de", "uk", "bd", "in", "us", "tv", "info", "ru", "fr",
        "jp", "cn", "br", "edu", "gov", "nl", "it", "es", "ca", "au", "kr", "tw", "pl", "ch", "se", "at", "be",
    )

    /** Last labels that say website, for the guess below. */
    private val CommonTlds = setOf("com", "org", "net", "io", "app", "dev", "me", "co", "info", "xyz", "tv", "gg", "ai", "uk", "bd", "in", "us", "de")

    /**
     * The Android app an address names, in the forms exports write one: `androidapp://package`,
     * `android://<digest>@package/`, or a bare package name such as `com.bkash.customerapp` that
     * is installed here or starts the way package names do. The engine reads the same forms.
     */
    fun app(address: String, context: android.content.Context? = null): String? {
        val text = address.trim()
        val lower = text.lowercase()
        if (lower.startsWith("androidapp://")) return text.substring(13).trimEnd('/').ifEmpty { null }
        if (lower.startsWith("android://")) return text.substring(10).trimEnd('/').substringAfterLast('@').ifEmpty { null }
        if (text.isEmpty() || text.any { it in "/:@?#" || it.isWhitespace() }) return null
        val labels = text.split('.')
        if (labels.size < 2 || labels.any { label -> label.isEmpty() || !label.all { it.isLetterOrDigit() || it == '_' } }) return null
        if (context != null && runCatching { context.packageManager.getPackageInfo(text, 0) }.isSuccess) return text
        return text.takeIf { labels.first().lowercase() in PackageRoots && labels.last().lowercase() !in CommonTlds }
    }

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
        // Other names before the same name under other endings: youtube.com before google.de.
        val seedWords = seeds.map { it.substringBefore('.') }.toSet()
        val siteOffers = family
            .sortedBy { it !in seeds && it.substringBefore('.') in seedWords }
            .filter { it !in have && registrable(it) == it }
            .take(6)
        // Each site's own word, and each brand's name, to know its apps by.
        val words = (family.map { it.substringBefore('.') } + brandNames.map(::squeeze)).filter { it.length >= 3 }.toSet()
        val installed = installed(context)
        // A short word must be the name's whole first word: "one" is not OneDrive's.
        val named = { app: App ->
            val label = squeeze(app.label)
            val first = squeeze(app.label.trim().substringBefore(' '))
            words.any { label.startsWith(it) && (it.length >= 5 || first == it) }
        }
        val appOffers = installed.filter { app ->
            if (app.pkg in apps) return@filter false
            val parts = app.pkg.split('.')
            val company = parts.getOrNull(1).orEmpty()
            company in words || named(app)
        }.sortedByDescending(named).take(6)
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

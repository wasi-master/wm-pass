package com.wasimaster.wmpass.ui.parts

import android.content.Context
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/**
 * SVG lets an arc's two flags run straight into what follows, as in `a1 1 0 00-1 2`, and minified
 * icon sets do. The path parser reads `00` as one number, which shifts every argument after it
 * and draws wreckage, so the flags are spaced out before it sees them.
 */
internal fun spaceArcFlags(path: String): String {
    val out = StringBuilder(path.length + 32)
    var i = 0
    fun skipGaps() {
        while (i < path.length && (path[i] == ' ' || path[i] == ',' || path[i] == '\n' || path[i] == '\t')) i++
    }
    fun number(): String {
        skipGaps()
        val start = i
        if (i < path.length && (path[i] == '-' || path[i] == '+')) i++
        var dot = false
        while (i < path.length) {
            val c = path[i]
            if (c.isDigit()) {
                i++
            } else if (c == '.' && !dot) {
                dot = true
                i++
            } else if ((c == 'e' || c == 'E') && i + 1 < path.length) {
                i++
                if (path[i] == '-' || path[i] == '+') i++
            } else {
                break
            }
        }
        return path.substring(start, i)
    }
    while (i < path.length) {
        val c = path[i]
        if (c != 'a' && c != 'A') {
            out.append(c)
            i++
            continue
        }
        out.append(c)
        i++
        while (true) {
            skipGaps()
            if (i >= path.length || !(path[i].isDigit() || path[i] == '.' || path[i] == '-' || path[i] == '+')) break
            val rx = number()
            val ry = number()
            val turn = number()
            skipGaps()
            val large = path[i++]
            skipGaps()
            val sweep = path[i++]
            val x = number()
            val y = number()
            out.append(rx).append(' ').append(ry).append(' ').append(turn).append(' ')
                .append(large).append(' ').append(sweep).append(' ').append(x).append(' ').append(y).append(' ')
        }
    }
    return out.toString()
}

/**
 * A site's mark and colour, from `data/brands`. Bundled, so showing one asks nobody anything.
 * The drawing is read only when something first shows it.
 */
class Brand(val slug: String, val name: String, val color: Color, val otherNames: List<String>, val domains: List<String>, private val context: Context) {
    val glyph: ImageVector by lazy {
        ImageVector.Builder(slug, 24.dp, 24.dp, 24f, 24f)
            .addPath(pathData = addPathNodes(spaceArcFlags(Brands.drawing(context, slug))), fill = SolidColor(Color.White))
            .build()
    }
}

object Brands {
    @Volatile private var loaded = false
    private val byDomain = HashMap<String, Brand>()
    private val byName = HashMap<String, Brand>()
    private val bySlug = LinkedHashMap<String, Brand>()

    /** The last few files of drawings. Most vaults use a small part of the set. */
    private val chunks = object : LinkedHashMap<Char, Map<String, String>>(8, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<Char, Map<String, String>>?) = size > 6
    }

    private fun load(context: Context) {
        if (loaded) return
        synchronized(this) {
            if (loaded) return
            runCatching {
                context.assets.open("brands.tsv").bufferedReader().useLines { lines ->
                    lines.forEach { line ->
                        val parts = line.split('\t')
                        if (parts.size >= 4) {
                            val others = parts.getOrNull(4).orEmpty().split(',').filter { it.isNotEmpty() }
                            val domains = parts[3].split(',').filter { it.isNotEmpty() }
                            val brand = Brand(parts[0], parts[1], Color(0xFF000000 or parts[2].toLong(16)), others, domains, context.applicationContext)
                            bySlug[brand.slug] = brand
                            // The well-known sites come first in the file and keep their claim on a name.
                            byName.putIfAbsent(normal(brand.name), brand)
                            byName.putIfAbsent(brand.slug, brand)
                            others.forEach { byName.putIfAbsent(normal(it), brand) }
                            domains.forEach { byDomain.putIfAbsent(it, brand) }
                        }
                    }
                }
            }
            loaded = true
        }
    }

    /** Reads the list ahead of need, off the main thread. */
    fun warm(context: Context) = load(context)

    internal fun drawing(context: Context, slug: String): String = synchronized(chunks) {
        val letter = slug.first()
        val chunk = chunks.getOrPut(letter) {
            context.assets.open("brand-paths/$letter.tsv").bufferedReader().useLines { lines ->
                lines.associate { line -> line.substringBefore('\t') to line.substringAfter('\t') }
            }
        }
        chunk[slug].orEmpty()
    }

    private fun normal(text: String) = text.lowercase().filter { it.isLetterOrDigit() }

    fun all(context: Context): List<Brand> {
        load(context)
        return bySlug.values.toList()
    }

    fun bySlug(context: Context, slug: String): Brand? {
        load(context)
        return bySlug[slug]
    }

    /**
     * By the site if there is one, then by the name: an authenticator often has only a name.
     * Failing both, a site's own name often is the brand, as with amazon.co.uk or spotify.se.
     */
    fun find(context: Context, site: String?, title: String): Brand? {
        load(context)
        val whole = site?.lowercase()?.removePrefix("www.")
        var host = whole
        while (!host.isNullOrEmpty() && '.' in host) {
            byDomain[host]?.let { return it }
            host = host.substringAfter('.')
        }
        byName[normal(title)]?.let { return it }
        val labels = whole?.split('.')?.dropLast(1).orEmpty()
        val label = labels.lastOrNull { it.length > 3 } ?: labels.lastOrNull().takeIf { labels.size == 1 }
        return label?.let { byName[normal(it)] }
    }
}

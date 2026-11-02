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

/** A site's mark and colour, from `data/brands`. Bundled, so showing one asks nobody anything. */
class Brand(val slug: String, val name: String, val color: Color, private val path: String) {
    val glyph: ImageVector by lazy {
        ImageVector.Builder(slug, 24.dp, 24.dp, 24f, 24f)
            .addPath(pathData = addPathNodes(spaceArcFlags(path)), fill = SolidColor(Color.White))
            .build()
    }
}

object Brands {
    @Volatile private var loaded = false
    private val byDomain = HashMap<String, Brand>()
    private val byName = HashMap<String, Brand>()
    private val bySlug = LinkedHashMap<String, Brand>()

    private fun load(context: Context) {
        if (loaded) return
        synchronized(this) {
            if (loaded) return
            runCatching {
                context.assets.open("brands.tsv").bufferedReader().useLines { lines ->
                    lines.forEach { line ->
                        val parts = line.split('\t')
                        if (parts.size == 5) {
                            val hex = parts[2].toLong(16)
                            val brand = Brand(parts[0], parts[1], Color(0xFF000000 or hex), parts[4])
                            bySlug[brand.slug] = brand
                            byName[normal(brand.name)] = brand
                            parts[3].split(',').forEach { byDomain[it] = brand }
                        }
                    }
                }
            }
            loaded = true
        }
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

    /** By the site if there is one, then by the name: an authenticator often has only a name. */
    fun find(context: Context, site: String?, title: String): Brand? {
        load(context)
        var host = site?.lowercase()?.removePrefix("www.")
        while (!host.isNullOrEmpty() && '.' in host) {
            byDomain[host]?.let { return it }
            host = host.substringAfter('.')
        }
        return byName[normal(title)]
    }
}

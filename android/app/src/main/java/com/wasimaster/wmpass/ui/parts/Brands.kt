package com.wasimaster.wmpass.ui.parts

import android.content.Context
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

/** A site's mark and colour, from `data/brands`. Bundled, so showing one asks nobody anything. */
class Brand(val slug: String, val name: String, val color: Color, private val path: String) {
    val glyph: ImageVector by lazy {
        ImageVector.Builder(slug, 24.dp, 24.dp, 24f, 24f)
            .addPath(pathData = addPathNodes(path), fill = SolidColor(Color.White))
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

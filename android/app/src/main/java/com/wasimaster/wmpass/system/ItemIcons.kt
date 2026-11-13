package com.wasimaster.wmpass.system

import android.content.Context
import android.graphics.BitmapFactory
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.core.graphics.drawable.toBitmap
import com.wasimaster.wmpass.core.Engine
import java.security.MessageDigest
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withPermit
import kotlinx.coroutines.withContext
import kotlinx.serialization.json.put

/**
 * Pictures for items beyond the bundled brand marks: the icon of the app an item is for, when
 * that app is installed, and the website's own icon, when the owner adds website icons to the
 * order in Settings, Icons. Website icons come through the engine, so the network switch and
 * the activity log cover them; they are kept sealed on this phone under a hash of the host, and
 * never fetched for addresses on the local network.
 */
object ItemIcons {
    /** What a missing icon is remembered as, and for how long before asking again. */
    private const val MISS: Byte = 0
    private const val HIT: Byte = 1
    private const val RETRY_MS = 14L * 86_400_000

    private val memory = object : LinkedHashMap<String, ImageBitmap?>(64, 0.75f, true) {
        override fun removeEldestEntry(eldest: MutableMap.MutableEntry<String, ImageBitmap?>?) = size > 300
    }
    private val fetching = Semaphore(4)

    /** A picture already in memory, so a row that comes back draws it at once. */
    fun remembered(key: String): ImageBitmap? = synchronized(memory) { memory[key] }

    fun appKey(pkg: String) = "app:$pkg"

    fun siteKey(host: String) = "site:$host"

    /** The launcher icon of an installed app, or null when it is not installed. */
    suspend fun app(context: Context, pkg: String): ImageBitmap? {
        val key = appKey(pkg)
        synchronized(memory) { if (memory.containsKey(key)) return memory[key] }
        val picture = withContext(Dispatchers.IO) {
            runCatching { context.packageManager.getApplicationIcon(pkg).toBitmap(144, 144).asImageBitmap() }.getOrNull()
        }
        synchronized(memory) { memory[key] = picture }
        return picture
    }

    /** A website's icon, from the sealed cache or, when allowed, fetched once. */
    suspend fun site(context: Context, host: String, from: String): ImageBitmap? {
        val key = siteKey(host)
        synchronized(memory) { if (memory.containsKey(key)) return memory[key] }
        val name = "icon-" + MessageDigest.getInstance("SHA-256").digest(host.toByteArray()).joinToString("") { "%02x".format(it) }
        val picture = withContext(Dispatchers.IO) {
            val kept = Sealed.read(context, name)
            when {
                kept != null && kept.isNotEmpty() && kept[0] == HIT -> decode(kept.copyOfRange(1, kept.size))
                kept != null && kept.size == 9 && kept[0] == MISS &&
                    System.currentTimeMillis() - java.nio.ByteBuffer.wrap(kept, 1, 8).long < RETRY_MS -> null
                !NetAccess.allowed(context) -> return@withContext null
                else -> fetching.withPermit {
                    val bytes = runCatching {
                        Engine.net("icon.fetch", online = true) {
                            put("host", host)
                            put("source", from)
                        }.blobs.firstOrNull()
                    }.getOrNull()
                    val image = bytes?.let(::decode)
                    // The cache only saves fetching again: a keystore or disk that refuses it must
                    // not take the list down with it, since every row's tile comes through here.
                    runCatching {
                        if (image != null) {
                            Sealed.write(context, name, byteArrayOf(HIT) + bytes)
                        } else {
                            Sealed.write(context, name, java.nio.ByteBuffer.allocate(9).put(MISS).putLong(System.currentTimeMillis()).array())
                        }
                    }
                    image
                }
            }
        }
        synchronized(memory) { memory[key] = picture }
        return picture
    }

    /** Forgets every website icon, in memory and on disk, as when the owner turns them off. */
    fun forgetSites(context: Context) {
        synchronized(memory) { memory.keys.removeAll { it.startsWith("site:") } }
        java.io.File(context.filesDir, "sealed").listFiles { file -> file.name.startsWith("icon-") }?.forEach { it.delete() }
    }

    private fun decode(bytes: ByteArray): ImageBitmap? =
        runCatching { BitmapFactory.decodeByteArray(bytes, 0, bytes.size)?.asImageBitmap() }.getOrNull()
}

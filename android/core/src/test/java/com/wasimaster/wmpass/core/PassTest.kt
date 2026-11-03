package com.wasimaster.wmpass.core

import kotlinx.coroutines.coroutineScope
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner
import org.robolectric.RuntimeEnvironment

/** The door and the vault, end to end through the real Rust library. */
@RunWith(RobolectricTestRunner::class)
class PassTest {
    private val password get() = "correct horse".toByteArray()

    @Test
    fun `a vault survives locking, and the folder holds nothing readable`() = runBlocking {
        val context = RuntimeEnvironment.getApplication()
        Pass.init(context)
        assertEquals(LockState.NoVault, Pass.state.value)
        // As the door does it: in a scope that goes away as soon as the vault opens.
        coroutineScope {
            val door = launch { Pass.create("Personal", password) }
            Pass.state.first { it is LockState.Unlocked }
            door.cancel()
        }
        assertTrue(Pass.state.value is LockState.Unlocked)
        val made = java.io.File(context.filesDir, "vaults").listFiles()!!
        assertEquals("the vault and both decoys are on disk before anything else saves", 3, made.size)
        assertEquals("and already carry the same time", 1, made.map { it.lastModified() }.distinct().size)

        val id = Pass.change("entry.create") {
            put("kind", "login")
            put("title", "Mail")
        }.obj["id"]!!.jsonPrimitive.content
        Pass.change("entry.set_secret", secret = "s3cret-value".toByteArray()) {
            put("id", id)
            put("field", "password")
        }
        assertEquals(1, Pass.entries.value.size)
        assertTrue(Pass.entries.value.single().hasPassword)
        Pass.lock()
        assertEquals(LockState.Locked, Pass.state.value)
        assertTrue(Pass.entries.value.isEmpty())

        val files = java.io.File(context.filesDir, "vaults").listFiles()!!
        assertEquals("the real file and two chaff files", 3, files.size)
        // The vault has grown past the static decoy; its partner has kept pace with it.
        assertTrue("every size but the static one shared", files.map { it.length() }.distinct().size <= 2)
        assertTrue("the largest size is on two files", files.count { it.length() == files.maxOf { f -> f.length() } } >= 2)
        assertEquals("every file carries the same time", 1, files.map { it.lastModified() }.distinct().size)
        for (file in files) {
            assertFalse(String(file.readBytes(), Charsets.ISO_8859_1).contains("s3cret-value"))
        }

        assertTrue(Pass.unlock("wrong".toByteArray()) is UnlockResult.Wrong)
        assertEquals(UnlockResult.Opened, Pass.unlock(password))
        assertEquals(1, Pass.entries.value.size)
        val secret = Pass.reveal(id, "password")
        secret.consume { assertArrayEquals("s3cret-value".toByteArray(), it) }
        Pass.lock()
    }
}

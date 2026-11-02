package com.wasimaster.wmpass.system

import android.content.Context
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Small files only this app on this phone can read, under a key in the phone's secure hardware.
 *
 * This is for what has to work while the vault is locked, such as a scheduled backup's WebDAV
 * password. It needs no fingerprint, so it does not stand against someone who has taken over the
 * app's process. Nothing kept here can open a vault.
 */
object Sealed {
    private const val STORE = "AndroidKeyStore"
    private const val ALIAS = "wmpass.local"
    private const val IV = 12

    private fun key(): SecretKey {
        val store = KeyStore.getInstance(STORE).apply { load(null) }
        (store.getKey(ALIAS, null) as? SecretKey)?.let { return it }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, STORE)
        generator.init(
            KeyGenParameterSpec.Builder(ALIAS, KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT)
                .setKeySize(256)
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                .build(),
        )
        return generator.generateKey()
    }

    private fun file(context: Context, name: String) = File(File(context.filesDir, "sealed").apply { mkdirs() }, name)

    fun read(context: Context, name: String): ByteArray? {
        val target = file(context, name)
        if (!target.exists()) return null
        return runCatching {
            val raw = target.readBytes()
            val cipher = Cipher.getInstance("AES/GCM/NoPadding")
            cipher.init(Cipher.DECRYPT_MODE, key(), GCMParameterSpec(128, raw, 0, IV))
            cipher.doFinal(raw, IV, raw.size - IV)
        }.getOrNull()
    }

    fun write(context: Context, name: String, bytes: ByteArray) {
        val cipher = Cipher.getInstance("AES/GCM/NoPadding")
        cipher.init(Cipher.ENCRYPT_MODE, key())
        val sealed = cipher.doFinal(bytes)
        val target = file(context, name)
        val temp = File(target.parentFile, ".$name.tmp")
        temp.writeBytes(cipher.iv + sealed)
        temp.renameTo(target)
    }

    fun delete(context: Context, name: String) {
        file(context, name).delete()
    }

    fun wipe(context: Context) {
        File(context.filesDir, "sealed").deleteRecursively()
    }
}

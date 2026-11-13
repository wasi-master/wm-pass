package com.wasimaster.wmpass.core

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyPermanentlyInvalidatedException
import android.security.keystore.KeyProperties
import android.security.keystore.StrongBoxUnavailableException
import java.io.File
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * Fingerprint or face unlock, bound to the key rather than to a yes from a dialog.
 *
 * The vault key is encrypted under a hardware key that releases one operation per
 * biometric prompt, and is destroyed by the system when a new finger is enrolled. A
 * process that skips the prompt gets nothing it can use, which is what separates this
 * from an app lock.
 */
class QuickUnlock(private val context: Context) {
    private val keyStore: KeyStore = KeyStore.getInstance(STORE).apply { load(null) }
    private val folder get() = File(context.filesDir, "quick")

    /** The vault file quick unlock is set up for, or null. */
    // Not a write's temporary file, which a process killed part way through leaves behind: taken
    // for the enrolment, its key would not be found and quick unlock would be switched off.
    fun enrolledFile(): String? = folder.listFiles()?.firstOrNull { !it.name.startsWith(".") }?.name

    private fun alias(file: String) = "wmpass.quick.$file"

    private fun createKey(file: String, strongBox: Boolean): SecretKey {
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, STORE)
        val spec = KeyGenParameterSpec.Builder(
            alias(file),
            KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
        )
            .setKeySize(256)
            .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
            .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
            .setUserAuthenticationRequired(true)
            .setInvalidatedByBiometricEnrollment(true)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            spec.setUserAuthenticationParameters(0, KeyProperties.AUTH_BIOMETRIC_STRONG)
        } else {
            @Suppress("DEPRECATION")
            spec.setUserAuthenticationValidityDurationSeconds(-1)
        }
        if (strongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) spec.setIsStrongBoxBacked(true)
        generator.init(spec.build())
        return generator.generateKey()
    }

    /** A cipher to hand to the prompt for enrolling. */
    fun cipherForEnrolling(file: String): Cipher {
        keyStore.deleteEntry(alias(file))
        val key = try {
            createKey(file, strongBox = true)
        } catch (_: StrongBoxUnavailableException) {
            createKey(file, strongBox = false)
        }
        return Cipher.getInstance(TRANSFORM).apply { init(Cipher.ENCRYPT_MODE, key) }
    }

    /** Stores the vault key, using a cipher the prompt has just authorised. */
    fun finishEnrolling(file: String, cipher: Cipher, vaultKey: SecretBytes) {
        vaultKey.consume { raw ->
            val sealed = cipher.doFinal(raw)
            File(folder, file).writeAtomically(cipher.iv + sealed)
        }
        // One vault at a time: an enrolment left for another file would leave the door to pick
        // whichever the folder lists first.
        folder.listFiles()?.filter { it.name != file }?.forEach {
            keyStore.deleteEntry(alias(it.name))
            it.delete()
        }
    }

    /**
     * A cipher to hand to the prompt for unlocking, or null when quick unlock has gone
     * away: nothing enrolled, or the system destroyed the key because a new finger was
     * added. Either way the master password is what opens the vault next.
     */
    fun cipherForUnlocking(): Cipher? {
        val file = enrolledFile() ?: return null
        val stored = File(folder, file).readBytes()
        val key = keyStore.getKey(alias(file), null) as? SecretKey ?: return null.also { disable() }
        return try {
            Cipher.getInstance(TRANSFORM).apply {
                init(Cipher.DECRYPT_MODE, key, GCMParameterSpec(128, stored, 0, IV))
            }
        } catch (_: KeyPermanentlyInvalidatedException) {
            disable()
            null
        }
    }

    /** The vault key, using a cipher the prompt has just authorised. */
    fun unwrap(cipher: Cipher): ByteArray {
        val file = checkNotNull(enrolledFile())
        val stored = File(folder, file).readBytes()
        return cipher.doFinal(stored, IV, stored.size - IV)
    }

    fun disable() {
        folder.listFiles()?.forEach {
            keyStore.deleteEntry(alias(it.name))
            it.delete()
        }
    }

    private companion object {
        const val STORE = "AndroidKeyStore"
        const val TRANSFORM = "AES/GCM/NoPadding"
        const val IV = 12
    }
}

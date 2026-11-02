package com.wasimaster.wmpass.core

import java.io.File
import java.io.FileOutputStream
import java.security.SecureRandom

/** Writes so a crash leaves the old file or the new one, never half of either. */
internal fun File.writeAtomically(bytes: ByteArray) {
    parentFile?.mkdirs()
    val temp = File(parentFile, ".$name.${randomHex(4)}.tmp")
    try {
        FileOutputStream(temp).use { out ->
            out.write(bytes)
            out.flush()
            out.fd.sync()
        }
        check(temp.renameTo(this)) { "could not replace the file" }
    } finally {
        temp.delete()
    }
}

/** Random lower-case hex, for file names that say nothing. */
internal fun randomHex(bytes: Int): String {
    val raw = ByteArray(bytes)
    SecureRandom().nextBytes(raw)
    return raw.joinToString("") { "%02x".format(it) }
}

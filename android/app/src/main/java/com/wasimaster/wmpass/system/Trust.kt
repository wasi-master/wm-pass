package com.wasimaster.wmpass.system

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import java.security.MessageDigest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive

/** Who is really asking: a package name alone is not proof, its signing certificate is. */
object Trust {
    @Volatile private var privileged: Map<String, Set<String>>? = null

    /** Lowercase hex SHA-256 of the app's signing certificate, or null if it is not installed. */
    fun certOf(context: Context, packageName: String): String? = runCatching {
        val pm = context.packageManager
        val signatures = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val info = pm.getPackageInfo(packageName, PackageManager.GET_SIGNING_CERTIFICATES).signingInfo ?: return null
            // Several signers at once is not something a browser or a login app does. Refuse it.
            if (info.hasMultipleSigners()) return null
            info.signingCertificateHistory
        } else {
            @Suppress("DEPRECATION")
            pm.getPackageInfo(packageName, PackageManager.GET_SIGNATURES).signatures
        }
        val latest = signatures?.lastOrNull() ?: return null
        MessageDigest.getInstance("SHA-256").digest(latest.toByteArray()).joinToString("") { "%02x".format(it) }
    }.getOrNull()

    fun labelOf(context: Context, packageName: String): String = runCatching {
        val pm = context.packageManager
        pm.getApplicationLabel(pm.getApplicationInfo(packageName, 0)).toString()
    }.getOrDefault(packageName)

    private fun load(context: Context): Map<String, Set<String>> {
        privileged?.let { return it }
        val parsed = runCatching {
            val root = Json.parseToJsonElement(privilegedJson(context)).jsonObject
            root["apps"]!!.jsonArray.associate { app ->
                val info = app.jsonObject["info"]!!.jsonObject
                val name = info["package_name"]!!.jsonPrimitive.content
                val certs = info["signatures"]!!.jsonArray.mapNotNull { signature ->
                    val fields = signature.jsonObject
                    if (fields["build"]?.jsonPrimitive?.content != "release") return@mapNotNull null
                    fields["cert_fingerprint_sha256"]?.jsonPrimitive?.content?.replace(":", "")?.lowercase()
                }.toSet()
                name to certs
            }
        }.getOrDefault(emptyMap())
        privileged = parsed
        return parsed
    }

    fun privilegedJson(context: Context): String =
        context.assets.open("privileged.json").bufferedReader().use { it.readText() }

    /** A browser we trust to say what page it is on: the right package, signed by the right key. */
    fun isVerifiedBrowser(context: Context, packageName: String): Boolean {
        val expected = load(context)[packageName] ?: return false
        val cert = certOf(context, packageName) ?: return false
        return cert in expected
    }
}

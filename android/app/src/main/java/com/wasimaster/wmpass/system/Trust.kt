package com.wasimaster.wmpass.system

import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import java.security.MessageDigest
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import org.xmlpull.v1.XmlPullParser

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

    /** Every signer of an app, as [PanicPins] keeps them, or null if it is not installed. */
    fun signersOf(context: Context, packageName: String): PanicPins.Signers? = runCatching {
        val pm = context.packageManager
        fun hex(signature: android.content.pm.Signature) = MessageDigest.getInstance("SHA-256").digest(signature.toByteArray()).joinToString("") { "%02x".format(it) }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val info = pm.getPackageInfo(packageName, PackageManager.GET_SIGNING_CERTIFICATES).signingInfo ?: return null
            if (info.hasMultipleSigners()) {
                val all = info.apkContentsSigners?.map(::hex)?.toSet().orEmpty()
                PanicPins.Signers(all, all)
            } else {
                // Oldest first, the one signing it now last.
                val history = info.signingCertificateHistory?.map(::hex).orEmpty()
                val now = history.lastOrNull() ?: return null
                PanicPins.Signers(setOf(now), history.toSet())
            }
        } else {
            @Suppress("DEPRECATION")
            val all = pm.getPackageInfo(packageName, PackageManager.GET_SIGNATURES).signatures?.map(::hex)?.toSet().orEmpty()
            PanicPins.Signers(all, all)
        }
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

    @Volatile private var compat: Set<String>? = null

    /**
     * The browsers filled in compatibility mode, as res/xml/autofill_service.xml names them. Android
     * fills one only when it is on its own allowed list too, and takes the page from its address
     * bar itself, so what such a browser says the page is comes from the system.
     */
    fun compatBrowsers(context: Context): Set<String> {
        compat?.let { return it }
        val found = runCatching {
            val names = HashSet<String>()
            val xml = context.resources.getXml(com.wasimaster.wmpass.R.xml.autofill_service)
            try {
                while (xml.next() != XmlPullParser.END_DOCUMENT) {
                    if (xml.eventType == XmlPullParser.START_TAG && xml.name == "compatibility-package") {
                        xml.getAttributeValue(ANDROID_NS, "name")?.let(names::add)
                    }
                }
            } finally {
                xml.close()
            }
            names
        }.getOrDefault(emptySet())
        compat = found
        return found
    }

    /**
     * Whether [packageName] may say which website its form is on. A browser may: one on the
     * privileged list and signed as it says, or one filled in compatibility mode. Any other app can
     * put any domain it likes in its own views, so its forms are matched by the app alone.
     */
    fun mayNameDomain(context: Context, packageName: String): Boolean =
        domainTrusted(packageName, compatBrowsers(context)) { isVerifiedBrowser(context, it) }

    /** [mayNameDomain] with its two lists handed in, for tests. */
    internal fun domainTrusted(packageName: String, compat: Set<String>, verified: (String) -> Boolean): Boolean =
        packageName in compat || verified(packageName)

    private const val ANDROID_NS = "http://schemas.android.com/apk/res/android"
}

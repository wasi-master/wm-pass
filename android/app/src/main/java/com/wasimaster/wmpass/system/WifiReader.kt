package com.wasimaster.wmpass.system

import android.content.AttributionSource
import android.net.wifi.WifiConfiguration
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.os.Process
import kotlin.system.exitProcess
import org.json.JSONArray
import org.json.JSONObject
import rikka.shizuku.SystemServiceHelper

/**
 * Reads the saved Wi-Fi networks from inside Shizuku's own process.
 *
 * Shizuku starts this class with `app_process` as the shell, or as root, so it calls the Wi-Fi
 * service as that user directly. That process has no hidden-API checks, which an app process has,
 * so nothing in WM Pass itself touches a hidden API.
 *
 * The call is the one wifi-password-manager makes (github.com/Khh-vu/wifi-password-manager, MIT,
 * Copyright (c) 2025-2026 Khh-vu): `IWifiManager.getPrivilegedConfiguredNetworks`, whose arguments
 * change between Android versions.
 */
class WifiReader() : IWifiReader.Stub() {
    override fun destroy() {
        exitProcess(0)
    }

    override fun read(): String =
        try {
            JSONArray(networks().map { it.toJson() }).toString()
        } catch (e: Throwable) {
            // Only some exception types cross a binder; this one carries the message back.
            val cause = (e as? java.lang.reflect.InvocationTargetException)?.targetException ?: e
            throw IllegalStateException(cause.message ?: cause.javaClass.simpleName)
        }

    private fun networks(): List<SavedWifi.Network> {
        val binder = SystemServiceHelper.getSystemService("wifi") ?: error("No Wi-Fi service")
        val manager = Class.forName("android.net.wifi.IWifiManager\$Stub")
            .getMethod("asInterface", IBinder::class.java)
            .invoke(null, binder)
        val uid = Process.myUid()
        val user = when (uid) {
            ROOT -> "root"
            SYSTEM -> "android"
            else -> "shell"
        }
        val caller = if (uid == SYSTEM) "android" else "com.android.shell"
        val method = Class.forName("android.net.wifi.IWifiManager").methods
            .filter { it.name == "getPrivilegedConfiguredNetworks" }
            .maxByOrNull { it.parameterCount }
            ?: error("This version of Android has no way to read saved networks")
        val args: Array<Any?> = when (method.parameterCount) {
            1 -> arrayOf(user)
            2 -> arrayOf(user, caller)
            else -> arrayOf(user, caller, extras(uid, caller))
        }
        // From Android 11 the list comes back in the Wi-Fi module's renamed ParceledListSlice.
        val slice = method.invoke(manager, *args) ?: return emptyList()
        @Suppress("UNCHECKED_CAST")
        val configs = slice.javaClass.getMethod("getList").invoke(slice) as? List<WifiConfiguration> ?: emptyList()
        return configs.mapNotNull(::read)
    }

    /** From Android 13 the service wants to know who is asking, as the shell. */
    private fun extras(uid: Int, caller: String): Bundle? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return null
        val source = AttributionSource.Builder(uid).setPackageName(caller).setAttributionTag(caller).build()
        return Bundle().apply { putParcelable("EXTRA_PARAM_KEY_ATTRIBUTION_SOURCE", source) }
    }

    @Suppress("DEPRECATION")
    private fun read(config: WifiConfiguration): SavedWifi.Network? {
        if (config.isPasspoint) return null
        val ssid = unquote(config.SSID ?: return null)
        if (ssid.isEmpty()) return null
        val keys = config.allowedKeyManagement
        val wep = config.wepKeys?.getOrNull(config.wepTxKeyIndex)?.takeIf { it.isNotEmpty() }
        val psk = config.preSharedKey?.takeIf { it.isNotEmpty() && it != "*" }
        val security = when {
            keys.get(WifiConfiguration.KeyMgmt.SAE) -> "sae"
            keys.get(WifiConfiguration.KeyMgmt.WPA_PSK) -> "wpa"
            keys.get(WifiConfiguration.KeyMgmt.WPA_EAP) || keys.get(WifiConfiguration.KeyMgmt.IEEE8021X) -> "eap"
            wep != null -> "wep"
            psk != null -> "wpa"
            else -> "open"
        }
        val password = when (security) {
            "wep" -> unquote(wep.orEmpty())
            "wpa", "sae" -> unquote(psk.orEmpty())
            else -> ""
        }
        return SavedWifi.Network(ssid, security, password, config.hiddenSSID)
    }

    private fun SavedWifi.Network.toJson() =
        JSONObject().put("ssid", ssid).put("security", security).put("password", password).put("hidden", hidden)

    /** Text comes quoted; a name or key that is not text comes as bare hex, and stays that way. */
    private fun unquote(value: String): String =
        if (value.length >= 2 && value.startsWith('"') && value.endsWith('"')) value.substring(1, value.length - 1) else value

    private companion object {
        const val ROOT = 0
        const val SYSTEM = 1000
    }
}

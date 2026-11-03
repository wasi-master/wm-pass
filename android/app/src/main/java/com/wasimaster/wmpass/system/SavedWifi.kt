package com.wasimaster.wmpass.system

import android.content.AttributionSource
import android.content.Context
import android.content.pm.PackageManager
import android.net.wifi.WifiConfiguration
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import kotlin.coroutines.resume
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.lsposed.hiddenapibypass.HiddenApiBypass
import rikka.shizuku.Shizuku
import rikka.shizuku.ShizukuBinderWrapper
import rikka.shizuku.SystemServiceHelper

/**
 * The Wi-Fi networks this phone has saved, with their passwords, read through Shizuku.
 *
 * Android hands saved passwords only to the system and the shell. Shizuku runs as the shell when
 * started over wireless debugging, or as root, and passes one call through to the Wi-Fi service
 * for us. Nothing is read until the person asks, and nothing is kept outside the items they save.
 *
 * The call is the one wifi-password-manager makes (github.com/Khh-vu/wifi-password-manager, MIT,
 * Copyright (c) 2025-2026 Khh-vu): `IWifiManager.getPrivilegedConfiguredNetworks`, whose arguments
 * change between Android versions.
 */
object SavedWifi {
    class Network(val ssid: String, val security: String, val password: String, val hidden: Boolean)

    enum class State { MISSING, STOPPED, ASK, REFUSED, UNSUPPORTED, READY }

    const val SHIZUKU_PACKAGE = "moe.shizuku.privileged.api"
    private const val ROOT = 0
    private const val SYSTEM = 1000
    private const val REQUEST = 7101

    /** Shizuku sends its binder a moment after the process starts, so give it that moment. */
    suspend fun state(context: Context): State {
        if (!Shizuku.pingBinder()) {
            withTimeoutOrNull(1500) {
                suspendCancellableCoroutine { done ->
                    val listener = object : Shizuku.OnBinderReceivedListener {
                        override fun onBinderReceived() {
                            Shizuku.removeBinderReceivedListener(this)
                            if (done.isActive) done.resume(Unit)
                        }
                    }
                    Shizuku.addBinderReceivedListenerSticky(listener)
                    done.invokeOnCancellation { Shizuku.removeBinderReceivedListener(listener) }
                }
            }
        }
        if (!Shizuku.pingBinder()) {
            val installed = runCatching { context.packageManager.getPackageInfo(SHIZUKU_PACKAGE, 0) }.isSuccess
            return if (installed) State.STOPPED else State.MISSING
        }
        if (Shizuku.isPreV11()) return State.UNSUPPORTED
        // Android 10 gives the shell no saved passwords; only root reads them there.
        if (Build.VERSION.SDK_INT == Build.VERSION_CODES.Q && Shizuku.getUid() !in setOf(ROOT, SYSTEM)) return State.UNSUPPORTED
        return when {
            Shizuku.checkSelfPermission() == PackageManager.PERMISSION_GRANTED -> State.READY
            Shizuku.shouldShowRequestPermissionRationale() -> State.REFUSED
            else -> State.ASK
        }
    }

    /** Shows Shizuku's own prompt. True once WM Pass is allowed. */
    suspend fun ask(): Boolean = suspendCancellableCoroutine { done ->
        val listener = object : Shizuku.OnRequestPermissionResultListener {
            override fun onRequestPermissionResult(requestCode: Int, grantResult: Int) {
                if (requestCode != REQUEST) return
                Shizuku.removeRequestPermissionResultListener(this)
                if (done.isActive) done.resume(grantResult == PackageManager.PERMISSION_GRANTED)
            }
        }
        Shizuku.addRequestPermissionResultListener(listener)
        done.invokeOnCancellation { Shizuku.removeRequestPermissionResultListener(listener) }
        Shizuku.requestPermission(REQUEST)
    }

    /** Every saved network with a name, by name. Throws if the Wi-Fi service refuses. */
    suspend fun list(): List<Network> = withContext(Dispatchers.IO) {
        // From Android 11 the Wi-Fi service ships as its own module, and the list comes back in that
        // module's renamed copy of ParceledListSlice, `com.android.wifi.x.com.android.modules.utils`.
        HiddenApiBypass.addHiddenApiExemptions(
            "Landroid/net/wifi/IWifiManager",
            "Landroid/content/pm/ParceledListSlice",
            "Landroid/content/pm/BaseParceledListSlice",
            "Lcom/android/wifi/x/com/android/modules/utils/",
            "Lcom/android/modules/utils/",
        )
        val binder = ShizukuBinderWrapper(SystemServiceHelper.getSystemService(Context.WIFI_SERVICE))
        val manager = Class.forName("android.net.wifi.IWifiManager\$Stub")
            .getMethod("asInterface", IBinder::class.java)
            .invoke(null, binder)
        val uid = Shizuku.getUid()
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
        val slice = method.invoke(manager, *args) ?: return@withContext emptyList()
        @Suppress("UNCHECKED_CAST")
        val configs = slice.javaClass.getMethod("getList").invoke(slice) as? List<WifiConfiguration> ?: emptyList()
        configs.mapNotNull(::read).distinctBy { it.ssid }.sortedBy { it.ssid.lowercase() }
    }

    /** From Android 13 the service wants to know who is asking, as the shell. */
    private fun extras(uid: Int, caller: String): Bundle? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU) return null
        val source = AttributionSource.Builder(uid).setPackageName(caller).setAttributionTag(caller).build()
        return Bundle().apply { putParcelable("EXTRA_PARAM_KEY_ATTRIBUTION_SOURCE", source) }
    }

    @Suppress("DEPRECATION")
    private fun read(config: WifiConfiguration): Network? {
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
        return Network(ssid, security, password, config.hiddenSSID)
    }

    /** Text comes quoted; a name or key that is not text comes as bare hex, and stays that way. */
    private fun unquote(value: String): String =
        if (value.length >= 2 && value.startsWith('"') && value.endsWith('"')) value.substring(1, value.length - 1) else value
}

package com.wasimaster.wmpass.system

import android.content.ComponentName
import android.content.Context
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Build
import android.os.IBinder
import com.wasimaster.wmpass.BuildConfig
import kotlin.coroutines.resume
import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.suspendCancellableCoroutine
import kotlinx.coroutines.withContext
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONArray
import rikka.shizuku.Shizuku

/**
 * The Wi-Fi networks this phone has saved, with their passwords, read through Shizuku.
 *
 * Android hands saved passwords only to the system and the shell. Shizuku runs as the shell when
 * started over wireless debugging, or as root, and runs [WifiReader] as that user to ask the Wi-Fi
 * service for us. Nothing is read until the person asks, and nothing is kept outside the items they
 * save.
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

    /**
     * Every saved network with a name, by name. Throws if the Wi-Fi service refuses.
     *
     * Shizuku runs [WifiReader] in its own process for the one call, then stops it.
     */
    suspend fun list(context: Context): List<Network> = withContext(Dispatchers.IO) {
        val args = Shizuku.UserServiceArgs(ComponentName(context.packageName, WifiReader::class.java.name))
            .processNameSuffix("wifi")
            .tag("wifi")
            .version(BuildConfig.VERSION_CODE)
            .debuggable(BuildConfig.DEBUG)
            .daemon(false)
        val bound = CompletableDeferred<IWifiReader>()
        val connection = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
                if (service != null && service.pingBinder()) bound.complete(IWifiReader.Stub.asInterface(service))
            }

            override fun onServiceDisconnected(name: ComponentName?) {
                bound.completeExceptionally(IllegalStateException("Shizuku stopped before the networks came back"))
            }
        }
        Shizuku.bindUserService(args, connection)
        try {
            // Shizuku starts a fresh process for the reader, which takes a moment on a slow phone.
            val reader = withTimeoutOrNull(15_000) { bound.await() } ?: error("Shizuku did not start the reader")
            val found = JSONArray(reader.read())
            (0 until found.length()).map { found.getJSONObject(it) }
                .map { Network(it.getString("ssid"), it.getString("security"), it.getString("password"), it.getBoolean("hidden")) }
                .distinctBy { it.ssid }
                .sortedBy { it.ssid.lowercase() }
        } finally {
            runCatching { Shizuku.unbindUserService(args, connection, true) }
        }
    }
}

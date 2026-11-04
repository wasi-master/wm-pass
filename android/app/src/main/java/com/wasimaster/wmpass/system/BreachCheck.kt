package com.wasimaster.wmpass.system

import android.Manifest
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.app.MainActivity
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.vault.BreachCounts
import kotlinx.coroutines.CancellationException
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.launch
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.long
import kotlinx.serialization.json.put

/**
 * The password breach check, run for the whole app rather than for one screen, so going back
 * does not stop it. While it runs a notification shows how far it has got and can stop it. What
 * it finds stays in memory, as before; only when it ran, with which service and how much it
 * found are kept, so the page can say so.
 */
object BreachCheck {
    data class Progress(
        val running: Boolean = false,
        val done: Int = 0,
        val total: Int = 0,
        val source: String = "hibp",
        val problem: String? = null,
    )

    /** The last finished check: when, with which service, and what it found. */
    data class Last(val atMs: Long, val source: String, val found: Int, val checked: Int)

    private val state = MutableStateFlow(Progress())
    val progress: StateFlow<Progress> = state.asStateFlow()
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private var job: Job? = null

    private const val CHANNEL = "breach_check"
    private const val NOTE = 4107
    private const val STOP = "com.wasimaster.wmpass.STOP_BREACH_CHECK"

    fun sourceName(source: String): String = if (source == "xon") "XposedOrNot" else "Have I Been Pwned"

    fun last(context: Context): Last? {
        val parts = Prefs.of(context).breachLast.value.split('|')
        if (parts.size < 4) return null
        return Last(parts[0].toLongOrNull() ?: return null, parts[1], parts[2].toIntOrNull() ?: 0, parts[3].toIntOrNull() ?: 0)
    }

    /** Checks every password in [ids] with [source]. Does nothing while a check is running. */
    fun start(context: Context, ids: List<String>, source: String) {
        if (job?.isActive == true || ids.isEmpty()) return
        val app = context.applicationContext
        state.value = Progress(running = true, total = ids.size, source = source)
        notifyProgress(app)
        job = scope.launch {
            val counts = mutableMapOf<String, Long>()
            var problem: String? = null
            try {
                for ((index, id) in ids.withIndex()) {
                    ensureActive()
                    val secret = Pass.reveal(id, "password")
                    val reply = secret.consume { bytes ->
                        Engine.net("breach.password", online = NetAccess.allowed(app), secret = bytes) { put("source", source) }
                    }
                    val count = reply.obj["count"]?.jsonPrimitive?.long ?: 0
                    if (count > 0) counts[id] = count
                    state.value = state.value.copy(done = index + 1)
                    notifyProgress(app)
                }
            } catch (stopped: CancellationException) {
                problem = "Stopped after ${state.value.done} of ${ids.size}."
            } catch (failure: Exception) {
                problem = if (Pass.state.value !is com.wasimaster.wmpass.core.LockState.Unlocked) {
                    "The vault locked before the check finished. Checked ${state.value.done} of ${ids.size}."
                } else {
                    failure.message ?: "The check stopped."
                }
            }
            val checked = state.value.done
            if (checked > 0) {
                // What a part-finished check found still counts; the rest keep their earlier answer.
                BreachCounts.counts.value = if (checked == ids.size) counts else BreachCounts.counts.value + counts
                Prefs.of(app).breachLast.set("${System.currentTimeMillis()}|$source|${counts.size}|$checked")
            }
            state.value = state.value.copy(running = false, problem = problem)
            NotificationManagerCompat.from(app).cancel(NOTE)
        }
    }

    fun stop() {
        job?.cancel()
    }

    private fun notifyProgress(context: Context) {
        if (Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return
        val manager = context.getSystemService(NotificationManager::class.java) ?: return
        if (manager.getNotificationChannel(CHANNEL) == null) {
            manager.createNotificationChannel(
                NotificationChannel(CHANNEL, "Breach checks", NotificationManager.IMPORTANCE_LOW).apply {
                    description = "Shows a password breach check while it runs"
                    setShowBadge(false)
                },
            )
        }
        val now = state.value
        val open = PendingIntent.getActivity(
            context,
            0,
            Intent(context, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val stop = PendingIntent.getBroadcast(
            context,
            1,
            Intent(context, StopReceiver::class.java).setAction(STOP),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        val title = "Checking passwords with ${sourceName(now.source)}"
        val builder = NotificationCompat.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_tile_lock)
            .setContentTitle(title)
            .setContentText("${now.done} of ${now.total}")
            .setProgress(now.total, now.done, now.total == 0)
            .setOngoing(true)
            .setOnlyAlertOnce(true)
            .setSilent(true)
            .setContentIntent(open)
            .addAction(0, "Stop", stop)
            // Nothing here says which password, but a lock screen need not show even this much.
            .setVisibility(NotificationCompat.VISIBILITY_PRIVATE)
            .setPublicVersion(
                NotificationCompat.Builder(context, CHANNEL)
                    .setSmallIcon(R.drawable.ic_tile_lock)
                    .setContentTitle("WM Pass is working")
                    .build(),
            )
        runCatching { NotificationManagerCompat.from(context).notify(NOTE, builder.build()) }
    }

    /** The notification's Stop button. */
    class StopReceiver : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            if (intent.action == STOP) stop()
        }
    }
}

package com.wasimaster.wmpass.system.widgets

import android.app.AlarmManager
import android.app.Application
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.os.Build
import androidx.compose.runtime.Immutable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetManager
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.updateAll
import com.wasimaster.wmpass.BuildConfig
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.system.Backups
import com.wasimaster.wmpass.system.Destinations
import com.wasimaster.wmpass.system.QuickWidgetReceiver
import com.wasimaster.wmpass.ui.vault.BreachCounts
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.FlowPreview
import kotlinx.coroutines.Job
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.coroutines.flow.debounce
import kotlinx.coroutines.flow.drop
import kotlinx.coroutines.flow.update
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlin.reflect.KClass

/** The last health report's counts. Not secret: kept outside the vault so a locked phone shows them. */
@Immutable
data class HealthCache(val score: Int, val weak: Int, val reused: Int, val breached: Int, val atMs: Long) {
    val issues: Int get() = weak + reused + breached

    companion object
}

/** One backup place as a widget shows it. */
@Immutable
data class BackupRow(val id: String, val label: String, val lastMs: Long?, val failed: Boolean)

/** One code as a widget shows it, while the vault is open and widgets may show codes. */
@Immutable
data class CodeRow(val id: String, val title: String, val code: String?, val next: String?, val endsAtMs: Long)

/** One favorite as a widget shows it: a letter and a name, never more. */
@Immutable
data class FavoriteRow(val id: String, val title: String, val color: String?)

/** Everything the widgets draw from. Codes and favorites are empty unless the vault is open and widgets may show them. */
@Immutable
data class WidgetData(
    val state: LockState = LockState.Locked,
    val health: HealthCache? = null,
    val backups: List<BackupRow> = emptyList(),
    val backupEveryDays: Int = 0,
    val codes: List<CodeRow> = emptyList(),
    val favorites: List<FavoriteRow> = emptyList(),
    /** What was copied from a widget a moment ago: `generator` or a code's item id. */
    val copied: String? = null,
    val copiedUntilMs: Long = 0,
) {
    fun justCopied(key: String, now: Long = System.currentTimeMillis()) = copied == key && now < copiedUntilMs

    companion object {
        /** What the widget picker shows. Made up, and plainly so. */
        val SAMPLE = WidgetData(
            state = LockState.Unlocked("Personal", "", false),
            health = HealthCache(86, 1, 2, 0, System.currentTimeMillis() - 3_600_000),
            backups = listOf(
                BackupRow("a", "Home folder", System.currentTimeMillis() - 7_200_000, false),
                BackupRow("b", "WebDAV", System.currentTimeMillis() - 90_000_000, true),
            ),
            codes = listOf(
                CodeRow("a", "Example", "284913", null, System.currentTimeMillis() + 21_000),
                CodeRow("b", "Sample", "640027", null, System.currentTimeMillis() + 21_000),
            ),
            favorites = listOf(FavoriteRow("a", "Example", null), FavoriteRow("b", "Sample", null), FavoriteRow("c", "Notes", null)),
        )
    }
}

/**
 * Keeps every widget's data and redraws them. The rules it holds to:
 *
 * - Codes and favorites only while the vault is open and Show on widgets is on. Locking empties
 *   them and redraws at once.
 * - A start of the app redraws everything first, from a locked state, so a widget drawn before the
 *   process died does not keep showing what it showed.
 * - While vault content is on a widget, an alarm two minutes ahead is kept armed. If the process is
 *   killed, the alarm starts it again, which redraws everything locked.
 */
@OptIn(FlowPreview::class)
object WidgetFeed {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)
    private val mutable = MutableStateFlow(WidgetData())
    val data: StateFlow<WidgetData> = mutable.asStateFlow()

    private lateinit var app: Context
    private var ticker: Job? = null
    private var flashJob: Job? = null

    /** Every widget, by the receiver the launcher knows it by. */
    private val all: List<Pair<KClass<out GlanceAppWidgetReceiver>, () -> GlanceAppWidget>> = listOf(
        QuickWidgetReceiver::class to { QuickWidget() },
        StatusWidgetReceiver::class to { StatusWidget() },
        HealthWidgetReceiver::class to { HealthWidget() },
        BackupsWidgetReceiver::class to { BackupsWidget() },
        GeneratorWidgetReceiver::class to { GeneratorWidget() },
        CodesWidgetReceiver::class to { CodesWidget() },
        FavoritesWidgetReceiver::class to { FavoritesWidget() },
    )

    fun start(application: Application) {
        app = application.applicationContext
        mutable.update { it.copy(health = HealthCache.load(app), backups = backupRows(app), backupEveryDays = Backups.everyDays(app)) }
        scope.launch {
            Pass.state.collect { state ->
                mutable.update { it.copy(state = state) }
                if (state is LockState.Unlocked) {
                    vaultChanged()
                } else {
                    ticker?.cancel()
                    mutable.update { it.copy(codes = emptyList(), favorites = emptyList()) }
                    cancelBlank(app)
                }
                redrawAll()
            }
        }
        scope.launch { Pass.revision.drop(1).debounce(2_000).collect { if (open()) vaultChanged(health = true) } }
        scope.launch { Pass.entries.drop(1).debounce(500).collect { if (open()) vaultChanged(health = false) } }
        scope.launch { Destinations.list.drop(1).collect { backupsChanged(app) } }
        scope.launch { publishPreviews() }
    }

    private fun open() = Pass.state.value is LockState.Unlocked

    private fun allowed() = open() && Prefs.of(app).widgetAccess.value

    /** Redraws every widget, after a change to how they look or what they may show. */
    fun redraw(context: Context) {
        if (!::app.isInitialized) app = context.applicationContext
        scope.launch {
            if (open()) vaultChanged(health = false)
            redrawAll()
        }
    }

    private suspend fun redrawAll() {
        for ((_, make) in all) runCatching { make().updateAll(app) }
    }

    private suspend fun placed(widget: KClass<out GlanceAppWidget>): Boolean =
        runCatching { GlanceAppWidgetManager(app).getGlanceIds(widget.java).isNotEmpty() }.getOrDefault(false)

    /** The vault opened or changed: health, favorites and codes. */
    private suspend fun vaultChanged(health: Boolean = true) {
        if (health || data.value.health == null) refreshHealth()
        val prefs = Prefs.of(app)
        val favorites = if (allowed()) {
            Pass.entries.value.filter { it.favourite && !it.trashed }.take(MAX_FAVORITES).map { FavoriteRow(it.id, it.title, it.color) }
        } else {
            emptyList()
        }
        if (favorites != data.value.favorites) {
            mutable.update { it.copy(favorites = favorites) }
            runCatching { FavoritesWidget().updateAll(app) }
        }
        if (favorites.isNotEmpty() && placed(FavoritesWidget::class)) armBlank(app)
        if (allowed() && prefs.widgetAccess.value) codesWanted()
    }

    /** A Health widget was drawn: works out the counts if there are none yet and the vault is open. */
    fun healthWanted() {
        if (!::app.isInitialized || data.value.health != null || !open()) return
        scope.launch { refreshHealth() }
    }

    private suspend fun refreshHealth() {
        if (!open() || !placed(HealthWidget::class)) return
        val report = runCatching { Pass.health(BreachCounts.counts.value) }.getOrNull() ?: return
        val cache = HealthCache(report.score, report.weak.size, report.reused.sumOf { it.size }, report.breached.size, System.currentTimeMillis())
        cache.save(app)
        mutable.update { it.copy(health = cache) }
        runCatching { HealthWidget().updateAll(app) }
    }

    /** A place was added, removed or has just run. */
    fun backupsChanged(context: Context) {
        if (!::app.isInitialized) return
        mutable.update { it.copy(backups = backupRows(context), backupEveryDays = Backups.everyDays(context)) }
        scope.launch { runCatching { BackupsWidget().updateAll(app) } }
    }

    private fun backupRows(context: Context): List<BackupRow> =
        Destinations.list.value.filter { it.backup }.map { place ->
            val run = Destinations.lastRun(context, place.id)
            BackupRow(place.id, place.label.ifBlank { place.kind }, run?.atMs, run?.problem != null)
        }

    /** Starts the codes ticking, if a Codes widget is placed and may show them. */
    @Synchronized
    fun codesWanted() {
        if (!::app.isInitialized || ticker?.isActive == true) return
        ticker = scope.launch {
            while (isActive && allowed() && placed(CodesWidget::class)) {
                val rows = runCatching { codeRows() }.getOrDefault(emptyList())
                mutable.update { it.copy(codes = rows) }
                runCatching { CodesWidget().updateAll(app) }
                if (rows.isNotEmpty()) armBlank(app)
                delay(nextWake(rows, Prefs.of(app).codeNext.value))
            }
            if (!allowed()) mutable.update { it.copy(codes = emptyList()) }
        }
    }

    private suspend fun codeRows(): List<CodeRow> {
        val prefs = Prefs.of(app)
        val ids = prefs.widgetCodes.ifEmpty { Pass.entries.value.filter { it.favourite && it.hasOtp && !it.trashed }.map { it.id } }.take(MAX_CODES)
        if (ids.isEmpty()) return emptyList()
        val now = System.currentTimeMillis()
        val rows = Pass.codes(ids).filter { it.otp.kind != "hotp" }.associateBy { it.id }
        return ids.mapNotNull { id ->
            val row = rows[id] ?: return@mapNotNull null
            val left = row.secondsLeft ?: row.otp.period ?: 30
            CodeRow(row.id, row.title, row.code, row.nextCode, now + left * 1000L)
        }
    }

    /** A wipe: the counts and the chosen codes go with the vault, and every widget draws empty. */
    fun forget(context: Context) {
        context.getSharedPreferences(STORE, Context.MODE_PRIVATE).edit().clear().apply()
        Prefs.of(context).updateWidgetCodes(emptyList())
        mutable.update { WidgetData(state = it.state) }
        redraw(context)
    }

    /** Shows "Copied" on a widget for a moment. */
    fun flash(context: Context, key: String) {
        if (!::app.isInitialized) app = context.applicationContext
        flashJob?.cancel()
        mutable.update { it.copy(copied = key, copiedUntilMs = System.currentTimeMillis() + FLASH_MS) }
        flashJob = scope.launch {
            redrawFor(key)
            delay(FLASH_MS + 100)
            mutable.update { it.copy(copied = null, copiedUntilMs = 0) }
            redrawFor(key)
        }
    }

    private suspend fun redrawFor(key: String) {
        runCatching { if (key == "generator") GeneratorWidget().updateAll(app) else CodesWidget().updateAll(app) }
    }

    /** Android 15 draws the picker's previews from the widgets themselves, once per version. */
    private suspend fun publishPreviews() {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.VANILLA_ICE_CREAM) return
        val store = app.getSharedPreferences(STORE, Context.MODE_PRIVATE)
        if (store.getInt("previews", 0) == BuildConfig.VERSION_CODE) return
        val manager = GlanceAppWidgetManager(app)
        val done = all.all { (receiver, _) ->
            runCatching { manager.setWidgetPreviews(receiver) == GlanceAppWidgetManager.SET_WIDGET_PREVIEWS_RESULT_SUCCESS }.getOrDefault(false)
        }
        if (done) store.edit().putInt("previews", BuildConfig.VERSION_CODE).apply()
    }

    private fun blankIntent(context: Context) = PendingIntent.getBroadcast(
        context,
        0,
        Intent(context, WidgetBlankReceiver::class.java),
        PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

    private fun armBlank(context: Context) {
        val alarms = context.getSystemService(AlarmManager::class.java) ?: return
        alarms.set(AlarmManager.RTC, System.currentTimeMillis() + BLANK_AFTER_MS, blankIntent(context))
    }

    private fun cancelBlank(context: Context) {
        context.getSystemService(AlarmManager::class.java)?.cancel(blankIntent(context))
    }

    /** The alarm went off: with the vault still open, arm it again; otherwise draw everything locked. */
    internal fun blankDue(context: Context) {
        if (!::app.isInitialized) app = context.applicationContext
        if (allowed() && (data.value.codes.isNotEmpty() || data.value.favorites.isNotEmpty())) {
            armBlank(app)
        } else {
            redraw(app)
        }
    }

    internal const val STORE = "widgets"
    private const val MAX_CODES = 8
    private const val MAX_FAVORITES = 12
    private const val FLASH_MS = 2_000L
    private const val BLANK_AFTER_MS = 120_000L

    /** When to draw the codes again: as each runs out, and ten seconds before, to show the next one. */
    internal fun nextWake(rows: List<CodeRow>, showNext: Boolean, now: Long = System.currentTimeMillis()): Long {
        val wakes = rows.flatMap { row ->
            val end = row.endsAtMs - now
            listOfNotNull(end + 50, (end - NEXT_WINDOW_MS).takeIf { showNext && it > 0 })
        }
        // A code already run out wants drawing again at once, so nothing is dropped for being past.
        return (wakes.minOrNull() ?: 30_000L).coerceIn(1_000L, 30_000L)
    }

    internal const val NEXT_WINDOW_MS = 10_000L
}

/** Starts the process again if it was killed while a widget showed codes, which draws them locked. */
class WidgetBlankReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        WidgetFeed.blankDue(context)
    }
}

internal fun HealthCache.save(context: Context) {
    context.getSharedPreferences(WidgetFeed.STORE, Context.MODE_PRIVATE).edit()
        .putInt("health_score", score)
        .putInt("health_weak", weak)
        .putInt("health_reused", reused)
        .putInt("health_breached", breached)
        .putLong("health_at", atMs)
        .apply()
}

internal fun HealthCache.Companion.load(context: Context): HealthCache? {
    val store = context.getSharedPreferences(WidgetFeed.STORE, Context.MODE_PRIVATE)
    if (!store.contains("health_at")) return null
    return HealthCache(
        store.getInt("health_score", 0),
        store.getInt("health_weak", 0),
        store.getInt("health_reused", 0),
        store.getInt("health_breached", 0),
        store.getLong("health_at", 0),
    )
}

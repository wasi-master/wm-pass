package com.wasimaster.wmpass.system.widgets

import android.content.Context
import android.os.SystemClock
import android.widget.RemoteViews
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.toArgb
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.GlanceModifier
import androidx.glance.LocalSize
import androidx.glance.action.clickable
import androidx.glance.appwidget.AndroidRemoteViews
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.SizeMode
import androidx.glance.appwidget.cornerRadius
import androidx.glance.background
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Column
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.padding
import androidx.glance.layout.size
import androidx.glance.layout.width
import androidx.glance.semantics.contentDescription
import androidx.glance.semantics.semantics
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import androidx.glance.text.TextAlign
import androidx.glance.unit.ColorProvider
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.ui.parts.tileColors

/** "284 913" for six digits, "2849 1307" for eight, anything else as it is. */
internal fun spacedCode(code: String): String = when (code.length) {
    6 -> "${code.take(3)} ${code.drop(3)}"
    8 -> "${code.take(4)} ${code.drop(4)}"
    else -> code
}

/** The same shape, with dots in place of the digits. */
internal fun hiddenCode(code: String): String = spacedCode("•".repeat(code.length))

/**
 * Why a widget that would show vault content shows nothing, or null if it may show it. The order
 * matters: a widget that is not allowed says so even when the vault is locked, so the setting is
 * found.
 */
private fun blocked(context: Context, data: WidgetData, what: String): Pair<Int, String>? = when {
    !Prefs.of(context).widgetAccess.value -> R.drawable.ic_widget_shield to "Turn on Show on widgets in WM Pass, under Security, to see $what here."
    data.state !is LockState.Unlocked -> R.drawable.ic_widget_lock to "Locked. Open WM Pass to see $what."
    else -> null
}

// ---- codes ----

/**
 * The codes the owner picked, while the vault is open and Show on widgets is on. Locked, it says
 * only that it is locked. Counter-based codes are left out, since showing one uses it up.
 */
class CodesWidget : PassWidget() {
    override val sizeMode = SizeMode.Responsive(setOf(DpSize(130.dp, 60.dp), DpSize(250.dp, 60.dp), DpSize(250.dp, 120.dp), DpSize(250.dp, 200.dp)))

    override fun shown() = WidgetFeed.codesWanted()

    @Composable
    override fun Body(context: Context, data: WidgetData, colors: WidgetColors) {
        val size = LocalSize.current
        val prefs = Prefs.of(context)
        val stop = blocked(context, data, "codes")
        WidgetFrame(colors, onClick = if (stop != null || data.codes.isEmpty()) openApp(context, "codes") else null, padding = 10.dp) {
            if (stop != null) {
                Placeholder(colors, stop.first, stop.second)
                return@WidgetFrame
            }
            if (data.codes.isEmpty()) {
                Placeholder(colors, R.drawable.ic_widget_codes, "Star an item that has a code, or pick codes under Settings, Appearance, Widgets.")
                return@WidgetFrame
            }
            val room = (size.height / 56.dp).toInt().coerceAtLeast(1)
            val now = System.currentTimeMillis()
            Column(GlanceModifier.fillMaxSize()) {
                data.codes.take(room).forEachIndexed { index, row ->
                    val copied = data.justCopied(row.id, now)
                    val code = row.code
                    val shown = when {
                        code == null -> "–"
                        prefs.codeHidden.value -> hiddenCode(code)
                        else -> spacedCode(code)
                    }
                    val showNext = prefs.codeNext.value && !prefs.codeHidden.value && row.next != null && row.endsAtMs - now <= WidgetFeed.NEXT_WINDOW_MS
                    Row(
                        GlanceModifier
                            .fillMaxWidth()
                            .padding(top = if (index == 0) 0.dp else 6.dp)
                            .clickable(copyCodeAction(row.id))
                            .semantics { contentDescription = "Copy the code for ${row.title}" },
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Column(GlanceModifier.defaultWeight()) {
                            Text(row.title, style = WidgetText.style(colors.textMid, 12.sp), maxLines = 1)
                            Text(
                                if (copied) "Copied" else shown,
                                style = WidgetText.style(if (copied) colors.primary else colors.text, 22.sp, FontWeight.Medium, mono = !copied),
                                maxLines = 1,
                            )
                        }
                        Column(horizontalAlignment = Alignment.End) {
                            Countdown(context, row.endsAtMs, colors.textMid)
                            if (showNext) {
                                Text("Next ${spacedCode(row.next.orEmpty())}", style = WidgetText.style(colors.textMid, 11.sp, mono = true), maxLines = 1)
                            }
                        }
                    }
                }
            }
        }
    }
}

/** Seconds left, counted down by the launcher itself, so the widget is not redrawn every second. */
@Composable
private fun Countdown(context: Context, endsAtMs: Long, color: ColorProvider) {
    val views = RemoteViews(context.packageName, R.layout.widget_countdown)
    val left = (endsAtMs - System.currentTimeMillis()).coerceAtLeast(0)
    views.setChronometer(R.id.widget_countdown, SystemClock.elapsedRealtime() + left, null, true)
    views.setChronometerCountDown(R.id.widget_countdown, true)
    views.setTextColor(R.id.widget_countdown, color.getColor(context).toArgb())
    AndroidRemoteViews(views)
}

class CodesWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = CodesWidget()
}

// ---- favorites ----

/** Favorites as letters in their colours, while the vault is open and Show on widgets is on. */
class FavoritesWidget : PassWidget() {
    override val sizeMode = SizeMode.Responsive(setOf(DpSize(200.dp, 80.dp), DpSize(320.dp, 80.dp), DpSize(250.dp, 170.dp), DpSize(320.dp, 170.dp)))

    @Composable
    override fun Body(context: Context, data: WidgetData, colors: WidgetColors) {
        val size = LocalSize.current
        val stop = blocked(context, data, "favorites")
        WidgetFrame(colors, onClick = if (stop != null || data.favorites.isEmpty()) openApp(context, null) else null, padding = 10.dp) {
            if (stop != null) {
                Placeholder(colors, stop.first, stop.second)
                return@WidgetFrame
            }
            if (data.favorites.isEmpty()) {
                Placeholder(colors, R.drawable.ic_widget_star, "No favorites yet. Star an item to put it here.")
                return@WidgetFrame
            }
            val across = ((size.width - 20.dp) / 68.dp).toInt().coerceAtLeast(1)
            val down = ((size.height - 20.dp) / 76.dp).toInt().coerceAtLeast(1)
            Column(GlanceModifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically) {
                data.favorites.take(across * down).chunked(across).forEach { line ->
                    Row(GlanceModifier.fillMaxWidth(), horizontalAlignment = Alignment.CenterHorizontally) {
                        line.forEachIndexed { index, item ->
                            if (index > 0) Spacer(GlanceModifier.width(4.dp))
                            Favorite(context, item, colors)
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Favorite(context: Context, item: FavoriteRow, colors: WidgetColors) {
    val (from, _) = tileColors(item.title, item.color)
    val letter = item.title.firstOrNull { it.isLetterOrDigit() }?.uppercase() ?: "•"
    Column(
        GlanceModifier
            .width(64.dp)
            .padding(vertical = 4.dp)
            .clickable(openApp(context, "item:${item.id}"))
            .semantics { contentDescription = "Open ${item.title}" },
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        Box(GlanceModifier.size(44.dp).background(ColorProvider(from)).cornerRadius(22.dp), contentAlignment = Alignment.Center) {
            Text(letter, style = WidgetText.style(ColorProvider(Color.White), 18.sp, FontWeight.Medium).copy(textAlign = TextAlign.Center), maxLines = 1)
        }
        Text(item.title, GlanceModifier.padding(top = 4.dp), style = WidgetText.style(colors.textMid, 11.sp).copy(textAlign = TextAlign.Center), maxLines = 1)
    }
}

class FavoritesWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = FavoritesWidget()
}

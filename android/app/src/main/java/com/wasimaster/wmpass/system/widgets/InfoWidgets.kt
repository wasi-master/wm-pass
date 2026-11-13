package com.wasimaster.wmpass.system.widgets

import androidx.glance.semantics.semantics
import androidx.glance.semantics.contentDescription
import android.content.Context
import android.text.format.DateFormat
import android.text.format.DateUtils
import androidx.compose.runtime.Composable
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.GlanceModifier
import androidx.glance.LocalSize
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.SizeMode
import androidx.glance.layout.Alignment
import androidx.glance.layout.Column
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.fillMaxWidth
import androidx.glance.layout.padding
import androidx.glance.layout.width
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.app.Prefs
import java.util.Date

/**
 * "14:02" today, "Yesterday" or "3 Nov" before that. [inSentence] gives "yesterday", for after
 * another word: lowering the first letter of the whole would make "Nov 3" "nov 3" where the month
 * comes first.
 */
internal fun whenText(context: Context, ms: Long, inSentence: Boolean = false): String = when {
    DateUtils.isToday(ms) -> DateFormat.getTimeFormat(context).format(Date(ms))
    DateUtils.isToday(ms + DateUtils.DAY_IN_MILLIS) -> if (inSentence) "yesterday" else "Yesterday"
    else -> DateUtils.formatDateTime(context, ms, DateUtils.FORMAT_SHOW_DATE or DateUtils.FORMAT_ABBREV_MONTH or DateUtils.FORMAT_NO_YEAR)
}

// ---- health ----

/** The health score and what needs fixing, as counts. It names no item. */
class HealthWidget : PassWidget() {
    override val sizeMode = SizeMode.Responsive(setOf(DpSize(120.dp, 60.dp), DpSize(120.dp, 120.dp), DpSize(250.dp, 60.dp), DpSize(250.dp, 120.dp)))

    override fun shown() = WidgetFeed.healthWanted()

    @Composable
    override fun Body(context: Context, data: WidgetData, colors: WidgetColors) {
        val health = data.health
        val size = LocalSize.current
        // Read as one sentence, the score with what it is out of, rather than a bare number.
        val spoken = when {
            health == null -> "Password health. Open WM Pass to check your passwords"
            else -> buildString {
                append("Password health")
                if (Prefs.of(context).healthScore.value) append(": ${health.score} out of 100")
                append(". ")
                append(if (health.issues == 0) "All good" else if (health.issues == 1) "1 to fix" else "${health.issues} to fix")
                append(". ${health.weak} weak, ${health.reused} reused, ${health.breached} breached")
            }
        }
        WidgetFrame(colors, GlanceModifier.semantics { contentDescription = spoken }, onClick = openApp(context, "health")) {
            if (health == null) {
                Placeholder(colors, R.drawable.ic_widget_health, "Open WM Pass to check your passwords")
                return@WidgetFrame
            }
            val showScore = Prefs.of(context).healthScore.value
            val verdict = if (health.issues == 0) "All good" else if (health.issues == 1) "1 to fix" else "${health.issues} to fix"
            Column(GlanceModifier.fillMaxSize()) {
                Halo(colors) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Glyph(R.drawable.ic_widget_health, null, colors.primary, 16.dp)
                        Text("Health", GlanceModifier.padding(start = 6.dp), style = WidgetText.style(colors.textMid, 12.sp), maxLines = 1)
                    }
                }
                Spacer(GlanceModifier.defaultWeight())
                Halo(colors) {
                    Column {
                        Row(verticalAlignment = Alignment.Bottom) {
                            if (showScore) {
                                Text("${health.score}", style = WidgetText.style(colors.text, 30.sp, FontWeight.Medium), maxLines = 1)
                                Spacer(GlanceModifier.width(8.dp))
                            }
                            Text(
                                verdict,
                                GlanceModifier.padding(bottom = if (showScore) 5.dp else 0.dp),
                                style = WidgetText.style(if (health.issues == 0) colors.primary else colors.warn, if (showScore) 13.sp else 18.sp, FontWeight.Medium),
                                maxLines = 1,
                            )
                        }
                        if (size.width >= 240.dp || size.height >= 110.dp) {
                            Text(
                                "${health.weak} weak · ${health.reused} reused · ${health.breached} breached",
                                GlanceModifier.padding(top = 2.dp),
                                style = WidgetText.style(colors.textMid, 12.sp),
                                maxLines = 1,
                            )
                        }
                        if (size.height >= 110.dp) {
                            Text("Checked ${whenText(context, health.atMs, inSentence = true)}", style = WidgetText.style(colors.textMid, 11.sp), maxLines = 1)
                        }
                    }
                }
            }
        }
    }
}

class HealthWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = HealthWidget()
}

// ---- backups ----

/** Whether a place's backups are late: past the schedule, with a day's grace. */
internal fun overdue(row: BackupRow, everyDays: Int, now: Long = System.currentTimeMillis()): Boolean {
    if (everyDays <= 0) return false
    val last = row.lastMs ?: return true
    return now - last > (everyDays + 1) * DateUtils.DAY_IN_MILLIS
}

/** Each backup place, when it last ran, and whether that went wrong. Nothing from the vault. */
class BackupsWidget : PassWidget() {
    override val sizeMode = SizeMode.Responsive(setOf(DpSize(120.dp, 60.dp), DpSize(200.dp, 60.dp), DpSize(200.dp, 120.dp), DpSize(250.dp, 180.dp)))

    @Composable
    override fun Body(context: Context, data: WidgetData, colors: WidgetColors) {
        val size = LocalSize.current
        WidgetFrame(colors, onClick = openApp(context, "backups")) {
            if (data.backups.isEmpty()) {
                Placeholder(colors, R.drawable.ic_widget_backup, "No backup places yet. Tap to add one.")
                return@WidgetFrame
            }
            val room = ((size.height - 36.dp) / 34.dp).toInt().coerceAtLeast(1)
            Column(GlanceModifier.fillMaxSize()) {
                Halo(colors) {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Glyph(R.drawable.ic_widget_backup, null, colors.primary, 16.dp)
                        Text("Backups", GlanceModifier.padding(start = 6.dp), style = WidgetText.style(colors.textMid, 12.sp), maxLines = 1)
                    }
                }
                data.backups.take(room).forEach { row ->
                    val late = overdue(row, data.backupEveryDays)
                    val (icon, tint, detail) = when {
                        row.failed -> Triple(R.drawable.ic_widget_error, colors.danger, "Failed ${row.lastMs?.let { whenText(context, it, inSentence = true) }.orEmpty()}".trim())
                        row.lastMs == null -> Triple(R.drawable.ic_widget_schedule, colors.warn, "Never")
                        late -> Triple(R.drawable.ic_widget_warning, colors.warn, "Late, last ${whenText(context, row.lastMs, inSentence = true)}")
                        else -> Triple(R.drawable.ic_widget_check, colors.primary, whenText(context, row.lastMs))
                    }
                    // A good backup shows only its time beside a tick; the tick is said in words.
                    val fine = !row.failed && row.lastMs != null && !late
                    Row(GlanceModifier.fillMaxWidth().padding(top = 8.dp), verticalAlignment = Alignment.CenterVertically) {
                        Glyph(icon, if (fine) "Backed up" else null, tint, 18.dp)
                        Spacer(GlanceModifier.width(8.dp))
                        if (size.width >= 190.dp) {
                            WidgetLabel(row.label, colors, WidgetText.style(colors.text, 13.sp), GlanceModifier.defaultWeight())
                            WidgetLabel(detail, colors, WidgetText.style(colors.textMid, 12.sp))
                        } else {
                            Halo(colors) {
                                Column {
                                    Text(row.label, style = WidgetText.style(colors.text, 13.sp), maxLines = 1)
                                    Text(detail, style = WidgetText.style(colors.textMid, 11.sp), maxLines = 1)
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

class BackupsWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = BackupsWidget()
}

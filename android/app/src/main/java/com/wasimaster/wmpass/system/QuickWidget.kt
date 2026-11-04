package com.wasimaster.wmpass.system

import android.content.Context
import androidx.glance.GlanceId
import androidx.glance.action.ActionParameters
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.action.ActionCallback
import com.wasimaster.wmpass.system.widgets.QuickWidget

/**
 * The quick bar. Its class name stays here, where the first version put it: a launcher remembers a
 * widget by its receiver's name, and moving it would drop every quick bar already placed.
 */
class QuickWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = QuickWidget()
}

/**
 * What the first version's lock button named. A quick bar drawn before an update still points here
 * until it is drawn again, so a tap on it has to keep working.
 */
class LockAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        com.wasimaster.wmpass.system.widgets.LockAction().onAction(context, glanceId, parameters)
    }
}

package com.wasimaster.wmpass.system

import android.content.Context
import android.content.Intent
import androidx.compose.runtime.Composable
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.unit.dp
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.GlanceTheme
import androidx.glance.Image
import androidx.glance.ImageProvider
import androidx.glance.action.ActionParameters
import androidx.glance.action.clickable
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.action.ActionCallback
import androidx.glance.appwidget.action.actionRunCallback
import androidx.glance.appwidget.action.actionStartActivity
import androidx.glance.appwidget.cornerRadius
import androidx.glance.appwidget.provideContent
import androidx.glance.background
import androidx.glance.layout.Alignment
import androidx.glance.layout.Box
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.padding
import androidx.glance.layout.size
import androidx.glance.layout.width
import androidx.glance.unit.ColorProvider
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.app.MainActivity
import com.wasimaster.wmpass.core.Pass

/**
 * A row of shortcuts on the home screen. It names no site and shows no code, because a home
 * screen is visible to anyone holding the phone, locked vault or not.
 */
class QuickWidget : GlanceAppWidget() {
    override suspend fun provideGlance(context: Context, id: GlanceId) {
        provideContent { GlanceTheme { Bar(context) } }
    }

    @Composable
    private fun Bar(context: Context) {
        Row(
            GlanceModifier
                .fillMaxSize()
                .background(ColorProvider(Color(0xFF0D1C19)))
                .cornerRadius(26.dp)
                .padding(10.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            Button(context, R.drawable.ic_widget_search, "search")
            Spacer(GlanceModifier.width(8.dp))
            Button(context, R.drawable.ic_widget_add, "add")
            Spacer(GlanceModifier.width(8.dp))
            Button(context, R.drawable.ic_widget_codes, "codes")
            Spacer(GlanceModifier.width(8.dp))
            Box(
                GlanceModifier
                    .size(48.dp)
                    .background(ColorProvider(Color(0xFF1A2F2A)))
                    .cornerRadius(24.dp)
                    .clickable(actionRunCallback<LockAction>()),
                contentAlignment = Alignment.Center,
            ) {
                Image(ImageProvider(R.drawable.ic_widget_lock), "Lock", GlanceModifier.size(24.dp))
            }
        }
    }

    @Composable
    private fun Button(context: Context, icon: Int, route: String) {
        val intent = Intent(context, MainActivity::class.java).setAction(Intent.ACTION_VIEW).putExtra(MainActivity.EXTRA_ROUTE, route).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        Box(
            GlanceModifier
                .size(48.dp)
                .background(ColorProvider(Color(0xFF1A2F2A)))
                .cornerRadius(24.dp)
                .clickable(actionStartActivity(intent)),
            contentAlignment = Alignment.Center,
        ) {
            Image(ImageProvider(icon), route, GlanceModifier.size(24.dp))
        }
    }
}

class LockAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        Pass.lock()
    }
}

class QuickWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = QuickWidget()
}

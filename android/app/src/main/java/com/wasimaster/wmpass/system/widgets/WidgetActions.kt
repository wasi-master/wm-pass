package com.wasimaster.wmpass.system.widgets

import android.content.Context
import android.content.Intent
import android.net.Uri
import androidx.glance.GlanceId
import androidx.glance.action.Action
import androidx.glance.action.ActionParameters
import androidx.glance.action.actionParametersOf
import androidx.glance.appwidget.action.ActionCallback
import androidx.glance.appwidget.action.actionRunCallback
import androidx.glance.appwidget.action.actionStartActivity
import com.wasimaster.wmpass.app.MainActivity
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.SecretBytes
import com.wasimaster.wmpass.core.SecureClipboard
import com.wasimaster.wmpass.ui.vault.GenState
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.withContext

/**
 * Opens WM Pass, on [route] if there is one, after the door if the vault is locked. Each route gets
 * its own address, so the system does not take two buttons' intents for the same one.
 */
fun openApp(context: Context, route: String?): Action {
    val intent = Intent(context, MainActivity::class.java)
        .setAction(Intent.ACTION_VIEW)
        .setData(Uri.parse("wmpass-widget://open/${Uri.encode(route.orEmpty())}"))
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    if (route != null) intent.putExtra(MainActivity.EXTRA_ROUTE, route)
    return actionStartActivity(intent)
}

fun lockAction(): Action = actionRunCallback<LockAction>()

fun generateAction(): Action = actionRunCallback<GenerateAction>()

fun copyCodeAction(id: String): Action = actionRunCallback<CopyCodeAction>(actionParametersOf(CopyCodeAction.Id to id))

/** Locks, or locks and refuses passwords for an hour, as Settings, Appearance, Widgets says. */
class LockAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        val panic = Prefs.of(context).widgetLock.value == "panic"
        withContext(Dispatchers.Main) { if (panic) Pass.panic() else Pass.lock() }
    }
}

/** Copies a new password made the way the generator is set up. Nothing is shown. */
class GenerateAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        val generator = GenState.fromPrefs(Prefs.of(context))
        runCatching { generator.roll() }.getOrElse { return }
        val value = generator.value
        if (value.isEmpty()) return
        withContext(Dispatchers.Main) { SecureClipboard.copyGenerated(context.applicationContext, "Password", SecretBytes(value.toByteArray())) }
        WidgetFeed.flash(context, "generator")
    }
}

/**
 * Copies one code. The widget only offers this while the vault is open and widgets may show codes,
 * and both are checked again here, since the vault may have locked since it was drawn.
 */
class CopyCodeAction : ActionCallback {
    override suspend fun onAction(context: Context, glanceId: GlanceId, parameters: ActionParameters) {
        val id = parameters[Id] ?: return
        if (Pass.state.value !is LockState.Unlocked || !Prefs.of(context).widgetAccess.value) {
            WidgetFeed.redraw(context)
            return
        }
        val code = runCatching { Pass.code(id) }.getOrNull() ?: return
        withContext(Dispatchers.Main) { SecureClipboard.copy(context.applicationContext, "Code", code) }
        WidgetFeed.flash(context, id)
    }

    companion object {
        val Id = ActionParameters.Key<String>("id")
    }
}

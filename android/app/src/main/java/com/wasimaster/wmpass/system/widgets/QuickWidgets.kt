package com.wasimaster.wmpass.system.widgets

import androidx.glance.semantics.semantics
import androidx.glance.semantics.contentDescription
import androidx.glance.action.clickable
import android.content.Context
import androidx.compose.runtime.Composable
import androidx.compose.runtime.collectAsState
import androidx.compose.runtime.getValue
import androidx.compose.ui.unit.DpSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.glance.GlanceId
import androidx.glance.GlanceModifier
import androidx.glance.LocalSize
import androidx.glance.action.Action
import androidx.glance.appwidget.GlanceAppWidget
import androidx.glance.appwidget.GlanceAppWidgetReceiver
import androidx.glance.appwidget.SizeMode
import androidx.glance.appwidget.provideContent
import androidx.glance.layout.Alignment
import androidx.glance.layout.Column
import androidx.glance.layout.Row
import androidx.glance.layout.Spacer
import androidx.glance.layout.fillMaxSize
import androidx.glance.layout.padding
import androidx.glance.layout.width
import androidx.glance.text.FontWeight
import androidx.glance.text.Text
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.LockState

/**
 * A widget drawn from [WidgetFeed]. The picker's preview draws the same thing from made-up data, so
 * what someone sees there is what they get.
 */
abstract class PassWidget : GlanceAppWidget() {
    override suspend fun provideGlance(context: Context, id: GlanceId) {
        shown()
        provideContent {
            val data by WidgetFeed.data.collectAsState()
            Body(context, data, WidgetLook.colors(context))
        }
    }

    override suspend fun providePreview(context: Context, widgetCategory: Int) {
        provideContent { Body(context, WidgetData.SAMPLE, WidgetLook.colors(context)) }
    }

    /** Placed or redrawn: a chance to fetch what this widget needs. */
    protected open fun shown() {}

    @Composable
    abstract fun Body(context: Context, data: WidgetData, colors: WidgetColors)
}

// ---- quick bar ----

private class QuickButton(val icon: Int, val label: String, val spoken: String, val action: Action, val active: Boolean = false)

private fun quickButton(context: Context, id: String, state: LockState): QuickButton = when (id) {
    "search" -> QuickButton(R.drawable.ic_widget_search, "Search", "Search WM Pass", openApp(context, "search"))
    "add" -> QuickButton(R.drawable.ic_widget_add, "Add", "Add an item", openApp(context, "add"))
    "codes" -> QuickButton(R.drawable.ic_widget_codes, "Codes", "Open codes", openApp(context, "codes"))
    "generator" -> QuickButton(R.drawable.ic_widget_generate, "Generator", "Open the generator", openApp(context, "generator"))
    "health" -> QuickButton(R.drawable.ic_widget_health, "Health", "Open password health", openApp(context, "health"))
    else -> when (state) {
        // Said as what it does: with the widget lock set to panic, a tap also refuses every password for an hour.
        is LockState.Unlocked -> if (Prefs.of(context).widgetLock.value == "panic") {
            QuickButton(R.drawable.ic_widget_lock_open, "Panic", "Panic lock WM Pass, refusing passwords for an hour", lockAction(), active = true)
        } else {
            QuickButton(R.drawable.ic_widget_lock_open, "Lock", "Lock WM Pass", lockAction(), active = true)
        }
        LockState.Locked -> QuickButton(R.drawable.ic_widget_lock, "Locked", "WM Pass is locked. Unlock it", unlockHere(context))
        // Nothing to unlock yet: the app, to make one, as the status widget does.
        LockState.NoVault -> QuickButton(R.drawable.ic_widget_lock, "No vault", "Open WM Pass to make a vault", openApp(context, null))
    }
}

/** A row of the buttons the owner picked. It shows nothing from the vault. */
class QuickWidget : PassWidget() {
    override val sizeMode = SizeMode.Responsive(
        setOf(
            DpSize(60.dp, 60.dp),
            DpSize(130.dp, 60.dp),
            DpSize(190.dp, 60.dp),
            DpSize(250.dp, 60.dp),
            DpSize(320.dp, 60.dp),
            DpSize(250.dp, 110.dp),
            DpSize(320.dp, 110.dp),
        ),
    )

    @Composable
    override fun Body(context: Context, data: WidgetData, colors: WidgetColors) {
        val size = LocalSize.current
        val ids = Prefs.of(context).widgetButtons.ifEmpty { Prefs.DEFAULT_WIDGET_BUTTONS }
        val labelled = size.height >= 100.dp
        val step = if (labelled) 68.dp else 56.dp
        val fits = ((size.width - 16.dp) / step).toInt().coerceAtLeast(1)
        WidgetFrame(colors, padding = 8.dp) {
            Row(GlanceModifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically, horizontalAlignment = Alignment.CenterHorizontally) {
                ids.take(fits).forEachIndexed { index, id ->
                    if (index > 0) Spacer(GlanceModifier.width(if (labelled) 4.dp else 8.dp))
                    val button = quickButton(context, id, data.state)
                    if (labelled) {
                        // One stop: the column takes the tap and the name, so the button and the
                        // label under it are not read one after the other.
                        Column(
                            GlanceModifier
                                .width(64.dp)
                                .clickable(button.action)
                                .semantics { contentDescription = button.spoken },
                            horizontalAlignment = Alignment.CenterHorizontally,
                        ) {
                            RoundButton(button.icon, null, colors, null, button.active)
                            WidgetLabel(button.label, colors, WidgetText.style(colors.textMid, 11.sp), GlanceModifier.padding(top = 4.dp))
                        }
                    } else {
                        RoundButton(button.icon, button.spoken, colors, button.action, button.active)
                    }
                }
            }
        }
    }
}

// ---- lock status ----

/** Whether the vault is open. A tap locks an open vault, or opens the door to a locked one. */
class StatusWidget : PassWidget() {
    override val sizeMode = SizeMode.Responsive(setOf(DpSize(60.dp, 60.dp), DpSize(130.dp, 60.dp), DpSize(200.dp, 60.dp)))

    @Composable
    override fun Body(context: Context, data: WidgetData, colors: WidgetColors) {
        val state = data.state
        val open = state is LockState.Unlocked
        val panic = Prefs.of(context).widgetLock.value == "panic"
        val title = when (state) {
            is LockState.Unlocked -> "Open"
            LockState.Locked -> "Locked"
            LockState.NoVault -> "No vault"
        }
        val hint = when {
            state is LockState.Unlocked && panic -> "${state.name}. Tap for a panic lock."
            state is LockState.Unlocked -> "${state.name}. Tap to lock."
            state == LockState.Locked -> "Tap to unlock"
            else -> "Tap to make one"
        }
        val icon = if (open) R.drawable.ic_widget_lock_open else R.drawable.ic_widget_lock
        val action = when {
            open -> lockAction()
            state == LockState.Locked -> unlockHere(context)
            else -> openApp(context, null)
        }
        // The whole widget is the one button, named once; the round button inside is only drawn.
        WidgetFrame(colors, GlanceModifier.semantics { contentDescription = "WM Pass ${title.lowercase()}. $hint" }, onClick = action, padding = 10.dp) {
            if (LocalSize.current.width < 120.dp) {
                Column(GlanceModifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically, horizontalAlignment = Alignment.CenterHorizontally) {
                    Glyph(icon, null, if (open) colors.primary else colors.textMid, 26.dp)
                    WidgetLabel(title, colors, WidgetText.style(colors.text, 12.sp, FontWeight.Medium), GlanceModifier.padding(top = 4.dp))
                }
            } else {
                Row(GlanceModifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically) {
                    RoundButton(icon, null, colors, null, active = open, size = 40.dp)
                    Spacer(GlanceModifier.width(10.dp))
                    Halo(colors) {
                        Column {
                            Text(title, style = WidgetText.style(colors.text, 15.sp, FontWeight.Medium), maxLines = 1)
                            Text(hint, style = WidgetText.style(colors.textMid, 12.sp), maxLines = 1)
                        }
                    }
                }
            }
        }
    }
}

class StatusWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = StatusWidget()
}

// ---- generator ----

/** One tap copies a new password made the generator's way. Nothing is shown on the home screen. */
class GeneratorWidget : PassWidget() {
    override val sizeMode = SizeMode.Responsive(setOf(DpSize(60.dp, 60.dp), DpSize(130.dp, 60.dp), DpSize(200.dp, 60.dp)))

    @Composable
    override fun Body(context: Context, data: WidgetData, colors: WidgetColors) {
        val prefs = Prefs.of(context)
        val copied = data.justCopied("generator")
        val what = when (prefs.genMode.value) {
            "passphrase" -> "passphrase"
            "pin" -> "PIN"
            else -> "password"
        }
        val title = if (copied) "Copied" else "New $what"
        // A new password is never wiped from the clipboard (SecureClipboard.copyGenerated): it
        // is saved nowhere yet, so the clear-after setting does not apply to it.
        val hint = if (copied) "Stays on the clipboard" else "Tap to copy one"
        val icon = if (copied) R.drawable.ic_widget_check else R.drawable.ic_widget_generate
        // The whole widget is the one button, named once; the round button inside is only drawn.
        WidgetFrame(colors, GlanceModifier.semantics { contentDescription = if (copied) "New $what copied. $hint" else "Copy a new $what" }, onClick = generateAction(), padding = 10.dp) {
            if (LocalSize.current.width < 120.dp) {
                Column(GlanceModifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically, horizontalAlignment = Alignment.CenterHorizontally) {
                    Glyph(icon, null, colors.primary, 26.dp)
                    WidgetLabel(if (copied) "Copied" else "New", colors, WidgetText.style(colors.text, 12.sp, FontWeight.Medium), GlanceModifier.padding(top = 4.dp))
                }
            } else {
                Row(GlanceModifier.fillMaxSize(), verticalAlignment = Alignment.CenterVertically) {
                    RoundButton(icon, null, colors, null, active = copied, size = 40.dp)
                    Spacer(GlanceModifier.width(10.dp))
                    Halo(colors) {
                        Column {
                            Text(title, style = WidgetText.style(colors.text, 15.sp, FontWeight.Medium), maxLines = 1)
                            Text(hint, style = WidgetText.style(colors.textMid, 12.sp), maxLines = 1)
                        }
                    }
                }
            }
        }
    }
}

class GeneratorWidgetReceiver : GlanceAppWidgetReceiver() {
    override val glanceAppWidget: GlanceAppWidget = GeneratorWidget()
}


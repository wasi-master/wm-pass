package com.wasimaster.wmpass.wear

import android.content.Context
import androidx.wear.protolayout.ActionBuilders
import androidx.wear.protolayout.ColorBuilders
import androidx.wear.protolayout.DimensionBuilders
import androidx.wear.protolayout.LayoutElementBuilders
import androidx.wear.protolayout.ModifiersBuilders
import androidx.wear.protolayout.ResourceBuilders
import androidx.wear.protolayout.TimelineBuilders
import androidx.wear.protolayout.TypeBuilders
import androidx.wear.tiles.RequestBuilders
import androidx.wear.tiles.TileBuilders
import androidx.wear.tiles.TileService
import com.google.common.util.concurrent.ListenableFuture
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.guava.future

/**
 * The first code at a glance. A tile refreshes on the system's schedule rather than ours, so the
 * code carries a timeline: when it runs out the tile says so, rather than showing a dead code.
 */
class CodesTile : TileService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Default)

    override fun onTileRequest(requestParams: RequestBuilders.TileRequest): ListenableFuture<TileBuilders.Tile> = scope.future {
        val now = System.currentTimeMillis()
        val answer = PhoneLink.items(this@CodesTile)
        val first = (answer as? PhoneAnswer.Open)?.codes?.firstOrNull()
        val timeline = TimelineBuilders.Timeline.Builder()
        if (first != null) {
            val end = now + first.secondsLeft * 1000L
            timeline.addTimelineEntry(
                TimelineBuilders.TimelineEntry.Builder()
                    .setValidity(TimelineBuilders.TimeInterval.Builder().setEndMillis(end).build())
                    .setLayout(layout(this@CodesTile, first.title, first.code, "${first.title}, code ${first.code.filter { !it.isWhitespace() }.toList().joinToString(" ")}"))
                    .build(),
            )
            timeline.addTimelineEntry(
                TimelineBuilders.TimelineEntry.Builder()
                    .setValidity(TimelineBuilders.TimeInterval.Builder().setStartMillis(end).build())
                    .setLayout(layout(this@CodesTile, first.title, "Tap for a new code"))
                    .build(),
            )
        } else {
            val words = when (answer) {
                PhoneAnswer.Away -> "Can't reach your phone"
                PhoneAnswer.Locked -> "Unlock WM Pass on your phone"
                PhoneAnswer.Off -> "Turned off on your phone"
                is PhoneAnswer.Open -> "No codes yet"
            }
            timeline.addTimelineEntry(TimelineBuilders.TimelineEntry.Builder().setLayout(layout(this@CodesTile, "WM Pass", words)).build())
        }
        TileBuilders.Tile.Builder()
            .setResourcesVersion(RESOURCES)
            .setTileTimeline(timeline.build())
            .setFreshnessIntervalMillis(FRESH_MS)
            .build()
    }

    override fun onDestroy() {
        scope.cancel()
        super.onDestroy()
    }

    override fun onTileResourcesRequest(requestParams: RequestBuilders.ResourcesRequest): ListenableFuture<ResourceBuilders.Resources> =
        scope.future { ResourceBuilders.Resources.Builder().setVersion(RESOURCES).build() }

    private companion object {
        const val RESOURCES = "1"
        const val FRESH_MS = 5 * 60_000L

        fun text(words: String, sizeSp: Float, colour: Int) = LayoutElementBuilders.Text.Builder()
            .setText(TypeBuilders.StringProp.Builder(words).build())
            .setMaxLines(TypeBuilders.Int32Prop.Builder().setValue(2).build())
            .setFontStyle(
                LayoutElementBuilders.FontStyle.Builder()
                    .setSize(DimensionBuilders.sp(sizeSp))
                    .setColor(ColorBuilders.argb(colour))
                    .build(),
            )
            .build()

        /** [spoken] is what a screen reader says for the tile, when the words on it do not read well aloud. */
        fun layout(context: Context, title: String, body: String, spoken: String = "$title, $body"): LayoutElementBuilders.Layout {
            val open = ModifiersBuilders.Clickable.Builder()
                .setId("open")
                .setOnClick(
                    ActionBuilders.LaunchAction.Builder()
                        .setAndroidActivity(
                            ActionBuilders.AndroidActivity.Builder()
                                .setPackageName(context.packageName)
                                .setClassName(MainActivity::class.java.name)
                                .build(),
                        )
                        .build(),
                )
                .build()
            val column = LayoutElementBuilders.Column.Builder()
                .setModifiers(
                    ModifiersBuilders.Modifiers.Builder()
                        .setClickable(open)
                        // One button to a screen reader, saying what it shows: a code is read digit by digit.
                        .setSemantics(
                            ModifiersBuilders.Semantics.Builder()
                                .setContentDescription(TypeBuilders.StringProp.Builder(spoken).build())
                                .setRole(ModifiersBuilders.SEMANTICS_ROLE_BUTTON)
                                .build(),
                        )
                        .build(),
                )
                .addContent(text(title, 14f, 0xFFB0BEC5.toInt()))
                .addContent(text(body, 24f, 0xFFFFFFFF.toInt()))
                .build()
            return LayoutElementBuilders.Layout.Builder().setRoot(column).build()
        }
    }
}

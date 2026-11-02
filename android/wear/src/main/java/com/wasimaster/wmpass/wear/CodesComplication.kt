package com.wasimaster.wmpass.wear

import android.app.PendingIntent
import android.content.Intent
import androidx.wear.watchface.complications.data.ComplicationData
import androidx.wear.watchface.complications.data.ComplicationType
import androidx.wear.watchface.complications.data.PlainComplicationText
import androidx.wear.watchface.complications.data.ShortTextComplicationData
import androidx.wear.watchface.complications.datasource.ComplicationRequest
import androidx.wear.watchface.complications.datasource.SuspendingComplicationDataSourceService

/**
 * A way into the codes from the watch face. It shows no code: a complication refreshes every few
 * minutes at best, and a code lasts thirty seconds, so it would nearly always show a dead one.
 */
class CodesComplication : SuspendingComplicationDataSourceService() {
    override fun getPreviewData(type: ComplicationType): ComplicationData? = data(null)

    override suspend fun onComplicationRequest(request: ComplicationRequest): ComplicationData? {
        if (request.complicationType != ComplicationType.SHORT_TEXT) return null
        val open = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK),
            PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
        return data(open)
    }

    private fun data(open: PendingIntent?): ComplicationData {
        val words = PlainComplicationText.Builder("Codes").build()
        return ShortTextComplicationData.Builder(words, PlainComplicationText.Builder("Open WM Pass codes").build())
            .setTapAction(open)
            .build()
    }
}

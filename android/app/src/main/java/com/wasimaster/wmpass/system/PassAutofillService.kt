package com.wasimaster.wmpass.system

import android.annotation.SuppressLint
import android.app.PendingIntent
import android.app.assist.AssistStructure
import android.app.slice.Slice
import android.content.Context
import android.content.Intent
import android.content.IntentSender
import android.graphics.drawable.Icon
import android.os.Build
import android.os.CancellationSignal
import android.service.autofill.AutofillService
import android.service.autofill.Dataset
import android.service.autofill.FillCallback
import android.service.autofill.FillRequest
import android.service.autofill.FillResponse
import android.service.autofill.InlinePresentation
import android.service.autofill.SaveCallback
import android.service.autofill.SaveInfo
import android.service.autofill.SaveRequest
import android.view.autofill.AutofillId
import android.widget.RemoteViews
import android.widget.inline.InlinePresentationSpec
import androidx.autofill.inline.UiVersions
import androidx.autofill.inline.v1.InlineSuggestionUi
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.MatchRow
import com.wasimaster.wmpass.core.Pass
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.cancel
import kotlinx.coroutines.launch

/**
 * The system's autofill. It never hands over a secret here: each suggestion carries an
 * authentication step, so a value leaves the vault only after that suggestion is tapped, and a
 * locked vault has to be opened first.
 */
class PassAutofillService : AutofillService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    override fun onDestroy() {
        super.onDestroy()
        scope.cancel()
    }

    override fun onFillRequest(request: FillRequest, cancellation: CancellationSignal, callback: FillCallback) {
        val structure = request.fillContexts.lastOrNull()?.structure
        if (structure == null) {
            callback.onSuccess(null)
            return
        }
        val form = parseForm(structure)
        if (!form.isLogin || form.packageName == packageName) {
            callback.onSuccess(null)
            return
        }
        val job = scope.launch {
            val response = runCatching { AutofillBuilder.response(this@PassAutofillService, form, request) }.getOrNull()
            callback.onSuccess(response)
        }
        cancellation.setOnCancelListener { job.cancel() }
    }

    override fun onSaveRequest(request: SaveRequest, callback: SaveCallback) {
        val structure = request.fillContexts.lastOrNull()?.structure
        if (structure == null) {
            callback.onFailure("Nothing to save")
            return
        }
        val form = parseForm(structure)
        if (form.passwordValue == null) {
            callback.onFailure("No password on this screen")
            return
        }
        // The values stay in this process. Nothing secret goes into an intent.
        PendingSave.set(form)
        val intent = AutofillActivity.intent(this, AutofillActivity.MODE_SAVE).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
            val sender = PendingIntent.getActivity(this, 7, intent, PendingIntent.FLAG_CANCEL_CURRENT or PendingIntent.FLAG_IMMUTABLE).intentSender
            callback.onSuccess(sender)
        } else {
            startActivity(intent)
            callback.onSuccess()
        }
    }
}

/** A save waiting for the owner to approve it. */
object PendingSave {
    @Volatile var form: ParsedForm? = null
        private set

    fun set(value: ParsedForm) {
        form = value
    }

    fun clear() {
        form = null
    }
}

object AutofillBuilder {
    private var nextCode = 100

    private fun sender(context: Context, intent: Intent): IntentSender =
        // Mutable because the system adds the screen's structure to it.
        PendingIntent.getActivity(
            context,
            nextCode++,
            intent,
            PendingIntent.FLAG_CANCEL_CURRENT or if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) PendingIntent.FLAG_MUTABLE else 0,
        ).intentSender

    private fun row(context: Context, title: String, subtitle: String): RemoteViews =
        RemoteViews(context.packageName, R.layout.autofill_row).apply {
            setTextViewText(R.id.autofill_title, title)
            setTextViewText(R.id.autofill_subtitle, subtitle)
        }

    @SuppressLint("RestrictedApi")
    private fun inline(context: Context, request: FillRequest, index: Int, title: String, subtitle: String): InlinePresentation? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
        if (!com.wasimaster.wmpass.app.Prefs.of(context).autofillInline.value) return null
        val inlineRequest = request.inlineSuggestionsRequest ?: return null
        if (index >= inlineRequest.maxSuggestionCount) return null
        val specs: List<InlinePresentationSpec> = inlineRequest.inlinePresentationSpecs
        val spec = specs.getOrNull(index) ?: specs.lastOrNull() ?: return null
        if (UiVersions.INLINE_UI_VERSION_1 !in UiVersions.getVersions(spec.style)) return null
        val attribution = PendingIntent.getActivity(
            context,
            nextCode++,
            AutofillActivity.intent(context, AutofillActivity.MODE_PICK),
            PendingIntent.FLAG_CANCEL_CURRENT or PendingIntent.FLAG_IMMUTABLE,
        )
        val content = InlineSuggestionUi.newContentBuilder(attribution)
            .setTitle(title)
            .setSubtitle(subtitle)
            .setStartIcon(Icon.createWithResource(context, R.drawable.ic_autofill))
            .build()
        val slice: Slice = content.slice
        return InlinePresentation(slice, spec, false)
    }

    private fun Dataset.Builder.placeholders(ids: Array<AutofillId>, view: RemoteViews, inline: InlinePresentation?): Dataset.Builder {
        ids.forEachIndexed { index, id ->
            if (index == 0 && inline != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                @Suppress("DEPRECATION")
                setValue(id, null, view, inline)
            } else {
                @Suppress("DEPRECATION")
                setValue(id, null, view)
            }
        }
        return this
    }

    /** What may be matched against, and how far the request's own claim can be believed. */
    suspend fun matches(context: Context, form: ParsedForm): List<MatchRow> {
        val domain = form.webDomain
        return if (domain != null) {
            Pass.matchWeb("https://$domain")
        } else {
            val cert = Trust.certOf(context, form.packageName) ?: return emptyList()
            Pass.matchApp(form.packageName, cert)
        }
    }

    suspend fun response(context: Context, form: ParsedForm, request: FillRequest?): FillResponse? {
        val builder = FillResponse.Builder()
        val ids = form.ids
        if (ids.isEmpty()) return null
        if (Pass.state.value !is LockState.Unlocked) {
            if (Pass.state.value == LockState.NoVault) return null
            val view = row(context, "Unlock WM Pass", "To fill this form")
            val intent = AutofillActivity.intent(context, AutofillActivity.MODE_UNLOCK)
            builder.setAuthentication(ids, sender(context, intent), view)
            return builder.build()
        }
        var index = 0
        matches(context, form).take(6).forEach { match ->
            val entry = match.entry
            val subtitle = entry.subtitle.ifEmpty { "WM Pass" }
            val view = row(context, entry.title, subtitle)
            val intent = AutofillActivity.intent(context, AutofillActivity.MODE_FILL).putExtra(AutofillActivity.EXTRA_ID, entry.id)
            @Suppress("DEPRECATION")
            val dataset = Dataset.Builder(view)
                .placeholders(ids, view, request?.let { inline(context, it, index, entry.title, subtitle) })
                .setAuthentication(sender(context, intent))
                .build()
            builder.addDataset(dataset)
            index++
        }
        val searchView = row(context, "Search WM Pass", "Choose a login for this form")
        @Suppress("DEPRECATION")
        builder.addDataset(
            Dataset.Builder(searchView)
                .placeholders(ids, searchView, request?.let { inline(context, it, index, "Search WM Pass", "") })
                .setAuthentication(sender(context, AutofillActivity.intent(context, AutofillActivity.MODE_PICK)))
                .build(),
        )
        if (form.password != null) {
            val save = SaveInfo.Builder(
                if (form.username != null) SaveInfo.SAVE_DATA_TYPE_USERNAME or SaveInfo.SAVE_DATA_TYPE_PASSWORD else SaveInfo.SAVE_DATA_TYPE_PASSWORD,
                arrayOf(form.password),
            )
            form.username?.let { save.setOptionalIds(arrayOf(it)) }
            if (com.wasimaster.wmpass.app.Prefs.of(context).autofillSave.value) builder.setSaveInfo(save.build())
        }
        return builder.build()
    }
}

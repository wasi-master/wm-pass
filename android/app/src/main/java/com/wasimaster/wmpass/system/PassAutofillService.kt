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
import android.view.inputmethod.InlineSuggestionsRequest
import android.widget.RemoteViews
import android.widget.inline.InlinePresentationSpec
import androidx.autofill.inline.UiVersions
import androidx.autofill.inline.v1.InlineSuggestionUi
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.app.AutoLock
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
        val parsed = parseForm(structure)
        // Asked for by hand, on a field that says nothing of a login: the field gets the search, so
        // the person who asked is not met with nothing.
        val manual = request.flags and FillRequest.FLAG_MANUAL_REQUEST != 0
        val asked = if (manual && !parsed.isLogin && parsed.focused != null) parsed.withUsernameField(parsed.focused) else parsed
        // The username typed on an earlier screen of this login, carried here by FLAG_DELAY_SAVE:
        // the save this screen asks for is then for a login, not a password alone.
        val form = asked.withEarlierUsername(earlierForms(request.fillContexts))
        if (!form.isLogin || form.packageName == packageName) {
            callback.onSuccess(null)
            return
        }
        val job = scope.launch {
            // The vault's background time may have run out while the phone slept.
            // A lock that fails offers nothing rather than logins from a vault meant to be shut.
            if (runCatching { AutoLock.lockIfOverdue() }.isFailure) {
                callback.onSuccess(null)
                return@launch
            }
            val inline = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) request.inlineSuggestionsRequest else null
            val response = runCatching { AutofillBuilder.response(this@PassAutofillService, form, inline) }.getOrNull()
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
        // A password screen that came after the username's: the session's earlier screens hold it,
        // and on Android 10 and up, those of earlier screens of the task (FLAG_DELAY_SAVE).
        val form = parseForm(structure).withEarlierUsername(earlierForms(request.fillContexts))
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

/** The screens before the last one, newest first, read only as far as they are needed. */
private fun earlierForms(contexts: List<android.service.autofill.FillContext>): Sequence<ParsedForm> =
    contexts.dropLast(1).asReversed().asSequence().map { parseForm(it.structure) }

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
    private fun inline(context: Context, inlineRequest: InlineSuggestionsRequest?, index: Int, title: String, subtitle: String): InlinePresentation? {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.R) return null
        if (!com.wasimaster.wmpass.app.Prefs.of(context).autofillInline.value) return null
        if (inlineRequest == null) return null
        if (index >= inlineRequest.maxSuggestionCount) return null
        val specs: List<InlinePresentationSpec> = inlineRequest.inlinePresentationSpecs
        val spec = specs.getOrNull(index) ?: specs.lastOrNull() ?: return null
        if (UiVersions.INLINE_UI_VERSION_1 !in UiVersions.getVersions(spec.style)) return null
        // A long press on the chip opens the app. The picker would have no form to fill from here.
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
            ?: AutofillActivity.intent(context, AutofillActivity.MODE_PICK)
        val attribution = PendingIntent.getActivity(
            context,
            nextCode++,
            launch,
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
            // On every field: the keyboard asks for the chip of the field that has the focus, and
            // the password field or a username already typed would otherwise show none.
            if (inline != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                @Suppress("DEPRECATION")
                setValue(id, null, view, inline)
            } else {
                @Suppress("DEPRECATION")
                setValue(id, null, view)
            }
        }
        return this
    }

    /**
     * What may be matched against, and how far the request's own claim can be believed. Only a
     * browser may name the website: any other app can put "bank.com" in its own views, so its
     * forms are matched by the app itself, its package and signing certificate, whatever page
     * it says it shows.
     */
    suspend fun matches(context: Context, form: ParsedForm): List<MatchRow> {
        val domain = form.webDomain?.takeIf { Trust.mayNameDomain(context, form.packageName) }
        return if (domain != null) {
            Pass.matchWeb("https://$domain")
        } else {
            val cert = Trust.certOf(context, form.packageName) ?: return emptyList()
            Pass.matchApp(form.packageName, cert)
        }
    }

    /**
     * [request] is the keyboard's ask for chips: from the fill request, or after an unlock, from
     * the extra the system hands the unlock screen, so the logins land on the keyboard's strip too.
     */
    suspend fun response(context: Context, form: ParsedForm, request: InlineSuggestionsRequest?): FillResponse? {
        val builder = FillResponse.Builder()
        val ids = form.ids
        if (ids.isEmpty()) return null
        if (Pass.state.value !is LockState.Unlocked) {
            if (Pass.state.value == LockState.NoVault) return null
            val view = row(context, "Unlock WM Pass", "To fill this form")
            val intent = AutofillActivity.intent(context, AutofillActivity.MODE_UNLOCK)
            // On the keyboard's strip too, so a keyboard has no gap to fill with a button of its own.
            val inline = request?.let { inline(context, it, 0, "Unlock WM Pass", "To fill this form") }
            if (inline != null && Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                @Suppress("DEPRECATION")
                builder.setAuthentication(ids, sender(context, intent), view, inline)
            } else {
                builder.setAuthentication(ids, sender(context, intent), view)
            }
            // A login typed by hand while the vault is shut can still be saved: the save opens it first.
            saveInfo(context, form)?.let(builder::setSaveInfo)
            return builder.build()
        }
        val found = matches(context, form).take(6)
        // The keyboard shows only so many chips. With more logins than fit, the last chip is the
        // search, or a login not among the first few could not be reached from the keyboard at all.
        val room = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) request?.maxSuggestionCount ?: 0 else 0
        val loginChips = if (found.size < room) found.size else (room - 1).coerceAtLeast(0)
        var index = 0
        found.forEach { match ->
            val entry = match.entry
            val subtitle = entry.subtitle.ifEmpty { "WM Pass" }
            val view = row(context, entry.title, subtitle)
            val intent = AutofillActivity.intent(context, AutofillActivity.MODE_FILL)
                .putExtra(AutofillActivity.EXTRA_ID, entry.id)
                // The core asks for a confirming tap below a domain match: here, a package name with no certificate.
                .putExtra(AutofillActivity.EXTRA_UNVERIFIED, match.level == "unverified")
            @Suppress("DEPRECATION")
            val dataset = Dataset.Builder(view)
                .placeholders(ids, view, request?.takeIf { index < loginChips }?.let { inline(context, it, index, entry.title, subtitle) })
                .setAuthentication(sender(context, intent))
                .build()
            builder.addDataset(dataset)
            index++
        }
        val searchView = row(context, "Search WM Pass", "Choose a login for this form")
        @Suppress("DEPRECATION")
        builder.addDataset(
            Dataset.Builder(searchView)
                .placeholders(ids, searchView, request?.let { inline(context, it, minOf(index, loginChips), "Search WM Pass", "") })
                .setAuthentication(sender(context, AutofillActivity.intent(context, AutofillActivity.MODE_PICK)))
                .build(),
        )
        saveInfo(context, form)?.let(builder::setSaveInfo)
        return builder.build()
    }

    /**
     * What to watch for a save after the form is sent, if saving is on. A screen with a username
     * and no password is the first step of a login that asks for the password on the next screen:
     * on Android 10 and up its value is held over, with no save asked yet, and reaches the
     * password screen's save in [SaveRequest.getFillContexts]. Before Android 10 there is no way
     * to hold it, and asking to save a username alone would only be in the way.
     */
    private fun saveInfo(context: Context, form: ParsedForm): SaveInfo? {
        if (!com.wasimaster.wmpass.app.Prefs.of(context).autofillSave.value) return null
        val password = form.password
        if (password == null) {
            val username = form.username ?: return null
            if (form.otp != null || Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) return null
            return SaveInfo.Builder(SaveInfo.SAVE_DATA_TYPE_USERNAME, arrayOf(username))
                // Gone from view, as when the next screen or fragment takes its place, counts as sent.
                .setFlags(SaveInfo.FLAG_DELAY_SAVE or SaveInfo.FLAG_SAVE_ON_ALL_VIEWS_INVISIBLE)
                .build()
        }
        val withUser = form.username != null || form.usernameValue != null
        val save = SaveInfo.Builder(
            if (withUser) SaveInfo.SAVE_DATA_TYPE_USERNAME or SaveInfo.SAVE_DATA_TYPE_PASSWORD else SaveInfo.SAVE_DATA_TYPE_PASSWORD,
            arrayOf(password),
        )
        // The new password's box too: a change-password form sent with the current one filled
        // unchanged would otherwise raise no save, or save that box as it was when the page opened.
        val optional = listOfNotNull(form.username) + form.morePasswords
        if (optional.isNotEmpty()) save.setOptionalIds(optional.toTypedArray())
        // A login page that goes away, as a web page does when the site moves on, counts as sent.
        save.setFlags(SaveInfo.FLAG_SAVE_ON_ALL_VIEWS_INVISIBLE)
        return save.build()
    }
}

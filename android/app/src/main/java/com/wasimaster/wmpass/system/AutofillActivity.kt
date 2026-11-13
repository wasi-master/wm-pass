package com.wasimaster.wmpass.system

import android.app.assist.AssistStructure
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.service.autofill.Dataset
import android.view.autofill.AutofillManager
import android.view.autofill.AutofillValue
import android.view.inputmethod.InlineSuggestionsRequest
import android.widget.RemoteViews
import androidx.activity.compose.BackHandler
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.imePadding
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.core.Detail
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.SecureClipboard
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.ui.door.PasswordPill
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.SheetField
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.add
import kotlinx.serialization.json.addJsonObject
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/** Between a form in another app and the vault: open it, choose, confirm, save. */
class AutofillActivity : SecureActivity() {
    override val overlayUnlock = true

    private val form: ParsedForm? by lazy {
        val structure = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(AutofillManager.EXTRA_ASSIST_STRUCTURE, AssistStructure::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(AutofillManager.EXTRA_ASSIST_STRUCTURE)
        }
        // A form with no login field was offered because the person asked on a field by hand
        // (PassAutofillService): that field is the one filled, as it was there.
        structure?.let(::parseForm)?.let { parsed ->
            val field = parsed.focused
            if (!parsed.isLogin && field != null) parsed.withUsernameField(field) else parsed
        }
    }

    /** A page's domain reported by anything but a verified browser could be anything. */
    private val unverifiedDomain: Boolean
        get() = form?.let { it.webDomain != null && !Trust.mayNameDomain(this, it.packageName) } ?: false

    /** A login saved for this app's package name with no certificate, so any app of that name matches it. */
    private val unverifiedApp: Boolean
        get() = intent.getBooleanExtra(EXTRA_UNVERIFIED, false)

    /** The save this screen was opened for, so leaving it forgets that one and not a newer one. */
    private var shownSave: ParsedForm? = null

    override fun onDestroy() {
        // Backed out of rather than answered: the password sent with the form goes too, instead
        // of waiting in memory for the next save.
        if (isFinishing && shownSave != null && PendingSave.form === shownSave) PendingSave.clear()
        super.onDestroy()
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val mode = intent.getStringExtra(EXTRA_MODE)
        if (mode == MODE_SAVE) shownSave = PendingSave.form
        gated {
            when (mode) {
                MODE_UNLOCK -> LaunchedEffect(Unit) { finishWithResponse() }
                MODE_FILL -> FillStep(intent.getStringExtra(EXTRA_ID).orEmpty())
                MODE_PICK -> PickStep()
                MODE_SAVE -> SaveStep()
            }
        }
    }

    private suspend fun finishWithResponse() {
        val parsed = form ?: return finish()
        val inline = when {
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU ->
                intent.getParcelableExtra(AutofillManager.EXTRA_INLINE_SUGGESTIONS_REQUEST, InlineSuggestionsRequest::class.java)
            Build.VERSION.SDK_INT >= Build.VERSION_CODES.R ->
                @Suppress("DEPRECATION")
                intent.getParcelableExtra<InlineSuggestionsRequest>(AutofillManager.EXTRA_INLINE_SUGGESTIONS_REQUEST)
            else -> null
        }
        val response = runCatching { AutofillBuilder.response(this, parsed, inline) }.getOrNull() ?: return cancelFill()
        setResult(RESULT_OK, Intent().putExtra(AutofillManager.EXTRA_AUTHENTICATION_RESULT, response))
        finish()
    }

    private suspend fun finishWithEntry(id: String) {
        val parsed = form ?: return finish()
        // An item deleted since the suggestion was made, or a broken code, ends the fill quietly
        // instead of closing the app in the middle of it.
        val detail = runCatching { Pass.detail(id) }.getOrNull() ?: return cancelFill()
        val view = RemoteViews(packageName, R.layout.autofill_row)
        @Suppress("DEPRECATION")
        val dataset = Dataset.Builder(view)
        // Only what the item has: an empty value would wipe what the person already typed, such
        // as the email in front of a login saved with no username.
        var filled = 0
        parsed.username?.let { field ->
            val name = detail.fields.firstOrNull { it.key == "username" }?.text.orEmpty()
            if (name.isNotEmpty()) {
                @Suppress("DEPRECATION")
                dataset.setValue(field, AutofillValue.forText(name))
                filled++
            }
        }
        parsed.password?.let { field ->
            val secret = runCatching { Pass.reveal(id, "password").consumeText() }.getOrNull().orEmpty()
            if (secret.isNotEmpty()) {
                @Suppress("DEPRECATION")
                dataset.setValue(field, AutofillValue.forText(secret))
                filled++
            }
        }
        var codeFilled = false
        parsed.otp?.let { field ->
            val code = if (detail.otp != null) runCatching { codeToUse(id, detail.otp)?.consumeText() }.getOrNull() else null
            if (!code.isNullOrEmpty()) {
                @Suppress("DEPRECATION")
                dataset.setValue(field, AutofillValue.forText(code))
                codeFilled = true
                filled++
            }
        }
        // A site that asks for the code on the next screen will find it on the clipboard.
        if (detail.otp != null && !codeFilled && detail.otp?.period != null) {
            runCatching { SecureClipboard.copy(this, "Code", Pass.code(id)) }
        }
        if (filled == 0) return cancelFill()
        setResult(RESULT_OK, Intent().putExtra(AutofillManager.EXTRA_AUTHENTICATION_RESULT, dataset.build()))
        finish()
    }

    private fun cancelFill() {
        setResult(RESULT_CANCELED)
        finish()
    }

    /**
     * A web page inside an ordinary app's own WebView says whatever that app wants it to. So
     * filling there takes a tap that names both the login and the app that would receive it.
     */
    /** [onDecline], when the question came from the picker, goes back to its list rather than ending the fill. */
    @Composable
    private fun FillStep(id: String, onDecline: (() -> Unit)? = null) {
        if (onDecline != null) BackHandler(onBack = onDecline)
        val scope = rememberCoroutineScope()
        val colors = palette
        var detail by remember { mutableStateOf<Detail?>(null) }
        LaunchedEffect(id) {
            if (unverifiedDomain || unverifiedApp) detail = runCatching { Pass.detail(id) }.getOrNull() ?: return@LaunchedEffect cancelFill() else finishWithEntry(id)
        }
        val entry = detail ?: return
        val parsed = form
        val app = parsed?.let { Trust.labelOf(this, it.packageName) } ?: "this app"
        Panel {
            SiteTile(entry.title, entry.site, entry.kind, size = 64.dp, label = entry.color, icon = entry.icon)
            Spacer(Modifier.height(16.dp))
            Text("Fill “${entry.title}” in $app?", color = colors.text, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(8.dp))
            Text(
                if (parsed?.webDomain != null) {
                    "$app shows a page it says is ${parsed.webDomain}. Only a browser can vouch for that, so check that you trust $app with this password."
                } else {
                    "This login names the app ${parsed?.packageName.orEmpty()} but not its signature, so any app given that name would match it. Check that $app is the one you meant."
                },
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(20.dp))
            PrimaryButton("Fill it", Modifier.fillMaxWidth()) { scope.launch { finishWithEntry(id) } }
            Spacer(Modifier.height(10.dp))
            QuietButton("Do not fill", Modifier.fillMaxWidth()) { onDecline?.invoke() ?: finish() }
        }
    }

    @Composable
    private fun PickStep() {
        val colors = palette
        val scope = rememberCoroutineScope()
        val all by Pass.entries.collectAsStateWithLifecycle()
        val field = rememberTextFieldState()
        val query = field.text.toString().trim().lowercase()
        val parsed = form
        // A page an ordinary app's WebView claims to be gets the same question as a suggestion.
        var confirming by remember { mutableStateOf<String?>(null) }
        var filling by remember { mutableStateOf(false) }
        confirming?.let {
            FillStep(it, onDecline = { confirming = null })
            return
        }
        val shown = remember(all, query) {
            all.filter { it.kind == "login" && (query.isEmpty() || query in it.title.lowercase() || query in it.subtitle.lowercase() || query in it.site.orEmpty().lowercase()) }
        }
        Column(Modifier.fillMaxSize().background(colors.ground).statusBarsPadding().imePadding().navigationBarsPadding()) {
            Text(
                parsed?.let {
                    val app = Trust.labelOf(this@AutofillActivity, it.packageName)
                    when {
                        it.webDomain == null -> "Choose a login for $app"
                        unverifiedDomain -> "Choose a login for $app, which says it shows ${it.webDomain}"
                        else -> "Choose a login for ${it.webDomain}"
                    }
                } ?: "Choose a login",
                color = colors.text,
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 8.dp),
            )
            if (shown.isEmpty()) {
                // Said, rather than an empty page that looks as if the list is still coming.
                Box(Modifier.weight(1f).fillMaxWidth().padding(horizontal = 20.dp), contentAlignment = Alignment.BottomStart) {
                    Text(
                        if (query.isEmpty()) "No logins in this vault yet." else "No login matches “${field.text.toString().trim()}”.",
                        color = colors.textMid,
                        style = MaterialTheme.typography.bodyMedium,
                    )
                }
            }
            if (shown.isNotEmpty()) LazyColumn(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp), reverseLayout = true) {
                items(shown, key = { it.id }) { entry ->
                    PickRow(entry) {
                        if (unverifiedDomain) {
                            confirming = entry.id
                            return@PickRow
                        }
                        // A second tap while the first fills would link the app to the login twice.
                        if (filling) return@PickRow
                        filling = true
                        scope.launch {
                            // Teaching the app is a nicety: a failure there must not stop the fill.
                            if (parsed != null) runCatching { rememberApp(entry.id, parsed) }
                            finishWithEntry(entry.id)
                        }
                    }
                }
            }
            SheetField(field, "Search", Modifier.padding(16.dp))
        }
    }

    /** Choosing a login for an app teaches it: next time the app's own signature finds it. */
    private suspend fun rememberApp(id: String, parsed: ParsedForm) {
        if (parsed.webDomain != null) return
        // A browser whose page gave no domain is not the app the login is for: teaching it would
        // offer this login on every page that browser shows.
        if (Trust.mayNameDomain(this, parsed.packageName)) return
        val cert = Trust.certOf(this, parsed.packageName) ?: return
        val detail = Pass.detail(id)
        // By the exact name: "com.example" must not count as saved because "com.example.other" is.
        val already = detail.sites.any {
            ((it as? JsonObject)?.get("AndroidApp") as? JsonObject)?.get("package")?.jsonPrimitive?.content == parsed.packageName
        }
        if (already) return
        Pass.change("entry.common") {
            put("id", id)
            putJsonArray("sites") {
                detail.sites.forEach { add(it) }
                addJsonObject {
                    putJsonObject("AndroidApp") {
                        put("package", parsed.packageName)
                        put("cert_sha256", cert)
                    }
                }
            }
        }
    }

    @Composable
    private fun PickRow(entry: Summary, onClick: () -> Unit) {
        val colors = palette
        Row(
            Modifier
                .padding(horizontal = 16.dp)
                .fillMaxWidth()
                .clip(Shapes.cardSmall)
                .background(colors.surface1)
                .rim(Shapes.cardSmall, colors)
                .clickable(onClick = onClick)
                .padding(12.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            SiteTile(entry.title, entry.site, entry.kind, size = 44.dp, label = entry.color, icon = entry.icon)
            Spacer(Modifier.width(13.dp))
            Column(Modifier.weight(1f)) {
                Text(entry.title, color = colors.text, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold), maxLines = 1, overflow = TextOverflow.Ellipsis)
                if (entry.subtitle.isNotEmpty()) Text(entry.subtitle, color = colors.textMid, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis)
            }
        }
    }

    /**
     * After a form is sent: a new login, or a changed password on one already here. Everything
     * can be changed before it is kept, and an app with no website of its own is offered the
     * sites that go with it.
     */
    @OptIn(ExperimentalLayoutApi::class)
    @Composable
    private fun SaveStep() {
        val colors = palette
        val scope = rememberCoroutineScope()
        val parsed = PendingSave.form
        var candidates by remember { mutableStateOf<List<Summary>?>(null) }
        var offers by remember { mutableStateOf<List<String>>(emptyList()) }
        val appLabel = remember(parsed) { parsed?.takeIf { it.webDomain == null }?.let { Trust.labelOf(this, it.packageName) } }
        // A browser is not the app the login is for, and linking it would offer the login on every
        // page it shows. Any other app is linked, even one that says it shows a website: that is
        // how its forms are matched (AutofillBuilder.matches), since its word for the site is not.
        val linksApp = remember(parsed) { parsed != null && !Trust.mayNameDomain(this, parsed.packageName) }
        // A page named by anything but a verified browser is only that app's word for it.
        val claimedBy = remember(parsed) {
            parsed?.takeIf { it.webDomain != null && !Trust.mayNameDomain(this, it.packageName) }?.let { Trust.labelOf(this, it.packageName) }
        }
        val name = rememberTextFieldState(
            parsed?.webDomain?.let { siteName(it) } ?: appLabel.orEmpty(),
        )
        val user = rememberTextFieldState(parsed?.usernameValue.orEmpty())
        // Not saveable: a turned screen would put the password in the saved state.
        val password = remember { TextFieldState(parsed?.passwordValue.orEmpty()) }
        // Only a browser's page goes in as the website: another app's claim would put this login
        // on that site in every browser. The person can still type it.
        val website = rememberTextFieldState(parsed?.webDomain?.takeIf { claimedBy == null }?.removePrefix("www.").orEmpty())
        LaunchedEffect(Unit) {
            if (parsed == null) {
                finish()
            } else {
                // A match that fails leaves a new login to save, rather than closing the app.
                val found = runCatching { AutofillBuilder.matches(this@AutofillActivity, parsed) }.getOrDefault(emptyList()).map { it.entry }
                // An address typed with a capital letter is still the same account.
                val sameUser = found.filter { parsed.usernameValue == null || it.subtitle.trim().equals(parsed.usernameValue.trim(), ignoreCase = true) }
                // Filling a saved password and sending it unchanged is not a change.
                val same = sameUser.any { entry ->
                    runCatching { Pass.reveal(entry.id, "password").consume { String(it) == parsed.passwordValue } }.getOrDefault(false)
                }
                if (same) {
                    PendingSave.clear()
                    finish()
                } else {
                    candidates = sameUser
                    if (parsed.webDomain == null) {
                        offers = Related.find(this@AutofillActivity, "", emptyList(), listOf(parsed.packageName)).sites
                        if (website.text.isEmpty()) offers.firstOrNull()?.let { website.setTextAndPlaceCursorAtEnd(it) }
                    }
                }
            }
        }
        val list = candidates ?: return
        val where = parsed?.webDomain ?: appLabel.orEmpty()
        // One save at a time: a second tap while the first runs would make a second login.
        var saving by remember { mutableStateOf(false) }
        var problem by remember { mutableStateOf<String?>(null) }
        var created by remember { mutableStateOf<String?>(null) }
        fun saveWith(work: suspend () -> Unit) {
            if (saving) return
            saving = true
            problem = null
            scope.launch {
                try {
                    work()
                    Pass.flush()
                    PendingSave.clear()
                    finish()
                } catch (failure: Exception) {
                    if (failure is kotlinx.coroutines.CancellationException) throw failure
                    problem = failure.message ?: "The login could not be saved."
                    saving = false
                }
            }
        }
        val savePassword: suspend (String) -> Unit = { id ->
            val bytes = password.text.toString().toByteArray()
            // Wiped whether or not the save goes through: a failure shows a message and leaves the screen up.
            try {
                Pass.change("entry.set_secret", secret = bytes) {
                    put("id", id)
                    put("field", "password")
                }
            } finally {
                bytes.fill(0)
            }
        }
        Panel {
            Text(if (list.isEmpty()) "Save this login?" else "Update the saved password?", color = colors.text, style = MaterialTheme.typography.titleLarge)
            if (where.isNotEmpty()) {
                Spacer(Modifier.height(6.dp))
                Text(
                    when {
                        appLabel != null -> "From the $appLabel app"
                        claimedBy != null -> "From $claimedBy, which says it shows $where"
                        else -> "From $where"
                    },
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                )
            }
            Spacer(Modifier.height(16.dp))
            Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
                if (list.isEmpty()) SheetField(name, "Name")
                if (list.isEmpty()) SheetField(user, "Username or email")
                PasswordPill(password, "Password", imeAction = ImeAction.Next)
                if (list.isEmpty()) {
                    SheetField(website, "Website")
                    val more = offers.filter { it != website.text.toString().trim() }
                    if (more.isNotEmpty()) {
                        FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                            more.forEach { site -> Pill(site, selected = false) { website.setTextAndPlaceCursorAtEnd(site) } }
                        }
                    }
                    val linkedLabel = appLabel ?: claimedBy
                    if (linkedLabel != null && linksApp) {
                        Text("Linked to the $linkedLabel app, so it fills there too.", color = colors.textLow, style = MaterialTheme.typography.bodySmall)
                    }
                }
            }
            Spacer(Modifier.height(18.dp))
            list.forEach { entry ->
                PrimaryButton("Update ${entry.title}", Modifier.fillMaxWidth(), enabled = password.text.isNotEmpty() && !saving) {
                    saveWith { savePassword(entry.id) }
                }
                Spacer(Modifier.height(10.dp))
            }
            val saveNew: () -> Unit = {
                saveWith {
                    val form = parsed ?: return@saveWith
                    val title = name.text.toString().trim().ifEmpty { where.ifEmpty { "Login" } }
                    val site = website.text.toString().trim().lowercase()
                        .substringAfter("://").substringBefore('/').removePrefix("www.")
                    // A login made on a try whose password then failed to save is used again: a
                    // second tap must not leave two logins, one with no password.
                    val id = created ?: Pass.change("entry.create") {
                        put("kind", "login")
                        put("title", title)
                        put("fields", buildJsonObject { user.text.toString().trim().takeIf { it.isNotEmpty() }?.let { put("username", it) } })
                        putJsonArray("sites") {
                            if (site.isNotEmpty()) addJsonObject { put("Domain", site) }
                            if (linksApp) {
                                Trust.certOf(this@AutofillActivity, form.packageName)?.let { cert ->
                                    addJsonObject {
                                        putJsonObject("AndroidApp") {
                                            put("package", form.packageName)
                                            put("cert_sha256", cert)
                                        }
                                    }
                                }
                            }
                        }
                    }.obj["id"]!!.jsonPrimitive.content.also { created = it }
                    savePassword(id)
                }
            }
            if (list.isEmpty()) {
                PrimaryButton("Save", Modifier.fillMaxWidth(), enabled = password.text.isNotEmpty() && !saving, onClick = saveNew)
            } else {
                QuietButton("Save as a new login", Modifier.fillMaxWidth(), enabled = password.text.isNotEmpty() && !saving, onClick = saveNew)
            }
            problem?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, color = colors.danger, style = MaterialTheme.typography.bodySmall)
            }
            Spacer(Modifier.height(10.dp))
            QuietButton("Not now", Modifier.fillMaxWidth()) {
                PendingSave.clear()
                finish()
            }
        }
    }

    @Composable
    private fun Panel(content: @Composable () -> Unit) {
        val colors = palette
        // Clear of the status bar too: a save form on a short screen, or on its side, scrolls rather than running under it.
        Box(Modifier.fillMaxSize().background(colors.ground).statusBarsPadding().navigationBarsPadding().imePadding(), contentAlignment = Alignment.BottomCenter) {
            Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(22.dp)) { content() }
        }
    }

    companion object {
        const val MODE_UNLOCK = "unlock"
        const val MODE_FILL = "fill"
        const val MODE_PICK = "pick"
        const val MODE_SAVE = "save"
        private const val EXTRA_MODE = "mode"
        const val EXTRA_ID = "id"

        /** Set on a suggestion whose match is by package name alone, which needs a tap naming it. */
        const val EXTRA_UNVERIFIED = "unverified"

        fun intent(context: Context, mode: String): Intent =
            Intent(context, AutofillActivity::class.java).putExtra(EXTRA_MODE, mode)

        /** Labels that sit under a country's ending, as in bbc.co.uk, rather than name the site. */
        private val SecondLevel = setOf("co", "com", "net", "org", "gov", "ac", "edu", "ne", "or", "go")

        /**
         * A login's name from its page: "Google" for accounts.google.com, "Bbc" for www.bbc.co.uk.
         * The label before the ending, not the first one, which is so often "accounts" or "login".
         */
        internal fun siteName(host: String): String {
            // A router's page or the like, by its address: the whole address is its only name.
            if (host.all { it.isDigit() || it == '.' || it == ':' || it == '[' || it == ']' }) return host
            val parts = host.removePrefix("www.").split('.').filter { it.isNotEmpty() }
            val ending = if (parts.size >= 3 && parts[parts.size - 2] in SecondLevel && parts.last().length == 2) 2 else 1
            val name = parts.getOrNull(parts.size - 1 - ending) ?: parts.firstOrNull().orEmpty()
            return name.replaceFirstChar { it.uppercase() }
        }
    }
}

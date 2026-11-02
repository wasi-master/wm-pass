package com.wasimaster.wmpass.system

import android.app.assist.AssistStructure
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.service.autofill.Dataset
import android.view.autofill.AutofillManager
import android.view.autofill.AutofillValue
import android.widget.RemoteViews
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
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
import androidx.compose.foundation.text.input.rememberTextFieldState
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
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.R
import com.wasimaster.wmpass.core.Detail
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.SecureClipboard
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.ui.fx.rim
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
    private val form: ParsedForm? by lazy {
        val structure = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            intent.getParcelableExtra(AutofillManager.EXTRA_ASSIST_STRUCTURE, AssistStructure::class.java)
        } else {
            @Suppress("DEPRECATION")
            intent.getParcelableExtra(AutofillManager.EXTRA_ASSIST_STRUCTURE)
        }
        structure?.let(::parseForm)
    }

    /** A page's domain reported by anything but a verified browser could be anything. */
    private val unverifiedDomain: Boolean
        get() = form?.let { it.webDomain != null && !Trust.isVerifiedBrowser(this, it.packageName) } ?: false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val mode = intent.getStringExtra(EXTRA_MODE)
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
        val response = AutofillBuilder.response(this, parsed, null)
        setResult(RESULT_OK, Intent().putExtra(AutofillManager.EXTRA_AUTHENTICATION_RESULT, response))
        finish()
    }

    private suspend fun finishWithEntry(id: String) {
        val parsed = form ?: return finish()
        val detail = Pass.detail(id)
        val view = RemoteViews(packageName, R.layout.autofill_row)
        @Suppress("DEPRECATION")
        val dataset = Dataset.Builder(view)
        parsed.username?.let { field ->
            val name = detail.fields.firstOrNull { it.key == "username" }?.text.orEmpty()
            @Suppress("DEPRECATION")
            dataset.setValue(field, AutofillValue.forText(name))
        }
        parsed.password?.let { field ->
            val secret = runCatching { Pass.reveal(id, "password").consumeText() }.getOrNull().orEmpty()
            @Suppress("DEPRECATION")
            dataset.setValue(field, AutofillValue.forText(secret))
        }
        var codeFilled = false
        parsed.otp?.let { field ->
            if (detail.otp != null) {
                @Suppress("DEPRECATION")
                dataset.setValue(field, AutofillValue.forText(Pass.code(id).consumeText()))
                codeFilled = true
            }
        }
        // A site that asks for the code on the next screen will find it on the clipboard.
        if (detail.otp != null && !codeFilled && detail.otp?.period != null) {
            SecureClipboard.copy(this, "Code", Pass.code(id))
        }
        setResult(RESULT_OK, Intent().putExtra(AutofillManager.EXTRA_AUTHENTICATION_RESULT, dataset.build()))
        finish()
    }

    /**
     * A web page inside an ordinary app's own WebView says whatever that app wants it to. So
     * filling there takes a tap that names both the login and the app that would receive it.
     */
    @Composable
    private fun FillStep(id: String) {
        val scope = rememberCoroutineScope()
        val colors = palette
        var detail by remember { mutableStateOf<Detail?>(null) }
        LaunchedEffect(id) {
            if (unverifiedDomain) detail = Pass.detail(id) else finishWithEntry(id)
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
                "$app shows a page it says is ${parsed?.webDomain}. Only a browser can vouch for that, so check that you trust $app with this password.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(20.dp))
            PrimaryButton("Fill it", Modifier.fillMaxWidth()) { scope.launch { finishWithEntry(id) } }
            Spacer(Modifier.height(10.dp))
            QuietButton("Do not fill", Modifier.fillMaxWidth()) { finish() }
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
        val shown = remember(all, query) {
            all.filter { it.kind == "login" && (query.isEmpty() || query in it.title.lowercase() || query in it.subtitle.lowercase() || query in it.site.orEmpty()) }
        }
        Column(Modifier.fillMaxSize().background(colors.ground).statusBarsPadding().imePadding().navigationBarsPadding()) {
            Text(
                parsed?.let { "Choose a login for ${it.webDomain ?: Trust.labelOf(this@AutofillActivity, it.packageName)}" } ?: "Choose a login",
                color = colors.text,
                style = MaterialTheme.typography.titleLarge,
                modifier = Modifier.padding(start = 20.dp, end = 20.dp, top = 20.dp, bottom = 8.dp),
            )
            LazyColumn(Modifier.weight(1f), verticalArrangement = Arrangement.spacedBy(8.dp), reverseLayout = true) {
                items(shown, key = { it.id }) { entry ->
                    PickRow(entry) {
                        scope.launch {
                            if (parsed != null) rememberApp(entry.id, parsed)
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
        val cert = Trust.certOf(this, parsed.packageName) ?: return
        val detail = Pass.detail(id)
        val already = detail.sites.any { (it as? JsonObject)?.get("AndroidApp") != null && it.toString().contains(parsed.packageName) }
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

    /** After a form is sent: a new login, or a changed password on one already here. */
    @Composable
    private fun SaveStep() {
        val colors = palette
        val scope = rememberCoroutineScope()
        val parsed = PendingSave.form
        var candidates by remember { mutableStateOf<List<Summary>?>(null) }
        LaunchedEffect(Unit) {
            if (parsed == null) {
                finish()
            } else {
                val found = AutofillBuilder.matches(this@AutofillActivity, parsed).map { it.entry }
                val sameUser = found.filter { parsed.usernameValue == null || it.subtitle == parsed.usernameValue }
                // Filling a saved password and sending it unchanged is not a change.
                val same = sameUser.any { entry ->
                    runCatching { Pass.reveal(entry.id, "password").consume { String(it) == parsed.passwordValue } }.getOrDefault(false)
                }
                if (same) {
                    PendingSave.clear()
                    finish()
                } else {
                    candidates = sameUser
                }
            }
        }
        val list = candidates ?: return
        val where = parsed?.webDomain ?: parsed?.let { Trust.labelOf(this, it.packageName) }.orEmpty()
        Panel {
            Text(if (list.isEmpty()) "Save this login?" else "Update the saved password?", color = colors.text, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(6.dp))
            Text(
                listOfNotNull(parsed?.usernameValue, where.takeIf { it.isNotEmpty() }).joinToString(" on "),
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
            )
            Spacer(Modifier.height(18.dp))
            list.forEach { entry ->
                PrimaryButton("Update ${entry.title}", Modifier.fillMaxWidth()) {
                    scope.launch {
                        val bytes = parsed?.passwordValue.orEmpty().toByteArray()
                        Pass.change("entry.set_secret", secret = bytes) {
                            put("id", entry.id)
                            put("field", "password")
                        }
                        bytes.fill(0)
                        Pass.flush()
                        PendingSave.clear()
                        finish()
                    }
                }
                Spacer(Modifier.height(10.dp))
            }
            val saveNew: () -> Unit = {
                scope.launch {
                    val form = parsed ?: return@launch
                    val title = form.webDomain?.removePrefix("www.")?.substringBefore('.')?.replaceFirstChar { it.uppercase() }
                        ?: Trust.labelOf(this@AutofillActivity, form.packageName)
                    val id = Pass.change("entry.create") {
                        put("kind", "login")
                        put("title", title)
                        put("fields", buildJsonObject { form.usernameValue?.let { put("username", it) } })
                        putJsonArray("sites") {
                            val domain = form.webDomain
                            if (domain != null) {
                                addJsonObject { put("Domain", domain.removePrefix("www.")) }
                            } else {
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
                    }.obj["id"]!!.jsonPrimitive.content
                    val bytes = form.passwordValue.orEmpty().toByteArray()
                    Pass.change("entry.set_secret", secret = bytes) {
                        put("id", id)
                        put("field", "password")
                    }
                    bytes.fill(0)
                    Pass.flush()
                    PendingSave.clear()
                    finish()
                }
            }
            if (list.isEmpty()) PrimaryButton("Save", Modifier.fillMaxWidth(), onClick = saveNew) else QuietButton("Save as a new login", Modifier.fillMaxWidth(), onClick = saveNew)
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
        Box(Modifier.fillMaxSize().background(colors.ground).navigationBarsPadding(), contentAlignment = Alignment.BottomCenter) {
            Column(Modifier.fillMaxWidth().padding(22.dp)) { content() }
        }
    }

    companion object {
        const val MODE_UNLOCK = "unlock"
        const val MODE_FILL = "fill"
        const val MODE_PICK = "pick"
        const val MODE_SAVE = "save"
        private const val EXTRA_MODE = "mode"
        const val EXTRA_ID = "id"

        fun intent(context: Context, mode: String): Intent =
            Intent(context, AutofillActivity::class.java).putExtra(EXTRA_MODE, mode)
    }
}

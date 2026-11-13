package com.wasimaster.wmpass.system

import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.os.CancellationSignal
import android.os.OutcomeReceiver
import android.util.Base64
import androidx.annotation.RequiresApi
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.navigationBarsPadding
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.statusBarsPadding
import androidx.compose.foundation.rememberScrollState
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
import androidx.compose.ui.unit.dp
import androidx.credentials.CreatePublicKeyCredentialRequest
import androidx.credentials.CreatePublicKeyCredentialResponse
import androidx.credentials.GetCredentialResponse
import androidx.credentials.GetPublicKeyCredentialOption
import androidx.credentials.PublicKeyCredential
import androidx.credentials.exceptions.ClearCredentialException
import androidx.credentials.exceptions.CreateCredentialCancellationException
import androidx.credentials.exceptions.CreateCredentialException
import androidx.credentials.exceptions.GetCredentialCancellationException
import androidx.credentials.exceptions.GetCredentialException
import androidx.credentials.provider.AuthenticationAction
import androidx.credentials.provider.BeginCreateCredentialRequest
import androidx.credentials.provider.BeginCreateCredentialResponse
import androidx.credentials.provider.BeginCreatePublicKeyCredentialRequest
import androidx.credentials.provider.BeginGetCredentialRequest
import androidx.credentials.provider.BeginGetCredentialResponse
import androidx.credentials.provider.BeginGetPublicKeyCredentialOption
import androidx.credentials.provider.CallingAppInfo
import androidx.credentials.provider.CreateEntry
import androidx.credentials.provider.CredentialEntry
import androidx.credentials.provider.CredentialProviderService
import androidx.credentials.provider.PendingIntentHandler
import androidx.credentials.provider.ProviderClearCredentialStateRequest
import androidx.credentials.provider.PublicKeyCredentialEntry
import com.wasimaster.wmpass.app.AutoLock
import com.wasimaster.wmpass.core.LockState
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.palette
import java.net.URI
import java.security.MessageDigest
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.launch
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.add
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

private fun b64url(bytes: ByteArray) = Base64.encodeToString(bytes, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)
private fun fromB64url(text: String) = Base64.decode(text, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)
private fun std(bytes: ByteArray) = Base64.encodeToString(bytes, Base64.NO_WRAP)
private fun fromStd(text: String) = Base64.decode(text, Base64.DEFAULT)

/** One spelling for base64 written either way, url-safe or standard, padded or not, to compare ids. */
private fun sameBase64(text: String) = text.trim().replace('+', '-').replace('/', '_').trimEnd('=')

/** Where a request says it comes from, and whether that can be believed. */
private class Origin(val origin: String, val host: String?, val verified: Boolean)

@RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
private fun originOf(context: Context, info: CallingAppInfo): Origin {
    // getOrigin returns an origin only for a browser on the allowlist, signed as listed.
    val stated = runCatching { info.getOrigin(Trust.privilegedJson(context)) }.getOrNull()
    if (stated != null) {
        return Origin(stated, runCatching { URI(stated).host }.getOrNull(), true)
    }
    // Anything else speaks for itself only, as the platform's own origin format does.
    val cert = Trust.certOf(context, info.packageName).orEmpty()
    val raw = cert.chunked(2).map { it.toInt(16).toByte() }.toByteArray()
    return Origin("android:apk-key-hash:${b64url(raw)}", null, false)
}

/** One per process, so no two pending intents handed out share a request code. */
private var passkeyCode = 500

@RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
private fun passkeyIntent(context: Context, action: String, extras: Bundle = Bundle()): PendingIntent = PendingIntent.getActivity(
    context,
    passkeyCode++,
    Intent(context, PasskeyActivity::class.java).setAction(action).putExtras(extras).setPackage(context.packageName),
    PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
)

/** The passkeys the open vault holds for [request], one entry each. */
@RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
private suspend fun passkeyEntries(context: Context, request: BeginGetCredentialRequest): List<CredentialEntry> {
    val entries = mutableListOf<CredentialEntry>()
    val info = request.callingAppInfo
    for (option in request.beginGetCredentialOptions) {
        if (option !is BeginGetPublicKeyCredentialOption) continue
        val json = runCatching { Json.parseToJsonElement(option.requestJson).jsonObject }.getOrNull() ?: continue
        val origin = info?.let { originOf(context, it) }
        // A request that names no site is for the page's own, as WebAuthn has it; only a browser
        // vouching for its page can say what that is.
        val rp = runCatching { json["rpId"]?.jsonPrimitive?.contentOrNull }.getOrNull()?.takeIf { it.isNotEmpty() }
            ?: origin?.takeIf { it.verified }?.host ?: continue
        // A site that names the passkeys it accepts, as after a username is typed, takes only those:
        // offering another would end in the site turning it down.
        val allowed = runCatching {
            json["allowCredentials"]?.jsonArray?.mapNotNull { it.jsonObject["id"]?.jsonPrimitive?.contentOrNull?.let(::sameBase64) }?.toSet()
        }.getOrNull().orEmpty()
        val rows = runCatching {
            Pass.plainCall("passkey.list") {
                put("rp_id", rp)
                put("origin_host", origin?.host ?: rp)
            }.json.jsonArray
        }.getOrNull().orEmpty()
        rows.forEach { row ->
            // A row in a shape not expected is left out, rather than costing every other passkey.
            val fields = row as? JsonObject ?: return@forEach
            val entryId = fields["id"]?.jsonPrimitive?.contentOrNull ?: return@forEach
            val rawCredential = fields["credential_id"]?.jsonPrimitive?.contentOrNull ?: return@forEach
            val credentialId = sameBase64(rawCredential)
            if (allowed.isNotEmpty() && credentialId !in allowed) return@forEach
            val extras = Bundle().apply {
                putString(PasskeyService.EXTRA_ENTRY, entryId)
                putString(PasskeyService.EXTRA_CREDENTIAL, rawCredential)
            }
            entries += PublicKeyCredentialEntry(
                context,
                fields["user_name"]?.jsonPrimitive?.contentOrNull.orEmpty()
                    .ifEmpty { fields["title"]?.jsonPrimitive?.contentOrNull.orEmpty() }
                    .ifEmpty { rp },
                passkeyIntent(context, PasskeyService.ACTION_GET, extras),
                option,
            )
        }
    }
    return entries
}

/**
 * Passkeys for other apps and browsers. Android 14 and up. Private keys stay in the vault, so a
 * locked vault offers nothing but the way to unlock it.
 */
@RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
class PasskeyService : CredentialProviderService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)

    private fun pending(action: String, extras: Bundle = Bundle()): PendingIntent = passkeyIntent(this, action, extras)

    override fun onBeginCreateCredentialRequest(
        request: BeginCreateCredentialRequest,
        cancellationSignal: CancellationSignal,
        callback: OutcomeReceiver<BeginCreateCredentialResponse, CreateCredentialException>,
    ) {
        if (request !is BeginCreatePublicKeyCredentialRequest) {
            callback.onResult(BeginCreateCredentialResponse())
            return
        }
        callback.onResult(
            BeginCreateCredentialResponse(listOf(CreateEntry("WM Pass", pending(ACTION_CREATE)))),
        )
    }

    override fun onBeginGetCredentialRequest(
        request: BeginGetCredentialRequest,
        cancellationSignal: CancellationSignal,
        callback: OutcomeReceiver<BeginGetCredentialResponse, GetCredentialException>,
    ) {
        // Only passkeys live here: a request for a password alone gets no unlock entry to tap for nothing.
        if (request.beginGetCredentialOptions.none { it is BeginGetPublicKeyCredentialOption }) {
            callback.onResult(BeginGetCredentialResponse())
            return
        }
        scope.launch {
            // The vault's background time may have run out while the phone slept, as autofill checks too.
            // A lock that fails offers nothing, rather than passkeys from a vault meant to be shut.
            if (runCatching { AutoLock.lockIfOverdue() }.isFailure) {
                callback.onResult(BeginGetCredentialResponse())
                return@launch
            }
            // No vault at all: an unlock entry would lead nowhere.
            if (Pass.state.value == LockState.NoVault) {
                callback.onResult(BeginGetCredentialResponse())
                return@launch
            }
            if (Pass.state.value !is LockState.Unlocked) {
                callback.onResult(
                    BeginGetCredentialResponse(authenticationActions = listOf(AuthenticationAction("Unlock WM Pass", pending(ACTION_UNLOCK)))),
                )
                return@launch
            }
            // A row the core sent in a shape not expected costs the list, not the whole process.
            val entries = runCatching { passkeyEntries(this@PasskeyService, request) }.getOrDefault(emptyList())
            callback.onResult(BeginGetCredentialResponse(entries))
        }
    }

    override fun onClearCredentialStateRequest(
        request: ProviderClearCredentialStateRequest,
        cancellationSignal: CancellationSignal,
        callback: OutcomeReceiver<Void?, ClearCredentialException>,
    ) {
        callback.onResult(null)
    }

    companion object {
        const val ACTION_CREATE = "com.wasimaster.wmpass.PASSKEY_CREATE"
        const val ACTION_GET = "com.wasimaster.wmpass.PASSKEY_GET"
        const val ACTION_UNLOCK = "com.wasimaster.wmpass.PASSKEY_UNLOCK"
        const val EXTRA_ENTRY = "entry"
        const val EXTRA_CREDENTIAL = "credential"
    }
}

/**
 * The approval step for a passkey. "User verified" is claimed only after a real biometric or
 * device check here, whatever state the vault is already in.
 */
@RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
class PasskeyActivity : SecureActivity() {
    override val overlayUnlock = true

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        gated {
            when (intent.action) {
                PasskeyService.ACTION_CREATE -> CreateStep()
                PasskeyService.ACTION_GET -> GetStep()
                // Unlocked from the system's sheet: it now lists what the open vault holds for the request.
                else -> LaunchedEffect(Unit) {
                    val begin = PendingIntentHandler.retrieveBeginGetCredentialRequest(intent)
                    val entries = begin?.let { runCatching { passkeyEntries(this@PasskeyActivity, it) }.getOrNull() }.orEmpty()
                    val response = BeginGetCredentialResponse(entries)
                    val out = Intent()
                    PendingIntentHandler.setBeginGetCredentialResponse(out, response)
                    setResult(RESULT_OK, out)
                    finish()
                }
            }
        }
    }

    /**
     * Asks the phone to confirm the person, then runs [then] with whether it did. A prompt waved
     * away runs [onCancel] instead: that is a no, not a go-ahead without the check.
     */
    private fun verifyPerson(title: String, onCancel: () -> Unit = {}, then: (Boolean) -> Unit) {
        // The fingerprint that opened the vault for this very request was the check.
        if (justVerified) return then(true)
        val can = BiometricManager.from(this).canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG or BiometricManager.Authenticators.DEVICE_CREDENTIAL)
        if (can != BiometricManager.BIOMETRIC_SUCCESS) return then(false)
        BiometricPrompt(
            this,
            mainExecutor,
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) = then(true)
                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) = when (errorCode) {
                    BiometricPrompt.ERROR_USER_CANCELED, BiometricPrompt.ERROR_CANCELED, BiometricPrompt.ERROR_NEGATIVE_BUTTON -> onCancel()
                    // A lockout or a sensor that cannot be used: on, without claiming the check.
                    else -> then(false)
                }
            },
        ).authenticate(
            BiometricPrompt.PromptInfo.Builder()
                .setTitle(title)
                .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG or BiometricManager.Authenticators.DEVICE_CREDENTIAL)
                .build(),
        )
    }

    private fun clientData(kind: String, challenge: String, origin: String): ByteArray =
        buildJsonObject {
            put("type", kind)
            put("challenge", challenge)
            put("origin", origin)
            put("crossOrigin", false)
        }.toString().toByteArray()

    private fun cancelCreate() {
        val out = Intent()
        PendingIntentHandler.setCreateCredentialException(out, CreateCredentialCancellationException("Not approved"))
        setResult(RESULT_OK, out)
        finish()
    }

    /** WebAuthn's answer for a passkey the account already has here, so the site can say so. */
    private fun refuseCreateAsDuplicate() {
        val out = Intent()
        PendingIntentHandler.setCreateCredentialException(
            out,
            androidx.credentials.exceptions.publickeycredential.CreatePublicKeyCredentialDomException(
                androidx.credentials.exceptions.domerrors.InvalidStateError(),
                "A passkey for this account is already saved in WM Pass.",
            ),
        )
        setResult(RESULT_OK, out)
        finish()
    }

    private fun cancelGet() {
        val out = Intent()
        PendingIntentHandler.setGetCredentialException(out, GetCredentialCancellationException("Not approved"))
        setResult(RESULT_OK, out)
        finish()
    }

    @Composable
    private fun CreateStep() {
        val colors = palette
        val scope = rememberCoroutineScope()
        val request = remember { PendingIntentHandler.retrieveProviderCreateCredentialRequest(intent) }
        val call = request?.callingRequest as? CreatePublicKeyCredentialRequest
        if (request == null || call == null) {
            LaunchedEffect(Unit) { cancelCreate() }
            return
        }
        val json = remember { runCatching { Json.parseToJsonElement(call.requestJson) as? JsonObject }.getOrNull() }
        val rp = json?.get("rp") as? JsonObject
        val user = json?.get("user") as? JsonObject
        // A request with no site or no account is turned down, rather than closing the app on it.
        if (json == null || rp == null || user == null) {
            LaunchedEffect(Unit) { cancelCreate() }
            return
        }
        val origin = remember { originOf(this, request.callingAppInfo) }
        // Left out, the site is the page's own (WebAuthn); only a browser vouching for its page can say which.
        val rpId = rp["id"]?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotEmpty() } ?: origin.host?.takeIf { origin.verified }
        if (rpId == null) {
            LaunchedEffect(Unit) { cancelCreate() }
            return
        }
        val app = remember { Trust.labelOf(this, request.callingAppInfo.packageName) }
        var error by remember { mutableStateOf<String?>(null) }
        // One at a time: a second tap while the first is checked or saved would make a second passkey.
        var busy by remember { mutableStateOf(false) }
        // A site lists the passkeys the account has already. One of them here means this account has
        // its passkey in WM Pass, and a second would only be one more to choose between.
        var already by remember { mutableStateOf(false) }
        LaunchedEffect(rpId) {
            val excluded = runCatching {
                (json["excludeCredentials"] as? kotlinx.serialization.json.JsonArray)
                    ?.mapNotNull { (it as? JsonObject)?.get("id")?.jsonPrimitive?.contentOrNull?.let(::sameBase64) }?.toSet()
            }.getOrNull().orEmpty()
            if (excluded.isEmpty()) return@LaunchedEffect
            val held = runCatching {
                Pass.plainCall("passkey.list") {
                    put("rp_id", rpId)
                    put("origin_host", origin.host ?: rpId)
                }.json.jsonArray.mapNotNull { (it as? JsonObject)?.get("credential_id")?.jsonPrimitive?.contentOrNull?.let(::sameBase64) }
            }.getOrDefault(emptyList())
            already = held.any { it in excluded }
        }
        if (already) {
            Sheet {
                Text("This account already has a passkey in WM Pass", color = colors.text, style = MaterialTheme.typography.titleLarge)
                Spacer(Modifier.height(6.dp))
                Text("$rpId can sign you in with the passkey saved before, so there is nothing new to save.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
                Spacer(Modifier.height(18.dp))
                PrimaryButton("Close", Modifier.fillMaxWidth()) { refuseCreateAsDuplicate() }
            }
            return
        }
        Sheet {
            Text("Save a passkey for $rpId?", color = colors.text, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(6.dp))
            Text(
                listOf(user["name"]?.jsonPrimitive?.content.orEmpty(), if (origin.verified) "Asked by $app" else "$app is asking, and only it can say it belongs to $rpId").joinToString("\n"),
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
            )
            error?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium)
            }
            Spacer(Modifier.height(18.dp))
            PrimaryButton("Save passkey", Modifier.fillMaxWidth(), enabled = !busy) {
                busy = true
                verifyPerson("Save a passkey for $rpId", onCancel = { busy = false }) { verified ->
                    scope.launch {
                        try {
                            val reply = Pass.plainCall("passkey.create") {
                                put("rp_id", rpId)
                                put("rp_name", rp["name"]?.jsonPrimitive?.content.orEmpty())
                                put("user_handle", std(fromB64url(user["id"]!!.jsonPrimitive.content)))
                                put("user_name", user["name"]?.jsonPrimitive?.content.orEmpty())
                                put("user_verified", verified)
                                put("origin_host", origin.host ?: rpId)
                            }.obj
                            Pass.flush()
                            val challenge = json["challenge"]!!.jsonPrimitive.content
                            val data = clientData("webauthn.create", challenge, origin.origin)
                            val credential = b64url(fromStd(reply["credential_id"]!!.jsonPrimitive.content))
                            val response = buildJsonObject {
                                put("id", credential)
                                put("rawId", credential)
                                put("type", "public-key")
                                put("authenticatorAttachment", "platform")
                                putJsonObject("clientExtensionResults") {}
                                putJsonObject("response") {
                                    put("clientDataJSON", b64url(data))
                                    put("attestationObject", b64url(fromStd(reply["attestation_object"]!!.jsonPrimitive.content)))
                                    put("authenticatorData", b64url(fromStd(reply["authenticator_data"]!!.jsonPrimitive.content)))
                                    put("publicKey", b64url(fromStd(reply["public_key_der"]!!.jsonPrimitive.content)))
                                    put("publicKeyAlgorithm", -7)
                                    putJsonArray("transports") { add("internal") }
                                }
                            }
                            val out = Intent()
                            PendingIntentHandler.setCreateCredentialResponse(out, CreatePublicKeyCredentialResponse(response.toString()))
                            setResult(RESULT_OK, out)
                            finish()
                        } catch (failure: NativeFailure) {
                            error = failure.message
                            busy = false
                        } catch (cancelled: kotlinx.coroutines.CancellationException) {
                            throw cancelled
                        } catch (_: Exception) {
                            // A request the site put together wrong says so here, rather than closing the sheet.
                            error = "This request could not be answered."
                            busy = false
                        }
                    }
                }
            }
            Spacer(Modifier.height(10.dp))
            QuietButton("Not now", Modifier.fillMaxWidth()) { cancelCreate() }
        }
    }

    @Composable
    private fun GetStep() {
        val colors = palette
        val scope = rememberCoroutineScope()
        val request = remember { PendingIntentHandler.retrieveProviderGetCredentialRequest(intent) }
        val option = request?.credentialOptions?.filterIsInstance<GetPublicKeyCredentialOption>()?.firstOrNull()
        val entry = intent.getStringExtra(PasskeyService.EXTRA_ENTRY)
        val credential = intent.getStringExtra(PasskeyService.EXTRA_CREDENTIAL)
        if (request == null || option == null || entry == null || credential == null) {
            LaunchedEffect(Unit) { cancelGet() }
            return
        }
        // A request the site put together wrong is turned down, as one to save a passkey is.
        val json = remember { runCatching { Json.parseToJsonElement(option.requestJson) as? JsonObject }.getOrNull() }
        if (json == null) {
            LaunchedEffect(Unit) { cancelGet() }
            return
        }
        val origin = remember { originOf(this, request.callingAppInfo) }
        val rpId = json["rpId"]?.jsonPrimitive?.contentOrNull?.takeIf { it.isNotEmpty() } ?: origin.host.orEmpty()
        val app = remember { Trust.labelOf(this, request.callingAppInfo.packageName) }
        var error by remember { mutableStateOf<String?>(null) }
        // One sign-in at a time, however often the button is tapped.
        var busy by remember { mutableStateOf(false) }
        fun sign() {
            if (busy) return
            busy = true
            // Started straight from the system's sheet there is no sheet here to go back to.
            verifyPerson("Sign in to $rpId", onCancel = { if (origin.verified) cancelGet() else { busy = false } }) { verified ->
                scope.launch {
                    try {
                        val challenge = json["challenge"]!!.jsonPrimitive.content
                        val data = clientData("webauthn.get", challenge, origin.origin)
                        // Only a verified browser may hand over a hash of its own: from any other app it
                        // could hide client data naming another site's origin, which the signature would vouch for.
                        val hash = option.clientDataHash?.takeIf { origin.verified } ?: MessageDigest.getInstance("SHA-256").digest(data)
                        val reply = Pass.plainCall("passkey.sign") {
                            put("id", entry)
                            put("credential_id", credential)
                            put("client_data_hash", std(hash))
                            put("user_verified", verified)
                            put("origin_host", origin.host ?: rpId)
                        }.obj
                        Pass.flush()
                        val id = b64url(fromStd(reply["credential_id"]!!.jsonPrimitive.content))
                        val response = buildJsonObject {
                            put("id", id)
                            put("rawId", id)
                            put("type", "public-key")
                            put("authenticatorAttachment", "platform")
                            putJsonObject("clientExtensionResults") {}
                            putJsonObject("response") {
                                put("clientDataJSON", b64url(data))
                                put("authenticatorData", b64url(fromStd(reply["authenticator_data"]!!.jsonPrimitive.content)))
                                put("signature", b64url(fromStd(reply["signature"]!!.jsonPrimitive.content)))
                                put("userHandle", b64url(fromStd(reply["user_handle"]!!.jsonPrimitive.content)))
                            }
                        }
                        val out = Intent()
                        PendingIntentHandler.setGetCredentialResponse(out, GetCredentialResponse(PublicKeyCredential(response.toString())))
                        setResult(RESULT_OK, out)
                        finish()
                    } catch (failure: NativeFailure) {
                        error = failure.message ?: "The passkey could not sign in."
                        busy = false
                    } catch (cancelled: kotlinx.coroutines.CancellationException) {
                        throw cancelled
                    } catch (_: Exception) {
                        error = "This request could not be answered."
                        busy = false
                    }
                }
            }
        }
        // A browser vouches for the site, so the system's own sheet was approval enough to ask
        // for the check. An app vouching only for itself has to be named first. A sign-in that
        // failed shows the sheet after all, since an empty window would otherwise sit there.
        if (origin.verified && error == null) {
            LaunchedEffect(Unit) { sign() }
            return
        }
        Sheet {
            Text("Sign in to $rpId from $app?", color = colors.text, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(6.dp))
            if (!origin.verified) {
                Text("Only $app can say it belongs to $rpId. Go on only if you trust it.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            }
            error?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium)
            }
            Spacer(Modifier.height(18.dp))
            PrimaryButton("Sign in", Modifier.fillMaxWidth(), enabled = !busy) { sign() }
            Spacer(Modifier.height(10.dp))
            QuietButton("Not now", Modifier.fillMaxWidth()) { cancelGet() }
        }
    }

    @Composable
    private fun Sheet(content: @Composable () -> Unit) {
        val colors = palette
        Box(Modifier.fillMaxSize().background(colors.ground).statusBarsPadding().navigationBarsPadding(), contentAlignment = Alignment.BottomCenter) {
            // Scrolls when a long site name and an error leave a short screen too little room.
            Column(Modifier.fillMaxWidth().verticalScroll(rememberScrollState()).padding(22.dp)) { content() }
        }
    }
}

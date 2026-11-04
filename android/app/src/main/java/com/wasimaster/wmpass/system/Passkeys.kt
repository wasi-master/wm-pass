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

/**
 * Passkeys for other apps and browsers. Android 14 and up. Private keys stay in the vault, so a
 * locked vault offers nothing but the way to unlock it.
 */
@RequiresApi(Build.VERSION_CODES.UPSIDE_DOWN_CAKE)
class PasskeyService : CredentialProviderService() {
    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.Main.immediate)
    private var code = 500

    private fun pending(action: String, extras: Bundle = Bundle()): PendingIntent = PendingIntent.getActivity(
        this,
        code++,
        Intent(this, PasskeyActivity::class.java).setAction(action).putExtras(extras).setPackage(packageName),
        PendingIntent.FLAG_MUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
    )

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
        if (Pass.state.value !is LockState.Unlocked) {
            callback.onResult(
                BeginGetCredentialResponse(authenticationActions = listOf(AuthenticationAction("Unlock WM Pass", pending(ACTION_UNLOCK)))),
            )
            return
        }
        scope.launch {
            val entries = mutableListOf<CredentialEntry>()
            val info = request.callingAppInfo
            for (option in request.beginGetCredentialOptions) {
                if (option !is BeginGetPublicKeyCredentialOption) continue
                val rp = runCatching { Json.parseToJsonElement(option.requestJson).jsonObject["rpId"]?.jsonPrimitive?.content }.getOrNull() ?: continue
                val origin = info?.let { originOf(this@PasskeyService, it) }
                val rows = runCatching {
                    Pass.plainCall("passkey.list") {
                        put("rp_id", rp)
                        put("origin_host", origin?.host ?: rp)
                    }.json.jsonArray
                }.getOrNull().orEmpty()
                rows.forEach { row ->
                    val fields = row.jsonObject
                    val extras = Bundle().apply {
                        putString(EXTRA_ENTRY, fields["id"]!!.jsonPrimitive.content)
                        putString(EXTRA_CREDENTIAL, fields["credential_id"]!!.jsonPrimitive.content)
                    }
                    entries += PublicKeyCredentialEntry(
                        this@PasskeyService,
                        fields["user_name"]?.jsonPrimitive?.contentOrNull.orEmpty().ifEmpty { fields["title"]!!.jsonPrimitive.content },
                        pending(ACTION_GET, extras),
                        option,
                    )
                }
            }
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
                else -> LaunchedEffect(Unit) {
                    val response = BeginGetCredentialResponse()
                    val out = Intent()
                    PendingIntentHandler.setBeginGetCredentialResponse(out, response)
                    setResult(RESULT_OK, out)
                    finish()
                }
            }
        }
    }

    /** Asks the phone to confirm the person, then runs [then] with whether it did. */
    private fun verifyPerson(title: String, then: (Boolean) -> Unit) {
        // The fingerprint that opened the vault for this very request was the check.
        if (justVerified) return then(true)
        val can = BiometricManager.from(this).canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG or BiometricManager.Authenticators.DEVICE_CREDENTIAL)
        if (can != BiometricManager.BIOMETRIC_SUCCESS) return then(false)
        BiometricPrompt(
            this,
            mainExecutor,
            object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(result: BiometricPrompt.AuthenticationResult) = then(true)
                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) = then(false)
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
        val json = remember { Json.parseToJsonElement(call.requestJson).jsonObject }
        val rp = json["rp"]!!.jsonObject
        val rpId = rp["id"]?.jsonPrimitive?.content.orEmpty()
        val user = json["user"]!!.jsonObject
        val origin = remember { originOf(this, request.callingAppInfo) }
        val app = remember { Trust.labelOf(this, request.callingAppInfo.packageName) }
        var error by remember { mutableStateOf<String?>(null) }
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
            PrimaryButton("Save passkey", Modifier.fillMaxWidth()) {
                verifyPerson("Save a passkey for $rpId") { verified ->
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
        val json = remember { Json.parseToJsonElement(option.requestJson).jsonObject }
        val rpId = json["rpId"]?.jsonPrimitive?.content.orEmpty()
        val origin = remember { originOf(this, request.callingAppInfo) }
        val app = remember { Trust.labelOf(this, request.callingAppInfo.packageName) }
        var error by remember { mutableStateOf<String?>(null) }
        fun sign() {
            verifyPerson("Sign in to $rpId") { verified ->
                scope.launch {
                    try {
                        val challenge = json["challenge"]!!.jsonPrimitive.content
                        val data = clientData("webauthn.get", challenge, origin.origin)
                        val hash = option.clientDataHash ?: MessageDigest.getInstance("SHA-256").digest(data)
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
                        error = failure.message
                    }
                }
            }
        }
        // A browser vouches for the site, so the system's own sheet was approval enough to ask
        // for the check. An app vouching only for itself has to be named first.
        if (origin.verified) {
            LaunchedEffect(Unit) { sign() }
            return
        }
        Sheet {
            Text("Sign in to $rpId from $app?", color = colors.text, style = MaterialTheme.typography.titleLarge)
            Spacer(Modifier.height(6.dp))
            Text("Only $app can say it belongs to $rpId. Go on only if you trust it.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            error?.let {
                Spacer(Modifier.height(8.dp))
                Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium)
            }
            Spacer(Modifier.height(18.dp))
            PrimaryButton("Sign in", Modifier.fillMaxWidth()) { sign() }
            Spacer(Modifier.height(10.dp))
            QuietButton("Not now", Modifier.fillMaxWidth()) { cancelGet() }
        }
    }

    @Composable
    private fun Sheet(content: @Composable () -> Unit) {
        val colors = palette
        Box(Modifier.fillMaxSize().background(colors.ground).navigationBarsPadding(), contentAlignment = Alignment.BottomCenter) {
            Column(Modifier.fillMaxWidth().padding(22.dp)) { content() }
        }
    }
}

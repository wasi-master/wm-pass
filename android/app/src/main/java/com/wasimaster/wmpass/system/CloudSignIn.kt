package com.wasimaster.wmpass.system

import android.app.Activity
import android.app.ActivityManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.util.Base64
import com.wasimaster.wmkeyboard.core.settings.DriveSpace
import com.wasimaster.wmkeyboard.core.settings.sink.BackupClients
import com.wasimaster.wmkeyboard.core.settings.sink.BoxSink
import com.wasimaster.wmkeyboard.core.settings.sink.DriveSink
import com.wasimaster.wmkeyboard.core.settings.sink.DriveTokenProvider
import com.wasimaster.wmkeyboard.core.settings.sink.DropboxSink
import com.wasimaster.wmkeyboard.core.settings.sink.OAuthTokens
import com.wasimaster.wmkeyboard.core.settings.sink.OneDriveSink
import com.wasimaster.wmpass.BuildConfig
import java.io.File
import java.security.MessageDigest
import java.security.SecureRandom
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.launch

/**
 * The browser half of signing in to Google Drive, Dropbox, OneDrive or Box: PKCE in the
 * owner's own browser, never a WebView, so WM Pass never sees the account password. What
 * comes back is a refresh token, kept sealed with the place like any other login.
 *
 * Each service needs WM Pass registered in its console, and the ids that produces come in at
 * build time (`wmpass.dropboxAppKey` and the rest, see app/build.gradle.kts). A build without
 * one does not offer that service at all.
 */
object CloudSignIn {
    /** Dropbox and OneDrive come back here. */
    const val REDIRECT = "wmpass://oauth"

    /** Box only sends a sign-in to https. The page hands the code on to [REDIRECT]. */
    const val BOX_REDIRECT = "https://wmpass.pages.dev/oauth/"

    private const val GOOGLE_AUTHORIZE = "https://accounts.google.com/o/oauth2/v2/auth"
    private const val GOOGLE_TOKEN = "https://oauth2.googleapis.com/token"
    private const val PENDING_FILE = "oauth_pending"
    private const val VERIFIER_BYTES = 64
    private const val STATE_CHARS = 32

    fun install() {
        BackupClients.install(BuildConfig.DROPBOX_APP_KEY, BuildConfig.ONEDRIVE_CLIENT_ID, BuildConfig.BOX_CLIENT_ID, BuildConfig.BOX_CLIENT_SECRET)
    }

    /**
     * Google's id is an iOS-type client: those take a sign-in back on the reversed id as a
     * scheme with PKCE and no secret, which Android-type clients only do through Play services.
     */
    private val driveScheme: String
        get() = BuildConfig.DRIVE_CLIENT_ID.removeSuffix(".apps.googleusercontent.com").let { if (it.isEmpty()) "" else "com.googleusercontent.apps.$it" }

    private val driveRedirect: String get() = "$driveScheme:/oauth2redirect"

    private val driveTokens by lazy { OAuthTokens(tokenUrl = GOOGLE_TOKEN, clientId = BuildConfig.DRIVE_CLIENT_ID) }

    /** Whether this build can offer [kind] at all. */
    fun available(kind: String): Boolean = when (kind) {
        "drive" -> BuildConfig.DRIVE_CLIENT_ID.isNotEmpty()
        "dropbox" -> BackupClients.dropboxAvailable
        "onedrive" -> BackupClients.oneDriveAvailable
        "box" -> BackupClients.boxAvailable
        else -> true
    }

    fun signsInWithBrowser(kind: String) = kind in setOf("drive", "dropbox", "onedrive", "box")

    /** Tokens for one Drive place, which signed in with the scope its space needs. */
    fun driveTokens(place: Destination): DriveTokenProvider? {
        if (BuildConfig.DRIVE_CLIENT_ID.isEmpty()) return null
        return object : DriveTokenProvider {
            override suspend fun accessToken(scope: String): String? = driveTokens.accessToken(place.refreshToken)
        }
    }

    private data class Pending(val kind: String, val placeId: String, val verifier: String)

    @Volatile
    private var pending: Pending? = null

    enum class Outcome { SIGNED_IN, CANCELLED, FAILED }

    data class Result(val placeId: String, val outcome: Outcome)

    private val mutableResult = MutableStateFlow<Result?>(null)

    /** How the last sign-in ended, until a screen has said so. */
    val result: StateFlow<Result?> = mutableResult

    private val mutableExchanging = MutableStateFlow<String?>(null)

    /** The place whose code is being traded for a token right now. */
    val exchanging: StateFlow<String?> = mutableExchanging

    private val scope = CoroutineScope(SupervisorJob() + Dispatchers.IO)

    fun consume() {
        mutableResult.value = null
    }

    /**
     * The pending sign-in goes to disk too: the browser is in front while the owner types,
     * and on a phone short of memory WM Pass is what gets killed. No backup copies it, and it
     * is gone once the browser comes back.
     */
    private fun pendingFile(context: Context) = File(context.noBackupFilesDir, PENDING_FILE)

    private fun takePending(context: Context): Pending? {
        val file = pendingFile(context)
        val saved = runCatching {
            val (kind, id, verifier) = file.readText().split('\n', limit = 3)
            Pending(kind, id, verifier)
        }.getOrNull()
        file.delete()
        return saved
    }

    /** Opens the service's consent page. The place must already be saved, without a token. */
    fun start(activity: Activity, place: Destination): Boolean {
        val verifier = base64Url(ByteArray(VERIFIER_BYTES).also { SecureRandom().nextBytes(it) })
        val challenge = base64Url(MessageDigest.getInstance("SHA-256").digest(verifier.toByteArray(Charsets.US_ASCII)))
        val url = when (place.kind) {
            "dropbox" -> Uri.parse(DropboxSink.AUTHORIZE_URL).buildUpon()
                .appendQueryParameter("client_id", BackupClients.dropboxClientId)
                .appendQueryParameter("redirect_uri", REDIRECT)
                .appendQueryParameter("code_challenge", challenge)
                .appendQueryParameter("code_challenge_method", "S256")
                // Without it Dropbox issues a four-hour token and no refresh token.
                .appendQueryParameter("token_access_type", "offline")
            "onedrive" -> Uri.parse(OneDriveSink.AUTHORIZE_URL).buildUpon()
                .appendQueryParameter("client_id", BackupClients.oneDriveClientId)
                .appendQueryParameter("redirect_uri", REDIRECT)
                .appendQueryParameter("scope", OneDriveSink.SCOPE)
                .appendQueryParameter("code_challenge", challenge)
                .appendQueryParameter("code_challenge_method", "S256")
            // Box takes its client secret instead of PKCE; the state only comes back.
            "box" -> Uri.parse(BoxSink.AUTHORIZE_URL).buildUpon()
                .appendQueryParameter("client_id", BackupClients.boxClientId)
                .appendQueryParameter("redirect_uri", BOX_REDIRECT)
                .appendQueryParameter("state", verifier.take(STATE_CHARS))
            "drive" -> Uri.parse(GOOGLE_AUTHORIZE).buildUpon()
                .appendQueryParameter("client_id", BuildConfig.DRIVE_CLIENT_ID)
                .appendQueryParameter("redirect_uri", driveRedirect)
                .appendQueryParameter("scope", DriveSink.scopeFor(DriveSpace.of(place.driveSpace)))
                .appendQueryParameter("code_challenge", challenge)
                .appendQueryParameter("code_challenge_method", "S256")
                // Both, or Google issues no refresh token on a second sign-in.
                .appendQueryParameter("access_type", "offline")
                .appendQueryParameter("prompt", "consent")
            else -> return false
        }.appendQueryParameter("response_type", "code").build()
        val waiting = Pending(place.kind, place.id, verifier)
        pending = waiting
        runCatching { pendingFile(activity).writeText("${waiting.kind}\n${waiting.placeId}\n${waiting.verifier}") }
        return runCatching { activity.startActivity(Intent(Intent.ACTION_VIEW, url)) }.isSuccess
    }

    internal fun deliver(context: Context, uri: Uri?) {
        val saved = takePending(context)
        val waiting = pending ?: saved ?: return
        pending = null
        val code = uri?.getQueryParameter("code")
        if (code == null) {
            mutableResult.value = Result(waiting.placeId, Outcome.CANCELLED)
            return
        }
        val app = context.applicationContext
        mutableExchanging.value = waiting.placeId
        scope.launch {
            try {
                val (tokens, redirect) = when (waiting.kind) {
                    "dropbox" -> BackupClients.dropbox() to REDIRECT
                    "onedrive" -> BackupClients.oneDrive() to REDIRECT
                    "box" -> BackupClients.box() to BOX_REDIRECT
                    else -> driveTokens to driveRedirect
                }
                val refresh = tokens?.exchangeCode(code, waiting.verifier, redirect)
                if (refresh == null) {
                    mutableResult.value = Result(waiting.placeId, Outcome.FAILED)
                } else {
                    // The places may not be loaded when the browser brought a killed process back.
                    if (Destinations.list.value.isEmpty()) Destinations.load(app)
                    Destinations.update(app, waiting.placeId) { it.copy(refreshToken = refresh) }
                    mutableResult.value = Result(waiting.placeId, Outcome.SIGNED_IN)
                }
            } finally {
                mutableExchanging.value = null
            }
        }
    }

    /** Puts WM Pass back in front of the browser when its task is still there, or starts it. */
    internal fun returnToApp(activity: Activity) {
        val manager = activity.getSystemService(ActivityManager::class.java)
        val task = runCatching { manager?.appTasks?.firstOrNull { it.taskInfo?.baseActivity?.packageName == activity.packageName } }.getOrNull()
        if (task != null && runCatching { task.moveToFront() }.isSuccess) return
        activity.packageManager.getLaunchIntentForPackage(activity.packageName)
            ?.addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
            ?.let { runCatching { activity.startActivity(it) } }
    }

    private fun base64Url(bytes: ByteArray): String = Base64.encodeToString(bytes, Base64.URL_SAFE or Base64.NO_PADDING or Base64.NO_WRAP)
}

/** Catches the redirect and gets out of the way; the Backups screen watches [CloudSignIn.result]. */
class OAuthRedirectActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        CloudSignIn.deliver(this, intent?.data)
        CloudSignIn.returnToApp(this)
        finish()
    }

    override fun onNewIntent(intent: Intent) {
        super.onNewIntent(intent)
        CloudSignIn.deliver(this, intent.data)
        CloudSignIn.returnToApp(this)
        finish()
    }
}

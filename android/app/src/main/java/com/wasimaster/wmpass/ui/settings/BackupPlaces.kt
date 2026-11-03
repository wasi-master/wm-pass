package com.wasimaster.wmpass.ui.settings

import android.app.Activity
import android.content.ActivityNotFoundException
import android.content.Context
import android.content.ContextWrapper
import android.content.Intent
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.text.input.TextFieldState
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
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
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmkeyboard.core.settings.GitProvider
import com.wasimaster.wmkeyboard.core.settings.ICloudConfig
import com.wasimaster.wmkeyboard.core.settings.ImapSecurity
import com.wasimaster.wmkeyboard.core.settings.MegaConfig
import com.wasimaster.wmkeyboard.core.settings.NasConfig
import com.wasimaster.wmkeyboard.core.settings.NfsConfig
import com.wasimaster.wmkeyboard.core.settings.PCloudConfig
import com.wasimaster.wmkeyboard.core.settings.PCloudRegion
import com.wasimaster.wmkeyboard.core.settings.ProtonConfig
import com.wasimaster.wmkeyboard.core.settings.S3Account
import com.wasimaster.wmkeyboard.core.settings.S3Preset
import com.wasimaster.wmkeyboard.core.settings.WebDavPreset
import com.wasimaster.wmkeyboard.core.settings.WebDavServerField
import com.wasimaster.wmkeyboard.core.settings.sink.ICloudLogin
import com.wasimaster.wmkeyboard.core.settings.sink.ICloudSession
import com.wasimaster.wmkeyboard.core.settings.sink.MegaLogin
import com.wasimaster.wmkeyboard.core.settings.sink.NasLogin
import com.wasimaster.wmkeyboard.core.settings.sink.PCloudLogin
import com.wasimaster.wmkeyboard.core.settings.sink.SinkError
import com.wasimaster.wmkeyboard.core.settings.sink.proton.ProtonLogin
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.system.CloudSignIn
import com.wasimaster.wmpass.system.Destination
import com.wasimaster.wmpass.system.Destinations
import com.wasimaster.wmpass.system.KeyboardProvider
import com.wasimaster.wmpass.system.PlaceWire
import com.wasimaster.wmpass.ui.door.PasswordPill
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.PassSheet
import com.wasimaster.wmpass.ui.vault.PassSwitch
import com.wasimaster.wmpass.ui.vault.SheetField
import kotlinx.coroutines.launch

/** One kind of place: what it is called on the chooser, on its row, and its glyph. */
internal data class PlaceKind(val id: String, val name: String, val blurb: String, val icon: ImageVector, val group: Int)

internal val placeKinds = listOf(
    PlaceKind("folder", "Folder", "Folder on this phone or a cloud app", PassIcons.Folder, 0),
    PlaceKind("drive", "Google Drive", "Google Drive", PassIcons.CloudUpload, 1),
    PlaceKind("dropbox", "Dropbox", "Dropbox", PassIcons.CloudUpload, 1),
    PlaceKind("onedrive", "OneDrive", "OneDrive", PassIcons.CloudUpload, 1),
    PlaceKind("box", "Box", "Box", PassIcons.CloudUpload, 1),
    PlaceKind("pcloud", "pCloud", "pCloud", PassIcons.CloudUpload, 1),
    PlaceKind("mega", "MEGA", "MEGA", PassIcons.CloudUpload, 1),
    PlaceKind("icloud", "iCloud Drive", "iCloud Drive", PassIcons.CloudUpload, 1),
    PlaceKind("proton", "Proton Drive", "Proton Drive", PassIcons.CloudUpload, 1),
    PlaceKind("webdav", "WebDAV", "WebDAV, such as Nextcloud", PassIcons.Language, 2),
    PlaceKind("s3", "S3", "S3, such as Backblaze or MinIO", PassIcons.Database, 2),
    PlaceKind("sftp", "SFTP", "SFTP server", PassIcons.Terminal, 2),
    PlaceKind("ftp", "FTP", "FTP server", PassIcons.Dns, 2),
    PlaceKind("smb", "SMB", "Windows or Samba share", PassIcons.HomeStorage, 2),
    PlaceKind("nfs", "NFS", "NFS export", PassIcons.Router, 2),
    PlaceKind("synology", "Synology", "Synology NAS", PassIcons.HomeStorage, 2),
    PlaceKind("qnap", "QNAP", "QNAP NAS", PassIcons.HomeStorage, 2),
    PlaceKind("git", "Git", "A Git repository", PassIcons.Hub, 3),
    PlaceKind("imap", "Email", "A mailbox, over IMAP", PassIcons.Mail, 3),
)

private val groupNames = listOf("On this phone", "Accounts", "Servers and storage", "Somewhere else")

internal fun placeKind(id: String) = placeKinds.firstOrNull { it.id == id }

internal fun placeLabel(kind: String) = placeKind(kind)?.blurb ?: kind

internal fun placeIcon(kind: String) = placeKind(kind)?.icon ?: PassIcons.Terminal

private fun defaultPort(kind: String, security: String) = when (kind) {
    "ftp" -> 21
    "smb" -> 445
    "imap" -> ImapSecurity.of(security).defaultPort
    else -> 22
}

private tailrec fun Context.activity(): Activity? = when (this) {
    is Activity -> this
    is ContextWrapper -> baseContext.activity()
    else -> null
}

/** Words for a sign-in that did not go through. */
private fun refusal(error: SinkError): String = when (error) {
    SinkError.PERMISSION_LOST -> "That sign-in was refused. Check the address, name and password."
    SinkError.UNSAFE -> "The server could not prove who it is, so WM Pass stopped."
    SinkError.TARGET_MISSING -> "That folder or account was not found."
    SinkError.NOT_CONFIGURED -> "Something it needs is missing."
    else -> "The service could not be reached. Check the connection and try again."
}

/**
 * Add or change a place to keep copies. Works at the door too, where no vault is open yet, so
 * a phone can be restored from a place that was set up on another one.
 *
 * An account kind is saved only once it is signed in: through the browser for Google Drive,
 * Dropbox, OneDrive and Box, here with a password and maybe a code for the rest.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun DestinationSheet(existing: Destination?, onDismiss: () -> Unit, onSaved: (Destination) -> Unit) {
    val colors = palette
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val prefs = remember { Prefs.of(context) }
    val id = remember { existing?.id ?: java.util.UUID.randomUUID().toString() }
    var kind by remember { mutableStateOf(existing?.kind ?: "folder") }
    val label = rememberTextFieldState(existing?.label.orEmpty())
    val url = rememberTextFieldState(existing?.url.orEmpty())
    val user = rememberTextFieldState(existing?.user.orEmpty())
    val password = rememberTextFieldState(existing?.password.orEmpty())
    val endpoint = rememberTextFieldState(existing?.endpoint.orEmpty())
    val region = rememberTextFieldState(existing?.region ?: "us-east-1")
    val bucket = rememberTextFieldState(existing?.bucket.orEmpty())
    val keyId = rememberTextFieldState(existing?.accessKeyId.orEmpty())
    val keySecret = rememberTextFieldState(existing?.secretAccessKey.orEmpty())
    val prefix = rememberTextFieldState(existing?.prefix.orEmpty())
    val host = rememberTextFieldState(existing?.host.orEmpty())
    val port = rememberTextFieldState(existing?.port?.toString().orEmpty())
    val path = rememberTextFieldState(existing?.path.orEmpty())
    val folder = rememberTextFieldState(existing?.folder.orEmpty())
    val share = rememberTextFieldState(existing?.share.orEmpty())
    val domain = rememberTextFieldState(existing?.domain.orEmpty())
    val repository = rememberTextFieldState(existing?.repository.orEmpty())
    val branch = rememberTextFieldState(existing?.branch.orEmpty())
    val token = rememberTextFieldState(existing?.token.orEmpty())
    // An NFS place keeps its host in nfs; its nas.server is empty, not null.
    val server = rememberTextFieldState(existing?.takeIf { it.kind == "nfs" }?.nfs?.host ?: existing?.nas?.server.orEmpty())
    val export = rememberTextFieldState(existing?.nfs?.export.orEmpty())
    val uid = rememberTextFieldState((existing?.nfs?.uid ?: NfsConfig.DEFAULT_ID).toString())
    val gid = rememberTextFieldState((existing?.nfs?.gid ?: NfsConfig.DEFAULT_ID).toString())
    val account = rememberTextFieldState("")
    val presetServer = rememberTextFieldState("")
    val code = rememberTextFieldState("")
    val mailboxPassword = rememberTextFieldState("")
    var preset by remember { mutableStateOf(existing?.preset.orEmpty()) }
    var pathStyle by remember { mutableStateOf(existing?.pathStyle ?: false) }
    var secure by remember { mutableStateOf(existing?.secure ?: true) }
    var encrypt by remember { mutableStateOf(existing?.encrypt ?: true) }
    var security by remember { mutableStateOf(existing?.security ?: "tls") }
    var provider by remember { mutableStateOf(existing?.provider ?: "github") }
    var allowPublic by remember { mutableStateOf(existing?.allowPublic ?: false) }
    var driveSpace by remember { mutableStateOf(existing?.driveSpace ?: "appdata") }
    var ownCertificate by remember { mutableStateOf(existing?.nas?.ownCertificate ?: false) }
    var pcloudRegion by remember { mutableStateOf(existing?.pcloud?.region ?: PCloudRegion.US) }
    var folderUri by remember { mutableStateOf(existing?.uri.orEmpty()) }
    var folderName by remember { mutableStateOf(if (existing?.uri.isNullOrEmpty()) null else "Chosen") }
    // A sign-in under way: what it is waiting for, and what went wrong.
    var busy by remember { mutableStateOf(false) }
    var problem by remember { mutableStateOf<String?>(null) }
    var askCode by remember { mutableStateOf(false) }
    var askMailbox by remember { mutableStateOf(false) }
    var pcloudToken by remember { mutableStateOf<String?>(null) }
    var protonPending by remember { mutableStateOf<ProtonLogin.Pending?>(null) }
    var icloudSession by remember { mutableStateOf<ICloudSession?>(null) }
    var waitingForBrowser by remember { mutableStateOf(false) }
    val pick = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocumentTree()) { uri ->
        if (uri != null) {
            context.contentResolver.takePersistableUriPermission(uri, Intent.FLAG_GRANT_READ_URI_PERMISSION or Intent.FLAG_GRANT_WRITE_URI_PERMISSION)
            folderUri = uri.toString()
            folderName = uri.lastPathSegment?.substringAfterLast(':')?.ifEmpty { null } ?: "Chosen"
        }
    }

    // A place WM Keyboard handed over fills the sheet in as if it had been typed. Nothing is saved
    // until the person saves it, after signing in where the kind needs that.
    var seeded by remember { mutableStateOf<Destination?>(null) }

    fun fill(place: Destination) {
        seeded = place
        kind = place.kind
        preset = ""
        problem = null
        askCode = false
        askMailbox = false
        pcloudToken = null
        protonPending = null
        icloudSession = null
        label.setTextAndPlaceCursorAtEnd(place.label)
        url.setTextAndPlaceCursorAtEnd(place.url)
        user.setTextAndPlaceCursorAtEnd(place.user)
        password.setTextAndPlaceCursorAtEnd(place.password)
        endpoint.setTextAndPlaceCursorAtEnd(place.endpoint)
        region.setTextAndPlaceCursorAtEnd(place.region)
        bucket.setTextAndPlaceCursorAtEnd(place.bucket)
        keyId.setTextAndPlaceCursorAtEnd(place.accessKeyId)
        keySecret.setTextAndPlaceCursorAtEnd(place.secretAccessKey)
        prefix.setTextAndPlaceCursorAtEnd(place.prefix)
        host.setTextAndPlaceCursorAtEnd(place.host)
        port.setTextAndPlaceCursorAtEnd(if (place.kind in setOf("sftp", "ftp", "smb", "imap")) place.port.toString() else "")
        path.setTextAndPlaceCursorAtEnd(place.path)
        folder.setTextAndPlaceCursorAtEnd(place.folder)
        share.setTextAndPlaceCursorAtEnd(place.share)
        domain.setTextAndPlaceCursorAtEnd(place.domain)
        repository.setTextAndPlaceCursorAtEnd(place.repository)
        branch.setTextAndPlaceCursorAtEnd(place.branch)
        token.setTextAndPlaceCursorAtEnd(place.token)
        server.setTextAndPlaceCursorAtEnd(if (place.kind == "nfs") place.nfs.host else place.nas.server)
        export.setTextAndPlaceCursorAtEnd(place.nfs.export)
        uid.setTextAndPlaceCursorAtEnd(place.nfs.uid.toString())
        gid.setTextAndPlaceCursorAtEnd(place.nfs.gid.toString())
        pathStyle = place.pathStyle
        secure = place.secure
        encrypt = place.encrypt
        security = place.security
        provider = place.provider
        allowPublic = place.allowPublic
        driveSpace = place.driveSpace
        ownCertificate = place.nas.ownCertificate
        pcloudRegion = place.pcloud.region
        folderUri = ""
        folderName = null
    }

    val keyboardPackage = remember { KeyboardProvider.installedPackage(context) }
    val fromKeyboard = rememberLauncherForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        if (result.resultCode != Activity.RESULT_OK) return@rememberLauncherForActivityResult
        val raw = result.data?.getStringExtra(PlaceWire.EXTRA) ?: return@rememberLauncherForActivityResult
        val place = PlaceWire.decode(raw, id)
        when {
            place == null -> problem = "WM Keyboard sent a place this version of WM Pass cannot use."
            !CloudSignIn.available(place.kind) -> problem = "This build of WM Pass cannot reach ${placeKind(place.kind)?.name ?: place.kind}."
            else -> fill(place)
        }
    }

    fun text(state: TextFieldState) = state.text.toString().trim()

    fun webDavUrl(): String {
        val known = WebDavPreset.of(preset)
        return if (known == WebDavPreset.CUSTOM) text(url) else known.url(text(presetServer), text(user), text(folder).ifEmpty { Destination.DEFAULT_FOLDER })
    }

    fun s3Endpoint(): String {
        val known = S3Preset.of(preset)
        return if (known == S3Preset.CUSTOM) text(endpoint) else known.endpoint(text(region), text(account))
    }

    /** Everything typed so far, as the place it would be. */
    fun draft(): Destination {
        // What the sheet has no field for, such as an SFTP key from WM Keyboard, comes from here.
        val base = existing ?: seeded ?: Destination(id = id, kind = kind, label = "")
        val portNumber = text(port).toIntOrNull() ?: defaultPort(kind, security)
        return base.copy(
            kind = kind,
            label = text(label).ifEmpty { placeKind(kind)?.name ?: kind },
            uri = folderUri,
            url = if (kind == "webdav") webDavUrl() else text(url),
            user = text(user),
            password = password.text.toString(),
            endpoint = if (kind == "s3") s3Endpoint() else text(endpoint),
            region = text(region).ifEmpty { "us-east-1" },
            bucket = text(bucket),
            prefix = text(prefix),
            accessKeyId = text(keyId),
            secretAccessKey = keySecret.text.toString(),
            pathStyle = if (kind == "s3" && S3Preset.of(preset) != S3Preset.CUSTOM) S3Preset.of(preset).pathStyle else pathStyle,
            preset = preset,
            host = text(host),
            port = portNumber,
            path = text(path),
            folder = text(folder),
            share = text(share).trim('/', '\\'),
            domain = text(domain),
            encrypt = encrypt,
            secure = secure,
            security = security,
            provider = provider,
            repository = text(repository).trim('/'),
            branch = text(branch),
            token = token.text.toString().trim(),
            allowPublic = allowPublic,
            driveSpace = driveSpace,
            nfs = NfsConfig(host = text(server), export = text(export), path = text(path), uid = text(uid).toIntOrNull() ?: NfsConfig.DEFAULT_ID, gid = text(gid).toIntOrNull() ?: NfsConfig.DEFAULT_ID),
            nas = ((existing ?: seeded)?.nas ?: NasConfig()).let { old ->
                val same = old.server == text(server) && old.user == text(user)
                NasConfig(
                    server = text(server),
                    user = text(user),
                    password = password.text.toString(),
                    path = text(path),
                    deviceToken = if (same) old.deviceToken else "",
                    ownCertificate = ownCertificate,
                    certPin = if (same && old.ownCertificate == ownCertificate) old.certPin else "",
                )
            },
        )
    }

    fun save(place: Destination) {
        // Setting up a remote place is the owner's yes to reaching it.
        if (place.remote) prefs.updateNetworkAllowed(true)
        Destinations.put(context, place)
        onSaved(place)
    }

    fun fail(error: SinkError) {
        busy = false
        problem = refusal(error)
    }

    val exchanging by CloudSignIn.exchanging.collectAsStateWithLifecycle()
    val signInResult by CloudSignIn.result.collectAsStateWithLifecycle()
    LaunchedEffect(signInResult) {
        val result = signInResult ?: return@LaunchedEffect
        if (result.placeId != id) return@LaunchedEffect
        CloudSignIn.consume()
        waitingForBrowser = false
        when (result.outcome) {
            CloudSignIn.Outcome.SIGNED_IN -> Destinations.list.value.firstOrNull { it.id == id }?.let(onSaved)
            CloudSignIn.Outcome.CANCELLED -> problem = "The sign-in was cancelled."
            CloudSignIn.Outcome.FAILED -> problem = "The service did not accept that sign-in. Try again."
        }
    }

    /** The sign-in for an account kind, from the password through any code. */
    fun signIn() {
        val place = draft()
        problem = null
        if (CloudSignIn.signsInWithBrowser(kind)) {
            if (place.remote) prefs.updateNetworkAllowed(true)
            Destinations.put(context, place.copy(refreshToken = existing?.refreshToken?.takeIf { existing.kind == kind }.orEmpty()))
            val activity = context.activity()
            if (activity == null || !CloudSignIn.start(activity, place)) {
                problem = "No browser could open the sign-in page."
            } else {
                waitingForBrowser = true
            }
            return
        }
        busy = true
        scope.launch {
            when (kind) {
                "pcloud" -> {
                    val waiting = pcloudToken
                    var outcome = if (waiting != null) PCloudLogin.submitCode(pcloudRegion, waiting, text(code)) else PCloudLogin.signIn(pcloudRegion, place.user, place.password)
                    if (outcome is PCloudLogin.Outcome.OtherRegion) {
                        pcloudRegion = outcome.region
                        outcome = PCloudLogin.signIn(outcome.region, place.user, place.password)
                    }
                    when (outcome) {
                        is PCloudLogin.Outcome.SignedIn -> save(place.copy(password = "", pcloud = PCloudConfig(pcloudRegion, place.user, outcome.auth, place.folderOrDefault)))
                        is PCloudLogin.Outcome.NeedsCode -> {
                            pcloudToken = outcome.token
                            askCode = true
                        }
                        is PCloudLogin.Outcome.OtherRegion -> fail(SinkError.TARGET_MISSING)
                        is PCloudLogin.Outcome.Failed -> fail(outcome.error)
                    }
                }
                "mega" -> when (val outcome = MegaLogin.signIn(place.user, place.password, if (askCode) text(code) else "")) {
                    is MegaLogin.Outcome.SignedIn -> save(place.copy(password = "", mega = MegaConfig(place.user, outcome.sessionId, outcome.masterKey, place.folderOrDefault)))
                    MegaLogin.Outcome.NeedsCode -> askCode = true
                    is MegaLogin.Outcome.Failed -> fail(outcome.error)
                }
                "proton" -> {
                    val waiting = protonPending
                    val outcome = if (waiting != null) ProtonLogin.submitCode(waiting, text(code)) else ProtonLogin.signIn(place.user, place.password, mailboxPassword.text.toString())
                    when (outcome) {
                        is ProtonLogin.Outcome.SignedIn -> save(
                            place.copy(
                                password = "",
                                proton = ProtonConfig(place.user, outcome.uid, outcome.accessToken, outcome.refreshToken, outcome.saltedKeyPass, place.folderOrDefault),
                            ),
                        )
                        is ProtonLogin.Outcome.NeedsCode -> {
                            protonPending = outcome.state
                            askCode = true
                        }
                        ProtonLogin.Outcome.NeedsMailboxPassword -> askMailbox = true
                        ProtonLogin.Outcome.NeedsHumanVerification -> {
                            busy = false
                            problem = "Proton wants a CAPTCHA first. Sign in once at proton.me from this network, then try again here."
                        }
                        is ProtonLogin.Outcome.Failed -> fail(outcome.error)
                    }
                }
                "icloud" -> {
                    val clientId = existing?.icloud?.clientId?.ifEmpty { null } ?: ICloudSession.CLIENT_ID
                    val session = icloudSession
                    val outcome = if (session != null) ICloudLogin.finish(session, text(code)) else ICloudLogin.start(place.user, place.password, clientId)
                    when (outcome) {
                        is ICloudLogin.Outcome.SignedIn -> save(
                            place.copy(
                                password = "",
                                icloud = ICloudConfig(
                                    appleId = place.user,
                                    password = place.password,
                                    trustToken = outcome.trustToken,
                                    cookies = outcome.cookies,
                                    clientId = clientId,
                                    trustedAtMs = System.currentTimeMillis(),
                                    folder = place.folderOrDefault,
                                ),
                            ),
                        )
                        is ICloudLogin.Outcome.NeedsCode -> {
                            icloudSession = outcome.session
                            askCode = true
                        }
                        ICloudLogin.Outcome.NeedsApproval -> {
                            busy = false
                            problem = "Approve this sign-in on one of your Apple devices, then try again."
                        }
                        is ICloudLogin.Outcome.Failed -> fail(outcome.error)
                    }
                }
                else -> {
                    var pin = place.nas.certPin
                    val answer = if (kind == "synology") {
                        NasLogin.synology(place.nas, if (askCode) text(code) else "") { pin = it }
                    } else {
                        NasLogin.qnap(place.nas, if (askCode) text(code) else "") { pin = it }
                    }
                    val error = answer.error
                    when {
                        answer.needsCode -> askCode = true
                        error != null -> fail(error)
                        else -> save(place.copy(nas = place.nas.copy(deviceToken = answer.deviceToken ?: place.nas.deviceToken, certPin = pin)))
                    }
                }
            }
            busy = false
        }
    }

    val shownKinds = placeKinds.filter { CloudSignIn.available(it.id) || it.id == existing?.kind }
    val chosen = placeKind(kind)
    PassSheet(onDismiss, existing?.label ?: "A place for copies") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            if (existing == null && keyboardPackage != null) {
                QuietButton("Use a place from WM Keyboard", Modifier.fillMaxWidth(), PassIcons.Keyboard, colors.primary) {
                    // Checked again at the tap: the package could have changed hands since.
                    val target = KeyboardProvider.installedPackage(context) ?: return@QuietButton
                    try {
                        fromKeyboard.launch(Intent().setClassName(target, "com.wasimaster.wmkeyboard.app.PlaceShareActivity"))
                    } catch (_: ActivityNotFoundException) {
                        problem = "Update WM Keyboard to share its places."
                    }
                }
            }
            if (existing == null) {
                shownKinds.groupBy { it.group }.forEach { (group, kinds) ->
                    Text(groupNames[group], color = colors.textLow, style = MaterialTheme.typography.labelMedium, modifier = Modifier.padding(horizontal = 6.dp))
                    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        kinds.forEach { option ->
                            Pill(option.name, kind == option.id) {
                                kind = option.id
                                preset = ""
                                problem = null
                                askCode = false
                                askMailbox = false
                                pcloudToken = null
                                protonPending = null
                                icloudSession = null
                                port.setTextAndPlaceCursorAtEnd("")
                            }
                        }
                    }
                }
            }
            Text(
                when (kind) {
                    "folder" -> "Pick a folder. Syncthing, Nextcloud or a cloud provider's own app can carry it from there."
                    "drive", "dropbox", "onedrive", "box" -> "You sign in on the service's own page, in your browser. WM Pass never sees that password, and can reach only its own folder there."
                    "pcloud", "mega", "proton" -> "Only the encrypted file goes there. WM Pass keeps the session the sign-in hands back, not your password."
                    else -> "Only the encrypted file goes there, so the server cannot read it. Its login is kept on this phone, sealed by the phone's secure hardware."
                },
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
            SheetField(label, "A name for it")
            when (kind) {
                "folder" -> QuietButton(folderName?.let { "Folder: $it" } ?: "Choose a folder", Modifier.fillMaxWidth(), PassIcons.Folder, colors.primary) { pick.launch(null) }
                "webdav" -> {
                    PresetPills(listOf("" to "Custom") + WebDavPreset.entries.filter { it != WebDavPreset.CUSTOM }.map { it.id to it.label }, preset) { preset = it }
                    val known = WebDavPreset.of(preset)
                    if (known == WebDavPreset.CUSTOM) {
                        SheetField(url, "https://example.com/remote.php/dav/files/you/")
                    } else {
                        if (known.needsServer) {
                            SheetField(
                                presetServer,
                                when (known.serverField) {
                                    WebDavServerField.ACCOUNT_ID -> "Account number"
                                    WebDavServerField.TAILDRIVE_SHARE -> "Share, as tailnet/machine/share"
                                    WebDavServerField.HOST -> "Server, such as cloud.example.com"
                                },
                            )
                        }
                        SheetField(folder, "Folder there, ${Destination.DEFAULT_FOLDER} if empty")
                    }
                    if (known.signsIn) {
                        SheetField(user, "User name")
                        PasswordPill(password, "Password")
                    }
                }
                "s3" -> {
                    PresetPills(listOf("" to "Custom") + S3Preset.entries.filter { it != S3Preset.CUSTOM }.map { it.id to it.label }, preset) { picked ->
                        preset = picked
                        S3Preset.of(picked).takeIf { it != S3Preset.CUSTOM }?.let { region.setTextAndPlaceCursorAtEnd(it.defaultRegion) }
                    }
                    val known = S3Preset.of(preset)
                    if (known == S3Preset.CUSTOM) SheetField(endpoint, "Endpoint, empty for AWS")
                    when (known.account) {
                        S3Account.ACCOUNT_ID -> SheetField(account, "Account id")
                        S3Account.ENDPOINT -> SheetField(account, "Endpoint, from the service's console")
                        S3Account.SERVER -> SheetField(account, "Server address")
                        S3Account.NONE -> Unit
                    }
                    if (known.regions.isNotEmpty()) {
                        PresetPills(known.regions.map { it to it }, text(region)) { region.setTextAndPlaceCursorAtEnd(it) }
                    } else {
                        SheetField(region, "Region")
                    }
                    SheetField(bucket, "Bucket")
                    SheetField(prefix, "Folder inside it, if any")
                    SheetField(keyId, "Access key")
                    PasswordPill(keySecret, "Secret key")
                    if (known == S3Preset.CUSTOM) SwitchLine("Path-style addresses", "For MinIO, Garage, or a server addressed by IP.", pathStyle) { pathStyle = it }
                }
                "sftp", "ftp", "smb", "imap" -> {
                    SheetField(host, "Host")
                    SheetField(port, "Port, ${defaultPort(kind, security)} if empty")
                    if (kind == "smb") {
                        SheetField(share, "Share")
                        SheetField(domain, "Domain or workgroup, if any")
                    }
                    if (kind == "imap") {
                        PresetPills(listOf("tls" to "TLS", "starttls" to "STARTTLS", "none" to "None"), security) { security = it }
                    }
                    SheetField(user, if (kind == "imap") "Email address or user name" else "User name")
                    PasswordPill(password, "Password")
                    if (kind == "imap") SheetField(folder, "Mailbox, WM Pass backups if empty") else SheetField(path, "Folder on the server")
                    when (kind) {
                        "sftp" -> Hint("The server's key is remembered on the first connection. If it ever changes, WM Pass refuses to connect.")
                        "ftp" -> {
                            SwitchLine("Encrypt the connection", "Plain FTP sends the password as text. Turn this off only for an old server that offers nothing else.", secure) { secure = it }
                        }
                        "smb" -> SwitchLine("Require encryption", "SMB 3 encrypts the file on the way. Off lets an older server sign without encrypting.", encrypt) { encrypt = it }
                        "imap" -> if (security == "none") Hint("Without TLS the password crosses the network as text.")
                    }
                }
                "nfs" -> {
                    SheetField(server, "Host")
                    SheetField(export, "Export, such as /srv/nfs")
                    SheetField(path, "Folder inside it, if any")
                    Row(horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                        SheetField(uid, "User id", Modifier.weight(1f))
                        SheetField(gid, "Group id", Modifier.weight(1f))
                    }
                    Hint("NFS without Kerberos takes these ids on trust and does not encrypt. Use it on a network you control.")
                }
                "synology", "qnap" -> {
                    SheetField(server, if (kind == "synology") "nas.local or 192.168.1.20:5001" else "nas.local or 192.168.1.20")
                    SheetField(user, "User name")
                    PasswordPill(password, "Password")
                    SheetField(path, if (kind == "synology") "Folder, such as /home/Drive/WM Pass" else "Folder, such as /Public/WM Pass")
                    SwitchLine(
                        "Accept the NAS's own certificate",
                        "For a NAS without a certificate from a known authority. The first one seen is remembered; a different one later is refused.",
                        ownCertificate,
                    ) { ownCertificate = it }
                }
                "git" -> {
                    PresetPills(GitProvider.entries.map { it.id to it.name.lowercase().replaceFirstChar(Char::uppercase) }, provider) { provider = it }
                    SheetField(url, "Server, ${GitProvider.of(provider).defaultServer} if empty")
                    SheetField(repository, "owner/repository")
                    SheetField(branch, "Branch, the default one if empty")
                    SheetField(path, "Folder in the repository, if any")
                    PasswordPill(token, "Access token")
                    SwitchLine("Allow a public repository", "Anyone could download the encrypted file. Only your password opens it, but a private repository is safer.", allowPublic) { allowPublic = it }
                }
                "drive" -> {
                    PresetPills(listOf("appdata" to "Hidden app folder", "folder" to "A folder you can see"), driveSpace) { driveSpace = it }
                    if (driveSpace == "folder") SheetField(folder, "Folder in My Drive, ${Destination.DEFAULT_FOLDER} if empty")
                    Hint(
                        if (driveSpace == "appdata") {
                            "Only WM Pass can see or restore copies in the hidden folder."
                        } else {
                            "WM Pass can see only the files it made itself in your Drive."
                        },
                    )
                }
                "dropbox", "onedrive" -> Hint("Copies go to WM Pass's own app folder there.")
                "box", "pcloud", "mega", "icloud", "proton" -> {
                    if (kind == "pcloud") {
                        PresetPills(PCloudRegion.entries.map { it.name to if (it == PCloudRegion.US) "United States" else "Europe" }, pcloudRegion.name) { pcloudRegion = PCloudRegion.valueOf(it) }
                    }
                    if (kind != "box") {
                        SheetField(user, if (kind == "icloud") "Apple Account email" else if (kind == "proton") "Proton address or user name" else "Email")
                        PasswordPill(password, "Password")
                    }
                    if (askMailbox) PasswordPill(mailboxPassword, "Mailbox password")
                    SheetField(folder, "Folder there, ${Destination.DEFAULT_FOLDER} if empty")
                    if (kind == "icloud") Hint("Apple asks for a code from one of your devices. The sign-in lasts about a month, then WM Pass asks again.")
                }
            }
            if (askCode) SheetField(code, "Code")
            problem?.let { Text(it, color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(horizontal = 6.dp)) }
            val place = draft()
            val signsIn = kind in setOf("drive", "dropbox", "onedrive", "box", "pcloud", "mega", "proton", "icloud", "synology", "qnap")
            val ready = when (kind) {
                "drive", "dropbox", "onedrive", "box" -> true
                "pcloud", "mega", "proton", "icloud" -> place.user.isNotEmpty() && place.password.isNotEmpty() && (!askCode || text(code).isNotEmpty())
                "synology", "qnap" -> place.nas.configured && (!askCode || text(code).isNotEmpty())
                else -> place.configured
            }
            Spacer(Modifier.height(6.dp))
            when {
                waitingForBrowser || exchanging == id -> Row(Modifier.fillMaxWidth().padding(vertical = 14.dp), verticalAlignment = Alignment.CenterVertically) {
                    androidx.compose.material3.CircularProgressIndicator(Modifier.padding(end = 12.dp).height(20.dp), color = colors.primary, strokeWidth = 2.dp)
                    Text(if (exchanging == id) "Finishing the sign-in…" else "Waiting for the browser…", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
                }
                signsIn -> PrimaryButton(
                    when {
                        busy -> "Signing in…"
                        askCode -> "Continue"
                        CloudSignIn.signsInWithBrowser(kind) -> "Sign in with ${chosen?.name ?: kind}"
                        else -> "Sign in"
                    },
                    Modifier.fillMaxWidth(),
                    enabled = ready && !busy,
                ) { signIn() }
                else -> PrimaryButton("Save", Modifier.fillMaxWidth(), enabled = ready) { save(place) }
            }
            if (existing != null && signsIn && existing.configured && !askCode) {
                QuietButton("Keep the current sign-in", Modifier.fillMaxWidth()) {
                    save(place.copy(refreshToken = existing.refreshToken, pcloud = existing.pcloud, mega = existing.mega, proton = existing.proton, icloud = existing.icloud))
                }
            }
        }
    }
}

@OptIn(ExperimentalLayoutApi::class)
@Composable
private fun PresetPills(options: List<Pair<String, String>>, selected: String, onPick: (String) -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        options.forEach { (id, name) -> Pill(name, selected == id) { onPick(id) } }
    }
}

@Composable
private fun Hint(text: String) {
    Text(text, color = palette.textLow, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 6.dp))
}

@Composable
private fun SwitchLine(title: String, detail: String, checked: Boolean, onChange: (Boolean) -> Unit) {
    val colors = palette
    Row(Modifier.fillMaxWidth().padding(horizontal = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f).padding(end = 12.dp)) {
            Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge)
            Text(detail, color = colors.textLow, style = MaterialTheme.typography.bodySmall)
        }
        PassSwitch(checked, onChange)
    }
}

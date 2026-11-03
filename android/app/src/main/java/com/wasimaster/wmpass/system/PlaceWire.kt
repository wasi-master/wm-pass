package com.wasimaster.wmpass.system

import com.wasimaster.wmkeyboard.core.settings.DriveSpace
import com.wasimaster.wmkeyboard.core.settings.GitProvider
import com.wasimaster.wmkeyboard.core.settings.ImapSecurity
import com.wasimaster.wmkeyboard.core.settings.NasConfig
import com.wasimaster.wmkeyboard.core.settings.NfsConfig
import com.wasimaster.wmkeyboard.core.settings.PCloudConfig
import com.wasimaster.wmkeyboard.core.settings.PCloudRegion
import kotlinx.serialization.json.Json
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.intOrNull
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * A place for copies as it goes between WM Pass and WM Keyboard, so a place set up in one app can
 * be picked in the other. It crosses once, when the person picks it on the other app's screen, as
 * that screen's result: in memory, never written anywhere on the way, and only to an app whose
 * signing certificate checked out. Each app keeps its own copy after that, WM Pass sealed as every
 * place is; nothing keeps the two in step.
 *
 * WM Keyboard reads and writes the same format in its own `PlaceWire`, where the whole list of keys
 * is set out. A change here is a change there too. In short: one JSON object, version 1, with
 * `kind` and `name`, then the fields a server checks (`url`, `user`, `password`, `host`, `port`,
 * `path`, the S3 and SFTP keys, a Git token...) and `folder` inside an account, empty for the
 * receiving app's own default.
 *
 * A sign-in never crosses. Each app has its own OAuth registration, so a Drive, Dropbox, OneDrive or
 * Box token from one does nothing in the other; Box and Proton rotate theirs on every use, so two
 * apps sharing one would sign each other out; pCloud, MEGA and iCloud keep sessions, and a NAS keeps
 * the token that skips its 2FA code. The receiving app signs in for those. A folder on the phone
 * crosses without its folder, since Android grants one to one app.
 */
object PlaceWire {
    const val VERSION = 1

    /** The extra both apps' place pickers put the place in. */
    const val EXTRA = "com.wasimaster.suite.extra.PLACE"

    /** The kinds both apps know. WM Keyboard's nearby computer is its own alone. */
    private val KINDS = setOf(
        "folder", "webdav", "s3", "sftp", "ftp", "smb", "nfs", "git", "imap", "synology", "qnap",
        "drive", "dropbox", "onedrive", "box", "pcloud", "mega", "icloud", "proton",
    )

    /** What an empty IMAP folder means here, as [Destinations.sink] reads it. */
    private const val IMAP_DEFAULT = "WM Pass backups"

    private val json = Json { ignoreUnknownKeys = true }

    /** [d] as WM Keyboard reads it, or null for a kind it does not know. */
    fun encode(d: Destination): String? {
        if (d.kind !in KINDS) return null
        return buildJsonObject {
            put("v", VERSION)
            put("kind", d.kind)
            put("name", d.label)
            when (d.kind) {
                "webdav" -> {
                    put("url", d.url)
                    put("user", d.user)
                    put("password", d.password)
                }
                "s3" -> {
                    put("endpoint", d.endpoint)
                    put("region", d.region)
                    put("bucket", d.bucket)
                    put("prefix", d.prefix)
                    put("accessKeyId", d.accessKeyId)
                    put("secretAccessKey", d.secretAccessKey)
                    put("pathStyle", d.pathStyle)
                }
                "sftp" -> {
                    put("host", d.host)
                    put("port", d.port)
                    put("user", d.user)
                    put("password", d.password)
                    put("privateKey", d.privateKey)
                    put("keyPassphrase", d.keyPassphrase)
                    put("path", d.path)
                    put("hostKey", d.hostKey)
                }
                "ftp" -> {
                    put("host", d.host)
                    put("port", d.port)
                    put("user", d.user)
                    put("password", d.password)
                    put("path", d.path)
                    put("secure", d.secure)
                }
                "smb" -> {
                    put("host", d.host)
                    put("port", d.port)
                    put("share", d.share)
                    put("path", d.path)
                    put("domain", d.domain)
                    put("user", d.user)
                    put("password", d.password)
                    put("encrypt", d.encrypt)
                }
                "nfs" -> {
                    put("host", d.nfs.host)
                    put("export", d.nfs.export)
                    put("path", d.nfs.path)
                    put("uid", d.nfs.uid)
                    put("gid", d.nfs.gid)
                    put("port", d.nfs.port)
                    put("mountPort", d.nfs.mountPort)
                }
                "git" -> {
                    put("provider", d.provider)
                    put("url", d.url)
                    put("repository", d.repository)
                    put("branch", d.branch)
                    put("path", d.path)
                    put("token", d.token)
                    put("allowPublic", d.allowPublic)
                }
                "imap" -> {
                    put("host", d.host)
                    put("port", d.port)
                    put("security", d.security)
                    put("user", d.user)
                    put("password", d.password)
                    put("folder", ownFolder(d.folder, IMAP_DEFAULT))
                }
                "synology", "qnap" -> {
                    put("host", d.nas.server)
                    put("user", d.nas.user)
                    put("password", d.nas.password)
                    put("path", d.nas.path)
                    put("ownCertificate", d.nas.ownCertificate)
                    put("certPin", d.nas.certPin)
                }
                "drive" -> {
                    put("driveSpace", d.driveSpace)
                    put("folder", ownFolder(d.folder, Destination.DEFAULT_FOLDER))
                }
                "box" -> put("folder", ownFolder(d.folder, Destination.DEFAULT_FOLDER))
                "pcloud" -> {
                    put("pcloudRegion", if (d.pcloud.region == PCloudRegion.EU) "eu" else "us")
                    put("user", d.user.ifEmpty { d.pcloud.email })
                    put("folder", ownFolder(d.folder, Destination.DEFAULT_FOLDER))
                }
                "mega" -> {
                    put("user", d.user.ifEmpty { d.mega.email })
                    put("folder", ownFolder(d.folder, Destination.DEFAULT_FOLDER))
                }
                "icloud" -> {
                    put("user", d.user.ifEmpty { d.icloud.appleId })
                    // Kept in the session's config once signed in; the plain field is emptied then.
                    put("password", d.icloud.password.ifEmpty { d.password })
                    put("folder", ownFolder(d.folder, Destination.DEFAULT_FOLDER))
                }
                "proton" -> {
                    put("user", d.user.ifEmpty { d.proton.user })
                    put("folder", ownFolder(d.folder, Destination.DEFAULT_FOLDER))
                }
            }
        }.toString()
    }

    /**
     * A new place under [id] from what WM Keyboard sent, or null when it does not parse or names a
     * kind WM Pass does not know. Not saved: it fills in the place sheet, where the person signs in
     * or picks the folder where one is needed, and saves.
     */
    fun decode(raw: String, id: String): Destination? {
        val o = runCatching { json.parseToJsonElement(raw).jsonObject }.getOrNull() ?: return null
        val kind = o.str("kind")
        if (kind !in KINDS) return null
        val base = Destination(id = id, kind = kind, label = o.str("name").trim())
        val folder = o.str("folder").trim()
        return when (kind) {
            "webdav" -> base.copy(url = o.str("url"), user = o.str("user"), password = o.str("password"))
            "s3" -> base.copy(
                endpoint = o.str("endpoint"),
                region = o.str("region").ifEmpty { "us-east-1" },
                bucket = o.str("bucket"),
                prefix = o.str("prefix"),
                accessKeyId = o.str("accessKeyId"),
                secretAccessKey = o.str("secretAccessKey"),
                pathStyle = o.flag("pathStyle", false),
            )
            "sftp" -> base.copy(
                host = o.str("host"),
                port = o.int("port", 22),
                user = o.str("user"),
                password = o.str("password"),
                privateKey = o.str("privateKey"),
                keyPassphrase = o.str("keyPassphrase"),
                path = o.str("path"),
                hostKey = o.str("hostKey"),
            )
            "ftp" -> base.copy(
                host = o.str("host"),
                port = o.int("port", 21),
                user = o.str("user"),
                password = o.str("password"),
                path = o.str("path"),
                secure = o.flag("secure", true),
            )
            "smb" -> base.copy(
                host = o.str("host"),
                port = o.int("port", 445),
                share = o.str("share"),
                path = o.str("path"),
                domain = o.str("domain"),
                user = o.str("user"),
                password = o.str("password"),
                encrypt = o.flag("encrypt", true),
            )
            "nfs" -> base.copy(
                path = o.str("path"),
                nfs = NfsConfig(
                    host = o.str("host"),
                    export = o.str("export"),
                    path = o.str("path"),
                    uid = o.int("uid", NfsConfig.DEFAULT_ID),
                    gid = o.int("gid", NfsConfig.DEFAULT_ID),
                    port = o.int("port", 0),
                    mountPort = o.int("mountPort", 0),
                ),
            )
            "git" -> base.copy(
                provider = GitProvider.of(o.str("provider")).id,
                url = o.str("url"),
                repository = o.str("repository"),
                branch = o.str("branch"),
                path = o.str("path"),
                token = o.str("token"),
                allowPublic = o.flag("allowPublic", false),
            )
            "imap" -> {
                val security = ImapSecurity.of(o.str("security"))
                base.copy(
                    host = o.str("host"),
                    port = o.int("port", security.defaultPort),
                    security = security.id,
                    user = o.str("user"),
                    password = o.str("password"),
                    folder = folder,
                )
            }
            "synology", "qnap" -> base.copy(
                user = o.str("user"),
                password = o.str("password"),
                path = o.str("path"),
                nas = NasConfig(
                    server = o.str("host"),
                    user = o.str("user"),
                    password = o.str("password"),
                    path = o.str("path"),
                    ownCertificate = o.flag("ownCertificate", false),
                    certPin = o.str("certPin"),
                ),
            )
            "drive" -> base.copy(driveSpace = DriveSpace.of(o.str("driveSpace")).id, folder = folder)
            "box", "mega", "proton" -> base.copy(user = o.str("user"), folder = folder)
            "pcloud" -> base.copy(
                user = o.str("user"),
                folder = folder,
                pcloud = PCloudConfig(region = if (o.str("pcloudRegion") == "eu") PCloudRegion.EU else PCloudRegion.US, email = o.str("user")),
            )
            "icloud" -> base.copy(user = o.str("user"), password = o.str("password"), folder = folder)
            else -> base
        }
    }

    /** [folder], or none when it is this app's own default name for one. */
    private fun ownFolder(folder: String, default: String): String = folder.trim().takeIf { it != default }.orEmpty()

    private fun JsonObject.str(key: String): String =
        runCatching { this[key]?.jsonPrimitive?.contentOrNull }.getOrNull().orEmpty()

    private fun JsonObject.int(key: String, default: Int): Int =
        runCatching { this[key]?.jsonPrimitive?.intOrNull }.getOrNull() ?: default

    private fun JsonObject.flag(key: String, default: Boolean): Boolean =
        runCatching { this[key]?.jsonPrimitive?.booleanOrNull }.getOrNull() ?: default
}

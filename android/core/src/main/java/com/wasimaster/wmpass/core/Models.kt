package com.wasimaster.wmpass.core

import androidx.compose.runtime.Immutable
import kotlinx.serialization.SerialName
import kotlinx.serialization.Serializable
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.longOrNull

// The views `wmpass-session` sends. None of them can hold a secret value.

@Immutable
@Serializable
data class Summary(
    val id: String,
    val kind: String,
    val title: String = "",
    val subtitle: String = "",
    val site: String? = null,
    val app: String? = null,
    val icon: String? = null,
    val color: String? = null,
    val folder: String? = null,
    val tags: List<String> = emptyList(),
    val pinned: Boolean = false,
    val favourite: Boolean = false,
    val hidden: Boolean = false,
    val trashed: Boolean = false,
    @SerialName("has_password") val hasPassword: Boolean = false,
    @SerialName("has_otp") val hasOtp: Boolean = false,
    @SerialName("has_passkey") val hasPasskey: Boolean = false,
    val attachments: Int = 0,
    @SerialName("modified_ms") val modifiedMs: Long = 0,
    @SerialName("destroy_at_ms") val destroyAtMs: Long? = null,
)

@Immutable
@Serializable
data class FieldSpec(
    val key: String,
    val label: String,
    val input: String,
    val secret: Boolean = false,
    val choices: List<String> = emptyList(),
)

@Immutable
@Serializable
data class KindSpec(val kind: String, val fields: List<FieldSpec>)

@Immutable
@Serializable
data class FieldView(
    val key: String,
    val label: String,
    val input: String,
    val secret: Boolean = false,
    val choices: List<String> = emptyList(),
    @SerialName("has_value") val hasValue: Boolean = false,
    val value: JsonElement? = null,
) {
    /** The plain value as text; empty for a secret. */
    val text: String get() = (value as? JsonPrimitive)?.contentOrNull.orEmpty()
    val flag: Boolean get() = (value as? JsonPrimitive)?.booleanOrNull ?: false
    val number: Long? get() = (value as? JsonPrimitive)?.longOrNull
}

@Immutable
@Serializable
data class CustomFieldView(
    val index: Int,
    val name: String,
    val kind: String,
    @SerialName("has_value") val hasValue: Boolean = false,
    val value: String? = null,
)

@Immutable
@Serializable
data class AttachmentView(val blob: String, val name: String, val mime: String = "", val size: Long = 0)

@Immutable
@Serializable
data class PasskeyView(
    @SerialName("credential_id") val credentialId: String,
    @SerialName("rp_id") val rpId: String,
    @SerialName("rp_name") val rpName: String = "",
    @SerialName("user_name") val userName: String = "",
    @SerialName("created_ms") val createdMs: Long = 0,
)

@Immutable
@Serializable
data class OtpView(
    val issuer: String = "",
    val account: String = "",
    val kind: String = "totp",
    val digits: Int = 6,
    val period: Int? = null,
)

@Immutable
@Serializable
data class RevisionView(val index: Int, val field: String, @SerialName("at_ms") val atMs: Long)

@Immutable
@Serializable
data class RemindersView(
    @SerialName("change_every_days") val changeEveryDays: Int? = null,
    @SerialName("expires_ms") val expiresMs: Long? = null,
    @SerialName("warn_days") val warnDays: Int = 0,
)

@Immutable
@Serializable
data class Detail(
    val id: String,
    val kind: String,
    val title: String = "",
    val subtitle: String = "",
    val site: String? = null,
    val icon: String? = null,
    val color: String? = null,
    val folder: String? = null,
    val tags: List<String> = emptyList(),
    val pinned: Boolean = false,
    val favourite: Boolean = false,
    val hidden: Boolean = false,
    val trashed: Boolean = false,
    @SerialName("has_password") val hasPassword: Boolean = false,
    @SerialName("modified_ms") val modifiedMs: Long = 0,
    @SerialName("destroy_at_ms") val destroyAtMs: Long? = null,
    @SerialName("created_ms") val createdMs: Long = 0,
    val fields: List<FieldView> = emptyList(),
    @SerialName("custom_fields") val customFields: List<CustomFieldView> = emptyList(),
    @SerialName("has_notes") val hasNotes: Boolean = false,
    val sites: List<JsonElement> = emptyList(),
    val otp: OtpView? = null,
    val passkeys: List<PasskeyView> = emptyList(),
    @SerialName("attachment_list") val attachmentList: List<AttachmentView> = emptyList(),
    val links: List<String> = emptyList(),
    val reminders: RemindersView = RemindersView(),
    val history: List<RevisionView> = emptyList(),
    val alias: String? = null,
    val questions: List<String> = emptyList(),
)

@Immutable
@Serializable
data class OtpRow(
    val id: String,
    val title: String = "",
    val site: String? = null,
    val icon: String? = null,
    val otp: OtpView = OtpView(),
    val code: String? = null,
    @SerialName("next_code") val nextCode: String? = null,
    @SerialName("seconds_left") val secondsLeft: Int? = null,
)

@Immutable
@Serializable
data class FolderView(val id: String, val name: String, val parent: String? = null)

@Immutable
@Serializable
data class HealthView(
    val weak: List<String> = emptyList(),
    val reused: List<List<String>> = emptyList(),
    val old: List<String> = emptyList(),
    val breached: List<JsonElement> = emptyList(),
    @SerialName("missing_two_factor") val missingTwoFactor: List<String> = emptyList(),
    @SerialName("insecure_url") val insecureUrl: List<String> = emptyList(),
    val duplicates: List<List<String>> = emptyList(),
    val expiring: List<String> = emptyList(),
    val score: Int = 100,
)

@Immutable
@Serializable
data class StrengthView(
    val score: Int = 0,
    @SerialName("crack_slow_hash") val crackSlowHash: String = "",
    @SerialName("crack_fast_hash") val crackFastHash: String = "",
    val warning: String? = null,
    val suggestions: List<String> = emptyList(),
)

@Immutable
@Serializable
data class CompareView(
    val first: StrengthView = StrengthView(),
    val second: StrengthView = StrengthView(),
    val winner: String = "even",
    val factor: Double = 1.0,
)

@Immutable
@Serializable
data class MatchRow(val entry: Summary, val level: String)

@Immutable
@Serializable
data class SlotView(val index: Int, val label: String = "")

@Immutable
@Serializable
data class VaultInfo(
    val name: String = "",
    val generation: Long = 0,
    @SerialName("read_only") val readOnly: Boolean = false,
    val dirty: Boolean = false,
    @SerialName("shelf_open") val shelfOpen: Boolean = false,
    val entries: Int = 0,
    @SerialName("trash_days") val trashDays: Int = 30,
    @SerialName("history_limit") val historyLimit: Int = 0,
)

@Immutable
@Serializable
data class ImportPreview(
    val format: String = "",
    val count: Int = 0,
    val titles: List<String> = emptyList(),
    val skipped: List<String> = emptyList(),
)

@Immutable
@Serializable
data class PresetView(val name: String, val recipe: kotlinx.serialization.json.JsonElement)

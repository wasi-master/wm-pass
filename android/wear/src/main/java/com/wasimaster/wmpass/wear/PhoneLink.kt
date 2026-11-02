package com.wasimaster.wmpass.wear

import android.content.Context
import com.google.android.gms.wearable.Wearable
import kotlinx.coroutines.tasks.await
import kotlinx.coroutines.withTimeoutOrNull
import org.json.JSONObject

/** One code as the phone gave it. Lives on screen only, and never on the watch's disk. */
data class CodeRow(val id: String, val title: String, val code: String, val secondsLeft: Int)

data class FavouriteRow(val id: String, val title: String, val username: String, val hasPassword: Boolean)

sealed interface PhoneAnswer {
    data class Open(val codes: List<CodeRow>, val favourites: List<FavouriteRow>) : PhoneAnswer

    /** The vault on the phone is locked. */
    data object Locked : PhoneAnswer

    /** Turned off in WM Pass on the phone. */
    data object Off : PhoneAnswer

    /** The phone is out of reach, or has no WM Pass. */
    data object Away : PhoneAnswer
}

/** The watch asks and the phone answers each time, so nothing secret rests on the watch. */
object PhoneLink {
    private const val TIMEOUT_MS = 8_000L

    private suspend fun ask(context: Context, path: String, body: ByteArray): JSONObject? = withTimeoutOrNull(TIMEOUT_MS) {
        runCatching {
            val nodes = Wearable.getNodeClient(context).connectedNodes.await()
            val phone = nodes.firstOrNull { it.isNearby } ?: nodes.firstOrNull() ?: return@runCatching null
            val reply = Wearable.getMessageClient(context).sendRequest(phone.id, path, body).await()
            JSONObject(String(reply))
        }.getOrNull()
    }

    suspend fun items(context: Context): PhoneAnswer {
        val reply = ask(context, "/wmpass/items", ByteArray(0)) ?: return PhoneAnswer.Away
        return when (reply.optString("state")) {
            "open" -> {
                val codes = reply.optJSONArray("codes")
                val favourites = reply.optJSONArray("favourites")
                PhoneAnswer.Open(
                    codes = List(codes?.length() ?: 0) { i ->
                        val row = codes!!.getJSONObject(i)
                        CodeRow(row.getString("id"), row.optString("title"), row.optString("code"), row.optInt("seconds"))
                    },
                    favourites = List(favourites?.length() ?: 0) { i ->
                        val row = favourites!!.getJSONObject(i)
                        FavouriteRow(row.getString("id"), row.optString("title"), row.optString("username"), row.optBoolean("has_password"))
                    },
                )
            }
            "locked" -> PhoneAnswer.Locked
            else -> PhoneAnswer.Off
        }
    }

    suspend fun password(context: Context, id: String): String? =
        ask(context, "/wmpass/password", id.toByteArray())?.optString("password")?.takeIf { it.isNotEmpty() }
}

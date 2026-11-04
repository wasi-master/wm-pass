package com.wasimaster.wmpass.system

import com.wasimaster.wmpass.core.Engine
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * Where a site lets you change your password. The core works it out, so every app gives the
 * same answer: Apple's list of known pages first, and for any other website its
 * `/.well-known/change-password` address. Apps and addresses on the local network get none.
 */
object ChangePassword {
    /** The page for a saved address, or null when it names no website. */
    suspend fun url(site: String?): String? {
        if (site.isNullOrBlank()) return null
        return runCatching {
            Engine.call("site.change_password", args = { put("site", site) }).obj["url"]?.jsonPrimitive?.contentOrNull
        }.getOrNull()
    }
}

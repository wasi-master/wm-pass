package com.wasimaster.wmpass.system

import com.wasimaster.wmpass.core.OtpView
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.SecretBytes
import kotlinx.serialization.json.put

/**
 * The code to hand over now, for a fill or a type. A counter code ([OtpView.period] null) is used
 * up by the site that takes it, so it moves the counter on: [Pass.code] alone would give the same
 * code again next time, which the site has already seen and turns down.
 */
internal suspend fun codeToUse(id: String, otp: OtpView?): SecretBytes? =
    if (otp != null && otp.period == null) {
        Pass.change("otp.hotp_next") { put("id", id) }.secret
    } else {
        Pass.code(id)
    }

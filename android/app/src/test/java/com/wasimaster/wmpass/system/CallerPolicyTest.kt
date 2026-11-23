package com.wasimaster.wmpass.system

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class CallerPolicyTest {
    @Test
    fun wmTranslateReachesOnlyApiKeyCalls() {
        for (m in listOf("status", "api_keys", "save_api_key", "value")) assertTrue(m, CallerPolicy.methodAllowed(Caller.TRANSLATE, m))
        for (m in listOf("items", "item", "copy", "add_login", "set_password", "generate", "unlock", "unlock_prompt", "lock", "logins", "code", "action")) {
            assertFalse(m, CallerPolicy.methodAllowed(Caller.TRANSLATE, m))
        }
    }

    @Test
    fun wmTranslateReadsOnlyTheKeyOrSecretOfAnApiKeyItem() {
        assertTrue(CallerPolicy.valueAllowed(Caller.TRANSLATE, "api", "key"))
        assertTrue(CallerPolicy.valueAllowed(Caller.TRANSLATE, "api", "secret"))
        assertFalse(CallerPolicy.valueAllowed(Caller.TRANSLATE, "login", "password"))
        assertFalse(CallerPolicy.valueAllowed(Caller.TRANSLATE, "api", "notes"))
        assertFalse(CallerPolicy.valueAllowed(Caller.TRANSLATE, "card", "key"))
        assertFalse(CallerPolicy.valueAllowed(Caller.TRANSLATE, null, "key"))
    }

    @Test
    fun theKeyboardKeepsEveryCall() {
        assertTrue(CallerPolicy.methodAllowed(Caller.KEYBOARD, "items"))
        assertTrue(CallerPolicy.valueAllowed(Caller.KEYBOARD, "login", "password"))
    }
}

package com.wasimaster.wmpass.system

import android.app.assist.AssistStructure
import android.text.InputType
import android.view.View
import android.view.autofill.AutofillId
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

@RunWith(RobolectricTestRunner::class)
class AutofillParseTest {
    // AssistStructure has no public way to build one, so the test fills its fields directly.
    private fun <T> blank(type: Class<T>): T {
        val unsafeType = Class.forName("sun.misc.Unsafe")
        val unsafe = unsafeType.getDeclaredField("theUnsafe").apply { isAccessible = true }.get(null)
        return type.cast(unsafeType.getMethod("allocateInstance", Class::class.java).invoke(unsafe, type))
    }

    private fun Any.put(name: String, value: Any?) {
        var type: Class<*>? = javaClass
        while (type != null) {
            type.declaredFields.firstOrNull { it.name == name }?.let {
                it.isAccessible = true
                it.set(this, value)
                return
            }
            type = type.superclass
        }
        error("${javaClass.name} has no field $name")
    }

    private fun id(n: Int): AutofillId =
        AutofillId::class.java.getDeclaredConstructor(Int::class.javaPrimitiveType).apply { isAccessible = true }.newInstance(n)

    private fun field(n: Int, hints: Array<String?>?, inputType: Int) = blank(AssistStructure.ViewNode::class.java).apply {
        put("mAutofillId", id(n))
        put("mAutofillHints", hints)
        put("mAutofillType", View.AUTOFILL_TYPE_TEXT)
        put("mInputType", inputType)
    }

    private fun screen(vararg fields: AssistStructure.ViewNode): AssistStructure {
        val root = blank(AssistStructure.ViewNode::class.java).apply { put("mChildren", fields) }
        val window = blank(AssistStructure.WindowNode::class.java).apply { put("mRoot", root) }
        return blank(AssistStructure::class.java).apply {
            put("mHaveData", true)
            put("mWindowNodes", arrayListOf(window))
        }
    }

    @Test
    fun `a null among an app's autofill hints is passed over`() {
        val form = parseForm(
            screen(
                field(1, arrayOf(null, "emailAddress"), InputType.TYPE_CLASS_TEXT),
                field(2, arrayOf(null), InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD),
            ),
        )
        assertEquals(id(1), form.username)
        assertEquals(id(2), form.password)
    }
}

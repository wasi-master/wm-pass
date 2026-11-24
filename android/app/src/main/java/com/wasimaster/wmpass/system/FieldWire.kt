package com.wasimaster.wmpass.system

/**
 * How a field in WM Pass tells WM Keyboard what its own buttons can do, so the keyboard can offer
 * the same on its strip: **Scan the card** in a card's number, **Fill in from a QR code** in a
 * Wi-Fi network's fields. The keyboard draws the chips with its own words; a tap on one asks WM
 * Pass, through [KeyboardProvider]'s `action` call, to press that button for the person.
 *
 * WM Keyboard reads the same format in its own `FieldWire`. A change here is a change there too.
 *
 * The field says it in `EditorInfo.privateImeOptions`, which a field fills from Compose's
 * `PlatformImeOptions`. Several apps' options can share that string, separated by commas, so WM
 * Pass's is one item of its own:
 *
 * ```
 * com.wasimaster.suite.field=1;token=Kx3m9QaZ0bLw7RtE;actions=card_scan
 * ```
 *
 * - `1` is the format's version. A reader takes the keys it knows from any version and leaves
 *   the rest.
 * - `token` names the screen showing the field, made fresh for each screen ([FieldActions]). The
 *   keyboard sends it back with the tap, so a tap meant for one screen never presses a button on
 *   another, and a screen that has closed takes nothing.
 * - `actions` are the buttons, joined by `+`, as ids from [ACTIONS]. An id the reader does not know
 *   is skipped, so an older keyboard shows the chips it has words for and no others.
 *
 * Nothing in it is secret. The keyboard reads it only from a field whose package is WM Pass and
 * whose signing certificate is WM Pass's, and WM Pass runs an action only for a caller that is the
 * real WM Keyboard.
 */
object FieldWire {
    const val VERSION = 1

    /** Where WM Pass's item starts inside `privateImeOptions`. */
    const val PREFIX = "com.wasimaster.suite.field="

    /** [KeyboardProvider]'s call that presses a button: the action id as its argument, [TOKEN] in its extras. */
    const val ACTION_CALL = "action"

    /** The extra holding the screen's token, and the key in the tag. */
    const val TOKEN = "token"

    /** The reply's key: whether a screen took the action. */
    const val DONE = "done"

    /** A card's **Scan the card**: its number, and with Google's reader its expiry and name too. */
    const val CARD_SCAN = "card_scan"

    /** A Wi-Fi network's **Fill in from a QR code**. */
    const val WIFI_QR = "wifi_qr"

    /** A Wi-Fi network's **Pick a network saved on this phone**. */
    const val WIFI_SAVED = "wifi_saved"

    /**
     * WM Translate's **Fill from WM Pass**, in a provider's key field. WM Translate's fields carry
     * the same tag as WM Pass's, and the keyboard asks WM Translate's provider for the tap.
     */
    const val PASS_KEY = "pass_key"

    val ACTIONS = setOf(CARD_SCAN, WIFI_QR, WIFI_SAVED, PASS_KEY)

    private val TOKEN_SHAPE = Regex("[A-Za-z0-9_-]{8,64}")
    private val ACTION_SHAPE = Regex("[a-z0-9_]{1,32}")
    private const val MOST_ACTIONS = 8

    /** What a field's tag says: the screen's [token] and the [actions] the reader knows. */
    data class Tag(val token: String, val actions: List<String>)

    /** The item for [actions] on the screen called [token], or null when there is nothing to offer. */
    fun tag(token: String, actions: List<String>): String? {
        val known = actions.filter { it in ACTIONS }.distinct()
        if (known.isEmpty() || !TOKEN_SHAPE.matches(token)) return null
        return "$PREFIX$VERSION;$TOKEN=$token;actions=${known.joinToString("+")}"
    }

    /** WM Pass's item in a field's whole `privateImeOptions`, or null when there is none or it is not well formed. */
    fun parse(privateImeOptions: String?): Tag? {
        val item = privateImeOptions?.split(',')?.map { it.trim() }?.firstOrNull { it.startsWith(PREFIX) } ?: return null
        val parts = item.removePrefix(PREFIX).split(';')
        if (parts.first().toIntOrNull()?.takeIf { it >= 1 } == null) return null
        val keys = parts.drop(1).mapNotNull { part ->
            val at = part.indexOf('=')
            if (at <= 0) null else part.substring(0, at) to part.substring(at + 1)
        }.toMap()
        val token = keys[TOKEN]?.takeIf { TOKEN_SHAPE.matches(it) } ?: return null
        val actions = keys["actions"].orEmpty().split('+')
            .filter { ACTION_SHAPE.matches(it) && it in ACTIONS }
            .distinct()
            .take(MOST_ACTIONS)
        if (actions.isEmpty()) return null
        return Tag(token, actions)
    }
}

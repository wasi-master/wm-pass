package com.wasimaster.wmpass.system

import android.app.Activity
import android.os.Bundle
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.core.Pass
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

/**
 * Answers panic signals from apps such as Ripple, once the owner has said what a signal does.
 * Only the apps they chose are listened to, so no other app can lock the vault over and over or
 * erase it. With Lock chosen and no app named, any app may ask for a lock, which costs nothing
 * but a retype. A wipe always needs a named app, since one we cannot identify must not be able to
 * erase a vault.
 */
class PanicActivity : Activity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val prefs = Prefs.of(this)
        val caller = callingActivity?.packageName
        // Chosen by name, and still signed as it was when chosen. An app chosen before pins
        // existed is pinned when the app next starts, or when Settings, Security is next opened.
        val chosen = caller != null && caller in prefs.panicApps &&
            PanicPins.matches(prefs.panicPins[caller].orEmpty(), Trust.signersOf(this, caller))
        val act = when (prefs.panicAction) {
            "wipe" -> if (chosen) "wipe" else null
            "lock" -> if (chosen || prefs.panicApps.isEmpty()) "lock" else null
            else -> null
        }
        val trigger = intent?.action == ACTION_TRIGGER
        // An activity with no display has to finish before onResume ends, or the system throws.
        // The work goes on without it, in a scope of its own.
        finish()
        if (!trigger || act == null) return
        CoroutineScope(Dispatchers.Main.immediate).launch {
            // Nothing left on screen to say a failure to, and it must not end the process half way.
            runCatching {
                when (act) {
                    "wipe" -> Pass.wipe()
                    "lock" -> Pass.panic()
                }
            }
        }
    }

    companion object {
        const val ACTION_TRIGGER = "info.guardianproject.panic.action.TRIGGER"

        /**
         * Apps known to send PanicKit's signal, put first when choosing: Guardian Project's
         * Ripple and Amnesty International's Panic Button.
         */
        val KNOWN = mapOf(
            "info.guardianproject.ripple" to "Ripple",
            "org.iilab.pb" to "Panic Button",
        )
    }
}

/**
 * The signing certificates a panic app had when it was chosen, and the test a signal's sender must
 * pass against them. Kept as `package=sha256|sha256;...`.
 */
object PanicPins {
    /** An app's signers now ([current]), and every certificate its key has rotated through ([lineage]). */
    data class Signers(val current: Set<String>, val lineage: Set<String>)

    fun encode(pins: Map<String, Set<String>>): String =
        pins.filterValues { it.isNotEmpty() }.toSortedMap().entries.joinToString(";") { (name, certs) -> "$name=${certs.sorted().joinToString("|")}" }

    fun decode(text: String): Map<String, Set<String>> =
        text.split(';').mapNotNull { pair ->
            val name = pair.substringBefore('=', "").trim()
            val certs = pair.substringAfter('=', "").split('|').map { it.trim().lowercase() }.filter { it.isNotEmpty() }.toSet()
            if (name.isEmpty() || certs.isEmpty()) null else name to certs
        }.toMap()

    /**
     * Whether [caller] is the app that was pinned: the same signers, or, for an app with one
     * signer, a key rotated since whose lineage, which Android itself checks, names the pinned one.
     * No pin, or no signers to read, never matches.
     */
    fun matches(pinned: Set<String>, caller: Signers?): Boolean {
        if (pinned.isEmpty() || caller == null || caller.current.isEmpty()) return false
        if (caller.current == pinned) return true
        return pinned.size == 1 && caller.current.size == 1 && pinned.single() in caller.lineage
    }

    /** Pins every chosen app that has no pin yet, by its signers now. One not installed stays unpinned. */
    fun pinMissing(context: android.content.Context, prefs: Prefs) {
        val pins = prefs.panicPins
        val fresh = prefs.panicApps.filter { it !in pins }.mapNotNull { name -> Trust.signersOf(context, name)?.let { name to it.current } }.toMap()
        if (fresh.isNotEmpty()) prefs.pinPanicApps(fresh)
    }
}

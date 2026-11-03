package com.wasimaster.wmpass.ui.help

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import com.wasimaster.wmpass.app.Prefs

/**
 * What the guide is doing right now: which topics are open in the sheet, which tour is running and
 * on which step, and whether the welcome is up. One for the whole app, like the settings kit's
 * highlight, because a "?" on any screen and the door's link all open the same sheet.
 */
object Guide {
    /** Topics opened in the sheet, the last one showing. Empty means the sheet is closed. */
    val stack = mutableStateListOf<String>()

    /** Explanations with no topic behind them, opened from a row the guide does not know. */
    private val loose = HashMap<String, Pair<String, String>>()

    var tour by mutableStateOf<HelpTour?>(null)
        private set
    var step by mutableIntStateOf(0)
    var welcome by mutableStateOf(false)

    val current: String? get() = stack.lastOrNull()

    /** Opens [id] in the sheet, on top of whatever it shows. False when the phone has no such topic. */
    fun open(id: String): Boolean {
        if (Help.topic(id) == null) return false
        if (stack.lastOrNull() != id) stack.add(id)
        return true
    }

    /** Shows a row's own words in the sheet, for a row with no topic of its own. */
    fun openText(title: String, detail: String) {
        val id = "text:$title"
        loose[id] = title to detail
        if (stack.lastOrNull() != id) stack.add(id)
    }

    fun looseText(id: String): Pair<String, String>? = loose[id]

    fun back() {
        if (stack.isNotEmpty()) stack.removeAt(stack.lastIndex)
    }

    fun close() {
        stack.clear()
    }

    /** Whether a tour can run here: the phone has it. */
    fun hasTour(id: String?): Boolean = id != null && Help.tour(id) != null

    fun startTour(id: String): Boolean {
        val found = Help.tour(id) ?: return false
        close()
        welcome = false
        step = 0
        tour = found
        return true
    }

    /** Ends the tour. One followed to its last step is marked done in Settings, Guide. */
    fun endTour(prefs: Prefs?, finished: Boolean) {
        val ended = tour
        tour = null
        step = 0
        if (finished && ended != null) prefs?.markTourDone(ended.id)
    }

    fun dismissWelcome(prefs: Prefs) {
        welcome = false
        prefs.helpWelcomeSeen.set(true)
    }

    /** Opens the topic for a settings row, found by its title on [route], or the row's own words. */
    fun openRow(route: String?, title: String, detail: String) {
        // A row named differently from its topic still carries the topic's own words as its detail.
        val topic = Help.forTitle(route, title) ?: Help.topics.firstOrNull { it.plain == detail }
        if (topic == null || !open(topic.id)) openText(title, detail)
    }
}

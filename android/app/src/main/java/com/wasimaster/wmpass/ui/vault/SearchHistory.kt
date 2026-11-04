package com.wasimaster.wmpass.ui.vault

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.Stable
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.theme.palette
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.launch
import kotlinx.serialization.json.buildJsonObject
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.putJsonArray

/**
 * What was searched for lately, newest first. A search counts once it found something that was
 * opened, so half-typed words do not pile up.
 *
 * The words can name sites and accounts, so they are kept inside the vault, sealed with
 * everything else in it, and never in the plain settings file. They go where the vault goes;
 * a vault that keeps no settings of its own (an account held elsewhere) keeps them only until
 * it locks.
 */
@Stable
class SearchHistory(private val scope: CoroutineScope) {
    val items = mutableStateListOf<String>()
    private var loaded = false

    fun load() {
        if (loaded) return
        loaded = true
        scope.launch {
            val kept = runCatching {
                if (!Pass.carriesSettings) return@runCatching null
                Pass.syncedSettings()[KEY]?.jsonArray?.mapNotNull { it.jsonPrimitive.contentOrNull }
            }.getOrNull().orEmpty()
            // Anything searched while it loaded stays in front.
            kept.filter { old -> items.none { it.equals(old, ignoreCase = true) } }.take(MAX - items.size).let { items.addAll(it) }
        }
    }

    fun remember(query: String) {
        val text = query.trim()
        if (text.length < 2) return
        items.removeAll { it.equals(text, ignoreCase = true) }
        items.add(0, text)
        while (items.size > MAX) items.removeAt(items.lastIndex)
        save()
    }

    fun forget(query: String) {
        items.remove(query)
        save()
    }

    fun clear() {
        items.clear()
        save()
    }

    /** Earlier searches that carry on from what is typed, for finishing it with one tap. */
    fun matching(typed: String): List<String> {
        val text = typed.trim()
        if (text.isEmpty()) return emptyList()
        return items.filter { it.contains(text, ignoreCase = true) && !it.equals(text, ignoreCase = true) }.take(3)
    }

    private fun save() {
        val list = items.toList()
        scope.launch {
            runCatching {
                if (Pass.carriesSettings) Pass.syncSettings(buildJsonObject { putJsonArray(KEY) { list.forEach { add(it) } } })
            }
        }
    }

    private companion object {
        const val KEY = "search.recent"
        const val MAX = 8
    }
}

/** Earlier searches as chips, for the glass search page, with a way to clear them. */
@OptIn(ExperimentalLayoutApi::class)
@Composable
internal fun RecentSearchChips(history: SearchHistory, onPick: (String) -> Unit) {
    FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
        com.wasimaster.wmpass.ui.parts.AppearingEach(history.items.toList()) { text ->
            Pill(text, false, icon = PassIcons.History) { onPick(text) }
        }
    }
}

@Composable
internal fun RecentSearchesTitle(history: SearchHistory) {
    val colors = palette
    Eyebrow("Searched lately") {
        Text(
            "Clear",
            color = colors.primary,
            style = MaterialTheme.typography.labelLarge,
            modifier = Modifier.clickable(onClickLabel = "Clear recent searches") { history.clear() }.padding(horizontal = 6.dp, vertical = 2.dp),
        )
    }
}

/** One earlier search as a Material row: tap to search again, the cross to forget it. */
@Composable
internal fun RecentSearchRow(text: String, onPick: () -> Unit, onForget: (() -> Unit)?, modifier: Modifier = Modifier) {
    val colors = palette
    Row(
        modifier
            .fillMaxWidth()
            .clickable(onClick = onPick)
            .padding(start = 16.dp, end = 4.dp, top = 4.dp, bottom = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Icon(PassIcons.History, null, tint = colors.textMid, modifier = Modifier.size(24.dp))
        Spacer(Modifier.width(16.dp))
        Text(text, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f).padding(vertical = 12.dp))
        if (onForget != null) {
            androidx.compose.material3.IconButton(onClick = onForget) {
                Icon(PassIcons.Close, "Remove “$text” from recent searches", tint = colors.textMid, modifier = Modifier.size(20.dp))
            }
        }
    }
}

/** Puts an earlier search back in the box. */
internal fun Shell.searchAgain(text: String) {
    searchField.setTextAndPlaceCursorAtEnd(text)
}

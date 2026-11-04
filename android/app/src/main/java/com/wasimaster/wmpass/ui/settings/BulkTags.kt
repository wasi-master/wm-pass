package com.wasimaster.wmpass.ui.settings

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.animateContentSize
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ExperimentalLayoutApi
import androidx.compose.foundation.layout.FlowRow
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.material3.Checkbox
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.material3.TriStateCheckbox
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateListOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.state.ToggleableState
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.UndoOffer
import com.wasimaster.wmsuite.settings.WmScreen
import kotlinx.coroutines.launch
import kotlinx.serialization.json.add
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray

/** How a suggestion was reached, from surest to least sure. */
internal enum class TagSource(val word: String, val label: String, val about: String) {
    List("list", "Site list", "The site is on WM Pass's list of known sites"),
    Name("name", "Item's name", "The item's name is a known site's, such as PyPI"),
    Kind("kind", "Kind", "What the item is, such as a card or an SSH key"),
    Guess("guess", "Guesses", "The offline model's best guess from the name"),
    ;

    companion object {
        fun of(word: String?) = entries.firstOrNull { it.word == word }
    }
}

/** One tag one item would get, and why. [untagged] is whether the item has no tags yet. */
private class Offer(val id: String, val title: String, val tag: String, val source: TagSource, val untagged: Boolean)

/**
 * Suggested tags for the items already in the vault, added in one go after a look. The surer
 * sources are on to start with and the guesses off, each can be turned either way, and every
 * item can be left out by hand before anything is added. The whole batch can be undone.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun BulkTagsScreen() {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    var offers by remember { mutableStateOf<List<Offer>?>(null) }
    var tick by remember { mutableStateOf(0) }
    LaunchedEffect(tick) {
        offers = runCatching {
            Pass.plainCall("categorise.all").json.jsonArray.flatMap { row ->
                val o = row.jsonObject
                val id = o["id"]!!.jsonPrimitive.content
                val title = o["title"]?.jsonPrimitive?.content.orEmpty()
                val has = o["tags"]?.jsonArray?.mapNotNull { it.jsonPrimitive.contentOrNull }.orEmpty()
                listOfNotNull(
                    o["site"]?.jsonPrimitive?.contentOrNull?.let { tag -> TagSource.of(o["source"]?.jsonPrimitive?.contentOrNull)?.let { Offer(id, title, tag, it, has.isEmpty()) } },
                    o["by_kind"]?.jsonPrimitive?.contentOrNull?.let { tag -> Offer(id, title, tag, TagSource.Kind, has.isEmpty()) },
                ).filter { it.tag !in has }
            }
        }.getOrDefault(emptyList())
    }
    var sources by rememberSaveable { mutableStateOf(setOf(TagSource.List.word, TagSource.Name.word)) }
    var onlyUntagged by rememberSaveable { mutableStateOf(false) }
    // Left out by hand: "id\ntag".
    val skipped = remember { mutableStateListOf<String>() }
    val open = remember { mutableStateListOf<String>() }
    var adding by remember { mutableStateOf(false) }
    WmScreen("Suggested tags", route = "settings/tags", onBack = { shell.nav.popBackStack() }) {
        Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(
                "Tags WM Pass would give the items you already have. Pick how sure a suggestion has to be, look through them, and leave out any you do not want.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
            val all = offers
            if (all == null) {
                Row(Modifier.fillMaxWidth().padding(24.dp), horizontalArrangement = Arrangement.Center) {
                    CircularProgressIndicator(Modifier.size(24.dp), color = colors.primary, strokeWidth = 2.dp)
                }
                return@Column
            }
            Eyebrow("Suggestions from")
            FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                TagSource.entries.forEach { source ->
                    val count = all.count { it.source == source }
                    Pill("${source.label}  $count", source.word in sources) {
                        sources = if (source.word in sources) sources - source.word else sources + source.word
                    }
                }
            }
            Text(
                TagSource.entries.joinToString("\n") { "${it.label}: ${it.about.replaceFirstChar { c -> c.lowercase() }}." },
                color = colors.textLow,
                style = MaterialTheme.typography.bodySmall,
                modifier = Modifier.padding(horizontal = 6.dp),
            )
            Row(
                Modifier.fillMaxWidth().clickable { onlyUntagged = !onlyUntagged }.padding(horizontal = 6.dp, vertical = 4.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Only items with no tags yet", color = colors.text, style = MaterialTheme.typography.bodyLarge, modifier = Modifier.weight(1f))
                com.wasimaster.wmpass.ui.vault.PassSwitch(onlyUntagged) { onlyUntagged = it }
            }
            val shown = all.filter { it.source.word in sources && (!onlyUntagged || it.untagged) }
            val chosen = shown.filter { "${it.id}\n${it.tag}" !in skipped }
            val byTag = shown.groupBy { it.tag }.toSortedMap(String.CASE_INSENSITIVE_ORDER)
            if (byTag.isEmpty()) {
                Text(
                    if (all.isEmpty()) "Every item already has the tags WM Pass would suggest." else "Nothing from these sources. Turn on another above.",
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(6.dp),
                )
            }
            byTag.forEach { (tag, group) ->
                val picked = group.count { "${it.id}\n${it.tag}" !in skipped }
                GroupCard(Modifier.animateContentSize()) {
                    Row(
                        Modifier.fillMaxWidth().clickable { if (tag in open) open.remove(tag) else open.add(tag) }.padding(start = 4.dp, end = 12.dp, top = 4.dp, bottom = 4.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        TriStateCheckbox(
                            when (picked) {
                                0 -> ToggleableState.Off
                                group.size -> ToggleableState.On
                                else -> ToggleableState.Indeterminate
                            },
                            onClick = {
                                val keys = group.map { "${it.id}\n${it.tag}" }
                                if (picked == group.size) skipped.addAll(keys - skipped.toSet()) else skipped.removeAll(keys)
                            },
                        )
                        Column(Modifier.weight(1f)) {
                            Text(tag, color = colors.text, style = MaterialTheme.typography.bodyLarge)
                            Text(
                                if (picked == group.size) "${group.size} items" else "$picked of ${group.size} items",
                                color = colors.textMid,
                                style = MaterialTheme.typography.bodySmall,
                            )
                        }
                        Icon(
                            PassIcons.ExpandMore,
                            if (tag in open) "Hide the items" else "Show the items",
                            tint = colors.textLow,
                            modifier = Modifier.graphicsLayer { rotationZ = if (tag in open) 180f else 0f },
                        )
                    }
                    AnimatedVisibility(tag in open) {
                        Column {
                            group.sortedBy { it.title.lowercase() }.forEach { offer ->
                                val key = "${offer.id}\n${offer.tag}"
                                Hairline(inset = 56.dp)
                                Row(
                                    Modifier.fillMaxWidth().clickable { if (key in skipped) skipped.remove(key) else skipped.add(key) }.padding(start = 4.dp, end = 12.dp),
                                    verticalAlignment = Alignment.CenterVertically,
                                ) {
                                    Checkbox(key !in skipped, onCheckedChange = { on -> if (on) skipped.remove(key) else skipped.add(key) })
                                    Text(offer.title.ifEmpty { "Untitled" }, color = colors.text, style = MaterialTheme.typography.bodyMedium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f))
                                    Spacer(Modifier.width(8.dp))
                                    Text(offer.source.label, color = if (offer.source == TagSource.Guess) colors.warn else colors.textLow, style = MaterialTheme.typography.labelSmall)
                                }
                            }
                        }
                    }
                }
            }
            Spacer(Modifier.height(6.dp))
            val items = chosen.map { it.id }.distinct().size
            PrimaryButton(
                when {
                    adding -> "Adding…"
                    chosen.isEmpty() -> "Nothing chosen"
                    else -> "Add ${chosen.size} ${if (chosen.size == 1) "tag" else "tags"} to $items ${if (items == 1) "item" else "items"}"
                },
                Modifier.fillMaxWidth(),
                enabled = chosen.isNotEmpty() && !adding,
                icon = PassIcons.Sell,
            ) {
                adding = true
                val batch = chosen.groupBy({ it.tag }, { it.id })
                scope.launch {
                    suspend fun apply(op: String) = batch.forEach { (tag, ids) ->
                        Pass.change("entries.bulk") {
                            putJsonArray("ids") { ids.forEach { add(it) } }
                            put("op", op)
                            put("tag", tag)
                        }
                    }
                    runCatching { apply("tag_add") }
                        .onSuccess {
                            shell.haptics.confirm()
                            shell.undo = UndoOffer("Tagged $items ${if (items == 1) "item" else "items"}") {
                                apply("tag_remove")
                                tick++
                            }
                        }
                        .onFailure { shell.haptics.reject() }
                    skipped.clear()
                    adding = false
                    tick++
                }
            }
            Spacer(Modifier.height(40.dp))
        }
    }
}

package com.wasimaster.wmpass.ui.vault

import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.foundation.text.input.setTextAndPlaceCursorAtEnd
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.RadioButton
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.produceState
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.ui.door.PasswordPill
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.parts.StrengthBar
import com.wasimaster.wmpass.ui.theme.palette
import kotlinx.coroutines.launch
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.JsonElement
import kotlinx.serialization.json.JsonObject
import kotlinx.serialization.json.JsonPrimitive
import kotlinx.serialization.json.add
import kotlinx.serialization.json.booleanOrNull
import kotlinx.serialization.json.contentOrNull
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.longOrNull
import kotlinx.serialization.json.put
import kotlinx.serialization.json.putJsonArray
import kotlinx.serialization.json.putJsonObject

/** One item in a merge, as the plan names it. */
private class MergeItem(val id: String, val title: String, val subtitle: String, val changedMs: Long)

/** A value one or more of the items hold for a field. [shown] is null for a secret. */
private class MergeValue(val ids: List<String>, val shown: JsonElement?)

/** A field the items fill differently, and how a losing value can be kept. */
private class MergeQuestion(val key: String, val label: String, val secret: Boolean, val both: String, val values: List<MergeValue>)

private fun shownText(key: String, shown: JsonElement?): String? {
    val primitive = shown as? JsonPrimitive ?: return null
    primitive.booleanOrNull?.let { return if (it) "On" else "Off" }
    if (key.endsWith("_ms")) primitive.longOrNull?.let { return whenText(it) }
    return primitive.contentOrNull
}

/**
 * Folds several items into one. First which item stays, then a choice for each field the items
 * fill differently; everything else joins up by itself, and the others go to the trash. Opened
 * from a selection in the vault and from Health's groups.
 */
@Composable
fun MergeSheet(ids: List<String>, onDismiss: () -> Unit, onDone: () -> Unit = {}) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    var problem by remember { mutableStateOf<String?>(null) }
    val plan by produceState<Pair<List<MergeItem>, List<MergeQuestion>>?>(null, ids) {
        value = try {
            val reply = Pass.plainCall("entries.merge_plan") { putJsonArray("ids") { ids.forEach { add(it) } } }.obj
            val items = reply["items"]!!.jsonArray.map {
                val o = it.jsonObject
                MergeItem(o["id"]!!.jsonPrimitive.content, o["title"]?.jsonPrimitive?.content.orEmpty(), o["subtitle"]?.jsonPrimitive?.content.orEmpty(), o["changed_ms"]?.jsonPrimitive?.longOrNull ?: 0L)
            }
            val questions = reply["questions"]!!.jsonArray.map {
                val o = it.jsonObject
                MergeQuestion(
                    o["key"]!!.jsonPrimitive.content,
                    o["label"]!!.jsonPrimitive.content,
                    o["secret"]?.jsonPrimitive?.booleanOrNull ?: false,
                    o["both"]?.jsonPrimitive?.content ?: "none",
                    o["values"]!!.jsonArray.map { v ->
                        val value = v.jsonObject
                        MergeValue(value["ids"]!!.jsonArray.map { id -> id.jsonPrimitive.content }, value["shown"]?.takeIf { s -> s !is kotlinx.serialization.json.JsonNull })
                    },
                )
            }
            items to questions
        } catch (failure: NativeFailure) {
            problem = failure.message?.replaceFirstChar { it.uppercase() }
            null
        }
    }
    // The one that stays: the most recently changed, unless the owner picks another.
    var keep by remember { mutableStateOf<String?>(null) }
    val picked = remember { mutableStateMapOf<String, Int>() }
    val both = remember { mutableStateMapOf<String, Boolean>() }
    val revealed = remember { mutableStateMapOf<String, String>() }
    var merging by remember { mutableStateOf(false) }
    LaunchedEffect(plan) {
        val items = plan?.first ?: return@LaunchedEffect
        if (keep == null) keep = items.maxByOrNull { it.changedMs }?.id
    }
    PassSheet(onDismiss, "Merge ${ids.size} items") {
        val current = plan
        when {
            problem != null -> Text(problem.orEmpty(), color = colors.danger, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(4.dp))
            current == null -> Row(Modifier.fillMaxWidth().padding(16.dp), horizontalArrangement = Arrangement.Center) {
                CircularProgressIndicator(Modifier.size(24.dp), color = colors.primary, strokeWidth = 2.dp)
            }
            else -> {
                val (items, questions) = current
                val names = items.associate { it.id to it.title.ifEmpty { "Untitled" } }
                Text(
                    "Websites, tags, files, passkeys and custom fields from each join the one that stays. The others go to the trash, where they can still be restored.",
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                    modifier = Modifier.padding(start = 4.dp, end = 4.dp, bottom = 6.dp),
                )
                Eyebrow("Which one stays")
                items.forEach { item ->
                    Choice(keep == item.id, item.title.ifEmpty { "Untitled" }, listOf(item.subtitle, "changed ${whenText(item.changedMs)}").filter { it.isNotEmpty() }.joinToString(" · ")) {
                        keep = item.id
                        picked.clear()
                    }
                }
                if (questions.isEmpty()) {
                    Text("Nothing else differs between them.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(4.dp))
                }
                questions.forEach { question ->
                    Eyebrow(question.label)
                    val chosen = picked[question.key] ?: question.values.indexOfFirst { keep in it.ids }.takeIf { it >= 0 } ?: 0
                    question.values.forEachIndexed { index, value ->
                        val from = value.ids.mapNotNull { names[it] }.distinct().joinToString(", ")
                        val text = shownText(question.key, value.shown)
                        val holder = value.ids.first()
                        val revealKey = "${question.key}/$holder"
                        Choice(
                            chosen == index,
                            text ?: revealed[revealKey] ?: "••••••••",
                            "From $from",
                            trailing = if (question.secret && question.key != "otp") {
                                {
                                    val shown = revealed[revealKey] != null
                                    RowIconButton(if (shown) PassIcons.VisibilityOff else PassIcons.Visibility, if (shown) "Hide" else "Show") {
                                        if (shown) {
                                            revealed.remove(revealKey)
                                        } else {
                                            scope.launch { runCatching { revealed[revealKey] = Pass.reveal(holder, question.key).consumeText() } }
                                        }
                                    }
                                }
                            } else {
                                null
                            },
                        ) { picked[question.key] = index }
                    }
                    when (question.both) {
                        "history" -> Note("The other stays in this item's password history.")
                        "field", "joined" -> Row(
                            Modifier.fillMaxWidth().clickable { both[question.key] = !(both[question.key] ?: false) }.padding(horizontal = 4.dp, vertical = 6.dp),
                            verticalAlignment = Alignment.CenterVertically,
                        ) {
                            Text(
                                if (question.both == "joined") "Keep both, one after the other" else "Keep the other too, as a field of its own",
                                color = colors.text,
                                style = MaterialTheme.typography.bodyMedium,
                                modifier = Modifier.weight(1f),
                            )
                            PassSwitch(both[question.key] ?: false) { both[question.key] = it }
                        }
                    }
                }
                Spacer(Modifier.height(12.dp))
                PrimaryButton(if (merging) "Merging…" else "Merge into ${names[keep] ?: "the one picked"}", Modifier.fillMaxWidth(), enabled = keep != null && !merging, icon = PassIcons.Layers) {
                    val kept = keep ?: return@PrimaryButton
                    merging = true
                    scope.launch {
                        try {
                            Pass.change("entries.merge") {
                                put("keep", kept)
                                putJsonArray("ids") { items.forEach { add(it.id) } }
                                putJsonObject("take") {
                                    questions.forEach { question ->
                                        val index = picked[question.key] ?: return@forEach
                                        val holders = question.values.getOrNull(index)?.ids ?: return@forEach
                                        if (kept !in holders) put(question.key, holders.first())
                                    }
                                }
                                putJsonArray("both") { both.filterValues { it }.keys.forEach { add(it) } }
                            }
                            shell.haptics.confirm()
                            shell.undo = UndoOffer(if (items.size == 2) "Merged. The other one is in the trash" else "Merged. The other ${items.size - 1} are in the trash")
                            onDismiss()
                            onDone()
                        } catch (failure: NativeFailure) {
                            problem = failure.message?.replaceFirstChar { it.uppercase() }
                            merging = false
                            shell.haptics.reject()
                        }
                    }
                }
            }
        }
    }
}

@Composable
private fun Choice(selected: Boolean, title: String, subtitle: String?, trailing: (@Composable () -> Unit)? = null, onClick: () -> Unit) {
    val colors = palette
    Row(
        Modifier.fillMaxWidth().clickable(onClick = onClick).padding(vertical = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        RadioButton(selected, onClick = onClick)
        Spacer(Modifier.width(4.dp))
        Column(Modifier.weight(1f)) {
            Text(title, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 3, overflow = TextOverflow.Ellipsis)
            if (!subtitle.isNullOrEmpty()) Text(subtitle, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 2, overflow = TextOverflow.Ellipsis)
        }
        trailing?.invoke()
    }
}

@Composable
private fun Note(text: String) {
    Text(text, color = palette.textLow, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(start = 52.dp, end = 4.dp, bottom = 4.dp))
}

/**
 * A new password for one item, typed or made by the generator, without opening the editor. The
 * old one stays in the item's history. [changeUrl] is the site's own page for it, when it has one.
 */
@Composable
fun SetPasswordSheet(entry: Summary, changeUrl: String?, onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val field = rememberTextFieldState()
    val generator = remember { GenState.fromPrefs(shell.prefs) }
    var strength by remember { mutableStateOf<com.wasimaster.wmpass.core.StrengthView?>(null) }
    var saving by remember { mutableStateOf(false) }
    LaunchedEffect(Unit) {
        generator.roll()
        field.setTextAndPlaceCursorAtEnd(generator.value)
    }
    LaunchedEffect(field.text) {
        val bytes = field.text.toString().toByteArray()
        strength = if (bytes.isEmpty()) null else runCatching { Pass.strength(bytes) }.getOrNull()
        bytes.fill(0)
    }
    PassSheet(onDismiss, "New password for ${entry.title.ifEmpty { "this item" }}") {
        Column(verticalArrangement = Arrangement.spacedBy(10.dp)) {
            Text(
                if (changeUrl != null) "Change it on the site first, then save the same one here. The old one stays in this item's history." else "Change it on the site, then save the same one here. The old one stays in this item's history.",
                color = colors.textMid,
                style = MaterialTheme.typography.bodyMedium,
                modifier = Modifier.padding(horizontal = 4.dp),
            )
            PasswordPill(field, "New password", trailing = {
                RowIconButton(PassIcons.Casino, "Make another") {
                    scope.launch {
                        generator.roll()
                        field.setTextAndPlaceCursorAtEnd(generator.value)
                    }
                }
            })
            strength?.let { s ->
                StrengthBar(s.score, Modifier.fillMaxWidth().padding(horizontal = 4.dp))
                Text("Time to crack: ${s.crackSlowHash}", color = colors.textMid, style = MaterialTheme.typography.bodySmall, modifier = Modifier.padding(horizontal = 4.dp))
            }
            Row(horizontalArrangement = Arrangement.spacedBy(10.dp)) {
                QuietButton("Copy", Modifier.weight(1f), PassIcons.ContentCopy) {
                    val text = field.text.toString()
                    shell.copySecret("Password") { com.wasimaster.wmpass.core.SecretBytes(text.toByteArray()) }
                }
                if (changeUrl != null) {
                    QuietButton("Open the site", Modifier.weight(1f), PassIcons.OpenInNew) { shell.openUrl(changeUrl) }
                }
            }
            PrimaryButton(if (saving) "Saving…" else "Save", Modifier.fillMaxWidth(), enabled = field.text.isNotEmpty() && !saving) {
                saving = true
                val bytes = field.text.toString().toByteArray()
                scope.launch {
                    try {
                        Pass.change("entry.set_secret", secret = bytes) {
                            put("id", entry.id)
                            put("field", "password")
                        }
                        shell.haptics.confirm()
                        shell.undo = UndoOffer("New password saved. The old one is in its history")
                        onDismiss()
                    } catch (failure: NativeFailure) {
                        saving = false
                        shell.haptics.reject()
                    } finally {
                        bytes.fill(0)
                    }
                }
            }
        }
    }
}

/** An item's http addresses as https, keeping everything else about each one. */
suspend fun switchToHttps(id: String): Int {
    val detail = Pass.detail(id)
    var changed = 0
    val sites = detail.sites.map { site ->
        val url = ((site as? JsonObject)?.get("Url") as? JsonPrimitive)?.contentOrNull
        if (url != null && url.startsWith("http://", ignoreCase = true)) {
            changed++
            kotlinx.serialization.json.buildJsonObject { put("Url", "https://" + url.substring(7)) }
        } else {
            site
        }
    }
    if (changed > 0) {
        Pass.change("entry.common") {
            put("id", id)
            put("sites", JsonArray(sites))
        }
    }
    return changed
}

/** The first http address an item is saved for, as https, for trying before switching. */
fun httpsOf(sites: List<JsonElement>): String? = sites.firstNotNullOfOrNull { site ->
    ((site as? JsonObject)?.get("Url") as? JsonPrimitive)?.contentOrNull?.takeIf { it.startsWith("http://", ignoreCase = true) }?.let { "https://" + it.substring(7) }
}

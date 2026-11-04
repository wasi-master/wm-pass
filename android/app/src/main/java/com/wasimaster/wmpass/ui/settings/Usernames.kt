package com.wasimaster.wmpass.ui.settings

import android.net.Uri
import androidx.compose.animation.AnimatedContent
import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.fadeIn
import androidx.compose.animation.fadeOut
import androidx.compose.animation.scaleIn
import androidx.compose.animation.togetherWith
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.text.input.rememberTextFieldState
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.Icon
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.core.Engine
import com.wasimaster.wmpass.core.NativeFailure
import com.wasimaster.wmpass.system.NetAccess
import com.wasimaster.wmpass.system.PageCheck
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.GroupCard
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.RowIconButton
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.SheetField
import kotlinx.coroutines.launch
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.Semaphore
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.sync.withPermit
import kotlinx.serialization.json.JsonArray
import kotlinx.serialization.json.jsonObject
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * A site the checker knows, and where a name's page lives there. `check` sites are asked by the
 * engine, `page` sites are read in a hidden browser view, and `link` sites are only opened.
 */
private class CheckSite(val name: String, val profile: String, val icon: String, val kind: String) {
    val host: String = Uri.parse(profile.replace("{}", "x")).host.orEmpty().removePrefix("www.")

    fun page(name: String): String = profile.replace("{}", Uri.encode(name))
}

private sealed interface Answer {
    data object Checking : Answer
    data object Free : Answer
    data object Taken : Answer
    data object Unclear : Answer
    class Failed(val reason: String) : Answer
}

private fun answerOf(word: String?): Answer = when (word) {
    "free" -> Answer.Free
    "taken" -> Answer.Taken
    else -> Answer.Unclear
}

/** The names typed, one per word: spaces, commas, semicolons and new lines all part them. */
private fun namesIn(text: String): List<String> =
    text.split(Regex("[\\s,;]+")).map { it.trim().removePrefix("@") }.filter { it.isNotEmpty() }.distinct().take(MAX_NAMES)

/** More would be a few hundred requests at once, which sites start refusing. */
private const val MAX_NAMES = 10

@OptIn(androidx.compose.foundation.layout.ExperimentalLayoutApi::class)
@Composable
fun UsernameScreen() {
    val colors = palette
    val context = LocalContext.current
    val shell = LocalShell.current
    val scope = rememberCoroutineScope()
    val typed = rememberTextFieldState()
    var known by remember { mutableStateOf<List<CheckSite>>(emptyList()) }
    // By name, then site: "jane\nGitHub".
    val answers = remember { mutableStateMapOf<String, Answer>() }
    // The names the answers are for, which may no longer be what the field holds.
    var asked by remember { mutableStateOf<List<String>>(emptyList()) }
    // The name whose sites are listed below, when several were checked.
    var showing by remember { mutableStateOf("") }
    // Hidden page views share one cookie jar, which each wipes when done, so they take turns.
    val pageTurn = remember { Mutex() }
    // Many names times many sites: a few requests at a time, so no site sees a burst.
    val turns = remember { Semaphore(6) }
    LaunchedEffect(Unit) {
        known = runCatching {
            (Engine.net("usernames.site_list", online = false).json as JsonArray).map {
                val site = it.jsonObject
                CheckSite(
                    site["name"]!!.jsonPrimitive.content,
                    site["profile"]!!.jsonPrimitive.content,
                    site["icon"]!!.jsonPrimitive.content,
                    site["kind"]?.jsonPrimitive?.content ?: "check",
                )
            }
        }.getOrDefault(emptyList())
    }
    val sites = known.filter { it.kind != "link" }
    val linked = known.filter { it.kind == "link" }
    fun key(name: String, site: CheckSite) = "$name\n${site.name}"
    fun check(site: CheckSite, name: String) {
        answers[key(name, site)] = Answer.Checking
        scope.launch {
            val online = NetAccess.allowed(context)
            val answer = try {
                when {
                    site.kind != "page" -> turns.withPermit {
                        answerOf(
                            Engine.net("usernames.check", online = online) {
                                put("site", site.name)
                                put("name", name)
                            }.obj["answer"]?.jsonPrimitive?.content,
                        )
                    }
                    online -> answerOf(pageTurn.withLock { PageCheck.check(context, site.name, name) })
                    else -> Answer.Failed("Not checked: WM Pass may not use this network now")
                }
            } catch (failure: NativeFailure) {
                Answer.Failed(failure.message?.replaceFirstChar { it.uppercase() } ?: "No answer")
            }
            if (name in asked) answers[key(name, site)] = answer
        }
    }
    fun answersFor(name: String) = sites.mapNotNull { answers[key(name, it)] }
    val total = sites.size * asked.size
    val done = answers.values.count { it != Answer.Checking }
    val running = answers.values.any { it == Answer.Checking }
    val pending = namesIn(typed.text.toString())
    Page("Username checker") {
        NetGate("Asks every site at once whether a name is taken. Each site learns the names you looked up. TikTok and Instagram only answer a browser, so their pages load in a hidden browser view that runs their scripts and keeps nothing afterwards. X, Threads, Reddit and a few others refuse or give the same answer for every name, so their profile pages are linked for you to open yourself.") {
            Column(Modifier.padding(horizontal = 16.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                SheetField(typed, "One or more usernames, with spaces between")
                if (pending.size > 1) {
                    Text(
                        "${pending.size} names, each checked on every site.",
                        color = colors.textLow,
                        style = MaterialTheme.typography.bodySmall,
                        modifier = Modifier.padding(horizontal = 6.dp),
                    )
                }
                PrimaryButton(if (running) "Checking…" else if (pending.size > 1) "Check ${pending.size} names" else "Check", Modifier.fillMaxWidth(), enabled = !running && pending.isNotEmpty(), icon = PassIcons.PersonSearch) {
                    asked = pending
                    showing = pending.first()
                    answers.clear()
                    pending.forEach { name -> sites.forEach { check(it, name) } }
                }
                AnimatedVisibility(answers.isNotEmpty()) {
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        val all = answersFor(showing)
                        val free = all.count { it == Answer.Free }
                        val failed = all.count { it is Answer.Failed || it == Answer.Unclear }
                        Text(
                            when {
                                running -> "Checked $done of $total"
                                failed > 0 -> "$showing is free on $free of ${sites.size}. $failed gave no clear answer."
                                else -> "$showing is free on $free of ${sites.size}."
                            },
                            color = colors.textMid,
                            style = MaterialTheme.typography.bodyMedium,
                            modifier = Modifier.padding(horizontal = 6.dp),
                        )
                        val progress by animateFloatAsState(if (total == 0) 0f else done.toFloat() / total, label = "checked")
                        LinearProgressIndicator(
                            progress = { progress },
                            color = colors.primary,
                            trackColor = colors.surface3,
                            modifier = Modifier.fillMaxWidth().clip(CircleShape),
                        )
                    }
                }
                // One chip per name with how it did, the chosen one's sites below.
                if (asked.size > 1) {
                    androidx.compose.foundation.layout.FlowRow(horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        asked.forEach { name ->
                            val results = answersFor(name)
                            val checking = results.any { it == Answer.Checking } || results.size < sites.size
                            val free = results.count { it == Answer.Free }
                            Pill(if (checking) "$name  …" else "$name  $free/${sites.size}", showing == name) { showing = name }
                        }
                    }
                }
                Eyebrow(if (sites.isEmpty()) "Sites" else if (asked.size > 1) "$showing on ${sites.size} sites" else "${sites.size} sites")
                GroupCard {
                    sites.forEachIndexed { index, site ->
                        if (index > 0) Hairline(inset = 70.dp)
                        SiteLine(
                            site,
                            answers[key(showing, site)],
                            showing,
                            onOpen = { shell.openUrl(site.page(showing)) },
                            onRetry = { check(site, showing) },
                        )
                    }
                }
                Text(
                    "Tap a site to open that name's page there.",
                    color = colors.textLow,
                    style = MaterialTheme.typography.bodySmall,
                    modifier = Modifier.padding(horizontal = 6.dp),
                )
                if (linked.isNotEmpty()) {
                    Eyebrow("Look for yourself") { com.wasimaster.wmpass.ui.help.HelpMark("group.usernames.links", "the profile links") }
                    GroupCard {
                        linked.forEachIndexed { index, site ->
                            if (index > 0) Hairline(inset = 70.dp)
                            val ready = showing.isNotEmpty()
                            Row(
                                Modifier.fillMaxWidth().clickable(enabled = ready) { shell.openUrl(site.page(showing)) }.padding(start = 16.dp, end = 16.dp, top = 10.dp, bottom = 10.dp),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                SiteTile(site.name, null, "login", size = 38.dp, icon = site.icon)
                                Spacer(Modifier.width(14.dp))
                                Column(Modifier.weight(1f)) {
                                    Text(site.name, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
                                    Text(
                                        if (ready) site.page(showing).removePrefix("https://").removePrefix("www.") else site.host,
                                        color = colors.textLow,
                                        style = MaterialTheme.typography.bodySmall,
                                        maxLines = 1,
                                    )
                                }
                                if (ready) Icon(PassIcons.OpenInNew, contentDescription = "Open in your browser", tint = colors.textLow, modifier = Modifier.size(20.dp))
                            }
                        }
                    }
                }
                Spacer(Modifier.height(30.dp))
            }
        }
    }
}

@Composable
private fun SiteLine(site: CheckSite, answer: Answer?, asked: String, onOpen: () -> Unit, onRetry: () -> Unit) {
    val colors = palette
    val opens = answer != null && answer != Answer.Checking && asked.isNotEmpty()
    Row(
        Modifier.fillMaxWidth().clickable(enabled = opens, onClick = onOpen).padding(start = 16.dp, end = 12.dp, top = 10.dp, bottom = 10.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        SiteTile(site.name, null, "login", size = 38.dp, icon = site.icon)
        Spacer(Modifier.width(14.dp))
        Column(Modifier.weight(1f)) {
            Text(site.name, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1)
            Text(
                when {
                    answer is Answer.Failed -> answer.reason
                    asked.isNotEmpty() && answer != null -> site.page(asked).removePrefix("https://").removePrefix("www.")
                    else -> site.host
                },
                color = if (answer is Answer.Failed) colors.danger else colors.textLow,
                style = MaterialTheme.typography.bodySmall,
                maxLines = 1,
            )
        }
        AnimatedContent(
            answer,
            transitionSpec = { (fadeIn() + scaleIn(initialScale = 0.85f)) togetherWith fadeOut() },
            contentKey = { it?.javaClass },
            label = "answer",
        ) { now ->
            when (now) {
                null -> Spacer(Modifier.size(22.dp))
                Answer.Checking -> CircularProgressIndicator(Modifier.size(20.dp), color = colors.primary, strokeWidth = 2.dp)
                is Answer.Failed -> RowIconButton(PassIcons.Refresh, "Ask ${site.name} again") { onRetry() }
                else -> Text(
                    when (now) {
                        Answer.Free -> "Free"
                        Answer.Taken -> "Taken"
                        else -> "No clear answer"
                    },
                    color = when (now) {
                        Answer.Free -> colors.primary
                        Answer.Taken -> colors.textMid
                        else -> colors.textLow
                    },
                    style = MaterialTheme.typography.labelLarge,
                )
            }
        }
    }
}

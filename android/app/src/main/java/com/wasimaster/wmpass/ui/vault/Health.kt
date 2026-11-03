package com.wasimaster.wmpass.ui.vault

import androidx.compose.animation.AnimatedVisibility
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.FastOutSlowInEasing
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.animation.core.tween
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.WindowInsets
import androidx.compose.foundation.layout.asPaddingValues
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.statusBars
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.compose.collectAsStateWithLifecycle
import com.wasimaster.wmpass.core.HealthView
import com.wasimaster.wmpass.core.Pass
import com.wasimaster.wmpass.core.Summary
import com.wasimaster.wmpass.ui.fx.Aurora
import com.wasimaster.wmpass.ui.fx.rim
import com.wasimaster.wmpass.ui.help.HelpAction
import com.wasimaster.wmpass.ui.help.Hint
import com.wasimaster.wmpass.ui.help.helpAnchor
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlyphTile
import com.wasimaster.wmpass.ui.parts.bleed
import com.wasimaster.wmpass.ui.parts.Hairline
import com.wasimaster.wmpass.ui.parts.PrimaryButton
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.parts.SiteTile
import com.wasimaster.wmpass.ui.theme.LocalReduceMotion
import com.wasimaster.wmpass.ui.theme.PassType
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.Springs
import com.wasimaster.wmpass.ui.theme.arrive
import com.wasimaster.wmpass.ui.theme.palette
import kotlin.math.roundToInt
import kotlinx.serialization.json.jsonArray
import kotlinx.serialization.json.jsonPrimitive
import kotlinx.serialization.json.put

/**
 * What the last breach check found. Kept in memory only: a list on disk of which passwords
 * have leaked would be a gift to anyone who took the phone.
 */
object BreachCounts {
    val counts = kotlinx.coroutines.flow.MutableStateFlow<Map<String, Long>>(emptyMap())
}

private data class Issue(
    val key: String,
    val title: String,
    val why: String,
    val icon: ImageVector,
    val ids: List<String>,
    val serious: Boolean,
) {
    val lead: String get() = "Start with the ones ${title.replaceFirstChar { it.lowercase() }.let { LeadWords[key] ?: it }}."
}

private val LeadWords = mapOf(
    "breached" to "in a known breach",
    "reused" to "used more than once",
    "weak" to "that are easy to guess",
    "insecure" to "on unencrypted sites",
    "expiring" to "expiring soon",
    "old" to "you have not changed in a year",
    "twofactor" to "with no second factor",
    "duplicates" to "saved twice",
)

private fun issuesOf(h: HealthView): List<Issue> = listOf(
    Issue(
        "breached", "In a known breach", "These passwords have appeared in published leaks. Change them first.",
        PassIcons.Dangerous,
        h.breached.mapNotNull { runCatching { it.jsonArray[0].jsonPrimitive.content }.getOrNull() },
        true,
    ),
    Issue("reused", "Used more than once", "One leak opens every site that shares the password.", PassIcons.SwapHoriz, h.reused.flatten(), true),
    Issue("weak", "Easy to guess", "These would fall quickly to an attacker with a password list.", PassIcons.GppBad, h.weak, true),
    Issue("insecure", "Unencrypted sites", "These sites are saved with http, so a password could be read on the way.", PassIcons.Warning, h.insecureUrl, false),
    Issue("expiring", "Expiring soon", "Their reminders or expiry dates are close.", PassIcons.Schedule, h.expiring, false),
    Issue("old", "Not changed in a year", "Worth changing if the site holds anything you care about.", PassIcons.History, h.old, false),
    Issue("twofactor", "No second factor", "These sites offer one-time codes and you have none saved.", PassIcons.Pin, h.missingTwoFactor, false),
    Issue("duplicates", "Duplicate items", "The same login saved more than once. Merging keeps both histories.", PassIcons.Layers, h.duplicates.flatten(), false),
)

/** The state of the vault as one number, with what would raise it right below. */
@Composable
fun HealthTab() {
    val colors = palette
    val shell = LocalShell.current
    val revision by Pass.revision.collectAsStateWithLifecycle()
    val entries by Pass.entries.collectAsStateWithLifecycle()
    var health by remember { mutableStateOf<HealthView?>(null) }
    val breachCounts by BreachCounts.counts.collectAsStateWithLifecycle()
    LaunchedEffect(revision, breachCounts) { health = runCatching { Pass.health(breachCounts) }.getOrNull() }
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val h = health
    val hiddenChecks = shell.prefs.healthHidden
    val issues = remember(h, hiddenChecks) { h?.let { issuesOf(it).filter { issue -> issue.ids.isNotEmpty() && issue.key !in hiddenChecks } }.orEmpty() }
    Box(Modifier.fillMaxSize()) {
        Aurora(Modifier.fillMaxWidth().height(460.dp), strength = if (colors.dark) 0.5f else 0.7f)
        Box(
            Modifier
                .fillMaxWidth()
                .height(460.dp)
                .background(Brush.verticalGradient(0.45f to Color.Transparent, 1f to colors.ground)),
        )
        LazyColumn(
            Modifier.fillMaxSize(),
            contentPadding = PaddingValues(
                top = if (colors.flat) 8.dp else top + 8.dp,
                bottom = if (colors.flat) 32.dp else 190.dp,
                start = 16.dp,
                end = 16.dp,
            ),
            verticalArrangement = Arrangement.spacedBy(if (colors.flat) 0.dp else 10.dp),
        ) {
            item("hint") {
                Hint(shell.prefs, "health", "screen.health", Modifier.padding(vertical = 4.dp))
            }
            if (colors.flat && shell.prefs.healthScore.value) {
                item("score") {
                    Column(Modifier.fillMaxWidth().padding(bottom = 12.dp).helpAnchor("health.score")) {
                        Row(verticalAlignment = Alignment.Bottom) {
                            Text(h?.score?.toString() ?: "-", color = colors.text, style = MaterialTheme.typography.displaySmall.merge(PassType.numeric))
                            Spacer(Modifier.width(8.dp))
                            Text("out of 100", color = colors.textMid, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.padding(bottom = 6.dp))
                        }
                        Spacer(Modifier.height(10.dp))
                        // Fills to the score once the check has an answer.
                        val filled by animateFloatAsState((h?.score ?: 0) / 100f, androidx.compose.animation.core.tween(800), label = "score")
                        androidx.compose.material3.LinearProgressIndicator(
                            progress = { filled },
                            modifier = Modifier.fillMaxWidth(),
                            color = when {
                                (h?.score ?: 0) >= 80 -> colors.primary
                                (h?.score ?: 0) >= 50 -> colors.warn
                                else -> colors.danger
                            },
                            trackColor = colors.surface3,
                        )
                    }
                }
            } else if (!colors.flat) {
                item("title") {
                    Row(verticalAlignment = Alignment.CenterVertically) {
                        Text(
                            "Health",
                            color = colors.text,
                            style = MaterialTheme.typography.headlineLarge.copy(fontSize = 32.sp),
                            modifier = Modifier.weight(1f).padding(start = 6.dp, top = 12.dp),
                        )
                        HelpAction("screen.health", "Health", tint = colors.text)
                    }
                }
                item("ring") {
                    Box(Modifier.fillMaxWidth().padding(vertical = 18.dp), contentAlignment = Alignment.Center) {
                        ScoreRing(h?.score, Modifier.helpAnchor("health.score"))
                    }
                }
            }
            item("line") {
                val count = issues.sumOf { it.ids.size }
                Text(
                    when {
                        h == null -> " "
                        entries.isEmpty() -> "Nothing to check yet."
                        count == 0 -> "No problems found."
                        else -> issues.first().lead
                    },
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyMedium,
                    textAlign = if (colors.flat) androidx.compose.ui.text.style.TextAlign.Start else androidx.compose.ui.text.style.TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = if (colors.flat) 0.dp else 24.dp),
                )
            }
            if (issues.isNotEmpty()) {
                item("next") {
                    val first = issues.first().ids.first()
                    if (colors.flat) {
                        Spacer(Modifier.height(12.dp))
                    } else {
                        PrimaryButton("Fix the most urgent", Modifier.fillMaxWidth().padding(top = 8.dp, bottom = 6.dp), icon = PassIcons.Bolt) {
                            shell.nav.navigate("edit/$first")
                        }
                    }
                }
            }
            issues.forEachIndexed { index, issue ->
                item(issue.key) {
                    // The first group stands for all of them when a tour points at them.
                    val placed = Modifier.arrive(index)
                    IssueCard(issue, entries, if (index == 0) placed.helpAnchor("health.groups") else placed, if (issue.key == "duplicates") health?.duplicates.orEmpty() else emptyList())
                }
            }
            item("tools") {
                if (colors.flat) {
                    Column(Modifier.bleed(16.dp)) {
                        com.wasimaster.wmpass.ui.parts.Eyebrow("Tools", Modifier.padding(horizontal = 16.dp))
                        com.wasimaster.wmpass.ui.parts.NavRow("Check for breaches", "Look your passwords and email addresses up in known leaks", PassIcons.LocalFireDepartment) { shell.nav.navigate("settings/breaches") }
                        com.wasimaster.wmpass.ui.parts.NavRow("Compare two passwords", icon = PassIcons.Balance) { shell.nav.navigate("settings/compare") }
                    }
                } else Column(Modifier.padding(top = 10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    QuietButton("Check for breaches", Modifier.fillMaxWidth(), PassIcons.LocalFireDepartment) { shell.nav.navigate("settings/breaches") }
                    QuietButton("Compare two passwords", Modifier.fillMaxWidth(), PassIcons.Balance) { shell.nav.navigate("settings/compare") }
                }
            }
        }
    }
}

/** Sweeps up to the score while the number counts with it. */
@Composable
private fun ScoreRing(score: Int?, modifier: Modifier = Modifier) {
    val colors = palette
    val reduce = LocalReduceMotion.current
    val shown = remember { Animatable(0f) }
    LaunchedEffect(score) {
        if (score == null) return@LaunchedEffect
        if (reduce) {
            shown.snapTo(score.toFloat())
        } else {
            shown.animateTo(score.toFloat(), tween(1250, easing = FastOutSlowInEasing))
        }
    }
    val tone = when {
        shown.value >= 80 -> colors.primary
        shown.value >= 50 -> colors.warn
        else -> colors.danger
    }
    val end by androidx.compose.animation.animateColorAsState(tone, tween(400), label = "tone")
    Box(modifier, contentAlignment = Alignment.Center) {
        Canvas(Modifier.size(196.dp)) {
            val stroke = 13.dp.toPx()
            val box = Size(size.width - stroke, size.height - stroke)
            val at = Offset(stroke / 2, stroke / 2)
            // A soft pool of the same colour under the ring.
            drawCircle(
                Brush.radialGradient(listOf(end.copy(alpha = 0.17f), Color.Transparent)),
                radius = size.width * 0.62f,
            )
            drawArc(colors.hairline, 135f, 270f, false, at, box, style = Stroke(stroke, cap = StrokeCap.Round))
            rotate(135f) {
                drawArc(
                    // Stops past 0.75 only colour the round cap at the start of the arc.
                    Brush.sweepGradient(0f to colors.brandTo, 0.34f to colors.primary, 0.75f to end, 0.86f to end, 0.96f to colors.brandTo, 1f to colors.brandTo),
                    0f,
                    270f * shown.value / 100f,
                    false,
                    at,
                    box,
                    style = Stroke(stroke, cap = StrokeCap.Round),
                )
            }
        }
        Column(horizontalAlignment = Alignment.CenterHorizontally) {
            Text(
                shown.value.roundToInt().toString(),
                color = colors.text,
                style = MaterialTheme.typography.displayLarge.merge(PassType.numeric).copy(fontSize = 62.sp, fontWeight = FontWeight.Bold),
            )
            Text("out of 100", color = colors.textLow, style = MaterialTheme.typography.labelMedium)
        }
    }
}

@Composable
private fun IssueCard(issue: Issue, entries: List<Summary>, modifier: Modifier = Modifier, groups: List<List<String>> = emptyList()) {
    val colors = palette
    val shell = LocalShell.current
    var open by remember { mutableStateOf(false) }
    var merging by remember { mutableStateOf<List<String>?>(null) }
    val turn by animateFloatAsState(if (open) 180f else 0f, Springs.glide(), label = "chevron")
    val tint = if (issue.serious) colors.danger else colors.warn
    Column(
        if (colors.flat) {
            modifier.bleed(16.dp).fillMaxWidth()
        } else {
            modifier
                .fillMaxWidth()
                .clip(Shapes.card)
                .background(colors.surface1)
                .rim(Shapes.card, colors)
        },
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .clickable {
                    shell.haptics.tick()
                    open = !open
                }
                .padding(if (colors.flat) 16.dp else 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GlyphTile(issue.icon, tint, size = 44.dp)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(issue.title, color = colors.text, style = if (colors.flat) MaterialTheme.typography.bodyLarge else MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold))
                Text(issue.why, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = if (open) 4 else 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.width(10.dp))
            Text(issue.ids.distinct().size.toString(), color = tint, style = (if (colors.flat) MaterialTheme.typography.titleMedium else MaterialTheme.typography.titleLarge).merge(PassType.numeric))
            Spacer(Modifier.width(6.dp))
            Icon(PassIcons.ExpandMore, if (open) "Collapse" else "Expand", tint = colors.textLow, modifier = Modifier.graphicsLayer { rotationZ = turn })
        }
        AnimatedVisibility(open) {
            Column {
                issue.ids.distinct().forEach { id ->
                    val entry = entries.firstOrNull { it.id == id } ?: return@forEach
                    Hairline(inset = 72.dp)
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clickable { shell.open(id) }
                            .padding(horizontal = 16.dp, vertical = 10.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        SiteTile(entry.title, entry.site, entry.kind, size = 40.dp, label = entry.color, icon = entry.icon)
                        Spacer(Modifier.width(14.dp))
                        Column(Modifier.weight(1f)) {
                            Text(entry.title, color = colors.text, style = MaterialTheme.typography.bodyLarge, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            if (entry.subtitle.isNotEmpty()) {
                                Text(entry.subtitle, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = 1, overflow = TextOverflow.Ellipsis)
                            }
                        }
                        Icon(PassIcons.ChevronRight, null, tint = colors.textLow, modifier = Modifier.size(20.dp))
                    }
                }
                groups.forEach { group ->
                    val first = entries.firstOrNull { it.id == group.first() } ?: return@forEach
                    Hairline()
                    Row(
                        Modifier
                            .fillMaxWidth()
                            .clickable { merging = group }
                            .padding(horizontal = 18.dp, vertical = 14.dp),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        Icon(PassIcons.Layers, null, tint = colors.primary, modifier = Modifier.size(20.dp))
                        Spacer(Modifier.width(12.dp))
                        Text("Merge ${group.size} copies of ${first.title}", color = colors.primary, style = MaterialTheme.typography.labelLarge, modifier = Modifier.weight(1f))
                    }
                }
            }
        }
    }
    merging?.let { group ->
        PassSheet({ merging = null }, "Which one stays?") {
            Text("The others fold into it. Their passwords move to its history and their extra fields come along, so nothing is lost.", color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
            Spacer(Modifier.height(10.dp))
            group.forEach { id ->
                val entry = entries.firstOrNull { it.id == id } ?: return@forEach
                SheetRow(PassIcons.Key, entry.title, entry.subtitle.ifEmpty { null }) {
                    merging = null
                    shell.launch {
                        group.filter { it != id }.forEach { other ->
                            Pass.change("health.merge_duplicates") {
                                put("keep", id)
                                put("remove", other)
                            }
                        }
                        shell.haptics.confirm()
                    }
                }
            }
        }
    }
}

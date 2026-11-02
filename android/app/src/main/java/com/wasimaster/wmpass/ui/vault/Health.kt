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
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.GlyphTile
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
    LaunchedEffect(revision) { health = runCatching { Pass.health() }.getOrNull() }
    val top = WindowInsets.statusBars.asPaddingValues().calculateTopPadding()
    val h = health
    val issues = remember(h) { h?.let { issuesOf(it).filter { issue -> issue.ids.isNotEmpty() } }.orEmpty() }
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
            contentPadding = PaddingValues(top = top + 8.dp, bottom = 190.dp, start = 16.dp, end = 16.dp),
            verticalArrangement = Arrangement.spacedBy(10.dp),
        ) {
            item("title") {
                Text(
                    "Health",
                    color = colors.text,
                    style = MaterialTheme.typography.headlineLarge.copy(fontSize = 32.sp),
                    modifier = Modifier.padding(start = 6.dp, top = 12.dp),
                )
            }
            item("ring") {
                Box(Modifier.fillMaxWidth().padding(vertical = 18.dp), contentAlignment = Alignment.Center) {
                    ScoreRing(h?.score)
                }
            }
            item("line") {
                val count = issues.sumOf { it.ids.size }
                Text(
                    when {
                        h == null -> " "
                        entries.isEmpty() -> "Nothing to check yet."
                        count == 0 -> "Nothing needs you. Every password is strong and used once."
                        else -> issues.first().lead
                    },
                    color = colors.textMid,
                    style = MaterialTheme.typography.bodyLarge,
                    textAlign = androidx.compose.ui.text.style.TextAlign.Center,
                    modifier = Modifier.fillMaxWidth().padding(horizontal = 24.dp),
                )
            }
            if (issues.isNotEmpty()) {
                item("next") {
                    val first = issues.first().ids.first()
                    PrimaryButton("Fix the most urgent", Modifier.fillMaxWidth().padding(top = 8.dp, bottom = 6.dp), icon = PassIcons.Bolt) {
                        shell.nav.navigate("edit/$first")
                    }
                }
            }
            issues.forEachIndexed { index, issue ->
                item(issue.key) {
                    IssueCard(issue, entries, Modifier.arrive(index))
                }
            }
            item("tools") {
                Column(Modifier.padding(top = 10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
                    QuietButton("Check for breaches", Modifier.fillMaxWidth(), PassIcons.LocalFireDepartment) { shell.nav.navigate("settings/breaches") }
                    QuietButton("Compare two passwords", Modifier.fillMaxWidth(), PassIcons.Balance) { shell.nav.navigate("settings/compare") }
                }
            }
        }
    }
}

/** Sweeps up to the score while the number counts with it. */
@Composable
private fun ScoreRing(score: Int?) {
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
    Box(contentAlignment = Alignment.Center) {
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
private fun IssueCard(issue: Issue, entries: List<Summary>, modifier: Modifier = Modifier) {
    val colors = palette
    val shell = LocalShell.current
    var open by remember { mutableStateOf(false) }
    val turn by animateFloatAsState(if (open) 180f else 0f, Springs.glide(), label = "chevron")
    val tint = if (issue.serious) colors.danger else colors.warn
    Column(
        modifier
            .fillMaxWidth()
            .clip(Shapes.card)
            .background(colors.surface1)
            .rim(Shapes.card, colors),
    ) {
        Row(
            Modifier
                .fillMaxWidth()
                .clickable {
                    shell.haptics.tick()
                    open = !open
                }
                .padding(14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            GlyphTile(issue.icon, tint, size = 44.dp)
            Spacer(Modifier.width(14.dp))
            Column(Modifier.weight(1f)) {
                Text(issue.title, color = colors.text, style = MaterialTheme.typography.bodyLarge.copy(fontWeight = FontWeight.SemiBold))
                Text(issue.why, color = colors.textMid, style = MaterialTheme.typography.bodySmall, maxLines = if (open) 4 else 2, overflow = TextOverflow.Ellipsis)
            }
            Spacer(Modifier.width(10.dp))
            Text(issue.ids.distinct().size.toString(), color = tint, style = MaterialTheme.typography.titleLarge.merge(PassType.numeric))
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
            }
        }
    }
}

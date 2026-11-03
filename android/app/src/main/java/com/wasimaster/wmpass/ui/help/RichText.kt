package com.wasimaster.wmpass.ui.help

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.width
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.app.Prefs
import com.wasimaster.wmpass.ui.theme.MonoFamily
import com.wasimaster.wmpass.ui.theme.Palette
import com.wasimaster.wmpass.ui.theme.palette

/**
 * The guide's text: paragraphs and lists, with a name on screen in a heavier weight, links that
 * open the next topic, and key chords as the person has set them. A chord for something nobody
 * has bound is left out rather than shown as a gap.
 */
@Composable
fun RichText(
    blocks: List<HelpBlock>?,
    modifier: Modifier = Modifier,
    style: TextStyle = MaterialTheme.typography.bodyMedium,
    color: Color = palette.textMid,
    onLink: (String) -> Unit = { Guide.open(it) },
) {
    if (blocks.isNullOrEmpty()) return
    val colors = palette
    val prefs = Prefs.of(LocalContext.current)
    Column(modifier, verticalArrangement = Arrangement.spacedBy(8.dp)) {
        for (block in blocks) {
            if (block.t == "ul") {
                Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
                    for (item in block.items) {
                        Row {
                            Text("•", style = style, color = color)
                            Spacer(Modifier.width(8.dp))
                            Text(annotated(item, colors, prefs, onLink), style = style, color = color, modifier = Modifier.weight(1f))
                        }
                    }
                }
            } else {
                Text(annotated(block.runs, colors, prefs, onLink), style = style, color = color)
            }
        }
    }
}

/** One line of runs, ready to draw. */
fun annotated(runs: List<HelpRun>, colors: Palette, prefs: Prefs?, onLink: (String) -> Unit): AnnotatedString = buildAnnotatedString {
    for (run in runs) {
        when (run.t) {
            "strong" -> withStyle(SpanStyle(fontWeight = FontWeight.SemiBold, color = colors.text)) { append(run.s.orEmpty()) }
            "em" -> withStyle(SpanStyle(fontStyle = FontStyle.Italic)) { append(run.s.orEmpty()) }
            "key" -> {
                val chord = run.action?.let { Help.chord(prefs, it) }
                if (!chord.isNullOrEmpty()) {
                    withStyle(SpanStyle(fontFamily = MonoFamily, color = colors.text, background = colors.surface3)) { append(" $chord ") }
                }
            }
            "link" -> {
                val id = run.id
                val words = Help.runText(run, prefs)
                // A topic the phone does not have is already plain words in the file; this is the
                // same rule for one that went missing since.
                if (id == null || Help.topic(id) == null) {
                    append(words)
                } else {
                    val link = LinkAnnotation.Clickable(
                        tag = id,
                        styles = TextLinkStyles(SpanStyle(color = colors.primary, textDecoration = TextDecoration.Underline)),
                    ) { onLink(id) }
                    withLink(link) { append(words) }
                }
            }
            else -> append(run.s.orEmpty())
        }
    }
}

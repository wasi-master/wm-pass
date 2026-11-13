package com.wasimaster.wmpass.ui.settings

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
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
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.AnnotatedString
import androidx.compose.ui.text.LinkAnnotation
import androidx.compose.ui.text.SpanStyle
import androidx.compose.ui.text.TextLinkStyles
import androidx.compose.ui.text.buildAnnotatedString
import androidx.compose.ui.text.font.FontFamily
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextDecoration
import androidx.compose.ui.text.withLink
import androidx.compose.ui.text.withStyle
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.parts.Eyebrow
import com.wasimaster.wmpass.ui.parts.Pill
import com.wasimaster.wmpass.ui.parts.QuietButton
import com.wasimaster.wmpass.ui.theme.Palette
import com.wasimaster.wmpass.ui.theme.Shapes
import com.wasimaster.wmpass.ui.theme.palette
import com.wasimaster.wmpass.ui.vault.LocalShell
import com.wasimaster.wmpass.ui.vault.PassSheet

private sealed interface Block {
    /** The device tab it sits under, or null for the whole page. */
    val tab: String?

    class Heading(val text: String, override val tab: String?) : Block
    class Paragraph(val text: String, override val tab: String?) : Block
    class Bullet(val text: String, override val tab: String?) : Block
    class Step(val number: String, val text: String, override val tab: String?) : Block
    class Note(val title: String, val text: String, override val tab: String?) : Block
    class Picture(val caption: String, val publisher: String, val src: String, override val tab: String?) : Block
}

private class GuidePage(val title: String, val blocks: List<Block>) {
    val tabs: List<String> = blocks.mapNotNull { it.tab }.distinct()
}

/** The device picked last, so the next guide opens on the same one, as the website does. */
private var lastDevice = "Android"

/**
 * One app's export guide from the website, read from the copy bundled with the app, so following
 * it does not mean leaving for the browser. Its pictures stay on the vendor's site, a tap away.
 */
@OptIn(ExperimentalLayoutApi::class)
@Composable
fun GuidePageSheet(file: String, web: String, onDismiss: () -> Unit) {
    val colors = palette
    val shell = LocalShell.current
    val context = LocalContext.current
    val page = remember(file) {
        runCatching { context.assets.open(file).bufferedReader().use { it.readText() } }.mapCatching(::readGuide).getOrNull()
    }
    var device by remember(page) { mutableStateOf(page?.tabs?.firstOrNull { it == lastDevice } ?: page?.tabs?.firstOrNull()) }
    PassSheet(onDismiss, page?.title ?: "Export guide") {
        if (page == null) {
            Text("This guide is not in this build of the app. The website has it.", color = colors.textMid, style = MaterialTheme.typography.bodyLarge)
        } else {
            var tabsShown = false
            page.blocks.forEach { block ->
                if (block.tab != null && !tabsShown) {
                    tabsShown = true
                    FlowRow(Modifier.padding(vertical = 8.dp), horizontalArrangement = Arrangement.spacedBy(8.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                        page.tabs.forEach { tab ->
                            Pill(tab, device == tab) {
                                device = tab
                                lastDevice = tab
                            }
                        }
                    }
                }
                if (block.tab == null || block.tab == device) GuideBlock(block, colors) { shell.openUrl(it) }
            }
        }
        Spacer(Modifier.height(12.dp))
        QuietButton("Open on the website", Modifier.fillMaxWidth(), PassIcons.OpenInNew) { shell.openUrl(web) }
    }
}

@Composable
private fun GuideBlock(block: Block, colors: Palette, open: (String) -> Unit) {
    val body = MaterialTheme.typography.bodyLarge
    when (block) {
        is Block.Heading -> Eyebrow(block.text, Modifier.padding(top = 8.dp))
        is Block.Paragraph -> Text(inline(block.text, colors), color = colors.textMid, style = body, modifier = Modifier.padding(horizontal = 2.dp, vertical = 4.dp))
        is Block.Bullet -> Row(Modifier.padding(start = 2.dp, top = 4.dp, bottom = 4.dp)) {
            Text("•", color = colors.textLow, style = body)
            Spacer(Modifier.width(10.dp))
            Text(inline(block.text, colors), color = colors.text, style = body)
        }
        is Block.Step -> Row(Modifier.padding(top = 6.dp, bottom = 6.dp), verticalAlignment = Alignment.Top) {
            Box(Modifier.size(26.dp).clip(CircleShape).background(colors.surface3), contentAlignment = Alignment.Center) {
                Text(block.number, color = colors.text, style = MaterialTheme.typography.labelLarge)
            }
            Spacer(Modifier.width(12.dp))
            Text(inline(block.text, colors), color = colors.text, style = body, modifier = Modifier.padding(top = 2.dp))
        }
        is Block.Note -> Column(
            Modifier.fillMaxWidth().padding(vertical = 6.dp).clip(Shapes.cardSmall).background(colors.surface2).padding(horizontal = 14.dp, vertical = 12.dp),
        ) {
            if (block.title.isNotEmpty()) Text(block.title, color = colors.text, style = MaterialTheme.typography.titleSmall)
            Text(inline(block.text, colors), color = colors.textMid, style = MaterialTheme.typography.bodyMedium)
        }
        is Block.Picture -> Row(
            Modifier.fillMaxWidth().padding(start = 38.dp, top = 2.dp, bottom = 6.dp).clip(Shapes.cardSmall).clickable { open(block.src) }.padding(horizontal = 8.dp, vertical = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Icon(PassIcons.Image, null, tint = colors.textLow, modifier = Modifier.size(20.dp))
            Spacer(Modifier.width(10.dp))
            Text("Picture on ${publisherSite(block.publisher)}", color = colors.primary, style = MaterialTheme.typography.bodyMedium, modifier = Modifier.weight(1f))
        }
    }
}

private fun publisherSite(publisher: String): String = if (publisher.endsWith("s")) "$publisher' site" else "$publisher's site"

private fun attribute(tag: String, name: String): String = Regex("""\b$name="([^"]*)"""").find(tag)?.groupValues?.get(1).orEmpty()

/** The website's MDX, as far as its import guides use it: tabs, steps, asides and pictures. */
private fun readGuide(mdx: String): GuidePage {
    var lines = mdx.lines()
    var title = "Export guide"
    if (lines.firstOrNull()?.trim() == "---") {
        val end = lines.drop(1).indexOfFirst { it.trim() == "---" } + 1
        lines.subList(1, end).firstOrNull { it.startsWith("title:") }?.let { title = it.removePrefix("title:").trim().trim('"') }
        lines = lines.drop(end + 1)
    }
    val blocks = mutableListOf<Block>()
    val paragraph = StringBuilder()
    var tab: String? = null
    var aside: Pair<String, StringBuilder>? = null
    fun flush() {
        if (paragraph.isNotBlank()) blocks += Block.Paragraph(paragraph.toString().trim(), tab)
        paragraph.clear()
    }
    for (raw in lines) {
        val line = raw.trim()
        val open = aside
        when {
            line.startsWith("import ") -> Unit
            line.startsWith("<Aside") -> {
                flush()
                aside = attribute(line, "title") to StringBuilder()
            }
            line.startsWith("</Aside>") && open != null -> {
                blocks += Block.Note(open.first, open.second.toString().trim(), tab)
                aside = null
            }
            open != null -> open.second.append(line).append(' ')
            line.startsWith("<TabItem") -> {
                flush()
                tab = attribute(line, "label")
            }
            line.startsWith("</TabItem>") -> {
                flush()
                tab = null
            }
            line.startsWith("<Shot") -> {
                flush()
                blocks += Block.Picture(attribute(line, "alt"), attribute(line, "publisher"), attribute(line, "src"), tab)
            }
            // The wrappers themselves carry nothing to show.
            line.startsWith("<") -> flush()
            line.startsWith("## ") || line.startsWith("### ") -> {
                flush()
                blocks += Block.Heading(line.trimStart('#').trim(), tab)
            }
            Regex("""^\d+\.\s""").containsMatchIn(line) -> {
                flush()
                blocks += Block.Step(line.substringBefore('.'), line.substringAfter('.').trim(), tab)
            }
            line.startsWith("- ") -> {
                flush()
                blocks += Block.Bullet(line.removePrefix("- "), tab)
            }
            line.isEmpty() -> flush()
            else -> paragraph.append(line).append(' ')
        }
    }
    flush()
    return GuidePage(title, blocks)
}

/** Bold, code and links: all the guides use. Links to the site's own pages stay plain text. */
private fun inline(text: String, colors: Palette): AnnotatedString = buildAnnotatedString {
    val link = TextLinkStyles(SpanStyle(color = colors.primary, textDecoration = TextDecoration.Underline))
    var i = 0
    while (i < text.length) {
        when {
            text.startsWith("**", i) && text.indexOf("**", i + 2) > 0 -> {
                val end = text.indexOf("**", i + 2)
                withStyle(SpanStyle(fontWeight = FontWeight.SemiBold)) { append(text.substring(i + 2, end)) }
                i = end + 2
            }
            text[i] == '`' && text.indexOf('`', i + 1) > 0 -> {
                val end = text.indexOf('`', i + 1)
                withStyle(SpanStyle(fontFamily = FontFamily.Monospace, background = colors.surface3)) { append(text.substring(i + 1, end)) }
                i = end + 1
            }
            text[i] == '[' && text.indexOf("](", i) > 0 && text.indexOf(')', text.indexOf("](", i)) > 0 -> {
                val middle = text.indexOf("](", i)
                val end = text.indexOf(')', middle)
                val url = text.substring(middle + 2, end)
                if (url.startsWith("https://")) {
                    withLink(LinkAnnotation.Url(url, link)) { append(text.substring(i + 1, middle)) }
                } else {
                    append(text.substring(i + 1, middle))
                }
                i = end + 1
            }
            else -> {
                append(text[i])
                i++
            }
        }
    }
}

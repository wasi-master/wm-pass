package com.wasimaster.wmpass.ui.help

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.layout.LayoutCoordinates
import androidx.compose.ui.layout.onPlaced
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import com.wasimaster.wmpass.ui.icons.PassIcons
import com.wasimaster.wmpass.ui.theme.palette
import kotlin.math.abs

/**
 * The glyph for a figure's box. The figures are written for every platform with the desktop's
 * icon names; a computer, a cloud or a browser extension has no glyph of its own on the phone, so
 * each takes the nearest one it has.
 */
fun figureIcon(name: String): ImageVector? = when (name) {
    "security_key" -> PassIcons.SecurityKey
    "inventory_2" -> PassIcons.Inventory2
    "lock" -> PassIcons.Lock
    "content_paste" -> PassIcons.ContentPaste
    "person" -> PassIcons.Person
    "visibility_lock" -> PassIcons.VisibilityLock
    "draft" -> PassIcons.Draft
    "cloud" -> PassIcons.CloudSync
    "computer" -> PassIcons.Devices
    "devices" -> PassIcons.Devices
    "group" -> PassIcons.Group
    "extension" -> PassIcons.Widgets
    "terminal" -> PassIcons.Terminal
    "mail" -> PassIcons.Mail
    "key" -> PassIcons.Key
    "folder" -> PassIcons.Folder
    "sync" -> PassIcons.Sync
    "shield" -> PassIcons.Shield
    "phone", "smartphone" -> PassIcons.MobileLockPortrait
    "fingerprint" -> PassIcons.Fingerprint
    "backup" -> PassIcons.Backup
    "language", "public" -> PassIcons.Language
    "dns" -> PassIcons.Dns
    "keyboard" -> PassIcons.Keyboard
    else -> null
}

/**
 * A diagram from the guide: rows of plain boxes, top to bottom, with arrows between neighbours and
 * from one row to the next. The boxes lay themselves out in the theme's own type, then the arrows
 * are drawn between wherever they ended up. Nothing moves.
 */
@Composable
fun FigureView(id: String, modifier: Modifier = Modifier) {
    val figure = Help.figure(id) ?: return
    val colors = palette
    val measurer = rememberTextMeasurer()
    val labelStyle = MaterialTheme.typography.labelSmall.copy(color = colors.textMid)
    // Where each box landed, kept as the layout's own coordinates and turned into a place inside
    // the frame only while the arrows are drawn. A new placement bumps the count, which redraws.
    val placed = remember(id) { Placed() }
    val rowOf = remember(id) { figure.rows.flatMapIndexed { i, row -> row.map { it.id to i } }.toMap() }
    Column(modifier.fillMaxWidth().semantics { contentDescription = figure.caption }) {
        Column(
            Modifier
                .fillMaxWidth()
                .onPlaced {
                    placed.frame = it
                    placed.bump()
                }
                .drawWithContent {
                    drawContent()
                    // Nothing placed yet; reading the count also redraws once something is.
                    if (placed.count == 0) return@drawWithContent
                    val inside = placed.frame?.takeIf { it.isAttached } ?: return@drawWithContent
                    val boxes = placed.boxes.mapNotNull { (key, at) -> if (at.isAttached) key to inside.localBoundingBoxOf(at, clipBounds = false) else null }.toMap()
                    val line = colors.textLow
                    for (edge in figure.edges) {
                        val a = boxes[edge.from] ?: continue
                        val b = boxes[edge.to] ?: continue
                        val dash = if (edge.dashed) PathEffect.dashPathEffect(floatArrayOf(4.dp.toPx(), 3.dp.toPx())) else null
                        val path = Path()
                        val end: Offset
                        val toward: Offset
                        val labelAt: Offset
                        if (abs(a.top - b.top) < 4f) {
                            val leftToRight = a.left < b.left
                            val y = a.center.y
                            val x1 = if (leftToRight) a.right else a.left
                            val x2 = if (leftToRight) b.left else b.right
                            path.moveTo(x1, y)
                            path.lineTo(x2, y)
                            end = Offset(x2, y)
                            toward = Offset(if (leftToRight) 1f else -1f, 0f)
                            labelAt = Offset((x1 + x2) / 2, y - 14.dp.toPx())
                        } else {
                            val down = a.top < b.top
                            val x1 = a.center.x
                            val x2 = b.center.x
                            val y1 = if (down) a.bottom else a.top
                            val y2 = if (down) b.top else b.bottom
                            // The elbow sits halfway across the gap under the row's tallest box, so
                            // arrows leaving one row turn on one line.
                            val rowEdge = boxes.filterKeys { rowOf[it] == rowOf[edge.from] }.values
                                .let { all -> if (down) all.maxOf { it.bottom } else all.minOf { it.top } }
                            val mid = (rowEdge + y2) / 2
                            path.moveTo(x1, y1)
                            if (abs(x1 - x2) < 2f) {
                                path.lineTo(x2, y2)
                            } else {
                                path.lineTo(x1, mid)
                                path.lineTo(x2, mid)
                                path.lineTo(x2, y2)
                            }
                            end = Offset(x2, y2)
                            toward = Offset(0f, if (down) 1f else -1f)
                            labelAt = Offset(x2 + 6.dp.toPx(), mid - 7.dp.toPx())
                        }
                        drawPath(path, line, style = Stroke(1.3.dp.toPx(), cap = StrokeCap.Round, pathEffect = dash))
                        arrowHead(end, toward, line)
                        edge.label?.let { words ->
                            val layout = measurer.measure(words, labelStyle)
                            val x = if (toward.y == 0f) labelAt.x - layout.size.width / 2f else labelAt.x
                            drawText(layout, topLeft = Offset(x, labelAt.y))
                        }
                    }
                },
        ) {
            figure.rows.forEachIndexed { index, row ->
                if (index > 0) Spacer(Modifier.height(30.dp))
                // Neighbours an arrow joins need room for it between them.
                val sideways = row.zipWithNext().any { (a, b) -> figure.edges.any { (it.from == a.id && it.to == b.id) || (it.from == b.id && it.to == a.id) } }
                Row(
                    Modifier.fillMaxWidth(),
                    horizontalArrangement = Arrangement.spacedBy(if (sideways) 26.dp else 8.dp, Alignment.CenterHorizontally),
                    verticalAlignment = Alignment.Top,
                ) {
                    for (box in row) {
                        FigureBoxView(
                            box,
                            Modifier
                                .weight(1f, fill = row.size > 1)
                                .onPlaced {
                                    placed.boxes[box.id] = it
                                    placed.bump()
                                },
                        )
                    }
                }
            }
        }
        Spacer(Modifier.height(10.dp))
        Text(figure.caption, color = colors.textMid, style = MaterialTheme.typography.bodySmall, textAlign = TextAlign.Start)
    }
}

/** The boxes' and the frame's coordinates, as last placed. */
private class Placed {
    var frame: LayoutCoordinates? = null
    val boxes = HashMap<String, LayoutCoordinates>()
    private var placements = 0

    /** Read only while drawing, so a new placement redraws the arrows. */
    var count by mutableIntStateOf(0)
        private set

    /**
     * Called while laying out. It writes the count without reading it: a read here would tie the
     * layout to the count, and the write would then lay it out again, for ever.
     */
    fun bump() {
        placements++
        count = placements
    }
}

private fun DrawScope.arrowHead(tip: Offset, toward: Offset, color: Color) {
    val length = 6.dp.toPx()
    val half = 3.5.dp.toPx()
    val back = Offset(tip.x - toward.x * length, tip.y - toward.y * length)
    val side = Offset(-toward.y * half, toward.x * half)
    val head = Path().apply {
        moveTo(tip.x, tip.y)
        lineTo(back.x + side.x, back.y + side.y)
        lineTo(back.x - side.x, back.y - side.y)
        close()
    }
    drawPath(head, color)
}

@Composable
private fun FigureBoxView(box: FigureBox, modifier: Modifier) {
    val colors = palette
    val edge = when (box.tone) {
        "good" -> colors.primary
        "danger" -> colors.danger
        "warn" -> colors.warn
        else -> colors.hairline
    }
    val muted = box.tone == "muted"
    val shape = RoundedCornerShape(10.dp)
    Column(
        modifier
            .border(1.dp, edge, shape)
            .background(if (colors.flat) MaterialTheme.colorScheme.surfaceContainerHigh else colors.surface2, shape)
            .padding(horizontal = 10.dp, vertical = 8.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
    ) {
        box.icon?.let(::figureIcon)?.let { glyph ->
            Icon(glyph, null, tint = if (muted) colors.textLow else colors.textMid, modifier = Modifier.size(18.dp))
            Spacer(Modifier.height(4.dp))
        }
        Text(
            box.label,
            color = if (muted) colors.textLow else colors.text,
            style = MaterialTheme.typography.labelLarge,
            textAlign = TextAlign.Center,
        )
        box.sub?.let {
            Spacer(Modifier.height(2.dp))
            Text(it, color = colors.textMid, style = MaterialTheme.typography.labelSmall, textAlign = TextAlign.Center)
        }
    }
}

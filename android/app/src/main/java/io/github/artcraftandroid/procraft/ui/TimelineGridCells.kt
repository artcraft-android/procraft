package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.Canvas
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.Paint
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipPath
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.drawscope.withTransform
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.painter.Painter
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TimelineTool
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import kotlin.math.ceil
import kotlin.math.roundToInt

// The grid timeline's cells (see TimelineGrid.kt), as the iPad's TimelineGridCells.swift draws
// them: every frame's empty slot, each clip as a block over the frames it lasts (as an edge or a
// block being dragged would leave them), a mark under each keyed frame, and the handles on the
// frame picked (or on the last of the frames picked together). They fade out under the tracks'
// names.

/**
 * The rows' cells as they stand along the bar at [place]: the frames [lo] to [hi] (those on the
 * bar and some either side), frame `f`'s cell `f` pitches from [GridPlace.origin].
 */
@Composable
internal fun GridCells(
    vm: CanvasViewModel,
    tool: TimelineTool,
    grid: GridState,
    rows: GridRows,
    place: GridPlace,
    lo: Int,
    hi: Int,
    sz: GridSizes,
    labels: Float,
    modifier: Modifier,
) {
    val measurer = rememberTextMeasurer()
    val chevronRight = rememberVectorPainter(PcIcons.ChevronRight)
    val chevronDown = rememberVectorPainter(PcIcons.ChevronDown)
    val chevronLeft = rememberVectorPainter(PcIcons.ChevronLeft)
    // what stands behind a drawing's picture: the canvas's own background (white while it is
    // hidden), so the picture reads as the drawing does on the canvas
    val paper = vm.background.let { if (it.visible) Color(it.r.coerceIn(0f, 1f), it.g.coerceIn(0f, 1f), it.b.coerceIn(0f, 1f)) else Color.White }
    val look = CellLook(
        slot = Grid.Slot, block = Grid.Block, box = Grid.Box, inkBright = Grid.InkBright, accent = Pc.Accent, paper = paper,
        chevronRight = chevronRight, chevronDown = chevronDown, chevronLeft = chevronLeft, measurer = measurer,
    )
    Canvas(
        modifier
            // fading out under the tracks' names
            .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
            .drawWithContent {
                drawContent()
                drawRect(
                    Brush.horizontalGradient(0f to Color.Transparent, 0.45f to Color.Transparent, 1f to Color.Black, startX = 0f, endX = labels * density),
                    blendMode = BlendMode.DstIn,
                )
            },
    ) {
        val cells = CellsDraw(this, tool, grid, rows, place, lo, hi, sz, look)
        cells.slots()
        cells.blocks()
        cells.keyMarks()
        cells.handles()
    }
}

/** What the cells are drawn with, read where the composition can see it changing. */
private class CellLook(
    val slot: Color,
    val block: Color,
    val box: Color,
    val inkBright: Color,
    val accent: Color,
    val paper: Color,
    val chevronRight: Painter,
    val chevronDown: Painter,
    val chevronLeft: Painter,
    val measurer: TextMeasurer,
)

/** One drawing of the cells, in pixels. */
private class CellsDraw(
    val scope: DrawScope,
    val tool: TimelineTool,
    val grid: GridState,
    val rows: GridRows,
    val place: GridPlace,
    val lo: Int,
    val hi: Int,
    val sz: GridSizes,
    val look: CellLook,
) {
    private val d = scope.density
    private val pitch = sz.pitch * d
    private val cellW = sz.cellWidth * d
    private val cellH = sz.cellHeight * d
    private val gap = sz.gap * d
    private val corner = CornerRadius(sz.corner * d)
    private val edges = GridEdges(tool, grid, rows)

    /** The left edge of frame [f]'s cell. */
    private fun x(f: Int): Float = place.origin * d + f * pitch

    /** The top of a cell on row [r]. */
    private fun top(r: GridRow): Float = (r.y + (r.height - sz.cellHeight) / 2f + place.inset - place.scroll) * d

    /** Every frame's slot, empty; and where a repeating group plays a frame again, its ghost. */
    fun slots() = with(scope) {
        for (r in rows.list) {
            val y = top(r)
            // (a track inside a group has slots only for the frames the group lasts)
            for (s in r.segments) {
                val first = maxOf(s.start, lo)
                val last = minOf(s.end - 1, hi)
                if (first > last) continue
                // (too narrow to tell one from the next, they are drawn as one strip)
                if (sz.pitch < 5f) {
                    drawRect(look.slot, Offset(x(first), y), Size((last - first + 1) * pitch, cellH))
                    continue
                }
                for (f in first..last) drawRoundRect(look.slot, Offset(x(f), y), Size(cellW, cellH), corner)
            }
            for (s in r.segments) {
                for (again in tool.place(s.track.id)?.repeats.orEmpty()) {
                    if (again.start.toInt() > hi || (again.start + again.duration).toInt() <= lo) continue
                    drawRoundRect(look.block.copy(alpha = 0.45f), Offset(x(again.start.toInt()), y), Size(again.duration.toInt() * pitch - gap, cellH), corner)
                }
            }
        }
    }

    /**
     * Each clip over the frames it lasts (as an edge or a block being dragged would leave them); an
     * unfolded group with an outline round it and its own tracks' rows. A block held for its menu or
     * dragged elsewhere is drawn over the rest, as is the frame with handles out.
     */
    fun blocks() {
        class Item(val c: TimelineClipInfo, val r: GridRow, val t: TimelineTrackInfo, val hiddenAbove: Boolean, val z: Int)
        val items = ArrayList<Item>()
        for (r in rows.list) {
            for (s in r.segments) {
                val hiddenAbove = tool.place(s.track.id)?.hiddenAbove == true
                for (c in edges.shown(s.track)) {
                    if (!c.within(lo, hi)) continue
                    if (c.kind == TimelineClipKind.GROUP) rows.bottomOfGroup(c.id)?.let { groupOutline(c, r, it) }
                    val z = when {
                        grid.move?.ids?.contains(c.id) == true -> 3
                        tool.selecting == null && (grid.clipMenu?.clip == c.id || tool.picked == c.id) -> 1
                        grid.trim?.clip == c.id -> 1
                        else -> 0
                    }
                    items.add(Item(c, r, s.track, hiddenAbove, z))
                }
            }
        }
        items.sortBy { it.z }
        for (i in items) block(i.c, i.r, i.t, i.hiddenAbove)
    }

    private fun groupOutline(c: TimelineClipInfo, r: GridRow, bottom: Float) = with(scope) {
        val (sx, sw) = edges.span(c)
        val left = (place.origin + sx) * d
        val width = sw * d
        val y = top(r)
        val lit = tool.selecting == null && tool.picked == c.id
        val line = (if (lit) 2f else 1.2f) * d
        val h = (bottom - place.scroll + place.inset) * d - y + 3f * d
        drawRoundRect(
            if (lit) look.accent else look.box.copy(alpha = 0.7f),
            Offset(left - 3f * d + line / 2f, y - 3f * d + line / 2f),
            Size(width + 6f * d - line, h - line),
            CornerRadius((sz.corner + 3f) * d),
            style = Stroke(line),
        )
    }

    /**
     * A clip over the frames it lasts. A drawing shows its picture on the canvas's paper in each
     * frame, the first as it is and the held ones shaded; a group, collapsed, the drawings in it
     * on each frame; a sound or a video is a plain block with its name.
     */
    private fun block(c: TimelineClipInfo, r: GridRow, t: TimelineTrackInfo, hiddenAbove: Boolean) = with(scope) {
        val (sx, sw) = edges.span(c)
        val left = (place.origin + sx) * d
        val y = top(r)
        val w = sw * d
        val chosen = tool.selecting?.contains(c.id) == true
        val picked = tool.selecting == null && tool.picked == c.id
        // held for its menu: lifted a little off the bar
        val lifted = tool.selecting == null && grid.clipMenu?.clip == c.id
        // dragged somewhere else: lifted off the bar where it would land (faint where it is, while
        // it can't be dropped or is over the line between two rows)
        val moving = grid.move?.ids?.contains(c.id) == true
        val nowhere = moving && (grid.move?.valid != true || grid.move?.between != null)
        // the frames of it on the bar, counted from its first
        val from = maxOf(lo - c.start.toInt(), 0)
        val to = minOf(hi - c.start.toInt(), ceil((w + gap) / pitch).toInt() - 1)
        val tint = c.label?.let { rgb(it) }
        val alpha = if (nowhere) 0.4f else if (t.hidden || hiddenAbove) 0.45f else if (c.disabled) 0.4f else 1f
        val bounds = Rect(left, y, left + w, y + cellH)
        val up = lifted || moving
        // (a little bigger all round, however long it is)
        withTransform({ if (up) scale(1f + 5f * d / maxOf(w, 1f), 1.1f, bounds.center) }) {
            if (up) drawRoundRect(Color.Black.copy(alpha = 0.3f), Offset(left - 2f * d, y), Size(w + 4f * d, cellH + 4f * d), CornerRadius((sz.corner + 2f) * d))
            if (alpha < 1f) drawContext.canvas.saveLayer(bounds, Paint().apply { this.alpha = alpha })
            clipPath(Path().apply { addRoundRect(RoundRect(bounds, corner)) }) {
                when (c.kind) {
                    TimelineClipKind.DRAWING -> {
                        drawRect(look.paper, bounds.topLeft, bounds.size)
                        val picture = if (c.root != 0uL) tool.thumbnails[c.root] else null
                        if (picture != null && sz.pictures && from <= to) {
                            for (f in from..to) picture(picture, left + f * pitch, y)
                        }
                        // held past its first frame
                        if (w > cellW + 1f) drawRect(Grid.HeldShade, Offset(left + cellW, y), Size(w - cellW, cellH))
                    }
                    TimelineClipKind.GROUP -> group(c, bounds, from, to, tint)
                    else -> {
                        drawRect(tint ?: look.block, bounds.topLeft, bounds.size)
                        // (its name follows the bar's edge no further than its own first frame)
                        val laid = look.measurer.measure(c.name, TextStyle(color = look.inkBright, fontSize = 12.sp), maxLines = 1)
                        drawText(laid, topLeft = Offset(left + from * pitch + 7f * d, y + (cellH - laid.size.height) / 2f))
                    }
                }
                // touched by the stroke under way: glowing
                if (chosen && grid.stroke.isNotEmpty()) drawRect(look.accent.copy(alpha = 0.28f), bounds.topLeft, bounds.size)
            }
            if (alpha < 1f) drawContext.canvas.restore()
            if (chosen || picked || moving) {
                val line = 2.5f * d
                drawRoundRect(
                    look.accent,
                    Offset(left - line / 2f, y - line / 2f),
                    Size(w + line, cellH + line),
                    CornerRadius((sz.corner + 2f) * d - line / 2f),
                    style = Stroke(line),
                )
            }
        }
    }

    /** [image] fitted into the cell at [left], [y], in its middle. */
    private fun picture(image: ImageBitmap, left: Float, y: Float) = with(scope) {
        val scale = minOf(cellW / maxOf(image.width, 1), cellH / maxOf(image.height, 1))
        val w = (image.width * scale).roundToInt()
        val h = (image.height * scale).roundToInt()
        drawImage(
            image,
            srcOffset = IntOffset.Zero,
            srcSize = IntSize(image.width, image.height),
            dstOffset = IntOffset((left + (cellW - w) / 2f).roundToInt(), (y + (cellH - h) / 2f).roundToInt()),
            dstSize = IntSize(w, h),
        )
    }

    /**
     * A group over the frames it lasts. Collapsed, each of its frames shows the drawings in it
     * there, one over another; unfolded (its own tracks' rows showing under it), a plain strip
     * heading them. Either way its name, and a chevron that folds or unfolds it.
     */
    private fun group(c: TimelineClipInfo, bounds: Rect, from: Int, to: Int, tint: Color?) = with(scope) {
        val open = tool.unfolded(c.id)
        val shots = tool.groupShots[c.id].orEmpty()
        val merged = !open && sz.pictures && shots.isNotEmpty()
        drawRect(if (merged) look.paper else tint ?: look.block, bounds.topLeft, bounds.size)
        if (merged && from <= to) {
            for (shot in shots) {
                val first = maxOf(shot.start.toInt() - c.start.toInt(), from)
                val last = minOf((shot.start + shot.frames).toInt() - c.start.toInt() - 1, to)
                if (first > last) continue
                // everything it shows there, bottom first (a video's pictures aren't read here)
                for (item in shot.items) {
                    if (item.root == 0uL) continue
                    val image = tool.thumbnails[item.root] ?: continue
                    for (f in first..last) picture(image, bounds.left + f * pitch, bounds.top)
                }
            }
            if (tint != null) drawRect(tint.copy(alpha = 0.3f), bounds.topLeft, bounds.size)
        }
        // where it plays its frames again: a notch at each repeat
        if (c.cycle > 0u) {
            var f = c.cycle.toInt()
            while (f < c.duration.toInt()) {
                drawRect(look.accent.copy(alpha = 0.85f), Offset(bounds.left + f * pitch - (pitch - cellW) / 2f - 0.75f * d, bounds.top), Size(1.5f * d, cellH))
                f += c.cycle.toInt()
            }
        }
        if (sz.rowScale > 0.7f) {
            val laid = look.measurer.measure(c.name, TextStyle(color = Color.White, fontSize = 9.5.sp, fontWeight = FontWeight.SemiBold), maxLines = 1)
            val icon = 11f * d
            val tagH = 15f * d
            val tagW = 5f * d + icon + 3f * d + laid.size.width + 5f * d
            val tx = bounds.left + from * pitch + 3f * d
            val ty = bounds.top + 2f * d
            drawRoundRect(Color.Black.copy(alpha = 0.55f), Offset(tx, ty), Size(tagW, tagH), CornerRadius(tagH / 2f))
            translate(tx + 5f * d, ty + (tagH - icon) / 2f) {
                with(if (open) look.chevronDown else look.chevronRight) { draw(Size(icon, icon), colorFilter = ColorFilter.tint(Color.White)) }
            }
            drawText(laid, topLeft = Offset(tx + 5f * d + icon + 3f * d, ty + (tagH - laid.size.height) / 2f))
        }
    }

    /**
     * The handles on the picked frame's edges (or on the last of the frames picked together): a tab
     * in the accent with an arrowhead against each edge (while one is dragged, only it). The rows'
     * touch takes the drags (see GridEdges.handle). A repeating group's end dragged: how many times
     * it plays now.
     */
    fun handles() = with(scope) {
        if (tool.isPlaying) return@with
        val h = edges.handled ?: return@with
        val (sx, sw) = edges.span(h.clip)
        val row = rows[h.row]
        val mid = (row.y + row.height / 2f + place.inset - place.scroll) * d
        val leading = grid.trim?.leading ?: grid.pickedTrim?.leading
        val tabW = 16f * d
        val tabH = (sz.cellHeight - 3f) * d
        val round = CornerRadius(minOf(6f, sz.cellHeight / 4f) * d)
        val icon = 14f * d
        fun tab(x: Float, arrow: Painter) {
            drawRoundRect(look.accent, Offset(x, mid - tabH / 2f), Size(tabW, tabH), round)
            translate(x + (tabW - icon) / 2f, mid - icon / 2f) {
                with(arrow) { draw(Size(icon, icon), colorFilter = ColorFilter.tint(Color.White)) }
            }
        }
        val x0 = place.origin + sx
        // (a repeating group's start stays put)
        if (leading != false && h.clip.cycle == 0u) tab((x0 + GridHandle.OVERLAP) * d - tabW, look.chevronLeft)
        if (leading != true) tab((x0 + sw - GridHandle.OVERLAP) * d, look.chevronRight)
        val tr = grid.trim ?: return@with
        if (tr.cycle <= 0) return@with
        val now = tr.clips.firstOrNull { it.id == tr.clip } ?: return@with
        val laid = look.measurer.measure(cycleTag(now.duration.toInt(), tr.cycle), TextStyle(color = Color.White, fontSize = 12.sp, fontWeight = FontWeight.SemiBold), maxLines = 1)
        val tagH = 20f * d
        val tagW = laid.size.width + 14f * d
        val tx = (x0 + sw + 14f) * d
        drawRoundRect(look.accent, Offset(tx, mid - tagH / 2f), Size(tagW, tagH), CornerRadius(tagH / 2f))
        drawText(laid, topLeft = Offset(tx + 7f * d, mid - laid.size.height / 2f))
    }

    /** A mark under each keyed frame of a block. */
    fun keyMarks() = with(scope) {
        if (sz.pitch < 9f) return@with
        val marks = Path()
        for (r in rows.list) {
            val y = (r.y + (r.height + sz.cellHeight) / 2f - 4.5f + place.inset - place.scroll) * d
            for (s in r.segments) {
                for (c in s.track.clips) {
                    val done = HashSet<UInt>()
                    for (k in tool.keys[c.id].orEmpty()) {
                        val f = k.frame.toInt()
                        if (f < lo || f > hi || !done.add(k.frame)) continue
                        val cx = x(f) + cellW / 2f
                        val m = 3.2f * d
                        marks.moveTo(cx, y - m)
                        marks.lineTo(cx + m, y)
                        marks.lineTo(cx, y + m)
                        marks.lineTo(cx - m, y)
                        marks.close()
                    }
                }
            }
        }
        drawPath(marks, Color.White, style = Stroke(1.6f * d))
        drawPath(marks, look.accent)
    }
}

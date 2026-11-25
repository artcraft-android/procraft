package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.geometry.Offset
import io.github.artcraftandroid.procraft.canvas.TimelineTool
import io.github.artcraftandroid.procraft.canvas.groupRoom
import io.github.artcraftandroid.procraft.canvas.setGroupLength
import io.github.artcraftandroid.procraft.canvas.trim
import io.github.artcraftandroid.procraft.canvas.trimPicked
import io.github.artcraftandroid.procraft.canvas.trimPreview
import io.github.artcraftandroid.procraft.canvas.trimsPreview
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import kotlin.math.abs
import kotlin.math.roundToInt

// The frames' edges in the grid (see TimelineGrid.kt), as the iPad's `GridEdges`
// (TimelineGridCells.swift): which frame has handles, where they are, and what dragging one does.
// The frame under the playhead, tapped, has a handle on each edge (with the lasso on, the last of
// the frames picked together has them): dragged, it lasts longer or shorter, shown as it goes from
// the core's preview, one step when let go; held at a limit, a line stands down the tracks there.
// A repeating group's end sets how long it plays, with a ×3 tag.

/** A handle: how wide it reaches for the finger, and how much of that lies over the frame's own edge (dp). */
internal object GridHandle {
    const val WIDTH = 28f
    const val OVERLAP = 6f
}

/**
 * An edge of the frame with handles being dragged: whose and which, the frame that edge started on,
 * the frame the clip ended on (one past its last) as the drag began, the frame asked for now (a
 * start's may be before the first), how far the finger has gone (dp), its track's clips as the drag
 * would leave them (empty: as they are), and the frame a limit holds the edge at. A repeating
 * group's end: how many frames it plays again and again ([cycle], 0: no such thing), and the most
 * frames it has room for.
 */
internal data class GridTrim(
    val clip: ULong,
    val track: ULong,
    val leading: Boolean,
    val from: Int,
    val end: Int,
    val frame: Int,
    val dx: Float,
    val clips: List<TimelineClipInfo> = emptyList(),
    val limit: Float? = null,
    val cycle: Int = 0,
    val most: Int = 0,
)

/**
 * An edge of the frames picked together being dragged: whose handle, which edge, the frame it
 * started on, how many columns the handle goes for each frame every picked frame grows, how many
 * frames they have grown, the tracks as that would leave them, and a limit's frame.
 */
internal data class GridPickedTrim(
    val clip: ULong,
    val leading: Boolean,
    val from: Int,
    val steps: Int,
    val by: Int,
    val tracks: Map<ULong, List<TimelineClipInfo>> = emptyMap(),
    val limit: Float? = null,
)

/** The clip with handles, its track and row (top first), and whether it stands for the frames picked together. */
internal data class GridHandled(val clip: TimelineClipInfo, val track: TimelineTrackInfo, val row: Int, val picked: Boolean)

/** How a repeating group's length reads while its end is dragged: whole cycles as "×3", a last one cut short as "×2 +1". */
internal fun cycleTag(frames: Int, length: Int): String {
    val whole = frames / maxOf(length, 1)
    val over = frames % maxOf(length, 1)
    return if (over == 0) "×$whole" else "×$whole +$over"
}

/** The frames' edges as they show and as they are dragged. Positions are dp in the frames' own space: frame `f` starts `f` pitches along. */
internal class GridEdges(private val tool: TimelineTool, private val grid: GridState, private val rows: GridRows) {
    private val sz = grid.sizes
    private val pitch = sz.pitch

    /** Track [t]'s clips as they show: as a block dragged elsewhere or an edge being dragged would leave them, else as they are. */
    fun shown(t: TimelineTrackInfo): List<TimelineClipInfo> {
        grid.move?.shown?.get(t.id)?.let { return it }
        grid.trim?.let { if (it.track == t.id && it.clips.isNotEmpty()) return it.clips }
        return grid.pickedTrim?.tracks?.get(t.id) ?: t.clips
    }

    /** Where clip [c] shows: its left edge and width. An edge of it being dragged follows the finger between frames, as far as the drag can take it. */
    fun span(c: TimelineClipInfo): Pair<Float, Float> {
        var x0 = c.start.toInt() * pitch
        var x1 = (c.start + c.duration).toInt() * pitch
        val tr = grid.trim
        if (tr != null && tr.clip == c.id) {
            val raw = tr.from * pitch + tr.dx
            // within half a frame of the frame it is on (held at a limit, it stays on the frame it
            // got to), and never shorter than a frame
            val near = raw.coerceIn((tr.frame - 0.5f) * pitch, (tr.frame + 0.5f) * pitch)
            if (tr.leading) {
                if (c.start.toInt() == tr.frame) {
                    x0 = maxOf(minOf(near, x1 - pitch), 0f)
                } else if (raw < x0 && (c.start + c.duration).toInt() > tr.end) {
                    // the frames before it have no more room: it grows at its end by as much as
                    // its start falls short of the finger
                    val wanted = tr.end * pitch + (x0 - raw)
                    x1 = minOf(maxOf(wanted, x1 - pitch / 2f), x1 + pitch / 2f)
                }
            } else if ((c.start + c.duration).toInt() == tr.frame) {
                x1 = maxOf(near, x0 + pitch)
            }
        }
        return x0 to maxOf(x1 - x0 - sz.gap, minOf(8f, sz.cellWidth))
    }

    /** The clip with handles on its edges: with the lasso on, the last of the frames picked in time (the topmost of those ending together); else the picked one. */
    val handled: GridHandled?
        get() {
            // (none while a block is dragged elsewhere)
            if (grid.move != null) return null
            val chosen = tool.selecting
            if (chosen != null) {
                if (chosen.isEmpty() || grid.stroke.isNotEmpty()) return null
                var best: GridHandled? = null
                for ((r, t) in rows.tracks) {
                    if (tool.held(t)) continue
                    for (c in shown(t)) {
                        if (c.id !in chosen) continue
                        if (best == null || c.start + c.duration > best.clip.start + best.clip.duration) best = GridHandled(c, t, r, true)
                    }
                }
                return best
            }
            val id = tool.picked ?: return null
            for ((r, t) in rows.tracks) {
                if (tool.held(t)) continue
                shown(t).firstOrNull { it.id == id }?.let { return GridHandled(it, t, r, false) }
            }
            return null
        }

    /** Which handle is at [point] (frames' space across, the rows' down), if one is: the start's (true) or the end's. A handle takes a finger's width beyond its edge. */
    fun handle(point: Offset): Boolean? {
        val h = handled ?: return null
        if (tool.isPlaying) return null
        val row = rows[h.row]
        if (point.y < row.y || point.y >= row.y + row.height) return null
        val (x, w) = span(h.clip)
        val reach = GridHandle.WIDTH - GridHandle.OVERLAP
        // on a frame too narrow for both, each keeps its own half
        val inside = minOf(GridHandle.OVERLAP + 4f, w / 2f)
        if (point.x >= x + w - inside && point.x <= x + w + reach) return false
        // (a repeating group's start stays put)
        if (h.clip.cycle == 0u && point.x >= x - reach && point.x <= x + inside) return true
        return null
    }

    /** The handle on the start ([leading]) or the end dragged [dx] from where it was. */
    fun dragged(leading: Boolean, dx: Float) {
        // (the drag under way keeps the edge it began on)
        grid.trim?.let {
            edgeDragged(it.clip, it.track, it.from, it.end, it.leading, dx)
            return
        }
        grid.pickedTrim?.let {
            pickedDragged(it.clip, it.steps, it.from, it.leading, dx)
            return
        }
        val h = handled ?: return
        val from = if (leading) h.clip.start.toInt() else (h.clip.start + h.clip.duration).toInt()
        if (h.picked) {
            // an end goes as far as every picked frame before it on its track grows, and its own
            val chosen = tool.selecting.orEmpty()
            val before = h.track.clips.count { it.id in chosen && it.start <= h.clip.start }
            pickedDragged(h.clip.id, if (leading) 1 else maxOf(before, 1), from, leading, dx)
        } else {
            edgeDragged(h.clip.id, h.track.id, from, (h.clip.start + h.clip.duration).toInt(), leading, dx)
        }
    }

    /** The frame's edge dragged [dx] from where it was: the frame asked for, its track as that would leave it, and the limit holding it short of there if one does. */
    private fun edgeDragged(clip: ULong, track: ULong, from: Int, end: Int, leading: Boolean, dx: Float) {
        val first = grid.trim == null
        if (first) {
            tool.stopPlaying()
            grid.closeMenus(tool)
        }
        var tr = (grid.trim ?: GridTrim(clip, track, leading, from, end, frame = from, dx = 0f)).copy(dx = dx)
        // a repeating group's end: how long it plays, up to the next block on its track
        if (first && !leading) {
            tool.clip(clip)?.second?.let { c -> if (c.cycle > 0u) tr = tr.copy(cycle = c.cycle.toInt(), most = tool.groupRoom(clip) ?: c.duration.toInt()) }
        }
        if (tr.cycle > 0) {
            grid.trim = cycleDragged(tr, dx, first)
            return
        }
        val raw = tr.from + dx / pitch
        // (a start may be asked before the first frame: what it can't have there goes on its end)
        val asked = if (leading) raw.roundToInt() else maxOf(raw.roundToInt(), 0)
        val before = edges(tr)
        if (asked != tr.frame || first) {
            tr = tr.copy(frame = asked, clips = if (asked == tr.from) emptyList() else tool.trimPreview(tr.clip, leading, asked))
        }
        // where the edge got to: short of what was asked, something holds it there
        val got = edge(tr)
        if (edges(tr) != before) grid.haptic(GridHaptic.TICK)
        val held = abs(raw - got) > 0.5f
        val limit = if (held) limitLine(got, leading) else null
        if (limit != null && tr.limit == null) grid.haptic(GridHaptic.FIRM)
        grid.trim = tr.copy(limit = limit)
    }

    /** A repeating group's end dragged [dx]: it plays as many frames, a tick as it passes each whole cycle, no further than there is room for. */
    private fun cycleDragged(tr: GridTrim, dx: Float, first: Boolean): GridTrim {
        val (t, c) = tool.clip(tr.clip) ?: return tr
        val start = c.start.toInt()
        val asked = (tr.from + dx / pitch).roundToInt()
        val frames = minOf(maxOf(asked - start, 1), maxOf(tr.most, 1))
        val before = tr.frame
        val now = start + frames
        val clips = t.clips.map { if (it.id == c.id) it.copy(duration = frames.toUInt()) else it }
        val held = asked - start > tr.most
        if (held && tr.limit == null) {
            grid.haptic(GridHaptic.FIRM)
        } else if (now != before || first) {
            // (firmer where a whole cycle ends)
            grid.haptic(if (frames % tr.cycle == 0) GridHaptic.LIGHT else GridHaptic.TICK)
        }
        return tr.copy(frame = now, clips = clips, limit = if (held) limitLine(now, false) else null)
    }

    /** Where the line of an edge held on frame [got] stands, in frames: a start's just before it; an end's (a frame can be no shorter than one) through the frame it ends on. */
    private fun limitLine(got: Int, leading: Boolean): Float =
        if (leading) got - sz.gap / 2f / pitch else got - (pitch - sz.cellWidth / 2f) / pitch

    /** The frames the dragged clip starts and ends on, as the drag has left it so far. */
    private fun edges(tr: GridTrim): List<Int> {
        val c = tr.clips.firstOrNull { it.id == tr.clip } ?: return emptyList()
        return listOf(c.start.toInt(), (c.start + c.duration).toInt())
    }

    /** The frame the dragged edge is on, as the drag has left it so far. */
    private fun edge(tr: GridTrim): Int {
        val c = tr.clips.firstOrNull { it.id == tr.clip } ?: return tr.from
        return if (tr.leading) c.start.toInt() else (c.start + c.duration).toInt()
    }

    /** The handle of the frames picked together dragged [dx]: every one of them a frame longer (or shorter) for each [steps] columns it goes, the tracks as that would leave them. */
    private fun pickedDragged(clip: ULong, steps: Int, from: Int, leading: Boolean, dx: Float) {
        if (grid.pickedTrim == null) {
            tool.stopPlaying()
            grid.closeMenus(tool)
        }
        var g = grid.pickedTrim ?: GridPickedTrim(clip, leading, from, steps, 0)
        val by = (dx / (pitch * g.steps)).roundToInt()
        if (by != g.by) {
            val tracks = if (by == 0) emptyMap() else tool.trimsPreview(leading, by)
            val got = tracks.values.flatten().firstOrNull { it.id == g.clip }?.let { if (leading) it.start.toInt() else (it.start + it.duration).toInt() } ?: g.from
            // short of where it was asked to go: held there
            val held = got != g.from + by * g.steps
            if (held && g.limit == null) grid.haptic(GridHaptic.FIRM) else if (!held) grid.haptic(GridHaptic.TICK)
            g = g.copy(by = by, tracks = tracks, limit = if (held) limitLine(got, leading) else null)
        }
        grid.pickedTrim = g
    }

    /** The handle let go: the edge stays where the drag left it, as one undo step. */
    fun ended() {
        grid.trim?.let { tr ->
            grid.trim = null
            if (tr.cycle > 0) {
                if (tr.frame != tr.from) tool.clip(tr.clip)?.second?.let { tool.setGroupLength(tr.clip, tr.frame - it.start.toInt()) }
            } else if (tr.frame != tr.from && tr.clips.isNotEmpty()) {
                tool.trim(tr.clip, tr.leading, tr.frame)
            }
        }
        grid.pickedTrim?.let { g ->
            grid.pickedTrim = null
            if (g.by != 0 && g.tracks.isNotEmpty()) tool.trimPicked(g.leading, g.by)
        }
    }

    /** The drag taken away: the edge stays as it was. */
    fun cancel() {
        grid.trim = null
        grid.pickedTrim = null
    }
}

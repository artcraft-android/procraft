package io.github.artcraftandroid.procraft.ui

import io.github.artcraftandroid.procraft.canvas.KeyLaneRow
import io.github.artcraftandroid.procraft.core.TimelineRow
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import kotlin.math.abs
import kotlin.math.roundToInt

// The grid timeline's sizes and rows (see TimelineGrid.kt), as the iPad's TimelineGridRows.swift
// has them. The grid zooms (TimelineGridZoom.kt): a frame's cell and a row get smaller together as
// it is pinched out, then the frames alone go on narrowing until the whole timeline fits; pinched
// in past where it starts, the frames alone get wider. So every size comes from [GridSizes], at a
// zoom, and where a row stands from [GridRows]. Sizes are in dp, the iPad's points.

/** How big the grid's cells and rows are at a zoom (1: as it starts). */
internal data class GridSizes(val zoom: Float = 1f) {
    /** From one frame's cell to the next. */
    val pitch: Float get() = PITCH * zoom

    /** A row's height: smaller with the frames down to half, never taller than it starts. */
    val rowHeight: Float get() = (ROW_HEIGHT * zoom.coerceIn(LEAST_ROWS, 1f)).roundToInt().toFloat()

    /** The space after a cell, narrowing with it. */
    val gap: Float get() = (PITCH - CELL_WIDTH) * minOf(zoom, 1f)
    val cellWidth: Float get() = pitch - gap
    val cellHeight: Float get() = (rowHeight * CELL_HEIGHT / ROW_HEIGHT).roundToInt().toFloat()
    val corner: Float get() = minOf(CORNER, cellWidth / 3f, cellHeight / 4f)

    /** How much smaller than it starts a row's own furniture (a track's name) is drawn. */
    val rowScale: Float get() = rowHeight / ROW_HEIGHT

    /** Whether a frame is wide enough to show its drawing. */
    val pictures: Boolean get() = cellWidth >= 20f

    /** Where frame 0's cell's left edge is with frame [at] under the [playhead]'s middle: zoomed, the frames grow and shrink about the playhead. */
    fun origin(playhead: Float, at: Float): Float = playhead - cellWidth / 2f - at * pitch

    companion object {
        /** A frame's cell and a row as the grid starts: from one frame to the next, the cell's own size, and a row's height. */
        const val PITCH = 44f
        const val CELL_WIDTH = 40f
        const val CELL_HEIGHT = 31f
        const val ROW_HEIGHT = 44f
        const val CORNER = 7f

        /** The widest a frame gets, as a zoom; and how small a row gets (zoomed out further, the frames alone narrow). */
        const val MOST = 2f
        const val LEAST_ROWS = 0.5f

        /** The narrowest a frame ever is (dp). */
        const val LEAST_PITCH = 0.75f
    }
}

/**
 * A track's frames on a row, and the frames of the timeline it has: a track of the timeline's own,
 * all of them; a track inside a group, the frames the group lasts.
 */
internal data class GridSegment(
    val track: TimelineTrackInfo,
    val start: Int,
    val end: Int,
    /** The group clip whose own track it is (0: none), and every group it is inside, outermost first. */
    val group: ULong,
    val groups: List<ULong>,
)

/**
 * One row of the grid, and where it stands among the rows. A track of the timeline's own has a row
 * to itself. The tracks inside groups share theirs: the unfolded groups on one track have their
 * first tracks side by side on the first row under it, their second on the next, and so on, each
 * over the frames its group lasts.
 */
internal data class GridRow(
    /** The tracks on it, in time order (one, but under a track holding several unfolded groups). */
    val segments: List<GridSegment>,
    /** How many groups its tracks are inside. */
    val depth: Int,
    /** Its top edge among all the rows, and its height. */
    val y: Float = 0f,
    val height: Float = 0f,
    /** A row of keys under its tracks' row, not of frames: one kind of key (moved, scaled…, or a sound's level) of each of its tracks whose keys are out. */
    val lanes: List<KeyLaneRow> = emptyList(),
) {
    /** The segment whose frames [frame] is among; where it is among none's, the nearest's. */
    fun segment(frame: Int): GridSegment =
        segments.firstOrNull { frame >= it.start && frame < it.end }
            ?: segments.minByOrNull { minOf(abs(frame - it.start), abs(frame - it.end)) }
            ?: segments[0]

    /** The track whose frames [frame] is among; where it is among none's, the nearest's. */
    fun track(frame: Int): TimelineTrackInfo = segment(frame).track

    /** Whether [frame] is one of a track's on it (a row of keys has no frames). */
    fun has(frame: Int): Boolean = lanes.isEmpty() && segments.any { frame >= it.start && frame < it.end }
}

/** How tall a row of keys is before the grid is zoomed out. */
private const val KEYS_HEIGHT = 28f

/** The grid's rows, top first, each where it stands; [keys] gives the rows of keys that are out under a track. */
internal class GridRows(rows: List<TimelineRow>, sizes: GridSizes, keys: (TimelineTrackInfo) -> List<KeyLaneRow> = { emptyList() }) {
    val list: List<GridRow>

    /** How tall they all are together; and how tall before the grid is zoomed out (the bar stays that tall, the rows standing in the middle of it). */
    val height: Float
    val startHeight: Float

    init {
        var y = 0f
        val out = ArrayList<GridRow>()
        for (row in level(rows, 0, emptyList())) {
            out.add(row.copy(y = y, height = sizes.rowHeight))
            y += sizes.rowHeight
            // its tracks' rows of keys: one row for each kind any of them has (the tracks of
            // several groups sharing the row share those too)
            val lanes = row.segments.flatMap { keys(it.track) }
            for (kind in KeyLaneRow.Kinds) {
                val these = lanes.filter { it.sameKind(kind) }
                if (these.isEmpty()) continue
                val h = maxOf((KEYS_HEIGHT * sizes.rowScale).roundToInt().toFloat(), 16f)
                out.add(GridRow(row.segments, row.depth, y, h, these))
                y += h
            }
        }
        list = out
        height = y
        startHeight = out.sumOf { (if (it.lanes.isEmpty()) GridSizes.ROW_HEIGHT else KEYS_HEIGHT).toDouble() }.toFloat()
    }

    val size: Int get() = list.size

    operator fun get(index: Int): GridRow = list[index]

    /** The row at height [y] of all the rows; none off them. */
    fun index(y: Float): Int? {
        if (y < 0f) return null
        return list.indexOfFirst { y >= it.y && y < it.y + it.height }.takeIf { it >= 0 }
    }

    /** The row track [id] is on. */
    fun indexOf(id: ULong?): Int? {
        id ?: return null
        return list.indexOfFirst { r -> r.lanes.isEmpty() && r.segments.any { it.track.id == id } }.takeIf { it >= 0 }
    }

    /** Every track on the rows, with its row, top first. */
    val tracks: List<Pair<Int, TimelineTrackInfo>> get() = list.flatMapIndexed { r, row -> if (row.lanes.isEmpty()) row.segments.map { r to it.track } else emptyList() }

    /** The bottom of the last row that has a track inside group [id]: how far down the group's own rows go. None where it has none showing. */
    fun bottomOfGroup(id: ULong): Float? = list.lastOrNull { r -> r.segments.any { id in it.groups } }?.let { it.y + it.height }

    private companion object {
        fun segment(r: TimelineRow, groups: List<ULong>) =
            GridSegment(r.track, r.start.toInt(), minOf(r.end.toLong(), Int.MAX_VALUE.toLong()).toInt(), r.group, groups)

        /** The row at the start of [rows], with the rows deeper than it that follow it (its groups'), and the rest. */
        fun cut(rows: List<TimelineRow>): Triple<TimelineRow, List<TimelineRow>, List<TimelineRow>>? {
            val first = rows.firstOrNull() ?: return null
            var end = 1
            while (end < rows.size && rows[end].depth > first.depth) end++
            return Triple(first, rows.subList(1, end), rows.subList(end, rows.size))
        }

        /** The rows of one level's tracks, each with its own row, and the rows of its groups under it. */
        fun level(rows: List<TimelineRow>, depth: Int, groups: List<ULong>): List<GridRow> {
            val out = ArrayList<GridRow>()
            var left = rows
            while (true) {
                val (row, inside, rest) = cut(left) ?: break
                out.add(GridRow(listOf(segment(row, groups)), depth))
                out.addAll(shared(inside, depth + 1, groups))
                left = rest
            }
            return out
        }

        /** The rows of the tracks of the groups on one track: the groups' first tracks on one row, their second on the next, each followed by the rows of the groups on those tracks. */
        fun shared(rows: List<TimelineRow>, depth: Int, groups: List<ULong>): List<GridRow> {
            val byGroup = ArrayList<Pair<ULong, MutableList<Pair<TimelineRow, List<TimelineRow>>>>>()
            var left = rows
            while (true) {
                val (row, inside, rest) = cut(left) ?: break
                val i = byGroup.indexOfFirst { it.first == row.group }
                if (i >= 0) byGroup[i].second.add(row to inside) else byGroup.add(row.group to mutableListOf(row to inside))
                left = rest
            }
            val out = ArrayList<GridRow>()
            val most = byGroup.maxOfOrNull { it.second.size } ?: 0
            for (k in 0 until most) {
                val there = byGroup.filter { k < it.second.size }
                val segments = there.map { segment(it.second[k].first, groups + it.first) }.sortedBy { it.start }
                out.add(GridRow(segments, depth))
                for (g in there) out.addAll(shared(g.second[k].second, depth + 1, groups + g.first))
            }
            return out
        }
    }
}

package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.animate
import androidx.compose.animation.core.calculateTargetValue
import androidx.compose.animation.core.exponentialDecay
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEvent
import androidx.compose.ui.input.pointer.PointerInputChange
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.util.VelocityTracker
import io.github.artcraftandroid.procraft.canvas.TimelineTool
import io.github.artcraftandroid.procraft.canvas.drop
import io.github.artcraftandroid.procraft.canvas.dropOnNewTrack
import io.github.artcraftandroid.procraft.canvas.placeManyPreview
import io.github.artcraftandroid.procraft.canvas.placePreview
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelineInfo
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import kotlin.math.abs
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.roundToInt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

// A finger on the grid's rows (see TimelineGrid.kt), as the iPad's GridBody tells its touches
// apart (TimelineGrid.swift, TimelineGridMove.swift): one handler for a tap, a drag sideways
// (scrub) or up and down (more tracks than show), a lasso stroke, a handle, and a finger held
// still (a menu; dragged on from there, the block held goes along by frames and rows).

/** What a finger down on the frames is doing. */
internal enum class GridTouch {
    /** Not yet told apart: a tap, a hold, or the start of a drag. */
    UNDECIDED,

    /** Dragged sideways: the frames slide under the playhead. */
    SCROLL,

    /** Dragged up or down where there are more tracks than show: they scroll. */
    PAN,

    /** Dragged with the lasso on: a stroke picking the frames it crosses. */
    STROKE,

    /** Down on a frame's handle: dragged, the frame lasts longer or shorter. */
    HANDLE,

    /** Held still: a menu opened. */
    HELD,

    /** Dragged on from a hold on a block: the block goes with the finger. */
    MOVE,

    /** Nothing more of the grid's (a second finger dropped the stroke). */
    IGNORE,
}

/** A buzz for what a touch does: a tick, a limit reached, a light knock, a hold, a refusal. */
internal enum class GridHaptic { TICK, FIRM, LIGHT, HOLD, WARN }

/** A block held, its menu open: whose, and where along the bar (dp from its left edge) it stands. */
internal data class GridClipMenu(val clip: ULong, val x: Float)

/** An empty frame held: its track, the frame, and where along the bar its menu stands. */
internal data class GridEmptySpot(val track: ULong, val frame: UInt, val x: Float)

/** The line between two rows a dragged block is over: the track it is beside, whether the new track goes below that one (else above), and where the line is among the rows. */
internal data class GridBetween(val beside: ULong, val below: Boolean, val y: Float)

/**
 * A block (or the blocks picked together) being dragged somewhere else: the block under the
 * finger and every block going with it; its track and first frame as the drag began, and the
 * frame under the playhead then; how many frames along it has gone, the track it is over or the
 * line between two; the tracks as dropping it here would leave them, and whether it can be.
 */
internal data class GridMove(
    val clip: ULong,
    val ids: List<ULong>,
    val track: ULong,
    val start: Int,
    val scrollFrom: Float,
    val by: Int = 0,
    val over: ULong? = null,
    val between: GridBetween? = null,
    val shown: Map<ULong, List<TimelineClipInfo>> = emptyMap(),
    val valid: Boolean = false,
)

/** How long a finger stays still on a frame to hold it, and how far it may move meanwhile (dp). */
private const val HOLD_MS = 400L
private const val STILL = 6f

/**
 * What the rows' touch handler reads, as the grid was last laid out (it is set again each time):
 * the rows, where the frames stand, the timeline, how wide the bar is and where its names end.
 */
internal class GridRowsInput(val tool: TimelineTool, val grid: GridState) {
    lateinit var rows: GridRows
    lateinit var place: GridPlace
    lateinit var info: TimelineInfo
    var width = 0f
    var labels = 0f

    private val sz: GridSizes get() = grid.sizes

    fun edges(): GridEdges = GridEdges(tool, grid, rows)

    /** The row (top first) and frame at [p] of all the rows; none off them. */
    fun cell(p: Offset): Pair<Int, Int>? {
        val f = floor((p.x - place.origin) / sz.pitch).toInt()
        val r = rows.index(p.y) ?: return null
        if (f < 0 || f > place.last) return null
        return r to f
    }

    /** The clip at [p] of all the rows, with its row's track (on a row of groups' tracks, only the frames a group lasts are anyone's). */
    fun clipAt(p: Offset): Pair<TimelineTrackInfo, TimelineClipInfo>? {
        val (r, f) = cell(p) ?: return null
        if (!rows[r].has(f)) return null
        val t = rows[r].track(f)
        val at = f.toUInt()
        val c = t.clips.firstOrNull { it.start <= at && at < it.start + it.duration } ?: return null
        return t to c
    }

    /**
     * A tap at [at] (dp of the rows as they show): with a menu open, it closes; with the lasso on,
     * the frame there is picked with the others or let go; else the playhead goes to it on its
     * track, or, there already, the frame's handles come out or go. A group's chevron folds or
     * unfolds it.
     */
    fun tapped(at: Offset) {
        val p = place.inRows(at)
        if (grid.menuOpen(tool)) {
            grid.closeMenus(tool)
            return
        }
        val cell = cell(p)
        if (cell == null || !rows[cell.first].has(cell.second)) {
            tool.picked = null
            return
        }
        val (r, f) = cell
        val t = rows[r].track(f)
        val found = clipAt(p)?.second
        if (tool.selecting == null && found != null && found.kind == TimelineClipKind.GROUP && p.x - (place.origin + found.start.toFloat() * sz.pitch) < 30f) {
            // its chevron: its own tracks' rows shown, or put away
            tool.fold(found.id, tool.unfolded(found.id))
            return
        }
        tool.selecting?.let { chosen ->
            if (found != null) {
                tool.selecting = if (found.id in chosen) chosen - found.id else chosen + found.id
                grid.haptic(GridHaptic.TICK)
            }
            return
        }
        if (tool.isPlaying) {
            tool.stopPlaying()
            return
        }
        if (t.id == tool.info?.track && f.toUInt() == tool.frame) {
            if (found == null || tool.held(t)) {
                tool.picked = null
                return
            }
            tool.picked = if (tool.picked == found.id) null else found.id
            grid.haptic(GridHaptic.TICK)
        } else {
            tool.picked = null
            tool.selectTrack(t.id)
            if (f.toUInt() != tool.frame) tool.goTo(f.toUInt())
        }
    }

    /** A finger held still at [at] (dp of the rows as they show) on a frame: its menu, standing on the bar over it; with the lasso on, the picked frames' menu when it is one of them. An empty frame has its own. */
    fun held(at: Offset) {
        if (tool.isPlaying) return
        val p = place.inRows(at)
        grid.heldClip = null
        val found = clipAt(p)
        if (found == null) {
            val cell = cell(p) ?: return
            if (tool.selecting == null && rows[cell.first].has(cell.second)) {
                grid.closeMenus(tool)
                grid.emptyMenu = GridEmptySpot(rows[cell.first].track(cell.second).id, cell.second.toUInt(), at.x)
                grid.haptic(GridHaptic.HOLD)
            }
            return
        }
        val c = found.second
        grid.heldClip = c.id
        val chosen = tool.selecting
        if (chosen != null) {
            if (c.id !in chosen) return
            grid.closeMenus(tool)
            grid.pickedMenu = at.x
        } else {
            grid.closeMenus(tool)
            grid.clipMenu = GridClipMenu(c.id, at.x)
        }
        grid.haptic(GridHaptic.HOLD)
    }

    // ---- The lasso's stroke ----

    /** A stroke begins at [start] and has come to [here] (dp of the rows as they show). */
    fun strokeBegan(start: Offset, here: Offset) {
        grid.closeMenus(tool)
        grid.faded = emptyList()
        val p = place.inRows(start)
        grid.stroke.clear()
        grid.stroke.add(p)
        grid.touched.clear()
        grid.strokeBefore = tool.selecting
        take(p)
        stroked(here)
    }

    /** The stroke on to [here]: every frame between the last point and this one is picked or, picked already, let go (each once a stroke). */
    fun stroked(here: Offset) {
        val point = place.inRows(here)
        val last = grid.stroke.lastOrNull() ?: point
        val far = hypot(point.x - last.x, point.y - last.y)
        val steps = maxOf((far / 4f).toInt(), 1)
        for (i in 1..steps) {
            val t = i.toFloat() / steps
            take(Offset(last.x + (point.x - last.x) * t, last.y + (point.y - last.y) * t))
        }
        grid.stroke.add(point)
    }

    private fun take(p: Offset) {
        val (t, c) = clipAt(p) ?: return
        if (tool.held(t) || !grid.touched.add(c.id)) return
        val chosen = tool.selecting ?: return
        tool.selecting = if (c.id in chosen) chosen - c.id else chosen + c.id
        grid.haptic(GridHaptic.TICK)
    }

    /** The stroke lifted: it fades away. */
    fun endStroke(scope: CoroutineScope) {
        grid.faded = grid.stroke.toList()
        grid.stroke.clear()
        grid.touched.clear()
        grid.strokeBefore = null
        grid.fadeAlpha = 1f
        scope.launch {
            animate(1f, 0f, animationSpec = tween(350)) { v, _ -> grid.fadeAlpha = v }
            if (grid.stroke.isEmpty()) grid.faded = emptyList()
        }
    }

    /** A second finger landed: the stroke under way and what it picked are dropped. */
    fun dropStroke() {
        grid.strokeBefore?.let { tool.selecting = it }
        grid.stroke.clear()
        grid.touched.clear()
        grid.strokeBefore = null
    }

    // ---- Moving blocks ----

    /** The finger that held a block has moved on: the block (with the others picked, the lasso on) is picked up. False if there is nothing to pick up. */
    fun moveBegan(): Boolean {
        val held = grid.heldClip ?: return false
        val (t, c) = tool.clip(held) ?: return false
        if (tool.held(t) || tool.isPlaying) return false
        var ids = listOf(c.id)
        tool.selecting?.let { chosen ->
            if (c.id !in chosen) return false
            ids = chosen.toList()
        }
        grid.closeMenus(tool)
        tool.picked = null
        grid.move = GridMove(c.id, ids, t.id, c.start.toInt(), tool.frame.toFloat())
        // (the frames stand where they are until it nears an end of the bar)
        grid.dragged = tool.frame.toFloat()
        grid.haptic(GridHaptic.HOLD)
        return true
    }

    /** The line between rows at height [y] of them, if it is near one: above the first row and below the last count. */
    private fun line(y: Float, frame: Int): GridBetween? {
        val first = rows.list.firstOrNull() ?: return null
        val last = rows.list.last()
        val near = minOf(7f, first.height / 4f)
        if (y < first.y + near) return GridBetween(first.track(frame).id, false, first.y)
        if (y > last.y + last.height - near) return GridBetween(last.track(frame).id, true, last.y + last.height)
        // (the line over a row: a track made above that row's, among its own: inside a group,
        // where the row is a group's)
        return rows.list.drop(1).firstOrNull { abs(y - it.y) <= near }?.let { GridBetween(it.track(frame).id, false, it.y) }
    }

    /** The block dragged on to [here] (dp of the rows as they show), [dx] across from where the finger came down: where it would land now, and the tracks as that would leave them. */
    fun moved(here: Offset, dx: Float, scope: CoroutineScope) {
        var m = grid.move ?: return
        grid.moveAgain = { moved(here, dx, scope) }
        slide(here.x, scope)
        val p = place.inRows(here)
        // the frames slid under it count as far as it has gone
        val slid = (grid.dragged ?: m.scrollFrom) - m.scrollFrom
        val by = maxOf((dx / sz.pitch + slid).roundToInt(), -m.start)
        val under = floor((p.x - place.origin) / sz.pitch).toInt()
        val between = if (m.ids.size == 1) line(p.y, under) else null
        val over = if (between != null) null else rows.index(p.y.coerceIn(0f, maxOf(rows.height - 1f, 0f)))?.let { rows[it].track(under).id }
        if (by == m.by && between == m.between && over == m.over && !(m.shown.isEmpty() && !m.valid)) return
        val shown = HashMap<ULong, List<TimelineClipInfo>>()
        var valid = false
        val start = m.start + by
        if (between != null) {
            valid = true
        } else if (over != null && m.ids.size == 1) {
            tool.placePreview(m.clip, over, start, by >= 0)?.let { landed ->
                shown[over] = landed.clips
                // (off its own track, which shows without it)
                if (over != m.track) tool.tracks.firstOrNull { it.id == m.track }?.let { from -> shown[m.track] = from.clips.filter { it.id != m.clip } }
                valid = true
            }
        } else if (over != null) {
            for (t in tool.placeManyPreview(m.ids, m.clip, over, by, by >= 0)) {
                shown[t.track] = t.clips
                valid = true
            }
        }
        m = m.copy(by = by, over = over, between = between, shown = shown, valid = valid)
        grid.haptic(GridHaptic.TICK)
        grid.move = m
    }

    /** Near either end of the bar, the frames slide under the dragged block for as long as it is held there. */
    private fun slide(x: Float, scope: CoroutineScope) {
        val zone = 44f
        grid.moveSlide = if (x < labels + zone) -1f else if (x > width - zone) 1f else 0f
        if (grid.moveSlide == 0f || grid.moveTask != null) return
        grid.moveTask = scope.launch {
            while (isActive && grid.move != null && grid.moveSlide != 0f) {
                val at = (grid.dragged ?: 0f) + grid.moveSlide * 0.22f
                grid.dragged = at.coerceIn(0f, place.last.toFloat())
                grid.moveAgain?.invoke()
                delay(33)
            }
            grid.moveTask = null
        }
    }

    private fun stopMoving(): GridMove? {
        val m = grid.move ?: return null
        grid.move = null
        grid.moveAgain = null
        grid.moveSlide = 0f
        grid.moveTask?.cancel()
        grid.moveTask = null
        return m
    }

    /** The block let go: it stays where it showed, as one undo step; the frames slid under it stay slid. */
    fun moveEnded() {
        val m = stopMoving() ?: return
        val slid = grid.dragged?.roundToInt()?.toUInt()
        grid.dragged = null
        if (m.valid) {
            val line = m.between
            val over = m.over
            if (line != null) {
                tool.dropOnNewTrack(m.clip, line.beside, line.below, m.start + m.by)
            } else if (over != null && (over != m.track || m.by != 0)) {
                if (m.ids.size == 1) tool.drop(m.clip, over, m.start + m.by, m.by >= 0) else tool.drop(m.ids, m.clip, over, m.by, m.by >= 0)
            }
        } else {
            grid.haptic(GridHaptic.WARN)
        }
        if (slid != null && slid != tool.frame) tool.goTo(slid)
    }

    /** The drag taken away: the block stays where it was. */
    fun moveCancelled() {
        stopMoving() ?: return
        grid.dragged = null
    }
}

/**
 * The rows' one touch handler: told apart as it goes into a tap, a hold (a menu), a drag sideways
 * (the frames slide, scrubbing, and settle on a frame), a drag up or down (more tracks than show),
 * a lasso stroke, a handle dragged, or a block held and dragged on. Over the tracks' names, the
 * touch is theirs.
 */
internal fun Modifier.gridRowTouches(input: GridRowsInput, settle: CoroutineScope): Modifier = pointerInput(input) {
    val d = density
    val tool = input.tool
    val grid = input.grid
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false)
        val start = down.position / d
        if (start.x < input.labels - 12f) return@awaitEachGesture
        down.consume()
        // a touch on the frames while they play stops them there
        if (tool.isPlaying) {
            tool.stopPlaying(stay = true)
            return@awaitEachGesture
        }
        val p0 = input.place
        val inRows = p0.inRows(start)
        // down on a handle: its edge follows the finger
        val handle = input.edges().handle(Offset(inRows.x - p0.origin, inRows.y))
        var touch = if (handle != null) GridTouch.HANDLE else GridTouch.UNDECIDED
        grid.scrollFrom = grid.dragged ?: tool.frame.toFloat()
        val panFrom = p0.scroll
        val velocity = VelocityTracker()
        velocity.addPosition(down.uptimeMillis, down.position)
        var lastChange: PointerInputChange = down
        var lifted = false
        val downAt = System.currentTimeMillis()
        try {
            while (true) {
                val event: PointerEvent
                if (touch == GridTouch.UNDECIDED) {
                    // still, it is held: its menu (the lift that ends the hold is no tap)
                    val left = HOLD_MS - (System.currentTimeMillis() - downAt)
                    val next = if (left > 0) withTimeoutOrNull(left) { awaitPointerEvent() } else null
                    if (next == null) {
                        touch = GridTouch.HELD
                        input.held(start)
                        continue
                    }
                    event = next
                } else {
                    event = awaitPointerEvent()
                }
                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                lastChange = change
                if (!change.pressed) {
                    lifted = true
                    break
                }
                // a second finger landing drops the stroke under way, and what it picked
                if (touch == GridTouch.STROKE && event.changes.count { it.pressed } > 1) {
                    input.dropStroke()
                    touch = GridTouch.IGNORE
                }
                velocity.addPosition(change.uptimeMillis, change.position)
                val moved = (change.position - down.position) / d
                val here = change.position / d
                when (touch) {
                    GridTouch.UNDECIDED -> if (hypot(moved.x, moved.y) > STILL) {
                        if (tool.selecting != null) {
                            touch = GridTouch.STROKE
                            input.strokeBegan(start, here)
                        } else if (abs(moved.x) >= abs(moved.y) || p0.over <= 0f) {
                            touch = GridTouch.SCROLL
                            grid.closeMenus(tool)
                            grid.settling?.cancel()
                            grid.scrolling = true
                            grid.scrolled(moved.x, p0.last, tool)
                        } else {
                            touch = GridTouch.PAN
                            grid.closeMenus(tool)
                            grid.rowsOffset = (panFrom - moved.y).coerceIn(0f, p0.over)
                        }
                    }
                    GridTouch.SCROLL -> grid.scrolled(moved.x, p0.last, tool)
                    GridTouch.PAN -> grid.rowsOffset = (panFrom - moved.y).coerceIn(0f, p0.over)
                    GridTouch.STROKE -> input.stroked(here)
                    // (a tap on a handle does nothing)
                    GridTouch.HANDLE -> if (grid.trim != null || grid.pickedTrim != null || abs(moved.x) > 1f) input.edges().dragged(handle == true, moved.x)
                    // dragged on from the hold: the block held is picked up, and its menu goes
                    GridTouch.HELD -> if (hypot(moved.x, moved.y) > 8f && input.moveBegan()) {
                        touch = GridTouch.MOVE
                        input.moved(here, moved.x, settle)
                    }
                    GridTouch.MOVE -> input.moved(here, moved.x, settle)
                    GridTouch.IGNORE -> {}
                }
                change.consume()
            }
        } finally {
            // (taken away part way, a drag settles where it is, and an edge or a block stays as it was)
            when (touch) {
                GridTouch.UNDECIDED -> if (lifted) input.tapped(start)
                GridTouch.SCROLL -> {
                    grid.scrolling = false
                    val vx = if (lifted) velocity.calculateVelocity().x / d else 0f
                    val from = grid.dragged
                    if (from != null) {
                        val to = exponentialDecay<Float>().calculateTargetValue(from, -vx / grid.sizes.pitch).roundToInt().coerceIn(0, p0.last)
                        tool.endScrub(to.toUInt())
                        grid.settling = settle.launch {
                            animate(from, to.toFloat(), animationSpec = Motion.frameSlide()) { v, _ -> grid.dragged = v }
                            if (!grid.scrolling) grid.dragged = null
                        }
                    }
                }
                GridTouch.STROKE -> input.endStroke(settle)
                GridTouch.HANDLE -> if (lifted) input.edges().ended() else input.edges().cancel()
                GridTouch.MOVE -> if (lifted) input.moveEnded() else input.moveCancelled()
                else -> {}
            }
            grid.heldClip = null
            if (lifted) lastChange.consume()
        }
    }
}

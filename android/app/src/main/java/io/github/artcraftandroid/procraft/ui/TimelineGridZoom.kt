package io.github.artcraftandroid.procraft.ui

import android.content.Context
import android.content.SharedPreferences
import androidx.compose.animation.core.LinearEasing
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.tween
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.input.pointer.PointerEventPass
import androidx.compose.ui.input.pointer.PointerId
import androidx.compose.ui.input.pointer.pointerInput
import io.github.artcraftandroid.procraft.canvas.TimelineTool
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import kotlin.math.abs
import kotlin.math.exp
import kotlin.math.hypot
import kotlin.math.ln
import kotlin.math.roundToInt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.launch

// The grid timeline's zoom (see TimelineGrid.kt), as the iPad's TimelineGridZoom.swift has it:
// two fingers on the frames pinch them wider or narrower about the playhead, and carry them and
// the rows along as they move together. A pinch shut fast as the fingers lift goes out to where
// the whole timeline shows; one flung open goes back to the size the grid starts at. Whatever one
// finger was doing when the second came down (a stroke of the lasso, a drag, a hold) is dropped.
// A group tapped three times fills the grid, and it alone plays.

/**
 * How far the grid is zoomed (1: as it starts, a frame 44 dp wide), kept from one canvas to the
 * next; the pinch under way; and the group the grid was fitted to.
 */
@Stable
internal class GridZoom {
    var value by mutableFloatStateOf(1f)

    /** Two fingers are on the frames: whether they have moved together far enough to carry the grid along, and how far they had by then; the zoom and the rows' scroll as they came down. */
    var pinching = false
    var panning = false
    var panShift = Offset.Zero
    var zoomFrom = 1f
    var panFrom = 0f

    /** What eases the zoom somewhere, and what it runs in (the bar's). */
    var task: Job? = null
    var scope: CoroutineScope? = null

    /** The frames of the group the grid was fitted to: playing goes round them alone, until the grid is slid or zoomed again. */
    var playOnly: UIntRange? = null

    /** The taps on a group in a row, for the third. */
    private var tappedClip: ULong? = null
    private var taps = 0
    private var tappedAt = 0L

    private var prefs: SharedPreferences? = null

    /** The zoom kept from the last canvas, and kept from here on; the bar's [scope] for easing it. */
    fun attach(context: Context, scope: CoroutineScope) {
        this.scope = scope
        val p = context.applicationContext.getSharedPreferences("timeline", Context.MODE_PRIVATE)
        prefs = p
        val kept = p.getFloat(KEPT, 0f)
        if (kept > 0f) value = kept.coerceIn(GridSizes.LEAST_PITCH / GridSizes.PITCH, GridSizes.MOST)
    }

    /** The zoom is kept for the next canvas. */
    fun keep() {
        prefs?.edit()?.putFloat(KEPT, value)?.apply()
    }

    /** Group [clip] tapped at [now] (ms): whether that is the third tap on it in a row. */
    fun thirdTap(clip: ULong, now: Long): Boolean {
        val again = tappedClip == clip && now - tappedAt < 450L
        tappedClip = clip
        taps = if (again) taps + 1 else 1
        tappedAt = now
        if (taps < 3) return false
        taps = 0
        return true
    }

    companion object {
        private const val KEPT = "timelineGridZoom"

        /** How fast a pinch must be closing as the fingers lift to count as flicked shut, in parts of the spread it started at a second: the canvas's own (`QUICK_PINCH_SPEED`, touch.rs). And how fast one must be opening, in parts of the spread it has by then. */
        const val QUICK_PINCH = -2.5f
        const val QUICK_SPREAD = 3f

        /** How long the zoom takes to ease somewhere (ms). */
        const val EASE_MS = 340

        /**
         * The zoom at which all [frames] of the timeline show on either side of the playhead, with
         * [room] dp there: never further in than the grid starts. A pinch goes out as far as that,
         * or as far as the rows get smaller where the timeline fits sooner.
         */
        fun least(frames: Int, room: Float): Float {
            val fit = maxOf(room, 1f) / maxOf(frames, 1)
            return minOf(maxOf(fit, GridSizes.LEAST_PITCH) / GridSizes.PITCH, 1f)
        }

        /** The zoom of fingers [scale] times as far apart as they came down at zoom [from]: no further out than [least] (or than the rows get smaller), no further in than the grid's most. */
        fun pinched(from: Float, scale: Float, least: Float): Float =
            (from * scale).coerceIn(minOf(least, GridSizes.LEAST_ROWS), GridSizes.MOST)

        /**
         * Where the zoom goes as the fingers lift, the pinch [speed] fast (closing: below zero) and
         * [opening] (in parts of its spread then): flicked shut, out to [least], where the whole
         * timeline shows; flung open while it is smaller than it starts, back to 1. None: it
         * stays.
         */
        fun landing(speed: Float, opening: Float, zoom: Float, least: Float): Float? = when {
            speed < QUICK_PINCH -> least
            opening > QUICK_SPREAD && zoom < 1f -> 1f
            else -> null
        }

        /** The zoom [t] (0 to 1) of the way from [from] to [to], easing in and out, evenly as it reads (by ratio, not by dp). */
        fun eased(from: Float, to: Float, t: Float): Float {
            val s = t.coerceIn(0f, 1f)
            val e = s * s * (3f - 2f * s)
            return exp(ln(from) + (ln(to) - ln(from)) * e)
        }

        /** The zoom at which a group [frames] long fills the [room] dp beside the playhead on both sides, its middle frame under it. */
        fun fit(frames: Int, room: Float): Float =
            (maxOf(room, 44f) / (maxOf(frames, 1) / 2f) / GridSizes.PITCH).coerceIn(GridSizes.LEAST_PITCH / GridSizes.PITCH, GridSizes.MOST)
    }
}

/** The pinch's scale at a moment (ms, as the touches tell it). */
internal data class PinchSample(val at: Long, val scale: Float)

/**
 * How fast a pinch was changing as a finger lifted at [now], over its last 40 ms (nothing heard
 * meanwhile is fingers held still): in parts of its first spread a second (closing: below zero),
 * and in parts of the spread it had then.
 */
internal fun pinchSpeed(samples: List<PinchSample>, now: Long): Pair<Float, Float> {
    val last = samples.lastOrNull() ?: PinchSample(now, 1f)
    val before = samples.lastOrNull { it.at <= now - 40L } ?: samples.firstOrNull() ?: last
    val seconds = (now - before.at) / 1000f
    val speed = if (seconds > 0.0005f) (last.scale - before.scale) / seconds else 0f
    return speed to speed / maxOf(last.scale, 0.05f)
}

/**
 * How the ruler's numbers stand at [pitch] dp a frame and [fps]: how many frames from one to the
 * next, so that each has 26 dp (every frame's where they fit); whether they are seconds instead
 * (numbers rarer than four a second say less than the seconds do), how many seconds from one
 * written to the next (44 dp apart), and how many frames from one tick to the next (a second's,
 * or half where there is room).
 */
internal data class RulerSteps(val every: Int, val seconds: Boolean, val apart: Int, val tick: Int) {
    companion object {
        fun at(pitch: Float, fps: Int): RulerSteps {
            val rate = maxOf(fps, 1)
            val every = listOf(1, 2, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000).firstOrNull { it * pitch >= 26f } ?: 10000
            val apart = listOf(1, 2, 5, 10, 15, 30, 60, 120, 300, 600).firstOrNull { it * rate * pitch >= 44f } ?: 1200
            val tick = if (rate % 2 == 0 && rate * pitch >= 28f) rate / 2 else rate
            return RulerSteps(every, every >= 5 && every * 4 >= rate, apart, tick)
        }
    }
}

/**
 * A second finger came down on the frames: the two of them zoom and move the grid from here, and
 * what the first was doing alone is dropped (a stroke's picks as they were, an edge or a block or
 * keys where they were).
 */
internal fun GridState.pinchBegan(tool: TimelineTool) {
    val z = zoom
    z.pinching = true
    z.panning = false
    z.playOnly = null
    z.task?.cancel()
    settling?.cancel()
    scrolling = false
    if (move != null) {
        move = null
        moveAgain = null
        moveSlide = 0f
        moveTask?.cancel()
        moveTask = null
        dragged = null
    }
    z.zoomFrom = z.value
    scrollFrom = dragged ?: tool.frame.toFloat()
    z.panFrom = rowsOffset
    // a stroke begun by the first finger: what it picked is as it was
    strokeBefore?.let { tool.selecting = it }
    strokeBefore = null
    stroke.clear()
    touched.clear()
    faded = emptyList()
    trim = null
    pickedTrim = null
    if (keys.heldKey != null || keys.stretching != null) {
        tool.lanes.cancelDrag()
        keys.heldKey = null
        keys.stretching = null
    }
    keys.menu = null
    heldClip = null
    closeMenus(tool)
}

/**
 * The two fingers [scale] times as far apart as they came down, and moved [shift] (dp) together:
 * the grid is that much wider (no further out than [least]), and the frames (up to [last]) and the
 * rows (scrolled up to [over]) go along with them.
 */
internal fun GridState.pinchChanged(scale: Float, shift: Offset, least: Float, last: Int, over: Float, tool: TimelineTool) {
    val z = zoom
    if (!z.pinching) return
    val next = GridZoom.pinched(z.zoomFrom, scale, least)
    if (abs(next - z.value) > 0.0005f) z.value = next
    // (a pinch's fingers never close quite evenly: a little drift moves nothing)
    if (!z.panning && hypot(shift.x, shift.y) > 14f) {
        z.panning = true
        z.panShift = shift
    }
    if (z.panning) {
        scrolled(shift.x - z.panShift.x, last, tool)
        rowsOffset = (z.panFrom - (shift.y - z.panShift.y)).coerceIn(0f, over)
    }
}

/**
 * The fingers lifted, the pinch [speed] fast as they did, [opening] (see [pinchSpeed]): the frames
 * slid settle on a frame, and the zoom eases to where [GridZoom.landing] says, or stays.
 */
internal fun GridState.pinchEnded(speed: Float, opening: Float, least: Float, last: Int, tool: TimelineTool) {
    val z = zoom
    if (!z.pinching) return
    z.pinching = false
    dragged?.let { from ->
        val to = from.roundToInt().coerceIn(0, last)
        tool.endScrub(to.toUInt())
        settling = z.scope?.launch {
            animate(from, to.toFloat(), animationSpec = Motion.frameSlide()) { v, _ -> dragged = v }
            if (!scrolling) dragged = null
        }
        if (settling == null) dragged = null
    }
    val target = GridZoom.landing(speed, opening, z.value, least)
    if (target != null) easeZoom(target) else z.keep()
}

/** The zoom goes to [target] over a moment, evenly as it reads. */
internal fun GridState.easeZoom(target: Float) {
    val z = zoom
    z.task?.cancel()
    val from = z.value
    val scope = z.scope
    if (abs(from - target) <= 0.001f || scope == null) {
        z.value = target
        z.keep()
        return
    }
    z.task = scope.launch {
        animate(0f, 1f, animationSpec = tween(GridZoom.EASE_MS, easing = LinearEasing)) { t, _ -> z.value = GridZoom.eased(from, target, t) }
        z.keep()
    }
}

/**
 * Group [c] tapped: the third tap in a row zooms the grid so that it fills it, its middle frame
 * under the playhead ([room] dp on either side clear of the tracks' names), and playing goes round
 * its frames alone. Whether it was the third.
 */
internal fun GridState.groupTapped(c: TimelineClipInfo, room: Float, tool: TimelineTool): Boolean {
    if (!zoom.thirdTap(c.id, System.currentTimeMillis())) return false
    tool.picked = null
    tool.goTo(c.start + c.duration / 2u)
    easeZoom(GridZoom.fit(c.duration.toInt(), room))
    zoom.playOnly = c.start until c.start + c.duration
    haptic(GridHaptic.LIGHT)
    return true
}

/**
 * Tells of two fingers on what it is on, beside whatever else takes them (it consumes nothing, and
 * hears them before the touches under it do): when the second comes down ([began]), how far apart
 * they are since, as a part of how far they were, and how far (dp) they have moved together
 * ([changed]), and, as one lifts, how fast they were closing or opening ([ended], see
 * [pinchSpeed]). The finger left is no pinch, and no new one starts with it.
 */
internal fun Modifier.gridPinch(
    began: () -> Unit,
    changed: (scale: Float, shift: Offset) -> Unit,
    ended: (speed: Float, opening: Float) -> Unit,
): Modifier = pointerInput(Unit) {
    awaitEachGesture {
        val first = awaitFirstDown(requireUnconsumed = false, pass = PointerEventPass.Initial)
        var second: PointerId? = null
        var spread = 1f
        var centre = Offset.Zero
        val samples = ArrayList<PinchSample>()
        try {
            while (true) {
                val e = awaitPointerEvent(PointerEventPass.Initial)
                val a = e.changes.firstOrNull { it.id == first.id }
                if (second == null) {
                    if (a == null || !a.pressed) break
                    val b = e.changes.firstOrNull { it.id != first.id && it.pressed && !it.previousPressed } ?: continue
                    second = b.id
                    spread = maxOf((a.position - b.position).getDistance() / density, 1f)
                    centre = (a.position + b.position) / (2f * density)
                    samples.add(PinchSample(b.uptimeMillis, 1f))
                    began()
                    continue
                }
                val b = e.changes.firstOrNull { it.id == second }
                if (a == null || b == null || !a.pressed || !b.pressed) {
                    val now = listOfNotNull(a, b).firstOrNull { !it.pressed }?.uptimeMillis ?: samples.last().at
                    val (speed, opening) = pinchSpeed(samples, now)
                    second = null
                    ended(speed, opening)
                    break
                }
                val scale = maxOf((a.position - b.position).getDistance() / density, 1f) / spread
                samples.add(PinchSample(maxOf(a.uptimeMillis, b.uptimeMillis), scale))
                if (samples.size > 120) samples.subList(0, 60).clear()
                changed(scale, (a.position + b.position) / (2f * density) - centre)
            }
        } finally {
            // (taken away part way, it ends where it is)
            if (second != null) ended(0f, 0f)
        }
    }
}

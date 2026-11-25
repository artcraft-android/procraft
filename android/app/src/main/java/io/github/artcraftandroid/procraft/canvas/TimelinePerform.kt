package io.github.artcraftandroid.procraft.canvas

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.lifecycle.viewModelScope
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelinePose
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch

// Perform, as the iPad's GridPerformOverlay (TimelineGridKeys.swift): with Animate on, the header's
// record button arms a take, and the next touch on the canvas starts it. The frames play on one at
// a time while the finger is down, the block under the playhead on the chosen track going where the
// finger takes it (a second finger makes it bigger or smaller and turns it) and keyed there on each;
// lifted (or at the block's last frame), the take ends, evened out by as much as Settings' Perform
// smoothing asks and thinned to the keys that keep its path, as one "Perform" undo step.

/** A take under way, and what plays its frames. */
class TimelinePerform internal constructor(private val vm: CanvasViewModel) {
    private val tool: TimelineTool get() = vm.timeline

    /** The block, how it was as the finger came down, the frames it has, and how the fingers have changed it since. */
    private class Take(val clip: ULong, val pose: TimelinePose, val first: UInt, val end: UInt) {
        var last: UInt = first

        /** How far the finger has gone, canvas pixels. */
        var moved: Offset = Offset.Zero

        /** With a second finger down at some time: how many times as big, and turned how far (degrees clockwise). */
        var scale: Float? = null
        var turn: Float? = null
    }

    private var take: Take? = null
    private var playing: Job? = null

    /** A take is under way (the overlay says so). */
    var going by mutableStateOf(false)
        private set

    /** The first finger down on the canvas: the take starts. False, with a notice, where there is nothing at the playhead to move. */
    fun begin(): Boolean {
        if (take != null) return true
        val s = vm.session ?: return false
        val clip = tool.clipHere
        val pose = clip?.takeIf { it.kind != TimelineClipKind.AUDIO }?.let { tool.pose(it.id) }
        if (clip == null || pose == null) {
            vm.showNotice("Put the playhead on the frame to move first.", warning = true)
            tool.performArmed = false
            return false
        }
        tool.stopPlaying()
        tool.performing = true
        val first = tool.frame
        val made = Take(clip.id, pose, first, clip.start + clip.duration - 1u)
        take = made
        going = true
        val fps = maxOf(tool.info?.fps?.toLong() ?: 12L, 1L)
        playing = vm.viewModelScope.launch {
            var at = first
            while (isActive) {
                val now = take ?: return@launch
                s.performTimelinePose(
                    now.clip, at,
                    now.pose.x + now.moved.x, now.pose.y + now.moved.y,
                    now.scale?.let { now.pose.scaleX * it }, now.turn?.let { now.pose.rotation + it },
                )
                now.last = at
                tool.scrub(at)
                delay(1000L / fps)
                if (at >= now.end) {
                    end()
                    return@launch
                }
                at++
            }
        }
        return true
    }

    /** The fingers moved: the first [moved] canvas pixels from where it came down; with a second down, [scale] times as far apart and turned [turn] degrees. */
    fun changed(moved: Offset, scale: Float?, turn: Float?) {
        val t = take ?: return
        t.moved = moved
        if (scale != null && turn != null) {
            t.scale = scale
            t.turn = turn
        }
    }

    /** The first finger lifted (or the block's last frame reached): the take ends as one undo step, its keys' rows put out to see what was made. */
    fun end() {
        val done = take ?: return
        take = null
        going = false
        playing?.cancel()
        playing = null
        val s = vm.session
        if (s != null) {
            // within a five-hundredth of the canvas's longer side of where it was dragged, and as
            // much evened out as the settings ask
            val size = s.info()
            val tolerance = maxOf(size.width, size.height).toFloat() / 500f
            s.performTimelineEnd(done.clip, done.first, done.last, tolerance, vm.timelinePerformSmoothing)
        }
        tool.performing = false
        tool.performArmed = false
        tool.endScrub(done.last)
        tool.clip(done.clip)?.first?.id?.let { tool.gridKeyTracks = tool.gridKeyTracks + it }
    }
}

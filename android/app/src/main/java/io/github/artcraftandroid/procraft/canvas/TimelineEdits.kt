package io.github.artcraftandroid.procraft.canvas

import android.net.Uri
import androidx.lifecycle.viewModelScope
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelineGap
import io.github.artcraftandroid.procraft.core.TimelineMask
import io.github.artcraftandroid.procraft.core.TimelinePlacement
import io.github.artcraftandroid.procraft.core.TimelineTrackClips
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// What the grid's frame editing changes on the timeline, as the iPad's `TimelineTool` has it
// (TimelineGrid.swift, TimelineMenus.swift, TimelinePicked.swift, TimelineGroupRows.swift,
// TimelineGridMove.swift, TimelineGridAdd.swift): lengthening a frame by its handles, the frames
// picked together, the menus' actions, moving blocks, groups, empty runs, a picture brought in.
// Each is one undo step in the core; the panel is read again after it.

private fun TimelineTool.done() = vm.refresh()

private fun TimelineTool.warn(text: String) = vm.showNotice(text, warning = true)

private val TimelineTool.trackLocked: Boolean get() = track?.locked == true

// ---- Lasting longer: a frame's handles ----

/**
 * Its track's clips as clip [id]'s start ([leading]) or end dragged to [frame] would leave them;
 * empty if it would change nothing. An end made later takes up the empty frames after it first
 * and pushes only the frames it reaches; made earlier, nothing else moves. A start pushes the
 * frames before it into the empty frames before them, and, asked earlier than they have room for
 * ([frame] may be before the first), the clip lasts that much longer at its end instead.
 */
fun TimelineTool.trimPreview(id: ULong, leading: Boolean, frame: Int): List<TimelineClipInfo> =
    session?.timelineLengthenPreview(id, leading, frame) ?: emptyList()

/** That edge dragged there: one undo step. */
fun TimelineTool.trim(id: ULong, leading: Boolean, frame: Int) {
    stopPlaying()
    session?.lengthenTimelineClip(id, leading, frame)
    done()
}

/** The tracks every picked frame's same edge dragged [by] frames would change, with their clips then. */
fun TimelineTool.trimsPreview(leading: Boolean, by: Int): Map<ULong, List<TimelineClipInfo>> =
    session?.timelineLengthensPreview(selecting.orEmpty().toList(), leading, by).orEmpty().associate { it.track to it.clips }

/** Every picked frame's same edge dragged [by] frames: one undo step. */
fun TimelineTool.trimPicked(leading: Boolean, by: Int) {
    if (by == 0) return
    stopPlaying()
    session?.lengthenTimelineClips(selecting.orEmpty().toList(), leading, by)
    done()
}

// ---- The frames picked together ----

/** The lasso on, the frame with handles out picked first; or off. */
fun TimelineTool.setLasso(on: Boolean) {
    if (on) {
        stopPlaying()
        selecting = picked?.let { setOf(it) } ?: emptySet()
        vm.closeTimelineSettings()
    } else {
        selecting = null
    }
}

/** Frames [ids] copied with their keys, to paste at the playhead; what is picked stays picked. */
fun TimelineTool.copyFrames(ids: List<ULong>) {
    if (ids.isEmpty()) return
    if (session?.copyTimelineClips(ids) == true) {
        vm.showNotice(
            if (ids.size == 1) "Copied. Paste puts it at the playhead on the chosen track."
            else "${ids.size} frames copied. Paste puts them at the playhead, from the chosen track up.",
        )
    }
    done()
}

/** Frames [ids] copied, then taken off with their drawings: one undo step. */
fun TimelineTool.cutFrames(ids: List<ULong>) {
    if (ids.isEmpty()) return
    stopPlaying()
    picked = null
    if (session?.cutTimelineClips(ids) != true) warn("One of them is on a locked track.")
    done()
}

/** Frames [ids] taken off with their drawings: one undo step. */
fun TimelineTool.deleteFrames(ids: List<ULong>) {
    if (ids.isEmpty()) return
    stopPlaying()
    picked = null
    if (session?.deleteTimelineClips(ids) != true) warn("One of them is on a locked track.")
    done()
}

/** Copies of frames [ids] just after the last of them: one undo step. The copies. */
fun TimelineTool.duplicateFrames(ids: List<ULong>): List<ULong> {
    if (ids.isEmpty()) return emptyList()
    stopPlaying()
    val made = session?.duplicateTimelineClips(ids).orEmpty()
    if (made.isEmpty()) warn("Only frames on tracks that aren't locked, with room for their layers, can be duplicated.")
    done()
    return made
}

/** The clips copied, at the playhead on the track drawn on (those above on the tracks above); the first pasted is picked. */
fun TimelineTool.pasteClips() {
    stopPlaying()
    val made = session?.pasteTimelineClips().orEmpty()
    if (made.isEmpty()) warn(if (trackLocked) "The track is locked." else "There's no room for more layers.")
    else picked = made.first()
    done()
}

/** The drawings of frames [ids] made blank, the frames staying where they are: one undo step. */
fun TimelineTool.clearFrames(ids: List<ULong>) {
    if (ids.isEmpty()) return
    stopPlaying()
    if (session?.clearTimelineClips(ids) != true) warn("Only drawings on tracks that aren't locked can be cleared.")
    done()
}

/** Frames [ids] put in the opposite order in time: one undo step. Whether they were. */
fun TimelineTool.flipFrames(ids: List<ULong>): Boolean {
    if (ids.size < 2) return false
    stopPlaying()
    val flipped = session?.reverseTimelineClips(ids) == true
    if (!flipped) warn("They can't be flipped: a frame between them isn't picked, or a track is locked.")
    done()
    return flipped
}

/** Whether any of clips [ids] has empty frames after it to last through (and none is on a locked track). */
fun TimelineTool.canExtend(ids: List<ULong>): Boolean = ids.isNotEmpty() && session?.timelineClipsExtend(ids) == true

/** Clips [ids] each made to last through the empty frames after it, up to the next clip or the timeline's end: one undo step. */
fun TimelineTool.extend(ids: List<ULong>) {
    if (ids.isEmpty()) return
    stopPlaying()
    if (session?.extendTimelineClips(ids) != true) {
        warn(
            if (ids.size == 1) "There's no empty space after this clip to fill, or its track is locked."
            else "There's no empty space after these clips to fill, or one is on a locked track.",
        )
    }
    done()
}

/** The clips picked together made into a group, which is picked. */
fun TimelineTool.makeGroup() {
    val ids = selecting.orEmpty().toList()
    selecting = null
    if (ids.isEmpty()) return
    val made = session?.groupTimelineClips(ids) ?: 0uL
    if (made == 0uL) warn("Only clips shown together can be grouped.") else picked = made
    done()
}

/** Whether every one of clips [ids] is turned off. */
fun TimelineTool.allDisabled(ids: List<ULong>): Boolean = ids.isNotEmpty() && ids.all { clip(it)?.second?.disabled == true }

/** Clips [ids] all turned off, or all on again: one undo step. */
fun TimelineTool.setClipsDisabled(ids: List<ULong>, disabled: Boolean) {
    if (session?.setTimelineClipsDisabled(ids, disabled) != true) warn("They are on locked tracks.")
    done()
}

// ---- A frame's and a block's menus ----

/** A new blank frame just after [clip] on [track], the frames after it moving along; the playhead goes onto it. */
fun TimelineTool.insertFrame(after: TimelineClipInfo, track: TimelineTrackInfo) {
    selectTrack(track.id)
    goTo(after.start)
    addDrawing()
}

/** What was copied put at [clip]'s first frame on [track]. */
fun TimelineTool.paste(at: TimelineClipInfo, track: TimelineTrackInfo) {
    selectTrack(track.id)
    goTo(at.start)
    pasteClips()
}

/** Clip [id] turned off (it stays, but neither shows nor plays) or on. */
fun TimelineTool.setClipDisabled(id: ULong, disabled: Boolean) {
    session?.setTimelineClipDisabled(id, disabled)
    done()
}

/** Clip [id]'s own opacity: shown at once, one undo step when [end]. */
fun TimelineTool.setClipOpacity(id: ULong, opacity: Float, end: Boolean) {
    session?.setTimelineClipOpacity(id, opacity, end)
    if (end) done()
}

fun TimelineTool.setClipBlend(id: ULong, blend: UInt) {
    session?.setTimelineClipBlend(id, blend)
    done()
}

fun TimelineTool.setClipMask(id: ULong, mask: TimelineMask) {
    session?.setTimelineClipMask(id, mask)
    done()
}

fun TimelineTool.setClipLabel(id: ULong, label: UInt?) {
    session?.setTimelineClipLabel(id, label)
    done()
}

fun TimelineTool.renameClip(id: ULong, name: String) {
    session?.renameTimelineClip(id, name)
    done()
}

/** Clip [id] cut in two at the playhead (a group with every clip inside the cut goes through). */
fun TimelineTool.split(id: ULong) {
    stopPlaying()
    if (session?.splitTimelineClip(id) != true) warn("Move the playhead inside the clip to split it there.")
    done()
}

/** Clip [id] taken out of the group it is in, onto the level the group is on. */
fun TimelineTool.moveOutOfGroup(id: ULong) {
    if (session?.moveTimelineClipOutOfGroup(id) != true) warn("It can't be moved out: its track is locked.")
    done()
}

/**
 * The Keys card opened for block [clip] (the keys' own card shows it): keys go on the playhead's
 * frame, so the playhead comes onto the block first if it isn't on one of its frames (its first,
 * or its last when the playhead is past it).
 */
fun TimelineTool.openKeys(clip: TimelineClipInfo) {
    if (clip.duration > 0u && (frame < clip.start || frame >= clip.start + clip.duration)) {
        goTo(if (frame < clip.start) clip.start else clip.start + clip.duration - 1u)
    }
    keysCardFor = clip.id
}

// ---- A track's menu ----

fun TimelineTool.deleteTrack(id: ULong) {
    session?.deleteTimelineTrack(id)
    done()
}

fun TimelineTool.toggleLocked(t: TimelineTrackInfo) {
    session?.setTimelineTrack(t.id, t.hidden, !t.locked)
    done()
}

/** Track [id]'s opacity: shown at once, one undo step when [end]. */
fun TimelineTool.setTrackOpacity(id: ULong, opacity: Float, end: Boolean) {
    session?.setTimelineTrackOpacity(id, opacity, end)
    if (end) done()
}

fun TimelineTool.renameTrack(id: ULong, name: String) {
    session?.renameTimelineTrack(id, name)
    done()
}

fun TimelineTool.setTrackLabel(id: ULong, label: UInt?) {
    session?.setTimelineTrackLabel(id, label)
    done()
}

fun TimelineTool.setTrackBlend(id: ULong, blend: UInt) {
    if (session?.setTimelineTrackBlend(id, blend) != true && tracks.firstOrNull { it.id == id }?.locked == true) warn("The track is locked.")
    done()
}

fun TimelineTool.setTrackMask(id: ULong, mask: TimelineMask) {
    if (session?.setTimelineTrackMask(id, mask) != true && tracks.firstOrNull { it.id == id }?.locked == true) warn("The track is locked.")
    done()
}

// ---- Groups ----

fun TimelineTool.ungroup(id: ULong) {
    picked = null
    session?.ungroupTimelineClip(id)
    done()
}

/** Group [id] plays its first [length] frames over and over (0: once). */
fun TimelineTool.setCycle(id: ULong, length: UInt, continueAcross: Boolean) {
    session?.setTimelineCycle(id, length, continueAcross)
    done()
}

/** How many times cycling group [id] plays its cycle, and the most it has room for. */
fun TimelineTool.cycles(id: ULong): Pair<Int, Int>? {
    val n = session?.timelineGroupCycles(id) ?: return null
    if (n.size != 2) return null
    return n[0].toInt() to n[1].toInt()
}

/** Cycling group [id] made [count] cycles long: one undo step. */
fun TimelineTool.setCycles(id: ULong, count: Int) {
    stopPlaying()
    session?.setTimelineGroupCycles(id, maxOf(count, 1).toUInt())
    done()
}

/** The most frames cycling group [id] may last on its track. */
fun TimelineTool.groupRoom(id: ULong): Int? = session?.timelineGroupRoom(id)?.toInt()

/** Cycling group [id] made [frames] long, its last cycle cut short where that falls inside one: one undo step. */
fun TimelineTool.setGroupLength(id: ULong, frames: Int) {
    stopPlaying()
    session?.setTimelineGroupLength(id, maxOf(frames, 1).toUInt())
    done()
}

/** The group clip whose track holds track [id] as one of its own, if any (0: none). */
fun TimelineTool.groupOf(id: ULong): ULong = place(id)?.group ?: 0uL

// ---- Empty frames ----

/** The run of empty frames [frame] is in on track [track]; none where a frame is there. */
fun TimelineTool.gap(track: ULong, frame: UInt): TimelineGap? = session?.timelineGap(track, frame)

/** That run made [length] frames long (0: closed), the frames after it moving to suit: one undo step. */
fun TimelineTool.setGap(track: ULong, frame: UInt, length: Int) {
    stopPlaying()
    session?.setTimelineGap(track, frame, maxOf(length, 0).toUInt())
    done()
}

/** A new blank frame on [frame] of track [track], the playhead going onto it. */
fun TimelineTool.insertFrame(at: UInt, track: ULong) {
    selectTrack(track)
    goTo(at)
    addDrawing()
}

/** What was copied put on [frame] of track [track]. */
fun TimelineTool.paste(atFrame: UInt, track: ULong) {
    selectTrack(track)
    goTo(atFrame)
    pasteClips()
}

// ---- Moving blocks ----

/** Where clip [id] would land put on [track] at [start], what it covers pushed aside; none if it can't go there. */
fun TimelineTool.placePreview(id: ULong, track: ULong, start: Int, later: Boolean): TimelinePlacement? =
    session?.timelinePlacePreview(id, track, maxOf(start, 0).toUInt(), later)

/** Clips [ids] put [by] frames along, on the tracks as many rows from their own as [track] is from [primary]'s, as they would land. */
fun TimelineTool.placeManyPreview(ids: List<ULong>, primary: ULong, track: ULong, by: Int, later: Boolean): List<TimelineTrackClips> =
    session?.timelinePlaceClipsPreview(ids, primary, track, by, later).orEmpty()

/** Block [id] dropped on [track] starting on [start], what it covers pushed aside: one undo step. */
fun TimelineTool.drop(id: ULong, track: ULong, start: Int, later: Boolean) {
    stopPlaying()
    session?.dropTimelineClip(id, track, maxOf(start, 0).toUInt(), later)
    done()
}

/** Blocks [ids] dropped [by] frames along, on the tracks as many rows from their own as [track] is from [primary]'s: one undo step. */
fun TimelineTool.drop(ids: List<ULong>, primary: ULong, track: ULong, by: Int, later: Boolean) {
    stopPlaying()
    session?.dropTimelineClips(ids, primary, track, by, later)
    done()
}

/** Block [id] dropped on a new track of its own just above (or [below]) track [beside]: one undo step. */
fun TimelineTool.dropOnNewTrack(id: ULong, beside: ULong, below: Boolean, start: Int) {
    stopPlaying()
    if ((session?.dropTimelineClipOnNewTrack(id, beside, below, maxOf(start, 0).toUInt()) ?: 0uL) == 0uL) warn("It can't go there.")
    done()
}

// ---- The timeline's length ----

/** The timeline [frames] long (what is past its end stays, shaded): one undo step. */
fun TimelineTool.setLength(frames: Int) {
    stopPlaying()
    vm.setTimelineLength(frames)
}

/** The timeline as long as its clips: one undo step. */
fun TimelineTool.fitToContent() {
    stopPlaying()
    vm.fitTimelineToContent()
}

/** [count] empty frames before the first (the clips moving along) or after the last: one undo step. */
fun TimelineTool.addFrames(atStart: Boolean, count: Int) {
    stopPlaying()
    session?.addTimelineFrames(atStart, maxOf(count, 1).toUInt())
    done()
}

// ---- Add new ----

/**
 * The picture at [uri] as a new frame at the playhead on the chosen track (where a frame is
 * already, just after it), fitted to the canvas: one undo step.
 */
fun TimelineTool.addPicture(uri: Uri) {
    stopPlaying()
    val s = session ?: return
    vm.viewModelScope.launch {
        val image = withContext(Dispatchers.IO) { vm.decodePicture(uri, io.github.artcraftandroid.procraft.ui.CanvasSpace.p3) }
        if (s !== session) return@launch
        if (image == null) {
            warn("This picture can't be opened.")
            return@launch
        }
        if (s.insertImageFrame(image) == null) warn(if (trackLocked) "The track is locked." else "There's no room for another layer.")
        done()
    }
}

/** Whether clip [c] is a drawing (a frame of its own pictures), not a group, a sound or a video. */
val TimelineClipInfo.isDrawing: Boolean get() = kind == TimelineClipKind.DRAWING

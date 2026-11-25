package io.github.artcraftandroid.procraft.canvas

import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import io.github.artcraftandroid.procraft.core.EasingSpec
import io.github.artcraftandroid.procraft.core.MotionPath
import io.github.artcraftandroid.procraft.core.TimelineChannel
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelineEasing
import io.github.artcraftandroid.procraft.core.TimelineKeyEdit
import io.github.artcraftandroid.procraft.core.TimelineKeyInfo
import io.github.artcraftandroid.procraft.core.TimelineKeyLane
import io.github.artcraftandroid.procraft.core.TimelineKeyPick
import io.github.artcraftandroid.procraft.core.TimelineKeysEdited
import io.github.artcraftandroid.procraft.core.TimelineLaneKey
import io.github.artcraftandroid.procraft.core.TimelinePose
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import kotlin.math.abs

// Keys on the grid timeline, as the iPad's TimelineKeyLanes.swift and TimelineGridKeys.swift have
// them: what the rows of keys under a track show, the keys picked on them, and what a drag on them
// would do. A track whose blocks have keys has a chevron after its name; turned, a row for each
// kind of key its blocks have (moved, scaled, turned, faded, an adjustment's values; a sound's
// level) stands under it, a mark on each keyed frame. A mark is tapped to pick it (the playhead
// going to it), held for its menu, held and dragged to move it and the keys picked with it; keys
// picked on two frames or more stretch in time from a grip either side. A drag shows what it would
// do (the core works it out on a copy) and is one undo step when let go.

/** The channels in the order their rows stand. */
val LaneOrder = listOf(TimelineChannel.POSITION, TimelineChannel.SCALE, TimelineChannel.ROTATION, TimelineChannel.OPACITY, TimelineChannel.ADJUSTMENT)

/** A row of keys under track [track]: one channel of every clip on it, or its sounds' level. */
sealed interface KeyLaneRow {
    val track: ULong

    data class Channel(override val track: ULong, val channel: TimelineChannel) : KeyLaneRow
    data class Volume(override val track: ULong) : KeyLaneRow

    /** What its row is called. */
    val said: String
        get() = when (this) {
            is Volume -> "Volume"
            is Channel -> when (channel) {
                TimelineChannel.POSITION -> "Position"
                TimelineChannel.SCALE -> "Scale"
                TimelineChannel.ROTATION -> "Rotation"
                TimelineChannel.OPACITY -> "Opacity"
                TimelineChannel.ADJUSTMENT -> "Adjustment"
            }
        }

    /** Whether it and [other] are of one kind, whatever tracks they are of. */
    fun sameKind(other: KeyLaneRow): Boolean = when (this) {
        is Channel -> other is Channel && other.channel == channel
        is Volume -> other is Volume
    }

    companion object {
        /** The kinds of row, in the order they stand. */
        val Kinds: List<KeyLaneRow> = LaneOrder.map { Channel(0u, it) } + Volume(0u)
    }
}

/** The clip a lane's keys are on (none: the camera's). */
val TimelineKeyLane.clip: ULong?
    get() = when (this) {
        is TimelineKeyLane.Clip -> id
        is TimelineKeyLane.Volume -> id
        TimelineKeyLane.Camera -> null
    }

/** The keys picked on the rows of keys, and what a drag on them would do. */
class TimelineKeyLanes internal constructor(private val vm: CanvasViewModel) {
    private val tool: TimelineTool get() = vm.timeline

    /** The keys picked, on any row: a track's rows put away keep its picks, which come back with them. */
    var picks by mutableStateOf<Set<TimelineKeyPick>>(emptySet())
        private set

    /** A drag under way: the lanes it changes, their keys as it would leave them, and where the picked ones would go. */
    var preview by mutableStateOf<TimelineKeysEdited?>(null)
        private set

    /** Each sound's level keys, with how they ease (the clips' own come with the timeline's keys). */
    var volumeKeys by mutableStateOf<Map<ULong, List<TimelineLaneKey>>>(emptyMap())
        private set

    /** A tick as a drag shows another outcome ([warning]: an edit that could change nothing); set by the grid. */
    var tick: (warning: Boolean) -> Unit = {}

    /** The drag under way: the keys it takes, and what it asks of them. */
    private var dragPicks: List<TimelineKeyPick>? = null
    private var dragEdit: TimelineKeyEdit? = null

    /** After a refresh: the sounds' level keys as the panel's reading gives them, and picks on keys gone let go. */
    internal fun refreshed(keys: Map<ULong, List<TimelineKeyInfo>>, volumes: Map<ULong, List<TimelineLaneKey>>) {
        if (volumeKeys != volumes) volumeKeys = volumes
        if (picks.isEmpty()) return
        val there = HashSet<TimelineKeyPick>()
        for (k in volumes.values.flatten()) there.add(TimelineKeyPick(k.lane, k.frame))
        for ((id, ks) in keys) for (k in ks) there.add(TimelineKeyPick(TimelineKeyLane.Clip(id, k.channel), k.frame))
        val kept = picks.intersect(there)
        if (kept != picks) picks = kept
    }

    fun clear() {
        if (picks.isNotEmpty()) picks = emptySet()
        preview = null
        dragPicks = null
        dragEdit = null
    }

    /** The keys picked on rows showing now. */
    val shown: Set<TimelineKeyPick>
        get() {
            val lanes = tool.keyLanesShown
            return picks.filter { it.lane == TimelineKeyLane.Camera || it.lane in lanes }.toSet()
        }

    /** The keys picked as a drag would leave them: where it takes those it moves, the rest as they are. */
    val shownPicks: Set<TimelineKeyPick>
        get() {
            val p = preview ?: return shown
            val moved = p.lanes.toSet()
            return p.picks.toSet() + shown.filter { it.lane !in moved }
        }

    /** Lane [lane]'s keys [keys] as shown: as a drag would leave them while one is under way. */
    fun shownKeys(lane: TimelineKeyLane, keys: List<TimelineLaneKey>): List<TimelineLaneKey> {
        val p = preview ?: return keys
        if (lane !in p.lanes) return keys
        return p.keys.filter { it.lane == lane }
    }

    /** Key [key] tapped: taken in with the others, or out. Taken in, the playhead goes to it. */
    fun tap(key: TimelineKeyPick) {
        // a drag the system ended without letting go shows no more
        cancelDrag()
        if (key in picks) {
            picks = picks - key
        } else {
            picks = picks + key
            tool.goTo(key.frame)
        }
        tick(false)
    }

    /** Every key of [keys] (a row's) picked, or, all of them picked already, none of them. */
    fun toggleAll(keys: List<TimelineLaneKey>) {
        cancelDrag()
        val row = keys.map { TimelineKeyPick(it.lane, it.frame) }.toSet()
        picks = if (picks.containsAll(row)) picks - row else picks + row
        tick(false)
    }

    /** Key [key] taken in with the others picked, or out, leaving the playhead where it is. */
    fun toggle(key: TimelineKeyPick) = toggleAll(listOf(TimelineLaneKey(key.lane, key.frame, TimelineEasing.LINEAR)))

    /** Key [key] is one of those picked that show; if it isn't, it alone is picked. For a menu or a drag that acts on the keys picked. */
    fun take(key: TimelineKeyPick) {
        if (key in shown) return
        cancelDrag()
        picks = setOf(key)
    }

    /** [edit] done to the keys picked that show, as one undo step; they stay picked where they went. */
    fun apply(edit: TimelineKeyEdit) {
        val taken = shown
        if (taken.isEmpty()) return
        commit(edit, taken.toList())
    }

    /** Clip [id]'s key of [channel] at frame [frame] taken away, the others there kept: one undo step. */
    fun removeKey(channel: TimelineChannel, frame: UInt, id: ULong) =
        commit(TimelineKeyEdit.Remove, listOf(TimelineKeyPick(TimelineKeyLane.Clip(id, channel), frame)))

    private fun commit(edit: TimelineKeyEdit, taken: List<TimelineKeyPick>) {
        tool.stopPlaying()
        val ended = vm.session?.editTimelineKeys(taken, edit)
        if (ended != null) {
            picks = (picks - taken.toSet()) + ended
        } else {
            // nothing changed: one of them has no room that way, or they ease so already
            tick(true)
        }
        vm.refresh()
    }

    /** Key [key] dragged: it, or every key picked that shows if it is one of them, [by] frames along. Shown, not done, until [endDrag]. */
    fun drag(key: TimelineKeyPick, by: Int) {
        if (dragPicks == null) {
            if (key !in shown) picks = setOf(key)
            dragPicks = shown.toList()
        }
        show(if (by == 0) null else TimelineKeyEdit.Move(by))
    }

    /** A stretch grip dragged: the picked keys' distance from frame [anchor] times [factor]. */
    fun stretch(anchor: UInt, factor: Double) {
        if (dragPicks == null) {
            val taken = shown
            if (taken.isEmpty()) return
            dragPicks = taken.toList()
        }
        show(if (abs(factor - 1.0) < 0.001) null else TimelineKeyEdit.Stretch(anchor, factor))
    }

    private fun show(edit: TimelineKeyEdit?) {
        val taken = dragPicks ?: return
        val asked = dragEdit
        if (edit != null && asked != null && same(edit, asked)) return
        dragEdit = edit
        val next = edit?.let { vm.session?.timelineKeysEditPreview(taken, it) }
        if (next?.picks != preview?.picks) tick(false)
        preview = next
    }

    /** The drag let go: what it showed, done as one undo step. */
    fun endDrag() {
        val taken = dragPicks ?: return
        val edit = dragEdit
        val went = preview != null
        dragPicks = null
        dragEdit = null
        preview = null
        if (went && edit != null) commit(edit, taken)
    }

    /** The drag taken away before it was let go: nothing done, nothing shown. */
    fun cancelDrag() {
        if (dragPicks == null && preview == null) return
        dragPicks = null
        dragEdit = null
        preview = null
    }

    private fun same(a: TimelineKeyEdit, b: TimelineKeyEdit): Boolean = when {
        a is TimelineKeyEdit.Move && b is TimelineKeyEdit.Move -> a.by == b.by
        a is TimelineKeyEdit.Stretch && b is TimelineKeyEdit.Stretch -> a.anchor == b.anchor && abs(a.factor - b.factor) < 0.005
        else -> false
    }
}

// ---- what the rows of keys and the cards read and change, on the timeline ----

/** The clip of the track drawn on at the playhead. */
val TimelineTool.clipHere: TimelineClipInfo?
    get() {
        val at = frame
        return track?.clips?.firstOrNull { it.start <= at && at < it.start + it.duration }
    }

/** The rows of keys track [t] has: one for each kind any block on it is keyed in, and its sounds' level. */
fun TimelineTool.keyRows(t: TimelineTrackInfo): List<KeyLaneRow> {
    val keyed = HashSet<TimelineChannel>()
    for (c in t.clips) for (k in keys[c.id].orEmpty()) keyed.add(k.channel)
    val rows: MutableList<KeyLaneRow> = LaneOrder.filter { it in keyed }.map { KeyLaneRow.Channel(t.id, it) }.toMutableList()
    if (t.clips.any { lanes.volumeKeys[it.id] != null }) rows.add(KeyLaneRow.Volume(t.id))
    return rows
}

/** Whether any block on track [t] has keys. */
fun TimelineTool.keyed(t: TimelineTrackInfo): Boolean = t.clips.any { keys[it.id].orEmpty().isNotEmpty() || lanes.volumeKeys[it.id] != null }

/** Track [id]'s rows of keys put out under it, or away (keys picked on rows no longer showing aren't acted on, and come back with them). */
fun TimelineTool.toggleKeyRows(id: ULong) {
    gridKeyTracks = if (id in gridKeyTracks) gridKeyTracks - id else gridKeyTracks + id
}

/** Row [row]'s keys, every clip's on its track, in frame order. */
fun TimelineTool.keyLaneKeys(row: KeyLaneRow): List<TimelineLaneKey> {
    val t = tracks.firstOrNull { it.id == row.track } ?: return emptyList()
    val out = ArrayList<TimelineLaneKey>()
    for (c in t.clips) {
        when (row) {
            is KeyLaneRow.Channel -> for (k in keys[c.id].orEmpty()) {
                if (k.channel == row.channel) out.add(TimelineLaneKey(TimelineKeyLane.Clip(c.id, row.channel), k.frame, k.easing))
            }
            is KeyLaneRow.Volume -> out.addAll(lanes.volumeKeys[c.id].orEmpty())
        }
    }
    return out.sortedBy { it.frame }
}

/** Row [row]'s keys as they show (as a drag under way would leave them): each block's keys are a lane of their own to the drag. */
fun TimelineTool.shownKeys(row: KeyLaneRow): List<TimelineLaneKey> =
    keyLaneKeys(row).groupBy { it.lane }.flatMap { (lane, ks) -> lanes.shownKeys(lane, ks) }.sortedBy { it.frame }

/** The lanes whose rows of keys show. */
val TimelineTool.keyLanesShown: Set<TimelineKeyLane>
    get() {
        val out = HashSet<TimelineKeyLane>()
        for (t in tracks) {
            if (t.id !in gridKeyTracks) continue
            for (c in t.clips) {
                for (channel in LaneOrder) out.add(TimelineKeyLane.Clip(c.id, channel))
                if (c.kind == TimelineClipKind.AUDIO) out.add(TimelineKeyLane.Volume(c.id))
            }
        }
        return out
    }

/** The key on rows [rows] (those sharing one row of the grid) on timeline frame [frame], if there is one. */
fun TimelineTool.key(rows: List<KeyLaneRow>, frame: Int): TimelineKeyPick? {
    for (lane in rows) {
        val k = shownKeys(lane).firstOrNull { it.frame.toInt() == frame } ?: continue
        return TimelineKeyPick(k.lane, k.frame)
    }
    return null
}

/** The playhead brought onto block [clip] if it isn't on one of its frames (its first, or its last when the playhead is past it): keys are set on the playhead's frame. */
fun TimelineTool.bringPlayhead(clip: TimelineClipInfo) {
    if (clip.duration == 0u || (frame >= clip.start && frame < clip.start + clip.duration)) return
    goTo(if (frame < clip.start) clip.start else clip.start + clip.duration - 1u)
}

/** Where clip [id] is at the playhead, as its keys put it. */
fun TimelineTool.pose(id: ULong): TimelinePose? = session?.timelinePose(id)

/** A key of [channel] at the playhead for clip [id]: shown at once, one undo step when [done]. */
fun TimelineTool.setKey(id: ULong, channel: TimelineChannel, values: List<Float>, done: Boolean) {
    stopPlaying()
    session?.setTimelineKey(id, channel, values, keyEasing, done)
    if (done) vm.refresh()
}

/** A key of [channel] for block [id] on the playhead's frame, holding what it shows there; one already there is taken away instead. One undo step. */
fun TimelineTool.toggleKey(id: ULong, channel: TimelineChannel) {
    if (keys[id].orEmpty().any { it.frame == frame && it.channel == channel }) {
        lanes.removeKey(channel, frame, id)
        return
    }
    val p = pose(id) ?: return
    val values = when (channel) {
        TimelineChannel.POSITION -> listOf(p.x, p.y)
        TimelineChannel.SCALE -> listOf(p.scaleX, p.scaleY)
        TimelineChannel.ROTATION -> listOf(p.rotation)
        TimelineChannel.OPACITY -> listOf(p.opacity)
        TimelineChannel.ADJUSTMENT -> return
    }
    setKey(id, channel, values, done = true)
}

/** Clip [id]'s keys on the playhead's frame, taken away (every channel's). */
fun TimelineTool.removeKeys(id: ULong) {
    session?.removeTimelineKeys(id)
    vm.refresh()
}

/** Clip [id]'s keys on the playhead's frame ([here]) or all of them, copied. */
fun TimelineTool.copyKeys(id: ULong, here: Boolean) {
    if (session?.copyTimelineKeys(id, here) == true) vm.showDone("Keys copied. Paste lays them on the frame at the playhead.")
    vm.refresh()
}

/** The keys copied, onto the clip at the playhead on the track drawn on, from the playhead on. */
fun TimelineTool.pasteKeys() {
    stopPlaying()
    if (session?.pasteTimelineKeys() != true) vm.showNotice("Move the playhead onto a frame to paste keys onto it.", warning = true)
    vm.refresh()
}

/** How clip [id]'s keys at [frame] ease into the next. */
fun TimelineTool.easing(id: ULong, frame: UInt): EasingSpec? = session?.timelineKeyEasing(id, frame)

/** Clip [id]'s keys at [frame] ease as [spec]: shown at once, one undo step when [done]. */
fun TimelineTool.setEasing(id: ULong, frame: UInt, spec: EasingSpec, done: Boolean) {
    session?.setTimelineKeyEasing(id, frame, spec, done)
    if (done) vm.refresh()
}

/** The clip whose motion path shows over the canvas: the one under the playhead on the chosen track while Animate is on or that track's rows of keys are out. */
val TimelineTool.pathClip: ULong?
    get() {
        val t = track ?: return null
        if (!autokey && t.id !in gridKeyTracks) return null
        return clipHere?.id
    }

/** Clip [id]'s motion path. */
fun TimelineTool.motionPath(id: ULong): MotionPath? = session?.timelineMotionPath(id)

/** Clip [id]'s position key at [frame] moved by [delta] (canvas pixels, as its keys count) from [start]: the playhead goes to it first. Shown at once, one undo step when [done]. */
fun TimelineTool.moveKey(id: ULong, frame: UInt, delta: Offset, start: TimelinePose, done: Boolean) {
    val s = session ?: return
    if (info?.frame != frame) s.setTimelineFrame(frame)
    // a key already there keeps how it eases
    val easing = keys[id].orEmpty().firstOrNull { it.frame == frame && it.channel == TimelineChannel.POSITION }?.easing ?: keyEasing
    s.setTimelineKey(id, TimelineChannel.POSITION, listOf(start.x + delta.x, start.y + delta.y), easing, done)
    if (done) vm.refresh()
}

/** Clip [id]'s position key at [frame] bent by [handles] (before x y, after x y, on the canvas). Shown at once, one undo step when [done]. */
fun TimelineTool.setHandles(id: ULong, frame: UInt, handles: List<Float>, done: Boolean) {
    session?.setTimelinePathHandles(id, frame, handles, done)
    if (done) vm.refresh()
}

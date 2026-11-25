package io.github.artcraftandroid.procraft.canvas

import android.graphics.Bitmap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.unit.dp
import androidx.lifecycle.viewModelScope
import io.github.artcraftandroid.procraft.core.CanvasSession
import io.github.artcraftandroid.procraft.core.LayerInfo
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelineClipboardInfo
import io.github.artcraftandroid.procraft.core.TimelineGroupShot
import io.github.artcraftandroid.procraft.core.TimelineInfo
import io.github.artcraftandroid.procraft.core.TimelineKeyInfo
import io.github.artcraftandroid.procraft.core.TimelineOnion
import io.github.artcraftandroid.procraft.core.TimelinePanel
import io.github.artcraftandroid.procraft.core.TimelinePicture
import io.github.artcraftandroid.procraft.core.TimelineRow
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import java.nio.ByteBuffer
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// The timeline at the bottom of the canvas, once the artwork is on one: what the grid
// (ui/TimelineGrid.kt) shows, read from the core in one call, and what it changes there. The iPad's
// `TimelineTool` (TimelineBar.swift), as far as the grid's first wave goes: its tracks and clips,
// the playhead, scrubbing, playing, the drawings' pictures.

/** The timeline as the core keeps it: its tracks and clips, the playhead, playing. */
class TimelineTool internal constructor(internal val vm: CanvasViewModel) {
    /** The timeline's settings and playhead; none while the canvas isn't on one. */
    var info by mutableStateOf<TimelineInfo?>(null)
        private set

    /** The panel's rows, bottom first as the core gives them: an unfolded group's own tracks just below the track holding it. */
    var rows by mutableStateOf<List<TimelineRow>>(emptyList())
        private set

    /** Each clip's keys, by clip (its keyed frames have a mark under them). */
    var keys by mutableStateOf<Map<ULong, List<TimelineKeyInfo>>>(emptyMap())
        private set

    /** What each folded group clip shows on each of its frames: its block's pictures are of the drawings there. */
    var groupShots by mutableStateOf<Map<ULong, List<TimelineGroupShot>>>(emptyMap())
        private set

    /** The onion skins either side. */
    var onion by mutableStateOf<TimelineOnion?>(null)
        private set

    /** Each drawing's picture, by its layer: the whole canvas. */
    val thumbnails = mutableStateMapOf<ULong, ImageBitmap>()

    /** Playing (or scrubbing): the frame shown. */
    var shownFrame by mutableStateOf<UInt?>(null)
        private set
    var isPlaying by mutableStateOf(false)
        private set

    /** The frame standing under the grid's playhead while it plays: the grid holds still there, the playhead's outline going over the frames instead. */
    var playHeld by mutableStateOf<UInt?>(null)
        private set

    /** What the timeline's clipboard holds (pasting needs something there). */
    var clipboard by mutableStateOf(TimelineClipboardInfo(0u, false))
        private set

    /** Counts the panel's readings: what is read through the session rather than kept here (an empty run's length, a block's pose, its motion path) is read again with it. */
    var revision by mutableStateOf(0)
        private set

    /** The frame tapped under the playhead: its handles out, it lasts longer or shorter by them (see ui/TimelineGridEdges.kt). */
    var picked by mutableStateOf<ULong?>(null)

    /** The lasso on: the frames picked together (a tap takes one in or out, a stroke every one it crosses). None: it is off. */
    var selecting by mutableStateOf<Set<ULong>?>(null)

    /** The clip whose Keys card is open (a frame's or a block's menu's Keys asked for it): the card is the keys' own (none: closed). */
    var keysCardFor by mutableStateOf<ULong?>(null)

    /** How tall the grid's bar stands from the bottom of the screen: the toolbars at the bottom (the transform's, the selection's, an adjustment's) stand above it. */
    var gridLift by mutableStateOf(0.dp)

    /** The frames the grid shows with frame `at` under its playhead (the grid sets it as it is laid out; none of the timeline's in sight, none): playing goes round them. */
    var gridFrames: ((UInt) -> UIntRange?)? = null

    // ---- keys (see TimelineKeyLanes.kt and ui/TimelineGridKeys.kt; the Keys card's block is keysCardFor) ----

    /** Animate (autokey): the transform tool keys the pose of what is under the playhead. Off as a canvas opens. */
    var autokey by mutableStateOf(false)

    /** The tracks whose rows of keys are put out under them. */
    var gridKeyTracks by mutableStateOf<Set<ULong>>(emptySet())

    /**
     * The app's own features showing (their built-in plugins on), as the canvas reads them: the
     * record button (Performed moves), My curves, and Add new's sounds and videos and the
     * animatic detector (Timeline media) go with theirs.
     */
    var features by mutableStateOf(io.github.artcraftandroid.procraft.core.Feature.entries.toSet())

    /** Whether [feature]'s ways in show. */
    fun shows(feature: io.github.artcraftandroid.procraft.core.Feature): Boolean = feature in features

    /** With Animate on, a take armed: the next touch on the canvas performs a move; and a take under way. */
    var performArmed by mutableStateOf(false)
    var performing by mutableStateOf(false)

    /** How new keys ease into the next (autokey's too). */
    var keyEasing by mutableStateOf(io.github.artcraftandroid.procraft.core.TimelineEasing.SMOOTH)
        private set

    /** Whether the timeline clipboard holds keys, to paste. */
    var keysCopied by mutableStateOf(false)
        private set

    /** The keys picked on the rows of keys, and a drag on them. */
    val lanes = TimelineKeyLanes(vm)


    /** Perform's take (TimelinePerform.kt). */
    val perform = TimelinePerform(vm)

    /** Its sounds and videos (TimelineMedia.kt). */
    val media = TimelineMedia(this)

    /** The tracks, bottom first. */
    val tracks: List<TimelineTrackInfo> get() = rows.map { it.track }

    val on: Boolean get() = info != null

    /** The frame in the middle: the one played or scrubbed past, else the playhead. */
    val frame: UInt get() = shownFrame ?: info?.frame ?: 0u

    /** The track drawn on. */
    val track: TimelineTrackInfo?
        get() {
            val id = info?.track ?: return tracks.firstOrNull()
            return tracks.firstOrNull { it.id == id }
        }

    /** Where track [id] sits among the unfolded groups, if it is shown. */
    fun place(id: ULong): TimelineRow? = rows.firstOrNull { it.track.id == id }

    /** Track [t] or a track above it holding a group it is inside is locked: its clips stay as they are. */
    fun held(t: TimelineTrackInfo): Boolean = t.locked || place(t.id)?.lockedAbove == true

    /** Whether group clip [id] shows its tracks unfolded under its own. */
    fun unfolded(id: ULong): Boolean = rows.any { it.group == id }

    /** Clip [id], with its track. */
    fun clip(id: ULong): Pair<TimelineTrackInfo, TimelineClipInfo>? {
        for (t in tracks) t.clips.firstOrNull { it.id == id }?.let { return t to it }
        return null
    }

    internal val session: CanvasSession? get() = vm.session

    /** The session the panel was last read from, and the number of that reading (see [refresh]). */
    private var readFrom: CanvasSession? = null
    private var panelRead = 0uL
    private var pictures: List<TimelinePicture> = emptyList()
    private var volumes: Map<ULong, List<io.github.artcraftandroid.procraft.core.TimelineLaneKey>> = emptyMap()
    private val pictureVersions = HashMap<ULong, ULong>()
    private var reading: Job? = null
    private var playing: Job? = null

    /**
     * Reads it again from the session, after any change: in one call, which gives nothing when
     * nothing the panel shows has changed since it last read it, and each part kept as it is
     * unless it changed (so what shows it isn't drawn again).
     */
    fun refresh() {
        val s = session
        if (s !== readFrom) {
            // another canvas (or none): nothing of the last one's is kept
            readFrom = s
            panelRead = 0uL
            pictures = emptyList()
            pictureVersions.clear()
            reading?.cancel()
            thumbnails.clear()
            stopPlaying()
            take(null)
            lanes.clear()
            autokey = false
            performArmed = false
            performing = false
            keysCardFor = null
        }
        if (s == null) return
        s.timelinePanel(panelRead)?.let {
            panelRead = it.generation
            take(it)
        }
        if (info == null) {
            stopPlaying()
            if (vm.timelineSettingsOpen) vm.closeTimelineSettings()
            if (picked != null) picked = null
            if (selecting != null) selecting = null
            if (keysCardFor != null) keysCardFor = null
            performArmed = false
            // (off a timeline there is nothing to animate)
            if (autokey) {
                autokey = false
                s.setTimelineAutokey(false, keyEasing)
            }
            return
        }
        // clips that have gone are no longer picked; a Keys card whose block has gone shuts; the
        // keys picked on keys gone are let go
        picked?.let { if (clip(it) == null) picked = null }
        selecting?.let { chosen ->
            val kept = chosen.filterTo(HashSet()) { clip(it) != null }
            if (kept != chosen) selecting = kept
        }
        keysCardFor?.let { if (clip(it) == null) keysCardFor = null }
        lanes.refreshed(keys, volumes)
        revision++
        readThumbnails(s)
    }

    /** A stroke ended: the drawing painted on has a new picture; a first stroke on an empty frame made a drawing there, which the layers show too. */
    fun strokeEnded() {
        val before = rows
        refresh()
        if (rows != before) vm.refresh()
    }

    /** The panel's reading kept: each part set only where it differs from what is shown. */
    private fun take(p: TimelinePanel?) {
        if (info != p?.info) info = p?.info
        val r = p?.rows.orEmpty()
        if (rows != r) rows = r
        val k = p?.keys.orEmpty().associate { it.id to it.keys }
        if (keys != k) keys = k
        val g = p?.groups.orEmpty().associate { it.id to it.shots }
        if (groupShots != g) groupShots = g
        if (onion != p?.onion) onion = p?.onion
        val held = p?.clipboard ?: TimelineClipboardInfo(0u, false)
        if (clipboard != held) clipboard = held
        val copied = p?.clipboard?.keys == true
        if (keysCopied != copied) keysCopied = copied
        volumes = p?.volumes.orEmpty().associate { it.id to it.keys }
        pictures = p?.pictures.orEmpty()
    }

    /** Draws again the pictures of the drawings whose version has changed since theirs was drawn (or that have none yet); one that can't be drawn is tried again on the next read. */
    private fun readThumbnails(s: CanvasSession) {
        val stale = pictures.filter { pictureVersions[it.root] != it.version }
        if (stale.isEmpty()) return
        reading?.cancel()
        reading = vm.viewModelScope.launch {
            for (picture in stale) {
                val made = withContext(Dispatchers.Default) {
                    val t = s.frameThumbnail(picture.root, THUMB_SIDE) ?: return@withContext null
                    val bitmap = io.github.artcraftandroid.procraft.ui.CanvasSpace.tag(Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888))
                    bitmap.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                    bitmap.asImageBitmap()
                }
                if (!isActive || s !== readFrom) return@launch
                if (made != null) {
                    thumbnails[picture.root] = made
                    pictureVersions[picture.root] = picture.version
                } else {
                    thumbnails.remove(picture.root)
                    pictureVersions.remove(picture.root)
                }
            }
        }
    }

    /** The artwork on a timeline (Animation Assist's frames becoming its drawings), or off it. */
    fun setOn(on: Boolean) {
        stopPlaying()
        vm.stopPlaying()
        val s = session ?: return
        if (on) s.startTimeline() else s.stopTimeline()
        if (!on) vm.closeTimelineSettings()
        vm.refresh()
    }

    /** The timeline's settings asked for (Actions → Canvas → Timeline Settings, or the grid's header): their card opens over the bar. */
    fun openSettings() {
        stopPlaying()
        vm.openTimelineSettings()
    }

    /** Draws on frame [frame] (playing stops): a layer of the drawing there becomes current. It may be an empty frame a little past the timeline's end, which the grid shows. */
    fun goTo(frame: UInt) {
        stopPlaying()
        session?.restTimelineFrame(frame)
        vm.refresh()
    }

    /** Scrubbing: frame [frame] shows as it plays. */
    fun scrub(frame: UInt) {
        if (shownFrame == null) stopPlaying()
        if (shownFrame != frame) media.blip(frame)
        shownFrame = frame
        session?.setTimelinePlaying(frame)
    }

    /** Scrubbing ends on [frame], which is drawn on. */
    fun endScrub(frame: UInt) {
        shownFrame = null
        session?.setTimelinePlaying(null)
        goTo(frame)
    }

    /** Track [id] is drawn on. */
    fun selectTrack(id: ULong) {
        if (info?.track == id) return
        session?.selectTimelineTrack(id)
        vm.refresh()
    }

    /** One frame on or back from the playhead (an empty one past the end too). */
    fun stepFrame(by: Int) {
        val last = (info?.duration ?: 1u).toInt() - 1 + PAST_END
        val to = (frame.toInt() + by).coerceIn(0, maxOf(last, 0))
        if (to == frame.toInt()) return
        picked = null
        goTo(to.toUInt())
        media.blip(to.toUInt())
    }

    /** The playhead to the start of the drawing before or after it on the track drawn on (a group counts as a drawing; sounds and videos don't). */
    fun stepDrawing(by: Int) {
        val clips = track?.clips ?: return
        val at = frame
        val starts = clips.filter { it.kind != TimelineClipKind.AUDIO && it.kind != TimelineClipKind.VIDEO }.map { it.start }
        val next = if (by > 0) starts.filter { it > at }.minOrNull() else starts.filter { it < at }.maxOrNull()
        if (next != null) {
            goTo(next)
            media.blip(next)
        }
    }

    /** A new drawing at the playhead on the track drawn on, the ones after it moving along. */
    fun addDrawing() {
        stopPlaying()
        val s = session ?: return
        if (s.addTimelineDrawing() == 0uL) {
            vm.showNotice(if (track?.locked == true) "The track is locked." else "There's no room for another layer.", warning = true)
        }
        vm.refresh()
    }

    /** A new empty track on top, drawn on. */
    fun addTrack() {
        stopPlaying()
        session?.addTimelineTrack()
        vm.refresh()
    }

    /** Track [t] shown or hidden. */
    fun toggleHidden(t: TimelineTrackInfo) {
        session?.setTimelineTrack(t.id, !t.hidden, t.locked)
        vm.refresh()
    }

    /** Group clip [id] folded, or its tracks unfolded under its own. */
    fun fold(id: ULong, folded: Boolean) {
        session?.setTimelineGroupFolded(id, folded)
        vm.refresh()
    }

    /** Plays, or stops on the frame it got to. */
    fun togglePlaying() = if (isPlaying) stopPlaying(stay = true) else play()

    private fun play() {
        val first = info ?: return
        if (first.duration == 0u) return
        val s = session ?: return
        vm.closeTimelineSettings()
        // the frames the grid shows are the ones played; with none of the timeline's in sight (the
        // playhead resting past its end), those showing from its first frame
        var here = frame
        var shown = gridFrames?.invoke(here)
        if (gridFrames != null && shown == null) {
            goTo(0u)
            here = 0u
            shown = gridFrames?.invoke(0u)
        }
        val range = shown ?: (0u until first.duration)
        if (range.isEmpty()) return
        isPlaying = true
        playHeld = here
        playing?.cancel()
        playing = vm.viewModelScope.launch {
            var at = if (here in range) here else range.first
            var step = 1
            // each frame on time by the clock, so the sounds stay with the pictures
            var due = System.nanoTime() + seconds(media.play(at))
            while (isActive) {
                val now = info ?: return@launch
                shownFrame = at
                s.setTimelinePlaying(at)
                due += 1_000_000_000L / maxOf(now.fps.toLong(), 1L)
                delay(maxOf((due - System.nanoTime()) / 1_000_000L, 0L))
                val next = s.timelineNextStepWithin(at, step, range.first, range.last + 1u)
                if (next == null) {
                    // a single pass ended: the playhead stays on its last frame
                    stopPlaying(stay = true)
                    return@launch
                }
                due += seconds(media.follow(at, next.at, next.step))
                at = next.at
                step = next.step
            }
        }
    }

    /** Playing stops. With [stay], the playhead stays on the frame it stopped on, rather than going back to the one drawn on before. */
    fun stopPlaying(stay: Boolean = false) {
        val at = if (isPlaying) shownFrame else null
        val job = playing
        playing = null
        isPlaying = false
        playHeld = null
        media.stop()
        if (shownFrame != null) {
            shownFrame = null
            session?.setTimelinePlaying(null)
        }
        if (stay && at != null) goTo(at)
        job?.cancel()
    }

    private fun seconds(s: Double): Long = (s * 1_000_000_000L).toLong()

    companion object {
        /** How far past the timeline's last frame the playhead may rest (the core's `PAST_END`). */
        const val PAST_END = 64

        /** The longest side of a drawing's picture. */
        private const val THUMB_SIDE = 96u
    }
}

/**
 * The layers the Layers panel lists: off a timeline, every one but those in closed groups; on one,
 * only those of the drawing drawn on (the one at the playhead on the track drawn on), as the iPad's
 * LayersOnTimeline.swift has it.
 */
fun CanvasViewModel.listedLayers(): List<LayerInfo> {
    val root = if (timeline.on) session?.timelineDrawingShown() else null
    val shown = layers.filter { !hidden(it) }
    if (root == null) return shown
    val byId = layers.associateBy { it.id }
    return shown.filter { l ->
        var at: ULong? = l.id
        var found = false
        for (i in 0..layers.size) {
            val id = at ?: break
            if (id == root) {
                found = true
                break
            }
            at = byId[id]?.parent
        }
        found
    }
}

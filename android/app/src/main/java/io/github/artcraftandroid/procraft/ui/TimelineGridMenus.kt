package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.layout.wrapContentHeight
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.focus.FocusRequester
import androidx.compose.ui.focus.focusRequester
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.Layout
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextRange
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.TextFieldValue
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.Constraints
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TimelineTool
import io.github.artcraftandroid.procraft.canvas.addFrames
import io.github.artcraftandroid.procraft.canvas.allDisabled
import io.github.artcraftandroid.procraft.canvas.canExtend
import io.github.artcraftandroid.procraft.canvas.clearFrames
import io.github.artcraftandroid.procraft.canvas.copyFrames
import io.github.artcraftandroid.procraft.canvas.cutFrames
import io.github.artcraftandroid.procraft.canvas.cycles
import io.github.artcraftandroid.procraft.canvas.deleteFrames
import io.github.artcraftandroid.procraft.canvas.deleteTrack
import io.github.artcraftandroid.procraft.canvas.duplicateFrames
import io.github.artcraftandroid.procraft.canvas.extend
import io.github.artcraftandroid.procraft.canvas.fitToContent
import io.github.artcraftandroid.procraft.canvas.flipFrames
import io.github.artcraftandroid.procraft.canvas.gap
import io.github.artcraftandroid.procraft.canvas.groupOf
import io.github.artcraftandroid.procraft.canvas.insertFrame
import io.github.artcraftandroid.procraft.canvas.isDrawing
import io.github.artcraftandroid.procraft.canvas.makeGroup
import io.github.artcraftandroid.procraft.canvas.moveOutOfGroup
import io.github.artcraftandroid.procraft.canvas.openKeys
import io.github.artcraftandroid.procraft.canvas.paste
import io.github.artcraftandroid.procraft.canvas.pasteClips
import io.github.artcraftandroid.procraft.canvas.renameClip
import io.github.artcraftandroid.procraft.canvas.renameTrack
import io.github.artcraftandroid.procraft.canvas.setClipBlend
import io.github.artcraftandroid.procraft.canvas.setClipDisabled
import io.github.artcraftandroid.procraft.canvas.setClipLabel
import io.github.artcraftandroid.procraft.canvas.setClipMask
import io.github.artcraftandroid.procraft.canvas.setClipOpacity
import io.github.artcraftandroid.procraft.canvas.setClipsDisabled
import io.github.artcraftandroid.procraft.canvas.setCycle
import io.github.artcraftandroid.procraft.canvas.setCycles
import io.github.artcraftandroid.procraft.canvas.setGap
import io.github.artcraftandroid.procraft.canvas.setLength
import io.github.artcraftandroid.procraft.canvas.setTrackBlend
import io.github.artcraftandroid.procraft.canvas.setTrackLabel
import io.github.artcraftandroid.procraft.canvas.setTrackMask
import io.github.artcraftandroid.procraft.canvas.setTrackOpacity
import io.github.artcraftandroid.procraft.canvas.split
import io.github.artcraftandroid.procraft.canvas.toggleLocked
import io.github.artcraftandroid.procraft.canvas.trim
import io.github.artcraftandroid.procraft.canvas.ungroup
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelineInfo
import io.github.artcraftandroid.procraft.core.TimelineMask
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import kotlin.math.roundToInt

// The grid timeline's menus (see TimelineGrid.kt), as the iPad's TimelineGridMenus.swift,
// TimelineGridAdd.swift, TimelineGridMedia.swift and TimelineGridSettings.swift have them: each a
// card standing on the bar over what was held. A frame's (held), a block's (a group, a sound or a
// video held), a track's (its name held, or Track options in another), the frames picked together
// (one of them held with the lasso on), an empty frame's, the end flap's (tapped), and Add new (`+`
// held). What a row opens stands beside its card.

/** How wide a menu's card is (dp). */
private const val CARD = 228f
private const val ADD_CARD = 268f
private const val EMPTY_CARD = 248f
private const val BLOCK_CARD = 252f
private const val CYCLE_CARD = 262f

/** The longest a timeline is typed (the core's own limit). */
private const val MOST_LENGTH = 100_000

/** The colours a track's name or a block is highlighted in (0xRRGGBB). */
private val Highlights = listOf(
    "Red" to 0xFF453Au, "Orange" to 0xFF9F0Au, "Yellow" to 0xFFD60Au, "Green" to 0x30D158u, "Blue" to 0x0A84FFu,
    "Purple" to 0xBF5AF2u, "Pink" to 0xF050C8u, "Grey" to 0x8E8E93u,
)

/** How a block or a track is masked by the one below it. */
private val Masks = listOf(TimelineMask.NONE to "None", TimelineMask.CLIPPING to "Clipping mask", TimelineMask.ALPHA to "Alpha mask", TimelineMask.LUMINANCE to "Luminance mask")

/**
 * Whichever menu is open, standing on the bar where it belongs: Add new over `+`, an empty
 * frame's, the end flap's, a track's over its name, a frame's or a block's or the picked frames'
 * over the frame held. [width]: the bar's (dp); [room]: how tall a menu may stand (dp).
 */
@Composable
internal fun GridMenus(
    vm: CanvasViewModel,
    tool: TimelineTool,
    info: TimelineInfo,
    grid: GridState,
    width: Float,
    room: Float,
    onPhoto: () -> Unit,
    onVideo: () -> Unit,
    onFile: () -> Unit,
) {
    val close = { grid.closeMenus(tool) }
    val clipMenu = grid.clipMenu
    val emptyMenu = grid.emptyMenu
    val flapMenu = grid.flapMenu
    val trackMenu = grid.trackMenu
    val pickedMenu = grid.pickedMenu
    Box(Modifier.width(width.dp).padding(bottom = 8.dp)) {
        when {
            // over the `+` it was opened from
            grid.adding -> GridMenuPlace(width - 41f, first = ADD_CARD, trailing = true) { GridAddMenu(tool.media.bringing, onPhoto, onVideo, onFile, close) }
            emptyMenu != null -> tool.tracks.firstOrNull { it.id == emptyMenu.track }?.let { track ->
                GridMenuPlace(emptyMenu.x) { GridEmptyMenu(vm, tool, track, emptyMenu.frame, room, close) }
            }
            flapMenu != null -> GridMenuPlace(flapMenu) { GridFlapMenu(tool, info, close) }
            trackMenu != null -> tool.tracks.firstOrNull { it.id == trackMenu }?.let { track ->
                GridMenuPlace(14f + CARD / 2f) { GridTrackMenu(vm, tool, track, room, close) }
            }
            clipMenu != null && tool.selecting == null -> tool.clip(clipMenu.clip)?.let { (track, clip) ->
                GridMenuPlace(clipMenu.x) {
                    if (clip.isDrawing) GridFrameMenu(vm, tool, track, clip, room, close) else GridBlockMenu(vm, tool, track, clip, room, close)
                }
            }
            pickedMenu != null && !tool.selecting.isNullOrEmpty() -> GridMenuPlace(pickedMenu) { GridPickedMenu(tool, tool.selecting.orEmpty().toList(), close) }
        }
    }
}

/** Its cards with the first of them ([first] wide; the last, with [trailing]) centred on [x] along the bar, as near as they all fit on it. */
@Composable
private fun GridMenuPlace(x: Float, first: Float = CARD, trailing: Boolean = false, content: @Composable () -> Unit) {
    Layout(content = content, modifier = Modifier.fillMaxWidth()) { measurables, constraints ->
        val placeables = measurables.map { it.measure(Constraints(maxHeight = constraints.maxHeight)) }
        val w = placeables.maxOfOrNull { it.width } ?: 0
        val h = placeables.maxOfOrNull { it.height } ?: 0
        val firstPx = first.dp.toPx()
        val left = (x.dp.toPx() - firstPx / 2f - (if (trailing) w - firstPx else 0f)).roundToInt().coerceIn(0, maxOf(constraints.maxWidth - w, 0))
        layout(constraints.maxWidth, h) { placeables.forEach { it.place(left, 0) } }
    }
}

/** What a frame's menu has open beside it. */
private enum class FrameBeside { OPTIONS, TYPING }

/**
 * A frame's menu: cut, copy, paste and duplicate; a new frame after it, lasting through the empty
 * frames after it, out of the group it is in; how many frames it lasts; its keys (the keys' own
 * card) and its track's options; turning it off, clearing and deleting it.
 */
@Composable
private fun GridFrameMenu(vm: CanvasViewModel, tool: TimelineTool, track: TimelineTrackInfo, clip: TimelineClipInfo, room: Float, close: () -> Unit) {
    var beside by remember(clip.id) { mutableStateOf<FrameBeside?>(null) }
    val drawing = clip.isDrawing
    val locked = tool.held(track)
    val ids = listOf(clip.id)
    Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        GridCard {
            GridMenuTools(
                cut = if (locked) null else ({ close(); tool.cutFrames(ids) }),
                copy = { close(); tool.copyFrames(ids) },
                paste = if (tool.clipboard.clips == 0u || locked) null else ({ close(); tool.paste(clip, track) }),
                duplicate = if (locked || clip.kind == TimelineClipKind.GROUP) null else ({ close(); tool.duplicateFrames(ids) }),
            )
            GridMenuLine()
            if (drawing) GridMenuRow(GridIcons.Insert, "Insert frame", dim = locked) { close(); tool.insertFrame(clip, track) }
            GridMenuRow(GridIcons.Fill, "Fill duration", dim = !tool.canExtend(ids)) { close(); tool.extend(ids) }
            // inside a group: out onto the level the group is on
            if (tool.groupOf(track.id) != 0uL) GridMenuRow(PcIcons.Layers, "Move out of group", dim = locked) { close(); tool.moveOutOfGroup(clip.id) }
            GridMenuLine()
            GridDurationRow(frames = clip.duration.toInt(), locked = locked, keypad = { beside = if (beside == FrameBeside.TYPING) null else FrameBeside.TYPING }) {
                tool.trim(clip.id, false, clip.start.toInt() + maxOf(it, 1))
            }
            GridMenuLine()
            // (its keys go on the playhead's frame: the playhead comes to it; the card is the keys' own)
            GridMenuRow(GridIcons.Key, "Keys", chevron = true) {
                close()
                tool.openKeys(clip)
            }
            GridMenuRow(GridIcons.Options, "Track options", chevron = true, lit = beside == FrameBeside.OPTIONS) {
                beside = if (beside == FrameBeside.OPTIONS) null else FrameBeside.OPTIONS
            }
            GridMenuLine()
            // turned off, it stays where it is but neither shows nor plays
            GridMenuRow(GridIcons.Highlight, if (clip.disabled) "Turn on" else "Turn off", dim = locked) { tool.setClipDisabled(clip.id, !clip.disabled) }
            if (drawing) GridMenuRow(GridIcons.Clear, "Clear frame", dim = locked) { close(); tool.clearFrames(ids) }
            GridMenuRow(PcIcons.Trash, "Delete frame", tint = Grid.Danger, dim = locked) { close(); tool.deleteFrames(ids) }
        }
        when (beside) {
            FrameBeside.OPTIONS -> GridTrackMenu(vm, tool, track, room, close)
            FrameBeside.TYPING -> GridCard {
                GridKeypad("frames", clip.duration.toInt(), 999) { n ->
                    tool.trim(clip.id, false, clip.start.toInt() + maxOf(n, 1))
                    beside = null
                }
            }
            null -> {}
        }
    }
}

/** What a track's menu has open beside it. */
private enum class TrackBeside { HIGHLIGHT, BLEND, MASK }

/** A track's menu: cut, copy, paste and duplicate (every frame of it); its opacity, name, highlight colour, blend mode and mask; locking it, clearing it, deleting it. */
@Composable
private fun GridTrackMenu(vm: CanvasViewModel, tool: TimelineTool, track: TimelineTrackInfo, room: Float, close: () -> Unit) {
    var sub by remember(track.id) { mutableStateOf<TrackBeside?>(null) }
    val ids = track.clips.map { it.id }
    val locked = tool.held(track)
    Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        GridCard {
            GridMenuTools(
                cut = if (ids.isEmpty() || locked) null else ({ close(); tool.cutFrames(ids) }),
                copy = if (ids.isEmpty()) null else ({ close(); tool.copyFrames(ids) }),
                paste = if (tool.clipboard.clips == 0u || locked) null else ({
                    close()
                    tool.selectTrack(track.id)
                    tool.pasteClips()
                }),
                duplicate = if (ids.isEmpty() || locked) null else ({ close(); tool.duplicateFrames(ids) }),
            )
            GridMenuLine()
            GridMenuSlider("Opacity", track.opacity) { v, done -> tool.setTrackOpacity(track.id, v, done) }
            GridRenameRow(track.name) { tool.renameTrack(track.id, it) }
            GridMenuRow(GridIcons.Highlight, "Highlight", chevron = true, lit = sub == TrackBeside.HIGHLIGHT) { sub = if (sub == TrackBeside.HIGHLIGHT) null else TrackBeside.HIGHLIGHT }
            GridMenuRow(GridIcons.Blend, "Blend mode", chevron = true, lit = sub == TrackBeside.BLEND) { sub = if (sub == TrackBeside.BLEND) null else TrackBeside.BLEND }
            GridMenuRow(GridIcons.Mask, "Mask", chevron = true, lit = sub == TrackBeside.MASK) { sub = if (sub == TrackBeside.MASK) null else TrackBeside.MASK }
            GridMenuRow(PcIcons.Lock, if (track.locked) "Unlock track" else "Lock track") { tool.toggleLocked(track) }
            GridMenuLine()
            // every frame off it, the track staying
            GridMenuRow(GridIcons.Broom, "Clear track", tint = Grid.Danger, dim = ids.isEmpty() || locked) { close(); tool.deleteFrames(ids) }
            // the last track stays
            GridMenuRow(PcIcons.Trash, "Delete track", tint = Grid.Danger, dim = tool.tracks.size <= 1) { close(); tool.deleteTrack(track.id) }
        }
        when (sub) {
            TrackBeside.HIGHLIGHT -> GridHighlightCard(track.label) { tool.setTrackLabel(track.id, it) }
            TrackBeside.BLEND -> GridBlendCard(vm, track.blend, room) { tool.setTrackBlend(track.id, it) }
            TrackBeside.MASK -> GridMaskCard(track.mask) { tool.setTrackMask(track.id, it) }
            null -> {}
        }
    }
}

/** The menu of the frames picked together: cut, copy, paste and duplicate; grouping them, flipping their order, filling their durations, turning them off or on, clearing and deleting them. The lasso stays on after each. */
@Composable
private fun GridPickedMenu(tool: TimelineTool, ids: List<ULong>, close: () -> Unit) {
    GridCard {
        GridMenuTools(
            cut = { close(); tool.cutFrames(ids) },
            copy = { close(); tool.copyFrames(ids) },
            paste = if (tool.clipboard.clips == 0u) null else ({ close(); tool.pasteClips() }),
            duplicate = {
                close()
                // the copies picked in their place
                val made = tool.duplicateFrames(ids)
                if (made.isNotEmpty()) tool.selecting = made.toSet()
            },
        )
        GridMenuLine()
        GridMenuRow(PcIcons.Layers, "Group frames") { close(); tool.makeGroup() }
        GridMenuRow(GridIcons.Flip, "Flip frames", dim = ids.size < 2) { close(); tool.flipFrames(ids) }
        GridMenuRow(GridIcons.Fill, "Fill duration", dim = !tool.canExtend(ids)) { close(); tool.extend(ids) }
        GridMenuLine()
        // turned off, they stay where they are but neither show nor play: all of them off while
        // any is on, else all back on
        val allOff = tool.allDisabled(ids)
        GridMenuRow(GridIcons.Highlight, if (allOff) "Turn on" else "Turn off") { close(); tool.setClipsDisabled(ids, !allOff) }
        GridMenuRow(GridIcons.Clear, "Clear frames") { close(); tool.clearFrames(ids) }
        GridMenuRow(PcIcons.Trash, "Delete frames", tint = Grid.Danger) { close(); tool.deleteFrames(ids) }
    }
}

/** What a block's menu has open beside it. */
private enum class BlockBeside { HIGHLIGHT, BLEND, MASK, SIZE, SLIP, CYCLE, OPTIONS }

/**
 * The menu of a block that isn't a drawing (a group, a sound, a video): cut, copy, paste and
 * duplicate; its own opacity (a sound's level, its keys and mute), name, highlight colour, blend
 * mode and mask; a video's sound taken out, its timing track and size (TimelineGridMedia.kt); the
 * part of its file a sound or video plays; a group's folding, cycle and ungrouping; its keys (the
 * keys' own card); out of the group it is in; turning it off, splitting it at the playhead, a copy
 * after it, lasting through the empty frames after it; its track's options; deleting it.
 */
@Composable
private fun GridBlockMenu(vm: CanvasViewModel, tool: TimelineTool, track: TimelineTrackInfo, clip: TimelineClipInfo, room: Float, close: () -> Unit) {
    var beside by remember(clip.id) { mutableStateOf<BlockBeside?>(null) }
    val locked = tool.held(track)
    val sound = clip.kind == TimelineClipKind.AUDIO
    val video = clip.kind == TimelineClipKind.VIDEO
    val group = clip.kind == TimelineClipKind.GROUP
    // (read again after each change)
    tool.revision
    val media = if (group) null else tool.media.clip(clip.id)
    val ids = listOf(clip.id)
    val moves = tool.groupOf(track.id) != 0uL
    // whether the playhead is inside the block (past its first frame), where it can be split
    val inside = tool.frame > clip.start && tool.frame < clip.start + clip.duration
    fun opens(what: BlockBeside) {
        beside = if (beside == what) null else what
    }
    Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        GridCard(BLOCK_CARD) {
            // (as tall as there is room for: the rest scrolls)
            Column(Modifier.heightIn(max = maxOf(room - 40f, 200f).dp).verticalScroll(rememberScrollState())) {
                GridMenuTools(
                    cut = if (locked) null else ({ close(); tool.cutFrames(ids) }),
                    copy = { close(); tool.copyFrames(ids) },
                    paste = if (tool.clipboard.clips == 0u || locked) null else ({ close(); tool.paste(clip, track) }),
                    duplicate = if (locked || group) null else ({ close(); tool.duplicateFrames(ids) }),
                )
                GridMenuLine()
                if (sound) {
                    media?.let { GridSoundRows(tool, clip, it, locked) }
                } else {
                    GridMenuSlider("Opacity", clip.opacity) { v, done -> tool.setClipOpacity(clip.id, v, done) }
                }
                GridRenameRow(clip.name) { tool.renameClip(clip.id, it) }
                GridMenuRow(GridIcons.Highlight, "Highlight", chevron = true, lit = beside == BlockBeside.HIGHLIGHT) { opens(BlockBeside.HIGHLIGHT) }
                if (!sound) {
                    GridMenuRow(GridIcons.Blend, "Blend mode", chevron = true, lit = beside == BlockBeside.BLEND) { opens(BlockBeside.BLEND) }
                    GridMenuRow(GridIcons.Mask, "Mask", chevron = true, lit = beside == BlockBeside.MASK) { opens(BlockBeside.MASK) }
                }
                GridMenuLine()
                if (video && media != null) GridVideoRows(tool, clip, media, locked, close, beside == BlockBeside.SIZE) { opens(BlockBeside.SIZE) }
                if (media != null) GridMenuRow(GridIcons.Flip, "Slip", chevron = true, lit = beside == BlockBeside.SLIP) { opens(BlockBeside.SLIP) }
                if (group) {
                    val open = tool.unfolded(clip.id)
                    GridMenuRow(PcIcons.Layers, if (open) "Collapse group" else "Expand group") { tool.fold(clip.id, open) }
                    GridMenuRow(GridIcons.Flip, "Cycle", chevron = true, lit = beside == BlockBeside.CYCLE) { opens(BlockBeside.CYCLE) }
                    GridMenuRow(GridIcons.Clear, "Ungroup", dim = locked) { close(); tool.ungroup(clip.id) }
                }
                // a group or a video moves, scales, turns and fades by keys as a frame does
                if (!sound) {
                    GridMenuRow(GridIcons.Key, "Keys", chevron = true) {
                        close()
                        tool.openKeys(clip)
                    }
                }
                GridMenuLine()
                // inside a group: out onto the level the group is on
                if (moves) GridMenuRow(PcIcons.Layers, "Move out of group", dim = locked) { close(); tool.moveOutOfGroup(clip.id) }
                // turned off, it stays where it is but neither shows nor plays
                GridMenuRow(GridIcons.Highlight, if (clip.disabled) "Turn on" else "Turn off", dim = locked) { tool.setClipDisabled(clip.id, !clip.disabled) }
                // (a repeating group isn't cut)
                GridMenuRow(GridIcons.Scissors, "Split content", dim = locked || !inside || clip.cycle > 0u) { close(); tool.split(clip.id) }
                GridMenuRow(PcIcons.Duplicate, "Duplicate after", dim = locked || group) { close(); tool.duplicateFrames(ids) }
                GridMenuRow(GridIcons.Fill, "Fill duration", dim = !tool.canExtend(ids)) { close(); tool.extend(ids) }
                GridMenuRow(GridIcons.Options, "Track options", chevron = true, lit = beside == BlockBeside.OPTIONS) { opens(BlockBeside.OPTIONS) }
                GridMenuLine()
                GridMenuRow(PcIcons.Trash, "Delete content", tint = Grid.Danger, dim = locked) { close(); tool.deleteFrames(ids) }
            }
        }
        when (beside) {
            BlockBeside.HIGHLIGHT -> GridHighlightCard(clip.label) { tool.setClipLabel(clip.id, it) }
            BlockBeside.BLEND -> GridBlendCard(vm, clip.blend, room) { tool.setClipBlend(clip.id, it) }
            BlockBeside.MASK -> GridMaskCard(clip.mask) { tool.setClipMask(clip.id, it) }
            BlockBeside.SIZE -> media?.let { GridVideoSizeCard(vm, tool, clip, it) }
            BlockBeside.SLIP -> media?.let { GridSlipCard(tool, clip, it) }
            BlockBeside.CYCLE -> GridCycleCard(tool, clip)
            BlockBeside.OPTIONS -> GridTrackMenu(vm, tool, track, room, close)
            null -> {}
        }
    }
}

/** A group's cycle: playing its frames over and over, how many times, and whether its own keys run on across the repeats. */
@Composable
private fun GridCycleCard(tool: TimelineTool, clip: TimelineClipInfo) {
    val repeats = clip.cycle > 0u
    GridCard(CYCLE_CARD) {
        // what it lasts now is what is played again
        GridToggleRow("Repeat", repeats) { tool.setCycle(clip.id, if (repeats) 0u else clip.duration, clip.continueAcross) }
        if (repeats) {
            tool.revision
            tool.cycles(clip.id)?.let { (count, most) ->
                GridDurationRow(label = "Repeats", frames = count, locked = false) { tool.setCycles(clip.id, minOf(it, maxOf(most, 1))) }
            }
            GridToggleRow("Keys run on across repeats", clip.continueAcross) { tool.setCycle(clip.id, clip.cycle, !clip.continueAcross) }
        }
    }
}

/** What an empty frame's menu has open beside it. */
private enum class EmptyBeside { OPTIONS, LENGTH }

/** An empty frame's menu: pasting or a new frame there; how many empty frames the run it is in lasts, one of them or all of them taken out (the frames after moving earlier); its track's options. */
@Composable
private fun GridEmptyMenu(vm: CanvasViewModel, tool: TimelineTool, track: TimelineTrackInfo, frame: UInt, room: Float, close: () -> Unit) {
    var beside by remember(track.id, frame) { mutableStateOf<EmptyBeside?>(null) }
    // (read again after each change)
    tool.revision
    val gap = tool.gap(track.id, frame)
    val locked = tool.held(track)
    // with no frame after them, the empty frames have no length to change
    val fixed = locked || gap == null || gap.open
    val length = gap?.length?.toInt() ?: 0
    val at = gap?.start ?: frame
    Row(verticalAlignment = Alignment.Top, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        GridCard(EMPTY_CARD) {
            GridMenuRow(GridIcons.Paste, "Paste", dim = tool.clipboard.clips == 0u || locked) { close(); tool.paste(frame, track.id) }
            GridMenuRow(GridIcons.Insert, "Insert frame", dim = locked) { close(); tool.insertFrame(frame, track.id) }
            GridMenuLine()
            GridDurationRow(frames = length, locked = fixed, keypad = { beside = if (beside == EmptyBeside.LENGTH) null else EmptyBeside.LENGTH }) {
                tool.setGap(track.id, at, it)
            }
            GridMenuLine()
            GridMenuRow(GridIcons.Options, "Track options", chevron = true, lit = beside == EmptyBeside.OPTIONS) {
                beside = if (beside == EmptyBeside.OPTIONS) null else EmptyBeside.OPTIONS
            }
            GridMenuLine()
            GridMenuRow(GridIcons.Remove, "Remove single frame", dim = fixed) {
                // (the last of them gone, there is no empty frame left to have a menu)
                if (length <= 1) close()
                tool.setGap(track.id, at, length - 1)
            }
            GridMenuRow(GridIcons.Broom, "Remove empty section", dim = fixed) { close(); tool.setGap(track.id, at, 0) }
        }
        when (beside) {
            EmptyBeside.OPTIONS -> GridTrackMenu(vm, tool, track, room, close)
            EmptyBeside.LENGTH -> GridCard {
                GridKeypad("frames", length, 999) { n ->
                    tool.setGap(track.id, at, n)
                    beside = null
                }
            }
            null -> {}
        }
    }
}

/** The end flap's menu: the timeline's length typed in or fitted to its frames, and frames added before the first or after the last. */
@Composable
private fun GridFlapMenu(tool: TimelineTool, info: TimelineInfo, close: () -> Unit) {
    var typing by remember { mutableStateOf(false) }
    var adding by remember { mutableStateOf(0) }
    val fps = info.fps.toInt()
    val count = if (adding > 0) adding else fps
    Row(verticalAlignment = Alignment.Bottom, horizontalArrangement = Arrangement.spacedBy(6.dp)) {
        GridCard {
            Row(
                Modifier.fillMaxWidth().height(36.dp).clickable { typing = !typing }.padding(horizontal = 16.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Length", color = Grid.InkBright, fontSize = 15.sp, modifier = Modifier.weight(1f))
                Text("${info.duration} · ${timelineClock(info.duration.toInt(), fps)}", color = if (typing) Pc.Accent else Grid.InkBright, fontSize = 15.sp)
            }
            GridMenuRow(GridIcons.Fill, "Fit to content") { close(); tool.fitToContent() }
            GridMenuLine()
            GridDurationRow(label = "Frames to add", frames = count, locked = false) { adding = it }
            GridMenuRow(GridIcons.Insert, "Add at start") { close(); tool.addFrames(true, count) }
            GridMenuRow(GridIcons.Insert, "Add at end") { close(); tool.addFrames(false, count) }
        }
        if (typing) {
            GridCard {
                GridKeypad("frames", info.duration.toInt(), MOST_LENGTH, note = { timelineClock(it, fps) }) { n ->
                    close()
                    tool.setLength(n)
                }
            }
        }
    }
}

/** The card `+` held opens: a picture or a video from the photo picker, or a sound, a picture or a video from a file, at the playhead. While a file is being brought in ([busy]), no other is. */
@Composable
private fun GridAddMenu(busy: Boolean, onPhoto: () -> Unit, onVideo: () -> Unit, onFile: () -> Unit, close: () -> Unit) {
    GridCard(ADD_CARD) {
        Text(if (busy) "Bringing in…" else "Add new", color = Grid.InkBright, fontSize = 22.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 18.dp, end = 18.dp, top = 8.dp, bottom = 12.dp))
        Row(Modifier.padding(horizontal = 10.dp).fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(8.dp)) {
            GridAddTile(GridIcons.Photo, "Photo", dim = false) { close(); onPhoto() }
            GridAddTile(GridIcons.Video, "Video", dim = busy) { close(); onVideo() }
        }
        val ink = if (busy) Grid.InkDim else Grid.InkBright
        Row(
            Modifier.padding(start = 10.dp, end = 10.dp, top = 8.dp, bottom = 4.dp).fillMaxWidth().height(46.dp).clip(RoundedCornerShape(14.dp))
                .background(Grid.InkBright.copy(alpha = 0.08f))
                .clickable(enabled = !busy) { close(); onFile() }
                .semantics {
                    role = Role.Button
                    contentDescription = "File"
                }
                .padding(horizontal = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("File", color = ink, fontSize = 16.sp, modifier = Modifier.weight(1f))
            Icon(GridIcons.File, contentDescription = null, tint = ink, modifier = Modifier.size(24.dp))
        }
    }
}

/** One of Add new's tiles: its glyph over its name, dimmed where it can't be used. */
@Composable
private fun RowScope.GridAddTile(icon: ImageVector, label: String, dim: Boolean, action: () -> Unit) {
    Column(
        Modifier.weight(1f).height(92.dp).clip(RoundedCornerShape(18.dp)).background(Grid.InkBright.copy(alpha = 0.08f))
            .clickable(enabled = !dim, onClick = action)
            .semantics {
                role = Role.Button
                contentDescription = label
            },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(icon, contentDescription = null, tint = if (dim) Grid.InkDim else Pc.Accent, modifier = Modifier.size(36.dp))
        Spacer(Modifier.height(8.dp))
        Text(label, color = if (dim) Grid.InkDim else Grid.InkBright, fontSize = 16.sp)
    }
}

// ---- What a row opens ----

/** The highlight colours, the one [label] ticked, and None. */
@Composable
private fun GridHighlightCard(label: UInt?, set: (UInt?) -> Unit) {
    GridCard {
        for ((name, value) in Highlights) GridChoiceRow(name, swatch = rgb(value), chosen = label == value) { set(value) }
        GridChoiceRow("None", none = true, chosen = label == null) { set(null) }
    }
}

/** The blend modes, the one [blend] ticked; as tall as there is room for, the rest scrolling. */
@Composable
private fun GridBlendCard(vm: CanvasViewModel, blend: UInt, room: Float, set: (UInt) -> Unit) {
    val modes = vm.blendModeList
    GridCard {
        Column(Modifier.heightIn(max = minOf(modes.size * 34f, minOf(maxOf(room - 60f, 170f), 306f)).dp).verticalScroll(rememberScrollState())) {
            for (m in modes) GridChoiceRow(m.name, chosen = blend == m.id) { set(m.id) }
        }
    }
}

/** The masks, the one [mask] ticked. */
@Composable
private fun GridMaskCard(mask: TimelineMask, set: (TimelineMask) -> Unit) {
    GridCard {
        for ((m, name) in Masks) GridChoiceRow(name, chosen = mask == m) { set(m) }
    }
}

// ---- Pieces ----

/** A menu's card: its own presses go no further. */
@Composable
internal fun GridCard(width: Float = CARD, content: @Composable ColumnScope.() -> Unit) {
    val shape = RoundedCornerShape(22.dp)
    Column(
        Modifier.width(width.dp).shadow(18.dp, shape).clip(shape).background(Grid.Card)
            .pointerInput(Unit) { detectTapGestures { } }
            .padding(vertical = 8.dp),
        content = content,
    )
}

@Composable
internal fun GridMenuLine() {
    Box(Modifier.fillMaxWidth().padding(horizontal = 14.dp, vertical = 6.dp).height(1.dp).background(Grid.Line.copy(alpha = 0.7f)))
}

/** A row of a menu: a glyph and what it does ([lit]: what it opens is open); dimmed where it can't. */
@Composable
internal fun GridMenuRow(icon: ImageVector?, label: String, tint: Color? = null, chevron: Boolean = false, dim: Boolean = false, lit: Boolean = false, action: () -> Unit) {
    val color = if (dim) Grid.InkDim else if (lit) Pc.Accent else tint ?: Grid.InkBright
    Row(
        Modifier.fillMaxWidth().height(36.dp)
            .clickable(enabled = !dim, onClickLabel = label, role = Role.Button, onClick = action)
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (icon != null) Box(Modifier.width(22.dp), contentAlignment = Alignment.Center) { Icon(icon, contentDescription = null, tint = color, modifier = Modifier.size(20.dp)) }
        Text(label, color = color, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
        if (chevron) Icon(PcIcons.ChevronRight, contentDescription = null, tint = color, modifier = Modifier.size(15.dp))
    }
}

/** One of a list to choose from, the one chosen ticked: a colour's [swatch] before its name, or the "none" ring. */
@Composable
internal fun GridChoiceRow(label: String, swatch: Color? = null, none: Boolean = false, chosen: Boolean, action: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(34.dp).clickable(role = Role.Button, onClick = action).padding(horizontal = 16.dp)
            .semantics { if (chosen) contentDescription = "$label, chosen" },
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (swatch != null) Box(Modifier.size(18.dp).clip(CircleShape).background(swatch))
        if (none) Icon(GridIcons.Highlight, contentDescription = null, tint = Grid.InkBright, modifier = Modifier.size(18.dp))
        Text(label, color = Grid.InkBright, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
        if (chosen) {
            Box(Modifier.size(18.dp).clip(CircleShape).background(Pc.Accent), contentAlignment = Alignment.Center) {
                Icon(PcIcons.Tick, contentDescription = null, tint = Color.White, modifier = Modifier.size(13.dp))
            }
        }
    }
}

/** A menu's first row: cut, copy, paste and duplicate, each dimmed where it can't be done (none given). */
@Composable
private fun GridMenuTools(cut: (() -> Unit)?, copy: (() -> Unit)?, paste: (() -> Unit)?, duplicate: (() -> Unit)?) {
    Row(Modifier.fillMaxWidth().padding(horizontal = 6.dp)) {
        GridTool(GridIcons.Scissors, "Cut", cut)
        GridTool(GridIcons.Copy, "Copy", copy)
        GridTool(GridIcons.Paste, "Paste", paste)
        GridTool(PcIcons.Duplicate, "Duplicate", duplicate)
    }
}

@Composable
private fun RowScope.GridTool(icon: ImageVector, label: String, action: (() -> Unit)?) {
    val color = if (action == null) Grid.InkDim else Grid.InkBright
    Column(
        Modifier.weight(1f).heightIn(min = 46.dp).clickable(enabled = action != null, onClickLabel = label, role = Role.Button) { action?.invoke() },
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Icon(icon, contentDescription = null, tint = color, modifier = Modifier.size(21.dp))
        Spacer(Modifier.height(3.dp))
        Text(label, color = color, fontSize = 10.5.sp, maxLines = 1)
    }
}

/** A slider in a menu: its name and value (as a percentage, or as [text] says it) over a thin track, dragged or tapped anywhere along it; [changed] says whether it was let go there. */
@Composable
internal fun GridMenuSlider(label: String, value: Float, text: ((Float) -> String)? = null, changed: (Float, Boolean) -> Unit) {
    var live by remember { mutableStateOf<Float?>(null) }
    val shown = live ?: value
    val change by rememberUpdatedState(changed)
    val accent = Pc.Accent
    val track = Grid.Line
    Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 6.dp, bottom = 2.dp)) {
        Row(Modifier.fillMaxWidth()) {
            Text(label, color = Grid.InkBright, fontSize = 15.sp, modifier = Modifier.weight(1f), maxLines = 1)
            Text(text?.invoke(shown) ?: "${(shown * 100f).roundToInt()} %", color = Grid.InkBright, fontSize = 15.sp, maxLines = 1)
        }
        Spacer(Modifier.height(4.dp))
        Box(
            Modifier.fillMaxWidth().height(22.dp)
                .pointerInput(Unit) {
                    detectTapGestures { p ->
                        val v = (p.x / size.width).coerceIn(0f, 1f)
                        change(v, true)
                        live = null
                    }
                }
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(
                        onDragStart = { p ->
                            val v = (p.x / size.width).coerceIn(0f, 1f)
                            live = v
                            change(v, false)
                        },
                        onDragEnd = {
                            live?.let { change(it, true) }
                            live = null
                        },
                        onDragCancel = {
                            live?.let { change(it, true) }
                            live = null
                        },
                    ) { c, _ ->
                        c.consume()
                        val v = (c.position.x / size.width).coerceIn(0f, 1f)
                        live = v
                        change(v, false)
                    }
                }
                .drawBehind {
                    val y = size.height / 2f
                    val h = 3.dp.toPx()
                    val x = shown.coerceIn(0f, 1f) * size.width
                    drawRoundRect(track, Offset(0f, y - h / 2f), Size(size.width, h), CornerRadius(h / 2f))
                    drawRoundRect(accent, Offset(0f, y - h / 2f), Size(x, h), CornerRadius(h / 2f))
                    val knob = Size(14.dp.toPx(), 8.dp.toPx())
                    drawRoundRect(Color.White, Offset((x - knob.width / 2f).coerceIn(0f, size.width - knob.width), y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
                },
        )
    }
}

/** "Rename", which becomes a field for the new name (all of it chosen, so typing replaces it): Done renames. */
@Composable
private fun GridRenameRow(name: String, rename: (String) -> Unit) {
    var editing by remember { mutableStateOf<TextFieldValue?>(null) }
    val text = editing
    if (text == null) {
        GridMenuRow(GridIcons.Rename, "Rename") { editing = TextFieldValue(name, TextRange(0, name.length)) }
        return
    }
    val focus = remember { FocusRequester() }
    var focused by remember { mutableStateOf(false) }
    BasicTextField(
        value = text,
        onValueChange = { editing = it },
        singleLine = true,
        textStyle = TextStyle(color = Grid.InkBright, fontSize = 15.sp),
        cursorBrush = SolidColor(Pc.Accent),
        keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
        keyboardActions = KeyboardActions(onDone = {
            rename(text.text)
            editing = null
        }),
        modifier = Modifier.fillMaxWidth().height(36.dp).padding(horizontal = 16.dp).wrapContentHeight(Alignment.CenterVertically)
            .focusRequester(focus)
            .onFocusChanged {
                if (it.isFocused) focused = true else if (focused) editing = null
            },
    )
    LaunchedEffect(Unit) { focus.requestFocus() }
}

/** How many frames something lasts: a step shorter or longer, or the number typed: its number tapped opens [keypad] beside the menu (none given: it is typed in place). */
@Composable
private fun GridDurationRow(label: String = "Frame duration", frames: Int, locked: Boolean, keypad: (() -> Unit)? = null, set: (Int) -> Unit) {
    var typed by remember { mutableStateOf<String?>(null) }
    val color = if (locked) Grid.InkDim else Grid.InkBright
    fun commit() {
        val t = typed ?: return
        typed = null
        val n = t.toIntOrNull()
        if (n != null && n >= 1 && n != frames) set(minOf(n, 999))
    }
    Row(Modifier.fillMaxWidth().height(36.dp).padding(start = 16.dp, end = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = color, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
        Spacer(Modifier.width(6.dp))
        GridStep("−", if (frames > 1) color else Grid.InkDim) {
            if (!locked && frames > 1) {
                typed = null
                set(frames - 1)
            }
        }
        val box = Modifier.widthIn(min = 34.dp).height(20.dp).border(1.2.dp, color.copy(alpha = 0.8f), CircleShape)
        val shownText = typed
        if (shownText != null) {
            val focus = remember { FocusRequester() }
            var focused by remember { mutableStateOf(false) }
            BasicTextField(
                value = shownText,
                onValueChange = { v -> typed = v.filter { it.isDigit() }.take(4) },
                singleLine = true,
                textStyle = TextStyle(color = color, fontSize = 13.sp, fontWeight = FontWeight.Medium, textAlign = TextAlign.Center),
                cursorBrush = SolidColor(Pc.Accent),
                keyboardOptions = KeyboardOptions(keyboardType = KeyboardType.Number, imeAction = ImeAction.Done),
                keyboardActions = KeyboardActions(onDone = { commit() }),
                modifier = box.width(34.dp).wrapContentHeight(Alignment.CenterVertically).focusRequester(focus)
                    .onFocusChanged { if (it.isFocused) focused = true else if (focused) commit() },
            )
            LaunchedEffect(Unit) { focus.requestFocus() }
        } else {
            Box(
                box.clickable(enabled = !locked) {
                    if (keypad != null) keypad() else typed = "$frames"
                },
                contentAlignment = Alignment.Center,
            ) {
                Text("$frames", color = color, fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 1, modifier = Modifier.padding(horizontal = 4.dp))
            }
        }
        GridStep("+", color) {
            if (!locked) {
                typed = null
                set(frames + 1)
            }
        }
    }
}

@Composable
private fun GridStep(glyph: String, color: Color, action: () -> Unit) {
    Box(Modifier.size(28.dp, 34.dp).clickable(onClickLabel = if (glyph == "+") "Longer" else "Shorter", role = Role.Button, onClick = action), contentAlignment = Alignment.Center) {
        Text(glyph, color = color, fontSize = 18.sp, fontWeight = FontWeight.Medium)
    }
}

/** A setting that is on or off, with its switch. */
@Composable
private fun GridToggleRow(label: String, on: Boolean, action: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(44.dp).clickable(role = Role.Switch, onClick = action).padding(start = 16.dp, end = 12.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = Grid.InkBright, fontSize = 15.sp, maxLines = 2, modifier = Modifier.weight(1f))
        Box(
            Modifier.size(48.dp, 28.dp).clip(CircleShape).background(if (on) Pc.Accent else Grid.InkBright.copy(alpha = 0.28f)).padding(horizontal = 3.dp),
            contentAlignment = if (on) Alignment.CenterEnd else Alignment.CenterStart,
        ) { Box(Modifier.size(28.dp, 22.dp).clip(CircleShape).background(Color.White)) }
    }
}

/** A number typed on its own keys: what is typed so far over them ([from] until one is), something said of it ([note]), and Set to take it, from 1 to [most]. */
@Composable
private fun GridKeypad(unit: String, from: Int, most: Int, note: ((Int) -> String)? = null, set: (Int) -> Unit) {
    var typed by remember { mutableStateOf("") }
    val number = typed.toIntOrNull()
    val valid = number != null && number in 1..most
    Column(Modifier.fillMaxWidth().padding(horizontal = 10.dp, vertical = 2.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(Modifier.fillMaxWidth().height(34.dp).padding(horizontal = 8.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(typed.ifEmpty { "$from" }, color = if (typed.isEmpty()) Grid.InkDim else Grid.InkBright, fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.width(5.dp))
            Text(unit, color = Grid.Ink, fontSize = 13.sp, modifier = Modifier.weight(1f))
            if (note != null) Text(note(number ?: from), color = Grid.Ink, fontSize = 13.sp)
        }
        for (row in listOf(listOf("7", "8", "9"), listOf("4", "5", "6"), listOf("1", "2", "3"), listOf("0", "⌫", "Set"))) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for (key in row) {
                    val isSet = key == "Set"
                    val fill = when {
                        isSet && valid -> Pc.Accent
                        isSet -> Pc.Accent.copy(alpha = 0.35f)
                        else -> Grid.InkBright.copy(alpha = 0.1f)
                    }
                    Box(
                        Modifier.weight(1f).height(40.dp).clip(CircleShape).background(fill).clickable(role = Role.Button) {
                            when (key) {
                                "Set" -> if (valid && number != null) set(number)
                                "⌫" -> typed = typed.dropLast(1)
                                // no more than six, none starting with 0
                                else -> if (typed.length < 6 && !(typed.isEmpty() && key == "0")) typed += key
                            }
                        },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(key, color = if (isSet) Color.White else Grid.InkBright, fontSize = if (isSet) 16.sp else 19.sp, fontWeight = if (isSet) FontWeight.SemiBold else FontWeight.Normal)
                    }
                }
            }
        }
    }
}

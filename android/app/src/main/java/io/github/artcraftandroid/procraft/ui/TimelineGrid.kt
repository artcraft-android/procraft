package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animate
import androidx.compose.animation.core.calculateTargetValue
import androidx.compose.animation.core.exponentialDecay
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.alpha
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerInputChange
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.rememberTextMeasurer
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.min
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TimelineTool
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelineInfo
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import kotlin.math.abs
import kotlin.math.ceil
import kotlin.math.floor
import kotlin.math.hypot
import kotlin.math.roundToInt
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Job
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

// The timeline as a grid, as the iPad's TimelineGrid.swift has it: every track a row of frame
// cells under a ruler of frame numbers, the playhead standing still in the middle of the bar while
// the frames slide under it. A drawing held for more than a frame is one block, its first frame
// plain and the held ones shaded; a group, a sound or a video is a plain block with its name. The
// frames dragged sideways scrub and settle on a frame; a frame tapped slides under the playhead
// and its track is drawn on. Play goes round the frames the grid shows.
//
// The grid's first wave: the lasso, Animate, Add new, the menus held open, the lengthening
// handles, moving blocks, zoom and the rows of keys come later.

/** The grid's fixed sizes (dp, the iPad's points) and colours; its cells' and rows' sizes, which will zoom, are [GridSizes]. */
internal object Grid {
    val RulerHeight = 26.dp
    val HeaderHeight = 46.dp

    /** How far in from the bar's left edge the cells show whole: before it they fade out under the tracks' names, and touches there are the names'. Narrower on a narrow bar (a phone's), so the playhead stays clear of them. */
    fun labelsWidth(width: Dp): Dp = min(176.dp, width * 0.36f)

    private fun pick(dark: Long, light: Long) = Color(if (Pc.light) light else dark)

    val Panel: Color get() = pick(0xFF212121, 0xFFE4E4E4)
    val Slot: Color get() = pick(0xFF161616, 0xFFD1D1D1)
    val Line: Color get() = pick(0xFF474747, 0xFFC0C0C0)
    val Capsule: Color get() = pick(0xFF353535, 0xFFCBCBCB)
    val Ink: Color get() = pick(0xFFC9C9C9, 0xFF393939)
    val InkBright: Color get() = pick(0xFFF2F2F2, 0xFF1C1C1C)
    val InkDim: Color get() = pick(0xFF757575, 0xFF8C8C8C)
    val Box: Color get() = pick(0xFF868686, 0xFF8A8A8A)

    /** A clip that isn't a drawing (a group, a sound, a video). */
    val Block: Color get() = pick(0xFF4E4E4E, 0xFFB2B2B2)

    /** Laid over a drawing's held frames, so they read as the same drawing going on. */
    val HeldShade = Color(0x99404040)

    /** What is written on a track's highlight colour. */
    val OnLabel = Color(0xE6000000)
}

/** A colour the core gives as 0xRRGGBB. */
internal fun rgb(packed: UInt): Color = Color(0xFF000000.toInt() or packed.toInt())

/** What a finger down on the frames is doing. */
private enum class GridTouch { UNDECIDED, SCROLL, PAN }

/** What the grid shows that isn't the timeline's own: where it is scrolled to while dragged, and how far its rows are scrolled up. */
@Stable
internal class GridState {
    /** The frame under the playhead while the frames are dragged (between frames too); none when they rest on the playhead's own. */
    var dragged by mutableStateOf<Float?>(null)

    /** How far the tracks are scrolled up, where there are more of them than show. */
    var rowsOffset by mutableFloatStateOf(0f)

    /** Its cells' and rows' sizes (it doesn't zoom yet). */
    val sizes = GridSizes()

    /** A finger is dragging the frames sideways: the settling of the last drag leaves them be. */
    var scrolling = false
    var scrollFrom = 0f
    var scrubbed = 0u
    var settling: Job? = null

    /** The frames dragged sideways [translation] (dp) from where they were: the frame now under the playhead shows as it plays. */
    fun scrolled(translation: Float, last: Int, tool: TimelineTool) {
        if (dragged == null) scrubbed = tool.frame
        val at = (scrollFrom - translation / sizes.pitch).coerceIn(0f, last.toFloat())
        dragged = at
        val frame = at.roundToInt().toUInt()
        if (frame != scrubbed) {
            scrubbed = frame
            tool.scrub(frame)
        }
    }
}

/**
 * The timeline at the bottom of the canvas while the artwork is on one: a dark card, its header
 * (Play, a frame back and on, the settings, a new track, a new frame) over the ruler and the
 * tracks' rows.
 */
@Composable
fun BoxScope.TimelineGridBar(vm: CanvasViewModel) {
    val tool = vm.timeline
    val info = tool.info ?: return
    val grid = remember { GridState() }
    BoxWithConstraints(Modifier.align(Alignment.BottomCenter).fillMaxSize()) {
        val width = min(maxWidth - 32.dp, 1180.dp)
        val sizes = grid.sizes
        // top first; the core's order is bottom first
        val rows = remember(tool.rows, sizes) { GridRows(tool.rows.reversed(), sizes) }
        val all = maxOf(rows.height, sizes.rowHeight)
        val most = maxOf(floor(maxHeight.value * 0.5f - Grid.HeaderHeight.value - Grid.RulerHeight.value - 12f), sizes.rowHeight)
        val shown = minOf(all, most)
        val shape = RoundedCornerShape(28.dp)
        Column(
            Modifier.align(Alignment.BottomCenter).padding(bottom = 19.dp).width(width)
                .shadow(16.dp, shape)
                .clip(shape)
                .background(Grid.Panel)
                // the whole bar takes its touches: none reach the canvas under it
                .pointerInput(Unit) { detectTapGestures { } },
        ) {
            GridHeader(vm, tool)
            Box(Modifier.fillMaxWidth().height(1.dp).background(Grid.Line))
            GridBody(vm, tool, info, grid, rows, width, shown.dp)
            Spacer(Modifier.height(9.dp))
        }
    }
}

/** The bar's top row: Play, a frame back and on; then the settings, a new track, a new frame. */
@Composable
private fun GridHeader(vm: CanvasViewModel, tool: TimelineTool) {
    Row(
        Modifier.fillMaxWidth().height(Grid.HeaderHeight).padding(start = 26.dp, end = 16.dp, top = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.widthIn(min = 54.dp).height(40.dp)
                .clickable(remember { MutableInteractionSource() }, indication = null) { tool.togglePlaying() },
            contentAlignment = Alignment.CenterStart,
        ) {
            Text(if (tool.isPlaying) "Pause" else "Play", color = Grid.Ink, fontSize = 17.sp, fontWeight = FontWeight.Bold, maxLines = 1)
        }
        // tapped, a frame back or on; held, to the drawing before or after on the chosen track
        Spacer(Modifier.width(18.dp))
        GridIconButton(PcIcons.ChevronLeft, 21.dp, name = "Previous Frame", held = { tool.stepDrawing(-1) }) { tool.stepFrame(-1) }
        GridIconButton(PcIcons.ChevronRight, 21.dp, name = "Next Frame", held = { tool.stepDrawing(1) }) { tool.stepFrame(1) }
        Spacer(Modifier.weight(1f))
        GridIconButton(PcIcons.GridSettings, 27.dp, on = vm.timelineSettingsAsked, name = "Timeline Settings") {
            if (vm.timelineSettingsAsked) vm.timelineSettingsAsked = false else tool.openSettings()
        }
        GridIconButton(PcIcons.GridTrack, 27.dp, name = "Add Track") { tool.addTrack() }
        GridIconButton(PcIcons.Add, 38.dp, name = "Add Frame") { tool.addDrawing() }
    }
}

/** One of the header's glyphs: lit in the accent while it is on, and for a moment when tapped; [held], what holding it does. */
@Composable
private fun GridIconButton(icon: ImageVector, size: Dp, on: Boolean = false, name: String, held: (() -> Unit)? = null, action: () -> Unit) {
    var flash by remember { mutableStateOf(false) }
    val haptics = LocalHapticFeedback.current
    val scope = rememberCoroutineScope()
    val now by rememberUpdatedState(action)
    val hold by rememberUpdatedState(held)
    Box(
        Modifier.size(50.dp, 40.dp)
            .semantics {
                role = Role.Button
                contentDescription = name
            }
            .pointerInput(held != null) {
                detectTapGestures(
                    onTap = {
                        flash = true
                        now()
                        scope.launch {
                            delay(180)
                            flash = false
                        }
                    },
                    onLongPress = if (held != null) ({
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        hold?.invoke()
                    }) else null,
                )
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(icon, contentDescription = null, tint = if (on || flash) Pc.Accent else Grid.Ink, modifier = Modifier.size(size))
    }
}

/** Where the frames stand along the bar (dp): frame 0's cell's left edge, the playhead's middle, the last frame shown, and how the rows are scrolled and set in. */
internal data class GridPlace(
    val origin: Float,
    val playhead: Float,
    val last: Int,
    val scroll: Float,
    val over: Float,
    val inset: Float,
) {
    /** A point of the rows as they show, as a point of all of them. */
    fun inRows(p: Offset): Offset = Offset(p.x, p.y + scroll - inset)

    /** The frame at [x] (the gap after a cell counts as its own); none off the frames. */
    fun frame(x: Float, sizes: GridSizes): Int? = floor((x - origin) / sizes.pitch).toInt().takeIf { it in 0..last }
}

/** The ruler and the tracks' rows, the playhead's outline standing over them. */
@Composable
private fun GridBody(vm: CanvasViewModel, tool: TimelineTool, info: TimelineInfo, grid: GridState, rows: GridRows, width: Dp, height: Dp) {
    val sz = grid.sizes
    val focus = tool.frame.toFloat()
    // while it plays the grid holds still, the playhead's outline going over its frames
    val held = if (tool.isPlaying) tool.playHeld?.toFloat() else null
    val target = held ?: focus
    // a frame tapped or stepped to slides under the playhead, as does the one playing stopped on;
    // dragged, it follows the finger
    val slide = remember { Animatable(target) }
    LaunchedEffect(target) {
        if (grid.dragged != null || tool.isPlaying) slide.snapTo(target) else slide.animateTo(target, Motion.frameSlide())
    }
    val at = grid.dragged ?: slide.value
    val w = width.value
    val labels = Grid.labelsWidth(width).value
    val playhead = (w / 2f).roundToInt().toFloat()
    val last = maxOf(info.duration.toInt() - 1, 0) + TimelineTool.PAST_END
    val origin = playhead - sz.cellWidth / 2f - at * sz.pitch
    val duration = info.duration
    // the frames the grid shows are the ones playing goes round
    SideEffect {
        tool.gridFrames = { from -> shownFrames(from.toFloat(), duration, playhead, w, labels, sz) }
    }
    // the frames on the bar, and some either side for the slide to a frame tapped
    val lo = (floor(-origin / sz.pitch).toInt() - 14).coerceIn(0, last)
    val hi = (ceil((w - origin) / sz.pitch).toInt() + 14).coerceIn(lo, last)
    val all = maxOf(rows.height, sz.rowHeight)
    val h = height.value
    // more tracks than show: they scroll up and down
    val over = maxOf(all - h, 0f)
    val place = GridPlace(origin, playhead, last, minOf(grid.rowsOffset, over), over, maxOf(((h - all) / 2f).roundToInt().toFloat(), 0f))
    // the track drawn on is kept in sight where not every track shows
    LaunchedEffect(info.track, rows.size, over) {
        val row = rows.indexOf(info.track) ?: return@LaunchedEffect
        val top = rows[row].y
        val now = minOf(grid.rowsOffset, over)
        val next = minOf(maxOf(minOf(now, top), top + rows[row].height - h), over)
        if (abs(next - grid.rowsOffset) > 0.5f) grid.rowsOffset = maxOf(next, 0f)
    }
    Column {
        GridRuler(tool, grid, info, place, lo, hi, width)
        GridTracks(vm, tool, info, grid, rows, place, lo, hi, width, height, labels)
    }
}

/** The timeline's frames whose cells show whole right of the tracks' names, with frame [at] under the playhead; none of them in sight, none. */
private fun shownFrames(at: Float, frames: UInt, playhead: Float, width: Float, labels: Float, sz: GridSizes): UIntRange? {
    val origin = playhead - sz.cellWidth / 2f - at * sz.pitch
    val first = maxOf(ceil((labels - origin) / sz.pitch).toInt(), 0)
    val last = minOf(floor((width - origin - sz.cellWidth) / sz.pitch).toInt(), frames.toInt() - 1)
    if (first > last) return null
    return first.toUInt()..last.toUInt()
}

/**
 * A finger on the frames (or on the ruler, [ruler]): told apart as it goes into a tap, a drag
 * sideways (the frames slide, scrubbing, and settle on a frame), or, on the rows where there are
 * more tracks than show, a drag up or down.
 */
private fun Modifier.gridTouches(
    tool: TimelineTool,
    grid: GridState,
    settle: CoroutineScope,
    place: () -> GridPlace,
    ruler: Boolean,
    labels: Float,
    tapped: (Offset) -> Unit,
): Modifier = pointerInput(ruler, labels) {
    val d = density
    awaitEachGesture {
        val down = awaitFirstDown(requireUnconsumed = false)
        val start = down.position / d
        // over the tracks' names, the touch is theirs
        if (!ruler && start.x < labels - 12f) return@awaitEachGesture
        down.consume()
        // a touch on the frames while they play stops them there
        if (tool.isPlaying) {
            tool.stopPlaying(stay = true)
            return@awaitEachGesture
        }
        var touch = GridTouch.UNDECIDED
        val p0 = place()
        grid.scrollFrom = grid.dragged ?: tool.frame.toFloat()
        val panFrom = p0.scroll
        val velocity = VelocityTracker()
        velocity.addPosition(down.uptimeMillis, down.position)
        var lastChange: PointerInputChange = down
        var lifted = false
        try {
            while (true) {
                val event = awaitPointerEvent()
                val change = event.changes.firstOrNull { it.id == down.id } ?: break
                lastChange = change
                if (!change.pressed) {
                    lifted = true
                    break
                }
                velocity.addPosition(change.uptimeMillis, change.position)
                val moved = (change.position - down.position) / d
                when (touch) {
                    GridTouch.UNDECIDED -> if (hypot(moved.x, moved.y) > 6f) {
                        if (ruler || abs(moved.x) >= abs(moved.y) || p0.over <= 0f) {
                            touch = GridTouch.SCROLL
                            grid.settling?.cancel()
                            grid.scrolling = true
                            grid.scrolled(moved.x, p0.last, tool)
                        } else {
                            touch = GridTouch.PAN
                            grid.rowsOffset = (panFrom - moved.y).coerceIn(0f, p0.over)
                        }
                    }
                    GridTouch.SCROLL -> grid.scrolled(moved.x, p0.last, tool)
                    GridTouch.PAN -> grid.rowsOffset = (panFrom - moved.y).coerceIn(0f, p0.over)
                }
                change.consume()
            }
        } finally {
            // (taken away part way, a drag settles where it is)
            when (touch) {
                GridTouch.UNDECIDED -> if (lifted) tapped(start)
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
                GridTouch.PAN -> {}
            }
            if (lifted) lastChange.consume()
        }
    }
}

/**
 * The frame numbers, sliding with the frames: the playhead's bright. Dragged, the frames slide; a
 * number tapped, the playhead goes to it.
 */
@Composable
private fun GridRuler(tool: TimelineTool, grid: GridState, info: TimelineInfo, place: GridPlace, lo: Int, hi: Int, width: Dp) {
    val sz = grid.sizes
    val measurer = rememberTextMeasurer()
    val current = tool.frame.toInt()
    val dim = Grid.InkDim
    val lit = Grid.InkBright
    val latest by rememberUpdatedState(place)
    val scope = rememberCoroutineScope()
    Canvas(
        Modifier.size(width, Grid.RulerHeight).clipToBounds()
            .gridTouches(tool, grid, scope, { latest }, ruler = true, labels = 0f) { at ->
                val f = latest.frame(at.x, sz) ?: return@gridTouches
                if (f.toUInt() != tool.frame) tool.goTo(f.toUInt())
            },
    ) {
        val d = density
        val pitch = sz.pitch
        // every frame's number where they fit (44 apart, they always do until the grid zooms)
        val every = listOf(1, 2, 5, 10, 25, 50, 100, 250, 500, 1000).firstOrNull { it * pitch >= 26f } ?: 10000
        val mid = size.height / 2f + 3f * d
        fun x(f: Int) = (place.origin + f * pitch + sz.cellWidth / 2f) * d
        fun write(text: String, f: Int, here: Boolean) {
            val style = TextStyle(color = if (here) lit else dim, fontSize = 11.sp, fontWeight = if (here) FontWeight.Bold else FontWeight.Normal)
            val laid = measurer.measure(text, style)
            drawText(laid, topLeft = Offset(x(f) - laid.size.width / 2f, mid - laid.size.height / 2f))
        }
        for (f in lo..hi) {
            if (f == current) continue
            if ((f + 1) % every == 0) {
                // (a number too near the playhead's own gives way to it)
                if (every == 1 || abs((f - current) * pitch) >= 30f) write("${f + 1}", f, false)
            } else if (pitch >= 5f) {
                drawRect(dim.copy(alpha = 0.7f), Offset(x(f) - 0.5f * d, size.height - 5f * d), Size(1f * d, 3f * d))
            }
        }
        if (current in lo..hi) write("${current + 1}", current, true)
    }
}

/** The tracks' rows: the cells, the frames past the end shaded, the playhead's outline, and the tracks' names standing still over them. */
@Composable
private fun GridTracks(
    vm: CanvasViewModel,
    tool: TimelineTool,
    info: TimelineInfo,
    grid: GridState,
    rows: GridRows,
    place: GridPlace,
    lo: Int,
    hi: Int,
    width: Dp,
    height: Dp,
    labels: Float,
) {
    val sz = grid.sizes
    val latest by rememberUpdatedState(place)
    // (the touches' handler stays as it began: what a tap reads is read as it is now)
    val onTap by rememberUpdatedState<(Offset) -> Unit> { at -> tapped(tool, info, rows, place, sz, at) }
    val scope = rememberCoroutineScope()
    val all = maxOf(rows.height, sz.rowHeight)
    val chosenRow = rows.indexOf(info.track)
    // while it plays, where the playhead's outline stands as the grid holds still under it
    val head = if (tool.isPlaying) place.origin + tool.frame.toFloat() * sz.pitch else null
    val accent = Pc.Accent
    val panel = Grid.Panel
    val line = Grid.Line
    Box(
        Modifier.size(width, height).clipToBounds()
            .gridTouches(tool, grid, scope, { latest }, ruler = false, labels = labels) { at -> onTap(at) },
    ) {
        GridCells(vm, tool, rows, place, lo, hi, sz, labels, Modifier.fillMaxSize())
        Canvas(Modifier.fillMaxSize()) {
            val d = density
            val dy = (place.inset - place.scroll) * d
            // past the timeline's end: shaded, since nothing there plays
            val end = (place.origin + info.duration.toFloat() * sz.pitch - sz.gap / 2f) * d
            if (end < size.width) {
                val from = maxOf(end, 0f)
                drawRect(panel.copy(alpha = 0.55f), Offset(from, dy), Size(size.width - from, all * d))
                drawRect(line, Offset(from, dy), Size(1f * d, all * d))
            }
            // the playhead: an outline on the chosen track's frame, where it stands whatever slides under it
            if (chosenRow != null) {
                val r = rows[chosenRow]
                val stroke = 2.5f * d
                val left = ((head ?: (place.playhead - sz.cellWidth / 2f)) - 2.5f) * d
                val top = (r.y + (r.height - sz.cellHeight) / 2f - 2.5f) * d + dy
                drawRoundRect(
                    accent,
                    Offset(left + stroke / 2f, top + stroke / 2f),
                    Size((sz.cellWidth + 5f) * d - stroke, (sz.cellHeight + 5f) * d - stroke),
                    CornerRadius((sz.corner + 2f) * d - stroke / 2f),
                    style = Stroke(stroke),
                )
            }
        }
        // the tracks' names, standing still over the frames (a row several groups' tracks share
        // is named for the group the playhead is in)
        for (r in rows.list) {
            val t = r.track(tool.frame.toInt())
            Box(Modifier.offset(y = (r.y + place.inset - place.scroll).dp).height(r.height.dp)) {
                GridTrackLabel(tool, t, chosen = t.id == info.track, height = r.height, depth = r.depth, labels = labels)
            }
        }
    }
}

/** A tap on the rows at [at] (dp of the rows as they show): the playhead goes to the frame there on its track; a group's chevron folds it or unfolds it. */
private fun tapped(tool: TimelineTool, info: TimelineInfo, rows: GridRows, place: GridPlace, sz: GridSizes, at: Offset) {
    val p = place.inRows(at)
    val f = place.frame(p.x, sz) ?: return
    val r = rows.index(p.y) ?: return
    val row = rows[r]
    if (!row.has(f)) return
    val t = row.track(f)
    val found = t.clips.firstOrNull { f.toUInt() >= it.start && f.toUInt() < it.start + it.duration }
    if (found != null && found.kind == TimelineClipKind.GROUP && p.x - (place.origin + found.start.toFloat() * sz.pitch) < 30f) {
        tool.fold(found.id, tool.unfolded(found.id))
        return
    }
    if (t.id == info.track && f.toUInt() == tool.frame) return
    tool.selectTrack(t.id)
    if (f.toUInt() != tool.frame) tool.goTo(f.toUInt())
}

/** A track's name in its capsule, with its checkbox: tapped, the track is drawn on; its box tapped, it shows or hides. */
@Composable
private fun GridTrackLabel(tool: TimelineTool, track: TimelineTrackInfo, chosen: Boolean, height: Float, depth: Int, labels: Float) {
    val scale = height / GridSizes.ROW_HEIGHT
    val label = track.label?.let { rgb(it) }
    val text = if (label != null) Grid.OnLabel else if (chosen) Grid.InkBright else Grid.Ink
    val fill = when {
        label != null -> Grid.OnLabel
        track.soloed -> Pc.Accent
        chosen -> Grid.InkBright
        else -> Grid.Box
    }
    val capsule = CircleShape
    Box(Modifier.fillMaxSize().padding(start = (12f + depth * 14f * scale).dp), contentAlignment = Alignment.CenterStart) {
        Row(
            Modifier.graphicsLayer {
                scaleX = scale
                scaleY = scale
                transformOrigin = androidx.compose.ui.graphics.TransformOrigin(0f, 0.5f)
            }
                .alpha(if (track.hidden) 0.6f else 1f)
                .widthIn(max = (labels - 34f).coerceAtLeast(60f).dp)
                .height(32.dp)
                .clip(capsule)
                .background(label ?: if (chosen) Grid.Capsule else Color.Transparent)
                .then(if (chosen && label != null) Modifier.border(1.5.dp, Grid.InkBright.copy(alpha = 0.75f), capsule) else Modifier)
                .clickable(remember { MutableInteractionSource() }, indication = null) { tool.selectTrack(track.id) }
                .padding(start = 6.dp, end = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            Box(
                Modifier.size(26.dp, 30.dp).clickable(remember { MutableInteractionSource() }, indication = null) { tool.toggleHidden(track) }
                    .semantics { contentDescription = if (track.hidden) "Show ${track.name}" else "Hide ${track.name}" },
                contentAlignment = Alignment.Center,
            ) { TrackCheck(!track.hidden, fill) }
            Text(track.name, color = text, fontSize = 14.sp, fontWeight = FontWeight.Medium, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.weight(1f, fill = false))
            if (track.locked) Icon(PcIcons.Lock, contentDescription = "Locked", tint = text, modifier = Modifier.size(14.dp))
        }
    }
}

/** A track's visibility box: filled with a tick cut out of it while it shows, an outline while hidden. */
@Composable
private fun TrackCheck(checked: Boolean, fill: Color) {
    val shape = RoundedCornerShape(5.dp)
    val tick = rememberVectorPainter(PcIcons.Tick)
    Box(
        Modifier.size(18.dp)
            .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
            .clip(shape)
            .then(if (checked) Modifier.background(fill) else Modifier.border(1.5.dp, fill, shape))
            .drawWithContent {
                drawContent()
                if (!checked) return@drawWithContent
                val side = 14.dp.toPx()
                val inset = (size.width - side) / 2f
                val cut = androidx.compose.ui.graphics.Paint().apply { blendMode = BlendMode.DstOut }
                drawContext.canvas.saveLayer(Rect(Offset.Zero, size), cut)
                translate(inset, inset) { with(tick) { draw(Size(side, side)) } }
                drawContext.canvas.restore()
            },
    )
}

/** Whether clip [c] is on the frames [lo]..[hi]. */
internal fun TimelineClipInfo.within(lo: Int, hi: Int): Boolean = start.toInt() <= hi && (start + duration).toInt() > lo

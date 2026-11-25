package io.github.artcraftandroid.procraft.ui

import android.view.HapticFeedbackConstants
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
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
import androidx.compose.foundation.gestures.waitForUpOrCancellation
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
import androidx.compose.runtime.mutableStateListOf
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
import androidx.compose.ui.geometry.RoundRect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.translate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.rememberVectorPainter
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.PointerInputChange
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.util.VelocityTracker
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.platform.LocalView
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
import io.github.artcraftandroid.procraft.canvas.addFile
import io.github.artcraftandroid.procraft.canvas.addPicture
import io.github.artcraftandroid.procraft.canvas.setLasso
import io.github.artcraftandroid.procraft.canvas.setLength
import io.github.artcraftandroid.procraft.canvas.keyRows
import io.github.artcraftandroid.procraft.canvas.keyed
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineMediaKind
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
// The frame under the playhead, tapped, gets a handle on each edge: dragged, it lasts longer or
// shorter (TimelineGridEdges.kt). The lasso picks frames together with a stroke drawn across them.
// A frame, an empty frame, a track's name or the picked frames held open a menu standing on the
// bar (TimelineGridMenus.kt); a block held and dragged on goes along by frames and rows
// (TimelineGridTouch.kt). The flap on the ruler sets the timeline's length; `+` held, Add new.
// The rows of keys, their marks' menus, Animate and Perform are TimelineGridKeys.kt's and
// TimelineAnimate.kt's. Two fingers zoom the grid (TimelineGridZoom.kt).

/** The grid's fixed sizes (dp, the iPad's points) and colours; its cells' and rows' sizes, which zoom, are [GridSizes]. */
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

    /** A menu's card standing on the bar (the iPad's is see-through and blurred; this one is nearly solid). */
    val Card: Color get() = pick(0xF22C2C2C, 0xF5F2F2F2)
    val Danger = Color(0xFFFF453A)
}

/** A colour the core gives as 0xRRGGBB. */
internal fun rgb(packed: UInt): Color = Color(0xFF000000.toInt() or packed.toInt())

/** What the grid shows that isn't the timeline's own: where it is scrolled to while dragged, how far its rows are scrolled up, an edge or a block being dragged, the lasso's stroke, the menus open. */
@Stable
internal class GridState {
    /** The frame under the playhead while the frames are dragged (between frames too); none when they rest on the playhead's own. */
    var dragged by mutableStateOf<Float?>(null)

    /** How far the tracks are scrolled up, where there are more of them than show. */
    var rowsOffset by mutableFloatStateOf(0f)

    /** How far it is zoomed, and the pinch under way (TimelineGridZoom.kt); its cells' and rows' sizes as it is. */
    val zoom = GridZoom()
    val sizes: GridSizes get() = GridSizes(zoom.value)

    /** The key menu open, the key held, the keys being stretched (TimelineGridKeys.kt). */
    val keys = GridKeyState()

    /** A finger is dragging the frames sideways: the settling of the last drag leaves them be. */
    var scrolling = false
    var scrollFrom = 0f
    var scrubbed = 0u
    var settling: Job? = null

    /** An edge of the frame with handles being dragged, or of the frames picked together (see TimelineGridEdges.kt). */
    var trim by mutableStateOf<GridTrim?>(null)
    var pickedTrim by mutableStateOf<GridPickedTrim?>(null)

    /** The lasso's stroke under way (dp of all the rows), and the last one fading out. */
    val stroke = mutableStateListOf<Offset>()
    var faded by mutableStateOf<List<Offset>>(emptyList())
    var fadeAlpha by mutableFloatStateOf(1f)

    /** A block held, its menu open; a track's name held; the picked frames' menu (where along the bar it stands); an empty frame held; the end flap's menu; the Add new card. */
    var clipMenu by mutableStateOf<GridClipMenu?>(null)
    var trackMenu by mutableStateOf<ULong?>(null)
    var pickedMenu by mutableStateOf<Float?>(null)
    var emptyMenu by mutableStateOf<GridEmptySpot?>(null)
    var flapMenu by mutableStateOf<Float?>(null)
    var adding by mutableStateOf(false)

    /** The timeline's length as the end flap is dragged, before it is let go. */
    var lengthDraft by mutableStateOf<Int?>(null)

    /** A block being dragged somewhere else. */
    var move by mutableStateOf<GridMove?>(null)

    /** The frames the stroke under way has crossed, and what was picked as it began. */
    val touched = HashSet<ULong>()
    var strokeBefore: Set<ULong>? = null

    /** The block the finger down is holding, if it held one; and of a block dragged, which way the frames slide under it near an end of the bar, what slides them, and what works out again where it would land once they have. */
    var heldClip: ULong? = null
    var moveSlide = 0f
    var moveTask: Job? = null
    var moveAgain: (() -> Unit)? = null

    /** A buzz for what a touch does (the bar sets it). */
    var haptic: (GridHaptic) -> Unit = {}

    /** Every menu standing on the bar closed (the Keys card and the settings too). */
    fun closeMenus(tool: TimelineTool) {
        closeOwn()
        if (tool.keysCardFor != null) tool.keysCardFor = null
        if (tool.vm.timelineSettingsOpen) tool.vm.closeTimelineSettings()
    }

    /** The grid's own menus closed. */
    fun closeOwn() {
        if (clipMenu != null) clipMenu = null
        if (trackMenu != null) trackMenu = null
        if (pickedMenu != null) pickedMenu = null
        if (emptyMenu != null) emptyMenu = null
        if (flapMenu != null) flapMenu = null
        if (adding) adding = false
    }

    fun menuOpen(tool: TimelineTool): Boolean =
        clipMenu != null || trackMenu != null || pickedMenu != null || emptyMenu != null || flapMenu != null || adding || tool.keysCardFor != null

    /** The frames dragged sideways [translation] (dp) from where they were: the frame now under the playhead shows as it plays. */
    fun scrolled(translation: Float, last: Int, tool: TimelineTool) {
        if (dragged == null) {
            zoom.playOnly = null
            scrubbed = tool.frame
            tool.picked = null
        }
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
    val context = androidx.compose.ui.platform.LocalContext.current
    val zoomScope = rememberCoroutineScope()
    val grid = remember { GridState().also { it.zoom.attach(context, zoomScope) } }
    val view = LocalView.current
    grid.haptic = remember(view) {
        { kind ->
            view.performHapticFeedback(
                when (kind) {
                    GridHaptic.TICK -> HapticFeedbackConstants.CLOCK_TICK
                    GridHaptic.FIRM -> HapticFeedbackConstants.CONTEXT_CLICK
                    GridHaptic.LIGHT -> HapticFeedbackConstants.KEYBOARD_TAP
                    GridHaptic.HOLD -> HapticFeedbackConstants.LONG_PRESS
                    GridHaptic.WARN -> if (android.os.Build.VERSION.SDK_INT >= 30) HapticFeedbackConstants.REJECT else HapticFeedbackConstants.LONG_PRESS
                },
            )
        }
    }
    // Add new's picture or video from the photo picker, or a sound, picture or video from a file,
    // at the playhead (see TimelineMedia.kt)
    val photo = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri -> uri?.let { tool.addPicture(it) } }
    val video = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri -> uri?.let { tool.media.bringIn(it, TimelineMediaKind.VIDEO) } }
    val file = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> uri?.let { tool.addFile(it) } }
    // the animatic detector, opened from a video's menu
    tool.media.detecting?.let { AnimaticSheet(vm, tool, it) }
    // the grid's menus close as the settings' card opens
    LaunchedEffect(vm.timelineSettingsOpen) { if (vm.timelineSettingsOpen) grid.closeOwn() }
    BoxWithConstraints(Modifier.align(Alignment.BottomCenter).fillMaxSize()) {
        val width = min(maxWidth - 32.dp, 1180.dp)
        val sizes = grid.sizes
        // top first; the core's order is bottom first; under each track whose keys are out, its rows of keys
        val keyTracks = tool.gridKeyTracks
        val rows = remember(tool.rows, sizes, tool.keys, tool.lanes.volumeKeys, keyTracks) {
            GridRows(tool.rows.reversed(), sizes) { t -> if (t.id in keyTracks) tool.keyRows(t) else emptyList() }
        }
        // as tall as its rows are before the grid is zoomed out: zoomed out, the bar stays as tall
        // and the rows, smaller, stand in the middle of it
        val all = maxOf(rows.startHeight, GridSizes.ROW_HEIGHT)
        val most = maxOf(floor(maxHeight.value * 0.5f - Grid.HeaderHeight.value - Grid.RulerHeight.value - 12f), GridSizes.ROW_HEIGHT)
        val shown = minOf(all, most)
        // what is left above the bar for a menu standing on it
        val room = maxOf(maxHeight.value - shown - Grid.HeaderHeight.value - Grid.RulerHeight.value - 110f, 240f)
        val shape = RoundedCornerShape(28.dp)
        // the settings' card stands on the bar, clear of it; the bottom toolbars stand above it
        val density = androidx.compose.ui.platform.LocalDensity.current
        androidx.compose.runtime.DisposableEffect(Unit) {
            onDispose {
                vm.timelineSettingsLift = 0.dp
                tool.gridLift = 0.dp
            }
        }
        // a press anywhere else closes a menu standing on the bar
        if (grid.menuOpen(tool)) {
            Box(Modifier.fillMaxSize().pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume(); grid.closeMenus(tool) } })
        }
        Column(Modifier.align(Alignment.BottomCenter).padding(bottom = 19.dp).width(width)) {
            GridMenus(
                vm, tool, info, grid, width.value, room,
                onPhoto = { photo.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)) },
                onVideo = { video.launch(PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.VideoOnly)) },
                // sounds and videos while Timeline media's switches for them are on
                onFile = {
                    val kinds = listOfNotNull(
                        "audio/*".takeIf { tool.shows(io.github.artcraftandroid.procraft.core.Feature.SOUNDS) },
                        "image/*",
                        "video/*".takeIf { tool.shows(io.github.artcraftandroid.procraft.core.Feature.VIDEOS) },
                    )
                    file.launch(kinds.toTypedArray())
                },
            )
            Box {
                Column(
                    Modifier.fillMaxWidth()
                        .onSizeChanged {
                            val tall = with(density) { it.height.toDp() }
                            vm.timelineSettingsLift = 19.dp + tall + 10.dp
                            tool.gridLift = 19.dp + tall
                        }
                        .shadow(16.dp, shape)
                        .clip(shape)
                        .background(Grid.Panel)
                        // the whole bar takes its touches: none reach the canvas under it
                        .pointerInput(Unit) { detectTapGestures { } },
                ) {
                    GridHeader(vm, tool, grid)
                    Box(Modifier.fillMaxWidth().height(1.dp).background(Grid.Line))
                    GridBody(vm, tool, info, grid, rows, width, shown.dp)
                    Spacer(Modifier.height(9.dp))
                }
                // where the playhead is in time, over the bar's corner
                if (vm.timelineTimecode && !grid.menuOpen(tool)) GridTimecode(tool.frame.toInt(), info.fps.toInt(), Modifier.offset(x = 4.dp, y = (-36).dp))
            }
        }
        // a key's menu, or a block's Keys card, standing on the bar
        GridKeyCards(vm, grid.keys, maxWidth, width, lift = vm.timelineSettingsLift - 2.dp, room = maxHeight - vm.timelineSettingsLift - 70.dp)
    }
}

/** Where the playhead is, in time (Settings → Prefs → Timecode), over the bar's corner. */
@Composable
private fun GridTimecode(frame: Int, fps: Int, modifier: Modifier) {
    Box(
        modifier.height(28.dp).clip(CircleShape).background(Grid.Panel.copy(alpha = 0.82f)).padding(horizontal = 10.dp)
            .semantics { contentDescription = "Timecode" },
        contentAlignment = Alignment.Center,
    ) {
        Text(timelineTimecode(frame, fps), color = Grid.Ink, fontSize = 17.sp, fontFamily = androidx.compose.ui.text.font.FontFamily.Monospace, maxLines = 1)
    }
}

/** The bar's top row: Play, a frame back and on; then the settings, the lasso, a new track, a new frame (held, Add new). A tap beside its buttons closes a menu. */
@Composable
private fun GridHeader(vm: CanvasViewModel, tool: TimelineTool, grid: GridState) {
    Row(
        Modifier.fillMaxWidth().height(Grid.HeaderHeight)
            .pointerInput(Unit) { detectTapGestures { grid.closeMenus(tool) } }
            .padding(start = 26.dp, end = 16.dp, top = 4.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Box(
            Modifier.widthIn(min = 54.dp).height(40.dp)
                .clickable(remember { MutableInteractionSource() }, indication = null) {
                    grid.closeMenus(tool)
                    tool.picked = null
                    tool.togglePlaying()
                },
            contentAlignment = Alignment.CenterStart,
        ) {
            Text(if (tool.isPlaying) "Pause" else "Play", color = Grid.Ink, fontSize = 17.sp, fontWeight = FontWeight.Bold, maxLines = 1)
        }
        // with Animate on: a take armed, which the next touch on the canvas starts (a record
        // light, ringed while a take is armed), while the Timeline plugin's Performed moves is on
        if (tool.autokey && tool.shows(io.github.artcraftandroid.procraft.core.Feature.PERFORMED_MOVES)) {
            Box(
                Modifier.size(40.dp)
                    .semantics {
                        role = Role.Button
                        contentDescription = "Perform"
                    }
                    .clickable(remember { MutableInteractionSource() }, indication = null) {
                        tool.stopPlaying()
                        tool.performArmed = !tool.performArmed
                    },
                contentAlignment = Alignment.Center,
            ) {
                Box(Modifier.size(15.dp).clip(CircleShape).background(AutokeyRed))
                if (tool.performArmed) Box(Modifier.size(25.dp).border(2.dp, AutokeyRed, CircleShape))
            }
        }
        // tapped, a frame back or on; held, to the drawing before or after on the chosen track
        Spacer(Modifier.width(18.dp))
        GridIconButton(PcIcons.ChevronLeft, 21.dp, name = "Previous Frame", held = {
            grid.closeMenus(tool)
            tool.picked = null
            tool.stepDrawing(-1)
        }) {
            grid.closeMenus(tool)
            tool.stepFrame(-1)
        }
        GridIconButton(PcIcons.ChevronRight, 21.dp, name = "Next Frame", held = {
            grid.closeMenus(tool)
            tool.picked = null
            tool.stepDrawing(1)
        }) {
            grid.closeMenus(tool)
            tool.stepFrame(1)
        }
        Spacer(Modifier.weight(1f))
        GridIconButton(PcIcons.GridSettings, 27.dp, on = vm.timelineSettingsOpen, name = "Timeline Settings") {
            val open = !vm.timelineSettingsOpen
            grid.closeMenus(tool)
            if (open) tool.openSettings()
        }
        GridIconButton(GridIcons.Lasso, 27.dp, on = tool.selecting != null, name = "Select Frames") {
            grid.closeMenus(tool)
            tool.setLasso(tool.selecting == null)
        }
        // Animate: the transform tool keys what is under the playhead where it is put
        GridIconButton(KeyGlyphs.Key, 27.dp, on = tool.autokey, name = "Animate") {
            grid.closeMenus(tool)
            vm.setAutokey(!tool.autokey)
        }
        GridIconButton(PcIcons.GridTrack, 27.dp, name = "Add Track") {
            grid.closeMenus(tool)
            tool.picked = null
            tool.addTrack()
        }
        // tapped, a new frame; held a moment, the Add new card
        GridIconButton(PcIcons.Add, 38.dp, on = grid.adding, name = "Add Frame", holdFor = 200L, held = {
            grid.closeMenus(tool)
            tool.picked = null
            tool.stopPlaying()
            grid.adding = true
        }) {
            grid.closeMenus(tool)
            tool.picked = null
            tool.addDrawing()
        }
    }
}

/** One of the header's glyphs: lit in the accent while it is on, and for a moment when tapped; [held], what holding it [holdFor] ms does. */
@Composable
private fun GridIconButton(icon: ImageVector, size: Dp, on: Boolean = false, name: String, holdFor: Long = 400L, held: (() -> Unit)? = null, action: () -> Unit) {
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
            .pointerInput(holdFor) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    // held long enough (if holding does something), the lift is no tap
                    // (none: held that long; a list of none: taken away)
                    val lifted = if (hold != null) withTimeoutOrNull(holdFor) { listOf(waitForUpOrCancellation()) } else listOf(waitForUpOrCancellation())
                    if (lifted == null) {
                        haptics.performHapticFeedback(HapticFeedbackType.LongPress)
                        hold?.invoke()
                        do {
                            val e = awaitPointerEvent()
                            e.changes.forEach { it.consume() }
                        } while (e.changes.any { it.pressed })
                        return@awaitEachGesture
                    }
                    val up = lifted[0] ?: return@awaitEachGesture
                    up.consume()
                    flash = true
                    now()
                    scope.launch {
                        delay(180)
                        flash = false
                    }
                }
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
    val origin = sz.origin(playhead, at)
    val duration = info.duration
    // the frames the grid shows are the ones playing goes round (a group the grid was fitted to plays alone)
    SideEffect {
        tool.gridFrames = { from -> grid.zoom.playOnly ?: shownFrames(from.toFloat(), duration, playhead, w, labels, sz) }
    }
    // as far out as it zooms, the whole timeline shows whichever frame the playhead is on
    val least = GridZoom.least(duration.toInt(), playhead - labels - sz.cellWidth)
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
        if (grid.zoom.pinching) return@LaunchedEffect
        val row = rows.indexOf(info.track) ?: return@LaunchedEffect
        val top = rows[row].y
        val now = minOf(grid.rowsOffset, over)
        val next = minOf(maxOf(minOf(now, top), top + rows[row].height - h), over)
        if (abs(next - grid.rowsOffset) > 0.5f) grid.rowsOffset = maxOf(next, 0f)
    }
    // the handles stay with the frame they are on: gone when the playhead leaves it
    LaunchedEffect(tool.frame, tool.picked) {
        val c = tool.picked?.let { tool.clip(it) }?.second ?: return@LaunchedEffect
        if (tool.frame < c.start || tool.frame >= c.start + c.duration) tool.picked = null
    }
    // the lasso off: the picked frames' menu and handle go with it
    LaunchedEffect(tool.selecting == null) {
        if (tool.selecting == null) {
            grid.pickedMenu = null
            grid.pickedTrim = null
        }
    }
    // two fingers on the frames: zoomed and carried along
    val pinch by rememberUpdatedState(Triple(least, last, over))
    Column(
        Modifier.gridPinch(
            began = { grid.pinchBegan(tool) },
            changed = { scale, shift -> grid.pinchChanged(scale, shift, pinch.first, pinch.second, pinch.third, tool) },
            ended = { speed, opening -> grid.pinchEnded(speed, opening, pinch.first, pinch.second, tool) },
        ),
    ) {
        Box(Modifier.size(width, Grid.RulerHeight).clipToBounds()) {
            GridRuler(tool, grid, info, place, lo, hi, width)
            // the flap where the timeline ends
            GridEndFlap(tool, grid, info, place)
        }
        GridTracks(vm, tool, info, grid, rows, place, lo, hi, width, height, labels)
    }
}

/** The timeline's frames whose cells show whole right of the tracks' names, with frame [at] under the playhead; none of them in sight, none. */
private fun shownFrames(at: Float, frames: UInt, playhead: Float, width: Float, labels: Float, sz: GridSizes): UIntRange? {
    val origin = sz.origin(playhead, at)
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
        // the second of two fingers: theirs is the pinch's
        if (grid.zoom.pinching) return@awaitEachGesture
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
                // two fingers down: they zoom and move the grid, and this one alone does nothing more
                if (grid.zoom.pinching) touch = GridTouch.IGNORE
                velocity.addPosition(change.uptimeMillis, change.position)
                val moved = (change.position - down.position) / d
                when (touch) {
                    GridTouch.UNDECIDED -> if (hypot(moved.x, moved.y) > 6f) {
                        if (ruler || abs(moved.x) >= abs(moved.y) || p0.over <= 0f) {
                            touch = GridTouch.SCROLL
                            grid.closeMenus(tool)
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
                    else -> {}
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
                else -> {}
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
    val fps = maxOf(info.fps.toInt(), 1)
    val dim = Grid.InkDim
    val lit = if (tool.selecting != null) Pc.Accent else Grid.InkBright
    val latest by rememberUpdatedState(place)
    val scope = rememberCoroutineScope()
    Canvas(
        Modifier.size(width, Grid.RulerHeight).clipToBounds()
            .gridTouches(tool, grid, scope, { latest }, ruler = true, labels = 0f) { at ->
                val f = latest.frame(at.x, sz) ?: return@gridTouches
                if (f.toUInt() == tool.frame) return@gridTouches
                grid.closeMenus(tool)
                tool.picked = null
                tool.goTo(f.toUInt())
            },
    ) {
        val d = density
        val pitch = sz.pitch
        // every frame's number where they fit; zoomed out, fewer (or the seconds)
        val steps = RulerSteps.at(pitch, fps)
        val mid = size.height / 2f + 3f * d
        fun x(f: Int) = (place.origin + f * pitch + sz.cellWidth / 2f) * d
        fun write(text: String, f: Int, here: Boolean) {
            val style = TextStyle(color = if (here) lit else dim, fontSize = 11.sp, fontWeight = if (here) FontWeight.Bold else FontWeight.Normal, fontFeatureSettings = "tnum")
            val laid = measurer.measure(text, style)
            drawText(laid, topLeft = Offset(x(f) - laid.size.width / 2f, mid - laid.size.height / 2f))
        }
        fun tick(f: Int, tall: Float) = drawRect(dim.copy(alpha = 0.7f), Offset(x(f) - 0.5f * d, size.height - (tall + 2f) * d), Size(1f * d, tall * d))
        // (a number too near the playhead's own gives way to it)
        fun clear(f: Int) = abs((f - current) * pitch) >= 30f
        if (steps.seconds) {
            // a tick each second (each half where there is room), a number where they fit
            var f = (lo / steps.tick) * steps.tick
            while (f <= hi) {
                if (f >= lo && f != current) {
                    val second = if (f % fps == 0) f / fps else null
                    if (second != null && second % steps.apart == 0) {
                        if (clear(f)) write("${second}s", f, false)
                    } else {
                        tick(f, if (second != null) 5f else 3f)
                    }
                }
                f += steps.tick
            }
        } else {
            for (f in lo..hi) {
                if (f == current) continue
                if ((f + 1) % steps.every == 0) {
                    if (steps.every == 1 || clear(f)) write("${f + 1}", f, false)
                } else if (pitch >= 5f) {
                    tick(f, 3f)
                }
            }
        }
        if (current in lo..hi) write("${current + 1}", current, true)
    }
}

/**
 * The tracks' rows: the cells, the frames past the end shaded, an empty frame held ringed, the
 * playhead's outline, an edge held at a limit, the line a block would be dropped on, the lasso's
 * stroke, and the tracks' names standing still over them.
 */
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
    // (the touches' handler stays as it began: what it reads is read as the grid was last laid out)
    val input = remember(tool, grid) { GridRowsInput(tool, grid) }
    input.rows = rows
    input.place = place
    input.info = info
    input.width = width.value
    input.labels = labels
    // the keys' own touches (their marks held, dragged and stretched), asked first
    input.keys = remember(tool, grid) { GridKeyTouch(tool, grid.keys, { input.rows }, { input.place }, { grid.sizes }) }
    val scope = rememberCoroutineScope()
    val all = maxOf(rows.height, sz.rowHeight)
    val chosenRow = rows.indexOf(info.track)
    // while it plays, where the playhead's outline stands as the grid holds still under it
    val head = if (tool.isPlaying) place.origin + tool.frame.toFloat() * sz.pitch else null
    // (a frame with its handles out, or the frames picked together, are outlined instead)
    val outlined = tool.selecting == null && tool.picked?.let { tool.clip(it) } == null
    val accent = Pc.Accent
    val panel = Grid.Panel
    val line = Grid.Line
    val ring = Grid.InkBright.copy(alpha = 0.85f)
    val frames = grid.lengthDraft ?: info.duration.toInt()
    Box(
        Modifier.size(width, height).clipToBounds()
            .gridRowTouches(input, scope),
    ) {
        GridCells(vm, tool, grid, rows, place, lo, hi, sz, labels, Modifier.fillMaxSize())
        GridKeyRows(tool, rows, place, lo, hi, sz, labels, Modifier.fillMaxSize())
        Canvas(Modifier.fillMaxSize()) {
            val d = density
            val dy = (place.inset - place.scroll) * d
            // past the timeline's end: shaded, since nothing there plays
            val end = (place.origin + frames.toFloat() * sz.pitch - sz.gap / 2f) * d
            if (end < size.width) {
                val from = maxOf(end, 0f)
                drawRect(panel.copy(alpha = 0.55f), Offset(from, dy), Size(size.width - from, all * d))
                drawRect(line, Offset(from, dy), Size(1f * d, all * d))
            }
            // an empty frame held for its menu: ringed
            grid.emptyMenu?.let { spot ->
                val r = rows.indexOf(spot.track) ?: return@let
                val stroke = 2f * d
                val left = (place.origin + spot.frame.toFloat() * sz.pitch - 2.5f) * d
                val top = (rows[r].y + (rows[r].height - sz.cellHeight) / 2f - 2.5f) * d + dy
                drawRoundRect(
                    ring,
                    Offset(left + stroke / 2f, top + stroke / 2f),
                    Size((sz.cellWidth + 5f) * d - stroke, (sz.cellHeight + 5f) * d - stroke),
                    CornerRadius((sz.corner + 2f) * d - stroke / 2f),
                    style = Stroke(stroke),
                )
            }
            // the playhead: an outline on the chosen track's frame, where it stands whatever slides under it
            if (chosenRow != null && outlined) {
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
            // a dragged edge held at a limit: a line down the tracks there
            (grid.trim?.limit ?: grid.pickedTrim?.limit)?.let { limit ->
                drawRect(accent, Offset((place.origin + limit * sz.pitch - 0.75f) * d, dy), Size(1.5f * d, all * d))
            }
            // a dragged block over the line between two rows: dropped there, a track of its own
            grid.move?.between?.let { between ->
                val y = between.y * d + dy
                drawLine(
                    accent, Offset((labels - 40f) * d, y), Offset(size.width, y), strokeWidth = 2f * d, cap = StrokeCap.Round,
                    pathEffect = PathEffect.dashPathEffect(floatArrayOf(7f * d, 6f * d)),
                )
            }
            // the lasso's stroke: glowing while it is drawn, fading once lifted
            fun lasso(points: List<Offset>, alpha: Float) {
                if (points.size < 2 || alpha <= 0f) return
                val path = Path()
                path.moveTo(points[0].x * d, points[0].y * d + dy)
                for (i in 1 until points.size) path.lineTo(points[i].x * d, points[i].y * d + dy)
                drawPath(path, accent.copy(alpha = 0.3f * alpha), style = Stroke(11f * d, cap = StrokeCap.Round, join = StrokeJoin.Round))
                drawPath(path, accent.copy(alpha = alpha), style = Stroke(5f * d, cap = StrokeCap.Round, join = StrokeJoin.Round))
                drawPath(path, Color.White.copy(alpha = 0.9f * alpha), style = Stroke(2f * d, cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
            lasso(grid.stroke, 1f)
            lasso(grid.faded, grid.fadeAlpha)
        }
        // the tracks' names, standing still over the frames (a row several groups' tracks share
        // is named for the group the playhead is in)
        for (r in rows.list) {
            val t = r.track(tool.frame.toInt())
            Box(Modifier.offset(y = (r.y + place.inset - place.scroll).dp).height(r.height.dp)) {
                if (r.lanes.isNotEmpty()) {
                    GridKeyLabel(tool, r.lanes, r.depth)
                } else {
                    GridTrackLabel(tool, grid, t, chosen = t.id == info.track, height = r.height, depth = r.depth, labels = labels)
                }
            }
        }
    }
}

/**
 * The flap on the ruler where the timeline ends: dragged, the timeline is longer or shorter (what
 * is left past its end stays, shaded, and neither plays nor is exported); tapped, its menu opens.
 */
@Composable
private fun GridEndFlap(tool: TimelineTool, grid: GridState, info: TimelineInfo, place: GridPlace) {
    val sz = grid.sizes
    val frames = grid.lengthDraft ?: info.duration.toInt()
    val color = if (grid.lengthDraft != null || grid.flapMenu != null) Pc.Accent else Grid.Box
    val panel = Grid.Panel
    val x = place.origin + frames * sz.pitch - sz.gap / 2f
    val latestX by rememberUpdatedState(x)
    val duration by rememberUpdatedState(info.duration.toInt())
    Canvas(
        Modifier.offset(x = (x - 9f).dp).size(30.dp, Grid.RulerHeight)
            .semantics {
                role = Role.Button
                contentDescription = "Timeline end, $frames frames"
            }
            .pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    var from: Int? = null
                    try {
                        while (true) {
                            val e = awaitPointerEvent()
                            val c = e.changes.firstOrNull { it.id == down.id } ?: break
                            if (!c.pressed) {
                                c.consume()
                                val length = grid.lengthDraft
                                if (from == null) {
                                    // tapped: its menu
                                    val open = grid.flapMenu == null
                                    grid.closeMenus(tool)
                                    if (open) grid.flapMenu = latestX
                                } else if (length != null && length != duration) {
                                    tool.setLength(length)
                                }
                                break
                            }
                            val dx = (c.position.x - down.position.x) / density
                            if (from == null && abs(dx) >= 4f) {
                                from = duration
                                tool.stopPlaying()
                                grid.closeMenus(tool)
                            }
                            from?.let { f ->
                                val next = maxOf(f + (dx / sz.pitch).roundToInt(), 1)
                                if (next != grid.lengthDraft) {
                                    grid.lengthDraft = next
                                    grid.haptic(GridHaptic.TICK)
                                }
                            }
                            c.consume()
                        }
                    } finally {
                        grid.lengthDraft = null
                    }
                }
            },
    ) {
        val d = density
        // its pole, and the flap flying from the top of it
        drawRect(color, Offset(8f * d, 0f), Size(2f * d, size.height))
        val flap = Path().apply {
            addRoundRect(
                RoundRect(
                    Rect(Offset(10f * d, 2f * d), Size(13f * d, 13f * d)),
                    topLeft = CornerRadius.Zero,
                    topRight = CornerRadius(5f * d),
                    bottomRight = CornerRadius(5f * d),
                    bottomLeft = CornerRadius.Zero,
                ),
            )
        }
        drawPath(flap, color)
        for (i in 0..1) {
            drawRoundRect(panel, Offset(13.5f * d, (6.5f + i * 4f) * d), Size(6f * d, 1.5f * d), CornerRadius(0.75f * d))
        }
    }
}

/** A track's name in its capsule, with its checkbox: tapped, the track is drawn on; its box tapped, it shows or hides; held, its menu opens. */
@Composable
private fun GridTrackLabel(tool: TimelineTool, grid: GridState, track: TimelineTrackInfo, chosen: Boolean, height: Float, depth: Int, labels: Float) {
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
                .pointerInput(track.id) {
                    detectTapGestures(
                        onTap = {
                            grid.closeMenus(tool)
                            tool.selectTrack(track.id)
                        },
                        onLongPress = {
                            if (!tool.isPlaying) {
                                grid.closeMenus(tool)
                                grid.trackMenu = track.id
                                grid.haptic(GridHaptic.HOLD)
                            }
                        },
                    )
                }
                .padding(start = 6.dp, end = if (tool.keyed(track)) 6.dp else 14.dp),
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
            // its blocks have keys: their rows put out under it, or away
            if (tool.keyed(track)) GridKeysChevron(tool, track, text)
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

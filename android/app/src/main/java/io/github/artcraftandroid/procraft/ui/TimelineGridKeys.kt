package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.SideEffect
import androidx.compose.runtime.Stable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.draw.drawWithContent
import androidx.compose.ui.draw.shadow
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.BlendMode
import androidx.compose.ui.graphics.Brush
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.CompositingStrategy
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.rotate
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.hapticfeedback.HapticFeedbackType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalHapticFeedback
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.KeyLaneRow
import io.github.artcraftandroid.procraft.canvas.SavedCurves
import io.github.artcraftandroid.procraft.canvas.TimelineTool
import io.github.artcraftandroid.procraft.canvas.bringPlayhead
import io.github.artcraftandroid.procraft.canvas.clip
import io.github.artcraftandroid.procraft.canvas.copyKeys
import io.github.artcraftandroid.procraft.canvas.easing
import io.github.artcraftandroid.procraft.canvas.key
import io.github.artcraftandroid.procraft.canvas.keyLaneKeys
import io.github.artcraftandroid.procraft.canvas.pasteKeys
import io.github.artcraftandroid.procraft.canvas.pose
import io.github.artcraftandroid.procraft.canvas.removeKeys
import io.github.artcraftandroid.procraft.canvas.setEasing
import io.github.artcraftandroid.procraft.canvas.setKey
import io.github.artcraftandroid.procraft.canvas.shownKeys
import io.github.artcraftandroid.procraft.canvas.toggleKey
import io.github.artcraftandroid.procraft.canvas.toggleKeyRows
import io.github.artcraftandroid.procraft.core.AdjustmentUnit
import io.github.artcraftandroid.procraft.core.AdjustmentValue
import io.github.artcraftandroid.procraft.core.EasingKind
import io.github.artcraftandroid.procraft.core.TimelineChannel
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelineEasing
import io.github.artcraftandroid.procraft.core.TimelineKeyEdit
import io.github.artcraftandroid.procraft.core.TimelineKeyLane
import io.github.artcraftandroid.procraft.core.TimelineKeyPick
import io.github.artcraftandroid.procraft.core.TimelineTrackInfo
import io.github.artcraftandroid.procraft.core.defaultEasing
import kotlin.math.abs
import kotlin.math.log10
import kotlin.math.pow
import kotlin.math.roundToInt
import kotlin.math.sqrt

// Keys on the grid timeline, as the iPad's TimelineGridKeys.swift has them (the model is
// canvas/TimelineKeyLanes.kt). A block's keyed frames have a mark under them (TimelineGridCells.kt).
// A track whose blocks have keys has a chevron after its name: turned, its rows of keys stand under
// it, a mark on each keyed frame (a diamond, a square where it holds, a dot where it eases). A mark
// tapped is picked and the playhead goes to it; held, its menu (how it eases, with the easing editor
// and My curves; copy, paste, reverse, delete); held and dragged, it and the keys picked go along by
// frames; keys picked on two frames or more have a grip either side, dragged to spread them out or
// close them up in time about the far end. A block's Keys card (opened from a frame's or a block's
// menu through `TimelineTool.keysCardFor`) keys its position, scale, rotation and opacity on the
// playhead's frame by slider, and an adjustment frame's own values.

/** A key held, its menu open, and where along the bar (dp) it stands. */
internal data class GridKeySpot(val key: TimelineKeyPick, val x: Float)

/** What the grid's keys show that isn't the timeline's own: the key menu open, the key held, the keys being stretched. */
@Stable
internal class GridKeyState {
    var menu by mutableStateOf<GridKeySpot?>(null)

    /** The key the finger down is holding, if it held one. */
    var heldKey: TimelineKeyPick? = null

    /** The keys picked being stretched in time: the frame they stretch about, and the frame the grip dragged began on. */
    var stretching: Pair<Int, Int>? = null
}

/** The glyphs the keys use (the iPad's grid icons), drawn here, in a 24 square. */
internal object KeyGlyphs {
    val Key by lazy { glyph("grid-key", "M12 4.2L19.8 12L12 19.8L4.2 12z" to 1.9f, "M12 9L15 12L12 15L9 12z" to 0f) }
    val Blend by lazy { glyph("grid-blend", "M6 5H18A2 2 0 0 1 20 7V17A2 2 0 0 1 18 19H6A2 2 0 0 1 4 17V7A2 2 0 0 1 6 5z" to 1.7f, "M12 8.6A3.4 3.4 0 0 1 12 15.4A3.4 3.4 0 0 1 12 8.6z" to 0f) }
    val Copy by lazy {
        glyph(
            "grid-copy",
            "M10 8.5H17A1.5 1.5 0 0 1 18.5 10V18A1.5 1.5 0 0 1 17 19.5H10A1.5 1.5 0 0 1 8.5 18V10A1.5 1.5 0 0 1 10 8.5z" to 1.6f,
            "M15.5 8.5V6A1.5 1.5 0 0 0 14 4.5H7A1.5 1.5 0 0 0 5.5 6V14A1.5 1.5 0 0 0 7 15.5H8.5" to 1.6f,
        )
    }
    val Paste by lazy {
        glyph(
            "grid-paste",
            "M9 6H7.5A1.5 1.5 0 0 0 6 7.5V18A1.5 1.5 0 0 0 7.5 19.5H16.5A1.5 1.5 0 0 0 18 18V7.5A1.5 1.5 0 0 0 16.5 6H15" to 1.6f,
            "M10 4.5H14A1 1 0 0 1 15 5.5V6.5A1 1 0 0 1 14 7.5H10A1 1 0 0 1 9 6.5V5.5A1 1 0 0 1 10 4.5z" to 1.6f,
        )
    }
    val Flip by lazy { glyph("grid-flip", "M5 9H19" to 1.7f, "M15.5 5.5L19 9L15.5 12.5" to 1.7f, "M19 15H5" to 1.7f, "M8.5 11.5L5 15L8.5 18.5" to 1.7f) }
    val Options by lazy {
        glyph("grid-options", "M6 5.5H18A2 2 0 0 1 20 7.5V16.5A2 2 0 0 1 18 18.5H6A2 2 0 0 1 4 16.5V7.5A2 2 0 0 1 6 5.5z" to 1.7f, "M8 10H16" to 1.7f, "M8 14H13" to 1.7f)
    }

    /** Parts each stroked as wide as given, or filled (0). */
    private fun glyph(name: String, vararg parts: Pair<String, Float>): ImageVector {
        val b = ImageVector.Builder(name = name, defaultWidth = 24.dp, defaultHeight = 24.dp, viewportWidth = 24f, viewportHeight = 24f)
        for ((d, w) in parts) {
            if (w > 0f) {
                b.addPath(addPathNodes(d), stroke = SolidColor(Color.Black), strokeLineWidth = w, strokeLineCap = StrokeCap.Round, strokeLineJoin = StrokeJoin.Round)
            } else {
                b.addPath(addPathNodes(d), fill = SolidColor(Color.Black))
            }
        }
        return b.build()
    }
}

/** Animate's colour, a recording light's. */
internal val AutokeyRed = Color(0xFFFF453A)

private val MarkInk get() = if (Pc.light) Color(0xFF393939) else Color(0xFFC5C5C5)
private val MarkInkDim get() = if (Pc.light) Color(0x80393939) else Color(0x80C5C5C5)
private val KeyCardFill get() = if (Pc.light) Color(0xF5F2F2F2) else Color(0xF52C2C2C)
private val KeyCardEdge get() = if (Pc.light) Color(0xFFD6D6D6) else Color(0xFF3C3C3C)
private val KeyDanger = Color(0xFFFF453A)
private val KeyMenuWidth = 228.dp
private val KeysCardWidth = 262.dp
private val KeyCurveCardWidth = 320.dp

// ---- the rows ----

/**
 * The rows of keys over the cells, in the frames' own space as they stand at [place]: a strip for
 * each, a mark on each keyed frame; and with keys on two frames or more picked, a grip either side.
 * They fade out under the tracks' names, as the cells do.
 */
@Composable
internal fun GridKeyRows(tool: TimelineTool, rows: GridRows, place: GridPlace, lo: Int, hi: Int, sz: GridSizes, labels: Float, modifier: Modifier) {
    if (rows.list.none { it.lanes.isNotEmpty() }) return
    val slot = Grid.Slot
    val accent = Pc.Accent
    val ink = MarkInk
    val dim = MarkInkDim
    val picked = tool.lanes.shownPicks
    val here = tool.frame
    // (read here, so the marks are drawn again as a drag moves them)
    tool.lanes.preview
    Canvas(
        modifier
            .graphicsLayer { compositingStrategy = CompositingStrategy.Offscreen }
            .drawWithContent {
                drawContent()
                drawRect(
                    Brush.horizontalGradient(0f to Color.Transparent, 0.45f to Color.Transparent, 1f to Color.Black, startX = 0f, endX = labels * density),
                    blendMode = BlendMode.DstIn,
                )
            },
    ) {
        val d = density
        val dy = place.inset - place.scroll
        fun x(f: Int) = (place.origin + f * sz.pitch) * d
        for (row in rows.list) {
            if (row.lanes.isEmpty()) continue
            val from = row.segments.firstOrNull()?.let { maxOf(it.start, lo) } ?: lo
            if (from <= hi) {
                drawRoundRect(
                    slot.copy(alpha = 0.55f),
                    Offset(x(from), (row.y + 3f + dy) * d),
                    Size((hi - from + 1) * sz.pitch * d, (row.height - 6f) * d),
                    CornerRadius(5f * d),
                )
            }
            val cy = (row.y + row.height / 2f + dy) * d
            for (lane in row.lanes) {
                for (k in tool.shownKeys(lane)) {
                    val f = k.frame.toInt()
                    if (f < lo || f > hi) continue
                    val on = TimelineKeyPick(k.lane, k.frame) in picked
                    keyMark(Offset(x(f) + sz.cellWidth / 2f * d, cy), k.easing, fill = if (on) accent else if (k.frame == here) ink else dim, edge = on)
                }
            }
        }
        keyGrips(tool, rows)?.let { g ->
            for (leading in listOf(true, false)) {
                val gx = (place.origin + g.x(leading, sz)) * d
                drawRoundRect(accent, Offset(gx - 3f * d, (g.top + 4f + dy) * d), Size(6f * d, (g.bottom - g.top - 8f) * d), CornerRadius(3f * d))
            }
        }
    }
}

/** A key's mark at [at]: a diamond, a square where it holds, a dot where it eases; ringed in white while picked. */
private fun DrawScope.keyMark(at: Offset, easing: TimelineEasing, fill: Color, edge: Boolean) {
    val d = density
    val ring = 1.5f * d
    when (easing) {
        TimelineEasing.HOLD -> {
            val side = 9f * d
            drawRect(fill, at - Offset(side / 2f, side / 2f), Size(side, side))
            if (edge) drawRect(Color.White, at - Offset(side / 2f - ring / 2f, side / 2f - ring / 2f), Size(side - ring, side - ring), style = Stroke(ring))
        }
        TimelineEasing.SMOOTH, TimelineEasing.EASE_IN, TimelineEasing.EASE_OUT, TimelineEasing.EASE_IN_OUT -> {
            val r = 5f * d
            drawCircle(fill, r, at)
            if (edge) drawCircle(Color.White, r - ring / 2f, at, style = Stroke(ring))
        }
        TimelineEasing.LINEAR -> rotate(45f, at) {
            val side = 9f * d
            drawRect(fill, at - Offset(side / 2f, side / 2f), Size(side, side))
            if (edge) drawRect(Color.White, at - Offset(side / 2f - ring / 2f, side / 2f - ring / 2f), Size(side - ring, side - ring), style = Stroke(ring))
        }
    }
}

/** Where the grips stand beside the keys picked: the first and last frames they are on, and the rows of keys from top to bottom (dp of all the rows). */
internal class KeyGrips(val first: Int, val last: Int, val top: Float, val bottom: Float) {
    /** A grip's middle, in the frames' space: a little clear of the keys' own marks. */
    fun x(leading: Boolean, sz: GridSizes): Float =
        if (leading) first * sz.pitch + sz.cellWidth / 2f - 18f else last * sz.pitch + sz.cellWidth / 2f + 18f

    /** Which grip is at [p] (the frames' space across, the rows' down), if one is: the first's (true) or the last's. */
    fun grip(p: Offset, sz: GridSizes): Boolean? {
        if (p.y < top || p.y >= bottom) return null
        if (abs(p.x - x(false, sz)) <= 11f) return false
        return if (abs(p.x - x(true, sz)) <= 11f) true else null
    }
}

internal fun keyGrips(tool: TimelineTool, rows: GridRows): KeyGrips? {
    val frames = tool.lanes.shownPicks.map { it.frame.toInt() }
    val keyed = rows.list.filter { it.lanes.isNotEmpty() }
    val first = frames.minOrNull() ?: return null
    val last = frames.maxOrNull() ?: return null
    if (first >= last || keyed.isEmpty()) return null
    return KeyGrips(first, last, keyed.first().y, keyed.last().let { it.y + it.height })
}

/** A row of keys' name, in the names' column: tapped, all its keys are picked, or none. */
@Composable
internal fun GridKeyLabel(tool: TimelineTool, lanes: List<KeyLaneRow>, depth: Int) {
    val keys = lanes.flatMap { tool.keyLaneKeys(it) }
    val all = keys.isNotEmpty() && tool.lanes.picks.containsAll(keys.map { TimelineKeyPick(it.lane, it.frame) })
    Box(Modifier.fillMaxSize().padding(start = (50f + depth * 14f).dp), contentAlignment = Alignment.CenterStart) {
        Box(
            Modifier.fillMaxHeight()
                .clickable(remember { MutableInteractionSource() }, indication = null) { tool.lanes.toggleAll(keys) }
                .semantics { role = Role.Button }
                .padding(end = 8.dp),
            contentAlignment = Alignment.CenterStart,
        ) {
            Text(lanes.firstOrNull()?.said.orEmpty(), color = if (all) Pc.Accent else Grid.Ink, fontSize = 11.5.sp, fontWeight = FontWeight.Medium, maxLines = 1)
        }
    }
}

/** A track's chevron after its name, where its blocks have keys: tapped, their rows put out under it, or away. */
@Composable
internal fun GridKeysChevron(tool: TimelineTool, track: TimelineTrackInfo, ink: Color) {
    val out = track.id in tool.gridKeyTracks
    Box(
        Modifier.size(22.dp, 30.dp)
            .clickable(remember { MutableInteractionSource() }, indication = null) { tool.toggleKeyRows(track.id) }
            .semantics {
                role = Role.Button
                contentDescription = if (out) "Hide keys" else "Show keys"
            },
        contentAlignment = Alignment.Center,
    ) {
        Icon(if (out) PcIcons.ChevronDown else PcIcons.ChevronRight, contentDescription = null, tint = if (out) Pc.Accent else ink, modifier = Modifier.size(13.dp))
    }
}

// ---- touches ----

/**
 * What a finger on the rows does to keys (the grid's one touch handler asks it, TimelineGrid.kt):
 * down on a grip, the keys picked stretch; held on a mark, its menu, and dragged on from there, it
 * and the keys picked go along; a tap on a row of keys picks the key there or lets them all go.
 * Points are dp of the rows as they show.
 */
internal class GridKeyTouch(
    private val tool: TimelineTool,
    private val keys: GridKeyState,
    private val rows: () -> GridRows,
    private val place: () -> GridPlace,
    private val sizes: () -> GridSizes,
) {
    /** Down on a grip beside the keys picked: they will stretch about the far end. */
    fun grip(at: Offset): Boolean {
        val p = place()
        val inRows = p.inRows(at)
        val g = keyGrips(tool, rows()) ?: return false
        val leading = g.grip(Offset(inRows.x - p.origin, inRows.y), sizes()) ?: return false
        keys.stretching = if (leading) g.last to g.first else g.first to g.last
        keys.menu = null
        return true
    }

    /** The key at [at], on a row of keys, if there is one; and whether [at] is on a row of keys at all. */
    private fun keyAt(at: Offset): Pair<Boolean, TimelineKeyPick?> {
        val p = place()
        val inRows = p.inRows(at)
        val all = rows()
        val r = all.index(inRows.y) ?: return false to null
        val row = all[r]
        if (row.lanes.isEmpty()) return false to null
        val f = p.frame(inRows.x, sizes()) ?: return true to null
        return true to tool.key(row.lanes, f)
    }

    /** A finger held still at [at]: on a mark, its menu opens (with the keys picked, if it is one of them). Whether it did. */
    fun hold(at: Offset): Boolean {
        if (tool.isPlaying) return false
        val key = keyAt(at).second ?: return false
        tool.lanes.take(key)
        keys.heldKey = key
        keys.menu = GridKeySpot(key, at.x)
        tool.lanes.tick(false)
        return true
    }

    /** Dragged on from a hold, [dx] (dp) from where it came down: the key held and the keys picked go along by frames. */
    fun dragged(dx: Float) {
        val key = keys.heldKey ?: return
        if (keys.menu != null) keys.menu = null
        tool.lanes.drag(key, (dx / sizes().pitch).roundToInt())
    }

    /** A grip dragged [dx] (dp): as far from the far end as the finger has taken it, against how far it was. */
    fun stretched(dx: Float) {
        val (anchor, from) = keys.stretching ?: return
        if (from == anchor) return
        val now = from + dx / sizes().pitch
        val factor = ((now - anchor) / (from - anchor)).toDouble()
        tool.lanes.stretch(anchor.toUInt(), maxOf(factor, 0.05))
    }

    /** A drag of keys let go ([lifted]) or taken away: done as one undo step, or nothing. */
    fun ended(lifted: Boolean) {
        if (lifted) tool.lanes.endDrag() else tool.lanes.cancelDrag()
        keys.stretching = null
        keys.heldKey = null
    }

    /** The finger lifted after a hold that did nothing more: the menu stays. */
    fun letGo() {
        keys.heldKey = null
    }

    /** A tap at [at]: on a row of keys, the key there picked with the others or let go; elsewhere on it, all let go. Whether it was on one. */
    fun tapped(at: Offset): Boolean {
        val (onKeys, key) = keyAt(at)
        if (!onKeys) return false
        if (key != null) tool.lanes.tap(key) else tool.lanes.clear()
        return true
    }
}

// ---- what stands on the bar ----

/**
 * The key menu and the Keys card, standing on the bar ([barWidth] wide, in the middle of this box)
 * [lift] over the box's bottom; a press anywhere else closes them.
 */
@Composable
internal fun BoxScope.GridKeyCards(vm: CanvasViewModel, keys: GridKeyState, boxWidth: Dp, barWidth: Dp, lift: Dp, room: Dp) {
    val tool = vm.timeline
    val haptics = LocalHapticFeedback.current
    SideEffect {
        tool.lanes.tick = { warning ->
            haptics.performHapticFeedback(if (warning) HapticFeedbackType.LongPress else HapticFeedbackType.TextHandleMove)
        }
    }
    val spot = keys.menu
    val card = tool.keysCardFor?.let { tool.clip(it) }
    // its block's keys go on the playhead's frame: the playhead comes to it
    LaunchedEffect(tool.keysCardFor) { card?.second?.let { tool.bringPlayhead(it) } }
    if (spot == null && card == null) return
    fun close() {
        keys.menu = null
        tool.keysCardFor = null
    }
    Box(Modifier.matchParentSize().pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume(); close() } })
    val barLeft = (boxWidth - barWidth) / 2
    if (spot != null) {
        var beside by remember(spot.key) { mutableStateOf<KeyBeside?>(null) }
        val besideWidth = when (beside) {
            KeyBeside.EASING -> KeyMenuWidth + 6.dp
            KeyBeside.CURVE -> KeyCurveCardWidth + 6.dp
            null -> 0.dp
        }
        val total = KeyMenuWidth + besideWidth
        val left = (spot.x.dp - KeyMenuWidth / 2).coerceIn(0.dp, maxOf(barWidth - total, 0.dp))
        Row(
            Modifier.align(Alignment.BottomStart).offset(x = barLeft + left).padding(bottom = lift),
            verticalAlignment = Alignment.Bottom,
            horizontalArrangement = Arrangement.spacedBy(6.dp),
        ) {
            KeyMenu(tool, spot.key, beside, room, open = { beside = it }, close = { close() })
        }
    } else if (card != null) {
        val left = (barWidth / 2 - KeysCardWidth / 2).coerceAtLeast(0.dp)
        Box(Modifier.align(Alignment.BottomStart).offset(x = barLeft + left).padding(bottom = lift)) {
            KeysCard(vm, card.second, room) { close() }
        }
    }
}

/** What a key's menu opens beside it: the easings, or the easing editor. */
private enum class KeyBeside { EASING, CURVE }

/** The simple easings a key's menu offers, by name. */
private val KeyEasings = listOf(
    TimelineEasing.LINEAR to "Linear", TimelineEasing.HOLD to "Hold", TimelineEasing.SMOOTH to "Smooth",
    TimelineEasing.EASE_IN to "Ease in", TimelineEasing.EASE_OUT to "Ease out", TimelineEasing.EASE_IN_OUT to "Ease in and out",
)

/** A key's menu (and the other keys picked with it): how they ease into the next, copying and pasting, playing them backwards, taking them away. */
@Composable
private fun KeyMenu(tool: TimelineTool, key: TimelineKeyPick, beside: KeyBeside?, room: Dp, open: (KeyBeside?) -> Unit, close: () -> Unit) {
    // (read again whenever the timeline is: an easing set shows as it now is)
    tool.revision
    val several = tool.lanes.shown.size > 1
    val clip = (key.lane as? TimelineKeyLane.Clip)?.id
    // how it eases into the next now
    val now = when (val lane = key.lane) {
        is TimelineKeyLane.Clip -> tool.keys[lane.id]?.firstOrNull { it.frame == key.frame && it.channel == lane.channel }?.easing
        is TimelineKeyLane.Volume -> tool.lanes.volumeKeys[lane.id]?.firstOrNull { it.frame == key.frame }?.easing
        TimelineKeyLane.Camera -> null
    }
    KeyCard(KeyMenuWidth) {
        KeyMenuRow(KeyGlyphs.Blend, "Easing", chevron = true, lit = beside != null) { open(if (beside == null) KeyBeside.EASING else null) }
        KeyMenuLine()
        if (clip != null) {
            KeyMenuRow(KeyGlyphs.Copy, "Copy keys") {
                close()
                tool.goTo(key.frame)
                tool.copyKeys(clip, here = true)
            }
            KeyMenuRow(KeyGlyphs.Paste, "Paste keys", dim = !tool.keysCopied) {
                close()
                tool.pasteKeys()
            }
        }
        KeyMenuRow(KeyGlyphs.Flip, "Reverse", dim = !several) {
            close()
            tool.lanes.apply(TimelineKeyEdit.Reverse)
        }
        KeyMenuLine()
        KeyMenuRow(PcIcons.Trash, if (several) "Delete keys" else "Delete key", tint = KeyDanger) {
            close()
            tool.lanes.apply(TimelineKeyEdit.Remove)
        }
    }
    when (beside) {
        KeyBeside.EASING -> KeyCard(KeyMenuWidth) {
            for ((easing, name) in KeyEasings) {
                KeyChoiceRow(name, chosen = now == easing) { tool.lanes.apply(TimelineKeyEdit.Ease(easing)) }
            }
            if (clip != null) {
                // (the curves kept under the easing editor, for this key)
                val saved = SavedCurves.of(LocalContext.current)
                if (saved.all.isNotEmpty()) {
                    KeyMenuLine()
                    for (choice in saved.easingChoices(tool.easing(clip, key.frame))) {
                        KeyChoiceRow(choice.name, chosen = choice.chosen) { tool.setEasing(clip, key.frame, choice.spec, done = true) }
                    }
                }
                KeyMenuLine()
                KeyMenuRow(KeyGlyphs.Options, "Edit curve…", chevron = true) {
                    tool.goTo(key.frame)
                    open(KeyBeside.CURVE)
                }
            }
        }
        KeyBeside.CURVE -> if (clip != null) {
            KeyCard(KeyCurveCardWidth) {
                Column(Modifier.fillMaxWidth().height(minOf(maxOf(room - 60.dp, 220.dp), 520.dp)).verticalScroll(rememberScrollState()).padding(horizontal = 10.dp)) {
                    EasingEditor(
                        easing = tool.easing(clip, key.frame) ?: defaultEasing(EasingKind.LINEAR),
                        onChange = { tool.setEasing(clip, key.frame, it, done = false) },
                        onDone = { tool.setEasing(clip, key.frame, it, done = true) },
                        onBack = { open(KeyBeside.EASING) },
                    )
                }
            }
        }
        null -> {}
    }
}

/**
 * A block's Keys card: a key of each kind on the playhead's frame put there or taken away; where
 * it is there, set by slider (each keys it there); an adjustment frame's own values; a sound's
 * level there; its keys copied, pasted or all taken away.
 */
@Composable
private fun KeysCard(vm: CanvasViewModel, clip: TimelineClipInfo, room: Dp, close: () -> Unit) {
    val tool = vm.timeline
    // (read again whenever the timeline is)
    val keys = tool.keys[clip.id].orEmpty()
    val frame = tool.frame
    val here = keys.filter { it.frame == frame }
    val inside = frame >= clip.start && frame < clip.start + clip.duration
    val sound = clip.kind == TimelineClipKind.AUDIO
    KeyCard(KeysCardWidth) {
        Column(Modifier.heightIn(max = maxOf(room, 200.dp)).verticalScroll(rememberScrollState())) {
            Text(
                "Keys on frame ${frame.toInt() + 1}", color = Grid.InkDim, fontSize = 12.sp, fontWeight = FontWeight.Medium,
                modifier = Modifier.padding(start = 16.dp, top = 2.dp, bottom = 4.dp),
            )
            if (sound) {
                LevelRows(vm, clip, inside)
            } else {
                val kinds = listOf(TimelineChannel.POSITION to "Key position here", TimelineChannel.SCALE to "Key scale here", TimelineChannel.ROTATION to "Key rotation here")
                for ((channel, label) in kinds) {
                    Box(Modifier.graphicsLayer { alpha = if (inside) 1f else 0.45f }) {
                        KeyChoiceRow(label, chosen = here.any { it.channel == channel }) { if (inside) tool.toggleKey(clip.id, channel) }
                    }
                }
                // where it is on the playhead's frame, set by hand: each keys it there
                val pose = remember(tool.revision, frame) { tool.pose(clip.id) }
                if (pose != null) {
                    val size = remember(vm.session) { vm.session?.info() }
                    val w = (size?.width ?: 1u).toFloat()
                    val h = (size?.height ?: 1u).toFloat()
                    Column(Modifier.graphicsLayer { alpha = if (inside) 1f else 0.45f }) {
                        KeyMenuSlider("Move across", (pose.x / w + 1f) / 2f, enabled = inside, text = { "${((it * 2f - 1f) * w).roundToInt()} px" }) { v, done ->
                            tool.setKey(clip.id, TimelineChannel.POSITION, listOf((v * 2f - 1f) * w, pose.y), done)
                        }
                        KeyMenuSlider("Move down", (pose.y / h + 1f) / 2f, enabled = inside, text = { "${((it * 2f - 1f) * h).roundToInt()} px" }) { v, done ->
                            tool.setKey(clip.id, TimelineChannel.POSITION, listOf(pose.x, (v * 2f - 1f) * h), done)
                        }
                        // (a tenth to ten times its size, evenly by ratio)
                        KeyMenuSlider(
                            "Scale", log10(pose.scaleX.coerceIn(0.1f, 10f)) / 2f + 0.5f, enabled = inside,
                            text = { "${(10f.pow((it - 0.5f) * 2f) * 100f).roundToInt()} %" },
                        ) { v, done ->
                            val s = 10f.pow((v - 0.5f) * 2f)
                            tool.setKey(clip.id, TimelineChannel.SCALE, listOf(s, s), done)
                        }
                        KeyMenuSlider("Rotation", (pose.rotation + 360f) / 720f, enabled = inside, text = { "${(it * 720f - 360f).roundToInt()}°" }) { v, done ->
                            tool.setKey(clip.id, TimelineChannel.ROTATION, listOf(v * 720f - 360f), done)
                        }
                        KeyMenuSlider("Opacity", pose.opacity, enabled = inside) { v, done ->
                            tool.setKey(clip.id, TimelineChannel.OPACITY, listOf(v), done)
                        }
                    }
                }
                // an adjustment frame's own values, each keyed there as it is slid
                AdjustmentKeyRows(vm, clip.id, frame)
            }
            KeyMenuLine()
            KeyMenuRow(KeyGlyphs.Copy, "Copy keys", dim = keys.isEmpty()) {
                close()
                tool.copyKeys(clip.id, here = false)
            }
            KeyMenuRow(KeyGlyphs.Paste, "Paste keys", dim = !tool.keysCopied) {
                close()
                tool.pasteKeys()
            }
            KeyMenuRow(PcIcons.Trash, "Remove keys here", tint = KeyDanger, dim = here.isEmpty()) {
                tool.removeKeys(clip.id)
            }
        }
    }
}

/** A sound's level on the playhead's frame: slid, it is keyed there; and its key there put there or taken away. */
@Composable
private fun LevelRows(vm: CanvasViewModel, clip: TimelineClipInfo, inside: Boolean) {
    val tool = vm.timeline
    val frame = tool.frame
    val volumeKeys = tool.lanes.volumeKeys[clip.id].orEmpty()
    val keyedHere = volumeKeys.any { it.frame == frame }
    val level = remember(tool.revision, frame) { vm.session?.timelineMediaClip(clip.id)?.level } ?: return
    Column(Modifier.graphicsLayer { alpha = if (inside) 1f else 0.45f }) {
        KeyChoiceRow("Key level here", chosen = keyedHere) {
            if (!inside) return@KeyChoiceRow
            tool.stopPlaying()
            if (keyedHere) vm.session?.removeTimelineVolumeKey(clip.id) else vm.session?.setTimelineVolumeKey(clip.id, level, true)
            vm.refresh()
        }
        KeyMenuSlider("Level here", level.coerceIn(0f, 1f), enabled = inside) { v, done ->
            tool.stopPlaying()
            vm.session?.setTimelineVolumeKey(clip.id, v, done)
            if (done) vm.refresh()
        }
    }
}

/**
 * An adjustment clip's values at the playhead, a plain row each, as the iPad's AdjustmentKeyRows: a
 * row slid keys the whole adjustment there as it moves (one undo step when let go), a new key
 * easing as new keys do.
 */
@Composable
private fun AdjustmentKeyRows(vm: CanvasViewModel, clip: ULong, frame: UInt) {
    val tool = vm.timeline
    // the row being slid and its value, before it is let go
    var live by remember(frame) { mutableStateOf<Pair<Int, Float>?>(null) }
    val all = remember(tool.revision, frame, clip) { vm.session?.timelineAdjustmentValues(clip).orEmpty() }
    // (curves' keyed values are its exposure's, an HDR canvas's only)
    val values = if (vm.isHdr || all.firstOrNull()?.unit != AdjustmentUnit.STOPS) all else emptyList()
    if (values.isEmpty()) return
    KeyMenuLine()
    values.forEachIndexed { index, v ->
        val shown = live?.takeIf { it.first == index }?.second ?: v.value
        val span = v.high - v.low
        KeyMenuSlider(v.name, if (span > 0f) (shown - v.low) / span else 0f, text = { said(v, v.low + it * span) }) { s, done ->
            val value = v.low + s * span
            tool.stopPlaying()
            vm.session?.setTimelineAdjustmentValue(clip, index.toUInt(), value, tool.keyEasing, done)
            if (done) {
                live = null
                vm.refresh()
            } else {
                live = index to value
            }
        }
    }
}

/** An adjustment value as its row says it. */
private fun said(v: AdjustmentValue, value: Float): String = when (v.unit) {
    AdjustmentUnit.PLAIN -> "%.2f".format(java.util.Locale.ROOT, value)
    AdjustmentUnit.PIXELS -> "${value.roundToInt()} px"
    AdjustmentUnit.PERCENT -> "${(value * 100f).roundToInt()} %"
    AdjustmentUnit.DEGREES -> "${(value * 180f / Math.PI.toFloat()).roundToInt()}°"
    AdjustmentUnit.STOPS -> "%+.1f stops".format(java.util.Locale.ROOT, value)
}

// ---- the menus' pieces ----

/** A menu's card: its presses go no further. */
@Composable
private fun KeyCard(width: Dp, content: @Composable ColumnScope.() -> Unit) {
    val shape = RoundedCornerShape(22.dp)
    Column(
        Modifier.width(width).shadow(18.dp, shape).clip(shape).background(KeyCardFill).border(1.dp, KeyCardEdge, shape)
            .pointerInput(Unit) { detectTapGestures { } }
            .padding(vertical = 8.dp),
        content = content,
    )
}

@Composable
private fun KeyMenuLine() {
    Box(Modifier.padding(horizontal = 14.dp, vertical = 6.dp).fillMaxWidth().height(1.dp).background(Grid.Line.copy(alpha = 0.7f)))
}

/** A row of a menu: a glyph and what it does; dimmed where it can't. */
@Composable
private fun KeyMenuRow(icon: ImageVector?, label: String, tint: Color? = null, chevron: Boolean = false, dim: Boolean = false, lit: Boolean = false, action: () -> Unit) {
    val color = if (dim) Grid.InkDim else if (lit) Pc.Accent else tint ?: Grid.InkBright
    Row(
        Modifier.fillMaxWidth().height(36.dp)
            .clickable(remember { MutableInteractionSource() }, indication = null) { if (!dim) action() }
            .semantics { role = Role.Button }
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(10.dp),
    ) {
        if (icon != null) Box(Modifier.width(22.dp), contentAlignment = Alignment.Center) { Icon(icon, contentDescription = null, tint = color, modifier = Modifier.size(20.dp)) }
        Text(label, color = color, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
        if (chevron) Icon(PcIcons.ChevronRight, contentDescription = null, tint = color, modifier = Modifier.size(15.dp))
    }
}

/** One of a list to choose from: the one chosen ticked. */
@Composable
private fun KeyChoiceRow(label: String, chosen: Boolean, action: () -> Unit) {
    Row(
        Modifier.fillMaxWidth().height(34.dp)
            .clickable(remember { MutableInteractionSource() }, indication = null, onClick = action)
            .semantics { role = Role.Button }
            .padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
    ) {
        Text(label, color = Grid.InkBright, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
        if (chosen) {
            Box(Modifier.size(18.dp).clip(CircleShape).background(Pc.Accent), contentAlignment = Alignment.Center) {
                Icon(PcIcons.Tick, contentDescription = null, tint = Color.White, modifier = Modifier.size(13.dp))
            }
        }
    }
}

/** A slider in a menu: its name and value over a thin track, dragged or tapped anywhere along it; [changed] with the value and whether it was let go there. */
@Composable
private fun KeyMenuSlider(label: String, value: Float, enabled: Boolean = true, text: ((Float) -> String)? = null, changed: (Float, Boolean) -> Unit) {
    var live by remember { mutableStateOf<Float?>(null) }
    val shown = live ?: value
    val change by rememberUpdatedState(changed)
    val on by rememberUpdatedState(enabled)
    Column(Modifier.fillMaxWidth().padding(start = 16.dp, end = 16.dp, top = 6.dp, bottom = 2.dp)) {
        Row(Modifier.fillMaxWidth()) {
            Text(label, color = Grid.InkBright, fontSize = 15.sp, maxLines = 1, modifier = Modifier.weight(1f))
            Text(text?.invoke(shown) ?: "${(shown * 100f).roundToInt()} %", color = Grid.InkBright, fontSize = 15.sp, maxLines = 1)
        }
        Spacer(Modifier.height(4.dp))
        val accent = Pc.Accent
        val track = Grid.Line
        Box(
            Modifier.fillMaxWidth().height(22.dp)
                .pointerInput(Unit) {
                    detectTapGestures {
                        if (!on) return@detectTapGestures
                        change((it.x / size.width).coerceIn(0f, 1f), true)
                    }
                }
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(
                        onDragStart = {
                            if (!on) return@detectHorizontalDragGestures
                            val v = (it.x / size.width).coerceIn(0f, 1f)
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
                        if (!on) return@detectHorizontalDragGestures
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

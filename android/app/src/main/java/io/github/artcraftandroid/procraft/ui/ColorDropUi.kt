package io.github.artcraftandroid.procraft.ui

import android.os.SystemClock
import androidx.compose.animation.core.Animatable
import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.runtime.withFrameMillis
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.composed
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.clipToBounds
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.graphicsLayer
import androidx.compose.ui.input.pointer.PointerType
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onGloballyPositioned
import androidx.compose.ui.layout.positionInRoot
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.role
import androidx.compose.ui.semantics.selected
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel.DropBand
import kotlin.math.roundToInt

/** What a touch on a colour that can be dragged out turned out to be. */
private enum class DropTouch { TAP, HOLD, DRAG, GONE }

/**
 * A colour that can be dragged out onto the canvas to fill there (the colour button, or a swatch
 * with [swatch] giving its colour): tapped it does [onTap], held still [onHold]; dragged, its colour
 * is pulled out (of the colour button's centre, or where the swatch was pressed) and goes with the
 * finger, filling where it is let go below the top bar.
 */
internal fun Modifier.dropSource(vm: CanvasViewModel, swatch: (() -> Int)? = null, onHold: (() -> Unit)? = null, onTap: () -> Unit): Modifier = composed {
    val tap by rememberUpdatedState(onTap)
    val hold by rememberUpdatedState(onHold)
    val colour by rememberUpdatedState(swatch)
    var origin by remember { mutableStateOf(Offset.Zero) }
    var centre by remember { mutableStateOf(Offset.Zero) }
    // on the canvas: in the window, less what the interface keeps clear of
    val corner = LocalClearance.current.corner
    onGloballyPositioned {
        origin = it.positionInRoot() - corner
        centre = origin + Offset(it.size.width / 2f, it.size.height / 2f)
        // the colour button's centre: over the bars a dragged colour shrinks the further it is from it
        if (colour == null) vm.dropButton = centre
    }.pointerInput(vm) {
        awaitEachGesture {
            val down = awaitFirstDown()
            down.consume()
            // whichever comes first: lifted, moved past the slop, or (when holding does something) held
            val wait = if (hold != null) viewConfiguration.longPressTimeoutMillis else Long.MAX_VALUE
            val touch = withTimeoutOrNull(wait) {
                var seen: DropTouch? = null
                while (seen == null) {
                    val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id }
                    seen = when {
                        c == null -> DropTouch.GONE
                        !c.pressed -> DropTouch.TAP.also { c.consume() }
                        (c.position - down.position).getDistance() > viewConfiguration.touchSlop -> DropTouch.DRAG
                        else -> null
                    }
                }
                seen
            } ?: DropTouch.HOLD
            when (touch) {
                DropTouch.TAP -> tap()
                DropTouch.HOLD -> hold?.invoke()
                DropTouch.GONE -> Unit
                DropTouch.DRAG -> {
                    var at = origin + down.position
                    val pen = down.type == PointerType.Stylus || down.type == PointerType.Eraser
                    val swatchRgb = colour?.invoke()
                    vm.dropDragStart(at, swatchRgb, pen, down.uptimeMillis, from = if (swatchRgb == null) centre else origin + down.position)
                    var lifted = false
                    while (true) {
                        // held still a moment, the colour comes to rest (a slide then sets the threshold)
                        val due = vm.dropRestDue()
                        val event = if (due == null) {
                            awaitPointerEvent()
                        } else {
                            withTimeoutOrNull((due - SystemClock.uptimeMillis()).coerceAtLeast(1L)) { awaitPointerEvent() }
                        }
                        if (event == null) {
                            vm.dropDragRest(SystemClock.uptimeMillis())
                            continue
                        }
                        val c = event.changes.firstOrNull { it.id == down.id } ?: break
                        c.consume()
                        at = origin + c.position
                        if (!c.pressed) {
                            lifted = true
                            break
                        }
                        vm.dropDragMove(at, c.uptimeMillis)
                    }
                    // let go on the top bar, it is put back
                    vm.dropDragEnd(at.takeIf { lifted && it.y > Pc.barShown.value * vm.density })
                }
            }
        }
    }
}

/**
 * The colour being dragged out, as the core's [io.github.artcraftandroid.procraft.core.ColourBlob]
 * works it out each frame: a drop pulled out of the colour button that lets go and follows the
 * finger as a ball, springing and wobbling on its own.
 */
@Composable
fun DropDot(vm: CanvasViewModel) {
    val blob = vm.dropBlob ?: return
    val rgb = vm.dropDot?.second ?: return
    var shape by remember(blob) { mutableStateOf<io.github.artcraftandroid.procraft.core.BlobShape?>(null) }
    LaunchedEffect(blob) {
        while (true) withFrameMillis { shape = blob.shape(SystemClock.uptimeMillis().toDouble()) }
    }
    val outlines = shape?.outlines ?: return
    val colour = CanvasSpace.color(rgb)
    val scale = vm.density
    Canvas(Modifier.fillMaxSize()) {
        // points to pixels; the outlines filled together, where they cross too
        val path = Path()
        for (outline in outlines) {
            val p = outline.points
            if (p.size < 6) continue
            path.moveTo(p[0] * scale, p[1] * scale)
            var i = 2
            while (i + 1 < p.size) {
                path.lineTo(p[i] * scale, p[i + 1] * scale)
                i += 2
            }
            path.close()
        }
        drawPath(path, colour)
    }
}

/**
 * ColorDrop's band under the top bar, sliding down from behind it: while a colour is dragged and
 * while filling goes on, with the threshold as a blue line across the screen under it; after a
 * drop, for a moment, offering to continue filling. Sliding the threshold adds a row reading it.
 * Its segmented control picks what a colour fills up to: the current layer (or the reference
 * layer) or All Layers.
 */
@Composable
fun DropBandUi(vm: CanvasViewModel) {
    val band = vm.dropBand
    val shown = remember { Animatable(0f) }
    // what it showed, kept while it slides away
    val kept = remember { arrayOf(DropBand.NONE) }
    if (band != DropBand.NONE) kept[0] = band
    LaunchedEffect(band == DropBand.NONE) {
        if (band == DropBand.NONE) shown.animateTo(0f, Motion.noticeOut()) else shown.animateTo(1f, Motion.noticeIn())
    }
    if (band == DropBand.NONE && shown.value <= 0f) return
    val what = kept[0]
    val ink = if (Pc.light) Color(0xFF222222) else Color(0xFFC5C5C5)
    val blue = Pc.tinted(if (Pc.light) Color(0xFF0A8CFD) else Color(0xFF0076FD))
    BoxWithConstraints(Modifier.offset(y = Pc.barShown).fillMaxWidth().clipToBounds()) {
        val width = maxWidth
        Column(Modifier.graphicsLayer { translationY = -(1f - shown.value) * size.height }) {
            Box(Modifier.fillMaxWidth().height(32.dp).background(if (Pc.light) BandLight else BandDark), contentAlignment = Alignment.Center) {
                Row(verticalAlignment = Alignment.CenterVertically) {
                    // on a narrow screen the control and button have the band to themselves
                    if (width >= BandRoomForTitle) {
                        Text("ColorDrop", color = ink, fontSize = 16.5.sp, fontWeight = FontWeight.Bold, maxLines = 1)
                        Spacer(Modifier.width(13.dp))
                    }
                    Segmented(
                        listOf(if (vm.dropOnReference) "Reference" else "Layer", "All Layers"),
                        chosen = if (vm.dropAllLayers) 1 else 0, ink = ink, blue = blue,
                    ) { vm.chooseDropAllLayers(it == 1) }
                    Spacer(Modifier.width(13.dp))
                    if (what == DropBand.FILLING) {
                        // the tick ends filling
                        Box(Modifier.size(20.dp).clip(CircleShape).background(blue).press { vm.endFilling() }) {
                            Canvas(Modifier.size(20.dp)) {
                                val u = size.width / 20f
                                val tick = Path().apply {
                                    moveTo(10f * u - 4.2f * u, 10f * u + 0.1f * u)
                                    lineTo(10f * u - 1.4f * u, 10f * u + 2.6f * u)
                                    lineTo(10f * u + 4f * u, 10f * u - 2.8f * u)
                                }
                                drawPath(tick, Color.White, style = Stroke(2f * u, cap = StrokeCap.Round, join = StrokeJoin.Round))
                            }
                        }
                    } else {
                        Box(
                            Modifier.size(101.dp, 20.dp).clip(RoundedCornerShape(10.dp)).background(blue).press { vm.continueFilling() },
                            contentAlignment = Alignment.Center,
                        ) {
                            Text("Continue filling", color = Color.White, fontSize = 11.7.sp, maxLines = 1)
                        }
                    }
                }
            }
            if (vm.dropSliding) {
                Box(Modifier.fillMaxWidth().height(24.dp).background(if (Pc.light) RowLight else RowDark), contentAlignment = Alignment.Center) {
                    Text("Threshold ${(vm.dropThreshold * 100).roundToInt()}%", color = ink, fontSize = 14.4.sp, maxLines = 1)
                }
            }
            if (what == DropBand.DRAGGING || what == DropBand.FILLING) {
                Box(Modifier.width(width * vm.dropThreshold).height(4.dp).background(blue))
            }
        }
    }
}

/**
 * The Eraser band under the top bar, sliding down from behind it as ColorDrop's does: while the
 * Erase tool is up on a vector layer, how the eraser works there — taking what its brush covers
 * (Subtract), whole strokes, or a stroke up to where it crosses others. It stays while erasing.
 */
@Composable
fun EraserBandUi(vm: CanvasViewModel) {
    val on = vm.eraserBandShown
    val shown = remember { Animatable(0f) }
    LaunchedEffect(on) {
        if (on) shown.animateTo(1f, Motion.noticeIn()) else shown.animateTo(0f, Motion.noticeOut())
    }
    if (!on && shown.value <= 0f) return
    val ink = if (Pc.light) Color(0xFF222222) else Color(0xFFC5C5C5)
    val blue = Pc.tinted(if (Pc.light) Color(0xFF0A8CFD) else Color(0xFF0076FD))
    val modes = io.github.artcraftandroid.procraft.core.VectorEraser.entries
    BoxWithConstraints(Modifier.offset(y = Pc.BarHeight).fillMaxWidth().clipToBounds()) {
        val width = maxWidth
        val named = width >= EraserRoomForNames
        Box(
            Modifier.graphicsLayer { translationY = -(1f - shown.value) * size.height }
                .fillMaxWidth().height(32.dp).background(if (Pc.light) BandLight else BandDark),
            contentAlignment = Alignment.Center,
        ) {
            Row(verticalAlignment = Alignment.CenterVertically) {
                // on a narrow screen the control has the band to itself
                if (width >= EraserRoomForTitle) {
                    Text("Eraser", color = ink, fontSize = 16.5.sp, fontWeight = FontWeight.Bold, maxLines = 1)
                    Spacer(Modifier.width(13.dp))
                }
                Segmented(
                    if (named) listOf("Subtract", "Whole Stroke", "To Crossing") else listOf("Subtract", "Stroke", "Crossing"),
                    chosen = modes.indexOf(vm.vectorEraser).coerceAtLeast(0),
                    segment = if (named) EraserSegmentWidth else SegmentWidth, ink = ink, blue = blue,
                    enabled = on,
                ) { vm.chooseVectorEraser(modes[it]) }
            }
        }
    }
}

/**
 * A band's choices side by side in a rounded track, the chosen one under a blue thumb that slides
 * across to another when it is tapped.
 */
@Composable
private fun Segmented(
    labels: List<String>,
    chosen: Int,
    segment: Dp = SegmentWidth,
    ink: Color,
    blue: Color,
    enabled: Boolean = true,
    choose: (Int) -> Unit,
) {
    val at by animateFloatAsState(chosen.toFloat(), Motion.switchKnob(), label = "segment")
    Box(Modifier.size(segment * labels.size, 20.dp).clip(RoundedCornerShape(10.dp)).background(if (Pc.light) TrackLight else TrackDark)) {
        Box(Modifier.offset(x = segment * at).size(segment, 20.dp).clip(RoundedCornerShape(10.dp)).background(blue))
        Row {
            for ((index, label) in labels.withIndex()) {
                val on = index == chosen
                Box(
                    Modifier.size(segment, 20.dp)
                        .semantics {
                            role = Role.Tab
                            selected = on
                        }
                        .press { if (enabled) choose(index) },
                    contentAlignment = Alignment.Center,
                ) {
                    Text(label, color = if (on) Color.White else ink, fontSize = 11.7.sp, maxLines = 1)
                }
            }
        }
    }
}

/** Each of the Eraser band's segments: room for "Whole Stroke" and "To Crossing". */
private val EraserSegmentWidth = 84.dp

/** Under this, the Eraser band leaves its title out for the segmented control. */
private val EraserRoomForTitle: Dp = 352.dp

/** Under this, the Eraser band's segments take shorter names. */
private val EraserRoomForNames: Dp = 268.dp

/** Each of the band's segments, room for "All Layers" and "Reference". */
private val SegmentWidth = 68.dp

/** Under this, the band leaves its title out for the segmented control. */
private val BandRoomForTitle: Dp = 380.dp

/** The segmented control's track, dark and light. */
private val TrackDark = Color(0x33FFFFFF)
private val TrackLight = Color(0x1F000000)

/** A tap target without a ripple. */
private fun Modifier.press(onClick: () -> Unit): Modifier = composed {
    clickable(remember { MutableInteractionSource() }, indication = null, onClick = onClick)
}

/** The band, translucent over the canvas, dark and light. */
private val BandDark = Color(0xD63B3B3B)
private val BandLight = Color(0xD6EEEEEE)

/** The threshold's row under it. */
private val RowDark = Color(0xCC4A4A4A)
private val RowLight = Color(0xCCDCDCDC)

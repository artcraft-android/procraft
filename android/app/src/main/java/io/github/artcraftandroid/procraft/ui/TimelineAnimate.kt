package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.interaction.MutableInteractionSource
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.PointerId
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.input.pointer.positionChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.moveKey
import io.github.artcraftandroid.procraft.canvas.motionPath
import io.github.artcraftandroid.procraft.canvas.pathClip
import io.github.artcraftandroid.procraft.canvas.pose
import io.github.artcraftandroid.procraft.canvas.setHandles
import io.github.artcraftandroid.procraft.core.MotionPath
import io.github.artcraftandroid.procraft.core.TimelinePose
import kotlin.math.PI
import kotlin.math.atan2
import kotlin.math.hypot
import kotlin.math.roundToInt

// What Animate puts over the canvas, as the iPad's TimelineAutokey.swift, GridPerformOverlay
// (TimelineGridKeys.swift) and MotionPathOverlay (TimelineBar.swift) have it: the badge saying which
// frame the transform keys (tapped, Animate goes off); with a take armed, the pad that takes the
// next touch on the canvas to perform a move; and the motion path of the block under the playhead
// while Animate is on or its track's rows of keys are out.

/** Over the canvas while the transform keys a pose: the frame it keys. Tapped, Animate goes off (the transform keying ends, its keys kept). */
@Composable
fun BoxScope.AutokeyBadge(vm: CanvasViewModel) {
    // (the box read here, so the badge is drawn again as it moves)
    if (vm.transformBox == null || !vm.timeline.on) return
    val posing = vm.session?.timelineAutokeyPosing() == true
    if (!posing) return
    val top = Modifier.align(Alignment.TopCenter).padding(top = Pc.barShown + 10.dp)
    if (vm.timeline.autokey) {
        Row(
            top.height(28.dp).clip(CircleShape).background(AutokeyRed)
                .clickable(remember { MutableInteractionSource() }, indication = null) { vm.setAutokey(false) }
                .padding(horizontal = 12.dp),
            verticalAlignment = Alignment.CenterVertically,
            horizontalArrangement = Arrangement.spacedBy(7.dp),
        ) {
            Box(Modifier.size(7.dp).clip(CircleShape).background(Color.White))
            Text("Keying frame ${vm.timeline.frame.toInt() + 1}", color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
            Box(Modifier.width(1.dp).height(14.dp).background(Color.White.copy(alpha = 0.5f)))
            Text("Turn Off", color = Color.White, fontSize = 14.sp)
        }
    } else {
        // a video placed with Animate off: where it is put holds for its whole length
        Box(top.height(28.dp).clip(CircleShape).background(Color(0xE63A3A3C)).padding(horizontal = 12.dp), contentAlignment = Alignment.Center) {
            Text("Placing the video for its whole length", color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.SemiBold)
        }
    }
}

/**
 * Over the canvas while a take is armed: the first touch starts it (see TimelinePerform.kt), the
 * finger taking the block under the playhead along; a second finger makes it bigger or smaller and
 * turns it; the first lifted, the take ends.
 */
@Composable
fun BoxScope.PerformOverlay(vm: CanvasViewModel) {
    val tool = vm.timeline
    if (!tool.performArmed || !tool.on || !tool.shows(io.github.artcraftandroid.procraft.core.Feature.PERFORMED_MOVES)) return
    val perform = tool.perform
    Box(
        Modifier.fillMaxSize().pointerInput(Unit) {
            awaitEachGesture {
                val down = awaitFirstDown()
                down.consume()
                if (!perform.begin()) return@awaitEachGesture
                val first = down.id
                val start = down.position
                var second: PointerId? = null
                // the two fingers as the second came down: how far apart, and the line between them's
                // angle; how far it has turned since, counted through the half turn; and the scale
                var spread = 1f
                var angle = 0f
                var turned = 0f
                var scale: Float? = null
                try {
                    while (true) {
                        val event = awaitPointerEvent()
                        val a = event.changes.firstOrNull { it.id == first } ?: break
                        if (!a.pressed) break
                        val b = second?.let { id -> event.changes.firstOrNull { it.id == id && it.pressed } }
                        if (second != null && b == null) second = null
                        if (second == null) {
                            event.changes.firstOrNull { it.id != first && it.pressed && !it.previousPressed }?.let { c ->
                                second = c.id
                                // (from how they are now: what a finger lifted and put down again changed stays)
                                spread = maxOf(hypot(c.position.x - a.position.x, c.position.y - a.position.y), 1f) / (scale ?: 1f)
                                angle = atan2(c.position.y - a.position.y, c.position.x - a.position.x)
                            }
                        }
                        val pair = second?.let { id -> event.changes.firstOrNull { it.id == id } }
                        if (pair != null) {
                            val now = atan2(pair.position.y - a.position.y, pair.position.x - a.position.x)
                            var step = now - angle
                            while (step > PI) step -= (2 * PI).toFloat()
                            while (step < -PI) step += (2 * PI).toFloat()
                            turned += step
                            angle = now
                            scale = maxOf(hypot(pair.position.x - a.position.x, pair.position.y - a.position.y), 1f) / spread
                        }
                        // how far the finger has gone, in the canvas's own pixels
                        val from = vm.windowToCanvas(start)
                        val to = vm.windowToCanvas(a.position)
                        val moved = if (from != null && to != null) to - from else Offset.Zero
                        perform.changed(moved, scale, scale?.let { turned * 180f / PI.toFloat() })
                        event.changes.forEach { it.consume() }
                    }
                } finally {
                    perform.end()
                }
            }
        },
    ) {
        Box(
            Modifier.align(Alignment.TopCenter).padding(top = Pc.barShown + 46.dp).height(28.dp).clip(CircleShape).background(AutokeyRed).padding(horizontal = 12.dp),
            contentAlignment = Alignment.Center,
        ) {
            Text(
                if (perform.going) "Performing…" else "Touch the canvas and drag to perform the move",
                color = Color.White, fontSize = 14.sp, fontWeight = FontWeight.SemiBold,
            )
        }
    }
}

/**
 * The motion path of the block under the playhead over the canvas: the path its pivot takes, a dot
 * at each position key, and a bent key's two handles on lines to it. A key's dot is tapped to go to
 * its frame or dragged to move the key there; a handle's is dragged to bend the path, the other
 * mirroring it. Only the dots take touches: the canvas keeps the rest.
 */
@Composable
fun MotionPathOverlay(vm: CanvasViewModel) {
    val tool = vm.timeline
    val id = tool.pathClip ?: return
    if (tool.isPlaying || tool.shownFrame != null || tool.performing) return
    // the path as a drag changes it, before the change is let go; and the dots' places as the drag
    // began (where they take touches while it lasts)
    var live by remember(id) { mutableStateOf<MotionPath?>(null) }
    var held by remember(id) { mutableStateOf<MotionPath?>(null) }
    val read = remember(id, tool.revision) { tool.motionPath(id) }
    val path = live ?: read ?: return
    if (path.keys.isEmpty()) return
    val m = vm.viewMatrix
    val accent = Pc.Accent
    val here = tool.frame
    fun screen(x: Float, y: Float): Offset = Offset(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
    Canvas(Modifier.fillMaxSize()) {
        val line = Path()
        for (i in 0 until path.points.size / 2) {
            val at = screen(path.points[i * 2], path.points[i * 2 + 1])
            if (i == 0) line.moveTo(at.x, at.y) else line.lineTo(at.x, at.y)
        }
        drawPath(line, Color.White.copy(alpha = 0.7f), style = Stroke(4.dp.toPx()))
        drawPath(line, accent, style = Stroke(2.dp.toPx()))
        for (key in path.keys) {
            val at = screen(key.x, key.y)
            val h = key.handles
            if (h != null && h.size == 4) {
                for ((dx, dy) in listOf(h[0] to h[1], h[2] to h[3])) {
                    val end = screen(key.x + dx, key.y + dy)
                    drawLine(accent, at, end, 1.5.dp.toPx())
                    dot(end, 5.dp.toPx(), accent, current = false)
                }
            }
            dot(at, 6.5.dp.toPx(), accent, current = key.frame == here)
        }
    }
    val density = LocalDensity.current.density
    val grips = held ?: path
    // a drag's start: the key's frame, the canvas point it began at, and the block's pose there
    var dragging by remember(id) { mutableStateOf<Triple<UInt, Offset, TimelinePose?>?>(null) }
    fun settle(done: Boolean) {
        if (done) {
            dragging = null
            live = null
            held = null
        } else {
            live = tool.motionPath(id)
        }
    }
    for (k in grips.keys) {
        val h = k.handles
        if (h != null && h.size == 4) {
            for (handle in 0..1) {
                androidx.compose.runtime.key(k.frame, handle) {
                    PathGrip(screen(k.x + h[handle * 2], k.y + h[handle * 2 + 1]), density, tap = null) { to, done ->
                        if (held == null) held = path
                        val at = vm.windowToCanvas(to) ?: return@PathGrip
                        if (dragging == null) dragging = Triple(k.frame, at, null)
                        val dx = at.x - k.x
                        val dy = at.y - k.y
                        tool.setHandles(id, k.frame, if (handle == 0) listOf(dx, dy, -dx, -dy) else listOf(-dx, -dy, dx, dy), done)
                        settle(done)
                    }
                }
            }
        }
        androidx.compose.runtime.key(k.frame, -1) {
            PathGrip(screen(k.x, k.y), density, tap = { tool.goTo(k.frame) }) { to, done ->
                val at = vm.windowToCanvas(to) ?: return@PathGrip
                if (dragging == null) {
                    if (held == null) held = path
                    tool.goTo(k.frame)
                    dragging = Triple(k.frame, at, tool.pose(id))
                }
                val d = dragging ?: return@PathGrip
                val pose = d.third ?: return@PathGrip
                // as the clip's keys count it, through the groups it is in
                val by = vm.session?.timelineMotionDelta(id, d.first, at.x - d.second.x, at.y - d.second.y).orEmpty()
                tool.moveKey(id, d.first, Offset(by.firstOrNull() ?: 0f, by.lastOrNull() ?: 0f), pose, done)
                settle(done)
            }
        }
    }
}

/** A white dot ringed in the accent colour (filled with it on the playhead's key). */
private fun DrawScope.dot(at: Offset, r: Float, accent: Color, current: Boolean) {
    drawCircle(accent, r, at)
    drawCircle(if (current) accent else Color.White, r - 2.dp.toPx(), at)
}

/**
 * Where a dot at [at] (window pixels) takes touches: a tap (if it has one), or a drag to the window
 * point given and whether it is let go. It stays where it was as a drag began, so the finger's
 * points keep to one frame of reference.
 */
@Composable
private fun PathGrip(at: Offset, density: Float, tap: (() -> Unit)?, drag: (Offset, Boolean) -> Unit) {
    val side = 40.dp
    val half = side.value * density / 2f
    val origin = Offset(at.x - half, at.y - half)
    val onTap by rememberUpdatedState(tap)
    val onDrag by rememberUpdatedState(drag)
    val from by rememberUpdatedState(origin)
    Box(
        Modifier.offset { IntOffset(origin.x.roundToInt(), origin.y.roundToInt()) }.size(side).clip(CircleShape)
            .pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    // (the dot's place as the touch came down: where the finger's points count from)
                    val base = from
                    var moving = false
                    var last = down.position
                    var lifted = false
                    try {
                        while (true) {
                            val event = awaitPointerEvent()
                            val c = event.changes.firstOrNull { it.id == down.id } ?: break
                            if (!c.pressed) {
                                lifted = true
                                last = c.position
                                c.consume()
                                break
                            }
                            if (c.positionChanged()) {
                                last = c.position
                                if (moving || hypot(c.position.x - down.position.x, c.position.y - down.position.y) > 4f * density) {
                                    moving = true
                                    onDrag(base + last, false)
                                }
                            }
                            c.consume()
                        }
                    } finally {
                        if (moving) onDrag(base + last, true) else if (lifted) onTap?.invoke()
                    }
                }
            },
    )
}

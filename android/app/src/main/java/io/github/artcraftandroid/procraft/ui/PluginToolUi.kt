package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.PathEffect
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.PluginTool
import io.github.artcraftandroid.procraft.core.PluginToolShape
import io.github.artcraftandroid.procraft.core.PluginToolShown
import io.github.artcraftandroid.procraft.core.PluginToolTouch

/**
 * A plugin's tool (interface 24), opened from Actions → Canvas, as the iPad's (PluginTools.swift):
 * Comic Panels is one, from its plugin. The canvas under the tool's header (the whole of it fitted
 * there, for a tool that asks), with what the plugin shows drawn over it. A drag's touches go to the
 * plugin, which answers with what to show and its edits to the layers (each kept one undo step);
 * the header's modes, Undo and Done are the app's, and two fingers zoom, cancelling the drag under
 * way. With the plugin off, what it made stays as it was made: Comic Panels' panels are groups, each
 * a frame and the art clipped to it.
 */
class PluginToolState {
    /** The tool open, if one is. */
    var tool by mutableStateOf<PluginTool?>(null)
        internal set
    /** What it shows over the canvas. */
    var shapes by mutableStateOf<List<PluginToolShape>>(emptyList())
        internal set
    /** Its mode chosen, by its place among the tool's. */
    var mode by mutableStateOf(0u)
        internal set
}

/** What an answer of the open tool leaves: its shapes, what it said, or the tool closed. */
private fun CanvasViewModel.answered(shown: PluginToolShown, warning: Boolean) {
    shown.said?.let { showNotice(it, warning = warning) }
    if (shown.closed) {
        pluginTool.tool = null
        pluginTool.shapes = emptyList()
    } else if (shown.shapes != pluginTool.shapes) {
        pluginTool.shapes = shown.shapes
    }
}

/** Opens [tool], which may change the layers as it opens (Comic Panels makes a page). */
fun CanvasViewModel.openPluginTool(tool: PluginTool) {
    val s = session ?: return
    endQuickShape()
    fadePanels()
    val shown = s.pluginToolOpen(plugins, tool.key)
    shown.said?.let { showNotice(it, warning = shown.closed) }
    if (shown.changed) refresh()
    if (shown.closed) return
    pluginTool.shapes = shown.shapes
    pluginTool.mode = 0u
    pluginTool.tool = tool
}

/** Closes the open tool; one that fitted the canvas leaves it fitted to the window again. */
fun CanvasViewModel.closePluginTool() {
    val tool = pluginTool.tool ?: return
    session?.pluginToolClose()
    pluginTool.tool = null
    pluginTool.shapes = emptyList()
    if (tool.fit) {
        session?.fitView()
        refreshView()
    }
    refresh()
}

/** What the open tool shows, asked again (an undo, a redo). */
fun CanvasViewModel.readPluginTool() {
    if (pluginTool.tool == null) return
    val shown = session?.pluginToolAgain() ?: return
    answered(shown, warning = true)
}

/** A touch at [at] (window pixels) on the open tool, in its mode chosen: what it shows, and the layers read again if it changed them as it lets go. */
private fun CanvasViewModel.touchPluginTool(touch: PluginToolTouch, at: Offset) {
    val shown = session?.pluginToolTouch(touch, at.x, at.y, pluginTool.mode) ?: return
    answered(shown, warning = true)
    if (shown.changed && touch != PluginToolTouch.MOVED) refresh()
}

/** The canvas's room for a tool that fits it: this far below the header, and in from the window's sides and foot. */
private val UnderHeader = 16.dp
private val PageSide = 24.dp

/** The open plugin tool over the canvas: what it shows, its header and its mode's hint. */
@Composable
fun BoxScope.PluginToolOverlay(vm: CanvasViewModel) {
    val tool = vm.pluginTool.tool ?: return
    val density = LocalDensity.current
    var size by remember { mutableStateOf(IntSize.Zero) }
    // the whole canvas fitted under the header, again when the window changes
    LaunchedEffect(size, tool.key) {
        if (size == IntSize.Zero || !tool.fit) return@LaunchedEffect
        val (side, top) = with(density) { PageSide.toPx() to (ToolHeaderHeight + UnderHeader).toPx() }
        vm.session?.pluginToolFit(side, top, size.width - side, size.height - side)
        vm.refreshView()
    }
    Canvas(
        Modifier.fillMaxSize().onSizeChanged { size = it }.pointerInput(tool.key) {
            val slop = 4.dp.toPx()
            awaitEachGesture {
                val down = awaitFirstDown()
                down.consume()
                val start = down.position
                var at = start
                // past the slop, the drag has begun and goes to the tool
                var begun = false
                // a second finger came: a pinch, which zooms and cancels the drag
                var pinched = false
                var span: Float? = null
                while (true) {
                    val event = awaitPointerEvent()
                    val held = event.changes.filter { it.pressed }
                    if (held.size >= 2) {
                        if (begun && !pinched) vm.touchPluginTool(PluginToolTouch.CANCELLED, at)
                        pinched = true
                        event.changes.forEach { it.consume() }
                        val (a, b) = held[0].position to held[1].position
                        val now = (a - b).getDistance()
                        val was = span
                        span = now
                        if (was != null && was > 0f && now > 0f) {
                            val pivot = (a + b) / 2f
                            vm.session?.viewGesture(pivot.x, pivot.y, 0f, 0f, now / was, 0f)
                            vm.refreshView()
                        }
                        continue
                    }
                    span = null
                    val c = event.changes.firstOrNull { it.id == down.id } ?: break
                    if (!c.pressed) break
                    c.consume()
                    at = c.position
                    if (pinched) continue
                    if (!begun && (at - start).getDistance() < slop) continue
                    if (!begun) {
                        begun = true
                        vm.touchPluginTool(PluginToolTouch.DOWN, start)
                    }
                    vm.touchPluginTool(PluginToolTouch.MOVED, at)
                }
                if (begun && !pinched) vm.touchPluginTool(PluginToolTouch.UP, at)
            }
        },
    ) {
        val dashed = PathEffect.dashPathEffect(floatArrayOf(6.dp.toPx(), 4.dp.toPx()))
        for (shape in vm.pluginTool.shapes) {
            val p = shape.points
            val corners = (0 until p.size - 1 step 2).map { vm.canvasToWindow(Offset(p[it], p[it + 1])) }
            if (corners.isEmpty()) continue
            val line = Path().apply {
                moveTo(corners[0].x, corners[0].y)
                corners.drop(1).forEach { lineTo(it.x, it.y) }
                if (shape.closed) close()
            }
            if (shape.look == 0u) {
                // a guide: thin and dashed
                drawPath(line, Pc.Accent.copy(alpha = 0.8f), style = Stroke(1.5.dp.toPx(), pathEffect = dashed))
            } else {
                // what the touch draws: white under the accent, round at its ends
                drawPath(line, Color.White, style = Stroke(5.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
                drawPath(line, Pc.Accent, style = Stroke(3.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
            }
        }
    }
    val mode = vm.pluginTool.mode
    ToolHeader(
        tool.label,
        tool.modes.mapIndexed { i, m -> ToolButton(m.label, chosen = mode == i.toUInt()) { vm.pluginTool.mode = i.toUInt() } } +
            ToolButton("Undo") { vm.undo() },
        onDone = { vm.closePluginTool() },
        modifier = Modifier.align(Alignment.TopCenter),
    )
    val hint = tool.modes.getOrNull(mode.toInt())?.hint.orEmpty()
    if (hint.isNotEmpty()) {
        Text(
            hint,
            color = Pc.TextDim,
            fontSize = 15.sp,
            modifier = Modifier.align(Alignment.TopCenter).padding(top = ToolHeaderHeight + 8.dp),
        )
    }
}

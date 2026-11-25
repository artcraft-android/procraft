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
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.CanvasSession

/**
 * Comic Panels, as the iPad's (ComicPanels.swift), opened from Actions → Canvas: the canvas made a
 * comic page the first time (one panel inside a margin, white with a black border), then the page
 * fitted under the tool's header with each panel outlined. A line dragged across panels cuts each
 * in two, straight across or down when near it, with a gutter between. With Move chosen, a gutter
 * dragged moves the panels' edges on both sides of it (the page's edge alone, its margin). Two
 * fingers zoom; a pinch during a drag cuts or moves nothing. Each panel is a group in the layers:
 * its frame (locked) and the art above it, clipped to it. The panels themselves are the core's
 * (panels.rs).
 */
class ComicPanels {
    /** Comic Panels is open over the canvas. */
    var open by mutableStateOf(false)
        internal set
    /** The panels' shapes while it is (canvas pixels, x and y by turns). */
    var shapes by mutableStateOf<List<List<Float>>>(emptyList())
        private set

    /** The panels' shapes read again (a cut, an undo). */
    fun read(session: CanvasSession?) {
        val now = session?.comicPanels()?.map { it.points } ?: emptyList()
        if (now != shapes) shapes = now
    }
}

/** Opens Comic Panels, making the canvas a page first if it has no panels yet. */
fun CanvasViewModel.openComicPanels() {
    val s = session ?: return
    endQuickShape()
    fadePanels()
    if (s.comicPanels().isEmpty()) {
        if (s.addComicPage() == null) {
            showNotice("Comic panels can't be added to this canvas.", warning = true)
            return
        }
        refresh()
    }
    comicPanels.read(s)
    comicPanels.open = true
}

/** Closes it, the canvas fitted to the window again. */
fun CanvasViewModel.closeComicPanels() {
    if (!comicPanels.open) return
    comicPanels.open = false
    session?.fitView()
    refreshView()
}

/** The page's room: this far below the header, and in from the window's sides and foot. */
private val UnderHeader = 16.dp
private val PageSide = 24.dp

/** Comic Panels over the canvas: the panels outlined, the cut being dragged, the header and its hint. */
@Composable
fun BoxScope.ComicPanelsOverlay(vm: CanvasViewModel) {
    if (!vm.comicPanels.open) return
    val density = LocalDensity.current
    // Move chosen in the header: a drag takes a gutter (or the page's edge) instead of cutting
    var moving by remember { mutableStateOf(false) }
    // the line being dragged (window pixels), while one is
    var line by remember { mutableStateOf<Pair<Offset, Offset>?>(null) }
    var size by remember { mutableStateOf(IntSize.Zero) }
    // the whole page fitted under the header, again when the window changes
    LaunchedEffect(size) {
        if (size == IntSize.Zero) return@LaunchedEffect
        val (side, top) = with(density) { PageSide.toPx() to (ToolHeaderHeight + UnderHeader).toPx() }
        vm.session?.panelToolFit(side, top, size.width - side, size.height - side)
        vm.refreshView()
    }
    val movingNow by androidx.compose.runtime.rememberUpdatedState(moving)
    Canvas(
        Modifier.fillMaxSize().onSizeChanged { size = it }.pointerInput(Unit) {
            val slop = 4.dp.toPx()
            awaitEachGesture {
                val down = awaitFirstDown()
                down.consume()
                val start = down.position
                var at = start
                // past the slop, the drag has begun; moving, whether it took a gutter
                var begun = false
                var grabbed = false
                // a second finger came: a pinch, which cuts and moves nothing
                var pinched = false
                var span: Float? = null
                while (true) {
                    val event = awaitPointerEvent()
                    val held = event.changes.filter { it.pressed }
                    if (held.size >= 2) {
                        pinched = true
                        line = null
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
                    if (!begun && (at - start).getDistance() < slop) continue
                    if (!begun) {
                        begun = true
                        grabbed = movingNow && !pinched && vm.session?.gutterBegin(start.x, start.y) == true
                    }
                    if (grabbed) {
                        vm.session?.gutterDrag(at.x, at.y)
                        vm.comicPanels.read(vm.session)
                    } else if (!movingNow && !pinched) {
                        line = start to at
                    }
                }
                if (grabbed) {
                    vm.session?.gutterEnd(!pinched)
                    vm.refresh()
                } else if (begun && !movingNow && !pinched) {
                    val cut = vm.session?.cutPanelsOnScreen(start.x, start.y, at.x, at.y) ?: 0u
                    if (cut > 0u) vm.refresh()
                }
                line = null
            }
        },
    ) {
        val dashed = PathEffect.dashPathEffect(floatArrayOf(6.dp.toPx(), 4.dp.toPx()))
        for (shape in vm.comicPanels.shapes) {
            val corners = (0 until shape.size - 1 step 2).map { vm.canvasToWindow(Offset(shape[it], shape[it + 1])) }
            if (corners.isEmpty()) continue
            val outline = Path().apply {
                moveTo(corners[0].x, corners[0].y)
                corners.drop(1).forEach { lineTo(it.x, it.y) }
                close()
            }
            drawPath(outline, Pc.Accent.copy(alpha = 0.8f), style = Stroke(1.5.dp.toPx(), pathEffect = dashed))
        }
        // the cut as it would be made: straight across or down when near it
        val l = line
        val ends = l?.let { (a, b) -> vm.session?.panelCutOnScreen(a.x, a.y, b.x, b.y) }
        if (ends != null && ends.size == 4) {
            val (a, b) = Offset(ends[0], ends[1]) to Offset(ends[2], ends[3])
            drawLine(Color.White, a, b, 5.dp.toPx(), StrokeCap.Round)
            drawLine(Pc.Accent, a, b, 3.dp.toPx(), StrokeCap.Round)
        }
    }
    ToolHeader(
        "Comic Panels",
        listOf(
            ToolButton("Cut", chosen = !moving) { moving = false },
            ToolButton("Move", chosen = moving) { moving = true },
            ToolButton("Undo") { vm.undo() },
        ),
        onDone = { vm.closeComicPanels() },
        modifier = Modifier.align(Alignment.TopCenter),
    )
    Text(
        if (moving) "Drag a gutter, or the page's edge, to move it" else "Drag across panels to cut them",
        color = Pc.TextDim,
        fontSize = 15.sp,
        modifier = Modifier.align(Alignment.TopCenter).padding(top = ToolHeaderHeight + 8.dp),
    )
}

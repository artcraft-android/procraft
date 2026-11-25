package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
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
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.SavedCurves
import io.github.artcraftandroid.procraft.core.EasingKind
import io.github.artcraftandroid.procraft.core.EasingSpec
import io.github.artcraftandroid.procraft.core.defaultEasing
import io.github.artcraftandroid.procraft.core.easingCurve
import kotlin.math.hypot
import kotlin.math.roundToInt

// The easing editor, as the iPad app's (TimelineBar.swift, EasingPage and EasingCurveView;
// TimelineCurves.swift, MyCurves): how a key eases into the next, as a curve, its kind, that kind's
// numbers, how often it repeats (arriving each time, or looping back) and maybe backwards, and
// under it "My curves". Every change shows at once; a slider or point let go, or a tap, is done
// (one undo step).
//
// A key's rows open it as a page of their menu:
//
//     EasingEditor(
//         easing = vm.session.timelineKeyEasing(clip, frame) ?: defaultEasing(EasingKind.LINEAR),
//         onChange = { vm.session.setTimelineKeyEasing(clip, frame, it, false) },  // shown at once
//         onDone = { vm.session.setTimelineKeyEasing(clip, frame, it, true) },     // one undo step
//         onBack = { page = Page.EASING },
//     )
//
// It is a column as wide as it is given, scrolled by what holds it. A key's Easing menu lists My
// curves with SavedCurves.of(context).easingChoices(easing).

/** The kinds, as the iPad names and orders them. */
val EasingKinds = listOf(
    EasingKind.LINEAR to "Linear", EasingKind.SMOOTH to "Smooth", EasingKind.CUBIC_BEZIER to "Ease", EasingKind.HOLD to "Hold",
    EasingKind.BOUNCE to "Bounce", EasingKind.ELASTIC to "Elastic", EasingKind.STEPS to "Steps", EasingKind.ELASTIC_STEPS to "Springy Steps",
    EasingKind.CYCLIC to "Cyclic", EasingKind.RANDOM to "Random", EasingKind.CURVE to "Custom",
)

private class Control(val label: String, val least: Float, val most: Float)

/** Each kind's numbers. */
private fun controls(kind: EasingKind): List<Control> = when (kind) {
    EasingKind.CUBIC_BEZIER -> listOf(Control("Start pull", 0f, 1f), Control("Start lift", -0.5f, 1.5f), Control("End pull", 0f, 1f), Control("End lift", -0.5f, 1.5f))
    EasingKind.BOUNCE -> listOf(Control("First bounce", 0.01f, 1f), Control("Bounciness", 0.01f, 0.99f))
    EasingKind.ELASTIC -> listOf(Control("Wave length", 0.01f, 1f), Control("Attack", 0f, 2f), Control("Decay", 0f, 1f), Control("Swing", 0f, 1f))
    EasingKind.STEPS -> listOf(Control("Step length", 0.01f, 1f), Control("Smoothing", 0f, 1f))
    EasingKind.ELASTIC_STEPS -> listOf(Control("Step length", 0.01f, 1f), Control("Springiness", 0f, 1f))
    EasingKind.CYCLIC -> listOf(Control("Cycle length", 0.01f, 1f), Control("Sharpness", 0f, 1f), Control("Turn", 0.01f, 0.99f), Control("Drift", 0f, 1f))
    EasingKind.RANDOM -> listOf(Control("Frequency", 0f, 1f), Control("Magnitude", 0f, 2f), Control("Seed", 0f, 1f), Control("Fade", 0f, 1f), Control("Warp", 0f, 1f), Control("Tilt", 0f, 1f))
    EasingKind.LINEAR, EasingKind.HOLD, EasingKind.SMOOTH, EasingKind.CURVE -> emptyList()
}

private val InkDim get() = if (Pc.light) Color(0xFF7A7A7A) else Color(0xFF8E8E8E)
private val EasingRowLine get() = if (Pc.light) Color(0x0F000000) else Color(0x09FFFFFF)
private val Danger = Color(0xFFFE604D)

/**
 * The easing editor for [easing]: [onChange] as a slider, point or handle is held (shown at once,
 * no undo step yet), [onDone] when it is let go or a kind, switch or kept curve is tapped (one undo
 * step), [onBack] from its back chevron.
 */
@Composable
fun EasingEditor(
    easing: EasingSpec,
    onChange: (EasingSpec) -> Unit,
    onDone: (EasingSpec) -> Unit,
    onBack: () -> Unit,
    modifier: Modifier = Modifier,
) {
    // the easing as a slider or point moves it, before it is let go
    var live by remember { mutableStateOf<EasingSpec?>(null) }
    val spec = live ?: easing
    fun drag(next: EasingSpec) {
        live = next
        onChange(next)
    }
    fun end() {
        live?.let(onDone)
        live = null
    }
    fun commit(next: EasingSpec) {
        live = null
        onDone(next)
    }
    Column(modifier.fillMaxWidth()) {
        Row(Modifier.fillMaxWidth().padding(top = 6.dp).height(34.dp), verticalAlignment = Alignment.CenterVertically) {
            Box(Modifier.width(44.dp).height(34.dp).clickable(onClick = onBack), contentAlignment = Alignment.Center) {
                Icon(PcIcons.ChevronLeft, "Back", Modifier.size(34.dp), tint = Pc.SlideValue)
            }
            Text("Easing", color = Pc.PanelTitle, fontSize = 23.sp, fontWeight = FontWeight.Bold)
        }
        // (a curve of your own is drawn by its points, and has more room for it)
        val drawn = spec.kind == EasingKind.CURVE
        Spacer(Modifier.height(10.dp))
        EasingCurve(spec, if (drawn) 170.dp else 110.dp) { next, done ->
            drag(next)
            if (done) end()
        }
        if (drawn) {
            Text(
                "Tap to add a point. Drag a point to shape the curve, or off the top or bottom to remove it.",
                color = InkDim, fontSize = 12.sp, modifier = Modifier.padding(start = 6.dp, end = 6.dp, top = 4.dp, bottom = 8.dp),
            )
        } else {
            Spacer(Modifier.height(10.dp))
        }
        Column(verticalArrangement = Arrangement.spacedBy(4.dp)) {
            for (row in EasingKinds.chunked(3)) {
                Row(horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                    for ((kind, label) in row) {
                        val chosen = spec.kind == kind
                        Box(
                            Modifier.weight(1f).heightIn(min = 30.dp).clip(RoundedCornerShape(9.dp))
                                .background(if (chosen) Pc.Accent else Pc.ActionsGroup)
                                .clickable { commit(defaultEasing(kind).copy(repeats = spec.repeats, looping = spec.looping, reversed = spec.reversed)) },
                            contentAlignment = Alignment.Center,
                        ) {
                            Text(
                                label, color = if (chosen) Color.White else Pc.PanelTitle, fontSize = 14.sp,
                                fontWeight = if (chosen) FontWeight.SemiBold else FontWeight.Normal, maxLines = 1, overflow = TextOverflow.Ellipsis,
                            )
                        }
                    }
                    // (the last row's empty places)
                    repeat(3 - row.size) { Spacer(Modifier.weight(1f)) }
                }
            }
        }
        Spacer(Modifier.height(10.dp))
        EasingSliderGroup {
            controls(spec.kind).forEachIndexed { i, c ->
                val value = spec.params.getOrNull(i) ?: c.least
                EasingSlider(c.label, "%.2f".format(java.util.Locale.ROOT, value), (value - c.least) / (c.most - c.least), onEnd = { end() }) { v ->
                    val params = spec.params.toMutableList()
                    while (params.size <= i) params.add(c.least)
                    params[i] = c.least + v * (c.most - c.least)
                    drag(spec.copy(params = params))
                }
            }
            EasingSlider("Repeats", "${spec.repeats}", (spec.repeats.toInt().coerceAtLeast(1) - 1) / 7f, onEnd = { end() }) { v ->
                drag(spec.copy(repeats = (1 + (v * 7f).roundToInt()).toUInt()))
            }
        }
        Spacer(Modifier.height(10.dp))
        EasingGroup {
            EasingToggle("Loop back", spec.looping) { commit(spec.copy(looping = !spec.looping)) }
            EasingRowLine()
            EasingToggle("Backwards", spec.reversed) { commit(spec.copy(reversed = !spec.reversed)) }
        }
        Spacer(Modifier.height(14.dp))
        MyCurves(spec) { commit(it) }
        Spacer(Modifier.height(10.dp))
    }
}

/** A point of a curve: time across, how far along up. */
private data class Pt(val x: Float, val y: Float)

/** A drawn curve's points as the engine reads them: in time's order, the ends pinned. */
private fun curvePoints(spec: EasingSpec): List<Pt> {
    val p = spec.params
    val pts = (0 until p.size / 2).map { Pt(p[2 * it].coerceIn(0f, 1f), p[2 * it + 1].coerceIn(-0.5f, 1.5f)) }.sortedBy { it.x }.toMutableList()
    if (pts.size < 2) return listOf(Pt(0f, 0f), Pt(1f, 1f))
    pts[0] = Pt(0f, 0f)
    pts[pts.lastIndex] = Pt(1f, 1f)
    return pts
}

/** Ease's two handles. */
private fun handles(spec: EasingSpec): List<Pt> {
    fun p(i: Int, or: Float) = spec.params.getOrNull(i)?.takeIf { it.isFinite() } ?: or
    return listOf(Pt(p(0, 0.42f), p(1, 0f)), Pt(p(2, 0.58f), p(3, 1f)))
}

private fun place(p: Pt, size: Size) = Offset(size.width * p.x, size.height * (1.5f - p.y) / 2f)

private fun withPoints(spec: EasingSpec, pts: List<Pt>) = spec.copy(params = pts.flatMap { listOf(it.x, it.y) })

/**
 * An easing's curve: time across, how far along up (-0.5 to 1.5, so overshoots show), the start
 * and end levels faint. Ease's two handles are dragged on it. A drawn curve (Custom) shows the
 * points it goes through, and they are its controls: one dragged moves between its neighbours, a
 * touch away from them all adds one there, and one let go well above or below the frame is taken
 * away. The first and last stay where they are. [onEdit] gives the easing and whether the finger is
 * up.
 */
@Composable
private fun EasingCurve(spec: EasingSpec, height: Dp, onEdit: (EasingSpec, Boolean) -> Unit) {
    val custom = spec.kind == EasingKind.CURVE
    val ease = spec.kind == EasingKind.CUBIC_BEZIER
    // the point (or handle) held, and whether it is far enough off the frame to be taken away
    var held by remember { mutableStateOf<Int?>(null) }
    var off by remember { mutableStateOf(false) }
    val current by rememberUpdatedState(spec)
    val edit by rememberUpdatedState(onEdit)
    val samples = remember(spec) { easingCurve(spec, 96u) }
    val accent = Pc.Accent
    val dim = InkDim
    Box(
        Modifier.fillMaxWidth().height(height).clip(RoundedCornerShape(14.dp)).background(Pc.ActionsGroup)
            .pointerInput(custom, ease) {
                if (!custom && !ease) return@pointerInput
                val reach = 26.dp.toPx()
                val away = 30.dp.toPx()
                val apart = 0.03f
                awaitEachGesture {
                    val down = awaitFirstDown()
                    val start = down.position
                    val w = size.width.toFloat()
                    val h = size.height.toFloat()
                    if (w <= 0f || h <= 0f) return@awaitEachGesture
                    val frame = Size(w, h)
                    val base = current
                    // the points as the touch has made them so far
                    var working: MutableList<Pt>? = null
                    var last: EasingSpec? = null
                    fun touched(at: Offset): EasingSpec? {
                        val x = at.x / w
                        val y = (1.5f - 2f * at.y / h).coerceIn(-0.5f, 1.5f)
                        if (ease) {
                            val hs = handles(base).toMutableList()
                            if (held == null) {
                                val far = hs.map { hypot(place(it, frame).x - start.x, place(it, frame).y - start.y) }
                                val near = if (far[1] < far[0]) 1 else 0
                                if (far[near] > reach) return null
                                held = near
                            }
                            val i = held ?: return null
                            hs[i] = Pt(x.coerceIn(0f, 1f), y)
                            return base.copy(params = hs.flatMap { listOf(it.x, it.y) })
                        }
                        val pts = working ?: curvePoints(base).toMutableList()
                        if (held == null) {
                            val far = { i: Int -> hypot(place(pts[i], frame).x - start.x, place(pts[i], frame).y - start.y) }
                            val near = (1 until pts.lastIndex).minByOrNull(far)
                            if (near != null && far(near) <= reach) {
                                held = near
                            } else {
                                // a new one, where there is room between two for it
                                val i = pts.indexOfFirst { it.x > x }.let { if (it < 0) pts.lastIndex else it }
                                if (i <= 0 || pts[i].x - pts[i - 1].x < 2 * apart) return null
                                pts.add(i, Pt(x, y))
                                held = i
                            }
                        }
                        val i = held ?: return null
                        if (i <= 0 || i >= pts.lastIndex) return null
                        val least = pts[i - 1].x + apart
                        pts[i] = Pt(x.coerceIn(least, maxOf(pts[i + 1].x - apart, least)), y)
                        off = at.y < -away || at.y > h + away
                        working = pts
                        return withPoints(base, pts)
                    }
                    touched(start)?.let { last = it; edit(it, false) }
                    while (true) {
                        val event = awaitPointerEvent()
                        val change = event.changes.firstOrNull { it.id == down.id } ?: break
                        if (!change.pressed) break
                        change.consume()
                        touched(change.position)?.let { last = it; edit(it, false) }
                    }
                    // let go: the point held taken away if it was off the frame
                    val pts = working
                    val i = held
                    if (custom && pts != null && i != null) {
                        if (off && i > 0 && i < pts.lastIndex) pts.removeAt(i)
                        edit(withPoints(base, pts), true)
                    } else {
                        last?.let { edit(it, true) }
                    }
                    held = null
                    off = false
                }
            }
            .drawBehind {
                fun y(v: Float) = size.height * (1.5f - v.coerceIn(-0.5f, 1.5f)) / 2f
                for (level in listOf(0f, 1f)) drawRect(dim.copy(alpha = 0.4f), Offset(0f, y(level)), Size(size.width, 1.dp.toPx()))
                val line = Path()
                samples.forEachIndexed { i, v ->
                    val x = size.width * i / (samples.size - 1).coerceAtLeast(1)
                    if (i == 0) line.moveTo(x, y(v)) else line.lineTo(x, y(v))
                }
                val faint = custom && off
                drawPath(line, accent.copy(alpha = if (faint) 0.5f else 1f), style = Stroke(2.5.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
                if (custom) {
                    val pts = curvePoints(spec)
                    pts.forEachIndexed { i, p ->
                        val at = place(p, size)
                        if (i == 0 || i == pts.lastIndex) {
                            drawCircle(dim, 4.dp.toPx(), at)
                        } else {
                            val fade = if (i == held && off) 0.3f else 1f
                            val r = (if (i == held) 9 else 7).dp.toPx()
                            drawCircle(accent.copy(alpha = fade), r, at)
                            drawCircle(Color.White.copy(alpha = fade), r, at, style = Stroke(2.dp.toPx()))
                        }
                    }
                } else if (ease) {
                    for ((from, h) in listOf(Pt(0f, 0f), Pt(1f, 1f)).zip(handles(spec))) {
                        val at = place(h, size)
                        drawLine(dim, place(from, size), at, 1.5.dp.toPx())
                        drawCircle(accent, 7.dp.toPx(), at)
                        drawCircle(Color.White, 7.dp.toPx(), at, style = Stroke(2.dp.toPx()))
                    }
                }
            },
    )
}

/** A kept curve's name being asked for: a new one's ([renamed] none), or another for [renamed]. */
private data class CurveNaming(val renamed: Long?, val typed: String)

/**
 * The editor's "My curves": the easing showing ([spec]) kept as a new one, and each one kept, put
 * on the key with a tap ([apply]), or renamed, duplicated, replaced with what is showing, or
 * deleted (its ⋯).
 */
@Composable
private fun MyCurves(spec: EasingSpec, apply: (EasingSpec) -> Unit) {
    val context = LocalContext.current
    val curves = remember { SavedCurves.of(context) }
    // the curve whose actions show
    var open by remember { mutableStateOf<Long?>(null) }
    var naming by remember { mutableStateOf<CurveNaming?>(null) }
    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Text("My curves", color = Pc.PanelTitle.copy(alpha = 0.6f), fontSize = 13.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.padding(start = 16.dp))
        EasingGroup {
            Row(
                Modifier.fillMaxWidth().height(44.dp).clickable {
                    open = null
                    naming = CurveNaming(null, curves.nextName)
                }.padding(start = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Box(Modifier.width(46.dp), contentAlignment = Alignment.Center) {
                    Icon(PcIcons.PlusMedium, null, Modifier.size(22.dp), tint = Pc.Accent)
                }
                Spacer(Modifier.width(10.dp))
                Text("Save this curve", color = Pc.Accent, fontSize = 17.sp)
            }
            for (curve in curves.all) {
                EasingRowLine()
                val chosen = curve.spec == spec
                Row(
                    Modifier.fillMaxWidth().height(44.dp).clickable { apply(curve.spec) }.padding(start = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    CurveThumb(curve.spec, Modifier.width(46.dp).height(28.dp))
                    Spacer(Modifier.width(10.dp))
                    Text(
                        curve.name, color = if (chosen) Pc.Accent else Pc.PanelTitle, fontSize = 17.sp,
                        fontWeight = if (chosen) FontWeight.SemiBold else FontWeight.Normal, maxLines = 1, overflow = TextOverflow.Ellipsis,
                        modifier = Modifier.weight(1f),
                    )
                    Box(Modifier.size(44.dp).clickable { open = if (open == curve.id) null else curve.id }, contentAlignment = Alignment.Center) {
                        Icon(PcIcons.More, "${curve.name} options", Modifier.size(22.dp), tint = if (open == curve.id) Pc.Accent else Pc.PanelTitle.copy(alpha = 0.6f))
                    }
                }
                if (open == curve.id) {
                    Row(Modifier.fillMaxWidth().padding(start = 8.dp, end = 8.dp, bottom = 8.dp), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
                        CurveAction("Rename") { naming = CurveNaming(curve.id, curve.name) }
                        CurveAction("Duplicate") { curves.duplicate(curve.id) }
                        // (what the editor shows now, kept under this name)
                        CurveAction("Replace", dim = chosen) { curves.replace(curve.id, spec) }
                        CurveAction("Delete", tint = Danger) {
                            curves.remove(curve.id)
                            open = null
                        }
                    }
                }
            }
        }
    }
    Alert(
        naming,
        title = { n -> if (n.renamed != null) "Rename curve" else "Save curve" },
        message = { "" },
        action = if (naming?.renamed != null) "Rename" else "Save",
        destructive = false,
        field = naming?.typed,
        onField = { typed -> naming = naming?.copy(typed = typed) },
        onCancel = { naming = null },
        onAction = { n ->
            val renamed = n.renamed
            if (renamed != null) curves.rename(renamed, n.typed) else curves.add(spec, n.typed)
            naming = null
        },
    )
}

@Composable
private fun androidx.compose.foundation.layout.RowScope.CurveAction(label: String, tint: Color? = null, dim: Boolean = false, act: () -> Unit) {
    Box(
        Modifier.weight(1f).heightIn(min = 30.dp).clip(RoundedCornerShape(9.dp)).background(Pc.PanelTitle.copy(alpha = 0.08f))
            .clickable(enabled = !dim, onClick = act),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = (tint ?: Pc.PanelTitle).copy(alpha = if (dim) 0.35f else 1f), fontSize = 13.sp, fontWeight = FontWeight.Medium, maxLines = 1)
    }
}

/** A curve small: time across, how far along up. */
@Composable
fun CurveThumb(spec: EasingSpec, modifier: Modifier = Modifier) {
    val samples = remember(spec) { easingCurve(spec, 32u) }
    val accent = Pc.Accent
    Box(
        modifier.clip(RoundedCornerShape(7.dp)).background(Pc.PanelTitle.copy(alpha = 0.08f)).drawBehind {
            val inset = 3.dp.toPx()
            val line = Path()
            samples.forEachIndexed { i, v ->
                val x = inset + (size.width - 2 * inset) * i / (samples.size - 1).coerceAtLeast(1)
                val y = inset + (size.height - 2 * inset) * (1.5f - v.coerceIn(-0.5f, 1.5f)) / 2f
                if (i == 0) line.moveTo(x, y) else line.lineTo(x, y)
            }
            drawPath(line, accent, style = Stroke(1.8.dp.toPx(), cap = StrokeCap.Round, join = StrokeJoin.Round))
        },
    )
}

@Composable
private fun EasingToggle(label: String, on: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(44.dp).clickable(onClick = onClick).padding(start = 16.dp, end = 6.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Pc.PanelTitle, fontSize = 17.sp, modifier = Modifier.weight(1f))
        Switch(on, enabled = true)
    }
}

/** The faint 2 pt line between a group's rows. */
@Composable
private fun EasingRowLine() {
    Box(Modifier.fillMaxWidth().height(2.dp).background(EasingRowLine))
}

@Composable
private fun EasingGroup(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(Pc.ActionsGroup), content = content)
}

/** A group of sliders: 8 above the first, 11.5 between. */
@Composable
private fun EasingSliderGroup(content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier.fillMaxWidth().clip(RoundedCornerShape(18.dp)).background(Pc.ActionsGroup).padding(top = 8.dp),
        verticalArrangement = Arrangement.spacedBy(11.5.dp),
        content = content,
    )
}

/** A setting's name and value over a bar dragged or tapped along. */
@Composable
private fun EasingSlider(label: String, value: String, fraction: Float, onEnd: () -> Unit, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    val end by rememberUpdatedState(onEnd)
    Column(Modifier.fillMaxWidth().height(55.dp).padding(horizontal = 16.dp)) {
        Row(Modifier.fillMaxWidth().padding(top = 6.5.dp)) {
            Text(label, color = Pc.SlideLabel, fontSize = 17.sp, modifier = Modifier.weight(1f), maxLines = 1)
            Text(value, color = Pc.SlideValue, fontSize = 17.sp, maxLines = 1)
        }
        Box(
            Modifier.fillMaxWidth().height(22.dp)
                .pointerInput(Unit) {
                    detectTapGestures {
                        change((it.x / size.width).coerceIn(0f, 1f))
                        end()
                    }
                }
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(onDragStart = { change((it.x / size.width).coerceIn(0f, 1f)) }, onDragEnd = { end() }, onDragCancel = { end() }) { c, _ ->
                        c.consume()
                        change((c.position.x / size.width).coerceIn(0f, 1f))
                    }
                }
                .drawBehind {
                    val y = size.height / 2f
                    val h = 3.dp.toPx()
                    val x = fraction.coerceIn(0f, 1f) * size.width
                    drawRect(Pc.SlideTrack, Offset(0f, y - h / 2f), Size(size.width, h))
                    drawRect(Pc.Accent, Offset(0f, y - h / 2f), Size(x, h))
                    val knob = Size(16.dp.toPx(), 10.dp.toPx())
                    drawRoundRect(Color.White, Offset((x - knob.width / 2f).coerceIn(0f, size.width - knob.width), y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
                },
        )
    }
}

/**
 * The editor on its own, an easing of its own (Bounce to start with) changed as it is used: to look
 * at it before the key rows open it.
 */
@Composable
fun EasingEditorPreview() {
    var spec by remember { mutableStateOf(defaultEasing(EasingKind.BOUNCE)) }
    Column(Modifier.width(320.dp).clip(RoundedCornerShape(24.dp)).background(Pc.PopoverFill).padding(8.dp).verticalScroll(rememberScrollState())) {
        EasingEditor(spec, onChange = { spec = it }, onDone = { spec = it }, onBack = {})
    }
}

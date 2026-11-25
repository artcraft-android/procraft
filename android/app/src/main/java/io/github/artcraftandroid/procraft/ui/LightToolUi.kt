package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.offset
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.DrawMode
import io.github.artcraftandroid.procraft.core.ToolKind
import io.github.artcraftandroid.procraft.core.LightKind
import io.github.artcraftandroid.procraft.core.LightSettings
import kotlin.math.roundToInt

// the light tool and the Pen's and Eraser's lassos, as the iPad app's (LightPanel.swift): the Pen draws with light on an HDR canvas, or either with its lasso,
// chosen at the brush library's top; its settings take the brushes' place there

private val TrackFill get() = if (Pc.light) Color(0x1F767680) else Color(0x3D767680)
private val ThumbFill get() = if (Pc.light) Color.White else Color(0xFF636366)

/**
 * The library's top, over its title: how the Pen or Eraser draws (Brush, Lasso, Light), as a
 * segmented control. Not there for the Smudge, or with only Brush to choose.
 */
@Composable
fun DrawModeBar(vm: CanvasViewModel) {
    val modes = vm.drawModes
    if (modes.isEmpty()) return
    val chosen = modes.indexOf(vm.drawMode).coerceAtLeast(0)
    val at by animateFloatAsState(chosen.toFloat(), Motion.switchKnob(), label = "draw mode")
    Box(Modifier.fillMaxWidth().height(44.dp).padding(horizontal = 12.dp), contentAlignment = Alignment.BottomCenter) {
        BoxWithConstraints(Modifier.fillMaxWidth().height(32.dp).clip(RoundedCornerShape(9.dp)).background(TrackFill)) {
            val segment = maxWidth / modes.size
            Box(Modifier.offset(x = segment * at).padding(2.dp).size(segment - 4.dp, 28.dp).clip(RoundedCornerShape(7.dp)).background(ThumbFill))
            Row(Modifier.fillMaxWidth()) {
                for ((i, mode) in modes.withIndex()) {
                    Box(Modifier.weight(1f).fillMaxHeight().clickable { vm.chooseDrawMode(mode) }, contentAlignment = Alignment.Center) {
                        Text(mode.label, color = if (i == chosen) Pc.PanelTitle else MenuText, fontSize = 13.sp, maxLines = 1)
                    }
                }
            }
        }
    }
}

/** The library while the Pen or Eraser draws with its lasso or with light: the Brush | Lasso | Light row, and their settings under it. */
@Composable
fun DrawModeLibrary(vm: CanvasViewModel, arrowX: Dp, modifier: Modifier, floating: FloatingFrame?) {
    Popover(floating?.width ?: LibraryWidth, arrowX, modifier, side = if (floating != null) ArrowSide.NONE else ArrowSide.TOP) {
        floating?.let { FloatingTop(it) }
        DrawModeBar(vm)
        Spacer(Modifier.height(14.dp))
        if (vm.drawMode == DrawMode.LIGHT) LightSettingsView(vm) else LassoSettingsView(vm)
    }
}

/**
 * The Pen's lasso's settings, under the library's Lasso, as the iPad's. Outline: the brush paints
 * the edge as it goes, and what it goes round fills under it as it lifts, in the paint colour or
 * the second colour, at a strength of its own (the bar's opacity is the brush's). Without, each
 * shape drawn fills with the paint colour at the bar's opacity. The Eraser's lasso has only its
 * Preview and its words.
 */
@Composable
private fun LassoSettingsView(vm: CanvasViewModel) {
    val preview = vm.lassoPreviewOpacity
    val previewSlider = @Composable {
        Group {
            LightSlider("Preview", preview, if (preview <= 0f) "Off" else "${(preview * 100).roundToInt()}%") { vm.changeLassoPreviewOpacity(it) }
        }
    }
    Column(Modifier.padding(start = 10.dp, end = 10.dp, bottom = 14.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        if (vm.tool == ToolKind.ERASE) {
            previewSlider()
            LassoCaption("Draw a shape to erase it, at the eraser's opacity.")
            return@Column
        }
        val outline = vm.lassoOutline
        Group {
            Row(
                Modifier.fillMaxWidth().height(44.dp).clickable { vm.changeLassoOutline(!outline) }.padding(start = 15.dp, end = 12.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text("Outline with brush", color = MenuText, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                Switch(outline, enabled = true)
            }
        }
        if (outline) {
            Group {
                Row(Modifier.fillMaxWidth().height(50.dp).padding(start = 15.dp, end = 12.dp), verticalAlignment = Alignment.CenterVertically) {
                    Text("Fill", color = MenuText, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                    FillSegments(listOf("Color", "Second color"), if (vm.lassoFillSecondary) 1 else 0) { vm.changeLassoFillSecondary(it == 1) }
                }
                Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                val opacity = vm.lassoFillOpacity
                LightSlider("Fill opacity", opacity, "${(opacity * 100).roundToInt()}%") { vm.changeLassoFillOpacity(it) }
            }
        }
        previewSlider()
        LassoCaption(
            if (outline) "The brush draws the edge; what it goes round fills under it as you lift."
            else "Draw a shape to fill it with the color, at the brush's opacity.",
        )
    }
}

/** A note under the lasso's settings. */
@Composable
private fun LassoCaption(text: String) {
    Text(text, color = MenuText.copy(alpha = 0.6f), fontSize = 13.sp, modifier = Modifier.fillMaxWidth().padding(horizontal = 15.dp))
}

/** A small segmented control of [labels], [chosen] lit. */
@Composable
private fun FillSegments(labels: List<String>, chosen: Int, choose: (Int) -> Unit) {
    Row(Modifier.height(32.dp).clip(RoundedCornerShape(9.dp)).background(TrackFill).padding(2.dp)) {
        labels.forEachIndexed { i, label ->
            Box(
                Modifier.fillMaxHeight().clip(RoundedCornerShape(7.dp)).background(if (i == chosen) ThumbFill else Color.Transparent)
                    .clickable { choose(i) }.padding(horizontal = 10.dp),
                contentAlignment = Alignment.Center,
            ) {
                Text(label, color = if (i == chosen) Pc.PanelTitle else MenuText, fontSize = 13.sp, maxLines = 1)
            }
        }
    }
}

/**
 * The light tool's settings: what a stroke does to the paint under the brush (brighten, dim, or tame
 * what is past white) and how strongly; and how far past white Brighten goes. The brush is the
 * Pen's, chosen under Brush.
 */
@Composable
private fun LightSettingsView(vm: CanvasViewModel) {
    val light = vm.lightSettings
    val kinds = listOf(LightKind.BRIGHTEN to "Brighten", LightKind.DIM to "Dim", LightKind.TAME to "Tame")
    Column(Modifier.padding(start = 10.dp, end = 10.dp, bottom = 10.dp), verticalArrangement = Arrangement.spacedBy(10.dp)) {
        Group {
            kinds.forEachIndexed { i, (kind, name) ->
                if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                val chosen = light.kind == kind
                Row(
                    Modifier.fillMaxWidth().height(44.dp).clickable { vm.setLight(LightSettings(kind, light.strength)) }.padding(start = 15.dp, end = 12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Text(name, color = if (chosen) Pc.Accent else MenuText, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
                    if (chosen) Icon(PcIcons.Tick, contentDescription = null, tint = Pc.Accent, modifier = Modifier.size(24.dp))
                }
            }
        }
        Group {
            LightSlider("Strength", light.strength, "${(light.strength * 100).roundToInt()}%") { v ->
                vm.setLight(LightSettings(light.kind, v))
            }
        }
        // how far past white Brighten goes: an HDR canvas's only (a standard one stops at white)
        if (vm.isHdr && light.kind == LightKind.BRIGHTEN) {
            Group {
                LightSlider("HDR limit", vm.lightLimit / CanvasViewModel.HDR_LIMIT_MOST, hdrLimitShown(vm.lightLimit)) { v ->
                    vm.chooseLightLimit(v * CanvasViewModel.HDR_LIMIT_MOST)
                }
            }
        }
    }
}

/** The HDR limit as its slider shows it. */
private fun hdrLimitShown(stops: Float): String {
    val rounded = (stops * 10).roundToInt() / 10f
    if (rounded == 0f) return "White"
    val number = if (rounded % 1f == 0f) rounded.toInt().toString() else rounded.toString()
    return "+$number stops"
}

@Composable
private fun Group(content: @Composable () -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(13.dp)).background(Pc.ActionsGroup)) { content() }
}

/** A slider of [value] (0..1), read as [shown], set by dragging anywhere along it. */
@Composable
private fun LightSlider(label: String, value: Float, shown: String, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    Column(Modifier.fillMaxWidth().height(63.dp).padding(horizontal = 15.dp), verticalArrangement = Arrangement.Center) {
        Row(Modifier.fillMaxWidth()) {
            Text(label, color = MenuText, fontSize = 17.sp, modifier = Modifier.weight(1f))
            Text(shown, color = MenuText, fontSize = 17.sp)
        }
        Spacer(Modifier.height(8.dp))
        BoxWithConstraints(
            Modifier.fillMaxWidth().height(16.dp).pointerInput(Unit) {
                awaitEachGesture {
                    val down = awaitFirstDown()
                    down.consume()
                    val along = { x: Float -> (x / size.width.coerceAtLeast(1)).coerceIn(0f, 1f) }
                    change(along(down.position.x))
                    while (true) {
                        val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                        c.consume()
                        if (!c.pressed) break
                        change(along(c.position.x))
                    }
                }
            },
            contentAlignment = Alignment.CenterStart,
        ) {
            val part = value.coerceIn(0f, 1f)
            Box(Modifier.fillMaxWidth().height(3.dp).clip(CircleShape).background(Pc.SliderRest))
            Box(Modifier.fillMaxWidth(part).height(3.dp).clip(CircleShape).background(Pc.Accent))
            val x = (maxWidth * part).coerceIn(13.dp, (maxWidth - 13.dp).coerceAtLeast(13.dp))
            Box(Modifier.offset(x = x - 13.dp).size(26.dp, 15.dp).clip(CircleShape).background(Color.White))
        }
    }
}

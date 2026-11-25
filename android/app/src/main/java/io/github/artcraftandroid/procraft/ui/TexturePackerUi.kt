package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.BackHandler
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.gestures.detectTransformGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxWithConstraints
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardActions
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.focus.onFocusChanged
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalFocusManager
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.input.ImeAction
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.min
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewModelScope
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TexturePackerState
import io.github.artcraftandroid.procraft.canvas.closeTexturePacker
import io.github.artcraftandroid.procraft.canvas.packedSprites
import io.github.artcraftandroid.procraft.canvas.packerMoveView
import io.github.artcraftandroid.procraft.canvas.packerName
import io.github.artcraftandroid.procraft.canvas.packerResized
import io.github.artcraftandroid.procraft.canvas.packerTapped
import kotlinx.coroutines.launch
import kotlin.math.roundToInt

/** A box not chosen: grey and see-through, its edge light. */
private val BoxFill = Color(0x59808080)
private val BoxEdge = Color(0xE6CCCCCC)
private val PackerLabel get() = if (Pc.light) Color(0xFF595959) else Color(0xFF808080)

/** A press on a panel stays there, never reaching the canvas under it. */
private val Swallows = Modifier.pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume() } }

/**
 * The Texture Packer over the canvas, as the iPad app's (canvas/TexturePacker.kt): the drawings'
 * boxes, grey until tapped, blue while chosen; the bar under them with Cancel, how many are
 * chosen, Export, Separation and Padding; and over it the card of the sprite tapped last, to
 * rename it. The canvas can be moved and zoomed with a finger or two, but not drawn on.
 */
@Composable
fun TexturePackerOverlay(vm: CanvasViewModel, state: TexturePackerState) {
    val context = LocalContext.current
    val focus = LocalFocusManager.current
    var typed by remember { mutableStateOf("") }
    LaunchedEffect(state.focused) {
        state.focused?.takeIf { it in state.boxes.indices }?.let { typed = vm.packerName(state, it) }
    }
    // the name typed kept for sprite [index] (an empty one going back to what it was)
    fun rename(index: Int) {
        val b = state.boxes.getOrNull(index) ?: return
        val name = typed.trim()
        if (name.isEmpty()) typed = vm.packerName(state, index) else state.names[TexturePackerState.key(b)] = name
    }
    BackHandler { vm.closeTexturePacker() }
    BoxWithConstraints(Modifier.fillMaxSize()) {
        // a tap chooses (or lets go) the drawing under it; a drag or a pinch moves the view
        Box(
            Modifier.fillMaxSize()
                .pointerInput(state) {
                    detectTapGestures {
                        focus.clearFocus()
                        vm.packerTapped(state, it)
                    }
                }
                .pointerInput(Unit) { detectTransformGestures { centroid, pan, zoom, _ -> vm.packerMoveView(centroid, pan, zoom) } },
        )
        val m = vm.viewMatrix
        Canvas(Modifier.fillMaxSize()) {
            state.boxes.forEachIndexed { i, b ->
                val (x0, y0) = b.x.toFloat() to b.y.toFloat()
                val (x1, y1) = x0 + b.width.toFloat() to y0 + b.height.toFloat()
                val at = { x: Float, y: Float -> Offset(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]) }
                val outline = Path().apply {
                    at(x0, y0).let { moveTo(it.x, it.y) }
                    at(x1, y0).let { lineTo(it.x, it.y) }
                    at(x1, y1).let { lineTo(it.x, it.y) }
                    at(x0, y1).let { lineTo(it.x, it.y) }
                    close()
                }
                if (i in state.chosen) {
                    drawPath(outline, Pc.Accent.copy(alpha = 0.3f))
                    drawPath(outline, Pc.Accent, style = Stroke(2.dp.toPx()))
                } else {
                    drawPath(outline, BoxFill)
                    drawPath(outline, BoxEdge, style = Stroke(1.5.dp.toPx()))
                }
            }
        }
        val barWidth = min(668.dp, maxWidth - 32.dp)
        val slider = (barWidth - 48.dp - 20.dp) / 2
        Column(
            Modifier.align(Alignment.BottomCenter).padding(bottom = 24.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
        ) {
            // the sprite tapped last: its picture, its size, and its name to change
            val shown = state.focused?.takeIf { it in state.boxes.indices }
            if (shown != null) {
                val b = state.boxes[shown]
                Row(
                    Modifier.clip(RoundedCornerShape(22.dp)).background(Pc.Panel).then(Swallows).padding(14.dp),
                    horizontalArrangement = Arrangement.spacedBy(12.dp),
                    verticalAlignment = Alignment.CenterVertically,
                ) {
                    Box(Modifier.size(56.dp).clip(RoundedCornerShape(8.dp)).background(Pc.Row), contentAlignment = Alignment.Center) {
                        state.thumbnail?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize()) }
                    }
                    Column(verticalArrangement = Arrangement.spacedBy(6.dp)) {
                        Text("Sprite ${shown + 1} · ${b.width} × ${b.height} px", color = Pc.TextDim, fontSize = 13.sp, maxLines = 1)
                        var wasFocused by remember { mutableStateOf(false) }
                        BasicTextField(
                            typed,
                            { typed = it.take(256) },
                            singleLine = true,
                            textStyle = TextStyle(color = Pc.Text, fontSize = 16.sp),
                            cursorBrush = SolidColor(Pc.Accent),
                            keyboardOptions = KeyboardOptions(imeAction = ImeAction.Done),
                            keyboardActions = KeyboardActions(onDone = { focus.clearFocus() }),
                            modifier = Modifier.width(240.dp).height(32.dp).clip(RoundedCornerShape(8.dp)).background(Pc.Row)
                                .onFocusChanged {
                                    if (wasFocused && !it.isFocused) rename(shown)
                                    wasFocused = it.isFocused
                                }
                                .padding(horizontal = 10.dp, vertical = 6.dp),
                        )
                    }
                }
            }
            val chosenAny = state.chosen.isNotEmpty()
            Column(
                Modifier.width(barWidth).height(130.dp).clip(RoundedCornerShape(33.dp)).background(Pc.Panel).then(Swallows)
                    .padding(start = 24.dp, end = 24.dp, top = 10.dp),
                verticalArrangement = Arrangement.spacedBy(10.dp),
            ) {
                Row(Modifier.fillMaxWidth().height(44.dp), verticalAlignment = Alignment.CenterVertically) {
                    Box(Modifier.width(110.dp).fillMaxHeight().tap { vm.closeTexturePacker() }, contentAlignment = Alignment.CenterStart) {
                        Text("Cancel", color = Pc.Text, fontSize = 17.sp)
                    }
                    val status = when (val n = state.chosen.size) {
                        0 -> "Tap drawings to choose them"
                        1 -> "1 sprite chosen"
                        else -> "$n sprites chosen"
                    }
                    Text(status, color = Pc.TextDim, fontSize = 15.sp, maxLines = 1, textAlign = TextAlign.Center, modifier = Modifier.weight(1f))
                    Box(
                        Modifier.width(110.dp).fillMaxHeight().tap {
                            // the name being typed kept first
                            state.focused?.let { rename(it) }
                            focus.clearFocus()
                            val sprites = vm.packedSprites(state)
                            if (sprites.isNotEmpty()) {
                                // made outside the packer, which may be closed meanwhile; cancelled
                                // or failed (that is told), nothing is shared
                                vm.viewModelScope.launch {
                                    val files = vm.exportSprites(sprites)
                                    if (files.isNotEmpty()) runCatching { shareFiles(context, files, "image/png") }
                                }
                            }
                        },
                        contentAlignment = Alignment.CenterEnd,
                    ) {
                        Text("Export", color = if (chosenAny) Pc.Accent else Pc.Disabled, fontSize = 17.sp, fontWeight = FontWeight.SemiBold)
                    }
                }
                Row(horizontalArrangement = Arrangement.spacedBy(20.dp)) {
                    OptionSlider("Separation", (state.separation - 1) / 39f, "${state.separation} px", slider, labelColor = PackerLabel) { v ->
                        val s = (v * 39).roundToInt() + 1
                        if (s != state.separation) {
                            state.separation = s
                            vm.packerResized(state, keepChosen = false)
                        }
                    }
                    OptionSlider(
                        "Padding",
                        state.padding / 32f,
                        if (state.padding == 0) "None" else "${state.padding} px",
                        slider,
                        labelColor = PackerLabel,
                    ) { v ->
                        val p = (v * 32).roundToInt()
                        if (p != state.padding) {
                            state.padding = p
                            vm.packerResized(state, keepChosen = true)
                        }
                    }
                }
            }
        }
    }
}

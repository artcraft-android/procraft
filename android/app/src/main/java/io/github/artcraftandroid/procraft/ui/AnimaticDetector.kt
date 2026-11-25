package io.github.artcraftandroid.procraft.ui

import android.content.Context
import android.graphics.Bitmap
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.LinearProgressIndicator
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableFloatStateOf
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.TimelineTool
import io.github.artcraftandroid.procraft.canvas.VideoFrameReader
import io.github.artcraftandroid.procraft.canvas.VideoFrames
import io.github.artcraftandroid.procraft.core.AnimaticHold
import io.github.artcraftandroid.procraft.core.animaticHolds
import io.github.artcraftandroid.procraft.core.animaticScores
import io.github.artcraftandroid.procraft.core.animaticThreshold
import io.github.artcraftandroid.procraft.core.animaticThumbnailSide
import java.nio.ByteBuffer
import kotlin.coroutines.coroutineContext
import kotlin.math.log10
import kotlin.math.roundToInt
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.ensureActive
import kotlinx.coroutines.withContext

// The animatic detector, as the iPad's AnimaticDetector.swift has it: for a video of held drawings
// (a storyboard or animatic export), where it changes drawing, so a track of new drawings can be
// made with its timing to draw over. Android decodes the clip's frames into small grey pictures;
// the core scores and cuts them.

/** Decoding a video clip's frames for the detector. */
internal object AnimaticFrames {
    class Read(val width: Int, val height: Int, val frames: List<ByteArray?>)

    /**
     * One grey picture ([side] at most along its longer edge) for each of [times] (seconds into the
     * file, rising): the frame on show then. One pass through the file; [progress] hears 0..1.
     * Null where it can't be decoded; a frame that couldn't be read is the one before it again.
     */
    suspend fun read(path: String, times: List<Double>, side: Int, progress: (Float) -> Unit): Read? {
        val frames = VideoFrames.open(path) ?: return null
        try {
            var w = 0
            var h = 0
            var current: ByteArray? = null
            var shown = -1
            val out = ArrayList<ByteArray?>(times.size)
            for ((i, time) in times.withIndex()) {
                coroutineContext.ensureActive()
                val at = frames.frameAt((maxOf(time, 0.0) * 1_000_000).toLong())
                if (at != shown) {
                    frames.read(at) { p, _ ->
                        if (w == 0) {
                            val scale = minOf(side.toFloat() / maxOf(p.width, p.height), 1f)
                            w = maxOf((p.width * scale).roundToInt(), 1)
                            h = maxOf((p.height * scale).roundToInt(), 1)
                        }
                        VideoFrameReader.luma(p, w, h)
                    }?.let {
                        current = it
                        shown = at
                    }
                }
                out.add(current)
                if (i % 16 == 0 || i == times.size - 1) progress((i + 1).toFloat() / times.size)
            }
            return if (w == 0) null else Read(w, h, out)
        } finally {
            frames.close()
        }
    }
}

/**
 * "Make Timing Track" for video clip [clip]: the clip's change scores as bars, the cuts they make at
 * the chosen sensitivity and shortest hold, the frame looked at; Make Track lays a new, empty
 * drawing on a track above the video for each hold.
 */
@Composable
internal fun AnimaticSheet(vm: CanvasViewModel, tool: TimelineTool, clip: ULong) {
    val session = vm.session ?: return
    val close = { tool.media.detecting = null }
    val prefs = LocalContext.current.getSharedPreferences("animatic", Context.MODE_PRIVATE)
    var scores by remember(clip) { mutableStateOf<List<Float>>(emptyList()) }
    var read by remember(clip) { mutableFloatStateOf(0f) }
    var failed by remember(clip) { mutableStateOf<String?>(null) }
    var refused by remember(clip) { mutableStateOf<String?>(null) }
    var sensitivity by remember { mutableFloatStateOf(prefs.getFloat(SENSITIVITY, 0.5f)) }
    var shortest by remember { mutableIntStateOf(prefs.getInt(SHORTEST, 2)) }
    var at by remember(clip) { mutableIntStateOf(0) }
    var picture by remember(clip) { mutableStateOf<ImageBitmap?>(null) }
    val media = remember(clip) { session.timelineMediaClip(clip) }
    val times = remember(clip) { session.timelineVideoTimes(clip) }
    val path = media?.path
    // the scores, read once
    LaunchedEffect(clip) {
        if (path == null) {
            failed = "The video's file can't be found."
            return@LaunchedEffect
        }
        val got = withContext(Dispatchers.Default) {
            AnimaticFrames.read(path, times, animaticThumbnailSide().toInt()) { p -> read = p }
                ?.let { animaticScores(it.frames, it.width.toUInt(), it.height.toUInt()) }
        }
        if (got == null) failed = "That video can't be read." else scores = got
    }
    // the frame looked at
    LaunchedEffect(clip, at) {
        if (path == null || at !in times.indices) return@LaunchedEffect
        picture = withContext(Dispatchers.Default) {
            VideoFrameReader.picture(path, times[at], 640u)?.let { t ->
                val b = Bitmap.createBitmap(t.width.toInt(), t.height.toInt(), Bitmap.Config.ARGB_8888)
                b.copyPixelsFromBuffer(ByteBuffer.wrap(t.rgba))
                b.asImageBitmap()
            }
        } ?: picture
    }
    val holds = remember(scores, sensitivity, shortest) { animaticHolds(scores, sensitivity, shortest.toUInt()) }
    // each drawing takes a layer: how many more the canvas has room for
    val left = remember(clip) { session.layerStats().let { it.maximum.toInt() - it.used.toInt() } }
    val fits = holds.size <= left
    val canMake = holds.isNotEmpty() && fits
    fun make(holds: List<AnimaticHold>) {
        if (session.makeTimingTrack(clip, holds) == 0uL) {
            // stays open, saying why, rather than closing on nothing
            refused = if (holds.size > left) tooMany(holds.size, left) else "The timing track couldn't be made."
            return
        }
        vm.refresh()
        close()
    }
    Dialog(onDismissRequest = close, properties = DialogProperties(usePlatformDefaultWidth = false)) {
        Column(
            Modifier.padding(16.dp).widthIn(max = 640.dp).fillMaxWidth().clip(RoundedCornerShape(22.dp)).background(Pc.PopoverFill).padding(20.dp),
            verticalArrangement = Arrangement.spacedBy(12.dp),
        ) {
            Row(Modifier.fillMaxWidth().padding(bottom = 4.dp), verticalAlignment = Alignment.CenterVertically) {
                Text("Cancel", color = Pc.PanelTitle, fontSize = 17.sp, modifier = Modifier.clickable(onClick = close))
                Text("Make Timing Track", color = Pc.PanelTitle, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, modifier = Modifier.weight(1f), textAlign = androidx.compose.ui.text.style.TextAlign.Center)
                Text(
                    "Make Track",
                    color = Pc.PanelTitle.copy(alpha = if (canMake) 1f else 0.4f),
                    fontSize = 17.sp,
                    fontWeight = FontWeight.SemiBold,
                    modifier = Modifier.clickable(enabled = canMake) { make(holds) },
                )
            }
            Box(Modifier.fillMaxWidth().height(260.dp).clip(RoundedCornerShape(14.dp)).background(Color.Black), contentAlignment = Alignment.Center) {
                picture?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxWidth().height(260.dp)) }
            }
            if (scores.isEmpty()) {
                Column(Modifier.height(120.dp), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                    Text(failed ?: "Looking for where the drawings change…", color = Pc.SlideLabel, fontSize = 15.sp)
                    if (failed == null) LinearProgressIndicator(progress = { read }, color = Pc.Accent, modifier = Modifier.fillMaxWidth())
                }
            } else {
                Text(summary(holds, at), color = Pc.SlideLabel, fontSize = 15.sp)
                (refused ?: if (fits) null else tooMany(holds.size, left))?.let { Text(it, color = Color(0xFFFF9F0A), fontSize = 15.sp) }
                ChangeGraph(scores, animaticThreshold(sensitivity), holds.drop(1).map { it.start.toInt() }, at) { at = it }
                SliderRow(
                    "Sensitivity",
                    sensitivity,
                    when {
                        sensitivity < 0.2f -> "Cuts only"
                        sensitivity > 0.8f -> "Every change"
                        else -> "${(sensitivity * 100).roundToInt()}%"
                    },
                    onEnd = { prefs.edit().putFloat(SENSITIVITY, sensitivity).apply() },
                ) {
                    sensitivity = it
                    refused = null
                }
                SliderRow(
                    "Shortest Hold",
                    (shortest - 1) / 23f,
                    if (shortest == 1) "1 frame" else "$shortest frames",
                    onEnd = { prefs.edit().putInt(SHORTEST, shortest).apply() },
                ) {
                    shortest = 1 + (it * 23).roundToInt()
                    refused = null
                }
            }
            Spacer(Modifier.height(0.dp))
        }
    }
}

private const val SENSITIVITY = "sensitivity"
private const val SHORTEST = "shortestHold"

private fun summary(holds: List<AnimaticHold>, at: Int): String {
    val n = holds.size
    val i = holds.indexOfLast { it.start.toInt() <= at }
    val here = if (i >= 0) "frame ${at + 1} is drawing ${i + 1}, held ${holds[i].length}" else ""
    return "$n ${if (n == 1) "drawing" else "drawings"} with the video's timing · $here"
}

/** Why [count] drawings can't be made with room for [left] more layers, and what to change. */
private fun tooMany(count: Int, left: Int): String {
    val room = if (left <= 0) "no room for more layers" else "room for $left more ${if (left == 1) "layer" else "layers"}"
    return "$count drawings take a layer each, and this canvas has $room. Lower the sensitivity or lengthen the shortest hold."
}

/** The change scores as bars on a log scale, the threshold across them, a tick at each cut and the frame looked at ([at]); dragged or tapped along to [look] at another. */
@Composable
private fun ChangeGraph(scores: List<Float>, threshold: Float, cuts: List<Int>, at: Int, look: (Int) -> Unit) {
    val ground = Pc.ActionsGroup
    val accent = Pc.Accent
    val dim = Pc.SlideLabel.copy(alpha = 0.4f)
    val n = maxOf(scores.size, 1)
    Canvas(
        Modifier.fillMaxWidth().height(96.dp).pointerInput(n) {
            awaitEachGesture {
                val down = awaitFirstDown()
                fun to(x: Float) = look((x / (size.width / n.toFloat())).toInt().coerceIn(0, n - 1))
                to(down.position.x)
                while (true) {
                    val c = awaitPointerEvent().changes.firstOrNull { it.id == down.id } ?: break
                    if (!c.pressed) break
                    c.consume()
                    to(c.position.x)
                }
            }
        },
    ) {
        val w = size.width
        val h = size.height
        val bar = w / n
        // a score's bar height: log scale from 0.0002 to 1
        fun height(s: Float): Float {
            val low = log10(0.0002f)
            val v = (log10(maxOf(s, 0.0002f)) - low) / -low
            return v.coerceIn(0f, 1f) * (h - 6f * density) + 2f * density
        }
        drawRoundRect(ground, Offset.Zero, size, CornerRadius(12f * density))
        for ((i, s) in scores.withIndex()) {
            val y = height(s)
            drawRect(if (s >= threshold) accent else dim, Offset(i * bar, h - y), Size(maxOf(bar - 1f, 1f), y))
        }
        for (c in cuts) drawRect(accent.copy(alpha = 0.6f), Offset(c * bar, 0f), Size(1f * density, h))
        drawRect(Color.White.copy(alpha = 0.8f), Offset(0f, h - height(threshold)), Size(w, 1.5f * density))
        drawRect(Color.White, Offset((at + 0.5f) * bar - density, 0f), Size(2f * density, h))
    }
}

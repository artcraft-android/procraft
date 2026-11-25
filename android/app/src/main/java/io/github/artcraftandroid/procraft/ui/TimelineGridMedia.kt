package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.Path
import androidx.compose.ui.graphics.Shadow
import androidx.compose.ui.graphics.drawscope.DrawScope
import androidx.compose.ui.graphics.drawscope.Stroke
import androidx.compose.ui.graphics.drawscope.clipRect
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.TextMeasurer
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.drawText
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.GridVideoPictures
import io.github.artcraftandroid.procraft.canvas.TimelineTool
import io.github.artcraftandroid.procraft.core.TimelineClipInfo
import io.github.artcraftandroid.procraft.core.TimelineClipKind
import io.github.artcraftandroid.procraft.core.TimelineMediaClip
import io.github.artcraftandroid.procraft.core.TimelineMediaKind
import io.github.artcraftandroid.procraft.core.mediaPeaksPerSecond
import kotlin.math.floor
import kotlin.math.ln
import kotlin.math.pow
import kotlin.math.roundToInt

// Sounds and videos on the grid timeline (see TimelineGrid.kt), as the iPad's
// TimelineGridMedia.swift and TimelineMediaViews.swift have them. A sound is a dark block with its
// waveform down the middle as bars; a video its own pictures laid side by side along it, its
// sound's waveform (while it still has one of its own) over their lower part. Both follow the
// zoom (everything is in terms of the grid's pitch): the bars keep their width and stand for more
// or less time, and there are more or fewer pictures. And what a sound's or a video's block menu
// has of its own: its level and its keys, muting it, taking a video's sound out, the animatic
// detector, where a video sits on the canvas, the part of its file it plays.

private object MediaLook {
    private fun pick(dark: Long, light: Long) = Color(if (Pc.light) light else dark)

    val SoundGround: Color get() = pick(0xFF303030, 0xFFBEBEBE)
    val VideoGround: Color get() = pick(0xFF3C3836, 0xFFB0ACA8)
    val InkDim: Color get() = pick(0x80C5C5C5, 0x80393939)
    val SoundWave: Color get() = pick(0xFF8FD6CF, 0xFF2F6E6A)

    /** From one waveform bar to the next, and a bar's own width (dp). */
    const val STEP = 3.5f
    const val BAR = 2f
}

/** How many waveform peaks a second of sound has (the core's). */
private val peaksPerSecond: Double by lazy { mediaPeaksPerSecond().toDouble() }

/**
 * Sound or video clip [c] over the frames it lasts, in [bounds] (pixels; the block's clip already
 * set), its frames [from] to [to] (counted from its first) those on the bar, [pitch] pixels from
 * one frame to the next; in its highlight colour [tint], if it has one.
 */
internal fun DrawScope.gridMediaBlock(
    tool: TimelineTool,
    c: TimelineClipInfo,
    bounds: Rect,
    from: Int,
    to: Int,
    pitch: Float,
    rowScale: Float,
    tint: Color?,
    measurer: TextMeasurer,
    inkBright: Color,
) {
    val info = tool.media.clip(c.id)
    val video = c.kind == TimelineClipKind.VIDEO
    // a video's waveform is its own sound's, while it has one
    val wave = info?.let { if (video) it.sound else it.media } ?: 0uL
    val peaks = if (wave != 0uL) tool.media.peaks(wave) else emptyList()
    val perSecond = maxOf(tool.info?.fps?.toInt() ?: 24, 1) * pitch
    val start = info?.trimStart ?: 0.0
    val d = density
    val h = bounds.height
    drawRect(tint ?: if (video) MediaLook.VideoGround else MediaLook.SoundGround, bounds.topLeft, bounds.size)
    val x0 = from * pitch
    val x1 = minOf((to + 1) * pitch, bounds.width)
    if (x1 > x0) {
        val path = info?.path
        if (video && info != null && path != null && info.width > 0u && info.height > 0u) {
            // (drawn again as its pictures come in)
            GridVideoPictures.read
            // its pictures side by side, each from the time it stands at: as wide as the picture
            // is for its height, to the nearest doubling of how long that lasts, so the same
            // pictures serve as the grid is zoomed a little
            val wide = h * info.width.toFloat() / info.height.toFloat()
            val lasts = 2.0.pow((ln(maxOf(wide / perSecond, 0.001f).toDouble()) / ln(2.0)).roundToInt())
            val tile = (lasts * perSecond).toFloat()
            var k = floor((start + x0 / perSecond) / lasts).toLong()
            while (true) {
                val at = k * lasts
                val x = ((at - start) * perSecond).toFloat()
                if (x >= x1) break
                // (no nearer than a fifth of a second: nearer takes the decoder longer and can't be
                // told apart at this size)
                GridVideoPictures.picture(path, at, maxOf(lasts / 2, 0.2))?.let { picture ->
                    // (filling its tile: what doesn't fit is cut off)
                    val scale = maxOf(tile / maxOf(picture.width, 1), h / maxOf(picture.height, 1))
                    val w = picture.width * scale
                    val ph = picture.height * scale
                    clipRect(bounds.left + x, bounds.top, bounds.left + x + tile, bounds.bottom) {
                        drawImage(
                            picture,
                            srcOffset = IntOffset.Zero,
                            srcSize = IntSize(picture.width, picture.height),
                            dstOffset = IntOffset((bounds.left + x + (tile - w) / 2f).roundToInt(), (bounds.top + (h - ph) / 2f).roundToInt()),
                            dstSize = IntSize(w.roundToInt(), ph.roundToInt()),
                        )
                    }
                }
                k++
            }
            // a film's edges
            val holes = Path()
            val every = 6f * d
            var hx = floor(x0 / every) * every + 3f * d
            val r = 0.8f * d
            while (hx < x1) {
                holes.addOval(Rect(Offset(bounds.left + hx, bounds.top + 1.6f * d + r), r))
                holes.addOval(Rect(Offset(bounds.left + hx, bounds.bottom - 3.2f * d + r), r))
                hx += every
            }
            drawPath(holes, Color.Black.copy(alpha = 0.55f))
        }
        if (peaks.isNotEmpty()) {
            // the waveform: a bar for each few points, as tall as the loudest of the sound it
            // stands for. A sound's down the middle; a video's along its lower part
            val step = MediaLook.STEP * d
            val bar = MediaLook.BAR * d
            val tall = if (video) h * 0.42f else h - 8f * d
            val mid = bounds.top + if (video) h - 3f * d - tall / 2f else h / 2f
            val bars = Path()
            var x = floor(x0 / step) * step
            while (x < x1) {
                val a = (start + x / perSecond) * peaksPerSecond
                val b = (start + (x + step) / perSecond) * peaksPerSecond
                val i = maxOf(a.toInt(), 0)
                val j = minOf(maxOf(b.toInt(), i + 1), peaks.size)
                if (i < j) {
                    var loudest = 0f
                    var n = i
                    val by = maxOf((j - i) / 48, 1)
                    while (n < j) {
                        loudest = maxOf(loudest, peaks[n])
                        n += by
                    }
                    val bh = maxOf(loudest.toDouble().pow(0.7).toFloat() * tall, 1.5f * d)
                    bars.addRoundRect(androidx.compose.ui.geometry.RoundRect(Rect(bounds.left + x, mid - bh / 2f, bounds.left + x + bar, mid + bh / 2f), CornerRadius(d)))
                }
                x += step
            }
            val muted = info?.muted == true
            drawPath(bars, if (video) Color.White.copy(alpha = if (muted) 0.4f else 0.92f) else inkBright.copy(alpha = if (muted) 0.3f else 0.72f))
        }
    }
    // its name, no further along than its first frame on the bar
    if (rowScale > 0.7f) {
        val style = TextStyle(
            color = if (video) Color.White else inkBright,
            fontSize = 9.5.sp,
            fontWeight = FontWeight.SemiBold,
            shadow = if (video) Shadow(Color.Black.copy(alpha = 0.8f), blurRadius = 1.5f * d) else null,
        )
        val laid = measurer.measure(c.name, style, maxLines = 1)
        drawText(laid, topLeft = Offset(bounds.left + from * pitch + 6f * d, bounds.top + 3f * d))
    }
}

// ---- A block's menu: what a sound or a video has of its own ----

/** A sound's level (keyed at the playhead once it has a key: the slider keys it there), its key at the playhead, and muting it. */
@Composable
internal fun GridSoundRows(tool: TimelineTool, clip: TimelineClipInfo, media: TimelineMediaClip, locked: Boolean) {
    val keyed = media.volumeKeys.isNotEmpty()
    GridMenuSlider(if (keyed) "Level here" else "Level", if (keyed) media.level else media.volume) { v, done ->
        if (keyed) tool.media.setLevel(media, v, done = done) else tool.media.setSound(media, volume = v, done = done)
    }
    // whether the playhead is on one of the block's frames
    val within = tool.frame >= clip.start && tool.frame < clip.start + clip.duration
    GridMenuRow(GridIcons.Key, if (media.keyedHere) "Remove level key" else "Key level here", dim = locked || !within) {
        if (media.keyedHere) tool.media.removeLevelKey(media) else tool.media.setLevel(media, media.level, key = true, done = true)
    }
    GridMenuRow(GridIcons.Mask, if (media.muted) "Unmute" else "Mute", dim = locked) { tool.media.setSound(media, muted = !media.muted, done = true) }
}

/** A video's own: its sound taken out, the animatic detector, and its size on the canvas ([sizeLit]: its card open). */
@Composable
internal fun GridVideoRows(tool: TimelineTool, clip: TimelineClipInfo, media: TimelineMediaClip, locked: Boolean, close: () -> Unit, sizeLit: Boolean, size: () -> Unit) {
    GridMenuRow(GridIcons.Options, "Separate audio and video", dim = locked || media.sound == 0uL) {
        close()
        tool.media.separateAudio(clip.id)
    }
    // the animatic detector, while Timeline media's switch for it is on
    if (tool.shows(io.github.artcraftandroid.procraft.core.Feature.ANIMATIC_DETECTOR)) {
        GridMenuRow(GridIcons.Insert, "Make timing track…") {
            close()
            tool.stopPlaying()
            tool.media.detecting = clip.id
        }
    }
    GridMenuRow(GridIcons.Blend, "Size", chevron = true, lit = sizeLit, action = size)
}

/** Where a video sits on the canvas. */
private enum class Placement(val label: String) {
    FIT("Fit Canvas"), FILL("Fill Canvas"), ACTUAL("Actual Size");

    /** [c] placed so on a [cw] x [ch] canvas: left, top, width, height; none for a video with no size. */
    fun rect(c: TimelineMediaClip, cw: Float, ch: Float): List<Float>? {
        val w = c.width.toFloat()
        val h = c.height.toFloat()
        if (w <= 0f || h <= 0f) return null
        val scale = when (this) {
            FIT -> minOf(cw / w, ch / h)
            FILL -> maxOf(cw / w, ch / h)
            ACTUAL -> 1f
        }
        val sw = w * scale
        val sh = h * scale
        return listOf((cw - sw) / 2f, (ch - sh) / 2f, sw, sh)
    }
}

/** The video's place on the canvas: fitted, filling it, or at its own size, the one it has ticked. */
@Composable
internal fun GridVideoSizeCard(vm: CanvasViewModel, tool: TimelineTool, clip: TimelineClipInfo, media: TimelineMediaClip) {
    val (cw, ch) = vm.canvasSize()
    GridCard {
        for (p in Placement.entries) {
            val rect = p.rect(media, cw.toFloat(), ch.toFloat())
            val chosen = rect != null && rect.size == media.rect.size && rect.zip(media.rect).all { (a, b) -> kotlin.math.abs(a - b) < 0.5f }
            GridChoiceRow(p.label, chosen = chosen) { if (rect != null) tool.media.placeVideo(clip.id, rect, done = true) }
        }
    }
}

/** The part of its file a sound or video plays, dragged along the whole file; a sound's start nudged a part of a frame later. */
@Composable
internal fun GridSlipCard(tool: TimelineTool, clip: TimelineClipInfo, media: TimelineMediaClip) {
    val fps = maxOf(tool.info?.fps?.toInt() ?: 12, 1).toDouble()
    GridCard(300f) {
        Text("Drag to play another part of the file", color = Grid.Ink, fontSize = 13.sp, modifier = Modifier.padding(horizontal = 16.dp, vertical = 6.dp))
        SlipStrip(tool, media, clip.duration.toDouble() / fps)
        // a sound's start a part of a frame later, for sync finer than a frame
        if (media.kind == TimelineMediaKind.AUDIO) {
            GridMenuSlider("Nudge", (media.offset * fps).coerceIn(0.0, 1.0).toFloat(), text = { "${(it / fps * 1000).roundToInt()} ms" }) { v, done ->
                tool.media.setSound(media, offset = v / fps, done = done)
            }
        }
    }
}

/** The whole file as a strip, the part the clip plays ([plays] seconds) lit: dragged along, the clip plays another part of its file where it is (a slip). */
@Composable
private fun SlipStrip(tool: TimelineTool, c: TimelineMediaClip, plays: Double) {
    var moved by remember(c.clip) { mutableStateOf<Double?>(null) }
    val now by rememberUpdatedState(c)
    val peaks = if (c.kind == TimelineMediaKind.AUDIO) tool.media.peaks(c.media) else emptyList()
    val ground = Pc.ActionsGroup
    val accent = Pc.Accent
    val dim = MediaLook.InkDim
    val lit = MediaLook.SoundWave
    Canvas(
        Modifier.padding(horizontal = 12.dp).padding(bottom = 6.dp).fillMaxWidth().height(54.dp)
            .pointerInput(c.clip) {
                var dragged = 0f
                detectHorizontalDragGestures(
                    onDragStart = { dragged = 0f },
                    onDragEnd = {
                        moved?.let { tool.media.slip(now.clip, it, done = true) }
                        moved = null
                    },
                    onDragCancel = { moved = null },
                ) { change, dx ->
                    change.consume()
                    dragged += dx
                    val whole = maxOf(now.mediaSeconds, 0.001)
                    moved = (now.trimStart + dragged / size.width * whole).coerceIn(0.0, maxOf(whole - plays, 0.0))
                }
            },
    ) {
        val w = size.width
        val whole = maxOf(c.mediaSeconds, 0.001)
        val start = moved ?: c.trimStart
        val litW = maxOf((plays / whole).toFloat() * w, 4f * density)
        val x = minOf((start / whole).toFloat() * w, maxOf(w - litW, 0f))
        drawRoundRect(ground, Offset.Zero, size, CornerRadius(10f * density))
        val inset = 8f * density
        if (peaks.isNotEmpty()) waveform(peaks, 0.0, whole, Rect(0f, inset, w, size.height - inset), dim)
        val box = Size(minOf(litW, w), size.height)
        drawRoundRect(accent.copy(alpha = 0.25f), Offset(x, 0f), box, CornerRadius(8f * density))
        drawRoundRect(accent, Offset(x + density, density), Size(box.width - 2f * density, box.height - 2f * density), CornerRadius(7f * density), style = Stroke(2f * density))
        if (peaks.isNotEmpty()) waveform(peaks, start, minOf(plays, whole - start), Rect(x, inset, x + box.width, size.height - inset), lit)
    }
}

/** A sound's waveform from [from] seconds into it, for [seconds], mirrored about the middle of [rect]: a bar every three points, each the loudest peak it covers. */
private fun DrawScope.waveform(peaks: List<Float>, from: Double, seconds: Double, rect: Rect, color: Color) {
    if (seconds <= 0 || rect.width <= 0f) return
    val bars = maxOf((rect.width / (3f * density)).toInt(), 1)
    val path = Path()
    for (b in 0 until bars) {
        val t0 = from + seconds * b / bars
        val t1 = from + seconds * (b + 1) / bars
        val i0 = maxOf((t0 * peaksPerSecond).toInt(), 0)
        val i1 = minOf(maxOf((t1 * peaksPerSecond).toInt(), i0 + 1), peaks.size)
        if (i0 >= i1) continue
        var loud = 0f
        for (i in i0 until i1) loud = maxOf(loud, peaks[i])
        val h = maxOf(loud * rect.height / 2f, 0.5f * density)
        val x = rect.left + b * rect.width / bars
        path.addRoundRect(androidx.compose.ui.geometry.RoundRect(Rect(x, rect.center.y - h, x + 2f * density, rect.center.y + h), CornerRadius(density)))
    }
    drawPath(path, color)
}

package io.github.artcraftandroid.procraft.canvas

import android.graphics.Bitmap
import android.media.MediaMetadataRetriever
import android.os.Build
import android.os.Handler
import android.os.Looper
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import io.github.artcraftandroid.procraft.AppLog
import io.github.artcraftandroid.procraft.core.ThumbnailImage
import java.io.File
import java.util.concurrent.Executors
import kotlin.math.roundToInt
import io.github.artcraftandroid.procraft.core.VideoFrames as CoreVideoFrames

// Videos' pictures on Android's decoders: those the canvas draws (the core asks for them, see
// `VideoFrames` in the core's media.rs), read on from frame to frame by [VideoFrames] as the
// times asked go steadily on, and a video strip's small ones on the grid. The iPad's
// VideoFrameReader (TimelineMedia.swift) and GridVideoPictures (TimelineGridMedia.swift).

/**
 * Videos' pictures for the canvas, scaled to the side asked for: each video's decoder goes on from
 * the frame it last gave to a later one near it, and seeks back to a key frame for any other (see
 * [VideoFrames.read]). Either way the picture is the frame showing at the time asked. A video the
 * decoder here can't read is left to the system's reader. Called from the canvas's own threads.
 */
object VideoFrameReader : CoreVideoFrames {
    /** How many videos keep a decoder open: past this, the one asked for longest ago lets its go. */
    private const val OPEN_MOST = 4

    /** The decoders open, by file, the one asked for last at the end (none: the system's reader reads it). */
    private val open = LinkedHashMap<String, VideoFrames?>(8, 0.75f, true)

    @Synchronized
    override fun picture(path: String, seconds: Double, maxSide: UInt): ThumbnailImage? {
        val side = maxSide.toInt()
        val us = (maxOf(seconds, 0.0) * 1_000_000).toLong()
        val frames = if (open.containsKey(path)) open[path] else VideoFrames.open(path).also { opened(path, it) }
        if (frames != null) {
            try {
                frames.read(frames.frameAt(us)) { p, m -> rgba(p, m, frames.rotation, side) }?.let { return it }
            } catch (e: Exception) {
                AppLog.w("ProCraft", "a video's decoder gave up: ${File(path).name}", e)
                frames.close()
                open[path] = null
            }
        }
        return retrieved(path, us, side)
    }

    private fun opened(path: String, frames: VideoFrames?) {
        open[path] = frames
        while (open.size > OPEN_MOST) {
            val (eldest, f) = open.entries.first()
            f?.close()
            open.remove(eldest)
        }
    }

    /** The frame showing [us] in, by the system's reader (turned as the video shows), as [rgba] gives it. */
    private fun retrieved(path: String, us: Long, side: Int): ThumbnailImage? {
        val bitmap = frameBitmap(path, us, side, exact = true) ?: return null
        val pixels = IntArray(bitmap.width * bitmap.height)
        bitmap.getPixels(pixels, 0, bitmap.width, 0, 0, bitmap.width, bitmap.height)
        val out = ByteArray(pixels.size * 4)
        for (i in pixels.indices) {
            val c = pixels[i]
            out[4 * i] = (c shr 16).toByte()
            out[4 * i + 1] = (c shr 8).toByte()
            out[4 * i + 2] = c.toByte()
            out[4 * i + 3] = 255.toByte()
        }
        return ThumbnailImage(bitmap.width.toUInt(), bitmap.height.toUInt(), out).also { bitmap.recycle() }
    }

    /** A decoded frame's picture as straight RGBA8, turned [rotation] degrees clockwise as the video shows it, fitted to [side] pixels (0: as it is). */
    internal fun rgba(p: YuvPicture, m: Yuv.Matrix, rotation: Int, side: Int): ThumbnailImage {
        val sideways = rotation == 90 || rotation == 270
        val sw = if (sideways) p.height else p.width
        val sh = if (sideways) p.width else p.height
        val scale = if (side > 0) minOf(side.toFloat() / maxOf(sw, sh), 1f) else 1f
        val ow = maxOf((sw * scale).roundToInt(), 1)
        val oh = maxOf((sh * scale).roundToInt(), 1)
        val out = ByteArray(ow * oh * 4)
        val w = p.width
        val h = p.height
        for (oy in 0 until oh) {
            val v = ((oy + 0.5f) * sh / oh).toInt()
            for (ox in 0 until ow) {
                val u = ((ox + 0.5f) * sw / ow).toInt()
                val (sx, sy) = when (rotation) {
                    90 -> v to h - 1 - u
                    180 -> w - 1 - u to h - 1 - v
                    270 -> w - 1 - v to u
                    else -> u to v
                }
                val c = sampleArgb(p, m, sx.coerceIn(0, w - 1), sy.coerceIn(0, h - 1))
                val o = (oy * ow + ox) * 4
                out[o] = (c shr 16).toByte()
                out[o + 1] = (c shr 8).toByte()
                out[o + 2] = c.toByte()
                out[o + 3] = 255.toByte()
            }
        }
        return ThumbnailImage(ow.toUInt(), oh.toUInt(), out)
    }

    /** The colour of [p]'s picture at [x], [y] (within it), by [m]. */
    private fun sampleArgb(p: YuvPicture, m: Yuv.Matrix, x: Int, y: Int): Int {
        val sx = p.left + x
        val sy = p.top + y
        return m.argb(byte(p.y, sx, sy), byte(p.u, sx shr 1, sy shr 1), byte(p.v, sx shr 1, sy shr 1))
    }

    private fun byte(plane: YuvPlane, x: Int, y: Int): Int {
        val at = y * plane.rowStride + x * plane.pixelStride
        return if (at < plane.buffer.limit()) plane.buffer.get(at).toInt() and 255 else 0
    }

    /**
     * The lightness of [p]'s picture [w] x [h] (its own way up), each sample the average of four
     * points in its share: what the animatic detector compares.
     */
    internal fun luma(p: YuvPicture, w: Int, h: Int): ByteArray {
        val out = ByteArray(w * h)
        for (oy in 0 until h) for (ox in 0 until w) {
            var sum = 0
            for (k in 0 until 4) {
                val x = (((ox + 0.25f + 0.5f * (k and 1)) * p.width / w).toInt()).coerceIn(0, p.width - 1)
                val y = (((oy + 0.25f + 0.5f * (k shr 1)) * p.height / h).toInt()).coerceIn(0, p.height - 1)
                sum += byte(p.y, p.left + x, p.top + y)
            }
            out[oy * w + ox] = (sum / 4).toByte()
        }
        return out
    }
}

/**
 * The frame of the video at [path] showing [us] microseconds in, by the system's reader, turned as
 * the video shows and no bigger than [side] along its longer edge (0: as it is): [exact], the
 * frame then; else the key frame nearest it, much sooner found. Null if it can't be read.
 */
internal fun frameBitmap(path: String, us: Long, side: Int, exact: Boolean): Bitmap? {
    val r = MediaMetadataRetriever()
    return try {
        r.setDataSource(path)
        val option = if (exact) MediaMetadataRetriever.OPTION_CLOSEST else MediaMetadataRetriever.OPTION_CLOSEST_SYNC
        val w = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)?.toIntOrNull() ?: 0
        val h = r.extractMetadata(MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)?.toIntOrNull() ?: 0
        val scale = if (side > 0 && w > 0 && h > 0) minOf(side.toFloat() / maxOf(w, h), 1f) else 1f
        if (scale < 1f && Build.VERSION.SDK_INT >= 27) {
            r.getScaledFrameAtTime(us, option, maxOf((w * scale).roundToInt(), 1), maxOf((h * scale).roundToInt(), 1))
        } else {
            r.getFrameAtTime(us, option)?.let { b ->
                val s = if (side > 0) minOf(side.toFloat() / maxOf(b.width, b.height), 1f) else 1f
                if (s >= 1f) b else Bitmap.createScaledBitmap(b, maxOf((b.width * s).roundToInt(), 1), maxOf((b.height * s).roundToInt(), 1), true).also { b.recycle() }
            }
        }
    } catch (e: Exception) {
        AppLog.w("ProCraft", "a video's frame couldn't be read: ${File(path).name}", e)
        null
    } finally {
        runCatching { r.release() }
    }
}

/**
 * A video's pictures for its block on the grid, small, read as they are wanted and kept: a block
 * asks for the picture at a time and draws it once it is there.
 *
 * They are read one at a time, the one asked for last first, and only so many wait: the grid slid
 * quickly along a long video asks for many it has passed by the time they could be read. One that
 * waited too long is dropped, and asked for again if its block shows it again.
 */
object GridVideoPictures {
    private data class Key(val path: String, /** (hundredths of a second into the file) */ val at: Int)

    /** How many are kept: past this, those read longest ago are let go, to be read again if shown. */
    private const val MOST = 600

    /** How many wait to be read at most. */
    private const val WAITING_MOST = 40

    /** The longest side they are read at. */
    private const val SIDE = 220

    private val kept = LinkedHashMap<Key, ImageBitmap>()
    private val asked = HashSet<Key>()

    /** Those waiting to be read, the one asked for last at the end, each with how far from its time the picture may be (seconds). */
    private val waiting = ArrayDeque<Pair<Key, Double>>()
    private var reading = false
    private val reader = Executors.newSingleThreadExecutor { Thread(it, "procraft-grid-video").apply { priority = Thread.MIN_PRIORITY } }
    private val main = Handler(Looper.getMainLooper())

    /** How many have been read: a block showing them is drawn again as it changes. */
    var read by mutableIntStateOf(0)
        private set

    /**
     * The picture [seconds] into the video at [path], if it has been read; asked for, else (on the
     * main thread). One from up to [within] seconds either side will do: a second or more, the
     * nearest key frame, which the decoder finds much sooner.
     */
    fun picture(path: String, seconds: Double, within: Double): ImageBitmap? {
        val key = Key(path, (maxOf(seconds, 0.0) * 100).roundToInt())
        kept[key]?.let { return it }
        if (!asked.add(key)) return null
        waiting.addLast(key to within)
        if (waiting.size > WAITING_MOST) asked.remove(waiting.removeFirst().first)
        readNext()
        return null
    }

    private fun readNext() {
        if (reading) return
        val (key, within) = waiting.removeLastOrNull() ?: return
        reading = true
        reader.execute {
            val bitmap = frameBitmap(key.path, key.at * 10_000L, SIDE, exact = within < 1.0)
            main.post {
                reading = false
                if (bitmap != null) {
                    kept[key] = bitmap.asImageBitmap()
                    if (kept.size > MOST) {
                        val old = kept.keys.take(MOST / 4)
                        for (k in old) {
                            kept.remove(k)
                            asked.remove(k)
                        }
                    }
                    read++
                }
                readNext()
            }
        }
    }
}

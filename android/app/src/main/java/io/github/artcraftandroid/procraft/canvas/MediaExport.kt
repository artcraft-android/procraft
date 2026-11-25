package io.github.artcraftandroid.procraft.canvas

import android.media.MediaCodec
import android.media.MediaCodecInfo
import android.media.MediaExtractor
import android.media.MediaFormat
import android.media.MediaMuxer
import io.github.artcraftandroid.procraft.AppLog
import io.github.artcraftandroid.procraft.core.CanvasSession
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder

// The timeline's sounds in an exported video, as the iPad's MediaExport.swift has them: mixed under
// the frames once they are written, each at its time and level, encoded as AAC and put beside the
// video's own track in a new file. Only video carries sound; GIF and PNG exports are left as they
// are.

object MediaExport {
    private const val RATE = 48_000
    private const val BIT_RATE = 192_000
    /** Frames of sound given the encoder at once. */
    private const val PIECE = 1024
    private const val TIMEOUT_US = 10_000L

    /** One encoded piece of the sound. */
    private class Piece(val bytes: ByteArray, val time: Long, val flags: Int)

    /**
     * The sounds of [session]'s timeline mixed under the video at [file] (its frames written at
     * [fps] a second), in place, their samples from [bank]. True when there was nothing to add or
     * it was added.
     */
    fun addSounds(file: File, session: CanvasSession, fps: Float, bank: SoundBank): Boolean {
        val sounds = session.timelineSounds(0u).filter { it.audible }
        val timelineFps = session.timelineInfo()?.fps ?: return true
        if (sounds.isEmpty()) return true
        // played at another rate than the timeline's, the sounds start where their frames are shown
        val scale = timelineFps.toDouble() / maxOf(fps.toDouble(), 1.0)
        val extractor = MediaExtractor()
        try {
            extractor.setDataSource(file.path)
            val track = (0 until extractor.trackCount).firstOrNull {
                extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("video/") == true
            } ?: return false
            val video = extractor.getTrackFormat(track)
            val length = if (video.containsKey(MediaFormat.KEY_DURATION)) video.getLong(MediaFormat.KEY_DURATION) / 1e6 else return false
            val mix = SoundMix(sounds, timelineFps.toDouble(), scale) { bank.now(it) }
            val pieces = encode(mix, length) ?: return false
            val out = File(file.parentFile, "with-sound-${file.name}")
            if (!mux(extractor, track, video, pieces.first, pieces.second, out)) {
                out.delete()
                return false
            }
            if (!out.renameTo(file)) {
                out.delete()
                return false
            }
            return true
        } catch (e: Exception) {
            AppLog.w("ProCraft", "adding the timeline's sounds to a video", e)
            return false
        } finally {
            extractor.release()
        }
    }

    /** [mix]'s first [seconds] encoded as AAC: its format and its pieces. */
    private fun encode(mix: SoundMix, seconds: Double): Pair<MediaFormat, List<Piece>>? {
        val format = MediaFormat.createAudioFormat(MediaFormat.MIMETYPE_AUDIO_AAC, RATE, 2).apply {
            setInteger(MediaFormat.KEY_AAC_PROFILE, MediaCodecInfo.CodecProfileLevel.AACObjectLC)
            setInteger(MediaFormat.KEY_BIT_RATE, BIT_RATE)
            setInteger(MediaFormat.KEY_MAX_INPUT_SIZE, PIECE * 4 * 2)
        }
        val codec = MediaCodec.createEncoderByType(MediaFormat.MIMETYPE_AUDIO_AAC)
        val total = (seconds * RATE).toLong()
        val floats = FloatArray(PIECE * 2)
        val pieces = ArrayList<Piece>()
        var made: MediaFormat? = null
        val info = MediaCodec.BufferInfo()
        try {
            codec.configure(format, null, null, MediaCodec.CONFIGURE_FLAG_ENCODE)
            codec.start()
            var given = 0L
            var ended = false
            val started = System.nanoTime()
            while (true) {
                if (System.nanoTime() - started > 600_000_000_000L) return null.also { AppLog.w("ProCraft", "the sound's encoder took too long") }
                if (!ended) {
                    val i = codec.dequeueInputBuffer(TIMEOUT_US)
                    if (i >= 0) {
                        val input = codec.getInputBuffer(i) ?: return null
                        val time = given * 1_000_000L / RATE
                        if (given >= total) {
                            codec.queueInputBuffer(i, 0, 0, time, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                            ended = true
                        } else {
                            val n = minOf(PIECE.toLong(), total - given, (input.capacity() / 4).toLong()).toInt()
                            mix.render(floats, n, given.toDouble() / RATE, RATE)
                            input.clear()
                            input.order(ByteOrder.nativeOrder())
                            for (k in 0 until n * 2) input.putShort((floats[k] * 32767f).toInt().toShort())
                            codec.queueInputBuffer(i, 0, n * 4, time, 0)
                            given += n
                        }
                    }
                }
                val o = codec.dequeueOutputBuffer(info, TIMEOUT_US)
                if (o == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                    made = codec.outputFormat
                } else if (o >= 0) {
                    val buffer = codec.getOutputBuffer(o)
                    if (buffer != null && info.size > 0 && info.flags and MediaCodec.BUFFER_FLAG_CODEC_CONFIG == 0) {
                        val bytes = ByteArray(info.size)
                        buffer.position(info.offset)
                        buffer.get(bytes)
                        pieces.add(Piece(bytes, info.presentationTimeUs, info.flags))
                    }
                    codec.releaseOutputBuffer(o, false)
                    if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) break
                }
            }
        } finally {
            runCatching { codec.stop() }
            codec.release()
        }
        return made?.let { it to pieces }
    }

    /** The video track [track] of [extractor] ([video] its format) and the sound's [pieces] ([sound] their format) written together at [out], in time order. */
    private fun mux(extractor: MediaExtractor, track: Int, video: MediaFormat, sound: MediaFormat, pieces: List<Piece>, out: File): Boolean {
        val muxer = MediaMuxer(out.path, MediaMuxer.OutputFormat.MUXER_OUTPUT_MPEG_4)
        var started = false
        var stopped = false
        try {
            val pictures = muxer.addTrack(video)
            val heard = muxer.addTrack(sound)
            if (video.containsKey(MediaFormat.KEY_ROTATION)) muxer.setOrientationHint(video.getInteger(MediaFormat.KEY_ROTATION))
            muxer.start()
            started = true
            val most = maxOf(
                if (video.containsKey(MediaFormat.KEY_MAX_INPUT_SIZE)) video.getInteger(MediaFormat.KEY_MAX_INPUT_SIZE) else 0,
                video.getInteger(MediaFormat.KEY_WIDTH) * video.getInteger(MediaFormat.KEY_HEIGHT) * 3 / 2,
            )
            val buffer = ByteBuffer.allocate(most)
            val info = MediaCodec.BufferInfo()
            extractor.selectTrack(track)
            extractor.seekTo(0, MediaExtractor.SEEK_TO_CLOSEST_SYNC)
            var next = 0
            fun soundUntil(time: Long) {
                while (next < pieces.size && pieces[next].time <= time) {
                    val p = pieces[next++]
                    info.set(0, p.bytes.size, p.time, p.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM.inv())
                    muxer.writeSampleData(heard, ByteBuffer.wrap(p.bytes), info)
                }
            }
            while (true) {
                val size = extractor.readSampleData(buffer, 0)
                if (size < 0) break
                val time = extractor.sampleTime
                soundUntil(time)
                val key = extractor.sampleFlags and MediaExtractor.SAMPLE_FLAG_SYNC != 0
                info.set(0, size, time, if (key) MediaCodec.BUFFER_FLAG_KEY_FRAME else 0)
                muxer.writeSampleData(pictures, buffer, info)
                extractor.advance()
            }
            soundUntil(Long.MAX_VALUE)
            muxer.stop()
            stopped = true
            return true
        } catch (e: Exception) {
            AppLog.w("ProCraft", "writing a video with its sound", e)
            return false
        } finally {
            runCatching { if (started && !stopped) muxer.stop() }
            runCatching { muxer.release() }
        }
    }
}

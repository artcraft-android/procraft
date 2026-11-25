package io.github.artcraftandroid.procraft.canvas

import android.media.AudioFormat
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import io.github.artcraftandroid.procraft.AppLog
import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.nio.ShortBuffer
import java.nio.channels.FileChannel

// Sounds as the timeline's media uses them, on Android's decoders: a file's sound decoded a piece
// at a time (a sound file's, or a video's own), written out as a WAV file of 16-bit samples, its
// waveform's peaks read on the way; and such a WAV file read back by mapping it, for playing.

/**
 * The sound of the file at [path] (its first sound track) decoded, each piece of it handed to
 * [take] as 16-bit samples, its channels taking turns (no more than two: more are cut to the first
 * two), with how many channels there are and the rate. False if it has no sound or can't be
 * decoded.
 */
internal fun decodeSound(path: String, take: (samples: ShortArray, count: Int, channels: Int, rate: Int) -> Unit): Boolean {
    val extractor = MediaExtractor()
    var codec: MediaCodec? = null
    return try {
        extractor.setDataSource(path)
        val track = (0 until extractor.trackCount).firstOrNull {
            extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true
        } ?: return false
        extractor.selectTrack(track)
        val format = extractor.getTrackFormat(track)
        var rate = format.getInteger(MediaFormat.KEY_SAMPLE_RATE)
        var channels = format.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
        var encoding = AudioFormat.ENCODING_PCM_16BIT
        val decoder = MediaCodec.createDecoderByType(format.getString(MediaFormat.KEY_MIME) ?: return false)
        codec = decoder
        decoder.configure(format, null, null, 0)
        decoder.start()
        val info = MediaCodec.BufferInfo()
        var out = ShortArray(0)
        var fed = false
        var given = 0L
        while (true) {
            if (!fed) {
                val i = decoder.dequeueInputBuffer(10_000)
                if (i >= 0) {
                    val input = decoder.getInputBuffer(i) ?: return false
                    val read = extractor.readSampleData(input, 0)
                    if (read < 0) {
                        decoder.queueInputBuffer(i, 0, 0, 0, MediaCodec.BUFFER_FLAG_END_OF_STREAM)
                        fed = true
                    } else {
                        decoder.queueInputBuffer(i, 0, read, extractor.sampleTime, 0)
                        extractor.advance()
                    }
                }
            }
            val o = decoder.dequeueOutputBuffer(info, 10_000)
            if (o == MediaCodec.INFO_OUTPUT_FORMAT_CHANGED) {
                val now = decoder.outputFormat
                rate = now.getInteger(MediaFormat.KEY_SAMPLE_RATE)
                channels = now.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
                if (now.containsKey(MediaFormat.KEY_PCM_ENCODING)) encoding = now.getInteger(MediaFormat.KEY_PCM_ENCODING)
            } else if (o >= 0) {
                val buffer = decoder.getOutputBuffer(o) ?: return false
                buffer.position(info.offset)
                buffer.limit(info.offset + info.size)
                buffer.order(ByteOrder.nativeOrder())
                val kept = minOf(channels, 2)
                val frames = when (encoding) {
                    AudioFormat.ENCODING_PCM_FLOAT -> info.size / 4 / maxOf(channels, 1)
                    AudioFormat.ENCODING_PCM_16BIT -> info.size / 2 / maxOf(channels, 1)
                    else -> return false.also { AppLog.w("ProCraft", "a sound's samples come as $encoding, which isn't read here") }
                }
                if (out.size < frames * kept) out = ShortArray(frames * kept)
                if (encoding == AudioFormat.ENCODING_PCM_FLOAT) {
                    val b = buffer.asFloatBuffer()
                    for (f in 0 until frames) for (c in 0 until channels) {
                        val v = b.get()
                        if (c < kept) out[f * kept + c] = (v.coerceIn(-1f, 1f) * 32767f).toInt().toShort()
                    }
                } else {
                    val b = buffer.asShortBuffer()
                    for (f in 0 until frames) for (c in 0 until channels) {
                        val v = b.get()
                        if (c < kept) out[f * kept + c] = v
                    }
                }
                decoder.releaseOutputBuffer(o, false)
                if (frames > 0) {
                    take(out, frames * kept, kept, rate)
                    given += frames
                }
                if (info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0) break
            }
        }
        given > 0
    } catch (e: Exception) {
        AppLog.w("ProCraft", "a sound couldn't be decoded: ${File(path).name}", e)
        false
    } finally {
        runCatching { codec?.stop() }
        runCatching { codec?.release() }
        extractor.release()
    }
}

/** A sound decoded into a WAV file, with what was learnt on the way. */
internal class DecodedSound(val seconds: Double, val peaks: List<Float>)

/**
 * The sound of the file at [path] written to [to] as a WAV file of 16-bit samples (one or two
 * channels, its own rate), and its waveform: the loudest sample in each of [perSecond]ths of a
 * second, 0..1. Null if it has no sound or can't be decoded ([to] is then gone).
 */
internal fun decodeSoundTo(path: String, to: File, perSecond: Int): DecodedSound? {
    val peaks = ArrayList<Float>()
    var frames = 0L
    var rate = 0
    var channels = 0
    var loudest = 0
    var filled = 0
    var bucket = 1
    val ok = RandomAccessFile(to, "rw").use { file ->
        file.setLength(0)
        file.write(ByteArray(WAV_HEADER))
        var bytes = ByteArray(0)
        val decoded = decodeSound(path) { samples, count, ch, r ->
            // (the rate and channels are the first piece's: a decoder says them before any)
            if (rate == 0) {
                rate = r
                channels = ch
                bucket = maxOf(r / perSecond, 1)
            }
            if (ch != channels) return@decodeSound
            if (bytes.size < count * 2) bytes = ByteArray(count * 2)
            for (i in 0 until count) {
                val v = samples[i].toInt()
                bytes[2 * i] = v.toByte()
                bytes[2 * i + 1] = (v shr 8).toByte()
            }
            file.write(bytes, 0, count * 2)
            for (f in 0 until count / ch) {
                for (c in 0 until ch) loudest = maxOf(loudest, kotlin.math.abs(samples[f * ch + c].toInt()))
                if (++filled == bucket) {
                    peaks.add(minOf(loudest / 32767f, 1f))
                    loudest = 0
                    filled = 0
                }
            }
            frames += count / ch
        }
        if (decoded && frames > 0) {
            if (filled > 0) peaks.add(minOf(loudest / 32767f, 1f))
            file.seek(0)
            file.write(wavHeader(frames * channels * 2, channels, rate))
        }
        decoded && frames > 0
    }
    if (!ok) {
        to.delete()
        return null
    }
    return DecodedSound(frames.toDouble() / rate, peaks)
}

private const val WAV_HEADER = 44

/** A WAV file's header for [dataBytes] of 16-bit samples, [channels] taking turns, [rate] a second. */
private fun wavHeader(dataBytes: Long, channels: Int, rate: Int): ByteArray {
    val out = ByteBuffer.allocate(WAV_HEADER).order(ByteOrder.LITTLE_ENDIAN)
    val data = dataBytes.coerceAtMost(Int.MAX_VALUE.toLong() - 36).toInt()
    out.put("RIFF".toByteArray()).putInt(36 + data).put("WAVEfmt ".toByteArray()).putInt(16)
    out.putShort(1).putShort(channels.toShort()).putInt(rate).putInt(rate * channels * 2).putShort((channels * 2).toShort()).putShort(16)
    out.put("data".toByteArray()).putInt(data)
    return out.array()
}

/** A sound's samples to play: 16-bit, [channels] (one or two) taking turns, [rate] a second. */
internal class Pcm(val samples: ShortBuffer, val channels: Int, val rate: Int) {
    val frames: Int = samples.capacity() / channels

    companion object {
        /** The WAV file at [file] mapped, if it is one of 16-bit samples in one or two channels; else null. */
        fun mapped(file: File): Pcm? = runCatching {
            RandomAccessFile(file, "r").use { raf ->
                val map = raf.channel.map(FileChannel.MapMode.READ_ONLY, 0, raf.length()).order(ByteOrder.LITTLE_ENDIAN)
                if (map.remaining() < 12 || map.getInt(0) != RIFF || map.getInt(8) != WAVE) return@use null
                var at = 12
                var channels = 0
                var rate = 0
                var bits = 0
                var kind = 0
                while (at + 8 <= map.limit()) {
                    val id = map.getInt(at)
                    val size = map.getInt(at + 4)
                    val body = at + 8
                    if (id == FMT && size >= 16) {
                        kind = map.getShort(body).toInt()
                        channels = map.getShort(body + 2).toInt()
                        rate = map.getInt(body + 4)
                        bits = map.getShort(body + 14).toInt()
                    } else if (id == DATA) {
                        if (kind != 1 || bits != 16 || channels !in 1..2 || rate <= 0) return@use null
                        val length = minOf(size.toLong() and 0xFFFFFFFFL, (map.limit() - body).toLong()).toInt()
                        map.position(body)
                        map.limit(body + length / (2 * channels) * (2 * channels))
                        return@use Pcm(map.slice().order(ByteOrder.LITTLE_ENDIAN).asShortBuffer(), channels, rate)
                    }
                    if (size < 0) break
                    at = body + size + (size and 1)
                }
                null
            }
        }.getOrNull()

        private const val RIFF = 0x46464952
        private const val WAVE = 0x45564157
        private const val FMT = 0x20746d66
        private const val DATA = 0x61746164
    }
}

package io.github.artcraftandroid.procraft.canvas

import android.app.Application
import android.media.AudioFormat
import android.media.MediaCodec
import android.media.MediaExtractor
import android.media.MediaFormat
import io.github.artcraftandroid.procraft.AppLog
import io.github.artcraftandroid.procraft.core.MobileException
import io.github.artcraftandroid.procraft.core.PluginShare
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.sync.Mutex
import kotlinx.coroutines.sync.withLock
import kotlinx.coroutines.withContext
import java.io.File
import java.io.RandomAccessFile
import java.nio.ByteBuffer
import java.nio.ByteOrder
import kotlin.math.roundToInt

// What a plugin makes of the animation for Share (interface 23), as the iPad's
// (AnimatedExport.swift `exportPluginShare`): Sprite Sheet and Harmony Scene come from the Sprite
// Sheet and Harmony plugin. The core draws and writes what the plugin asks; a sound the export takes
// that it can't read as it is (an m4a, an mp3) is copied as WAV here first.

/** One plugin's export made at a time: the preview's and Done's go through the same renderer. */
private val shareMaking = Mutex()

/**
 * The files [share] makes of the animation, in a folder of its own in the exports folder, and what
 * the plugin says of them; null if they couldn't be made. Off the main thread.
 */
suspend fun CanvasViewModel.exportPluginShare(share: PluginShare): Pair<List<File>, String?>? {
    val s = session ?: return null
    val plugins = plugins
    return shareMaking.withLock {
        withContext(Dispatchers.IO) {
            val cache = getApplication<Application>().cacheDir
            val dir = File(File(cache, "exports"), java.util.UUID.randomUUID().toString())
            val sounds = File(cache, "share-sounds-" + java.util.UUID.randomUUID())
            try {
                val wav = mutableMapOf<String, String>()
                for (path in s.pluginShareSounds(plugins, share.key)) {
                    val copy = File(sounds, "${wav.size}.wav")
                    if (wavCopy(File(path), copy)) wav[path] = copy.path
                }
                val made = s.pluginShare(plugins, share.key, dir.path, wav)
                made.files.map(::File) to made.said
            } catch (e: MobileException) {
                AppLog.e("ProCraft", "making ${share.label}", e)
                dir.deleteRecursively()
                null
            } finally {
                sounds.deleteRecursively()
            }
        }
    }
}

/** [exportPluginShare] with the export card, to share: its files, none if it was cancelled or couldn't be made (that is told). */
suspend fun CanvasViewModel.sharePluginExport(share: PluginShare): List<File> =
    withExportCard("${share.label} couldn't be made.") { exportPluginShare(share)?.first }.orEmpty()

/** How long a sound's decoder may give nothing before the copy is given up on. */
private const val STALLED_NS = 10_000_000_000L

/**
 * The sound at [source] written at [out] as 16-bit WAV, its rate and channels as they are; false,
 * and nothing left at [out], if it can't be read.
 */
internal fun wavCopy(source: File, out: File): Boolean {
    val extractor = MediaExtractor()
    var codec: MediaCodec? = null
    var copied = false
    try {
        out.parentFile?.mkdirs()
        extractor.setDataSource(source.path)
        val track = (0 until extractor.trackCount).firstOrNull { extractor.getTrackFormat(it).getString(MediaFormat.KEY_MIME)?.startsWith("audio/") == true } ?: return false
        extractor.selectTrack(track)
        val format = extractor.getTrackFormat(track)
        var rate = format.getInteger(MediaFormat.KEY_SAMPLE_RATE)
        var channels = format.getInteger(MediaFormat.KEY_CHANNEL_COUNT)
        var encoding = AudioFormat.ENCODING_PCM_16BIT
        val decoder = MediaCodec.createDecoderByType(format.getString(MediaFormat.KEY_MIME) ?: return false)
        codec = decoder
        decoder.configure(format, null, null, 0)
        decoder.start()
        RandomAccessFile(out, "rw").use { file ->
            file.setLength(0)
            // the header, written once the sound's length is known
            file.write(ByteArray(WAV_HEADER))
            var data = 0L
            val info = MediaCodec.BufferInfo()
            var fed = false
            var done = false
            var heard = System.nanoTime()
            while (!done) {
                if (System.nanoTime() - heard > STALLED_NS) return false
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
                    heard = System.nanoTime()
                    val buffer = decoder.getOutputBuffer(o) ?: return false
                    buffer.position(info.offset)
                    buffer.limit(info.offset + info.size)
                    buffer.order(ByteOrder.nativeOrder())
                    val bytes = when (encoding) {
                        AudioFormat.ENCODING_PCM_16BIT -> ByteArray(buffer.remaining()).also { buffer.get(it) }
                        AudioFormat.ENCODING_PCM_FLOAT -> {
                            val floats = buffer.asFloatBuffer()
                            val shorts = ByteBuffer.allocate(floats.remaining() * 2).order(ByteOrder.LITTLE_ENDIAN)
                            while (floats.hasRemaining()) shorts.putShort((floats.get().coerceIn(-1f, 1f) * 32767f).roundToInt().toShort())
                            shorts.array()
                        }
                        else -> return false
                    }
                    // a WAV holds at most 4 GB
                    if (data + bytes.size > MOST_WAV_DATA) return false
                    file.write(bytes)
                    data += bytes.size
                    decoder.releaseOutputBuffer(o, false)
                    done = info.flags and MediaCodec.BUFFER_FLAG_END_OF_STREAM != 0
                }
            }
            if (data == 0L || channels <= 0) return false
            file.seek(0)
            file.write(wavHeader(rate, channels, data))
        }
        copied = true
        return true
    } catch (e: Exception) {
        AppLog.w("ProCraft", "a sound couldn't be copied as WAV for a plugin's export", e)
        return false
    } finally {
        runCatching { codec?.stop() }
        runCatching { codec?.release() }
        extractor.release()
        if (!copied) out.delete()
    }
}

private const val WAV_HEADER = 44
private const val MOST_WAV_DATA = 0xFFFF_FFFFL - 36

/** A 16-bit PCM WAV's header for [data] bytes of [channels] channels at [rate] a second. */
private fun wavHeader(rate: Int, channels: Int, data: Long): ByteArray =
    ByteBuffer.allocate(WAV_HEADER).order(ByteOrder.LITTLE_ENDIAN).apply {
        put("RIFF".toByteArray()).putInt((36 + data).toInt()).put("WAVEfmt ".toByteArray()).putInt(16)
        putShort(1).putShort(channels.toShort()).putInt(rate).putInt(rate * channels * 2).putShort((channels * 2).toShort()).putShort(16)
        put("data".toByteArray()).putInt(data.toInt())
    }.array()

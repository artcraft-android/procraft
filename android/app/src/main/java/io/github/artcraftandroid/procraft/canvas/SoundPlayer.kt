package io.github.artcraftandroid.procraft.canvas

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import io.github.artcraftandroid.procraft.AppLog
import io.github.artcraftandroid.procraft.core.TimelineSound
import java.io.File
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors
import java.util.concurrent.TimeUnit
import java.util.concurrent.locks.ReentrantLock
import kotlin.concurrent.withLock

// Playing the timeline's sounds on Android: each sound's file decoded once into a WAV file kept in
// the cache and mapped ([SoundBank]), the sounds mixed at their times and levels ([SoundMix]), and
// the mix played through one AudioTrack on a thread of its own ([SoundPlayer]). The iPad's
// SoundPlayer (TimelineMedia.swift).

/** Heard somewhere: its level, or any of its keyed levels, above silence. */
val TimelineSound.audible: Boolean get() = gain > 0f || levels.any { it > 0f }

/**
 * The timeline's sounds' samples, ready to mix: a WAV file of 16-bit samples is mapped as it is (a
 * video's own sound, as it is brought in); any other is decoded first, on a thread of its own, into
 * a WAV file in [cache]. A sound not ready yet is silent meanwhile.
 */
class SoundBank(private val cache: File) {
    private val ready = ConcurrentHashMap<String, Pcm>()
    /** Those being made ready, or that couldn't be (they aren't tried again). */
    private val asked = ConcurrentHashMap.newKeySet<String>()
    private val worker = Executors.newSingleThreadExecutor { Thread(it, "procraft-sound-decode").apply { isDaemon = true } }

    /** The file at [path]'s samples, if they are ready; made ready, else. */
    internal fun get(path: String): Pcm? {
        ready[path]?.let { return it }
        prepare(path)
        return null
    }

    /** The file at [path]'s samples made ready, if they aren't (or being made so). */
    fun prepare(path: String) {
        if (ready.containsKey(path) || !asked.add(path)) return
        worker.execute { load(path)?.let { ready[path] = it } }
    }

    /** The file at [path]'s samples, made ready here and now if they aren't (for an export). */
    internal fun now(path: String): Pcm? = ready[path] ?: load(path)?.also { ready[path] = it }

    private fun load(path: String): Pcm? {
        val file = File(path)
        Pcm.mapped(file)?.let { return it }
        // (media files are named by their bytes' hash: the name is the sound's own)
        val wav = File(cache, "${file.name}.wav")
        if (!wav.exists()) {
            cache.mkdirs()
            val making = File(cache, "${file.name}.making")
            if (decodeSoundTo(path, making, PEAKS_UNUSED) == null || !making.renameTo(wav)) {
                making.delete()
                AppLog.w("ProCraft", "a sound can't be played: ${file.name}")
                return null
            }
        }
        return Pcm.mapped(wav)
    }

    private companion object {
        /** (decoding a sound to play it reads its waveform too, which isn't kept) */
        const val PEAKS_UNUSED = 50
    }
}

/**
 * [sounds] mixed, as from the timeline played at [fps] frames a second: [scale] times as slowly as
 * the timeline's own rate (an export at another rate), each sound starting where its frame shows,
 * at its own speed. Its keyed levels go from each frame's to the next's over the frame.
 */
internal class SoundMix(private val sounds: List<TimelineSound>, private val fps: Double, private val scale: Double, private val bank: (String) -> Pcm?) {
    /**
     * [frames] of the mix (two channels taking turns) into [out], from [time] seconds in, at [rate]
     * a second; nothing from [end] seconds on. Whether any sound was there.
     */
    fun render(out: FloatArray, frames: Int, time: Double, rate: Int, end: Double = Double.POSITIVE_INFINITY): Boolean {
        java.util.Arrays.fill(out, 0, frames * 2, 0f)
        var any = false
        for (s in sounds) {
            val pcm = bank(s.path) ?: continue
            val start = s.at * scale
            val stop = minOf(start + s.length, end)
            val k0 = maxOf(kotlin.math.ceil((start - time) * rate).toInt(), 0)
            val k1 = minOf(((stop - time) * rate).toLong(), frames.toLong()).toInt()
            if (k0 >= k1) continue
            any = true
            val samples = pcm.samples
            val two = pcm.channels == 2
            val step = pcm.rate.toDouble() / rate
            var pos = (s.from + time + k0.toDouble() / rate - start) * pcm.rate
            for (k in k0 until k1) {
                val i = pos.toInt()
                if (i < 0) {
                    pos += step
                    continue
                }
                if (i >= pcm.frames - 1) break
                val f = (pos - i).toFloat()
                val g = level(s, time + k.toDouble() / rate)
                val l: Float
                val r: Float
                if (two) {
                    l = samples.get(2 * i) * (1 - f) + samples.get(2 * i + 2) * f
                    r = samples.get(2 * i + 1) * (1 - f) + samples.get(2 * i + 3) * f
                } else {
                    l = samples.get(i) * (1 - f) + samples.get(i + 1) * f
                    r = l
                }
                out[2 * k] += l * g / 32768f
                out[2 * k + 1] += r * g / 32768f
                pos += step
            }
        }
        if (any) for (k in 0 until frames * 2) out[k] = out[k].coerceIn(-1f, 1f)
        return any
    }

    /** [s]'s level [t] seconds in: keyed, eased from frame to frame, or its one level. */
    private fun level(s: TimelineSound, t: Double): Float {
        val levels = s.levels
        if (levels.isEmpty()) return s.gain
        val x = t * fps / scale - s.levelsFrom.toDouble()
        if (x <= 0) return levels[0]
        val i = x.toInt()
        if (i >= levels.size - 1) return levels[levels.size - 1]
        val f = (x - i).toFloat()
        return levels[i] + (levels[i + 1] - levels[i]) * f
    }
}

/**
 * Plays the timeline's sounds: all of them mixed into one AudioTrack, fed a little at a time on a
 * thread of its own, from a timeline time on (playing) or for a moment there (a scrubbed frame's
 * blip). With nothing to play for a while, the thread lets the audio go and ends.
 */
class SoundPlayer(private val bank: SoundBank) {
    /** What is being played: the mix from timeline time [from] (at the first sample written), until [end]. */
    private class Plan(val mix: SoundMix, val from: Double, val end: Double, val playing: Boolean)

    private val lock = ReentrantLock()
    private val planned = lock.newCondition()
    private var plan: Plan? = null
    /** Counts the plans: a new one throws away what the audio holds of the last. */
    private var generation = 0
    private var thread: Thread? = null

    /** Playing along with the timeline (not a blip). */
    val playing: Boolean get() = lock.withLock { plan?.playing == true }

    /** [sounds] playing as from [time] seconds into the timeline at [fps]; returns how long until they are heard (0 with nothing to play). */
    fun start(sounds: List<TimelineSound>, time: Double, fps: Double): Double {
        val heard = sounds.filter { it.audible && it.at + it.length > time }
        if (heard.isEmpty()) {
            stop()
            return 0.0
        }
        heard.forEach { bank.prepare(it.path) }
        set(Plan(SoundMix(heard, fps, 1.0, bank::get), time, Double.POSITIVE_INFINITY, true))
        return LEAD
    }

    /** The slice of [sounds] under [time], [length] seconds of it. */
    fun blip(sounds: List<TimelineSound>, time: Double, length: Double, fps: Double) {
        val under = sounds.filter { it.gain > 0f && it.at <= time && time < it.at + it.length }
        if (under.isEmpty()) {
            stop()
            return
        }
        set(Plan(SoundMix(under, fps, 1.0, bank::get), time, time + length, false))
    }

    fun stop() = set(null)

    private fun set(p: Plan?) = lock.withLock {
        plan = p
        generation++
        if (p != null && thread == null) thread = Thread(::play, "procraft-timeline-sound").apply { isDaemon = true; start() }
        planned.signalAll()
    }

    private fun play() {
        val track = runCatching {
            AudioTrack.Builder()
                .setAudioAttributes(AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_MEDIA).setContentType(AudioAttributes.CONTENT_TYPE_MUSIC).build())
                .setAudioFormat(AudioFormat.Builder().setEncoding(AudioFormat.ENCODING_PCM_FLOAT).setSampleRate(RATE).setChannelMask(AudioFormat.CHANNEL_OUT_STEREO).build())
                .setBufferSizeInBytes(maxOf(AudioTrack.getMinBufferSize(RATE, AudioFormat.CHANNEL_OUT_STEREO, AudioFormat.ENCODING_PCM_FLOAT), FRAMES * 8 * 3))
                .setTransferMode(AudioTrack.MODE_STREAM)
                .setPerformanceMode(AudioTrack.PERFORMANCE_MODE_LOW_LATENCY)
                .build()
        }.getOrNull()
        if (track == null) {
            lock.withLock { thread = null }
            return
        }
        val out = FloatArray(FRAMES * 2)
        val silence = FloatArray(FRAMES * 2)
        var seen = -1
        var written = 0L
        var silent = 0
        try {
            track.play()
            while (true) {
                val (p, gen) = lock.withLock {
                    // nothing to play for a while: the audio rests until there is, or the thread ends
                    if (plan == null && silent >= REST_AFTER) {
                        track.pause()
                        track.flush()
                        if (!planned.await(END_AFTER_S, TimeUnit.SECONDS) && plan == null) {
                            thread = null
                            return
                        }
                        silent = 0
                        track.play()
                    }
                    plan to generation
                }
                if (gen != seen) {
                    // what the audio holds of the last plan goes at once
                    seen = gen
                    written = 0
                    track.pause()
                    track.flush()
                    track.play()
                }
                if (p == null) {
                    silent++
                    track.write(silence, 0, silence.size, AudioTrack.WRITE_BLOCKING)
                    continue
                }
                val t = p.from + written.toDouble() / RATE
                if (t >= p.end) {
                    lock.withLock { if (plan === p) plan = null }
                    continue
                }
                p.mix.render(out, FRAMES, t, RATE, p.end)
                track.write(out, 0, out.size, AudioTrack.WRITE_BLOCKING)
                written += FRAMES
            }
        } catch (e: Exception) {
            AppLog.w("ProCraft", "the timeline's sound stopped", e)
            lock.withLock { thread = null }
        } finally {
            runCatching { track.stop() }
            track.release()
        }
    }

    companion object {
        /** The mix's rate, and how much is written at once (10 ms). */
        const val RATE = 48_000
        private const val FRAMES = 480

        /** How long until sounds started are heard, about (seconds): the pictures wait as long. */
        private const val LEAD = 0.06

        /** Writes of silence after which the audio rests (a third of a second), and how long it rests before the thread ends (seconds). */
        private const val REST_AFTER = 33
        private const val END_AFTER_S = 20L
    }
}

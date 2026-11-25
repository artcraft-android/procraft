package io.github.artcraftandroid.procraft.canvas

import io.github.artcraftandroid.procraft.core.TimelineSound
import java.io.File
import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNotNull
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class SoundMixTest {
    /** A second of one channel at [rate] a second, every sample [value]. */
    private fun steady(value: Short, rate: Int = 100): Pcm {
        val samples = ShortArray(rate) { value }
        return Pcm(ByteBuffer.allocate(rate * 2).order(ByteOrder.LITTLE_ENDIAN).asShortBuffer().apply { put(samples); flip() }, 1, rate)
    }

    private fun sound(at: Double, gain: Float, levels: List<Float> = emptyList(), from: UInt = 0u) =
        TimelineSound(1uL, "s", at, 0.0, 1.0, gain, levels, from)

    @Test
    fun a_sound_is_heard_from_its_time_at_its_level() {
        val pcm = steady(16384)
        val mix = SoundMix(listOf(sound(at = 0.5, gain = 0.5f)), fps = 10.0, scale = 1.0) { pcm }
        val out = FloatArray(200)
        assertTrue(mix.render(out, 100, 0.0, 100))
        // silent before it starts, then half of a half-loud sample
        assertEquals(0f, out[2 * 40], 0f)
        assertEquals(0.25f, out[2 * 60], 0.001f)
        assertEquals(out[2 * 60], out[2 * 60 + 1], 0f)
    }

    @Test
    fun nothing_is_heard_past_the_end_asked() {
        val pcm = steady(16384)
        val mix = SoundMix(listOf(sound(at = 0.0, gain = 1f)), fps = 10.0, scale = 1.0) { pcm }
        val out = FloatArray(200)
        mix.render(out, 100, 0.0, 100, end = 0.2)
        assertEquals(0.5f, out[2 * 10], 0.001f)
        assertEquals(0f, out[2 * 30], 0f)
    }

    @Test
    fun keyed_levels_go_from_one_frame_to_the_next() {
        val pcm = steady(32767)
        // keyed on frames 0 and 1 at ten frames a second: silent, then whole
        val mix = SoundMix(listOf(sound(at = 0.0, gain = 0f, levels = listOf(0f, 1f))), fps = 10.0, scale = 1.0) { pcm }
        val out = FloatArray(400)
        mix.render(out, 200, 0.0, 1000)
        assertEquals(0f, out[0], 0.001f)
        assertEquals(0.5f, out[2 * 50], 0.01f)
        assertEquals(1f, out[2 * 150], 0.01f)
    }

    @Test
    fun a_sound_not_ready_is_silent() {
        val mix = SoundMix(listOf(sound(at = 0.0, gain = 1f)), fps = 10.0, scale = 1.0) { null }
        val out = FloatArray(20)
        assertFalse(mix.render(out, 10, 0.0, 100))
    }

    @Test
    fun a_wav_file_of_16_bit_samples_is_mapped_and_others_are_not() {
        val dir = java.nio.file.Files.createTempDirectory("wav").toFile()
        try {
            fun wav(bits: Int, channels: Int): File {
                val data = 8
                val b = ByteBuffer.allocate(44 + data).order(ByteOrder.LITTLE_ENDIAN)
                b.put("RIFF".toByteArray()).putInt(36 + data).put("WAVEfmt ".toByteArray()).putInt(16)
                b.putShort(1).putShort(channels.toShort()).putInt(8000).putInt(8000 * channels * bits / 8).putShort((channels * bits / 8).toShort()).putShort(bits.toShort())
                b.put("data".toByteArray()).putInt(data)
                b.putShort(1).putShort(2).putShort(3).putShort(4)
                return File(dir, "$bits-$channels.wav").apply { writeBytes(b.array()) }
            }
            val stereo = Pcm.mapped(wav(16, 2))
            assertNotNull(stereo)
            assertEquals(2, stereo!!.frames)
            assertEquals(3.toShort(), stereo.samples.get(2))
            assertNull(Pcm.mapped(wav(8, 1)))
        } finally {
            dir.deleteRecursively()
        }
    }
}

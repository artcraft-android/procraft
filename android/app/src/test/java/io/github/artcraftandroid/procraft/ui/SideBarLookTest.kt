package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.graphics.Color
import org.junit.After
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.abs

class SideBarLookTest {
    /** The grey (0..255) [top] shows over a [under] grey. */
    private fun over(top: Color, under: Float): Float = top.red * 255f * top.alpha + under * (1f - top.alpha)

    @After
    fun dark() {
        Pc.light = false
    }

    @Test
    fun aSavedValuesMarkShowsOnTheTrackInEitherLook() {
        for (light in listOf(false, true)) {
            Pc.light = light
            // over the white canvas, the black surround and a mid grey
            for (behind in listOf(255f, 0f, 128f)) {
                val track = over(Pc.SliderTrack, behind)
                val mark = over(Pc.NotchMark, track)
                assertTrue("light $light over $behind: $mark on $track", abs(mark - track) >= 15f)
            }
        }
    }

    @Test
    fun theMarkIsAsMeasuredOverWhite() {
        Pc.light = false
        assertEquals(103f, over(Pc.SliderTrack, 255f), 2f)
        assertEquals(119f, over(Pc.NotchMark, over(Pc.SliderTrack, 255f)), 2f)
        Pc.light = true
        assertEquals(244f, over(Pc.SliderTrack, 255f), 2f)
        assertEquals(162f, over(Pc.NotchMark, over(Pc.SliderTrack, 255f)), 2f)
    }
}

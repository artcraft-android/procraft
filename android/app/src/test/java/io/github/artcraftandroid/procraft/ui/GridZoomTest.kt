package io.github.artcraftandroid.procraft.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import kotlin.math.sqrt

class GridZoomTest {
    @Test
    fun theLeastZoomFitsTheWholeTimelineBesideThePlayhead() {
        // 300 dp beside the playhead: 100 frames fit at 3 dp a frame
        assertEquals(3f / 44f, GridZoom.least(100, 300f), 1e-6f)
        // a short timeline already fits: never further in than the grid starts
        assertEquals(1f, GridZoom.least(4, 300f), 0f)
        // a very long one: no narrower than the narrowest a frame gets
        assertEquals(0.75f / 44f, GridZoom.least(100_000, 300f), 1e-6f)
        // no frames or no room counts as one of each
        assertEquals(1f / 44f, GridZoom.least(0, 0f), 1e-6f)
    }

    @Test
    fun aPinchZoomsFromWhereItBeganAsFarAsTheGridGoes() {
        assertEquals(1.5f, GridZoom.pinched(1f, 1.5f, 0.1f), 1e-6f)
        assertEquals(GridSizes.MOST, GridZoom.pinched(1.5f, 4f, 0.1f), 0f)
        // out: as far as the whole timeline fits
        assertEquals(0.1f, GridZoom.pinched(1f, 0.01f, 0.1f), 0f)
        // or as far as the rows get smaller, where the timeline fits sooner
        assertEquals(GridSizes.LEAST_ROWS, GridZoom.pinched(1f, 0.01f, 1f), 0f)
    }

    @Test
    fun aPinchFlickedShutGoesOutAndOneFlungOpenGoesBack() {
        assertEquals(0.2f, GridZoom.landing(-3f, -3f, 0.8f, 0.2f))
        assertEquals(1f, GridZoom.landing(4f, 3.5f, 0.6f, 0.2f))
        // flung open while it is already as big as it starts, or bigger: it stays
        assertNull(GridZoom.landing(4f, 3.5f, 1.2f, 0.2f))
        // let go gently: it stays
        assertNull(GridZoom.landing(-1f, -1f, 0.6f, 0.2f))
        assertNull(GridZoom.landing(2f, 2f, 0.6f, 0.2f))
    }

    @Test
    fun theZoomEasesEvenlyAsItReads() {
        assertEquals(0.25f, GridZoom.eased(0.25f, 2f, 0f), 1e-6f)
        assertEquals(2f, GridZoom.eased(0.25f, 2f, 1f), 1e-5f)
        // halfway in time is halfway by ratio
        assertEquals(sqrt(0.25f * 2f), GridZoom.eased(0.25f, 2f, 0.5f), 1e-5f)
        assertEquals(2f, GridZoom.eased(0.25f, 2f, 3f), 1e-5f)
    }

    @Test
    fun aPinchSpeedIsOverItsLast40Ms() {
        val closing = listOf(PinchSample(0, 1f), PinchSample(50, 0.9f), PinchSample(80, 0.8f), PinchSample(100, 0.7f))
        // from 0.9 (the last before 60 ms) to 0.7 in 50 ms
        val (speed, opening) = pinchSpeed(closing, 100)
        assertEquals(-4f, speed, 1e-4f)
        assertEquals(-4f / 0.7f, opening, 1e-4f)
        assertTrue(speed < GridZoom.QUICK_PINCH)
        // fingers held still a while before lifting: nothing heard meanwhile, so slow
        val (still, _) = pinchSpeed(closing, 1000)
        assertTrue(still > GridZoom.QUICK_PINCH)
        // lifted the moment it began: no speed
        assertEquals(0f, pinchSpeed(listOf(PinchSample(10, 1f)), 10).first, 0f)
        assertEquals(0f, pinchSpeed(emptyList(), 10).first, 0f)
    }

    @Test
    fun zoomingKeepsTheFrameUnderThePlayhead() {
        val playhead = 400f
        for (zoom in listOf(0.02f, 0.3f, 0.5f, 1f, 1.7f, 2f)) {
            val sz = GridSizes(zoom)
            // frame 37's cell's middle stays under the playhead's middle
            val origin = sz.origin(playhead, 37f)
            assertEquals(playhead, origin + 37 * sz.pitch + sz.cellWidth / 2f, 1e-3f)
        }
    }

    @Test
    fun theRowsShrinkWithTheFramesDownToHalf() {
        assertEquals(44f, GridSizes(2f).rowHeight, 0f)
        assertEquals(44f, GridSizes(1f).rowHeight, 0f)
        assertEquals(33f, GridSizes(0.75f).rowHeight, 0f)
        assertEquals(22f, GridSizes(0.1f).rowHeight, 0f)
        // pinched in, the frames alone get wider; the gap after a cell stays as it starts
        assertEquals(84f, GridSizes(2f).cellWidth, 1e-4f)
        assertTrue(GridSizes(1f).pictures)
        assertFalse(GridSizes(0.3f).pictures)
    }

    @Test
    fun aGroupTappedThreeTimesFillsTheRoomBesideThePlayhead() {
        // 20 frames: 10 either side of the playhead, in 220 dp
        assertEquals(0.5f, GridZoom.fit(20, 220f), 1e-6f)
        assertEquals(GridSizes.MOST, GridZoom.fit(1, 220f), 0f)
        assertEquals(GridSizes.LEAST_PITCH / GridSizes.PITCH, GridZoom.fit(1_000_000, 220f), 1e-6f)
        val zoom = GridZoom()
        assertFalse(zoom.thirdTap(7u, 0))
        assertFalse(zoom.thirdTap(7u, 300))
        assertTrue(zoom.thirdTap(7u, 600))
        // too slow, or another group between: counted afresh
        assertFalse(zoom.thirdTap(7u, 700))
        assertFalse(zoom.thirdTap(7u, 1200))
        assertFalse(zoom.thirdTap(8u, 1300))
        assertFalse(zoom.thirdTap(7u, 1400))
    }

    @Test
    fun theRulersNumbersThinOutAsTheFramesNarrow() {
        // as the grid starts: every frame's number
        assertEquals(RulerSteps(1, false, 1, 12), RulerSteps.at(44f, 24))
        // at 10 dp a frame, every third would crowd: every fifth
        assertEquals(5, RulerSteps.at(10f, 24).every)
        assertFalse(RulerSteps.at(10f, 24).seconds)
        // rarer than four a second: the seconds, 2 s apart at 1 dp a frame, a tick each second
        val far = RulerSteps.at(1f, 24)
        assertTrue(far.seconds)
        assertEquals(2, far.apart)
        assertEquals(24, far.tick)
        // a tick each half second where there is room
        assertEquals(12, RulerSteps.at(2f, 24).tick)
        assertEquals(25, RulerSteps.at(2f, 25).tick)
    }
}

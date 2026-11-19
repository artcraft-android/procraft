package io.github.artcraftandroid.procraft.ui

import io.github.artcraftandroid.procraft.canvas.CanvasViewModel.CursorStyle
import org.junit.Assert.assertEquals
import org.junit.Test

class BrushCursorTest {
    @Test
    fun perBrushABrushWithNoOutlineOfItsOwnStillShowsLinesUnlessItsFillShows() {
        // painting (no fill) or hovering with no fill: the contrasting lines, never nothing
        assertEquals(CursorLines.CONTRAST, cursorLines(CursorStyle.BRUSH, 0, filled = false))
        // hovering, its stamp filling it: the fill alone, as the brush says
        assertEquals(CursorLines.NONE, cursorLines(CursorStyle.BRUSH, 0, filled = true))
        // its own outline, whatever the fill
        assertEquals(CursorLines.CONTRAST, cursorLines(CursorStyle.BRUSH, 1, filled = true))
        assertEquals(CursorLines.COLOUR, cursorLines(CursorStyle.BRUSH, 2, filled = false))
        assertEquals(CursorLines.COLOUR, cursorLines(CursorStyle.BRUSH, 2, filled = true))
    }

    @Test
    fun theStyleChosenForEveryBrushGoesOverTheBrushsOwn() {
        for (outline in 0..2) for (filled in listOf(false, true)) {
            assertEquals(CursorLines.CONTRAST, cursorLines(CursorStyle.CONTRAST, outline, filled))
            assertEquals(CursorLines.COLOUR, cursorLines(CursorStyle.COLOR, outline, filled))
        }
    }
}

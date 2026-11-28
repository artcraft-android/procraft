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
    fun theHoverFillShowsAsStrongAsTheBrushPaintsAtItsEstimatedPressure() {
        // a brush whose opacity follows the pressure: faint at the estimate, the picture showing through
        assertEquals(0.44f, hoverFillStrength(1f, 0.44f), 1e-6f)
        // one that doesn't: as strong as it paints
        assertEquals(0.8f, hoverFillStrength(0.8f, 1f), 1e-6f)
        assertEquals(0f, hoverFillStrength(1.5f, -1f), 1e-6f)
    }

    @Test
    fun theEraserAndTheSmudgeShowNoFillAndAHoverFadesItsFillIn() {
        // (the core gives the eraser and the smudge no fill: #424's black disc under the eraser)
        // a hover only just begun shows no fill, however strongly the brush paints
        assertEquals(0f, hoverFillStrength(1f, 1f, shown = 0f), 1e-6f)
        assertEquals(0.4f, hoverFillStrength(0.8f, 1f, shown = 0.5f), 1e-6f)
        assertEquals(0.8f, hoverFillStrength(0.8f, 1f, shown = 2f), 1e-6f)
    }

    @Test
    fun theStyleChosenForEveryBrushGoesOverTheBrushsOwn() {
        for (outline in 0..2) for (filled in listOf(false, true)) {
            assertEquals(CursorLines.CONTRAST, cursorLines(CursorStyle.CONTRAST, outline, filled))
            assertEquals(CursorLines.COLOUR, cursorLines(CursorStyle.COLOR, outline, filled))
        }
    }
}

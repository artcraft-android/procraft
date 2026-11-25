package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Rect
import io.github.artcraftandroid.procraft.core.BrushDropOver
import io.github.artcraftandroid.procraft.core.BrushInfo
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

class BrushHoldTest {
    // the sets down the left, the shown set's (set 1) five brushes beside them in its list, 90 tall
    // and 10 apart
    private val sets = mapOf(0 to Rect(0f, 0f, 200f, 60f), 1 to Rect(0f, 70f, 200f, 130f), 2 to Rect(0f, 140f, 200f, 200f))
    private val list = Rect(210f, 0f, 630f, 800f)
    private val rows = (0 until 5).map { it to Rect(220f, it * 100f, 620f, it * 100f + 90f) }

    private fun over(x: Float, y: Float) = brushDropOver(Offset(x, y), sets, 1, 5, list, rows)

    @Test
    fun aStillHoldShowsTheMenuAndLettingGoKeepsIt() {
        val hold = BrushHold(slop = 8f, movable = true)
        // a finger never quite still: nothing past the slop drags
        for (jitter in listOf(Offset(1f, 0f), Offset(0f, -2f), Offset(-3f, 4f), Offset(5.6f, 5.6f))) assertFalse(hold.move(jitter))
        assertFalse(hold.dragging)
    }

    @Test
    fun aHoldMovedOnDragsTheBrushAndLetGoIsOverTheHalfOfTheBrushUnderIt() {
        val hold = BrushHold(slop = 8f, movable = true)
        // held on the second brush, moved down: the drag starts once (the menu goes then)
        assertTrue(hold.move(Offset(0f, 9f)))
        assertTrue(hold.dragging)
        assertFalse(hold.move(Offset(0f, 200f)))
        // over the fourth brush (300 to 390): its upper half, its lower half (the core puts it just
        // above or just below)
        assertEquals(BrushDropOver.OnBrush(1u, 3u, false), over(400f, 330f))
        assertEquals(BrushDropOver.OnBrush(1u, 3u, true), over(400f, 370f))
        // over a set's row, its own too (the core moves nothing there)
        assertEquals(BrushDropOver.OnSet(2u), over(100f, 150f))
        assertEquals(BrushDropOver.OnSet(1u), over(100f, 100f))
        // between the sets, or off the lists: nothing
        assertNull(over(100f, 65f))
        assertNull(over(700f, 145f))
    }

    @Test
    fun letGoInAGapOrBelowTheLastBrushTheNearestBrushTakesIt() {
        // the gap under the second brush (190 to 200): nearer it, below it; nearer the third, above it
        assertEquals(BrushDropOver.OnBrush(1u, 1u, true), over(400f, 193f))
        assertEquals(BrushDropOver.OnBrush(1u, 2u, false), over(400f, 198f))
        // the list's room below the last brush: after it
        assertEquals(BrushDropOver.OnBrush(1u, 4u, true), over(400f, 700f))
    }

    @Test
    fun anEmptySetsListTakesThemAndTheRecentSetsListNothing() {
        // a set opened under them with no brushes: its list itself
        assertEquals(BrushDropOver.InList(3u, 0u), brushDropOver(Offset(400f, 300f), sets, 3, 0, list, emptyList()))
        // its rows not laid out yet: the list, after the brushes it holds
        assertEquals(BrushDropOver.InList(1u, 5u), brushDropOver(Offset(400f, 300f), sets, 1, 5, list, emptyList()))
        // the Recent set's list (no set shown): nothing, its rows or not
        assertNull(brushDropOver(Offset(400f, 330f), sets, null, 5, list, rows))
    }

    /**
     * Why the drop reads the library afresh as it is let go: a state given a newer reference to the
     * same local function keeps the first (they are equal), so the drop the gesture held that way
     * saw the set shown when the library first appeared, not one opened since, and let go on a
     * brush found nothing there.
     */
    @Test
    fun aStateGivenANewerReferenceToALocalFunctionKeepsTheFirst() {
        fun dropFor(shown: Int): () -> Int {
            fun drop() = shown
            return ::drop
        }
        val held = androidx.compose.runtime.mutableStateOf(dropFor(1))
        held.value = dropFor(2)
        assertEquals(1, held.value())
    }

    @Test
    fun theRecentSetsBrushesKeepTheirMenuAndStay() {
        val hold = BrushHold(slop = 8f, movable = false)
        assertFalse(hold.move(Offset(0f, 300f)))
        assertFalse(hold.dragging)
    }

    @Test
    fun brushesHeldOverAnotherSetUntilItOpensAreOverItsBrushes() {
        // set 2 opened under brushes of set 1: over one of its brushes, that brush of set 2 (the core
        // copies them there, a different set from theirs)
        val opened = (0 until 3).map { it to Rect(220f, it * 100f, 620f, it * 100f + 90f) }
        assertEquals(BrushDropOver.OnBrush(2u, 1u, false), brushDropOver(Offset(400f, 125f), sets, 2, 3, list, opened))
        // it opens only after a hold as long as the original's (1.76 s measured)
        assertTrue(SPRING_LOAD_MS in 1600L..2000L)
    }

    @Test
    fun aPickedBrushHeldTakesThePickedOnesWithItInTheirOrder() {
        val set = (0 until 6).map { BrushInfo(1u, it.toUInt(), "Brush $it") }
        // not picked: it alone; picked, or the current brush while others are: all of them in order
        assertEquals(listOf(set[2]), draggedTogether(set[2], emptyList(), false, set))
        assertEquals(listOf(set[2]), draggedTogether(set[2], listOf("1-4"), false, set))
        assertEquals(listOf(set[1], set[4], set[5]), draggedTogether(set[4], listOf("1-5", "1-1", "1-4"), false, set))
        assertEquals(listOf(set[0], set[3]), draggedTogether(set[0], listOf("1-3"), true, set))
    }
}

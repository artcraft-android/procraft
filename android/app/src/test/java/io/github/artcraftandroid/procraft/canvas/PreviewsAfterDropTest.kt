package io.github.artcraftandroid.procraft.canvas

import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Brushes dropped in the library: each sample stroke held goes to its brush's new place (a copy
 * takes its original's), so no row shows blank while every stroke is drawn again.
 */
class PreviewsAfterDropTest {
    @Test
    fun aMoveAlongASetTakesEachStrokeWithItsBrush() {
        // set 2's brushes 0 to 3 held, the oldest first; brush 0 moved below brush 2: 1, 2 up a place
        val order = listOf("2-0", "2-1", "2-2", "2-3")
        val changes = listOf("2-0" to "2-2", "2-1" to "2-0", "2-2" to "2-1", "2-3" to "2-3")
        assertEquals(changes, previewsAfterDrop(order, changes))
    }

    @Test
    fun aCopyTakesItsOriginalsStrokeJustAfterIt() {
        // set 1's brush 4 copied to the top of set 0, whose two brushes go down a place
        val order = listOf("0-1", "1-4", "0-0")
        val changes = listOf("0-0" to "0-1", "0-1" to "0-2", "1-4" to "1-4", "1-4" to "0-0")
        assertEquals(
            listOf("0-1" to "0-2", "1-4" to "1-4", "1-4" to "0-0", "0-0" to "0-1"),
            previewsAfterDrop(order, changes),
        )
    }

    @Test
    fun aStrokeNotHeldIsDrawnAsItsRowShows() {
        // only 3-0 is held: the change naming 3-1 has no stroke to take
        assertEquals(listOf("3-0" to "3-1"), previewsAfterDrop(listOf("3-0"), listOf("3-0" to "3-1", "3-1" to "3-0")))
        assertEquals(emptyList<Pair<String, String>>(), previewsAfterDrop(emptyList(), listOf("3-0" to "3-1")))
    }
}

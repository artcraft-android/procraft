package io.github.artcraftandroid.procraft.canvas

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class PenHeightTest {
    @Test
    fun aPensHeightIsAShareOfItsReachAsItsDeviceSays() {
        assertEquals(0f, penHeight(0f, 0f, 255f)!!, 1e-6f)
        assertEquals(0.5f, penHeight(127.5f, 0f, 255f)!!, 1e-6f)
        assertEquals(1f, penHeight(255f, 0f, 255f)!!, 1e-6f)
        // a range not from 0, and past its ends held to them
        assertEquals(0.25f, penHeight(2f, 1f, 5f)!!, 1e-6f)
        assertEquals(1f, penHeight(9f, 1f, 5f)!!, 1e-6f)
        assertEquals(0f, penHeight(-3f, 1f, 5f)!!, 1e-6f)
    }

    @Test
    fun aPenThatDoesntSayHowHighItIsHasNoHeight() {
        // (its cursor then shows whole at any height)
        assertNull(penHeight(3f, null, null))
        assertNull(penHeight(3f, 0f, 0f))
        assertNull(penHeight(3f, 5f, 1f))
        assertNull(penHeight(Float.NaN, 0f, 1f))
    }
}

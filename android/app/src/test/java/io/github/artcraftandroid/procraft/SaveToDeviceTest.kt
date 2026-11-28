package io.github.artcraftandroid.procraft

import java.io.ByteArrayInputStream
import java.io.ByteArrayOutputStream
import java.io.IOException
import java.io.OutputStream
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Save to device writes a file whole or takes it away again: one cut short (the storage full part
 * way) was left in the folder picked, looking like the export.
 */
class SaveToDeviceTest {
    @Test
    fun aFileWrittenWholeStays() {
        val out = ByteArrayOutputStream()
        var removed = false
        val written = SaveToDeviceActivity.copyWholeOrRemove({ out }, { ByteArrayInputStream(byteArrayOf(1, 2, 3)) }, { removed = true })
        assertTrue(written)
        assertFalse(removed)
        assertArrayEquals(byteArrayOf(1, 2, 3), out.toByteArray())
    }

    @Test
    fun aFileCutShortIsTakenAwayAndTheFailureHeard() {
        // room for 4 bytes of 64 KB
        val full = object : OutputStream() {
            var count = 0
            override fun write(b: Int) {
                if (++count > 4) throw IOException("No space left on device")
            }
        }
        var removed = 0
        val heard = mutableListOf<Throwable>()
        val written = SaveToDeviceActivity.copyWholeOrRemove({ full }, { ByteArrayInputStream(ByteArray(65_536)) }, { removed++ }, { heard += it })
        assertFalse(written)
        assertEquals(1, removed)
        assertEquals("No space left on device", heard.single().message)
    }

    @Test
    fun aDocumentThatCantBeOpenedIsTakenAwayToo() {
        var removed = 0
        assertFalse(SaveToDeviceActivity.copyWholeOrRemove({ null }, { ByteArrayInputStream(byteArrayOf(1)) }, { removed++ }))
        assertEquals(1, removed)
    }
}

package io.github.artcraftandroid.procraft

import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.nio.file.Files

class SaveToGalleryTest {
    @Test
    fun picturesAndVideosGalleriesShowCanBeSavedThere() {
        for (mime in listOf("image/png", "image/jpeg", "image/gif", "image/webp", "video/mp4")) {
            assertTrue(mime, savesToGallery(mime))
        }
        // files a gallery doesn't show: the share sheet's Save to device only
        for (mime in listOf("image/vnd.adobe.photoshop", "image/tiff", "image/svg+xml", "application/pdf", "application/octet-stream", "text/plain")) {
            assertFalse(mime, savesToGallery(mime))
        }
    }

    /** Every export a gallery shows goes there from the share sheet (#423): the still JPEG and PNG, the layers' PNGs, and each animation. */
    @Test
    fun everyExportAGalleryShowsCanBeSavedToIt() {
        for (mime in listOf("image/jpeg", "image/png", CanvasViewModel.Export.LAYER_PNGS.mime, CanvasViewModel.Export.MODEL_JPEG.mime)) {
            assertTrue(mime, savesToGallery(mime))
        }
        for (format in CanvasViewModel.Animated.entries) {
            assertTrue(format.title, savesToGallery(format.mime))
        }
        // documents, layered files and models go to the device instead
        for (export in listOf(CanvasViewModel.Export.PSD, CanvasViewModel.Export.PDF, CanvasViewModel.Export.TIFF, CanvasViewModel.Export.MODEL_OBJ, CanvasViewModel.Export.SVG)) {
            assertFalse(export.name, savesToGallery(export.mime))
        }
    }

    @Test
    fun howASaveToTheGalleryWentIsSaidOnce() {
        assertEquals("Saved to Gallery", gallerySavedNote(1, 1))
        assertEquals("12 saved to Gallery", gallerySavedNote(12, 12))
        assertEquals("Saved 3 of 4 to Gallery", gallerySavedNote(3, 4))
        assertEquals("Couldn't save to Gallery", gallerySavedNote(0, 2))
    }

    @Test
    fun aNameTakenGetsTheNextNumberBeforeItsExtension() {
        val dir = Files.createTempDirectory("gallery").toFile()
        try {
            assertEquals("Untitled Artwork.png", freeName(dir, "Untitled Artwork.png").name)
            java.io.File(dir, "Untitled Artwork.png").writeText("")
            assertEquals("Untitled Artwork (2).png", freeName(dir, "Untitled Artwork.png").name)
            java.io.File(dir, "Untitled Artwork (2).png").writeText("")
            assertEquals("Untitled Artwork (3).png", freeName(dir, "Untitled Artwork.png").name)
            java.io.File(dir, "notes").writeText("")
            assertEquals("notes (2)", freeName(dir, "notes").name)
        } finally {
            dir.deleteRecursively()
        }
    }
}

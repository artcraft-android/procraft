package io.github.artcraftandroid.procraft.ui

import java.io.File
import java.util.zip.ZipFile
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Rule
import org.junit.Test
import org.junit.rules.TemporaryFolder

class StorePackageTest {
    @get:Rule val temp = TemporaryFolder()

    @Test fun archivesTheSameFilesWithoutRenamingOrChangingThem() {
        val folder = temp.newFolder("package")
        val contents = mapOf(
            "details.json" to """{"preview":"preview.png","pictures":["preview.png","strokes-1.png","picture-1.png"]}""".encodeToByteArray(),
            "file.brushset" to byteArrayOf(80, 75, 3, 4),
            "preview.png" to byteArrayOf(1, 5, 6),
            "strokes-1.png" to byteArrayOf(1, 2, 3),
            "picture-1.png" to byteArrayOf(7, 8, 9),
        )
        contents.forEach { (name, bytes) -> File(folder, name).writeBytes(bytes) }
        val exported = File(temp.root, "Inks.zip")
        zipStorePackage(folder, exported)
        ZipFile(exported).use { zip ->
            assertEquals(contents.keys, zip.entries().toList().map { it.name }.toSet())
            contents.forEach { (name, bytes) ->
                assertArrayEquals(bytes, zip.getInputStream(zip.getEntry(name)).use { it.readBytes() })
            }
        }
    }
}

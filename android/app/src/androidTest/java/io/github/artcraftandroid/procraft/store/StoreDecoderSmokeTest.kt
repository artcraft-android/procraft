package io.github.artcraftandroid.procraft.store

import android.app.Activity
import android.app.Instrumentation
import android.graphics.Bitmap
import android.os.Bundle
import io.github.artcraftandroid.procraft.core.StoreDecodeKind
import java.io.File

/** Exercises a real Binder round trip and loads the native decoder in the isolated UID. */
class StoreDecoderSmokeTest : Instrumentation() {
    override fun onCreate(arguments: Bundle?) {
        super.onCreate(arguments)
        start()
    }

    override fun onStart() {
        val outcome = runCatching {
            val decoder = AndroidStoreDecoder(targetContext)
            val input = File.createTempFile("preview", ".png", targetContext.cacheDir)
            val output = File.createTempFile("pixels", ".raw", targetContext.cacheDir)
            try {
                val bitmap = Bitmap.createBitmap(2, 2, Bitmap.Config.ARGB_8888)
                bitmap.eraseColor(0xff2860d0.toInt())
                input.outputStream().use { check(bitmap.compress(Bitmap.CompressFormat.PNG, 100, it)) }
                bitmap.recycle()

                val reason = decoder.decode(StoreDecodeKind.COVER, input.path, output.path)
                check(reason == null) { reason.orEmpty() }
                val pixels = output.readBytes()
                check(pixels.size == 6 + 4 * 4 + 2 * 2 * 4)
                check(pixels.copyOfRange(0, 6).contentEquals(byteArrayOf(80, 67, 83, 68, 1, 0)))
                check(pixels.takeLast(4).map { it.toInt() and 255 } == listOf(40, 96, 208, 255))

                input.writeText("not an image")
                val damaged = decoder.decode(StoreDecodeKind.COVER, input.path, output.path)
                check(damaged == null) { damaged.orEmpty() }
                check(output.readBytes().getOrNull(5) == 1.toByte())
            } finally {
                input.delete()
                output.delete()
            }
        }
        finish(
            if (outcome.isSuccess) Activity.RESULT_OK else Activity.RESULT_CANCELED,
            Bundle().apply {
                putString("stream", outcome.exceptionOrNull()?.stackTraceToString() ?: "Store decoder round trip passed.")
            },
        )
    }
}

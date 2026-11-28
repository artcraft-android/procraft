package io.github.artcraftandroid.procraft

import android.Manifest
import android.content.ContentValues
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.media.MediaScannerConnection
import android.net.Uri
import android.os.Build
import android.os.Bundle
import android.os.Environment
import android.provider.MediaStore
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import java.io.File
import java.util.concurrent.Executors

/**
 * The share sheet's "Save to Gallery": exported pictures and videos written into the device's
 * shared Pictures/ProCraft and Movies/ProCraft, where gallery apps show them, with no storage
 * permission from Android 10 (through the media store) and leave to write there before. Shows
 * nothing of its own but a short note of how it went, and stays only to ask that leave: the files
 * are written behind it with the app going on (it took every touch, unseen, while a long video
 * was copied, and a turn of the screen meanwhile left it there for good).
 */
class SaveToGalleryActivity : ComponentActivity() {
    private val asked by lazy { intent.getStringArrayExtra(EXTRA_PATHS).orEmpty() }
    private val files by lazy { asked.map(::File).filter { it.isFile } }
    private val mime by lazy { intent.getStringExtra(EXTRA_MIME) ?: "image/png" }

    // Android 8 and 9 let an app into the shared folders only with leave to write there
    private val askToWrite = registerForActivityResult(ActivityResultContracts.RequestPermission()) { granted ->
        if (granted) {
            save()
        } else {
            Toast.makeText(applicationContext, "Couldn't save to Gallery without storage access", Toast.LENGTH_LONG).show()
            finish()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState != null) return
        when {
            files.isEmpty() -> {
                // (gone from the cache since they were shared): said, not a window that closes on nothing
                AppLog.w("ProCraft", "none of the ${asked.size} files to save to the gallery is there any more")
                Toast.makeText(applicationContext, gallerySavedNote(0, asked.size.coerceAtLeast(1)), Toast.LENGTH_LONG).show()
                finish()
            }
            Build.VERSION.SDK_INT < Build.VERSION_CODES.Q &&
                checkSelfPermission(Manifest.permission.WRITE_EXTERNAL_STORAGE) != PackageManager.PERMISSION_GRANTED ->
                askToWrite.launch(Manifest.permission.WRITE_EXTERNAL_STORAGE)
            else -> save()
        }
    }

    /** Writes every file off the main thread, saying how it went once they are written; closes at once. */
    private fun save() {
        val app = applicationContext
        // (read here: the activity is gone by the time they are written)
        val (paths, type) = files to mime
        val of = asked.size.coerceAtLeast(paths.size)
        if (of > paths.size) AppLog.w("ProCraft", "${of - paths.size} of the $of files to save to the gallery weren't there any more")
        val video = type.startsWith("video/")
        Executors.newSingleThreadExecutor().execute {
            val saved = paths.count { f ->
                val put = { if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) insert(app, f, type, video) else copyShared(app, f, type, video) }
                // a file that can't go is said in the log, with why
                runCatching(put)
                    .onFailure { AppLog.e("ProCraft", "saving ${f.name} (${f.length()} bytes, $type) to the gallery", it) }
                    .getOrDefault(false)
                    .also { if (!it) AppLog.w("ProCraft", "${f.name} (${f.length()} bytes, $type) wasn't saved to the gallery") }
            }
            android.os.Handler(android.os.Looper.getMainLooper()).post {
                Toast.makeText(app, gallerySavedNote(saved, of), if (saved == of) Toast.LENGTH_SHORT else Toast.LENGTH_LONG).show()
            }
        }
        finish()
    }

    /**
     * [file] put in the shared Pictures/ProCraft (Movies/ProCraft if a [video]): kept pending, so no
     * gallery shows it part written, until it is whole; taken out again if it can't be.
     */
    @androidx.annotation.RequiresApi(Build.VERSION_CODES.Q)
    private fun insert(app: Context, file: File, mime: String, video: Boolean): Boolean {
        val contentResolver = app.contentResolver
        val collection =
            if (video) MediaStore.Video.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
            else MediaStore.Images.Media.getContentUri(MediaStore.VOLUME_EXTERNAL_PRIMARY)
        val values = ContentValues().apply {
            put(MediaStore.MediaColumns.DISPLAY_NAME, file.name)
            put(MediaStore.MediaColumns.MIME_TYPE, mime)
            put(MediaStore.MediaColumns.RELATIVE_PATH, "${sharedFolder(video)}/$FOLDER")
            put(MediaStore.MediaColumns.IS_PENDING, 1)
        }
        val uri = contentResolver.insert(collection, values) ?: return false
        val written = runCatching { write(app, file, uri) }.getOrDefault(false)
        if (!written) {
            runCatching { contentResolver.delete(uri, null, null) }
            return false
        }
        // one that can't be shown stays pending, hidden: taken out rather than left half there
        val shown = runCatching { contentResolver.update(uri, ContentValues().apply { put(MediaStore.MediaColumns.IS_PENDING, 0) }, null, null) > 0 }
            .onFailure { AppLog.e("ProCraft", "showing ${file.name} in the gallery", it) }
            .getOrDefault(false)
        if (!shown) runCatching { contentResolver.delete(uri, null, null) }
        return shown
    }

    /** [file] copied whole into [uri]; false if it couldn't be. */
    private fun write(app: Context, file: File, uri: Uri): Boolean {
        val out = app.contentResolver.openOutputStream(uri, "w") ?: return false
        out.use { o -> file.inputStream().use { it.copyTo(o) } }
        return true
    }

    /**
     * Android 8 and 9: [file] copied into the shared folder under a name of its own (written beside it
     * first, then put in place whole), then told to the media store so galleries list it.
     */
    @Suppress("DEPRECATION")
    private fun copyShared(app: Context, file: File, mime: String, video: Boolean): Boolean {
        val dir = File(Environment.getExternalStoragePublicDirectory(sharedFolder(video)), FOLDER)
        if (!dir.isDirectory && !dir.mkdirs()) return false
        val target = freeName(dir, file.name)
        val part = File(dir, ".${target.name}.part")
        try {
            file.inputStream().use { input -> part.outputStream().use { input.copyTo(it) } }
            if (!part.renameTo(target)) return false
        } finally {
            part.delete()
        }
        MediaScannerConnection.scanFile(app, arrayOf(target.absolutePath), arrayOf(mime), null)
        return true
    }

    companion object {
        private const val EXTRA_PATHS = "paths"
        private const val EXTRA_MIME = "mime"
        private const val FOLDER = "ProCraft"

        @Suppress("DEPRECATION")
        private fun sharedFolder(video: Boolean) = if (video) Environment.DIRECTORY_MOVIES else Environment.DIRECTORY_PICTURES

        /** What the share sheet starts to save [files] (of type [mime]) to the gallery. */
        fun intent(context: Context, files: List<File>, mime: String): Intent =
            Intent(context, SaveToGalleryActivity::class.java)
                .putExtra(EXTRA_PATHS, files.map { it.absolutePath }.toTypedArray())
                .putExtra(EXTRA_MIME, mime)
    }
}

/** What is said once [saved] of [of] files have gone to the gallery. */
internal fun gallerySavedNote(saved: Int, of: Int): String = when {
    saved == of && saved == 1 -> "Saved to Gallery"
    saved == of -> "$saved saved to Gallery"
    saved == 0 -> "Couldn't save to Gallery"
    else -> "Saved $saved of $of to Gallery"
}

/** Whether files of type [mime] can go to the gallery: the pictures and videos gallery apps show. */
internal fun savesToGallery(mime: String): Boolean = mime in GALLERY_PICTURES || mime.startsWith("video/")

private val GALLERY_PICTURES = setOf("image/png", "image/jpeg", "image/gif", "image/webp")

/** A name in [dir] no file has yet: [name] itself, else with " (2)", " (3)" … before its extension. */
internal fun freeName(dir: File, name: String): File {
    val first = File(dir, name)
    if (!first.exists()) return first
    val dot = name.lastIndexOf('.').takeIf { it > 0 } ?: name.length
    val (stem, ext) = name.substring(0, dot) to name.substring(dot)
    return generateSequence(2) { it + 1 }.map { File(dir, "$stem ($it)$ext") }.first { !it.exists() }
}

package io.github.artcraftandroid.procraft

import android.content.ContentResolver
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.provider.DocumentsContract
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.result.contract.ActivityResultContracts
import java.io.File
import java.util.concurrent.Executors

/**
 * The share sheet's "Save to device": exported files written where the person picks with the
 * system's own picker (one file under a name it suggests, several into a folder), as the share
 * sheet elsewhere always offers. Shows nothing of its own but a note if it fails, and stays only
 * while the picker is open: the files are written behind it with the app going on (its see-through
 * window took every touch, unseen, while a long video was copied, and a turn of the screen
 * meanwhile left it there for good, the app seeming frozen).
 */
class SaveToDeviceActivity : ComponentActivity() {
    private val asked by lazy { intent.getStringArrayExtra(EXTRA_PATHS).orEmpty() }
    private val files by lazy { asked.map(::File).filter { it.isFile } }
    private val mime by lazy { intent.getStringExtra(EXTRA_MIME) ?: "application/octet-stream" }

    // (the type is only known once the intent is: the picker's own intent made at launch)
    private val saveOne = registerForActivityResult(ActivityResultContracts.StartActivityForResult()) { result ->
        val uri = result.data?.data
        if (result.resultCode != RESULT_OK || uri == null) finish() else write { resolver -> copyWhole(resolver, files[0], uri) }
    }

    private val saveInto = registerForActivityResult(ActivityResultContracts.OpenDocumentTree()) { tree ->
        if (tree == null) {
            finish()
            return@registerForActivityResult
        }
        write { resolver ->
            val folder = DocumentsContract.buildDocumentUriUsingTree(tree, DocumentsContract.getTreeDocumentId(tree))
            // each tried, so one that fails doesn't keep the rest from going
            files.map { f ->
                val made = DocumentsContract.createDocument(resolver, folder, mime, f.name)
                made != null && copyWhole(resolver, f, made)
            }.all { it }
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (savedInstanceState != null) return
        when {
            files.isEmpty() -> {
                // (gone from the cache since they were shared): said, not a window that closes on nothing
                AppLog.w("ProCraft", "none of the ${asked.size} files to save to the device is there any more")
                Toast.makeText(applicationContext, "Couldn't save the file", Toast.LENGTH_LONG).show()
                finish()
            }
            files.size == 1 -> saveOne.launch(
                Intent(Intent.ACTION_CREATE_DOCUMENT).addCategory(Intent.CATEGORY_OPENABLE).setType(mime)
                    .putExtra(Intent.EXTRA_TITLE, files[0].name),
            )
            else -> saveInto.launch(null)
        }
    }

    /** Runs [work] off the main thread, saying if it failed once it has; the window closes at once. */
    private fun write(work: (ContentResolver) -> Boolean) {
        val app = applicationContext
        // (read here: the activity is gone by the time they are written)
        val (paths, type) = files to mime
        val missing = asked.size - paths.size
        Executors.newSingleThreadExecutor().execute {
            val saved = runCatching { work(app.contentResolver) }
                .onFailure { AppLog.e("ProCraft", "saving ${paths.joinToString { "${it.name} (${it.length()} bytes)" }} to the device", it) }
                .getOrDefault(false) && missing == 0
            if (missing > 0) AppLog.w("ProCraft", "$missing of the ${asked.size} files to save to the device weren't there any more")
            if (!saved) {
                AppLog.w("ProCraft", "${paths.joinToString { it.name }} ($type) wasn't saved to the device")
                android.os.Handler(android.os.Looper.getMainLooper()).post { Toast.makeText(app, "Couldn't save the file", Toast.LENGTH_LONG).show() }
            }
        }
        finish()
    }

    /**
     * [file] written whole into the document [uri]; false if it couldn't be, and then the document
     * goes again (a file cut short, the storage full say, was left there looking like the export).
     */
    private fun copyWhole(resolver: ContentResolver, file: File, uri: Uri): Boolean =
        copyWholeOrRemove(
            open = { resolver.openOutputStream(uri, "wt") },
            from = { file.inputStream() },
            remove = { DocumentsContract.deleteDocument(resolver, uri) },
            failed = { AppLog.e("ProCraft", "writing ${file.name} (${file.length()} bytes) to the device", it) },
        )

    companion object {
        private const val EXTRA_PATHS = "paths"
        private const val EXTRA_MIME = "mime"

        /**
         * What [open] gives written whole from [from], or nothing: a copy that fails part way (or
         * never begins) has what it began taken away again by [remove]. [failed] hears why. Whether
         * it was written.
         */
        internal fun copyWholeOrRemove(
            open: () -> java.io.OutputStream?,
            from: () -> java.io.InputStream,
            remove: () -> Unit,
            failed: (Throwable) -> Unit = {},
        ): Boolean {
            val written = runCatching {
                val out = open() ?: return@runCatching false
                out.use { o -> from().use { it.copyTo(o) } }
                true
            }.onFailure(failed).getOrDefault(false)
            if (!written) runCatching(remove).onFailure(failed)
            return written
        }

        /** What the share sheet starts to save [files] (of type [mime]) to the device. */
        fun intent(context: Context, files: List<File>, mime: String): Intent =
            Intent(context, SaveToDeviceActivity::class.java)
                .putExtra(EXTRA_PATHS, files.map { it.absolutePath }.toTypedArray())
                .putExtra(EXTRA_MIME, mime)
    }
}

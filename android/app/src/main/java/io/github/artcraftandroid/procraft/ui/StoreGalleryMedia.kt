package io.github.artcraftandroid.procraft.ui

import android.graphics.Bitmap
import android.widget.MediaController
import android.widget.VideoView
import androidx.compose.foundation.Image
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.ButtonDefaults
import androidx.compose.material3.Text
import androidx.compose.material3.TextButton
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.key
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import io.github.artcraftandroid.procraft.core.Store
import io.github.artcraftandroid.procraft.core.StoreGalleryEntry
import io.github.artcraftandroid.procraft.core.StoreVideo
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.isActive
import kotlinx.coroutines.withContext

@Composable
internal fun StoreGalleryMedia(store: Store?, entry: StoreGalleryEntry, version: Int, active: Boolean) {
    if (store == null) return
    key(store, entry, version) {
        val video = entry.video
        when {
            video == null -> GalleryImage(store, entry.source, entry.url, active)
            active -> GalleryVideo(store, entry.source, entry.url, video)
            else -> GalleryMessage("Gallery video", null) {}
        }
    }
}

private data class GalleryFrame(val bitmap: Bitmap, val delayMs: Long)

@Composable
private fun GalleryImage(store: Store, source: String, address: String, active: Boolean) {
    var retry by remember { mutableStateOf(0) }
    var frames by remember { mutableStateOf(emptyList<GalleryFrame>()) }
    var failure by remember { mutableStateOf<String?>(null) }
    var index by remember { mutableStateOf(0) }
    LaunchedEffect(retry) {
        failure = null
        index = 0
        val (decoded, error) = withContext(Dispatchers.IO) {
            runCatching {
                val read = store.fetchGalleryImageFrom(source, address)
                read.frames.map { GalleryFrame(it.image.toStoreBitmap(), it.delayMs.toLong()) } to read.failure
            }.getOrElse { emptyList<GalleryFrame>() to "The gallery image couldn't be opened." }
        }
        frames = decoded
        failure = error
    }
    LaunchedEffect(frames, active) {
        if (active && frames.size > 1) {
            while (isActive) {
                delay(frames[index].delayMs)
                index = (index + 1) % frames.size
            }
        }
    }
    val frame = frames.getOrNull(index)
    if (frame != null) {
        Image(frame.bitmap.asImageBitmap(), "Store gallery image", Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
    } else {
        GalleryMessage(failure ?: "Loading gallery image...", if (failure == null) null else "Retry") { retry++ }
    }
}

@Composable
private fun GalleryVideo(store: Store, source: String, address: String, format: StoreVideo) {
    val context = LocalContext.current
    val extension = if (format == StoreVideo.WEBM) "webm" else "mp4"
    val file = remember { File(context.cacheDir, "store-demo-" + System.nanoTime() + "." + extension) }
    var playing by remember { mutableStateOf(false) }
    var ready by remember { mutableStateOf(false) }
    var failure by remember { mutableStateOf<String?>(null) }
    DisposableEffect(file) { onDispose { file.delete() } }
    LaunchedEffect(playing) {
        if (!playing || ready) return@LaunchedEffect
        val error = withContext(Dispatchers.IO) {
            val reason = runCatching { store.downloadStoreVideoFrom(source, address, file.path) }
                .getOrElse { "The video couldn't be downloaded." }
            if (!isActive) file.delete()
            reason
        }
        failure = error
        ready = error == null
        if (error != null) playing = false
    }
    if (ready && playing) {
        val player = remember(file) {
            VideoView(context).apply {
                val controls = MediaController(context)
                controls.setAnchorView(this)
                setMediaController(controls)
                setOnPreparedListener { start() }
                setOnErrorListener { _, _, _ ->
                    failure = "This device couldn't play the video."
                    playing = false
                    true
                }
                setVideoPath(file.path)
            }
        }
        DisposableEffect(player) { onDispose { player.stopPlayback() } }
        AndroidView(factory = { player }, modifier = Modifier.fillMaxSize())
    } else {
        val message = failure ?: if (playing) "Downloading video..." else "Gallery video"
        GalleryMessage(message, if (playing) null else "Play video") {
            failure = null
            playing = true
        }
    }
}

@Composable
private fun GalleryMessage(message: String, actionLabel: String?, action: () -> Unit) {
    Column(
        Modifier.padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center,
    ) {
        Text(message, color = ROW_INK, textAlign = TextAlign.Center)
        if (actionLabel != null) {
            TextButton(onClick = action, colors = ButtonDefaults.textButtonColors(contentColor = ACCENT)) {
                Text(actionLabel)
            }
        }
    }
}

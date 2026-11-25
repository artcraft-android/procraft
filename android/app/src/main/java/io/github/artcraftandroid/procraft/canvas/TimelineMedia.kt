package io.github.artcraftandroid.procraft.canvas

import android.content.Context
import android.media.MediaMetadataRetriever
import android.net.Uri
import android.provider.OpenableColumns
import android.webkit.MimeTypeMap
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.lifecycle.viewModelScope
import io.github.artcraftandroid.procraft.AppLog
import io.github.artcraftandroid.procraft.core.CanvasSession
import io.github.artcraftandroid.procraft.core.MediaImport
import io.github.artcraftandroid.procraft.core.MobileException
import io.github.artcraftandroid.procraft.core.TimelineMediaClip
import io.github.artcraftandroid.procraft.core.TimelineMediaKind
import io.github.artcraftandroid.procraft.core.mediaPeaksPerSecond
import java.io.File
import java.util.UUID
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

// Sounds and videos on the timeline, Android's half, as the iPad's TimelineMediaTool
// (TimelineMedia.swift): reading a file brought in (its length, picture size and waveform, a
// video's own sound taken out), the media clips as the grid shows them, their edits, and playing
// the sounds with the timeline. The core keeps the files and their timing; nothing here is saved.

/** The timeline's media as the grid uses it. */
class TimelineMedia internal constructor(private val tool: TimelineTool) {
    private val vm get() = tool.vm
    private val session: CanvasSession? get() = tool.session
    private val context: Context get() = vm.getApplication()

    val bank by lazy { SoundBank(File(context.cacheDir, "procraft-sounds")) }
    private val sounds by lazy { SoundPlayer(bank) }

    /** Reading a file brought in. */
    var bringing by mutableStateOf(false)
        private set

    /** The animatic detector open on this video clip. */
    var detecting by mutableStateOf<ULong?>(null)

    /** The clips read for the panel's reading [readAt] (a block is drawn again with every frame the grid slides), and the session they were read from. */
    private var readAt = -1
    private var readFrom: CanvasSession? = null
    private val clips = HashMap<ULong, TimelineMediaClip>()
    private val peaksRead = HashMap<ULong, List<Float>>()

    private fun follow(s: CanvasSession) {
        if (s !== readFrom) {
            readFrom = s
            peaksRead.clear()
            readAt = -1
        }
        if (readAt != tool.revision) {
            readAt = tool.revision
            clips.clear()
            // the sounds made ready to play, in case they are played
            s.timelineSounds(0u).forEach { bank.prepare(it.path) }
        }
    }

    /** Sound or video clip [id] as its block shows it, read once for each reading of the panel. */
    fun clip(id: ULong): TimelineMediaClip? {
        val s = session ?: return null
        follow(s)
        clips[id]?.let { return it }
        return s.timelineMediaClip(id)?.also { clips[id] = it }
    }

    /** Media [media]'s waveform, read once. */
    fun peaks(media: ULong): List<Float> {
        val s = session ?: return emptyList()
        follow(s)
        return peaksRead.getOrPut(media) { s.timelineMediaPeaks(media) }
    }

    /** Brings in the file at [uri] (picked) as [kind], at the playhead; it is copied first, and the copy goes once the core has its own. */
    fun bringIn(uri: Uri, kind: TimelineMediaKind) {
        val s = session ?: return
        tool.stopPlaying()
        bringing = true
        vm.viewModelScope.launch {
            val outcome = withContext(Dispatchers.IO) {
                val read = try {
                    MediaReading.read(context, uri, kind)
                } catch (e: MediaReading.Problem) {
                    return@withContext Result.failure(e)
                }
                try {
                    Result.success(s.importTimelineMedia(read))
                } catch (e: MobileException) {
                    AppLog.w("ProCraft", "bringing in a sound or video", e)
                    Result.failure(MediaReading.Problem("That file can't be read."))
                } finally {
                    MediaReading.forget(read)
                }
            }
            bringing = false
            if (s !== session) return@launch
            outcome.fold(
                { clip ->
                    if (clip == 0uL) {
                        vm.showNotice("It couldn't go on the timeline.", warning = true)
                    } else {
                        vm.refresh()
                        tool.picked = clip
                    }
                },
                { vm.showNotice((it as? MediaReading.Problem)?.said ?: "That file can't be read.", warning = true) },
            )
        }
    }

    fun slip(id: ULong, seconds: Double, done: Boolean) {
        if (session?.slipTimelineMedia(id, seconds, done) == true || done) vm.refresh()
    }

    fun setSound(c: TimelineMediaClip, volume: Float = c.volume, muted: Boolean = c.muted, offset: Double = c.offset, done: Boolean) {
        session?.setTimelineSound(c.clip, volume, muted, offset, done)
        vm.refresh()
    }

    /** A sound's level: keyed at the playhead when it has level keys (or [key], keying it there first), else its one level. */
    fun setLevel(c: TimelineMediaClip, level: Float, key: Boolean = false, done: Boolean) {
        if (c.volumeKeys.isEmpty() && !key) {
            setSound(c, volume = level, done = done)
            return
        }
        session?.setTimelineVolumeKey(c.clip, level, done)
        vm.refresh()
    }

    fun removeLevelKey(c: TimelineMediaClip) {
        session?.removeTimelineVolumeKey(c.clip)
        vm.refresh()
    }

    fun placeVideo(id: ULong, rect: List<Float>, done: Boolean) {
        session?.setTimelineVideoRect(id, rect, done)
        vm.refresh()
    }

    /** Video [id]'s sound taken out as a sound block of its own on a new track above it: one undo step. */
    fun separateAudio(id: ULong) {
        tool.stopPlaying()
        if (session?.separateTimelineAudio(id) == 0uL) vm.showNotice("It has no sound of its own, or its track is locked.", warning = true)
        vm.refresh()
    }

    // ---- playing along ----

    private val fps: Double get() = maxOf(tool.info?.fps?.toInt() ?: 24, 1).toDouble()

    /** Starts the sounds playing from frame [frame]; returns how long until they are heard (seconds), for the pictures to wait as long. */
    fun play(frame: UInt): Double {
        val s = session ?: return 0.0
        return sounds.start(s.timelineSounds(frame), frame.toDouble() / fps, fps)
    }

    /** Playing went on from [frame] to [next] (by [step]): a jump starts the sounds again from there, going backwards (ping-pong) silences them. Returns the wait, as [play]. */
    fun follow(frame: UInt, next: UInt, step: Int): Double {
        if (step < 0) {
            sounds.stop()
            return 0.0
        }
        if (next != frame + 1u || !sounds.playing) return play(next)
        return 0.0
    }

    fun stop() = sounds.stop()

    /** Scrubbed onto [frame]: the slice of sound under it. Nothing with the setting off. */
    fun blip(frame: UInt) {
        if (!vm.timelineScrubSound) return
        val s = session ?: return
        sounds.blip(s.timelineSounds(frame), frame.toDouble() / fps, maxOf(1.0 / fps, 0.06), fps)
    }
}

/** Reading a file brought in, on Android's decoders. */
object MediaReading {
    class Problem(val said: String) : Exception(said)

    /**
     * The file at [uri] copied into a folder of its own in the cache and read as [kind]: its length,
     * a video's size (turned as it shows) and own sound (taken out as a WAV file beside it), and the
     * sound's waveform. [forget] removes the folder once the core has its own copies.
     */
    fun read(context: Context, uri: Uri, kind: TimelineMediaKind): MediaImport {
        val resolver = context.contentResolver
        val name = runCatching {
            resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)?.use { c -> if (c.moveToFirst()) c.getString(0) else null }
        }.getOrNull() ?: "Media"
        // (the core keeps the file by its extension: the name's, else the kind of file it says it is)
        val extension = name.substringAfterLast('.', "").ifEmpty {
            resolver.getType(uri)?.let { MimeTypeMap.getSingleton().getExtensionFromMimeType(it) } ?: ""
        }
        val folder = File(context.cacheDir, "procraft-bring-${UUID.randomUUID()}").apply { mkdirs() }
        try {
            val file = File(folder, if (extension.isEmpty()) "media" else "media.$extension")
            val copied = runCatching { resolver.openInputStream(uri)?.use { input -> file.outputStream().use { input.copyTo(it) } } }.getOrNull()
            if (copied == null) throw Problem("That file can't be read.")
            return readFile(file, name, kind, folder)
        } catch (e: Exception) {
            folder.deleteRecursively()
            throw e as? Problem ?: Problem("That file can't be read.").also { AppLog.w("ProCraft", "reading a sound or video", e) }
        }
    }

    private fun readFile(file: File, name: String, kind: TimelineMediaKind, folder: File): MediaImport {
        val r = MediaMetadataRetriever()
        val meta = try {
            r.setDataSource(file.path)
            fun key(k: Int) = r.extractMetadata(k)
            Meta(
                seconds = (key(MediaMetadataRetriever.METADATA_KEY_DURATION)?.toLongOrNull() ?: 0L) / 1000.0,
                video = key(MediaMetadataRetriever.METADATA_KEY_HAS_VIDEO) == "yes",
                audio = key(MediaMetadataRetriever.METADATA_KEY_HAS_AUDIO) == "yes",
                width = key(MediaMetadataRetriever.METADATA_KEY_VIDEO_WIDTH)?.toIntOrNull() ?: 0,
                height = key(MediaMetadataRetriever.METADATA_KEY_VIDEO_HEIGHT)?.toIntOrNull() ?: 0,
                rotation = key(MediaMetadataRetriever.METADATA_KEY_VIDEO_ROTATION)?.toIntOrNull() ?: 0,
            )
        } catch (e: Exception) {
            throw Problem("That file can't be read.")
        } finally {
            runCatching { r.release() }
        }
        val perSecond = mediaPeaksPerSecond().toInt()
        return when (kind) {
            TimelineMediaKind.AUDIO -> {
                // (a file the system doesn't say has sound may still have it: decoding tells)
                val sound = decodeSoundTo(file.path, File(folder, "peaks.wav"), perSecond) ?: throw Problem("That file has no sound.")
                val seconds = if (meta.seconds > 0) meta.seconds else sound.seconds
                if (!(seconds > 0)) throw Problem("That file has no length.")
                MediaImport(file.path, name, TimelineMediaKind.AUDIO, seconds, 0u, 0u, sound.peaks, null, 0.0)
            }
            TimelineMediaKind.VIDEO -> {
                if (!meta.video || meta.width <= 0 || meta.height <= 0) throw Problem("That file has no picture.")
                if (!(meta.seconds > 0)) throw Problem("That file has no length.")
                // its sound, if it has one, as a sound file of its own: it plays with the picture,
                // and can be taken out as a sound clip
                val wav = File(folder, "sound.wav")
                val sound = if (meta.audio) decodeSoundTo(file.path, wav, perSecond) else null
                val sideways = meta.rotation == 90 || meta.rotation == 270
                MediaImport(
                    file.path, name, TimelineMediaKind.VIDEO, meta.seconds,
                    (if (sideways) meta.height else meta.width).toUInt(), (if (sideways) meta.width else meta.height).toUInt(),
                    sound?.peaks.orEmpty(), sound?.let { wav.path }, sound?.seconds ?: 0.0,
                )
            }
        }
    }

    private class Meta(val seconds: Double, val video: Boolean, val audio: Boolean, val width: Int, val height: Int, val rotation: Int)

    /** The folder [read] copied the file into, removed. */
    fun forget(read: MediaImport) {
        File(read.path).parentFile?.takeIf { it.name.startsWith("procraft-bring-") }?.deleteRecursively()
    }
}

/** A file from the system's picker: a sound or a video onto the timeline at the playhead, a picture as a new frame there. */
fun TimelineTool.addFile(uri: Uri) {
    val type = vm.getApplication<android.app.Application>().contentResolver.getType(uri).orEmpty()
    when {
        type.startsWith("audio/") -> media.bringIn(uri, TimelineMediaKind.AUDIO)
        type.startsWith("video/") -> media.bringIn(uri, TimelineMediaKind.VIDEO)
        type.startsWith("image/") -> addPicture(uri)
        else -> vm.showNotice("Only sounds, pictures and videos can be added.", warning = true)
    }
}

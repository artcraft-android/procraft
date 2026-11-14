package io.github.artcraftandroid.procraft

import android.app.ActivityManager
import android.content.Context
import android.os.Build
import android.util.Log
import io.github.artcraftandroid.procraft.core.LogLevel
import java.io.File

/**
 * The app's log, kept in a file as well so that it can be shared (the core keeps it:
 * procraft.log in the app's folder, the run before's beside it): a header about the app and the
 * device, the core's log, the interface's warnings and errors, which go through [w] and [e], and
 * the stack of an exception nothing caught.
 */
object AppLog {
    private const val TAG = "ProCraft"

    /** Whether the file has begun, so lines can go into it. */
    @Volatile
    private var kept = false

    /**
     * Begins the log's file with its header; once a process. Called by the app's own screen, not
     * on every start of the process (a widget's), so that a run that crashed stays the previous log
     * until the app is opened again.
     */
    @Synchronized
    fun start(context: Context) {
        if (kept) return
        val app = context.applicationContext
        runCatching { io.github.artcraftandroid.procraft.core.startLogFile(app.filesDir.path, header(app)) }
            .onSuccess { kept = true }
            .onFailure { Log.e(TAG, "the log's file", it) }
        if (kept) catchCrashes()
    }

    /** An exception nothing caught: its stack into the file, before the system's handler ends the app. */
    private fun catchCrashes() {
        val before = Thread.getDefaultUncaughtExceptionHandler()
        Thread.setDefaultUncaughtExceptionHandler { thread, error ->
            runCatching { toFile(LogLevel.ERROR, "crash", "uncaught on thread \"${thread.name}\"", error) }
            before?.uncaughtException(thread, error)
        }
    }

    /** A warning, to the system's log and the file. */
    fun w(tag: String, message: String, error: Throwable? = null) {
        Log.w(tag, message, error)
        toFile(LogLevel.WARN, tag, message, error)
    }

    /** An error, to the system's log and the file. */
    fun e(tag: String, message: String, error: Throwable? = null) {
        Log.e(tag, message, error)
        toFile(LogLevel.ERROR, tag, message, error)
    }

    /** A line into the file only (its stack after it, for an error). */
    private fun toFile(level: LogLevel, tag: String, message: String, error: Throwable? = null) {
        if (!kept) return
        val text = if (error == null) message else "$message\n${error.stackTraceToString().trimEnd()}"
        runCatching { io.github.artcraftandroid.procraft.core.logLine(level, tag, text) }
    }

    /**
     * What the log says first: the app's version, the device, its system and its memory, and the
     * screen. (The GPU follows, as the core makes its device.)
     */
    private fun header(context: Context): String {
        val info = runCatching { context.packageManager.getPackageInfo(context.packageName, 0) }.getOrNull()
        val memory = ActivityManager.MemoryInfo()
        runCatching { context.getSystemService(ActivityManager::class.java)?.getMemoryInfo(memory) }
        val screen = context.resources.displayMetrics
        val abi = File(context.applicationInfo.nativeLibraryDir ?: "").name
        fun gb(bytes: Long) = "%.1f GB".format(java.util.Locale.ROOT, bytes / 1e9)
        return buildString {
            appendLine("ProCraft ${info?.versionName ?: "?"} (version code ${info?.longVersionCode ?: "?"}), started ${java.time.Instant.now()}")
            appendLine("device: ${Build.MANUFACTURER} ${Build.MODEL} (${Build.DEVICE}), build ${Build.FINGERPRINT}")
            appendLine("Android ${Build.VERSION.RELEASE} (SDK ${Build.VERSION.SDK_INT}), ABIs ${Build.SUPPORTED_ABIS.joinToString()}; the app's library: $abi")
            appendLine(
                "memory: ${gb(memory.totalMem)}, ${gb(memory.availMem)} free${if (memory.lowMemory) " (low)" else ""}; " +
                    "the app's heap up to ${Runtime.getRuntime().maxMemory() shr 20} MB",
            )
            appendLine("screen: ${screen.widthPixels}×${screen.heightPixels}, ${screen.densityDpi} dpi")
        }
    }
}

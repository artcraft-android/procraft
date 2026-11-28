package io.github.artcraftandroid.procraft.canvas

import kotlinx.coroutines.CompletableDeferred
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.async
import kotlinx.coroutines.launch
import kotlinx.coroutines.selects.select
import kotlinx.coroutines.withTimeoutOrNull

/**
 * Exports run one at a time, each apart from the card that waits on it: the card's Cancel ends the
 * wait at once (the card can go), the work is told to stop, and whatever it still makes is thrown
 * away as it ends. A stage that goes on without any progress for [stuckMs] is given up on the same
 * way. An export still ending after its Cancel keeps the next from beginning ([busy]), so two never
 * write one file.
 *
 * Before, the card waited for the work itself: an export stuck in a stage (or only slow, with no
 * progress to show) left the card up with its Cancel doing nothing until the work came back.
 */
internal class ExportRunner(
    private val scope: CoroutineScope,
    private val log: (String) -> Unit,
    private val stuckMs: Long = STUCK_MS,
    private val pollMs: Long = POLL_MS,
    private val now: () -> Long = { System.nanoTime() / 1_000_000 },
) {
    /** How an export ended, as its card waited on it. */
    sealed interface Ended<out T> {
        /** How long the card waited, ms. */
        val ms: Long

        /** It made [value]. */
        data class Made<T>(val value: T, override val ms: Long) : Ended<T>

        /** It made nothing ([error]: it threw). */
        data class Failed(val error: Throwable?, override val ms: Long) : Ended<Nothing>

        /** Cancel ended the wait, at [stage]. */
        data class Cancelled(val stage: String, override val ms: Long) : Ended<Nothing>

        /** It made no progress at [stage] for [stuckMs], and was given up on. */
        data class Stuck(val stage: String, override val ms: Long) : Ended<Nothing>
    }

    /** An export is under way, or one cancelled or given up on is still ending. */
    @Volatile
    var busy = false
        private set

    /** The Cancel of the export under way, if one is. */
    @Volatile
    private var cancelling: CompletableDeferred<Unit>? = null

    /** The card's Cancel: the export under way stops (see [run]). */
    fun cancel() {
        cancelling?.complete(Unit)
    }

    /**
     * Runs [work], an export, apart, and waits on it: [shown] hears its [progress] every [pollMs];
     * its [stage] names where it is for the log. Cancel, or [stuckMs] with neither its progress nor its
     * stage changing, ends the wait at once and calls [stop]; what the work still makes goes to
     * [discard] when it ends. Null (nothing begun) while another is [busy].
     */
    suspend fun <T : Any> run(
        what: String,
        progress: () -> Float,
        stage: () -> String,
        shown: (Float) -> Unit,
        stop: () -> Unit,
        discard: (T) -> Unit,
        work: suspend () -> T?,
    ): Ended<T>? {
        if (busy) return null
        busy = true
        val cancel = CompletableDeferred<Unit>()
        cancelling = cancel
        val started = now()
        val job = scope.async(Dispatchers.IO) { runCatching { work() } }
        job.invokeOnCompletion { busy = false }
        var last = progress() to safely(stage)
        var since = now()
        try {
            while (true) {
                withTimeoutOrNull(pollMs) {
                    select<Unit> {
                        job.onJoin {}
                        cancel.onJoin {}
                    }
                }
                if (job.isCompleted) {
                    val ms = now() - started
                    return job.await().fold(
                        { made -> if (made != null) Ended.Made(made, ms) else Ended.Failed(null, ms) },
                        { error -> Ended.Failed(error, ms) },
                    )
                }
                val at = safely(stage)
                if (cancel.isCompleted) return letGo(what, job, stop, discard, started).let { Ended.Cancelled(at, it) }
                val p = progress()
                shown(p)
                if (p != last.first || at != last.second) {
                    last = p to at
                    since = now()
                } else if (now() - since >= stuckMs) {
                    log("the export ($what) made no progress for ${now() - since} ms at ${at.ifEmpty { "its start" }}: given up on")
                    return letGo(what, job, stop, discard, started).let { Ended.Stuck(at, it) }
                }
            }
        } finally {
            if (cancelling === cancel) cancelling = null
        }
    }

    /**
     * The wait for [job] ended without it: it is told to [stop], and what it makes after goes to
     * [discard], said in the log with how long it took to end. How long the card waited, ms.
     */
    private fun <T : Any> letGo(
        what: String,
        job: kotlinx.coroutines.Deferred<Result<T?>>,
        stop: () -> Unit,
        discard: (T) -> Unit,
        started: Long,
    ): Long {
        val waited = now() - started
        runCatching(stop).onFailure { log("stopping the export ($what) failed: $it") }
        val letGoAt = now()
        scope.launch {
            val late = job.await()
            val ms = now() - letGoAt
            late.getOrNull()?.let { made -> runCatching { discard(made) }.onFailure { log("throwing away what the export ($what) made failed: $it") } }
            log("the export ($what) ended $ms ms after it was let go${late.exceptionOrNull()?.let { ": $it" }.orEmpty()}")
        }
        return waited
    }

    /** [stage], or what went wrong asking it (a session already closed). */
    private fun safely(stage: () -> String): String = runCatching(stage).getOrElse { "" }

    companion object {
        /** How often the card asks how far the export has come (ms). */
        const val POLL_MS = 50L

        /**
         * How long an export may go on with neither its progress nor its stage changing before it is
         * given up on: far past any one stage's work (the core gives up on the GPU after a minute,
         * and on the canvas after half of one; an encoder after 10 s).
         */
        const val STUCK_MS = 120_000L
    }
}

/**
 * What is said when [missed] of [of] artworks shared from the gallery couldn't be written, the rest
 * shared (they were left out without a word).
 */
internal fun galleryMissedNote(missed: Int, of: Int): String =
    if (missed == 1) "1 of $of artworks couldn't be exported and was left out." else "$missed of $of artworks couldn't be exported and were left out."

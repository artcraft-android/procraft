package io.github.artcraftandroid.procraft.canvas

import java.util.Collections
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.runBlocking
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * The export card's wait (#419: "it just stay like this and I can't cancel it"). Before, the card
 * waited for the export itself, so Cancel did nothing until the work came back, however long that
 * took, or never.
 */
class ExportRunnerTest {
    private val scope = CoroutineScope(SupervisorJob())
    private val said: MutableList<String> = Collections.synchronizedList(mutableListOf())

    private fun ms(since: Long) = TimeUnit.NANOSECONDS.toMillis(System.nanoTime() - since)

    /** Cancel ends the wait (the card goes) within a poll, while the work is stuck for seconds yet. */
    @Test
    fun cancelEndsTheWaitAtOnceWhileTheWorkIsStuck() = runBlocking {
        val runner = ExportRunner(scope, { said += it })
        val letGo = CountDownLatch(1)
        val stopped = AtomicBoolean(false)
        val discarded = Collections.synchronizedList(mutableListOf<String>())
        var cancelledAt = 0L
        launch {
            delay(200)
            cancelledAt = System.nanoTime()
            runner.cancel()
        }
        val ended = runner.run(
            "PNG",
            progress = { 0f },
            stage = { "reading the picture" },
            shown = {},
            stop = { stopped.set(true) },
            discard = { made: String -> discarded += made },
        ) {
            // a GPU wait that never comes back while the card is up
            letGo.await(5, TimeUnit.SECONDS)
            "late file"
        }
        val cardWent = ms(cancelledAt)
        println("the card went $cardWent ms after Cancel (the work still stuck)")
        assertTrue("$ended", ended is ExportRunner.Ended.Cancelled)
        assertEquals("reading the picture", (ended as ExportRunner.Ended.Cancelled).stage)
        assertTrue("took $cardWent ms", cardWent <= 150)
        assertTrue("told to stop", stopped.get())
        // still ending: no new export begins over it
        assertTrue(runner.busy)
        assertNull(runner.run("PNG", { 0f }, { "" }, {}, {}, { _: String -> }) { "second" })
        // once it ends, what it made goes, and the next may begin
        letGo.countDown()
        val until = System.nanoTime()
        while ((runner.busy || discarded.isEmpty()) && ms(until) < 2_000) delay(5)
        assertEquals(listOf("late file"), discarded.toList())
        assertFalse(runner.busy)
        assertTrue(said.joinToString(), said.any { it.contains("ended") && it.contains("let go") })
    }

    /** A stage with no progress for its most is given up on (not waited on for good), named. */
    @Test
    fun anExportMakingNoProgressIsGivenUpOn() = runBlocking {
        val runner = ExportRunner(scope, { said += it }, stuckMs = 300, pollMs = 10)
        val letGo = CountDownLatch(1)
        val stopped = AtomicBoolean(false)
        val started = System.nanoTime()
        val ended = runner.run("PSD", { 0.1f }, { "reading layers (3 of 12)" }, {}, { stopped.set(true) }, { _: String -> }) {
            letGo.await(5, TimeUnit.SECONDS)
            "never shared"
        }
        val waited = ms(started)
        letGo.countDown()
        println("given up on after $waited ms with no progress (most 300 ms)")
        assertTrue("$ended", ended is ExportRunner.Ended.Stuck)
        assertEquals("reading layers (3 of 12)", (ended as ExportRunner.Ended.Stuck).stage)
        assertTrue("waited $waited ms", waited in 300..1_000)
        assertTrue(stopped.get())
        assertTrue(said.joinToString(), said.any { it.contains("made no progress") && it.contains("reading layers (3 of 12)") })
    }

    /** An export that keeps making progress (or moving through its stages) is never given up on, however long. */
    @Test
    fun anExportMakingProgressIsWaitedFor() = runBlocking {
        val runner = ExportRunner(scope, { said += it }, stuckMs = 150, pollMs = 10)
        val tenths = java.util.concurrent.atomic.AtomicInteger(0)
        val shown = Collections.synchronizedList(mutableListOf<Float>())
        val ended = runner.run("package", { tenths.get() / 10f }, { "" }, { shown += it }, {}, { _: String -> }) {
            repeat(10) {
                Thread.sleep(60)
                tenths.set(it + 1)
            }
            "file"
        }
        assertEquals("file", (ended as ExportRunner.Ended.Made).value)
        assertTrue("${ended.ms} ms", ended.ms >= 600)
        assertTrue("the card followed it: $shown", shown.distinct().size >= 3)
        assertFalse(runner.busy)
    }

    /** Work that makes nothing, or throws, ends as failed (to be told), the next free to begin. */
    @Test
    fun workThatMakesNothingOrThrowsFails() = runBlocking {
        val runner = ExportRunner(scope, { said += it })
        val none = runner.run("TIFF", { 0f }, { "" }, {}, {}, { _: String -> }) { null }
        assertTrue("$none", none is ExportRunner.Ended.Failed && none.error == null)
        val threw = runner.run("TIFF", { 0f }, { "" }, {}, {}, { _: String -> }) { error("the GPU device was lost") }
        assertTrue("$threw", threw is ExportRunner.Ended.Failed && threw.error?.message == "the GPU device was lost")
        assertFalse(runner.busy)
    }

    /** A stage asked of a session already closed doesn't end the wait wrongly. */
    @Test
    fun aStageThatCantBeAskedIsEmpty() = runBlocking {
        val runner = ExportRunner(scope, { said += it })
        val ended = runner.run("JPEG", { 0f }, { throw IllegalStateException("destroyed") }, {}, {}, { _: String -> }) {
            Thread.sleep(120)
            "file"
        }
        assertEquals("file", (ended as ExportRunner.Ended.Made).value)
    }

    @Test
    fun artworksLeftOutOfAGalleryShareAreSaid() {
        assertEquals("1 of 5 artworks couldn't be exported and was left out.", galleryMissedNote(1, 5))
        assertEquals("2 of 5 artworks couldn't be exported and were left out.", galleryMissedNote(2, 5))
    }

    /** A package's pie: half for its time-lapse's videos when there are any to make, else all the package's. */
    @Test
    fun aPackagesTimeLapseVideosTakeHalfItsPieOnlyWhenThereAreAny() {
        val none = VideosMade()
        assertEquals(0.4f, none.share(0.4f), 0f)
        assertNull(none.stage)
    }
}

package io.github.artcraftandroid.procraft.store

import android.app.Service
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.Looper
import android.os.Message
import android.os.Messenger
import android.os.ParcelFileDescriptor
import android.os.Process
import io.github.artcraftandroid.procraft.core.StoreDecodeKind
import io.github.artcraftandroid.procraft.core.StoreDecoder
import io.github.artcraftandroid.procraft.core.decodeStoreDescriptors
import io.github.artcraftandroid.procraft.core.setStoreDecoder
import java.io.File
import java.util.concurrent.CountDownLatch
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicReference

private const val DECODE = 1
private const val DECODED = 2
private const val FAILED = 3
private const val TIMEOUT_SECONDS = 60L

internal fun registerStoreDecoder(context: Context) {
    setStoreDecoder(AndroidStoreDecoder(context.applicationContext))
}

/** The isolated process receives only read and write descriptors for this job. */
class StoreDecoderService : Service() {
    private val decoder = HandlerThread("Store archive decoder")
    private val deadline = Handler(Looper.getMainLooper())
    private val stopWorker = Runnable { Process.killProcess(Process.myPid()) }

    override fun onCreate() {
        super.onCreate()
        decoder.start()
    }

    override fun onBind(intent: Intent): IBinder =
        Messenger(Handler(decoder.looper) { message ->
            if (message.what != DECODE) return@Handler false
            val args = message.data.also { it.classLoader = ParcelFileDescriptor::class.java.classLoader }
            deadline.postDelayed(stopWorker, TimeUnit.SECONDS.toMillis(TIMEOUT_SECONDS))
            val decoded = runCatching {
                val kind = StoreDecodeKind.valueOf(requireNotNull(args.getString("kind")))
                @Suppress("DEPRECATION")
                val input = requireNotNull(args.getParcelable<ParcelFileDescriptor>("input"))
                input.use {
                    @Suppress("DEPRECATION")
                    val output = requireNotNull(args.getParcelable<ParcelFileDescriptor>("output"))
                    output.use { decodeStoreDescriptors(kind, input.detachFd(), output.detachFd()) }
                }
            }.getOrDefault(false)
            deadline.removeCallbacks(stopWorker)
            runCatching { message.replyTo?.send(Message.obtain(null, if (decoded) DECODED else FAILED)) }
            true
        }).binder

    override fun onDestroy() {
        decoder.quitSafely()
        super.onDestroy()
    }
}

internal class AndroidStoreDecoder(private val context: Context) : StoreDecoder {
    override fun decode(kind: StoreDecodeKind, input: String, output: String): String? {
        if (Looper.myLooper() == Looper.getMainLooper()) return "The Store decoder must run in the background."
        val done = CountDownLatch(1)
        val failure = AtomicReference<String?>("The Store decoder closed before finishing.")
        val reply = Messenger(Handler(Looper.getMainLooper()) { message ->
            if (message.what == DECODED) failure.set(null)
            done.countDown()
            true
        })
        val connection = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName, service: IBinder) {
                if (done.count == 0L) return
                runCatching {
                    ParcelFileDescriptor.open(File(input), ParcelFileDescriptor.MODE_READ_ONLY).use { source ->
                        ParcelFileDescriptor.open(File(output), ParcelFileDescriptor.MODE_WRITE_ONLY).use { target ->
                            val request = Message.obtain(null, DECODE)
                            request.replyTo = reply
                            request.data = Bundle().apply {
                                putString("kind", kind.name)
                                putParcelable("input", source)
                                putParcelable("output", target)
                            }
                            Messenger(service).send(request)
                        }
                    }
                }.onFailure { done.countDown() }
            }

            override fun onServiceDisconnected(name: ComponentName) = done.countDown()
            override fun onBindingDied(name: ComponentName) = done.countDown()
            override fun onNullBinding(name: ComponentName) = done.countDown()
        }
        val intent = Intent(context, StoreDecoderService::class.java)
        return try {
            if (!context.bindService(intent, connection, Context.BIND_AUTO_CREATE)) {
                "The Store decoder couldn't start."
            } else {
                try {
                    if (done.await(TIMEOUT_SECONDS, TimeUnit.SECONDS)) failure.get()
                    else "The Store decoder took too long."
                } finally {
                    done.countDown()
                    context.unbindService(connection)
                }
            }
        } catch (_: Exception) {
            "The Store decoder couldn't be reached."
        }
    }
}

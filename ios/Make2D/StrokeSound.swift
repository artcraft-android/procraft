import AudioToolbox
import AVFAudio
import Foundation

/// Brush sounds (a plugin's): plays the sound the core makes of the stroke being drawn, a few short
/// buffers queued one after another on an Audio Queue, each filled again on the queue's own thread
/// as it finishes, while `follow` says so.
final class StrokeSound {
    static let shared = StrokeSound()

    /// The sound's rate, how much each buffer holds (10 ms) and how many are queued (30 ms ahead).
    private static let rate: UInt32 = 48_000
    private static let frames: UInt32 = 480
    private static let queued = 3

    /// The artwork whose strokes are heard, and whether buffers go back on the queue, as the
    /// queue's thread reads them.
    private let lock = NSLock()
    private var heard: CanvasSession?
    private var running = false

    private var queue: AudioQueueRef?
    private let fills = DispatchQueue(label: "procraft-stroke-sound", qos: .userInteractive)

    private init() {}

    /// Plays `session`'s strokes while `on` (a plugin makes their sound); stops otherwise.
    func follow(_ session: CanvasSession?, on: Bool) {
        let wanted = on && session != nil
        lock.lock()
        heard = wanted ? session : nil
        lock.unlock()
        if wanted, queue == nil {
            start()
        } else if !wanted, queue != nil {
            stop()
        }
    }

    private func start() {
        // heard alongside what else plays (music goes on), and quiet with the device silenced
        try? AVAudioSession.sharedInstance().setCategory(.ambient, options: [.mixWithOthers])
        try? AVAudioSession.sharedInstance().setActive(true)
        var format = AudioStreamBasicDescription(
            mSampleRate: Float64(Self.rate),
            mFormatID: kAudioFormatLinearPCM,
            mFormatFlags: kAudioFormatFlagIsFloat | kAudioFormatFlagIsPacked,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: 1,
            mBitsPerChannel: 32,
            mReserved: 0
        )
        var made: AudioQueueRef?
        let status = AudioQueueNewOutputWithDispatchQueue(&made, &format, 0, fills) { [weak self] queue, buffer in
            self?.refill(queue, buffer)
        }
        guard status == noErr, let made else {
            NSLog("Make2D: brush sounds: no audio queue to play them on (%d)", status)
            return
        }
        queue = made
        lock.lock()
        running = true
        lock.unlock()
        for _ in 0 ..< Self.queued {
            var buffer: AudioQueueBufferRef?
            if AudioQueueAllocateBuffer(made, Self.frames * 4, &buffer) == noErr, let buffer {
                refill(made, buffer)
            }
        }
        let started = AudioQueueStart(made, nil)
        if started != noErr {
            NSLog("Make2D: brush sounds: the audio queue wouldn't start (%d)", started)
        }
    }

    private func stop() {
        guard let queue else { return }
        lock.lock()
        running = false
        lock.unlock()
        // stopped at once, it calls back no more and frees its buffers
        AudioQueueStop(queue, true)
        AudioQueueDispose(queue, true)
        self.queue = nil
    }

    /// The queue has played `buffer` (or it is new): filled from the heard artwork (silence with
    /// none) and queued again.
    private func refill(_ queue: AudioQueueRef, _ buffer: AudioQueueBufferRef) {
        lock.lock()
        let session = heard
        let going = running
        lock.unlock()
        guard going else { return }
        let count = Int(Self.frames)
        let sound = session?.strokeSound(frames: Self.frames, rate: Self.rate) ?? []
        let out = buffer.pointee.mAudioData.assumingMemoryBound(to: Float.self)
        for i in 0 ..< count {
            out[i] = i < sound.count ? max(-1, min(1, sound[i])) : 0
        }
        buffer.pointee.mAudioDataByteSize = Self.frames * 4
        AudioQueueEnqueueBuffer(queue, buffer, 0, nil)
    }
}

/// The sound at `url` (whatever the system can play: MP3, AAC, M4A, WAV, AIFF, FLAC...) as a WAV
/// file of one channel of floats, at most a minute of it, for a plugin's recording; nil if it can't
/// be read.
func decodedSound(_ url: URL) -> Data? {
    let scoped = url.startAccessingSecurityScopedResource()
    defer {
        if scoped {
            url.stopAccessingSecurityScopedResource()
        }
    }
    guard let file = try? AVAudioFile(forReading: url) else { return nil }
    let format = file.processingFormat
    let rate = format.sampleRate
    let channels = Int(format.channelCount)
    guard rate > 0, channels > 0 else { return nil }
    let most = AVAudioFrameCount(min(Double(file.length), rate * 60))
    guard most > 0, let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: most) else { return nil }
    do {
        try file.read(into: buffer, frameCount: most)
    } catch {
        NSLog("Make2D: a sound couldn't be read: %@", "\(error)")
        return nil
    }
    guard let data = buffer.floatChannelData else { return nil }
    let count = Int(buffer.frameLength)
    guard count > 0 else { return nil }
    var mono = [Float](repeating: 0, count: count)
    for c in 0 ..< channels {
        let channel = data[c]
        for i in 0 ..< count {
            mono[i] += channel[i]
        }
    }
    if channels > 1 {
        let share = 1 / Float(channels)
        for i in 0 ..< count {
            mono[i] *= share
        }
    }
    return wavOf(mono, rate: UInt32(rate.rounded()))
}

/// `samples` at `rate` a second as a WAV file of 32-bit floats, one channel.
private func wavOf(_ samples: [Float], rate: UInt32) -> Data {
    var out = Data()
    func put(_ text: String) {
        out.append(contentsOf: Array(text.utf8))
    }
    func put32(_ v: UInt32) {
        withUnsafeBytes(of: v.littleEndian) { out.append(contentsOf: $0) }
    }
    func put16(_ v: UInt16) {
        withUnsafeBytes(of: v.littleEndian) { out.append(contentsOf: $0) }
    }
    let bytes = UInt32(samples.count * 4)
    put("RIFF")
    put32(36 + bytes)
    put("WAVEfmt ")
    put32(16)
    put16(3) // IEEE float
    put16(1)
    put32(rate)
    put32(rate * 4)
    put16(4)
    put16(32)
    put("data")
    put32(bytes)
    // the iPad is little-endian, as a WAV file's samples are
    samples.withUnsafeBufferPointer { out.append(Data(buffer: $0)) }
    return out
}

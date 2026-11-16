import AVFoundation
import SwiftUI
import UIKit

// Sounds and videos on the timeline, the iPad's half: reading a file brought in (its length,
// picture size and waveform), videos' pictures for the canvas, and playing the sounds with the
// timeline. The core keeps the files and their timing; nothing here is saved.

/// The timeline's media as the panel uses it.
@Observable
final class TimelineMediaTool {
    @ObservationIgnored private unowned let model: CanvasModel
    @ObservationIgnored let sounds = SoundPlayer()
    @ObservationIgnored private var peaksRead: [UInt64: [Float]] = [:]

    /// The file picker shown, for this kind.
    var picking: TimelineMediaKind?
    /// Reading a file brought in.
    var bringing = false
    /// Why the last file couldn't be brought in.
    var failed: String?
    /// The animatic detector open on this video clip.
    var detecting: UInt64?

    init(model: CanvasModel) {
        self.model = model
    }

    var session: CanvasSession { model.session }

    func clip(_ id: UInt64) -> TimelineMediaClip? {
        session.timelineMediaClip(id: id)
    }

    /// A sound's waveform, read once.
    func peaks(_ media: UInt64) -> [Float] {
        if let p = peaksRead[media] {
            return p
        }
        let p = session.timelineMediaPeaks(id: media)
        peaksRead[media] = p
        return p
    }

    /// Brings in the file at `url` (picked, so reached through its security scope) as `kind`, at
    /// the playhead.
    func bringIn(_ url: URL, kind: TimelineMediaKind) {
        bringing = true
        failed = nil
        let session = session
        Task.detached(priority: .userInitiated) { [weak self] in
            let scoped = url.startAccessingSecurityScopedResource()
            defer {
                if scoped {
                    url.stopAccessingSecurityScopedResource()
                }
            }
            let outcome: Result<UInt64, Error>
            do {
                let read = try await MediaReading.read(url, kind: kind)
                outcome = .success(try session.importTimelineMedia(import: read))
            } catch {
                outcome = .failure(error)
            }
            await MainActor.run {
                guard let self else { return }
                self.bringing = false
                switch outcome {
                case let .success(clip) where clip != 0:
                    self.model.refresh()
                    self.model.timeline.picked = clip
                case .success:
                    self.failed = "It couldn't go on the timeline."
                case let .failure(e):
                    self.failed = (e as? MediaReading.Problem)?.said ?? "That file can't be read."
                }
            }
        }
    }

    func slip(_ id: UInt64, to seconds: Double, done: Bool) {
        if session.slipTimelineMedia(id: id, trimStart: seconds, done: done) || done {
            model.refresh()
        }
    }

    func setSound(_ c: TimelineMediaClip, volume: Float? = nil, muted: Bool? = nil, offset: Double? = nil, done: Bool) {
        _ = session.setTimelineSound(
            id: c.clip, volume: volume ?? c.volume, muted: muted ?? c.muted, offset: offset ?? c.offset, done: done
        )
        model.refresh()
    }

    /// A sound's level: keyed at the playhead when it has level keys (or `key`, keying it there
    /// first), else its one level.
    func setLevel(_ c: TimelineMediaClip, _ level: Float, key: Bool = false, done: Bool) {
        if c.volumeKeys.isEmpty, !key {
            setSound(c, volume: level, done: done)
            return
        }
        _ = session.setTimelineVolumeKey(id: c.clip, volume: level, done: done)
        model.refresh()
    }

    func removeLevelKey(_ c: TimelineMediaClip) {
        _ = session.removeTimelineVolumeKey(id: c.clip)
        model.refresh()
    }

    func placeVideo(_ id: UInt64, _ rect: [Float], done: Bool) {
        _ = session.setTimelineVideoRect(id: id, rect: rect, done: done)
        model.refresh()
    }

    // MARK: playing along

    /// Starts the sounds playing from frame `frame`; returns how long until they start (seconds),
    /// for the pictures to wait as long.
    @discardableResult
    func play(from frame: UInt32) -> Double {
        let fps = Double(max(model.timeline.info?.fps ?? 24, 1))
        return sounds.start(session.timelineSounds(frame: frame), at: Double(frame) / fps)
    }

    /// Playing went on from `frame` to `next` (by `step`): a jump starts the sounds again from
    /// there, going backwards (ping-pong) silences them. Returns the wait, as `play`.
    func follow(from frame: UInt32, to next: UInt32, step: Int32) -> Double {
        if step < 0 {
            sounds.stop()
            return 0
        }
        if next != frame &+ 1 || !sounds.playing {
            return play(from: next)
        }
        sounds.level(at: next)
        return 0
    }

    func stop() {
        sounds.stop()
    }

    /// Scrubbed onto `frame`: a frame's sound, the slice under it.
    func blip(_ frame: UInt32) {
        let fps = Double(max(model.timeline.info?.fps ?? 24, 1))
        sounds.blip(session.timelineSounds(frame: frame), at: Double(frame) / fps, length: max(1 / fps, 0.06))
    }
}

/// Reading a file brought in, on the iPad's decoders.
enum MediaReading {
    struct Problem: Error {
        let said: String
    }

    static func read(_ url: URL, kind: TimelineMediaKind) async throws -> MediaImport {
        let asset = AVURLAsset(url: url)
        let seconds = try await asset.load(.duration).seconds
        guard seconds.isFinite, seconds > 0 else { throw Problem(said: "That file has no length.") }
        switch kind {
        case .audio:
            guard try await !asset.loadTracks(withMediaType: .audio).isEmpty else {
                throw Problem(said: "That file has no sound.")
            }
            return MediaImport(
                path: url.path, name: url.lastPathComponent, kind: .audio, seconds: seconds, width: 0, height: 0,
                peaks: peaks(url)
            )
        case .video:
            guard let track = try await asset.loadTracks(withMediaType: .video).first else {
                throw Problem(said: "That file has no picture.")
            }
            let (size, turn) = try await track.load(.naturalSize, .preferredTransform)
            let shown = size.applying(turn)
            return MediaImport(
                path: url.path, name: url.lastPathComponent, kind: .video, seconds: seconds,
                width: UInt32(abs(shown.width).rounded()), height: UInt32(abs(shown.height).rounded()), peaks: []
            )
        }
    }

    /// The loudest sample in each [`mediaPeaksPerSecond`]th of a second, 0...1: one pass over the
    /// file. Empty when it can't be decoded.
    static func peaks(_ url: URL) -> [Float] {
        guard let file = try? AVAudioFile(forReading: url) else { return [] }
        let format = file.processingFormat
        let bucket = max(AVAudioFrameCount(format.sampleRate) / AVAudioFrameCount(mediaPeaksPerSecond()), 1)
        guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: bucket * 64) else { return [] }
        var out: [Float] = []
        var loudest: Float = 0
        var filled: AVAudioFrameCount = 0
        while file.framePosition < file.length {
            guard (try? file.read(into: buffer)) != nil, buffer.frameLength > 0,
                  let channels = buffer.floatChannelData else { break }
            let count = Int(format.channelCount)
            for i in 0 ..< Int(buffer.frameLength) {
                for c in 0 ..< count {
                    loudest = max(loudest, abs(channels[c][i]))
                }
                filled += 1
                if filled == bucket {
                    out.append(min(loudest, 1))
                    loudest = 0
                    filled = 0
                }
            }
        }
        if filled > 0 {
            out.append(min(loudest, 1))
        }
        return out
    }
}

/// Videos' pictures for the canvas, read with AVFoundation: one image generator a file, scaled
/// to the side asked for. Called from the canvas's rendering, off the main thread.
final class VideoFrameReader: VideoFrames, @unchecked Sendable {
    static let shared = VideoFrameReader()

    private let lock = NSLock()
    private var generators: [String: AVAssetImageGenerator] = [:]

    func picture(path: String, seconds: Double, maxSide: UInt32) -> ThumbnailImage? {
        let generator = generator(path, side: maxSide)
        let time = CMTime(seconds: max(seconds, 0), preferredTimescale: 600)
        guard let image = try? generator.copyCGImage(at: time, actualTime: nil) else { return nil }
        return Self.rgba(image)
    }

    private func generator(_ path: String, side: UInt32) -> AVAssetImageGenerator {
        lock.lock()
        defer { lock.unlock() }
        if let g = generators[path] {
            return g
        }
        let g = AVAssetImageGenerator(asset: AVURLAsset(url: URL(fileURLWithPath: path)))
        g.appliesPreferredTrackTransform = true
        // near enough a frame for drawing over, and quick to find while playing
        let near = CMTime(value: 1, timescale: 100)
        g.requestedTimeToleranceBefore = near
        g.requestedTimeToleranceAfter = near
        if side > 0 {
            g.maximumSize = CGSize(width: Int(side), height: Int(side))
        }
        generators[path] = g
        return g
    }

    /// `image`'s pixels as RGBA8, rows top to bottom.
    static func rgba(_ image: CGImage) -> ThumbnailImage? {
        let (w, h) = (image.width, image.height)
        guard w > 0, h > 0 else { return nil }
        var bytes = Data(count: w * h * 4)
        let drawn = bytes.withUnsafeMutableBytes { raw -> Bool in
            guard let context = CGContext(
                data: raw.baseAddress, width: w, height: h, bitsPerComponent: 8, bytesPerRow: w * 4,
                space: CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB(),
                bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            context.draw(image, in: CGRect(x: 0, y: 0, width: w, height: h))
            return true
        }
        return drawn ? ThumbnailImage(width: UInt32(w), height: UInt32(h), rgba: bytes) : nil
    }
}

extension TimelineSound {
    /// Heard somewhere: its level, or any of its keyed levels, above silence.
    var audible: Bool {
        gain > 0 || levels.contains { $0 > 0 }
    }

    /// Its level on timeline frame `frame`: keyed, or its one level.
    func level(at frame: UInt32) -> Float {
        guard !levels.isEmpty else { return gain }
        let i = min(max(Int(frame) - Int(levelsFrom), 0), levels.count - 1)
        return levels[i]
    }
}

/// Plays the timeline's sounds: a player for each, all started together at one moment, a keyed
/// sound's level following the frames as they play.
final class SoundPlayer {
    private let engine = AVAudioEngine()
    private var players: [(node: AVAudioPlayerNode, sound: TimelineSound)] = []
    private var files: [String: AVAudioFile] = [:]
    private(set) var playing = false
    /// How far ahead sounds are scheduled, so they all start at once.
    private let lead = 0.06

    private func file(_ path: String) -> AVAudioFile? {
        if let f = files[path] {
            return f
        }
        let f = try? AVAudioFile(forReading: URL(fileURLWithPath: path))
        files[path] = f
        return f
    }

    private func ready() -> Bool {
        let session = AVAudioSession.sharedInstance()
        try? session.setCategory(.playback, options: [.mixWithOthers])
        try? session.setActive(true)
        if !engine.isRunning {
            // (the mixer, and its way out, are made when first asked for: an engine started with
            // neither throws)
            _ = engine.mainMixerNode
            engine.prepare()
            do {
                try engine.start()
            } catch {
                return false
            }
        }
        return true
    }

    /// `sounds` playing as from `time` seconds into the timeline; returns how long until they
    /// start (0 with nothing to play).
    func start(_ sounds: [TimelineSound], at time: Double) -> Double {
        stop()
        let heard = sounds.filter { $0.audible && $0.at + $0.length > time }
        guard !heard.isEmpty, ready() else { return 0 }
        schedule(heard, at: time, length: nil)
        playing = true
        return lead
    }

    /// The slice of `sounds` under `time`, `length` seconds of it.
    func blip(_ sounds: [TimelineSound], at time: Double, length: Double) {
        stop()
        let under = sounds.filter { $0.gain > 0 && $0.at <= time && time < $0.at + $0.length }
        guard !under.isEmpty, ready() else { return }
        schedule(under, at: time, length: length)
    }

    private func schedule(_ sounds: [TimelineSound], at time: Double, length: Double?) {
        let startHost = mach_absolute_time() + AVAudioTime.hostTime(forSeconds: lead)
        for s in sounds {
            guard let f = file(s.path) else { continue }
            let rate = f.processingFormat.sampleRate
            let into = max(time - s.at, 0)
            let wait = max(s.at - time, 0)
            let from = s.from + into
            let left = min(s.length - into, length ?? .infinity)
            let first = AVAudioFramePosition(from * rate)
            let count = AVAudioFrameCount(max(left * rate, 0))
            guard count > 0, first < f.length else { continue }
            let node = AVAudioPlayerNode()
            engine.attach(node)
            engine.connect(node, to: engine.mainMixerNode, format: f.processingFormat)
            node.volume = s.gain
            node.scheduleSegment(f, startingFrame: first, frameCount: min(count, AVAudioFrameCount(f.length - first)), at: nil)
            node.play(at: AVAudioTime(hostTime: startHost + AVAudioTime.hostTime(forSeconds: wait)))
            players.append((node, s))
        }
    }

    /// The keyed sounds' levels as they are on timeline frame `frame`.
    func level(at frame: UInt32) {
        for p in players where !p.sound.levels.isEmpty {
            p.node.volume = p.sound.level(at: frame)
        }
    }

    func stop() {
        for (node, _) in players {
            node.stop()
            engine.detach(node)
        }
        players = []
        playing = false
    }
}

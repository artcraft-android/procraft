import AVFoundation
import PhotosUI
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
    /// the playhead. With `copied`, the file is a copy made for this and is removed afterwards
    /// (the core keeps its own).
    func bringIn(_ url: URL, kind: TimelineMediaKind, copied: Bool = false) {
        bringing = true
        failed = nil
        let session = session
        Task.detached(priority: .userInitiated) { [weak self] in
            let scoped = url.startAccessingSecurityScopedResource()
            defer {
                if scoped {
                    url.stopAccessingSecurityScopedResource()
                }
                if copied {
                    try? FileManager.default.removeItem(at: url.deletingLastPathComponent())
                }
            }
            let outcome: Result<UInt64, Error>
            do {
                let read = try await MediaReading.read(url, kind: kind)
                defer { MediaReading.forget(read) }
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

    /// Brings in a video picked from the Photos library, at the playhead: copied out of the
    /// library to a file of its own first (from iCloud, if it is only there), then brought in as
    /// one from Files is.
    func bringIn(photo item: PhotosPickerItem) {
        bringing = true
        failed = nil
        Task { @MainActor [weak self] in
            let movie = try? await item.loadTransferable(type: PickedMovie.self)
            guard let self else { return }
            guard let movie else {
                self.bringing = false
                self.failed = "That video couldn't be read from Photos."
                return
            }
            self.bringIn(movie.url, kind: .video, copied: true)
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

    /// Scrubbed onto `frame`: a frame's sound, the slice under it. Nothing with the pref off.
    func blip(_ frame: UInt32) {
        guard UserDefaults.standard.object(forKey: "timelineScrubSound") as? Bool ?? true else { return }
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
                peaks: peaks(url), soundPath: nil, soundSeconds: 0
            )
        case .video:
            guard let track = try await asset.loadTracks(withMediaType: .video).first else {
                throw Problem(said: "That file has no picture.")
            }
            let (size, turn) = try await track.load(.naturalSize, .preferredTransform)
            let shown = size.applying(turn)
            // its sound, if it has one, as a sound file of its own: it plays with the picture,
            // and can be taken out as a sound clip
            let sound = await sound(of: asset)
            let soundSeconds = sound.map { AVURLAsset(url: $0) }
            return MediaImport(
                path: url.path, name: url.lastPathComponent, kind: .video, seconds: seconds,
                width: UInt32(abs(shown.width).rounded()), height: UInt32(abs(shown.height).rounded()),
                peaks: sound.map { peaks($0) } ?? [], soundPath: sound?.path,
                soundSeconds: (try? await soundSeconds?.load(.duration).seconds) ?? 0
            )
        }
    }

    /// A video's sound written out as a sound file in a folder of its own in the temporary
    /// folder (the core copies it; `forget` removes it); none if the video has no sound or it
    /// can't be written.
    static func sound(of asset: AVURLAsset) async -> URL? {
        guard (try? await asset.loadTracks(withMediaType: .audio).isEmpty) == false,
              let export = AVAssetExportSession(asset: asset, presetName: AVAssetExportPresetAppleM4A) else { return nil }
        let folder = FileManager.default.temporaryDirectory.appendingPathComponent("procraft-sound-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
        let out = folder.appendingPathComponent("sound.m4a")
        export.outputURL = out
        export.outputFileType = .m4a
        await withCheckedContinuation { (done: CheckedContinuation<Void, Never>) in
            export.exportAsynchronously { done.resume() }
        }
        guard export.status == .completed else {
            try? FileManager.default.removeItem(at: folder)
            return nil
        }
        return out
    }

    /// The sound file `sound(of:)` wrote for `read` is removed, once the core has its own copy.
    static func forget(_ read: MediaImport) {
        if let path = read.soundPath {
            try? FileManager.default.removeItem(at: URL(fileURLWithPath: path).deletingLastPathComponent())
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

/// Videos' pictures for the canvas, read with AVFoundation, scaled to the side asked for: read on
/// by a `VideoRun` while the times asked for go steadily forward (playing, an export, a scrub
/// carried on), found afresh by an image generator when they jump. Either way the picture is the
/// frame showing at the time asked, so a frame drawn over is the same one however the playhead
/// came to it. Called from the canvas's own threads, off the main one; one picture at a time.
final class VideoFrameReader: VideoFrames, @unchecked Sendable {
    static let shared = VideoFrameReader()

    private final class Video {
        var generator: AVAssetImageGenerator?
        var run: VideoRun?
        /// No run reads it (it is shown skewed or mirrored): always the generator.
        var unread = false
        /// The time last asked for.
        var last: Double?
    }

    private let lock = NSLock()
    /// By the file and the side its pictures are scaled to.
    private var videos: [String: Video] = [:]

    func picture(path: String, seconds: Double, maxSide: UInt32) -> ThumbnailImage? {
        lock.lock()
        defer { lock.unlock() }
        let key = "\(maxSide) \(path)"
        let v = videos[key] ?? Video()
        videos[key] = v
        let t = max(seconds, 0)
        let last = v.last
        v.last = t
        // on from the frame its run has
        if let run = v.run, run.follows(t), let p = run.picture(at: t) {
            return p
        }
        // a short step forward from the last time asked: they are going steadily on, so a run
        // starts here
        if let last, t > last, t - last <= VideoRun.reach, !v.unread {
            if v.run == nil {
                v.run = VideoRun(path: path, maxSide: Int(maxSide))
                v.unread = v.run == nil
            }
            if let p = v.run?.picture(at: t) {
                return p
            }
        }
        // a jump: found afresh, and the run (with the frames it has read ahead) let go
        v.run = nil
        return generate(v, path, t, maxSide)
    }

    private func generate(_ v: Video, _ path: String, _ seconds: Double, _ side: UInt32) -> ThumbnailImage? {
        let generator: AVAssetImageGenerator
        if let has = v.generator {
            generator = has
        } else {
            generator = AVAssetImageGenerator(asset: AVURLAsset(url: URL(fileURLWithPath: path)))
            generator.appliesPreferredTrackTransform = true
            // the frame showing at the time asked, as a run gives it: one a hundredth of a
            // second off would be the next frame as often as not
            generator.requestedTimeToleranceBefore = .zero
            generator.requestedTimeToleranceAfter = .zero
            if side > 0 {
                generator.maximumSize = CGSize(width: Int(side), height: Int(side))
            }
            v.generator = generator
        }
        let time = CMTime(seconds: seconds + VideoRun.slack, preferredTimescale: 90000)
        guard let image = try? generator.copyCGImage(at: time, actualTime: nil) else { return nil }
        return Self.rgba(image)
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

    /// The audio session and the engine are being made ready, off the main thread.
    private var readying = false

    /// Whether sounds can be scheduled now. The first time, and after the system took the audio
    /// away, the session and the engine are made ready off the main thread (see `AppAudio`: its
    /// calls can hold the interface up, and were made here before every play and every scrubbed
    /// frame) and this says no meanwhile: what is asked for then is silent, and playing picks the
    /// sounds up on its next frame, where they are by then.
    private func ready() -> Bool {
        if AppAudio.inForce == .playback, engine.isRunning {
            return true
        }
        guard !readying else { return false }
        readying = true
        let engine = engine
        AppAudio.use(.playback, work: {
            // (nothing else touches the engine while `readying`)
            guard !engine.isRunning else { return }
            // (the mixer, and its way out, are made when first asked for: an engine started with
            // neither throws)
            _ = engine.mainMixerNode
            engine.prepare()
            try? engine.start()
        }, then: { [weak self] in
            self?.readying = false
        })
        return false
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
        // (none can be playing while the engine is being made ready)
        for (node, _) in players where !readying {
            node.stop()
            engine.detach(node)
        }
        players = []
        playing = false
    }
}

/// A video from the Photos library, copied out to a file of its own: in a folder of its own in
/// the temporary folder, under the name Photos gave it (the file handed over goes away once it
/// has been received).
struct PickedMovie: Transferable {
    let url: URL

    static var transferRepresentation: some TransferRepresentation {
        FileRepresentation(contentType: .movie) { movie in
            SentTransferredFile(movie.url)
        } importing: { received in
            let folder = FileManager.default.temporaryDirectory
                .appendingPathComponent("PhotosVideos", isDirectory: true)
                .appendingPathComponent(UUID().uuidString, isDirectory: true)
            try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)
            let to = folder.appendingPathComponent(received.file.lastPathComponent)
            try FileManager.default.copyItem(at: received.file, to: to)
            return PickedMovie(url: to)
        }
    }
}

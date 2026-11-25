import Accelerate
import AVFoundation
import CoreVideo
import Foundation

// Reading a video's pictures one after another (see `VideoFrameReader`, TimelineMedia.swift).
// An image generator finds each picture afresh from the key frame before it: 15 to 25 ms for a
// 1080p video with a key frame a second, 45 to 100 ms for one with few key frames or for 4K. A
// video played, exported or scrubbed steadily forward asks for pictures in order, and read in
// order the next one costs a frame's decoding: 1 to 5 ms playing, measured on an M-series Mac
// with the same files. Times that jump (a scrub back and forth, a frame back) are still found
// afresh, which costs what it did.

/// A video read frame after frame: asked for a time at or a little after the last, it decodes
/// only the frames between, where an image generator finds its way from the key frame before
/// every time. Asked for an earlier time, or one far on, it starts again from there.
final class VideoRun {
    /// How far ahead of the frame it has it reads on to a time asked for (seconds): further, and
    /// the frames between would take longer to decode than finding the frame afresh.
    static let reach = 0.3
    /// A frame this much after a time asked for still shows at it (the time is a float's).
    static let slack = 0.001

    private let asset: AVURLAsset
    private let track: AVAssetTrack
    /// The size its frames are stored at, before they are turned.
    private let natural: CGSize
    /// Quarter turns clockwise the frames are shown turned by.
    private let turns: Int
    private let size: (width: Int, height: Int)
    private var reader: AVAssetReader?
    private var output: AVAssetReaderTrackOutput?
    /// The frame it has (shown from `at`), and the one after it, read ahead to know when it ends.
    private var frame: (at: Double, buffer: CVPixelBuffer)?
    private var next: (at: Double, buffer: CVPixelBuffer)?
    private var ended = false

    /// None if the video isn't one this reads (no picture, or shown skewed or mirrored).
    init?(path: String, maxSide: Int) {
        asset = AVURLAsset(url: URL(fileURLWithPath: path))
        guard let read = Self.loaded(asset) else { return nil }
        track = read.track
        natural = read.natural
        let t = read.turn
        // only whole quarter turns: anything else is the image generator's
        let quarter: Int
        switch (t.a.rounded(), t.b.rounded(), t.c.rounded(), t.d.rounded()) {
        case (1, 0, 0, 1): quarter = 0
        case (0, 1, -1, 0): quarter = 1
        case (-1, 0, 0, -1): quarter = 2
        case (0, -1, 1, 0): quarter = 3
        default: return nil
        }
        guard abs(abs(t.a) + abs(t.b) - 1) < 0.01, abs(abs(t.c) + abs(t.d) - 1) < 0.01 else { return nil }
        turns = quarter
        let (w, h) = (Double(abs(natural.width)), Double(abs(natural.height)))
        guard w >= 1, h >= 1 else { return nil }
        let scale = maxSide > 0 ? min(Double(maxSide) / max(w, h), 1) : 1
        size = (max(Int((w * scale).rounded()), 1), max(Int((h * scale).rounded()), 1))
    }

    /// The video's picture track with how its frames are stored and turned, waited for here (so
    /// never on the main thread). Read through the loading calls that don't block a thread of
    /// their own: the properties' plain getters wait inside AVFoundation on a thread less urgent
    /// than the one asking, which Xcode reports as a hang risk.
    private static func loaded(_ asset: AVURLAsset) -> (track: AVAssetTrack, natural: CGSize, turn: CGAffineTransform)? {
        final class Read: @unchecked Sendable {
            var got: (track: AVAssetTrack, natural: CGSize, turn: CGAffineTransform)?
        }
        let read = Read()
        let done = DispatchSemaphore(value: 0)
        Task.detached(priority: .userInitiated) {
            if let track = try? await asset.loadTracks(withMediaType: .video).first,
               let (natural, turn) = try? await track.load(.naturalSize, .preferredTransform) {
                read.got = (track, natural, turn)
            }
            done.signal()
        }
        done.wait()
        return read.got
    }

    /// Whether `seconds` is at the frame it has or closely after it, so reading on gets there.
    func follows(_ seconds: Double) -> Bool {
        guard let f = frame, reader?.status != .failed else { return false }
        return seconds + Self.slack >= f.at && seconds - f.at <= Self.reach
    }

    /// The picture shown `seconds` in, read on from the frame it has (see `follows`; else from
    /// `seconds` afresh): RGBA8, rows top to bottom. None if it can't be read.
    func picture(at seconds: Double) -> ThumbnailImage? {
        if !follows(seconds), !start(at: seconds) {
            return nil
        }
        while let n = next, n.at <= seconds + Self.slack {
            frame = n
            next = read()
        }
        // (a reader the system stopped, as it does in the background, has no more to give)
        guard let f = frame, reader?.status != .failed else {
            frame = nil
            return nil
        }
        return rgba(f.buffer)
    }

    /// Reading from a little before `seconds`, so the frame showing then is the first it has.
    private func start(at seconds: Double) -> Bool {
        reader?.cancelReading()
        frame = nil
        next = nil
        ended = false
        guard let r = try? AVAssetReader(asset: asset) else { return false }
        var settings: [String: Any] = [kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA]
        if size.width != Int(abs(natural.width).rounded()) || size.height != Int(abs(natural.height).rounded()) {
            settings[kCVPixelBufferWidthKey as String] = size.width
            settings[kCVPixelBufferHeightKey as String] = size.height
        }
        let o = AVAssetReaderTrackOutput(track: track, outputSettings: settings)
        o.alwaysCopiesSampleData = false
        guard r.canAdd(o) else { return false }
        r.add(o)
        let from = max(seconds - 0.25, 0)
        r.timeRange = CMTimeRange(start: CMTime(seconds: from, preferredTimescale: 600), duration: .positiveInfinity)
        guard r.startReading() else { return false }
        reader = r
        output = o
        frame = read()
        next = read()
        // (its first frame must be the one showing then, or one before it)
        return frame.map { $0.at <= seconds + Self.slack } ?? false
    }

    private func read() -> (at: Double, buffer: CVPixelBuffer)? {
        guard !ended, let output else { return nil }
        while let sample = output.copyNextSampleBuffer() {
            guard let buffer = CMSampleBufferGetImageBuffer(sample) else { continue }
            return (CMSampleBufferGetPresentationTimeStamp(sample).seconds, buffer)
        }
        ended = true
        return nil
    }

    /// `buffer` (BGRA) as RGBA8 turned the way the video shows.
    private func rgba(_ buffer: CVPixelBuffer) -> ThumbnailImage? {
        CVPixelBufferLockBaseAddress(buffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(buffer) else { return nil }
        let (w, h) = (CVPixelBufferGetWidth(buffer), CVPixelBufferGetHeight(buffer))
        var src = vImage_Buffer(data: base, height: vImagePixelCount(h), width: vImagePixelCount(w), rowBytes: CVPixelBufferGetBytesPerRow(buffer))
        let sideways = turns % 2 == 1
        let (ow, oh) = sideways ? (h, w) : (w, h)
        var out = Data(count: ow * oh * 4)
        let map: [UInt8] = [2, 1, 0, 3]
        let ok = out.withUnsafeMutableBytes { raw -> Bool in
            guard let to = raw.baseAddress else { return false }
            var dst = vImage_Buffer(data: to, height: vImagePixelCount(oh), width: vImagePixelCount(ow), rowBytes: ow * 4)
            if turns == 0 {
                return vImagePermuteChannels_ARGB8888(&src, &dst, map, vImage_Flags(kvImageNoFlags)) == kvImageNoError
            }
            // turned first (in the buffer's own channel order), then its channels put in order
            var back: [UInt8] = [0, 0, 0, 0]
            // (vImage's quarter turns go counter-clockwise)
            let turn = UInt8((4 - turns) % 4)
            guard vImageRotate90_ARGB8888(&src, &dst, turn, &back, vImage_Flags(kvImageNoFlags)) == kvImageNoError else { return false }
            return vImagePermuteChannels_ARGB8888(&dst, &dst, map, vImage_Flags(kvImageNoFlags)) == kvImageNoError
        }
        return ok ? ThumbnailImage(width: UInt32(ow), height: UInt32(oh), rgba: out) : nil
    }
}

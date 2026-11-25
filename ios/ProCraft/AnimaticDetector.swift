import AVFoundation
import CoreVideo
import SwiftUI

// The animatic detector: for a video of held drawings (a storyboard or animatic export), where it
// changes drawing, so a track of new drawings can be made with its timing to draw over. The iPad
// decodes the clip's frames into small grey pictures; the core scores and cuts them.

/// Decoding a video clip's frames for the detector.
enum AnimaticFrames {
    /// One grey picture (`side` at most along its longer edge) for each of `times` (seconds into
    /// the file, rising): the frame on show then. One pass through the file; `progress` hears
    /// 0...1. Nil where a frame couldn't be read.
    static func read(path: String, times: [Double], side: Int, progress: @escaping (Double) -> Void) async throws
        -> (width: UInt32, height: UInt32, frames: [Data?])
    {
        guard let first = times.first, let last = times.last else { return (0, 0, []) }
        let asset = AVURLAsset(url: URL(fileURLWithPath: path))
        guard let track = try await asset.loadTracks(withMediaType: .video).first else {
            throw MediaReading.Problem(said: "That video has no picture.")
        }
        let (size, fps) = try await track.load(.naturalSize, .nominalFrameRate)
        let step = 1 / Double(fps > 0 ? fps : 30)
        // decoded at twice the size and pooled 2×2: that averages away most compression noise
        let scale = min(Double(2 * side) / Double(max(size.width, size.height, 1)), 1)
        let dw = max(2, Int((Double(size.width) * scale).rounded()) & ~1)
        let dh = max(2, Int((Double(size.height) * scale).rounded()) & ~1)
        let reader = try AVAssetReader(asset: asset)
        let output = AVAssetReaderTrackOutput(track: track, outputSettings: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: dw,
            kCVPixelBufferHeightKey as String: dh,
        ])
        output.alwaysCopiesSampleData = false
        guard reader.canAdd(output) else { throw MediaReading.Problem(said: "That video can't be read.") }
        reader.add(output)
        // a little early, so the frame already on show at the first time comes too
        let start = max(0, first - max(step * 2, 0.1))
        reader.timeRange = CMTimeRange(
            start: CMTime(seconds: start, preferredTimescale: 600),
            end: CMTime(seconds: last + step * 2 + 0.1, preferredTimescale: 600)
        )
        guard reader.startReading() else { throw MediaReading.Problem(said: "That video can't be read.") }
        defer { reader.cancelReading() }

        var out = [Data?](repeating: nil, count: times.count)
        var current: Data?
        var pending: (at: Double, luma: Data?)?
        var done = false
        func next() -> (Double, Data?)? {
            guard reader.status == .reading, let sample = output.copyNextSampleBuffer() else { return nil }
            let at = CMSampleBufferGetPresentationTimeStamp(sample).seconds
            return (at, CMSampleBufferGetImageBuffer(sample).flatMap(pooledLuma))
        }
        for (i, time) in times.enumerated() {
            try Task.checkCancellation()
            while !done {
                if pending == nil {
                    pending = next()
                    if pending == nil {
                        done = true
                        break
                    }
                }
                // (a little slack, so a frame landing on the time isn't missed)
                guard let p = pending, p.at <= time + 0.001 else { break }
                if let luma = p.luma {
                    current = luma
                }
                pending = nil
            }
            // before the first frame, the first frame (as the canvas shows it)
            if current == nil, let luma = pending?.luma {
                current = luma
            }
            out[i] = current
            if i % 16 == 0 || i == times.count - 1 {
                progress(Double(i + 1) / Double(times.count))
            }
        }
        return (UInt32(dw / 2), UInt32(dh / 2), out)
    }

    /// The lightness of a BGRA picture, pooled 2×2 (Rec. 601).
    private static func pooledLuma(_ buffer: CVPixelBuffer) -> Data? {
        guard CVPixelBufferGetPixelFormatType(buffer) == kCVPixelFormatType_32BGRA else { return nil }
        CVPixelBufferLockBaseAddress(buffer, .readOnly)
        defer { CVPixelBufferUnlockBaseAddress(buffer, .readOnly) }
        guard let base = CVPixelBufferGetBaseAddress(buffer) else { return nil }
        let w = CVPixelBufferGetWidth(buffer) / 2
        let h = CVPixelBufferGetHeight(buffer) / 2
        guard w > 0, h > 0 else { return nil }
        let row = CVPixelBufferGetBytesPerRow(buffer)
        let bytes = base.assumingMemoryBound(to: UInt8.self)
        var luma = Data(count: w * h)
        luma.withUnsafeMutableBytes { raw in
            guard let dst = raw.baseAddress?.assumingMemoryBound(to: UInt8.self) else { return }
            for y in 0 ..< h {
                let r0 = bytes + 2 * y * row
                let r1 = r0 + row
                for x in 0 ..< w {
                    let a = r0 + 8 * x, b = r1 + 8 * x
                    let blue = Int(a[0]) + Int(a[4]) + Int(b[0]) + Int(b[4])
                    let green = Int(a[1]) + Int(a[5]) + Int(b[1]) + Int(b[5])
                    let red = Int(a[2]) + Int(a[6]) + Int(b[2]) + Int(b[6])
                    dst[y * w + x] = UInt8(min(255, (29 * blue + 150 * green + 77 * red) >> 10))
                }
            }
        }
        return luma
    }
}

/// "Make Timing Track": the video clip's change scores as bars, the cuts they make at the chosen
/// sensitivity and shortest hold, the frame scrubbed to; Make Track lays a new, empty drawing on a
/// track above the video for each hold.
struct AnimaticSheet: View {
    let model: CanvasModel
    let clip: UInt64

    @Environment(\.dismiss) private var dismiss
    @State private var scores: [Float] = []
    @State private var read: Double = 0
    @State private var failed: String?
    @AppStorage("animaticSensitivity") private var sensitivity: Double = 0.5
    @AppStorage("animaticShortestHold") private var shortest: Int = 2
    @State private var at = 0
    @State private var picture: UIImage?
    @State private var reading: Task<Void, Never>?

    @State private var refused: String?

    private var holds: [AnimaticHold] {
        animaticHolds(scores: scores, sensitivity: Float(sensitivity), minimumHold: UInt32(shortest))
    }

    /// How many more layers the canvas has room for: each drawing of the track takes one.
    private var layersLeft: Int {
        let stats = model.session.layerStats()
        return Int(stats.maximum) - Int(stats.used)
    }

    var body: some View {
        let holds = holds
        let left = layersLeft
        let fits = holds.count <= left
        VStack(alignment: .leading, spacing: 12) {
            HStack {
                Button("Cancel") { dismiss() }
                Spacer()
                Text(verbatim: "Make Timing Track").font(.system(size: 17, weight: .semibold))
                Spacer()
                Button("Make Track") { make(holds) }
                    .fontWeight(.semibold)
                    .disabled(holds.isEmpty || !fits)
                    // (the header's colour would hide that it is off)
                    .opacity(holds.isEmpty || !fits ? 0.4 : 1)
            }
            .foregroundStyle(Pc.panelTitle)
            .padding(.bottom, 4)
            ZStack {
                RoundedRectangle(cornerRadius: 14, style: .circular).fill(Color.black)
                if let picture {
                    Image(uiImage: picture).resizable().scaledToFit()
                }
            }
            .frame(height: 260)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .circular))
            if scores.isEmpty {
                VStack(alignment: .leading, spacing: 8) {
                    Text(verbatim: failed ?? "Looking for where the drawings change…")
                        .font(.system(size: 15))
                        .foregroundStyle(Pc.slideLabel)
                    if failed == nil {
                        ProgressView(value: read).tint(Pc.accent)
                    }
                }
                .frame(height: 120, alignment: .top)
            } else {
                Text(verbatim: summary(holds))
                    .font(.system(size: 15).monospacedDigit())
                    .foregroundStyle(Pc.slideLabel)
                // each drawing takes a layer: say so before Make Track can't
                if let line = refused ?? (fits ? nil : tooMany(holds.count, left: left)) {
                    Text(verbatim: line)
                        .font(.system(size: 15))
                        .foregroundStyle(.orange)
                        .fixedSize(horizontal: false, vertical: true)
                }
                ChangeGraph(scores: scores, threshold: animaticThreshold(sensitivity: Float(sensitivity)),
                            cuts: holds.dropFirst().map { Int($0.start) }, at: at) { frame in
                    at = frame
                    showPicture()
                }
                .frame(height: 96)
                TlSliderGroup {
                    TlSlider(label: "Sensitivity", value: sensitivity < 0.2 ? "Cuts only" : sensitivity > 0.8 ? "Every change" : "\(Int((sensitivity * 100).rounded()))%",
                             share: Float(sensitivity)) { v in sensitivity = Double(v) }
                    TlSlider(label: "Shortest Hold", value: shortest == 1 ? "1 frame" : "\(shortest) frames",
                             share: Float(shortest - 1) / 23) { v in shortest = 1 + wholeNumber(v * 23) }
                }
            }
            Spacer(minLength: 0)
        }
        .padding(20)
        .background(Pc.popoverFill)
        .presentationDetents([.large])
        .onAppear(perform: start)
        .onChange(of: sensitivity) { refused = nil }
        .onChange(of: shortest) { refused = nil }
        .onDisappear { reading?.cancel() }
    }

    private func summary(_ holds: [AnimaticHold]) -> String {
        let n = holds.count
        let here = holds.lastIndex { Int($0.start) <= at }.map { i in
            "frame \(at + 1) is drawing \(i + 1), held \(holds[i].length)"
        } ?? ""
        return "\(n) \(n == 1 ? "drawing" : "drawings") with the video's timing · \(here)"
    }

    private func start() {
        let session = model.session
        guard let c = session.timelineMediaClip(id: clip), let path = c.path else {
            failed = "The video's file can't be found."
            return
        }
        let times = session.timelineVideoTimes(id: clip)
        showPicture()
        reading = Task.detached(priority: .userInitiated) {
            do {
                let side = Int(animaticThumbnailSide())
                let (w, h, frames) = try await AnimaticFrames.read(path: path, times: times, side: side) { p in
                    Task { @MainActor in read = p }
                }
                let s = animaticScores(frames: frames, width: w, height: h)
                await MainActor.run { scores = s }
            } catch is CancellationError {
            } catch {
                await MainActor.run { failed = (error as? MediaReading.Problem)?.said ?? "That video can't be read." }
            }
        }
    }

    private func showPicture() {
        let session = model.session
        let times = session.timelineVideoTimes(id: clip)
        guard let c = session.timelineMediaClip(id: clip), let path = c.path, at < times.count else { return }
        let time = times[at]
        Task.detached(priority: .userInitiated) {
            let image = VideoFrameReader.shared.picture(path: path, seconds: time, maxSide: 640).flatMap(CanvasModel.image)
            await MainActor.run { picture = image }
        }
    }

    /// Why `count` drawings can't be made with room for `left` more layers, and what to change.
    private func tooMany(_ count: Int, left: Int) -> String {
        let room = left <= 0 ? "no room for more layers" : "room for \(left) more \(left == 1 ? "layer" : "layers")"
        return "\(count) drawings take a layer each, and this canvas has \(room). Lower the sensitivity or lengthen the shortest hold."
    }

    private func make(_ holds: [AnimaticHold]) {
        guard model.session.makeTimingTrack(id: clip, holds: holds) != 0 else {
            // stays open, saying why, rather than closing on nothing
            refused = holds.count > layersLeft ? tooMany(holds.count, left: layersLeft) : "The timing track couldn't be made."
            UINotificationFeedbackGenerator().notificationOccurred(.warning)
            return
        }
        model.refresh()
        dismiss()
    }
}

/// The change scores as bars on a log scale, the threshold across them, a tick at each cut and
/// the frame looked at; dragged or tapped along to look at another.
private struct ChangeGraph: View {
    let scores: [Float]
    let threshold: Float
    let cuts: [Int]
    let at: Int
    let look: (Int) -> Void

    var body: some View {
        GeometryReader { place in
            let w = place.size.width, h = place.size.height
            let n = max(scores.count, 1)
            let bar = w / CGFloat(n)
            Canvas { ctx, _ in
                ctx.fill(Path(roundedRect: CGRect(x: 0, y: 0, width: w, height: h), cornerRadius: 12), with: .color(Pc.actionsGroup))
                for (i, s) in scores.enumerated() {
                    let y = height(s, h)
                    ctx.fill(Path(CGRect(x: CGFloat(i) * bar, y: h - y, width: max(bar - 1, 1), height: y)),
                             with: .color(s >= threshold ? Pc.accent : Pc.slideLabel.opacity(0.4)))
                }
                for c in cuts {
                    ctx.fill(Path(CGRect(x: CGFloat(c) * bar, y: 0, width: 1, height: h)), with: .color(Pc.accent.opacity(0.6)))
                }
                let line = h - height(threshold, h)
                ctx.fill(Path(CGRect(x: 0, y: line, width: w, height: 1.5)), with: .color(Color.white.opacity(0.8)))
                ctx.fill(Path(CGRect(x: (CGFloat(at) + 0.5) * bar - 1, y: 0, width: 2, height: h)), with: .color(.white))
            }
            .contentShape(Rectangle())
            .gesture(DragGesture(minimumDistance: 0).onChanged { drag in
                look(min(max(Int(drag.location.x / bar), 0), n - 1))
            })
        }
    }

    /// A score's bar height: log scale from 0.0002 to 1.
    private func height(_ s: Float, _ h: CGFloat) -> CGFloat {
        let low = log10(Float(0.0002))
        let v = (log10(max(s, 0.0002)) - low) / -low
        return CGFloat(min(max(v, 0), 1)) * (h - 6) + 2
    }
}

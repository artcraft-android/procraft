import AVFoundation
import UIKit

// Debug launch scenes for the timeline's media (`-screen timeline-sound`, `timeline-video`,
// `timeline-animatic`): a sound and an animatic made on the spot, brought in as a file would be.
// `timeline-sound-trim`: the sound twice on one track, six frames apart, the second starting ten
// frames into its file and picked, to trim its start against the first.

enum MediaDebug {
    static let scenes = ["timeline-sound", "timeline-video", "timeline-animatic", "timeline-sound-trim"]

    static func open(_ name: String, in model: CanvasModel) {
        model.timeline.setOn(true)
        Task { @MainActor in
            let video = name == "timeline-video" || name == "timeline-animatic"
            guard let url = video ? animatic() : beeps(),
                  let read = try? await MediaReading.read(url, kind: video ? .video : .audio),
                  let clip = try? model.session.importTimelineMedia(import: read), clip != 0
            else { return }
            model.refresh()
            model.timeline.picked = clip
            switch name {
            case "timeline-video":
                model.timeline.goTo(9)
                model.timeline.picked = clip
            case "timeline-animatic":
                model.media.detecting = clip
            case "timeline-sound-trim":
                guard let again = try? model.session.importTimelineMedia(import: read), again != 0,
                      let track = model.timeline.tracks.first(where: { $0.clips.contains { $0.id == clip } }),
                      let first = track.clips.first(where: { $0.id == clip })
                else { return }
                model.refresh()
                let gap = Int(first.start + first.duration) + 6
                model.timeline.drop(again, on: track.id, start: gap, later: true)
                let fps = Double(max(model.session.timelineInfo()?.fps ?? 12, 1))
                _ = model.session.slipTimelineMedia(id: again, trimStart: 10 / fps, done: true)
                model.refresh()
                model.timeline.picked = again
            default:
                break
            }
        }
    }

    /// Two seconds of beeps getting louder, as a WAV.
    static func beeps() -> URL? {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("Beeps.wav")
        try? FileManager.default.removeItem(at: url)
        let rate = 44100.0
        guard let format = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 1),
              let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(rate * 2)),
              let samples = buffer.floatChannelData?[0],
              let file = try? AVAudioFile(forWriting: url, settings: format.settings)
        else { return nil }
        buffer.frameLength = buffer.frameCapacity
        for i in 0 ..< Int(buffer.frameLength) {
            let t = Double(i) / rate
            let beat = t.truncatingRemainder(dividingBy: 0.25)
            let on = beat < 0.12 ? 1.0 : 0.0
            samples[i] = Float(on * (0.15 + 0.4 * t) * sin(2 * .pi * 440 * t))
        }
        guard (try? file.write(from: buffer)) != nil else { return nil }
        return url
    }

    /// Two seconds at 12 frames a second: four drawings (a coloured ball in four places), held
    /// 6, 4, 8 and 6 frames, as an H.264 movie.
    private static func animatic() -> URL? {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("Animatic.mov")
        try? FileManager.default.removeItem(at: url)
        let (w, h) = (640, 360)
        guard let writer = try? AVAssetWriter(outputURL: url, fileType: .mov) else { return nil }
        let input = AVAssetWriterInput(mediaType: .video, outputSettings: [
            AVVideoCodecKey: AVVideoCodecType.h264, AVVideoWidthKey: w, AVVideoHeightKey: h,
        ])
        let adaptor = AVAssetWriterInputPixelBufferAdaptor(assetWriterInput: input, sourcePixelBufferAttributes: [
            kCVPixelBufferPixelFormatTypeKey as String: kCVPixelFormatType_32BGRA,
            kCVPixelBufferWidthKey as String: w, kCVPixelBufferHeightKey as String: h,
        ])
        writer.add(input)
        guard writer.startWriting() else { return nil }
        writer.startSession(atSourceTime: .zero)
        let holds = [6, 4, 8, 6]
        let balls: [(CGFloat, CGFloat, UIColor)] = [(120, 260, .systemRed), (260, 120, .systemBlue), (400, 250, .systemGreen), (520, 110, .systemOrange)]
        var frame = 0
        for (hold, ball) in zip(holds, balls) {
            for _ in 0 ..< hold {
                while !input.isReadyForMoreMediaData {
                    Thread.sleep(forTimeInterval: 0.005)
                }
                guard let pool = adaptor.pixelBufferPool else { return nil }
                var made: CVPixelBuffer?
                CVPixelBufferPoolCreatePixelBuffer(nil, pool, &made)
                guard let buffer = made else { return nil }
                CVPixelBufferLockBaseAddress(buffer, [])
                if let context = CGContext(
                    data: CVPixelBufferGetBaseAddress(buffer), width: w, height: h, bitsPerComponent: 8,
                    bytesPerRow: CVPixelBufferGetBytesPerRow(buffer), space: CGColorSpaceCreateDeviceRGB(),
                    bitmapInfo: CGImageAlphaInfo.premultipliedFirst.rawValue | CGBitmapInfo.byteOrder32Little.rawValue
                ) {
                    context.setFillColor(UIColor(white: 0.95, alpha: 1).cgColor)
                    context.fill(CGRect(x: 0, y: 0, width: w, height: h))
                    context.setFillColor(ball.2.cgColor)
                    context.fillEllipse(in: CGRect(x: ball.0 - 50, y: ball.1 - 50, width: 100, height: 100))
                }
                CVPixelBufferUnlockBaseAddress(buffer, [])
                adaptor.append(buffer, withPresentationTime: CMTime(value: CMTimeValue(frame), timescale: 12))
                frame += 1
            }
        }
        input.markAsFinished()
        let done = DispatchSemaphore(value: 0)
        writer.finishWriting { done.signal() }
        done.wait()
        return writer.status == .completed ? url : nil
    }
}

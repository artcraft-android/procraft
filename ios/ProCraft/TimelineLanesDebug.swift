import AVFoundation
import UIKit

// Debug launch scenes for the key lanes (`-screen timeline-lanes`, `timeline-lanes-sound`): the
// keyed line and a second drawing after it on its track, scaled over its four frames, the camera
// keyed on frames 2 and 9, and a sound on its own track, its level keyed on frames 0 and 6. The
// line's keys open (its track's lanes showing), or the sound's page (its volume lane showing).

enum TimelineLanesDebug {
    static let scenes = ["timeline-lanes", "timeline-lanes-sound"]

    static func open(_ name: String, in model: CanvasModel) {
        // keys go on the playhead's frame, which the canvas keeps: it is made just after this
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { build(name, in: model) }
    }

    private static func build(_ name: String, in model: CanvasModel) {
        guard let line = ScreenCapture.timelineKeyed(model) else { return }
        let s = model.session
        s.setTimelineFrame(frame: 7)
        let next = s.addTimelineDrawing()
        if next != 0 {
            _ = s.setTimelineClipDuration(id: next, duration: 4, push: true)
            for (f, scale) in [(UInt32(8), Float(1)), (11, 1.6)] {
                s.setTimelineFrame(frame: f)
                _ = s.setTimelineKey(id: next, channel: .scale, values: [scale, scale], easing: .linear, done: true)
            }
        }
        let size = s.info()
        for (f, zoom) in [(UInt32(2), Float(1)), (9, 1.4)] {
            s.setTimelineFrame(frame: f)
            _ = s.setTimelineCameraKey(x: Float(size.width) / 2, y: Float(size.height) / 2, zoom: zoom, rotation: 0, easing: .smooth, done: true)
        }
        s.setTimelineFrame(frame: 3)
        model.refresh()
        Task { @MainActor in
            if let url = tone(),
               let read = try? await MediaReading.read(url, kind: .audio),
               let sound = try? s.importTimelineMedia(import: read), sound != 0 {
                for (f, level) in [(UInt32(0), Float(0.2)), (6, 1)] {
                    s.setTimelineFrame(frame: f)
                    _ = s.setTimelineVolumeKey(id: sound, volume: level, done: true)
                }
                s.setTimelineFrame(frame: 3)
                model.refresh()
                if name == "timeline-lanes-sound" {
                    model.timeline.picked = sound
                    return
                }
            }
            model.timeline.picked = line
        }
    }

    /// A second of a quiet tone, as a WAV.
    private static func tone() -> URL? {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent("Lanes.wav")
        try? FileManager.default.removeItem(at: url)
        let rate = 44100.0
        guard let format = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 1),
              let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(rate)),
              let samples = buffer.floatChannelData?[0],
              let file = try? AVAudioFile(forWriting: url, settings: format.settings)
        else { return nil }
        buffer.frameLength = buffer.frameCapacity
        for i in 0 ..< Int(buffer.frameLength) {
            samples[i] = Float(0.2 * sin(2 * .pi * 330 * Double(i) / rate))
        }
        guard (try? file.write(from: buffer)) != nil else { return nil }
        return url
    }
}

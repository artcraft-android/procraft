import AVFoundation
import UIKit

// Debug launch scene `-screen export-sound`: a track drawn through an alpha matte, a layer that
// multiplies, and two sounds (a WAV started part of a frame late and slipped, and an m4a, which
// Harmony can't read) on the timeline; then a Harmony scene and an MP4 made of it. What was made
// is said, and written beside them in the exports folder as `export-sound.txt`.

enum ExportDebug {
    static let scenes = ["export-sound"]

    static func open(_ name: String, in model: CanvasModel) {
        let s = model.session
        _ = s.setName(name: "Sound check")
        model.addLayer()
        model.addLayer()
        model.timeline.setOn(true)
        let tracks = s.timelineTracks()
        guard tracks.count == 3, let below = tracks[0].clips.first, let above = tracks[1].clips.first,
              let top = tracks[2].clips.first
        else { return }
        // the matte: a broad black stroke; through it, a red layer
        model.selectLayer(below.root)
        s.setBrushSize(slider: 1)
        s.setColor(r: 0, g: 0, b: 0)
        stroke(s) { k in (500 + Float(k) * 30, 900 + Float(k % 6) * 60) }
        s.setColor(r: 0.86, g: 0.16, b: 0.16)
        _ = s.fillLayer(id: above.root)
        _ = s.setTimelineClipMask(id: above.id, mask: .alpha)
        // over them, a blue line that multiplies
        model.selectLayer(top.root)
        s.setColor(r: 0.1, g: 0.3, b: 0.9)
        stroke(s) { k in (300 + Float(k) * 40, 1400 - Float(k) * 20) }
        _ = s.setLayerBlend(id: top.root, blend: 1)
        model.refresh()

        Task { @MainActor in
            guard let wav = tone(hz: 440, seconds: 2, name: "Beeps.wav"),
                  let m4a = aac(of: tone(hz: 660, seconds: 1.5, name: "Music.wav")),
                  let readWav = try? await MediaReading.read(wav, kind: .audio),
                  let readM4a = try? await MediaReading.read(m4a, kind: .audio)
            else {
                model.saying = ("Export sound", "The sounds couldn't be made.")
                return
            }
            s.setTimelineFrame(frame: 0)
            _ = try? s.importTimelineMedia(import: readM4a)
            s.setTimelineFrame(frame: 6)
            if let beeps = try? s.importTimelineMedia(import: readWav), beeps != 0 {
                _ = s.slipTimelineMedia(id: beeps, trimStart: 0.25, done: true)
                _ = s.setTimelineSound(id: beeps, volume: 1, muted: false, offset: 0.02, done: true)
            }
            s.setTimelineFrame(frame: 0)
            let length = s.timelineInfo()?.duration ?? 1
            for clip in [below, above, top] {
                _ = s.setTimelineClipDuration(id: clip.id, duration: length, push: false)
            }
            model.refresh()
            let fps = Float(s.timelineInfo()?.fps ?? 12)
            DispatchQueue.global(qos: .userInitiated).async {
                var said = ["\(length) frames at \(Int(fps)) fps"]
                for sound in s.timelineSounds(frame: 0) {
                    said.append("Sound \(URL(fileURLWithPath: sound.path).lastPathComponent): at \(sound.at) s, from \(sound.from) s, \(sound.length) s")
                }
                let zip = Sharing.exportsDir.appendingPathComponent("Sound check Harmony.zip")
                do {
                    let made = try HarmonyExport.export(s, to: zip)
                    said.append("Harmony: \(made.layers) layers, \(made.drawings) drawings, \(made.frames) frames, \(made.sounds) sounds, \(zip.path)")
                } catch {
                    said.append("Harmony: failed, \(error)")
                }
                if let url = CanvasModel.exportAnimation(s, .mp4, AnimatedChoices(fps: fps)) {
                    let asset = AVURLAsset(url: url)
                    let audio = asset.tracks(withMediaType: .audio)
                    said.append("MP4: \(asset.duration.seconds) s, \(audio.count) audio tracks, audio \(audio.first?.timeRange.duration.seconds ?? 0) s, \(url.path)")
                } else {
                    // made without its sound, or not at all
                    let base = exportFileName(name: s.name(), package: false)
                    let url = Sharing.exportsDir.appendingPathComponent(base + ".mp4")
                    let there = FileManager.default.fileExists(atPath: url.path)
                    said.append("MP4: failed, \(there ? "the picture was made, the sound wasn't added" : "no picture made")")
                }
                let text = said.joined(separator: "\n")
                try? text.write(to: Sharing.exportsDir.appendingPathComponent("export-sound.txt"), atomically: true, encoding: .utf8)
                DispatchQueue.main.async { model.saying = ("Export sound", text) }
            }
        }
    }

    private static func stroke(_ s: CanvasSession, _ point: (Int) -> (Float, Float)) {
        let samples = (0 ... 30).map { k -> PenSample in
            let (x, y) = point(k)
            return PenSample(x: x, y: y, pressure: 1, tilt: 0, orientation: 0, timeMs: Double(k) * 8, roll: 0)
        }
        s.strokeBegin(pointer: .stylus, samples: Array(samples.prefix(1)))
        s.strokeAdd(samples: Array(samples.dropFirst()))
        s.strokeEnd(samples: [])
    }

    /// `seconds` of a tone at `hz`, as a WAV named `name` in the temporary folder.
    private static func tone(hz: Double, seconds: Double, name: String) -> URL? {
        let url = FileManager.default.temporaryDirectory.appendingPathComponent(name)
        try? FileManager.default.removeItem(at: url)
        let rate = 44100.0
        guard let format = AVAudioFormat(standardFormatWithSampleRate: rate, channels: 1),
              let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: AVAudioFrameCount(rate * seconds)),
              let samples = buffer.floatChannelData?[0],
              let file = try? AVAudioFile(forWriting: url, settings: format.settings)
        else { return nil }
        buffer.frameLength = buffer.frameCapacity
        for i in 0 ..< Int(buffer.frameLength) {
            samples[i] = Float(0.3 * sin(2 * .pi * hz * Double(i) / rate))
        }
        guard (try? file.write(from: buffer)) != nil else { return nil }
        return url
    }

    /// The WAV at `wav` as AAC in an m4a beside it.
    private static func aac(of wav: URL?) -> URL? {
        guard let wav, let input = try? AVAudioFile(forReading: wav) else { return nil }
        let url = wav.deletingPathExtension().appendingPathExtension("m4a")
        try? FileManager.default.removeItem(at: url)
        let format = input.processingFormat
        guard let output = try? AVAudioFile(forWriting: url, settings: [
            AVFormatIDKey: kAudioFormatMPEG4AAC,
            AVSampleRateKey: format.sampleRate,
            AVNumberOfChannelsKey: format.channelCount,
        ]), let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 8192)
        else { return nil }
        while input.framePosition < input.length {
            guard (try? input.read(into: buffer)) != nil, buffer.frameLength > 0,
                  (try? output.write(from: buffer)) != nil
            else { break }
        }
        return url
    }
}

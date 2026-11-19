import AVFoundation

// A Harmony scene's sounds: Harmony reads WAV and AIFF, so each other sound the timeline plays
// (an m4a, a caf, an mp3) is copied as WAV first, for the core to put in the scene.

enum HarmonyExport {
    /// `session`'s animation as a Harmony scene zipped at `url`. Slow: off the main thread.
    static func export(_ session: CanvasSession, to url: URL) throws -> HarmonyMade {
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("harmony-sounds-" + UUID().uuidString, isDirectory: true)
        defer { try? FileManager.default.removeItem(at: folder) }
        var wav: [String: String] = [:]
        for path in session.harmonySoundsToConvert() {
            let out = folder.appendingPathComponent("\(wav.count).wav")
            if wavCopy(of: URL(fileURLWithPath: path), to: out) {
                wav[path] = out.path
            }
        }
        return try session.exportHarmony(path: url.path, wav: wav)
    }

    /// The sound at `source` written at `url` as 16-bit WAV, as it is otherwise (its rate and
    /// channels). False if it can't be read.
    static func wavCopy(of source: URL, to url: URL) -> Bool {
        do {
            try FileManager.default.createDirectory(at: url.deletingLastPathComponent(), withIntermediateDirectories: true)
            let input = try AVAudioFile(forReading: source)
            let format = input.processingFormat
            let output = try AVAudioFile(forWriting: url, settings: [
                AVFormatIDKey: kAudioFormatLinearPCM,
                AVSampleRateKey: format.sampleRate,
                AVNumberOfChannelsKey: format.channelCount,
                AVLinearPCMBitDepthKey: 16,
                AVLinearPCMIsFloatKey: false,
                AVLinearPCMIsBigEndianKey: false,
                AVLinearPCMIsNonInterleaved: false,
            ], commonFormat: format.commonFormat, interleaved: format.isInterleaved)
            guard let buffer = AVAudioPCMBuffer(pcmFormat: format, frameCapacity: 8192) else { return false }
            while input.framePosition < input.length {
                try input.read(into: buffer)
                if buffer.frameLength == 0 {
                    break
                }
                try output.write(from: buffer)
            }
            return true
        } catch {
            try? FileManager.default.removeItem(at: url)
            return false
        }
    }
}

import AVFoundation

// Sounds for a plugin's export that takes them (a Harmony scene): it reads WAV and AIFF, so each
// other sound the timeline plays (an m4a, a caf, an mp3) is copied as WAV first, for the core to
// hand over.

enum SoundCopies {
    /// A WAV copy of each sound file at `paths`, in a folder of their own: each path to its
    /// copy, those that couldn't be read left out. Slow: off the main thread.
    static func wavCopies(_ paths: [String]) -> [String: String] {
        guard !paths.isEmpty else { return [:] }
        let folder = FileManager.default.temporaryDirectory
            .appendingPathComponent("sound-copies-" + UUID().uuidString, isDirectory: true)
        var wav: [String: String] = [:]
        for path in paths {
            let out = folder.appendingPathComponent("\(wav.count).wav")
            if wavCopy(of: URL(fileURLWithPath: path), to: out) {
                wav[path] = out.path
            }
        }
        return wav
    }

    /// The copies `wav` made, and their folder, let go.
    static func forget(_ wav: [String: String]) {
        for copy in Set(wav.values.map { URL(fileURLWithPath: $0).deletingLastPathComponent() }) {
            try? FileManager.default.removeItem(at: copy)
        }
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

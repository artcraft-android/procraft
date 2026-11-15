import AVFoundation

// The timeline's sounds in an exported video: laid under the frames once they are written, each
// at its time and level. Only video carries sound; GIF and PNG exports are left as they are.

enum MediaExport {
    /// The sounds of `session`'s timeline mixed under the video at `url` (its frames written at
    /// `fps` a second), in place. True when there was nothing to add or it was added.
    static func addSounds(to url: URL, session: CanvasSession, fps: Float, hevc: Bool) -> Bool {
        let sounds = session.timelineSounds(frame: 0).filter { $0.gain > 0 }
        guard !sounds.isEmpty, let timelineFps = session.timelineInfo()?.fps else { return true }
        // played at another rate than the timeline's, the sounds start where their frames are shown
        let scale = Double(timelineFps) / Double(max(fps, 1))
        let video = AVURLAsset(url: url)
        let composition = AVMutableComposition()
        guard let picture = video.tracks(withMediaType: .video).first,
              let pictures = composition.addMutableTrack(withMediaType: .video, preferredTrackID: kCMPersistentTrackID_Invalid)
        else { return false }
        let length = video.duration
        do {
            try pictures.insertTimeRange(CMTimeRange(start: .zero, duration: length), of: picture, at: .zero)
        } catch {
            return false
        }
        var levels: [AVMutableAudioMixInputParameters] = []
        for s in sounds {
            let file = AVURLAsset(url: URL(fileURLWithPath: s.path))
            let start = s.at * scale
            let end = min(start + s.length, length.seconds)
            guard end > start, let sound = file.tracks(withMediaType: .audio).first,
                  let track = composition.addMutableTrack(withMediaType: .audio, preferredTrackID: kCMPersistentTrackID_Invalid)
            else { continue }
            let part = CMTimeRange(
                start: CMTime(seconds: s.from, preferredTimescale: 48000),
                duration: CMTime(seconds: end - start, preferredTimescale: 48000)
            )
            do {
                try track.insertTimeRange(part, of: sound, at: CMTime(seconds: start, preferredTimescale: 48000))
            } catch {
                continue
            }
            let level = AVMutableAudioMixInputParameters(track: track)
            level.setVolume(s.gain, at: .zero)
            levels.append(level)
        }
        guard !levels.isEmpty else { return true }
        let mix = AVMutableAudioMix()
        mix.inputParameters = levels
        let preset = hevc ? AVAssetExportPresetHEVCHighestQuality : AVAssetExportPresetHighestQuality
        guard let export = AVAssetExportSession(asset: composition, presetName: preset) else { return false }
        let out = url.deletingLastPathComponent().appendingPathComponent("with-sound-" + url.lastPathComponent)
        try? FileManager.default.removeItem(at: out)
        export.outputURL = out
        export.outputFileType = .mp4
        export.audioMix = mix
        let finished = DispatchSemaphore(value: 0)
        export.exportAsynchronously { finished.signal() }
        finished.wait()
        guard export.status == .completed else {
            try? FileManager.default.removeItem(at: out)
            return false
        }
        do {
            _ = try FileManager.default.replaceItemAt(url, withItemAt: out)
        } catch {
            return false
        }
        return true
    }
}

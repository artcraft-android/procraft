import UIKit

// Debug launch scenes for the grid timeline (`-screen timeline-grid`, `timeline-grid-held`,
// `timeline-grid-select`, `timeline-grid-menu`, `timeline-grid-track`): three tracks, the lowest
// with four drawings (the second held for three frames), the top one starting late.
// `timeline-grid-media` brings a sound in too, and the video at the path given as `-gridVideo`.

enum TimelineGridDebug {
    static let scenes = [
        "timeline-grid", "timeline-grid-held", "timeline-grid-select", "timeline-grid-menu", "timeline-grid-track", "timeline-grid-media",
    ]

    static func open(_ name: String, in model: CanvasModel) {
        let s = model.session
        let colours: [[Float]] = [[0.86, 0.16, 0.16], [0.16, 0.6, 0.2], [0.16, 0.3, 0.86], [0.9, 0.6, 0.1]]
        for i in 0 ..< 4 {
            if i > 0 {
                _ = s.addLayer()
            }
            let c = colours[i]
            s.setColor(r: c[0], g: c[1], b: c[2])
            s.setBrushSize(slider: 0.8)
            let y = 500 + Float(i) * 260
            let at = { (k: Int) in PenSample(x: 400 + Float(k) * 40, y: y + Float(k * k) * 0.4, pressure: 1, tilt: 0, orientation: 0, timeMs: Double(k) * 8, roll: 0) }
            s.strokeBegin(pointer: .stylus, samples: [at(0)])
            s.strokeAdd(samples: (1 ... 30).map(at))
            s.strokeEnd(samples: [])
        }
        _ = s.setAnimationAssist(on: true)
        model.timeline.setOn(true)
        guard let first = s.timelineTracks().first else { return }
        // the second drawing held for three frames
        if first.clips.count > 1 {
            _ = s.setTimelineClipDuration(id: first.clips[1].id, duration: 3, push: true)
        }
        _ = s.addTimelineTrack()
        // the top track starts late: nothing on its first frame
        _ = s.addTimelineTrack()
        s.restTimelineFrame(frame: 1)
        _ = s.addTimelineDrawing()
        _ = s.addTimelineDrawing()
        s.selectTimelineTrack(id: first.id)
        s.restTimelineFrame(frame: 1)
        model.refresh()
        let tool = model.timeline
        guard let held = tool.tracks.first(where: { $0.id == first.id })?.clips.dropFirst().first else { return }
        switch name {
        case "timeline-grid-held":
            // the held drawing's handles out
            tool.picked = held.id
        case "timeline-grid-select":
            // the lasso on, the lowest track's first two picked
            tool.startSelecting()
            tool.selecting = Set(tool.tracks.first(where: { $0.id == first.id })?.clips.prefix(2).map(\.id) ?? [])
        case "timeline-grid-menu":
            tool.clipMenu = ClipMenu(clip: held.id, x: 560)
        case "timeline-grid-track":
            tool.trackOpen = first.id
        case "timeline-grid-media":
            Task { @MainActor in
                if let url = MediaDebug.beeps(), let read = try? await MediaReading.read(url, kind: .audio) {
                    _ = try? s.importTimelineMedia(import: read)
                }
                if let path = UserDefaults.standard.string(forKey: "gridVideo"),
                   let read = try? await MediaReading.read(URL(fileURLWithPath: path), kind: .video) {
                    _ = try? s.importTimelineMedia(import: read)
                    MediaReading.forget(read)
                }
                model.refresh()
                tool.picked = nil
            }
        default:
            break
        }
    }
}

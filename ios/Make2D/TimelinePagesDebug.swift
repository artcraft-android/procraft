import UIKit

// Debug launch scenes for the onion skins turned off and on by the quick action, each side kept
// apart (`-screen timeline-onion-off`, `timeline-onion-back`), and a timeline ending before its
// drawings (`timeline-short`).

enum TimelinePagesDebug {
    static let scenes = ["timeline-onion-off", "timeline-onion-back", "timeline-short"]

    static func open(_ name: String, in model: CanvasModel) {
        let s = model.session
        drawings(model, 4)
        let tool = model.timeline
        switch name {
        case "timeline-onion-off", "timeline-onion-back":
            // two before in red, four after in blue; the quick action once (off) or twice (back)
            tool.setSimple(true)
            tool.refresh()
            tool.setOnion {
                $0.before = 2
                $0.after = 4
                $0.beforeColor = 0xE0_3030
                $0.afterColor = 0x30_60E0
            }
            s.setTimelineFrame(frame: 6)
            model.runQuickAction(.onionSkin, at: .zero)
            if name == "timeline-onion-back" {
                model.runQuickAction(.onionSkin, at: .zero)
            }
            tool.setSimple(false)
            model.refresh()
            tool.settingsOpen = true
        default:
            // four drawings three frames each (12 frames of them), the timeline 7 long
            tool.setSimple(false)
            for c in s.timelineTracks().first?.clips ?? [] {
                _ = s.setTimelineClipDuration(id: c.id, duration: 3, push: true)
            }
            _ = s.setTimelineLength(frames: 7)
            s.setTimelineFrame(frame: 2)
            model.refresh()
        }
    }

    /// `n` drawings on a timeline, a stroke of its own colour across each.
    private static func drawings(_ model: CanvasModel, _ n: Int) {
        let s = model.session
        let colours: [[Float]] = [[0.86, 0.16, 0.16], [0.16, 0.6, 0.2], [0.16, 0.3, 0.86], [0.9, 0.6, 0.1]]
        for i in 0 ..< n {
            if i > 0 {
                _ = s.addLayer()
            }
            let c = colours[i % colours.count]
            s.setColor(r: c[0], g: c[1], b: c[2])
            s.setBrushSize(slider: 0.8)
            let y = 600 + Float(i) * 200
            let at = { (k: Int) in PenSample(x: 500 + Float(k) * 40, y: y, pressure: 1, tilt: 0, orientation: 0, timeMs: Double(k) * 8, roll: 0) }
            s.strokeBegin(pointer: .stylus, samples: [at(0)])
            s.strokeAdd(samples: (1 ... 30).map(at))
            s.strokeEnd(samples: [])
        }
        _ = s.setAnimationAssist(on: true)
        model.timeline.setOn(true)
        s.setTimelineFrame(frame: 0)
        model.refresh()
    }
}

import UIKit

// Debug launch scenes for group clips, the layers panel's + on a timeline, pasting into another
// canvas and sounds split and keyed (`-screen timeline-group-path`, `timeline-group-out`,
// `timeline-layers-add`, `timeline-paste-other`, `timeline-sound-keys`).

enum TimelineFixesDebug {
    static let scenes = ["timeline-group-path", "timeline-group-out", "timeline-layers-add", "timeline-paste-other", "timeline-sound-keys"]

    static func open(_ name: String, in model: CanvasModel) {
        let s = model.session
        switch name {
        case "timeline-group-path":
            // the keyed line in a group the group's keys move 400 across: its path goes with it
            guard let clip = ScreenCapture.timelineKeyed(model) else { return }
            model.timeline.setSimple(false)
            let group = s.groupTimelineClips(ids: [clip])
            s.setTimelineFrame(frame: 0)
            _ = s.setTimelineKey(id: group, channel: .position, values: [0, 0], easing: .linear, done: true)
            s.setTimelineFrame(frame: 7)
            _ = s.setTimelineKey(id: group, channel: .position, values: [400, 0], easing: .linear, done: true)
            s.setTimelineFrame(frame: 3)
            model.refresh()
            model.timeline.open(group)
            model.timeline.picked = clip
            model.timeline.goTo(3)
            model.timeline.keysOpen = true
        case "timeline-group-out":
            // four drawings, the first two grouped; the group open on its first drawing's page
            drawings(model, 4)
            model.timeline.setSimple(false)
            guard let track = s.timelineTracks().first(where: { $0.clips.count >= 2 }) else { return }
            let group = s.groupTimelineClips(ids: [track.clips[0].id, track.clips[1].id])
            model.refresh()
            model.timeline.open(group)
            model.timeline.picked = track.clips[0].id
            model.timeline.clipOpen = true
        case "timeline-layers-add":
            // three drawings; + in the layers panel twice on the first
            drawings(model, 3)
            s.setTimelineFrame(frame: 0)
            model.addLayer()
            model.addLayer()
            model.refresh()
            model.toggle(.layers)
        case "timeline-paste-other":
            // the first of three drawings copied, then a new canvas of another size on a timeline,
            // the copy pasted into it
            drawings(model, 3)
            s.setTimelineFrame(frame: 0)
            guard let first = s.timelineTracks().first?.clips.first?.id, s.copyTimelineClips(ids: [first]) else { return }
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
                let size = s.info()
                let other = CanvasSession(width: size.width * 3 / 4, height: size.height)
                let next = CanvasModel(session: other, dir: Gallery.store.newFolder(folder: nil))
                // the canvas shown closed first, so the new one gets a canvas view of its own
                ScreenCapture.opener?(nil)
                DispatchQueue.main.asyncAfter(deadline: .now() + 1) { ScreenCapture.opener?(next) }
                DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
                    next.timeline.setOn(true)
                    next.timeline.setSimple(false)
                    other.setTimelineFrame(frame: 0)
                    next.refresh()
                    next.timeline.pasteClips()
                    next.timeline.clipOpen = true
                }
            }
        case "timeline-sound-keys":
            // a sound faded in by its level keys, split at frame 12: the second piece's page
            MediaDebug.open("timeline-sound", in: model)
            DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
                guard let clip = model.timeline.picked else { return }
                s.setTimelineFrame(frame: 0)
                _ = s.setTimelineVolumeKey(id: clip, volume: 0.1, done: true)
                s.setTimelineFrame(frame: 20)
                _ = s.setTimelineVolumeKey(id: clip, volume: 1, done: true)
                s.setTimelineFrame(frame: 12)
                model.timeline.split(clip)
                model.refresh()
                let track = s.timelineTracks().first { $0.clips.contains { $0.id == clip } }
                model.timeline.picked = track?.clips.last?.id
                s.setTimelineFrame(frame: 16)
                model.refresh()
                model.timeline.clipOpen = true
            }
        default:
            break
        }
    }

    /// `n` drawings on a timeline, a stroke of its own colour across each: `n` layers, then the
    /// timeline turned on with the animation assist so each layer is a drawing.
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

import Foundation
import os

// Debug launch scene for the timeline panel's refresh cost (`-screen timeline-big`): 20 tracks of
// short drawings, each with a mark drawn on it and its position keyed at its first and last frame,
// as many as the canvas has room for (30 a track at most). Once built, the panel is read again
// many times over and the time each read took is logged ("TLPERF"), with how many drawings'
// pictures were drawn again for the panel.

enum TimelineBigDebug {
    static let scenes = ["timeline-big"]

    private static let log = Logger(subsystem: "io.github.artcraftandroid.procraft", category: "TLPERF")

    static func open(_: String, in model: CanvasModel) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { build(model) }
    }

    private static func build(_ model: CanvasModel) {
        let s = model.session
        model.timeline.setOn(true)
        model.timeline.setSimple(false)
        var made = 0
        building: for t in 0 ..< 20 {
            if t > 0 {
                guard s.addTimelineTrack() != 0 else { break }
            }
            for c in 0 ..< 30 {
                s.setTimelineFrame(frame: UInt32(c * 3))
                let clip = s.addTimelineDrawing()
                guard clip != 0 else { break building }
                _ = s.setTimelineClipDuration(id: clip, duration: 3, push: false)
                let x = Float(200 + c * 40), y = Float(200 + t * 60)
                let at = { (k: Int) in PenSample(x: x + Float(k) * 6, y: y, pressure: 0.6, tilt: 0, orientation: 0, timeMs: Double(k) * 8, roll: 0) }
                s.strokeBegin(pointer: .stylus, samples: [at(0)])
                s.strokeAdd(samples: (1 ... 6).map(at))
                s.strokeEnd(samples: [])
                _ = s.setTimelineKey(id: clip, channel: .position, values: [0, 0], easing: .smooth, done: true)
                s.setTimelineFrame(frame: UInt32(c * 3 + 2))
                _ = s.setTimelineKey(id: clip, channel: .position, values: [0, 40], easing: .smooth, done: true)
                made += 1
            }
        }
        s.setTimelineFrame(frame: 0)
        model.refresh()
        log.notice("TLPERF built \(made) clips")
        // the pictures drawn by the first read are let finish before reads are timed
        DispatchQueue.main.asyncAfter(deadline: .now() + 15) { measure(model, clips: made) }
    }

    private static func measure(_ model: CanvasModel, clips: Int) {
        let tool = model.timeline
        let drawnBefore = tool.thumbnailsDrawn
        let runs = 30
        var panel = 0.0, whole = 0.0
        for i in 0 ..< runs {
            // every other read follows a move of the playhead, as scrubbing does
            if i % 2 == 1 {
                model.session.setTimelineFrame(frame: UInt32(i % 20))
            }
            let a = CFAbsoluteTimeGetCurrent()
            tool.refresh()
            let b = CFAbsoluteTimeGetCurrent()
            model.refresh()
            let c = CFAbsoluteTimeGetCurrent()
            panel += b - a
            whole += c - b
        }
        let p = panel / Double(runs) * 1000, w = whole / Double(runs) * 1000
        log.notice("TLPERF \(clips) clips: timeline refresh \(p, format: .fixed(precision: 2)) ms, model refresh \(w, format: .fixed(precision: 2)) ms (mean of \(runs))")
        DispatchQueue.main.asyncAfter(deadline: .now() + 10) {
            log.notice("TLPERF pictures drawn again over \(runs * 2) reads in a row: \(tool.thumbnailsDrawn - drawnBefore)")
            spaced(model, left: 3, drawnBefore: tool.thumbnailsDrawn, timeBefore: tool.picturesTime)
        }
    }

    /// Reads spaced out as edits come, each given time to draw what it would.
    private static func spaced(_ model: CanvasModel, left: Int, drawnBefore: Int, timeBefore: Double) {
        let tool = model.timeline
        guard left > 0 else {
            let ms = (tool.picturesTime - timeBefore) * 1000
            log.notice("TLPERF over 3 spaced reads: \(tool.thumbnailsDrawn - drawnBefore) pictures drawn again, \(ms, format: .fixed(precision: 1)) ms spent drawing them")
            shown(model, left: 10, total: 0)
            return
        }
        model.refresh()
        DispatchQueue.main.asyncAfter(deadline: .now() + 6) { spaced(model, left: left - 1, drawnBefore: drawnBefore, timeBefore: timeBefore) }
    }

    /// Reads with the main thread idle, each timed until the main thread is free again (the
    /// panel's views made again after it included), the playhead moved before every other one.
    private static func shown(_ model: CanvasModel, left: Int, total: Double) {
        let runs = 10
        guard left > 0 else {
            let ms = total / Double(runs) * 1000
            log.notice("TLPERF a read with the panel shown: \(ms, format: .fixed(precision: 2)) ms until the main thread is free (mean of \(runs))")
            drawnOn(model)
            return
        }
        if left % 2 == 0 {
            model.session.setTimelineFrame(frame: UInt32(left))
        }
        let start = CFAbsoluteTimeGetCurrent()
        model.refresh()
        DispatchQueue.main.async {
            DispatchQueue.main.async {
                let took = CFAbsoluteTimeGetCurrent() - start
                DispatchQueue.main.asyncAfter(deadline: .now() + 1.5) { shown(model, left: left - 1, total: total + took) }
            }
        }
    }

    /// The drawing at frame 0 cleared, then the clearing undone: each read should draw that one
    /// picture again, and only it.
    private static func drawnOn(_ model: CanvasModel) {
        let s = model.session, tool = model.timeline
        s.setTimelineFrame(frame: 0)
        model.refresh()
        DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
            let before = tool.thumbnailsDrawn
            s.clearLayer()
            model.refresh()
            DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
                let drawn = tool.thumbnailsDrawn - before
                let undone = s.undo() ?? "nothing"
                model.refresh()
                DispatchQueue.main.asyncAfter(deadline: .now() + 3) {
                    log.notice("TLPERF cleared: \(drawn) picture drawn again; \(undone, privacy: .public) undone: \(tool.thumbnailsDrawn - before - drawn)")
                }
            }
        }
    }
}

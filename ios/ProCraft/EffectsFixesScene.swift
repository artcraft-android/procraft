import Foundation

/// The screens that show adjustment layers and styles merged, a mask painted live, a blur layer
/// kept up with a stroke, a PSD written with its adjustments, and an adjustment clip's keys:
/// `-screen effect-merge`, `effect-merge-style`, `effect-mask-live`, `effect-blur-stroke`,
/// `effect-psd` and `timeline-adjustment`.
enum EffectsFixesScene {
    static let names = ["effect-merge", "effect-merge-style", "effect-mask-live", "effect-blur-stroke", "effect-psd",
                        "timeline-adjustment"]

    static func show(_ name: String, _ model: CanvasModel) {
        let s = model.session
        strokes(model)
        switch name {
        case "effect-merge":
            // the strokes and an adjustment layer turning their hues, merged into one layer
            model.addAdjustmentLayer(.hsb)
            model.adjust { $0.adjustmentHsb(hue: 0.75, saturation: 0.5, brightness: 0.5) }
            model.endAdjustment(commit: true)
            if let base = model.layers.first(where: { !$0.current && $0.adjustment == nil }) {
                model.pickLayer(base)
            }
            model.mergePicked()
            model.toggle(.layers)
        case "effect-merge-style":
            // an outlined, shadowed layer merged down onto the strokes: its styles in its pixels
            _ = s.addLayer()
            model.setColor(hue: 0.33, saturation: 0.8, brightness: 0.8)
            line(s, y: 1300)
            model.refresh()
            if let top = model.layers.first(where: { $0.current }) {
                var effects = model.layerEffects(top)
                if effects.count >= 4 {
                    effects[0].on = true
                    effects[0].size = 10
                    effects[1].on = true
                    model.setLayerEffect(top, 0, effects[0], done: true)
                    model.setLayerEffect(top, 1, effects[1], done: true)
                }
                model.refresh()
                if let top = model.layers.first(where: { $0.current }) {
                    model.mergeDown(top)
                }
            }
            model.toggle(.layers)
        case "effect-mask-live":
            // a grey adjustment layer, its mask being painted black: the colour comes back under
            // the stroke before it ends
            model.addAdjustmentLayer(.hsb)
            model.adjust { $0.adjustmentHsb(hue: 0.5, saturation: 0, brightness: 0.5) }
            model.endAdjustment(commit: true)
            if let a = model.layers.first(where: { $0.current }) {
                model.addMask(a)
            }
            model.setColor(hue: 0, saturation: 0, brightness: 0)
            s.setBrushSize(slider: 1)
            let at = { (i: Int) in
                PenSample(x: 400 + Float(i) * 25, y: 700 + Float(i) * 8, pressure: 1, tilt: 0, orientation: 0,
                          timeMs: Double(i) * 8, roll: 0)
            }
            s.strokeBegin(pointer: .stylus, samples: [at(0)])
            s.strokeAdd(samples: (1 ... 40).map(at))
            model.refresh()
        case "effect-blur-stroke":
            // a blur layer over the strokes, then a stroke painted under it: blurred as it is drawn
            model.addAdjustmentLayer(.gaussianBlur)
            model.adjust { $0.adjustmentAmountBy(step: 0.3) }
            model.endAdjustment(commit: true)
            if let base = model.layers.first(where: { !$0.current && $0.adjustment == nil }) {
                _ = s.selectLayer(id: base.id)
            }
            model.setColor(hue: 0.75, saturation: 0.9, brightness: 0.9)
            line(s, y: 1250)
            model.refresh()
        case "effect-psd":
            // a hue layer (kept as Photoshop's own) and a blur layer (flattened) written to a PSD
            model.addAdjustmentLayer(.hsb)
            model.adjust { $0.adjustmentHsb(hue: 0.75, saturation: 0.5, brightness: 0.5) }
            model.endAdjustment(commit: true)
            model.addAdjustmentLayer(.gaussianBlur)
            model.adjust { $0.adjustmentAmountBy(step: 0.3) }
            model.endAdjustment(commit: true)
            let dir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask).first
            if let path = dir?.appendingPathComponent("effects-check.psd").path {
                let written = (try? s.exportPsd(path: path)) != nil
                model.showNotice(written ? "PSD written: effects-check.psd" : "The PSD wasn't written", warning: !written)
            }
            model.toggle(.layers)
        case "timeline-adjustment":
            // a hue layer as a clip of eight frames, its hue keyed from none to a third of a turn
            model.addAdjustmentLayer(.hsb)
            model.adjust { $0.adjustmentHsb(hue: 0.5, saturation: 0.5, brightness: 0.5) }
            model.endAdjustment(commit: true)
            model.timeline.setOn(true)
            let clips = s.timelineTracks().flatMap(\.clips)
            for c in clips {
                _ = s.setTimelineClipDuration(id: c.id, duration: 8, push: true)
            }
            guard let clip = clips.first(where: { !s.timelineAdjustmentValues(id: $0.id).isEmpty })?.id else { break }
            s.setTimelineFrame(frame: 0)
            _ = s.setTimelineAdjustmentValue(id: clip, index: 0, value: 0, easing: .smooth, done: true)
            s.setTimelineFrame(frame: 7)
            _ = s.setTimelineAdjustmentValue(id: clip, index: 0, value: 2.1, easing: .smooth, done: true)
            s.setTimelineFrame(frame: 4)
            model.refresh()
            model.timeline.picked = clip
            model.timeline.keysOpen = true
        default:
            break
        }
    }

    /// Colourful strokes across the canvas (window points).
    private static func strokes(_ model: CanvasModel) {
        let s = model.session
        s.setBrushSize(slider: 0.5)
        for (k, hue) in [Float(0.0), 0.58, 0.14].enumerated() {
            model.setColor(hue: hue, saturation: 0.8, brightness: 0.9)
            line(s, y: 420 + Float(k) * 260)
        }
        model.refresh()
    }

    /// A wavy stroke across at `y` (window points).
    private static func line(_ s: CanvasSession, y: Float) {
        let at = { (i: Int) in
            PenSample(x: 500 + Float(i) * 40, y: y + 80 * sin(Float(i) / 3), pressure: 0.8, tilt: 0, orientation: 0,
                      timeMs: Double(i) * 8, roll: 0)
        }
        s.strokeBegin(pointer: .stylus, samples: [at(0)])
        s.strokeAdd(samples: (1 ... 40).map(at))
        s.strokeEnd(samples: [])
    }
}

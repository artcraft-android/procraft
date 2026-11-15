import Foundation

/// The screen `-screen layers-vector-merge` opens on: two vector layers, each a box of lines with
/// a fill dropped in it, merged down, the layers panel open on the one vector layer they make.
enum VectorMergeScene {
    static func show(_ model: CanvasModel) {
        let s = model.session
        s.setBrushSize(slider: 0.3)
        box(model, hue: 0.0, from: 500, to: 900, fill: (1, 0.35, 0.3))
        box(model, hue: 0.6, from: 750, to: 1150, fill: (0.3, 0.8, 0.45))
        if let upper = model.layers.first(where: { $0.current }) {
            model.mergeDown(upper)
        }
        model.refresh()
        model.toggle(.layers)
    }

    /// A new vector layer holding a box of four lines (window points) in `hue`, filled inside.
    private static func box(_ model: CanvasModel, hue: Float, from a: Float, to b: Float,
                            fill: (Float, Float, Float)) {
        let s = model.session
        _ = s.addVectorLayer()
        model.setColor(hue: hue, saturation: 0.9, brightness: 0.35)
        let sides: [((Float, Float), (Float, Float))] = [
            ((a - 30, a), (b + 30, a)), ((b, a - 30), (b, b + 30)),
            ((b + 30, b), (a - 30, b)), ((a, b + 30), (a, a - 30)),
        ]
        for (p, q) in sides {
            let at = { (i: Int) in
                let t = Float(i) / 30
                return PenSample(x: p.0 + (q.0 - p.0) * t, y: p.1 + (q.1 - p.1) * t, pressure: 0.8,
                                 tilt: 0, orientation: 0, timeMs: Double(i) * 8, roll: 0)
            }
            s.strokeBegin(pointer: .stylus, samples: [at(0)])
            s.strokeAdd(samples: (1 ..< 30).map(at))
            s.strokeEnd(samples: [at(30)])
        }
        let mid = (a + b) / 2
        _ = s.pourDrop(x: mid, y: mid, r: fill.0, g: fill.1, b: fill.2, threshold: 0.5)
        model.refresh()
    }
}

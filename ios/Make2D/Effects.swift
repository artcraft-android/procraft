import SwiftUI

/// The icons of Pixelate, Dither and Vignette, and of their kinds, worked out rather than listed:
/// drawn as the other adjustments' are, in one colour.
extension PcIcons {
    /// A square of four cells, two of them filled.
    static let adjustPixelate = PcIcon(name: "adjust-pixelate", width: 32, height: 32, parts: [
        PcIconPart(d: "M9.5 9.5h13v13h-13z", stroke: 1.6, fill: false, evenOdd: false),
        PcIconPart(d: "M9.5 9.5h6.5v6.5h-6.5zM16 16h6.5v6.5h-6.5z", stroke: 0, fill: true, evenOdd: false),
    ])

    /// Dots thinning out across a square, as a dither's do from dark to light.
    static let adjustDither = PcIcon(name: "adjust-dither", width: 32, height: 32, parts: [
        PcIconPart(d: "M9.5 9.5h13v13h-13z", stroke: 1.6, fill: false, evenOdd: false),
        PcIconPart(d: ditherDots(from: 11.2, step: 2.6, size: 1.4, count: 4, thinning: true), stroke: 0, fill: true, evenOdd: false),
    ])

    /// A frame darkening round a clear oval middle.
    static let adjustVignette = PcIcon(name: "adjust-vignette", width: 32, height: 32, parts: [
        PcIconPart(d: "M8.5 10h15v12h-15zM10.8 16a5.2 3.6 0 1 0 10.4 0a5.2 3.6 0 1 0 -10.4 0z", stroke: 0, fill: true, evenOdd: true),
    ])

    /// A dither's kinds: dots on an ordered grid, and dots strewn as noise.
    static let ditherOrdered = PcIcon(name: "dither-ordered", width: 50, height: 50, parts: [
        PcIconPart(d: ditherDots(from: 16, step: 5, size: 3, count: 4, thinning: false), stroke: 0, fill: true, evenOdd: false),
    ])
    static let ditherNoise = PcIcon(name: "dither-noise", width: 50, height: 50, parts: [
        PcIconPart(d: strewnDots(), stroke: 0, fill: true, evenOdd: false),
    ])

    /// A vignette's kinds: edges darkened round a clear middle, or lightened.
    static let vignetteDark = PcIcon(name: "vignette-dark", width: 50, height: 50, parts: [
        PcIconPart(d: "M14 16h22v18h-22zM17.5 25a7.5 5.5 0 1 0 15 0a7.5 5.5 0 1 0 -15 0z", stroke: 0, fill: true, evenOdd: true),
    ])
    static let vignetteLight = PcIcon(name: "vignette-light", width: 50, height: 50, parts: [
        PcIconPart(d: "M14 16h22v18h-22z", stroke: 1.6, fill: false, evenOdd: false),
        PcIconPart(d: "M17.5 25a7.5 5.5 0 1 0 15 0a7.5 5.5 0 1 0 -15 0z", stroke: 1.6, fill: false, evenOdd: false),
    ])

    /// A `count` x `count` grid of square dots `size` across, `step` apart from (`from`, `from`),
    /// every other one left out (a checker); `thinning`, the first column whole and the last
    /// sparser, as a dither goes from dark to light.
    private static func ditherDots(from: Double, step: Double, size: Double, count: Int, thinning: Bool) -> String {
        var d = ""
        for row in 0 ..< count {
            for col in 0 ..< count {
                let checker = (row + col) % 2 == 0
                let shown = thinning ? col == 0 || (checker && (col < count - 1 || row % 3 == 0)) : checker
                guard shown else { continue }
                let x = from + Double(col) * step
                let y = from + Double(row) * step
                d += "M\(x) \(y)h\(size)v\(size)h-\(size)z"
            }
        }
        return d
    }

    /// Dots strewn over a square, the same each time.
    private static func strewnDots() -> String {
        let at: [(Double, Double)] = [
            (16, 17), (22, 15.5), (29, 18), (33, 15), (18.5, 23), (25, 22), (31.5, 24.5),
            (15.5, 29), (21.5, 30.5), (27, 28.5), (33, 31), (19, 34), (25.5, 34.5), (30, 33.5),
        ]
        return at.map { "M\($0.0) \($0.1)h2.6v2.6h-2.6z" }.joined()
    }
}

/// A layer's styles (outline, drop shadow, outer and inner glow), as the core keeps them.
extension CanvasModel {
    /// Layer `l`'s four styles, in [`layerEffectNames`]'s order; none for a layer that takes none.
    func layerEffects(_ l: LayerInfo) -> [LayerEffectInfo] {
        session.layerEffects(id: l.id)
    }

    /// Style `index` of layer `l` set to `effect`: shown at once, one undo step when `done`.
    func setLayerEffect(_ l: LayerInfo, _ index: Int, _ effect: LayerEffectInfo, done: Bool) {
        _ = session.setLayerEffect(id: l.id, index: UInt32(index), effect: effect, done: done)
        refresh()
    }

    /// The paint colour, as a style's colour (sRGB 0..1).
    func paintColourParts() -> (Float, Float, Float) {
        let rgb = colorRgb()
        return (Float((rgb >> 16) & 0xFF) / 255, Float((rgb >> 8) & 0xFF) / 255, Float(rgb & 0xFF) / 255)
    }
}

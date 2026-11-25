import SwiftUI

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

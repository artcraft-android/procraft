//! A layer's styles, the same in every app: its outline, drop shadow, outer glow and inner glow,
//! each one's values as its panel shows them, and how a change to them is kept (shown at once, one
//! undo step when let go). The canvas draws them as it composites (see the GPU crate).

use make2d_engine::document::{LayerEffect, LayerId, LayerStyle, STYLE_MOST_SIZE};

use crate::CanvasSession;

/// One of a layer's styles as its panel shows it: on or off, its colour (sRGB 0..1), opacity
/// (0..1), size (canvas pixels), softness (0..1); a drop shadow's distance (canvas pixels) and
/// angle (degrees, 0 to the right, 90 down).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct LayerEffectInfo {
    pub on: bool,
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub opacity: f32,
    pub size: f32,
    pub softness: f32,
    pub distance: f32,
    pub angle: f32,
}

impl From<LayerEffect> for LayerEffectInfo {
    fn from(e: LayerEffect) -> LayerEffectInfo {
        let [r, g, b] = e.color;
        LayerEffectInfo {
            on: e.on,
            r,
            g,
            b,
            opacity: e.opacity,
            size: e.size,
            softness: e.softness,
            distance: e.distance,
            angle: e.angle,
        }
    }
}

impl From<LayerEffectInfo> for LayerEffect {
    fn from(e: LayerEffectInfo) -> LayerEffect {
        LayerEffect {
            on: e.on,
            color: [e.r, e.g, e.b],
            opacity: e.opacity,
            size: e.size,
            softness: e.softness,
            distance: e.distance,
            angle: e.angle,
        }
    }
}

/// The styles' names, in the order [`CanvasSession::layer_effects`] gives them.
#[uniffi::export]
pub fn layer_effect_names() -> Vec<String> {
    ["Outline", "Drop Shadow", "Outer Glow", "Inner Glow"].map(str::to_owned).to_vec()
}

/// A size or distance slider (0..1) in canvas pixels: finer low down, up to the most a style
/// reaches.
#[uniffi::export]
pub fn style_size(slider: f32) -> f32 {
    let t = if slider.is_finite() { slider.clamp(0.0, 1.0) } else { 0.0 };
    STYLE_MOST_SIZE * t * t
}

/// Where a size or distance (canvas pixels) sits on its slider (0..1).
#[uniffi::export]
pub fn style_size_slider(size: f32) -> f32 {
    if size.is_finite() { (size / STYLE_MOST_SIZE).clamp(0.0, 1.0).sqrt() } else { 0.0 }
}

#[uniffi::export]
impl CanvasSession {
    /// Layer `id`'s four styles (see [`layer_effect_names`]): as set, or as each starts (off) when
    /// it has none yet. None for a layer that takes none (a group, a mask, an adjustment layer).
    pub fn layer_effects(&self, id: u64) -> Vec<LayerEffectInfo> {
        let st = self.shared.lock();
        match st.doc.layer(LayerId(id)) {
            Some(l) if l.kind == make2d_engine::document::LayerKind::Paint && !l.is_adjustment() => {
                l.style.unwrap_or_default().effects().map(LayerEffectInfo::from).to_vec()
            }
            _ => Vec::new(),
        }
    }

    /// Style `index` (see [`layer_effect_names`]) of layer `id` set to `effect`: shown at once,
    /// kept as one undo step when `done` (a slider let go). False when nothing changed or the layer
    /// takes none.
    pub fn set_layer_effect(&self, id: u64, index: u32, effect: LayerEffectInfo, done: bool) -> bool {
        let name = if effect.on { "Layer Style" } else { "Layer Style off" };
        self.change_live(name, done, |d| {
            let Some(layer) = d.layer(LayerId(id)) else { return false };
            let mut style = layer.style.unwrap_or_default();
            let Some(slot) = style.effect_mut(index as usize) else { return false };
            *slot = effect.into();
            // none at all on: as a layer that never had any
            let style = Some(style).filter(|s: &LayerStyle| s.effects().iter().any(|e| e.on));
            d.set_style(LayerId(id), style)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_slider_is_finer_low_down_and_goes_both_ways() {
        assert_eq!(style_size(0.0), 0.0);
        assert_eq!(style_size(1.0), STYLE_MOST_SIZE);
        assert!(style_size(0.5) < STYLE_MOST_SIZE / 2.0);
        for px in [0.0, 3.0, 25.0, 100.0] {
            assert!((style_size(style_size_slider(px)) - px).abs() < 1e-3, "{px}");
        }
        assert_eq!(style_size(f32::NAN), 0.0);
    }

    #[test]
    fn an_effect_goes_to_the_panel_and_back_as_it_was() {
        let e = LayerStyle::default().shadow;
        assert_eq!(LayerEffect::from(LayerEffectInfo::from(e)), e);
        assert_eq!(layer_effect_names().len(), LayerStyle::default().effects().len());
    }
}

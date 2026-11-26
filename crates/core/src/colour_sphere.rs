//! The Disc's shaded centre. Drawing and colour sampling use the same lighting.

use std::sync::{Arc, OnceLock};

use parking_lot::Mutex;

use crate::ColourModel;
use crate::pickers::{byte, hsv_to_rgb, of_hex, rgb_to_hsv, to_encoded, to_linear};

const SIDE: usize = 256;
const LIGHT_DIRECTION: [f32; 3] = [0.41, -0.52, 0.749];
const SHADOW_TINT: [f32; 3] = [0.008, 0.006, 0.013];
const HIGHLIGHT_TINT: [f32; 3] = [0.84, 0.65, 0.52];
const SNAP_DURATION_MS: u32 = 200;

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SphereSample {
    /// From the centre in sphere radii, with y increasing down.
    pub x: f32,
    pub y: f32,
    pub rgb: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct SphereSnap {
    pub from_x: f32,
    pub from_y: f32,
    pub target: SphereSample,
    pub duration_ms: u32,
    pub revision: u32,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, uniffi::Enum)]
pub enum SphereTone {
    Shadow,
    Midtone,
    Highlight,
}

struct Selection {
    model: ColourModel,
    hue: f32,
    base_swatch: Option<u32>,
    sample: Option<(ColourModel, SphereSample)>,
    custom_enabled: bool,
    custom: Option<CustomLighting>,
    revision: u32,
}

impl Selection {
    fn hue_for(&self, model: ColourModel) -> f32 {
        if self.model == model { self.hue } else { model.of_rgb(self.model.pure(self.hue), self.hue)[0] }
    }

    fn active_custom(&self) -> Option<&CustomLighting> {
        self.custom.as_ref().filter(|_| self.custom_enabled)
    }

    fn base(&self, model: ColourModel) -> [f32; 3] {
        of_hex(self.base_swatch.unwrap_or_else(|| crate::colour_model_pure(model, self.hue_for(model))))
    }

    fn paint(&self, model: ColourModel) -> SpherePaint {
        match self.active_custom() {
            Some(custom) => {
                SpherePaint::new(Lighting::Custom, custom.colours.map(|hsv| hsv_to_rgb(rotated(hsv, custom.rotation))))
            }
            None => SpherePaint::new(Lighting::SingleHue, single_hue_colours(self.base(model))),
        }
    }

    fn changed(&mut self) {
        self.sample = None;
        self.revision = self.revision.wrapping_add(1);
    }

    fn sample(&mut self, model: ColourModel, x: f32, y: f32) -> Option<SphereSample> {
        let distance = x.hypot(y);
        if !distance.is_finite() {
            return None;
        }
        let (x, y) = (x / distance.max(1.0), y / distance.max(1.0));
        let sample = sample_at(self.paint(model), x, y);
        self.sample = Some((model, sample));
        Some(sample)
    }
}

#[derive(uniffi::Object)]
pub struct ColourSphere {
    selection: Mutex<Selection>,
}

#[uniffi::export]
impl ColourSphere {
    #[uniffi::constructor]
    pub fn new(model: ColourModel, hue: f32) -> Arc<Self> {
        Arc::new(Self {
            selection: Mutex::new(Selection {
                model,
                hue,
                base_swatch: None,
                sample: None,
                custom_enabled: false,
                custom: None,
                revision: 0,
            }),
        })
    }

    pub fn hue(&self, model: ColourModel) -> f32 {
        let selected = self.selection.lock();
        match selected.active_custom() {
            Some(custom) => model.ring_hue((custom.ring_start + custom.rotation).rem_euclid(1.0)),
            None => selected.hue_for(model),
        }
    }

    pub fn set_hue(&self, model: ColourModel, hue: f32) {
        let mut selected = self.selection.lock();
        if !selected.custom_enabled
            && hue.is_finite()
            && (selected.model != model || selected.hue != hue || selected.base_swatch.is_some())
        {
            selected.model = model;
            selected.hue = hue.rem_euclid(1.0);
            selected.base_swatch = None;
            selected.changed();
        }
    }

    /// Pick a palette swatch as the sphere's base and, in Custom, its new midtone.
    pub fn set_base_swatch(&self, model: ColourModel, rgb: u32) {
        let mut selected = self.selection.lock();
        let hue = model.of_rgb(of_hex(rgb), selected.hue_for(model))[0];
        selected.model = model;
        selected.hue = hue;
        selected.base_swatch = Some(rgb);
        selected.changed();
        if selected.custom_enabled {
            selected.custom = Some(CustomLighting::from_base(model, hue, selected.base(model)));
            let [_, middle, _] = Lighting::Custom.points();
            selected.sample(model, middle.x, middle.y);
        }
    }

    pub fn custom_enabled(&self) -> bool {
        self.selection.lock().custom_enabled
    }

    pub fn set_custom_enabled(&self, model: ColourModel, enabled: bool) {
        let mut selected = self.selection.lock();
        if enabled == selected.custom_enabled {
            return;
        }
        if enabled && selected.custom.is_none() {
            let hue = selected.hue_for(model);
            selected.custom = Some(CustomLighting::from_base(model, hue, selected.base(model)));
        }
        selected.custom_enabled = enabled;
        selected.changed();
    }

    pub fn tone_rgb(&self, tone: SphereTone) -> Option<u32> {
        let selected = self.selection.lock();
        let custom = selected.active_custom()?;
        let hsv = custom.colours.get(tone as usize)?;
        let [r, g, b] = hsv_to_rgb(rotated(*hsv, custom.rotation));
        Some(byte(r) << 16 | byte(g) << 8 | byte(b))
    }

    pub fn set_tone_rgb(&self, tone: SphereTone, rgb: u32) {
        let mut selected = self.selection.lock();
        if selected.custom_enabled
            && let Some(custom) = selected.custom.as_mut()
            && let Some(hsv) = custom.colours.get_mut(tone as usize)
        {
            *hsv = rotated(rgb_to_hsv(of_hex(rgb)), -custom.rotation);
            selected.changed();
        }
    }

    /// Turn all three custom hues by the same fraction of a revolution.
    pub fn rotate_hues(&self, turns: f32) {
        let mut selected = self.selection.lock();
        if selected.custom_enabled
            && turns.is_finite()
            && turns != 0.0
            && let Some(custom) = selected.custom.as_mut()
        {
            custom.rotation = (custom.rotation + turns.rem_euclid(1.0)).rem_euclid(1.0);
            selected.changed();
        }
    }

    pub fn picture_revision(&self) -> u32 {
        self.selection.lock().revision
    }

    /// A drag started in the centre keeps sampling up to the edge.
    pub fn sample(&self, model: ColourModel, x: f32, y: f32) -> Option<SphereSample> {
        self.selection.lock().sample(model, x, y)
    }

    /// The closest tonal landmark to a double tap anywhere on the sphere, in sphere radii.
    pub fn snap(&self, model: ColourModel, x: f32, y: f32) -> Option<SphereSnap> {
        if !x.is_finite() || !y.is_finite() || x.hypot(y) > 1.0 {
            return None;
        }
        let (paint, revision) = {
            let selected = self.selection.lock();
            (selected.paint(model), selected.revision)
        };
        let (_, to) = paint
            .lighting
            .points()
            .map(|point| ((x - point.x).hypot(y - point.y), point))
            .into_iter()
            .min_by(|a, b| a.0.total_cmp(&b.0))?;
        Some(SphereSnap {
            from_x: x,
            from_y: y,
            target: sample_at(paint, to.x, to.y),
            duration_ms: SNAP_DURATION_MS,
            revision,
        })
    }

    /// Move through the sampled colours along the glide and land on the exact target.
    pub fn sample_snap(&self, model: ColourModel, snap: SphereSnap, elapsed_ms: f32) -> Option<SphereSample> {
        let mut selected = self.selection.lock();
        if snap.revision != selected.revision {
            return None;
        }
        if elapsed_ms >= snap.duration_ms as f32 {
            return selected.sample(model, snap.target.x, snap.target.y);
        }
        let t = (elapsed_ms / (snap.duration_ms.max(1) as f32)).clamp(0.0, 1.0);
        let eased = t * t * (3.0 - 2.0 * t);
        let x = snap.from_x + (snap.target.x - snap.from_x) * eased;
        let y = snap.from_y + (snap.target.y - snap.from_y) * eased;
        selected.sample(model, x, y)
    }

    pub fn marker(&self, model: ColourModel) -> Option<SphereSample> {
        self.selection.lock().sample.filter(|(at, _)| *at == model).map(|(_, sample)| sample)
    }

    pub fn image_side(&self) -> u32 {
        SIDE as u32
    }

    /// Premultiplied RGBA for the sphere alone. The interface draws the same ring as Disc.
    pub fn picture(&self, model: ColourModel) -> Vec<u8> {
        let paint = self.selection.lock().paint(model);
        geometry()
            .iter()
            .flat_map(|(light, alpha)| {
                if *alpha == 0 {
                    return [0; 4];
                }
                let [r, g, b] = paint.shade(*light);
                let mul = |c| ((u16::from(c) * u16::from(*alpha) + 127) / 255) as u8;
                [mul(r), mul(g), mul(b), *alpha]
            })
            .collect()
    }
}

fn sample_at(paint: SpherePaint, x: f32, y: f32) -> SphereSample {
    let [r, g, b] = paint.shade(Light::at(x, y));
    SphereSample { x, y, rgb: u32::from_be_bytes([0, r, g, b]) }
}

struct CustomLighting {
    colours: [[f32; 3]; 3],
    ring_start: f32,
    rotation: f32,
}

impl CustomLighting {
    fn from_base(model: ColourModel, hue: f32, base: [f32; 3]) -> Self {
        let colours = single_hue_colours(base).map(rgb_to_hsv);
        Self { colours, ring_start: model.ring_turn(hue), rotation: 0.0 }
    }
}

fn single_hue_colours(base: [f32; 3]) -> [[f32; 3]; 3] {
    let [dark, _, light] = Lighting::SingleHue.points();
    let albedo = base.map(to_linear);
    let shadow = Light::at(dark.x, dark.y).linear_rgb(albedo).map(to_encoded);
    let highlight = Light::at(light.x, light.y).linear_rgb(albedo).map(to_encoded);
    let luma = |rgb: [f32; 3]| crate::colour_models::luma(rgb.map(to_linear));
    // An exact base can itself be black, white, or a fully luminous yellow.
    let [r, g, b] = base;
    let shadow =
        if luma(shadow) > luma(base) { [shadow[0].min(r), shadow[1].min(g), shadow[2].min(b)] } else { shadow };
    let highlight = if luma(highlight) < luma(base) {
        [highlight[0].max(r), highlight[1].max(g), highlight[2].max(b)]
    } else {
        highlight
    };
    [shadow, base, highlight]
}

fn rotated([h, s, v]: [f32; 3], turns: f32) -> [f32; 3] {
    [(h + turns).rem_euclid(1.0), s, v]
}

#[derive(Clone, Copy)]
struct TonePoint {
    x: f32,
    y: f32,
    value: f32,
}

#[derive(Clone, Copy)]
enum Lighting {
    SingleHue,
    Custom,
}

impl Lighting {
    fn points(self) -> [TonePoint; 3] {
        match self {
            Self::SingleHue => {
                static POINTS: OnceLock<[TonePoint; 3]> = OnceLock::new();
                *POINTS.get_or_init(|| tonal_points(self))
            }
            Self::Custom => {
                static POINTS: OnceLock<[TonePoint; 3]> = OnceLock::new();
                *POINTS.get_or_init(|| tonal_points(self))
            }
        }
    }

    fn level(self, light: Light) -> f32 {
        match self {
            Self::SingleHue => to_encoded(crate::colour_models::luma(light.linear_rgb([0.18; 3]))),
            Self::Custom => light.custom_value(),
        }
    }
}

#[derive(Clone, Copy)]
struct SpherePaint {
    lighting: Lighting,
    curve: ToneCurve,
}

impl SpherePaint {
    fn new(lighting: Lighting, colours: [[f32; 3]; 3]) -> Self {
        let levels = lighting.points().map(|point| point.value);
        let bias = match lighting {
            Lighting::SingleHue => 0.0,
            Lighting::Custom => 0.48,
        };
        Self { lighting, curve: ToneCurve::new(levels, colours, bias) }
    }

    fn shade(self, light: Light) -> [u8; 3] {
        self.curve.rgb(self.lighting.level(light)).map(|c| byte(c) as u8)
    }
}

#[derive(Clone, Copy)]
struct ToneCurve {
    levels: [f32; 3],
    colours: [[f32; 3]; 3],
    mid_slope: [f32; 3],
    bias: f32,
}

impl ToneCurve {
    fn new(levels: [f32; 3], colours: [[f32; 3]; 3], bias: f32) -> Self {
        let [low, mid, high] = levels;
        let (left_width, right_width) = ((mid - low).max(f32::EPSILON), (high - mid).max(f32::EPSILON));
        let [shadow, midtone, highlight] = colours;
        let slope = |shadow, midtone, highlight| {
            let left = (midtone - shadow) / left_width;
            let right = (highlight - midtone) / right_width;
            // A shared, monotone tangent avoids a seam at the middle swatch or channel overshoot.
            if left * right <= 0.0 {
                0.0
            } else {
                let (a, b) = (2.0 * right_width + left_width, 2.0 * left_width + right_width);
                (a + b) / (a / left + b / right)
            }
        };
        let mid_slope = [
            slope(shadow[0], midtone[0], highlight[0]),
            slope(shadow[1], midtone[1], highlight[1]),
            slope(shadow[2], midtone[2], highlight[2]),
        ];
        Self { levels, colours, mid_slope, bias }
    }

    fn rgb(self, value: f32) -> [f32; 3] {
        let [low, mid, high] = self.levels;
        let [shadow, midtone, highlight] = self.colours;
        let shaded = value < mid;
        let (from, to, start, end) = if shaded { (shadow, midtone, low, mid) } else { (midtone, highlight, mid, high) };
        let width = (end - start).max(f32::EPSILON);
        let t = ((value - start) / width).clamp(0.0, 1.0);
        // Keep more of the face in the middle tone while retaining the exact three swatches.
        let distance = if shaded { 1.0 - t } else { t };
        let distance = distance * (1.0 - self.bias * (1.0 - distance));
        let t = if shaded { 1.0 - distance } else { distance };
        let mix = t * t * (3.0 - 2.0 * t);
        let tangent = if shaded { t * t * (t - 1.0) } else { t * (1.0 - t).powi(2) } * width;
        let blend = |from, to, slope| (1.0 - mix) * from + mix * to + tangent * slope;
        let [r, g, b] = self.mid_slope;
        [blend(from[0], to[0], r), blend(from[1], to[1], g), blend(from[2], to[2], b)]
    }
}

fn tonal_points(lighting: Lighting) -> [TonePoint; 3] {
    let [lx, ly, _] = LIGHT_DIRECTION;
    let length = lx.hypot(ly);
    let (dx, dy) = (lx / length, ly / length);
    // Follow the light axis. Its cross section contains the peaks of the light and the shadow.
    let profile: [TonePoint; 513] = std::array::from_fn(|i| {
        let r = i as f32 / 256.0 - 1.0;
        let (x, y) = (dx * r, dy * r);
        TonePoint { x, y, value: lighting.level(Light::at(x, y)) }
    });
    let compare = |a: &&TonePoint, b: &&TonePoint| a.value.total_cmp(&b.value);
    let dark = profile.iter().min_by(compare).copied().unwrap_or(profile[0]);
    let bright = profile.iter().max_by(compare).copied().unwrap_or(profile[0]);
    let middle = TonePoint { x: 0.0, y: 0.0, value: lighting.level(Light::at(0.0, 0.0)) };
    [dark, middle, bright]
}

#[derive(Clone, Copy)]
struct Light {
    cosine: f32,
    diffuse: f32,
    reflected: f32,
    highlight: f32,
}

impl Light {
    fn at(x: f32, y: f32) -> Self {
        // On the visible unit hemisphere, the surface point is also its normal.
        let z = (1.0 - x * x - y * y).max(0.0).sqrt();
        let [lx, ly, lz] = LIGHT_DIRECTION;
        let cosine = x * lx + y * ly + z * lz;
        let t = ((cosine - 0.03) / 0.95).clamp(0.0, 1.0);
        let diffuse = t * t * (3.0 - 2.0 * t);
        let reflected = 0.66 * (1.0 - z).powi(2) * (1.0 - diffuse);
        // Spread the soft highlight farther toward the lit rim than toward the centre.
        let (dx, dy) = (x - lx, y - ly);
        let toward_rim = (dx * lx + dy * ly).max(0.0);
        let distance_squared = dx * dx + dy * dy - 0.80 * toward_rim.powi(2);
        let highlight = 0.84 * (1.0 - distance_squared).max(0.0).powi(6);
        Self { cosine, diffuse, reflected, highlight }
    }

    /// A broad fill, a soft key light, and a lift from reflected light at the shadow rim.
    fn custom_value(self) -> f32 {
        let fill = ((self.cosine + 0.14) / 1.14).clamp(0.0, 1.0);
        let key = self.highlight / 0.84;
        0.62 * fill * (0.8 + 0.2 * fill) + 0.38 * key * key + 0.33 * self.reflected
    }

    fn linear_rgb(self, base: [f32; 3]) -> [f32; 3] {
        let shade_channel = |base, shadow_tint, highlight_tint| {
            let shadow = (shadow_tint + 0.041 * base) * (1.0 + self.reflected);
            let surface = shadow + self.diffuse * (0.56 * base + 0.008);
            surface + (highlight_tint - surface) * self.highlight
        };
        [
            shade_channel(base[0], SHADOW_TINT[0], HIGHLIGHT_TINT[0]),
            shade_channel(base[1], SHADOW_TINT[1], HIGHLIGHT_TINT[1]),
            shade_channel(base[2], SHADOW_TINT[2], HIGHLIGHT_TINT[2]),
        ]
    }
}

fn geometry() -> &'static [(Light, u8)] {
    static GEOMETRY: OnceLock<Vec<(Light, u8)>> = OnceLock::new();
    GEOMETRY.get_or_init(|| {
        let coord = |p| (p as f32 + 0.5) * 2.0 / SIDE as f32 - 1.0;
        (0..SIDE * SIDE)
            .map(|i| {
                let (x, y) = (coord(i % SIDE), coord(i / SIDE));
                let radius = x.hypot(y);
                let alpha = byte((1.0 - radius) * (SIDE as f32 * 0.5) + 0.5) as u8;
                (Light::at(x / radius.max(1.0), y / radius.max(1.0)), alpha)
            })
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_match_the_preview_and_the_ring_keeps_its_base_hue() {
        let model = ColourModel::Hsb;
        let sphere = ColourSphere::new(model, 0.6);
        let picture = sphere.picture(model);
        let coord = |p| (p as f32 + 0.5) * 2.0 / SIDE as f32 - 1.0;
        for (px, py) in [(66, 78), (128, 128), (197, 170)] {
            let at = (py * SIDE + px) * 4;
            let rgb = u32::from_be_bytes([0, picture[at], picture[at + 1], picture[at + 2]]);
            let picked = sphere.sample(model, coord(px), coord(py)).unwrap();
            assert_eq!(picked.rgb, rgb);
            assert_eq!(sphere.marker(model), Some(picked));
        }
        assert_eq!(sphere.hue(model), 0.6);
        sphere.set_hue(model, 0.1);
        assert!(sphere.marker(model).is_none());
        assert_ne!(sphere.picture(model), picture);
        assert!(sphere.sample(model, f32::NAN, 0.0).is_none());
    }

    #[test]
    fn the_sphere_has_a_top_right_highlight_and_a_deep_shadow() {
        let model = ColourModel::Hsb;
        let sphere = ColourSphere::new(model, 0.6);
        let lit = sphere.sample(model, 0.41, -0.52).unwrap().rgb;
        let shoulder = sphere.sample(model, 0.12, -0.38).unwrap().rgb;
        let opposite = sphere.sample(model, -0.38, -0.47).unwrap().rgb;
        let lit_rim = sphere.sample(model, 0.59, -0.76).unwrap().rgb;
        let shadow = sphere.sample(model, -0.64, 0.64).unwrap().rgb;
        let rim = sphere.sample(model, -0.70, 0.70).unwrap().rgb;
        assert!(lit >> 16 > shoulder >> 16 && shoulder >> 16 > opposite >> 16);
        assert!((lit_rim >> 8) & 0xff > 140);
        assert!(shadow & 0xff < 80);
        assert!(rim & 0xff > shadow & 0xff);
        let edge = sphere.sample(model, 2.0, 0.0).unwrap();
        assert_eq!((edge.x, edge.y), (1.0, 0.0));
    }

    #[test]
    fn snaps_land_on_the_rendered_highlight_midtone_and_shadow() {
        let model = ColourModel::Hsb;
        let sphere = ColourSphere::new(model, 0.6);
        let bright = sphere.snap(model, 0.35, -0.46).unwrap();
        let middle = sphere.snap(model, 0.02, -0.07).unwrap();
        let dark = sphere.snap(model, -0.41, 0.52).unwrap();
        assert_eq!(middle.target.rgb, crate::colour_model_pure(model, sphere.hue(model)));
        assert_eq!((middle.target.x, middle.target.y), (0.0, 0.0));
        for ((x, y), nearest) in [((-0.65, -0.45), middle), ((0.95, -0.20), bright), ((0.15, 0.95), dark)] {
            let from_rim = sphere.snap(model, x, y).unwrap();
            assert_eq!((from_rim.from_x, from_rim.from_y), (x, y));
            assert_eq!(from_rim.target, nearest.target);
        }
        assert!(sphere.snap(model, -1.1, 0.0).is_none());
        assert!(sphere.marker(model).is_none());

        let start = sphere.sample_snap(model, bright, 0.0).unwrap();
        let halfway = sphere.sample_snap(model, bright, 100.0).unwrap();
        assert_eq!((start.x, start.y), (bright.from_x, bright.from_y));
        assert!(start.x < halfway.x && halfway.x < bright.target.x);
        assert_eq!(sphere.sample_snap(model, bright, 200.0), Some(bright.target));
        assert_eq!(sphere.marker(model), Some(bright.target));
        sphere.set_hue(model, 1.0 / 6.0);
        let yellow = sphere.snap(model, 0.0, 0.0).unwrap();
        assert_eq!(sphere.sample_snap(model, yellow, 200.0).unwrap().rgb, 0xFFFF00);
    }

    #[test]
    fn custom_swatches_are_exact_snap_colours_and_rotate_together() {
        let model = ColourModel::Hsb;
        let sphere = ColourSphere::new(model, 0.6);
        let initial = sphere.picture(model);
        sphere.set_custom_enabled(model, true);
        let inputs =
            [(SphereTone::Shadow, 0x0000FF), (SphereTone::Midtone, 0xFF0000), (SphereTone::Highlight, 0x00FF00)];
        for (tone, rgb) in inputs {
            sphere.set_tone_rgb(tone, rgb);
        }
        let anchors = [(-0.70, 0.64), (-0.17, 0.14), (0.75, -0.56)];
        for ((tone, rgb), (x, y)) in inputs.into_iter().zip(anchors) {
            let snap = sphere.snap(model, x, y).unwrap();
            assert_eq!(sphere.tone_rgb(tone), Some(rgb));
            assert_eq!(sphere.sample_snap(model, snap, 200.0).unwrap().rgb, rgb);
        }
        let before_rotation = sphere.snap(model, -0.17, 0.14).unwrap();
        sphere.rotate_hues(1.0 / 3.0);
        assert_eq!(sphere.sample_snap(model, before_rotation, 200.0), None);
        for ((tone, _), rgb) in inputs.into_iter().zip([0xFF0000, 0x00FF00, 0x0000FF]) {
            assert_eq!(sphere.tone_rgb(tone), Some(rgb));
        }
        let after_rotation = sphere.snap(model, -0.17, 0.14).unwrap();
        assert_eq!(
            (after_rotation.target.x, after_rotation.target.y),
            (before_rotation.target.x, before_rotation.target.y)
        );
        assert_eq!(after_rotation.target.rgb, 0x00FF00);
        sphere.set_tone_rgb(SphereTone::Highlight, 0x221433);
        let dark_highlight = sphere.snap(model, 0.76, -0.42).unwrap();
        assert_eq!(sphere.sample_snap(model, dark_highlight, 200.0).unwrap().rgb, 0x221433);
        sphere.set_custom_enabled(model, false);
        assert_eq!(sphere.picture(model), initial);
        sphere.set_custom_enabled(model, true);
        assert_eq!(sphere.tone_rgb(SphereTone::Midtone), Some(0x00FF00));
    }

    #[test]
    fn custom_tone_curve_hits_its_swatches_with_no_change_in_slope_at_the_midpoint() {
        let levels = [0.04, 0.52, 0.99];
        let colours = [[0.06, 0.21, 0.46], [0.62, 0.11, 0.19], [0.93, 0.75, 0.65]];
        let curve = ToneCurve::new(levels, colours, 0.48);
        for (at, rgb) in levels.into_iter().zip(colours) {
            assert_eq!(curve.rgb(at), rgb);
        }
        let before = curve.rgb(0.5199);
        let after = curve.rgb(0.5201);
        for ((a, b), mid) in before.into_iter().zip(after).zip(colours[1]) {
            assert!(((mid - a) - (b - mid)).abs() < 0.000_003);
        }
    }

    #[test]
    fn custom_lighting_keeps_a_tight_highlight_and_brighter_shadow_rim() {
        let model = ColourModel::Hsb;
        let sphere = ColourSphere::new(model, 0.6);
        sphere.set_custom_enabled(model, true);
        sphere.set_tone_rgb(SphereTone::Shadow, 0x18172A);
        sphere.set_tone_rgb(SphereTone::Midtone, 0x2B489C);
        sphere.set_tone_rgb(SphereTone::Highlight, 0xA8FFEA);
        let rim = sphere.sample(model, -0.61, 0.78).unwrap().rgb;
        assert!(((rim >> 8) & 0xff) > 29);
        let shoulder = sphere.sample(model, -0.03, -0.77).unwrap().rgb;
        assert!(((shoulder >> 8) & 0xff) < 140);
    }

    #[test]
    fn palette_swatches_set_the_base_and_reseed_the_custom_colours() {
        let model = ColourModel::Hsb;
        let sphere = ColourSphere::new(model, 0.6);
        let hue_picture = sphere.picture(model);
        sphere.set_base_swatch(model, 0x909090);
        assert_ne!(sphere.picture(model), hue_picture);
        let grey = sphere.snap(model, 0.0, 0.0).unwrap();
        assert_eq!(sphere.sample_snap(model, grey, 200.0).unwrap().rgb, 0x909090);
        sphere.set_hue(model, 0.6);
        assert_eq!(sphere.picture(model), hue_picture);

        sphere.set_custom_enabled(model, true);
        sphere.set_tone_rgb(SphereTone::Shadow, 0xFF00FF);
        sphere.rotate_hues(0.2);
        let old = sphere.snap(model, 0.0, 0.0).unwrap();
        sphere.set_base_swatch(model, 0xED4214);
        assert_eq!(sphere.tone_rgb(SphereTone::Midtone), Some(0xED4214));
        assert_ne!(sphere.tone_rgb(SphereTone::Shadow), Some(0xFF00FF));
        let mid = sphere.marker(model).unwrap();
        assert_eq!(mid.rgb, 0xED4214);
        assert_eq!(sphere.snap(model, mid.x, mid.y).unwrap().target, mid);
        assert_eq!(sphere.sample_snap(model, old, 200.0), None);
    }
}

//! The adjustments a layer is redrawn through, by their own values: drawn into the layer being
//! edited (destructively), or kept by an adjustment layer and drawn over what is below it each
//! time the canvas composites.

/// An adjustment: drawn into the layer being edited (begun as a transform is) from its copy, or by
/// an adjustment layer onto what is below it.
// derived as `Filter::serialize`/`Filter::deserialize`, with the traits below: a file from before
// Curves had its exposure wrote it as a bare "Curves"
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(remote = "Self")]
pub enum Filter {
    /// In luma and chroma: the chroma turned by `hue` (radians) and scaled by `saturation`, and
    /// `brightness` added to the luma.
    Hsb { hue: f32, saturation: f32, brightness: f32 },
    /// A Gaussian blur of `sigma` canvas pixels.
    Blur { sigma: f32 },
    /// Each pixel pushed away from its neighbours by `amount`.
    Sharpen { amount: f32 },
    /// Fractal noise of `kind` (0 clouds, 1 billows, 2 ridges): `amount`, `frequency` (per canvas
    /// pixel), `octaves` (1..5) and `turbulence` (how much each finer octave keeps); each channel
    /// its own when `multi`; added to the colour, or else mixed in.
    Noise { amount: f32, frequency: f32, octaves: u32, turbulence: f32, kind: u32, multi: bool, added: bool },
    /// Each pixel the mean along `along` (canvas pixels), centred on it.
    Motion { along: [f32; 2] },
    /// Each pixel streaked towards `centre` (canvas pixels) by `amount` of the way. With `toward`
    /// (a unit vector; zero for all round) only the side it points to: all of it straight along,
    /// less the further a pixel's way from the centre turns from it, none from square to it on.
    Zoom { centre: [f32; 2], amount: f32, toward: [f32; 2] },
    /// Red and blue pulled apart: from `centre` (canvas pixels) by `amount` of the distance, less
    /// within `falloff` (0..1 of the half diagonal) of it; or else (`displace`) by `offset` (canvas
    /// pixels) everywhere. `blur` (0..1) smears the fringes along the pull; `transparency` (0..1)
    /// fades the fringes beyond the picture.
    Chromatic {
        centre: [f32; 2],
        amount: f32,
        displace: bool,
        offset: [f32; 2],
        blur: f32,
        falloff: f32,
        transparency: f32,
    },
    /// A halftone of `kind` (0 full colour, 1 screen print, 2 newspaper) with cells `pitch` canvas
    /// pixels across.
    Halftone { pitch: f32, kind: u32 },
    /// The light of a blur (`sigma`) screened over the layer: `amount`, from `threshold` up over
    /// a ramp `transition` (0..1) long, strengthened by `burn` (−0.3..1).
    Bloom { sigma: f32, amount: f32, threshold: f32, transition: f32, burn: f32 },
    /// Each channel through the curves the canvas is given (its table), after an exposure: the
    /// light multiplied by 2^`exposure` (stops, −5..5) as one value per pixel, so hue holds. With
    /// `above` (0..1 of white) only what is at least that bright takes all of it, fading in from
    /// `soft` (0..1) of the way below; past white on an HDR canvas, scaled back to it on another.
    Curves {
        #[serde(default)]
        exposure: f32,
        #[serde(default)]
        above: f32,
        #[serde(default = "half")]
        soft: f32,
    },
    /// The gradient the canvas is given (its table) by brightness, `amount` of the way.
    GradientMap { amount: f32 },
    /// Each channel (red, green, blue) shifted in its shadows, midtones and highlights (−1..1).
    ColorBalance { shadows: [f32; 3], midtones: [f32; 3], highlights: [f32; 3] },
    /// The copy moved by the liquify field (begun on the canvas), `amount` of it.
    Liquify { amount: f32 },
    /// The copy from `offset` (canvas pixels) away: cloning, painted on.
    Clone { offset: [f32; 2] },
    /// A glitch of `kind` 0 artifact (blocks copied from a few blocks away), 1 wave (rows bent
    /// sideways), 2 signal (bands ghosted sideways), 3 diverge (the channels moved apart in
    /// blocks), `amount` (0..1) of it. `values` are the kind's own: artifact and signal how many and
    /// the block size (0..1), wave the amplitude and frequency (0..1), diverge the red, green and
    /// blue shifts (−1..1). `zoom` (0..1) scales the pattern; `seed` varies it.
    Glitch { kind: u32, amount: f32, values: [f32; 3], zoom: f32, seed: f32 },
    /// Square cells `size` canvas pixels across, each the mean of what it covers.
    Pixelate { size: f32 },
    /// Each channel brought to `levels` steps (2..), the steps between broken up by a pattern
    /// `scale` canvas pixels a dot: `kind` 0 an ordered (Bayer) grid, 1 noise. `amount` (0..1) of
    /// the way from the colour to that; `mono` in black and white.
    Dither { amount: f32, levels: f32, scale: f32, kind: u32, mono: bool },
    /// The edges darkened (`light`: lightened) towards the corners by `amount` (0..1), starting
    /// `size` (0..1) of the way from the middle to a corner and fading over `softness` (0..1) of the
    /// rest; round on the canvas, whatever its shape.
    Vignette { amount: f32, size: f32, softness: f32, light: bool },
}

impl Filter {
    /// Whether each pixel it draws comes from that pixel alone (and where it is): it can be drawn
    /// over any part of the canvas on its own. The others read their neighbours, so an adjustment
    /// layer of them is drawn over the whole canvas when anything below it changes.
    pub fn pointwise(&self) -> bool {
        matches!(
            self,
            Filter::Hsb { .. }
                | Filter::Noise { .. }
                | Filter::Halftone { .. }
                | Filter::Curves { .. }
                | Filter::GradientMap { .. }
                | Filter::ColorBalance { .. }
                | Filter::Dither { .. }
                | Filter::Vignette { .. }
        )
    }

    /// Whether it leaves what it draws over as it is (a slider at none, say), so an adjustment
    /// layer of it can be skipped.
    pub fn changes_nothing(&self) -> bool {
        match *self {
            Filter::Hsb { hue, saturation, brightness } => hue == 0.0 && saturation == 1.0 && brightness == 0.0,
            Filter::Blur { sigma } => sigma < 0.3,
            Filter::Sharpen { amount } => amount <= 0.0,
            Filter::Noise { amount, .. } => amount <= 0.0,
            Filter::Motion { along: [x, y] } => x.hypot(y) < 1.0,
            Filter::Zoom { amount, .. } => amount <= 0.0,
            Filter::Chromatic { amount, displace, offset: [x, y], .. } => {
                if displace {
                    x == 0.0 && y == 0.0
                } else {
                    amount <= 0.0
                }
            }
            Filter::Bloom { amount, .. } => amount <= 0.0,
            Filter::GradientMap { amount } => amount <= 0.0,
            Filter::ColorBalance { shadows, midtones, highlights } => {
                shadows.iter().chain(&midtones).chain(&highlights).all(|v| *v == 0.0)
            }
            Filter::Glitch { amount, .. } => amount <= 0.0,
            Filter::Pixelate { size } => size < 1.5,
            Filter::Dither { amount, .. } | Filter::Vignette { amount, .. } => amount <= 0.0,
            Filter::Halftone { .. } | Filter::Curves { .. } | Filter::Liquify { .. } | Filter::Clone { .. } => false,
        }
    }
}

/// How a filter's value reads on a panel's row: as it is, canvas pixels, a share shown as a
/// percentage, an angle kept in radians shown in degrees, or photographic stops.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParamUnit {
    Plain,
    Pixels,
    Percent,
    Degrees,
    Stops,
}

/// One of a filter's values that can change smoothly (and so be keyed on a timeline): its name,
/// its value, the range a slider gives it, and how it reads.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FilterParam {
    pub name: &'static str,
    pub value: f32,
    pub low: f32,
    pub high: f32,
    pub unit: ParamUnit,
}

/// The values that can change smoothly of each kind of filter, in [`Filter::values`]'s order:
/// name, low, high and unit. Kinds and on/off choices are left out: they change in steps.
fn param_specs(f: &Filter) -> &'static [(&'static str, f32, f32, ParamUnit)] {
    use ParamUnit::{Degrees, Percent, Pixels, Plain, Stops};
    match f {
        Filter::Hsb { .. } => &[
            ("Hue", -std::f32::consts::PI, std::f32::consts::PI, Degrees),
            ("Saturation", 0.0, 2.0, Percent),
            ("Brightness", -1.0, 1.0, Percent),
        ],
        Filter::Blur { .. } => &[("Blur", 0.0, 683.0, Pixels)],
        Filter::Sharpen { .. } => &[("Amount", 0.0, 1.0, Percent)],
        Filter::Noise { .. } => {
            &[("Amount", 0.0, 1.8, Percent), ("Frequency", 0.0, 0.46, Plain), ("Turbulence", 0.0, 0.32, Percent)]
        }
        Filter::Motion { .. } => &[("Across", -256.0, 256.0, Pixels), ("Down", -256.0, 256.0, Pixels)],
        Filter::Zoom { .. } => &[
            ("Centre across", 0.0, 4096.0, Pixels),
            ("Centre down", 0.0, 4096.0, Pixels),
            ("Amount", 0.0, 0.32, Percent),
        ],
        Filter::Chromatic { .. } => &[
            ("Centre across", 0.0, 4096.0, Pixels),
            ("Centre down", 0.0, 4096.0, Pixels),
            ("Amount", 0.0, 0.05, Percent),
            ("Offset across", -256.0, 256.0, Pixels),
            ("Offset down", -256.0, 256.0, Pixels),
            ("Blur", 0.0, 1.0, Percent),
            ("Fall off", 0.0, 1.0, Percent),
            ("Transparency", 0.0, 1.0, Percent),
        ],
        Filter::Halftone { .. } => &[("Size", 2.0, 98.0, Pixels)],
        Filter::Bloom { .. } => &[
            ("Size", 0.0, 683.0, Pixels),
            ("Amount", 0.0, 1.0, Percent),
            ("Threshold", 0.0, 1.0, Percent),
            ("Transition", 0.0, 1.0, Percent),
            ("Burn", -0.3, 1.0, Percent),
        ],
        Filter::GradientMap { .. } => &[("Amount", 0.0, 1.0, Percent)],
        Filter::ColorBalance { .. } => &[
            ("Shadows red", -1.0, 1.0, Percent),
            ("Shadows green", -1.0, 1.0, Percent),
            ("Shadows blue", -1.0, 1.0, Percent),
            ("Midtones red", -1.0, 1.0, Percent),
            ("Midtones green", -1.0, 1.0, Percent),
            ("Midtones blue", -1.0, 1.0, Percent),
            ("Highlights red", -1.0, 1.0, Percent),
            ("Highlights green", -1.0, 1.0, Percent),
            ("Highlights blue", -1.0, 1.0, Percent),
        ],
        Filter::Glitch { .. } => &[
            ("Amount", 0.0, 1.0, Percent),
            ("First", -1.0, 1.0, Percent),
            ("Second", -1.0, 1.0, Percent),
            ("Third", -1.0, 1.0, Percent),
            ("Zoom", 0.0, 1.0, Percent),
            ("Seed", 0.0, 1.0, Plain),
        ],
        Filter::Pixelate { .. } => &[("Size", 1.0, 128.0, Pixels)],
        Filter::Dither { .. } => {
            &[("Amount", 0.0, 1.0, Percent), ("Levels", 2.0, 16.0, Plain), ("Pattern size", 1.0, 8.0, Pixels)]
        }
        Filter::Vignette { .. } => {
            &[("Amount", 0.0, 1.0, Percent), ("Size", 0.0, 1.0, Percent), ("Softness", 0.0, 1.0, Percent)]
        }
        Filter::Curves { .. } => {
            &[("Exposure", -5.0, 5.0, Stops), ("Above", 0.0, 1.0, Percent), ("Soft", 0.0, 1.0, Percent)]
        }
        Filter::Liquify { .. } | Filter::Clone { .. } => &[],
    }
}

impl Filter {
    /// Its values that change smoothly, in a fixed order for its kind (see [`Self::params`]).
    pub fn values(&self) -> Vec<f32> {
        match *self {
            Filter::Hsb { hue, saturation, brightness } => vec![hue, saturation, brightness],
            Filter::Blur { sigma } => vec![sigma],
            Filter::Sharpen { amount } | Filter::GradientMap { amount } => vec![amount],
            Filter::Noise { amount, frequency, turbulence, .. } => vec![amount, frequency, turbulence],
            Filter::Motion { along: [x, y] } => vec![x, y],
            Filter::Zoom { centre: [x, y], amount, .. } => vec![x, y, amount],
            Filter::Chromatic { centre: [x, y], amount, offset: [ox, oy], blur, falloff, transparency, .. } => {
                vec![x, y, amount, ox, oy, blur, falloff, transparency]
            }
            Filter::Halftone { pitch, .. } => vec![pitch],
            Filter::Bloom { sigma, amount, threshold, transition, burn } => {
                vec![sigma, amount, threshold, transition, burn]
            }
            Filter::ColorBalance { shadows, midtones, highlights } => {
                shadows.iter().chain(&midtones).chain(&highlights).copied().collect()
            }
            Filter::Glitch { amount, values: [a, b, c], zoom, seed, .. } => vec![amount, a, b, c, zoom, seed],
            Filter::Pixelate { size } => vec![size],
            Filter::Dither { amount, levels, scale, .. } => vec![amount, levels, scale],
            Filter::Vignette { amount, size, softness, .. } => vec![amount, size, softness],
            Filter::Curves { exposure, above, soft } => vec![exposure, above, soft],
            Filter::Liquify { .. } | Filter::Clone { .. } => Vec::new(),
        }
    }

    /// The same kind with its smooth values `v` (in [`Self::values`]'s order; one missing or not
    /// a number keeps its own), its other choices kept.
    pub fn with_values(self, v: &[f32]) -> Filter {
        let mut own = self.values();
        for (o, n) in own.iter_mut().zip(v) {
            if n.is_finite() {
                *o = *n;
            }
        }
        let g = |i: usize| own.get(i).copied().unwrap_or(0.0);
        match self {
            Filter::Hsb { .. } => Filter::Hsb { hue: g(0), saturation: g(1), brightness: g(2) },
            Filter::Blur { .. } => Filter::Blur { sigma: g(0) },
            Filter::Sharpen { .. } => Filter::Sharpen { amount: g(0) },
            Filter::GradientMap { .. } => Filter::GradientMap { amount: g(0) },
            Filter::Noise { octaves, kind, multi, added, .. } => {
                Filter::Noise { amount: g(0), frequency: g(1), octaves, turbulence: g(2), kind, multi, added }
            }
            Filter::Motion { .. } => Filter::Motion { along: [g(0), g(1)] },
            Filter::Zoom { toward, .. } => Filter::Zoom { centre: [g(0), g(1)], amount: g(2), toward },
            Filter::Chromatic { displace, .. } => Filter::Chromatic {
                centre: [g(0), g(1)],
                amount: g(2),
                displace,
                offset: [g(3), g(4)],
                blur: g(5),
                falloff: g(6),
                transparency: g(7),
            },
            Filter::Halftone { kind, .. } => Filter::Halftone { pitch: g(0), kind },
            Filter::Bloom { .. } => {
                Filter::Bloom { sigma: g(0), amount: g(1), threshold: g(2), transition: g(3), burn: g(4) }
            }
            Filter::ColorBalance { .. } => Filter::ColorBalance {
                shadows: [g(0), g(1), g(2)],
                midtones: [g(3), g(4), g(5)],
                highlights: [g(6), g(7), g(8)],
            },
            Filter::Glitch { kind, .. } => {
                Filter::Glitch { kind, amount: g(0), values: [g(1), g(2), g(3)], zoom: g(4), seed: g(5) }
            }
            Filter::Pixelate { .. } => Filter::Pixelate { size: g(0) },
            Filter::Dither { kind, mono, .. } => Filter::Dither { amount: g(0), levels: g(1), scale: g(2), kind, mono },
            Filter::Vignette { light, .. } => Filter::Vignette { amount: g(0), size: g(1), softness: g(2), light },
            Filter::Curves { .. } => Filter::Curves { exposure: g(0), above: g(1), soft: g(2) },
            Filter::Liquify { .. } | Filter::Clone { .. } => self,
        }
    }

    /// Its smooth values as a panel lists them, each with its name, range and unit.
    pub fn params(&self) -> Vec<FilterParam> {
        param_specs(self)
            .iter()
            .zip(self.values())
            .map(|(&(name, low, high, unit), value)| FilterParam { name, value, low, high, unit })
            .collect()
    }

    /// The same with smooth value `index` set to `value` (kept within its range).
    pub fn with_param(self, index: usize, value: f32) -> Filter {
        let Some(&(_, low, high, _)) = param_specs(&self).get(index) else { return self };
        if !value.is_finite() {
            return self;
        }
        let mut v = self.values();
        if let Some(slot) = v.get_mut(index) {
            *slot = value.clamp(low.min(high), high.max(low));
        }
        self.with_values(&v)
    }

    /// Whether `other` is the same kind of filter (whatever its values).
    pub fn same_kind(&self, other: &Filter) -> bool {
        std::mem::discriminant(self) == std::mem::discriminant(other)
    }

    /// How far (canvas pixels) a change below an adjustment layer of it reaches in what it draws:
    /// none for a pointwise one, none at all (`None`) when it can reach anywhere (a zoom from its
    /// centre, a glitch's blocks), so the whole canvas is drawn again.
    pub fn reach(&self) -> Option<u32> {
        if self.pointwise() || self.changes_nothing() {
            return Some(0);
        }
        let px = |v: f32| if v.is_finite() { Some(v.max(0.0).ceil() as u32) } else { None };
        match *self {
            // three sigmas, and the halvings a big blur is made through
            Filter::Blur { sigma } | Filter::Bloom { sigma, .. } => px(sigma * 3.0 + 64.0),
            Filter::Sharpen { .. } => Some(1),
            Filter::Motion { along: [x, y] } => px(x.hypot(y) / 2.0 + 2.0),
            Filter::Pixelate { size } => px(size + 1.0),
            Filter::Chromatic { displace: true, offset: [x, y], blur, .. } => px(x.hypot(y) * (1.0 + blur) + 2.0),
            _ => None,
        }
    }
}

impl Filter {
    /// Curves with no exposure (the curves alone).
    pub const CURVES: Filter = Filter::Curves { exposure: 0.0, above: 0.0, soft: 0.5 };
}

fn half() -> f32 {
    0.5
}

impl serde::Serialize for Filter {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        Filter::serialize(self, s)
    }
}

impl<'de> serde::Deserialize<'de> for Filter {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Filter, D::Error> {
        let mut v: serde_json::Value = serde::Deserialize::deserialize(d)?;
        // Curves as it was written before it had its exposure: none
        if v == "Curves" {
            v = serde_json::json!({ "Curves": {} });
        }
        Filter::deserialize(v).map_err(serde::de::Error::custom)
    }
}

/// Keyed on a timeline, a filter eases value by value from one key to the next; its kind and
/// choices are the first key's. Keys of another kind (the layer changed since) don't mix: the
/// first one's are kept.
impl crate::timeline::Keyed for Filter {
    fn plus(self, other: Filter) -> Filter {
        if !self.same_kind(&other) {
            return self;
        }
        let sum: Vec<f32> = self.values().iter().zip(other.values()).map(|(a, b)| a + b).collect();
        self.with_values(&sum)
    }
    fn times(self, s: f32) -> Filter {
        let v: Vec<f32> = self.values().iter().map(|a| a * s).collect();
        self.with_values(&v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_filter_is_kept_in_a_file_as_it_was() {
        let f = Filter::Dither { amount: 0.5, levels: 4.0, scale: 2.0, kind: 1, mono: true };
        let text = serde_json::to_string(&f).ok();
        let back: Option<Filter> = text.as_deref().and_then(|t| serde_json::from_str(t).ok());
        assert_eq!(back, Some(f));
    }

    #[test]
    fn curves_written_before_their_exposure_read_with_none() {
        let back: Option<Filter> = serde_json::from_str("\"Curves\"").ok();
        assert_eq!(back, Some(Filter::CURVES));
        let f = Filter::Curves { exposure: 1.5, above: 0.6, soft: 0.25 };
        let text = serde_json::to_string(&f).ok();
        let back: Option<Filter> = text.as_deref().and_then(|t| serde_json::from_str(t).ok());
        assert_eq!(back, Some(f));
        // keyed, the exposure eases
        assert_eq!(f.values(), vec![1.5, 0.6, 0.25]);
        assert_eq!(Filter::CURVES.with_param(0, 9.0), Filter::Curves { exposure: 5.0, above: 0.0, soft: 0.5 });
    }

    #[test]
    fn colour_changes_are_pointwise_and_blurs_are_not() {
        assert!(Filter::Hsb { hue: 0.1, saturation: 1.0, brightness: 0.0 }.pointwise());
        assert!(Filter::Vignette { amount: 1.0, size: 0.5, softness: 0.5, light: false }.pointwise());
        assert!(!Filter::Blur { sigma: 4.0 }.pointwise());
        assert!(!Filter::Pixelate { size: 8.0 }.pointwise());
        assert!(Filter::Hsb { hue: 0.0, saturation: 1.0, brightness: 0.0 }.changes_nothing());
        assert!(!Filter::Pixelate { size: 8.0 }.changes_nothing());
    }
}

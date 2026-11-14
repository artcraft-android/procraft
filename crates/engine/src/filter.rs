//! The adjustments a layer is redrawn through, by their own values: drawn into the layer being
//! edited (destructively), or kept by an adjustment layer and drawn over what is below it each
//! time the canvas composites.

/// An adjustment: drawn into the layer being edited (begun as a transform is) from its copy, or by
/// an adjustment layer onto what is below it.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
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
    /// Each channel through the curves the canvas is given (its table).
    Curves,
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
                | Filter::Curves
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
            Filter::Halftone { .. } | Filter::Curves | Filter::Liquify { .. } | Filter::Clone { .. } => false,
        }
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
    fn colour_changes_are_pointwise_and_blurs_are_not() {
        assert!(Filter::Hsb { hue: 0.1, saturation: 1.0, brightness: 0.0 }.pointwise());
        assert!(Filter::Vignette { amount: 1.0, size: 0.5, softness: 0.5, light: false }.pointwise());
        assert!(!Filter::Blur { sigma: 4.0 }.pointwise());
        assert!(!Filter::Pixelate { size: 8.0 }.pointwise());
        assert!(Filter::Hsb { hue: 0.0, saturation: 1.0, brightness: 0.0 }.changes_nothing());
        assert!(!Filter::Pixelate { size: 8.0 }.changes_nothing());
    }
}

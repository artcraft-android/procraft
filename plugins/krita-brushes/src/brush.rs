//! A Krita pixel-brush preset made ready to draw: its tip or tips, its spacing, its options, how
//! it paints (washed or built up, its blend), its texture and its masking brush; and how it looks to
//! the app (its tips as grey pictures, the rest as the app's own numbers).

use crate::files::{Rgba, png_decode, png_size};
use crate::gimp::{self, Mask, Selection};
use crate::options::CurveOption;
use crate::preset::Preset;
use crate::rec::Record;
use crate::tips::AutoTip;
use crate::xml::Element;

/// How a tip set's pictures are picked from.
#[derive(Clone, Debug)]
pub enum Tips {
    /// An auto tip: `variants` drawings of it (holes and noise drawn afresh) at each of `levels`
    /// softnesses, by variant then level.
    Auto { variants: u32, levels: u32 },
    /// Pictures: one, or a pipe's cells and how it picks among them.
    Images { count: u32, pipe: Option<PipePick> },
}

#[derive(Clone, Debug)]
pub struct PipePick {
    pub ranks: Vec<u32>,
    pub selections: Vec<Selection>,
}

/// A tip as a brush (or its masking brush) has it.
#[derive(Clone, Debug)]
pub struct Tip {
    pub tips: Tips,
    /// Its own width and height (canvas pixels at size 1), its turn (radians) and its spacing.
    pub width: f32,
    pub height: f32,
    pub angle: f32,
    pub spacing: f32,
    pub auto_spacing: Option<f32>,
    /// Its pictures, for the app.
    pub masks: Vec<Mask>,
}

/// Options a brush and its masking brush both have.
#[derive(Clone, Debug)]
pub struct Dynamics {
    pub size: CurveOption,
    pub opacity: CurveOption,
    pub flow: CurveOption,
    pub rotation: CurveOption,
    pub ratio: CurveOption,
    pub spacing: CurveOption,
    pub scatter: CurveOption,
    pub mirror: CurveOption,
    pub softness: CurveOption,
    pub scatter_axes: (bool, bool),
    pub mirror_axes: (bool, bool),
    pub isotropic: bool,
}

impl Dynamics {
    fn read(p: &Preset, prefix: &str) -> Dynamics {
        let masked = !prefix.is_empty();
        Dynamics {
            size: CurveOption::read(p, prefix, "Size", true, 0.0, 1.0),
            // opacity and flow can be switched off only on a masking brush
            opacity: CurveOption::read(p, prefix, "Opacity", masked, 0.0, 1.0),
            flow: CurveOption::read(p, prefix, "Flow", masked, 0.0, 1.0),
            rotation: CurveOption::read(p, prefix, "Rotation", true, 0.0, 1.0),
            ratio: CurveOption::read(p, prefix, "Ratio", true, 0.0, 1.0),
            spacing: CurveOption::read(p, prefix, "Spacing", true, 0.0, 5.0),
            scatter: CurveOption::read(p, prefix, "Scatter", true, 0.0, 5.0),
            mirror: CurveOption::read(p, prefix, "Mirror", true, 0.0, 1.0),
            softness: CurveOption::read(p, prefix, "Softness", true, 0.1, 1.0),
            scatter_axes: (
                p.flag(&format!("{prefix}Scattering/AxisX"), true),
                p.flag(&format!("{prefix}Scattering/AxisY"), true),
            ),
            mirror_axes: (
                p.flag(&format!("{prefix}HorizontalMirrorEnabled"), false),
                p.flag(&format!("{prefix}VerticalMirrorEnabled"), false),
            ),
            isotropic: p.flag(&format!("{prefix}Spacing/Isotropic"), false),
        }
    }
}

/// A texture as Krita makes its mask of the pattern, and how it combines with each dab.
#[derive(Clone, Debug)]
pub struct Texture {
    pub mask: Mask,
    pub scale: f32,
    pub rule: u32,
    pub offset: (f32, f32),
    pub random_offset: (bool, bool),
    pub strength: CurveOption,
}

#[derive(Clone, Debug)]
pub struct Masking {
    pub tip: Tip,
    pub dynamics: Dynamics,
    /// Its size for each 1 of the main brush's own, and how it combines (the app's numbers).
    pub size: f32,
    pub combine: u32,
}

#[derive(Clone, Debug)]
pub struct Brush {
    pub tip: Tip,
    pub dynamics: Dynamics,
    pub darken: CurveOption,
    pub mix: CurveOption,
    pub hue: CurveOption,
    pub saturation: CurveOption,
    pub value: CurveOption,
    pub wash: bool,
    pub blend: Option<u32>,
    pub erases: bool,
    pub texture: Option<Texture>,
    pub masking: Option<Masking>,
}

/// Krita's composite ops as the app's blend modes number them.
pub fn blend_number(op: &str) -> Option<u32> {
    Some(match op {
        "normal" | "over" => 0,
        "multiply" => 1,
        "darken" => 2,
        "burn" => 4,
        "linear_burn" => 5,
        "darker color" => 6,
        "lighten" => 7,
        "screen" => 8,
        "dodge" => 9,
        "add" | "linear_dodge" => 10,
        "lighter color" => 11,
        "overlay" => 12,
        "soft_light" | "soft_light_svg" | "soft_light_pegtop_delphi" | "soft_light_ifs_illusions" => 13,
        "hard_light" => 14,
        "vivid_light" => 15,
        "linear light" => 16,
        "pin_light" => 17,
        "hard_mix" | "hard_mix_photoshop" | "hard_mix_softer_photoshop" => 18,
        "diff" => 19,
        "exclusion" => 20,
        "subtract" => 21,
        "divide" => 22,
        "hue" => 23,
        "saturation" => 24,
        "color" => 25,
        "luminize" => 26,
        "behind" => 29,
        _ => return None,
    })
}

/// A masking brush's op as the app's dual brushes combine two strokes: a blend mode's number, or
/// its two heights.
fn combine_number(op: &str) -> u32 {
    match op {
        "height" | "height_photoshop" => 27,
        "linear_height" | "linear_height_photoshop" => 28,
        other => blend_number(other).unwrap_or(1),
    }
}

/// Krita's texturing modes as the app's texture rules number them, soft ones 16 on.
fn texture_rule(mode: u32, soft: bool) -> u32 {
    let rule = match mode {
        0 => 32,
        4 => 34,
        5 => 35,
        6 => 36,
        7 => 37,
        8 => 38,
        9 => 39,
        10 => 40,
        11 => 41,
        12 => 42,
        13 => 43,
        14 => 44,
        15 => 45,
        // subtract, and the two that colour a dab (which the app doesn't), as subtract
        _ => 33,
    };
    if soft { rule + 16 } else { rule }
}

/// A mask of a picture file: a GIMP brush or pipe, or a PNG (grey and opaque: dark paints; else
/// dark paints as far as it is opaque). `light`: only their sizes, no pixels.
fn picture_masks(bytes: &[u8], light: bool) -> Option<(Vec<Mask>, Option<PipePick>)> {
    if bytes.starts_with(b"\x89PNG") {
        if light {
            let (width, height) = png_size(bytes)?;
            return Some((vec![Mask { width, height, pixels: Vec::new() }], None));
        }
        return Some((vec![gimp::mask_of(&png_decode(bytes)?)], None));
    }
    if let Some((mask, _)) = gimp::gbr(bytes, !light) {
        return Some((vec![mask], None));
    }
    let pipe = gimp::gih(bytes, !light)?;
    let pick = PipePick { ranks: pipe.ranks, selections: pipe.selections };
    Some((pipe.cells, Some(pick)))
}

/// A round tip `diameter` across, for a preset whose picture can't be had.
fn stand_in(diameter: f32) -> Tip {
    let xml = format!(
        r#"<Brush type="auto_brush" spacing="0.1"><MaskGenerator type="circle" id="default" diameter="{diameter}" ratio="1" hfade="1" vfade="1" antialiasEdges="1"/></Brush>"#
    );
    let brush = crate::xml::parse(&xml).unwrap_or_default();
    read_tip(&brush, &|_| None, None, false).unwrap_or(Tip {
        tips: Tips::Images { count: 1, pipe: None },
        width: diameter,
        height: diameter,
        angle: 0.0,
        spacing: 0.1,
        auto_spacing: None,
        masks: vec![Mask { width: 1, height: 1, pixels: vec![255] }],
    })
}

/// The tip a `Brush` element describes, its pictures found by `find`; `softness`: the brush varies
/// its softness, so an auto tip is drawn at several. `light`: its pictures' sizes only, none drawn.
fn read_tip(
    brush: &Element,
    find: &dyn Fn(&str) -> Option<Vec<u8>>,
    softness: Option<&CurveOption>,
    light: bool,
) -> Option<Tip> {
    let spacing = brush.number("spacing").unwrap_or(0.1).max(0.0);
    let auto_spacing = (brush.attribute("useAutoSpacing") == Some("1")
        || brush.attribute("useAutoSpacing") == Some("true"))
    .then(|| brush.number("autoSpacingCoeff").unwrap_or(1.0));
    let angle = brush.number("angle").unwrap_or(0.0);
    if brush.attribute("type") == Some("auto_brush") {
        let tip = AutoTip::read(brush)?;
        let (w, h) = tip.size();
        let variants = if tip.speckled() { 4 } else { 1 };
        let levels = if softness.is_some_and(CurveOption::varies) { 4 } else { 1 };
        // drawn at its own size (holes and noise as fine as Krita's), else large enough to scale (the GPU
        // draws it smaller through its mip levels)
        let side = if tip.speckled() {
            tip.diameter.round().clamp(8.0, 256.0)
        } else {
            tip.diameter.round().clamp(64.0, 256.0)
        };
        let mut seed = 0x2545_f491_u32;
        let mut random = move || {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            (seed >> 8) as f32 / (1 << 24) as f32
        };
        let mut masks = Vec::new();
        for _ in 0..if light { 0 } else { variants } {
            for level in 0..levels {
                let soft = if levels == 1 { 1.0 } else { 0.1 + 0.9 * level as f32 / (levels - 1) as f32 };
                masks.push(tip.draw(side as u32, soft, &mut random));
            }
        }
        return Some(Tip {
            tips: Tips::Auto { variants, levels },
            width: w,
            height: h,
            angle,
            spacing,
            auto_spacing,
            masks,
        });
    }
    let scale = brush.number("scale").unwrap_or(1.0).max(1e-3);
    let file = brush.attribute("filename")?;
    let (masks, pipe) = picture_masks(&find(file)?, light)?;
    let first = masks.first()?;
    let (w, h) = (first.width as f32 * scale, first.height as f32 * scale);
    let count = masks.len() as u32;
    Some(Tip { tips: Tips::Images { count, pipe }, width: w, height: h, angle, spacing, auto_spacing, masks })
}

/// A pattern's picture: a PNG or a GIMP pattern. `light`: its size only.
fn pattern(bytes: &[u8], light: bool) -> Option<Rgba> {
    if !bytes.starts_with(b"\x89PNG") {
        return gimp::pat(bytes, !light);
    }
    if light {
        let (width, height) = png_size(bytes)?;
        return Some(Rgba { width, height, pixels: Vec::new() });
    }
    png_decode(bytes)
}

/// The mask Krita makes of a pattern for its texture: each pixel's grey where it is opaque (white
/// where clear), made lighter, of more contrast, inverted, its middle moved, cut outside its
/// bounds.
fn texture_mask(p: &Preset, picture: &Rgba) -> Mask {
    let k = |key: &str, otherwise: f32| p.number(&format!("Texture/Pattern/{key}"), otherwise);
    let (brightness, contrast, neutral) = (k("Brightness", 0.0), k("Contrast", 1.0), k("NeutralPoint", 0.5));
    let invert = p.flag("Texture/Pattern/Invert", false);
    let policy = k("CutoffPolicy", 0.0) as u32;
    let (left, right) = (k("CutoffLeft", 0.0) / 255.0, k("CutoffRight", 255.0) / 255.0);
    let pixels = picture
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|px| {
            let alpha = f32::from(px[3]) / 255.0;
            let g = f32::from(gimp::grey(px[0], px[1], px[2])) / 255.0;
            let lit = g * alpha + (1.0 - alpha);
            let mut m = ((lit - brightness - 0.5) * contrast + 0.5).clamp(0.0, 1.0);
            if invert {
                m = 1.0 - m;
            }
            let n = neutral.clamp(0.0, 1.0);
            m = if n >= 1.0 || (n > 0.0 && m <= n) { m / (2.0 * n) } else { 0.5 + (m - n) / (2.0 - 2.0 * n) };
            let outside = m < left || m > right;
            m = match policy {
                1 if outside => 0.0,
                2 if outside => 1.0,
                _ => m.clamp(0.0, 1.0),
            };
            (m * 255.0).round() as u8
        })
        .collect();
    Mask { width: picture.width, height: picture.height, pixels }
}

impl Brush {
    /// The pixel brush `p` describes, its pictures found by `find`; none for another engine's.
    /// `light`: only to draw strokes with, its look not made (its pictures neither drawn nor
    /// decoded).
    pub fn read(p: &Preset, find: &dyn Fn(&str) -> Option<Vec<u8>>, light: bool) -> Option<Brush> {
        if p.paintop != "paintbrush" {
            return None;
        }
        let dynamics = Dynamics::read(p, "");
        let definition = p.element("brush_definition");
        let tip = definition
            .as_ref()
            .and_then(|b| read_tip(b, find, Some(&dynamics.softness), light))
            .unwrap_or_else(|| stand_in(definition.as_ref().and_then(|b| b.number("diameter")).unwrap_or(40.0)));
        let op = p.get("CompositeOp").unwrap_or("normal").trim().to_owned();
        let texture = p.flag("Texture/Pattern/Enabled", false).then(|| Brush::read_texture(p, find, light)).flatten();
        let masking = p.flag("MaskingBrush/Enabled", false).then(|| Brush::read_masking(p, find, light)).flatten();
        Some(Brush {
            tip,
            dynamics,
            darken: CurveOption::read(p, "", "Darken", true, 0.0, 1.0),
            mix: CurveOption::read(p, "", "Mix", true, 0.0, 1.0),
            hue: CurveOption::read(p, "", "h", true, 0.0, 1.0),
            saturation: CurveOption::read(p, "", "s", true, 0.0, 1.0),
            value: CurveOption::read(p, "", "v", true, 0.0, 1.0),
            // washed unless it says it builds up; a masked one always is
            wash: p.number("PaintOpAction", 2.0) != 1.0 || masking.is_some(),
            blend: blend_number(&op),
            erases: op == "erase" || p.flag("EraserMode", false),
            texture,
            masking,
        })
    }

    fn read_texture(p: &Preset, find: &dyn Fn(&str) -> Option<Vec<u8>>, light: bool) -> Option<Texture> {
        // its picture: carried in the setting itself (older presets), else a file it names
        // (read only to draw, only its header: its size)
        let carried =
            p.get("Texture/Pattern/Pattern").map(|t| if light { t.get(..t.len().min(8192)).unwrap_or(t) } else { t });
        let bytes = carried.and_then(crate::files::base64).filter(|b| !b.is_empty()).or_else(|| {
            ["Texture/Pattern/PatternFileName", "Texture/Pattern/Name"].iter().find_map(|k| find(p.get(k)?.trim()))
        })?;
        let picture = pattern(&bytes, light)?;
        let k = |key: &str, otherwise: f32| p.number(&format!("Texture/Pattern/{key}"), otherwise);
        Some(Texture {
            mask: texture_mask(p, &picture),
            scale: k("Scale", 1.0).max(0.01),
            rule: texture_rule(k("TexturingMode", 0.0) as u32, p.flag("Texture/Pattern/UseSoftTexturing", false)),
            offset: (k("OffsetX", 0.0), k("OffsetY", 0.0)),
            random_offset: (
                p.flag("Texture/Pattern/isRandomOffsetX", false),
                p.flag("Texture/Pattern/isRandomOffsetY", false),
            ),
            strength: CurveOption::read(p, "", "Texture/Strength/", true, 0.0, 1.0),
        })
    }

    fn read_masking(p: &Preset, find: &dyn Fn(&str) -> Option<Vec<u8>>, light: bool) -> Option<Masking> {
        let prefix = "MaskingBrush/Preset/";
        let dynamics = Dynamics::read(p, prefix);
        let definition = p.element(&format!("{prefix}brush_definition"))?;
        let tip = read_tip(&definition, find, None, light)?;
        let size = if p.flag("MaskingBrush/UseMasterSize", true) {
            p.number("MaskingBrush/MasterSizeCoeff", 1.0).max(0.0)
        } else {
            0.0
        };
        let combine = combine_number(p.get("MaskingBrush/MaskingCompositeOp").unwrap_or("multiply").trim());
        Some(Masking { tip, dynamics, size, combine })
    }

    /// How it looks to the app (see the app's guide to brush engines).
    pub fn look(&self) -> Vec<u8> {
        let mut look = Record::new();
        for m in &self.tip.masks {
            look.field(1, &tip_record(m));
        }
        look.byte(2, u8::from(self.wash));
        if let Some(blend) = self.blend {
            look.word(3, blend);
        }
        look.byte(4, u8::from(self.erases));
        if let Some(t) = &self.texture {
            let mut r = Record::new();
            r.word(1, t.mask.width).word(2, t.mask.height).field(3, &t.mask.pixels).real(4, t.scale).word(5, t.rule);
            look.field(5, &r.bytes());
        }
        if let Some(m) = &self.masking {
            let mut r = Record::new();
            for mask in &m.tip.masks {
                r.field(1, &tip_record(mask));
            }
            r.byte(2, 1).word(3, m.combine);
            look.field(6, &r.bytes());
        }
        look.bytes()
    }
}

fn tip_record(m: &Mask) -> Vec<u8> {
    let mut r = Record::new();
    r.word(1, m.width).word(2, m.height).field(3, &m.pixels);
    r.bytes()
}

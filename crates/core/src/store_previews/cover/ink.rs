use image::{GrayImage, Rgba, RgbaImage, imageops};
use procraft_engine::{brush::Brush, geom::Vec2, input::Sample, preview};
use tiny_skia::{IntSize, Pixmap};

use super::super::{PREVIEW_WIDTH, shape_of, shown_brushes, trimmed};
use super::{BRUSH, CoverTemplate, STAMP};
use crate::{CanvasSession, brushes::Painted};

// Three independent probe cells on one transparent canvas. The margins leave room for scatter
// and the largest tip, and keep taps isolated from anything drawn along a path.
const WIDTH: u32 = 480;
const HEIGHT: u32 = 272;
const TAP_PRESSURE: f32 = 0.91;
const MOST_PRINT_DIAMETER: f32 = 1536.0;

#[derive(Clone, Copy, PartialEq)]
enum Probe {
    Tap = 0,
    Drag = 1,
    Sweep = 2,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) enum Role {
    Stamp,
    Scatter,
    Bold,
    Fine,
    Wash,
    Grain,
}

impl Role {
    fn probe(self) -> Probe {
        match self {
            Self::Stamp | Self::Scatter => Probe::Tap,
            Self::Fine | Self::Grain => Probe::Drag,
            Self::Bold | Self::Wash => Probe::Sweep,
        }
    }
}

#[derive(Clone, Copy)]
struct Pixels {
    width: f32,
    height: f32,
    coverage: f32,
    density: f32,
    strength: f32,
    softness: f32,
    broken: f32,
}

impl Pixels {
    fn read(image: &RgbaImage) -> Self {
        let (width, height) = image.dimensions();
        let (mut ink, mut mass, mut soft, mut edges) = (0, 0u32, 0, 0);
        for (x, y, pixel) in image.enumerate_pixels() {
            let a = pixel.0[3];
            if a > 12 {
                ink += 1;
                mass += u32::from(a);
                soft += usize::from(a < 182);
            }
            if x > 0 {
                edges += usize::from(a.abs_diff(image.get_pixel(x - 1, y).0[3]) > 30);
            }
            if y > 0 {
                edges += usize::from(a.abs_diff(image.get_pixel(x, y - 1).0[3]) > 30);
            }
        }
        let area = (width * height) as f32;
        Self {
            width: width as f32,
            height: height as f32,
            coverage: ink as f32,
            density: ink as f32 / area,
            strength: mass as f32 / (255.0 * ink.max(1) as f32),
            softness: soft as f32 / ink.max(1) as f32,
            broken: (edges as f32 / ink.max(1) as f32).min(1.0),
        }
    }

    fn distance(self, other: Self) -> f32 {
        (self.density - other.density).abs()
            + (self.strength - other.strength).abs()
            + (self.softness - other.softness).abs() * 0.65
            + (self.broken - other.broken).abs()
            + (self.height / self.width - other.height / other.width).abs().min(1.0)
    }
}

pub(super) struct Mark {
    masks: Vec<Pixmap>,
    pixels: Pixels,
}

impl Mark {
    fn from_probe(image: &RgbaImage, probe: Probe) -> Option<Self> {
        let image = trimmed(image)?;
        let pixels = Pixels::read(&image);
        if pixels.coverage < 7.0 {
            return None;
        }
        let gain = if probe == Probe::Tap { tap_gain(pixels) } else { 1.0 };
        let mask = print_mask(&alpha_plane(&image, gain)?)?;
        Some(Self { masks: vec![mask], pixels })
    }

    /// Pick the smallest mask that can fill the slot without enlargement.
    pub(super) fn mask(&self, width: f32, height: f32) -> Option<&Pixmap> {
        self.masks
            .iter()
            .find(|mask| mask.width() as f32 >= width || mask.height() as f32 >= height)
            .or_else(|| self.masks.last())
    }

    fn prepare_tap(&mut self, session: &CanvasSession, brush: &Brush, probe_diameter: f32) -> Option<()> {
        let extent = self.pixels.width.max(self.pixels.height);
        let diameter = (probe_diameter * PREVIEW_WIDTH as f32 / extent).clamp(probe_diameter, MOST_PRINT_DIAMETER);
        let side = ((diameter * 1.75).ceil() as u32).next_multiple_of(64);
        let center = side as f32 * 0.5;
        let tap = Painted {
            brush: brush.clone(),
            samples: vec![Sample::at(Vec2::new(center, center), TAP_PRESSURE, 0.0)],
            ink: [1.0; 3],
            diameter,
            second_scale: diameter / probe_diameter,
        };
        let image = RgbaImage::from_raw(side, side, session.painted(&[tap], side, side)?)?;
        let image = trimmed(&image)?;
        let mut alpha = alpha_plane(&image, tap_gain(self.pixels))?;

        // Smaller slots get a downsampled copy of the same tap. Keep the probe's metrics for
        // matching so the layout and brush choices stay independent of print resolution.
        let mut masks = vec![print_mask(&alpha)?];
        while alpha.width().max(alpha.height()) > 256 {
            alpha = imageops::resize(
                &alpha,
                alpha.width().div_ceil(2),
                alpha.height().div_ceil(2),
                imageops::FilterType::Triangle,
            );
            masks.push(print_mask(&alpha)?);
        }
        masks.reverse();
        self.masks = masks;
        Some(())
    }
}

/// Give faint taps enough contrast for the shelf while keeping their silhouette.
fn tap_gain(pixels: Pixels) -> f32 {
    (0.53 / pixels.strength).clamp(1.0, 3.5)
}

fn alpha_plane(image: &RgbaImage, gain: f32) -> Option<GrayImage> {
    let bytes = image.pixels().map(|pixel| (f32::from(pixel.0[3]) * gain).round().min(255.0) as u8).collect();
    GrayImage::from_raw(image.width(), image.height(), bytes)
}

fn print_mask(image: &GrayImage) -> Option<Pixmap> {
    let size = IntSize::from_wh(image.width(), image.height())?;
    let bytes = image.as_raw().iter().flat_map(|&alpha| [alpha; 4]).collect();
    Pixmap::from_vec(bytes, size)
}

struct BrushProbe {
    source: usize,
    tap_diameter: f32,
    marks: [Option<Mark>; 3],
    stamp: f32,
    grain: f32,
}

impl BrushProbe {
    fn read(image: &RgbaImage, brush: &Brush, source: usize, tap_diameter: f32) -> Self {
        let marks = [Probe::Tap, Probe::Drag, Probe::Sweep].map(|probe| {
            let cell = imageops::crop_imm(image, probe as u32 * WIDTH, 0, WIDTH, HEIGHT).to_image();
            Mark::from_probe(&cell, probe)
        });
        let shape = shape_of(brush);
        let [tap, drag, _] = &marks;
        let silhouette = tap.as_ref().map_or(0.0, |mark| {
            let p = mark.pixels;
            let span = (p.width.min(p.height) / 32.0).clamp(0.0, 1.0);
            span * (0.55 + p.strength * 0.25 + (1.0 - p.density) * 0.20)
        });
        let separated = drag.as_ref().map_or(0.0, |mark| {
            let p = mark.pixels;
            (1.0 - p.density) * 0.22 + p.broken * 0.18
        });
        let stamp = silhouette * (shape.shape * 0.20 + shape.spacing * 0.83 + shape.scatter * 0.27 + separated);
        Self { source, tap_diameter, marks, stamp, grain: shape.grain }
    }

    fn for_role(&self, role: Role) -> Option<(&Mark, f32)> {
        let [tap, drag, sweep] = &self.marks;
        let mark = match role.probe() {
            Probe::Tap => tap.as_ref(),
            Probe::Drag => drag.as_ref().or(sweep.as_ref()),
            Probe::Sweep => sweep.as_ref().or(drag.as_ref()),
        }?;
        let p = mark.pixels;
        let span = (p.width / 250.0).min(1.0);
        let height = (p.height / 108.0).min(1.0);
        let score = match role {
            Role::Stamp => (p.coverage / 3100.0).min(1.0) * 0.45 + self.stamp * 0.65 + p.strength * 0.20,
            Role::Scatter => {
                (1.0 - p.density) * 0.42 + height * 0.15 + p.broken * 0.19 + (p.coverage / 620.0).min(1.0) * 0.24
            }
            Role::Bold => span * 0.25 + height * 0.33 + p.density * 0.23 + p.strength * 0.19,
            Role::Fine => span * 0.48 + (1.0 - height) * 0.30 + p.strength * 0.22,
            Role::Wash => {
                let body = (p.coverage / 3600.0).min(1.0) * (p.strength / 0.42).min(1.0);
                span * 0.14 + height * 0.22 + p.softness * 0.37 + (1.0 - p.broken) * 0.11 + body * 0.16
            }
            Role::Grain => span * 0.15 + p.broken * 0.45 + (1.0 - p.density) * 0.34 + self.grain * 0.06,
        };
        Some((mark, score))
    }
}

pub(super) struct BrushMarks {
    probes: Vec<BrushProbe>,
}

fn samples(probe: Probe) -> Vec<Sample> {
    let x = probe as u32 as f32 * WIDTH as f32;
    match probe {
        Probe::Tap => vec![Sample::at(Vec2::new(x + 240.0, 136.0), TAP_PRESSURE, 0.0)],
        Probe::Drag => (0..=38)
            .map(|step| {
                let t = step as f32 / 38.0;
                Sample::at(Vec2::new(x + 128.0 + t * 224.0, 135.0), 0.61, f64::from(step) * 8.0)
            })
            .collect(),
        Probe::Sweep => (0..=70)
            .map(|step| {
                let t = step as f32 / 70.0;
                let rise = (t * std::f32::consts::PI).sin();
                let y = 156.0 - 53.0 * rise + 20.0 * (2.0 * t - 1.0);
                Sample::at(Vec2::new(x + 79.0 + t * 319.0, y), 0.16 + 0.82 * rise, f64::from(step) * 8.0)
            })
            .collect(),
    }
}

impl BrushMarks {
    pub(super) fn probe(session: &CanvasSession, brushes: &[Brush], seed: u64) -> Self {
        let probes = shown_brushes(brushes, seed)
            .into_iter()
            .filter_map(|index| {
                let brush = brushes.get(index)?;
                let diameter = preview::diameter(brush).clamp(1.0, 124.0);
                let tap_diameter = if shape_of(brush).shape > 0.0 { diameter.max(92.0) } else { diameter };
                let strokes = [Probe::Tap, Probe::Drag, Probe::Sweep].map(|probe| Painted {
                    brush: brush.clone(),
                    samples: samples(probe),
                    ink: [1.0; 3],
                    diameter: if probe == Probe::Tap { tap_diameter } else { diameter },
                    second_scale: 1.0,
                });
                let pixels = session.painted(&strokes, WIDTH * 3, HEIGHT)?;
                let image = RgbaImage::from_raw(WIDTH * 3, HEIGHT, pixels)?;
                Some(BrushProbe::read(&image, brush, index, tap_diameter))
            })
            .collect();
        let mut marks = Self { probes };
        if marks.templates().contains(&CoverTemplate::ImpactPoster) {
            let selected = [
                marks.best(Role::Stamp).map(|(index, _, _)| index),
                marks.distinct(Role::Scatter, Role::Stamp).map(|(index, _)| index),
            ];
            for index in selected.into_iter().flatten() {
                let Some(probe) = marks.probes.get_mut(index) else { continue };
                let [Some(tap), ..] = &mut probe.marks else { continue };
                let Some(brush) = brushes.get(probe.source) else { continue };
                let _ = tap.prepare_tap(session, brush, probe.tap_diameter);
            }
        }
        marks
    }

    pub(super) fn templates(&self) -> Vec<CoverTemplate> {
        let taps = self.probes.iter().filter(|probe| probe.stamp >= 0.43).count();
        let strokes =
            self.probes.iter().filter(|probe| probe.stamp < 0.43 && probe.for_role(Role::Bold).is_some()).count();
        let mut templates = Vec::new();
        if taps > 0 || (strokes == 0 && self.best(Role::Stamp).is_some()) {
            templates.extend_from_slice(STAMP);
        }
        if strokes > 0 {
            templates.extend_from_slice(BRUSH);
        }
        templates
    }

    pub(super) fn recommended(&self) -> Option<CoverTemplate> {
        use CoverTemplate::*;
        let templates = self.templates();
        let score = |role| self.best(role).map_or(0.0, |(_, _, quality)| quality);
        let stamp = self.probes.iter().filter(|probe| probe.stamp >= 0.43).count();
        let has = |template| templates.contains(&template);
        if has(ImpactPoster) && (stamp * 2 >= self.probes.len() || !has(InkCurrent)) {
            return Some(if self.support(Role::Scatter, Role::Stamp).is_some() && score(Role::Scatter) > 0.56 {
                Constellation
            } else {
                ImpactPoster
            });
        }
        if !has(InkCurrent) {
            return None;
        }
        let bold = score(Role::Bold);
        let wash = score(Role::Wash);
        let grain = score(Role::Grain);
        Some(if wash > 0.70 && self.support(Role::Fine, Role::Wash).is_some() && bold > 0.49 {
            CoastalPrint
        } else if wash > 0.72 && wash > bold + 0.17 {
            WashFields
        } else if grain > 0.58 && self.support(Role::Grain, Role::Bold).is_some() {
            HalftoneHorizon
        } else if bold > 0.60 {
            GraphicPoster
        } else {
            InkCurrent
        })
    }

    fn ranked(&self, role: Role) -> impl Iterator<Item = (usize, &Mark, f32)> {
        self.probes
            .iter()
            .enumerate()
            .filter_map(move |(index, probe)| probe.for_role(role).map(|(mark, score)| (index, mark, score)))
    }

    fn best(&self, role: Role) -> Option<(usize, &Mark, f32)> {
        self.ranked(role).max_by(|a, b| a.2.total_cmp(&b.2))
    }

    pub(super) fn primary(&self, role: Role) -> Option<&Mark> {
        self.best(role).map(|(_, mark, _)| mark)
    }

    pub(super) fn support(&self, role: Role, against: Role) -> Option<&Mark> {
        self.distinct(role, against).map(|(_, mark)| mark)
    }

    fn distinct(&self, role: Role, against: Role) -> Option<(usize, &Mark)> {
        let first = self.primary(against)?;
        self.ranked(role)
            .filter_map(|(index, mark, fitness)| {
                if std::ptr::eq(first, mark) {
                    return None;
                }
                let difference = mark.pixels.distance(first.pixels);
                (difference >= 0.30 && fitness >= 0.43).then_some((index, mark, fitness + difference.min(1.0) * 0.14))
            })
            .max_by(|a, b| a.2.total_cmp(&b.2))
            .map(|(index, mark, _)| (index, mark))
    }

    pub(super) fn specimen_tap(session: &CanvasSession, brush: &Brush) -> Option<RgbaImage> {
        let diameter = preview::diameter(brush).clamp(92.0, 124.0);
        let stroke =
            Painted { brush: brush.clone(), samples: samples(Probe::Tap), ink: [1.0; 3], diameter, second_scale: 1.0 };
        let rgba = session.painted(&[stroke], WIDTH, HEIGHT)?;
        let image = RgbaImage::from_raw(WIDTH, HEIGHT, rgba)?;
        let cropped = trimmed(&image)?;
        Some(RgbaImage::from_fn(cropped.width(), cropped.height(), |x, y| {
            let a = cropped.get_pixel(x, y).0[3];
            Rgba([a, a, a, a])
        }))
    }

    pub(super) fn stamp_settings(brush: &Brush) -> bool {
        let shape = shape_of(brush);
        shape.shape > 0.0 && (shape.spacing >= 0.29 || shape.scatter >= 0.48)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use procraft_engine::{brush::Source, textures::TEXTURE_SIZE};

    #[test]
    fn a_stamp_cover_uses_a_larger_raster_than_its_matching_probe() {
        let Some(session) = crate::tests::session(64, 64) else { return };
        let mut brush = Brush::round_pen();
        let n = TEXTURE_SIZE;
        let tip = (0..n * n)
            .map(|i| {
                let (x, y) = (i % n, i / n);
                if (32..224).contains(&x) && (32..224).contains(&y) { 255 } else { 0 }
            })
            .collect();
        brush.shape.source = Source::new(tip);
        brush.spacing = 1.0;

        let marks = BrushMarks::probe(&session, &[brush], 0);
        assert!(marks.templates().contains(&CoverTemplate::ImpactPoster));
        let tap = marks.primary(Role::Stamp).unwrap();
        assert!(tap.pixels.width < 150.0);
        let large = tap.mask(860.0, 860.0).unwrap();
        assert!(large.width() >= 860 && large.height() >= 860);
        let small = tap.mask(160.0, 160.0).unwrap();
        assert!(small.width() < large.width() && small.height() < large.height());
    }
}

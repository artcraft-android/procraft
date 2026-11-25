use std::f32::consts::PI;

use tiny_skia::{FillRule, Paint, PathBuilder, Pixmap, Transform};

use super::super::{PREVIEW_WIDTH, Rng, box_blur};
use super::surface::pixmap;

const WIDTH: usize = PREVIEW_WIDTH as usize;

fn unit(rng: &mut Rng) -> f32 {
    (rng.next() >> 40) as f32 / (1 << 24) as f32
}

fn between(rng: &mut Rng, (lo, hi): (f32, f32)) -> f32 {
    lo + (hi - lo) * unit(rng)
}

fn fibers(
    rng: &mut Rng,
    density: f32,
    lengths: (f32, f32),
    widths: (f32, f32),
    levels: (f32, f32),
    blur: usize,
    height: usize,
) -> Vec<f32> {
    let mut map = pixmap(height as u32);
    map.fill(tiny_skia::Color::from_rgba8(28, 28, 28, 255));
    for _ in 0..(WIDTH as f32 * height as f32 * density) as usize {
        let x = between(rng, (-8.0, WIDTH as f32 + 8.0));
        let y = between(rng, (-8.0, height as f32 + 8.0));
        let angle = if unit(rng) < 0.52 {
            -0.17 + (rng.around(1.0) + rng.around(1.0) + rng.around(1.0)) * 0.63
        } else {
            unit(rng) * PI
        };
        let (uy, ux) = angle.sin_cos();
        let length = between(rng, lengths);
        let width = between(rng, widths);
        let bend = rng.around(0.43);
        let twist = rng.around(0.115);
        let outline = [(-0.53_f32, 0.12), (-0.37, 0.63), (-0.16, 0.94), (0.07, 1.0), (0.29, 0.73), (0.50, 0.22)].map(
            |(position, taper)| {
                let distance = position * length;
                let curve = (position * position * bend + (position * 4.7).sin() * twist) * length;
                let (cx, cy) = (x + ux * distance - uy * curve, y + uy * distance + ux * curve);
                let side = taper * width * 0.5;
                ((cx - uy * side, cy + ux * side), (cx + uy * side, cy - ux * side))
            },
        );
        let mut path = PathBuilder::new();
        for (index, &((x, y), _)) in outline.iter().enumerate() {
            if index == 0 {
                path.move_to(x, y);
            } else {
                path.line_to(x, y);
            }
        }
        for &(_, (x, y)) in outline.iter().rev() {
            path.line_to(x, y);
        }
        path.close();
        if let Some(path) = path.finish() {
            let value = between(rng, levels) as u8;
            let mut paint = Paint::default();
            paint.set_color_rgba8(value, value, value, 255);
            map.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
    let mut relief: Vec<f32> = map.pixels().iter().map(|p| f32::from(p.red())).collect();
    if blur > 0 {
        box_blur(&mut relief, WIDTH, height, blur);
    }
    relief
}

fn inclusions(rng: &mut Rng, height: usize, size: usize, radii: (f32, f32), levels: (f32, f32)) -> Vec<f32> {
    let mut map = pixmap(height as u32);
    for _ in 0..WIDTH * height / size {
        let x = unit(rng) * WIDTH as f32;
        let y = unit(rng) * height as f32;
        let radius = between(rng, radii);
        let value = between(rng, levels) as u8;
        let Some(rect) = tiny_skia::Rect::from_ltrb(x - radius * 1.25, y - radius, x + radius * 1.25, y + radius)
        else {
            continue;
        };
        if let Some(path) = PathBuilder::from_oval(rect) {
            let mut paint = Paint::default();
            paint.set_color_rgba8(value, value, value, 255);
            map.fill_path(&path, &paint, FillRule::Winding, Transform::identity(), None);
        }
    }
    map.pixels().iter().map(|p| f32::from(p.red())).collect()
}

/// Overlapping, tapered pulp at three scales, with pinholes and occasional pigment inclusions.
/// Directional shading comes from the height of the fibers and depressions. The neutral sheet
/// modulates the colors of the ink layers when the cover is printed onto it with Multiply.
pub(super) fn pressed_pulp(seed: u64, height: u32) -> Pixmap {
    let height = height as usize;
    let mut rng = Rng::new(seed);
    let bed = fibers(&mut rng, 0.010, (7.0, 34.5), (1.7, 8.5), (46.0, 232.0), 1, height);
    let ribbon = fibers(&mut rng, 0.017, (5.5, 28.0), (1.4, 5.0), (56.0, 242.0), 0, height);
    let lint = fibers(&mut rng, 0.069, (2.0, 12.0), (0.75, 1.7), (36.0, 210.0), 0, height);
    let holes = inclusions(&mut rng, height, 63, (0.34, 1.03), (19.0, 91.0));
    let flecks = inclusions(&mut rng, height, 2520, (0.32, 1.26), (12.0, 127.0));
    let materials = bed.iter().zip(&ribbon).zip(&lint).zip(&holes);
    let relief: Vec<_> = materials
        .clone()
        .map(|(((&bed, &ribbon), &lint), &holes)| (0.20 * bed + 0.15 * ribbon + 0.055 * lint - 0.23 * holes) / 21.0)
        .collect();
    let relief_at = |x, y| relief.get(x + y * WIDTH).copied().unwrap_or(0.0);
    let tone: Vec<_> = materials
        .zip(&flecks)
        .enumerate()
        .map(|(i, ((((&bed, &ribbon), &lint), &holes), &flecks))| {
            let (x, y) = (i % WIDTH, i / WIDTH);
            let dx = (relief_at((x + 1).min(WIDTH - 1), y) - relief_at(x.saturating_sub(1), y)) * 0.27;
            let dy = (relief_at(x, (y + 1).min(height - 1)) - relief_at(x, y.saturating_sub(1))) * 0.27;
            let light = (0.62 + 0.40 * dx + 0.66 * dy) / (1.0 + dx * dx + dy * dy).sqrt();
            (light - 0.62) * 29.0 + lint * 0.019 + ribbon * 0.038 + bed * 0.020 - holes * 0.045 - flecks * 0.48
        })
        .collect();
    let middle = tone.iter().sum::<f32>() / tone.len() as f32;
    let mut paper = pixmap(height as u32);
    for (pixel, value) in paper.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(tone) {
        let shade = (247.0 + (value - middle) * 1.42).round().clamp(0.0, 255.0) as u8;
        *pixel = [shade, shade, shade, 255];
    }
    paper
}

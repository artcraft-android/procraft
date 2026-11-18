//! The sample stroke a brush is shown with in the library and in the files it is shared in: a
//! gentle wave across a canvas of [`CANVAS`] pixels, drawn fast, its pressure swelling to full in
//! the middle, at a size the brush's preview size sets.

use crate::brush::Brush;
use crate::geom::Vec2;
use crate::input::Sample;

/// The canvas the sample stroke is drawn on (pixels); a preview of another size fits it in.
pub const CANVAS: (f32, f32) = (1060.0, 324.0);

/// How many pen samples the stroke has.
const SAMPLES: usize = 36;

/// The time between samples (milliseconds): a pen reporting 240 times a second.
const STEP_MS: f64 = 1000.0 / 240.0;

/// Randomized heads share the first turn of the preview session, including on redraw.
pub(crate) fn randomized_head_turn() -> f32 {
    static TURN: std::sync::OnceLock<f32> = std::sync::OnceLock::new();
    *TURN.get_or_init(|| {
        let seconds = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(1, |elapsed| elapsed.as_secs() as u32);
        first_head_turn(seconds)
    })
}

fn first_head_turn(seed: u32) -> f32 {
    // The session draws two 31-bit Park–Miller values for one fractional sample.
    let next = |value: u32| {
        let seed = if value == 0 { 123_459_876 } else { value };
        (u64::from(seed) * 16_807 % 2_147_483_647) as u32
    };
    let high = next(seed);
    let scale = 1.0 / 2_147_483_648.0_f32;
    let unit = high as f32 * scale + next(high) as f32 * scale * scale;
    unit.mul_add(std::f32::consts::TAU, -std::f32::consts::PI)
}

/// The recorded preview advances along a line tilted six degrees, with a sine wave across it.
fn along(t: f32) -> Vec2 {
    let (sin, cos) = 6.000_014_f32.to_radians().sin_cos();
    let forward = 810.0 * t;
    let across = 60.005_41 * (std::f32::consts::FRAC_PI_3 + 3.759_170_7 * t).sin();
    Vec2::new(122.068_92 + cos * forward - sin * across, 133.711_85 + sin * forward + cos * across)
}

/// The press along a brush's sample stroke, as the previews ship it: light at the ends,
/// full across the middle, one value per pen sample.
const PRESS: [f32; 36] = [
    0.105925, 0.135983, 0.172023, 0.214441, 0.263420, 0.318866, 0.380352, 0.447077, 0.517842, 0.591058, 0.664786,
    0.736804, 0.804713, 0.866061, 0.918489, 0.959884, 0.988511, 1.003143, 1.003143, 0.988511, 0.959884, 0.918489,
    0.866061, 0.804713, 0.736804, 0.664786, 0.591058, 0.517842, 0.447077, 0.380352, 0.318866, 0.263420, 0.214441,
    0.172023, 0.135983, 0.105925,
];

/// The press the preview's pen lays at `t` (0..1 along): the shipped press, turned up by the
/// brush's preview scale and clamped at full.
pub fn pressure_at(brush: &Brush, t: f32) -> f32 {
    let x = t.clamp(0.0, 1.0) * (PRESS.len() - 1) as f32;
    let i = (x as usize).min(PRESS.len() - 2);
    let a = PRESS.get(i).copied().unwrap_or(0.0);
    let b = PRESS.get(i + 1).copied().unwrap_or(a);
    // The thumbnail uses the recorded pen pressure. Half travel on the studio scale
    // represents the file's unit multiplier.
    let recorded = a + (b - a) * (x - i as f32);
    (recorded * 2.0 * brush.setting("preview_pressure_scale") + brush.setting("preview_pressure_min")).clamp(0.0, 1.0)
}

/// The sample stroke for `brush`, in canvas pixels.
pub fn samples(brush: &Brush) -> Vec<Sample> {
    use std::f32::consts::FRAC_PI_2;
    let rotation = (brush.setting("preview_tilt_offset") - 1.0) * FRAC_PI_2;
    let tilted = brush.setting("preview_tilt") >= 0.5;
    (0..SAMPLES)
        .map(|i| {
            let t = i as f32 / (SAMPLES - 1) as f32;
            // The recorded vector's length is altitude in quarter turns. Its heading
            // is constant after the initial zero vector. Input measures lean from upright.
            let altitude = 0.0473245 * t.powi(3);
            let tilt = if tilted { FRAC_PI_2 * (1.0 - altitude) } else { 0.0 };
            let azimuth = rotation + if i == 0 { 0.0 } else { 0.053381 };
            Sample { tilt, azimuth, ..Sample::at(along(t), pressure_at(brush, t), i as f64 * STEP_MS) }
        })
        .collect()
}

/// The brush's size along the sample stroke, from its least (0) to its most (1): the preview size,
/// less for brushes so big they would not fit, more for small ones (to be seen) and for those
/// that thin out with speed (the stroke is fast).
pub fn size_share(brush: &Brush) -> f32 {
    let units = size_units(brush);
    let small = if units < 4.0 { (0.1 - units.min(0.08)) * 9.5 } else { 0.0 };
    share(brush, units, small)
}

/// The size the studio's drawing pad draws at, from the brush's least (0) to its most (1): its
/// preview size, as its sliders start, so that a bigger most size draws bigger there too.
pub fn pad_size_share(brush: &Brush) -> f32 {
    let share = brush.setting("preview_size");
    if share.is_finite() { share.clamp(0.0, 1.0) } else { 0.0 }
}

/// The brush's most size in the units brush files keep it in.
fn size_units(brush: &Brush) -> f32 {
    brush.biggest() / brush.size_unit()
}

fn share(brush: &Brush, units: f32, small: f32) -> f32 {
    let speed = 0.3 * brush.setting("speed_size").abs();
    let share = speed + small + brush.setting("preview_size") / (units / 2.0).max(1.0);
    if share.is_finite() { share.clamp(0.0, 1.0) } else { 0.0 }
}

/// The diameter the sample stroke is drawn at (canvas pixels).
pub fn diameter(brush: &Brush) -> f32 {
    diameter_at_share(brush, size_share(brush))
}

/// Both parts of a combined preview receive the same size input and apply their own size range.
pub fn diameter_at_share(brush: &Brush, share: f32) -> f32 {
    let most = brush.biggest();
    let least = brush.min_size.min(most);
    (least + (most - least) * share).max(0.5)
}

/// The canvas (premultiplied RGBA8, [`CANVAS`] in size) fitted into a `width` × `height` preview:
/// as big as fits, in the middle, each pixel the average of the canvas pixels it covers.
pub fn fit(canvas: &[u8], width: u32, height: u32) -> Vec<u8> {
    fit_image(canvas, (CANVAS.0 as u32, CANVAS.1 as u32), width, height)
}

/// An image (premultiplied RGBA8, `size` big) fitted into a `width` × `height` preview, as
/// [`fit`] fits the canvas.
pub fn fit_image(canvas: &[u8], size: (u32, u32), width: u32, height: u32) -> Vec<u8> {
    let (sw, sh) = (size.0 as usize, size.1 as usize);
    let (w, h) = (width as usize, height as usize);
    let k = (width as f32 / size.0.max(1) as f32).min(height as f32 / size.1.max(1) as f32);
    // for each preview row or column, the canvas ones under it and how much of it each covers
    let spans = |n: usize, source: usize, fitted: f32| -> Vec<Vec<(usize, f32)>> {
        let start = (n as f32 - fitted) / 2.0;
        (0..n)
            .map(|o| {
                let (a, b) = ((o as f32 - start) / k, (o as f32 + 1.0 - start) / k);
                let (first, last) = (a.max(0.0).floor() as usize, (b.min(source as f32).ceil() as usize).min(source));
                (first..last)
                    .map(|i| (i, ((i as f32 + 1.0).min(b) - (i as f32).max(a)).max(0.0) * k))
                    .filter(|&(_, weight)| weight > 0.0)
                    .collect()
            })
            .collect()
    };
    let (columns, rows) = (spans(w, sw, sw as f32 * k), spans(h, sh, sh as f32 * k));
    let mut out = vec![0u8; w * h * 4];
    for (y, row) in rows.iter().enumerate() {
        for (x, column) in columns.iter().enumerate() {
            let mut sum = [0.0f32; 4];
            for &(sy, wy) in row {
                for &(sx, wx) in column {
                    let at = (sy * sw + sx) * 4;
                    if let Some(p) = canvas.get(at..at + 4) {
                        for (s, &v) in sum.iter_mut().zip(p) {
                            *s += f32::from(v) * wx * wy;
                        }
                    }
                }
            }
            let at = (y * w + x) * 4;
            if let Some(dst) = out.get_mut(at..at + 4) {
                for (d, s) in dst.iter_mut().zip(sum) {
                    *d = s.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wave_crosses_the_canvas() {
        let s = samples(&Brush::round_pen());
        assert_eq!(s.len(), SAMPLES);
        let (first, last) = (s.first().unwrap(), s.last().unwrap());
        assert!(first.pos.distance(Vec2::new(116.636_96, 185.393_37)) < 0.001);
        assert!(s[10].pos.distance(Vec2::new(346.883_88, 208.764_57)) < 0.001);
        assert!(last.pos.distance(Vec2::new(933.876_2, 158.966_75)) < 0.001);
        assert!(s.iter().all(|p| p.pos.y > 0.4 * CANVAS.1 && p.pos.y < 0.7 * CANVAS.1));
        assert!((first.pressure - 0.105925).abs() < 1e-5);
        assert_eq!(first.pressure, last.pressure);
        assert!((s[SAMPLES / 4].pressure - 0.591058).abs() < 1e-5);
        assert_eq!(s[SAMPLES / 2].pressure, 1.0);
        assert!((last.time_ms - 35.0 * STEP_MS).abs() < 1e-9);
    }

    #[test]
    fn preview_minimum_is_added_after_scaling_the_recorded_pressure() {
        let mut brush = Brush::round_pen();
        brush.set_setting("preview_pressure_scale", 0.75);
        brush.set_setting("preview_pressure_min", 0.2);
        assert!((pressure_at(&brush, 0.0) - (0.105925 * 1.5 + 0.2)).abs() < 1e-5);
        assert_eq!(pressure_at(&brush, 0.5), 1.0);
    }

    #[test]
    fn the_preview_s_pen_is_held_a_quarter_turn_round_then_turned_by_its_offset() {
        use std::f32::consts::FRAC_PI_2;
        let mut b = Brush::round_pen();
        assert!((samples(&b)[0].azimuth + FRAC_PI_2).abs() < 1e-6);
        b.set_setting("preview_tilt_offset", 1.0);
        b.set_setting("preview_tilt", 0.0);
        let flat = samples(&b);
        assert_eq!(flat[0].azimuth, 0.0);
        assert!(flat[1..].iter().all(|p| (p.azimuth - 0.053381).abs() < 1e-6));
        assert!(flat.iter().all(|p| p.tilt == 0.0));
        b.set_setting("preview_tilt", 1.0);
        let tilted = samples(&b);
        assert_eq!(tilted[0].tilt, FRAC_PI_2);
        assert!((tilted[SAMPLES - 1].tilt - FRAC_PI_2 * (1.0 - 0.0473245)).abs() < 1e-6);
    }

    #[test]
    fn small_brushes_show_bigger_and_big_ones_smaller() {
        let mut b = Brush::round_pen();
        b.set_setting("preview_size", 0.3);
        let unit = b.size_unit();
        // under 0.08 of a unit, a small brush is shown up to nearly its full size
        b.max_size = 0.04 * unit;
        assert!((size_share(&b) - (0.3 + 0.06 * 9.5)).abs() < 1e-4);
        b.max_size = 1.0 * unit;
        assert!((size_share(&b) - 0.49).abs() < 1e-4);
        // from 2 units the preview size is shared out over the size
        b.max_size = 8.0 * unit;
        assert!((size_share(&b) - 0.075).abs() < 1e-4);
    }

    #[test]
    fn the_drawing_pad_draws_at_its_preview_size_of_the_brush_s_range_however_big_its_most() {
        let mut b = Brush::round_pen();
        b.set_setting("preview_size", 0.3);
        let unit = b.size_unit();
        // the same share of a bigger most size: a bigger stroke on the pad
        for most in [0.5, 4.0, 10.0] {
            b.max_size = most * unit;
            assert!((pad_size_share(&b) - 0.3).abs() < 1e-6, "{most}");
        }
    }

    #[test]
    fn the_canvas_fits_in_the_middle() {
        let (sw, sh) = (CANVAS.0 as usize, CANVAS.1 as usize);
        // opaque white on the left half
        let canvas: Vec<u8> = (0..sw * sh).flat_map(|i| if i % sw < sw / 2 { [255u8; 4] } else { [0; 4] }).collect();
        // half the size: the halves stay halves
        let half = fit(&canvas, 530, 162);
        assert_eq!(half.len(), 530 * 162 * 4);
        assert_eq!(half.get((80 * 530 + 264) * 4..(80 * 530 + 265) * 4), Some(&[255u8; 4][..]));
        assert_eq!(half.get((80 * 530 + 265) * 4..(80 * 530 + 266) * 4), Some(&[0u8; 4][..]));
        // a taller preview: the canvas across its width, empty above and below
        let tall = fit(&canvas, 106, 100);
        let a = |x: usize, y: usize| tall.get((y * 106 + x) * 4 + 3).copied();
        assert_eq!(a(10, 2), Some(0));
        assert_eq!(a(10, 50), Some(255));
        assert_eq!(a(10, 97), Some(0));
    }
}

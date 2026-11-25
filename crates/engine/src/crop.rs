//! Cropping and resizing the canvas: where the new canvas's pixels come from on the old one, and
//! the pixels resampled.

use crate::document::canvas_edge_most;
use crate::geom::{Affine, Vec2};

/// A new canvas: its size, and the map from its pixels to the old canvas's.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crop {
    pub width: u32,
    pub height: u32,
    pub to_old: Affine,
}

/// Keeping the rectangle `rect` (x, y, width, height on the old canvas; it may reach past the
/// edges), turned by `angle` radians about its centre. `size` resamples what is kept to that many
/// pixels; otherwise it keeps a pixel for each of the old canvas's, the rectangle taken to whole
/// pixels (so an unturned crop only ever moves them: [`Crop::whole_pixels`]).
pub fn crop(rect: [f32; 4], angle: f32, size: Option<(u32, u32)>) -> Crop {
    let finite = |v: f32, or: f32| if v.is_finite() { v } else { or };
    let (mut x, mut y) = (finite(rect[0], 0.0), finite(rect[1], 0.0));
    let angle = finite(angle, 0.0);
    let most = canvas_edge_most();
    let max = most as f32;
    let (mut w, mut h) = (finite(rect[2], 1.0).clamp(1.0, max), finite(rect[3], 1.0).clamp(1.0, max));
    // kept at its size, every new pixel is one of the old canvas's: the rectangle goes onto whole
    // pixels (unturned, its sides; turned, its size about its middle). A side left between two
    // pixels would otherwise have each pixel blended with its neighbour and the whole a little
    // resized, which pixel art shows at once
    if size.is_none() {
        if angle == 0.0 {
            let (l, t) = (x.round(), y.round());
            (w, h) = (((x + w).round() - l).clamp(1.0, max), ((y + h).round() - t).clamp(1.0, max));
            (x, y) = (l, t);
        } else {
            let (cx, cy) = (x + w / 2.0, y + h / 2.0);
            (w, h) = (w.round(), h.round());
            (x, y) = (cx - w / 2.0, cy - h / 2.0);
        }
    }
    let (width, height) = size.unwrap_or((w as u32, h as u32));
    let (width, height) = (width.clamp(1, most), height.clamp(1, most));
    let (fw, fh) = (width as f32, height as f32);
    // a new pixel, about the new canvas's middle, scaled to the rectangle, turned, and placed
    let to_old = Affine::translate(Vec2::new(-fw / 2.0, -fh / 2.0))
        .then(Affine { a: w / fw, d: h / fh, ..Affine::IDENTITY })
        .then(Affine::rotate(angle))
        .then(Affine::translate(Vec2::new(x + w / 2.0, y + h / 2.0)));
    Crop { width, height, to_old }
}

impl Crop {
    /// Where the new canvas's top left is on the old one, when the crop only moves the pixels by
    /// whole pixels (each kept as it is); none when it turns or resizes them.
    pub fn whole_pixels(&self) -> Option<(i64, i64)> {
        let m = self.to_old;
        let whole = |v: f32| (v - v.round()).abs() < 1e-4;
        (m.a == 1.0 && m.b == 0.0 && m.c == 0.0 && m.d == 1.0 && whole(m.e) && whole(m.f))
            .then(|| (m.e.round() as i64, m.f.round() as i64))
    }
}

/// `src` (premultiplied RGBA8, `sw` × `sh`) seen through `crop`: bilinear, transparent past its
/// edges. A move by whole pixels copies them as they are.
pub fn resample(src: &[u8], sw: u32, sh: u32, crop: &Crop) -> Vec<u8> {
    resampled(src, sw, sh, crop, false)
}

/// As [`resample`], for pixel art: each new pixel is the one old pixel its middle falls in, so
/// nothing is blended. Its edges stay hard and its colours the ones it had, at any size or turn.
pub fn resample_pixelated(src: &[u8], sw: u32, sh: u32, crop: &Crop) -> Vec<u8> {
    resampled(src, sw, sh, crop, true)
}

fn resampled(src: &[u8], sw: u32, sh: u32, crop: &Crop, pixelated: bool) -> Vec<u8> {
    let (w, h) = (crop.width as usize, crop.height as usize);
    let mut out = vec![0u8; w * h * 4];
    let m = crop.to_old;
    if let Some((dx, dy)) = crop.whole_pixels() {
        for y in 0..h {
            let sy = y as i64 + dy;
            if sy < 0 || sy >= i64::from(sh) {
                continue;
            }
            // the part of the row that lands on the old canvas
            let x0 = (-dx).clamp(0, w as i64) as usize;
            let x1 = (i64::from(sw) - dx).clamp(0, w as i64) as usize;
            if x1 <= x0 {
                continue;
            }
            let from = ((sy as usize) * sw as usize + (x0 as i64 + dx) as usize) * 4;
            let to = (y * w + x0) * 4;
            let n = (x1 - x0) * 4;
            if let (Some(dst), Some(s)) = (out.get_mut(to..to + n), src.get(from..from + n)) {
                dst.copy_from_slice(s);
            }
        }
        return out;
    }
    let pixel = |x: i64, y: i64| -> [f32; 4] {
        if x < 0 || y < 0 || x >= i64::from(sw) || y >= i64::from(sh) {
            return [0.0; 4];
        }
        let i = ((y as usize) * sw as usize + x as usize) * 4;
        src.get(i..i + 4).and_then(|p| <[u8; 4]>::try_from(p).ok()).map_or([0.0; 4], |p| p.map(f32::from))
    };
    // the rows made side by side
    let rows: Vec<usize> = (0..h).collect();
    let made = crate::par::side_by_side(&rows, |&y| {
        let mut line = vec![0u8; w * 4];
        for (x, dst) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let p = m.apply(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
            if pixelated {
                *dst = pixel(p.x.floor() as i64, p.y.floor() as i64).map(|v| v as u8);
                continue;
            }
            let (fx, fy) = (p.x - 0.5, p.y - 0.5);
            let (x0, y0) = (fx.floor(), fy.floor());
            let (tx, ty) = (fx - x0, fy - y0);
            let (ix, iy) = (x0 as i64, y0 as i64);
            let (a, b, c, d) = (pixel(ix, iy), pixel(ix + 1, iy), pixel(ix, iy + 1), pixel(ix + 1, iy + 1));
            for (k, v) in dst.iter_mut().enumerate() {
                let at = |p: [f32; 4]| p.get(k).copied().unwrap_or(0.0);
                let top = at(a) + (at(b) - at(a)) * tx;
                let bottom = at(c) + (at(d) - at(c)) * tx;
                *v = (top + (bottom - top) * ty).round().clamp(0.0, 255.0) as u8;
            }
        }
        line
    });
    for (line, row) in made.into_iter().zip(out.chunks_exact_mut((w * 4).max(1))) {
        if let Some(line) = line {
            row.copy_from_slice(&line);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(w: u32, h: u32) -> Vec<u8> {
        (0..w * h).flat_map(|i| [(i % w) as u8 * 10, (i / w) as u8 * 10, 0, 255]).collect()
    }

    #[test]
    fn a_crop_keeps_the_rectangle() {
        let src = picture(8, 6);
        let c = crop([2.0, 1.0, 4.0, 3.0], 0.0, None);
        assert_eq!((c.width, c.height), (4, 3));
        let out = resample(&src, 8, 6, &c);
        // the new top left is the old (2, 1)
        assert_eq!(&out[..4], &[20, 10, 0, 255]);
        assert_eq!(&out[(2 * 4 + 3) * 4..(2 * 4 + 3) * 4 + 4], &[50, 30, 0, 255]);
    }

    #[test]
    fn past_the_edges_is_transparent_and_resampling_scales() {
        let src = picture(4, 4);
        let c = crop([-2.0, 0.0, 6.0, 4.0], 0.0, None);
        let out = resample(&src, 4, 4, &c);
        assert_eq!(c.width, 6);
        assert_eq!(&out[..4], &[0, 0, 0, 0]);
        assert_eq!(&out[2 * 4..2 * 4 + 4], &[0, 0, 0, 255]);
        // the whole of it at twice the size: a flat colour stays flat
        let flat = [9u8, 9, 9, 255].repeat(16);
        let c = crop([0.0, 0.0, 4.0, 4.0], 0.0, Some((8, 8)));
        let out = resample(&flat, 4, 4, &c);
        assert_eq!(out.len(), 8 * 8 * 4);
        assert_eq!(&out[(3 * 8 + 3) * 4..(3 * 8 + 3) * 4 + 4], &[9, 9, 9, 255]);
    }

    #[test]
    fn a_rectangle_between_pixels_keeps_each_pixel_as_it_was() {
        let src = picture(8, 6);
        // its sides go to the nearest whole pixels: 1 to 6 across, 1 to 4 down
        let c = crop([1.3, 0.6, 4.4, 3.2], 0.0, None);
        assert_eq!((c.width, c.height), (5, 3));
        assert_eq!(c.whole_pixels(), Some((1, 1)));
        let out = resample(&src, 8, 6, &c);
        for (i, px) in out.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (i as u32 % 5 + 1, i as u32 / 5 + 1);
            assert_eq!(px, &[x as u8 * 10, y as u8 * 10, 0, 255], "{x}, {y}");
        }
        // turned, it is still a pixel for a pixel (no bigger, no smaller), about the same middle
        let c = crop([1.3, 0.6, 4.4, 3.2], 0.3, None);
        assert_eq!((c.width, c.height), (4, 3));
        let m = c.to_old;
        assert!((m.a.hypot(m.b) - 1.0).abs() < 1e-6 && (m.c.hypot(m.d) - 1.0).abs() < 1e-6);
        let mid = m.apply(Vec2::new(2.0, 1.5));
        assert!((mid.x - 3.5).abs() < 1e-4 && (mid.y - 2.2).abs() < 1e-4);
        // resampled to a size, the rectangle is taken as it is
        let c = crop([1.5, 0.5, 4.0, 3.0], 0.0, Some((8, 6)));
        assert_eq!(c.to_old.apply(Vec2::new(0.0, 0.0)), Vec2::new(1.5, 0.5));
    }

    #[test]
    fn pixelated_takes_whole_pixels_and_blends_nothing() {
        // two colours side by side, as pixel art has them
        let src: Vec<u8> =
            (0..4 * 4).flat_map(|i| if i % 4 < 2 { [200, 0, 0, 255] } else { [0, 0, 200, 255] }).collect();
        let only_its_colours = |out: &[u8]| {
            out.as_chunks::<4>().0.iter().all(|p| [[200, 0, 0, 255], [0, 0, 200, 255], [0; 4]].contains(p))
        };
        // three times the size: every pixel a square of nine
        let c = crop([0.0, 0.0, 4.0, 4.0], 0.0, Some((12, 12)));
        let out = resample_pixelated(&src, 4, 4, &c);
        for (i, px) in out.as_chunks::<4>().0.iter().enumerate() {
            let (x, y) = (i % 12 / 3, i / 12 / 3);
            assert_eq!(px[..], src[(y * 4 + x) * 4..(y * 4 + x) * 4 + 4], "{i}");
        }
        assert!(!only_its_colours(&resample(&src, 4, 4, &c)), "smoothed, the two blend where they meet");
        // half the size, an odd size, turned: still only its own colours
        for (size, angle) in [(Some((2, 2)), 0.0), (Some((7, 5)), 0.0), (None, 0.4), (Some((9, 6)), -1.1)] {
            let c = crop([0.0, 0.0, 4.0, 4.0], angle, size);
            let out = resample_pixelated(&src, 4, 4, &c);
            assert!(only_its_colours(&out), "{size:?} {angle}");
            assert!(
                out.as_chunks::<4>().0.iter().any(|p| p[0] == 200)
                    && out.as_chunks::<4>().0.iter().any(|p| p[2] == 200)
            );
        }
        // a move by whole pixels is the same either way
        let c = crop([1.0, 1.0, 2.0, 3.0], 0.0, None);
        assert_eq!(resample_pixelated(&src, 4, 4, &c), resample(&src, 4, 4, &c));
    }

    #[test]
    fn a_half_turn_reverses_the_picture() {
        let src = picture(4, 2);
        let c = crop([0.0, 0.0, 4.0, 2.0], std::f32::consts::PI, None);
        let out = resample(&src, 4, 2, &c);
        // the new top left is the old bottom right
        assert_eq!(&out[..4], &[30, 10, 0, 255]);
    }
}

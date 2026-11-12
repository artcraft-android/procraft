//! Pixel art: a pixelated brush's stamps laid on whole pixels at whole-pixel sizes, and the
//! pixel perfect filter that keeps a freehand one-pixel line one pixel thick.
//!
//! A stamp of an odd number of pixels across sits in the middle of a pixel, one of an even number
//! on a corner between four: either way it covers whole pixels only. The stamp shader snaps them
//! the same way (for stamps mirrored off the grid), and draws them hard-edged.

use crate::geom::Vec2;
use crate::stroke::Stamp;

/// `s` laid on the pixel grid: its width a whole number of pixels (at least one), its middle where
/// a stamp that wide covers whole pixels.
pub fn snapped(mut s: Stamp) -> Stamp {
    if !(s.center.is_finite() && s.radius.is_finite()) {
        return s;
    }
    let across = (s.radius * 2.0).round().max(1.0);
    s.radius = across * 0.5;
    s.center = if across % 2.0 == 1.0 {
        Vec2::new(s.center.x.floor() + 0.5, s.center.y.floor() + 0.5)
    } else {
        Vec2::new(s.center.x.round(), s.center.y.round())
    };
    s
}

/// The pixel a one-pixel stamp covers, if it is one.
fn single_pixel(s: &Stamp) -> Option<[i32; 2]> {
    let one = s.center.is_finite() && (s.radius * 2.0).round() <= 1.0;
    one.then(|| [s.center.x.floor() as i32, s.center.y.floor() as i32])
}

/// Three pixels of a line in a row make an L-shaped corner when the middle one touches each of
/// the others along a side and those two touch each other only at a corner: the line reads as a
/// clean diagonal step without the middle one.
pub(crate) fn is_elbow(before: [i32; 2], middle: [i32; 2], after: [i32; 2]) -> bool {
    let a = [before[0] - middle[0], before[1] - middle[1]];
    let b = [after[0] - middle[0], after[1] - middle[1]];
    (a[0] == 0 || a[1] == 0) && (b[0] == 0 || b[1] == 0) && (a[0] + b[0]).abs() == 1 && (a[1] + b[1]).abs() == 1
}

/// The pixel perfect filter, as stamps come along a stroke (snapped first; see [`snapped`]): a
/// one-pixel stamp on the pixel the last one covered is left out, and so is the corner of an L
/// (see [`is_elbow`]). A stamp can't be taken back once drawn, so the newest one-pixel stamp is
/// held until the next shows whether it is a corner; wider stamps pass straight through.
#[derive(Clone, Debug, Default)]
pub struct PixelPerfect {
    /// The pixel of the last one-pixel stamp let through.
    last: Option<[i32; 2]>,
    held: Option<Stamp>,
}

impl PixelPerfect {
    /// Takes the stroke's next stamp, putting those now let through for good on `out`.
    pub fn push(&mut self, s: Stamp, out: &mut Vec<Stamp>) {
        let Some(pixel) = single_pixel(&s) else {
            out.extend(self.held.take());
            self.last = None;
            out.push(s);
            return;
        };
        let Some(held) = self.held else {
            if self.last != Some(pixel) {
                self.held = Some(s);
            }
            return;
        };
        // the held one is one pixel (only those are held)
        let held_pixel = single_pixel(&held).unwrap_or(pixel);
        if pixel == held_pixel || self.last == Some(pixel) {
            return;
        }
        if self.last.is_some_and(|last| is_elbow(last, held_pixel, pixel)) {
            self.held = Some(s);
            return;
        }
        out.push(held);
        self.last = Some(held_pixel);
        self.held = Some(s);
    }

    /// The stamp held back, to show until the next decides it (and to lay as the stroke ends).
    pub fn held(&self) -> Option<Stamp> {
        self.held
    }
}

/// A whole stroke's stamps through a fresh [`PixelPerfect`]: those it lets through, and the one
/// it still holds if `with_held` (as the stroke ends).
pub fn perfect(stamps: impl IntoIterator<Item = Stamp>, with_held: bool) -> Vec<Stamp> {
    let mut filter = PixelPerfect::default();
    let mut out = Vec::new();
    for s in stamps {
        filter.push(s, &mut out);
    }
    if with_held {
        out.extend(filter.held());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(x: f32, y: f32, across: f32) -> Stamp {
        Stamp::round(Vec2::new(x, y), across * 0.5, 1.0)
    }

    fn pixels(stamps: &[Stamp]) -> Vec<[i32; 2]> {
        stamps.iter().map(|s| [s.center.x.floor() as i32, s.center.y.floor() as i32]).collect()
    }

    #[test]
    fn odd_widths_sit_mid_pixel_and_even_on_corners() {
        let one = snapped(at(3.9, 7.2, 0.7));
        assert_eq!((one.center, one.radius), (Vec2::new(3.5, 7.5), 0.5));
        let two = snapped(at(3.9, 7.2, 2.2));
        assert_eq!((two.center, two.radius), (Vec2::new(4.0, 7.0), 1.0));
        let three = snapped(at(3.9, 7.2, 3.4));
        assert_eq!((three.center, three.radius), (Vec2::new(3.5, 7.5), 1.5));
        // snapping again changes nothing (the shader snaps what is already snapped)
        for s in [one, two, three] {
            assert_eq!(snapped(s), s);
        }
        // never under a pixel
        assert_eq!(snapped(at(0.0, 0.0, 0.1)).radius, 0.5);
    }

    #[test]
    fn doubled_pixels_are_left_out() {
        let stamps = [at(0.2, 0.5, 1.0), at(0.6, 0.5, 1.0), at(0.9, 0.5, 1.0), at(1.3, 0.5, 1.0)].map(snapped);
        assert_eq!(pixels(&perfect(stamps, true)), vec![[0, 0], [1, 0]]);
    }

    #[test]
    fn l_corners_are_left_out_of_a_diagonal() {
        // a staircase drawn with a corner pixel at each step
        let path = [[0, 0], [1, 0], [1, 1], [2, 1], [2, 2], [3, 2], [3, 3]];
        let stamps = path.map(|[x, y]| snapped(at(x as f32 + 0.5, y as f32 + 0.5, 1.0)));
        assert_eq!(pixels(&perfect(stamps, true)), vec![[0, 0], [1, 1], [2, 2], [3, 3]]);
    }

    #[test]
    fn straight_runs_keep_every_pixel_and_a_turn_loses_its_corner() {
        let path = [[0, 0], [1, 0], [2, 0], [3, 0], [3, 1], [3, 2]];
        let stamps = path.map(|[x, y]| snapped(at(x as f32 + 0.5, y as f32 + 0.5, 1.0)));
        // the corner at (3, 0) is an L like any other: its neighbours touch diagonally without it
        let kept = pixels(&perfect(stamps, true));
        assert_eq!(kept, vec![[0, 0], [1, 0], [2, 0], [3, 1], [3, 2]]);
        // a straight run keeps every pixel
        let run = [[0, 0], [1, 0], [2, 0], [3, 0]].map(|[x, y]| snapped(at(x as f32 + 0.5, y as f32 + 0.5, 1.0)));
        assert_eq!(pixels(&perfect(run, true)), vec![[0, 0], [1, 0], [2, 0], [3, 0]]);
    }

    #[test]
    fn wider_stamps_pass_through() {
        let stamps = [at(0.5, 0.5, 3.0), at(0.5, 0.5, 3.0), at(1.5, 0.5, 3.0)].map(snapped);
        assert_eq!(perfect(stamps, true), stamps.to_vec());
    }

    #[test]
    fn streaming_lets_through_what_the_whole_stroke_does() {
        let path = [[0, 0], [1, 0], [1, 1], [1, 1], [2, 1], [2, 2], [3, 2], [4, 2], [4, 3]];
        let stamps: Vec<Stamp> = path.iter().map(|&[x, y]| snapped(at(x as f32 + 0.5, y as f32 + 0.5, 1.0))).collect();
        let mut filter = PixelPerfect::default();
        let mut streamed = Vec::new();
        for (i, s) in stamps.iter().enumerate() {
            filter.push(*s, &mut streamed);
            // what has been let through is always the start of the whole stroke's
            assert_eq!(streamed, perfect(stamps[..=i].iter().copied(), false));
        }
        streamed.extend(filter.held());
        assert_eq!(streamed, perfect(stamps, true));
    }

    #[test]
    fn a_tap_keeps_its_pixel() {
        assert_eq!(pixels(&perfect([snapped(at(5.2, 5.2, 1.0))], true)), vec![[5, 5]]);
        assert!(perfect([snapped(at(5.2, 5.2, 1.0))], false).is_empty());
    }
}

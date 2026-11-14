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

/// A path drawn by hand as pixel art takes it: each point on the nearest corner between pixels,
/// and from each to the next along the pixels' edges, a pixel across or down at a time (whichever
/// keeps nearer the straight line between them), so no stretch of it slants across a pixel however
/// far apart the points came. `closed`, it goes on the same way back to where it began. Only its
/// turns are kept. The pixels inside it are then just the ones it goes round.
pub fn stepped(points: &[Vec2], closed: bool) -> Vec<Vec2> {
    let mut corners = points.iter().filter(|p| p.is_finite()).map(|p| (p.x.round() as i64, p.y.round() as i64));
    let Some(first) = corners.next() else { return Vec::new() };
    let mut out = vec![first];
    let mut at = first;
    for to in corners.chain(closed.then_some(first)) {
        let (across, down) = ((to.0 - at.0).abs(), (to.1 - at.1).abs());
        let (dx, dy) = ((to.0 - at.0).signum(), (to.1 - at.1).signum());
        let (mut i, mut j) = (0, 0);
        while i < across || j < down {
            // the step whose middle comes first along the line
            if j >= down || (i < across && (1 + 2 * i) * down < (1 + 2 * j) * across) {
                at.0 += dx;
                i += 1;
            } else {
                at.1 += dy;
                j += 1;
            }
            // a step straight on from the last (not one doubling back on it) only moves its end
            let n = out.len();
            let straight = match (n.checked_sub(2).and_then(|k| out.get(k)), out.last()) {
                (Some(a), Some(b)) => {
                    (a.0 == b.0 && b.0 == at.0 && (b.1 - a.1).signum() == (at.1 - b.1).signum())
                        || (a.1 == b.1 && b.1 == at.1 && (b.0 - a.0).signum() == (at.0 - b.0).signum())
                }
                _ => false,
            };
            if straight {
                if let Some(last) = out.last_mut() {
                    *last = at;
                }
            } else {
                out.push(at);
            }
        }
    }
    out.into_iter().map(|(x, y)| Vec2::new(x as f32, y as f32)).collect()
}

/// A picture (`width` × `height` RGBA pixels, rows top to bottom) twice as large each way by
/// Scale2x's rule: each pixel becomes four, a corner of it taking a neighbour's colour where the
/// two neighbours beside that corner are alike and unlike the ones across from them, which carries
/// a diagonal's steps on instead of doubling them. Outside the picture counts as clear.
pub fn grown(pixels: &[[u8; 4]], width: usize, height: usize) -> Vec<[u8; 4]> {
    let at = |x: isize, y: isize| -> [u8; 4] {
        if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
            return [0; 4];
        }
        pixels.get(y as usize * width + x as usize).copied().unwrap_or([0; 4])
    };
    let mut out = vec![[0u8; 4]; width * 2 * height * 2];
    for (i, o) in out.iter_mut().enumerate() {
        let (ux, uy) = ((i % (width * 2)) as isize, (i / (width * 2)) as isize);
        let (x, y) = (ux >> 1, uy >> 1);
        let (p, a, b, c, d) = (at(x, y), at(x, y - 1), at(x + 1, y), at(x - 1, y), at(x, y + 1));
        *o = quarter(p, a, b, c, d, ux & 1 == 1, uy & 1 == 1);
    }
    out
}

/// The quarter of pixel `p` to the `right` or left and `low` or high, between `a` above it, `b` to
/// its right, `c` to its left and `d` below.
fn quarter(p: [u8; 4], a: [u8; 4], b: [u8; 4], c: [u8; 4], d: [u8; 4], right: bool, low: bool) -> [u8; 4] {
    match (right, low) {
        (false, false) if c == a && c != d && a != b => a,
        (true, false) if a == b && a != c && b != d => b,
        (false, true) if d == c && d != b && c != a => c,
        (true, true) if b == d && b != a && d != c => d,
        _ => p,
    }
}

/// The picture a clean rotation reads: enlarged by [`grown`] `levels` times over (2, 4, 8 times
/// as large each way); what turns it then takes the enlarged pixel under each of its own pixels'
/// middles (RotSprite's way).
pub fn grown_by(pixels: &[[u8; 4]], width: usize, height: usize, levels: u32) -> Vec<[u8; 4]> {
    let mut picture = pixels.to_vec();
    let (mut w, mut h) = (width, height);
    for _ in 0..levels {
        picture = grown(&picture, w, h);
        w *= 2;
        h *= 2;
    }
    picture
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

    #[test]
    fn growing_keeps_a_lone_pixel_square_and_carries_a_diagonal_on() {
        let (o, x) = ([0u8; 4], [9u8, 9, 9, 255]);
        let lit = |picture: &[[u8; 4]]| -> Vec<usize> {
            picture.iter().enumerate().filter(|(_, p)| **p == x).map(|(i, _)| i).collect()
        };
        // one pixel alone: four of it
        assert_eq!(lit(&grown(&[o, o, o, o, x, o, o, o, o], 3, 3)), [14, 15, 20, 21]);
        // two corner to corner: the corners between them filled in, so the line goes on in steps
        // of one, not of two
        assert_eq!(lit(&grown(&[x, o, o, x], 2, 2)), [0, 1, 4, 5, 6, 9, 10, 11, 14, 15]);
        // three times over: eight times as large each way, a block's middle as it was (its outer
        // corners are rounded off, as the rule does to a corner that sticks out)
        let big = grown_by(&[x], 1, 1, 3);
        assert_eq!(big.len(), 64);
        assert!([27, 28, 35, 36].iter().all(|&i| big[i] == x));
        assert_eq!(big[0], o);
        assert_eq!(grown_by(&[x, o], 2, 1, 0), [x, o]);
    }

    #[test]
    fn a_stepped_path_keeps_to_the_pixels_edges() {
        // far apart and slanting: a pixel across or down at a time between them
        let path = stepped(&[Vec2::new(0.2, 0.1), Vec2::new(7.8, 3.1), Vec2::new(2.4, 9.6)], true);
        assert_eq!(path.first(), Some(&Vec2::new(0.0, 0.0)));
        assert_eq!(path.last(), Some(&Vec2::new(0.0, 0.0)), "closed back to where it began");
        // through each point's corner (which is in it only where it turns there)
        let through = |c: Vec2| {
            path.windows(2).any(|w| {
                (w[0].x.min(w[1].x)..=w[0].x.max(w[1].x)).contains(&c.x)
                    && (w[0].y.min(w[1].y)..=w[0].y.max(w[1].y)).contains(&c.y)
            })
        };
        assert!(through(Vec2::new(8.0, 3.0)) && through(Vec2::new(2.0, 10.0)));
        for pair in path.windows(2) {
            let (a, b) = (pair[0], pair[1]);
            assert!(a != b && (a.x == b.x || a.y == b.y), "{a:?} {b:?}");
            assert!(a.x.fract() == 0.0 && a.y.fract() == 0.0);
        }
        // it keeps by the straight line: never more than a pixel from it
        let open = stepped(&[Vec2::new(0.0, 0.0), Vec2::new(20.0, 7.0)], false);
        assert_eq!(open.last(), Some(&Vec2::new(20.0, 7.0)));
        for p in &open {
            assert!((p.y - p.x * 7.0 / 20.0).abs() <= 1.0, "{p:?}");
        }
        // straight along the grid, only its ends; a point alone, itself; nothing, nothing
        assert_eq!(
            stepped(&[Vec2::new(1.0, 1.0), Vec2::new(9.0, 1.0)], false),
            [Vec2::new(1.0, 1.0), Vec2::new(9.0, 1.0)]
        );
        assert_eq!(stepped(&[Vec2::new(3.4, 3.6)], true), [Vec2::new(3.0, 4.0)]);
        assert!(stepped(&[], true).is_empty());
    }
}

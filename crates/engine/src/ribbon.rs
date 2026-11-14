//! Ribbon brushes: the brush's shape image drawn as one strip along the stroke rather than stamped
//! again and again. The image's width runs along the stroke and its height across it, its tiles
//! following on from each other the length of the stroke (chains, ropes, lace, stitches).
//!
//! The stroke's stamps are the strip's cross-sections, its rungs: a stamp's middle is the strip's
//! middle there, its size the strip's width (so pressure, speed, tilt and taper thin the strip as
//! they would the stamps) and its alpha how much paint the strip lays there. Between each two rungs
//! the strip is a piece of four corners, which the GPU draws as two triangles reading the shape
//! image along it.

use crate::geom::Vec2;
use crate::stroke::Stamp;

/// A cross-section of a ribbon.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rung {
    /// The strip's middle there (canvas pixels).
    pub center: Vec2,
    /// From the middle to the strip's left edge as the stroke goes (where the image's top is): as
    /// long as half the strip's width.
    pub across: Vec2,
    /// How much paint the strip lays there, 0..1.
    pub alpha: f32,
    /// How far along the strip it is, in tiles of the shape image: the whole part counts the
    /// tiles before it, the rest is where across the image's width it falls.
    pub along: f32,
}

/// A piece of ribbon between two rungs, laid for stamp `stamp` (its place among those given).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Piece {
    pub from: Rung,
    pub to: Rung,
    pub stamp: usize,
}

/// A ribbon as it is laid, a few stamps at a time: each lot of pieces goes on from where the last
/// left off, the same rung and the same place in the image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ribbon {
    /// How long a tile of the image is along the strip, in the strip's widths.
    tile: f32,
    /// The last rung laid, and whether it knows which way the strip goes yet (a stroke's first
    /// doesn't until the next comes).
    last: Option<Rung>,
    turned: bool,
}

/// The least a stamp moves on from the last rung to make a rung of its own (canvas pixels): any
/// nearer and it only gives the last its width and alpha.
const LEAST_STEP: f32 = 0.01;

/// The shortest a tile of the image is along the strip (canvas pixels), however thin the strip.
const LEAST_TILE: f32 = 1.0;

impl Ribbon {
    /// A ribbon whose image is `tile` of the strip's widths long along it (1 keeps a square
    /// image's proportions); nothing laid yet.
    pub fn new(tile: f32) -> Ribbon {
        let tile = if tile.is_finite() { tile.clamp(0.01, 100.0) } else { 1.0 };
        Ribbon { tile, last: None, turned: false }
    }

    /// How long a tile of the image is along the strip, in the strip's widths.
    pub fn tile(&self) -> f32 {
        self.tile
    }

    /// The last rung laid, if any.
    pub fn last(&self) -> Option<Rung> {
        self.last
    }

    /// The pieces `stamps` lay on from where the ribbon has reached, each stamp the rung that
    /// ends one (stamps not finite are passed over). The strip's sides at a rung are square to
    /// the way the stroke came to it, so a piece's end and the next one's start are the same.
    pub fn extend(&mut self, stamps: &[Stamp]) -> Vec<Piece> {
        let mut pieces = Vec::with_capacity(stamps.len());
        for (i, s) in stamps.iter().enumerate() {
            if !(s.center.is_finite() && s.radius.is_finite() && s.alpha.is_finite()) {
                continue;
            }
            let half = s.radius.max(0.0);
            let alpha = s.alpha.clamp(0.0, 1.0);
            let Some(last) = self.last else {
                // up the canvas until the stroke goes somewhere
                self.last = Some(Rung { center: s.center, across: Vec2::new(0.0, -half), alpha, along: 0.0 });
                self.turned = false;
                continue;
            };
            let step = s.center - last.center;
            let ds = step.length();
            if ds.is_nan() || ds <= LEAST_STEP {
                // no further on: the rung there takes this one's width and alpha
                let across = last.across.normalized_or(Vec2::new(0.0, -1.0)) * half;
                self.last = Some(Rung { across, alpha, ..last });
                continue;
            }
            let way = step * (1.0 / ds);
            let left = Vec2::new(way.y, -way.x);
            let last_half = last.across.length();
            // the first rung goes the way the strip does from it
            let from = if self.turned { last } else { Rung { across: left * last_half, ..last } };
            // a tile as long as its share of the strip's width here (pressure narrows the tiles
            // with the strip, so they keep their proportions)
            let length = (self.tile * (last_half + half)).max(LEAST_TILE);
            let to = Rung { center: s.center, across: left * half, alpha, along: from.along + ds / length };
            pieces.push(Piece { from, to, stamp: i });
            self.last = Some(to);
            self.turned = true;
        }
        pieces
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stamp(x: f32, y: f32, radius: f32) -> Stamp {
        Stamp::round(Vec2::new(x, y), radius, 1.0)
    }

    #[test]
    fn a_straight_ribbon_is_one_strip_of_pieces_end_to_end() {
        let mut ribbon = Ribbon::new(1.0);
        let pieces = ribbon.extend(&[stamp(0.0, 0.0, 5.0), stamp(4.0, 0.0, 5.0), stamp(8.0, 0.0, 5.0)]);
        assert_eq!(pieces.len(), 2);
        // left of a stroke going right (y down the canvas) is up
        let first = pieces.first().unwrap();
        assert_eq!(first.from.across, Vec2::new(0.0, -5.0));
        assert_eq!(first.to.across, Vec2::new(0.0, -5.0));
        assert_eq!(first.stamp, 1);
        // each piece starts where the last ended, a tile ten pixels long (the strip's width)
        let second = pieces.get(1).unwrap();
        assert_eq!(second.from, first.to);
        assert!((first.to.along - 0.4).abs() < 1e-6);
        assert!((second.to.along - 0.8).abs() < 1e-6);
    }

    #[test]
    fn a_ribbon_goes_on_from_lot_to_lot() {
        let stamps: Vec<Stamp> = (0..12).map(|i| stamp(i as f32 * 3.0, (i as f32 * 0.4).sin() * 9.0, 4.0)).collect();
        let mut whole = Ribbon::new(2.0);
        let at_once = whole.extend(&stamps);
        let mut lots = Ribbon::new(2.0);
        let (a, b) = stamps.split_at(5);
        let mut in_lots = lots.extend(a);
        in_lots.extend(lots.extend(b).into_iter().map(|p| Piece { stamp: p.stamp + 5, ..p }));
        assert_eq!(at_once, in_lots);
        assert_eq!(whole, lots);
    }

    #[test]
    fn a_thinner_strip_has_shorter_tiles() {
        // the tile is as long as the strip is wide (times the scale), so a strip half as wide goes
        // through its tiles twice as fast
        let mut wide = Ribbon::new(1.0);
        let mut thin = Ribbon::new(1.0);
        let w = wide.extend(&[stamp(0.0, 0.0, 10.0), stamp(10.0, 0.0, 10.0)]);
        let t = thin.extend(&[stamp(0.0, 0.0, 5.0), stamp(10.0, 0.0, 5.0)]);
        let along = |p: &[Piece]| p.first().map_or(0.0, |p| p.to.along);
        assert!((along(&t) - 2.0 * along(&w)).abs() < 1e-6);
        // and a longer scale, longer tiles
        let mut long = Ribbon::new(4.0);
        let l = long.extend(&[stamp(0.0, 0.0, 10.0), stamp(10.0, 0.0, 10.0)]);
        assert!((along(&w) - 4.0 * along(&l)).abs() < 1e-6);
    }

    #[test]
    fn a_stamp_in_place_lays_nothing_but_sets_the_width() {
        let mut ribbon = Ribbon::new(1.0);
        assert!(ribbon.extend(&[stamp(0.0, 0.0, 5.0), stamp(0.0, 0.0, 8.0)]).is_empty());
        let pieces = ribbon.extend(&[stamp(0.0, 6.0, 8.0)]);
        // going down the canvas: left is to the right of the screen
        assert_eq!(pieces.first().map(|p| p.from.across), Some(Vec2::new(8.0, 0.0)));
        // a stamp that isn't a number is passed over
        let mut bad = stamp(f32::NAN, 0.0, 3.0);
        bad.alpha = 0.5;
        assert!(ribbon.extend(&[bad]).is_empty());
        assert_eq!(ribbon.last().map(|r| r.center), Some(Vec2::new(0.0, 6.0)));
    }

    #[test]
    fn the_strip_turns_square_to_the_way_it_comes() {
        let mut ribbon = Ribbon::new(1.0);
        let pieces = ribbon.extend(&[stamp(0.0, 0.0, 2.0), stamp(3.0, 3.0, 2.0)]);
        let across = pieces.first().map_or(Vec2::ZERO, |p| p.to.across);
        assert!((across.length() - 2.0).abs() < 1e-5);
        assert!(across.dot(Vec2::new(1.0, 1.0)).abs() < 1e-5);
        assert!(across.x > 0.0 && across.y < 0.0);
    }
}

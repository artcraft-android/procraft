//! The animatic detector: where a video of held drawings (a storyboard or animatic export)
//! changes drawing, so a track of new drawings can be made with its timing (Koi Studio's "Make
//! Timing Track").
//!
//! Each frame a video clip covers is sampled at the time it shows there, shrunk to a small grey
//! picture (on the platform, which decodes the video) and compared with the one before it. The
//! per-frame scores are worked out once; [`segments`] turns them into holds for any threshold, so
//! a sensitivity control can re-cut them at once.

use std::ops::Range;

/// The longest side of the grey pictures compared.
pub const THUMBNAIL_SIDE: u32 = 128;
/// How far (0 to 255) a pixel's lightness must move to count as a change rather than
/// compression noise.
pub const PIXEL_TOLERANCE: u8 = 16;
/// The most frames the shortest hold can be set to.
pub const MAX_HOLD: u32 = 24;

/// A small grey picture: `width` × `height` lightness bytes, row by row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Luma {
    pub width: u32,
    pub height: u32,
    pub luma: Vec<u8>,
}

/// The share of pixels (0 to 1) whose lightness moved by more than [`PIXEL_TOLERANCE`]. Pictures
/// of different sizes count as wholly changed.
pub fn change_fraction(a: &Luma, b: &Luma) -> f32 {
    if a.width != b.width || a.height != b.height || a.luma.is_empty() || a.luma.len() != b.luma.len() {
        return 1.0;
    }
    let changed = a.luma.iter().zip(&b.luma).filter(|(x, y)| x.abs_diff(**y) > PIXEL_TOLERANCE).count();
    changed as f32 / a.luma.len() as f32
}

/// Change scores for a run of sampled frames: the first is always 1 (the first drawing), a frame
/// that couldn't be read scores 0 (it goes on with the hold before it), and each other its change
/// from the last frame that could be read.
pub fn change_scores(frames: &[Option<Luma>]) -> Vec<f32> {
    let mut previous: Option<&Luma> = None;
    frames
        .iter()
        .enumerate()
        .map(|(i, frame)| {
            let score = match (i, frame, previous) {
                (0, ..) => 1.0,
                (_, Some(f), Some(p)) => change_fraction(p, f),
                _ => 0.0,
            };
            if let Some(f) = frame {
                previous = Some(f);
            }
            score
        })
        .collect()
}

/// The score a change must reach for a sensitivity from 0 (only big changes, like cuts) to 1 (any
/// small change), spread evenly on a log scale.
pub fn threshold(sensitivity: f32) -> f32 {
    let (low, high) = (0.3_f32, 0.0005_f32);
    let s = if sensitivity.is_finite() { sensitivity.clamp(0.0, 1.0) } else { 0.5 };
    low * (high / low).powf(s)
}

/// Holds from per-frame change scores: a new drawing starts wherever a score reaches `threshold`.
/// A change less than `minimum_hold` frames after the last one kept is folded into it (camera
/// moves and dissolves would otherwise make a drawing a frame), and so is a last hold shorter
/// than that. Frame 0 always starts a drawing; the holds cover `0..scores.len()`.
pub fn segments(scores: &[f32], threshold: f32, minimum_hold: u32) -> Vec<Range<u32>> {
    let Ok(len) = u32::try_from(scores.len()) else { return Vec::new() };
    if len == 0 {
        return Vec::new();
    }
    let hold = minimum_hold.max(1);
    let mut starts = vec![0_u32];
    for (frame, score) in (0..len).zip(scores).skip(1) {
        let last = starts.last().copied().unwrap_or(0);
        if *score >= threshold && frame - last >= hold {
            starts.push(frame);
        }
    }
    if starts.len() > 1 && starts.last().is_some_and(|s| len - s < hold) {
        starts.pop();
    }
    starts.iter().enumerate().map(|(i, s)| *s..starts.get(i + 1).copied().unwrap_or(len)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat(v: u8) -> Luma {
        Luma { width: 4, height: 2, luma: vec![v; 8] }
    }

    #[test]
    fn small_changes_are_noise_and_big_ones_count() {
        let a = flat(100);
        assert_eq!(change_fraction(&a, &flat(110)), 0.0);
        assert_eq!(change_fraction(&a, &flat(200)), 1.0);
        let mut half = flat(100);
        half.luma[..4].fill(0);
        assert_eq!(change_fraction(&a, &half), 0.5);
        assert_eq!(change_fraction(&a, &Luma { width: 2, height: 4, luma: vec![100; 8] }), 1.0);
    }

    #[test]
    fn unreadable_frames_go_on_with_the_hold_before() {
        let scores = change_scores(&[Some(flat(0)), None, Some(flat(0)), Some(flat(255)), None]);
        assert_eq!(scores, vec![1.0, 0.0, 0.0, 1.0, 0.0]);
    }

    #[test]
    fn holds_tile_the_frames_and_fold_short_ones() {
        let scores = [1.0, 0.0, 0.0, 0.5, 0.0, 0.2, 0.0, 0.0, 0.6, 0.0];
        assert_eq!(segments(&scores, 0.1, 1), vec![0..3, 3..5, 5..8, 8..10]);
        // the change at 5 is too soon after 3; the last hold (8..10) is too short
        assert_eq!(segments(&scores, 0.1, 3), vec![0..3, 3..10]);
        assert_eq!(segments(&scores, 0.1, 4), vec![0..5, 5..10]);
        assert_eq!(segments(&scores, 0.55, 1), vec![0..8, 8..10]);
        assert!(segments(&[], 0.1, 1).is_empty());
    }

    #[test]
    fn sensitivity_runs_from_cuts_to_every_change() {
        assert!((threshold(0.0) - 0.3).abs() < 1e-6);
        assert!((threshold(1.0) - 0.0005).abs() < 1e-7);
        assert!(threshold(0.25) > threshold(0.75));
    }
}

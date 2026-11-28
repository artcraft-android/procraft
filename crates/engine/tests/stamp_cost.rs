#![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]
//! What making a stroke's stamps costs the CPU, a stamp at a time: a long stroke of a brush with
//! a pressure curve for every control it can have one for, against the same brush with none, its
//! stamps close together, made as a stroke on the canvas makes them (the samples a frame's, then the path on to the input shown
//! ahead of them: a dual brush's tip, which a tight-spaced brush makes anew each frame, a
//! thousand stamps and more). A bench, run by hand with `--ignored --nocapture` in release.

use std::time::Instant;

use procraft_engine::brush::{Brush, Rendering};
use procraft_engine::brushfile::PRESSURE_CURVES;
use procraft_engine::curve::Curve;
use procraft_engine::geom::Vec2;
use procraft_engine::input::{Pointer, Sample};
use procraft_engine::stroke::StrokeBuilder;

/// A round pen with its stamps close together, a pressure curve for every control it can have
/// one for (`curves`), building up gradually (`gradual`) or over; holding as many controls of its
/// own as the stock brushes do (some 170, by name: these change nothing it draws).
fn brush(curves: bool, gradual: bool) -> Brush {
    let mut b = Brush::round_pen();
    for k in 0..170 {
        b.extra.insert(format!("held_control_{k:03}"), 0.5);
    }
    b.spacing = 0.01;
    b.max_size = 200.0;
    b.streamline = 0.6;
    if gradual {
        b.rendering = Rendering::Gradual;
    }
    let bent = Curve::new(&[Vec2::new(0.0, 0.0), Vec2::new(0.4, 0.6), Vec2::new(1.0, 1.0)]);
    for (k, _) in PRESSURE_CURVES {
        b.pressure_curves.set(k, curves.then(|| bent.clone()));
    }
    b
}

fn sample(k: usize) -> Sample {
    let along = k as f32 * 7.5;
    let line = (along / 1800.0) as usize;
    let into = along % 1800.0;
    let x = if line.is_multiple_of(2) { 100.0 + into } else { 1900.0 - into };
    Sample {
        pos: Vec2::new(x, 200.0 + (line % 5) as f32 * 330.0 + (along * 0.01).sin() * 110.0),
        pressure: 0.35 + 0.6 * (k as f32 * 0.02).sin().abs(),
        tilt: 0.0,
        azimuth: 0.0,
        roll: 0.0,
        time_ms: k as f64 * 4.1667,
    }
}

/// The most a stamp of the brush with every pressure curve may take, against one of the same
/// brush with none (the same path, stamps and walk): as many times its microseconds, over and
/// building up gradually. Measured on the PC (a laptop's Core Ultra 9), the best of five strokes
/// each, six runs alternating with the build before: 1.32 to 1.44 times over and 1.45 to 1.63
/// gradual when a brush's pressure curves were kept in a tree by kind (a stamp reads some thirty)
/// and a gradual stamp looked up by name whether it was mixed wet or laid in place; 1.13 to 1.25
/// and 1.14 to 1.22 since (a stamp 0.74 to 0.77 us then with every curve, 0.58 to 0.62 since).
const MOST_TIMES: [f64; 2] = [1.35, 1.35];

/// Microseconds a stamp the samples add and a stamp of what runs ahead of them take, for `brush`:
/// the best of `rounds` strokes, and how many of each it makes.
fn cost(brush: &Brush, rounds: usize) -> (f64, f64, usize, usize) {
    let frames = 240;
    let (mut best_add, mut best_ahead) = (f64::MAX, f64::MAX);
    let (mut added, mut ahead) = (0usize, 0usize);
    for _ in 0..rounds {
        let mut b = StrokeBuilder::with_seed(brush.clone(), 80.0, Pointer::Stylus, 7);
        let _ = b.add(&[sample(0)]);
        let (mut add_s, mut ahead_s) = (0.0, 0.0);
        (added, ahead) = (0, 0);
        for f in 1..=frames {
            let t = Instant::now();
            added += b.add(&[sample(2 * f - 1), sample(2 * f)]).len();
            add_s += t.elapsed().as_secs_f64();
            let t = Instant::now();
            let mut tip = b.held_back();
            tip.extend(b.preview_to_input(&[]));
            ahead_s += t.elapsed().as_secs_f64();
            ahead += tip.len();
        }
        best_add = best_add.min(add_s);
        best_ahead = best_ahead.min(ahead_s);
    }
    (best_add * 1e6 / added.max(1) as f64, best_ahead * 1e6 / ahead.max(1) as f64, added, ahead)
}

/// The stroke's stamps made frame by frame, and what runs ahead of them, for the brush with every
/// pressure curve (over, and building up gradually) and with none: microseconds a stamp, the best
/// of `STAMP_COST_ROUNDS` (5) strokes; with every curve at most [`MOST_TIMES`] as long as none.
#[test]
#[ignore]
fn stamp_cost() {
    let rounds: usize = std::env::var("STAMP_COST_ROUNDS").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let none = cost(&brush(false, false), rounds);
    let mut over_most = Vec::new();
    for (gradual, most) in [(false, MOST_TIMES[0]), (true, MOST_TIMES[1])] {
        let all = cost(&brush(true, gradual), rounds);
        let times = (all.0 + all.1) / (none.0 + none.1);
        println!(
            "{}: {} stamps added, {:.3} us each ({:.3} without curves); {} ahead, {:.3} us each ({:.3}): {times:.2} times",
            if gradual { "gradual" } else { "over" },
            all.2,
            all.0,
            none.0,
            all.3,
            all.1,
            none.1
        );
        assert!(all.2 > 1000 && all.3 > 10 * all.2, "{all:?}");
        over_most.push((gradual, times, most));
    }
    for (gradual, times, most) in over_most {
        assert!(times <= most, "{times} times as long with every pressure curve (building up gradually: {gradual})");
    }
}

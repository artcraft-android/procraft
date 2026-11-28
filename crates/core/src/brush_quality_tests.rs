//! Stock brushes against their own thumbnails: the round pens' soft rims, Dry Brush's streaks, and
//! the big pictures of the grains of noise their stand-ins draw with (on a headless GPU where they
//! draw; skipped without one).

use procraft_engine::textures::TEXTURE_SIZE;

use crate::tests::{sample, session};
use crate::{CanvasSession, PenSample, PointerKind};

/// The library's set `set`'s brush `name`: its place.
fn find(s: &CanvasSession, set: &str, name: &str) -> (u32, u32) {
    let st = s.shared.lock();
    let (i, l) = st.library.iter().enumerate().find(|(_, l)| l.name == set).unwrap();
    let j = l.brushes.iter().position(|b| b.name == name).unwrap();
    (i as u32, j as u32)
}

/// The composite drawn, so that what was queued has gone to the GPU.
fn drawn(s: &CanvasSession) {
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
    canvas.render(gpu, &mut st.doc, None);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
}

/// A straight line across a `side` square canvas, a little off its pixel rows, by the brush chosen
/// at `size` at full pressure, as a 60 Hz screen draws it: the layer's coverage (0..1), the stroke
/// undone after.
fn line(s: &CanvasSession, size: f32, side: usize) -> Vec<f32> {
    let layer = s.layers()[0].id;
    s.set_brush_size(size);
    let at = |e: usize| PenSample {
        pressure: 1.0,
        ..sample(30.0 + e as f32 * 2.6, side as f32 / 2.0 + 0.3, e as f64 * 4.0)
    };
    s.stroke_begin(PointerKind::Stylus, vec![at(0)]);
    for e in 1..=160 {
        s.stroke_add(vec![at(e)]);
        if e % 4 == 0 {
            drawn(s);
        }
    }
    s.stroke_end(vec![]);
    drawn(s);
    let px = s.copy_layer_contents(layer).unwrap().rgba;
    s.undo();
    drawn(s);
    px.chunks(4).map(|c| f32::from(c[3]) / 255.0).collect()
}

/// Across the middle of a line drawn by [`line`]: its width at half its strength and how far its
/// edges take to rise from a tenth to nine tenths of it (each column's, averaged).
fn edge_of(a: &[f32], side: usize) -> (f32, f32) {
    let (mut widths, mut edges) = (Vec::new(), Vec::new());
    for x in side / 3..side * 2 / 3 {
        let col: Vec<f32> = (0..side).map(|y| a[y * side + x]).collect();
        let peak = col.iter().copied().fold(0.0, f32::max);
        if peak < 0.5 {
            continue;
        }
        let first = |t: f32| {
            let k = col.iter().position(|&v| v >= t)?;
            let (lo, hi) = (col[k.checked_sub(1)?], col[k]);
            Some(k as f32 - 1.0 + (t - lo) / (hi - lo).max(1e-6))
        };
        let last = |t: f32| {
            let k = col.iter().rposition(|&v| v >= t)?;
            let (hi, lo) = (col[k], *col.get(k + 1)?);
            Some(k as f32 + (hi - t) / (hi - lo).max(1e-6))
        };
        let (Some(t10), Some(t50), Some(t90), Some(b90), Some(b50), Some(b10)) = (
            first(0.1 * peak),
            first(0.5 * peak),
            first(0.9 * peak),
            last(0.9 * peak),
            last(0.5 * peak),
            last(0.1 * peak),
        ) else {
            continue;
        };
        widths.push(b50 - t50);
        edges.extend([t90 - t10, b10 - b90]);
    }
    let mean = |v: &[f32]| v.iter().sum::<f32>() / v.len().max(1) as f32;
    (mean(&widths), mean(&edges))
}

/// Of a preview's coverage (`w` wide): how far its edges take from a tenth to nine tenths, as the
/// pixels between over the length of its outline at half (counted across and down, times pi / 4).
fn preview_edge(a: &[u8], w: usize) -> f32 {
    let h = a.len() / w;
    let between = a.iter().filter(|&&v| v > 25 && v < 230).count() as f32;
    let mut crossings = 0usize;
    for y in 0..h {
        for x in 0..w {
            let v = a[y * w + x] >= 128;
            crossings += usize::from(x + 1 < w && (a[y * w + x + 1] >= 128) != v);
            crossings += usize::from(y + 1 < h && (a[(y + 1) * w + x] >= 128) != v);
        }
    }
    between / (crossings as f32 * std::f32::consts::FRAC_PI_4).max(1.0)
}

/// The coverage of set `set`'s brush `name`'s preview, 1060 x 324 as its thumbnail is.
fn preview_of(s: &CanvasSession, set: &str, name: &str) -> Vec<u8> {
    let (i, j) = find(s, set, name);
    s.brush_preview(i, j, 1060, 324).unwrap().rgba.chunks(4).map(|c| c[3]).collect()
}

/// The least a line by Studio Pen at size 0.7 and up takes from a tenth to nine tenths of it at
/// its edge (pixels): 1.9 to 2.6 drawn with the medium-hard stock tip's own falloff, 1.2 to 1.8
/// with the round tip drawn by its hardness.
const EDGE_LEAST: f32 = 1.8;

/// Studio Pen and Gel Pen (the medium-hard stock tip) fade over their rims as their thumbnails do:
/// their previews' edges from a tenth to nine tenths within a fifth of the thumbnails' (1.55 and
/// 1.39 pixels; the round tip drawn by its hardness gave 1.00 and 0.96, now 1.68 and 1.52), and a
/// line's softer at the bigger sizes.
#[test]
fn the_round_pens_rims_fade_as_their_stock_tips_do() {
    let Some(s) = session(480, 480) else { return };
    for (name, thumbnail) in [("Studio Pen", 1.55), ("Gel Pen", 1.39)] {
        let edge = preview_edge(&preview_of(&s, "Inking", name), 1060);
        eprintln!("{name}: the preview's edge {edge:.2} px, its thumbnail's {thumbnail}");
        assert!((edge / thumbnail - 1.0).abs() < 0.2, "{name}: {edge}");
    }
    let (i, j) = find(&s, "Inking", "Studio Pen");
    assert!(s.select_brush(i, j));
    s.set_color(0.0, 0.0, 0.0);
    for size in [0.05, 0.1, 0.2, 0.3, 0.55, 0.7, 0.85, 1.0] {
        let (width, edge) = edge_of(&line(&s, size, 480), 480);
        eprintln!("Studio Pen at {size}: {width:.1} px wide, its edge {edge:.2} px");
        // (a line a few pixels across has an edge of about a pixel either way)
        assert!(size < 0.7 || edge > EDGE_LEAST, "{size}: {edge}");
    }
}

/// The grains of noise standing in for stock images have big pictures as big as those images
/// (Lestrade's mottled stand-in, for a 2048 image, 2048), and a paper or wash drawn by its stock
/// image's octaves one with a texel to each of that image's pixels (Quicksilver's wash, for a 2048
/// image, 512; HB Pencil's paper, for a 1024 one, none past the one brushes hold).
#[test]
fn the_noise_grains_stand_ins_have_big_pictures_as_big_as_their_stock_images() {
    let s = CanvasSession::new(32, 32);
    let side = |set: &str, name: &str| {
        let (i, j) = find(&s, set, name);
        let st = s.shared.lock();
        let b = &st.library[i as usize].brushes[j as usize];
        b.grain.source.as_ref().map_or(0, |g| g.pixels.most_side())
    };
    assert_eq!(side("Oils", "Lestrade"), 2048);
    assert_eq!(side("Inking", "Quicksilver"), 512);
    assert_eq!(side("Sketching", "HB Pencil"), TEXTURE_SIZE);
}

/// Dry Brush's preview lays as much as its thumbnail, in streaks as strong: its bark-chip grain
/// stood in as the noise of the stock image's own octaves (as our trunk's furrows, the preview's
/// paint came to 42 637 pixels, at most 129 and 41 on average, against the thumbnail's 69 559, 255
/// and 124; now 73 237, 250 and 105).
#[test]
fn dry_brush_streaks_as_its_thumbnail_does() {
    let Some(s) = session(64, 64) else { return };
    let a = preview_of(&s, "Painting", "Dry Brush");
    let mut painted: Vec<u8> = a.iter().copied().filter(|&v| v > 0).collect();
    painted.sort_unstable();
    let mean = painted.iter().map(|&v| f32::from(v)).sum::<f32>() / painted.len().max(1) as f32;
    let strongest = painted.get(painted.len().saturating_sub(1) * 99 / 100).copied().unwrap_or(0);
    eprintln!("Dry Brush: {} pixels painted, at most {strongest} (99th), {mean:.1} on average", painted.len());
    assert!((painted.len() as f32 / 69_559.0 - 1.0).abs() < 0.15, "{}", painted.len());
    assert!(strongest >= 240 && (mean / 124.4 - 1.0).abs() < 0.25, "{strongest} {mean}");
}

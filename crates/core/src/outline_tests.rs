//! A dual brush drawn as an outline: a bigger second behind the first, the first painting the
//! secondary colour by pressure and the second the stroke's colour, so the stroke shows ringed.

use procraft_engine::brush::Brush;
use procraft_engine::curve::Curve;
use procraft_engine::document::LayerId;
use procraft_engine::geom::Vec2;

use crate::tests::{sample, session};
use crate::{CanvasSession, PenSample, PointerKind};

/// An outline brush of our own, made as people make one: two round pens of one kind at the least
/// spacing and the most StreamLine, mixing as they go (Intense Blending), each cut at an alpha
/// threshold of nothing; the first paints the secondary colour fully at any pressure (its pressure
/// held full by its curve), the second, bigger, the stroke's own colour behind it.
fn outline_brush(first_size: f32, second_size: f32) -> Brush {
    let mut first = Brush {
        name: "Outline".to_owned(),
        spacing: 0.0,
        streamline: 1.0,
        min_size: 1.0,
        max_size: first_size,
        hardness: 1.0,
        pressure_size: 0.0,
        pressure_curve: Curve::new(&[Vec2::new(0.0, 1.0), Vec2::new(1.0, 1.0)]),
        ..Brush::round_pen()
    };
    for (key, value) in [
        ("rendering_mode", 5.0),
        ("alpha_threshold", 1.0),
        ("alpha_threshold_amount", 0.0),
        ("pressure_secondary", 1.0),
    ] {
        first.set_setting(key, value);
    }
    let second = Brush { name: "Outline Rim".to_owned(), max_size: second_size, ..first.clone() };
    first.secondary = Some(Box::new(second));
    first.set_setting("dual_blend", 0.0);
    first
}

/// A session `side` pixels square drawing with `brush` at its full size, the stroke's colour
/// black and the secondary colour white.
fn outline_session(side: u32, brush: Brush) -> Option<std::sync::Arc<CanvasSession>> {
    let s = session(side, side)?;
    {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let tool = st.tool;
        *st.brush_mut(tool)? = brush;
    }
    s.set_brush_size(1.0);
    s.set_brush_opacity(1.0);
    s.set_color(0.0, 0.0, 0.0);
    s.set_secondary_color(1.0, 1.0, 1.0);
    Some(s)
}

/// A straight pen stroke from `a` to `b`, `n` samples 240 a second.
fn line(a: (f32, f32), b: (f32, f32), n: usize) -> Vec<PenSample> {
    (0..=n)
        .map(|i| {
            let t = i as f32 / n as f32;
            sample(a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t, i as f64 * 1000.0 / 240.0)
        })
        .collect()
}

/// The layer's pixel (premultiplied RGBA8) at (x, y).
fn at(rgba: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
    let i = ((y * side + x) * 4) as usize;
    [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]]
}

fn is_white(p: [u8; 4]) -> bool {
    p[3] == 255 && p[..3].iter().all(|&c| c >= 250)
}

fn is_black(p: [u8; 4]) -> bool {
    p[3] == 255 && p[..3].iter().all(|&c| c <= 5)
}

/// An outline brush's stroke drawn across another joins it: the second stroke's rim doesn't cross
/// the first's paint (its own fill goes over that), so both strokes' fills run unbroken through
/// where they cross, and the rim rings only what they lay on the clear layer; each stroke's fill
/// runs unbroken from end to end.
#[test]
fn an_outline_brushs_strokes_join_where_they_cross_and_ring_only_the_clear_layer() {
    let side = 400;
    let Some(s) = outline_session(side, outline_brush(24.0, 40.0)) else { return };
    let layer = s.add_layer();
    draw(&s, &line((60.0, 200.0), (340.0, 200.0), 280));
    draw(&s, &line((200.0, 60.0), (200.0, 340.0), 280));
    let rgba = s.shared.lock().layer_pixels(LayerId(layer)).unwrap();
    // each fill, in the secondary colour, unbroken along its stroke (its tapered ends aside),
    // through the other's rim
    let broken: Vec<(u32, u32)> =
        (70..=330).flat_map(|k| [(k, 200), (200, k)]).filter(|&(x, y)| !is_white(at(&rgba, side, x, y))).collect();
    assert!(broken.is_empty(), "the fill broken at {} places: {:?}", broken.len(), &broken[..broken.len().min(12)]);
    // the rim, in the stroke's colour, either side of each stroke away from where they cross
    for (x, y) in [(110, 184), (110, 216), (290, 184), (290, 216), (184, 110), (216, 110), (184, 290), (216, 290)] {
        assert!(is_black(at(&rgba, side, x, y)), "no rim at {x},{y}: {:?}", at(&rgba, side, x, y));
    }
    // and nothing past the rim
    assert_eq!(at(&rgba, side, 110, 170)[3], 0);
}

/// An outline brush's closed loop: its fill runs unbroken all round, where it closes too, and its
/// rim is as wide all round and its outer edge smooth (cut hard on the clear layer, within a pixel
/// of a smooth circle).
#[test]
fn an_outline_brushs_loop_closes_and_its_rim_is_even_and_smooth() {
    let side = 400;
    let Some(s) = outline_session(side, outline_brush(24.0, 40.0)) else { return };
    let layer = s.add_layer();
    let (cx, cy, r) = (200.0f32, 200.0f32, 110.0f32);
    let lap: Vec<PenSample> = (0..=360)
        .map(|i| {
            // once round and a little past where it began
            let a = std::f32::consts::TAU * 1.03 * i as f32 / 360.0;
            sample(cx + r * a.cos(), cy + r * a.sin(), f64::from(i) * 1000.0 / 240.0)
        })
        .collect();
    draw(&s, &lap);
    let rgba = s.shared.lock().layer_pixels(LayerId(layer)).unwrap();
    // out along 720 rays from the loop's middle (StreamLine draws it a little inside the pen's
    // path): where the fill ends, and where the paint does
    let rays: Vec<Option<(f32, f32)>> = (0..720)
        .map(|j| {
            let a = std::f32::consts::TAU * j as f32 / 720.0;
            let px = |d: f32| at(&rgba, side, (cx + d * a.cos()).round() as u32, (cy + d * a.sin()).round() as u32);
            let steps = |from: f32, to: f32| (0..=((to - from) * 4.0) as usize).map(move |k| from + k as f32 * 0.25);
            let fill = steps(r - 40.0, r + 30.0).rev().find(|&d| is_white(px(d)))?;
            let paint = steps(fill, r + 40.0).take_while(|&d| px(d)[3] > 0).last()?;
            Some((fill, paint))
        })
        .collect();
    let gaps = rays.iter().filter(|r| r.is_none()).count();
    assert_eq!(gaps, 0, "the fill broken round the loop");
    let rays: Vec<(f32, f32)> = rays.into_iter().flatten().collect();
    // away from where the loop begins and closes (StreamLine draws it in there from the pen's
    // path, and the rays cross the rim aslant at the corners that leaves)
    let away = 60..660;
    // the rim (40 - 24) / 2 = 8 pixels wide
    let widths = rays.get(away.clone()).unwrap_or_default().iter().map(|(fill, paint)| paint - fill);
    let (least, most) = widths.fold((f32::MAX, f32::MIN), |(l, m), w| (l.min(w), m.max(w)));
    // its outer edge off its neighbours' mean (a twentieth of the way round) by under a pixel
    let outer: Vec<f32> = rays.iter().map(|r| r.1).collect();
    let rough = away
        .map(|j| {
            let near: f32 = (1..=18).map(|d| outer[j + d] + outer[j - d]).sum::<f32>() / 36.0;
            (outer[j] - near).abs()
        })
        .fold(0.0f32, f32::max);
    eprintln!("rim {least}..{most} wide, outer edge off its neighbours by at most {rough}");
    assert!(least >= 6.5 && most <= 9.5, "rim {least}..{most} wide");
    assert!(rough <= 1.0, "outer edge off its neighbours by {rough}");
}

/// A curve across the top of a `side` canvas, a closed loop below it, and a line down across both,
/// as pen samples 240 a second (a moderate hand).
fn outline_paths(side: f32) -> [Vec<PenSample>; 3] {
    let k = side / 1024.0;
    let curve: Vec<PenSample> = (0..=240)
        .map(|i| {
            let t = i as f32 / 240.0;
            let x = (150.0 + 724.0 * t) * k;
            let y = (260.0 + 110.0 * (std::f32::consts::TAU * t).sin()) * k;
            sample(x, y, f64::from(i) * 1000.0 / 240.0)
        })
        .collect();
    let around = 300;
    let lap: Vec<PenSample> = (0..=around)
        .map(|i| {
            // once round and a little past where it began, so the loop closes on itself
            let a = std::f32::consts::TAU * 1.04 * i as f32 / around as f32;
            let (x, y) = (512.0 + 170.0 * a.cos(), 700.0 + 170.0 * a.sin());
            sample(x * k, y * k, f64::from(i) * 1000.0 / 240.0)
        })
        .collect();
    let down: Vec<PenSample> = (0..=200)
        .map(|i| {
            let t = i as f32 / 200.0;
            sample(512.0 * k, (120.0 + 790.0 * t) * k, f64::from(i) * 1000.0 / 240.0)
        })
        .collect();
    [curve, lap, down]
}

/// Draws `samples` as one pen stroke.
fn draw(s: &crate::CanvasSession, samples: &[PenSample]) {
    let Some((first, rest)) = samples.split_first() else { return };
    s.stroke_begin(PointerKind::Stylus, vec![*first]);
    let (middle, last) = rest.split_at(rest.len().saturating_sub(1));
    for some in middle.chunks(4) {
        s.stroke_add(some.to_vec());
    }
    s.stroke_end(last.to_vec());
}

/// An outline brush's file (`OUTLINE_BRUSH`) drawn at a few sizes (`OUTLINE_SIZES`, slider
/// positions) as a curve, a loop and a line across both, the stroke's colour black and the
/// secondary white, each size's layer written to `PROCRAFT_DUMP` (with `BRUSH_SETTINGS` changing
/// the brush first, as `dump_brush_strokes` takes them).
#[test]
fn dump_outline_strokes() {
    let Ok(path) = std::env::var("OUTLINE_BRUSH") else { return };
    let Ok(root) = std::env::var("PROCRAFT_DUMP") else { return };
    let side = 1024;
    let Some(s) = session(side, side) else { return };
    let name = std::path::Path::new(&path).file_name().map(|n| n.to_string_lossy().into_owned());
    let set = s.import_any_brushes(std::fs::read(&path).unwrap(), name, 0).unwrap();
    assert!(s.select_brush(set, 0));
    if let Ok(changes) = std::env::var("BRUSH_SETTINGS") {
        s.studio_begin();
        for change in changes.split(',').filter(|c| !c.is_empty()) {
            let (key, value) = change.split_once('=').unwrap();
            s.set_studio_setting(key.to_owned(), value.parse().unwrap());
        }
        s.studio_end(true);
    }
    s.set_color(0.0, 0.0, 0.0);
    s.set_secondary_color(1.0, 1.0, 1.0);
    s.set_brush_opacity(1.0);
    let sizes: Vec<f32> = std::env::var("OUTLINE_SIZES")
        .map(|v| v.split(',').filter_map(|x| x.parse().ok()).collect())
        .unwrap_or_else(|_| vec![0.3, 0.5, 0.7]);
    let tag = std::env::var("OUTLINE_TAG").unwrap_or_default();
    for size in sizes {
        let layer = s.add_layer();
        s.set_brush_size(size);
        {
            let st = s.shared.lock();
            let brush = st.brush(crate::ToolKind::Paint);
            let second = brush.secondary.as_deref().map(|b| b.diameter(size));
            eprintln!("size {size}: first {} px, second {second:?} px", brush.diameter(size));
        }
        for path in outline_paths(side as f32) {
            draw(&s, &path);
        }
        let rgba = s.shared.lock().layer_pixels(procraft_engine::document::LayerId(layer)).unwrap();
        let file = format!("{root}/outline{tag}-{size}.png");
        std::fs::write(&file, crate::project::rgba_png(side, side, &rgba)).unwrap();
        s.set_layer_visible(layer, false);
    }
}

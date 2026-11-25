//! How far what shows of a stroke trails the pen while it is drawn, brush by brush.

use procraft_engine::view::View;
use procraft_gpu::Gpu;

use crate::{CanvasSession, PenSample, PointerKind};

const SIDE: u32 = 512;
const ROW: f32 = 256.0;

fn session() -> Option<std::sync::Arc<CanvasSession>> {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let s = CanvasSession::new(SIDE, SIDE);
    {
        let mut st = s.shared.lock();
        st.window_size = (SIDE, SIDE);
        st.view = Some(View::fit((SIDE, SIDE), (SIDE, SIDE)));
        st.gpu = Some(gpu);
        st.ensure_canvas();
    }
    Some(s)
}

/// The pen's `k`th sample: along the middle row at 240 a second, `pace` pixels a sample.
fn point(k: usize, pace: f32) -> PenSample {
    PenSample {
        x: 40.0 + k as f32 * pace,
        y: ROW,
        pressure: 0.8,
        tilt: 0.0,
        orientation: 0.0,
        time_ms: k as f64 * (1000.0 / 240.0),
        roll: 0.0,
    }
}

/// Renders what is queued; the furthest column right that shows anything but the paper.
fn shown_to(s: &CanvasSession) -> f32 {
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    // (as a frame does: what runs ahead of the stroke drawn if no prediction drew it)
    st.draw_ahead();
    let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
    canvas.render(gpu, &mut st.doc, None);
    let px = canvas.read_composite(gpu).unwrap();
    let w = SIDE as usize;
    for x in (0..w).rev() {
        for y in 0..w {
            let i = (y * w + x) * 4;
            // (the faintest paint counts: a grainy brush's leading edge is thin)
            if px[i..i + 3].iter().any(|&c| c < 255) {
                return x as f32;
            }
        }
    }
    0.0
}

/// One stroke with the current brush: how far behind the pen it shows on average and at worst,
/// in pixels (below 0: past the pen), frame by frame, and how far it reaches once it has ended.
fn lag(s: &CanvasSession, pace: f32, predict: bool) -> (f32, f32, f32) {
    let events = 40usize;
    s.stroke_begin(PointerKind::Stylus, vec![point(0, pace)]);
    let (mut sum, mut worst, mut n) = (0.0f32, f32::MIN, 0.0f32);
    for e in 1..=events {
        s.hold_frames(true);
        s.stroke_add(vec![point(2 * e - 1, pace), point(2 * e, pace)]);
        if predict {
            s.stroke_predict(vec![point(2 * e + 1, pace), point(2 * e + 2, pace)]);
        }
        s.hold_frames(false);
        // the first few frames, the stroke is shorter than its own brush
        if e > 10 {
            let behind = point(2 * e, pace).x - shown_to(s);
            sum += behind;
            worst = worst.max(behind);
            n += 1.0;
        }
    }
    s.stroke_end(vec![]);
    let ended = point(2 * events, pace).x - shown_to(s);
    s.undo();
    shown_to(s);
    (sum / n, worst, ended)
}

/// Run with `--ignored --nocapture`: every brush that paints wet, in place or with a second,
/// and a few that do none of those to compare with.
#[test]
#[ignore]
fn stroke_lag() {
    let Some(s) = session() else { return };
    s.set_color(0.0, 0.0, 0.0);
    let kinds: Vec<(u32, u32, String, bool, bool, bool)> = {
        let st = s.shared.lock();
        st.library
            .iter()
            .enumerate()
            .flat_map(|(i, set)| {
                set.brushes.iter().enumerate().map(move |(j, b)| {
                    (
                        i as u32,
                        j as u32,
                        format!("{} / {}", set.name, b.name),
                        b.mixes_wet(),
                        b.laid_in_place(),
                        b.secondary.is_some(),
                    )
                })
            })
            .collect()
    };
    let only = std::env::var("LAG_BRUSH").ok();
    let mut plain = 0;
    for (set, index, name, wet, in_place, dual) in kinds {
        if let Some(only) = &only {
            if !name.contains(only.as_str()) {
                continue;
            }
        } else if !(wet || in_place || dual) {
            plain += 1;
            if plain % 12 != 0 {
                continue;
            }
        }
        if !s.select_brush(set, index) {
            continue;
        }
        s.set_brush_size(0.3);
        s.set_brush_opacity(1.0);
        let pace: f32 = std::env::var("LAG_PACE").ok().and_then(|p| p.parse().ok()).unwrap_or(4.0);
        let (mean, worst, ended) = lag(&s, pace, false);
        let (mean_p, worst_p, _) = lag(&s, pace, true);
        let kind = format!(
            "{}{}{}",
            if wet { "wet " } else { "" },
            if in_place { "in-place " } else { "" },
            if dual { "dual" } else { "" }
        );
        println!(
            "{name:<48} {kind:<20} behind {mean:>7.1} (worst {worst:>7.1})  predicted {mean_p:>7.1} (worst {worst_p:>7.1})  ended {ended:>7.1}"
        );
    }
}

/// Run with `--ignored --nocapture`: dual brushes made of the library's, dry and wet firsts,
/// by each of a few combine modes.
#[test]
#[ignore]
fn dual_stroke_lag() {
    let Some(s) = session() else { return };
    s.set_color(0.0, 0.0, 0.0);
    let find = |name: &str| {
        let st = s.shared.lock();
        st.library.iter().flat_map(|set| set.brushes.iter()).find(|b| b.name == name).cloned()
    };
    let kept = s.shared.lock().library[0].brushes[0].clone();
    let firsts = ["Ballpoint", "Chalk", "Dry Brush", "Felt Brush", "Oil Paint", "Gouache", "Wet Wash", "Round Wash"];
    let seconds = ["Chalk", "Charcoal Block", "Felt Brush", "Fat Cap", "Hair"];
    for first in firsts {
        for second in seconds {
            let (Some(a), Some(b)) = (find(first), find(second)) else {
                println!("no {first} or {second}");
                continue;
            };
            for mode in [0u32, 1, 2] {
                let mut dual = a.clone();
                dual.secondary = Some(Box::new(b.clone()));
                dual.set_setting("dual_blend", mode as f32);
                let (wet, in_place) = (dual.mixes_wet(), dual.laid_in_place());
                s.shared.lock().library[0].brushes[0] = dual;
                assert!(s.select_brush(0, 0));
                s.set_brush_size(0.3);
                s.set_brush_opacity(1.0);
                let (mean, worst, ended) = lag(&s, 4.0, false);
                let (mean_p, worst_p, _) = lag(&s, 4.0, true);
                println!(
                    "{first:<12} + {second:<15} mode {mode} {}{} behind {mean:>7.1} (worst {worst:>7.1})  predicted {mean_p:>7.1} (worst {worst_p:>7.1})  ended {ended:>7.1}",
                    if wet { "wet " } else { "    " },
                    if in_place { "in-place " } else { "         " },
                );
            }
        }
    }
    s.shared.lock().library[0].brushes[0] = kept;
}

/// The library's brush `first` with `second` as its second, combined by `mode`, as the current
/// brush (in place of the library's first).
fn choose_dual(s: &CanvasSession, first: &str, second: &str, mode: u32) {
    let find = |name: &str| {
        let st = s.shared.lock();
        st.library.iter().flat_map(|set| set.brushes.iter()).find(|b| b.name == name).cloned().unwrap()
    };
    let mut dual = find(first);
    dual.secondary = Some(Box::new(find(second)));
    dual.set_setting("dual_blend", mode as f32);
    s.shared.lock().library[0].brushes[0] = dual;
    assert!(s.select_brush(0, 0));
    s.set_brush_size(0.3);
    s.set_brush_opacity(1.0);
}

#[test]
fn a_wet_dual_brush_shows_up_to_the_pen_as_it_is_drawn() {
    let Some(s) = session() else { return };
    s.set_color(0.0, 0.0, 0.0);
    // by Multiply and Screen the paint shows only as far as its second has come; these seconds'
    // own stamps trail the pen by their streamline
    for (first, second) in [("Oil Paint", "Chalk"), ("Oil Paint", "Felt Brush"), ("Gouache", "Chalk")] {
        for mode in [1, 2] {
            choose_dual(&s, first, second, mode);
            let (mean, worst, _) = lag(&s, 4.0, false);
            assert!(worst <= 6.0, "{first} + {second} by {mode}: {worst} pixels behind the pen at worst ({mean})");
            let (mean, worst, _) = lag(&s, 4.0, true);
            assert!(worst <= 0.0, "{first} + {second} by {mode}, predicted: {worst} behind at worst ({mean})");
        }
    }
}

#[test]
fn undoing_a_wet_dual_stroke_leaves_nothing_of_its_second() {
    let Some(s) = session() else { return };
    s.set_color(0.0, 0.0, 0.0);
    // a spray for a second: its stamps land well outside the first's
    for mode in [0, 1] {
        choose_dual(&s, "Oil Paint", "Fat Cap", mode);
        s.stroke_begin(PointerKind::Stylus, vec![point(0, 4.0)]);
        for e in 1..=40usize {
            s.hold_frames(true);
            s.stroke_add(vec![point(2 * e - 1, 4.0), point(2 * e, 4.0)]);
            s.stroke_predict(vec![point(2 * e + 1, 4.0), point(2 * e + 2, 4.0)]);
            s.hold_frames(false);
            shown_to(&s);
        }
        s.stroke_end(vec![]);
        assert!(shown_to(&s) > 300.0);
        s.undo();
        shown_to(&s);
        let st = s.shared.lock();
        let (canvas, gpu) = (st.canvas.as_ref().unwrap(), st.gpu.as_ref().unwrap());
        let layer = canvas.read_layer(gpu, st.doc.current()).unwrap();
        let left = layer.as_chunks::<4>().0.iter().filter(|[.., a]| *a != 0).count();
        assert_eq!(left, 0, "by {mode}: {left} pixels of paint left after undo");
    }
}

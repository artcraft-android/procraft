//! Scrubbing a timeline through the session's own calls takes frames kept from before, and
//! painting at the playhead composites again only the frames that show what was painted.

use make2d_engine::view::View;
use make2d_gpu::Gpu;

use crate::{CanvasSession, PenSample, PointerKind};

fn sample(x: f32, y: f32) -> PenSample {
    PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: 0.0, roll: 0.0 }
}

fn session(side: u32) -> Option<std::sync::Arc<CanvasSession>> {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let s = CanvasSession::new(side, side);
    {
        let mut st = s.shared.lock();
        st.window_size = (side, side);
        st.view = Some(View::fit((side, side), (side, side)));
        st.gpu = Some(gpu);
        st.ensure_canvas();
    }
    Some(s)
}

/// Renders the frame the session shows: its pixels, and whether they came from the cache.
fn shown(s: &CanvasSession) -> (Vec<u8>, bool) {
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return (Vec::new(), false) };
    let hits = canvas.frame_cache_stats().hits;
    canvas.render_finished(gpu, &mut st.doc);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
    (canvas.read_composite(gpu).unwrap_or_default(), canvas.frame_cache_stats().hits > hits)
}

#[test]
fn scrubbing_takes_kept_frames_and_painting_drops_only_its_own() {
    let Some(s) = session(40) else { return };
    s.set_brush_size(0.6);
    for (i, colour) in [[1.0, 0.0, 0.0], [0.0, 0.7, 0.0], [0.0, 0.0, 1.0]].into_iter().enumerate() {
        if i > 0 {
            s.add_layer();
        }
        s.set_color(colour[0], colour[1], colour[2]);
        s.stroke_begin(PointerKind::Finger, vec![sample(8.0 + 12.0 * i as f32, 20.0)]);
        s.stroke_end(vec![]);
    }
    assert!(s.set_animation_assist(true));
    let frames = s.animation_frames();
    s.set_frame_hold(frames[0], 1);
    if let Some(mut a) = s.animation_settings() {
        a.onion_frames = 1;
        s.set_animation_settings(a);
    }
    assert!(s.start_timeline());
    let duration = s.timeline_info().map_or(0, |i| i.duration);
    assert_eq!(duration, 4);
    // scrubbed once: each frame composited; scrubbed back: each from the cache, the same
    let first: Vec<(Vec<u8>, bool)> = (0..duration)
        .map(|f| {
            s.set_timeline_frame(f);
            shown(&s)
        })
        .collect();
    assert!(first.iter().all(|(_, hit)| !hit));
    // (the last frame still shows: setting it again draws nothing)
    for f in (0..duration - 1).rev() {
        s.set_timeline_frame(f);
        let (px, hit) = shown(&s);
        assert!(hit, "frame {f} kept");
        assert_eq!(px, first[f as usize].0, "frame {f}");
    }
    // played: kept apart from the frames drawn on (no onion skins)
    for kept in [false, true] {
        for f in 0..duration {
            s.set_timeline_playing(Some(f));
            assert_eq!(shown(&s).1, kept, "played frame {f}");
        }
    }
    s.set_timeline_playing(None);
    // painting the last drawing (frame 3, composited whole again as the stroke ends): frame 2
    // skins it, so it is composited again; frames 0 and 1 (the first drawing, held, one onion
    // skin each side) are as they were
    s.set_timeline_frame(3);
    shown(&s);
    s.set_color(1.0, 1.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0)]);
    s.stroke_end(vec![]);
    shown(&s);
    let hits: Vec<bool> = (0..duration)
        .map(|f| {
            s.set_timeline_frame(f);
            shown(&s).1
        })
        .collect();
    assert_eq!(hits, [true, true, false, true]);
    // and what was kept of it is as it is composited afresh
    let (kept, _) = shown(&s);
    if let Some(c) = s.shared.lock().canvas.as_mut() {
        c.forget_frames();
        c.invalidate();
    }
    let (afresh, hit) = shown(&s);
    assert!(!hit);
    assert!(kept == afresh, "frame 3 kept as painted");
}

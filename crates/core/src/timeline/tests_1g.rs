//! Timeline step 1g through the session: the clipboard, retiming and sync markers.

use procraft_engine::view::View;
use procraft_gpu::Gpu;

use super::clipboard::TimelineClipboardInfo;
use super::{TimelineChannel, TimelineEasing, TimelineSettings};
use crate::{CanvasSession, PenSample, PointerKind};

/// A session on a headless GPU, or `None` without a GPU.
pub(super) fn session(side: u32) -> Option<std::sync::Arc<CanvasSession>> {
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

/// A canvas on a timeline: one track of three one-frame drawings, a dot of its own colour on each.
pub(super) fn three_drawings() -> Option<std::sync::Arc<CanvasSession>> {
    let s = session(40)?;
    s.set_brush_size(0.4);
    for (i, colour) in [[1.0, 0.0, 0.0], [0.0, 0.7, 0.0], [0.0, 0.0, 1.0]].into_iter().enumerate() {
        if i > 0 {
            s.add_layer();
        }
        s.set_color(colour[0], colour[1], colour[2]);
        let x = 8.0 + 12.0 * i as f32;
        s.stroke_begin(
            PointerKind::Finger,
            vec![PenSample { x, y: 20.0, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: 0.0, roll: 0.0 }],
        );
        s.stroke_end(vec![]);
    }
    assert!(s.set_animation_assist(true));
    assert!(s.start_timeline());
    Some(s)
}

/// The frame as it plays.
pub(super) fn played(s: &CanvasSession, frame: u32) -> Vec<u8> {
    s.set_timeline_playing(Some(frame));
    let px = {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.invalidate();
        canvas.render(gpu, &mut st.doc, None);
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        canvas.read_composite(gpu).unwrap()
    };
    s.set_timeline_playing(None);
    px
}

pub(super) fn spans(s: &CanvasSession, t: usize) -> Vec<(u32, u32)> {
    s.timeline_tracks()[t].clips.iter().map(|c| (c.start, c.duration)).collect()
}

#[test]
fn a_cut_drawing_pastes_back_as_it_was_though_its_undo_step_is_gone() {
    let Some(s) = three_drawings() else { return };
    let green = played(&s, 1);
    let clip = s.timeline_tracks()[0].clips[1].id;
    assert!(s.cut_timeline_clips(vec![clip]));
    assert_eq!(s.timeline_clipboard().clips, 1);
    assert_eq!(spans(&s, 0), [(0, 1), (2, 1)]);
    assert_ne!(played(&s, 1), green);
    // nothing in the history keeps the cut drawing's pixels now: only the clipboard does
    s.shared.lock().canvas.as_mut().unwrap().forget_history();
    let _ = played(&s, 0);
    s.set_timeline_frame(1);
    let pasted = s.paste_timeline_clips();
    assert_eq!(pasted.len(), 1);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);
    assert_ne!(s.timeline_tracks()[0].clips[1].root, 0);
    assert_eq!(played(&s, 1), green, "the drawing comes back as it was cut");
    // pasted again: a copy more, the one at the playhead staying, the rest moving along
    assert_eq!(s.paste_timeline_clips().len(), 1);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (3, 1)]);
    assert_eq!(played(&s, 2), green);
    assert_eq!(s.undo().as_deref(), Some("Paste Clips"));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);
}

#[test]
fn copied_clips_paste_onto_the_tracks_above_in_one_undo_step() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let red = played(&s, 0);
    let ids: Vec<u64> = s.timeline_tracks()[0].clips.iter().take(2).map(|c| c.id).collect();
    assert!(s.copy_timeline_clips(ids));
    // onto a second track at frame 5: past the end, the timeline grows to hold them
    let second = s.add_timeline_track();
    s.set_timeline_frame(2);
    let mut settings = TimelineSettings {
        fps: 12,
        duration: 10,
        exposure: 1,
        playback: crate::animation::AnimationPlayback::Loop,
        onion_frames: 0,
        onion_opacity: 0.4,
    };
    s.set_timeline_settings(settings.clone());
    s.set_timeline_frame(5);
    s.select_timeline_track(second);
    let layers = s.layers().len();
    assert_eq!(s.paste_timeline_clips().len(), 2);
    assert_eq!(spans(&s, 1), [(5, 1), (6, 1)]);
    assert_eq!(s.layers().len(), layers + 2, "each drawing a copy of its layers");
    s.set_timeline_track(s.timeline_tracks()[0].id, true, false);
    assert_eq!(played(&s, 5), red);
    assert_eq!(s.undo().as_deref(), Some("Hide Track"));
    assert_eq!(s.undo().as_deref(), Some("Paste Clips"));
    assert_eq!(s.layers().len(), layers);
    assert!(spans(&s, 1).is_empty());
    // a locked track takes no paste
    s.set_timeline_track(second, false, true);
    assert!(s.paste_timeline_clips().is_empty());
    settings.duration = 20;
    s.set_timeline_settings(settings);
}

#[test]
fn keys_copy_and_paste_onto_the_clip_at_the_playhead() {
    let Some(s) = three_drawings() else { return };
    let tracks = s.timeline_tracks();
    let (first, last) = (tracks[0].clips[0].id, tracks[0].clips[2].id);
    s.set_timeline_frame(0);
    assert!(s.set_timeline_key(first, TimelineChannel::Opacity, vec![0.5], TimelineEasing::Linear, true));
    assert!(!s.copy_timeline_keys(last, true), "none there");
    assert!(s.copy_timeline_keys(first, true));
    assert_eq!(s.timeline_clipboard(), TimelineClipboardInfo { clips: 0, keys: true });
    s.set_timeline_frame(2);
    assert!(s.paste_timeline_keys());
    let keys = s.timeline_keys(last);
    assert_eq!(keys.len(), 1);
    assert_eq!((keys[0].frame, keys[0].channel), (2, TimelineChannel::Opacity));
    assert_eq!(s.undo().as_deref(), Some("Paste Keys"));
    assert!(s.timeline_keys(last).is_empty());
}

#[test]
fn retiming_keeps_the_timing_and_the_playhead() {
    let Some(s) = three_drawings() else { return };
    let fps = s.timeline_info().unwrap().fps;
    s.set_timeline_frame(1);
    assert!(s.retime_timeline(fps * 2));
    assert!(!s.retime_timeline(fps * 2));
    let info = s.timeline_info().unwrap();
    assert_eq!((info.fps, info.duration, info.frame), (fps * 2, 6, 2));
    assert_eq!(spans(&s, 0), [(0, 2), (2, 2), (4, 2)]);
    assert_eq!(s.undo().as_deref(), Some("Change Frame Rate"));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);
}

#[test]
fn the_layers_panel_lists_the_drawing_at_the_playhead() {
    let Some(s) = three_drawings() else { return };
    let roots: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.root).collect();
    s.set_timeline_frame(2);
    assert_eq!(s.timeline_drawing_shown(), Some(roots[2]));
    s.set_timeline_frame(0);
    assert_eq!(s.timeline_drawing_shown(), Some(roots[0]));
    assert!(s.stop_timeline());
    assert_eq!(s.timeline_drawing_shown(), None);
}

//! The timeline panel's clip and track pages through the session: each change one undo step.

use super::menus::{TimelineLoop, TimelineOnion};
use super::tests_1g::{played, three_drawings};
use crate::CanvasSession;

/// The full view, with a second track above the first; returns the two tracks' ids.
fn two_tracks(s: &CanvasSession) -> (u64, u64) {
    s.set_timeline_simple(false);
    let first = s.timeline_tracks()[0].id;
    let second = s.add_timeline_track();
    assert_ne!(second, 0);
    (first, second)
}

#[test]
fn a_clips_look_changes_and_undoes_a_step_at_a_time() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let red = played(&s, 0);
    let clip = s.timeline_tracks()[0].clips[0].id;

    assert!(s.set_timeline_clip_opacity(clip, 0.7, false));
    assert!(s.set_timeline_clip_opacity(clip, 0.25, true));
    let c = &s.timeline_tracks()[0].clips[0];
    assert!((c.opacity - 0.25).abs() < 1e-6);
    assert_ne!(played(&s, 0), red);
    assert!(s.undo().is_some());
    assert!((s.timeline_tracks()[0].clips[0].opacity - 1.0).abs() < 1e-6, "the drag is one step");
    assert_eq!(played(&s, 0), red);

    let multiply = crate::layers::blend_modes().into_iter().find(|m| m.name == "Multiply").unwrap().id;
    assert!(s.set_timeline_clip_blend(clip, multiply));
    assert!(!s.set_timeline_clip_blend(clip, multiply), "already");
    assert_eq!(s.timeline_tracks()[0].clips[0].blend, multiply);

    assert!(s.set_timeline_clip_disabled(clip, true));
    assert!(s.timeline_tracks()[0].clips[0].disabled);
    assert_ne!(played(&s, 0), red, "a clip turned off doesn't show");
    assert!(s.set_timeline_clip_label(clip, Some(0xFF453A)));
    assert_eq!(s.timeline_tracks()[0].clips[0].label, Some(0xFF453A));
    assert!(s.rename_timeline_clip(clip, "  Walk ".into()));
    assert_eq!(s.timeline_tracks()[0].clips[0].name, "Walk");
    assert!(!s.rename_timeline_clip(clip, " ".into()));

    for _ in 0..4 {
        assert!(s.undo().is_some());
    }
    let c = &s.timeline_tracks()[0].clips[0];
    assert!(!c.disabled && c.label.is_none() && c.blend != multiply && c.name != "Walk");
    assert_eq!(played(&s, 0), red);
}

#[test]
fn a_locked_tracks_clips_keep_their_look() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let t = &s.timeline_tracks()[0];
    let (track, clip) = (t.id, t.clips[0].id);
    assert!(s.set_timeline_track(track, false, true));
    assert!(!s.set_timeline_clip_opacity(clip, 0.5, true));
    assert!(!s.set_timeline_clip_disabled(clip, true));
    assert!(!s.set_timeline_clip_blend(clip, 1));
}

#[test]
fn a_tracks_opacity_order_and_solo() {
    let Some(s) = three_drawings() else { return };
    let (first, second) = two_tracks(&s);
    assert!(s.set_timeline_track_opacity(first, 0.4, false));
    assert!(s.set_timeline_track_opacity(first, 0.5, true));
    assert!((s.timeline_tracks()[0].opacity - 0.5).abs() < 1e-6);
    assert!(s.undo().is_some());
    assert!((s.timeline_tracks()[0].opacity - 1.0).abs() < 1e-6);

    assert!(s.move_timeline_track(first, 1));
    assert_eq!(s.timeline_tracks().iter().map(|t| t.id).collect::<Vec<_>>(), [second, first]);
    assert!(!s.move_timeline_track(first, 5), "on top already");
    assert!(s.undo().is_some());
    assert_eq!(s.timeline_tracks().iter().map(|t| t.id).collect::<Vec<_>>(), [first, second]);

    // a third track hidden beforehand comes back hidden once the solo is undone by asking again
    let third = s.add_timeline_track();
    assert!(s.set_timeline_track(third, true, false));
    assert!(s.solo_timeline_track(first));
    let shown = |s: &CanvasSession| s.timeline_tracks().iter().map(|t| !t.hidden).collect::<Vec<_>>();
    assert_eq!(shown(&s), [true, false, false]);
    assert!(s.timeline_tracks()[0].soloed);
    // another track shown alone instead keeps what was there before the first
    assert!(s.solo_timeline_track(second));
    assert_eq!(shown(&s), [false, true, false]);
    assert!(s.solo_timeline_track(second));
    assert_eq!(shown(&s), [true, true, false]);
    assert!(!s.timeline_tracks()[1].soloed);
    // each was one step
    assert!(s.undo().is_some());
    assert_eq!(shown(&s), [false, true, false]);
}

#[test]
fn playing_goes_round_the_looped_frames() {
    let Some(s) = three_drawings() else { return };
    let info = s.timeline_info().unwrap();
    s.set_timeline_settings(super::TimelineSettings {
        fps: info.fps,
        duration: 12,
        exposure: info.exposure,
        playback: info.playback,
        onion_frames: info.onion_frames,
        onion_opacity: info.onion_opacity,
    });
    assert!(s.set_timeline_loop(Some(TimelineLoop { start: 2, end: 5 })));
    assert_eq!(s.timeline_loop(), Some(TimelineLoop { start: 2, end: 5 }));
    assert!(!s.set_timeline_loop(Some(TimelineLoop { start: 2, end: 5 })));
    let step = |at| s.timeline_next_step(at, 1).map(|p| p.at);
    assert_eq!([step(0), step(2), step(4)], [Some(2), Some(3), Some(2)]);
    assert!(s.set_timeline_loop(Some(TimelineLoop { start: 9, end: 40 })));
    assert_eq!(s.timeline_loop(), Some(TimelineLoop { start: 9, end: 12 }), "kept within the timeline");
    assert!(s.undo().is_some());
    assert!(s.set_timeline_loop(None));
    assert_eq!(s.timeline_loop(), None);
    assert_eq!(step(11), Some(0));
}

/// The frame as it shows while drawing at `frame`: onion skins and all.
fn drawn(s: &CanvasSession, frame: u32) -> Vec<u8> {
    s.set_timeline_frame(frame);
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
    canvas.invalidate();
    canvas.render(gpu, &mut st.doc, None);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
    canvas.read_composite(gpu).unwrap()
}

#[test]
fn onion_skins_show_in_the_full_view_on_the_track_drawn_on() {
    let Some(s) = three_drawings() else { return };
    let simple = drawn(&s, 1);
    assert_ne!(simple, played(&s, 1), "simple mode shows the drawings either side");
    // the full view shows them on the track drawn on, as simple mode does
    s.set_timeline_simple(false);
    assert_eq!(drawn(&s, 1), simple);
    // drawing on another track (with nothing on it), none
    let (first, second) = (s.timeline_tracks()[0].id, s.add_timeline_track());
    assert_ne!(second, 0);
    assert_eq!(drawn(&s, 1), played(&s, 1));
    // back on the first, they show again
    s.select_timeline_track(first);
    assert_eq!(drawn(&s, 1), simple);
}

#[test]
fn onion_skins_either_side_are_set_apart() {
    let Some(s) = three_drawings() else { return };
    let onion = TimelineOnion { before: 3, after: 0, before_color: 0x0000FF, after_color: 0 };
    s.set_timeline_onion(onion);
    assert_eq!(s.timeline_onion(), Some(onion));
    assert_eq!(s.timeline_info().unwrap().onion_frames, 3);
    // another setting changed leaves the sides as they are
    let i = s.timeline_info().unwrap();
    let settings = super::TimelineSettings {
        fps: i.fps,
        duration: i.duration,
        exposure: 2,
        playback: i.playback,
        onion_frames: i.onion_frames,
        onion_opacity: i.onion_opacity,
    };
    s.set_timeline_settings(settings.clone());
    assert_eq!(s.timeline_onion().map(|o| (o.before, o.after)), Some((3, 0)));
    // the one count changed sets both
    s.set_timeline_settings(super::TimelineSettings { onion_frames: 2, ..settings });
    assert_eq!(s.timeline_onion().map(|o| (o.before, o.after)), Some((2, 2)));
}

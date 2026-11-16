//! The timeline's length through the session: each change one undo step.

use super::TimelineEdgePolicy;
use super::menus::TimelineLoop;
use super::tests_1g::{played, spans, three_drawings};

#[test]
fn the_length_drags_out_and_back_past_the_clips() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let end = s.timeline_content_end();
    assert!(end >= 3);
    let was = s.timeline_info().unwrap().duration;

    assert!(s.set_timeline_length(end + 10));
    assert_eq!(s.timeline_info().unwrap().duration, end + 10);
    assert!(!s.set_timeline_length(end + 10), "that long already");
    s.set_timeline_frame(end + 8);
    assert!(s.set_timeline_loop(Some(TimelineLoop { start: end, end: end + 9 })));

    assert!(s.set_timeline_length(1));
    let info = s.timeline_info().unwrap();
    assert_eq!(info.duration, 1, "shorter than the clips, as Koi's may be");
    assert_eq!(info.frame, 0, "the playhead within it");
    assert_eq!(s.timeline_loop(), None, "none of the looped frames are left");
    assert_eq!(s.timeline_content_end(), end, "the clips are kept");

    assert!(s.undo().is_some());
    assert_eq!(s.timeline_info().unwrap().duration, end + 10);
    assert!(s.undo().is_some());
    assert!(s.undo().is_some());
    assert_eq!(s.timeline_info().unwrap().duration, was);
}

#[test]
fn fit_to_content_is_one_step() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let end = s.timeline_content_end();
    assert!(s.set_timeline_length(end + 20));
    assert!(s.fit_timeline_to_content());
    assert_eq!(s.timeline_info().unwrap().duration, end);
    assert!(!s.fit_timeline_to_content());
    assert!(s.undo().is_some());
    assert_eq!(s.timeline_info().unwrap().duration, end + 20);
}

#[test]
fn frames_added_at_the_start_move_the_drawings_and_the_playhead_along() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let first = played(&s, 0);
    let before = spans(&s, 0);
    let was = s.timeline_info().unwrap().duration;
    s.set_timeline_frame(1);

    assert!(s.add_timeline_frames(true, 4));
    let info = s.timeline_info().unwrap();
    assert_eq!(info.duration, was + 4);
    assert_eq!(info.frame, 5, "the playhead stays on the drawing it was on");
    assert_eq!(spans(&s, 0), before.iter().map(|(a, d)| (a + 4, *d)).collect::<Vec<_>>());
    assert_eq!(played(&s, 4), first, "what showed first shows four frames on");
    assert_ne!(played(&s, 0), first);

    assert!(s.undo().is_some());
    assert_eq!(spans(&s, 0), before);
    assert_eq!(s.timeline_info().unwrap().duration, was);
}

#[test]
fn frames_added_at_the_end_lengthen_it_only() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let before = spans(&s, 0);
    let was = s.timeline_info().unwrap().duration;
    assert!(s.add_timeline_frames(false, 12));
    assert_eq!(s.timeline_info().unwrap().duration, was + 12);
    assert_eq!(spans(&s, 0), before);
    assert!(!s.add_timeline_frames(false, 0));
    assert!(s.undo().is_some());
    assert_eq!(s.timeline_info().unwrap().duration, was);
}

#[test]
fn a_timeline_ending_before_its_drawings_plays_and_exports_only_to_its_end() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let before = spans(&s, 0);
    let end = s.timeline_content_end();
    assert_eq!(end, 3);
    let blue = played(&s, 2);
    s.set_timeline_frame(2);

    assert!(s.set_timeline_length(2));
    let info = s.timeline_info().unwrap();
    assert_eq!(info.duration, 2);
    assert_eq!(info.frame, 1, "the playhead within it");
    assert_eq!(spans(&s, 0), before, "the drawing past the end stays where it is");
    // playing never reaches frame 2; the playhead stops at the last frame; exports take two
    assert_ne!(s.timeline_next_step(1, 1).map(|p| p.at), Some(2));
    s.set_timeline_frame(2);
    assert_eq!(s.timeline_info().unwrap().frame, 1);
    assert_eq!(s.animation_frame_count(), 2);
    // a drawing trimmed out there leaves the end where it was put
    let last = s.timeline_tracks()[0].clips[2].id;
    assert!(s.trim_timeline_clip(last, false, 6, TimelineEdgePolicy::Stop));
    assert_eq!(s.timeline_info().unwrap().duration, 2);
    assert_eq!(s.timeline_content_end(), 6);

    // fitted, it reaches the drawings again, and the last one plays as it did
    assert!(s.fit_timeline_to_content());
    assert_eq!(s.timeline_info().unwrap().duration, 6);
    assert_eq!(played(&s, 2), blue);
    assert!(s.undo().is_some());
    assert!(s.undo().is_some());
    assert!(s.undo().is_some());
    assert_eq!(s.timeline_info().unwrap().duration, end);
    assert_eq!(spans(&s, 0), before);
}

//! A clip put down over others pushing them aside, and clips picked together moved to another
//! track, trimmed and duplicated, through the session.

use super::edits::TimelineEdgePolicy;
use super::tests_1g::{played, spans, three_drawings};

/// The clips of the first track: (start, duration).
fn first(s: &crate::CanvasSession) -> Vec<(u32, u32)> {
    spans(s, 0)
}

#[test]
fn a_clip_put_down_over_others_pushes_them_aside_in_one_undo_step() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let ids: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let track = s.timeline_tracks()[0].id;
    assert!(s.set_timeline_clip_duration(ids[0], 3, true));
    assert_eq!(first(&s), [(0, 3), (3, 1), (4, 1)]);
    // the last one over the start: the first goes just past it, the second past that
    let preview = s.timeline_place_preview(ids[2], track, 0, true).unwrap();
    assert_eq!(preview.start, 0);
    let shown: Vec<(u64, u32, u32)> = preview.clips.iter().map(|c| (c.id, c.start, c.duration)).collect();
    assert_eq!(shown, [(ids[2], 0, 1), (ids[0], 1, 3), (ids[1], 4, 1)]);
    assert_eq!(first(&s), [(0, 3), (3, 1), (4, 1)], "a preview changes nothing");
    assert!(s.place_timeline_clip(ids[2], track, 0, true));
    assert_eq!(first(&s), [(0, 1), (1, 3), (4, 1)]);
    assert_eq!(s.undo().as_deref(), Some("Move Clip"));
    assert_eq!(first(&s), [(0, 3), (3, 1), (4, 1)]);
    // pushed earlier there is no room before the first: the middle one is pushed later instead
    assert!(s.place_timeline_clip(ids[2], track, 3, false));
    let order: Vec<(u64, u32)> = s.timeline_tracks()[0].clips.iter().map(|c| (c.id, c.start)).collect();
    assert_eq!(order, [(ids[0], 0), (ids[2], 3), (ids[1], 4)]);
    // a locked track takes nothing
    assert!(s.set_timeline_track(track, false, true));
    assert!(s.timeline_place_preview(ids[1], track, 0, true).is_none());
    assert!(!s.place_timeline_clip(ids[1], track, 0, true));
}

#[test]
fn clips_picked_together_move_to_another_track_in_one_undo_step() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let ids: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let above = s.add_timeline_track();
    assert_ne!(above, 0);
    let picked = vec![ids[0], ids[1]];
    let room = s.timeline_move_room(picked.clone(), ids[0], above, 1).unwrap();
    assert_eq!(room.by, 1);
    assert_eq!(room.tracks, [above, above]);
    assert_eq!(first(&s), [(0, 1), (1, 1), (2, 1)], "asking changes nothing");
    assert!(s.move_timeline_clips(picked.clone(), ids[0], above, 1));
    assert_eq!(first(&s), [(2, 1)]);
    assert_eq!(spans(&s, 1), [(1, 1), (2, 1)]);
    assert_eq!(s.undo().as_deref(), Some("Move Clips"));
    assert_eq!(first(&s), [(0, 1), (1, 1), (2, 1)]);
    // two tracks up isn't there; a locked track refuses them
    assert!(s.timeline_move_room(picked.clone(), ids[0], 9999, 0).is_none());
    assert!(s.set_timeline_track(above, false, true));
    assert!(!s.move_timeline_clips(picked, ids[0], above, 0));
}

#[test]
fn clips_picked_together_trim_together_in_one_undo_step() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let ids: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let track = s.timeline_tracks()[0].id;
    let picked = vec![ids[0], ids[2]];
    // both ends two later, pushing: the first pushes the others along, the last grows too
    let preview = s.timeline_trims_preview(picked.clone(), false, 2, TimelineEdgePolicy::Push);
    assert_eq!(preview.len(), 1);
    assert_eq!(preview[0].track, track);
    let shown: Vec<(u32, u32)> = preview[0].clips.iter().map(|c| (c.start, c.duration)).collect();
    assert_eq!(shown, [(0, 3), (3, 1), (4, 3)]);
    assert_eq!(first(&s), [(0, 1), (1, 1), (2, 1)], "a preview changes nothing");
    assert!(s.trim_timeline_clips(picked.clone(), false, 2, TimelineEdgePolicy::Push));
    assert_eq!(first(&s), [(0, 3), (3, 1), (4, 3)]);
    assert_eq!(s.undo().as_deref(), Some("Trim Clips"));
    assert_eq!(first(&s), [(0, 1), (1, 1), (2, 1)]);
    // stopping at the next clip: the first can't grow, the last can
    assert!(s.trim_timeline_clips(picked, false, 2, TimelineEdgePolicy::Stop));
    assert_eq!(first(&s), [(0, 1), (1, 1), (2, 3)]);
}

#[test]
fn clips_picked_together_duplicate_after_themselves_in_one_undo_step() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let red = played(&s, 0);
    let green = played(&s, 1);
    let layers = s.layers().len();
    let ids: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let made = s.duplicate_timeline_clips(vec![ids[0], ids[1]]);
    assert_eq!(made.len(), 2);
    // put down after the second, the third moving along
    assert_eq!(first(&s), [(0, 1), (1, 1), (2, 1), (3, 1), (4, 1)]);
    let clips = s.timeline_tracks()[0].clips.clone();
    assert_eq!(clips[4].id, ids[2]);
    assert!(made.contains(&clips[2].id) && made.contains(&clips[3].id));
    // each a copy of its own drawing's layers
    assert_eq!(s.layers().len(), layers + 2);
    assert!(clips[2].root != clips[0].root && clips[3].root != clips[1].root);
    assert_eq!(played(&s, 2), red);
    assert_eq!(played(&s, 3), green);
    assert_eq!(s.undo().as_deref(), Some("Duplicate Clips"));
    assert_eq!(first(&s), [(0, 1), (1, 1), (2, 1)]);
    assert_eq!(s.layers().len(), layers);
}

#[test]
fn clips_picked_together_put_down_push_aside_what_they_cover_in_one_undo_step() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let ids: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let track = s.timeline_tracks()[0].id;
    let picked = vec![ids[0], ids[1]];
    // a frame later along their own track: the third, which they land on, is pushed on
    let preview = s.timeline_place_clips_preview(picked.clone(), ids[0], track, 1, true);
    assert_eq!(preview.len(), 1);
    let shown: Vec<(u64, u32)> = preview[0].clips.iter().map(|c| (c.id, c.start)).collect();
    assert_eq!(shown, [(ids[0], 1), (ids[1], 2), (ids[2], 3)]);
    assert_eq!(first(&s), [(0, 1), (1, 1), (2, 1)], "a preview changes nothing");
    assert!(s.place_timeline_clips(picked.clone(), ids[0], track, 1, true));
    assert_eq!(first(&s), [(1, 1), (2, 1), (3, 1)]);
    assert_eq!(s.undo().as_deref(), Some("Move Clips"));
    assert_eq!(first(&s), [(0, 1), (1, 1), (2, 1)]);
    // before the first frame: refused
    assert!(s.timeline_place_clips_preview(picked.clone(), ids[0], track, -1, true).is_empty());
    assert!(!s.place_timeline_clips(picked, ids[0], track, -1, true));
}

#[test]
fn clips_picked_together_go_into_a_group_in_one_undo_step() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let ids: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let red = played(&s, 0);
    let blue = played(&s, 2);
    let g = s.group_timeline_clips(vec![ids[0]]);
    assert_ne!(g, 0);
    assert!(s.move_timeline_clips_into_group(vec![ids[1], ids[2]], g, 0));
    let top = s.timeline_tracks();
    assert_eq!(top[0].clips.len(), 1, "only the group is left on the track");
    assert_eq!((top[0].clips[0].id, top[0].clips[0].duration), (g, 3), "grown to hold them");
    assert_eq!(played(&s, 0), red);
    assert_eq!(played(&s, 2), blue, "each plays where it did");
    assert_eq!(s.undo().as_deref(), Some("Move Clips"));
    assert_eq!(s.timeline_tracks()[0].clips.len(), 3);
    // the group can't go into itself: nothing changes
    assert!(!s.move_timeline_clips_into_group(vec![ids[1], g], g, 0));
    assert_eq!(s.timeline_tracks()[0].clips.len(), 3);
}

#[test]
fn clips_picked_on_two_levels_duplicate_together_in_one_undo_step() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let ids: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let layers = s.layers().len();
    // the first two made a group on frames 0 to 2; the third after it on frame 2
    let g = s.group_timeline_clips(vec![ids[0], ids[1]]);
    assert_ne!(g, 0);
    // picked: the second (inside the group, frame 1) and the third (outside it, frame 2)
    let made = s.duplicate_timeline_clips(vec![ids[1], ids[2]]);
    assert_eq!(made.len(), 2);
    assert_eq!(s.layers().len(), layers + 2);
    // the third's copy goes two frames on (the span of both); the group grows to hold the
    // second's copy, pushing the third and its copy along
    assert_eq!(first(&s), [(0, 4), (4, 1), (5, 1)]);
    let rows = s.timeline_rows();
    let inner: Vec<(u32, u32)> = rows[0].track.clips.iter().map(|c| (c.start, c.duration)).collect();
    assert_eq!(inner, [(0, 1), (1, 1), (3, 1)]);
    assert_eq!(s.undo().as_deref(), Some("Duplicate Clips"));
    assert_eq!(first(&s), [(0, 2), (2, 1)]);
    assert_eq!(s.layers().len(), layers);
}

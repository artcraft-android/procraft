//! Group clips unfolded in the panel's rows, and a cycling group's repeats set, through the
//! session.

use super::groups_inline::{TimelineRepeat, TimelineRow};
use super::tests_1g::three_drawings;
use crate::CanvasSession;

/// The full view with the first track's clips 0 (held three frames) and 1 made into a group on
/// frames 0 to 4, clip 2 after it on frame 4; returns the group, the clips and the track.
fn grouped(s: &CanvasSession) -> (u64, Vec<u64>, u64) {
    s.set_timeline_simple(false);
    let track = s.timeline_tracks()[0].id;
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    assert!(s.set_timeline_clip_duration(clips[0], 3, true));
    let g = s.group_timeline_clips(vec![clips[0], clips[1]]);
    assert_ne!(g, 0);
    (g, clips, track)
}

/// Each row's clips as (start, duration).
fn spans(row: &TimelineRow) -> Vec<(u32, u32)> {
    row.track.clips.iter().map(|c| (c.start, c.duration)).collect()
}

#[test]
fn an_unfolded_group_shows_its_tracks_under_its_own() {
    let Some(s) = three_drawings() else { return };
    let (g, _, track) = grouped(&s);
    let rows = s.timeline_rows();
    assert_eq!(rows.len(), 2, "the group's own track, then the track holding it");
    let (inner, outer) = (&rows[0], &rows[1]);
    assert_eq!((outer.track.id, outer.depth, outer.group), (track, 0, 0));
    assert_eq!((inner.depth, inner.group, inner.start, inner.end), (1, g, 0, 4));
    assert_eq!(spans(inner), [(0, 3), (3, 1)]);
    assert!(inner.repeats.is_empty());

    // folded: only the track holding it, and not an undo step
    assert!(s.set_timeline_group_folded(g, true));
    assert!(!s.set_timeline_group_folded(g, true), "already");
    assert_eq!(s.timeline_rows().len(), 1);
    assert_eq!(s.undo().as_deref(), Some("Group Clips"));
}

#[test]
fn a_clip_in_an_unfolded_group_is_moved_and_trimmed_at_its_timeline_frames() {
    let Some(s) = three_drawings() else { return };
    let (g, clips, track) = grouped(&s);
    // the group later: its clips follow it
    assert!(s.move_timeline_clip(g, track, 6));
    let rows = s.timeline_rows();
    assert_eq!(spans(&rows[0]), [(6, 3), (9, 1)]);
    assert_eq!((rows[0].start, rows[0].end), (6, 10));
    // its first clip's end dragged to frame 8 on the timeline
    assert!(s.trim_timeline_clip(clips[0], false, 8, super::edits::TimelineEdgePolicy::Stop));
    assert_eq!(spans(&s.timeline_rows()[0]), [(6, 2), (9, 1)]);
    // its last moved two frames on, along its row
    assert!(s.move_timeline_clip(clips[1], s.timeline_rows()[0].track.id, 11));
    assert_eq!(spans(&s.timeline_rows()[0]), [(6, 2), (11, 1)]);
    // a track above locked: the rows say so
    assert!(s.set_timeline_track(track, false, true));
    assert!(s.timeline_rows()[0].locked_above);
}

#[test]
fn a_cycling_group_shows_its_repeats_and_its_cycles_are_set_by_count() {
    let Some(s) = three_drawings() else { return };
    let (g, clips, track) = grouped(&s);
    assert!(s.set_timeline_cycle(g, 2, false));
    let inner = &s.timeline_rows()[0];
    assert_eq!((inner.start, inner.end), (0, 2), "the first cycle");
    assert_eq!(inner.repeats, [TimelineRepeat { clip: clips[0], start: 2, duration: 2 }]);

    // two cycles, and no room for more before the clip after it
    assert_eq!(s.timeline_group_cycles(g), Some(vec![2, 2]));
    assert!(!s.set_timeline_group_cycles(g, 3));
    assert!(s.set_timeline_group_cycles(g, 1));
    assert_eq!(s.timeline_group_cycles(g), Some(vec![1, 2]));
    assert!(s.timeline_rows()[0].repeats.is_empty());
    assert_eq!(s.undo().as_deref(), Some("Cycles"));
    assert_eq!(s.timeline_group_cycles(g), Some(vec![2, 2]));

    // the clip after it gone: room for many more
    assert!(s.delete_timeline_clip(clips[2], false));
    assert!(s.set_timeline_group_cycles(g, 5));
    let tracks = s.timeline_tracks();
    let group = tracks[0].clips.iter().find(|c| c.id == g).unwrap();
    assert_eq!((group.start, group.duration), (0, 10));
    assert_eq!(s.timeline_rows()[0].repeats.len(), 4, "one in each later cycle");

    // a locked track's group keeps its cycles; a drawing has none
    assert!(s.set_timeline_track(track, false, true));
    assert!(!s.set_timeline_group_cycles(g, 2));
    assert_eq!(s.timeline_group_cycles(clips[0]), None);
}

#[test]
fn a_cycling_groups_end_stops_part_way_through_a_cycle() {
    let Some(s) = three_drawings() else { return };
    let (g, clips, track) = grouped(&s);
    assert!(s.set_timeline_cycle(g, 2, false));
    // the clip after it on frame 4: four frames of room
    assert_eq!(s.timeline_group_room(g), Some(4));
    assert_eq!(s.timeline_group_room(clips[0]), None, "a drawing doesn't cycle");
    assert!(s.set_timeline_group_length(g, 3));
    let group = s.timeline_tracks()[0].clips.iter().find(|c| c.id == g).map(|c| (c.start, c.duration));
    assert_eq!(group, Some((0, 3)));
    assert_eq!(s.timeline_group_cycles(g), Some(vec![2, 2]), "a last cycle cut short counts");
    assert_eq!(s.timeline_rows()[0].repeats, [TimelineRepeat { clip: clips[0], start: 2, duration: 1 }]);
    assert!(!s.set_timeline_group_length(g, 3), "already");
    assert_eq!(s.undo().as_deref(), Some("Cycles"));
    // a locked track's group stays as it is
    assert!(s.set_timeline_track(track, false, true));
    assert!(!s.set_timeline_group_length(g, 1));
}

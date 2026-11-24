//! What the grid timeline added, through the session: a track's own look, frames flipped and
//! cleared, and the playhead resting past the timeline's end.

use super::tests_1g::{played, spans, three_drawings};
use super::{PAST_END, TimelineMask};

#[test]
fn a_tracks_look_is_its_clips_and_undoes_a_step_at_a_time() {
    let Some(s) = three_drawings() else { return };
    let track = s.timeline_tracks()[0].id;
    let multiply = crate::layers::blend_modes().into_iter().find(|m| m.name == "Multiply").unwrap().id;
    let t = &s.timeline_tracks()[0];
    assert!(t.label.is_none() && t.blend != multiply && t.mask == TimelineMask::None);

    assert!(s.set_timeline_track_label(track, Some(0xFF2D95)));
    assert!(!s.set_timeline_track_label(track, Some(0xFF2D95)), "already");
    assert!(s.set_timeline_track_blend(track, multiply));
    assert!(!s.set_timeline_track_blend(track, multiply), "already");
    assert!(s.set_timeline_track_mask(track, TimelineMask::Clipping));
    let t = &s.timeline_tracks()[0];
    assert_eq!((t.label, t.blend, t.mask), (Some(0xFF2D95), multiply, TimelineMask::Clipping));
    // every clip of it that has none of its own draws with the track's
    let plan = s.shared.lock().doc.timeline.as_ref().unwrap().plan(1);
    assert_eq!(plan.items[0].blend.id(), multiply);
    // the rows the panel reads say the same
    assert_eq!(s.timeline_rows()[0].track.blend, multiply);

    for _ in 0..3 {
        assert!(s.undo().is_some());
    }
    let t = &s.timeline_tracks()[0];
    assert!(t.label.is_none() && t.blend != multiply && t.mask == TimelineMask::None);

    // a locked track keeps its blend mode and mask; its colour tag is only the panel's
    assert!(s.set_timeline_track(track, false, true));
    assert!(!s.set_timeline_track_blend(track, multiply));
    assert!(!s.set_timeline_track_mask(track, TimelineMask::Alpha));
    assert!(s.set_timeline_track_label(track, Some(0x0A84FF)));
}

#[test]
fn flipped_frames_play_in_the_opposite_order_as_one_step() {
    let Some(s) = three_drawings() else { return };
    let (red, green, blue) = (played(&s, 0), played(&s, 1), played(&s, 2));
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    // the first held for two frames: it keeps its length at the other end
    assert!(s.set_timeline_clip_duration(clips[0], 2, true));
    assert_eq!(spans(&s, 0), [(0, 2), (2, 1), (3, 1)]);

    assert!(s.reverse_timeline_clips(clips.clone()));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 2)]);
    assert_eq!(played(&s, 0), blue);
    assert_eq!(played(&s, 1), green);
    assert_eq!(played(&s, 2), red);
    assert_eq!(played(&s, 3), red);
    assert!(!s.reverse_timeline_clips(vec![clips[1]]), "one alone has nothing to change places with");

    assert!(s.undo().is_some());
    assert_eq!(spans(&s, 0), [(0, 2), (2, 1), (3, 1)]);
    assert_eq!(played(&s, 0), red);
    assert_eq!(played(&s, 3), blue);

    // a locked track's stay as they are
    let track = s.timeline_tracks()[0].id;
    assert!(s.set_timeline_track(track, false, true));
    assert!(!s.reverse_timeline_clips(clips));
}

#[test]
fn a_cleared_frame_is_blank_and_stays_where_it_is() {
    let Some(s) = three_drawings() else { return };
    let green = played(&s, 1);
    // nothing shows on a frame past the end
    let blank = played(&s, 9);
    assert_ne!(green, blank);
    let before = s.timeline_tracks()[0].clips.clone();
    let layers = s.shared.lock().doc.timeline.as_ref().unwrap().drawing_roots().len();

    assert!(s.clear_timeline_clips(vec![before[1].id]));
    let after = s.timeline_tracks()[0].clips.clone();
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);
    assert_eq!((after[1].id, &after[1].name), (before[1].id, &before[1].name), "the same clip, under its name");
    assert_ne!(after[1].root, before[1].root, "on a new layer");
    assert_eq!(s.shared.lock().doc.timeline.as_ref().unwrap().drawing_roots().len(), layers);
    assert_eq!(played(&s, 1), blank);
    assert_eq!(played(&s, 0), {
        assert!(s.undo().is_some());
        played(&s, 0)
    });
    assert_eq!(played(&s, 1), green, "one step back, its pixels with it");
    assert_eq!(s.timeline_tracks()[0].clips, before);

    // several at once are one step too
    assert!(s.clear_timeline_clips(vec![before[0].id, before[2].id]));
    assert_eq!((played(&s, 0), played(&s, 2)), (blank.clone(), blank));
    assert!(s.undo().is_some());
    assert_eq!(s.timeline_tracks()[0].clips, before);

    // a locked track's drawings keep their pixels
    let track = s.timeline_tracks()[0].id;
    assert!(s.set_timeline_track(track, false, true));
    assert!(!s.clear_timeline_clips(vec![before[1].id]));
    assert!(!s.clear_timeline_clips(vec![4242]));
}

#[test]
fn the_playhead_rests_past_the_end_and_a_drawing_there_lengthens_the_timeline() {
    let Some(s) = three_drawings() else { return };
    assert_eq!(s.timeline_info().unwrap().duration, 3);

    // the playhead as the other views move it stays within the timeline
    s.set_timeline_frame(5);
    assert_eq!(s.timeline_info().unwrap().frame, 2);
    s.rest_timeline_frame(5);
    let info = s.timeline_info().unwrap();
    assert_eq!((info.frame, info.duration), (5, 3), "resting there changes nothing");
    // no further than a little past the last frame
    s.rest_timeline_frame(100_000);
    assert_eq!(s.timeline_info().unwrap().frame, 2 + PAST_END);

    s.rest_timeline_frame(5);
    let clip = s.add_timeline_drawing();
    assert_ne!(clip, 0);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (5, 1)]);
    let info = s.timeline_info().unwrap();
    assert_eq!((info.frame, info.duration), (5, 6));
    assert!(s.undo().is_some());
    assert_eq!(s.timeline_info().unwrap().duration, 3);

    // playing from past the end starts over from the first frame
    assert_eq!(s.timeline_next_step(7, 1).map(|n| n.at), Some(0));
}

#[test]
fn a_start_with_no_room_before_it_grows_the_frame_at_its_end() {
    let Some(s) = three_drawings() else { return };
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);
    let (red, green, blue) = (played(&s, 0), played(&s, 1), played(&s, 2));

    // the frame before the second is against the first frame: dragged two frames earlier, the
    // second lasts two longer at its end, the third pushed along and the timeline grown to hold it
    let shown: Vec<(u32, u32)> =
        s.timeline_lengthen_preview(clips[1], true, -1).iter().map(|c| (c.start, c.duration)).collect();
    assert_eq!(shown, [(0, 1), (1, 3), (4, 1)]);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)], "a preview changes nothing");
    assert!(s.lengthen_timeline_clip(clips[1], true, -1));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 3), (4, 1)]);
    assert_eq!(s.timeline_info().unwrap().duration, 5);
    assert_eq!((played(&s, 0), played(&s, 3), played(&s, 4)), (red.clone(), green.clone(), blue.clone()));
    assert!(s.undo().is_some());
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)], "one step");
    assert_eq!(s.timeline_info().unwrap().duration, 3);

    // with room before them, the frames before it are pushed as far as they can go, and only
    // what is left over goes on its end
    s.rest_timeline_frame(6);
    let last = s.add_timeline_drawing();
    assert_ne!(last, 0);
    s.rest_timeline_frame(5);
    let before = s.add_timeline_drawing();
    assert_ne!(before, 0);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (5, 1), (6, 1)]);
    // frames 3 and 4 are empty: the one at 5 has two frames of room, and is asked for three
    assert!(s.lengthen_timeline_clip(last, true, 3));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (3, 1), (4, 4)]);
    assert_eq!(s.timeline_info().unwrap().duration, 8);

    // a start dragged later only gets shorter, down to a frame; an end past the timeline's makes
    // the timeline longer
    assert!(s.lengthen_timeline_clip(last, true, 100));
    assert_eq!(spans(&s, 0).last(), Some(&(7, 1)));
    assert!(s.lengthen_timeline_clip(last, false, 12));
    assert_eq!(spans(&s, 0).last(), Some(&(7, 5)));
    assert_eq!(s.timeline_info().unwrap().duration, 12);

    // picked together, each a frame longer: they take up the empty frames after them, and the
    // last frame, which none of them reaches, stays where it is
    assert!(s.lengthen_timeline_clips(clips.clone(), false, 1));
    assert_eq!(spans(&s, 0), [(0, 2), (2, 2), (4, 2), (6, 1), (7, 5)]);
    assert_eq!(s.timeline_info().unwrap().duration, 12);

    // a locked track's stay as they are
    let track = s.timeline_tracks()[0].id;
    assert!(s.set_timeline_track(track, false, true));
    assert!(!s.lengthen_timeline_clip(clips[1], true, -3));
    assert!(s.timeline_lengthen_preview(clips[1], false, 9).is_empty());
    assert!(!s.lengthen_timeline_clips(clips, false, 1));
}

#[test]
fn the_empty_frames_between_two_frames_are_counted_set_and_closed_as_a_step_each() {
    let Some(s) = three_drawings() else { return };
    let track = s.timeline_tracks()[0].id;
    s.rest_timeline_frame(6);
    assert_ne!(s.add_timeline_drawing(), 0);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (6, 1)]);
    let gap = s.timeline_gap(track, 4).unwrap();
    assert_eq!((gap.start, gap.length, gap.open), (3, 3, false));
    assert!(s.timeline_gap(track, 6).is_none(), "a frame is there");
    assert!(s.timeline_gap(track, 9).unwrap().open, "nothing after it");

    // a frame shorter, typed longer, then closed
    assert!(s.set_timeline_gap(track, 4, 2));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (5, 1)]);
    assert!(s.set_timeline_gap(track, 3, 6));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (9, 1)]);
    assert_eq!(s.timeline_info().unwrap().duration, 10, "the timeline holds the frames pushed later");
    assert!(s.set_timeline_gap(track, 8, 0));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (3, 1)]);
    assert!(!s.set_timeline_gap(track, 8, 2), "nothing after those frames to move");
    assert!(s.undo().is_some());
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (9, 1)], "one step");

    assert!(s.set_timeline_track(track, false, true));
    assert!(!s.set_timeline_gap(track, 4, 1), "a locked track's frames stay");
}

#[test]
fn a_frame_dropped_between_tracks_gets_a_track_of_its_own_there() {
    let Some(s) = three_drawings() else { return };
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let first = s.timeline_tracks()[0].id;
    let names = |s: &crate::CanvasSession| s.timeline_tracks().iter().map(|t| t.id).collect::<Vec<_>>();

    // above the track it was on, further along than the timeline is long
    let above = s.drop_timeline_clip_on_new_track(clips[1], first, false, 7);
    assert_ne!(above, 0);
    assert_eq!(names(&s), [first, above]);
    assert_eq!(spans(&s, 0), [(0, 1), (2, 1)]);
    assert_eq!(spans(&s, 1), [(7, 1)]);
    assert_eq!(s.timeline_info().unwrap().duration, 8, "the timeline holds it");
    assert!(s.undo().is_some());
    assert_eq!(names(&s), [first], "one step: the track goes with the move");
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);

    // below it
    let below = s.drop_timeline_clip_on_new_track(clips[2], first, true, 0);
    assert_ne!(below, 0);
    assert_eq!(names(&s), [below, first]);
    assert_eq!(spans(&s, 0), [(0, 1)]);
    assert_eq!(spans(&s, 1), [(0, 1), (1, 1)]);

    // dropped on a track, the frames it covers pushed along and the timeline grown to hold them
    assert!(s.drop_timeline_clip(clips[2], first, 1, true));
    assert_eq!(spans(&s, 1), [(0, 1), (1, 1), (2, 1)]);
    assert!(s.drop_timeline_clip(clips[0], first, 9, true));
    assert_eq!(spans(&s, 1).last(), Some(&(9, 1)));
    assert_eq!(s.timeline_info().unwrap().duration, 10);

    // a locked track's frame stays where it is
    assert!(s.set_timeline_track(first, false, true));
    assert_eq!(s.drop_timeline_clip_on_new_track(clips[0], first, false, 0), 0);
    assert!(!s.drop_timeline_clip(clips[0], first, 0, true));
}

#[test]
fn a_group_split_at_the_playhead_is_two_groups_showing_what_it_did_as_one_step() {
    let Some(s) = three_drawings() else { return };
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    // the second drawing held for three frames, then all of them grouped: frames 0, 1–3, 4
    assert!(s.set_timeline_clip_duration(clips[1], 3, true));
    let before: Vec<_> = (0..5).map(|f| played(&s, f)).collect();
    let group = s.group_timeline_clips(clips.clone());
    assert_ne!(group, 0);
    assert_eq!(spans(&s, 0), [(0, 5)]);
    // not on its first frame
    s.set_timeline_frame(0);
    assert!(!s.split_timeline_clip(group));
    // through the held drawing
    s.set_timeline_frame(2);
    assert!(s.split_timeline_clip(group));
    assert_eq!(spans(&s, 0), [(0, 2), (2, 3)]);
    for f in 0..5 {
        assert_eq!(played(&s, f), before[f as usize], "frame {f} shows what it did");
    }
    assert!(s.undo().is_some());
    assert_eq!(spans(&s, 0), [(0, 5)], "one step");
    for f in 0..5 {
        assert_eq!(played(&s, f), before[f as usize]);
    }
}

#[test]
fn picked_frames_starts_each_take_the_room_nearest_them_then_grow_at_their_ends() {
    let Some(s) = three_drawings() else { return };
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    s.rest_timeline_frame(6);
    let last = s.add_timeline_drawing();
    assert_ne!(last, 0);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (6, 1)]);
    // both two frames longer at their starts: the last has the empty frames before it; the
    // third, packed against the first two, grows at its end instead and pushes the last along
    // only by the one frame it reaches into it
    let shown = s.timeline_lengthens_preview(vec![clips[2], last], true, -2);
    assert_eq!(shown.len(), 1);
    assert_eq!(shown[0].clips.iter().map(|c| (c.start, c.duration)).collect::<Vec<_>>(), [(0, 1), (1, 1), (2, 3), (5, 3)]);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (6, 1)], "a preview changes nothing");
    assert!(s.lengthen_timeline_clips(vec![clips[2], last], true, -2));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 3), (5, 3)]);
    assert_eq!(s.timeline_info().unwrap().duration, 8);
    assert!(s.undo().is_some());
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (6, 1)], "one step");
    // a frame is at least one long: started later, these have nothing to give
    assert!(!s.lengthen_timeline_clips(vec![clips[2], last], true, 5));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (6, 1)]);
}

#[test]
fn new_drawings_start_as_vector_layers_once_that_is_asked_for() {
    let Some(s) = three_drawings() else { return };
    let vectors = |s: &crate::CanvasSession| s.layers().iter().filter(|l| l.vector).count();
    s.rest_timeline_frame(5);
    assert_ne!(s.add_timeline_drawing(), 0);
    assert_eq!(vectors(&s), 0, "a paint layer, as before");
    s.set_timeline_vector_frames(true);
    s.rest_timeline_frame(8);
    assert_ne!(s.add_timeline_drawing(), 0);
    assert_eq!(vectors(&s), 1);
    assert_eq!(spans(&s, 0).last(), Some(&(8, 1)));
    assert!(s.undo().is_some());
    assert_eq!(vectors(&s), 0, "one step");
    s.set_timeline_vector_frames(false);
    assert_ne!(s.add_timeline_drawing(), 0);
    assert_eq!(vectors(&s), 0);
}

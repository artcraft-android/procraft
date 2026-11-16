//! Group clips, the layers panel's +, the clipboard between canvases, and sounds and videos
//! split, duplicated and keyed, through the session.

use std::path::Path;

use super::tests_1g::{played, session, spans, three_drawings};
use super::{TimelineChannel, TimelineEasing};
use crate::CanvasSession;
use crate::media::{MediaImport, TimelineMediaKind};

/// Clips 0 and 1 of the first track (clip 0 held three frames) made into a group; returns the
/// group and the clips.
fn grouped(s: &CanvasSession) -> (u64, Vec<u64>) {
    s.set_timeline_simple(false);
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    assert!(s.set_timeline_clip_duration(clips[0], 3, true));
    let g = s.group_timeline_clips(vec![clips[0], clips[1]]);
    assert_ne!(g, 0);
    (g, clips)
}

fn group_of(s: &CanvasSession, clip: u64) -> Option<u64> {
    let st = s.shared.lock();
    st.doc.timeline.as_ref()?.group_of(make2d_engine::timeline::ClipId(clip)).map(|g| g.0)
}

/// The RGBA of pixel (`x`, `y`) of a `side`-wide frame.
fn pixel(px: &[u8], side: u32, x: u32, y: u32) -> [u8; 4] {
    let at = ((y * side + x) * 4) as usize;
    [px[at], px[at + 1], px[at + 2], px[at + 3]]
}

#[test]
fn a_motion_path_in_a_posed_group_is_drawn_where_the_clip_shows() {
    let Some(s) = three_drawings() else { return };
    let (g, clips) = grouped(&s);
    let c = clips[0];
    s.set_timeline_frame(0);
    assert!(s.set_timeline_key(c, TimelineChannel::Position, vec![0.0, 0.0], TimelineEasing::Linear, true));
    s.set_timeline_frame(2);
    assert!(s.set_timeline_key(c, TimelineChannel::Position, vec![0.0, 4.0], TimelineEasing::Linear, true));
    let alone = s.timeline_motion_path(c).unwrap();
    // the group moved 10 across: the path with it
    s.set_timeline_frame(0);
    assert!(s.set_timeline_key(g, TimelineChannel::Position, vec![10.0, 0.0], TimelineEasing::Linear, true));
    let moved = s.timeline_motion_path(c).unwrap();
    assert_eq!(moved.keys.len(), 2);
    for (a, b) in alone.keys.iter().zip(&moved.keys) {
        assert!((b.x - a.x - 10.0).abs() < 1e-3 && (b.y - a.y).abs() < 1e-3, "{a:?} {b:?}");
    }
    assert!((moved.points[0] - alone.points[0] - 10.0).abs() < 1e-3);
    // the group twice the size: a drag on the canvas is half as far for the clip's keys, and its
    // handles show twice as long as they are
    assert!(s.set_timeline_key(g, TimelineChannel::Scale, vec![2.0, 2.0], TimelineEasing::Linear, true));
    let d = s.timeline_motion_delta(c, 0, 10.0, -4.0);
    assert!((d[0] - 5.0).abs() < 1e-3 && (d[1] + 2.0).abs() < 1e-3, "{d:?}");
    assert_eq!(s.timeline_motion_delta(clips[2], 0, 10.0, -4.0), [10.0, -4.0], "in no group");
    assert!(s.set_timeline_path_handles(c, 0, Some(vec![-4.0, 0.0, 4.0, 0.0]), true));
    let bent = s.timeline_motion_path(c).unwrap();
    let h = bent.keys[0].handles.clone().unwrap();
    assert!(h.iter().zip([-4.0, 0.0, 4.0, 0.0]).all(|(a, b)| (a - b).abs() < 1e-3), "{h:?}");
    let st = s.shared.lock();
    let stored = st.doc.timeline.as_ref().unwrap().clip(make2d_engine::timeline::ClipId(c)).unwrap();
    assert_eq!(stored.keys.position.keys[0].path.unwrap().after, [2.0, 0.0]);
}

#[test]
fn a_group_deleted_takes_its_drawings_layers_in_one_undo_step() {
    let Some(s) = three_drawings() else { return };
    let layers = s.layers().len();
    let (g, _) = grouped(&s);
    assert!(s.delete_timeline_clip(g, false));
    assert_eq!(s.layers().len(), layers - 2);
    assert_eq!(s.undo().as_deref(), Some("Delete Group"));
    assert_eq!(s.layers().len(), layers);
}

#[test]
fn ungrouping_a_cycle_plays_as_it_did_with_copies_of_its_drawings() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let g = s.group_timeline_clips(vec![clips[0], clips[1]]);
    assert!(s.set_timeline_clip_duration(g, 6, true));
    assert!(s.set_timeline_cycle(g, 2, false));
    let before: Vec<Vec<u8>> = (0..7).map(|f| played(&s, f)).collect();
    let layers = s.layers().len();
    assert!(s.ungroup_timeline_clip(g));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1)]);
    assert_eq!(s.layers().len(), layers + 4, "a copy of each drawing for each repeat");
    for (f, was) in before.iter().enumerate() {
        assert!(played(&s, f as u32) == *was, "frame {f}");
    }
    assert_eq!(s.undo().as_deref(), Some("Ungroup Clips"));
    assert_eq!(s.layers().len(), layers);
}

#[test]
fn clips_move_out_of_a_group_and_back_in() {
    let Some(s) = three_drawings() else { return };
    let (g, clips) = grouped(&s);
    let red = played(&s, 0);
    assert!(s.move_timeline_clip_out_of_group(clips[0]));
    assert_eq!(group_of(&s, clips[0]), None);
    assert_eq!(played(&s, 0), red, "it shows where it did");
    assert_eq!(s.undo().as_deref(), Some("Move Out of Group"));
    assert_eq!(group_of(&s, clips[0]), Some(g));
    assert!(s.move_timeline_clip_out_of_group(clips[0]));
    assert!(s.move_timeline_clip_into_group(clips[0], g));
    assert_eq!(group_of(&s, clips[0]), Some(g));
    assert!(!s.move_timeline_clip_into_group(g, g));
    // dragged onto a track inside the group from outside it
    let inner = {
        let st = s.shared.lock();
        let tl = st.doc.timeline.as_ref().unwrap();
        tl.track_of_clip(make2d_engine::timeline::ClipId(clips[0])).unwrap().0
    };
    assert!(s.move_timeline_clip(clips[2], inner, 0));
    assert_eq!(group_of(&s, clips[2]), Some(g));
}

#[test]
fn the_layers_panel_adds_a_layer_inside_the_drawing_at_the_playhead() {
    let Some(s) = three_drawings() else { return };
    let red = played(&s, 0);
    let root = s.timeline_tracks()[0].clips[0].root;
    let layers = s.layers().len();
    let made = s.add_layer();
    assert_ne!(made, 0);
    // the drawing became a group of its layer and the new one
    let group = s.timeline_tracks()[0].clips[0].root;
    assert_ne!(group, root);
    let info = s.layers();
    assert_eq!(info.len(), layers + 2);
    let parent = |id: u64| info.iter().find(|l| l.id == id).and_then(|l| l.parent);
    assert_eq!((parent(made), parent(root)), (Some(group), Some(group)));
    assert_eq!(played(&s, 0), red);
    // again: in the same group
    let another = s.add_layer();
    assert_eq!(s.layers().iter().find(|l| l.id == another).and_then(|l| l.parent), Some(group));
    // where the track drawn on has no drawing, a new drawing
    s.set_timeline_simple(false);
    let empty = s.add_timeline_track();
    s.select_timeline_track(empty);
    let fresh = s.add_layer();
    let track = s.timeline_tracks().into_iter().find(|t| t.id == empty).unwrap();
    assert_eq!(track.clips.iter().map(|c| c.root).collect::<Vec<_>>(), [fresh]);
}

#[test]
fn clips_and_keys_paste_into_another_canvas_with_their_pixels() {
    let Some(a) = three_drawings() else { return };
    let Some(b) = session(60) else { return };
    assert!(b.start_timeline());
    let green = a.timeline_tracks()[0].clips[1].id;
    a.set_timeline_frame(1);
    assert!(a.set_timeline_key(green, TimelineChannel::Opacity, vec![0.5], TimelineEasing::Linear, true));
    assert!(a.copy_timeline_clips(vec![green]));
    assert_eq!(b.timeline_clipboard().clips, 1);
    b.set_timeline_frame(0);
    let layers = b.layers().len();
    let pasted = b.paste_timeline_clips();
    assert_eq!(pasted.len(), 1);
    assert_eq!(b.layers().len(), layers + 1);
    let at = b.timeline_tracks()[0].clips.iter().find(|c| c.id == pasted[0]).map(|c| c.start).unwrap();
    // its dot about the middle of the bigger canvas: 10 across and down from where it was
    let px = played(&b, at);
    let (dot, paper) = (pixel(&px, 60, 30, 30), pixel(&px, 60, 5, 5));
    assert!(dot[1] > dot[0] + 40 && dot[1] > dot[2] + 40, "green: {dot:?}");
    assert_eq!(pixel(&px, 60, 20, 20), paper, "not where it was");
    // half faded, the paper showing through: its keys came along
    assert!(dot[0] > 60, "{dot:?}");
    assert_eq!(b.undo().as_deref(), Some("Paste Clips"));
    assert_eq!(b.layers().len(), layers);
    assert_eq!(b.redo().as_deref(), Some("Paste Clips"));
    assert_eq!(pixel(&played(&b, at), 60, 30, 30), dot, "back with its pixels");
    // keys alone too
    assert!(a.copy_timeline_keys(green, false));
    assert!(b.timeline_clipboard().keys);
    let mine = b.timeline_tracks()[0].clips[0].id;
    b.set_timeline_frame(0);
    assert!(b.paste_timeline_keys());
    assert_eq!(b.timeline_keys(mine).len(), 1);
}

fn media_file(name: &str, seed: u8) -> String {
    let dir = std::env::temp_dir().join(format!("make2d-edits-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(name);
    std::fs::write(&path, (0..3000_u32).map(|i| (i as u8).wrapping_mul(seed)).collect::<Vec<_>>()).unwrap();
    path.to_string_lossy().into_owned()
}

fn import(s: &CanvasSession, path: &str, kind: TimelineMediaKind, seconds: f64) -> u64 {
    let (width, height) = if kind == TimelineMediaKind::Video { (32, 18) } else { (0, 0) };
    let name = Path::new(path).file_name().unwrap().to_string_lossy().into_owned();
    s.import_timeline_media(MediaImport { path: path.to_owned(), name, kind, seconds, width, height, peaks: vec![] })
        .unwrap()
}

#[test]
fn sounds_and_videos_split_and_duplicate() {
    let Some(s) = session(32) else { return };
    let sound = import(&s, &media_file("talk.wav", 7), TimelineMediaKind::Audio, 2.0);
    let fps = f64::from(s.timeline_info().unwrap().fps);
    s.set_timeline_frame(5);
    assert!(s.split_timeline_clip(sound));
    let track = s.timeline_tracks().into_iter().find(|t| t.clips.iter().any(|c| c.id == sound)).unwrap();
    assert_eq!(track.clips.len(), 2);
    let rest = s.timeline_media_clip(track.clips[1].id).unwrap();
    assert!((rest.trim_start - 5.0 / fps).abs() < 1e-9);
    let sounds = s.timeline_sounds(0);
    assert_eq!(sounds.len(), 2);
    assert!((sounds[1].at - sounds[0].at - sounds[0].length).abs() < 1e-9, "one after the other, seamless");
    assert_eq!(s.undo().as_deref(), Some("Split"));
    assert_ne!(s.duplicate_timeline_clip(sound), 0);
    assert_eq!(s.timeline_sounds(0).len(), 2);
    assert_eq!(s.undo().as_deref(), Some("Duplicate"));
    // a video likewise
    // (brought in at the playhead, frame 5)
    let video = import(&s, &media_file("board.mov", 11), TimelineMediaKind::Video, 1.0);
    s.set_timeline_frame(8);
    assert!(s.split_timeline_clip(video));
    let copy = s.duplicate_timeline_clip(video);
    assert_ne!(copy, 0);
    assert_eq!(s.timeline_media_clip(copy).unwrap().trim_start, 0.0, "a copy of the first piece");
}

#[test]
fn a_sounds_level_is_keyed_and_played_by_frame() {
    let Some(s) = session(32) else { return };
    let sound = import(&s, &media_file("music.wav", 5), TimelineMediaKind::Audio, 2.0);
    s.set_timeline_frame(0);
    assert!(s.set_timeline_volume_key(sound, 0.0, true));
    s.set_timeline_frame(4);
    assert!(s.set_timeline_volume_key(sound, 1.0, true));
    let c = s.timeline_media_clip(sound).unwrap();
    assert_eq!((c.volume_keys.clone(), c.level, c.keyed_here), (vec![0, 4], 1.0, true));
    let played = &s.timeline_sounds(0)[0];
    assert_eq!(played.levels_from, 0);
    assert_eq!(played.levels[..6], [0.0, 0.25, 0.5, 0.75, 1.0, 1.0]);
    assert_eq!(played.gain, 0.0, "at the frame asked for");
    s.set_timeline_frame(2);
    assert_eq!(s.timeline_media_clip(sound).unwrap().level, 0.5);
    assert!(!s.remove_timeline_volume_key(sound), "none here");
    s.set_timeline_frame(4);
    assert!(s.remove_timeline_volume_key(sound));
    assert_eq!(s.timeline_media_clip(sound).unwrap().volume_keys, [0]);
    assert_eq!(s.undo().as_deref(), Some("Remove Level Key"));
}

#[test]
fn an_edge_dragged_is_previewed_then_trimmed_in_one_undo_step() {
    use super::edits::TimelineEdgePolicy as Policy;
    let Some(s) = three_drawings() else { return };
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);
    // the preview says what would be, and changes nothing
    let shown = s.timeline_trim_preview(clips[0], false, 4, Policy::Push);
    assert_eq!(shown.iter().map(|c| (c.start, c.duration)).collect::<Vec<_>>(), [(0, 4), (4, 1), (5, 1)]);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);
    assert!(s.timeline_trim_preview(clips[0], false, 4, Policy::Stop).is_empty(), "the next clip is in the way");
    // let go: as previewed
    assert!(s.trim_timeline_clip(clips[0], false, 4, Policy::Push));
    assert_eq!(spans(&s, 0), [(0, 4), (4, 1), (5, 1)]);
    // its start into nothing, then the next clip's start into it
    assert!(s.trim_timeline_clip(clips[0], true, 2, Policy::Stop));
    assert!(s.trim_timeline_clip(clips[1], true, 3, Policy::Trim));
    assert_eq!(spans(&s, 0), [(2, 1), (3, 2), (5, 1)]);
    assert_eq!(s.undo().as_deref(), Some("Trim"));
    assert_eq!(spans(&s, 0), [(2, 2), (4, 1), (5, 1)]);
    // a locked track's clips keep their edges
    let track = s.timeline_tracks()[0].id;
    assert!(s.set_timeline_track(track, false, true));
    assert!(!s.trim_timeline_clip(clips[0], false, 9, Policy::Push));
    assert!(s.timeline_trim_preview(clips[0], false, 9, Policy::Push).is_empty());
    assert_eq!(s.timeline_move_landing(clips[0], track, 8), None);
}

#[test]
fn a_dragged_clip_is_told_where_it_would_land() {
    let Some(s) = three_drawings() else { return };
    let track = s.timeline_tracks()[0].id;
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    assert!(s.set_timeline_clip_duration(clips[2], 4, true));
    // onto the last clip (2..6): the nearest room is just before it or after it
    assert_eq!(s.timeline_move_landing(clips[0], track, 5), Some(6));
    assert_eq!(s.timeline_move_landing(clips[0], track, 12), Some(12));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 4)], "nothing moved");
    assert!(s.move_timeline_clip(clips[0], track, 5));
    assert_eq!(s.timeline_tracks()[0].clips.iter().find(|c| c.id == clips[0]).map(|c| c.start), Some(6));
}

#[test]
fn clips_picked_together_move_and_are_deleted_in_one_undo_step_each() {
    let Some(s) = three_drawings() else { return };
    let track = s.timeline_tracks()[0].id;
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    let layers = s.layers().len();
    // the last two go later together; the first stays, so earlier they stop at it
    assert_eq!(s.timeline_shift_room(vec![clips[1], clips[2]], 4), 4);
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)], "nothing moved");
    assert_eq!(s.shift_timeline_clips(vec![clips[1], clips[2]], 4), 4);
    assert_eq!(spans(&s, 0), [(0, 1), (5, 1), (6, 1)]);
    assert_eq!(s.timeline_shift_room(vec![clips[1], clips[2]], -9), -4);
    assert_eq!(s.undo().as_deref(), Some("Move Clips"));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);
    // deleted together, their layers with them, and back together
    assert!(s.delete_timeline_clips(vec![clips[0], clips[2]]));
    assert_eq!(spans(&s, 0), [(1, 1)]);
    assert_eq!(s.layers().len(), layers - 2);
    assert_eq!(s.undo().as_deref(), Some("Delete Clips"));
    assert_eq!(spans(&s, 0), [(0, 1), (1, 1), (2, 1)]);
    assert_eq!(s.layers().len(), layers);
    // a locked track's clips stay
    assert!(s.set_timeline_track(track, false, true));
    assert_eq!(s.shift_timeline_clips(vec![clips[2]], 3), 0);
    assert_eq!(s.timeline_shift_room(vec![clips[2]], 3), 0);
    assert!(!s.delete_timeline_clips(vec![clips[2]]));
}

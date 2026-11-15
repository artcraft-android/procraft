//! Timeline step 1g: retiming, sync markers and the track clipboard.

use std::collections::HashMap;

use super::*;

fn drawing(id: u64, start: u32, duration: u32, root: u64) -> Clip {
    Clip { id: ClipId(id), start, ..Clip::drawing("d", LayerId(root), start, duration) }
}

fn track(id: u64, clips: Vec<Clip>) -> Track {
    Track { id: TrackId(id), name: format!("T{id}"), hidden: false, locked: false, opacity: 1.0, clips }
}

fn spans(t: &Track) -> Vec<(u32, u32)> {
    t.clips.iter().map(|c| (c.start, c.duration)).collect()
}

fn timeline(tracks: Vec<Track>) -> Timeline {
    let mut tl = Timeline::new(12, 1);
    tl.tracks = tracks;
    tl.next_track = 10;
    tl.next_clip = 100;
    tl.fit();
    tl
}

fn opacity_key(frame: i32, value: f32) -> Key<f32> {
    Key { frame, value, easing: Easing::Linear, path: None }
}

#[test]
fn retiming_up_doubles_spans_and_keys() {
    let mut keyed = drawing(3, 2, 2, 3);
    keyed.keys.opacity.keys = vec![opacity_key(0, 0.0), opacity_key(1, 1.0)];
    let mut tl = timeline(vec![track(1, vec![drawing(1, 0, 1, 1), drawing(2, 1, 1, 2), keyed])]);
    tl.camera.keys = vec![Key { frame: 3, value: CameraPose::still(10.0, 10.0), easing: Easing::Linear, path: None }];
    tl.markers = vec![1, 3];
    tl.loop_range = Some((1, 3));
    assert!(tl.retime(24));
    assert!(!tl.retime(24), "the same rate is no change");
    let t = tl.tracks.first().unwrap();
    assert_eq!(spans(t), [(0, 2), (2, 2), (4, 4)]);
    assert_eq!(t.clips.last().unwrap().keys.opacity.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [0, 2]);
    assert_eq!(tl.camera.keys.first().unwrap().frame, 6);
    assert_eq!((tl.fps, tl.duration, tl.markers.clone(), tl.loop_range), (24, 8, vec![2, 6], Some((2, 6))));
}

#[test]
fn retiming_down_keeps_clips_apart_and_the_later_key() {
    let mut keyed = drawing(5, 3, 1, 5);
    keyed.keys.opacity.keys = vec![opacity_key(0, 0.0), opacity_key(1, 0.5), opacity_key(2, 1.0)];
    let clips = vec![drawing(1, 0, 1, 1), drawing(2, 1, 1, 2), drawing(3, 2, 1, 3), keyed];
    let mut tl = timeline(vec![track(1, clips)]);
    tl.markers = vec![0, 1, 2];
    assert!(tl.retime(6));
    let t = tl.tracks.first().unwrap();
    assert!(t.is_ordered(), "{:?}", spans(t));
    assert!(t.clips.iter().all(|c| c.duration >= 1));
    // 0, 1, 2 at half: 0, 1 (half rounds away from zero), 1, the later one kept
    let keys = &t.clips.last().unwrap().keys.opacity.keys;
    assert_eq!(keys.iter().map(|k| (k.frame, k.value)).collect::<Vec<_>>(), [(0, 0.0), (1, 1.0)]);
    assert_eq!(tl.markers, [0, 1], "markers on one frame become one");
}

#[test]
fn retiming_scales_groups_on_their_own_frames_and_keeps_sounds_in_seconds() {
    let inner = track(2, vec![drawing(1, 1, 2, 1)]);
    let group = Clip {
        content: ClipContent::Group(Group {
            tracks: vec![inner],
            collapsed: false,
            cycle: Some(Cycle { length: 3, keys: CycleKeys::Repeat }),
        }),
        ..drawing(2, 2, 4, 9)
    };
    let sound = Clip {
        content: ClipContent::Audio(AudioClip {
            media: MediaId(1),
            trim_start: 0.0,
            offset: 0.02,
            volume: 1.0,
            muted: false,
            volume_keys: Channel { keys: vec![opacity_key(4, 1.0)] },
        }),
        ..drawing(3, 5, 24, 0)
    };
    let mut tl = timeline(vec![track(1, vec![group]), track(3, vec![sound])]);
    let seconds = 5.0 / 12.0 + 0.02;
    assert!(tl.retime(25));
    let g = tl.tracks.first().and_then(|t| t.clips.first()).unwrap();
    assert_eq!((g.start, g.duration), (4, 9), "2 to 6 is 4.2 to 12.5");
    let ClipContent::Group(gr) = &g.content else { panic!() };
    assert_eq!(gr.cycle.map(|c| c.length), Some(6));
    assert_eq!(spans(gr.tracks.first().unwrap()), [(2, 4)]);
    let s = tl.tracks.get(1).and_then(|t| t.clips.first()).unwrap();
    let ClipContent::Audio(a) = &s.content else { panic!() };
    assert!((f64::from(s.start) / 25.0 + a.offset - seconds).abs() < 1e-9, "the sound starts when it did");
    assert!(a.offset >= 0.0 && a.offset < 1.0 / 25.0);
    assert_eq!(a.volume_keys.keys.first().map(|k| k.frame), Some(8));
}

#[test]
fn markers_stay_in_order_one_a_frame_within_the_timeline() {
    let mut tl = timeline(vec![track(1, vec![drawing(1, 0, 10, 1)])]);
    assert!(tl.add_marker(6));
    assert!(tl.add_marker(2));
    assert!(!tl.add_marker(6), "one a frame");
    assert!(tl.add_marker(50), "past the end: on the last frame");
    assert_eq!(tl.markers, [2, 6, 9]);
    assert!(tl.move_marker(2, 7));
    assert!(tl.move_marker(7, 6), "onto another: one");
    assert_eq!(tl.markers, [6, 9]);
    assert!(!tl.move_marker(3, 4), "none there");
    assert!(tl.remove_marker(9));
    assert!(!tl.remove_marker(9));
    assert_eq!(tl.markers, [6]);
}

#[test]
fn markers_are_read_from_older_files_as_none() {
    let tl = timeline(vec![track(1, vec![drawing(1, 0, 2, 1)])]);
    let mut json = serde_json::to_value(&tl).unwrap();
    json.as_object_mut().unwrap().remove("markers");
    let read: Timeline = serde_json::from_value(json).unwrap();
    assert!(read.markers.is_empty());
}

#[test]
fn copied_clips_paste_at_a_frame_across_tracks_in_step() {
    let mut keyed = drawing(2, 4, 2, 2);
    keyed.keys.opacity.keys = vec![opacity_key(0, 0.0), opacity_key(1, 1.0)];
    let mut tl = timeline(vec![
        track(1, vec![drawing(1, 2, 3, 1)]),
        track(2, vec![keyed]),
        track(3, vec![drawing(3, 0, 6, 3), drawing(4, 6, 2, 4)]),
    ]);
    let copied = tl.copy_clips(&[ClipId(1), ClipId(2)]).unwrap();
    assert_eq!(copied.roots(), [LayerId(1), LayerId(2)]);
    assert_eq!(
        copied.clips.iter().map(|c| (c.row, c.clip.start, c.clip.duration)).collect::<Vec<_>>(),
        [(0, 0, 3), (1, 2, 2)]
    );
    assert!(tl.copy_clips(&[ClipId(1), ClipId(77)]).is_none());
    // onto the top track at frame 3, inside a clip there: from just after it, and one row up a
    // new track
    let roots = HashMap::from([(LayerId(1), LayerId(11)), (LayerId(2), LayerId(12))]);
    let made = tl.paste_clips(&copied, TrackId(3), 3, &roots).unwrap();
    assert_eq!(made, [ClipId(100), ClipId(101)]);
    assert_eq!(tl.tracks.len(), 4);
    let top = tl.tracks.get(2).unwrap();
    assert_eq!(spans(top), [(0, 6), (6, 3), (9, 2)], "the clip after moves along");
    let new = tl.tracks.get(3).unwrap();
    assert_eq!((new.id, spans(new)), (TrackId(10), vec![(8, 2)]));
    let pasted = new.clips.first().unwrap();
    assert_eq!(pasted.keys.opacity.keys.len(), 2, "with its keys");
    assert!(matches!(&pasted.content, ClipContent::Drawing(d) if d.root == LayerId(12)));
    assert!(tl.duration >= 11);
}

#[test]
fn a_pasted_group_takes_new_ids_inside_and_a_locked_track_takes_none() {
    let inner = track(2, vec![drawing(1, 0, 2, 1)]);
    let group = Clip {
        content: ClipContent::Group(Group { tracks: vec![inner], collapsed: false, cycle: None }),
        ..drawing(2, 0, 2, 0)
    };
    let mut tl = timeline(vec![track(1, vec![group]), track(3, Vec::new())]);
    let copied = tl.copy_clips(&[ClipId(2)]).unwrap();
    assert_eq!(copied.roots(), [LayerId(1)]);
    let roots = HashMap::from([(LayerId(1), LayerId(5))]);
    let made = tl.paste_clips(&copied, TrackId(3), 4, &roots).unwrap();
    let pasted = tl.clip(*made.first().unwrap()).unwrap();
    let ClipContent::Group(g) = &pasted.content else { panic!() };
    let t = g.tracks.first().unwrap();
    assert_ne!(t.id, TrackId(2));
    assert_ne!(t.clips.first().unwrap().id, ClipId(1));
    assert_eq!(pasted.roots(), [LayerId(5)]);
    assert!(tl.drawing_of(LayerId(1)).is_some() && tl.drawing_of(LayerId(5)).is_some());
    if let Some(t) = tl.tracks.get_mut(0) {
        t.locked = true;
    }
    assert!(tl.paste_clips(&copied, TrackId(1), 9, &roots).is_none());
}

#[test]
fn keys_copy_from_a_frame_or_all_and_paste_at_a_frame() {
    let mut from = drawing(1, 0, 10, 1);
    from.keys.opacity.keys = vec![opacity_key(2, 0.2), opacity_key(5, 0.5)];
    from.keys.rotation.keys = vec![Key { frame: 5, value: 1.0, easing: Easing::Hold, path: None }];
    from.keys.pivot = Some([3.0, 4.0]);
    let mut to = drawing(2, 0, 10, 2);
    to.keys.opacity.keys = vec![opacity_key(4, 0.9)];
    let mut tl = timeline(vec![track(1, vec![from]), track(2, vec![to])]);
    assert!(tl.copy_keys(ClipId(1), Some(3)).is_none(), "none on that frame");
    let here = tl.copy_keys(ClipId(1), Some(5)).unwrap();
    assert_eq!(here.opacity.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [0]);
    assert_eq!(here.rotation.keys.first().map(|k| (k.frame, k.easing.clone())), Some((0, Easing::Hold)));
    let all = tl.copy_keys(ClipId(1), None).unwrap();
    assert_eq!(all.opacity.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [0, 3]);
    assert!(tl.paste_keys(ClipId(2), &all, 1));
    let keys = &tl.clip(ClipId(2)).unwrap().keys;
    assert_eq!(keys.opacity.keys.iter().map(|k| (k.frame, k.value)).collect::<Vec<_>>(), [(1, 0.2), (4, 0.5)]);
    assert_eq!(keys.rotation.keys.first().map(|k| k.frame), Some(4));
    assert_eq!(keys.pivot, Some([3.0, 4.0]), "the pivot comes with keys that turn it");
    assert!(!tl.paste_keys(ClipId(2), &ClipKeys::default(), 0));
}

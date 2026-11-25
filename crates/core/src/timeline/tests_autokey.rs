//! Autokey through the session: the transform tool keying the pose of the drawing at the playhead.

use super::tests_1g::{played, three_drawings};
use super::{TimelineChannel, TimelineEasing};
use crate::CanvasSession;
use crate::transform::{TransformAction, TransformBox, TransformHandle};

/// The first track's first clip held five frames, the playhead on its last, autokey on.
fn held_five(s: &CanvasSession) -> u64 {
    s.set_timeline_simple(false);
    let clip = s.timeline_tracks()[0].clips[0].id;
    assert!(s.set_timeline_clip_duration(clip, 5, true));
    s.set_timeline_frame(4);
    s.set_timeline_autokey(true, TimelineEasing::Linear);
    clip
}

/// The middle of the box.
fn middle(b: &TransformBox) -> (f32, f32) {
    let c = &b.corners;
    ((c[0] + c[4]) / 2.0, (c[1] + c[5]) / 2.0)
}

/// Clip `id`'s keys of `channel`, by the timeline's frame.
fn keyed(s: &CanvasSession, id: u64, channel: TimelineChannel) -> Vec<u32> {
    s.timeline_keys(id).into_iter().filter(|k| k.channel == channel).map(|k| k.frame).collect()
}

/// Where the red of frame `frame` is, as it plays: the middle of its reddest pixels.
fn red_at(s: &CanvasSession, frame: u32) -> (f32, f32) {
    let px = played(s, frame);
    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
    for (i, p) in px.as_chunks::<4>().0.iter().enumerate() {
        if p[0] > 150 && p[1] < 80 && p[2] < 80 {
            sx += (i % 40) as f32;
            sy += (i / 40) as f32;
            n += 1.0;
        }
    }
    assert!(n > 0.0, "no red on frame {frame}");
    (sx / n, sy / n)
}

#[test]
fn a_drag_keys_where_it_moves_the_drawing_at_the_playhead_as_one_step() {
    let Some(s) = three_drawings() else { return };
    let clip = held_five(&s);
    let (x0, y0) = red_at(&s, 4);
    let b = s.transform_begin().expect("a box around the red drawing");
    assert!(s.timeline_autokey_posing());
    assert!(s.timeline_keys(clip).is_empty(), "opening it keys nothing");
    let (cx, cy) = middle(&b);
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx + 5.0, cy);
    s.transform_drag(cx + 10.0, cy + 4.0);
    s.transform_release();
    // the playhead's frame keyed, and the clip's first with where it was
    assert_eq!(keyed(&s, clip, TimelineChannel::Position), [0, 4]);
    assert!(
        keyed(&s, clip, TimelineChannel::Scale).is_empty() && keyed(&s, clip, TimelineChannel::Rotation).is_empty()
    );
    s.transform_end(true);
    assert!(!s.timeline_autokey_posing());
    // it plays: still on the first frame, moved on the last
    let (x, y) = red_at(&s, 4);
    assert!((x - x0 - 10.0).abs() < 1.0 && (y - y0 - 4.0).abs() < 1.0, "({x0}, {y0}) to ({x}, {y})");
    let (x, y) = red_at(&s, 0);
    assert!((x - x0).abs() < 1.0 && (y - y0).abs() < 1.0);
    let (x, _) = red_at(&s, 2);
    assert!((x - x0 - 5.0).abs() < 1.0, "half way at {x}");
    // the pixels didn't move: the drawing's own layer is as it was
    assert_eq!(s.undo().as_deref(), Some("Key"));
    assert!(s.timeline_keys(clip).is_empty());
    let (x, _) = red_at(&s, 4);
    assert!((x - x0).abs() < 1.0);
}

#[test]
fn the_rotate_node_keys_a_turn_and_a_corner_the_size() {
    let Some(s) = three_drawings() else { return };
    let clip = held_five(&s);
    let b = s.transform_begin().unwrap();
    let (cx, cy) = middle(&b);
    s.transform_grab(TransformHandle::Rotate, cx, cy - 10.0);
    s.transform_drag(cx + 10.0, cy);
    s.transform_release();
    assert_eq!(keyed(&s, clip, TimelineChannel::Rotation), [0, 4]);
    assert!(keyed(&s, clip, TimelineChannel::Position).is_empty(), "turned about its middle");
    let pose = s.timeline_pose(clip).unwrap();
    assert!((pose.rotation - 90.0).abs() < 0.5, "{pose:?}");
    // a corner pulled out twice as far from the middle: twice the size
    let b = s.timeline_autokey_box().unwrap();
    let (cx, cy) = middle(&b);
    let (x, y) = (b.corners[4], b.corners[5]);
    s.transform_grab(TransformHandle::Corner { index: 2 }, x, y);
    s.transform_drag(2.0 * x - cx, 2.0 * y - cy);
    s.transform_release();
    assert_eq!(keyed(&s, clip, TimelineChannel::Scale), [0, 4]);
    let pose = s.timeline_pose(clip).unwrap();
    assert!((pose.rotation - 90.0).abs() < 0.5, "still turned: {pose:?}");
    assert!(pose.scale_x > 1.2 && pose.scale_y > 1.2, "{pose:?}");
    s.transform_end(true);
}

#[test]
fn its_undo_takes_back_its_own_keys_and_cancelling_puts_back_the_rest() {
    let Some(s) = three_drawings() else { return };
    let clip = held_five(&s);
    // a key the transform didn't make
    assert!(s.set_timeline_key(clip, TimelineChannel::Opacity, vec![0.5], TimelineEasing::Linear, true));
    let b = s.transform_begin().unwrap();
    let (cx, cy) = middle(&b);
    for step in 1..=2 {
        let from = (cx + 3.0 * (step - 1) as f32, cy);
        s.transform_grab(TransformHandle::Move, from.0, from.1);
        s.transform_drag(from.0 + 3.0, from.1);
        s.transform_release();
    }
    assert!((s.timeline_pose(clip).unwrap().x - 6.0).abs() < 0.1);
    // the transform's undo: its last drag back, the box with it
    let undone = s.transform_apply(TransformAction::Undo).unwrap();
    assert!((s.timeline_pose(clip).unwrap().x - 3.0).abs() < 0.1);
    assert!((middle(&undone).0 - cx - 3.0).abs() < 0.1, "{undone:?}");
    s.transform_apply(TransformAction::Undo);
    assert!(keyed(&s, clip, TimelineChannel::Position).is_empty());
    // with none of its own left, it takes back nothing else
    s.transform_apply(TransformAction::Undo);
    assert_eq!(keyed(&s, clip, TimelineChannel::Opacity), [4]);
    // cancelled, what it keyed goes
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx + 4.0, cy);
    s.transform_release();
    assert_eq!(keyed(&s, clip, TimelineChannel::Position), [0, 4]);
    s.transform_end(false);
    assert!(keyed(&s, clip, TimelineChannel::Position).is_empty());
    assert_eq!(keyed(&s, clip, TimelineChannel::Opacity), [4]);
}

#[test]
fn a_key_already_there_keeps_how_it_eases_and_the_box_follows_the_playhead() {
    let Some(s) = three_drawings() else { return };
    let clip = held_five(&s);
    s.set_timeline_frame(0);
    assert!(s.set_timeline_key(clip, TimelineChannel::Position, vec![0.0, 0.0], TimelineEasing::Hold, true));
    s.set_timeline_frame(4);
    assert!(s.set_timeline_key(clip, TimelineChannel::Position, vec![8.0, 0.0], TimelineEasing::Hold, true));
    let b = s.transform_begin().unwrap();
    let at_four = middle(&b);
    // the playhead back on the first frame: the box laid where the clip shows there
    s.set_timeline_frame(0);
    let b = s.timeline_autokey_box().unwrap();
    assert!((middle(&b).0 - at_four.0 + 8.0).abs() < 0.1, "{b:?}");
    let (cx, cy) = middle(&b);
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx, cy + 2.0);
    s.transform_release();
    let keys = s.timeline_keys(clip);
    let first = keys.iter().find(|k| k.frame == 0 && k.channel == TimelineChannel::Position).unwrap();
    assert_eq!(first.easing, TimelineEasing::Hold);
    assert!((s.timeline_pose(clip).unwrap().y - 2.0).abs() < 0.1);
    s.transform_end(true);
}

#[test]
fn with_nothing_at_the_playhead_it_says_so_and_without_autokey_the_pixels_move() {
    let Some(s) = three_drawings() else { return };
    let clip = held_five(&s);
    // a new track, drawn on, with nothing on it
    let track = s.add_timeline_track();
    assert_ne!(track, 0);
    assert!(s.transform_begin().is_none());
    assert_eq!(s.transform_refusal(), "No drawing at the playhead to key");
    // autokey off: the pixels are transformed, as ever
    s.select_timeline_track(s.timeline_tracks()[0].id);
    s.set_timeline_autokey(false, TimelineEasing::Linear);
    assert!(s.transform_begin().is_some());
    assert!(!s.timeline_autokey_posing());
    s.transform_end(false);
    assert!(s.timeline_keys(clip).is_empty());
}

#[test]
fn a_group_picked_in_the_panel_is_keyed_rather_than_the_drawing_in_it() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    let red = s.timeline_tracks()[0].clips[0].id;
    assert!(s.set_timeline_clip_duration(red, 5, true));
    let group = s.group_timeline_clips(vec![red]);
    assert_ne!(group, 0);
    s.set_timeline_frame(4);
    s.set_timeline_autokey_clip(Some(group));
    s.set_timeline_autokey(true, TimelineEasing::Linear);
    let (x0, y0) = red_at(&s, 4);
    let b = s.transform_begin().expect("a box around what the group shows");
    let (cx, cy) = middle(&b);
    assert!((cx - x0).abs() < 2.0 && (cy - y0).abs() < 2.0, "laid over the red: {b:?}");
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx + 10.0, cy + 4.0);
    s.transform_release();
    s.transform_end(true);
    assert_eq!(keyed(&s, group, TimelineChannel::Position), [0, 4]);
    assert!(s.timeline_keys(red).is_empty(), "the drawing in it keeps no keys of its own");
    let (x, y) = red_at(&s, 4);
    assert!((x - x0 - 10.0).abs() < 1.0 && (y - y0 - 4.0).abs() < 1.0, "({x0}, {y0}) to ({x}, {y})");
    // the group picked but not at the playhead: the drawing there is keyed, the group left be
    s.set_timeline_frame(6);
    let b = s.transform_begin().expect("a box around the drawing at the playhead");
    let (cx, cy) = middle(&b);
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx + 3.0, cy);
    s.transform_release();
    s.transform_end(true);
    assert_eq!(keyed(&s, group, TimelineChannel::Position), [0, 4]);
}

#[test]
fn with_autokey_on_the_clip_opacity_is_keyed_at_the_playhead() {
    let Some(s) = three_drawings() else { return };
    let clip = held_five(&s);
    assert!(s.autokey_timeline_clip_opacity(clip, 0.4, false));
    assert!(s.autokey_timeline_clip_opacity(clip, 0.25, true));
    assert_eq!(keyed(&s, clip, TimelineChannel::Opacity), [0, 4]);
    assert!((s.timeline_clip_opacity_here(clip) - 0.25).abs() < 1e-4);
    s.set_timeline_frame(0);
    assert!((s.timeline_clip_opacity_here(clip) - 1.0).abs() < 1e-4, "its start keeps what it had");
    assert_eq!(s.undo().as_deref(), Some("Key Opacity"));
    assert!(s.timeline_keys(clip).is_empty());
    // off, nothing keyed: the opacity is the clip's own to set
    s.set_timeline_autokey(false, TimelineEasing::Linear);
    assert!(!s.autokey_timeline_clip_opacity(clip, 0.5, true));
}

#[test]
fn the_keys_page_easing_reaches_autokey_without_turning_it_off_and_on() {
    let Some(s) = three_drawings() else { return };
    let clip = held_five(&s);
    s.set_timeline_autokey_easing(TimelineEasing::Hold);
    let b = s.transform_begin().unwrap();
    let (cx, cy) = middle(&b);
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx + 6.0, cy);
    s.transform_release();
    s.transform_end(true);
    let keys = s.timeline_keys(clip);
    let last = keys.iter().find(|k| k.frame == 4 && k.channel == TimelineChannel::Position).unwrap();
    assert_eq!(last.easing, TimelineEasing::Hold);
}

/// A video one second long brought into `s` at the playhead, its track the one drawn on.
fn video(s: &CanvasSession, name: &str) -> u64 {
    let dir = std::env::temp_dir().join(format!("procraft-autokey-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("clip.mov");
    std::fs::write(&path, name.as_bytes().repeat(200)).unwrap();
    let clip = s
        .import_timeline_media(crate::media::MediaImport {
            path: path.to_string_lossy().into_owned(),
            name: "clip.mov".into(),
            kind: crate::media::TimelineMediaKind::Video,
            seconds: 1.0,
            width: 20,
            height: 10,
            peaks: Vec::new(),
            sound_path: None,
            sound_seconds: 0.0,
        })
        .unwrap();
    assert_ne!(clip, 0);
    clip
}

#[test]
fn a_video_on_the_track_drawn_on_is_placed_whole_without_autokey_and_keyed_with_it() {
    let Some(s) = three_drawings() else { return };
    s.set_timeline_simple(false);
    s.set_timeline_frame(0);
    let clip = video(&s, "placed");
    // its track is the one drawn on; three frames in, nothing picked, autokey off
    s.set_timeline_frame(3);
    assert!(!s.timeline_autokey_posing());
    let rect = s.timeline_media_clip(clip).unwrap().rect;
    let b = s.transform_begin().expect("a box around the video's picture");
    assert!(s.timeline_autokey_posing(), "a video is posed, having no pixels of the canvas's");
    let (cx, cy) = middle(&b);
    assert!((cx - (rect[0] + rect[2] / 2.0)).abs() < 1.0 && (cy - (rect[1] + rect[3] / 2.0)).abs() < 1.0);
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx + 6.0, cy + 2.0);
    s.transform_release();
    // one key, on its first frame: it sits there for its whole length
    assert_eq!(keyed(&s, clip, TimelineChannel::Position), [0]);
    // moved again, from another frame: still the one key
    s.set_timeline_frame(5);
    let b = s.timeline_autokey_box().expect("the box follows the playhead");
    let (cx, cy) = middle(&b);
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx - 2.0, cy);
    s.transform_release();
    s.transform_end(true);
    assert_eq!(keyed(&s, clip, TimelineChannel::Position), [0]);
    let pose = |frame: u32| {
        s.set_timeline_frame(frame);
        let p = s.timeline_pose(clip).unwrap();
        (p.x, p.y)
    };
    for frame in [0, 3, 5, 9] {
        let (x, y) = pose(frame);
        assert!((x - 4.0).abs() < 0.01 && (y - 2.0).abs() < 0.01, "frame {frame}: ({x}, {y})");
    }
    // each drag was a step of its own, the video's rect untouched
    assert_eq!(s.timeline_media_clip(clip).unwrap().rect, rect);
    assert_eq!(s.undo().as_deref(), Some("Key"));
    assert_eq!(pose(3), (6.0, 2.0));
    assert!(s.redo().is_some());

    // with autokey on it is keyed at the playhead, picked in the panel or not
    s.set_timeline_autokey(true, TimelineEasing::Linear);
    s.set_timeline_frame(6);
    let b = s.transform_begin().expect("a box around the video, unpicked");
    let (cx, cy) = middle(&b);
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx + 10.0, cy);
    s.transform_release();
    s.transform_end(true);
    assert_eq!(keyed(&s, clip, TimelineChannel::Position), [0, 6]);
    assert!((pose(0).0 - 4.0).abs() < 0.01 && (pose(6).0 - 14.0).abs() < 0.01 && (pose(3).0 - 9.0).abs() < 0.01);

    // autokey off again: an animated channel is keyed at the playhead still, a still one placed whole
    s.set_timeline_autokey(false, TimelineEasing::Linear);
    s.set_timeline_frame(6);
    let b = s.transform_begin().expect("a box around the video");
    let (cx, cy) = middle(&b);
    s.transform_grab(TransformHandle::Move, cx, cy);
    s.transform_drag(cx, cy + 5.0);
    s.transform_release();
    s.transform_end(true);
    assert_eq!(keyed(&s, clip, TimelineChannel::Position), [0, 6]);
    assert!((pose(6).1 - 7.0).abs() < 0.01 && (pose(0).1 - 2.0).abs() < 0.01);

    // on a drawing's track without autokey, the pixels move as ever
    let drawing = s.timeline_tracks().into_iter().find(|t| t.clips.iter().any(|c| c.root != 0)).unwrap();
    s.select_timeline_track(drawing.id);
    s.set_timeline_frame(0);
    assert!(s.transform_begin().is_some());
    assert!(!s.timeline_autokey_posing());
    s.transform_end(false);

    // back on the video's track, past the video's end: nothing there, and the drawing that was
    // current (another frame's) is not what the transform takes up
    let track = s.timeline_tracks().into_iter().find(|t| t.clips.iter().any(|c| c.id == clip)).unwrap();
    s.select_timeline_track(track.id);
    let end = track.clips.iter().find(|c| c.id == clip).map(|c| c.start + c.duration).unwrap();
    s.rest_timeline_frame(end + 2);
    assert!(s.transform_begin().is_none());
    assert_eq!(s.transform_refusal(), "Nothing on this track at this frame to transform");
}

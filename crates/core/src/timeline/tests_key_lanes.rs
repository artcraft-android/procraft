//! The key lanes' edits through the session: each channel's keys apart, on several clips, a
//! sound's volume and the camera at once, previewed without a change, and each edit one undo step.

use std::path::Path;

use super::key_lanes::{TimelineKeyEdit, TimelineKeyLane, TimelineKeyPick};
use super::tests_1g::{session, three_drawings};
use super::{TimelineChannel, TimelineEasing};
use crate::CanvasSession;
use crate::media::{MediaImport, TimelineMediaKind};

/// The first clip six frames long, opacity keyed on frames 0, 2 and 4 and rotation on 2.
fn keyed(s: &CanvasSession) -> u64 {
    s.set_timeline_simple(false);
    let c = s.timeline_tracks()[0].clips[0].id;
    assert!(s.set_timeline_clip_duration(c, 6, true));
    for (f, o) in [(0, 0.0), (2, 0.5), (4, 1.0)] {
        s.set_timeline_frame(f);
        assert!(s.set_timeline_key(c, TimelineChannel::Opacity, vec![o], TimelineEasing::Linear, true));
    }
    s.set_timeline_frame(2);
    assert!(s.set_timeline_key(c, TimelineChannel::Rotation, vec![30.0], TimelineEasing::Linear, true));
    c
}

fn frames(s: &CanvasSession, c: u64, channel: TimelineChannel) -> Vec<u32> {
    s.timeline_keys(c).into_iter().filter(|k| k.channel == channel).map(|k| k.frame).collect()
}

fn lane_frames(s: &CanvasSession, lane: TimelineKeyLane) -> Vec<u32> {
    s.timeline_lane_keys(lane).into_iter().map(|k| k.frame).collect()
}

fn pick(c: u64, channel: TimelineChannel, frame: u32) -> TimelineKeyPick {
    TimelineKeyPick { lane: TimelineKeyLane::Clip { id: c, channel }, frame }
}

#[test]
fn a_lane_moves_one_channel_previewed_then_as_one_step() {
    let Some(s) = three_drawings() else { return };
    let c = keyed(&s);
    let picks = vec![pick(c, TimelineChannel::Opacity, 2)];
    // the preview shows it moved and changes nothing
    let shown = s.timeline_keys_edit_preview(picks.clone(), TimelineKeyEdit::Move { by: 1 }).unwrap();
    assert_eq!(shown.picks, [pick(c, TimelineChannel::Opacity, 3)]);
    assert_eq!(shown.lanes, [TimelineKeyLane::Clip { id: c, channel: TimelineChannel::Opacity }]);
    assert_eq!(shown.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [0, 3, 4]);
    assert_eq!(frames(&s, c, TimelineChannel::Opacity), [0, 2, 4]);
    // let go: the opacity key moves, the rotation key on its frame stays
    assert_eq!(s.edit_timeline_keys(picks, TimelineKeyEdit::Move { by: 1 }), Some(vec![pick(
        c,
        TimelineChannel::Opacity,
        3
    )]));
    assert_eq!(frames(&s, c, TimelineChannel::Opacity), [0, 3, 4]);
    assert_eq!(frames(&s, c, TimelineChannel::Rotation), [2]);
    assert_eq!(s.undo().as_deref(), Some("Move Keys"));
    assert_eq!(frames(&s, c, TimelineChannel::Opacity), [0, 2, 4]);
    // no farther than the clip's last frame
    let last = vec![pick(c, TimelineChannel::Opacity, 4)];
    assert_eq!(s.edit_timeline_keys(last, TimelineKeyEdit::Move { by: 9 }), Some(vec![pick(
        c,
        TimelineChannel::Opacity,
        5
    )]));
}

#[test]
fn a_lane_removes_one_channels_key() {
    let Some(s) = three_drawings() else { return };
    let c = keyed(&s);
    assert_eq!(s.edit_timeline_keys(vec![pick(c, TimelineChannel::Rotation, 2)], TimelineKeyEdit::Remove), Some(vec![]));
    assert!(frames(&s, c, TimelineChannel::Rotation).is_empty());
    assert_eq!(frames(&s, c, TimelineChannel::Opacity), [0, 2, 4]);
    assert_eq!(s.undo().as_deref(), Some("Remove Keys"));
}

#[test]
fn lanes_stretch_reverse_and_ease() {
    let Some(s) = three_drawings() else { return };
    let c = keyed(&s);
    let all =
        vec![pick(c, TimelineChannel::Opacity, 0), pick(c, TimelineChannel::Opacity, 2), pick(c, TimelineChannel::Opacity, 4)];
    let stretched = s.edit_timeline_keys(all, TimelineKeyEdit::Stretch { anchor: 0, factor: 1.25 }).unwrap();
    assert_eq!(stretched.iter().map(|p| p.frame).collect::<Vec<_>>(), [0, 3, 5]);
    assert_eq!(frames(&s, c, TimelineChannel::Opacity), [0, 3, 5]);
    // reversed: what played at the end now plays at the start
    s.set_timeline_frame(0);
    assert!(s.edit_timeline_keys(stretched.clone(), TimelineKeyEdit::Reverse).is_some());
    assert!((s.timeline_pose(c).unwrap().opacity - 1.0).abs() < 1e-4);
    assert_eq!(s.undo().as_deref(), Some("Reverse Keys"));
    assert!(s.timeline_pose(c).unwrap().opacity.abs() < 1e-4);
    // eased: only the picked key's easing changes
    assert!(s.edit_timeline_keys(vec![pick(c, TimelineChannel::Opacity, 3)], TimelineKeyEdit::Ease {
        easing: TimelineEasing::Hold
    })
    .is_some());
    let easings: Vec<TimelineEasing> = s
        .timeline_keys(c)
        .into_iter()
        .filter(|k| k.channel == TimelineChannel::Opacity)
        .map(|k| k.easing)
        .collect();
    assert_eq!(easings, [TimelineEasing::Linear, TimelineEasing::Hold, TimelineEasing::Linear]);
}

#[test]
fn a_locked_tracks_keys_stay() {
    let Some(s) = three_drawings() else { return };
    let c = keyed(&s);
    let t = s.timeline_tracks()[0].clone();
    assert!(s.set_timeline_track(t.id, t.hidden, true));
    let picks = vec![pick(c, TimelineChannel::Opacity, 2)];
    assert_eq!(s.timeline_keys_edit_preview(picks.clone(), TimelineKeyEdit::Move { by: 1 }), None);
    assert_eq!(s.edit_timeline_keys(picks, TimelineKeyEdit::Remove), None);
}

#[test]
fn keys_on_two_clips_and_the_camera_go_together_as_one_step() {
    let Some(s) = three_drawings() else { return };
    let c = keyed(&s);
    // the second clip keyed on its first frame and the one after, the camera on frames 1 and 7
    let next = s.timeline_tracks()[0].clips[1].clone();
    assert!(s.set_timeline_clip_duration(next.id, 4, true));
    s.set_timeline_frame(next.start);
    assert!(s.set_timeline_key(next.id, TimelineChannel::Scale, vec![1.0, 1.0], TimelineEasing::Linear, true));
    s.set_timeline_frame(next.start + 1);
    assert!(s.set_timeline_key(next.id, TimelineChannel::Scale, vec![2.0, 2.0], TimelineEasing::Linear, true));
    for f in [1, 7] {
        s.set_timeline_frame(f);
        assert!(s.set_timeline_camera_key(10.0, 10.0, 1.0, 0.0, TimelineEasing::Smooth, true));
    }
    assert_eq!(lane_frames(&s, TimelineKeyLane::Camera), [1, 7]);
    assert_eq!(s.timeline_lane_keys(TimelineKeyLane::Camera)[0].easing, TimelineEasing::Smooth);
    let scale = TimelineKeyLane::Clip { id: next.id, channel: TimelineChannel::Scale };
    let picks = vec![
        pick(c, TimelineChannel::Rotation, 2),
        TimelineKeyPick { lane: scale, frame: next.start },
        TimelineKeyPick { lane: TimelineKeyLane::Camera, frame: 1 },
    ];
    // a frame on, shown first: the three lanes it changes, the rest as they are
    let shown = s.timeline_keys_edit_preview(picks.clone(), TimelineKeyEdit::Move { by: 1 }).unwrap();
    assert_eq!(shown.lanes.len(), 3);
    assert_eq!(shown.picks.iter().map(|p| p.frame).collect::<Vec<_>>(), [3, next.start + 1, 2]);
    assert_eq!(lane_frames(&s, TimelineKeyLane::Camera), [1, 7]);
    let ended = s.edit_timeline_keys(picks, TimelineKeyEdit::Move { by: 1 }).unwrap();
    assert_eq!(frames(&s, c, TimelineChannel::Rotation), [3]);
    // the scale key moved onto the one after it, taking its place
    assert_eq!(lane_frames(&s, scale), [next.start + 1]);
    assert_eq!(lane_frames(&s, TimelineKeyLane::Camera), [2, 7]);
    assert_eq!(s.undo().as_deref(), Some("Move Keys"));
    assert_eq!(lane_frames(&s, TimelineKeyLane::Camera), [1, 7]);
    assert_eq!(lane_frames(&s, scale), [next.start, next.start + 1]);
    assert_eq!(frames(&s, c, TimelineChannel::Rotation), [2]);
    // eased together, then taken away together
    assert!(s.redo().is_some());
    assert!(s.edit_timeline_keys(ended.clone(), TimelineKeyEdit::Ease { easing: TimelineEasing::Hold }).is_some());
    assert_eq!(s.timeline_lane_keys(TimelineKeyLane::Camera)[0].easing, TimelineEasing::Hold);
    assert_eq!(s.edit_timeline_keys(ended, TimelineKeyEdit::Remove), Some(vec![]));
    assert_eq!(lane_frames(&s, TimelineKeyLane::Camera), [7]);
    assert!(frames(&s, c, TimelineChannel::Rotation).is_empty());
    // a locked track's key stays while the camera's goes on
    let t = s.timeline_tracks()[0].clone();
    assert!(s.set_timeline_track(t.id, t.hidden, true));
    let picks = vec![pick(c, TimelineChannel::Opacity, 4), TimelineKeyPick { lane: TimelineKeyLane::Camera, frame: 7 }];
    assert_eq!(s.edit_timeline_keys(picks, TimelineKeyEdit::Move { by: -1 }), Some(vec![TimelineKeyPick {
        lane: TimelineKeyLane::Camera,
        frame: 6
    }]));
    assert_eq!(frames(&s, c, TimelineChannel::Opacity), [0, 2, 4]);
}

#[test]
fn a_sounds_volume_keys_are_a_lane() {
    let Some(s) = session(32) else { return };
    let dir = std::env::temp_dir().join(format!("procraft-lanes-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("level.wav");
    std::fs::write(&path, (0..3000_u32).map(|i| (i as u8).wrapping_mul(3)).collect::<Vec<_>>()).unwrap();
    let path = path.to_string_lossy().into_owned();
    let name = Path::new(&path).file_name().unwrap().to_string_lossy().into_owned();
    let sound = s
        .import_timeline_media(MediaImport {
            path,
            name,
            kind: TimelineMediaKind::Audio,
            seconds: 2.0,
            width: 0,
            height: 0,
            peaks: vec![],
        })
        .unwrap();
    for (f, v) in [(0, 0.0), (4, 1.0)] {
        s.set_timeline_frame(f);
        assert!(s.set_timeline_volume_key(sound, v, true));
    }
    let volume = TimelineKeyLane::Volume { id: sound };
    assert_eq!(lane_frames(&s, volume), [0, 4]);
    // the later key two frames sooner: the level reaches 1 at frame 2
    let picks = vec![TimelineKeyPick { lane: volume, frame: 4 }];
    assert_eq!(s.edit_timeline_keys(picks, TimelineKeyEdit::Move { by: -2 }), Some(vec![TimelineKeyPick {
        lane: volume,
        frame: 2
    }]));
    assert_eq!(s.timeline_media_clip(sound).unwrap().volume_keys, [0, 2]);
    s.set_timeline_frame(1);
    assert!((s.timeline_media_clip(sound).unwrap().level - 0.5).abs() < 1e-4);
    // reversed: loud first, then quiet
    let both = vec![TimelineKeyPick { lane: volume, frame: 0 }, TimelineKeyPick { lane: volume, frame: 2 }];
    assert!(s.edit_timeline_keys(both, TimelineKeyEdit::Reverse).is_some());
    s.set_timeline_frame(0);
    assert!((s.timeline_media_clip(sound).unwrap().level - 1.0).abs() < 1e-4);
    assert_eq!(s.undo().as_deref(), Some("Reverse Keys"));
    // a volume lane on a clip that isn't a sound has no keys
    let drawing = s.timeline_tracks().iter().flat_map(|t| t.clips.clone()).find(|c| c.id != sound).map(|c| c.id);
    if let Some(d) = drawing {
        assert!(s.timeline_lane_keys(TimelineKeyLane::Volume { id: d }).is_empty());
    }
}

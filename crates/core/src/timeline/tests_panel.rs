//! The panel's one reading: the same as the calls it stands for, numbered again only when
//! something in it changes, and a drawing's picture version changing only with that drawing.

use std::path::Path;

use super::key_lanes::TimelineKeyLane;
use super::menus::TimelineLoop;
use super::panel::TimelinePanel;
use super::tests_1g::three_drawings;
use super::{TimelineChannel, TimelineClipKind, TimelineEasing};
use crate::media::{MediaImport, TimelineMediaKind};
use crate::{CanvasSession, PenSample, PointerKind};

/// Three drawings, the first keyed, the last two grouped, the camera keyed, a marker, a loop and
/// a sound with its volume keyed on a track of its own.
fn busy() -> Option<std::sync::Arc<CanvasSession>> {
    let s = three_drawings()?;
    s.set_timeline_simple(false);
    let clips: Vec<u64> = s.timeline_tracks()[0].clips.iter().map(|c| c.id).collect();
    assert!(s.set_timeline_clip_duration(clips[0], 4, true));
    for (f, x) in [(0, 0.0), (3, 10.0)] {
        s.set_timeline_frame(f);
        assert!(s.set_timeline_key(clips[0], TimelineChannel::Position, vec![x, 0.0], TimelineEasing::Linear, true));
    }
    assert_ne!(s.group_timeline_clips(clips[1..].to_vec()), 0);
    s.set_timeline_frame(1);
    assert!(s.set_timeline_camera_key(20.0, 20.0, 1.2, 0.0, TimelineEasing::Smooth, true));
    assert!(s.add_timeline_marker(2));
    assert!(s.set_timeline_loop(Some(TimelineLoop { start: 0, end: 4 })));
    // the sound starts at the playhead
    s.set_timeline_frame(0);
    let dir = std::env::temp_dir().join(format!("procraft-panel-test-{}", std::process::id()));
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
            seconds: 1.0,
            width: 0,
            height: 0,
            peaks: vec![],
        })
        .unwrap();
    for (f, v) in [(0, 0.2), (3, 1.0)] {
        s.set_timeline_frame(f);
        assert!(s.set_timeline_volume_key(sound, v, true));
    }
    s.set_timeline_frame(0);
    Some(s)
}

/// The reading the panel made before, one call at a time.
fn one_at_a_time(s: &CanvasSession) -> TimelinePanel {
    let rows = s.timeline_rows();
    let mut keys = Vec::new();
    let mut volumes = Vec::new();
    let mut roots: Vec<u64> = Vec::new();
    let mut seen = Vec::new();
    for c in rows.iter().flat_map(|r| &r.track.clips) {
        if seen.contains(&c.id) {
            continue;
        }
        seen.push(c.id);
        let k = s.timeline_keys(c.id);
        if !k.is_empty() {
            keys.push(super::panel::TimelineClipKeys { id: c.id, keys: k });
        }
        if c.kind == TimelineClipKind::Audio {
            let k = s.timeline_lane_keys(TimelineKeyLane::Volume { id: c.id });
            if !k.is_empty() {
                volumes.push(super::panel::TimelineVolumeKeys { id: c.id, keys: k });
            }
        }
        if c.root != 0 && !roots.contains(&c.root) {
            roots.push(c.root);
        }
    }
    TimelinePanel {
        generation: 0,
        info: s.timeline_info(),
        rows,
        keys,
        camera: s.timeline_camera(),
        camera_keys: s.timeline_camera_keys(),
        markers: s.timeline_markers(),
        clipboard: s.timeline_clipboard(),
        loop_frames: s.timeline_loop(),
        onion: s.timeline_onion(),
        camera_lane: s.timeline_lane_keys(TimelineKeyLane::Camera),
        volumes,
        pictures: Vec::new(),
    }
}

/// The reading without its number and pictures, to set beside `one_at_a_time`'s.
fn without_pictures(mut p: TimelinePanel) -> TimelinePanel {
    p.generation = 0;
    p.pictures = Vec::new();
    p
}

fn versions(p: &TimelinePanel) -> Vec<(u64, u64)> {
    p.pictures.iter().map(|p| (p.root, p.version)).collect()
}

#[test]
fn the_reading_is_what_the_calls_one_at_a_time_give() {
    let Some(s) = busy() else { return };
    let p = s.timeline_panel(0).unwrap();
    let before = one_at_a_time(&s);
    // every part is there, and is the same
    assert!(p.info.is_some());
    assert!(p.rows.len() >= 3, "the group's own track unfolded under the sound's and the drawings'");
    assert_eq!(p.keys.len(), 1);
    assert_eq!(p.volumes.len(), 1);
    assert_eq!(p.camera_lane.len(), 1);
    assert_eq!(p.markers, [2]);
    assert_eq!(p.loop_frames, Some(TimelineLoop { start: 0, end: 4 }));
    assert_eq!(without_pictures(p.clone()), before);
    // each drawing once, in the rows' order: the first and the two inside the group (a group clip
    // has no drawing of its own)
    let roots: Vec<u64> = p.pictures.iter().map(|x| x.root).collect();
    let mut once = roots.clone();
    once.dedup();
    assert_eq!(roots, once);
    assert_eq!(roots.len(), 3);
    // off a timeline it is only that
    assert!(s.stop_timeline());
    let off = s.timeline_panel(p.generation).unwrap();
    assert_eq!(off.info, None);
    assert!(off.rows.is_empty() && off.pictures.is_empty() && off.keys.is_empty());
}

#[test]
fn a_reading_is_numbered_again_only_when_it_changes() {
    let Some(s) = busy() else { return };
    let first = s.timeline_panel(0).unwrap();
    // asked again with its number: nothing has changed
    assert_eq!(s.timeline_panel(first.generation), None);
    assert_eq!(s.timeline_panel(first.generation), None);
    // asked with another number, it is given again under the same one
    assert_eq!(s.timeline_panel(0).map(|p| p.generation), Some(first.generation));
    // the playhead moved: a new reading
    s.set_timeline_frame(2);
    let moved = s.timeline_panel(first.generation).unwrap();
    assert_eq!(moved.generation, first.generation + 1);
    assert_eq!(versions(&moved), versions(&first));
    // a key added
    let c = moved.rows.iter().flat_map(|r| &r.track.clips).find(|c| c.kind == TimelineClipKind::Drawing).unwrap().id;
    assert!(s.set_timeline_key(c, TimelineChannel::Rotation, vec![15.0], TimelineEasing::Linear, true));
    let keyed = s.timeline_panel(moved.generation).unwrap();
    assert_eq!(keyed.generation, moved.generation + 1);
    // undone: what it was, under a new number all the same
    assert!(s.undo().is_some());
    let undone = s.timeline_panel(keyed.generation).unwrap();
    assert_eq!(without_pictures(undone.clone()), without_pictures(moved.clone()));
    assert_eq!(undone.generation, keyed.generation + 1);
}

#[test]
fn a_drawing_drawn_on_changes_only_its_own_pictures_version() {
    let Some(s) = busy() else { return };
    let before = s.timeline_panel(0).unwrap();
    // drawn on at frame 0: the first drawing
    s.set_timeline_frame(0);
    let at = |x: f32| PenSample { x, y: 30.0, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: 0.0, roll: 0.0 };
    s.stroke_begin(PointerKind::Finger, vec![at(5.0)]);
    s.stroke_add(vec![at(15.0)]);
    s.stroke_end(vec![]);
    let after = s.timeline_panel(before.generation).unwrap();
    let (a, b) = (versions(&before), versions(&after));
    assert_eq!(a.len(), b.len());
    let changed: Vec<u64> = a.iter().zip(&b).filter(|(x, y)| x.1 != y.1).map(|(x, _)| x.0).collect();
    assert_eq!(changed.len(), 1, "only the drawing drawn on");
    let first = before
        .rows
        .iter()
        .flat_map(|r| &r.track.clips)
        .find(|c| c.start == 0 && c.kind == TimelineClipKind::Drawing)
        .unwrap();
    assert_eq!(changed, [first.root]);
    // undone, it changes again (a picture is drawn again rather than kept wrong)
    assert!(s.undo().is_some());
    let undone = s.timeline_panel(after.generation).unwrap();
    assert_ne!(versions(&undone), versions(&after));
}

use super::*;

use make2d_engine::view::View;
use make2d_gpu::Gpu;

/// A folder of its own for test `name`, emptied.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("make2d-media-test-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// A "sound file" of its own bytes (the core never decodes it).
fn sound_file(dir: &Path, name: &str, seed: u8) -> String {
    let path = dir.join(name);
    fs::write(&path, (0..4000_u32).map(|i| (i as u8).wrapping_mul(seed)).collect::<Vec<_>>()).unwrap();
    path.to_string_lossy().into_owned()
}

fn import_sound(s: &CanvasSession, path: &str, seconds: f64) -> u64 {
    s.import_timeline_media(MediaImport {
        path: path.to_owned(),
        name: "Line 1.wav".to_owned(),
        kind: TimelineMediaKind::Audio,
        seconds,
        width: 0,
        height: 0,
        peaks: vec![0.25, 1.0, 0.5],
    })
    .unwrap()
}

/// A session on the GPU (for undo), if there is one.
fn shown(canvas: u32) -> Option<Arc<CanvasSession>> {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let s = CanvasSession::new(canvas, canvas);
    {
        let mut st = s.shared.lock();
        st.window_size = (canvas, canvas);
        st.view = Some(View::fit((canvas, canvas), (canvas, canvas)));
        st.gpu = Some(gpu);
        st.ensure_canvas();
    }
    Some(s)
}

#[test]
fn a_sound_brought_in_plays_from_the_playhead_on_a_track_of_its_own() {
    let dir = folder("bring-in");
    let s = CanvasSession::new(64, 64);
    let path = sound_file(&dir, "line.wav", 3);
    let clip = import_sound(&s, &path, 2.0);
    assert_ne!(clip, 0);
    // put on a timeline to take it
    let info = s.timeline_info().unwrap();
    let tracks = s.timeline_tracks();
    let sound_track = tracks.iter().find(|t| t.clips.iter().any(|c| c.id == clip)).unwrap();
    assert_eq!(sound_track.name, "Line 1");
    let c = s.timeline_media_clip(clip).unwrap();
    assert_eq!((c.kind, c.media_seconds, c.volume, c.muted), (TimelineMediaKind::Audio, 2.0, 1.0, false));
    assert_eq!(sound_track.clips[0].duration, 2 * info.fps);
    // copied in, by its bytes
    let kept = PathBuf::from(c.path.unwrap());
    assert_eq!(fs::read(&kept).unwrap(), fs::read(&path).unwrap());
    assert!(kept.starts_with(waiting_folder()));
    assert_eq!(s.timeline_media_peaks(c.media), vec![0.25, 1.0, 0.5]);
    let sounds = s.timeline_sounds(0);
    assert_eq!(sounds.len(), 1);
    assert_eq!((sounds[0].at, sounds[0].from, sounds[0].length, sounds[0].gain), (0.0, 0.0, 2.0, 1.0));
    // levels, trims and slips
    assert!(s.set_timeline_sound(clip, 0.5, true, 0.01, true));
    assert_eq!(s.timeline_sounds(0)[0].gain, 0.0);
    assert!(s.trim_timeline_media_start(clip, 6, true));
    let c = s.timeline_media_clip(clip).unwrap();
    assert!((c.trim_start - 6.0 / f64::from(info.fps)).abs() < 1e-9 && c.volume == 0.5 && c.muted);
    assert!(s.slip_timeline_media(clip, 0.5, true));
    assert_eq!(s.timeline_media_clip(clip).unwrap().trim_start, 0.5);
    // the same file again is the same media
    let again = import_sound(&s, &path, 2.0);
    assert_eq!(s.timeline_media_clip(again).unwrap().media, s.timeline_media_clip(clip).unwrap().media);
    // something with no length is turned away
    let none = MediaImport {
        path: path.clone(),
        name: "x".into(),
        kind: TimelineMediaKind::Audio,
        seconds: 0.0,
        width: 0,
        height: 0,
        peaks: Vec::new(),
    };
    assert!(s.import_timeline_media(none).is_err());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn media_is_saved_in_the_artworks_folder_and_opens_from_it() {
    let dir = folder("save");
    let art = dir.join("Artwork");
    let s = CanvasSession::new(64, 64);
    let path = sound_file(&dir, "take.m4a", 5);
    let clip = import_sound(&s, &path, 1.5);
    let media = s.timeline_media_clip(clip).unwrap().media;
    s.save(art.to_string_lossy().into_owned()).unwrap();
    let name = format!("{media:016x}.m4a");
    assert_eq!(fs::read(art.join("media").join(&name)).unwrap(), fs::read(&path).unwrap());
    assert!(art.join("media").join(format!("{media:016x}.peaks")).exists());
    let opened = CanvasSession::open(art.to_string_lossy().into_owned()).unwrap();
    let c = opened.timeline_media_clip(clip).unwrap();
    assert_eq!(c.media_seconds, 1.5);
    // its file found in the artwork's folder first
    assert_eq!(PathBuf::from(c.path.unwrap()), art.join("media").join(&name));
    assert_eq!(opened.timeline_sounds(0).len(), 1);
    assert_eq!(opened.timeline_media_peaks(media), vec![0.25, 1.0, 0.5]);
    // a sound no longer played leaves the folder on the next save
    assert!(opened.delete_timeline_clip(clip, false));
    opened.save(art.to_string_lossy().into_owned()).unwrap();
    assert!(!art.join("media").join(&name).exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn an_artwork_from_before_media_still_opens() {
    let dir = folder("old");
    let art = dir.join("Old");
    let s = CanvasSession::new(32, 32);
    assert!(s.start_timeline());
    s.save(art.to_string_lossy().into_owned()).unwrap();
    // as written before media: no media list on the timeline, no media folder
    let text = fs::read_to_string(art.join("artwork.json")).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&text).unwrap();
    json["document"]["timeline"].as_object_mut().unwrap().remove("media");
    fs::write(art.join("artwork.json"), json.to_string()).unwrap();
    assert!(!art.join("media").exists());
    let opened = CanvasSession::open(art.to_string_lossy().into_owned()).unwrap();
    assert!(opened.timeline_info().is_some());
    assert!(opened.timeline_sounds(0).is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn undo_brings_back_a_sound_whose_file_a_save_moved_out() {
    let Some(s) = shown(32) else { return };
    let dir = folder("undo");
    let art = dir.join("Artwork");
    let path = sound_file(&dir, "beat.aif", 11);
    let clip = import_sound(&s, &path, 1.0);
    s.save(art.to_string_lossy().into_owned()).unwrap();
    assert!(s.delete_timeline_clip(clip, false));
    s.save(art.to_string_lossy().into_owned()).unwrap();
    assert!(s.timeline_sounds(0).is_empty());
    s.undo();
    let sounds = s.timeline_sounds(0);
    assert_eq!(sounds.len(), 1, "its file is still to be found");
    s.save(art.to_string_lossy().into_owned()).unwrap();
    let media = s.timeline_media_clip(clip).unwrap().media;
    assert!(art.join("media").join(format!("{media:016x}.aif")).exists());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn a_video_is_fitted_under_the_drawings_and_a_timing_track_follows_its_holds() {
    let dir = folder("video");
    let s = CanvasSession::new(200, 100);
    assert!(s.start_timeline());
    let path = sound_file(&dir, "animatic.mov", 7);
    let clip = s
        .import_timeline_media(MediaImport {
            path,
            name: "animatic.mov".into(),
            kind: TimelineMediaKind::Video,
            seconds: 1.0,
            width: 100,
            height: 100,
            peaks: Vec::new(),
        })
        .unwrap();
    let c = s.timeline_media_clip(clip).unwrap();
    assert_eq!(c.rect, vec![50.0, 0.0, 100.0, 100.0]);
    assert!(s.set_timeline_video_rect(clip, vec![0.0, 0.0, 20.0, 10.0], true));
    assert_eq!(s.timeline_media_clip(clip).unwrap().rect, vec![0.0, 0.0, 20.0, 10.0]);
    assert!(!s.set_timeline_video_rect(clip, vec![1.0], true));
    let fps = s.timeline_info().unwrap().fps;
    let times = s.timeline_video_times(clip);
    assert_eq!(times.len(), fps as usize);
    // the video's track is under the canvas's own
    let tracks = s.timeline_tracks();
    let video_at = tracks.iter().position(|t| t.clips.iter().any(|c| c.id == clip)).unwrap();
    assert_eq!(video_at, 0);
    let layers = s.layers().len();
    let holds = vec![AnimaticHold { start: 0, length: 5 }, AnimaticHold { start: 5, length: fps - 5 }];
    let track = s.make_timing_track(clip, holds);
    assert_ne!(track, 0);
    assert_eq!(s.layers().len(), layers + 2);
    let tracks = s.timeline_tracks();
    let at = tracks.iter().position(|t| t.id == track).unwrap();
    assert_eq!(at, video_at + 1, "just above the video");
    let spans: Vec<(u32, u32)> = tracks[at].clips.iter().map(|c| (c.start, c.duration)).collect();
    assert_eq!(spans, vec![(0, 5), (5, fps - 5)]);
    // not for a sound, or with no holds
    assert_eq!(s.make_timing_track(clip, Vec::new()), 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn the_detector_finds_holds_in_grey_pictures() {
    let dark = Some(vec![0_u8; 4]);
    let light = Some(vec![255_u8; 4]);
    let scores = animatic_scores(vec![dark.clone(), dark.clone(), light.clone(), None, light, dark], 2, 2);
    assert_eq!(scores, vec![1.0, 0.0, 1.0, 0.0, 0.0, 1.0]);
    let holds = animatic_holds(scores.clone(), 0.5, 1);
    assert_eq!(
        holds,
        vec![
            AnimaticHold { start: 0, length: 2 },
            AnimaticHold { start: 2, length: 3 },
            AnimaticHold { start: 5, length: 1 }
        ]
    );
    // the last hold is too short at a minimum of two
    assert_eq!(animatic_holds(scores, 0.5, 2).len(), 2);
    assert!(animatic_threshold(0.0) > animatic_threshold(1.0));
}

use super::*;
use crate::document::LayerId;
use crate::timeline::{Cycle, CycleKeys, Shows};

fn sound(id: u64, seconds: f64) -> Media {
    Media {
        id: MediaId(id),
        name: format!("Sound {id}"),
        kind: MediaKind::Audio,
        seconds,
        extension: "m4a".into(),
        size: [0, 0],
    }
}

fn video(id: u64, seconds: f64, size: [u32; 2]) -> Media {
    Media { id: MediaId(id), name: "Animatic".into(), kind: MediaKind::Video, seconds, extension: "mov".into(), size }
}

fn audio_of(tl: &Timeline, id: ClipId) -> AudioClip {
    match &tl.clip(id).unwrap().content {
        ClipContent::Audio(a) => a.clone(),
        other => panic!("not a sound: {other:?}"),
    }
}

#[test]
fn a_file_hashes_the_same_however_it_is_read() {
    let bytes: Vec<u8> = (0..1000_u32).map(|i| (i * 7 % 251) as u8).collect();
    let mut whole = MediaHasher::default();
    whole.update(&bytes);
    let mut bits = MediaHasher::default();
    for piece in bytes.chunks(13) {
        bits.update(piece);
    }
    assert_eq!(whole.finish(), bits.finish());
    assert_eq!(media_id_of(&bytes[..]).unwrap(), whole.finish());
    let mut other = bytes.clone();
    other[500] ^= 1;
    assert_ne!(media_id_of(&other[..]).unwrap(), whole.finish());
}

#[test]
fn media_files_are_named_by_id_and_extension() {
    assert_eq!(media_extension("Take 3.WAV"), "wav");
    assert_eq!(media_extension("noext"), "bin");
    assert_eq!(media_extension("a.b/../c"), "c");
    let m = sound(0xab, 1.0);
    assert_eq!(m.file_name(), "00000000000000ab.m4a");
    assert_eq!(m.peaks_name(), "00000000000000ab.peaks");
}

#[test]
fn a_sound_goes_on_a_track_of_its_own_below_the_drawings() {
    let mut tl = Timeline::new(24, 10);
    tl.add_track("Drawings");
    tl.add_media(sound(1, 2.01));
    // not carried: nothing
    assert!(tl.add_sound(MediaId(9), 0).is_none());
    let (track, clip) = tl.add_sound(MediaId(1), 5).unwrap();
    assert_eq!(tl.tracks[0].id, track);
    let c = tl.clip(clip).unwrap();
    // 2.01 s at 24 fps is 48.24 frames: 49
    assert_eq!((c.start, c.duration), (5, 49));
    assert_eq!(tl.duration, 54);
    // sounds draw nothing
    assert!(tl.plan(10).items.is_empty());
    // a video goes above the sounds, still under the drawings
    tl.add_media(video(2, 1.0, [1920, 1080]));
    let (vtrack, vclip) = tl.add_video(MediaId(2), 0, (1000, 1000)).unwrap();
    assert_eq!(tl.tracks[1].id, vtrack);
    assert_eq!(tl.tracks[2].name, "Drawings");
    let plan = tl.plan(12);
    assert_eq!(plan.items.len(), 1);
    match &plan.items[0].shows {
        Shows::Video { media, seconds, rect } => {
            assert_eq!(*media, MediaId(2));
            assert!((seconds - 0.5).abs() < 1e-9);
            let want = [0.0, 218.75, 1000.0, 562.5];
            assert!(rect.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-3), "{rect:?}");
        }
        other => panic!("not a video: {other:?}"),
    }
    assert_eq!(tl.videos_at(12), vec![(MediaId(2), 0.5)]);
    assert_eq!(tl.video_times(vclip).len(), 24);
    // one frame exactly at a whole number of frames
    assert_eq!(tl.frames_of(1.0), 24);
    assert_eq!(tl.frames_of(0.0), 1);
}

#[test]
fn trimming_the_start_moves_into_the_media_and_keeps_the_end() {
    let mut tl = Timeline::new(10, 1);
    tl.add_media(sound(1, 3.0));
    let (_, clip) = tl.add_sound(MediaId(1), 10).unwrap();
    assert!(tl.trim_media_start(clip, 14));
    let c = tl.clip(clip).unwrap();
    assert_eq!((c.start, c.end()), (14, 40));
    assert!((audio_of(&tl, clip).trim_start - 0.4).abs() < 1e-9);
    // never earlier than the sound's start
    assert!(tl.trim_media_start(clip, 0));
    let c = tl.clip(clip).unwrap();
    assert_eq!((c.start, c.end()), (10, 40));
    assert!(audio_of(&tl, clip).trim_start.abs() < 1e-9);
    // nor past its end
    assert!(tl.trim_media_start(clip, 99));
    assert_eq!(tl.clip(clip).unwrap().duration, 1);
}

#[test]
fn a_clip_is_never_longer_than_what_is_left_of_its_media() {
    let mut tl = Timeline::new(10, 1);
    tl.add_media(sound(1, 3.0));
    let (_, clip) = tl.add_sound(MediaId(1), 0).unwrap();
    assert!(tl.set_duration(clip, 100, false));
    assert_eq!(tl.clip(clip).unwrap().duration, 30);
    assert!(tl.set_duration(clip, 12, false));
    assert_eq!(tl.clip(clip).unwrap().duration, 12);
    // slipped two seconds in, only a second is left
    assert!(tl.slip_media(clip, 2.0));
    assert_eq!(tl.clip(clip).unwrap().duration, 10);
    assert!(tl.slip_media(clip, 50.0));
    assert!((audio_of(&tl, clip).trim_start - 2.9).abs() < 1e-9);
    // only media clips slip
    let drawing = tl.add_track("Drawings");
    let d = tl.add_drawing(drawing, 0, LayerId(1), "Drawing").unwrap();
    assert!(!tl.slip_media(d, 1.0));
    assert!(!tl.trim_media_start(d, 1));
}

#[test]
fn sounds_play_at_their_time_offset_trim_and_volume() {
    let mut tl = Timeline::new(10, 1);
    tl.add_media(sound(1, 5.0));
    let (track, clip) = tl.add_sound(MediaId(1), 20).unwrap();
    tl.slip_media(clip, 1.0);
    assert!(tl.set_sound(clip, 0.5, false, 0.04));
    let s = tl.sounds();
    assert_eq!(s.len(), 1);
    let s = &s[0];
    assert!((s.at - 2.04).abs() < 1e-9 && (s.from - 1.0).abs() < 1e-9);
    // 40 frames left of it, less the offset
    assert!((s.length - 3.96).abs() < 1e-9, "{}", s.length);
    assert_eq!(s.gain_at(25), 0.5);
    // offsets stay under a frame, volume within range
    tl.set_sound(clip, 7.0, true, 3.0);
    let a = audio_of(&tl, clip);
    assert!(a.offset < 0.1 && a.volume == 1.0 && a.muted);
    assert_eq!(tl.sounds()[0].gain_at(25), 0.0);
    // a hidden track's sounds are silent; a disabled clip's don't play
    tl.set_sound(clip, 1.0, false, 0.0);
    tl.track_mut(track).unwrap().hidden = true;
    assert!(tl.sounds()[0].muted);
    tl.clip_mut(clip).unwrap().disabled = true;
    assert!(tl.sounds().is_empty());
}

#[test]
fn keyed_volume_counts_from_the_clips_first_frame() {
    let mut tl = Timeline::new(10, 1);
    tl.add_media(sound(1, 5.0));
    let (_, clip) = tl.add_sound(MediaId(1), 10).unwrap();
    if let ClipContent::Audio(a) = &mut tl.clip_mut(clip).unwrap().content {
        a.volume_keys.set(0, 0.0, crate::timeline::Easing::Linear);
        a.volume_keys.set(10, 1.0, crate::timeline::Easing::Linear);
    }
    let s = &tl.sounds()[0];
    assert_eq!(s.gain_at(10), 0.0);
    assert!((s.gain_at(15) - 0.5).abs() < 1e-6);
    assert_eq!(s.gain_at(30), 1.0);
}

#[test]
fn a_cycling_group_plays_its_sounds_each_cycle() {
    let mut tl = Timeline::new(10, 1);
    tl.add_media(sound(1, 5.0));
    let (_, clip) = tl.add_sound(MediaId(1), 0).unwrap();
    tl.set_duration(clip, 8, false);
    let group = tl.group(&[clip], "Loop").unwrap();
    {
        let g = tl.clip_mut(group).unwrap();
        g.start = 10;
        g.duration = 25;
        if let ClipContent::Group(gr) = &mut g.content {
            gr.cycle = Some(Cycle { length: 10, keys: CycleKeys::Repeat });
        }
    }
    let s = tl.sounds();
    let times: Vec<(f64, f64)> = s.iter().map(|s| (s.at, s.length)).collect();
    // three cycles of 10 frames; the last cut off five frames in
    assert_eq!(times.len(), 3);
    for ((at, length), (want_at, want_len)) in times.iter().zip([(1.0, 0.8), (2.0, 0.8), (3.0, 0.5)]) {
        assert!((at - want_at).abs() < 1e-9 && (length - want_len).abs() < 1e-9, "{times:?}");
    }
}

#[test]
fn media_round_trips_and_older_timelines_still_read() {
    let mut tl = Timeline::new(24, 10);
    tl.add_media(sound(1, 2.0));
    tl.add_media(video(2, 3.0, [640, 480]));
    let (_, s) = tl.add_sound(MediaId(1), 0).unwrap();
    tl.add_video(MediaId(2), 3, (800, 600)).unwrap();
    tl.set_sound(s, 0.25, true, 0.01);
    let json = serde_json::to_string(&tl).unwrap();
    let back: Timeline = serde_json::from_str(&json).unwrap();
    assert_eq!(back, tl);
    // a timeline written before media (no media list; media without extension or size)
    let mut old: serde_json::Value = serde_json::from_str(&json).unwrap();
    for m in old["media"].as_array_mut().unwrap() {
        m.as_object_mut().unwrap().remove("extension");
        m.as_object_mut().unwrap().remove("size");
    }
    let read: Timeline = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(read.media[1].size, [0, 0]);
    assert_eq!(read.media[0].file_name(), "0000000000000001.bin");
    old.as_object_mut().unwrap().remove("media");
    let read: Timeline = serde_json::from_value(old).unwrap();
    assert!(read.media.is_empty());
}

#[test]
fn repairing_keeps_media_values_in_range_and_unused_media_can_go() {
    let mut tl = Timeline::new(24, 10);
    tl.add_media(sound(1, 2.0));
    tl.add_media(sound(1, 9.0));
    assert_eq!(tl.media.len(), 1);
    tl.add_media(sound(3, 2.0));
    let (_, s) = tl.add_sound(MediaId(1), 0).unwrap();
    if let ClipContent::Audio(a) = &mut tl.clip_mut(s).unwrap().content {
        a.volume = f32::NAN;
        a.trim_start = -4.0;
        a.offset = f64::INFINITY;
    }
    tl.add_media(video(2, 1.0, [10, 10]));
    let (_, v) = tl.add_video(MediaId(2), 0, (10, 10)).unwrap();
    if let ClipContent::Video(v) = &mut tl.clip_mut(v).unwrap().content {
        v.rect = [0.0, 0.0, -1.0, f32::NAN];
    }
    let tl = tl.repaired(&[]);
    let a = audio_of(&tl, s);
    assert_eq!((a.volume, a.trim_start, a.offset), (1.0, 0.0, 0.0));
    assert_eq!(tl.used_media().len(), 2);
    let mut tl = tl;
    tl.prune_media();
    assert_eq!(tl.media.iter().map(|m| m.id.0).collect::<Vec<_>>(), vec![1, 2]);
}

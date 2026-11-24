//! Group clips and media clips edited further (`timeline_edits`).

use std::collections::HashMap;

use super::*;
use crate::media::Sound;
use crate::timeline::GroupShown;

fn drawing(id: u64, start: u32, duration: u32, root: u64) -> Clip {
    Clip { id: ClipId(id), start, ..Clip::drawing("d", LayerId(root), start, duration) }
}

fn track(id: u64, clips: Vec<Clip>) -> Track {
    Track { id: TrackId(id), name: format!("T{id}"), hidden: false, locked: false, opacity: 1.0, look: Default::default(), clips }
}

fn group(id: u64, start: u32, duration: u32, tracks: Vec<Track>, cycle: Option<Cycle>) -> Clip {
    Clip { content: ClipContent::Group(Group { tracks, collapsed: false, cycle }), ..drawing(id, start, duration, 0) }
}

fn timeline(tracks: Vec<Track>) -> Timeline {
    let mut tl = Timeline::new(12, 1);
    tl.tracks = tracks;
    tl.next_track = 50;
    tl.next_clip = 500;
    tl.fit();
    tl
}

fn key<T>(frame: i32, value: T) -> Key<T> {
    Key { frame, value, easing: Easing::Linear, path: None }
}

/// Each drawing a frame shows, with where its pose (through its groups') puts two points and its
/// opacity through them.
fn shown(tl: &Timeline, frame: u32) -> Vec<(LayerId, [f32; 4], f32)> {
    fn walk(items: &[PlanItem], outer: Option<Affine>, fade: f32, out: &mut Vec<(LayerId, [f32; 4], f32)>) {
        for item in items.iter().filter(|i| !i.matte_only) {
            let pose = match (item.pose, outer) {
                (Some(p), Some(o)) => Some(p.then(o)),
                (p, o) => p.or(o),
            };
            match &item.shows {
                Shows::Drawing(id) => {
                    let p = pose.unwrap_or(Affine::IDENTITY);
                    let (a, b) = (p.apply(Vec2::new(0.0, 0.0)), p.apply(Vec2::new(10.0, 5.0)));
                    out.push((*id, [a.x, a.y, b.x, b.y], item.opacity * fade));
                }
                Shows::Group(inner) => walk(inner, pose, item.opacity * fade, out),
                Shows::Video { .. } => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&tl.plan(frame).items, None, 1.0, &mut out);
    out
}

fn near(a: &[(LayerId, [f32; 4], f32)], b: &[(LayerId, [f32; 4], f32)]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|(x, y)| {
            x.0 == y.0 && x.1.iter().zip(&y.1).all(|(p, q)| (p - q).abs() < 1e-3) && (x.2 - y.2).abs() < 1e-4
        })
}

/// A group from frame 2 for 6 frames, moved 0 to 12 across over its frames 0 to 4, holding two
/// drawings one after the other.
fn moving_group() -> Timeline {
    let inner = track(2, vec![drawing(1, 0, 3, 10), drawing(2, 3, 3, 11)]);
    let mut g = group(3, 2, 6, vec![inner], None);
    g.keys.position.keys = vec![key(0, [0.0, 0.0]), key(4, [12.0, 0.0])];
    g.keys.pivot = Some([5.0, 5.0]);
    timeline(vec![track(1, vec![g])])
}

#[test]
fn a_clip_in_a_posed_group_is_where_the_group_puts_it() {
    let mut tl = moving_group();
    // drawing 2 starts on the group's frame 3, where the group has moved 9 across
    let pose = tl.groups_pose(ClipId(2), 0.0).unwrap();
    assert!((pose.apply(Vec2::new(1.0, 1.0)).x - 10.0).abs() < 1e-4);
    assert!(tl.groups_pose(ClipId(3), 0.0).is_none(), "a clip in no group");
    // nested: an outer group turned half round about the origin
    let inner_group = tl.tracks[0].clips.remove(0);
    let mut outer = group(4, 0, 10, vec![track(5, vec![inner_group])], None);
    outer.keys.rotation.keys = vec![key(0, std::f32::consts::PI)];
    outer.keys.pivot = Some([0.0, 0.0]);
    tl.tracks[0].clips.push(outer);
    let p = tl.groups_pose(ClipId(2), 0.0).unwrap().apply(Vec2::new(1.0, 1.0));
    assert!((p.x + 10.0).abs() < 1e-3 && (p.y + 1.0).abs() < 1e-3, "{p:?}");
}

#[test]
fn clips_move_into_groups_and_out_of_them() {
    let mut tl = moving_group();
    tl.tracks.push(track(6, vec![drawing(7, 9, 2, 12)]));
    let inner = TrackId(2);
    // dragged onto a track inside the group, on its own frames: the group grows to cover it
    assert!(tl.move_clip(ClipId(7), inner, 6));
    assert_eq!(tl.group_of(ClipId(7)), Some(ClipId(3)));
    assert_eq!(tl.origin(ClipId(7)), Some(8));
    assert_eq!(tl.clip(ClipId(3)).map(|g| g.duration), Some(8));
    // a group never goes inside itself
    assert!(!tl.move_clip(ClipId(3), inner, 0));
    // out again, onto a track made above the group's, where it showed, carrying the group's
    // motion: it shows just as it did
    let before = shown(&tl, 9);
    let onto = tl.move_out_of_group(ClipId(7)).unwrap();
    assert_eq!(tl.group_of(ClipId(7)), None);
    assert_eq!(tl.tracks.iter().position(|t| t.id == onto), Some(1));
    assert_eq!(tl.clip(ClipId(7)).map(|c| c.start), Some(8));
    assert!(near(&shown(&tl, 9), &before), "{:?} / {before:?}", shown(&tl, 9));
    // and in again, onto the group's top track with room at its frame
    let back = tl.move_into_group(ClipId(7), ClipId(3)).unwrap();
    assert_eq!(back, inner);
    assert_eq!(tl.origin(ClipId(7)), Some(8));
    assert!(tl.tracks.iter().all(Track::is_ordered));
    assert_eq!(tl.move_into_group(ClipId(3), ClipId(3)), None);
    assert_eq!(tl.move_out_of_group(ClipId(3)), None, "in no group");
}

#[test]
fn ungrouping_folds_the_groups_keys_into_its_clips() {
    let mut tl = moving_group();
    // the group fades too, and turns; one drawing has keys of its own
    if let Some(g) = tl.clip_mut(ClipId(3)) {
        g.keys.opacity.keys = vec![key(0, 1.0), key(5, 0.5)];
        g.keys.rotation.keys = vec![key(1, 0.0), key(5, 1.0)];
        g.opacity = 0.8;
    }
    if let Some(c) = tl.clip_mut(ClipId(2)) {
        c.keys.position.keys = vec![key(0, [0.0, 3.0]), key(2, [4.0, 3.0])];
        c.keys.pivot = Some([2.0, 2.0]);
        c.keys.opacity.keys = vec![key(1, 0.5)];
    }
    let before: Vec<_> = (0..10).map(|f| shown(&tl, f)).collect();
    assert!(tl.ungroup(ClipId(3)));
    assert_eq!(tl.group_depth(), 0);
    // the drawing with no keys of its own takes the group's as they were: every frame the same
    for f in 2..5 {
        assert!(near(&shown(&tl, f), &before[f as usize]), "frame {f}: {:?} / {:?}", shown(&tl, f), before[f as usize]);
    }
    // the one with keys of its own: the same on each frame either had a key on
    let keyed = tl.clip(ClipId(2)).unwrap();
    let frames: Vec<i32> = keyed.keys.position.keys.iter().map(|k| k.frame).collect();
    assert_eq!(frames, [-3, -2, 0, 1, 2]);
    for f in [5, 6, 7] {
        assert!(near(&shown(&tl, f), &before[f as usize]), "frame {f}: {:?} / {:?}", shown(&tl, f), before[f as usize]);
    }
}

#[test]
fn ungrouping_lays_out_a_cycle_with_copies_of_its_drawings() {
    // a 2-frame cycle played over 7 frames: frames 0 1 0 1 0 1 0; a drawing past the cycle never
    // showed
    let inner = track(2, vec![drawing(1, 0, 1, 10), drawing(2, 1, 3, 11), drawing(4, 4, 1, 13)]);
    let mut g = group(3, 1, 7, vec![inner], Some(Cycle { length: 2, keys: CycleKeys::ContinueAcross }));
    g.keys.position.keys = vec![key(0, [0.0, 0.0]), key(6, [12.0, 0.0])];
    let mut tl = timeline(vec![track(1, vec![g])]);
    let before: Vec<_> = (0..9).map(|f| shown(&tl, f)).collect();
    let mut made = 100;
    let mut copies: HashMap<LayerId, LayerId> = HashMap::new();
    let mut copy = |root: LayerId| {
        made += 1;
        copies.insert(LayerId(made), root);
        Some(LayerId(made))
    };
    assert!(tl.ungroup_copying(ClipId(3), &mut copy));
    let t = &tl.tracks[0];
    assert_eq!(
        t.clips.iter().map(|c| (c.start, c.duration)).collect::<Vec<_>>(),
        [(1, 1), (2, 1), (3, 1), (4, 1), (5, 1), (6, 1), (7, 1)]
    );
    // each repeat a drawing of its own, copied from the one it repeats; the motion runs on
    let original = |id: LayerId| copies.get(&id).copied().unwrap_or(id);
    for f in 0..9 {
        let now: Vec<_> = shown(&tl, f).into_iter().map(|(id, p, o)| (original(id), p, o)).collect();
        assert!(near(&now, &before[f as usize]), "frame {f}: {now:?} / {:?}", before[f as usize]);
    }
    let unshown = tl.tracks.iter().find(|t| t.hidden).expect("a hidden track");
    assert_eq!(unshown.clips.iter().map(|c| c.id).collect::<Vec<_>>(), [ClipId(4)]);
    // without copies, a drawing's repeats are left out
    let inner = track(2, vec![drawing(1, 0, 1, 10)]);
    let g = group(3, 0, 4, vec![inner], Some(Cycle { length: 2, keys: CycleKeys::Repeat }));
    let mut tl = timeline(vec![track(1, vec![g])]);
    assert!(tl.ungroup(ClipId(3)));
    assert_eq!(tl.tracks[0].clips.len(), 1);
}

fn sound_clip(id: u64, start: u32, duration: u32) -> Clip {
    Clip {
        content: ClipContent::Audio(AudioClip {
            media: MediaId(1),
            trim_start: 1.0,
            offset: 0.02,
            volume: 1.0,
            muted: false,
            volume_keys: Channel { keys: vec![key(0, 0.0), key(4, 1.0)] },
        }),
        ..drawing(id, start, duration, 0)
    }
}

#[test]
fn sounds_and_videos_split_and_duplicate() {
    let video = Clip {
        content: ClipContent::Video(VideoClip { media: MediaId(2), trim_start: 0.5, rect: [0.0, 0.0, 4.0, 4.0], sound: None }),
        ..drawing(2, 0, 6, 0)
    };
    let mut tl = timeline(vec![track(1, vec![sound_clip(1, 2, 8)]), track(2, vec![video, drawing(3, 6, 2, 9)])]);
    tl.add_media(Media {
        id: MediaId(1),
        name: "s".into(),
        kind: MediaKind::Audio,
        seconds: 10.0,
        extension: "m4a".into(),
        size: [0, 0],
    });
    let sound_before = tl.sounds();
    let rest = tl.split_media(ClipId(1), 5).unwrap();
    let (first, second) = (tl.clip(ClipId(1)).unwrap(), tl.clip(rest).unwrap());
    assert_eq!((first.start, first.duration, second.start, second.duration), (2, 3, 5, 5));
    let ClipContent::Audio(a) = &second.content else { panic!() };
    // on from where it was: 3 frames at 12 a second into it, less the offset
    assert!((a.trim_start - (1.0 + 0.25 - 0.02)).abs() < 1e-9 && a.offset == 0.0);
    assert_eq!(a.volume_keys.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [-3, 1]);
    // the two pieces play the same sound at the same levels
    let after = tl.sounds();
    assert_eq!(after.len(), 2);
    let whole = sound_before.first().unwrap();
    assert!((after[1].at + after[1].length - whole.end()).abs() < 1e-9);
    assert!((after[1].from - (whole.from + after[1].at - whole.at)).abs() < 1e-9);
    for f in 2..10 {
        let piece = if f < 5 { &after[0] } else { &after[1] };
        assert!((piece.gain_at(f) - whole.gain_at(f)).abs() < 1e-6, "frame {f}");
    }
    assert_eq!(tl.split_media(ClipId(1), 2), None, "not inside it");
    assert_eq!(tl.split_media(ClipId(3), 7), None, "a drawing");
    // a video's rest starts later in it
    let v = tl.split_media(ClipId(2), 2).unwrap();
    let ClipContent::Video(vc) = &tl.clip(v).unwrap().content else { panic!() };
    assert!((vc.trim_start - (0.5 + 2.0 / 12.0)).abs() < 1e-9);
    // duplicated: just after it, the clips after moving along
    let copy = tl.duplicate_media(ClipId(2)).unwrap();
    assert_eq!(tl.clip(copy).map(|c| (c.start, c.duration)), Some((2, 2)));
    assert_eq!(tl.tracks[1].clips.iter().map(|c| c.start).collect::<Vec<_>>(), [0, 2, 4, 8]);
    assert!(tl.tracks[1].is_ordered());
    assert_eq!(tl.duplicate_media(ClipId(3)), None);
}

#[test]
fn a_keyed_sound_has_a_level_on_each_frame_it_plays_through() {
    let s = Sound {
        clip: ClipId(1),
        media: MediaId(1),
        at: 0.5,
        from: 0.0,
        length: 0.5,
        key_origin: 6,
        volume: 1.0,
        muted: false,
        volume_keys: Channel { keys: vec![key(0, 0.0), key(4, 1.0)] },
    };
    let (first, levels) = s.levels(12).unwrap();
    assert_eq!(first, 6);
    assert_eq!(levels.len(), 7, "frames 6 to 12");
    assert_eq!(levels, [0.0, 0.25, 0.5, 0.75, 1.0, 1.0, 1.0]);
    assert!(Sound { volume_keys: Channel::default(), ..s.clone() }.levels(12).is_none());
    assert!(Sound { muted: true, ..s }.levels(12).is_none());
}

#[test]
fn a_layer_added_to_a_drawing_goes_inside_it() {
    let mut doc = Document::new(8, 8);
    let first = doc.current();
    let second = doc.add_layer();
    assert!(doc.start_timeline());
    // a drawing of one layer: put in a group, which its clip shows from then on
    let made = doc.add_layer_to_drawing(first).unwrap();
    let group = doc.group_of(made).unwrap();
    assert_eq!(doc.group_of(first), Some(group));
    assert_eq!(doc.current(), made);
    let tl = doc.timeline.as_ref().unwrap();
    assert!(tl.drawing_of(group).is_some() && tl.drawing_of(first).is_none());
    assert_eq!(doc.members(Some(group)), [first, made]);
    assert_eq!(doc.members(None).len(), 2, "nothing more at the top level");
    // again: above the current layer, inside it
    doc.set_current(first);
    let another = doc.add_layer_to_drawing(group).unwrap();
    assert_eq!(doc.members(Some(group)), [first, another, made]);
    // the group itself current: on top of what is in it
    doc.set_current(group);
    let top = doc.add_layer_to_drawing(group).unwrap();
    assert_eq!(doc.members(Some(group)).last(), Some(&top));
    assert!(doc.add_layer_to_drawing(LayerId(999)).is_none());
    let _ = second;
}

#[test]
fn clips_fill_out_the_empty_frames_after_them() {
    // a track with a gap between its clips and room after the last; another that sets the length
    let mut tl = timeline(vec![
        track(1, vec![drawing(10, 0, 2, 1), drawing(11, 5, 1, 2)]),
        track(2, vec![drawing(20, 0, 12, 3)]),
    ]);
    assert_eq!(tl.duration, 12);
    // the first up to the next clip, the last to the timeline's end: each on its own, as one
    assert!(tl.fill_out(&[ClipId(10), ClipId(11)]));
    let spans = |tl: &Timeline, t: u64| -> Vec<(u32, u32)> {
        tl.track(TrackId(t)).unwrap().clips.iter().map(|c| (c.start, c.duration)).collect()
    };
    assert_eq!(spans(&tl, 1), [(0, 5), (5, 7)]);
    assert_eq!(tl.duration, 12, "no longer than it was");
    // nothing empty after them now, nor after the clip the timeline ends with
    assert!(!tl.fill_out(&[ClipId(10), ClipId(11), ClipId(20), ClipId(99)]));
    // in a group: to the next clip there, and to the group's end, not the timeline's
    let inner = vec![track(3, vec![drawing(30, 0, 1, 4), drawing(31, 3, 1, 5)])];
    let mut tl =
        timeline(vec![track(1, vec![group(40, 2, 6, inner.clone(), None)]), track(2, vec![drawing(20, 0, 12, 3)])]);
    assert!(tl.fill_out(&[ClipId(30), ClipId(31)]));
    assert_eq!(spans(&tl, 3), [(0, 3), (3, 3)]);
    assert_eq!(tl.clip(ClipId(40)).map(|g| (g.start, g.duration)), Some((2, 6)));
    // a group that cycles plays only its first frames: no further than one cycle
    let cycle = Cycle { length: 4, keys: CycleKeys::Repeat };
    let mut tl =
        timeline(vec![track(1, vec![group(40, 2, 9, inner, Some(cycle))]), track(2, vec![drawing(20, 0, 12, 3)])]);
    assert!(!tl.fill_out(&[ClipId(31)]), "it ends where the cycle does");
    assert_eq!(spans(&tl, 3), [(0, 1), (3, 1)]);
    // a sound stops where its file does
    let mut tl = timeline(vec![track(1, vec![sound_clip(60, 0, 3)]), track(2, vec![drawing(20, 0, 12, 3)])]);
    let before = tl.clip(ClipId(60)).unwrap().duration;
    tl.fill_out(&[ClipId(60)]);
    let after = tl.clip(ClipId(60)).unwrap().duration;
    assert!(after >= before && after <= 12, "{before} {after}");
}

#[test]
fn a_groups_frames_are_told_as_runs_showing_the_same_drawings() {
    // two tracks: a drawing held 3 then another for 2 under one from frame 2 on; cycling 4
    let inner = vec![
        track(10, vec![drawing(1, 0, 3, 101), drawing(2, 3, 2, 102)]),
        track(11, vec![drawing(3, 2, 2, 103)]),
    ];
    let tl = timeline(vec![track(1, vec![group(9, 5, 6, inner.clone(), None)])]);
    let roots = |r: &[u64]| r.iter().map(|x| GroupShown::Drawing(LayerId(*x))).collect::<Vec<_>>();
    assert_eq!(
        tl.group_shots(ClipId(9)),
        [(0, 2, roots(&[101])), (2, 1, roots(&[101, 103])), (3, 1, roots(&[102, 103])), (4, 1, roots(&[102])), (5, 1, roots(&[]))]
    );
    // its cycle played round; a hidden track's drawings left out
    let mut cycling = inner;
    cycling[1].hidden = true;
    let tl = timeline(vec![track(1, vec![group(9, 0, 7, cycling, Some(Cycle { length: 4, keys: CycleKeys::Repeat }))])]);
    assert_eq!(
        tl.group_shots(ClipId(9)),
        [(0, 3, roots(&[101])), (3, 1, roots(&[102])), (4, 3, roots(&[101]))]
    );
    assert!(tl.group_shots(ClipId(1)).is_empty(), "not a group");
}

#[test]
fn a_group_is_cut_in_two_with_the_clips_the_cut_goes_through() {
    // inside: a drawing over frames 0–3, one over 4–5, and on a second track one over 2–4
    let inner = vec![
        track(10, vec![drawing(1, 0, 4, 101), drawing(2, 4, 2, 102)]),
        track(11, vec![drawing(3, 2, 3, 103)]),
    ];
    let mut tl = timeline(vec![track(1, vec![group(9, 10, 6, inner, None), drawing(4, 16, 1, 104)])]);
    // cut on its third frame: through drawings 1 and 3
    let through = tl.group_cut_drawings(ClipId(9), 13);
    assert_eq!(through, [(ClipId(1), LayerId(101)), (ClipId(3), LayerId(103))]);
    assert!(tl.split_group(ClipId(9), 13, &HashMap::new()).is_none(), "a copy of each drawing cut is wanted");
    let before = tl.clone();
    let copies = HashMap::from([(ClipId(1), LayerId(201)), (ClipId(3), LayerId(203))]);
    let made = tl.split_group(ClipId(9), 13, &copies).unwrap();

    let spans = |tl: &Timeline, id: ClipId| {
        let ClipContent::Group(g) = &tl.clip(id).unwrap().content else { panic!() };
        g.tracks
            .iter()
            .map(|t| {
                t.clips
                    .iter()
                    .map(|c| match &c.content {
                        ClipContent::Drawing(d) => (c.start, c.duration, d.root.0),
                        _ => (c.start, c.duration, 0),
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    };
    let (first, rest) = (tl.clip(ClipId(9)).unwrap(), tl.clip(made).unwrap());
    assert_eq!((first.start, first.duration, rest.start, rest.duration), (10, 3, 13, 3));
    assert_eq!(spans(&tl, ClipId(9)), [vec![(0, 3, 101)], vec![(2, 1, 103)]]);
    assert_eq!(spans(&tl, made), [vec![(0, 1, 201), (1, 2, 102)], vec![(0, 2, 203)]]);
    // the rest's tracks are its own, and every clip has an id of its own
    let ids = |tl: &Timeline| {
        let mut all = Vec::new();
        fn walk(tracks: &[Track], all: &mut Vec<u64>) {
            for c in tracks.iter().flat_map(|t| &t.clips) {
                all.push(c.id.0);
                if let ClipContent::Group(g) = &c.content {
                    walk(&g.tracks, all);
                }
            }
        }
        walk(&tl.tracks, &mut all);
        all
    };
    let mut all = ids(&tl);
    all.sort_unstable();
    all.dedup();
    assert_eq!(all.len(), ids(&tl).len());
    assert_eq!(tl.every_track().len(), before.every_track().len() + 2);
    // each frame shows what it did, the second parts their copies
    let shown = |tl: &Timeline, f: u32| tl.plan(f).roots().iter().map(|r| r.0 % 100).collect::<Vec<_>>();
    for f in 10..17 {
        assert_eq!(shown(&tl, f), shown(&before, f), "frame {f}");
    }
    // not at its first frame, past its end, or a repeating group
    assert!(tl.split_group(made, 13, &HashMap::new()).is_none());
    assert!(tl.split_group(made, 16, &HashMap::new()).is_none());
    assert!(tl.split_group(ClipId(4), 16, &HashMap::new()).is_none());
}

#[test]
fn a_performed_path_is_thinned_to_the_keys_that_keep_it() {
    // along a straight line for six frames, then off to one side and back
    let mut ch: Channel<[f32; 2]> = Channel::default();
    for f in 0..=6 {
        ch.set(f, [f as f32 * 10.0, 0.0], Easing::Smooth);
    }
    ch.set(7, [70.0, 30.0], Easing::Smooth);
    ch.set(8, [80.0, 0.3], Easing::Smooth);
    ch.set(12, [500.0, 500.0], Easing::Hold);
    ch.thin(0, 8, 1.0);
    let frames: Vec<i32> = ch.keys.iter().map(|k| k.frame).collect();
    assert_eq!(frames, [0, 6, 7, 8, 12], "the line's ends, the corner, and the key past the take");
    assert!(ch.keys[..4].iter().all(|k| k.easing == Easing::Linear));
    assert_eq!(ch.keys[4].easing, Easing::Hold, "a key outside the take is as it was");
    // each frame is within the tolerance of where it was
    for f in 0..=6 {
        let at = ch.value_at(f as f32).unwrap();
        assert!((at[0] - f as f32 * 10.0).abs() < 1.0 && at[1].abs() < 1.0, "frame {f}: {at:?}");
    }
    // nothing to thin: one key, or none in the frames
    let mut one: Channel<[f32; 2]> = Channel::default();
    one.set(3, [1.0, 2.0], Easing::Smooth);
    one.thin(0, 10, 1.0);
    assert_eq!(one.keys.len(), 1);
    one.thin(20, 30, 1.0);
    assert_eq!(one.keys.len(), 1);
}

#[test]
fn a_shaky_path_is_evened_out_and_carried_through_its_keys_in_a_curve() {
    let mut ch: Channel<[f32; 2]> = Channel::default();
    // a straight line with every other frame knocked off it
    for f in 0..=8 {
        ch.set(f, [f as f32 * 10.0, if f % 2 == 1 { 6.0 } else { -6.0 }], Easing::Linear);
    }
    let ends = (ch.keys[0].value, ch.keys[8].value);
    ch.even_out(0, 8, 6);
    assert_eq!((ch.keys[0].value, ch.keys[8].value), ends, "its ends stay where they were");
    assert!(ch.keys[2..7].iter().all(|k| k.value[1].abs() < 1.5), "the shake is less: {:?}", ch.keys);
    ch.curve(0, 8, 1.0);
    assert!(ch.keys[0].path.is_none() && ch.keys[8].path.is_none());
    let h = ch.keys[4].path.unwrap();
    assert!((h.after[0] - 20.0 / 6.0).abs() < 0.5 && (h.before[0] + h.after[0]).abs() < 1e-5, "{h:?}");
    ch.curve(0, 8, 0.0);
    assert!(ch.keys.iter().all(|k| k.path.is_none()), "no smoothing: corners");
    // a turn keyed frame by frame thins as a position does
    let mut turn: Channel<f32> = Channel::default();
    for f in 0..=6 {
        turn.set(f, f as f32 * 0.1, Easing::Linear);
    }
    turn.thin(0, 6, 0.01);
    assert_eq!(turn.keys.iter().map(|k| k.frame).collect::<Vec<_>>(), [0, 6]);
}

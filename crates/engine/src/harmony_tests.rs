//! Harmony scenes: blending nodes, sound columns, and tracks drawn through their mattes.

use super::tests::walk;
use super::*;
use crate::document::LayerId;
use crate::timeline::{Clip, ClipContent, ClipId, Drawing, Track};
use crate::xml;

fn stage(scene: &Scene) -> xml::Element {
    let files = files(scene, 1);
    let text = files.first().map(|(_, t)| t.clone()).unwrap_or_default();
    xml::parse(&text).expect("the scene reads as XML")
}

fn modules<'a>(stage: &'a xml::Element, kind: &str) -> Vec<&'a xml::Element> {
    stage.all("module").into_iter().filter(|m| m.attribute("type") == Some(kind)).collect()
}

fn links(stage: &xml::Element) -> Vec<(String, String, Option<String>)> {
    stage
        .all("link")
        .iter()
        .map(|l| {
            (
                l.attribute("out").unwrap_or_default().to_owned(),
                l.attribute("in").unwrap_or_default().to_owned(),
                l.attribute("inport").map(str::to_owned),
            )
        })
        .collect()
}

#[test]
fn a_layer_that_blends_goes_through_a_blending_node() {
    let mut scene = walk();
    scene.layers[1].blend = BlendMode::Multiply;
    let stage = stage(&scene);
    let blends = modules(&stage, "BLEND_MODE_MODULE");
    assert_eq!(blends.len(), 1);
    assert_eq!(blends[0].attribute("name"), Some("Character_Blending"));
    let mode = blends[0].find("blendMode").unwrap();
    assert_eq!(mode.attribute("val"), Some("eMULTIPLY_OVER"));
    assert_eq!(mode.attribute("version"), Some("2"));
    let s = |v: &str| v.to_owned();
    assert_eq!(
        links(&stage),
        [
            (s("Composite"), s("Display"), None),
            (s("Composite"), s("Write"), None),
            (s("Background"), s("Composite"), Some(s("0"))),
            (s("Character"), s("Character_Blending"), None),
            (s("Character_Blending"), s("Composite"), Some(s("1"))),
        ]
    );
}

#[test]
fn a_mode_harmony_hasnt_composites_normally() {
    let mut scene = walk();
    scene.layers[0].blend = BlendMode::HardMix;
    scene.layers[1].blend = BlendMode::Shade;
    let stage = stage(&scene);
    assert!(modules(&stage, "BLEND_MODE_MODULE").is_empty());
    assert_eq!(links(&stage).len(), 4);
    // every mode Harmony has is named; the others aren't
    let named = BlendMode::ALL.iter().filter(|m| blend_keyword(**m).is_some()).count();
    assert_eq!(named, BlendMode::ALL.len() - 5);
    assert_eq!(blend_keyword(BlendMode::Add), Some("eLINEAR_DODGE_OVER"));
    assert_eq!(blend_keyword(BlendMode::Luminosity), Some("eLUNINOSITY_OVER"));
}

#[test]
fn a_blending_node_takes_a_name_no_layer_has() {
    let mut scene = walk();
    scene.layers[0].name = "Character_Blending".into();
    scene.layers[1].blend = BlendMode::Screen;
    let stage = stage(&scene);
    let blends = modules(&stage, "BLEND_MODE_MODULE");
    assert_eq!(blends[0].attribute("name"), Some("Character_Blending_2"));
    // and no layer takes a node's name
    let mut names = Names::scene();
    assert_eq!(names.unique("Composite", "Layer"), "Composite_2");
    assert_eq!(names.unique("write", "Layer"), "write_2");
}

#[test]
fn sounds_are_sound_columns_after_the_drawings() {
    let mut scene = walk();
    scene.sounds = vec![
        Sound { name: "Voice".into(), file: "Voice.wav".into(), start: 6, clip: 0.0..0.5 },
        Sound { name: "Music".into(), file: "Music.aiff".into(), start: 0, clip: 1.25..3.0 },
    ];
    let stage = stage(&scene);
    let columns = stage.find("columns").unwrap().all("column");
    assert_eq!(columns.len(), 4);
    let sounds: Vec<_> = columns.iter().filter(|c| c.attribute("type") == Some("1")).collect();
    assert_eq!(sounds.len(), 2);
    assert_eq!(sounds[0].attribute("name"), Some("Voice"));
    assert_eq!(sounds[0].attribute("displayOrder"), Some("2"));
    let seq = sounds[0].child("soundSequence").unwrap();
    assert_eq!(seq.attribute("name"), Some("Voice.wav"));
    // frames from 1: half a second at 12 is 6 frames, 7 to 12
    assert_eq!((seq.attribute("startFrame"), seq.attribute("stopFrame")), (Some("7"), Some("12")));
    assert_eq!((seq.attribute("clippingTimeStart"), seq.attribute("clippingTimeStop")), (Some("0"), Some("0.5")));
    assert!(seq.child("lipSync").is_some());
    let seq = sounds[1].child("soundSequence").unwrap();
    assert_eq!((seq.attribute("startFrame"), seq.attribute("stopFrame")), (Some("1"), Some("21")));
    assert_eq!((seq.attribute("clippingTimeStart"), seq.attribute("clippingTimeStop")), (Some("1.25"), Some("3")));
    // sounds have no nodes
    assert_eq!(modules(&stage, "READ").len(), 2);
    assert_eq!(sound_path("Voice.wav"), "audio/Voice.wav");
}

#[test]
fn a_sound_starts_on_a_whole_frame() {
    let near = |a: Option<(u32, Range<f64>)>, b: (u32, Range<f64>)| {
        let a = a.expect("placed");
        a.0 == b.0 && (a.1.start - b.1.start).abs() < 1e-9 && (a.1.end - b.1.end).abs() < 1e-9
    };
    // on a frame: as it is
    assert!(near(Sound::placed(0.5, 1.0, 2.0, 12), (6, 1.0..3.0)));
    // a twentieth of a second past frame 6: that much earlier in the file, from frame 6
    assert!(near(Sound::placed(0.55, 1.0, 2.0, 12), (6, 0.95..3.0)));
    // too near the file's start for that: from frame 7, missing what plays before it
    assert!(near(Sound::placed(0.55, 0.02, 2.0, 12), (7, (0.02 + 7.0 / 12.0 - 0.55)..2.02)));
    // nothing left
    assert_eq!(Sound::placed(0.55, 0.0, 0.01, 12), None);
    assert_eq!(Sound::placed(f64::NAN, 0.0, 1.0, 12), None);
    let sound = Sound { name: "S".into(), file: "S.wav".into(), start: 0, clip: 0.0..2.0 };
    assert_eq!(sound.frames(12), 24);
    let sound = Sound { clip: 0.0..2.01, ..sound };
    assert_eq!(sound.frames(12), 25);
}

fn track(id: u64, clips: Vec<Clip>) -> Track {
    Track { id: TrackId(id), name: format!("T{id}"), hidden: false, locked: false, opacity: 1.0, look: Default::default(), clips }
}

fn drawing(id: u64, start: u32, duration: u32, mask: MaskMode) -> Clip {
    Clip {
        id: ClipId(id),
        name: "d".into(),
        start,
        duration,
        opacity: 1.0,
        blend: BlendMode::Normal,
        mask,
        adjustment: false,
        disabled: false,
        label: None,
        keys: Default::default(),
        content: ClipContent::Drawing(Drawing { root: LayerId(id) }),
    }
}

/// Three tracks, bottom first: a matte, a clip masked by it on frames 0 to 3, and one on top.
fn matted(mode: MaskMode) -> Timeline {
    let mut tl = Timeline::new(12, 8);
    tl.tracks = vec![
        track(1, vec![drawing(1, 0, 8, MaskMode::None)]),
        track(2, vec![drawing(2, 0, 4, mode), drawing(3, 4, 4, MaskMode::None)]),
        track(3, vec![drawing(4, 0, 8, MaskMode::None)]),
    ];
    tl.next_track = 4;
    tl.next_clip = 5;
    tl
}

#[test]
fn a_track_alone_keeps_its_matte_as_a_matte() {
    let mut tl = matted(MaskMode::Luminance);
    assert_eq!(matte_sources(&tl, TrackId(2), 1), [TrackId(1)]);
    assert!(matte_sources(&tl, TrackId(2), 5).is_empty());
    assert!(matte_sources(&tl, TrackId(3), 1).is_empty());
    tl.tracks[1].clips[0].blend = BlendMode::Screen;
    solo(&mut tl, TrackId(2), 1);
    let hidden: Vec<bool> = tl.tracks.iter().map(|t| t.hidden).collect();
    assert_eq!(hidden, [false, false, true]);
    let plan = tl.plan(1);
    assert_eq!(plan.items.len(), 2);
    // the matte only masks; the clip draws through it, normally
    assert!(plan.items[0].matte_only);
    assert_eq!(plan.items[1].mask, Some((MaskMode::Luminance, 0)));
    assert_eq!(plan.items[1].blend, BlendMode::Normal);
    assert_eq!(plan.drawings(), [LayerId(2)]);

    // where nothing masks it, it is alone
    let mut tl = matted(MaskMode::Alpha);
    solo(&mut tl, TrackId(2), 5);
    let hidden: Vec<bool> = tl.tracks.iter().map(|t| t.hidden).collect();
    assert_eq!(hidden, [true, false, true]);
    assert_eq!(tl.plan(5).drawings(), [LayerId(3)]);
}

#[test]
fn a_clipping_mask_alone_leaves_its_source_to_its_own_layer() {
    let mut tl = matted(MaskMode::Clipping);
    // on the timeline the source draws too
    assert_eq!(tl.plan(1).drawings(), [LayerId(1), LayerId(2), LayerId(4)]);
    solo(&mut tl, TrackId(2), 1);
    let plan = tl.plan(1);
    assert!(plan.items[0].matte_only);
    assert_eq!(plan.items[1].mask, Some((MaskMode::Alpha, 0)));
    assert_eq!(plan.drawings(), [LayerId(2)]);
}

#[test]
fn a_matte_of_a_matte_comes_along_too() {
    let mut tl = matted(MaskMode::Alpha);
    tl.tracks[2].clips[0].mask = MaskMode::Clipping;
    assert_eq!(matte_sources(&tl, TrackId(3), 2), [TrackId(2), TrackId(1)]);
    solo(&mut tl, TrackId(3), 2);
    assert!(tl.tracks.iter().all(|t| !t.hidden));
    let plan = tl.plan(2);
    assert!(plan.items[0].matte_only && plan.items[1].matte_only);
    assert_eq!(plan.drawings(), [LayerId(4)]);
}

//! Harmony scenes from the session: tracks drawn through their mattes, blend modes as blending
//! nodes, and sounds as sound columns with their files.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use procraft_engine::document::BlendMode;
use procraft_engine::view::View;
use procraft_engine::xml::{self, Element};
use procraft_engine::zip::ZipReader;
use procraft_gpu::Gpu;

use crate::media::{MediaImport, TimelineMediaKind};
use crate::timeline::TimelineMask;
use crate::{CanvasSession, PenSample, PointerKind};

fn sample(x: f32, y: f32, t: f64) -> PenSample {
    PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: t, roll: 0.0 }
}

fn session(side: u32) -> Option<Arc<CanvasSession>> {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let s = CanvasSession::new(side, side);
    {
        let mut st = s.shared.lock();
        st.window_size = (side, side);
        st.view = Some(View::fit((side, side), (side, side)));
        st.gpu = Some(gpu);
        st.ensure_canvas();
    }
    Some(s)
}

/// A folder of its own for test `name`, emptied.
fn folder(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("procraft-harmony-test-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn line(s: &CanvasSession, from: (f32, f32), to: (f32, f32), colour: [f32; 3]) {
    s.set_color(colour[0], colour[1], colour[2]);
    s.stroke_begin(PointerKind::Finger, vec![sample(from.0, from.1, 0.0)]);
    s.stroke_add(vec![sample((from.0 + to.0) / 2.0, (from.1 + to.1) / 2.0, 40.0)]);
    s.stroke_end(vec![sample(to.0, to.1, 80.0)]);
}

/// Two tracks four frames long, bottom first: "Matte", a red line down at x 10, and "Over", a
/// blue line across at y 20, over the background.
fn crossed() -> Option<Arc<CanvasSession>> {
    let s = session(40)?;
    s.set_name("Crossed".into());
    s.set_brush_size(0.3);
    line(&s, (10.0, 2.0), (10.0, 38.0), [1.0, 0.0, 0.0]);
    s.add_layer();
    line(&s, (2.0, 20.0), (38.0, 20.0), [0.0, 0.0, 1.0]);
    assert!(s.start_timeline());
    {
        let mut st = s.shared.lock();
        let tl = st.doc.timeline.as_mut().unwrap();
        tl.duration = 4;
        for (t, name) in tl.tracks.iter_mut().zip(["Matte", "Over"]) {
            t.name = name.into();
            t.clips[0].duration = 4;
        }
    }
    Some(s)
}

fn clip_of(s: &CanvasSession, track: usize) -> u64 {
    s.timeline_tracks()[track].clips[0].id
}

fn export(
    s: &CanvasSession,
    dir: &Path,
    wav: HashMap<String, String>,
) -> (crate::frame_exports::HarmonyMade, ZipReader<'static>) {
    let path = dir.join("scene.zip");
    let made = s.export_harmony(path.to_string_lossy().into_owned(), wav).unwrap();
    let bytes: &'static [u8] = Box::leak(std::fs::read(&path).unwrap().into_boxed_slice());
    (made, ZipReader::new(bytes).unwrap())
}

fn stage(zip: &ZipReader<'_>, scene: &str) -> Element {
    let text = String::from_utf8(zip.read(&format!("{scene}/{scene}.xstage")).unwrap()).unwrap();
    xml::parse(&text).unwrap()
}

fn columns(stage: &Element) -> Vec<(String, String)> {
    stage
        .find("columns")
        .unwrap()
        .all("column")
        .iter()
        .map(|c| (c.attribute("type").unwrap().to_owned(), c.attribute("name").unwrap().to_owned()))
        .collect()
}

/// Drawing `path`'s straight RGBA at `(x, y)`.
fn pixel(zip: &ZipReader<'_>, path: &str, x: usize, y: usize) -> [u8; 4] {
    let png = zip.read(path).unwrap();
    let mut reader = png::Decoder::new(std::io::Cursor::new(png)).read_info().unwrap();
    let mut px = vec![0; reader.output_buffer_size().unwrap()];
    let info = reader.next_frame(&mut px).unwrap();
    let at = (y * info.width as usize + x) * 4;
    px[at..at + 4].try_into().unwrap()
}

#[test]
fn a_matted_track_is_drawn_through_its_matte() {
    let Some(s) = crossed() else { return };
    assert!(s.set_timeline_clip_mask(clip_of(&s, 1), TimelineMask::Alpha));
    let dir = folder("alpha");
    let (made, zip) = export(&s, &dir, HashMap::new());
    // the matte is no layer of its own: it only shows through the line over it
    let stage = stage(&zip, "Crossed");
    assert_eq!(columns(&stage), [("0".to_owned(), "Background".to_owned()), ("0".to_owned(), "Over".to_owned())]);
    // one drawing, held: the line only where the matte is
    assert_eq!((made.layers, made.drawings, made.frames), (2, 2, 4));
    let over = "Crossed/elements/Over/Over-1.png";
    let crossing = pixel(&zip, over, 10, 20);
    assert!(crossing[3] > 200 && crossing[2] > 200 && crossing[0] < 60, "{crossing:?}");
    assert_eq!(pixel(&zip, over, 30, 20)[3], 0);
    // and the matte's own colour isn't in it
    assert_eq!(pixel(&zip, over, 10, 8)[3], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_clipping_mask_keeps_its_source_a_layer_of_its_own() {
    let Some(s) = crossed() else { return };
    assert!(s.set_timeline_clip_mask(clip_of(&s, 1), TimelineMask::Clipping));
    let dir = folder("clipping");
    let (_, zip) = export(&s, &dir, HashMap::new());
    let names: Vec<String> = columns(&stage(&zip, "Crossed")).into_iter().map(|(_, n)| n).collect();
    assert_eq!(names, ["Background", "Matte", "Over"]);
    let matte = "Crossed/elements/Matte/Matte-1.png";
    assert!(pixel(&zip, matte, 10, 8)[3] > 200);
    let over = "Crossed/elements/Over/Over-1.png";
    assert!(pixel(&zip, over, 10, 20)[3] > 200);
    assert_eq!(pixel(&zip, over, 30, 20)[3], 0);
    assert_eq!(pixel(&zip, over, 10, 8)[3], 0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_track_that_blends_goes_through_a_blending_node() {
    let Some(s) = crossed() else { return };
    {
        let mut st = s.shared.lock();
        let tl = st.doc.timeline.as_mut().unwrap();
        tl.tracks[1].clips[0].blend = BlendMode::Multiply;
        // a mode Harmony hasn't: normal there
        tl.tracks[0].clips[0].blend = BlendMode::HardMix;
    }
    let dir = folder("blend");
    let (_, zip) = export(&s, &dir, HashMap::new());
    let stage = stage(&zip, "Crossed");
    let blends: Vec<&Element> =
        stage.all("module").into_iter().filter(|m| m.attribute("type") == Some("BLEND_MODE_MODULE")).collect();
    assert_eq!(blends.len(), 1);
    assert_eq!(blends[0].attribute("name"), Some("Over_Blending"));
    assert_eq!(blends[0].find("blendMode").unwrap().attribute("val"), Some("eMULTIPLY_OVER"));
    let links: Vec<(&str, &str)> =
        stage.all("link").iter().map(|l| (l.attribute("out").unwrap(), l.attribute("in").unwrap())).collect();
    assert!(links.contains(&("Over", "Over_Blending")));
    assert!(links.contains(&("Over_Blending", "Composite")));
    assert!(links.contains(&("Matte", "Composite")));
    // its drawing is its own colours, drawn normally (Harmony blends it)
    let px = pixel(&zip, "Crossed/elements/Over/Over-1.png", 30, 20);
    assert!(px[2] > 200 && px[3] > 200, "{px:?}");
    let px = pixel(&zip, "Crossed/elements/Matte/Matte-1.png", 10, 8);
    assert!(px[0] > 200 && px[3] > 200, "{px:?}");
    let _ = std::fs::remove_dir_all(&dir);
}

fn import(s: &CanvasSession, path: &Path, name: &str, seconds: f64) -> u64 {
    s.import_timeline_media(MediaImport {
        path: path.to_string_lossy().into_owned(),
        name: name.to_owned(),
        kind: TimelineMediaKind::Audio,
        seconds,
        width: 0,
        height: 0,
        peaks: Vec::new(),
        sound_path: None,
        sound_seconds: 0.0,
    })
    .unwrap()
}

#[test]
fn sounds_are_sound_columns_with_their_files() {
    let Some(s) = crossed() else { return };
    let dir = folder("sound");
    let wav = dir.join("voice.wav");
    std::fs::write(&wav, b"RIFF-a-wav-file").unwrap();
    let m4a = dir.join("music.m4a");
    std::fs::write(&m4a, b"an-m4a-file").unwrap();
    s.set_timeline_frame(1);
    let voice = import(&s, &wav, "Voice line.wav", 2.0);
    let fps = s.timeline_info().unwrap().fps;
    // half a second into the file, a hundredth of a second after its first frame
    assert!(s.slip_timeline_media(voice, 0.5, true));
    assert!(s.set_timeline_sound(voice, 1.0, false, 0.01, true));
    s.set_timeline_frame(0);
    import(&s, &m4a, "Music.m4a", 3.0);
    let muted = import(&s, &wav, "Muted.wav", 1.0);
    assert!(s.set_timeline_sound(muted, 1.0, true, 0.0, true));

    // the m4a needs a WAV copy; the WAV doesn't
    let kept = s.harmony_sounds_to_convert();
    assert_eq!(kept.len(), 1);
    assert!(kept[0].ends_with(".m4a"));

    // without the copy, the m4a is left out
    let (made, zip) = export(&s, &dir, HashMap::new());
    assert_eq!(made.sounds, 1);
    let stage_ = stage(&zip, "Crossed");
    let sounds: Vec<&Element> = stage_
        .find("columns")
        .unwrap()
        .all("column")
        .into_iter()
        .filter(|c| c.attribute("type") == Some("1"))
        .collect();
    assert_eq!(sounds.len(), 1);
    assert_eq!(sounds[0].attribute("name"), Some("Voice_line"));
    let seq = sounds[0].child("soundSequence").unwrap();
    assert_eq!(seq.attribute("name"), Some("Voice_line.wav"));
    assert_eq!(zip.read("Crossed/audio/Voice_line.wav").unwrap(), b"RIFF-a-wav-file");
    // from frame 2 (1 counting from 0), starting a hundredth of a second earlier in the file
    assert_eq!(seq.attribute("startFrame"), Some("2"));
    let start: f64 = seq.attribute("clippingTimeStart").unwrap().parse().unwrap();
    let stop: f64 = seq.attribute("clippingTimeStop").unwrap().parse().unwrap();
    assert!((start - 0.49).abs() < 1e-6, "{start}");
    let sounds_now = s.timeline_sounds(0);
    let heard = sounds_now.iter().find(|t| t.clip == voice).unwrap();
    assert!((stop - (heard.from + heard.length)).abs() < 1e-6, "{stop}");
    let frames = ((stop - start) * f64::from(fps) - 1e-6).ceil() as u32;
    assert_eq!(seq.attribute("stopFrame"), Some((1 + frames).to_string().as_str()));

    // with it, both
    let copy = dir.join("music-copy.wav");
    std::fs::write(&copy, b"RIFF-converted").unwrap();
    let converted = HashMap::from([(kept[0].clone(), copy.to_string_lossy().into_owned())]);
    let (made, zip) = export(&s, &dir, converted);
    assert_eq!(made.sounds, 2);
    assert_eq!(zip.read("Crossed/audio/Music.wav").unwrap(), b"RIFF-converted");
    let stage_ = stage(&zip, "Crossed");
    let names: Vec<String> = columns(&stage_).into_iter().filter(|(t, _)| t == "1").map(|(_, n)| n).collect();
    assert_eq!(names, ["Music", "Voice_line"]);
    let _ = std::fs::remove_dir_all(&dir);
}

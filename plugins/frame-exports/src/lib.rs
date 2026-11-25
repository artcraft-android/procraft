//! Frame exports: the animation as a sprite sheet (its frames laid out on one picture, with the
//! JSON a game engine reads to cut them out) and as a Toon Boom Harmony scene (each track a
//! drawing layer whose exposure sheet carries the timing). It's a plugin, not part of the app:
//! the app draws the frames and puts the pictures together; what goes where, and every file's
//! text, is here. `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The interface is written down at the top of `crates/core/src/plugin_exports.rs`.

mod harmony;
mod sheet;

use std::sync::Mutex;

use kit::layer::{Record, fields};

/// The controls, in the manifest's order: the sprite sheet's longest side (0 full size), layout
/// (0 grid, 1 row, 2 column), columns and rows (0 worked out), padding, trim, held frames once,
/// JSON, and the background left out.
static CONTROLS: kit::Controls<9> = kit::Controls::new([0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
kit::controls!(CONTROLS);

/// Where records come in and go out.
static IN: kit::Place = kit::Place::new();
static OUT: kit::Place = kit::Place::new();

/// The export under way.
static EXPORT: Mutex<Option<Export>> = Mutex::new(None);

/// The exports, by their place in the manifest.
const SPRITE_SHEET: i32 = 0;
const HARMONY: i32 = 1;

/// What the app tells of the artwork as an export begins.
#[derive(Clone, Debug, Default)]
pub(crate) struct Artwork {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub frames: u32,
    pub name: String,
    pub tracks: Vec<String>,
    pub sounds: Vec<Sound>,
    pub background: bool,
}

/// A sound the animation plays, as the app tells it.
#[derive(Clone, Debug, Default)]
pub(crate) struct Sound {
    pub name: String,
    pub file: u32,
    pub stem: String,
    pub extension: String,
    pub at: f64,
    pub from: f64,
    pub length: f64,
}

/// What the app tells of a picture drawn.
#[derive(Clone, Debug, Default)]
pub(crate) struct Picture {
    pub kind: u32,
    pub track: u32,
    pub frame: u32,
    /// Where it is painted (x, y, width, height); none where nothing is.
    pub painted: Option<[u32; 4]>,
    pub hash: u64,
    pub blend: String,
    pub held: bool,
    pub matte: bool,
}

pub(crate) const FRAME: u32 = 0;
pub(crate) const TRACK: u32 = 1;
pub(crate) const BACKGROUND: u32 = 2;

/// A file to write, as a plan holds it.
pub(crate) enum File {
    Bytes(String, Vec<u8>),
    /// A picture put together: its path, width, height, and each part placed (the number kept
    /// by, x, y, width and height in its frame, and where its top left goes).
    Image(String, u32, u32, Vec<[u32; 7]>),
    Sound(String, u32),
}

/// An export taking its pictures in.
enum Export {
    Sheet(sheet::Sheet),
    Scene(harmony::SceneBuilder),
}

fn u32_of(bytes: &[u8]) -> u32 {
    bytes.get(..4).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes)
}

fn f64_of(bytes: &[u8]) -> f64 {
    bytes.get(..8).and_then(|b| b.try_into().ok()).map_or(0.0, f64::from_le_bytes)
}

fn text_of(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes).into_owned()
}

fn artwork(bytes: &[u8]) -> Artwork {
    let mut a = Artwork::default();
    for (id, value) in fields(bytes) {
        match id {
            1 => a.width = u32_of(value),
            2 => a.height = u32_of(value),
            3 => a.fps = u32_of(value),
            4 => a.frames = u32_of(value),
            5 => a.name = text_of(value),
            6 => a.tracks.push(fields(value).find(|(id, _)| *id == 1).map(|(_, v)| text_of(v)).unwrap_or_default()),
            7 => {
                let mut s = Sound::default();
                for (id, v) in fields(value) {
                    match id {
                        1 => s.name = text_of(v),
                        2 => s.file = u32_of(v),
                        3 => s.stem = text_of(v),
                        4 => s.extension = text_of(v),
                        5 => s.at = f64_of(v),
                        6 => s.from = f64_of(v),
                        7 => s.length = f64_of(v),
                        _ => {}
                    }
                }
                a.sounds.push(s);
            }
            8 => a.background = value.first().is_some_and(|b| *b != 0),
            _ => {}
        }
    }
    a
}

fn picture(bytes: &[u8]) -> Picture {
    let mut p = Picture::default();
    for (id, value) in fields(bytes) {
        match id {
            1 => p.kind = u32_of(value),
            2 => p.track = u32_of(value),
            3 => p.frame = u32_of(value),
            4 => {
                let v: Vec<u32> = value.chunks_exact(4).map(u32_of).collect();
                if let [x, y, w, h] = v[..] {
                    p.painted = (w > 0 && h > 0).then_some([x, y, w, h]);
                }
            }
            5 => p.hash = value.get(..8).and_then(|b| b.try_into().ok()).map_or(0, u64::from_le_bytes),
            6 => p.blend = text_of(value),
            7 => p.held = value.first().is_some_and(|b| *b != 0),
            8 => p.matte = value.first().is_some_and(|b| *b != 0),
            _ => {}
        }
    }
    p
}

/// A plan of `files`, with what the app says when it's done.
fn plan(files: Vec<File>, said: &str) -> Vec<u8> {
    let mut out = Record::default();
    for f in files {
        let mut r = Record::default();
        match f {
            File::Bytes(path, bytes) => {
                r.field(1, path.as_bytes()).field(2, &bytes);
            }
            File::Image(path, w, h, places) => {
                let mut image = Record::default();
                image.field(1, &w.to_le_bytes()).field(2, &h.to_le_bytes());
                for place in places {
                    let bytes: Vec<u8> = place.iter().flat_map(|v| v.to_le_bytes()).collect();
                    image.field(3, &bytes);
                }
                r.field(1, path.as_bytes()).field(3, &image.bytes());
            }
            File::Sound(path, file) => {
                r.field(1, path.as_bytes()).field(4, &file.to_le_bytes());
            }
        }
        out.field(1, &r.bytes());
    }
    out.field(2, said.as_bytes());
    out.bytes()
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_export_in(len: i32) -> i32 {
    IN.make(len)
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_export_out() -> i32 {
    OUT.with(|bytes| bytes.as_mut_ptr() as usize as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_export_begin(export: i32, len: i32) -> i32 {
    let bytes = IN.take();
    let art = artwork(bytes.get(..len.max(0) as usize).unwrap_or_default());
    let made = match export {
        SPRITE_SHEET => Export::Sheet(sheet::Sheet::new(art, sheet::Options::from_controls(&CONTROLS))),
        HARMONY => Export::Scene(harmony::SceneBuilder::new(art)),
        _ => return -1,
    };
    *EXPORT.lock().unwrap_or_else(|e| e.into_inner()) = Some(made);
    0
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_export_picture(len: i32) -> i32 {
    let bytes = IN.take();
    let p = picture(bytes.get(..len.max(0) as usize).unwrap_or_default());
    let mut export = EXPORT.lock().unwrap_or_else(|e| e.into_inner());
    let kept = match export.as_mut() {
        Some(Export::Sheet(s)) => s.add(&p),
        Some(Export::Scene(s)) => s.add(&p),
        None => None,
    };
    kept.map_or(-1, |n| n as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_export_finish() -> i32 {
    let export = EXPORT.lock().unwrap_or_else(|e| e.into_inner()).take();
    let made = match export {
        Some(Export::Sheet(s)) => s.finish(),
        Some(Export::Scene(s)) => Some(s.finish()),
        None => None,
    };
    let Some((files, said)) = made else { return -1 };
    let bytes = plan(files, &said);
    let len = bytes.len() as i32;
    OUT.put(bytes);
    len
}

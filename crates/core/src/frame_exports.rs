//! The animation's frames for other apps: a sprite sheet (a PNG of the frames laid out, with the
//! JSON a game engine reads to cut them out) and a Toon Boom Harmony scene (each track a drawing
//! layer, its holds on the exposure sheet).
//!
//! Both are built in, not plugins: they draw the timeline's frames through the canvas's renderer,
//! which a plugin's module can't reach.

use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use make2d_engine::document::{BlendMode, Document, LayerId};
use make2d_engine::harmony::{self, Drawings, Names};
use make2d_engine::media::media_extension;
use make2d_engine::sprites::{Arrangement, SheetBuilder, SheetOptions};
use make2d_engine::timeline::{ClipContent, MediaId, PlanItem, Shows, Timeline, TrackId};
use make2d_engine::zip::{Ready, ZipWriter};

use crate::animation::{Shot, shots};
use crate::project::{file_error, png_bytes, write_atomically};
use crate::{CanvasSession, MobileError};

/// How a sprite sheet lays out its frames.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SheetLayout {
    /// Rows of frames (`columns` and `rows`, 0 worked out).
    Grid,
    /// One row.
    Across,
    /// One column.
    Down,
}

/// What a sprite sheet is asked to be.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SpriteSheetChoices {
    pub layout: SheetLayout,
    /// A grid's columns and rows; 0 is worked out (both 0: as near square as can be).
    pub columns: u32,
    pub rows: u32,
    /// Clear pixels between neighbouring frames.
    pub padding: u32,
    /// Each frame cut to what is painted in it (where from is in the JSON).
    pub trim: bool,
    /// A held or repeated frame on the sheet once, lasting as long as it shows.
    pub held_once: bool,
    /// The JSON beside the picture.
    pub json: bool,
    /// The frames scaled to fit this (0: full size).
    pub max_side: u32,
    /// The background left out.
    pub transparent: bool,
}

/// A sprite sheet made.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct SpriteSheetMade {
    pub width: u32,
    pub height: u32,
    /// Pictures on the sheet.
    pub cells: u32,
    /// The animation's frames it shows.
    pub frames: u32,
    /// The JSON written beside it, if asked for.
    pub json_path: Option<String>,
}

/// A Harmony scene made.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct HarmonyMade {
    pub layers: u32,
    pub drawings: u32,
    pub frames: u32,
    pub sounds: u32,
}

/// A frame drawn: its width, height and straight RGBA.
type Drawn = (u32, u32, Vec<u8>);

/// The artwork's frame rate: its timeline's, Animation Assist's, else 15.
fn fps_of(doc: &Document) -> u32 {
    doc.timeline.as_ref().map(|t| t.fps).or(doc.animation.as_ref().map(|a| a.fps)).unwrap_or(15).max(1)
}

/// `items` with each one's own frame taken out: what draws the same picture compares equal.
fn still(items: &[PlanItem]) -> Vec<PlanItem> {
    items
        .iter()
        .map(|i| {
            let mut i = i.clone();
            i.local = 0;
            if let make2d_engine::timeline::Shows::Group(inner) = &i.shows {
                i.shows = make2d_engine::timeline::Shows::Group(still(inner));
            }
            i
        })
        .collect()
}

/// Whether shots `a` and `b` of `doc` are sure to look the same (the same frame alone, or
/// timeline frames showing the same drawings in the same way under a still camera), so the one
/// drawn can stand for the other.
pub(crate) fn same_shot(doc: &Document, a: Shot, b: Shot) -> bool {
    match (a, b) {
        (Shot::Unit(x), Shot::Unit(y)) => x == y,
        (Shot::At(x), Shot::At(y)) => doc
            .timeline
            .as_ref()
            .is_some_and(|tl| tl.camera.keys.len() <= 1 && still(&tl.plan(x).items) == still(&tl.plan(y).items)),
        _ => false,
    }
}

/// A plain file name from the artwork's name.
fn base_name(name: &str) -> String {
    Names::plain(name, "Animation")
}

#[uniffi::export]
impl CanvasSession {
    /// The animation's frames as a sprite sheet PNG at `path` (and, if asked, its JSON beside it,
    /// the same name ending `.json`), laid out as `choices` ask.
    pub fn export_sprite_sheet(
        &self,
        path: String,
        choices: SpriteSheetChoices,
    ) -> Result<SpriteSheetMade, MobileError> {
        let (doc, name) = {
            let st = self.shared.lock();
            (st.doc.clone(), st.name.clone())
        };
        let units = shots(&doc);
        let fps = fps_of(&doc);
        let arrangement = match choices.layout {
            SheetLayout::Grid => Arrangement::Grid { columns: choices.columns, rows: choices.rows },
            SheetLayout::Across => Arrangement::Across,
            SheetLayout::Down => Arrangement::Down,
        };
        let mut sheet = SheetBuilder::new(SheetOptions {
            arrangement,
            padding: choices.padding.min(1024),
            trim: choices.trim,
            once: choices.held_once,
        });
        // the frames to 0.9, the files to 1
        self.exported(0.0);
        let mut last: Option<(Shot, Drawn)> = None;
        for (k, &unit) in units.iter().enumerate() {
            let drawn = match last.take() {
                // held: the frame before again, not drawn twice
                Some((before, drawn)) if same_shot(&doc, before, unit) => drawn,
                _ => self.render_shot(unit, choices.transparent, choices.max_side)?,
            };
            let (w, h, rgba) = &drawn;
            sheet.add(*w, *h, rgba).map_err(|e| file_error("saving", e))?;
            last = Some((unit, drawn));
            self.exported(0.9 * (k + 1) as f32 / units.len().max(1) as f32);
        }
        let frames = sheet.frames();
        let sheet = sheet.finish().map_err(|e| file_error("saving", e))?;
        let png = png_bytes(sheet.width, sheet.height, &sheet.rgba, &[])?;
        let path = Path::new(&path);
        write_atomically(path, |f| fs::write(f, &png).map_err(|e| file_error("saving", e)))?;
        let json_path = if choices.json {
            let image = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            let json = sheet.json(&image, &base_name(&name), fps as f32);
            let at = path.with_extension("json");
            write_atomically(&at, |f| fs::write(f, json.as_bytes()).map_err(|e| file_error("saving", e)))?;
            Some(at.to_string_lossy().into_owned())
        } else {
            None
        };
        self.exported(1.0);
        Ok(SpriteSheetMade {
            width: sheet.width,
            height: sheet.height,
            cells: u32::try_from(sheet.places.len()).unwrap_or(u32::MAX),
            frames,
            json_path,
        })
    }

    /// The sound files the timeline plays that a Harmony scene can't carry as they are (Harmony
    /// reads WAV and AIFF), to be made WAV for [`Self::export_harmony`].
    pub fn harmony_sounds_to_convert(&self) -> Vec<String> {
        let st = self.shared.lock();
        let Some(tl) = st.doc.timeline.as_ref() else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for s in tl.sounds().iter().filter(|s| !s.muted) {
            let Some(path) = tl.media_of(s.media).and_then(|m| st.media.find(&m.file_name())) else { continue };
            let path = path.to_string_lossy().into_owned();
            if !harmony_reads(&path) && !out.contains(&path) {
                out.push(path);
            }
        }
        out
    }

    /// The animation as a Toon Boom Harmony scene, zipped at `path`: a folder named for the
    /// artwork holding the scene, each visible track a drawing layer (bottom first, the background
    /// a layer below them if it shows) whose drawings are what the track shows on its own through
    /// its mattes, each written once and exposed for as long as it holds, blending as its clips do
    /// where Harmony has the mode; and each sound that plays a sound column, its file in the
    /// scene. `wav` gives a WAV copy of each sound file Harmony can't read (see
    /// [`Self::harmony_sounds_to_convert`]); a sound with none is left out. A canvas with Animation
    /// Assist is put on a timeline for it first, as Timeline would.
    pub fn export_harmony(&self, path: String, wav: HashMap<String, String>) -> Result<HarmonyMade, MobileError> {
        let (mut doc, name) = {
            let st = self.shared.lock();
            (st.doc.clone(), st.name.clone())
        };
        // as the copies each frame is drawn from will be
        let on_timeline = |d: &mut Document| {
            if d.timeline.is_none() {
                d.start_timeline();
            }
        };
        on_timeline(&mut doc);
        let Some(tl) = doc.timeline.clone() else {
            return Err(file_error("saving", "the canvas can't be put on a timeline"));
        };
        let scene_name = Names::plain(&name, "Scene");
        let (w, h) = (doc.width(), doc.height());
        let mut names = Names::scene();
        let mut zip = ZipWriter::new();
        let mut layers = Vec::new();
        let tracks: Vec<&make2d_engine::timeline::Track> = tl.tracks.iter().filter(|t| draws(t)).collect();
        let jobs = (tracks.len() as u64 * u64::from(tl.duration)).max(1);
        let mut done = 0u64;
        self.exported(0.0);

        // the background, a layer of one drawing under the rest
        if doc.background_visible {
            let layer = names.unique("Background", "Background");
            let (_, _, rgba) = self.render_shot_of(Shot::At(0), false, 0, |d| {
                on_timeline(d);
                if let Some(t) = d.timeline.as_mut() {
                    t.tracks.iter_mut().for_each(|t| t.hidden = true);
                }
            })?;
            add_drawing(&mut zip, &scene_name, &layer, 1, w, h, &rgba)?;
            layers.push(harmony::Layer {
                name: layer,
                drawings: 1,
                exposures: vec![Some(1); tl.duration as usize],
                blend: BlendMode::Normal,
            });
        }

        // a still camera: a frame that shows the same as the one before is the same picture
        let still_camera = tl.camera.keys.len() <= 1;
        for track in tracks {
            let layer = names.unique(&track.name, "Layer");
            let mut drawings: Drawings<(u64, usize)> = Drawings::new();
            // what the frame last drawn showed, and how many frames each blend mode is shown in
            let mut last: Option<Vec<PlanItem>> = None;
            let mut blends: Vec<(BlendMode, u32)> = Vec::new();
            for frame in 0..tl.duration {
                done += 1;
                self.exported(0.95 * done as f32 / jobs as f32);
                // a matte for the clip above shows only through it, drawn there
                if consumed(&tl, track.id, frame) {
                    drawings.show(None);
                    last = None;
                    continue;
                }
                let blend = blend_of(&doc, &tl, track.id, frame);
                let mut alone = tl.clone();
                harmony::solo(&mut alone, track.id, frame);
                let shows = still(&alone.plan(frame).items);
                if still_camera && last.as_ref() == Some(&shows) {
                    drawings.hold();
                    if let Some(b) = blend {
                        count(&mut blends, b);
                    }
                    continue;
                }
                let (_, _, rgba) = self.render_shot_of(Shot::At(frame), true, 0, |d| {
                    on_timeline(d);
                    d.timeline = Some(alone);
                    // its layer's own mode is the layer's in Harmony too
                    if let Some(root) = root_at(&tl, track.id, frame) {
                        d.set_blend(root, BlendMode::Normal);
                    }
                })?;
                last = Some(shows);
                let key = rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0).then(|| (hash_of(&rgba), rgba.len()));
                if let Some((n, new)) = drawings.show(key) {
                    if new {
                        add_drawing(&mut zip, &scene_name, &layer, n, w, h, &rgba)?;
                    }
                    if let Some(b) = blend {
                        count(&mut blends, b);
                    }
                }
            }
            // a track that never showed anything has no drawings to be a layer
            if drawings.count() > 0 {
                // a layer has one mode: the one most of its frames show with
                let blend = blends.iter().max_by_key(|(_, n)| *n).map_or(BlendMode::Normal, |(b, _)| *b);
                layers.push(harmony::Layer {
                    name: layer,
                    drawings: drawings.count(),
                    exposures: drawings.exposures,
                    blend,
                });
            }
        }

        let sounds = self.harmony_sounds(&tl, &wav, &mut names, &mut zip, &scene_name);
        let scene = harmony::Scene {
            name: scene_name.clone(),
            width: w,
            height: h,
            fps: tl.fps,
            frames: tl.duration,
            layers,
            sounds,
        };
        let id = {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (&name, std::time::SystemTime::now()).hash(&mut hasher);
            hasher.finish()
        };
        for (file, text) in harmony::files(&scene, id) {
            zip.add(&format!("{scene_name}/{file}"), text.as_bytes());
        }
        let bytes = zip.finish();
        write_atomically(Path::new(&path), |f| fs::write(f, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(HarmonyMade {
            layers: u32::try_from(scene.layers.len()).unwrap_or(u32::MAX),
            drawings: scene.layers.iter().map(|l| l.drawings).sum(),
            frames: scene.frames,
            sounds: u32::try_from(scene.sounds.len()).unwrap_or(u32::MAX),
        })
    }
}

impl CanvasSession {
    /// The sounds `tl` plays as Harmony sound columns, their files (each once, as Harmony reads
    /// it: as it is, or its WAV copy in `wav`) into the zipped scene folder `scene`. A sound
    /// muted, or whose file can't be found or read, is left out: the picture is a whole scene
    /// without it.
    fn harmony_sounds(
        &self,
        tl: &Timeline,
        wav: &HashMap<String, String>,
        names: &mut Names,
        zip: &mut ZipWriter,
        scene: &str,
    ) -> Vec<harmony::Sound> {
        let found: Vec<_> = {
            let st = self.shared.lock();
            tl.sounds()
                .into_iter()
                .filter(|s| !s.muted)
                .filter_map(|s| {
                    let m = tl.media_of(s.media)?;
                    let path = st.media.find(&m.file_name())?.to_string_lossy().into_owned();
                    let name = tl.clip(s.clip).map(|c| c.name.clone()).filter(|n| !n.trim().is_empty());
                    Some((s, m.name.clone(), name, path))
                })
                .collect()
        };
        // the file each media went in as, named apart from the columns
        let mut files: Vec<(MediaId, String)> = Vec::new();
        let mut file_names = Names::default();
        let mut out = Vec::new();
        for (s, media_name, clip_name, path) in found {
            let Some((start, clip)) = harmony::Sound::placed(s.at, s.from, s.length, tl.fps) else { continue };
            let file = match files.iter().find(|(m, _)| *m == s.media) {
                Some((_, file)) => file.clone(),
                None => {
                    let readable = if harmony_reads(&path) { Some(path.clone()) } else { wav.get(&path).cloned() };
                    let Some(bytes) = readable.and_then(|p| fs::read(&p).ok().map(|b| (b, p))) else { continue };
                    let (bytes, from) = bytes;
                    let stem = media_name.rsplit_once('.').map_or(media_name.as_str(), |(stem, _)| stem);
                    let file = format!("{}.{}", file_names.unique(stem, "Sound"), media_extension(&from));
                    zip.add(&format!("{scene}/{}", harmony::sound_path(&file)), &bytes);
                    files.push((s.media, file.clone()));
                    file
                }
            };
            let name = names.unique(clip_name.as_deref().unwrap_or(&media_name), "Sound");
            out.push(harmony::Sound { name, file, start, clip });
        }
        out
    }
}

/// Whether Harmony reads the sound file at `path` as it is.
fn harmony_reads(path: &str) -> bool {
    matches!(media_extension(path).as_str(), "wav" | "wave" | "aif" | "aiff")
}

/// One more frame shown with `mode`.
fn count(blends: &mut Vec<(BlendMode, u32)>, mode: BlendMode) {
    match blends.iter_mut().find(|(b, _)| *b == mode) {
        Some((_, n)) => *n += 1,
        None => blends.push((mode, 1)),
    }
}

/// The drawing track `track` of `tl` shows at `frame`, if it shows one.
fn root_at(tl: &Timeline, track: TrackId, frame: u32) -> Option<LayerId> {
    let clip = tl.track(track)?.clip_at(frame)?;
    match clip.content {
        ClipContent::Drawing(ref d) => Some(d.root),
        _ => None,
    }
}

/// How what track `track` shows at `frame` blends with what is under it: its clip's mode, or
/// the mode of the layer it shows if the clip's is normal. None if it shows nothing.
fn blend_of(doc: &Document, tl: &Timeline, track: TrackId, frame: u32) -> Option<BlendMode> {
    let item = tl.plan(frame).items.into_iter().find(|i| i.track == track)?;
    if item.blend != BlendMode::Normal {
        return Some(item.blend);
    }
    Some(match item.shows {
        Shows::Drawing(root) => doc.layer(root).map_or(BlendMode::Normal, |l| l.blend),
        Shows::Group(_) | Shows::Video { .. } => BlendMode::Normal,
    })
}

/// Whether `track` has anything to draw: a drawing, a group or a video, shown.
fn draws(track: &make2d_engine::timeline::Track) -> bool {
    !track.hidden
        && track.clips.iter().any(|c| !c.disabled && !c.adjustment && !matches!(c.content, ClipContent::Audio(_)))
}

/// Whether what track `track` shows at `frame` is only a matte for the clip above it.
fn consumed(tl: &Timeline, track: TrackId, frame: u32) -> bool {
    tl.plan(frame).items.iter().any(|i| i.track == track && i.matte_only)
}

fn hash_of(rgba: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    rgba.hash(&mut h);
    h.finish()
}

/// Drawing `n` of `layer`, `w` × `h` straight RGBA, into the zipped scene folder `scene`.
fn add_drawing(
    zip: &mut ZipWriter,
    scene: &str,
    layer: &str,
    n: u32,
    w: u32,
    h: u32,
    rgba: &[u8],
) -> Result<(), MobileError> {
    let png = png_bytes(w, h, rgba, &[])?;
    // a PNG is squeezed already
    zip.add_ready(&format!("{scene}/{}", harmony::drawing_path(layer, n)), Ready::of(&png, false));
    Ok(())
}

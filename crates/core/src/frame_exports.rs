//! The animation's frames for other apps: a sprite sheet (a PNG of the frames laid out, with the
//! JSON a game engine reads to cut them out) and a Toon Boom Harmony scene (each track a drawing
//! layer, its holds on the exposure sheet).
//!
//! Both are built in, not plugins: they draw the timeline's frames through the canvas's renderer,
//! which a plugin's module can't reach.

use std::fs;
use std::hash::{Hash, Hasher};
use std::path::Path;

use make2d_engine::document::Document;
use make2d_engine::harmony::{self, Drawings, Names};
use make2d_engine::sprites::{Arrangement, SheetBuilder, SheetOptions};
use make2d_engine::timeline::{ClipContent, PlanItem, Timeline, TrackId};
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

    /// The animation as a Toon Boom Harmony scene, zipped at `path`: a folder named for the
    /// artwork holding the scene, each visible track a drawing layer (bottom first, the background
    /// a layer below them if it shows) whose drawings are what the track shows on its own, each
    /// written once and exposed for as long as it holds. A canvas with Animation Assist is put on
    /// a timeline for it first, as Timeline would.
    pub fn export_harmony(&self, path: String) -> Result<HarmonyMade, MobileError> {
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
        let mut names = Names::default();
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
            layers.push(harmony::Layer { name: layer, drawings: 1, exposures: vec![Some(1); tl.duration as usize] });
        }

        for track in tracks {
            let layer = names.unique(&track.name, "Layer");
            let alone = |d: &mut Document| {
                on_timeline(d);
                if let Some(t) = d.timeline.as_mut() {
                    t.tracks.iter_mut().for_each(|t| t.hidden = t.id != track.id);
                }
            };
            let mut shown = doc.clone();
            alone(&mut shown);
            let mut drawings: Drawings<(u64, usize)> = Drawings::new();
            let mut last: Option<u32> = None;
            for frame in 0..tl.duration {
                done += 1;
                self.exported(0.95 * done as f32 / jobs as f32);
                // a matte for the clip above shows only through it, which isn't drawn here
                if consumed(&tl, track.id, frame) {
                    drawings.show(None);
                    last = None;
                    continue;
                }
                if let Some(before) = last
                    && same_shot(&shown, Shot::At(before), Shot::At(frame))
                {
                    drawings.hold();
                    continue;
                }
                let (_, _, rgba) = self.render_shot_of(Shot::At(frame), true, 0, alone)?;
                last = Some(frame);
                let key = rgba.as_chunks::<4>().0.iter().any(|p| p[3] != 0).then(|| (hash_of(&rgba), rgba.len()));
                if let Some((n, true)) = drawings.show(key) {
                    add_drawing(&mut zip, &scene_name, &layer, n, w, h, &rgba)?;
                }
            }
            // a track that never showed anything has no drawings to be a layer
            if drawings.count() > 0 {
                layers.push(harmony::Layer { name: layer, drawings: drawings.count(), exposures: drawings.exposures });
            }
        }

        let scene =
            harmony::Scene { name: scene_name.clone(), width: w, height: h, fps: tl.fps, frames: tl.duration, layers };
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
        })
    }
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

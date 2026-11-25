//! Files a plugin makes of the animation for Share (interface 23): a sprite sheet, a scene for
//! another app. The pixels are the app's and the files' shape the plugin's. The app draws what the
//! export asks for and tells the plugin of each picture (where it is painted, a hash of it, whether
//! it holds the one before); the plugin keeps those it wants by a number of its own; the app keeps
//! what is painted of each, squeezed as a PNG. Then the plugin says what files to write: bytes of
//! its own (a scene's text, a sheet's JSON), pictures it lays out from those it kept, which the app
//! puts together and writes as PNGs, or a sound's file as the app has it. Its code never holds a
//! picture, so its memory holds an export of any length.
//!
//! What it asks to be drawn:
//! - `frames`: each of the animation's frames, whole, as the animated exports draw them (a held
//!   frame not drawn again).
//! - `tracks`: each drawing track alone, a frame at a time, as it shows through the mattes below
//!   it, blending normally (its mode is told), and the background on its own first if it shows.
//!
//! Records here are those `plugin_layers.rs` writes: fields one after another, each a 16-bit id, a
//! 32-bit length and its bytes, little-endian. Numbers are little-endian too; text is UTF-8.
//!
//! The artwork, as the export begins (`procraft_export_begin(export, len)`, not below 0 to go on):
//! [`artwork`]. Each picture (`procraft_export_picture(len)`, the number it keeps it by, or below 0
//! to leave it): [`picture`]. Its plan (`procraft_export_finish()`, its length, at
//! `procraft_export_out()`; below 0 for nothing): [`plan`].

use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use procraft_engine::document::{BlendMode, Document, LayerId};
use procraft_engine::media::media_extension;
use procraft_engine::timeline::{ClipContent, MediaId, PlanItem, Shows, Timeline, TrackId};
use procraft_engine::zip::{Ready, ZipWriter};

use crate::animation::{Shot, shots};
use crate::plugin_file::ExportAsks;
use crate::plugin_layers::{Record, fields};
use crate::plugins::{ExportToRun, Plugins};
use crate::project::{file_error, png_bytes, write_atomically};
use crate::{CanvasSession, MobileError};

/// The fields of the artwork told as an export begins.
pub mod artwork {
    /// The pictures' width and height (u32 each), as drawn.
    pub const WIDTH: u16 = 1;
    pub const HEIGHT: u16 = 2;
    /// Frames a second (u32).
    pub const FPS: u16 = 3;
    /// How many of the animation's frames there are (u32).
    pub const FRAMES: u16 = 4;
    /// The artwork's name.
    pub const NAME: u16 = 5;
    /// A track drawn (`tracks`), one field each, bottom first: a record of [`super::track`].
    pub const TRACK: u16 = 6;
    /// A sound the animation plays (an export that takes sounds), one field each: a record of
    /// [`super::sound`].
    pub const SOUND: u16 = 7;
    /// 1 if the background comes as a picture of its own (`tracks`), 0 if it doesn't (u8).
    pub const BACKGROUND: u16 = 8;
}

/// The fields of a track.
pub mod track {
    pub const NAME: u16 = 1;
}

/// The fields of a sound.
pub mod sound {
    /// Its clip's name, or its file's if the clip has none.
    pub const NAME: u16 = 1;
    /// The file it plays (u32), by a number of its own: sounds that play one file share it.
    pub const FILE: u16 = 2;
    /// That file's name, without its extension, and the extension of the file the app hands over
    /// (a WAV copy's `wav` where it was made one).
    pub const STEM: u16 = 3;
    pub const EXTENSION: u16 = 4;
    /// Where in the animation it is heard (seconds), where in its file it starts (seconds), and
    /// how long it plays (seconds), f64 each.
    pub const AT: u16 = 5;
    pub const FROM: u16 = 6;
    pub const LENGTH: u16 = 7;
}

/// The fields of a picture drawn.
pub mod picture {
    /// What it is (u32): [`super::FRAME`], [`super::TRACK`] or [`super::BACKGROUND`].
    pub const KIND: u16 = 1;
    /// A track's place among the artwork's tracks (u32).
    pub const TRACK: u16 = 2;
    /// The animation's frame it shows (u32).
    pub const FRAME: u16 = 3;
    /// Where it is painted: x, y, width and height (u32 each); all 0 where nothing is.
    pub const PAINTED: u16 = 4;
    /// A hash of where it is painted and what is (u64): pictures alike hash alike.
    pub const HASH: u16 = 5;
    /// A track's blend mode at the frame, by name (`Normal`, `Multiply`...).
    pub const BLEND: u16 = 6;
    /// 1 if it is the same as the one before it of its kind and track, not drawn again: nothing
    /// else is told (u8).
    pub const HELD: u16 = 7;
    /// 1 if what its track shows here only mattes the track above (u8): nothing of its own.
    pub const MATTE: u16 = 8;
}

/// A picture of one of the animation's frames, whole.
pub const FRAME: u32 = 0;
/// A picture of what one track shows at a frame.
pub const TRACK: u32 = 1;
/// The background alone.
pub const BACKGROUND: u32 = 2;

/// The fields of an export's plan.
pub mod plan {
    /// A file to write, one field each: a record of [`super::file`].
    pub const FILE: u16 = 1;
    /// What the app says when it's done ("12 frames on a 4 × 3 sheet").
    pub const SAID: u16 = 2;
}

/// The fields of a file in a plan: its path, and one of what it holds.
pub mod file {
    /// Its path: in a zip, with folders (`/`); otherwise a name alone.
    pub const PATH: u16 = 1;
    /// Bytes of its own.
    pub const BYTES: u16 = 2;
    /// A PNG the app puts together: a record of [`super::image`].
    pub const IMAGE: u16 = 3;
    /// The sound file of that number (u32), as the app hands it over.
    pub const SOUND: u16 = 4;
}

/// The fields of a picture to put together.
pub mod image {
    /// Its width and height (u32 each).
    pub const WIDTH: u16 = 1;
    pub const HEIGHT: u16 = 2;
    /// A part of a picture kept, put on it, one field each (u32 each): the number it was kept by,
    /// the part's x, y, width and height in its frame, and where its top left goes.
    pub const PLACE: u16 = 3;
}

/// The most pixels across or down a picture put together, and in all (a gigabyte of RGBA).
const MOST_SIDE: u32 = 32_768;
const MOST_PIXELS: u64 = 1 << 28;
/// The most files a plan writes.
const MOST_FILES: usize = 100_000;

const NOTHING_MADE: &str = "The plugin made nothing of this animation.";

/// Files a plugin made: where each was written, and what to say.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginShared {
    pub files: Vec<String>,
    pub said: Option<String>,
}

/// A rectangle in a frame's pixels.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
struct Rect {
    x: u32,
    y: u32,
    w: u32,
    h: u32,
}

/// What is painted of a picture kept: where, and its pixels as a PNG.
struct Kept {
    at: Rect,
    png: Vec<u8>,
}

/// A sound file to hand over: where it is read from, by the number the sounds give it.
type SoundFiles = HashMap<u32, String>;

fn u32s(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn u32_of(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

/// The smallest rectangle holding every pixel of `rgba` (straight, `width` × `height`) that isn't
/// wholly clear; none if all of it is.
fn painted(width: u32, height: u32, rgba: &[u8]) -> Option<Rect> {
    let w = width as usize;
    let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0, 0);
    for (y, row) in rgba.chunks_exact(w.max(1) * 4).take(height as usize).enumerate() {
        let Some(first) = row.as_chunks::<4>().0.iter().position(|p| p[3] != 0) else { continue };
        let last = row.as_chunks::<4>().0.iter().rposition(|p| p[3] != 0).unwrap_or(first);
        left = left.min(first);
        right = right.max(last);
        top = top.min(y);
        bottom = y;
    }
    (top != usize::MAX).then(|| Rect {
        x: left as u32,
        y: top as u32,
        w: (right - left + 1) as u32,
        h: (bottom - top + 1) as u32,
    })
}

/// The part `r` of `rgba` (`width` across), as its own pixels.
fn crop(width: u32, rgba: &[u8], r: Rect) -> Vec<u8> {
    let stride = width as usize * 4;
    let mut out = Vec::with_capacity(r.w as usize * r.h as usize * 4);
    for y in r.y..r.y + r.h {
        let from = y as usize * stride + r.x as usize * 4;
        match rgba.get(from..from + r.w as usize * 4) {
            Some(row) => out.extend_from_slice(row),
            None => out.resize(out.len() + r.w as usize * 4, 0),
        }
    }
    out
}

fn hash_of(at: Rect, rgba: &[u8]) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    at.hash(&mut h);
    rgba.hash(&mut h);
    h.finish()
}

/// A PNG's size and straight RGBA, as [`png_bytes`] writes one.
fn png_pixels(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    let mut reader = decoder.read_info().ok()?;
    let mut rgba = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut rgba).ok()?;
    rgba.truncate(info.buffer_size());
    (info.color_type == png::ColorType::Rgba && info.bit_depth == png::BitDepth::Eight).then_some((
        info.width,
        info.height,
        rgba,
    ))
}

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
            if let Shows::Group(inner) = &i.shows {
                i.shows = Shows::Group(still(inner));
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

/// Whether `track` has anything to draw: a drawing, a group or a video, shown.
fn draws(track: &procraft_engine::timeline::Track) -> bool {
    !track.hidden
        && track.clips.iter().any(|c| !c.disabled && !c.adjustment && !matches!(c.content, ClipContent::Audio(_)))
}

/// Whether what track `track` shows at `frame` is only a matte for the clip above it.
fn consumed(tl: &Timeline, track: TrackId, frame: u32) -> bool {
    tl.plan(frame).items.iter().any(|i| i.track == track && i.matte_only)
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

/// `name` as a file's: ASCII letters, digits, `_` and `-`, anything else `_`; `fallback` if
/// nothing of it is left.
fn plain_name(name: &str, fallback: &str) -> String {
    let mut out = String::new();
    for c in name.trim().chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            out.push(c);
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    let out = out.trim_matches('_');
    if out.is_empty() { fallback.to_owned() } else { out.to_owned() }
}

/// Whether a scene that takes sounds reads the sound file at `path` as it is (WAV and AIFF).
fn reads_as_it_is(path: &str) -> bool {
    matches!(media_extension(path).as_str(), "wav" | "wave" | "aif" | "aiff")
}

/// A path a plan may write: relative, its parts plain, and (outside a zip) a name alone.
fn safe_path(path: &str, zip: bool) -> bool {
    let parts: Vec<&str> = path.split('/').collect();
    let plain = |p: &&str| {
        !p.is_empty() && *p != "." && *p != ".." && p.chars().all(|c| !c.is_control() && c != '\\' && c != ':')
    };
    path.len() <= 512 && parts.iter().all(plain) && (zip || parts.len() == 1)
}

/// Where an export's run of pictures has got to, and what it keeps.
struct Run {
    export: ExportToRun,
    kept: HashMap<u32, Kept>,
}

impl Run {
    /// Picture `record` told: kept by the number the plugin gives it, if it keeps it, with what
    /// `drawn` paints.
    fn tell(&mut self, record: Record, drawn: Option<(Rect, Vec<u8>)>) -> Result<(), MobileError> {
        let kept = self.export.running.export_picture(&record.bytes()).map_err(stopped)?;
        if let (Some(number), Some((at, pixels))) = (kept, drawn) {
            let png = png_bytes(at.w, at.h, &pixels, &[])?;
            self.kept.insert(number, Kept { at, png });
        }
        Ok(())
    }
}

fn stopped(stop: crate::plugin_run::Stop) -> MobileError {
    file_error("the plugin", stop.text())
}

/// A picture's record and what it paints, from `rgba` (`w` × `h` straight): where, its pixels.
fn drawn_picture(kind: u32, frame: u32, w: u32, h: u32, rgba: &[u8]) -> (Record, Option<(Rect, Vec<u8>)>) {
    let mut record = Record::default();
    record.field(picture::KIND, &kind.to_le_bytes()).field(picture::FRAME, &frame.to_le_bytes());
    match painted(w, h, rgba) {
        Some(at) => {
            let pixels = crop(w, rgba, at);
            record.field(picture::PAINTED, &u32s(&[at.x, at.y, at.w, at.h]));
            record.field(picture::HASH, &hash_of(at, &pixels).to_le_bytes());
            (record, Some((at, pixels)))
        }
        None => {
            record.field(picture::PAINTED, &u32s(&[0, 0, 0, 0])).field(picture::HASH, &0u64.to_le_bytes());
            (record, None)
        }
    }
}

impl CanvasSession {
    /// The artwork as export `asks` draws it, with the timeline it draws from for `tracks` (the
    /// canvas put on one first if it has Animation Assist instead, as Timeline would).
    fn export_doc(&self, asks: ExportAsks) -> Result<(Document, String), MobileError> {
        let (mut doc, name) = {
            let st = self.shared.lock();
            (st.doc.clone(), st.name.clone())
        };
        if asks == ExportAsks::Tracks && doc.timeline.is_none() && !doc.start_timeline() {
            return Err(file_error("saving", "the canvas can't be put on a timeline"));
        }
        Ok((doc, name))
    }

    /// The sounds the timeline plays that the export of `key` takes, with the file each plays by
    /// its number and the path it is read from: as it is, or its WAV copy in `wav`. A sound muted,
    /// or whose file can't be found or read, is left out.
    fn export_sounds(&self, tl: &Timeline, wav: &HashMap<String, String>) -> (Vec<Record>, SoundFiles) {
        let st = self.shared.lock();
        let mut numbers: Vec<MediaId> = Vec::new();
        let mut files = SoundFiles::new();
        let mut records = Vec::new();
        for s in tl.sounds().into_iter().filter(|s| !s.muted) {
            let Some(media) = tl.media_of(s.media) else { continue };
            let Some(path) = st.media.find(&media.file_name()) else { continue };
            let path = path.to_string_lossy().into_owned();
            let readable = if reads_as_it_is(&path) { Some(path.clone()) } else { wav.get(&path).cloned() };
            let Some(readable) = readable.filter(|p| Path::new(p).is_file()) else { continue };
            let number = match numbers.iter().position(|m| *m == s.media) {
                Some(at) => at,
                None => {
                    numbers.push(s.media);
                    numbers.len() - 1
                }
            };
            let number = u32::try_from(number).unwrap_or(u32::MAX);
            let stem = media.name.rsplit_once('.').map_or(media.name.as_str(), |(stem, _)| stem).to_owned();
            let clip_name = tl.clip(s.clip).map(|c| c.name.clone()).filter(|n| !n.trim().is_empty());
            let mut record = Record::default();
            record
                .field(sound::NAME, clip_name.as_deref().unwrap_or(&media.name).as_bytes())
                .field(sound::FILE, &number.to_le_bytes())
                .field(sound::STEM, stem.as_bytes())
                .field(sound::EXTENSION, media_extension(&readable).as_bytes())
                .field(sound::AT, &s.at.to_le_bytes())
                .field(sound::FROM, &s.from.to_le_bytes())
                .field(sound::LENGTH, &s.length.to_le_bytes());
            records.push(record);
            files.insert(number, readable);
        }
        (records, files)
    }

    /// Each of the animation's frames drawn whole, told to the export.
    fn export_frames(&self, run: &mut Run, doc: &Document, transparent: bool, side: u32) -> Result<(), MobileError> {
        let units = shots(doc);
        let mut last: Option<Shot> = None;
        for (k, &unit) in units.iter().enumerate() {
            let frame = u32::try_from(k).unwrap_or(u32::MAX);
            if last.is_some_and(|before| same_shot(doc, before, unit)) {
                let mut record = Record::default();
                record
                    .field(picture::KIND, &FRAME.to_le_bytes())
                    .field(picture::FRAME, &frame.to_le_bytes())
                    .field(picture::HELD, &[1]);
                run.tell(record, None)?;
            } else {
                let (w, h, rgba) = self.render_shot(unit, transparent, side)?;
                let (record, drawn) = drawn_picture(FRAME, frame, w, h, &rgba);
                run.tell(record, drawn)?;
            }
            last = Some(unit);
            self.exported(0.9 * (k + 1) as f32 / units.len().max(1) as f32);
        }
        Ok(())
    }

    /// The background, then each drawing track alone, frame by frame, told to the export.
    fn export_tracks(
        &self,
        run: &mut Run,
        doc: &Document,
        tracks: &[TrackId],
        background: bool,
        side: u32,
    ) -> Result<(), MobileError> {
        let Some(tl) = doc.timeline.clone() else { return Ok(()) };
        let on_timeline = |d: &mut Document| {
            if d.timeline.is_none() {
                d.start_timeline();
            }
        };
        if background {
            let (w, h, rgba) = self.render_shot_of(Shot::At(0), false, side, |d| {
                on_timeline(d);
                if let Some(t) = d.timeline.as_mut() {
                    t.tracks.iter_mut().for_each(|t| t.hidden = true);
                }
            })?;
            let (record, drawn) = drawn_picture(BACKGROUND, 0, w, h, &rgba);
            run.tell(record, drawn)?;
        }
        // a still camera: a frame that shows the same as the one before is the same picture
        let still_camera = tl.camera.keys.len() <= 1;
        let jobs = (tracks.len() as u64 * u64::from(tl.duration)).max(1);
        let mut done = 0u64;
        for (index, &track) in tracks.iter().enumerate() {
            let index = u32::try_from(index).unwrap_or(u32::MAX);
            let mut last: Option<Vec<PlanItem>> = None;
            for frame in 0..tl.duration {
                done += 1;
                self.exported(0.9 * done as f32 / jobs as f32);
                let mut record = Record::default();
                record
                    .field(picture::KIND, &TRACK.to_le_bytes())
                    .field(picture::TRACK, &index.to_le_bytes())
                    .field(picture::FRAME, &frame.to_le_bytes());
                // a matte for the clip above shows only through it, drawn there
                if consumed(&tl, track, frame) {
                    record.field(picture::MATTE, &[1]);
                    run.tell(record, None)?;
                    last = None;
                    continue;
                }
                if let Some(blend) = blend_of(doc, &tl, track, frame) {
                    record.field(picture::BLEND, format!("{blend:?}").as_bytes());
                }
                let mut alone = tl.clone();
                procraft_engine::solo::solo(&mut alone, track, frame);
                let shows = still(&alone.plan(frame).items);
                if still_camera && last.as_ref() == Some(&shows) {
                    record.field(picture::HELD, &[1]);
                    run.tell(record, None)?;
                    continue;
                }
                let (w, h, rgba) = self.render_shot_of(Shot::At(frame), true, side, |d| {
                    on_timeline(d);
                    d.timeline = Some(alone);
                    // its layer's own mode is told, and drawn normally
                    if let Some(root) = root_at(&tl, track, frame) {
                        d.set_blend(root, BlendMode::Normal);
                    }
                })?;
                last = Some(shows);
                let (picture, drawn) = drawn_picture(TRACK, frame, w, h, &rgba);
                // (the kind and frame again, then where it is painted)
                let mut merged = Record::default();
                for (id, bytes) in fields(&record.bytes()).chain(fields(&picture.bytes())) {
                    merged.field(id, bytes);
                }
                run.tell(merged, drawn)?;
            }
        }
        Ok(())
    }

    /// The picture `image` (a record) put together from what `kept` holds, as a PNG.
    fn put_together(image: &[u8], kept: &HashMap<u32, Kept>) -> Result<Vec<u8>, MobileError> {
        let (mut width, mut height, mut places) = (0, 0, Vec::new());
        for (id, bytes) in fields(image) {
            match id {
                image::WIDTH => width = u32_of(bytes).unwrap_or(0),
                image::HEIGHT => height = u32_of(bytes).unwrap_or(0),
                image::PLACE => {
                    let v: Vec<u32> = bytes.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b)).collect();
                    if let [number, x, y, w, h, to_x, to_y] = v[..] {
                        places.push((number, Rect { x, y, w, h }, (to_x, to_y)));
                    }
                }
                _ => {}
            }
        }
        let pixels = u64::from(width) * u64::from(height);
        if width == 0 || height == 0 || width > MOST_SIDE || height > MOST_SIDE || pixels > MOST_PIXELS {
            return Err(file_error("saving", format!("a picture {width} × {height} pixels is too large to make")));
        }
        let mut rgba = vec![0u8; pixels as usize * 4];
        let mut opened: HashMap<u32, (u32, u32, Vec<u8>)> = HashMap::new();
        for (number, part, (to_x, to_y)) in places {
            let Some(k) = kept.get(&number) else { continue };
            let (pw, _, source) = match opened.entry(number) {
                std::collections::hash_map::Entry::Occupied(e) => &*e.into_mut(),
                std::collections::hash_map::Entry::Vacant(e) => {
                    let Some(decoded) = png_pixels(&k.png) else { continue };
                    &*e.insert(decoded)
                }
            };
            // each row of the part that the picture has painted (the rest is clear)
            for row in 0..part.h {
                let (fy, ty) = (part.y + row, u64::from(to_y) + u64::from(row));
                if fy < k.at.y || fy >= k.at.y + k.at.h || ty >= u64::from(height) {
                    continue;
                }
                let from_x = part.x.max(k.at.x);
                let to_end = (part.x + part.w).min(k.at.x + k.at.w);
                if from_x >= to_end {
                    continue;
                }
                let dest_x = u64::from(to_x) + u64::from(from_x - part.x);
                let run_w = u64::from(to_end - from_x).min(u64::from(width).saturating_sub(dest_x));
                if run_w == 0 {
                    continue;
                }
                let src = ((fy - k.at.y) as usize * *pw as usize + (from_x - k.at.x) as usize) * 4;
                let dst = (ty as usize * width as usize + dest_x as usize) * 4;
                let n = run_w as usize * 4;
                if let (Some(s), Some(d)) = (source.get(src..src + n), rgba.get_mut(dst..dst + n)) {
                    d.copy_from_slice(s);
                }
            }
        }
        png_bytes(width, height, &rgba, &[])
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The sound files the timeline plays that export `key` of `plugins` takes but can't read as
    /// they are (it reads WAV and AIFF), to be made WAV for [`Self::plugin_share`].
    pub fn plugin_share_sounds(&self, plugins: Arc<Plugins>, key: String) -> Vec<String> {
        if !plugins.shares_sounds(&key) {
            return Vec::new();
        }
        let st = self.shared.lock();
        let Some(tl) = st.doc.timeline.as_ref() else { return Vec::new() };
        let mut out: Vec<String> = Vec::new();
        for s in tl.sounds().iter().filter(|s| !s.muted) {
            let Some(path) = tl.media_of(s.media).and_then(|m| st.media.find(&m.file_name())) else { continue };
            let path = path.to_string_lossy().into_owned();
            if !reads_as_it_is(&path) && !out.contains(&path) {
                out.push(path);
            }
        }
        out
    }

    /// The files export `key` of `plugins` (as [`Plugins::shares`] lists it) makes of the
    /// animation, written into the folder `dir`: zipped, as `<the artwork's name>.zip`, for one
    /// that says so. `wav` gives a WAV copy of each sound file it can't read as it is (see
    /// [`Self::plugin_share_sounds`]). Slow: call it off the main thread; its progress is
    /// [`Self::export_progress`].
    pub fn plugin_share(
        &self,
        plugins: Arc<Plugins>,
        key: String,
        dir: String,
        wav: HashMap<String, String>,
    ) -> Result<PluginShared, MobileError> {
        let export = plugins.export_to_run(&key).map_err(|why| file_error("the plugin", why))?;
        let asks = export.exporter.asks;
        let number = |at: Option<usize>| at.and_then(|at| export.values.get(at)).copied().unwrap_or(0.0);
        let side = number(export.exporter.size).max(0.0) as u32;
        let transparent = number(export.exporter.transparent) >= 0.5;
        let takes_sounds = export.exporter.sounds;
        let zip = export.exporter.zip;
        let (doc, name) = self.export_doc(asks)?;
        self.exported(0.0);

        // what it begins on
        let (w, h) = {
            let (dw, dh) = (doc.width(), doc.height());
            let longest = if side == 0 { dw.max(dh) } else { side.min(dw.max(dh)) };
            let scale = (longest as f32 / dw.max(dh).max(1) as f32).min(1.0);
            (((dw as f32 * scale).round() as u32).max(1), ((dh as f32 * scale).round() as u32).max(1))
        };
        let tracks: Vec<TrackId> = match (&doc.timeline, asks) {
            (Some(tl), ExportAsks::Tracks) => tl.tracks.iter().filter(|t| draws(t)).map(|t| t.id).collect(),
            _ => Vec::new(),
        };
        let background = asks == ExportAsks::Tracks && doc.background_visible;
        let frames = match asks {
            ExportAsks::Frames => shots(&doc).len() as u32,
            ExportAsks::Tracks => doc.timeline.as_ref().map_or(0, |t| t.duration),
        };
        let mut artwork_record = Record::default();
        artwork_record
            .field(artwork::WIDTH, &w.to_le_bytes())
            .field(artwork::HEIGHT, &h.to_le_bytes())
            .field(artwork::FPS, &fps_of(&doc).to_le_bytes())
            .field(artwork::FRAMES, &frames.to_le_bytes())
            .field(artwork::NAME, name.as_bytes())
            .field(artwork::BACKGROUND, &[u8::from(background)]);
        if let Some(tl) = doc.timeline.as_ref() {
            for id in &tracks {
                let mut t = Record::default();
                t.field(track::NAME, tl.track(*id).map(|t| t.name.as_str()).unwrap_or_default().as_bytes());
                artwork_record.field(artwork::TRACK, &t.bytes());
            }
        }
        let mut sound_files = SoundFiles::new();
        if takes_sounds && let Some(tl) = doc.timeline.as_ref() {
            let (records, files) = self.export_sounds(tl, &wav);
            for r in records {
                artwork_record.field(artwork::SOUND, &r.bytes());
            }
            sound_files = files;
        }
        let mut run = Run { export, kept: HashMap::new() };
        let index = run.export.index;
        if !run.export.running.export_begin(index, &artwork_record.bytes()).map_err(stopped)? {
            return Err(file_error("the plugin", NOTHING_MADE));
        }

        // the pictures, then its plan
        match asks {
            ExportAsks::Frames => self.export_frames(&mut run, &doc, transparent, side)?,
            ExportAsks::Tracks => self.export_tracks(&mut run, &doc, &tracks, background, side)?,
        }
        let plan_bytes = run.export.running.export_finish().map_err(stopped)?;
        let plan_bytes = plan_bytes.ok_or_else(|| file_error("the plugin", NOTHING_MADE))?;
        let mut said = None;
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        let listed: Vec<&[u8]> = fields(&plan_bytes)
            .filter_map(|(id, bytes)| match id {
                plan::FILE => Some(bytes),
                plan::SAID => {
                    said = std::str::from_utf8(bytes).ok().map(str::to_owned).filter(|s| !s.trim().is_empty());
                    None
                }
                _ => None,
            })
            .collect();
        if listed.is_empty() || listed.len() > MOST_FILES {
            return Err(file_error("the plugin", NOTHING_MADE));
        }
        for (k, entry) in listed.iter().enumerate() {
            let mut path = None;
            let mut bytes = None;
            for (id, value) in fields(entry) {
                match id {
                    file::PATH => path = std::str::from_utf8(value).ok().map(str::to_owned),
                    file::BYTES => bytes = Some(value.to_vec()),
                    file::IMAGE => bytes = Some(Self::put_together(value, &run.kept)?),
                    file::SOUND => {
                        let read = u32_of(value).and_then(|n| sound_files.get(&n)).and_then(|p| fs::read(p).ok());
                        bytes = read;
                    }
                    _ => {}
                }
            }
            let (Some(path), Some(bytes)) = (path, bytes) else { continue };
            if !safe_path(&path, zip) || files.iter().any(|(p, _)| p.eq_ignore_ascii_case(&path)) {
                return Err(file_error("the plugin", format!("it named a file it can't: {path}")));
            }
            files.push((path, bytes));
            self.exported(0.9 + 0.1 * (k + 1) as f32 / listed.len() as f32);
        }

        // written
        let dir = PathBuf::from(dir);
        fs::create_dir_all(&dir).map_err(|e| file_error("saving", e))?;
        let mut written = Vec::new();
        if zip {
            let mut zipped = ZipWriter::new();
            for (path, bytes) in &files {
                // a PNG is squeezed already
                let squeezed = path.to_ascii_lowercase().ends_with(".png");
                zipped.add_ready(path, Ready::of(bytes, !squeezed));
            }
            let at = dir.join(format!("{}.zip", plain_name(&name, "Animation")));
            let zipped = zipped.finish();
            write_atomically(&at, |f| fs::write(f, &zipped).map_err(|e| file_error("saving", e)))?;
            written.push(at.to_string_lossy().into_owned());
        } else {
            for (path, bytes) in &files {
                let at = dir.join(path);
                write_atomically(&at, |f| fs::write(f, bytes).map_err(|e| file_error("saving", e)))?;
                written.push(at.to_string_lossy().into_owned());
            }
        }
        self.exported(1.0);
        Ok(PluginShared { files: written, said })
    }
}

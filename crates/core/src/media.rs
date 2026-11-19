//! Sounds and videos on the timeline as the apps use them (see the engine's `media`): bringing a
//! file in, its clips' timing and levels, the sounds to play, videos' pictures, and the animatic
//! detector's timing track.
//!
//! The core never decodes media. The app reads a file's length, size and waveform when it brings
//! it in, plays the sounds ([`CanvasSession::timeline_sounds`]), and hands over videos' pictures
//! through a [`VideoFrames`] it gives the session.
//!
//! Files are kept by the hash of their bytes as `media/<id>.<ext>` in the artwork's folder, with a
//! sound's waveform beside it (`<id>.peaks`). One brought in waits in a folder of the app's
//! temporary files until the artwork is saved; a save copies in those the document plays and
//! moves out those it no longer does (an undo can bring them back), so a file is always in one
//! of the places the store looks.

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};

use procraft_engine::animatic;
use procraft_engine::document::Document;
use procraft_engine::media::{MEDIA_FOLDER, media_extension, media_id_of};
use procraft_engine::timeline::{ClipContent, ClipId, Media, MediaId, MediaKind, Timeline, TrackId};

use crate::layers::ThumbnailImage;
use crate::project::file_error;
use crate::{CanvasSession, MobileError};

/// Where the app's videos' pictures come from: the platform's decoder.
#[uniffi::export(with_foreign)]
pub trait VideoFrames: Send + Sync {
    /// The picture of the video file at `path` `seconds` into it, scaled to fit `max_side`
    /// pixels (straight RGBA8, rows top to bottom); none if it can't be read.
    fn picture(&self, path: String, seconds: f64, max_side: u32) -> Option<ThumbnailImage>;
}

/// Whether media is a sound or a video.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TimelineMediaKind {
    Audio,
    Video,
}

impl TimelineMediaKind {
    fn of(k: MediaKind) -> TimelineMediaKind {
        match k {
            MediaKind::Audio => TimelineMediaKind::Audio,
            MediaKind::Video => TimelineMediaKind::Video,
        }
    }

    fn kind(self) -> MediaKind {
        match self {
            TimelineMediaKind::Audio => MediaKind::Audio,
            TimelineMediaKind::Video => MediaKind::Video,
        }
    }
}

/// A file brought in, as the app read it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MediaImport {
    /// Where the file is now (it is copied).
    pub path: String,
    /// What to call it (the file's name, say).
    pub name: String,
    pub kind: TimelineMediaKind,
    /// Its length, seconds.
    pub seconds: f64,
    /// A video's picture size, pixels (0 for a sound).
    pub width: u32,
    pub height: u32,
    /// A sound's waveform: the loudest sample in each of [`PEAKS_PER_SECOND`]ths of a second, 0..1.
    pub peaks: Vec<f32>,
}

/// How many waveform peaks a second of sound has.
pub const PEAKS_PER_SECOND: u32 = 50;

#[uniffi::export]
pub fn media_peaks_per_second() -> u32 {
    PEAKS_PER_SECOND
}

/// A sound or video clip as the apps show and edit it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineMediaClip {
    pub clip: u64,
    pub media: u64,
    pub kind: TimelineMediaKind,
    pub name: String,
    /// Where its file is, if it can be found.
    pub path: Option<String>,
    /// Its media's length, seconds.
    pub media_seconds: f64,
    /// A video's picture size, pixels (0 for a sound).
    pub width: u32,
    pub height: u32,
    /// Where in its media it starts, seconds.
    pub trim_start: f64,
    /// A sound's start after its first frame, seconds (under a frame).
    pub offset: f64,
    pub volume: f32,
    pub muted: bool,
    /// A video's place on the canvas: left, top, width, height.
    pub rect: Vec<f32>,
    /// A sound's level at the playhead: keyed, or its volume.
    pub level: f32,
    /// The timeline frames a sound's level is keyed on (none: it plays at its volume).
    pub volume_keys: Vec<u32>,
    /// Its level has a key at the playhead.
    pub keyed_here: bool,
}

/// A sound to play, in timeline time.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineSound {
    pub clip: u64,
    /// Its file.
    pub path: String,
    /// When it starts, seconds from the first frame.
    pub at: f64,
    /// Where in its file it starts, seconds.
    pub from: f64,
    /// How long it plays, seconds.
    pub length: f64,
    /// How loud, 0..1 (0 when muted): its level, or at the playhead when it is keyed.
    pub gain: f32,
    /// Keyed: its level on each timeline frame it plays through, from frame `levels_from` (the
    /// frame it starts in) on; empty when it isn't (it plays at `gain` throughout).
    pub levels: Vec<f32>,
    pub levels_from: u32,
}

/// A hold the animatic detector found: a drawing's first frame in the video clip, and how many
/// frames it is held.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct AnimaticHold {
    pub start: u32,
    pub length: u32,
}

/// The places media files are looked for, and how videos' pictures are read: shared by a
/// session's state and its canvas (which asks for pictures while it renders, the state held:
/// so this is only ever locked after the state).
#[derive(Clone, Default)]
pub(crate) struct MediaStore(Arc<Mutex<Places>>);

#[derive(Default)]
struct Places {
    /// The media folders of the artwork's folders it was opened from or saved to.
    folders: Vec<PathBuf>,
    /// Each media's file name, as the document has it.
    names: HashMap<MediaId, String>,
    frames: Option<Arc<dyn VideoFrames>>,
    /// The longest side pictures are read at (the canvas's).
    side: u32,
}

/// Where media brought in waits until a save copies it into an artwork's folder.
fn waiting_folder() -> PathBuf {
    std::env::temp_dir().join("procraft-media")
}

impl MediaStore {
    fn places(&self) -> std::sync::MutexGuard<'_, Places> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Looks in `dir`'s media folder too (an artwork's folder).
    pub(crate) fn look_in(&self, dir: &Path) {
        let folder = dir.join(MEDIA_FOLDER);
        let mut p = self.places();
        if !p.folders.contains(&folder) {
            p.folders.push(folder);
        }
    }

    /// Takes the file names of the document's media.
    pub(crate) fn know(&self, doc: &Document) {
        let mut p = self.places();
        p.side = doc.width().max(doc.height());
        if let Some(tl) = &doc.timeline {
            for m in &tl.media {
                p.names.insert(m.id, m.file_name());
            }
        }
    }

    /// Where file `name` is: an artwork's media folder, or the waiting folder.
    pub(crate) fn find(&self, name: &str) -> Option<PathBuf> {
        let p = self.places();
        p.folders.iter().cloned().chain([waiting_folder()]).map(|f| f.join(name)).find(|path| path.exists())
    }

    fn path_of(&self, id: MediaId) -> Option<PathBuf> {
        let name = self.places().names.get(&id).cloned()?;
        self.find(&name)
    }

    fn set_frames(&self, frames: Option<Arc<dyn VideoFrames>>) {
        self.places().frames = frames;
    }
}

impl procraft_gpu::VideoSource for MediaStore {
    fn picture(&self, media: MediaId, seconds: f64) -> Option<procraft_gpu::VideoPicture> {
        let path = self.path_of(media)?;
        let (frames, side) = {
            let p = self.places();
            (p.frames.clone()?, p.side)
        };
        let image = frames.picture(path.to_string_lossy().into_owned(), seconds, side)?;
        Some(procraft_gpu::VideoPicture { width: image.width, height: image.height, rgba: image.rgba })
    }
}

/// What a save does with media: the files the document plays, to copy into the folder if they
/// aren't there.
pub(crate) struct MediaToSave {
    store: MediaStore,
    /// The file names of the media the document plays, and their waveforms'.
    keep: Vec<String>,
}

impl MediaToSave {
    pub(crate) fn of(store: &MediaStore, doc: &Document) -> MediaToSave {
        store.know(doc);
        let keep = doc.timeline.as_ref().map_or_else(Vec::new, |tl| {
            let used = tl.used_media();
            tl.media.iter().filter(|m| used.contains(&m.id)).flat_map(|m| [m.file_name(), m.peaks_name()]).collect()
        });
        MediaToSave { store: store.clone(), keep }
    }

    /// Copies into `dir`'s media folder the files it doesn't have yet (staged beside their
    /// names, put in place with the rest of the save).
    pub(crate) fn write(&self, dir: &Path, staged: &mut Vec<(PathBuf, PathBuf)>) -> Result<(), MobileError> {
        let folder = dir.join(MEDIA_FOLDER);
        if !self.keep.is_empty() {
            fs::create_dir_all(&folder).map_err(|e| file_error("making the folder", e))?;
        }
        for name in &self.keep {
            let to = folder.join(name);
            if to.exists() {
                continue;
            }
            // (a sound brought in without a waveform has no peaks file)
            let Some(from) = self.store.find(name) else { continue };
            let tmp = folder.join(format!("{name}.saving"));
            fs::copy(&from, &tmp).map_err(|e| file_error("writing a sound or video", e))?;
            staged.push((tmp, to));
        }
        Ok(())
    }

    /// Once the document is in place: the files in `dir`'s media folder it no longer plays move
    /// to the waiting folder (an undo may want them again), and the folder is looked in.
    pub(crate) fn tidy(&self, dir: &Path) {
        let folder = dir.join(MEDIA_FOLDER);
        if let Ok(entries) = fs::read_dir(&folder) {
            let waiting = waiting_folder();
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if self.keep.contains(&name) {
                    continue;
                }
                let to = waiting.join(&name);
                let moved = fs::create_dir_all(&waiting).and_then(|()| fs::rename(entry.path(), &to));
                if moved.is_err() {
                    let _ = fs::remove_file(entry.path());
                }
            }
        }
        self.store.look_in(dir);
    }
}

/// `path`'s file copied into the waiting folder as media `m` (unless a file of the same bytes is
/// there already), with its waveform `peaks` beside it.
fn bring_in(path: &Path, m: &Media, peaks: &[f32]) -> Result<(), MobileError> {
    let waiting = waiting_folder();
    fs::create_dir_all(&waiting).map_err(|e| file_error("bringing in media", e))?;
    let to = waiting.join(m.file_name());
    if !to.exists() {
        let tmp = waiting.join(format!("{}.copying", m.file_name()));
        fs::copy(path, &tmp).map_err(|e| file_error("bringing in media", e))?;
        fs::rename(&tmp, &to).map_err(|e| file_error("bringing in media", e))?;
    }
    if !peaks.is_empty() {
        let bytes: Vec<u8> = peaks.iter().flat_map(|p| p.to_le_bytes()).collect();
        fs::write(waiting.join(m.peaks_name()), bytes).map_err(|e| file_error("bringing in media", e))?;
    }
    Ok(())
}

/// Media `m`'s file at `file`, and its waveform at `peaks`, copied into the waiting folder (a
/// sound or video pasted from another canvas), unless it is there already.
pub(crate) fn bring_along(m: &Media, file: &Path, peaks: Option<&Path>) {
    let peaks: Vec<f32> = peaks
        .and_then(|p| fs::read(p).ok())
        .map(|b| b.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect())
        .unwrap_or_default();
    if let Err(e) = bring_in(file, m, &peaks) {
        log::warn!("bringing along {}: {e:?}", m.name);
    }
}

impl crate::State {
    /// The canvas gets its videos' pictures from the store, once there is a canvas and a video.
    pub(crate) fn attach_media(&mut self) {
        let videos = self.doc.timeline.as_ref().is_some_and(|tl| tl.media.iter().any(|m| m.kind == MediaKind::Video));
        if videos
            && let Some(c) = self.canvas.as_mut()
            && !c.has_video_source()
        {
            self.media.know(&self.doc);
            c.set_video_source(Some(Arc::new(self.media.clone())));
        }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Where videos' pictures come from (the app's decoder).
    pub fn set_video_frames(&self, frames: Arc<dyn VideoFrames>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.media.set_frames(Some(frames));
        st.media.know(&st.doc);
        if let Some(c) = st.canvas.as_mut() {
            c.set_video_source(Some(Arc::new(st.media.clone())));
        }
        self.shared.frame(st);
    }

    /// Brings in a sound or video (copied, by the hash of its bytes) and puts it on a new track at
    /// the playhead, as an undo step: a sound below the other tracks, a video under the drawings,
    /// fitted to the canvas. The canvas is put on a timeline first if it isn't on one. Returns
    /// the new clip, or 0.
    pub fn import_timeline_media(&self, import: MediaImport) -> Result<u64, MobileError> {
        let path = PathBuf::from(&import.path);
        let file = fs::File::open(&path).map_err(|e| file_error("reading the file", e))?;
        let id = media_id_of(std::io::BufReader::new(file)).map_err(|e| file_error("reading the file", e))?;
        if !import.seconds.is_finite() || import.seconds <= 0.0 {
            return Err(file_error("reading the file", "it has no length"));
        }
        let name = import.name.rsplit_once('.').map_or(import.name.as_str(), |(stem, _)| stem).trim().to_owned();
        let media = Media {
            id,
            name: if name.is_empty() { "Media".to_owned() } else { name },
            kind: import.kind.kind(),
            seconds: import.seconds,
            extension: media_extension(&import.path),
            size: [import.width, import.height],
        };
        bring_in(&path, &media, &import.peaks)?;
        let frame = self.timeline_playhead();
        let step = match import.kind {
            TimelineMediaKind::Audio => "Add Sound",
            TimelineMediaKind::Video => "Add Video",
        };
        let made = self.change(step, |d| {
            if d.timeline.is_none() && !d.start_timeline() {
                return None;
            }
            let canvas = (d.width(), d.height());
            let tl = d.timeline.as_mut()?;
            tl.add_media(media.clone());
            match media.kind {
                MediaKind::Audio => tl.add_sound(id, frame),
                MediaKind::Video => tl.add_video(id, frame, canvas),
            }
        });
        let Some((track, clip)) = made else { return Ok(0) };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.media.know(&st.doc);
        if media.kind == MediaKind::Video {
            st.timeline_track = Some(track);
        }
        st.attach_media();
        self.shared.frame(st);
        Ok(clip.0)
    }

    /// Sound or video clip `id` as the apps show it.
    pub fn timeline_media_clip(&self, id: u64) -> Option<TimelineMediaClip> {
        let st = self.shared.lock();
        let tl = st.doc.timeline.as_ref()?;
        let clip = tl.clip(ClipId(id))?;
        let (media, trim_start, offset, volume, muted, rect) = match &clip.content {
            ClipContent::Audio(a) => (a.media, a.trim_start, a.offset, a.volume, a.muted, Vec::new()),
            ClipContent::Video(v) => (v.media, v.trim_start, 0.0, 1.0, false, v.rect.to_vec()),
            ClipContent::Drawing(_) | ClipContent::Group(_) => return None,
        };
        let m = tl.media_of(media)?;
        let [width, height] = m.size;
        let origin = i64::from(tl.origin(ClipId(id))?);
        let local = i64::from(st.canvas.as_ref().map_or(0, |c| c.timeline_frame())) - origin;
        let (level, volume_keys, keyed_here) = match &clip.content {
            ClipContent::Audio(a) => (
                a.volume_keys.value_at(local as f32).map_or(volume, |v| v.clamp(0.0, 1.0)),
                a.volume_keys.keys.iter().filter_map(|k| u32::try_from(origin + i64::from(k.frame)).ok()).collect(),
                a.volume_keys.keys.iter().any(|k| i64::from(k.frame) == local),
            ),
            _ => (1.0, Vec::new(), false),
        };
        Some(TimelineMediaClip {
            clip: id,
            media: media.0,
            kind: TimelineMediaKind::of(m.kind),
            name: m.name.clone(),
            path: st.media.find(&m.file_name()).map(|p| p.to_string_lossy().into_owned()),
            media_seconds: m.seconds,
            width,
            height,
            trim_start,
            offset,
            volume,
            muted,
            rect,
            level,
            volume_keys,
            keyed_here,
        })
    }

    /// Media `id`'s waveform (see [`MediaImport::peaks`]); empty if it has none.
    pub fn timeline_media_peaks(&self, id: u64) -> Vec<f32> {
        let name = {
            let st = self.shared.lock();
            let Some(m) = st.doc.timeline.as_ref().and_then(|tl| tl.media_of(MediaId(id))) else { return Vec::new() };
            m.peaks_name()
        };
        let path = self.shared.lock().media.find(&name);
        let bytes = path.and_then(|p| fs::read(p).ok()).unwrap_or_default();
        bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
    }

    /// The sounds to play: every one whose file can be found, in time order, its level at timeline
    /// frame `frame`.
    pub fn timeline_sounds(&self, frame: u32) -> Vec<TimelineSound> {
        let st = self.shared.lock();
        let Some(tl) = st.doc.timeline.as_ref() else { return Vec::new() };
        tl.sounds()
            .into_iter()
            .filter_map(|s| {
                let m = tl.media_of(s.media)?;
                let path = st.media.find(&m.file_name())?;
                let levels = s.levels(tl.fps);
                Some(TimelineSound {
                    clip: s.clip.0,
                    path: path.to_string_lossy().into_owned(),
                    at: s.at,
                    from: s.from,
                    length: s.length,
                    gain: s.gain_at(frame),
                    levels: levels.as_ref().map_or_else(Vec::new, |(_, l)| l.clone()),
                    levels_from: levels.map_or(0, |(f, _)| f),
                })
            })
            .collect()
    }

    /// Sound or video clip `id` starting `trim_start` seconds into its media where it is (a slip):
    /// shown at once, one "Slip" undo step when `done`.
    pub fn slip_timeline_media(&self, id: u64, trim_start: f64, done: bool) -> bool {
        let changed = self
            .change_live("Slip", done, |d| d.timeline.as_mut().is_some_and(|tl| tl.slip_media(ClipId(id), trim_start)));
        if changed {
            self.redraw_media();
        }
        changed
    }

    /// Sound clip `id`'s volume (0..1), whether it is muted, and its start `offset` seconds after
    /// its first frame (under a frame): one "Sound" undo step when `done`.
    pub fn set_timeline_sound(&self, id: u64, volume: f32, muted: bool, offset: f64, done: bool) -> bool {
        self.change_live("Sound", done, |d| {
            d.timeline.as_mut().is_some_and(|tl| tl.set_sound(ClipId(id), volume, muted, offset))
        })
    }

    /// Video clip `id` drawn into `rect` (left, top, width, height on the canvas): one "Place
    /// Video" undo step when `done`.
    pub fn set_timeline_video_rect(&self, id: u64, rect: Vec<f32>, done: bool) -> bool {
        let Ok(rect) = <[f32; 4]>::try_from(rect) else { return false };
        let changed = self.change_live("Place Video", done, |d| {
            d.timeline.as_mut().is_some_and(|tl| tl.set_video_rect(ClipId(id), rect))
        });
        if changed {
            self.redraw_media();
        }
        changed
    }

    /// The time in its video video clip `id` shows on each of its frames: what the animatic
    /// detector samples.
    pub fn timeline_video_times(&self, id: u64) -> Vec<f64> {
        self.shared.lock().doc.timeline.as_ref().map_or_else(Vec::new, |tl| tl.video_times(ClipId(id)))
    }

    /// A track of new, empty drawings just above video clip `id`'s track, one for each of `holds`
    /// (frames of the clip) held as long, as an undo step: the video's timing to draw over.
    /// Returns the track, or 0.
    pub fn make_timing_track(&self, id: u64, holds: Vec<AnimaticHold>) -> u64 {
        if holds.is_empty() {
            return 0;
        }
        let made = self.change("Make Timing Track", |d| {
            if !d.has_room(holds.len()) {
                return None;
            }
            let tl = d.timeline.as_ref()?;
            let clip = tl.clip(ClipId(id)).filter(|c| matches!(c.content, ClipContent::Video(_)))?;
            let (start, duration, name) = (clip.start, clip.duration, clip.name.clone());
            let video_track = tl.track_of_clip(ClipId(id))?;
            let mut layers = Vec::new();
            for n in 0..holds.len() {
                let layer = d.add_layer();
                d.rename(layer, &format!("{name} {}", n + 1));
                layers.push(layer);
            }
            let tl = d.timeline.as_mut()?;
            timing_track(tl, video_track, &format!("{name} Timing"), &name, start, duration, &holds, &layers)
        });
        let Some(track) = made else { return 0 };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.timeline_track = Some(track);
        st.follow_playhead();
        self.shared.frame(st);
        track.0
    }
}

/// A track named `name` just above `below` (on its level) holding a drawing of each of `layers`
/// (named `drawing` 1, 2...) for each of `holds` (frames from `start`, cut off at `duration`).
/// Returns the track.
fn timing_track(
    tl: &mut Timeline,
    below: TrackId,
    name: &str,
    drawing: &str,
    start: u32,
    duration: u32,
    holds: &[AnimaticHold],
    layers: &[procraft_engine::document::LayerId],
) -> Option<TrackId> {
    let track = tl.add_track(name);
    // just above the video's track
    let from = tl.tracks.iter().position(|t| t.id == track)?;
    let to = tl.tracks.iter().position(|t| t.id == below)? + 1;
    let moved = tl.tracks.remove(from);
    tl.tracks.insert(to.min(tl.tracks.len()), moved);
    for (n, (hold, layer)) in holds.iter().zip(layers).enumerate() {
        if hold.start >= duration || hold.length == 0 {
            continue;
        }
        let at = start + hold.start;
        let clip = tl.add_drawing(track, at, *layer, &format!("{drawing} {}", n + 1))?;
        tl.set_duration(clip, hold.length.min(duration - hold.start), false);
    }
    Some(track)
}

impl CanvasSession {
    /// The playhead's timeline frame (0 off a timeline).
    fn timeline_playhead(&self) -> u32 {
        self.shared.lock().canvas.as_ref().map_or(0, |c| c.timeline_frame())
    }

    fn redraw_media(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
    }
}

/// The animatic detector's change scores for a video clip's frames, each a `width` × `height`
/// grey picture (none where a frame couldn't be read): see the engine's `animatic`.
#[uniffi::export]
pub fn animatic_scores(frames: Vec<Option<Vec<u8>>>, width: u32, height: u32) -> Vec<f32> {
    let frames: Vec<Option<animatic::Luma>> =
        frames.into_iter().map(|f| f.map(|luma| animatic::Luma { width, height, luma })).collect();
    animatic::change_scores(&frames)
}

/// The holds `scores` make at `sensitivity` (0, cuts only, to 1, every change), none shorter
/// than `minimum_hold` frames.
#[uniffi::export]
pub fn animatic_holds(scores: Vec<f32>, sensitivity: f32, minimum_hold: u32) -> Vec<AnimaticHold> {
    let hold = minimum_hold.clamp(1, animatic::MAX_HOLD);
    animatic::segments(&scores, animatic::threshold(sensitivity), hold)
        .into_iter()
        .map(|r| AnimaticHold { start: r.start, length: r.end - r.start })
        .collect()
}

/// The score a change must reach at `sensitivity`, for drawing it over the scores.
#[uniffi::export]
pub fn animatic_threshold(sensitivity: f32) -> f32 {
    animatic::threshold(sensitivity)
}

/// The longest side of the grey pictures the detector compares.
#[uniffi::export]
pub fn animatic_thumbnail_side() -> u32 {
    animatic::THUMBNAIL_SIDE
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;

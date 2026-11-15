//! Sounds and videos on the timeline: the media a canvas carries, its audio and video clips, and
//! when each plays (see `docs/design/timeline.md`).
//!
//! The engine keeps only what is needed to place media in time. Decoding the files, playing the
//! sound and reading a video's pictures belong to the platform; the files themselves are kept in
//! the artwork's folder as `media/<file_name>` (see the core's `media`).
//!
//! A clip's start and length are frames, like every other clip's. Where it starts in its media
//! (`trim_start`) and a sound's sub-frame `offset` are seconds, so a sound stays in sync to the
//! sample however the frames fall.

use std::collections::HashSet;
use std::io::Read;

use crate::document::BlendMode;
use crate::timeline::{
    AudioClip, Channel, Clip, ClipContent, ClipId, ClipKeys, Group, MaskMode, Media, MediaId, MediaKind, Timeline,
    Track, TrackId, VideoClip, level_of_clip,
};

/// The loudest a sound plays (its own level).
pub const MAX_VOLUME: f32 = 1.0;
/// Media's folder in an artwork's folder.
pub const MEDIA_FOLDER: &str = "media";

/// Hashes a media file's bytes into its id as they are read (FNV-1a over 8-byte words, as
/// layers' pixels are hashed).
#[derive(Clone, Copy, Debug)]
pub struct MediaHasher {
    hash: u64,
    /// Bytes read short of a whole word.
    rest: [u8; 8],
    kept: usize,
}

impl Default for MediaHasher {
    fn default() -> MediaHasher {
        MediaHasher { hash: 0xCBF2_9CE4_8422_2325, rest: [0; 8], kept: 0 }
    }
}

const PRIME: u64 = 0x0000_0100_0000_01B3;

impl MediaHasher {
    pub fn update(&mut self, mut bytes: &[u8]) {
        // finish a word begun before
        while self.kept > 0 {
            let Some((first, more)) = bytes.split_first() else { return };
            if let Some(slot) = self.rest.get_mut(self.kept) {
                *slot = *first;
            }
            self.kept += 1;
            bytes = more;
            if self.kept == 8 {
                self.word(self.rest);
                self.kept = 0;
            }
        }
        let (words, rest) = bytes.as_chunks::<8>();
        for w in words {
            self.word(*w);
        }
        for b in rest {
            if let Some(slot) = self.rest.get_mut(self.kept) {
                *slot = *b;
            }
            self.kept += 1;
        }
    }

    fn word(&mut self, w: [u8; 8]) {
        self.hash = (self.hash ^ u64::from_le_bytes(w)).wrapping_mul(PRIME).rotate_left(29);
    }

    pub fn finish(&self) -> MediaId {
        let mut h = self.hash;
        for b in self.rest.iter().take(self.kept) {
            h = (h ^ u64::from(*b)).wrapping_mul(PRIME);
        }
        MediaId(h)
    }
}

/// The id of the media file `from` reads.
pub fn media_id_of(mut from: impl Read) -> std::io::Result<MediaId> {
    let mut hasher = MediaHasher::default();
    let mut buf = vec![0_u8; 1 << 16];
    loop {
        let n = from.read(&mut buf)?;
        if n == 0 {
            return Ok(hasher.finish());
        }
        hasher.update(buf.get(..n).unwrap_or_default());
    }
}

/// A file name's extension as media keeps it: lower case letters and digits, at most 8; `bin`
/// when it has none.
pub fn media_extension(name: &str) -> String {
    let ext = name.rsplit_once('.').map_or("", |(_, e)| e);
    let ext: String = ext.chars().filter(char::is_ascii_alphanumeric).take(8).collect::<String>().to_lowercase();
    if ext.is_empty() { "bin".to_owned() } else { ext }
}

impl Media {
    /// Its file's name in the artwork's media folder.
    pub fn file_name(&self) -> String {
        format!("{:016x}.{}", self.id.0, media_extension(&format!(".{}", self.extension)))
    }

    /// The name of its waveform's file beside it (a sound's peaks, little-endian f32s).
    pub fn peaks_name(&self) -> String {
        format!("{:016x}.peaks", self.id.0)
    }
}

/// A sound as it plays: when on the timeline, from where in its media, for how long, how loud.
#[derive(Clone, Debug, PartialEq)]
pub struct Sound {
    pub clip: ClipId,
    pub media: MediaId,
    /// When it starts, seconds from the timeline's first frame.
    pub at: f64,
    /// Where in its media it starts, seconds.
    pub from: f64,
    /// How long it plays, seconds.
    pub length: f64,
    /// Its timeline frame at `at` less its own first frame: what its volume keys count from.
    pub key_origin: i64,
    pub volume: f32,
    /// Muted, or on a hidden track.
    pub muted: bool,
    pub volume_keys: Channel<f32>,
}

impl Sound {
    /// How loud it is at timeline frame `frame`: its keyed volume, or its volume; 0 muted.
    pub fn gain_at(&self, frame: u32) -> f32 {
        if self.muted {
            return 0.0;
        }
        let local = i64::from(frame) - self.key_origin;
        let v = self.volume_keys.value_at(local as f32).unwrap_or(self.volume);
        if v.is_finite() { v.clamp(0.0, MAX_VOLUME) } else { 0.0 }
    }

    /// When it ends, seconds from the timeline's first frame.
    pub fn end(&self) -> f64 {
        self.at + self.length
    }
}

impl AudioClip {
    /// Values in range: a finite trim and offset, the volume 0 to [`MAX_VOLUME`].
    pub(crate) fn repair(&mut self) {
        self.trim_start = finite_at_least_0(self.trim_start);
        self.offset = finite_at_least_0(self.offset);
        self.volume = if self.volume.is_finite() { self.volume.clamp(0.0, MAX_VOLUME) } else { 1.0 };
    }
}

impl VideoClip {
    /// Values in range: a finite trim and a finite rect with some size.
    pub(crate) fn repair(&mut self) {
        self.trim_start = finite_at_least_0(self.trim_start);
        let [_, _, w, h] = self.rect;
        if self.rect.iter().any(|v| !v.is_finite()) || w <= 0.0 || h <= 0.0 {
            self.rect = [0.0, 0.0, 1.0, 1.0];
        }
    }

    /// The time in its video it shows on its own frame `local`, at `fps` frames a second.
    pub fn picture_at(&self, local: u32, fps: u32) -> f64 {
        self.trim_start + f64::from(local) / f64::from(fps.max(1))
    }
}

fn finite_at_least_0(v: f64) -> f64 {
    if v.is_finite() { v.max(0.0) } else { 0.0 }
}

impl Clip {
    /// A clip of `content` (a sound or a video), from frame `start` for `duration` frames.
    fn media(name: &str, start: u32, duration: u32, content: ClipContent) -> Clip {
        Clip {
            id: ClipId(0),
            name: name.to_owned(),
            start,
            duration: duration.max(1),
            opacity: 1.0,
            blend: BlendMode::Normal,
            mask: MaskMode::None,
            adjustment: false,
            disabled: false,
            label: None,
            keys: ClipKeys::default(),
            content,
        }
    }
}

/// `w` × `h` fitted inside `canvas`, in its middle: left, top, width, height.
pub fn fitted_rect(size: [u32; 2], canvas: (u32, u32)) -> [f32; 4] {
    let [w, h] = size.map(|v| f64::from(v.max(1)));
    let (cw, ch) = (f64::from(canvas.0.max(1)), f64::from(canvas.1.max(1)));
    let scale = (cw / w).min(ch / h);
    let (fw, fh) = (w * scale, h * scale);
    [(cw - fw) / 2.0, (ch - fh) / 2.0, fw, fh].map(|v| v as f32)
}

impl Timeline {
    /// Media `id`, if the timeline carries it.
    pub fn media_of(&self, id: MediaId) -> Option<&Media> {
        self.media.iter().find(|m| m.id == id)
    }

    /// Adds `media` to what the timeline carries (as it is, if it carries it already).
    pub fn add_media(&mut self, media: Media) {
        if self.media_of(media.id).is_none() {
            self.media.push(media);
        }
    }

    /// Frames `seconds` lasts, at least 1 (a part of a frame counts as one).
    pub fn frames_of(&self, seconds: f64) -> u32 {
        let frames = (seconds * f64::from(self.fps)).max(0.0);
        // just over a whole number of frames counts as that number
        let whole = (frames - 1e-6).ceil();
        if whole >= f64::from(u32::MAX) { u32::MAX } else { (whole as u32).max(1) }
    }

    /// How many frames of a sound or video clip's media are left from where it starts in it;
    /// none for other clips, or media the timeline doesn't carry.
    pub fn media_frames_left(&self, clip: &Clip) -> Option<u32> {
        let (media, trim) = match &clip.content {
            ClipContent::Audio(a) => (a.media, a.trim_start),
            ClipContent::Video(v) => (v.media, v.trim_start),
            ClipContent::Drawing(_) | ClipContent::Group(_) => return None,
        };
        let m = self.media_of(media)?;
        Some(self.frames_of(m.seconds - trim))
    }

    /// A sound of media `media` from frame `at` on a new track of its own below the others,
    /// named after it, as long as it is. Returns its track and clip.
    pub fn add_sound(&mut self, media: MediaId, at: u32) -> Option<(TrackId, ClipId)> {
        let m = self.media_of(media).filter(|m| m.kind == MediaKind::Audio)?.clone();
        let content = ClipContent::Audio(AudioClip {
            media,
            trim_start: 0.0,
            offset: 0.0,
            volume: 1.0,
            muted: false,
            volume_keys: Channel::default(),
        });
        let clip = Clip::media(&m.name, at, self.frames_of(m.seconds), content);
        let track = self.track_of(&m.name, false, vec![clip]);
        let ids = (track.id, track.clips.first()?.id);
        self.tracks.insert(0, track);
        self.fit();
        Some(ids)
    }

    /// A video of media `media` from frame `at`, fitted into a `canvas`-sized canvas, on a new
    /// track of its own under the drawings (above any sound tracks at the bottom), as long as it
    /// is. Returns its track and clip.
    pub fn add_video(&mut self, media: MediaId, at: u32, canvas: (u32, u32)) -> Option<(TrackId, ClipId)> {
        let m = self.media_of(media).filter(|m| m.kind == MediaKind::Video)?.clone();
        let content = ClipContent::Video(VideoClip { media, trim_start: 0.0, rect: fitted_rect(m.size, canvas) });
        let clip = Clip::media(&m.name, at, self.frames_of(m.seconds), content);
        let track = self.track_of(&m.name, false, vec![clip]);
        let ids = (track.id, track.clips.first()?.id);
        let above_sounds = self.tracks.iter().take_while(|t| is_sound_track(t)).count();
        self.tracks.insert(above_sounds, track);
        self.fit();
        Some(ids)
    }

    /// Sound or video clip `id`'s first frame moved to `start` (on its own level), its end kept:
    /// it starts that much later or earlier in its media. Kept within its media and clear of the
    /// clip before it; false if it isn't such a clip.
    pub fn trim_media_start(&mut self, id: ClipId, start: u32) -> bool {
        let fps = f64::from(self.fps);
        let Some((level, t, c)) = level_of_clip(&mut self.tracks, id) else { return false };
        let Some(track) = level.get_mut(t) else { return false };
        let before_end = c.checked_sub(1).and_then(|p| track.clips.get(p)).map_or(0, Clip::end);
        let Some(clip) = track.clips.get_mut(c) else { return false };
        let end = clip.end();
        let trim = match &clip.content {
            ClipContent::Audio(a) => a.trim_start,
            ClipContent::Video(v) => v.trim_start,
            ClipContent::Drawing(_) | ClipContent::Group(_) => return false,
        };
        // no earlier than its media's start, nor than the clip before; at least a frame long
        let earliest = (i64::from(clip.start) - (trim * fps).floor() as i64).max(i64::from(before_end));
        let start = i64::from(start).clamp(earliest, i64::from(end) - 1);
        let by = start - i64::from(clip.start);
        let trim = (trim + by as f64 / fps).max(0.0);
        let Ok(start) = u32::try_from(start) else { return false };
        clip.start = start;
        clip.duration = end - start;
        match &mut clip.content {
            ClipContent::Audio(a) => a.trim_start = trim,
            ClipContent::Video(v) => v.trim_start = trim,
            ClipContent::Drawing(_) | ClipContent::Group(_) => {}
        }
        true
    }

    /// Sound or video clip `id` starting `trim_start` seconds into its media, where it is (a
    /// slip), and no longer than what is left of the media from there.
    pub fn slip_media(&mut self, id: ClipId, trim_start: f64) -> bool {
        let Some(clip) = self.clip(id) else { return false };
        let media = match &clip.content {
            ClipContent::Audio(a) => a.media,
            ClipContent::Video(v) => v.media,
            ClipContent::Drawing(_) | ClipContent::Group(_) => return false,
        };
        let seconds = self.media_of(media).map_or(f64::MAX, |m| m.seconds);
        let one_frame = 1.0 / f64::from(self.fps);
        let trim = finite_at_least_0(trim_start).min((seconds - one_frame).max(0.0));
        let left = self.frames_of(seconds - trim);
        let Some(clip) = self.clip_mut(id) else { return false };
        match &mut clip.content {
            ClipContent::Audio(a) => a.trim_start = trim,
            ClipContent::Video(v) => v.trim_start = trim,
            ClipContent::Drawing(_) | ClipContent::Group(_) => return false,
        }
        clip.duration = clip.duration.min(left).max(1);
        true
    }

    /// Sound clip `id`'s volume (0 to [`MAX_VOLUME`]), whether it is muted, and its start
    /// `offset` seconds after its first frame (less than a frame).
    pub fn set_sound(&mut self, id: ClipId, volume: f32, muted: bool, offset: f64) -> bool {
        let frame = 1.0 / f64::from(self.fps);
        let Some(ClipContent::Audio(a)) = self.clip_mut(id).map(|c| &mut c.content) else { return false };
        a.volume = if volume.is_finite() { volume.clamp(0.0, MAX_VOLUME) } else { a.volume };
        a.muted = muted;
        a.offset = finite_at_least_0(offset).min(frame * 0.999);
        true
    }

    /// Video clip `id` drawn into `rect` (left, top, width, height on the canvas).
    pub fn set_video_rect(&mut self, id: ClipId, rect: [f32; 4]) -> bool {
        let Some(ClipContent::Video(v)) = self.clip_mut(id).map(|c| &mut c.content) else { return false };
        v.rect = rect;
        v.repair();
        true
    }

    /// The media its sound and video clips play, at any depth.
    pub fn used_media(&self) -> HashSet<MediaId> {
        fn walk(tracks: &[Track], out: &mut HashSet<MediaId>) {
            for clip in tracks.iter().flat_map(|t| &t.clips) {
                match &clip.content {
                    ClipContent::Audio(a) => {
                        out.insert(a.media);
                    }
                    ClipContent::Video(v) => {
                        out.insert(v.media);
                    }
                    ClipContent::Group(g) => walk(&g.tracks, out),
                    ClipContent::Drawing(_) => {}
                }
            }
        }
        let mut out = HashSet::new();
        walk(&self.tracks, &mut out);
        out
    }

    /// Lets go of the media no clip plays.
    pub fn prune_media(&mut self) {
        let used = self.used_media();
        self.media.retain(|m| used.contains(&m.id));
    }

    /// Every sound that plays, in timeline time: a group's sounds counted from its start, again
    /// on each of its cycles, and cut off where the group ends. Disabled clips are left out; a
    /// sound on a hidden track is muted.
    pub fn sounds(&self) -> Vec<Sound> {
        let mut out = Vec::new();
        let span = Span { from: 0, start: 0, end: u32::MAX, hidden: false };
        sounds_of(self, &self.tracks, span, &mut out);
        out.sort_by(|a, b| a.at.total_cmp(&b.at));
        out
    }

    /// The videos showing at `frame` and the time in each, at any depth.
    pub fn videos_at(&self, frame: u32) -> Vec<(MediaId, f64)> {
        fn walk(items: &[crate::timeline::PlanItem], out: &mut Vec<(MediaId, f64)>) {
            for item in items {
                match &item.shows {
                    crate::timeline::Shows::Video { media, seconds, .. } => out.push((*media, *seconds)),
                    crate::timeline::Shows::Group(inner) => walk(inner, out),
                    crate::timeline::Shows::Drawing(_) => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.plan(frame).items, &mut out);
        out
    }

    /// The time in its video that video clip `id` shows on each of its frames, first to last.
    pub fn video_times(&self, id: ClipId) -> Vec<f64> {
        let Some(clip) = self.clip(id) else { return Vec::new() };
        let ClipContent::Video(v) = &clip.content else { return Vec::new() };
        (0..clip.duration).map(|local| v.picture_at(local, self.fps)).collect()
    }
}

/// Where a level's frames sit on the timeline: its frame 0 is timeline frame `from`, and only
/// timeline frames `start..end` of it play.
#[derive(Clone, Copy)]
struct Span {
    from: i64,
    start: i64,
    end: u32,
    hidden: bool,
}

fn is_sound_track(t: &Track) -> bool {
    !t.clips.is_empty() && t.clips.iter().all(|c| matches!(c.content, ClipContent::Audio(_)))
}

fn sounds_of(tl: &Timeline, tracks: &[Track], span: Span, out: &mut Vec<Sound>) {
    let fps = f64::from(tl.fps);
    for track in tracks {
        let hidden = span.hidden || track.hidden;
        for clip in track.clips.iter().filter(|c| !c.disabled) {
            let first = span.from + i64::from(clip.start);
            let last = (span.from + i64::from(clip.end())).min(i64::from(span.end));
            let first_played = first.max(span.start);
            if first_played >= last {
                continue;
            }
            match &clip.content {
                ClipContent::Audio(a) => {
                    let Some(m) = tl.media_of(a.media) else { continue };
                    // cut short at the start (a cycle's or group's), it plays on from where it is
                    let skipped = (first_played - first) as f64 / fps;
                    let at = first_played as f64 / fps + if skipped > 0.0 { 0.0 } else { a.offset };
                    let from = a.trim_start + (skipped - a.offset).max(0.0);
                    let length = (last as f64 / fps - at).min(m.seconds - from);
                    if length <= 0.0 {
                        continue;
                    }
                    out.push(Sound {
                        clip: clip.id,
                        media: a.media,
                        at,
                        from,
                        length,
                        key_origin: first,
                        volume: a.volume,
                        muted: a.muted || hidden,
                        volume_keys: a.volume_keys.clone(),
                    });
                }
                ClipContent::Group(Group { tracks: inner, cycle, .. }) => {
                    let length = i64::from(cycle.map_or(clip.duration, |c| c.length.max(1)));
                    let mut from = first;
                    while from < last {
                        let end = u32::try_from((from + length).min(last)).unwrap_or(u32::MAX);
                        let inner_span = Span { from, start: from.max(first_played), end, hidden };
                        sounds_of(tl, inner, inner_span, out);
                        from += length;
                    }
                }
                ClipContent::Drawing(_) | ClipContent::Video(_) => {}
            }
        }
    }
}

#[cfg(test)]
#[path = "media_tests.rs"]
mod tests;

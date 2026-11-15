//! The timeline: tracks of clips placed in time, each clip with keyed channels of its own. Drawn
//! from Koi Studio's Timeline v2 (see `docs/design/timeline.md`).
//!
//! The pixels stay where they are today: a drawing clip names a layer or group in the document's
//! layer tree ([`LayerId`]), so its layers keep their textures, parking, undo and file format. The
//! timeline is only the structure that says when and how each one shows.
//!
//! Frames are whole numbers. A clip's keys are clip-relative (key 0 is the clip's first frame
//! wherever it sits), so moving or duplicating a clip carries its motion with it.
//!
//! This module holds the model, turning a canvas into one ([`Timeline::from_document`]), and
//! sampling a frame into what the renderer draws ([`Timeline::plan`]). Keyed values, the easings'
//! maths and editing come next.

use std::collections::BTreeMap;

use crate::animation::{self, Playback};
use crate::document::{BlendMode, Document, LayerId};
use crate::geom::{Affine, Vec2};

/// Most frames a second.
pub const MAX_FPS: u32 = 120;

/// Most frames a new drawing is held for.
pub const MAX_EXPOSURE: u32 = 12;

fn on_ones() -> u32 {
    1
}

/// A track, known for as long as it lasts (never reused within a document).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct TrackId(pub u64);

/// A clip, likewise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct ClipId(pub u64);

/// A piece of media kept in the canvas's package (a sound or a video), by the hash of its bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct MediaId(pub u64);

/// A layer effect in a clip's stack (live effects come in phase 4; their keys are kept here).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct EffectId(pub u64);

/// The document's animation: how fast and how long it plays, its tracks, and its camera.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Timeline {
    /// Frames a second, 1 to [`MAX_FPS`].
    pub fps: u32,
    /// How many frames it lasts, at least 1.
    pub duration: u32,
    /// How many frames a new drawing is held for (1 on ones, 2 on twos...), 1 to [`MAX_EXPOSURE`].
    #[serde(default = "on_ones")]
    pub exposure: u32,
    /// The track shown as a flipbook strip (simple mode); none, every track (the full view). A
    /// view only: the other tracks still draw.
    #[serde(default)]
    pub simple: Option<TrackId>,
    /// Bottom first, as the document's layers are: a higher track draws over a lower one.
    pub tracks: Vec<Track>,
    /// What part of the canvas fills the output frame, keyed. No keys: the whole canvas, as it is.
    #[serde(default)]
    pub camera: Channel<CameraPose>,
    #[serde(default)]
    pub playback: Playback,
    /// Frames played over and over while drawing (start, one past the end); none, the whole.
    #[serde(default)]
    pub loop_range: Option<(u32, u32)>,
    #[serde(default)]
    pub onion: OnionSkins,
    /// The media its audio and video clips play, in the canvas's package.
    #[serde(default)]
    pub media: Vec<Media>,
    /// The ids the next track and clip take.
    pub next_track: u64,
    pub next_clip: u64,
}

/// A lane of clips in time. Clips never overlap on a track; their order on it is their order in
/// time.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Track {
    pub id: TrackId,
    pub name: String,
    pub hidden: bool,
    /// Its clips can't be moved, trimmed or drawn on.
    pub locked: bool,
    /// Multiplies every clip's own opacity.
    pub opacity: f32,
    /// In time order, none overlapping.
    pub clips: Vec<Clip>,
}

/// Something placed on a track for a span of frames, and everything that says how it composites.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Clip {
    pub id: ClipId,
    pub name: String,
    /// Its first frame on the timeline it is on (the document's, or a group's own).
    pub start: u32,
    /// How many frames it lasts, at least 1.
    pub duration: u32,
    /// Its opacity when its opacity channel has no keys.
    pub opacity: f32,
    pub blend: BlendMode,
    pub mask: MaskMode,
    /// Draws nothing of its own: its effects change what is under it.
    #[serde(default)]
    pub adjustment: bool,
    /// Left out: draws nothing, masks nothing, exports nothing.
    #[serde(default)]
    pub disabled: bool,
    /// A colour tag shown on the clip, sRGB 0..1.
    #[serde(default)]
    pub label: Option<[f32; 3]>,
    /// Its keyed channels, clip-relative.
    #[serde(default)]
    pub keys: ClipKeys,
    pub content: ClipContent,
}

/// What a clip holds.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ClipContent {
    /// One drawing, shown for the clip's whole span (its exposure).
    Drawing(Drawing),
    /// A timeline of its own, in its own frames.
    Group(Group),
    Video(VideoClip),
    Audio(AudioClip),
}

/// One drawing: a layer, or a group of layers, in the document's layer tree.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Drawing {
    /// A top-level layer or group of the document. Nothing else names it.
    pub root: LayerId,
}

/// A clip that holds tracks of its own.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Group {
    /// Bottom first. Their clips' frames count from the group's start.
    pub tracks: Vec<Track>,
    /// Shown folded in the timeline panel.
    #[serde(default)]
    pub collapsed: bool,
    /// Plays its first `length` frames over and over for the clip's span.
    #[serde(default)]
    pub cycle: Option<Cycle>,
}

/// How a group repeats.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Cycle {
    /// Frames in one cycle, at least 1.
    pub length: u32,
    pub keys: CycleKeys,
}

/// What a repeating group's own keys do across its cycles.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CycleKeys {
    /// Every cycle plays the keys again from the start.
    Repeat,
    /// The keys run on across the whole span.
    ContinueAcross,
}

/// A video, for drawing over (rotoscoping) or as part of the picture.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct VideoClip {
    pub media: MediaId,
    /// Where in the video the clip starts, seconds.
    pub trim_start: f64,
    /// Where its picture sits on the canvas: left, top, width, height (canvas pixels).
    pub rect: [f32; 4],
}

/// A sound.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AudioClip {
    pub media: MediaId,
    /// Where in the sound the clip starts, seconds.
    pub trim_start: f64,
    /// Starts this far (seconds, under a frame) after the clip's first frame, for sync finer than
    /// a frame.
    #[serde(default)]
    pub offset: f64,
    pub volume: f32,
    #[serde(default)]
    pub muted: bool,
    /// The volume, keyed (clip-relative); no keys, `volume`.
    #[serde(default)]
    pub volume_keys: Channel<f32>,
}

/// A sound or video file the canvas carries (see `media`).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Media {
    pub id: MediaId,
    /// The name it came in with.
    pub name: String,
    pub kind: MediaKind,
    /// Its length, seconds.
    pub seconds: f64,
    /// Its file's extension (`m4a`, `mov`...), which says how to read it.
    #[serde(default)]
    pub extension: String,
    /// A video's picture size, pixels; none for a sound.
    #[serde(default)]
    pub size: [u32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MediaKind {
    Audio,
    Video,
}

/// How a clip is masked by the clip under it on the track below (its source), where the two
/// overlap in time. With no source there, it draws as it is: trimming or moving the clip below
/// never loses anything.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MaskMode {
    #[default]
    None,
    /// Shows only where the source is painted; the source still draws (a clipping mask).
    Clipping,
    /// The source's alpha is the matte, and the source no longer draws.
    Alpha,
    /// The source's lightness is the matte, and the source no longer draws.
    Luminance,
}

impl MaskMode {
    /// Whether the source stops drawing of its own.
    pub fn consumes_source(self) -> bool {
        matches!(self, MaskMode::Alpha | MaskMode::Luminance)
    }
}

/// A clip's keyed channels. A channel with no keys leaves the clip as it is.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ClipKeys {
    /// Moved this far, canvas pixels.
    #[serde(default)]
    pub position: Channel<[f32; 2]>,
    /// Scaled about the pivot, across and down.
    #[serde(default)]
    pub scale: Channel<[f32; 2]>,
    /// Turned about the pivot, radians, never wrapped: 4π is two whole turns.
    #[serde(default)]
    pub rotation: Channel<f32>,
    #[serde(default)]
    pub opacity: Channel<f32>,
    /// What it scales and turns about, canvas pixels: set with its first pose key (the middle of
    /// what is painted then), cleared with its last.
    #[serde(default)]
    pub pivot: Option<[f32; 2]>,
    /// Effects' parameters, keyed, by effect.
    #[serde(default)]
    pub effects: BTreeMap<EffectId, Channel<Vec<f32>>>,
}

impl ClipKeys {
    /// Whether any channel that moves, scales or turns the clip has keys.
    pub fn posed(&self) -> bool {
        !(self.position.keys.is_empty() && self.scale.keys.is_empty() && self.rotation.keys.is_empty())
    }
}

/// Keys of one value over time, in frame order, at most one a frame.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(bound(serialize = "T: serde::Serialize", deserialize = "T: serde::Deserialize<'de>"))]
pub struct Channel<T> {
    pub keys: Vec<Key<T>>,
}

impl<T> Default for Channel<T> {
    fn default() -> Channel<T> {
        Channel { keys: Vec::new() }
    }
}

impl<T> Channel<T> {
    /// The keys either side of `frame`: the last at or before it and the first after it.
    pub fn around(&self, frame: i64) -> (Option<&Key<T>>, Option<&Key<T>>) {
        let after = self.keys.partition_point(|k| i64::from(k.frame) <= frame);
        (after.checked_sub(1).and_then(|i| self.keys.get(i)), self.keys.get(after))
    }
}

/// A value at a frame, and how it goes on to the next key.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Key<T> {
    /// Clip-relative.
    pub frame: i32,
    pub value: T,
    /// How it eases into the next key.
    pub easing: Easing,
    /// For a position key: the motion path's curve through it, apart from its timing.
    #[serde(default)]
    pub path: Option<PathHandles>,
}

/// A motion path's handles at a position key, as offsets from the key's own value (so moving the
/// key carries them).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PathHandles {
    pub mode: HandleMode,
    /// Towards the key before, canvas pixels.
    pub before: [f32; 2],
    /// Towards the key after.
    pub after: [f32; 2],
}

/// How a key's two handles follow each other as one is dragged.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum HandleMode {
    /// Mirrored, in angle and length.
    Symmetric,
    /// In line, each its own length.
    Asymmetric,
    /// Each its own: the path can kink.
    Broken,
}

/// How a value goes from one key to the next, as progress 0..1 over the span to progress (which
/// may overshoot 0..1). Koi Studio's easings, with their parameters.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum Easing {
    #[default]
    Linear,
    /// Keeps the key's value until the next key.
    Hold,
    /// Through the keys either side as a smooth spline.
    Smooth,
    /// A CSS-style cubic Bézier from (0,0) to (1,1) with these two control points.
    CubicBezier([f32; 2], [f32; 2]),
    /// A drawn curve through these points (x 0..1, y may leave 0..1).
    Curve(Vec<[f32; 2]>),
    Bounce {
        first_step: f32,
        bounciness: f32,
    },
    Elastic {
        step: f32,
        attack: f32,
        decay: f32,
        magnitude: f32,
    },
    Steps {
        step: f32,
        smoothing: f32,
    },
    ElasticSteps {
        step: f32,
        magnitude: f32,
    },
    Cyclic {
        step: f32,
        sharpness: f32,
        skew: f32,
        decay: f32,
    },
    Random {
        frequency: f32,
        magnitude: f32,
        seed: f32,
        attenuation: f32,
        warp: f32,
        tilt: f32,
    },
    /// Another easing, repeated across the span.
    Fitted(Box<Easing>, RepeatFit),
    /// Another easing, played backwards.
    Reversed(Box<Easing>),
}

/// How an easing repeats across a span.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct RepeatFit {
    pub mode: FitMode,
    pub repetitions: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum FitMode {
    /// Each repetition arrives at the end value.
    Arrive,
    /// Back and forth.
    Loop,
}

/// What part of the canvas fills the output frame.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CameraPose {
    /// The canvas point at the frame's middle.
    pub center: [f32; 2],
    /// Output pixels to a canvas pixel: 2 is a 2× push-in.
    pub zoom: f32,
    /// How far the canvas seems turned in the frame, radians (eased the short way round).
    pub rotation: f32,
}

/// The least and most a camera zooms.
pub const CAMERA_ZOOM: (f32, f32) = (0.05, 20.0);

impl CameraPose {
    /// The whole of a `width` × `height` canvas, as it is.
    pub fn still(width: f32, height: f32) -> CameraPose {
        CameraPose { center: [width / 2.0, height / 2.0], zoom: 1.0, rotation: 0.0 }
    }

    /// Canvas pixels to the output frame's (the size of the `width` × `height` canvas): the
    /// center to the frame's middle, zoomed and turned about it.
    pub fn to_frame(&self, width: f32, height: f32) -> Affine {
        let [cx, cy] = self.center;
        Affine::translate(Vec2::new(-cx, -cy))
            .then(Affine::scale(self.zoom.clamp(CAMERA_ZOOM.0, CAMERA_ZOOM.1)))
            .then(Affine::rotate(self.rotation))
            .then(Affine::translate(Vec2::new(width / 2.0, height / 2.0)))
    }

    /// The output frame's corners on the canvas (top left, top right, bottom right, bottom left),
    /// for showing where the camera looks.
    pub fn corners(&self, width: f32, height: f32) -> [[f32; 2]; 4] {
        let back = self.to_frame(width, height).inverse().unwrap_or(Affine::IDENTITY);
        [[0.0, 0.0], [width, 0.0], [width, height], [0.0, height]].map(|[x, y]| {
            let p = back.apply(Vec2::new(x, y));
            [p.x, p.y]
        })
    }
}

/// How the drawings around the current one show while drawing.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct OnionSkins {
    /// Drawings shown on each side (before, after).
    pub count: [u32; 2],
    /// The nearest ones' opacity, 0..1 (further ones fade).
    pub opacity: f32,
    /// What the ones before and after are screened with (sRGB 0..1; black leaves them as they are).
    pub colors: [[f32; 3]; 2],
    /// The current drawing shows blended with the onion skins instead of over them.
    #[serde(default)]
    pub blend_primary: bool,
}

impl Default for OnionSkins {
    fn default() -> OnionSkins {
        OnionSkins { count: [1, 1], opacity: 0.4, colors: [[0.9, 0.2, 0.2], [0.2, 0.6, 0.2]], blend_primary: false }
    }
}

/// What dragging a clip's edge does when it reaches the next clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EdgePolicy {
    /// Stops at it.
    Clamp,
    /// Pushes it, and every clip after it, along: the gaps stay.
    Push,
    /// Eats into it, shortening it; nothing else moves.
    TrimNeighbour,
}

impl Clip {
    /// One past its last frame.
    pub fn end(&self) -> u32 {
        self.start + self.duration
    }

    /// Whether it shows at `frame` of the timeline it is on.
    pub fn covers(&self, frame: u32) -> bool {
        frame >= self.start && frame < self.end()
    }

    /// `frame` of the timeline it is on, in its own frames.
    pub fn local(&self, frame: u32) -> i64 {
        i64::from(frame) - i64::from(self.start)
    }
}

impl Track {
    /// The clip showing at `frame`, if any.
    pub fn clip_at(&self, frame: u32) -> Option<&Clip> {
        let i = self.clips.partition_point(|c| c.end() <= frame);
        self.clips.get(i).filter(|c| c.covers(frame))
    }

    /// Whether its clips are in time order with none overlapping.
    pub fn is_ordered(&self) -> bool {
        self.clips.windows(2).all(|w| matches!(w, [a, b] if a.end() <= b.start))
            && self.clips.iter().all(|c| c.duration >= 1)
    }
}

impl Timeline {
    /// An empty timeline: `fps` frames a second, `duration` frames long.
    pub fn new(fps: u32, duration: u32) -> Timeline {
        Timeline {
            fps: fps.clamp(1, MAX_FPS),
            duration: duration.max(1),
            exposure: on_ones(),
            simple: None,
            tracks: Vec::new(),
            camera: Channel::default(),
            playback: Playback::default(),
            loop_range: None,
            onion: OnionSkins::default(),
            media: Vec::new(),
            next_track: 1,
            next_clip: 1,
        }
    }

    /// Every drawing's root layer, at any depth, in no particular order.
    pub fn drawing_roots(&self) -> Vec<LayerId> {
        fn walk(tracks: &[Track], out: &mut Vec<LayerId>) {
            for clip in tracks.iter().flat_map(|t| &t.clips) {
                match &clip.content {
                    ClipContent::Drawing(d) => out.push(d.root),
                    ClipContent::Group(g) => walk(&g.tracks, out),
                    ClipContent::Video(_) | ClipContent::Audio(_) => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.tracks, &mut out);
        out
    }

    /// A track of its own id holding `clips` (given ids of their own here).
    pub(crate) fn track_of(&mut self, name: &str, hidden: bool, clips: Vec<Clip>) -> Track {
        let id = TrackId(self.next_track);
        self.next_track += 1;
        let clips = clips
            .into_iter()
            .map(|c| {
                let id = ClipId(self.next_clip);
                self.next_clip += 1;
                Clip { id, ..c }
            })
            .collect();
        Track { id, name: name.to_owned(), hidden, locked: false, opacity: 1.0, clips }
    }

    /// `doc` as a timeline, if it can be one: Animation Assist's frames in order, each a drawing
    /// held as long as it was, and the frames it always showed (background, foreground) and its
    /// hidden layers on tracks of their own, so every frame shows just what it showed before. A
    /// canvas without Animation Assist becomes one frame with a track for each top-level layer or
    /// group. None for Page Assist, whose pages aren't frames.
    pub fn from_document(doc: &Document) -> Option<Timeline> {
        let top = doc.members(None);
        let place = |id: LayerId| top.iter().position(|t| *t == id).unwrap_or(usize::MAX);
        let name = |id: LayerId| doc.layer(id).map_or_else(|| "Drawing".to_owned(), |l| l.name.clone());
        let Some(a) = &doc.animation else {
            let mut tl = Timeline::new(animation::Animation::default().fps, 1);
            for id in top {
                let hidden = doc.layer(id).is_none_or(|l| !l.visible);
                let track = tl.track_of(&name(id), hidden, vec![Clip::drawing(&name(id), id, 0, 1)]);
                tl.tracks.push(track);
            }
            return Some(tl);
        };
        if a.pages {
            return None;
        }
        let frames = animation::frames(doc);
        let background = frames.first().copied().filter(|_| a.background);
        let foreground = frames.last().copied().filter(|_| a.foreground && frames.len() > 1);
        let mut starts = Vec::with_capacity(frames.len());
        let mut at = 0_u32;
        for id in &frames {
            let held = 1 + a.hold(*id);
            starts.push((*id, at, held));
            at += held;
        }
        let mut tl = Timeline::new(a.fps, at);
        tl.playback = a.playback;
        tl.onion = OnionSkins {
            count: [a.onion_frames; 2],
            opacity: a.onion_opacity,
            colors: [a.before, a.after],
            blend_primary: a.blend_primary,
        };
        let whole = tl.duration;
        // each track goes where its lowest layer was, the frames always shown below and above
        let mut tracks: Vec<((usize, u8), Track)> = Vec::new();
        if let Some(id) = background {
            let track = tl.track_of("Background", false, vec![Clip::drawing(&name(id), id, 0, whole)]);
            tracks.push(((place(id), 0), track));
        }
        let drawn = starts
            .iter()
            .filter(|(id, ..)| Some(*id) != background && Some(*id) != foreground)
            .map(|(id, start, held)| Clip::drawing(&name(*id), *id, *start, *held))
            .collect();
        let frames_track = tl.track_of("Animation", false, drawn);
        tl.simple = Some(frames_track.id);
        tracks.push(((frames.first().map_or(0, |id| place(*id)), 1), frames_track));
        if let Some(id) = foreground {
            let track = tl.track_of("Foreground", false, vec![Clip::drawing(&name(id), id, 0, whole)]);
            tracks.push(((place(id), 2), track));
        }
        for id in top.iter().filter(|id| !frames.contains(id)) {
            let track = tl.track_of(&name(*id), true, vec![Clip::drawing(&name(*id), *id, 0, whole)]);
            tracks.push(((place(*id), 1), track));
        }
        tracks.sort_by_key(|(key, _)| *key);
        tl.tracks = tracks.into_iter().map(|(_, t)| t).collect();
        Some(tl)
    }

    /// The timeline made consistent with the document's top-level layers `top`: values in range,
    /// clips in order without overlaps, each drawing naming a top-level layer no other drawing
    /// names, keys in frame order, ids that won't collide. What can't be kept is left out (a
    /// drawing whose layer is gone, a clip overlapping the one before it); the layers stay.
    pub fn repaired(mut self, top: &[LayerId]) -> Timeline {
        self.fps = self.fps.clamp(1, MAX_FPS);
        self.duration = self.duration.max(1);
        self.exposure = self.exposure.clamp(1, MAX_EXPOSURE);
        self.onion.opacity = if self.onion.opacity.is_finite() { self.onion.opacity.clamp(0.0, 1.0) } else { 0.4 };
        self.onion.colors = self.onion.colors.map(|c| c.map(|v| if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 }));
        tidy(&mut self.camera);
        let mut seen = Seen::default();
        repair_tracks(&mut self.tracks, top, &mut seen);
        self.next_track = self.next_track.max(seen.most_track + 1);
        self.next_clip = self.next_clip.max(seen.most_clip + 1);
        if self.simple.is_some_and(|s| !self.tracks.iter().any(|t| t.id == s)) {
            self.simple = None;
        }
        self.loop_range = self.loop_range.filter(|(a, b)| a < b && *b <= self.duration);
        let mut media = std::collections::HashSet::new();
        self.media.retain(|m| media.insert(m.id));
        self
    }

    /// What shows at `frame`, bottom first: each track's clip there, groups opened at their own
    /// frame. Keyed values (pose, opacity keys, effects) aren't applied yet: each clip draws at its
    /// own opacity, in place.
    pub fn plan(&self, frame: u32) -> FramePlan {
        let mut items = Vec::new();
        sample(&self.tracks, frame, self.fps, &mut items);
        FramePlan { items }
    }

    /// The drawings the canvas shows at `frame`, bottom first, each composited as one: the
    /// plan's (groups opened, mattes left out until they are drawn as mattes), and, while drawing
    /// rather than `playing`, the drawings either side on the simple-mode track as onion skins,
    /// faded and tinted as Animation Assist's are ([`animation::look`]).
    pub fn showing(&self, frame: u32, playing: bool) -> Vec<Showing> {
        let plan = self.plan(frame);
        let mut out = Vec::new();
        for track in &self.tracks {
            let onions =
                (!playing && !track.hidden && self.simple == Some(track.id)).then(|| self.onions(track, frame));
            let (before, after) = onions.unwrap_or_default();
            out.extend(before);
            for item in plan.items.iter().filter(|i| i.track == track.id) {
                shown_of(item, 1.0, None, &mut out);
            }
            out.extend(after);
        }
        out
    }

    /// The onion skins either side of `frame` on `track`: the drawings before it (oldest first)
    /// and after it.
    fn onions(&self, track: &Track, frame: u32) -> (Vec<Showing>, Vec<Showing>) {
        let drawings: Vec<&Clip> = track
            .clips
            .iter()
            .filter(|c| !c.disabled && !c.adjustment && matches!(c.content, ClipContent::Drawing(_)))
            .collect();
        let skin = |clip: &Clip, distance: usize, side: usize| {
            let ClipContent::Drawing(d) = &clip.content else { return None };
            let n = self.onion.count.get(side).copied().unwrap_or(0) as usize;
            if distance == 0 || distance > n {
                return None;
            }
            // the nearest at the set opacity squared, fading to nothing past the furthest
            let fade = 1.0 - (distance - 1) as f32 / n as f32;
            let color = self.onion.colors.get(side).copied().unwrap_or([0.0; 3]);
            let opacity = self.onion.opacity * self.onion.opacity * fade;
            Some(Showing {
                root: d.root,
                look: animation::FrameLook::Onion { color, opacity },
                opacity: clip.keys.opacity_at(0.0).unwrap_or(clip.opacity) * track.opacity,
                pose: clip.keys.pose_at(0.0),
                blend: clip.blend,
            })
        };
        let mut before: Vec<Showing> = drawings
            .iter()
            .filter(|c| c.end() <= frame)
            .rev()
            .enumerate()
            .filter_map(|(i, c)| skin(c, i + 1, 0))
            .collect();
        before.reverse();
        let after =
            drawings.iter().filter(|c| c.start > frame).enumerate().filter_map(|(i, c)| skin(c, i + 1, 1)).collect();
        (before, after)
    }
}

/// A drawing as the canvas shows it: as it is, or as an onion skin.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Showing {
    pub root: LayerId,
    pub look: animation::FrameLook,
    /// Its clip's (and its tracks' and groups') opacity, on top of the look's.
    pub opacity: f32,
    /// How its clip's keys (and its groups') move, scale and turn it; none: in place.
    pub pose: Option<Affine>,
    /// How it blends, its clip's mode over its layer's own (Normal: the layer's).
    pub blend: BlendMode,
}

/// The drawings `item` shows, into `out`, its opacity times `outer` (a group's), its pose then
/// `around`'s (a group's).
fn shown_of(item: &PlanItem, outer: f32, around: Option<Affine>, out: &mut Vec<Showing>) {
    if item.matte_only {
        return;
    }
    let opacity = item.opacity * outer;
    let pose = match (item.pose, around) {
        (Some(p), Some(a)) => Some(p.then(a)),
        (p, a) => p.or(a),
    };
    match &item.shows {
        Shows::Drawing(root) => {
            out.push(Showing { root: *root, look: animation::FrameLook::Shown, opacity, pose, blend: item.blend })
        }
        Shows::Group(inner) => inner.iter().for_each(|i| shown_of(i, opacity, pose, out)),
        Shows::Video { .. } => {}
    }
}

impl Clip {
    /// A drawing of top-level layer `root`, shown from `start` for `duration` frames as it is.
    fn drawing(name: &str, root: LayerId, start: u32, duration: u32) -> Clip {
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
            content: ClipContent::Drawing(Drawing { root }),
        }
    }
}

impl Document {
    /// Puts the canvas on a timeline made from it (see [`Timeline::from_document`]), which takes
    /// over from Animation Assist. False if it is on one already, or can't be (Page Assist).
    pub fn start_timeline(&mut self) -> bool {
        if self.timeline.is_some() {
            return false;
        }
        let Some(tl) = Timeline::from_document(self) else { return false };
        self.timeline = Some(tl);
        self.animation = None;
        true
    }

    /// The top-level layer or group holding `layer` (itself, at the top level).
    pub fn top_of(&self, layer: LayerId) -> Option<LayerId> {
        let mut id = layer;
        if self.layer(id)?.kind == crate::document::LayerKind::Mask {
            id = self.layer(id)?.parent?;
        }
        for _ in 0..self.layers().len() {
            match self.group_of(id) {
                Some(g) => id = g,
                None => return Some(id),
            }
        }
        None
    }
}

/// Editing a timeline's tracks and clips, at the top level or inside group clips (a track or clip
/// is found by its id at any depth; ids are never shared). Each keeps the rules: clips in time
/// order with none overlapping, and the timeline as long as its clips at least. Frames given to
/// these are on the level of the track or clip named (a group's own, counted from its start).
impl Timeline {
    /// Where top-level clip `id` is: its track's index and its own.
    pub fn find(&self, id: ClipId) -> Option<(usize, usize)> {
        self.tracks
            .iter()
            .enumerate()
            .find_map(|(t, track)| track.clips.iter().position(|c| c.id == id).map(|c| (t, c)))
    }

    /// Clip `id`, at any depth.
    pub fn clip(&self, id: ClipId) -> Option<&Clip> {
        let mut path = Vec::new();
        clip_path(&self.tracks, id, &mut path).then(|| path.last().copied()).flatten()
    }

    pub fn clip_mut(&mut self, id: ClipId) -> Option<&mut Clip> {
        let (level, t, c) = level_of_clip(&mut self.tracks, id)?;
        level.get_mut(t)?.clips.get_mut(c)
    }

    /// The index of top-level track `id`.
    pub fn track_index(&self, id: TrackId) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == id)
    }

    /// Track `id`, at any depth.
    pub fn track(&self, id: TrackId) -> Option<&Track> {
        let groups = self.groups_of_track(id)?;
        let tracks = match groups.last().map(|g| &g.content) {
            Some(ClipContent::Group(g)) => &g.tracks,
            _ => &self.tracks,
        };
        tracks.iter().find(|t| t.id == id)
    }

    pub fn track_mut(&mut self, id: TrackId) -> Option<&mut Track> {
        level_of_track(&mut self.tracks, id)?.iter_mut().find(|t| t.id == id)
    }

    /// The group clips track `id` is inside, outermost first (none at the top level).
    pub fn groups_of_track(&self, id: TrackId) -> Option<Vec<&Clip>> {
        let mut path = Vec::new();
        track_path(&self.tracks, id, &mut path).then_some(path)
    }

    /// The track clip `id` is on, at any depth.
    pub fn track_of_clip(&self, id: ClipId) -> Option<TrackId> {
        fn walk(tracks: &[Track], id: ClipId) -> Option<TrackId> {
            tracks.iter().find_map(|t| {
                t.clips.iter().find_map(|c| match &c.content {
                    _ if c.id == id => Some(t.id),
                    ClipContent::Group(g) => walk(&g.tracks, id),
                    _ => None,
                })
            })
        }
        walk(&self.tracks, id)
    }

    /// The group clip clip `id` is directly inside, if it is in one.
    pub fn group_of(&self, id: ClipId) -> Option<ClipId> {
        let mut path = Vec::new();
        clip_path(&self.tracks, id, &mut path);
        path.iter().rev().nth(1).map(|g| g.id)
    }

    /// Where the level of track `id` starts on the timeline: its groups' starts added up (their
    /// first cycle).
    pub fn level_start(&self, id: TrackId) -> Option<u32> {
        Some(self.groups_of_track(id)?.iter().map(|g| g.start).sum())
    }

    /// The timeline frame clip `id` starts on (in its groups' first cycle).
    pub fn origin(&self, id: ClipId) -> Option<u32> {
        let mut path = Vec::new();
        clip_path(&self.tracks, id, &mut path).then(|| path.iter().map(|c| c.start).sum())
    }

    /// The frame on track `id`'s own level when the timeline is at `frame`: through each group's
    /// start and cycle. Before a group's start it is negative.
    pub fn track_frame(&self, id: TrackId, frame: u32) -> Option<i64> {
        Some(into_groups(&self.groups_of_track(id)?, i64::from(frame)))
    }

    /// Clip `id`'s own frame (counted from its start) when the timeline is at `frame`, through
    /// its groups' starts and cycles.
    pub fn local_frame(&self, id: ClipId, frame: u32) -> Option<i64> {
        let mut path = Vec::new();
        if !clip_path(&self.tracks, id, &mut path) {
            return None;
        }
        let clip = path.pop()?;
        Some(into_groups(&path, i64::from(frame)) - i64::from(clip.start))
    }

    /// The drawing clip of top-level layer `root` (at any depth), and its track.
    pub fn drawing_of(&self, root: LayerId) -> Option<(TrackId, ClipId)> {
        fn walk(tracks: &[Track], root: LayerId) -> Option<(TrackId, ClipId)> {
            tracks.iter().find_map(|t| {
                t.clips.iter().find_map(|c| match &c.content {
                    ClipContent::Drawing(d) if d.root == root => Some((t.id, c.id)),
                    ClipContent::Group(g) => walk(&g.tracks, root),
                    _ => None,
                })
            })
        }
        walk(&self.tracks, root)
    }

    /// A new empty track on top of the others.
    pub fn add_track(&mut self, name: &str) -> TrackId {
        let track = self.track_of(name, false, Vec::new());
        let id = track.id;
        self.tracks.push(track);
        id
    }

    /// A new empty track on top of group clip `group`'s own.
    pub fn add_group_track(&mut self, group: ClipId, name: &str) -> Option<TrackId> {
        let track = self.track_of(name, false, Vec::new());
        let id = track.id;
        match &mut self.clip_mut(group)?.content {
            ClipContent::Group(g) => g.tracks.push(track),
            _ => return None,
        }
        Some(id)
    }

    /// Takes track `id` out, with its clips.
    pub fn remove_track(&mut self, id: TrackId) -> Option<Track> {
        let level = level_of_track(&mut self.tracks, id)?;
        let at = level.iter().position(|t| t.id == id)?;
        let track = level.remove(at);
        if self.simple == Some(id) {
            self.simple = None;
        }
        Some(track)
    }

    /// A drawing of `root` on track `track` at frame `at` (just after the clip there, if one is),
    /// held for the document's exposure; the clips after it move along to make room. Returns its
    /// clip.
    pub fn add_drawing(&mut self, track: TrackId, at: u32, root: LayerId, name: &str) -> Option<ClipId> {
        let mut clip = Clip::drawing(name, root, 0, self.exposure);
        clip.id = ClipId(self.next_clip);
        let level = level_of_track(&mut self.tracks, track)?;
        let track = level.iter_mut().find(|t| t.id == track)?;
        let start = track.clip_at(at).map_or(at, Clip::end);
        clip.start = start;
        let id = clip.id;
        make_room(track, start, clip.end());
        let at = track.clips.partition_point(|c| c.start < start);
        track.clips.insert(at, clip);
        self.next_clip += 1;
        self.fit();
        Some(id)
    }

    /// Clip `id` held for `duration` frames (at least 1). `push`: the clips after it move along
    /// (back, for a shorter hold), keeping the gaps between them; else it stops at the next one.
    pub fn set_duration(&mut self, id: ClipId, duration: u32, push: bool) -> bool {
        // a sound or video no longer than what is left of it
        let duration =
            self.clip(id).and_then(|c| self.media_frames_left(c)).map_or(duration, |left| duration.min(left));
        let Some((level, t, c)) = level_of_clip(&mut self.tracks, id) else { return false };
        let Some(track) = level.get_mut(t) else { return false };
        let Some(clip) = track.clips.get(c) else { return false };
        let (start, old_end) = (clip.start, clip.end());
        let next = track.clips.get(c + 1).map(|n| n.start);
        let duration = match (push, next) {
            (false, Some(n)) => duration.clamp(1, n - start),
            _ => duration.max(1),
        };
        let new_end = start + duration;
        if push {
            shift_after(track, c, old_end, new_end);
        }
        if let Some(clip) = track.clips.get_mut(c) {
            clip.duration = duration;
        }
        self.fit();
        true
    }

    /// Clip `id` moved to track `track` (on the same level), as near frame `start` as it fits
    /// without covering another clip. False if it can't go there.
    pub fn move_clip(&mut self, id: ClipId, track: TrackId, start: u32) -> bool {
        let Some((level, t, c)) = level_of_clip(&mut self.tracks, id) else { return false };
        let Some(to) = level.iter().position(|x| x.id == track) else { return false };
        let Some(clip) = level.get_mut(t).map(|tr| tr.clips.remove(c)) else { return false };
        let Some(dest) = level.get_mut(to) else { return false };
        let start = nearest_room(dest, start, clip.duration);
        let at = dest.clips.partition_point(|x| x.start < start);
        dest.clips.insert(at, Clip { start, ..clip });
        self.fit();
        true
    }

    /// Clip `id` cut at frame `at` (inside it): it ends there, and a clip of `root` (a copy of its
    /// drawing) takes the rest. Returns the new clip.
    pub fn split(&mut self, id: ClipId, at: u32, root: LayerId) -> Option<ClipId> {
        let next = self.next_clip;
        let (level, t, c) = level_of_clip(&mut self.tracks, id)?;
        let track = level.get_mut(t)?;
        let clip = track.clips.get_mut(c)?;
        if at <= clip.start || at >= clip.end() {
            return None;
        }
        let mut rest = clip.clone();
        clip.duration = at - clip.start;
        rest.id = ClipId(next);
        rest.start = at;
        rest.duration -= clip.duration;
        rest.content = ClipContent::Drawing(Drawing { root });
        // its keys go on from where it was cut
        rest.keys = shifted_keys(&rest.keys, i64::from(clip.duration));
        track.clips.insert(c + 1, rest);
        self.next_clip += 1;
        Some(ClipId(next))
    }

    /// A clip of `root` (a copy of clip `id`'s drawing) just after it, as long; the clips after
    /// it move along. Returns the copy.
    pub fn duplicate(&mut self, id: ClipId, root: LayerId) -> Option<ClipId> {
        let next = self.next_clip;
        let (level, t, c) = level_of_clip(&mut self.tracks, id)?;
        let track = level.get_mut(t)?;
        let mut copy = track.clips.get(c)?.clone();
        copy.id = ClipId(next);
        copy.start = copy.end();
        copy.content = ClipContent::Drawing(Drawing { root });
        make_room(track, copy.start, copy.end());
        track.clips.insert(c + 1, copy);
        self.next_clip += 1;
        self.fit();
        Some(ClipId(next))
    }

    /// Takes clip `id` off its track. `close`: the clips after it move back over its place.
    pub fn remove(&mut self, id: ClipId, close: bool) -> Option<Clip> {
        let (level, t, c) = level_of_clip(&mut self.tracks, id)?;
        let track = level.get_mut(t)?;
        let clip = track.clips.remove(c);
        if close {
            let gap = clip.duration;
            for later in track.clips.iter_mut().skip(c) {
                later.start -= gap;
            }
        }
        Some(clip)
    }

    /// Clips `ids`, all on tracks of one level, made into a group clip named `name` spanning them:
    /// each of their tracks becomes a track of the group's own (in the same order, named as it
    /// was), their frames counted from the group's start. The group goes on the topmost of their
    /// tracks, or on a new track just above it when it doesn't fit there. Returns the group.
    pub fn group(&mut self, ids: &[ClipId], name: &str) -> Option<ClipId> {
        let first = *ids.first()?;
        let group_id = ClipId(self.next_clip);
        let mut next_track = self.next_track;
        let (level, ..) = level_of_clip(&mut self.tracks, first)?;
        let found = level.iter().flat_map(|t| &t.clips).filter(|c| ids.contains(&c.id)).count();
        if found != ids.iter().collect::<std::collections::HashSet<_>>().len() {
            return None;
        }
        let members = || level.iter().flat_map(|t| &t.clips).filter(|c| ids.contains(&c.id));
        let start = members().map(|c| c.start).min()?;
        let end = members().map(Clip::end).max()?;
        let mut inner = Vec::new();
        let mut home = 0;
        for (t, track) in level.iter_mut().enumerate() {
            let (taken, kept): (Vec<Clip>, Vec<Clip>) =
                std::mem::take(&mut track.clips).into_iter().partition(|c| ids.contains(&c.id));
            track.clips = kept;
            if taken.is_empty() {
                continue;
            }
            home = t;
            let clips = taken.into_iter().map(|c| Clip { start: c.start - start, ..c }).collect();
            let id = TrackId(next_track);
            next_track += 1;
            inner.push(Track { id, name: track.name.clone(), hidden: false, locked: false, opacity: 1.0, clips });
        }
        let group = Clip {
            id: group_id,
            name: name.to_owned(),
            start,
            duration: end - start,
            opacity: 1.0,
            blend: BlendMode::Normal,
            mask: MaskMode::None,
            adjustment: false,
            disabled: false,
            label: None,
            keys: ClipKeys::default(),
            content: ClipContent::Group(Group { tracks: inner, collapsed: false, cycle: None }),
        };
        let fits = level.get(home).is_some_and(|t| t.clips.iter().all(|c| c.end() <= start || c.start >= end));
        if !fits {
            let id = TrackId(next_track);
            next_track += 1;
            home += 1;
            let track = Track { id, name: name.to_owned(), hidden: false, locked: false, opacity: 1.0, clips: vec![] };
            level.insert(home.min(level.len()), track);
        }
        let track = level.get_mut(home)?;
        let at = track.clips.partition_point(|c| c.start < start);
        track.clips.insert(at, group);
        self.next_track = next_track;
        self.next_clip += 1;
        Some(group_id)
    }

    /// Group clip `id` taken apart: its bottom track's clips go back onto its own track (where it
    /// was), and each of its other tracks goes just above that, their frames back on this level.
    /// Its own keys and cycle are let go. False if it isn't a group.
    pub fn ungroup(&mut self, id: ClipId) -> bool {
        let next = self.next_track;
        let Some((level, t, c)) = level_of_clip(&mut self.tracks, id) else { return false };
        let Some(track) = level.get_mut(t) else { return false };
        if !matches!(track.clips.get(c).map(|x| &x.content), Some(ClipContent::Group(_))) {
            return false;
        }
        let group = track.clips.remove(c);
        let ClipContent::Group(g) = group.content else { return false };
        let mut above = t + 1;
        let mut made = 0;
        for (k, mut inner) in g.tracks.into_iter().enumerate() {
            inner.clips.iter_mut().for_each(|x| x.start += group.start);
            let onto = level.get_mut(t).filter(|_| k == 0);
            let fits = onto.as_ref().is_some_and(|own| {
                inner.clips.iter().all(|x| own.clips.iter().all(|o| o.end() <= x.start || o.start >= x.end()))
            });
            match onto {
                Some(own) if fits => {
                    own.clips.extend(inner.clips);
                    own.clips.sort_by_key(|x| x.start);
                }
                _ => {
                    if k == 0 {
                        // the bottom track no longer fits where it was: a track of its own
                        inner.id = TrackId(next + made);
                        made += 1;
                    }
                    level.insert(above.min(level.len()), inner);
                    above += 1;
                }
            }
        }
        self.next_track += made;
        self.fit();
        true
    }

    /// As long as its clips at least.
    pub(crate) fn fit(&mut self) {
        let end = self.tracks.iter().flat_map(|t| t.clips.last()).map(Clip::end).max().unwrap_or(1);
        self.duration = self.duration.max(end);
    }

    /// The camera at frame `frame` (between frames too), its rotation eased the short way round
    /// from key to key; none without camera keys.
    pub fn camera_at(&self, frame: f32) -> Option<CameraPose> {
        let mut unwound = self.camera.clone();
        let mut before: Option<f32> = None;
        for key in &mut unwound.keys {
            if let Some(b) = before {
                let tau = std::f32::consts::TAU;
                key.value.rotation =
                    b + (key.value.rotation - b + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
            }
            before = Some(key.value.rotation);
        }
        unwound.value_at(frame).map(|p| CameraPose {
            zoom: p.zoom.clamp(CAMERA_ZOOM.0, CAMERA_ZOOM.1),
            rotation: p.rotation.rem_euclid(std::f32::consts::TAU),
            ..p
        })
    }

    /// Every drawing a frame draws from, bottom first: the plan's (mattes too) and, while drawing
    /// rather than `playing`, the onion skins about it. What has to be on the GPU to draw it.
    pub fn drawn(&self, frame: u32, playing: bool) -> Vec<LayerId> {
        let mut out = self.plan(frame).roots();
        for track in &self.tracks {
            let (before, after) = self.onions_on(track, frame, playing);
            out.extend(before.iter().chain(&after).map(|s| s.root));
        }
        out
    }

    /// The onion skins about `frame` on top-level `track`, before it and after it: only on the
    /// simple-mode track, shown, while drawing rather than `playing`.
    pub fn onions_on(&self, track: &Track, frame: u32, playing: bool) -> (Vec<Showing>, Vec<Showing>) {
        if playing || track.hidden || self.simple != Some(track.id) {
            return (Vec::new(), Vec::new());
        }
        self.onions(track, frame)
    }

    /// How many levels of group clips deep it goes (0 with none).
    pub fn group_depth(&self) -> usize {
        fn depth(tracks: &[Track]) -> usize {
            tracks
                .iter()
                .flat_map(|t| &t.clips)
                .map(|c| match &c.content {
                    ClipContent::Group(g) => 1 + depth(&g.tracks),
                    _ => 0,
                })
                .max()
                .unwrap_or(0)
        }
        depth(&self.tracks)
    }
}

/// `frame` of the level `groups` (outermost first) are on, on the level inside the last of them:
/// through each one's start and cycle.
fn into_groups(groups: &[&Clip], frame: i64) -> i64 {
    groups.iter().fold(frame, |f, g| {
        let local = f - i64::from(g.start);
        match &g.content {
            ClipContent::Group(Group { cycle: Some(c), .. }) if local >= 0 => local % i64::from(c.length.max(1)),
            _ => local,
        }
    })
}

/// The clips from the top of `tracks` down to clip `id` (the groups it is in, then itself), into
/// `path`; false, `path` as it was, if it isn't there.
fn clip_path<'a>(tracks: &'a [Track], id: ClipId, path: &mut Vec<&'a Clip>) -> bool {
    for clip in tracks.iter().flat_map(|t| &t.clips) {
        path.push(clip);
        if clip.id == id {
            return true;
        }
        if let ClipContent::Group(g) = &clip.content
            && clip_path(&g.tracks, id, path)
        {
            return true;
        }
        path.pop();
    }
    false
}

/// The group clips from the top of `tracks` down to track `id`, into `path`; false if it isn't
/// there.
fn track_path<'a>(tracks: &'a [Track], id: TrackId, path: &mut Vec<&'a Clip>) -> bool {
    if tracks.iter().any(|t| t.id == id) {
        return true;
    }
    for clip in tracks.iter().flat_map(|t| &t.clips) {
        if let ClipContent::Group(g) = &clip.content {
            path.push(clip);
            if track_path(&g.tracks, id, path) {
                return true;
            }
            path.pop();
        }
    }
    false
}

/// The tracks holding track `id`, among `tracks` or inside a group clip on them.
fn level_of_track(tracks: &mut Vec<Track>, id: TrackId) -> Option<&mut Vec<Track>> {
    if tracks.iter().any(|t| t.id == id) {
        return Some(tracks);
    }
    for clip in tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
        if let ClipContent::Group(g) = &mut clip.content
            && let Some(level) = level_of_track(&mut g.tracks, id)
        {
            return Some(level);
        }
    }
    None
}

/// The tracks holding clip `id` (among `tracks` or inside a group clip on them), its track's
/// index in them and its own.
pub(crate) fn level_of_clip(tracks: &mut Vec<Track>, id: ClipId) -> Option<(&mut Vec<Track>, usize, usize)> {
    let here =
        tracks.iter().enumerate().find_map(|(t, track)| track.clips.iter().position(|c| c.id == id).map(|c| (t, c)));
    if let Some((t, c)) = here {
        return Some((tracks, t, c));
    }
    for clip in tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
        if let ClipContent::Group(g) = &mut clip.content
            && let Some(found) = level_of_clip(&mut g.tracks, id)
        {
            return Some(found);
        }
    }
    None
}

/// The clips of `track` from frame `start` on move along, gaps kept, so that none starts before
/// `end`.
fn make_room(track: &mut Track, start: u32, end: u32) {
    let Some(first) = track.clips.iter().filter(|c| c.start >= start).map(|c| c.start).min() else { return };
    if first < end {
        let by = end - first;
        for c in track.clips.iter_mut().filter(|c| c.start >= start) {
            c.start += by;
        }
    }
}

/// The clips after index `c` move by the change of its end from `old_end` to `new_end`.
fn shift_after(track: &mut Track, c: usize, old_end: u32, new_end: u32) {
    for later in track.clips.iter_mut().skip(c + 1) {
        later.start = later.start + new_end - old_end;
    }
}

/// The start nearest `want` where `duration` frames fit on `track` among its clips.
fn nearest_room(track: &Track, want: u32, duration: u32) -> u32 {
    let mut best: Option<u32> = None;
    let mut from = 0;
    let mut consider = |lo: u32, hi: Option<u32>| {
        let fits = hi.is_none_or(|h| h >= lo + duration);
        if fits {
            let latest = hi.map_or(u32::MAX, |h| h - duration);
            let at = want.clamp(lo, latest.max(lo));
            if best.is_none_or(|b| at.abs_diff(want) < b.abs_diff(want)) {
                best = Some(at);
            }
        }
    };
    for c in &track.clips {
        consider(from, Some(c.start));
        from = c.end();
    }
    consider(from, None);
    best.unwrap_or(from)
}

/// `keys` with every key `by` frames earlier (a clip cut `by` frames in).
fn shifted_keys(keys: &ClipKeys, by: i64) -> ClipKeys {
    fn shift<T: Clone>(c: &Channel<T>, by: i64) -> Channel<T> {
        let keys = c
            .keys
            .iter()
            .map(|k| Key { frame: i32::try_from(i64::from(k.frame) - by).unwrap_or(i32::MIN), ..k.clone() })
            .collect();
        Channel { keys }
    }
    ClipKeys {
        position: shift(&keys.position, by),
        scale: shift(&keys.scale, by),
        rotation: shift(&keys.rotation, by),
        opacity: shift(&keys.opacity, by),
        pivot: keys.pivot,
        effects: keys.effects.iter().map(|(id, c)| (*id, shift(c, by))).collect(),
    }
}

/// What one frame shows: what the renderer draws, bottom first.
#[derive(Clone, Debug, PartialEq)]
pub struct FramePlan {
    pub items: Vec<PlanItem>,
}

impl FramePlan {
    /// The drawings shown, bottom first, groups opened (and drawings only a matte left out).
    pub fn drawings(&self) -> Vec<LayerId> {
        fn walk(items: &[PlanItem], out: &mut Vec<LayerId>) {
            for item in items.iter().filter(|i| !i.matte_only) {
                match &item.shows {
                    Shows::Drawing(id) => out.push(*id),
                    Shows::Group(inner) => walk(inner, out),
                    Shows::Video { .. } => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.items, &mut out);
        out
    }
}

impl FramePlan {
    /// Every drawing it draws from, bottom first: those shown and those only a matte.
    pub fn roots(&self) -> Vec<LayerId> {
        fn walk(items: &[PlanItem], out: &mut Vec<LayerId>) {
            for item in items {
                match &item.shows {
                    Shows::Drawing(id) => out.push(*id),
                    Shows::Group(inner) => walk(inner, out),
                    Shows::Video { .. } => {}
                }
            }
        }
        let mut out = Vec::new();
        walk(&self.items, &mut out);
        out
    }
}

/// One clip as a frame shows it.
#[derive(Clone, Debug, PartialEq)]
pub struct PlanItem {
    pub clip: ClipId,
    pub track: TrackId,
    pub shows: Shows,
    /// The clip's frame, counted from its start.
    pub local: u32,
    /// How its keys move, scale and turn it here; none without such keys (drawn in place).
    pub pose: Option<Affine>,
    /// The clip's opacity (keyed, if it has opacity keys) times its track's.
    pub opacity: f32,
    pub blend: BlendMode,
    /// How it is masked, and by which item before it in the same list (the clip on the track just
    /// below). None when it isn't, or when nothing is there to mask it.
    pub mask: Option<(MaskMode, usize)>,
    /// Only a matte for the item above: it doesn't draw of its own.
    pub matte_only: bool,
}

/// What a clip in a frame draws.
#[derive(Clone, Debug, PartialEq)]
pub enum Shows {
    /// A top-level layer or group, composited as one.
    Drawing(LayerId),
    /// A group's own tracks at its own frame, composited together and then as one.
    Group(Vec<PlanItem>),
    /// A video's picture at this time in it, seconds, drawn into `rect` (left, top, width, height
    /// on the canvas).
    Video { media: MediaId, seconds: f64, rect: [f32; 4] },
}

/// The clips of `tracks` at `frame`, bottom first, into `out`.
fn sample(tracks: &[Track], frame: u32, fps: u32, out: &mut Vec<PlanItem>) {
    // the item the track just below put in, a mask's source
    let mut below: Option<usize> = None;
    for track in tracks {
        let clip = if track.hidden { None } else { track.clip_at(frame).filter(|c| !c.disabled && !c.adjustment) };
        let Some(clip) = clip else {
            below = None;
            continue;
        };
        // covering `frame`, it starts at or before it
        let local = frame - clip.start;
        // a repeating group's own keys play again each cycle, unless they run on across
        let keyed = match &clip.content {
            ClipContent::Group(Group { cycle: Some(c), .. }) if c.keys == CycleKeys::Repeat => local % c.length.max(1),
            _ => local,
        };
        let shows = match &clip.content {
            ClipContent::Drawing(d) => Shows::Drawing(d.root),
            ClipContent::Group(g) => {
                let at = g.cycle.map_or(local, |c| local % c.length.max(1));
                let mut inner = Vec::new();
                sample(&g.tracks, at, fps, &mut inner);
                Shows::Group(inner)
            }
            ClipContent::Video(v) => Shows::Video { media: v.media, seconds: v.picture_at(local, fps), rect: v.rect },
            ClipContent::Audio(_) => {
                below = None;
                continue;
            }
        };
        let mask = match (clip.mask, below) {
            (MaskMode::None, _) | (_, None) => None,
            (mode, Some(source)) => {
                if mode.consumes_source()
                    && let Some(s) = out.get_mut(source)
                {
                    s.matte_only = true;
                }
                Some((mode, source))
            }
        };
        out.push(PlanItem {
            clip: clip.id,
            track: track.id,
            shows,
            local,
            pose: clip.keys.pose_at(keyed as f32),
            opacity: clip.keys.opacity_at(keyed as f32).unwrap_or(clip.opacity) * track.opacity,
            blend: clip.blend,
            mask,
            matte_only: false,
        });
        below = Some(out.len() - 1);
    }
}

/// Ids met while repairing, so none is used twice.
#[derive(Default)]
struct Seen {
    roots: std::collections::HashSet<LayerId>,
    tracks: std::collections::HashSet<TrackId>,
    clips: std::collections::HashSet<ClipId>,
    most_track: u64,
    most_clip: u64,
}

fn repair_tracks(tracks: &mut Vec<Track>, top: &[LayerId], seen: &mut Seen) {
    tracks.retain(|t| seen.tracks.insert(t.id));
    for track in tracks.iter_mut() {
        seen.most_track = seen.most_track.max(track.id.0);
        track.opacity = if track.opacity.is_finite() { track.opacity.clamp(0.0, 1.0) } else { 1.0 };
        track.clips.retain_mut(|clip| {
            if !seen.clips.insert(clip.id) {
                return false;
            }
            seen.most_clip = seen.most_clip.max(clip.id.0);
            clip.duration = clip.duration.max(1);
            clip.opacity = if clip.opacity.is_finite() { clip.opacity.clamp(0.0, 1.0) } else { 1.0 };
            for c in [&mut clip.keys.position, &mut clip.keys.scale] {
                tidy(c);
            }
            tidy(&mut clip.keys.rotation);
            tidy(&mut clip.keys.opacity);
            clip.keys.effects.values_mut().for_each(tidy);
            match &mut clip.content {
                ClipContent::Drawing(d) => top.contains(&d.root) && seen.roots.insert(d.root),
                ClipContent::Group(g) => {
                    repair_tracks(&mut g.tracks, top, seen);
                    true
                }
                ClipContent::Audio(a) => {
                    tidy(&mut a.volume_keys);
                    a.repair();
                    true
                }
                ClipContent::Video(v) => {
                    v.repair();
                    true
                }
            }
        });
        track.clips.sort_by_key(|c| c.start);
        let mut end = 0;
        track.clips.retain(|c| {
            let keep = c.start >= end;
            if keep {
                end = c.end();
            }
            keep
        });
    }
}

/// A channel's keys in frame order, one a frame.
fn tidy<T>(channel: &mut Channel<T>) {
    channel.keys.sort_by_key(|k| k.frame);
    channel.keys.dedup_by_key(|k| k.frame);
}

/// A value keys hold, eased from key to key.
pub trait Keyed: Copy {
    fn plus(self, other: Self) -> Self;
    fn times(self, s: f32) -> Self;
}

impl Keyed for f32 {
    fn plus(self, other: f32) -> f32 {
        self + other
    }
    fn times(self, s: f32) -> f32 {
        self * s
    }
}

impl Keyed for [f32; 2] {
    fn plus(self, other: [f32; 2]) -> [f32; 2] {
        [self[0] + other[0], self[1] + other[1]]
    }
    fn times(self, s: f32) -> [f32; 2] {
        [self[0] * s, self[1] * s]
    }
}

impl Keyed for CameraPose {
    fn plus(self, other: CameraPose) -> CameraPose {
        CameraPose {
            center: self.center.plus(other.center),
            zoom: self.zoom + other.zoom,
            rotation: self.rotation + other.rotation,
        }
    }
    fn times(self, s: f32) -> CameraPose {
        CameraPose { center: self.center.times(s), zoom: self.zoom * s, rotation: self.rotation * s }
    }
}

fn lerp<T: Keyed>(a: T, b: T, t: f32) -> T {
    a.times(1.0 - t).plus(b.times(t))
}

impl Easing {
    /// How far along the way to the next key (0 to 1, past either for an overshooting easing) a
    /// share `t` (0 to 1) of the time between them is.
    pub fn progress(&self, t: f32) -> f32 {
        let t = if t.is_finite() { t.clamp(0.0, 1.0) } else { 0.0 };
        match self {
            // a smooth segment's curve needs the keys either side: the channel draws it
            Easing::Linear | Easing::Smooth => t,
            Easing::Hold => 0.0,
            Easing::CubicBezier(c1, c2) => cubic_bezier(*c1, *c2, t),
            Easing::Curve(points) => drawn_curve(points, t),
            Easing::Bounce { first_step, bounciness } => bounce(*first_step, *bounciness, t),
            Easing::Elastic { step, attack, decay, magnitude } => elastic(*step, *attack, *decay, *magnitude, t),
            Easing::Steps { step, smoothing } => steps(*step, *smoothing, t),
            Easing::ElasticSteps { step, magnitude } => elastic_steps(*step, *magnitude, t),
            Easing::Cyclic { step, sharpness, skew, decay } => cyclic(*step, *sharpness, *skew, *decay, t),
            Easing::Random { frequency, magnitude, seed, attenuation, warp, tilt } => {
                random(*frequency, *magnitude, *seed, *attenuation, *warp, *tilt, t)
            }
            Easing::Fitted(base, fit) => {
                // the base played back and forth across the span, corrected so it still leaves
                // from 0 and arrives at 1
                let local = fit.local(t);
                let blend = local * local * (3.0 - 2.0 * local);
                let start = -base.progress(0.0) * (1.0 - blend);
                let end = (1.0 - base.progress(1.0)) * blend;
                base.progress(local) + start + end
            }
            Easing::Reversed(e) => 1.0 - e.progress(1.0 - t),
        }
    }
}

impl RepeatFit {
    /// How many legs it plays: there and back counted apart.
    pub fn legs(&self) -> u32 {
        let reps = self.repetitions.clamp(1, 64);
        match self.mode {
            FitMode::Arrive => reps * 2 - 1,
            FitMode::Loop => reps * 2,
        }
    }

    /// Where in its base easing it is at progress `t`: each leg eased in and out, every other one
    /// backwards.
    fn local(&self, t: f32) -> f32 {
        let t = t.clamp(0.0, 1.0);
        if t >= 1.0 {
            return if self.mode == FitMode::Arrive { 1.0 } else { 0.0 };
        }
        let legs = self.legs();
        let scaled = t * legs as f32;
        let leg = (scaled.floor() as u32).min(legs - 1);
        let f = scaled - leg as f32;
        let smooth = f * f * (3.0 - 2.0 * f);
        if leg.is_multiple_of(2) { smooth } else { 1.0 - smooth }
    }
}

/// A drawn easing: a natural cubic spline through `points` (x 0 to 1, pinned to (0,0) and (1,1)
/// at its ends; y from -0.5 to 1.5, so it can dip before leaving or shoot past arriving).
fn drawn_curve(points: &[[f32; 2]], t: f32) -> f32 {
    let mut pts: Vec<[f32; 2]> = points.iter().map(|p| [p[0].clamp(0.0, 1.0), p[1].clamp(-0.5, 1.5)]).collect();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    if pts.len() < 2 {
        return t;
    }
    if let Some(first) = pts.first_mut() {
        *first = [0.0, 0.0];
    }
    if let Some(last) = pts.last_mut() {
        *last = [1.0, 1.0];
    }
    natural_spline(&pts, t).clamp(-0.5, 1.5)
}

/// The natural cubic spline through `pts` (sorted by x, at least two) at `x`: through every point,
/// flat outside the first and last.
fn natural_spline(pts: &[[f32; 2]], x: f32) -> f32 {
    let n = pts.len();
    let (Some(first), Some(last)) = (pts.first(), pts.last()) else { return x };
    if x <= first[0] {
        return first[1];
    }
    if x >= last[0] {
        return last[1];
    }
    let at = |i: usize| pts.get(i).copied().unwrap_or([0.0; 2]);
    let h: Vec<f32> = (0..n - 1).map(|i| (at(i + 1)[0] - at(i)[0]).max(1e-4)).collect();
    let hi = |i: usize| h.get(i).copied().unwrap_or(1e-4);
    // second derivatives, zero at both ends (Thomas's algorithm)
    let (mut c, mut d, mut m) = (vec![0.0_f32; n], vec![0.0_f32; n], vec![0.0_f32; n]);
    for i in 1..n - 1 {
        let below = hi(i - 1);
        let diagonal = 2.0 * (hi(i - 1) + hi(i));
        let rhs = 6.0 * ((at(i + 1)[1] - at(i)[1]) / hi(i) - (at(i)[1] - at(i - 1)[1]) / hi(i - 1));
        let pivot = diagonal - below * c.get(i - 1).copied().unwrap_or(0.0);
        if let Some(ci) = c.get_mut(i) {
            *ci = hi(i) / pivot;
        }
        let prev_d = d.get(i - 1).copied().unwrap_or(0.0);
        if let Some(di) = d.get_mut(i) {
            *di = (rhs - below * prev_d) / pivot;
        }
    }
    for i in (1..n - 1).rev() {
        let next = m.get(i + 1).copied().unwrap_or(0.0);
        if let Some(mi) = m.get_mut(i) {
            *mi = d.get(i).copied().unwrap_or(0.0) - c.get(i).copied().unwrap_or(0.0) * next;
        }
    }
    let mut s = 0;
    while s < n - 2 && x > at(s + 1)[0] {
        s += 1;
    }
    let w = hi(s);
    let a = (at(s + 1)[0] - x) / w;
    let b = (x - at(s)[0]) / w;
    let (ms, ms1) = (m.get(s).copied().unwrap_or(0.0), m.get(s + 1).copied().unwrap_or(0.0));
    a * at(s)[1] + b * at(s + 1)[1] + ((a * a * a - a) * ms + (b * b * b - b) * ms1) * w * w / 6.0
}

/// Falls to the end and bounces there, each bounce `bounciness` of the last, the first
/// `first_step` long.
fn bounce(first_step: f32, bounciness: f32, t: f32) -> f32 {
    let first = first_step.clamp(0.01, 1.0);
    let b = bounciness.clamp(0.01, 0.99);
    let mut length = first;
    let shifted = t + first * 0.5;
    let mut start = 0.0;
    let mut amplitude = 1.0;
    for _ in 0..256 {
        let end = start + length;
        if shifted <= end {
            let settles = start + length / 3.0;
            if settles > first * 0.5 + 1.0 || (length < 0.1 && end > first * 0.5 + 1.0) {
                return 1.0;
            }
            let from_impact = (((shifted - start) / length - 0.5) * 2.0).abs();
            return from_impact * from_impact * amplitude + (1.0 - amplitude);
        }
        start = end;
        length *= b;
        amplitude *= b;
        if amplitude < 0.005 {
            return 1.0;
        }
    }
    1.0
}

/// Springs past the end and settles: waves `step` long, `decay` how fast they die down,
/// `magnitude` how far they swing, `attack` how sharply it sets off.
fn elastic(step: f32, attack: f32, decay: f32, magnitude: f32, t: f32) -> f32 {
    let wavelength = step.clamp(0.01, 1.0);
    let decay = decay.clamp(0.0, 1.0);
    let magnitude = magnitude.clamp(0.0, 1.0);
    let exponent = decay * decay * 15.0 + 1.0;
    let wave = |x: f32| (std::f32::consts::PI * x / wavelength).cos() * (1.0 - x.max(0.005)).abs().powf(exponent);
    let arrival = 1.0 - wave(t) * magnitude;
    if t >= wavelength {
        return arrival;
    }
    let phase = t / wavelength;
    let gentle = (1.0 - (std::f32::consts::PI * phase).cos()) * 0.5 * (1.0 - wave(wavelength) * magnitude);
    let blend = phase.powf((attack.clamp(0.0, 2.0) * 3.0).max(1.0));
    gentle + (arrival - gentle) * blend
}

/// In steps `step` long, each edge eased by `smoothing`.
fn steps(step: f32, smoothing: f32, t: f32) -> f32 {
    if t >= 1.0 {
        return 1.0;
    }
    let length = step.max(0.01);
    let remainder = t % length;
    let base = t - remainder;
    let smoothing = smoothing.clamp(0.0, 1.0);
    if smoothing <= 0.0 {
        return base;
    }
    let edge = length * (1.0 - smoothing);
    let x = ((remainder - edge) / (length - edge).max(1e-4)).clamp(0.0, 1.0);
    (base + x * x * (3.0 - 2.0 * x) * length).min(1.0)
}

/// In steps `step` long, each landing with a springy `magnitude`.
fn elastic_steps(step: f32, magnitude: f32, t: f32) -> f32 {
    if t >= 1.0 {
        return 1.0;
    }
    let length = step.clamp(0.01, 1.0);
    let magnitude = magnitude.clamp(0.0, 1.0);
    if t < length {
        return 0.0;
    }
    let remainder = t % length;
    let transition =
        elastic(0.5 - 0.45 * magnitude, 1.0, 1.0 - 0.5 * magnitude, 0.5 + 0.5 * (1.0 - magnitude), remainder / length);
    (t - remainder + length * (transition - 1.0)).clamp(-1.0, 2.0)
}

/// Back and forth in cycles `step` long (`skew` where in each it turns, `sharpness` from round to
/// pointed), drifting to the end by `decay`.
fn cyclic(step: f32, sharpness: f32, skew: f32, decay: f32, t: f32) -> f32 {
    let length = step.clamp(0.01, 1.0);
    let skew = skew.clamp(0.01, 0.99);
    let sharpness = sharpness.clamp(0.0, 1.0);
    let decay = decay.clamp(0.0, 1.0);
    let remainder = t % length;
    let phase = remainder / length;
    let skewed = if phase < skew { 0.5 * phase / skew } else { 0.5 + 0.5 * (phase - skew) / (1.0 - skew) };
    let cosine = 0.5 - 0.5 * (skewed * 2.0 * std::f32::consts::PI).cos();
    let triangle = 1.0 - (skewed - 0.5).abs() * 2.0;
    let mut wave = cosine + (triangle - cosine) * sharpness;
    let cycle_start = t - remainder;
    if cycle_start + length * 0.25 > 1.0 {
        wave = 0.0;
    } else if cycle_start + length * 0.5 < 1.0 && cycle_start + length * 0.75 > 1.0 && phase > skew {
        wave = 1.0;
    }
    wave * (1.0 - t * decay) + t * decay
}

/// Wanders on the way, as noise `frequency` fast and `magnitude` far (by `seed`), fading toward
/// one end by `attenuation`, its timing warped by `warp`, tilted toward going back by `tilt`.
fn random(frequency: f32, magnitude: f32, seed: f32, attenuation: f32, warp: f32, tilt: f32, t: f32) -> f32 {
    let warp = warp.clamp(0.0, 1.0);
    let frequency = frequency.clamp(0.0, 1.0);
    let magnitude = magnitude.clamp(0.0, 2.0);
    let attenuation = attenuation.clamp(0.0, 1.0);
    let tilt = tilt.clamp(0.0, 1.0);
    let seed = seed.clamp(0.0, 1.0);
    let strength = (warp - 0.5).abs() * 10.0 + 1.0;
    let warped = if warp < 0.5 { t.powf(strength) } else { 1.0 - (1.0 - t).powf(strength) };
    let bits = u64::from(seed.to_bits());
    let first = value_noise(warped * frequency.max(0.002) * 30.0, bits);
    let second = value_noise(warped * frequency.max(0.002) * 60.0, bits.wrapping_add(0x9E37_79B9));
    let noise = first + second * 0.5;
    let centred = (t * 2.0 - 1.0).abs();
    let envelope = 1.0 - centred * centred * centred;
    let scale = 1.0 - (1.0 - magnitude).abs();
    let fade_strength = (attenuation - 0.5).abs() * 2.0;
    let target = if attenuation > 0.5 { 1.0 - t } else { t };
    let fade = 1.0 + (target - 1.0) * fade_strength;
    let baseline = warped + ((1.0 - warped) - warped) * tilt;
    baseline + noise * envelope * scale * fade * fade
}

fn value_noise(x: f32, seed: u64) -> f32 {
    let lower = x.floor() as i64;
    let f = x - lower as f32;
    let blend = f * f * (3.0 - 2.0 * f);
    let (a, b) = (noise_hash(lower, seed), noise_hash(lower + 1, seed));
    a + (b - a) * blend
}

fn noise_hash(at: i64, seed: u64) -> f32 {
    let mut bits = (at as u64).wrapping_add(seed).wrapping_add(0x9E37_79B9_7F4A_7C15);
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    bits ^= bits >> 31;
    (bits & 0x00FF_FFFF) as f32 / 0x00FF_FFFF as f32 * 2.0 - 1.0
}

/// A CSS-style cubic Bézier easing from (0,0) to (1,1) through control points `c1` and `c2`: its
/// height where it is `x` along.
fn cubic_bezier(c1: [f32; 2], c2: [f32; 2], x: f32) -> f32 {
    let curve = |a: f32, b: f32, u: f32| {
        let v = 1.0 - u;
        3.0 * v * v * u * a + 3.0 * v * u * u * b + u * u * u
    };
    let slope = |a: f32, b: f32, u: f32| {
        let v = 1.0 - u;
        3.0 * v * v * a + 6.0 * v * u * (b - a) + 3.0 * u * u * (1.0 - b)
    };
    let (x1, x2) = (c1[0].clamp(0.0, 1.0), c2[0].clamp(0.0, 1.0));
    // where along the curve it is that far across: Newton's way, then halving if that stalls
    let mut u = x;
    for _ in 0..8 {
        let error = curve(x1, x2, u) - x;
        let s = slope(x1, x2, u);
        if error.abs() < 1e-6 || s.abs() < 1e-6 {
            break;
        }
        u = (u - error / s).clamp(0.0, 1.0);
    }
    if (curve(x1, x2, u) - x).abs() > 1e-4 {
        let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
        for _ in 0..32 {
            u = 0.5 * (lo + hi);
            if curve(x1, x2, u) < x {
                lo = u;
            } else {
                hi = u;
            }
        }
    }
    curve(c1[1], c2[1], u)
}

impl<T> Channel<T> {
    /// A key at `frame` (the one there replaced) with `value`, easing into the next as `easing`.
    pub fn set(&mut self, frame: i32, value: T, easing: Easing) {
        let at = self.keys.partition_point(|k| k.frame < frame);
        let key = Key { frame, value, easing, path: None };
        match self.keys.get_mut(at) {
            Some(k) if k.frame == frame => *k = Key { path: k.path, ..key },
            _ => self.keys.insert(at, key),
        }
    }

    /// Takes away the key at `frame`; false if there is none.
    pub fn remove(&mut self, frame: i32) -> bool {
        let before = self.keys.len();
        self.keys.retain(|k| k.frame != frame);
        self.keys.len() != before
    }
}

impl<T: Keyed> Channel<T> {
    /// Its value at clip-relative frame `frame` (between frames too): the first key's before it,
    /// the last's after it, and between two keys as the first one eases into the next. None
    /// without keys.
    pub fn value_at(&self, frame: f32) -> Option<T> {
        let first = self.keys.first()?;
        let last = self.keys.last()?;
        if frame <= first.frame as f32 {
            return Some(first.value);
        }
        if frame >= last.frame as f32 {
            return Some(last.value);
        }
        let next = self.keys.partition_point(|k| k.frame as f32 <= frame);
        let (k0, k1) = (self.keys.get(next.checked_sub(1)?)?, self.keys.get(next)?);
        let span = (k1.frame - k0.frame).max(1) as f32;
        let t = ((frame - k0.frame as f32) / span).clamp(0.0, 1.0);
        Some(match &k0.easing {
            Easing::Hold => k0.value,
            Easing::Smooth => self.smooth(frame, next, k0, k1),
            easing => lerp(k0.value, k1.value, easing.progress(t)),
        })
    }

    /// A smooth segment, from key `next - 1` to key `next`, at `frame`: a Catmull-Rom curve through
    /// them and the keys either side, timed by their frames (Barry and Goldman's way). A key
    /// across a hold isn't one of them, so the curve comes in level instead of bending toward a
    /// jump.
    fn smooth(&self, frame: f32, next: usize, k0: &Key<T>, k1: &Key<T>) -> T {
        let before = next.checked_sub(2).and_then(|i| self.keys.get(i)).filter(|k| k.easing != Easing::Hold);
        let after = self.keys.get(next + 1).filter(|_| k1.easing != Easing::Hold);
        let (t1, t2) = (k0.frame as f32, k1.frame as f32);
        let t0 = before.map_or(t1 - (t2 - t1), |k| k.frame as f32);
        let t3 = after.map_or(t2 + (t2 - t1), |k| k.frame as f32);
        let p0 = before.map_or(k0.value, |k| k.value);
        let p3 = after.map_or(k1.value, |k| k.value);
        let f = |a: f32, b: f32| if b > a { (frame - a) / (b - a) } else { 0.0 };
        let a1 = lerp(p0, k0.value, f(t0, t1));
        let a2 = lerp(k0.value, k1.value, f(t1, t2));
        let a3 = lerp(k1.value, p3, f(t2, t3));
        let b1 = lerp(a1, a2, f(t0, t2));
        let b2 = lerp(a2, a3, f(t1, t3));
        lerp(b1, b2, f(t1, t2))
    }
}

impl Channel<[f32; 2]> {
    /// A position channel's value at `frame`: as [`Channel::value_at`], but where either key has
    /// motion path handles, along the Bézier curve they make, its easing measuring the distance
    /// travelled along it (the handles bend the path, the easing the timing); past the curve's
    /// ends (an overshoot), on along its direction there.
    pub fn position_at(&self, frame: f32) -> Option<[f32; 2]> {
        let first = self.keys.first()?;
        let last = self.keys.last()?;
        if frame <= first.frame as f32 || frame >= last.frame as f32 || self.keys.iter().all(|k| k.path.is_none()) {
            return self.value_at(frame);
        }
        let next = self.keys.partition_point(|k| k.frame as f32 <= frame);
        let (k0, k1) = (self.keys.get(next.checked_sub(1)?)?, self.keys.get(next)?);
        if k0.easing == Easing::Hold || (k0.path.is_none() && k1.path.is_none()) {
            return self.value_at(frame);
        }
        let span = (k1.frame - k0.frame).max(1) as f32;
        let eased = k0.easing.progress(((frame - k0.frame as f32) / span).clamp(0.0, 1.0));
        let (p0, p3) = (k0.value, k1.value);
        let p1 = k0.path.map_or(p0, |h| p0.plus(h.after));
        let p2 = k1.path.map_or(p3, |h| p3.plus(h.before));
        Some(along_bezier([p0, p1, p2, p3], eased))
    }
}

/// The point a share `eased` of the way along the cubic Bézier `p` by distance travelled (32
/// pieces measured); before its start or past its end, on along its direction there.
fn along_bezier(p: [[f32; 2]; 4], eased: f32) -> [f32; 2] {
    const PIECES: usize = 32;
    let point = |u: f32| {
        let v = 1.0 - u;
        let w = [v * v * v, 3.0 * v * v * u, 3.0 * v * u * u, u * u * u];
        [w.iter().zip(&p).map(|(w, q)| w * q[0]).sum::<f32>(), w.iter().zip(&p).map(|(w, q)| w * q[1]).sum::<f32>()]
    };
    let samples: Vec<[f32; 2]> = (0..=PIECES).map(|i| point(i as f32 / PIECES as f32)).collect();
    let mut lengths = [0.0_f32; PIECES + 1];
    for i in 1..=PIECES {
        let (a, b) = (samples.get(i - 1).copied().unwrap_or_default(), samples.get(i).copied().unwrap_or_default());
        let before = lengths.get(i - 1).copied().unwrap_or(0.0);
        if let Some(l) = lengths.get_mut(i) {
            *l = before + (b[0] - a[0]).hypot(b[1] - a[1]);
        }
    }
    let total = lengths.last().copied().unwrap_or(0.0);
    let [start, end] = [p[0], p[3]];
    if total <= 1e-4 {
        return start;
    }
    let beyond = |from: [f32; 2], away: [f32; 2], distance: f32| {
        let (dx, dy) = (from[0] - away[0], from[1] - away[1]);
        let len = dx.hypot(dy);
        if len <= 1e-6 { from } else { [from[0] + dx / len * distance, from[1] + dy / len * distance] }
    };
    if eased <= 0.0 {
        return beyond(start, samples.get(1).copied().unwrap_or(end), -eased * total);
    }
    if eased >= 1.0 {
        return beyond(end, samples.get(PIECES - 1).copied().unwrap_or(start), (eased - 1.0) * total);
    }
    let target = eased * total;
    let mut i = 1;
    while i < PIECES && lengths.get(i).copied().unwrap_or(total) < target {
        i += 1;
    }
    let (l0, l1) = (lengths.get(i - 1).copied().unwrap_or(0.0), lengths.get(i).copied().unwrap_or(total));
    let f = if l1 - l0 > 1e-4 { (target - l0) / (l1 - l0) } else { 0.0 };
    point(((i - 1) as f32 + f) / PIECES as f32)
}

impl ClipKeys {
    /// How the keys move, scale and turn the clip at clip-relative frame `frame`: scaled and
    /// turned about the pivot, then moved. None without such keys.
    pub fn pose_at(&self, frame: f32) -> Option<Affine> {
        if !self.posed() {
            return None;
        }
        let [px, py] = self.pivot.unwrap_or([0.0; 2]);
        let [tx, ty] = self.position.position_at(frame).unwrap_or([0.0; 2]);
        let [sx, sy] = self.scale.value_at(frame).unwrap_or([1.0; 2]);
        let turn = self.rotation.value_at(frame).unwrap_or(0.0);
        let to_pivot = Affine::translate(Vec2::new(-px, -py));
        let scale = Affine { a: sx, d: sy, ..Affine::IDENTITY };
        let back = Affine::translate(Vec2::new(px + tx, py + ty));
        Some(to_pivot.then(scale).then(Affine::rotate(turn)).then(back))
    }

    /// The clip's opacity at clip-relative frame `frame` (0 to 1, whatever an easing overshoots),
    /// if it has opacity keys.
    pub fn opacity_at(&self, frame: f32) -> Option<f32> {
        self.opacity.value_at(frame).map(|o| if o.is_finite() { o.clamp(0.0, 1.0) } else { 1.0 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawing(id: u64, start: u32, duration: u32, root: u64) -> Clip {
        Clip {
            id: ClipId(id),
            name: format!("Drawing {id}"),
            start,
            duration,
            opacity: 1.0,
            blend: BlendMode::Normal,
            mask: MaskMode::None,
            adjustment: false,
            disabled: false,
            label: None,
            keys: ClipKeys::default(),
            content: ClipContent::Drawing(Drawing { root: LayerId(root) }),
        }
    }

    fn track(clips: Vec<Clip>) -> Track {
        Track { id: TrackId(1), name: "Lines".into(), hidden: false, locked: false, opacity: 1.0, clips }
    }

    #[test]
    fn finds_the_clip_at_a_frame() {
        let t = track(vec![drawing(1, 0, 2, 10), drawing(2, 2, 3, 11), drawing(3, 8, 1, 12)]);
        assert!(t.is_ordered());
        let at = |f| t.clip_at(f).map(|c| c.id.0);
        assert_eq!(
            [at(0), at(1), at(2), at(4), at(5), at(7), at(8), at(9)],
            [Some(1), Some(1), Some(2), Some(2), None, None, Some(3), None]
        );
    }

    #[test]
    fn overlapping_clips_are_out_of_order() {
        assert!(!track(vec![drawing(1, 0, 3, 10), drawing(2, 2, 3, 11)]).is_ordered());
    }

    #[test]
    fn keys_around_a_frame() {
        let key = |frame, value| Key { frame, value, easing: Easing::Linear, path: None };
        let c = Channel { keys: vec![key(0, 0.0_f32), key(6, 1.0), key(12, 0.5)] };
        let (a, b) = c.around(7);
        assert_eq!((a.map(|k| k.frame), b.map(|k| k.frame)), (Some(6), Some(12)));
        let (a, b) = c.around(-1);
        assert_eq!((a.map(|k| k.frame), b.map(|k| k.frame)), (None, Some(0)));
        let (a, b) = c.around(12);
        assert_eq!((a.map(|k| k.frame), b.map(|k| k.frame)), (Some(12), None));
    }

    #[test]
    fn keeps_through_json() {
        let mut tl = Timeline::new(24, 48);
        tl.exposure = 2;
        tl.simple = Some(TrackId(1));
        let mut moving = drawing(2, 4, 8, 11);
        moving.keys.position.keys.push(Key {
            frame: 0,
            value: [0.0, 0.0],
            easing: Easing::Fitted(
                Box::new(Easing::Bounce { first_step: 0.3, bounciness: 0.5 }),
                RepeatFit { mode: FitMode::Loop, repetitions: 2 },
            ),
            path: Some(PathHandles { mode: HandleMode::Symmetric, before: [-4.0, 0.0], after: [4.0, 0.0] }),
        });
        let group = Clip {
            content: ClipContent::Group(Group {
                tracks: vec![track(vec![drawing(3, 0, 2, 12)])],
                collapsed: false,
                cycle: Some(Cycle { length: 2, keys: CycleKeys::Repeat }),
            }),
            ..drawing(4, 12, 8, 0)
        };
        tl.tracks.push(track(vec![drawing(1, 0, 4, 10), moving, group]));
        let json = serde_json::to_string(&tl).unwrap();
        let back: Timeline = serde_json::from_str(&json).unwrap();
        assert_eq!(back, tl);
        // a timeline saved before exposure and simple mode existed reads as on ones, full view
        let mut older: serde_json::Value = serde_json::from_str(&json).unwrap();
        older.as_object_mut().unwrap().remove("exposure");
        older.as_object_mut().unwrap().remove("simple");
        let older: Timeline = serde_json::from_value(older).unwrap();
        assert_eq!((older.exposure, older.simple), (1, None));
        let mut roots = back.drawing_roots();
        roots.sort();
        assert_eq!(roots, [LayerId(10), LayerId(11), LayerId(12)]);
    }

    /// A canvas of `n` top-level layers, bottom first.
    fn canvas(n: usize) -> (Document, Vec<LayerId>) {
        let mut doc = Document::new(8, 8);
        let mut ids = vec![doc.current()];
        for _ in 1..n {
            ids.push(doc.add_layer());
        }
        (doc, ids)
    }

    /// What Animation Assist shows at each place in its playing sequence, bottom first.
    fn assist_shows(doc: &Document) -> Vec<Vec<LayerId>> {
        let a = doc.animation.as_ref().unwrap();
        let frames = animation::frames(doc);
        let sequence: Vec<usize> =
            (0..frames.len()).flat_map(|i| std::iter::repeat_n(i, 1 + a.hold(frames[i]) as usize)).collect();
        sequence
            .iter()
            .map(|current| {
                (0..frames.len())
                    .filter(|i| animation::look(a, *i, frames.len(), *current, true) == animation::FrameLook::Shown)
                    .map(|i| frames[i])
                    .collect()
            })
            .collect()
    }

    /// Converts `doc` and checks every frame shows what Animation Assist showed there.
    fn plays_the_same(mut doc: Document) -> Timeline {
        let expected = assist_shows(&doc);
        let a = doc.animation.clone().unwrap();
        // drawing on each frame: what Animation Assist shows (with its onion skins), bottom first
        let frames = animation::frames(&doc);
        let drawing_on: Vec<Vec<(LayerId, animation::FrameLook)>> = (0..frames.len())
            .map(|current| {
                (0..frames.len())
                    .map(|i| (frames[i], animation::look(&a, i, frames.len(), current, false)))
                    .filter(|(_, look)| *look != animation::FrameLook::Hidden)
                    .collect()
            })
            .collect();
        assert!(doc.start_timeline());
        // the same, drawing at the start of each frame's place in the sequence
        let tl = doc.timeline.clone().unwrap();
        let mut at = 0;
        for (i, shown) in drawing_on.iter().enumerate() {
            let looks: Vec<(LayerId, animation::FrameLook)> =
                tl.showing(at, false).iter().map(|s| (s.root, s.look)).collect();
            assert_eq!(&looks, shown, "drawing on frame {i}");
            at += 1 + a.hold(frames[i]);
        }
        // and playing, only what plays
        for (frame, shown) in expected.iter().enumerate() {
            let roots: Vec<LayerId> = tl.showing(frame as u32, true).iter().map(|s| s.root).collect();
            assert_eq!(&roots, shown, "playing frame {frame}");
        }
        assert!(doc.animation.is_none());
        let tl = doc.timeline.clone().unwrap();
        assert_eq!(tl.duration as usize, expected.len().max(1));
        assert_eq!((tl.fps, tl.playback), (a.fps, a.playback));
        for (frame, shown) in expected.iter().enumerate() {
            assert_eq!(&tl.plan(frame as u32).drawings(), shown, "frame {frame}");
        }
        assert!(tl.tracks.iter().all(Track::is_ordered));
        // nothing is lost, and repairing it changes nothing
        let mut roots = tl.drawing_roots();
        roots.sort();
        let mut top = doc.members(None);
        top.sort();
        assert_eq!(roots, top);
        assert_eq!(tl.clone().repaired(&doc.members(None)), tl);
        tl
    }

    #[test]
    fn animation_assist_frames_play_the_same() {
        let (mut doc, ids) = canvas(5);
        doc.animation = Some(animation::Animation { fps: 8, ..Default::default() });
        let tl = plays_the_same(doc.clone());
        assert_eq!(tl.tracks.len(), 1);
        assert_eq!(tl.simple, Some(tl.tracks[0].id));

        // held frames last longer
        let a = doc.animation.as_mut().unwrap();
        a.set_hold(ids[1], 2);
        a.set_hold(ids[4], 3);
        let tl = plays_the_same(doc.clone());
        assert_eq!(tl.duration, 10);
        let lengths: Vec<u32> = tl.tracks[0].clips.iter().map(|c| c.duration).collect();
        assert_eq!(lengths, [1, 3, 1, 1, 4]);

        // a background and a foreground show all along, on tracks below and above
        for (background, foreground) in [(true, false), (false, true), (true, true)] {
            let mut d = doc.clone();
            let a = d.animation.as_mut().unwrap();
            (a.background, a.foreground) = (background, foreground);
            let tl = plays_the_same(d);
            let names: Vec<&str> = tl.tracks.iter().map(|t| t.name.as_str()).collect();
            let expected: Vec<&str> =
                [background.then_some("Background"), Some("Animation"), foreground.then_some("Foreground")]
                    .into_iter()
                    .flatten()
                    .collect();
            assert_eq!(names, expected);
        }

        // a hidden layer isn't a frame: it keeps a hidden track of its own
        let mut d = doc.clone();
        d.set_visible(ids[2], false);
        let tl = plays_the_same(d);
        let hidden: Vec<&Track> = tl.tracks.iter().filter(|t| t.hidden).collect();
        assert_eq!(hidden.len(), 1);
        assert_eq!(hidden[0].clips[0].duration, tl.duration);

        // one frame, the background too
        let (mut one, _) = canvas(1);
        one.animation = Some(animation::Animation { background: true, foreground: true, ..Default::default() });
        plays_the_same(one);
    }

    #[test]
    fn page_assist_and_timelines_stay_as_they_are() {
        let (mut doc, _) = canvas(3);
        doc.animation = Some(animation::Animation { pages: true, ..Default::default() });
        assert!(!doc.start_timeline());
        assert!(doc.timeline.is_none() && doc.animation.is_some());
        doc.animation = None;
        assert!(doc.start_timeline());
        assert!(!doc.start_timeline());
    }

    #[test]
    fn a_still_canvas_is_one_frame_of_every_layer() {
        let (mut doc, ids) = canvas(3);
        doc.set_visible(ids[1], false);
        assert!(doc.start_timeline());
        let tl = doc.timeline.as_ref().unwrap();
        assert_eq!(tl.duration, 1);
        assert_eq!(tl.simple, None);
        let hidden: Vec<bool> = tl.tracks.iter().map(|t| t.hidden).collect();
        assert_eq!(hidden, [false, true, false]);
        assert_eq!(tl.plan(0).drawings(), [ids[0], ids[2]]);
    }

    #[test]
    fn keeps_with_the_document() {
        let (mut doc, ids) = canvas(3);
        doc.animation = Some(animation::Animation::default());
        doc.start_timeline();
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str::<Document>(&json).unwrap().repaired();
        assert_eq!(back, doc);
        // a document saved before timelines reads without one
        let mut older: serde_json::Value = serde_json::from_str(&json).unwrap();
        older.as_object_mut().unwrap().remove("timeline");
        assert!(serde_json::from_value::<Document>(older).unwrap().timeline.is_none());
        // one that can't be read is left out, not the document
        let mut broken: serde_json::Value = serde_json::from_str(&json).unwrap();
        broken["timeline"] = serde_json::json!({ "fps": "fast" });
        assert!(serde_json::from_value::<Document>(broken).unwrap().timeline.is_none());
        // a drawing whose layer is gone is dropped when the document is repaired
        let mut gone = doc.clone();
        gone.timeline.as_mut().unwrap().tracks[0].clips[1].content =
            ClipContent::Drawing(Drawing { root: LayerId(999) });
        let repaired = gone.repaired().timeline.unwrap();
        assert_eq!(repaired.tracks[0].clips.len(), 2);
        assert_eq!(repaired.plan(1).drawings(), Vec::<LayerId>::new());
        assert_eq!(repaired.plan(2).drawings(), [ids[2]]);
    }

    #[test]
    fn repair_drops_overlaps_and_repeats() {
        let mut tl = Timeline::new(12, 10);
        tl.tracks.push(track(vec![drawing(1, 0, 4, 10), drawing(2, 2, 4, 11), drawing(3, 5, 2, 10)]));
        tl.tracks.push(Track { id: TrackId(1), ..track(vec![drawing(1, 0, 1, 12)]) });
        tl.simple = Some(TrackId(7));
        tl.loop_range = Some((4, 20));
        let tl = tl.repaired(&[LayerId(10), LayerId(11), LayerId(12)]);
        // the overlapping clip goes, the second drawing of layer 10 goes, the repeated track goes
        assert_eq!(tl.tracks.len(), 1);
        assert_eq!(tl.tracks[0].clips.iter().map(|c| c.id.0).collect::<Vec<_>>(), [1]);
        assert_eq!((tl.simple, tl.loop_range), (None, None));
        assert!(tl.next_track > 1 && tl.next_clip > 3);
    }

    #[test]
    fn masks_take_the_clip_just_below() {
        let mut tl = Timeline::new(12, 4);
        let mut matte = drawing(2, 0, 2, 11);
        matte.mask = MaskMode::Alpha;
        let mut clipped = drawing(4, 0, 4, 13);
        clipped.mask = MaskMode::Clipping;
        tl.tracks = vec![
            Track { id: TrackId(1), ..track(vec![drawing(1, 0, 4, 10)]) },
            Track { id: TrackId(2), ..track(vec![matte]) },
            Track { id: TrackId(3), ..track(vec![drawing(3, 1, 1, 12)]) },
            Track { id: TrackId(4), ..track(vec![clipped]) },
        ];
        // frame 0: the matte uses layer 10 (which then doesn't draw); nothing is under the clipped one
        let plan = tl.plan(0);
        assert_eq!(plan.items.iter().map(|i| i.mask.map(|m| m.1)).collect::<Vec<_>>(), [None, Some(0), None]);
        assert!(plan.items[0].matte_only);
        assert_eq!(plan.drawings(), [LayerId(11), LayerId(13)]);
        // frame 1: layer 12 is under the clipped one, which still lets it draw
        let plan = tl.plan(1);
        assert_eq!(plan.items[3].mask, Some((MaskMode::Clipping, 2)));
        assert_eq!(plan.drawings(), [LayerId(11), LayerId(12), LayerId(13)]);
        // frame 2: the matte has ended, so layer 10 draws again
        assert_eq!(tl.plan(2).drawings(), [LayerId(10), LayerId(13)]);
    }

    #[test]
    fn groups_play_their_own_frames_and_cycles() {
        let mut tl = Timeline::new(12, 12);
        let walk = track(vec![drawing(1, 0, 1, 10), drawing(2, 1, 1, 11), drawing(3, 2, 1, 12)]);
        let group = Clip {
            content: ClipContent::Group(Group {
                tracks: vec![walk],
                collapsed: false,
                cycle: Some(Cycle { length: 3, keys: CycleKeys::Repeat }),
            }),
            ..drawing(4, 2, 9, 0)
        };
        tl.tracks.push(track(vec![group]));
        let shown: Vec<Vec<LayerId>> = (0..12).map(|f| tl.plan(f).drawings()).collect();
        let l = |n| vec![LayerId(n)];
        assert_eq!(shown, [vec![], vec![], l(10), l(11), l(12), l(10), l(11), l(12), l(10), l(11), l(12), vec![]]);
        assert_eq!(tl.plan(5).items[0].local, 3);
    }

    /// Each clip of track `t` as (layer, start, duration).
    fn spans(tl: &Timeline, t: usize) -> Vec<(u64, u32, u32)> {
        tl.tracks[t]
            .clips
            .iter()
            .map(|c| match &c.content {
                ClipContent::Drawing(d) => (d.root.0, c.start, c.duration),
                _ => (0, c.start, c.duration),
            })
            .collect()
    }

    fn edited() -> Timeline {
        let mut tl = Timeline::new(12, 6);
        tl.exposure = 2;
        let a = tl.add_track("A");
        tl.add_track("B");
        for (root, at) in [(10, 0), (11, 2), (12, 5)] {
            tl.add_drawing(a, at, LayerId(root), "d").unwrap();
        }
        tl
    }

    #[test]
    fn drawings_go_in_after_the_one_there_making_room() {
        let tl = edited();
        // each held for the exposure; the one at 2 came after the one covering 0..2
        assert_eq!(spans(&tl, 0), [(10, 0, 2), (11, 2, 2), (12, 5, 2)]);
        let mut tl = tl;
        let a = tl.tracks[0].id;
        // added on frame 1, inside the first: it goes just after it, and the rest move along
        let id = tl.add_drawing(a, 1, LayerId(13), "d").unwrap();
        assert_eq!(spans(&tl, 0), [(10, 0, 2), (13, 2, 2), (11, 4, 2), (12, 7, 2)]);
        assert_eq!(tl.duration, 9);
        assert!(tl.tracks.iter().all(Track::is_ordered));
        assert_eq!(tl.drawing_of(LayerId(13)), Some((a, id)));
    }

    #[test]
    fn holds_push_or_stop_at_the_next() {
        let mut tl = edited();
        let first = tl.tracks[0].clips[0].id;
        assert!(tl.set_duration(first, 4, true));
        assert_eq!(spans(&tl, 0), [(10, 0, 4), (11, 4, 2), (12, 7, 2)]);
        assert!(tl.set_duration(first, 1, true));
        assert_eq!(spans(&tl, 0), [(10, 0, 1), (11, 1, 2), (12, 4, 2)]);
        // not pushing: stops at the next clip
        let second = tl.tracks[0].clips[1].id;
        assert!(tl.set_duration(second, 9, false));
        assert_eq!(spans(&tl, 0), [(10, 0, 1), (11, 1, 3), (12, 4, 2)]);
        assert!(tl.set_duration(second, 0, false));
        assert_eq!(spans(&tl, 0)[1], (11, 1, 1));
    }

    #[test]
    fn clips_move_to_the_nearest_room() {
        let mut tl = edited();
        let (a, b) = (tl.tracks[0].id, tl.tracks[1].id);
        let last = tl.tracks[0].clips[2].id;
        // over the first two: it goes to the gap at 4..5? too small for 2 frames, so the nearest fit
        assert!(tl.move_clip(last, a, 1));
        assert_eq!(spans(&tl, 0), [(10, 0, 2), (11, 2, 2), (12, 4, 2)]);
        // onto the other track, wherever asked
        assert!(tl.move_clip(last, b, 9));
        assert_eq!(spans(&tl, 1), [(12, 9, 2)]);
        assert_eq!(tl.duration, 11);
        assert!(tl.tracks.iter().all(Track::is_ordered));
    }

    #[test]
    fn clips_split_duplicate_and_go() {
        let mut tl = edited();
        let first = tl.tracks[0].clips[0].id;
        assert!(tl.split(first, 0, LayerId(20)).is_none(), "not at its start");
        let rest = tl.split(first, 1, LayerId(20)).unwrap();
        assert_eq!(spans(&tl, 0), [(10, 0, 1), (20, 1, 1), (11, 2, 2), (12, 5, 2)]);
        let copy = tl.duplicate(rest, LayerId(21)).unwrap();
        assert_eq!(spans(&tl, 0), [(10, 0, 1), (20, 1, 1), (21, 2, 1), (11, 3, 2), (12, 6, 2)]);
        assert_eq!(tl.remove(copy, true).map(|c| c.id), Some(copy));
        assert_eq!(spans(&tl, 0), [(10, 0, 1), (20, 1, 1), (11, 2, 2), (12, 5, 2)]);
        tl.remove(rest, false);
        assert_eq!(spans(&tl, 0), [(10, 0, 1), (11, 2, 2), (12, 5, 2)]);
        let b = tl.tracks[1].id;
        tl.simple = Some(b);
        assert_eq!(tl.remove_track(b).map(|t| t.name), Some("B".to_owned()));
        assert_eq!(tl.simple, None);
    }

    fn keys<T: Copy>(at: &[(i32, T, Easing)]) -> Channel<T> {
        Channel {
            keys: at
                .iter()
                .map(|(frame, value, easing)| Key { frame: *frame, value: *value, easing: easing.clone(), path: None })
                .collect(),
        }
    }

    fn near(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-3
    }

    #[test]
    fn values_ease_from_key_to_key() {
        let c = keys(&[(0, 0.0_f32, Easing::Linear), (10, 1.0, Easing::Hold), (20, 3.0, Easing::Linear)]);
        assert_eq!(c.value_at(-5.0), Some(0.0), "before the first: the first");
        assert!(near(c.value_at(2.5).unwrap(), 0.25));
        assert_eq!(c.value_at(15.0), Some(1.0), "held until the next key");
        assert_eq!(c.value_at(20.0), Some(3.0));
        assert_eq!(c.value_at(99.0), Some(3.0), "after the last: the last");
        assert_eq!(Channel::<f32>::default().value_at(3.0), None);
        // ease in and out: slow at both ends, half way at the middle
        let ease = Easing::CubicBezier([0.42, 0.0], [0.58, 1.0]);
        let e = keys(&[(0, 0.0_f32, ease.clone()), (10, 1.0, Easing::Linear)]);
        assert!(e.value_at(1.0).unwrap() < 0.05);
        assert!(near(e.value_at(5.0).unwrap(), 0.5));
        assert!(e.value_at(9.0).unwrap() > 0.95);
        // smooth: through every key, curving between them, and two keys alone go straight
        let s = keys(&[
            (0, [0.0, 0.0], Easing::Smooth),
            (10, [10.0, 10.0], Easing::Smooth),
            (20, [20.0, 0.0], Easing::Smooth),
        ]);
        assert_eq!(s.value_at(10.0), Some([10.0, 10.0]));
        let mid = s.value_at(5.0).unwrap();
        assert!(near(mid[0], 4.375) && near(mid[1], 5.625), "Koi's curve, easing away from its first key: {mid:?}");
        let two = keys(&[(0, 0.0_f32, Easing::Smooth), (10, 10.0, Easing::Smooth)]);
        assert!(near(two.value_at(5.0).unwrap(), 5.0));
        assert!(near(Easing::Reversed(Box::new(ease.clone())).progress(0.5), 0.5));
    }

    /// Koi Studio's own easing code, compiled and run on these samples (ProceduralEasing.swift,
    /// CurveSpline.swift), gave these values: the port matches them.
    #[test]
    fn easings_match_koi_studio_s() {
        let at = [0.0, 0.1, 0.25, 0.4, 0.5, 0.6, 0.75, 0.9, 1.0];
        let cases: [(&str, Easing, [f32; 9]); 8] = [
            (
                "bounce",
                Easing::Bounce { first_step: 0.3, bounciness: 0.6 },
                [0.0, 0.444444, 0.407407, 0.671605, 0.964279, 1.0, 1.0, 1.0, 1.0],
            ),
            (
                "elastic",
                Easing::Elastic { step: 0.25, attack: 1.0, decay: 0.5, magnitude: 1.0 },
                [0.0, 0.457852, 1.255, 0.972698, 0.962837, 0.996021, 1.001381, 0.999995, 1.0],
            ),
            ("steps", Easing::Steps { step: 0.25, smoothing: 0.3 }, [0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0]),
            (
                "elastic steps",
                Easing::ElasticSteps { step: 0.2, magnitude: 0.5 },
                [0.0, 0.0, 0.207909, 0.2, 0.399818, 0.4, 0.6, 0.799818, 1.0],
            ),
            (
                "cyclic",
                Easing::Cyclic { step: 2.0 / 7.0, sharpness: 0.3, skew: 0.4, decay: 0.2 },
                [0.0, 0.937141, 0.178085, 1.0, 0.445972, 0.276211, 0.720724, 0.449419, 0.922487],
            ),
            (
                "random",
                Easing::Random { frequency: 0.5, magnitude: 0.5, seed: 0.37, attenuation: 0.7, warp: 0.3, tilt: 0.1 },
                [0.1, -0.070499, -0.148967, -0.22192, 0.092261, 0.048519, 0.624138, 0.594274, 0.9],
            ),
            (
                "fitted",
                Easing::Fitted(Box::new(Easing::Linear), RepeatFit { mode: FitMode::Loop, repetitions: 2 }),
                [0.0, 0.352, 1.0, 0.352, 0.0, 0.352, 1.0, 0.352, 0.0],
            ),
            (
                "curve",
                Easing::Curve(vec![[0.0, 0.0], [0.35, 0.28], [0.62, 1.18], [1.0, 1.0]]),
                [0.0, -0.010245, 0.079673, 0.436399, 0.805326, 1.132071, 1.307631, 1.171568, 1.0],
            ),
        ];
        for (name, easing, koi) in cases {
            for (t, want) in at.iter().zip(koi) {
                let got = easing.progress(*t);
                assert!((got - want).abs() < 2e-4, "{name} at {t}: {got} where Koi gives {want}");
            }
        }
    }

    #[test]
    fn motion_paths_bend_the_way_and_keep_the_timing() {
        // two keys 100 apart across, the first's handle pulling the path down
        let mut c = keys(&[(0, [0.0, 0.0], Easing::Linear), (10, [100.0, 0.0], Easing::Linear)]);
        assert_eq!(c.position_at(5.0), Some([50.0, 0.0]), "no handles: straight");
        if let Some(k) = c.keys.first_mut() {
            k.path = Some(PathHandles { mode: HandleMode::Symmetric, before: [0.0, 0.0], after: [0.0, 80.0] });
        }
        let mid = c.position_at(5.0).unwrap();
        assert!(mid[1] > 10.0, "bent down: {mid:?}");
        assert_eq!(c.position_at(0.0), Some([0.0, 0.0]));
        assert_eq!(c.position_at(10.0), Some([100.0, 0.0]));
        // by distance along it: equal steps of time cover equal lengths of path
        let step = |a: f32, b: f32| {
            let (p, q) = (c.position_at(a).unwrap(), c.position_at(b).unwrap());
            (q[0] - p[0]).hypot(q[1] - p[1])
        };
        assert!((step(1.0, 2.0) - step(7.0, 8.0)).abs() < 1.0, "{} vs {}", step(1.0, 2.0), step(7.0, 8.0));
    }

    #[test]
    fn a_clip_moves_turns_scales_and_fades() {
        let mut k = ClipKeys { pivot: Some([10.0, 10.0]), ..ClipKeys::default() };
        assert_eq!(k.pose_at(0.0), None, "no pose keys, drawn in place");
        k.rotation = keys(&[(0, 0.0, Easing::Linear), (10, std::f32::consts::FRAC_PI_2, Easing::Linear)]);
        k.position = keys(&[(0, [0.0, 0.0], Easing::Linear), (10, [5.0, 0.0], Easing::Linear)]);
        k.scale = keys(&[(0, [2.0, 2.0], Easing::Linear)]);
        k.opacity = keys(&[(0, 1.0, Easing::Linear), (10, 0.0, Easing::Linear)]);
        // at frame 10: a quarter turn and twice the size about the pivot, then 5 along
        let p = k.pose_at(10.0).unwrap();
        let at = |x: f32, y: f32| {
            let v = p.apply(Vec2::new(x, y));
            [v.x, v.y]
        };
        let pivot = at(10.0, 10.0);
        assert!(near(pivot[0], 15.0) && near(pivot[1], 10.0), "the pivot only moves: {pivot:?}");
        let right = at(11.0, 10.0);
        assert!(
            near(right[0], 15.0) && near(right[1], 12.0),
            "a point right of it turns to below it, twice as far: {right:?}"
        );
        assert!(near(k.opacity_at(5.0).unwrap(), 0.5));
        // in the plan, the clip's pose and its keyed opacity at its own frame
        let mut tl = Timeline::new(12, 20);
        let mut clip = drawing(1, 4, 12, 10);
        clip.keys = k;
        tl.tracks.push(track(vec![clip]));
        let item = &tl.plan(9).items[0];
        assert_eq!(item.local, 5);
        assert!(near(item.opacity, 0.5));
        assert!(item.pose.is_some());
        let shown = tl.showing(9, true);
        assert_eq!(shown[0].pose, item.pose);
        assert!(near(shown[0].opacity, 0.5));
    }

    #[test]
    fn a_group_s_pose_carries_its_drawings() {
        let mut tl = Timeline::new(12, 4);
        let mut inner = drawing(1, 0, 4, 10);
        inner.keys.position = keys(&[(0, [1.0, 0.0], Easing::Linear)]);
        let mut group = Clip {
            content: ClipContent::Group(Group { tracks: vec![track(vec![inner])], collapsed: false, cycle: None }),
            ..drawing(2, 0, 4, 0)
        };
        group.keys.position = keys(&[(0, [0.0, 2.0], Easing::Linear)]);
        tl.tracks.push(track(vec![group]));
        let pose = tl.showing(0, true)[0].pose.unwrap();
        let moved = pose.apply(Vec2::new(0.0, 0.0));
        assert!(near(moved.x, 1.0) && near(moved.y, 2.0), "its own move, then the group's: {moved:?}");
    }

    #[test]
    fn the_camera_frames_the_canvas_and_turns_the_short_way() {
        let still = CameraPose::still(100.0, 50.0);
        let m = still.to_frame(100.0, 50.0);
        let p = m.apply(Vec2::new(10.0, 20.0));
        assert!(near(p.x, 10.0) && near(p.y, 20.0), "the whole canvas as it is: {p:?}");
        // looking at (70, 25) twice as close: that point in the middle, the rest spread about it
        let pushed = CameraPose { center: [70.0, 25.0], zoom: 2.0, rotation: 0.0 };
        let m = pushed.to_frame(100.0, 50.0);
        let mid = m.apply(Vec2::new(70.0, 25.0));
        let right = m.apply(Vec2::new(75.0, 25.0));
        assert!(near(mid.x, 50.0) && near(mid.y, 25.0) && near(right.x, 60.0), "{mid:?} {right:?}");
        let c = pushed.corners(100.0, 50.0);
        assert!(near(c[0][0], 45.0) && near(c[0][1], 12.5) && near(c[2][0], 95.0) && near(c[2][1], 37.5), "{c:?}");
        // no keys: no camera
        let mut tl = Timeline::new(12, 20);
        assert_eq!(tl.camera_at(3.0), None);
        // from just short of a whole turn to just past it: through 0, not back round
        let at = |rotation: f32| CameraPose { center: [0.0, 0.0], zoom: 1.0, rotation };
        tl.camera.set(0, at(350_f32.to_radians()), Easing::Linear);
        tl.camera.set(10, CameraPose { center: [10.0, 0.0], zoom: 3.0, ..at(10_f32.to_radians()) }, Easing::Linear);
        let half = tl.camera_at(5.0).unwrap();
        assert!(half.rotation < 0.01 || half.rotation > std::f32::consts::TAU - 0.01, "{}", half.rotation.to_degrees());
        assert!(near(half.zoom, 2.0) && near(half.center[0], 5.0));
        let quarter = tl.camera_at(2.5).unwrap().rotation.to_degrees();
        assert!((quarter - 355.0).abs() < 0.01, "{quarter}");
    }

    /// Clips 1 and 2 on track 1 and clip 3 on track 2 (layers 10, 11, 12), then clip 4 (layer 13)
    /// on track 1 after them.
    fn three_to_group() -> Timeline {
        let mut tl = Timeline::new(12, 20);
        tl.tracks = vec![
            Track { id: TrackId(1), ..track(vec![drawing(1, 2, 2, 10), drawing(2, 4, 3, 11), drawing(4, 9, 2, 13)]) },
            Track { id: TrackId(2), ..track(vec![drawing(3, 3, 5, 12)]) },
        ];
        tl.next_clip = 5;
        tl.next_track = 3;
        tl
    }

    #[test]
    fn clips_group_and_ungroup() {
        let mut tl = three_to_group();
        let before: Vec<Vec<LayerId>> = (0..12).map(|f| tl.plan(f).drawings()).collect();
        let g = tl.group(&[ClipId(1), ClipId(2), ClipId(3)], "Group").unwrap();
        // on the topmost member's track, from the first's start to the last's end
        assert_eq!(tl.tracks[1].clips.iter().map(|c| c.id).collect::<Vec<_>>(), [g]);
        let group = tl.clip(g).unwrap();
        assert_eq!((group.start, group.duration), (2, 6));
        let ClipContent::Group(inner) = &group.content else { panic!() };
        assert_eq!(inner.tracks.len(), 2);
        assert_eq!(inner.tracks[0].clips.iter().map(|c| (c.id.0, c.start)).collect::<Vec<_>>(), [(1, 0), (2, 2)]);
        assert_eq!(inner.tracks[1].clips.iter().map(|c| (c.id.0, c.start)).collect::<Vec<_>>(), [(3, 1)]);
        // every frame shows what it did
        assert_eq!((0..12).map(|f| tl.plan(f).drawings()).collect::<Vec<_>>(), before);
        // found inside it, and where it is on the timeline
        assert_eq!(tl.clip(ClipId(3)).map(|c| c.start), Some(1));
        assert_eq!(tl.origin(ClipId(3)), Some(3));
        assert_eq!(tl.group_of(ClipId(3)), Some(g));
        let inner_track = inner.tracks[1].id;
        assert_eq!(tl.level_start(inner_track), Some(2));
        assert_eq!(tl.track(inner_track).map(|t| t.clips.len()), Some(1));
        assert_eq!(tl.drawing_of(LayerId(12)), Some((inner_track, ClipId(3))));
        assert_eq!(tl.group_depth(), 1);
        // and apart again: the same frames, its bottom track back on its own
        assert!(tl.ungroup(g));
        assert_eq!((0..12).map(|f| tl.plan(f).drawings()).collect::<Vec<_>>(), before);
        assert_eq!(spans(&tl, 1), [(10, 2, 2), (11, 4, 3)]);
        assert!(tl.tracks.iter().all(Track::is_ordered));
        // a group that doesn't fit on its track goes on a new one just above
        let mut tl = three_to_group();
        let g = tl.group(&[ClipId(1), ClipId(4)], "Run").unwrap();
        assert_eq!(tl.tracks.len(), 3);
        assert_eq!(tl.tracks[1].clips[0].id, g);
        assert_eq!(tl.tracks[2].clips[0].id, ClipId(3));
        // members on another level, or none, make no group
        assert_eq!(tl.group(&[ClipId(2), ClipId(4)], "Mixed"), None);
        assert_eq!(tl.group(&[], "Empty"), None);
    }

    #[test]
    fn clips_inside_a_group_are_edited_on_its_own_frames() {
        let mut tl = three_to_group();
        let g = tl.group(&[ClipId(1), ClipId(2), ClipId(3)], "Group").unwrap();
        let inner = match &tl.clip(g).unwrap().content {
            ClipContent::Group(gr) => gr.tracks[0].id,
            _ => panic!(),
        };
        assert!(tl.set_duration(ClipId(1), 3, true));
        assert_eq!(tl.clip(ClipId(2)).map(|c| c.start), Some(3), "pushed along inside the group");
        assert!(tl.move_clip(ClipId(2), inner, 6));
        assert_eq!(tl.clip(ClipId(2)).map(|c| c.start), Some(6));
        let added = tl.add_drawing(inner, 0, LayerId(20), "New").unwrap();
        assert_eq!(tl.origin(added), Some(5), "after the first drawing, on the timeline at 2 + 3");
        let cut = tl.split(ClipId(2), 7, LayerId(21)).unwrap();
        assert_eq!(tl.clip(cut).map(|c| (c.start, c.duration)), Some((7, 2)));
        assert!(tl.remove(cut, false).is_some());
        assert!(tl.add_group_track(g, "More").is_some());
        assert!(tl.add_group_track(ClipId(4), "Not a group").is_none());
        let made = tl.track_mut(inner).map(|t| t.clips.len());
        assert_eq!(made, Some(3));
        assert!(tl.remove_track(inner).is_some());
        assert!(tl.track(inner).is_none());
    }

    #[test]
    fn a_cycling_group_s_keys_repeat_or_run_on() {
        let mut tl = Timeline::new(12, 12);
        let mut group = Clip {
            content: ClipContent::Group(Group {
                tracks: vec![Track { id: TrackId(5), ..track(vec![drawing(1, 0, 3, 10)]) }],
                collapsed: false,
                cycle: Some(Cycle { length: 3, keys: CycleKeys::Repeat }),
            }),
            ..drawing(2, 2, 9, 0)
        };
        group.keys.opacity = keys(&[(0, 0.0, Easing::Linear), (6, 1.0, Easing::Linear)]);
        tl.tracks.push(track(vec![group]));
        // frame 2 + 4 is the second cycle's frame 1: repeated, at 1/6; run on, at 4/6
        assert!(near(tl.plan(6).items[0].opacity, 1.0 / 6.0));
        assert_eq!(tl.local_frame(ClipId(1), 6), Some(1));
        assert_eq!(tl.track_frame(TrackId(5), 6), Some(1));
        assert_eq!(tl.track_frame(TrackId(1), 6), Some(6));
        if let Some(ClipContent::Group(g)) = tl.clip_mut(ClipId(2)).map(|c| &mut c.content) {
            g.cycle = Some(Cycle { length: 3, keys: CycleKeys::ContinueAcross });
        }
        assert!(near(tl.plan(6).items[0].opacity, 4.0 / 6.0));
    }

    #[test]
    fn mattes_are_drawn_from_though_they_are_not_shown() {
        let mut tl = Timeline::new(12, 4);
        let mut matte = drawing(2, 0, 4, 11);
        matte.mask = MaskMode::Luminance;
        tl.tracks = vec![
            Track { id: TrackId(1), ..track(vec![drawing(1, 0, 4, 10)]) },
            Track { id: TrackId(2), ..track(vec![matte]) },
        ];
        assert_eq!(tl.plan(0).drawings(), [LayerId(11)]);
        assert_eq!(tl.drawn(0, true), [LayerId(10), LayerId(11)]);
    }
}

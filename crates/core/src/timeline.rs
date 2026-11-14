//! The canvas on a timeline (see the engine's `timeline`): putting it on one, its tracks and clips
//! as the apps show them, editing them, and the frame shown, drawn on or played. Animated exports
//! and `animation_frame` follow the timeline once there is one (see `animation`).
//!
//! The playhead and the track drawn on lead the current layer: moving either makes a layer of the
//! drawing there current, and choosing a layer moves them to its drawing. Painting where the track
//! has no drawing makes one.

use make2d_engine::animation::Playback;
use make2d_engine::document::{Document, LayerId, LayerKind};
use make2d_engine::timeline::{ClipContent, ClipId, MAX_EXPOSURE, MAX_FPS, TrackId};

use crate::animation::AnimationPlayback;
use crate::{CanvasSession, State};

/// The most frames a timeline lasts.
const MOST_FRAMES: u32 = 100_000;

/// A timeline as the apps show it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineInfo {
    /// Frames a second.
    pub fps: u32,
    /// How many frames it lasts.
    pub duration: u32,
    /// How many frames a new drawing is held for.
    pub exposure: u32,
    pub playback: AnimationPlayback,
    /// Drawings shown faintly before and after the one drawn on, and the nearest ones' opacity.
    pub onion_frames: u32,
    pub onion_opacity: f32,
    /// Shown as a one-track flipbook (simple mode) rather than every track.
    pub simple: bool,
    /// The frame drawn on.
    pub frame: u32,
    /// The track drawn on.
    pub track: Option<u64>,
}

/// The settings of a timeline the apps change.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineSettings {
    pub fps: u32,
    pub duration: u32,
    pub exposure: u32,
    pub playback: AnimationPlayback,
    pub onion_frames: u32,
    pub onion_opacity: f32,
}

/// What a clip holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TimelineClipKind {
    Drawing,
    Group,
    Video,
    Audio,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineClipInfo {
    pub id: u64,
    pub name: String,
    pub start: u32,
    pub duration: u32,
    pub kind: TimelineClipKind,
    /// A drawing's layer (its picture is `frame_thumbnail` of it); 0 for other clips.
    pub root: u64,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineTrackInfo {
    pub id: u64,
    pub name: String,
    pub hidden: bool,
    pub locked: bool,
    /// In time order.
    pub clips: Vec<TimelineClipInfo>,
}

fn playback_of(p: Playback) -> AnimationPlayback {
    match p {
        Playback::Loop => AnimationPlayback::Loop,
        Playback::PingPong => AnimationPlayback::PingPong,
        Playback::OneShot => AnimationPlayback::OneShot,
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Puts the canvas on a timeline made from it (Animation Assist's frames, or each layer for a
    /// still canvas), as an undo step. False if it is on one already, or is a Page Assist canvas.
    pub fn start_timeline(&self) -> bool {
        let started = self.change("Use Timeline", |d| d.start_timeline().then_some(())).is_some();
        if started {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.timeline_track =
                st.doc.timeline.as_ref().and_then(|t| t.simple.or_else(|| t.tracks.first().map(|t| t.id)));
            if let Some(c) = st.canvas.as_mut() {
                c.set_play_frame(None);
                c.set_timeline_frame(0);
            }
            st.follow_playhead();
            self.shared.frame(st);
        }
        started
    }

    /// Takes the canvas off its timeline, as an undo step: its layers stay as they are.
    pub fn stop_timeline(&self) -> bool {
        let stopped = self.change("Stop Using Timeline", |d| d.timeline.take().map(|_| ())).is_some();
        if stopped {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            if let Some(c) = st.canvas.as_mut() {
                c.set_play_frame(None);
                c.invalidate();
            }
            self.shared.frame(st);
        }
        stopped
    }

    /// The timeline, if the canvas is on one.
    pub fn timeline_info(&self) -> Option<TimelineInfo> {
        let st = self.shared.lock();
        let tl = st.doc.timeline.as_ref()?;
        Some(TimelineInfo {
            fps: tl.fps,
            duration: tl.duration,
            exposure: tl.exposure,
            playback: playback_of(tl.playback),
            onion_frames: tl.onion.count[0],
            onion_opacity: tl.onion.opacity,
            simple: tl.simple.is_some(),
            frame: st.canvas.as_ref().map_or(0, |c| c.timeline_frame()),
            track: st.drawn_track().map(|t| t.0),
        })
    }

    /// The tracks, bottom first, with their clips.
    pub fn timeline_tracks(&self) -> Vec<TimelineTrackInfo> {
        let st = self.shared.lock();
        let Some(tl) = st.doc.timeline.as_ref() else { return Vec::new() };
        tl.tracks
            .iter()
            .map(|t| TimelineTrackInfo {
                id: t.id.0,
                name: t.name.clone(),
                hidden: t.hidden,
                locked: t.locked,
                clips: t
                    .clips
                    .iter()
                    .map(|c| {
                        let (kind, root) = match &c.content {
                            ClipContent::Drawing(d) => (TimelineClipKind::Drawing, d.root.0),
                            ClipContent::Group(_) => (TimelineClipKind::Group, 0),
                            ClipContent::Video(_) => (TimelineClipKind::Video, 0),
                            ClipContent::Audio(_) => (TimelineClipKind::Audio, 0),
                        };
                        TimelineClipInfo {
                            id: c.id.0,
                            name: c.name.clone(),
                            start: c.start,
                            duration: c.duration,
                            kind,
                            root,
                        }
                    })
                    .collect(),
            })
            .collect()
    }

    /// Changes the settings (live, as sliders move: not an undo step).
    pub fn set_timeline_settings(&self, s: TimelineSettings) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(tl) = st.doc.timeline.as_mut() else { return };
        tl.fps = s.fps.clamp(1, MAX_FPS);
        let end = tl.tracks.iter().flat_map(|t| t.clips.last()).map(|c| c.end()).max().unwrap_or(1);
        tl.duration = s.duration.clamp(end.max(1), MOST_FRAMES);
        tl.exposure = s.exposure.clamp(1, MAX_EXPOSURE);
        tl.playback = match s.playback {
            AnimationPlayback::Loop => Playback::Loop,
            AnimationPlayback::PingPong => Playback::PingPong,
            AnimationPlayback::OneShot => Playback::OneShot,
        };
        tl.onion.count = [s.onion_frames.min(make2d_engine::animation::MAX_ONION_FRAMES); 2];
        tl.onion.opacity = if s.onion_opacity.is_finite() { s.onion_opacity.clamp(0.0, 1.0) } else { 0.4 };
        st.revision += 1;
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
    }

    /// Simple mode (the track drawn on as a flipbook strip) or every track.
    pub fn set_timeline_simple(&self, on: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let track = st.drawn_track();
        let Some(tl) = st.doc.timeline.as_mut() else { return };
        tl.simple = if on { track } else { None };
        st.revision += 1;
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
    }

    /// Draws on frame `frame` (kept within the timeline), its onion skins around it; a layer of the
    /// drawing there on the track drawn on becomes current.
    pub fn set_timeline_frame(&self, frame: u32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(last) = st.doc.timeline.as_ref().map(|t| t.duration.saturating_sub(1)) else { return };
        if let Some(c) = st.canvas.as_mut() {
            c.set_timeline_frame(frame.min(last));
        }
        st.follow_playhead();
        self.shared.frame(st);
    }

    /// Draws on track `id` (simple mode then shows it): a layer of its drawing at the playhead
    /// becomes current.
    pub fn select_timeline_track(&self, id: u64) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(tl) = st.doc.timeline.as_mut() else { return };
        if tl.track_index(TrackId(id)).is_none() {
            return;
        }
        if tl.simple.is_some() {
            tl.simple = Some(TrackId(id));
        }
        st.timeline_track = Some(TrackId(id));
        st.follow_playhead();
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
    }

    /// Playing: frame `frame` shows as it plays, with no onion skins; none goes back to drawing on
    /// the timeline frame. (`animation_next_step` steps through the frames, as long as the
    /// timeline lasts.)
    pub fn set_timeline_playing(&self, frame: Option<u32>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.doc.timeline.is_none() {
            return;
        }
        if let Some(c) = st.canvas.as_mut() {
            c.set_play_frame(frame.and_then(|f| usize::try_from(f).ok()));
        }
        self.shared.frame(st);
    }

    /// A new empty track on top, drawn on; returns its id (0 if there is no timeline).
    pub fn add_timeline_track(&self) -> u64 {
        let id = self.change("Add Track", |d| {
            let tl = d.timeline.as_mut()?;
            let name = format!("Track {}", tl.tracks.len() + 1);
            Some(tl.add_track(&name))
        });
        let Some(id) = id else { return 0 };
        self.select_timeline_track(id.0);
        id.0
    }

    /// Takes track `id` away with its drawings' layers (the last layer stays), as an undo step.
    pub fn delete_timeline_track(&self, id: u64) -> bool {
        self.change("Delete Track", |d| {
            let track = d.timeline.as_mut()?.remove_track(TrackId(id))?;
            for clip in &track.clips {
                if let ClipContent::Drawing(dr) = &clip.content {
                    d.delete_layer(dr.root);
                }
            }
            Some(())
        })
        .is_some()
    }

    /// Hides or shows track `id`, or locks or unlocks it, as an undo step.
    pub fn set_timeline_track(&self, id: u64, hidden: bool, locked: bool) -> bool {
        self.change(if hidden { "Hide Track" } else { "Track" }, |d| {
            let tl = d.timeline.as_mut()?;
            let at = tl.track_index(TrackId(id))?;
            let track = tl.tracks.get_mut(at)?;
            (track.hidden != hidden || track.locked != locked).then(|| {
                track.hidden = hidden;
                track.locked = locked;
            })
        })
        .is_some()
    }

    /// Renames track `id`, as an undo step.
    pub fn rename_timeline_track(&self, id: u64, name: String) -> bool {
        let name: String = name.trim().chars().take(256).collect();
        if name.is_empty() {
            return false;
        }
        self.change("Rename Track", |d| {
            let tl = d.timeline.as_mut()?;
            let at = tl.track_index(TrackId(id))?;
            tl.tracks.get_mut(at)?.name = name;
            Some(())
        })
        .is_some()
    }

    /// A new drawing (an empty layer) at the playhead on the track drawn on (just after the
    /// drawing there, the ones after it moving along), made current, as an undo step; returns its
    /// clip, or 0 when it can't be made (no timeline, a locked track, no room for a layer).
    pub fn add_timeline_drawing(&self) -> u64 {
        let (frame, track) = {
            let st = self.shared.lock();
            (st.canvas.as_ref().map_or(0, |c| c.timeline_frame()), st.drawn_track())
        };
        let made = self.change("Add Drawing", |d| add_drawing(d, track, frame));
        let Some((track, clip, start)) = made else { return 0 };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.timeline_track = Some(track);
        if let Some(c) = st.canvas.as_mut() {
            c.set_timeline_frame(start);
        }
        self.shared.frame(st);
        clip.0
    }

    /// Clip `id` held for `duration` frames, as an undo step. `push`: the clips after it move
    /// along; else it stops at the next.
    pub fn set_timeline_clip_duration(&self, id: u64, duration: u32, push: bool) -> bool {
        self.change("Hold", |d| {
            let tl = d.timeline.as_mut()?;
            let before = tl.clip(ClipId(id))?.duration;
            (before != duration && tl.set_duration(ClipId(id), duration, push)).then_some(())
        })
        .is_some()
    }

    /// Clip `id` moved to track `track`, as near frame `start` as it fits, as an undo step.
    pub fn move_timeline_clip(&self, id: u64, track: u64, start: u32) -> bool {
        self.change("Move Clip", |d| {
            let tl = d.timeline.as_mut()?;
            let (t, _) = tl.find(ClipId(id))?;
            let before = tl.clip(ClipId(id)).map(|c| (tl.tracks.get(t).map(|t| t.id), c.start));
            if before == Some((Some(TrackId(track)), start)) {
                return None;
            }
            tl.move_clip(ClipId(id), TrackId(track), start).then_some(())
        })
        .is_some()
    }

    /// Clip `id` cut at the playhead: its drawing is copied for the rest. False unless the
    /// playhead is inside it (not on its first frame).
    pub fn split_timeline_clip(&self, id: u64) -> bool {
        let (frame, root) = {
            let st = self.shared.lock();
            let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
            let clip = st.doc.timeline.as_ref().and_then(|t| t.clip(ClipId(id)));
            let root = clip.filter(|c| frame > c.start && frame < c.end()).and_then(|c| match &c.content {
                ClipContent::Drawing(d) => Some(d.root),
                _ => None,
            });
            (frame, root)
        };
        let Some(root) = root else { return false };
        self.duplicated(root.0, "Split", |d, copy| {
            if let Some(tl) = d.timeline.as_mut() {
                tl.split(ClipId(id), frame, copy);
            }
        })
        .is_some()
    }

    /// A copy of clip `id`'s drawing just after it, as long, the clips after it moving along;
    /// returns the copy's clip, or 0.
    pub fn duplicate_timeline_clip(&self, id: u64) -> u64 {
        let root = {
            let st = self.shared.lock();
            st.doc.timeline.as_ref().and_then(|t| t.clip(ClipId(id))).and_then(|c| match &c.content {
                ClipContent::Drawing(d) => Some(d.root),
                _ => None,
            })
        };
        let Some(root) = root else { return 0 };
        let mut made = 0;
        self.duplicated(root.0, "Duplicate Drawing", |d, copy| {
            if let Some(clip) = d.timeline.as_mut().and_then(|tl| tl.duplicate(ClipId(id), copy)) {
                made = clip.0;
            }
        });
        made
    }

    /// Clip `id` taken off its track with its drawing's layers (the last layer stays), as an undo
    /// step. `close`: the clips after it move back over its place.
    pub fn delete_timeline_clip(&self, id: u64, close: bool) -> bool {
        let done = self
            .change("Delete Drawing", |d| {
                let clip = d.timeline.as_mut()?.remove(ClipId(id), close)?;
                if let ClipContent::Drawing(dr) = &clip.content {
                    d.delete_layer(dr.root);
                }
                Some(())
            })
            .is_some();
        if done {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.follow_playhead();
            self.shared.frame(st);
        }
        done
    }
}

/// A new drawing on `track` (or the first track, one made if there is none) at `frame`, in `d`:
/// (its track, its clip, its first frame).
fn add_drawing(d: &mut Document, track: Option<TrackId>, frame: u32) -> Option<(TrackId, ClipId, u32)> {
    if !d.has_room(1) {
        return None;
    }
    let tl = d.timeline.as_mut()?;
    let track = match track.filter(|t| tl.track_index(*t).is_some()) {
        Some(t) => t,
        None => match tl.tracks.first() {
            Some(t) => t.id,
            None => tl.add_track("Track 1"),
        },
    };
    if tl.tracks.get(tl.track_index(track)?)?.locked {
        return None;
    }
    let n = tl.drawing_roots().len() + 1;
    // a top-level layer, above the drawing current now
    if let Some(top) = d.top_of(d.current()) {
        d.set_current(top);
    }
    let layer = d.add_layer();
    let name = format!("Drawing {n}");
    d.rename(layer, &name);
    let tl = d.timeline.as_mut()?;
    let clip = tl.add_drawing(track, frame, layer, &name)?;
    let start = tl.clip(clip)?.start;
    Some((track, clip, start))
}

impl CanvasSession {
    /// Before a stroke on a timeline: if the current layer's drawing isn't showing at the playhead,
    /// the track drawn on's drawing there becomes current, or (none there, and painting) a new
    /// drawing is made for the stroke.
    pub(crate) fn drawing_for_stroke(&self) {
        let needs_one = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            if st.doc.timeline.is_none() || st.canvas.as_ref().is_some_and(|c| c.play_frame().is_some()) {
                return;
            }
            if st.current_drawing_shown() {
                return;
            }
            st.follow_playhead();
            !st.current_drawing_shown() && st.tool == crate::ToolKind::Paint
        };
        if needs_one {
            self.add_timeline_drawing();
        }
    }

    /// After a layer is chosen on a timeline: the track drawn on and the playhead go to its
    /// drawing, unless it shows at the playhead already.
    pub(crate) fn follow_layer(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(tl) = st.doc.timeline.as_ref() else { return };
        let Some((track, clip)) = st.doc.top_of(st.doc.current()).and_then(|top| tl.drawing_of(top)) else { return };
        let Some(start) = tl.clip(clip).map(|c| (c.start, c.end())) else { return };
        st.timeline_track = Some(track);
        if let Some(c) = st.canvas.as_mut() {
            let at = c.timeline_frame();
            if at < start.0 || at >= start.1 {
                c.set_timeline_frame(start.0);
            }
            c.invalidate();
        }
        self.shared.frame(st);
    }
}

impl State {
    /// The track drawn on: the one chosen, or simple mode's, or the first.
    pub(crate) fn drawn_track(&self) -> Option<TrackId> {
        let tl = self.doc.timeline.as_ref()?;
        self.timeline_track
            .filter(|t| tl.track_index(*t).is_some())
            .or(tl.simple)
            .or_else(|| tl.tracks.first().map(|t| t.id))
    }

    /// Whether the current layer is in the drawing at the playhead on the track drawn on.
    fn current_drawing_shown(&self) -> bool {
        let Some(tl) = self.doc.timeline.as_ref() else { return true };
        let frame = self.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(top) = self.doc.top_of(self.doc.current()) else { return false };
        let track = self.drawn_track().and_then(|t| tl.track_index(t)).and_then(|t| tl.tracks.get(t));
        matches!(track.and_then(|t| t.clip_at(frame)).map(|c| &c.content), Some(ClipContent::Drawing(d)) if d.root == top)
    }

    /// The current layer goes to the drawing at the playhead on the track drawn on (unless it is
    /// in it already): its topmost paint layer, or the drawing itself.
    pub(crate) fn follow_playhead(&mut self) {
        let Some(tl) = self.doc.timeline.as_ref() else { return };
        let frame = self.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(t) = self.drawn_track().and_then(|t| tl.track_index(t)) else { return };
        let Some(ClipContent::Drawing(d)) = tl.tracks.get(t).and_then(|t| t.clip_at(frame)).map(|c| &c.content) else {
            return;
        };
        let root = d.root;
        if self.doc.top_of(self.doc.current()) == Some(root) {
            return;
        }
        let target = topmost_paint(&self.doc, root).unwrap_or(root);
        if self.doc.set_current(target) {
            self.revision += 1;
            if let Some(c) = self.canvas.as_mut() {
                c.invalidate();
            }
        }
    }
}

/// The topmost paint layer in `id` (itself, if it is one).
fn topmost_paint(doc: &Document, id: LayerId) -> Option<LayerId> {
    let layer = doc.layer(id)?;
    match layer.kind {
        LayerKind::Paint => Some(id),
        LayerKind::Group => doc.members(Some(id)).into_iter().rev().find_map(|m| topmost_paint(doc, m)),
        LayerKind::Mask => None,
    }
}

/// A clip's keyed channel as the apps edit it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TimelineChannel {
    /// Moved, canvas pixels across and down.
    Position,
    /// Scaled, across and down (1: as drawn).
    Scale,
    /// Turned, degrees clockwise.
    Rotation,
    /// 0 to 1.
    Opacity,
}

/// How a key eases into the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TimelineEasing {
    Linear,
    Hold,
    Smooth,
    EaseIn,
    EaseOut,
    EaseInOut,
}

impl TimelineEasing {
    fn easing(self) -> make2d_engine::timeline::Easing {
        use make2d_engine::timeline::Easing;
        match self {
            TimelineEasing::Linear => Easing::Linear,
            TimelineEasing::Hold => Easing::Hold,
            TimelineEasing::Smooth => Easing::Smooth,
            TimelineEasing::EaseIn => Easing::CubicBezier([0.42, 0.0], [1.0, 1.0]),
            TimelineEasing::EaseOut => Easing::CubicBezier([0.0, 0.0], [0.58, 1.0]),
            TimelineEasing::EaseInOut => Easing::CubicBezier([0.42, 0.0], [0.58, 1.0]),
        }
    }

    fn of(e: &make2d_engine::timeline::Easing) -> TimelineEasing {
        use make2d_engine::timeline::Easing;
        match e {
            Easing::Hold => TimelineEasing::Hold,
            Easing::Smooth => TimelineEasing::Smooth,
            Easing::CubicBezier(a, b) if a[0] > 0.0 && b[0] >= 1.0 => TimelineEasing::EaseIn,
            Easing::CubicBezier(a, _) if a[0] <= 0.0 => TimelineEasing::EaseOut,
            Easing::CubicBezier(..) => TimelineEasing::EaseInOut,
            _ => TimelineEasing::Linear,
        }
    }
}

/// Where a clip is at the playhead, as its keys put it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelinePose {
    pub x: f32,
    pub y: f32,
    pub scale_x: f32,
    pub scale_y: f32,
    /// Degrees clockwise.
    pub rotation: f32,
    pub opacity: f32,
}

/// A key of a clip.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineKeyInfo {
    /// The timeline's frame it is on.
    pub frame: u32,
    pub channel: TimelineChannel,
    pub easing: TimelineEasing,
}

#[uniffi::export]
impl CanvasSession {
    /// Where clip `id` is at the playhead (inside it or not: its keys before and after hold).
    pub fn timeline_pose(&self, id: u64) -> Option<TimelinePose> {
        let st = self.shared.lock();
        let clip = st.doc.timeline.as_ref()?.clip(ClipId(id))?;
        let local = st.canvas.as_ref().map_or(0, |c| c.timeline_frame()) as f32 - clip.start as f32;
        let k = &clip.keys;
        let [x, y] = k.position.value_at(local).unwrap_or([0.0; 2]);
        let [scale_x, scale_y] = k.scale.value_at(local).unwrap_or([1.0; 2]);
        Some(TimelinePose {
            x,
            y,
            scale_x,
            scale_y,
            rotation: k.rotation.value_at(local).unwrap_or(0.0).to_degrees(),
            opacity: k.opacity_at(local).unwrap_or(clip.opacity),
        })
    }

    /// Clip `id`'s keys, in frame order.
    pub fn timeline_keys(&self, id: u64) -> Vec<TimelineKeyInfo> {
        let st = self.shared.lock();
        let Some(clip) = st.doc.timeline.as_ref().and_then(|t| t.clip(ClipId(id))) else { return Vec::new() };
        let at = |frame: i32| u32::try_from(i64::from(clip.start) + i64::from(frame)).unwrap_or(0);
        let k = &clip.keys;
        let mut keys: Vec<TimelineKeyInfo> = Vec::new();
        let mut add = |channel, frames: Vec<(i32, &make2d_engine::timeline::Easing)>| {
            keys.extend(frames.into_iter().map(|(f, e)| TimelineKeyInfo {
                frame: at(f),
                channel,
                easing: TimelineEasing::of(e),
            }));
        };
        add(TimelineChannel::Position, k.position.keys.iter().map(|k| (k.frame, &k.easing)).collect());
        add(TimelineChannel::Scale, k.scale.keys.iter().map(|k| (k.frame, &k.easing)).collect());
        add(TimelineChannel::Rotation, k.rotation.keys.iter().map(|k| (k.frame, &k.easing)).collect());
        add(TimelineChannel::Opacity, k.opacity.keys.iter().map(|k| (k.frame, &k.easing)).collect());
        keys.sort_by_key(|k| k.frame);
        keys
    }

    /// A key of `channel` for clip `id` at the playhead, `values` its value (Position and Scale
    /// two, Rotation and Opacity one), easing into the next as `easing`: shown at once, and one
    /// "Key" undo step when `done` (as a slider is let go). The first key that moves, scales or
    /// turns it sets what it turns about: the middle of what is painted in it.
    pub fn set_timeline_key(
        &self,
        id: u64,
        channel: TimelineChannel,
        values: Vec<f32>,
        easing: TimelineEasing,
        done: bool,
    ) -> bool {
        let (frame, middle) = {
            let st = self.shared.lock();
            let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
            let root = st.doc.timeline.as_ref().and_then(|t| t.clip(ClipId(id))).and_then(|c| match &c.content {
                ClipContent::Drawing(d) => Some(d.root),
                _ => None,
            });
            (frame, root.and_then(|r| st.painted_middle(r)))
        };
        let value = |i: usize| values.get(i).copied().filter(|v| v.is_finite());
        let changed = self.change_live("Key", done, |d| {
            let centre = [d.width() as f32 / 2.0, d.height() as f32 / 2.0];
            let Some(tl) = d.timeline.as_mut() else { return false };
            let Some((t, c)) = tl.find(ClipId(id)) else { return false };
            let Some(clip) = tl.tracks.get_mut(t).and_then(|t| t.clips.get_mut(c)) else { return false };
            let local = i32::try_from(i64::from(frame) - i64::from(clip.start)).unwrap_or(0);
            let k = &mut clip.keys;
            // a key already there keeps how it eases; a new one eases as asked
            let asked = easing.easing();
            let easing = match channel {
                TimelineChannel::Position => easing_at(&k.position.keys, local),
                TimelineChannel::Scale => easing_at(&k.scale.keys, local),
                TimelineChannel::Rotation => easing_at(&k.rotation.keys, local),
                TimelineChannel::Opacity => easing_at(&k.opacity.keys, local),
            }
            .unwrap_or(asked);
            match channel {
                TimelineChannel::Position => match (value(0), value(1)) {
                    (Some(x), Some(y)) => k.position.set(local, [x, y], easing),
                    _ => return false,
                },
                TimelineChannel::Scale => match (value(0), value(1)) {
                    (Some(x), Some(y)) => k.scale.set(local, [x.clamp(0.01, 100.0), y.clamp(0.01, 100.0)], easing),
                    _ => return false,
                },
                TimelineChannel::Rotation => match value(0) {
                    Some(deg) => k.rotation.set(local, deg.to_radians(), easing),
                    None => return false,
                },
                TimelineChannel::Opacity => match value(0) {
                    Some(o) => k.opacity.set(local, o.clamp(0.0, 1.0), easing),
                    None => return false,
                },
            }
            if k.posed() && k.pivot.is_none() {
                k.pivot = Some(middle.unwrap_or(centre));
            }
            true
        });
        if changed {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            if let Some(c) = st.canvas.as_mut() {
                c.invalidate();
            }
            self.shared.frame(st);
        }
        changed
    }

    /// Takes away clip `id`'s keys at the playhead (every channel's), as an undo step.
    pub fn remove_timeline_keys(&self, id: u64) -> bool {
        let frame = self.shared.lock().canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let removed = self
            .change("Remove Keys", |d| {
                let tl = d.timeline.as_mut()?;
                let (t, c) = tl.find(ClipId(id))?;
                let clip = tl.tracks.get_mut(t)?.clips.get_mut(c)?;
                let local = i32::try_from(i64::from(frame) - i64::from(clip.start)).ok()?;
                let k = &mut clip.keys;
                let any = [
                    k.position.remove(local),
                    k.scale.remove(local),
                    k.rotation.remove(local),
                    k.opacity.remove(local),
                ];
                if !k.posed() {
                    k.pivot = None;
                }
                any.contains(&true).then_some(())
            })
            .is_some();
        if removed {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            if let Some(c) = st.canvas.as_mut() {
                c.invalidate();
            }
            self.shared.frame(st);
        }
        removed
    }
}

impl State {
    /// The middle of what is painted in top-level layer or group `root`, canvas pixels.
    fn painted_middle(&self, root: LayerId) -> Option<[f32; 2]> {
        let (canvas, gpu) = (self.canvas.as_ref()?, self.gpu.as_ref()?);
        let layers: Vec<LayerId> =
            self.doc.pixel_layer_ids().filter(|l| *l == root || self.doc.is_inside(*l, root)).collect();
        let boxes = canvas.painted_boxes(gpu, &layers);
        let b = boxes.into_iter().flatten().reduce(|a, b| a.union(b))?;
        Some([(b.x0 + b.x1) as f32 / 2.0, (b.y0 + b.y1) as f32 / 2.0])
    }
}

/// The kinds of easing a key can have.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum EasingKind {
    Linear,
    Hold,
    Smooth,
    /// `params`: the two control points, x1 y1 x2 y2.
    CubicBezier,
    /// `params`: the drawn curve's points, x y each.
    Curve,
    /// `params`: first step, bounciness.
    Bounce,
    /// `params`: step, attack, decay, magnitude.
    Elastic,
    /// `params`: step, smoothing.
    Steps,
    /// `params`: step, magnitude.
    ElasticSteps,
    /// `params`: step, sharpness, skew, decay.
    Cyclic,
    /// `params`: frequency, magnitude, seed, attenuation, warp, tilt.
    Random,
}

/// An easing as the easing editor shows it: its kind and that kind's numbers, repeated across
/// its span `repeats` times (1: once) arriving or looping, and maybe played backwards.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct EasingSpec {
    pub kind: EasingKind,
    pub params: Vec<f32>,
    pub repeats: u32,
    /// Repeated back and forth (ending where it began) rather than arriving each time.
    pub looping: bool,
    pub reversed: bool,
}

/// What the motion path editor shows of a clip: its path, and its position keys with their
/// handles, all in canvas pixels.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct MotionPath {
    /// The path its pivot takes, x y each, along its position keys.
    pub points: Vec<f32>,
    pub keys: Vec<PathKey>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PathKey {
    /// The timeline's frame it is on.
    pub frame: u32,
    /// Where the pivot is there.
    pub x: f32,
    pub y: f32,
    /// Its handles (offsets from it), if it has any: toward the key before, toward the key after.
    pub handles: Option<Vec<f32>>,
}

use make2d_engine::timeline::{Easing, FitMode, HandleMode, PathHandles, RepeatFit};

/// The engine's easing for `s`.
fn easing_of(s: &EasingSpec) -> Easing {
    let p = |i: usize, or: f32| s.params.get(i).copied().filter(|v| v.is_finite()).unwrap_or(or);
    let base = match s.kind {
        EasingKind::Linear => Easing::Linear,
        EasingKind::Hold => Easing::Hold,
        EasingKind::Smooth => Easing::Smooth,
        EasingKind::CubicBezier => Easing::CubicBezier([p(0, 0.42), p(1, 0.0)], [p(2, 0.58), p(3, 1.0)]),
        EasingKind::Curve => Easing::Curve(s.params.as_chunks::<2>().0.to_vec()),
        EasingKind::Bounce => Easing::Bounce { first_step: p(0, 0.3), bounciness: p(1, 0.8) },
        EasingKind::Elastic => {
            Easing::Elastic { step: p(0, 0.25), attack: p(1, 1.0), decay: p(2, 0.5), magnitude: p(3, 1.0) }
        }
        EasingKind::Steps => Easing::Steps { step: p(0, 0.25), smoothing: p(1, 0.0) },
        EasingKind::ElasticSteps => Easing::ElasticSteps { step: p(0, 0.2), magnitude: p(1, 0.5) },
        EasingKind::Cyclic => {
            Easing::Cyclic { step: p(0, 2.0 / 7.0), sharpness: p(1, 0.0), skew: p(2, 0.5), decay: p(3, 0.0) }
        }
        EasingKind::Random => Easing::Random {
            frequency: p(0, 0.5),
            magnitude: p(1, 0.5),
            seed: p(2, 0.5),
            attenuation: p(3, 0.5),
            warp: p(4, 0.5),
            tilt: p(5, 0.0),
        },
    };
    let fitted = if s.repeats > 1 {
        let mode = if s.looping { FitMode::Loop } else { FitMode::Arrive };
        Easing::Fitted(Box::new(base), RepeatFit { mode, repetitions: s.repeats.min(64) })
    } else {
        base
    };
    if s.reversed { Easing::Reversed(Box::new(fitted)) } else { fitted }
}

/// `e` as the easing editor shows it.
fn spec_of(e: &Easing) -> EasingSpec {
    let plain = |kind, params: Vec<f32>| EasingSpec { kind, params, repeats: 1, looping: false, reversed: false };
    match e {
        Easing::Linear => plain(EasingKind::Linear, vec![]),
        Easing::Hold => plain(EasingKind::Hold, vec![]),
        Easing::Smooth => plain(EasingKind::Smooth, vec![]),
        Easing::CubicBezier(a, b) => plain(EasingKind::CubicBezier, vec![a[0], a[1], b[0], b[1]]),
        Easing::Curve(points) => plain(EasingKind::Curve, points.iter().flatten().copied().collect()),
        Easing::Bounce { first_step, bounciness } => plain(EasingKind::Bounce, vec![*first_step, *bounciness]),
        Easing::Elastic { step, attack, decay, magnitude } => {
            plain(EasingKind::Elastic, vec![*step, *attack, *decay, *magnitude])
        }
        Easing::Steps { step, smoothing } => plain(EasingKind::Steps, vec![*step, *smoothing]),
        Easing::ElasticSteps { step, magnitude } => plain(EasingKind::ElasticSteps, vec![*step, *magnitude]),
        Easing::Cyclic { step, sharpness, skew, decay } => {
            plain(EasingKind::Cyclic, vec![*step, *sharpness, *skew, *decay])
        }
        Easing::Random { frequency, magnitude, seed, attenuation, warp, tilt } => {
            plain(EasingKind::Random, vec![*frequency, *magnitude, *seed, *attenuation, *warp, *tilt])
        }
        Easing::Fitted(base, fit) => {
            EasingSpec { repeats: fit.repetitions, looping: fit.mode == FitMode::Loop, ..spec_of(base) }
        }
        Easing::Reversed(base) => EasingSpec { reversed: true, ..spec_of(base) },
    }
}

/// An easing of `kind` with its usual numbers, as the editor starts one.
#[uniffi::export]
pub fn default_easing(kind: EasingKind) -> EasingSpec {
    let params = match kind {
        EasingKind::Linear | EasingKind::Hold | EasingKind::Smooth => vec![],
        EasingKind::CubicBezier => vec![0.42, 0.0, 0.58, 1.0],
        EasingKind::Curve => vec![0.0, 0.0, 0.35, 0.28, 0.62, 1.18, 1.0, 1.0],
        EasingKind::Bounce => vec![0.3, 0.8],
        EasingKind::Elastic => vec![0.25, 1.0, 0.5, 1.0],
        EasingKind::Steps => vec![0.25, 0.0],
        EasingKind::ElasticSteps => vec![0.2, 0.5],
        EasingKind::Cyclic => vec![2.0 / 7.0, 0.0, 0.5, 0.0],
        EasingKind::Random => vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.0],
    };
    EasingSpec { kind, params, repeats: 1, looping: false, reversed: false }
}

/// `samples` points of easing `s` from start to end (how far along, at evenly spaced times), for
/// drawing it.
#[uniffi::export]
pub fn easing_curve(s: EasingSpec, samples: u32) -> Vec<f32> {
    let e = easing_of(&s);
    let n = samples.clamp(2, 1024);
    (0..n).map(|i| e.progress(i as f32 / (n - 1) as f32)).collect()
}

#[uniffi::export]
impl CanvasSession {
    /// How clip `id`'s key at timeline frame `frame` eases into the next: the first channel's
    /// key there (position, scale, rotation, opacity).
    pub fn timeline_key_easing(&self, id: u64, frame: u32) -> Option<EasingSpec> {
        let st = self.shared.lock();
        let clip = st.doc.timeline.as_ref()?.clip(ClipId(id))?;
        let local = i32::try_from(i64::from(frame) - i64::from(clip.start)).ok()?;
        let k = &clip.keys;
        let at = |e: Option<&Easing>| e.cloned();
        at(k.position.keys.iter().find(|x| x.frame == local).map(|x| &x.easing))
            .or_else(|| at(k.scale.keys.iter().find(|x| x.frame == local).map(|x| &x.easing)))
            .or_else(|| at(k.rotation.keys.iter().find(|x| x.frame == local).map(|x| &x.easing)))
            .or_else(|| at(k.opacity.keys.iter().find(|x| x.frame == local).map(|x| &x.easing)))
            .map(|e| spec_of(&e))
    }

    /// Every key of clip `id` at timeline frame `frame` eases into the next as `s`: shown at once,
    /// one "Easing" undo step when `done`.
    pub fn set_timeline_key_easing(&self, id: u64, frame: u32, s: EasingSpec, done: bool) -> bool {
        let easing = easing_of(&s);
        let changed = self.change_live("Easing", done, |d| {
            let Some(tl) = d.timeline.as_mut() else { return false };
            let Some((t, c)) = tl.find(ClipId(id)) else { return false };
            let Some(clip) = tl.tracks.get_mut(t).and_then(|t| t.clips.get_mut(c)) else { return false };
            let Ok(local) = i32::try_from(i64::from(frame) - i64::from(clip.start)) else { return false };
            let k = &mut clip.keys;
            let mut any = false;
            for key in k.position.keys.iter_mut().chain(k.scale.keys.iter_mut()).filter(|x| x.frame == local) {
                key.easing = easing.clone();
                any = true;
            }
            for key in k.rotation.keys.iter_mut().chain(k.opacity.keys.iter_mut()).filter(|x| x.frame == local) {
                key.easing = easing.clone();
                any = true;
            }
            any
        });
        if changed {
            self.redraw_timeline();
        }
        changed
    }

    /// Clip `id`'s motion path, if it has position keys.
    pub fn timeline_motion_path(&self, id: u64) -> Option<MotionPath> {
        let st = self.shared.lock();
        let clip = st.doc.timeline.as_ref()?.clip(ClipId(id))?;
        let keys = &clip.keys.position.keys;
        let (first, last) = (keys.first()?.frame, keys.last()?.frame);
        let [px, py] = clip.keys.pivot.unwrap_or([0.0; 2]);
        let steps = ((last - first).max(1) * 4).min(4000);
        let mut points = Vec::with_capacity(steps as usize * 2 + 2);
        for i in 0..=steps {
            let f = first as f32 + (last - first) as f32 * i as f32 / steps as f32;
            if let Some([x, y]) = clip.keys.position.position_at(f) {
                points.extend([px + x, py + y]);
            }
        }
        let at = |frame: i32| u32::try_from(i64::from(clip.start) + i64::from(frame)).unwrap_or(0);
        let keys = keys
            .iter()
            .map(|k| PathKey {
                frame: at(k.frame),
                x: px + k.value[0],
                y: py + k.value[1],
                handles: k.path.map(|h| vec![h.before[0], h.before[1], h.after[0], h.after[1]]),
            })
            .collect();
        Some(MotionPath { points, keys })
    }

    /// Clip `id`'s position key at timeline frame `frame` gets handles `before` and `after`
    /// (offsets, x y each), bending its motion path there; none takes them away (a corner).
    /// Shown at once, one "Motion Path" undo step when `done`.
    pub fn set_timeline_path_handles(&self, id: u64, frame: u32, handles: Option<Vec<f32>>, done: bool) -> bool {
        let handles = handles.and_then(|h| match h.as_slice() {
            [a, b, c, e] if [a, b, c, e].iter().all(|v| v.is_finite()) => {
                Some(PathHandles { mode: HandleMode::Symmetric, before: [*a, *b], after: [*c, *e] })
            }
            _ => None,
        });
        let changed = self.change_live("Motion Path", done, |d| {
            let Some(tl) = d.timeline.as_mut() else { return false };
            let Some((t, c)) = tl.find(ClipId(id)) else { return false };
            let Some(clip) = tl.tracks.get_mut(t).and_then(|t| t.clips.get_mut(c)) else { return false };
            let Ok(local) = i32::try_from(i64::from(frame) - i64::from(clip.start)) else { return false };
            match clip.keys.position.keys.iter_mut().find(|k| k.frame == local) {
                Some(k) => {
                    k.path = handles;
                    true
                }
                None => false,
            }
        });
        if changed {
            self.redraw_timeline();
        }
        changed
    }
}

impl CanvasSession {
    /// The canvas drawn again after a timeline change.
    fn redraw_timeline(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
    }
}

/// How the key at `frame` among `keys` eases, if there is one.
fn easing_at<T>(keys: &[make2d_engine::timeline::Key<T>], frame: i32) -> Option<Easing> {
    keys.iter().find(|k| k.frame == frame).map(|k| k.easing.clone())
}

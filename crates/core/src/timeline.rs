//! The canvas on a timeline (see the engine's `timeline`): putting it on one, its tracks and clips
//! as the apps show them, editing them, and the frame shown, drawn on or played. Animated exports
//! and `animation_frame` follow the timeline once there is one (see `animation`).
//!
//! The playhead and the track drawn on lead the current layer: moving either makes a layer of the
//! drawing there current, and choosing a layer moves them to its drawing. Painting where the track
//! has no drawing makes one.

use procraft_engine::animation::Playback;
use procraft_engine::document::{Document, LayerId, LayerKind};
use procraft_engine::timeline::{
    CameraPose, Clip, ClipContent, ClipId, Cycle, CycleKeys, MAX_EXPOSURE, MAX_FPS, MaskMode, Timeline, TrackId,
};

use crate::animation::AnimationPlayback;
use crate::{CanvasSession, State};

pub(crate) mod autokey;
mod clipboard;
mod edits;
mod gaps;
mod groups_inline;
mod key_lanes;
mod menus;
pub use menus::TimelineOnion;
mod moves;
pub(crate) mod panel;
mod perform;
mod picked;
mod playback;
#[cfg(test)]
mod tests_1g;
#[cfg(test)]
mod tests_autokey;
#[cfg(test)]
mod tests_edits;
#[cfg(test)]
mod tests_grid;
#[cfg(test)]
mod tests_groups_inline;
#[cfg(test)]
mod tests_key_lanes;
#[cfg(test)]
mod tests_menus;
#[cfg(test)]
mod tests_panel;
#[cfg(test)]
mod tests_picked;
#[cfg(test)]
mod tests_playback;
mod timing;

/// For the media tests, which trim a sound's start as any clip's.
#[cfg(test)]
pub(crate) use edits::TimelineEdgePolicy;

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
    /// Played frames show through the camera.
    pub camera_view: bool,
    /// The group clips opened, outermost first: the tracks shown are the last one's own (none,
    /// the timeline's).
    pub open_groups: Vec<TimelineClipInfo>,
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
    /// How the clip on the track below masks it.
    pub mask: TimelineMask,
    /// A group's: how many frames it plays over and over (0: it doesn't), and whether its own
    /// keys run on across the repeats rather than playing again each time.
    pub cycle: u32,
    pub continue_across: bool,
    /// Its own opacity (0..1), a blend mode id (see `blend_modes`), whether it is turned off, and
    /// the colour it is labelled in the panel (0xRRGGBB; none, the panel's own).
    pub opacity: f32,
    pub blend: u32,
    pub disabled: bool,
    pub label: Option<u32>,
}

/// How a clip is masked by the clip under it on the track below.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TimelineMask {
    None,
    /// Shows only where that one is painted, which still draws.
    Clipping,
    /// That one's alpha is a matte, and it no longer draws.
    Alpha,
    /// That one's lightness is a matte, and it no longer draws.
    Luminance,
}

impl TimelineMask {
    fn of(m: MaskMode) -> TimelineMask {
        match m {
            MaskMode::None => TimelineMask::None,
            MaskMode::Clipping => TimelineMask::Clipping,
            MaskMode::Alpha => TimelineMask::Alpha,
            MaskMode::Luminance => TimelineMask::Luminance,
        }
    }

    fn mode(self) -> MaskMode {
        match self {
            TimelineMask::None => MaskMode::None,
            TimelineMask::Clipping => MaskMode::Clipping,
            TimelineMask::Alpha => MaskMode::Alpha,
            TimelineMask::Luminance => MaskMode::Luminance,
        }
    }
}

/// Clip `c` as the apps show it, its start moved on by `offset` (where its level starts).
fn clip_info(c: &Clip, offset: u32) -> TimelineClipInfo {
    let (kind, root) = match &c.content {
        ClipContent::Drawing(d) => (TimelineClipKind::Drawing, d.root.0),
        ClipContent::Group(_) => (TimelineClipKind::Group, 0),
        ClipContent::Video(_) => (TimelineClipKind::Video, 0),
        ClipContent::Audio(_) => (TimelineClipKind::Audio, 0),
    };
    let cycle = match &c.content {
        ClipContent::Group(g) => g.cycle,
        _ => None,
    };
    TimelineClipInfo {
        id: c.id.0,
        name: c.name.clone(),
        start: c.start + offset,
        duration: c.duration,
        kind,
        root,
        mask: TimelineMask::of(c.mask),
        cycle: cycle.map_or(0, |c| c.length),
        continue_across: cycle.is_some_and(|c| c.keys == CycleKeys::ContinueAcross),
        opacity: c.opacity,
        blend: c.blend.id(),
        disabled: c.disabled,
        label: c.label.map(menus::packed),
    }
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineTrackInfo {
    pub id: u64,
    pub name: String,
    pub hidden: bool,
    pub locked: bool,
    /// In time order.
    pub clips: Vec<TimelineClipInfo>,
    /// Its opacity (0..1), over its clips'; whether it shows alone (see `solo_timeline_track`).
    pub opacity: f32,
    pub soloed: bool,
    /// The colour its name is tagged with in the panel (0xRRGGBB; none, the panel's own), and the
    /// blend mode (an id, see `blend_modes`) and mask of its clips that have none of their own.
    pub label: Option<u32>,
    pub blend: u32,
    pub mask: TimelineMask,
}

/// How far past the timeline's last frame the playhead may rest: on an empty frame there nothing
/// shows, and a drawing made there makes the timeline that much longer.
pub(crate) const PAST_END: u32 = 64;

fn playback_of(p: Playback) -> AnimationPlayback {
    match p {
        Playback::Loop => AnimationPlayback::Loop,
        Playback::PingPong => AnimationPlayback::PingPong,
        Playback::OneShot => AnimationPlayback::OneShot,
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Puts the canvas on a timeline, as an undo step: the one it was taken off before, as it
    /// was, or one made from it (Animation Assist's frames, or each layer for a still canvas).
    /// False if it is on one already, or is a Page Assist canvas.
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

    /// Takes the canvas off its timeline, as an undo step: its layers stay as they are, and the
    /// timeline is kept with the artwork, so [`CanvasSession::start_timeline`] brings it back as
    /// it was (tracks, timing, keys, sounds and videos).
    pub fn stop_timeline(&self) -> bool {
        let stopped = self.change("Stop Using Timeline", |d| d.stop_timeline().then_some(())).is_some();
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
            onion_frames: tl.onion.count[0].max(tl.onion.count[1]),
            onion_opacity: tl.onion.opacity,
            simple: tl.simple.is_some(),
            frame: st.canvas.as_ref().map_or(0, |c| c.timeline_frame()),
            track: st.drawn_track().map(|t| t.0),
            camera_view: st.canvas.as_ref().is_some_and(|c| c.camera_view()),
            open_groups: st
                .open_groups()
                .iter()
                .filter_map(|g| Some(clip_info(tl.clip(*g)?, tl.origin(*g)? - tl.clip(*g)?.start)))
                .collect(),
        })
    }

    /// The tracks shown, bottom first, with their clips: the open group's own (their frames as
    /// they are on the timeline, from the group's first frame), or the timeline's.
    pub fn timeline_tracks(&self) -> Vec<TimelineTrackInfo> {
        let st = self.shared.lock();
        let Some(tl) = st.doc.timeline.as_ref() else { return Vec::new() };
        let open = st.open_groups().last().copied();
        let (tracks, offset) = match open.and_then(|g| Some((tl.clip(g)?, tl.origin(g)?))) {
            Some((Clip { content: ClipContent::Group(g), .. }, at)) => (&g.tracks, at),
            _ => (&tl.tracks, 0),
        };
        tracks
            .iter()
            .map(|t| TimelineTrackInfo {
                id: t.id.0,
                name: t.name.clone(),
                hidden: t.hidden,
                locked: t.locked,
                clips: t.clips.iter().map(|c| clip_info(c, offset)).collect(),
                opacity: t.opacity,
                soloed: tl.soloed(t.id),
                label: t.look.label.map(menus::packed),
                blend: t.look.blend.id(),
                mask: TimelineMask::of(t.look.mask),
            })
            .collect()
    }

    /// Changes the settings (live, as sliders move: not an undo step).
    pub fn set_timeline_settings(&self, s: TimelineSettings) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(tl) = st.doc.timeline.as_mut() else { return };
        tl.fps = s.fps.clamp(1, MAX_FPS);
        // shorter than its clips if so (as the end flap's)
        let resized = tl.set_length(s.duration.min(MOST_FRAMES));
        tl.exposure = s.exposure.clamp(1, MAX_EXPOSURE);
        tl.playback = match s.playback {
            AnimationPlayback::Loop => Playback::Loop,
            AnimationPlayback::PingPong => Playback::PingPong,
            AnimationPlayback::OneShot => Playback::OneShot,
        };
        // the same count both sides, unless it is as it was (each side set apart, see
        // `set_timeline_onion`)
        if s.onion_frames != tl.onion.count[0].max(tl.onion.count[1]) {
            tl.onion.count = [s.onion_frames.min(procraft_engine::animation::MAX_ONION_FRAMES); 2];
        }
        tl.onion.opacity = if s.onion_opacity.is_finite() { s.onion_opacity.clamp(0.0, 1.0) } else { 0.4 };
        st.revision += 1;
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
        drop(guard);
        if resized {
            self.keep_playhead_within(0);
        }
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
        self.go_to_timeline_frame(frame, 0);
    }

    /// As [`CanvasSession::set_timeline_frame`], but the playhead may rest a little past the
    /// timeline's end (the grid shows empty frames there): nothing shows on one until something is
    /// drawn or added there, which makes the timeline that long.
    pub fn rest_timeline_frame(&self, frame: u32) {
        self.go_to_timeline_frame(frame, PAST_END);
    }

    /// Draws on track `id` (simple mode then shows it): a layer of its drawing at the playhead
    /// becomes current.
    pub fn select_timeline_track(&self, id: u64) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(tl) = st.doc.timeline.as_mut() else { return };
        if tl.track(TrackId(id)).is_none() {
            return;
        }
        if tl.simple.is_some() && tl.track_index(TrackId(id)).is_some() {
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

    /// A new empty track on top (of the open group's, if one is open), drawn on; returns its id
    /// (0 if there is no timeline).
    pub fn add_timeline_track(&self) -> u64 {
        let open = self.shared.lock().open_groups().last().copied();
        let id = self.change("Add Track", |d| {
            let tl = d.timeline.as_mut()?;
            match open {
                Some(g) => {
                    let n = match &tl.clip(g)?.content {
                        ClipContent::Group(gr) => gr.tracks.len(),
                        _ => 0,
                    };
                    tl.add_group_track(g, &format!("Track {}", n + 1))
                }
                None => {
                    let name = format!("Track {}", tl.tracks.len() + 1);
                    Some(tl.add_track(&name))
                }
            }
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
            let track = d.timeline.as_mut()?.track_mut(TrackId(id))?;
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
            d.timeline.as_mut()?.track_mut(TrackId(id))?.name = name;
            Some(())
        })
        .is_some()
    }

    /// Whether a timeline's new drawings start as vector layers, their strokes kept as lines,
    /// and not as paint layers. Off at first; the app keeps it.
    pub fn set_timeline_vector_frames(&self, on: bool) {
        self.shared.lock().vector_frames = on;
    }

    /// A new drawing (an empty layer, a vector one if new drawings start as those) at the playhead on the track drawn on (just after the
    /// drawing there, the ones after it moving along), made current, as an undo step; returns its
    /// clip, or 0 when it can't be made (no timeline, a locked track, no room for a layer).
    pub fn add_timeline_drawing(&self) -> u64 {
        let (frame, track, vector) = {
            let st = self.shared.lock();
            (st.canvas.as_ref().map_or(0, |c| c.timeline_frame()), st.drawn_track(), st.vector_frames)
        };
        let made = self.change("Add Drawing", |d| add_drawing(d, track, frame, vector));
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

    /// Clip `id` moved to track `track` (of the same level), as near timeline frame `start` as
    /// it fits, as an undo step.
    pub fn move_timeline_clip(&self, id: u64, track: u64, start: u32) -> bool {
        self.change("Move Clip", |d| {
            let tl = d.timeline.as_mut()?;
            let start = start.saturating_sub(tl.level_start(TrackId(track))?);
            let before = (tl.track_of_clip(ClipId(id))?, tl.clip(ClipId(id))?.start);
            if before == (TrackId(track), start) {
                return None;
            }
            tl.move_clip(ClipId(id), TrackId(track), start).then_some(())
        })
        .is_some()
    }

    /// Clip `id` cut at the playhead: a drawing's is copied for the rest, a sound or video plays
    /// on from there, and a group becomes two groups, the clips inside it that the cut goes
    /// through cut the same way. False unless the playhead is inside it (not on its first frame);
    /// a repeating group isn't cut.
    pub fn split_timeline_clip(&self, id: u64) -> bool {
        let (frame, root, media, group) = {
            let st = self.shared.lock();
            let tl = st.doc.timeline.as_ref();
            let clip = tl.and_then(|t| t.clip(ClipId(id)));
            // the playhead on the clip's own level
            let level = tl.and_then(|t| t.origin(ClipId(id))).zip(clip).map(|(o, c)| o - c.start);
            let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame()).saturating_sub(level.unwrap_or(0));
            let inside = clip.filter(|c| frame > c.start && frame < c.end());
            let root = inside.and_then(|c| match &c.content {
                ClipContent::Drawing(d) => Some(d.root),
                _ => None,
            });
            let media = inside.is_some_and(|c| matches!(c.content, ClipContent::Audio(_) | ClipContent::Video(_)));
            (frame, root, media, inside.is_some_and(|c| matches!(c.content, ClipContent::Group(_))))
        };
        if group {
            return self.split_timeline_group(ClipId(id), frame);
        }
        if media {
            return self.change("Split", |d| d.timeline.as_mut()?.split_media(ClipId(id), frame)).is_some();
        }
        let Some(root) = root else { return false };
        self.duplicated(root.0, "Split", |d, copy| {
            if let Some(tl) = d.timeline.as_mut() {
                tl.split(ClipId(id), frame, copy);
            }
        })
        .is_some()
    }

}

impl CanvasSession {
    /// Group clip `id` cut at frame `frame` of its level, as one "Split" undo step: each drawing
    /// the cut goes through gets a copy of its layers for its second part. False (nothing changed)
    /// if it can't be cut there, its track is locked, or there is no room for the copies.
    fn split_timeline_group(&self, id: ClipId, frame: u32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let Some(tl) = st.doc.timeline.as_ref() else { return false };
        if tl.track_of_clip(id).and_then(|t| tl.track(t)).is_none_or(|t| t.locked) {
            return false;
        }
        let through = tl.group_cut_drawings(id, frame);
        // (tried first with the drawings themselves standing in for their copies: where it can't
        // be cut, nothing is copied)
        let stand_ins = through.iter().copied().collect();
        if tl.clone().split_group(id, frame, &stand_ins).is_none() {
            return false;
        }
        let before = st.doc.clone();
        let current = st.doc.current();
        let mut copies = std::collections::HashMap::new();
        let mut pairs = Vec::new();
        for (clip, root) in through {
            let Some((copy, made)) = st.doc.duplicate(root) else {
                st.doc = before;
                return false;
            };
            copies.insert(clip, copy);
            pairs.extend(made);
        }
        if st.doc.timeline.as_mut().and_then(|tl| tl.split_group(id, frame, &copies)).is_none() {
            st.doc = before;
            return false;
        }
        st.doc.set_current(current);
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("Split");
            canvas.duplicate_layers(pairs, before);
        }
        st.revision += 1;
        self.shared.frame(st);
        true
    }
}

#[uniffi::export]
impl CanvasSession {
    /// A copy of clip `id` (a drawing's layers, or a sound or video) just after it, as long, the
    /// clips after it moving along; returns the copy's clip, or 0.
    pub fn duplicate_timeline_clip(&self, id: u64) -> u64 {
        let (root, media) = {
            let st = self.shared.lock();
            let clip = st.doc.timeline.as_ref().and_then(|t| t.clip(ClipId(id)));
            let root = clip.and_then(|c| match &c.content {
                ClipContent::Drawing(d) => Some(d.root),
                _ => None,
            });
            (root, clip.is_some_and(|c| matches!(c.content, ClipContent::Audio(_) | ClipContent::Video(_))))
        };
        if media {
            let made = self.change("Duplicate", |d| d.timeline.as_mut()?.duplicate_media(ClipId(id)));
            return made.map_or(0, |c| c.0);
        }
        let Some(root) = root else { return 0 };
        let mut made = 0;
        self.duplicated(root.0, "Duplicate Drawing", |d, copy| {
            if let Some(clip) = d.timeline.as_mut().and_then(|tl| tl.duplicate(ClipId(id), copy)) {
                made = clip.0;
            }
        });
        made
    }

    /// Clip `id` taken off its track with its drawing's layers, or a group's with every drawing's
    /// in it (the last layer stays), as an undo step. `close`: the clips after it move back over
    /// its place.
    pub fn delete_timeline_clip(&self, id: u64, close: bool) -> bool {
        let name = {
            let st = self.shared.lock();
            match st.doc.timeline.as_ref().and_then(|tl| tl.clip(ClipId(id))).map(|c| &c.content) {
                Some(ClipContent::Group(_)) => "Delete Group",
                Some(ClipContent::Audio(_)) => "Delete Sound",
                Some(ClipContent::Video(_)) => "Delete Video",
                _ => "Delete Drawing",
            }
        };
        let done = self
            .change(name, |d| {
                let clip = d.timeline.as_mut()?.remove(ClipId(id), close)?;
                for root in clip.roots() {
                    d.delete_layer(root);
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

/// A new drawing on `track` (or the first track, one made if there is none) at `frame`, in `d`,
/// its layer a vector one if `vector`: (its track, its clip, its first frame).
fn add_drawing(d: &mut Document, track: Option<TrackId>, frame: u32, vector: bool) -> Option<(TrackId, ClipId, u32)> {
    if !d.has_room(1) {
        return None;
    }
    let tl = d.timeline.as_mut()?;
    let track = match track.filter(|t| tl.track(*t).is_some()) {
        Some(t) => t,
        None => match tl.tracks.first() {
            Some(t) => t.id,
            None => tl.add_track("Track 1"),
        },
    };
    if tl.track(track)?.locked {
        return None;
    }
    // on the track's own level
    let level = tl.level_start(track)?;
    let frame = frame.saturating_sub(level);
    let n = tl.drawing_roots().len() + 1;
    // a top-level layer, above the drawing current now
    if let Some(top) = d.top_of(d.current()) {
        d.set_current(top);
    }
    let layer = if vector { d.add_vector_layer() } else { d.add_layer() };
    let name = format!("Drawing {n}");
    d.rename(layer, &name);
    let tl = d.timeline.as_mut()?;
    let clip = tl.add_drawing(track, frame, layer, &name)?;
    let start = tl.clip(clip)?.start + level;
    // a drawing is a group from the start (the layers panel shows it as one, to add layers to),
    // its first layer in it and current
    d.drawing_group(layer)?;
    Some((track, clip, start))
}

impl CanvasSession {
    /// The playhead on `frame`, no further than `past` frames past the timeline's last.
    fn go_to_timeline_frame(&self, frame: u32, past: u32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(last) = st
            .doc
            .timeline
            .as_ref()
            .map(|t| (t.duration.saturating_sub(1) + past).min(MOST_FRAMES.saturating_sub(1)))
        else {
            return;
        };
        if let Some(c) = st.canvas.as_mut() {
            c.set_timeline_frame(frame.min(last));
        }
        st.follow_playhead();
        self.shared.frame(st);
    }

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
        let Some(start) = tl.clip(clip).zip(tl.origin(clip)).map(|(c, o)| (o, o + c.duration)) else { return };
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
        self.timeline_track.filter(|t| tl.track(*t).is_some()).or(tl.simple).or_else(|| tl.tracks.first().map(|t| t.id))
    }

    /// Whether the current layer is in the drawing at the playhead on the track drawn on.
    fn current_drawing_shown(&self) -> bool {
        let Some(tl) = self.doc.timeline.as_ref() else { return true };
        let frame = self.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(top) = self.doc.top_of(self.doc.current()) else { return false };
        let Some(t) = self.drawn_track() else { return false };
        matches!(clip_on(tl, t, frame).map(|c| &c.content), Some(ClipContent::Drawing(d)) if d.root == top)
    }

    /// The current layer goes to the drawing at the playhead on the track drawn on (unless it is
    /// in it already): its topmost paint layer, or the drawing itself.
    pub(crate) fn follow_playhead(&mut self) {
        let Some(tl) = self.doc.timeline.as_ref() else { return };
        let frame = self.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(t) = self.drawn_track() else { return };
        let Some(ClipContent::Drawing(d)) = clip_on(tl, t, frame).map(|c| &c.content) else {
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

/// The clip on track `track` (at any depth) when the timeline is at `frame`.
fn clip_on(tl: &Timeline, track: TrackId, frame: u32) -> Option<&Clip> {
    let at = u32::try_from(tl.track_frame(track, frame)?).ok()?;
    tl.track(track)?.clip_at(at)
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
    /// An adjustment clip's adjustment values (keyed by `set_timeline_adjustment_value`).
    Adjustment,
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
    pub(crate) fn easing(self) -> procraft_engine::timeline::Easing {
        use procraft_engine::timeline::Easing;
        match self {
            TimelineEasing::Linear => Easing::Linear,
            TimelineEasing::Hold => Easing::Hold,
            TimelineEasing::Smooth => Easing::Smooth,
            TimelineEasing::EaseIn => Easing::CubicBezier([0.42, 0.0], [1.0, 1.0]),
            TimelineEasing::EaseOut => Easing::CubicBezier([0.0, 0.0], [0.58, 1.0]),
            TimelineEasing::EaseInOut => Easing::CubicBezier([0.42, 0.0], [0.58, 1.0]),
        }
    }

    fn of(e: &procraft_engine::timeline::Easing) -> TimelineEasing {
        use procraft_engine::timeline::Easing;
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
        let tl = st.doc.timeline.as_ref()?;
        let clip = tl.clip(ClipId(id))?;
        let local = tl.local_frame(ClipId(id), st.canvas.as_ref().map_or(0, |c| c.timeline_frame()))? as f32;
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
        let Some(tl) = st.doc.timeline.as_ref() else { return Vec::new() };
        let (Some(clip), Some(origin)) = (tl.clip(ClipId(id)), tl.origin(ClipId(id))) else { return Vec::new() };
        let at = |frame: i32| u32::try_from(i64::from(origin) + i64::from(frame)).unwrap_or(0);
        let k = &clip.keys;
        let mut keys: Vec<TimelineKeyInfo> = Vec::new();
        let mut add = |channel, frames: Vec<(i32, &procraft_engine::timeline::Easing)>| {
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
        add(TimelineChannel::Adjustment, k.adjustment.keys.iter().map(|k| (k.frame, &k.easing)).collect());
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
        let (local, middle) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            // (a stroke just ended is on its layer before what it paints is measured)
            st.flush();
            let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
            let tl = st.doc.timeline.as_ref();
            let root = tl.and_then(|t| t.clip(ClipId(id))).and_then(|c| match &c.content {
                ClipContent::Drawing(d) => Some(d.root),
                _ => None,
            });
            let local = tl.and_then(|t| t.local_frame(ClipId(id), frame)).and_then(|l| i32::try_from(l).ok());
            (local, root.and_then(|r| st.painted_middle(r)))
        };
        let Some(local) = local else { return false };
        let value = |i: usize| values.get(i).copied().filter(|v| v.is_finite());
        let changed = self.change_live("Key", done, |d| {
            let centre = [d.width() as f32 / 2.0, d.height() as f32 / 2.0];
            let Some(clip) = d.timeline.as_mut().and_then(|tl| tl.clip_mut(ClipId(id))) else { return false };
            let k = &mut clip.keys;
            // a key already there keeps how it eases; a new one eases as asked
            let asked = easing.easing();
            let easing = match channel {
                TimelineChannel::Position => easing_at(&k.position.keys, local),
                TimelineChannel::Scale => easing_at(&k.scale.keys, local),
                TimelineChannel::Rotation => easing_at(&k.rotation.keys, local),
                TimelineChannel::Opacity => easing_at(&k.opacity.keys, local),
                TimelineChannel::Adjustment => easing_at(&k.adjustment.keys, local),
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
                // keyed a value at a time, knowing its layer (see `set_timeline_adjustment_value`)
                TimelineChannel::Adjustment => return false,
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
                let local = i32::try_from(tl.local_frame(ClipId(id), frame)?).ok()?;
                let clip = tl.clip_mut(ClipId(id))?;
                let k = &mut clip.keys;
                let any = [
                    k.position.remove(local),
                    k.scale.remove(local),
                    k.rotation.remove(local),
                    k.opacity.remove(local),
                    k.adjustment.remove(local),
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

use procraft_engine::geom::{Affine, Vec2};
use procraft_engine::timeline::{Easing, FitMode, HandleMode, PathHandles, RepeatFit};

/// An offset `d` on the canvas taken back through `pose`'s scale and turn (not its move); none
/// if `pose` flattens everything.
fn unturned(pose: Affine, d: [f32; 2]) -> Option<[f32; 2]> {
    let back = Affine { e: 0.0, f: 0.0, ..pose }.inverse()?;
    let v = back.apply(Vec2::new(d[0], d[1]));
    Some([v.x, v.y])
}

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
        let tl = st.doc.timeline.as_ref()?;
        let clip = tl.clip(ClipId(id))?;
        let local = i32::try_from(i64::from(frame) - i64::from(tl.origin(ClipId(id))?)).ok()?;
        let k = &clip.keys;
        let at = |e: Option<&Easing>| e.cloned();
        at(k.position.keys.iter().find(|x| x.frame == local).map(|x| &x.easing))
            .or_else(|| at(k.scale.keys.iter().find(|x| x.frame == local).map(|x| &x.easing)))
            .or_else(|| at(k.rotation.keys.iter().find(|x| x.frame == local).map(|x| &x.easing)))
            .or_else(|| at(k.opacity.keys.iter().find(|x| x.frame == local).map(|x| &x.easing)))
            .or_else(|| at(k.adjustment.keys.iter().find(|x| x.frame == local).map(|x| &x.easing)))
            .map(|e| spec_of(&e))
    }

    /// Every key of clip `id` at timeline frame `frame` eases into the next as `s`: shown at once,
    /// one "Easing" undo step when `done`.
    pub fn set_timeline_key_easing(&self, id: u64, frame: u32, s: EasingSpec, done: bool) -> bool {
        let easing = easing_of(&s);
        let changed = self.change_live("Easing", done, |d| {
            let Some(tl) = d.timeline.as_mut() else { return false };
            let Some(origin) = tl.origin(ClipId(id)) else { return false };
            let Some(clip) = tl.clip_mut(ClipId(id)) else { return false };
            let Ok(local) = i32::try_from(i64::from(frame) - i64::from(origin)) else { return false };
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
            for key in k.adjustment.keys.iter_mut().filter(|x| x.frame == local) {
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

    /// Clip `id`'s motion path, if it has position keys: where it shows, through the poses of
    /// the groups it is in.
    pub fn timeline_motion_path(&self, id: u64) -> Option<MotionPath> {
        let st = self.shared.lock();
        let tl = st.doc.timeline.as_ref()?;
        let clip = tl.clip(ClipId(id))?;
        let origin = tl.origin(ClipId(id))?;
        let keys = &clip.keys.position.keys;
        let (first, last) = (keys.first()?.frame, keys.last()?.frame);
        let [px, py] = clip.keys.pivot.unwrap_or([0.0; 2]);
        // on the canvas, at the clip's frame `f`
        let shown = |f: f32, x: f32, y: f32| match tl.groups_pose(ClipId(id), f) {
            Some(pose) => {
                let p = pose.apply(Vec2::new(x, y));
                [p.x, p.y]
            }
            None => [x, y],
        };
        let steps = ((last - first).max(1) * 4).min(4000);
        let mut points = Vec::with_capacity(steps as usize * 2 + 2);
        for i in 0..=steps {
            let f = first as f32 + (last - first) as f32 * i as f32 / steps as f32;
            if let Some([x, y]) = clip.keys.position.position_at(f) {
                points.extend(shown(f, px + x, py + y));
            }
        }
        let at = |frame: i32| u32::try_from(i64::from(origin) + i64::from(frame)).unwrap_or(0);
        let keys = keys
            .iter()
            .map(|k| {
                let (x, y) = (px + k.value[0], py + k.value[1]);
                let [kx, ky] = shown(k.frame as f32, x, y);
                let arm = |h: [f32; 2]| {
                    let [hx, hy] = shown(k.frame as f32, x + h[0], y + h[1]);
                    [hx - kx, hy - ky]
                };
                PathKey {
                    frame: at(k.frame),
                    x: kx,
                    y: ky,
                    handles: k.path.map(|h| [arm(h.before), arm(h.after)].concat()),
                }
            })
            .collect();
        Some(MotionPath { points, keys })
    }

    /// A move of `dx`, `dy` on the canvas at timeline frame `frame` as clip `id`'s position keys
    /// count it: through the groups it is in, which may scale and turn it (as it is, in none).
    pub fn timeline_motion_delta(&self, id: u64, frame: u32, dx: f32, dy: f32) -> Vec<f32> {
        let st = self.shared.lock();
        let local = st.doc.timeline.as_ref().and_then(|tl| {
            let at = i64::from(frame) - i64::from(tl.origin(ClipId(id))?);
            let pose = tl.groups_pose(ClipId(id), at as f32)?;
            let d = unturned(pose, [dx, dy])?;
            Some(vec![d[0], d[1]])
        });
        local.unwrap_or_else(|| vec![dx, dy])
    }

    /// Clip `id`'s position key at timeline frame `frame` gets handles `before` and `after`
    /// (offsets on the canvas, x y each, as the path shows them), bending its motion path there;
    /// none takes them away (a corner). Shown at once, one "Motion Path" undo step when `done`.
    pub fn set_timeline_path_handles(&self, id: u64, frame: u32, handles: Option<Vec<f32>>, done: bool) -> bool {
        let handles = handles.and_then(|h| match h.as_slice() {
            [a, b, c, e] if [a, b, c, e].iter().all(|v| v.is_finite()) => {
                Some(PathHandles { mode: HandleMode::Symmetric, before: [*a, *b], after: [*c, *e] })
            }
            _ => None,
        });
        let changed = self.change_live("Motion Path", done, |d| {
            let Some(tl) = d.timeline.as_mut() else { return false };
            let Some(origin) = tl.origin(ClipId(id)) else { return false };
            let Ok(local) = i32::try_from(i64::from(frame) - i64::from(origin)) else { return false };
            // back through the groups' poses, onto the clip's own
            let handles = match (handles, tl.groups_pose(ClipId(id), local as f32)) {
                (Some(h), Some(pose)) => match (unturned(pose, h.before), unturned(pose, h.after)) {
                    (Some(before), Some(after)) => Some(PathHandles { before, after, ..h }),
                    _ => return false,
                },
                (h, _) => h,
            };
            let Some(clip) = tl.clip_mut(ClipId(id)) else { return false };
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
    pub(crate) fn redraw_timeline(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
    }
}

/// How the key at `frame` among `keys` eases, if there is one.
fn easing_at<T>(keys: &[procraft_engine::timeline::Key<T>], frame: i32) -> Option<Easing> {
    keys.iter().find(|k| k.frame == frame).map(|k| k.easing.clone())
}

/// The camera at the playhead, as the apps show and change it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineCamera {
    /// The canvas point in the middle of the frame, canvas pixels.
    pub x: f32,
    pub y: f32,
    /// 2: twice as close.
    pub zoom: f32,
    /// How far the canvas seems turned in the frame, degrees clockwise.
    pub rotation: f32,
    /// Whether the camera has a key at the playhead.
    pub keyed: bool,
    /// Where it looks on the canvas: the frame's corners (top left, top right, bottom right,
    /// bottom left), x y each, canvas pixels.
    pub corners: Vec<f32>,
}

#[uniffi::export]
impl CanvasSession {
    /// Clip `id` masked by the clip under it on the track below as `mask`, as an undo step.
    pub fn set_timeline_clip_mask(&self, id: u64, mask: TimelineMask) -> bool {
        let changed = self
            .change("Mask", |d| {
                let clip = d.timeline.as_mut()?.clip_mut(ClipId(id))?;
                (clip.mask != mask.mode()).then(|| clip.mask = mask.mode())
            })
            .is_some();
        if changed {
            self.redraw_timeline();
        }
        changed
    }

    /// Clips `ids` (on tracks of one level) made into a group clip spanning them, as an undo
    /// step: their tracks become its own. Returns the group, or 0 if they can't be grouped.
    pub fn group_timeline_clips(&self, ids: Vec<u64>) -> u64 {
        let ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
        let made = self.change("Group Clips", |d| {
            let tl = d.timeline.as_mut()?;
            let n = count_groups(&tl.tracks) + 1;
            tl.group(&ids, &format!("Group {n}"))
        });
        let Some(group) = made else { return 0 };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.timeline_track = st.doc.timeline.as_ref().and_then(|t| t.track_of_clip(group)).or(st.timeline_track);
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
        group.0
    }

    /// Group clip `id` taken apart, its clips back on the level it is on with its keys folded
    /// into them and its cycle laid out (each repeat of a drawing a copy of its layers, while
    /// there is room for them), as an undo step.
    pub fn ungroup_timeline_clip(&self, id: u64) -> bool {
        let done = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let before = st.doc.clone();
            let Some(mut tl) = st.doc.timeline.take() else { return false };
            let mut pairs = Vec::new();
            let doc = &mut st.doc;
            let done = tl.ungroup_copying(ClipId(id), &mut |root| {
                let layers = doc.unit_layers(root)?;
                let units = layers.iter().filter(|l| l.kind != LayerKind::Group).count();
                if !doc.has_room(units) {
                    return None;
                }
                let (copy, p) = doc.insert_copy(&layers, root)?;
                pairs.extend(p);
                Some(copy)
            });
            st.doc.timeline = Some(tl);
            if done && let Some(canvas) = st.canvas.as_mut() {
                canvas.name_next_step("Ungroup Clips");
                if pairs.is_empty() {
                    canvas.record_document(before);
                } else {
                    canvas.duplicate_layers(pairs, before);
                }
            }
            if done {
                st.revision += 1;
            }
            done
        };
        if done {
            self.redraw_timeline();
        }
        done
    }

    /// Opens group clip `id` in the panel (its tracks shown and drawn on), or with none, goes
    /// back to the timeline's own tracks. False if `id` isn't a group.
    pub fn open_timeline_group(&self, id: Option<u64>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(tl) = st.doc.timeline.as_ref() else { return false };
        let open = match id.map(ClipId) {
            Some(g) => match tl.clip(g).map(|c| &c.content) {
                Some(ClipContent::Group(gr)) => {
                    st.timeline_track = gr.tracks.first().map(|t| t.id).or(st.timeline_track);
                    Some(g)
                }
                _ => return false,
            },
            None => {
                // drawn on: the track the group was on
                st.timeline_track = st.timeline_open.and_then(|g| tl.track_of_clip(g)).or(st.timeline_track);
                None
            }
        };
        st.timeline_open = open;
        st.follow_playhead();
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
        true
    }

    /// Group clip `id` plays its first `length` frames over and over (0: once, as it is), its own
    /// keys running on across the repeats when `continue_across` (else playing again each time),
    /// as an undo step.
    pub fn set_timeline_cycle(&self, id: u64, length: u32, continue_across: bool) -> bool {
        let keys = if continue_across { CycleKeys::ContinueAcross } else { CycleKeys::Repeat };
        let cycle = (length > 0).then(|| Cycle { length: length.min(MOST_FRAMES), keys });
        let changed = self
            .change("Cycle", |d| match &mut d.timeline.as_mut()?.clip_mut(ClipId(id))?.content {
                ClipContent::Group(g) if g.cycle != cycle => {
                    g.cycle = cycle;
                    Some(())
                }
                _ => None,
            })
            .is_some();
        if changed {
            self.redraw_timeline();
        }
        changed
    }

    /// The camera at the playhead: as its keys put it, or the whole canvas without keys.
    pub fn timeline_camera(&self) -> Option<TimelineCamera> {
        let st = self.shared.lock();
        let tl = st.doc.timeline.as_ref()?;
        let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let (w, h) = (st.doc.width() as f32, st.doc.height() as f32);
        let pose = tl.camera_at(frame as f32).unwrap_or(CameraPose::still(w, h));
        let keyed = tl.camera.keys.iter().any(|k| i64::from(k.frame) == i64::from(frame));
        Some(TimelineCamera {
            x: pose.center[0],
            y: pose.center[1],
            zoom: pose.zoom,
            rotation: pose.rotation.to_degrees(),
            keyed,
            corners: pose.corners(w, h).into_iter().flatten().collect(),
        })
    }

    /// The frames the camera has keys on, in order.
    pub fn timeline_camera_keys(&self) -> Vec<u32> {
        let st = self.shared.lock();
        let Some(tl) = st.doc.timeline.as_ref() else { return Vec::new() };
        tl.camera.keys.iter().filter_map(|k| u32::try_from(k.frame).ok()).collect()
    }

    /// A camera key at the playhead: looking at canvas point (`x`, `y`), `zoom` close, the canvas
    /// turned `rotation` degrees, easing into the next as `easing` (a key there keeps its own).
    /// Shown at once, one "Camera Key" undo step when `done`.
    pub fn set_timeline_camera_key(
        &self,
        x: f32,
        y: f32,
        zoom: f32,
        rotation: f32,
        easing: TimelineEasing,
        done: bool,
    ) -> bool {
        if ![x, y, zoom, rotation].iter().all(|v| v.is_finite()) {
            return false;
        }
        let frame = self.shared.lock().canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Ok(frame) = i32::try_from(frame) else { return false };
        let pose = CameraPose {
            center: [x, y],
            zoom: zoom.clamp(procraft_engine::timeline::CAMERA_ZOOM.0, procraft_engine::timeline::CAMERA_ZOOM.1),
            rotation: rotation.to_radians(),
        };
        let changed = self.change_live("Camera Key", done, |d| {
            let Some(tl) = d.timeline.as_mut() else { return false };
            let easing = easing_at(&tl.camera.keys, frame).unwrap_or(easing.easing());
            tl.camera.set(frame, pose, easing);
            true
        });
        if changed {
            self.redraw_timeline();
        }
        changed
    }

    /// Takes away the camera key at the playhead, as an undo step.
    pub fn remove_timeline_camera_key(&self) -> bool {
        let frame = self.shared.lock().canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Ok(frame) = i32::try_from(frame) else { return false };
        let removed =
            self.change("Remove Camera Key", |d| d.timeline.as_mut()?.camera.remove(frame).then_some(())).is_some();
        if removed {
            self.redraw_timeline();
        }
        removed
    }

    /// Played frames show through the camera (or the canvas as it is). Exports always do.
    pub fn set_timeline_camera_view(&self, on: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(c) = st.canvas.as_mut() {
            c.set_camera_view(on);
        }
        self.shared.frame(st);
    }
}

impl State {
    /// The group clips open in the panel, outermost first (none if the one opened has gone).
    pub(crate) fn open_groups(&self) -> Vec<ClipId> {
        let (Some(tl), Some(open)) = (self.doc.timeline.as_ref(), self.timeline_open) else { return Vec::new() };
        if !matches!(tl.clip(open).map(|c| &c.content), Some(ClipContent::Group(_))) {
            return Vec::new();
        }
        let mut path = vec![open];
        while let Some(g) = path.last().and_then(|g| tl.group_of(*g)) {
            path.push(g);
        }
        path.reverse();
        path
    }
}

/// How many group clips `tracks` hold, at any depth.
fn count_groups(tracks: &[procraft_engine::timeline::Track]) -> usize {
    tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(|c| match &c.content {
            ClipContent::Group(g) => 1 + count_groups(&g.tracks),
            _ => 0,
        })
        .sum()
}

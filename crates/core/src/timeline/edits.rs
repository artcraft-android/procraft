//! Clips moved into and out of group clips, a sound's level keyed over time, and the layers
//! panel's + on a timeline (a layer in the drawing at the playhead).

use procraft_engine::timeline::{Clip, ClipContent, ClipId, Easing, Edge, EdgePolicy, Timeline, TrackId};

use super::TimelineClipInfo;
use crate::CanvasSession;

/// What dragging a clip's edge does when it reaches the clip next to it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TimelineEdgePolicy {
    /// It stops there.
    Stop,
    /// The clips beyond that edge move with it, the gaps between them kept.
    Push,
    /// It trims into that clip, which gets shorter; nothing else moves.
    Trim,
}

impl TimelineEdgePolicy {
    pub(super) fn policy(self) -> EdgePolicy {
        match self {
            TimelineEdgePolicy::Stop => EdgePolicy::Clamp,
            TimelineEdgePolicy::Push => EdgePolicy::Push,
            TimelineEdgePolicy::Trim => EdgePolicy::TrimNeighbour,
        }
    }
}

/// Clip `id`'s start (`leading`) or end dragged to timeline frame `frame` in `tl`; its track, if
/// that changed anything (never on a locked track).
fn trimmed(tl: &mut Timeline, id: u64, leading: bool, frame: u32, policy: TimelineEdgePolicy) -> Option<TrackId> {
    let id = ClipId(id);
    let level = tl.origin(id)?.checked_sub(tl.clip(id)?.start)?;
    let track = tl.track_of_clip(id)?;
    if tl.track(track)?.locked {
        return None;
    }
    let edge = if leading { Edge::Start } else { Edge::End };
    tl.trim(id, edge, frame.saturating_sub(level), policy.policy()).then_some(track)
}

/// The timeline made as long as its clips, where it is shorter: after what makes a clip last
/// longer where every frame shows as its own (the grid), so none is left past the end.
pub(super) fn hold_clips(tl: &mut Timeline) {
    let end = tl.content_end().min(super::MOST_FRAMES);
    if end > tl.duration {
        tl.set_length(end);
    }
}

/// Clip `id`'s start (`leading`) or end dragged to timeline frame `frame` in `tl` as the grid's
/// handles drag it. An end pushes the clips after it along, the timeline growing to hold them. A
/// start dragged earlier pushes the clips before it earlier, the nearest first, into the empty
/// frames before them ([`Timeline::close_up_before`]); asked earlier than they have room for
/// (before the timeline's first frame, even), the clip lasts as many frames longer at its end as
/// its start fell short, the clips after it pushed along, so it gets as long as was asked either
/// way. A start dragged later leaves the clips before it where they are. Its track, if that
/// changed anything (never on a locked track).
pub(super) fn lengthened(tl: &mut Timeline, id: u64, leading: bool, frame: i32) -> Option<TrackId> {
    let id = ClipId(id);
    let level = tl.origin(id)?.checked_sub(tl.clip(id)?.start)?;
    let track = tl.track_of_clip(id)?;
    if tl.track(track)?.locked {
        return None;
    }
    let asked = i64::from(frame) - i64::from(level);
    let to = u32::try_from(asked.max(0)).unwrap_or(0);
    let changed = if leading {
        let before = tl.clip(id)?.start;
        // (a cycling group's start stays put, so nothing is moved out of its way)
        let cycles = matches!(&tl.clip(id)?.content, ClipContent::Group(g) if g.cycle.is_some());
        let to = if asked < i64::from(before) && !cycles { tl.close_up_before(id, to)? } else { to };
        // the way is clear as far as `to` now, and a start dragged later leaves the clips before
        // it where they are (pushing would move every one of them together, either way)
        let mut changed = tl.trim(id, Edge::Start, to, EdgePolicy::Clamp);
        let clip = tl.clip(id)?;
        let short = i64::from(clip.start) - asked;
        // (only dragged earlier: a start dragged later than it can go just stops)
        // (a sound or video starts no earlier than its file does, and no longer for it)
        let media = matches!(clip.content, ClipContent::Audio(_) | ClipContent::Video(_));
        if asked < i64::from(before) && short > 0 && !media {
            let end = clip.end().saturating_add(u32::try_from(short).unwrap_or(u32::MAX));
            let end = tl.open_up_after(id, end)?;
            changed |= tl.trim(id, Edge::End, end, EdgePolicy::Clamp);
        }
        changed
    } else {
        // longer: into the empty frames after it first, the clips it reaches pushed only as far
        // as they must be. Shorter: nothing else moves
        let to = if to > tl.clip(id)?.end() { tl.open_up_after(id, to)? } else { to };
        tl.trim(id, Edge::End, to, EdgePolicy::Clamp)
    };
    if changed {
        hold_clips(tl);
    }
    changed.then_some(track)
}

/// Whether any of clips `ids` is on a locked track (one that isn't there counts as not).
fn any_locked(tl: &Timeline, ids: &[ClipId]) -> bool {
    ids.iter().any(|id| tl.track_of_clip(*id).and_then(|t| tl.track(t)).is_some_and(|t| t.locked))
}

#[uniffi::export]
impl CanvasSession {
    /// Clip `id`'s start (`leading`) or end (one past its last frame) dragged to timeline frame
    /// `frame`, the other edge staying put, as an undo step. `policy` says what happens at the
    /// clip next to it. A start takes the clip's keys along so its motion stays where it showed,
    /// and a sound or video starts later or earlier in its file. False if nothing changed (a
    /// locked track, a cycling group's start, an edge already as far as it goes).
    pub fn trim_timeline_clip(&self, id: u64, leading: bool, frame: u32, policy: TimelineEdgePolicy) -> bool {
        self.change("Trim", |d| trimmed(d.timeline.as_mut()?, id, leading, frame, policy).map(|_| ())).is_some()
    }

    /// The clips of clip `id`'s track as [`CanvasSession::trim_timeline_clip`] would leave them,
    /// changing nothing: what a drag shows before it is let go. Empty if it would change nothing.
    pub fn timeline_trim_preview(
        &self,
        id: u64,
        leading: bool,
        frame: u32,
        policy: TimelineEdgePolicy,
    ) -> Vec<TimelineClipInfo> {
        let st = self.shared.lock();
        let Some(mut tl) = st.doc.timeline.clone() else { return Vec::new() };
        let Some(track) = trimmed(&mut tl, id, leading, frame, policy) else { return Vec::new() };
        let offset = tl.level_start(track).unwrap_or(0);
        tl.track(track).map_or_else(Vec::new, |t| t.clips.iter().map(|c| super::clip_info(c, offset)).collect())
    }

    /// Clip `id`'s start (`leading`) or end dragged to timeline frame `frame` as the grid's handles
    /// drag it, as one "Trim" undo step. Longer, it takes up the empty frames beside it first and
    /// pushes only the clips it reaches, each only as far as it must go; shorter, nothing else
    /// moves. The timeline grows to hold what goes later, and a start asked earlier than the clips
    /// before it have room for (`frame` may be negative) makes the clip last that much longer at
    /// its end instead.
    /// False if nothing changed.
    pub fn lengthen_timeline_clip(&self, id: u64, leading: bool, frame: i32) -> bool {
        self.change("Trim", |d| lengthened(d.timeline.as_mut()?, id, leading, frame).map(|_| ())).is_some()
    }

    /// The clips of clip `id`'s track as [`CanvasSession::lengthen_timeline_clip`] would leave
    /// them, changing nothing. Empty if it would change nothing.
    pub fn timeline_lengthen_preview(&self, id: u64, leading: bool, frame: i32) -> Vec<TimelineClipInfo> {
        let st = self.shared.lock();
        let Some(mut tl) = st.doc.timeline.clone() else { return Vec::new() };
        let Some(track) = lengthened(&mut tl, id, leading, frame) else { return Vec::new() };
        let offset = tl.level_start(track).unwrap_or(0);
        tl.track(track).map_or_else(Vec::new, |t| t.clips.iter().map(|c| super::clip_info(c, offset)).collect())
    }

    /// Clips `ids` moved together along their own tracks by `by` frames (earlier when negative),
    /// or as far that way as the first of them has room before a clip left behind, as an undo
    /// step. Returns how far they went: 0 if nowhere, or if any is on a locked track.
    pub fn shift_timeline_clips(&self, ids: Vec<u64>, by: i32) -> i32 {
        let ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
        let went = self.change("Move Clips", |d| {
            let tl = d.timeline.as_mut()?;
            if any_locked(tl, &ids) {
                return None;
            }
            Some(tl.shift_clips(&ids, i64::from(by))).filter(|went| *went != 0)
        });
        went.and_then(|w| i32::try_from(w).ok()).unwrap_or(0)
    }

    /// Clips `ids` each made to last through the empty frames after it on its track, as one undo
    /// step: up to the next clip there or, with none after it, to the end of the timeline (or of
    /// the group it is in). See [`Timeline::fill_out`]. False if none had an empty frame after it,
    /// or any is on a locked track.
    pub fn extend_timeline_clips(&self, ids: Vec<u64>) -> bool {
        let ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
        self.change("Extend Clips", |d| {
            let tl = d.timeline.as_mut()?;
            (!any_locked(tl, &ids) && tl.fill_out(&ids)).then_some(())
        })
        .is_some()
    }

    /// Whether [`CanvasSession::extend_timeline_clips`] would make any of clips `ids` longer: for
    /// the panel to offer it only where it does something.
    pub fn timeline_clips_extend(&self, ids: Vec<u64>) -> bool {
        let ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
        let st = self.shared.lock();
        st.doc.timeline.clone().is_some_and(|mut tl| !any_locked(&tl, &ids) && tl.fill_out(&ids))
    }

    /// How far [`CanvasSession::shift_timeline_clips`] would move clips `ids` if asked for `by`
    /// frames, changing nothing: what a drag shows before it is let go.
    pub fn timeline_shift_room(&self, ids: Vec<u64>, by: i32) -> i32 {
        let ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
        let st = self.shared.lock();
        let Some(tl) = st.doc.timeline.as_ref() else { return 0 };
        if any_locked(tl, &ids) {
            return 0;
        }
        i32::try_from(tl.shift_room(&ids, i64::from(by))).unwrap_or(0)
    }

    /// Clips `ids` taken off their tracks with their drawings' layers (the last layer stays), as
    /// one undo step; the clips after them stay where they are. False if any is on a locked
    /// track, or none is there.
    pub fn delete_timeline_clips(&self, ids: Vec<u64>) -> bool {
        let ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
        let done = self
            .change("Delete Clips", |d| {
                let tl = d.timeline.as_mut()?;
                if any_locked(tl, &ids) {
                    return None;
                }
                let gone: Vec<_> = ids.iter().filter_map(|id| tl.remove(*id, false)).collect();
                for root in gone.iter().flat_map(|c| c.roots()) {
                    d.delete_layer(root);
                }
                (!gone.is_empty()).then_some(())
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

    /// The drawings of clips `ids` made blank, each clip staying where it is with its length and
    /// keys: a new empty layer takes the place of the drawing's layers, under its name. One "Clear
    /// Frames" undo step. Clips that aren't drawings (and adjustment clips) are left as they are.
    /// False if any is on a locked track, or none was cleared.
    pub fn clear_timeline_clips(&self, ids: Vec<u64>) -> bool {
        let ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
        {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
        }
        let done = self
            .change("Clear Frames", |d| {
                if any_locked(d.timeline.as_ref()?, &ids) {
                    return None;
                }
                let current = d.current();
                let mut cleared = false;
                for id in &ids {
                    let old = match d.timeline.as_ref()?.clip(*id) {
                        Some(Clip { content: ClipContent::Drawing(dr), adjustment: false, .. }) => dr.root,
                        _ => continue,
                    };
                    // (a layer more, until the old ones go)
                    if !d.has_room(1) {
                        continue;
                    }
                    let name = d.layer(old).map(|l| l.name.clone());
                    // a new top-level layer just above the drawing's own
                    d.set_current(old);
                    let fresh = d.add_layer();
                    if let Some(name) = name {
                        d.rename(fresh, &name);
                    }
                    if let Some(Clip { content: ClipContent::Drawing(dr), .. }) = d.timeline.as_mut()?.clip_mut(*id) {
                        dr.root = fresh;
                    }
                    // (a group with its layer in it, as a new drawing is)
                    d.drawing_group(fresh);
                    d.delete_layer(old);
                    cleared = true;
                }
                if d.layer(current).is_some() {
                    d.set_current(current);
                }
                cleared.then_some(())
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

    /// The timeline frame clip `id` would start on if [`CanvasSession::move_timeline_clip`] moved
    /// it to track `track` near frame `start`, changing nothing: where a dragged clip will land.
    /// None if it can't go there.
    pub fn timeline_move_landing(&self, id: u64, track: u64, start: u32) -> Option<u32> {
        let st = self.shared.lock();
        let mut tl = st.doc.timeline.clone()?;
        if tl.track(TrackId(track))?.locked || tl.track(tl.track_of_clip(ClipId(id))?)?.locked {
            return None;
        }
        let level = tl.level_start(TrackId(track))?;
        if !tl.move_clip(ClipId(id), TrackId(track), start.saturating_sub(level)) {
            return None;
        }
        Some(tl.clip(ClipId(id))?.start + level)
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Clip `id` taken out of the group clip it is in, onto the group's level where it shows (the
    /// group's keys folded into it), as an undo step. The track it lands on is drawn on.
    pub fn move_timeline_clip_out_of_group(&self, id: u64) -> bool {
        let onto = self.change("Move Out of Group", |d| d.timeline.as_mut()?.move_out_of_group(ClipId(id)));
        self.landed(onto)
    }

    /// Clip `id` put into group clip `group` (on the same level) where it is, as an undo step:
    /// on the group's topmost track with room there, or a new one. The group grows to cover it.
    pub fn move_timeline_clip_into_group(&self, id: u64, group: u64) -> bool {
        let onto = self.change("Move Into Group", |d| d.timeline.as_mut()?.move_into_group(ClipId(id), ClipId(group)));
        self.landed(onto)
    }

    /// Sound clip `id`'s level keyed at the playhead to `volume` (0..1): shown at once, one
    /// "Level Key" undo step when `done`. False if the playhead isn't on it.
    pub fn set_timeline_volume_key(&self, id: u64, volume: f32, done: bool) -> bool {
        if !volume.is_finite() {
            return false;
        }
        let frame = self.shared.lock().canvas.as_ref().map_or(0, |c| c.timeline_frame());
        self.change_live("Level Key", done, |d| {
            let Some(tl) = d.timeline.as_mut() else { return false };
            let Some(local) = tl.local_frame(ClipId(id), frame).and_then(|f| i32::try_from(f).ok()) else {
                return false;
            };
            let Some(clip) = tl.clip_mut(ClipId(id)) else { return false };
            if local < 0 || local >= i32::try_from(clip.duration).unwrap_or(i32::MAX) {
                return false;
            }
            let ClipContent::Audio(a) = &mut clip.content else { return false };
            let easing =
                a.volume_keys.keys.iter().find(|k| k.frame == local).map_or(Easing::Linear, |k| k.easing.clone());
            a.volume_keys.set(local, volume.clamp(0.0, procraft_engine::media::MAX_VOLUME), easing);
            true
        })
    }

    /// Takes away sound clip `id`'s level key at the playhead, as an undo step.
    pub fn remove_timeline_volume_key(&self, id: u64) -> bool {
        let frame = self.shared.lock().canvas.as_ref().map_or(0, |c| c.timeline_frame());
        self.change("Remove Level Key", |d| {
            let tl = d.timeline.as_mut()?;
            let local = i32::try_from(tl.local_frame(ClipId(id), frame)?).ok()?;
            match &mut tl.clip_mut(ClipId(id))?.content {
                ClipContent::Audio(a) => a.volume_keys.remove(local).then_some(()),
                _ => None,
            }
        })
        .is_some()
    }
}

impl CanvasSession {
    /// After a clip moved into or out of a group: the track it went on is drawn on, and the canvas
    /// drawn again.
    fn landed(&self, onto: Option<procraft_engine::timeline::TrackId>) -> bool {
        let Some(track) = onto else { return false };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.timeline_track = Some(track);
        st.follow_playhead();
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
        true
    }

    /// The layers panel's + on a timeline: a new empty layer in the drawing at the playhead on
    /// the track drawn on (a drawing of one layer becoming a group of two), or with no drawing
    /// there, a new drawing. Returns the layer, or 0.
    pub(crate) fn add_timeline_layer(&self) -> u64 {
        let root = {
            let st = self.shared.lock();
            let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
            st.doc.timeline.as_ref().zip(st.drawn_track()).and_then(|(tl, t)| {
                match &super::clip_on(tl, t, frame)?.content {
                    ClipContent::Drawing(d) => Some(d.root),
                    _ => None,
                }
            })
        };
        match root {
            Some(root) => self.change("new layer", |d| d.add_layer_to_drawing(root)).map_or(0, |l| l.0),
            None => {
                let clip = self.add_timeline_drawing();
                let st = self.shared.lock();
                match st.doc.timeline.as_ref().and_then(|tl| tl.clip(ClipId(clip))).map(|c| &c.content) {
                    Some(ClipContent::Drawing(d)) => d.root.0,
                    _ => 0,
                }
            }
        }
    }
}

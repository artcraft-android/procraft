//! A clip put down over others, pushing them aside; clips picked together moved to other tracks,
//! trimmed and duplicated as one undo step, with previews of each that change nothing.

use std::collections::HashMap;

use procraft_engine::document::LayerKind;
use procraft_engine::timeline::{ClipId, Edge, Timeline, TrackId};

use super::TimelineClipInfo;
use super::edits::TimelineEdgePolicy;
use crate::CanvasSession;

/// Where a dragged clip would land, and the clips of the track it would land on as they would be
/// then (timeline frames).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelinePlacement {
    pub start: u32,
    pub clips: Vec<TimelineClipInfo>,
}

/// How far clips picked together would go along, and the track each of `clips` would go onto.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineMoveRoom {
    pub by: i32,
    pub clips: Vec<u64>,
    pub tracks: Vec<u64>,
}

/// A track's clips as an edit would leave them (timeline frames).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineTrackClips {
    pub track: u64,
    pub clips: Vec<TimelineClipInfo>,
}

/// Clip `id` put on track `track` at timeline frame `start` in `tl`, pushing aside what it covers
/// (`later`, else earlier); false if it can't go there or either track is locked.
pub(super) fn placed(tl: &mut Timeline, id: u64, track: u64, start: u32, later: bool) -> bool {
    let (id, track) = (ClipId(id), TrackId(track));
    let locked = |tl: &Timeline, t: TrackId| tl.track(t).is_none_or(|t| t.locked);
    if locked(tl, track) || tl.track_of_clip(id).is_none_or(|t| locked(tl, t)) {
        return false;
    }
    let Some(level) = tl.level_start(track) else { return false };
    tl.place_clip(id, track, start.saturating_sub(level), later)
}

/// Track `track`'s clips in `tl`, in timeline frames.
fn clips_of(tl: &Timeline, track: TrackId) -> Vec<TimelineClipInfo> {
    let offset = tl.level_start(track).unwrap_or(0);
    tl.track(track).map_or_else(Vec::new, |t| t.clips.iter().map(|c| super::clip_info(c, offset)).collect())
}

/// Whether any of clips `ids` is on a locked track (one that isn't there counts as not).
fn any_locked(tl: &Timeline, ids: &[ClipId]) -> bool {
    ids.iter().any(|id| tl.track_of_clip(*id).and_then(|t| tl.track(t)).is_some_and(|t| t.locked))
}

/// Clips `ids` split by the level each is on, the deepest levels first (so a group grows to hold
/// its own copies, pushing what follows it along, before the copies outside it are put down after
/// their own). None if one isn't there.
fn levels_of(tl: &Timeline, ids: &[ClipId]) -> Option<Vec<Vec<ClipId>>> {
    let mut levels: Vec<(usize, Option<ClipId>, Vec<ClipId>)> = Vec::new();
    for id in ids {
        let groups = tl.groups_of_track(tl.track_of_clip(*id)?)?;
        let key = groups.last().map(|g| g.id);
        match levels.iter_mut().find(|l| l.1 == key) {
            Some(l) => l.2.push(*id),
            None => levels.push((groups.len(), key, vec![*id])),
        }
    }
    levels.sort_by_key(|l| std::cmp::Reverse(l.0));
    Some(levels.into_iter().map(|l| l.2).collect())
}

/// How many timeline frames clips `ids` span together, from the first one's start to the last
/// one's end. None if one isn't there.
fn span_of(tl: &Timeline, ids: &[ClipId]) -> Option<u32> {
    let mut first = u32::MAX;
    let mut last = 0;
    for id in ids {
        let at = tl.origin(*id)?;
        first = first.min(at);
        last = last.max(at + tl.clip(*id)?.duration);
    }
    last.checked_sub(first)
}

/// Clips `ids` put down `by` frames along on the tracks as many places up or down as track
/// `track` is from clip `primary`'s, pushing aside what they cover, in `tl`: the tracks that
/// changed. None if they can't go there.
pub(super) fn placed_together(tl: &mut Timeline, ids: &[ClipId], primary: u64, track: u64, by: i32, later: bool) -> Option<Vec<TrackId>> {
    tl.place_clips(ids, ClipId(primary), TrackId(track), i64::from(by), later)
}

/// Clips `ids` each `by` frames longer (shorter, when negative) at its end, or, `leading`, at its
/// start, in `tl`, the timeline growing to hold what goes later: the tracks that changed. Each
/// goes as one clip's alone does (see `lengthen_timeline_clip`): longer, into the empty frames
/// beside it first, the clips it reaches pushed only as far as they must be (a start with no
/// more room before it makes the clip longer at its end instead); shorter, nothing else moves.
/// The latest goes first, so the room one makes isn't taken by the next.
fn lengthened_together(tl: &mut Timeline, ids: &[ClipId], leading: bool, by: i32) -> Vec<TrackId> {
    let mut order: Vec<(u32, ClipId)> = ids.iter().filter_map(|id| Some((tl.origin(*id)?, *id))).collect();
    order.sort_by_key(|o| std::cmp::Reverse(o.0));
    let mut changed = Vec::new();
    for (_, id) in order {
        // (where its edge is now: one after it may have moved it)
        let Some(start) = tl.origin(id) else { continue };
        let Some(length) = tl.clip(id).map(|c| c.duration) else { continue };
        let edge = if leading { i64::from(start) } else { i64::from(start) + i64::from(length) };
        let to = i32::try_from(edge + i64::from(by)).unwrap_or(if by < 0 { i32::MIN } else { i32::MAX });
        if let Some(track) = super::edits::lengthened(tl, id.0, leading, to)
            && !changed.contains(&track)
        {
            changed.push(track);
        }
    }
    changed
}

pub(super) fn ids_of(ids: Vec<u64>) -> Vec<ClipId> {
    let mut ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
    ids.sort();
    ids.dedup();
    ids
}

#[uniffi::export]
impl CanvasSession {
    /// Clip `id` put on track `track` starting on timeline frame `start`, the clips there it would
    /// cover pushed aside (`later`, else earlier; later when there is no room before them), each
    /// as far as it must, as an undo step. False if it can't go there (a locked track, a group
    /// into itself) or nothing changed.
    pub fn place_timeline_clip(&self, id: u64, track: u64, start: u32, later: bool) -> bool {
        self.change("Move Clip", |d| {
            let tl = d.timeline.as_mut()?;
            let before = tl.clone();
            (placed(tl, id, track, start, later) && *tl != before).then_some(())
        })
        .is_some()
    }

    /// Where [`CanvasSession::place_timeline_clip`] would put clip `id`, and track `track`'s
    /// clips as it would leave them, changing nothing: what a drag shows before it is let go.
    /// None if it can't go there.
    pub fn timeline_place_preview(&self, id: u64, track: u64, start: u32, later: bool) -> Option<TimelinePlacement> {
        let st = self.shared.lock();
        let mut tl = st.doc.timeline.clone()?;
        if !placed(&mut tl, id, track, start, later) {
            return None;
        }
        let start = tl.origin(ClipId(id))?;
        Some(TimelinePlacement { start, clips: clips_of(&tl, TrackId(track)) })
    }

    /// How far clips `ids` would go along if moved `by` frames (earlier when negative) from their
    /// tracks to those as many places up or down as track `track` is from clip `primary`'s, and
    /// the track each would go onto, changing nothing: the nearest shift to `by` at which each
    /// fits among the clips left behind there. None if a track they would go to isn't there, or
    /// it or one of theirs is locked.
    pub fn timeline_move_room(&self, ids: Vec<u64>, primary: u64, track: u64, by: i32) -> Option<TimelineMoveRoom> {
        let ids = ids_of(ids);
        let st = self.shared.lock();
        let tl = st.doc.timeline.as_ref()?;
        let (went, tracks) = tl.move_room(&ids, ClipId(primary), TrackId(track), i64::from(by))?;
        Some(TimelineMoveRoom {
            by: i32::try_from(went).ok()?,
            clips: ids.iter().map(|c| c.0).collect(),
            tracks: tracks.iter().map(|t| t.0).collect(),
        })
    }

    /// Clips `ids` moved as [`CanvasSession::timeline_move_room`] says, as one "Move Clips" undo
    /// step. False if they can't go there or nothing changed.
    pub fn move_timeline_clips(&self, ids: Vec<u64>, primary: u64, track: u64, by: i32) -> bool {
        let ids = ids_of(ids);
        self.change("Move Clips", |d| {
            let tl = d.timeline.as_mut()?;
            let before = tl.clone();
            tl.move_clips(&ids, ClipId(primary), TrackId(track), i64::from(by))?;
            (*tl != before).then_some(())
        })
        .is_some()
    }

    /// Clips `ids` put down `by` frames along (earlier when negative) on the tracks as many
    /// places up or down as track `track` is from clip `primary`'s (their own, when it is
    /// `primary`'s), each starting exactly there, the clips there they would cover pushed aside
    /// (`later`, else earlier; later where there is no room before them), as one "Move Clips"
    /// undo step. False if they can't go there (a track that isn't there or is locked, or before
    /// the first frame) or nothing changed.
    pub fn place_timeline_clips(&self, ids: Vec<u64>, primary: u64, track: u64, by: i32, later: bool) -> bool {
        let ids = ids_of(ids);
        self.change("Move Clips", |d| {
            let tl = d.timeline.as_mut()?;
            let before = tl.clone();
            placed_together(tl, &ids, primary, track, by, later)?;
            (*tl != before).then_some(())
        })
        .is_some()
    }

    /// The tracks [`CanvasSession::place_timeline_clips`] would change, with their clips as it
    /// would leave them (timeline frames), changing nothing: what a drag shows before it is let
    /// go. Empty if they can't go there.
    pub fn timeline_place_clips_preview(
        &self,
        ids: Vec<u64>,
        primary: u64,
        track: u64,
        by: i32,
        later: bool,
    ) -> Vec<TimelineTrackClips> {
        let ids = ids_of(ids);
        let st = self.shared.lock();
        let Some(mut tl) = st.doc.timeline.clone() else { return Vec::new() };
        let Some(changed) = placed_together(&mut tl, &ids, primary, track, by, later) else { return Vec::new() };
        changed.into_iter().map(|t| TimelineTrackClips { track: t.0, clips: clips_of(&tl, t) }).collect()
    }

    /// Clips `ids` (each on group clip `group`'s level) put into it together, each `by` frames
    /// along from where it is (no earlier than the group's start), as one "Move Clips" undo step.
    /// False (nothing changes) if one can't go in, or one of them or the group is on a locked
    /// track.
    pub fn move_timeline_clips_into_group(&self, ids: Vec<u64>, group: u64, by: i32) -> bool {
        let ids = ids_of(ids);
        self.change("Move Clips", |d| {
            let tl = d.timeline.as_mut()?;
            if any_locked(tl, &ids) || any_locked(tl, &[ClipId(group)]) {
                return None;
            }
            tl.move_clips_into_group(&ids, ClipId(group), i64::from(by)).map(|_| ())
        })
        .is_some()
    }

    /// The starts (`leading`) or ends of clips `ids` dragged `by` frames together (earlier when
    /// negative), each as a trim of its own with `policy` at the clip next to it, as one "Trim
    /// Clips" undo step. False if nothing changed or one of them is on a locked track.
    pub fn trim_timeline_clips(&self, ids: Vec<u64>, leading: bool, by: i32, policy: TimelineEdgePolicy) -> bool {
        let ids = ids_of(ids);
        let edge = if leading { Edge::Start } else { Edge::End };
        self.change("Trim Clips", |d| {
            let tl = d.timeline.as_mut()?;
            if any_locked(tl, &ids) {
                return None;
            }
            (!tl.trim_clips(&ids, edge, i64::from(by), policy.policy()).is_empty()).then_some(())
        })
        .is_some()
    }

    /// Clips `ids` (on one level) played in the opposite order: each mirrored across the frames
    /// they span together, its length and keys kept, as one "Flip Frames" undo step. False, and
    /// nothing changed, if one is on a locked track, one would land on a clip that isn't among
    /// them, or there is nothing to turn round.
    pub fn reverse_timeline_clips(&self, ids: Vec<u64>) -> bool {
        let ids = ids_of(ids);
        let done = self
            .change("Flip Frames", |d| {
                let tl = d.timeline.as_mut()?;
                if any_locked(tl, &ids) {
                    return None;
                }
                tl.reverse_clips(&ids).then_some(())
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

    /// As [`CanvasSession::trim_timeline_clips`] with the clips beyond each edge pushed along,
    /// the timeline growing to hold what goes later: what the grid's handle on the frames picked
    /// together does. One "Trim Clips" undo step.
    pub fn lengthen_timeline_clips(&self, ids: Vec<u64>, leading: bool, by: i32) -> bool {
        let ids = ids_of(ids);
        self.change("Trim Clips", |d| {
            let tl = d.timeline.as_mut()?;
            if any_locked(tl, &ids) {
                return None;
            }
            (!lengthened_together(tl, &ids, leading, by).is_empty()).then_some(())
        })
        .is_some()
    }

    /// The tracks [`CanvasSession::lengthen_timeline_clips`] would change, with their clips as
    /// it would leave them, changing nothing. Empty if it would change nothing.
    pub fn timeline_lengthens_preview(&self, ids: Vec<u64>, leading: bool, by: i32) -> Vec<TimelineTrackClips> {
        let ids = ids_of(ids);
        let st = self.shared.lock();
        let Some(mut tl) = st.doc.timeline.clone() else { return Vec::new() };
        if any_locked(&tl, &ids) {
            return Vec::new();
        }
        let changed = lengthened_together(&mut tl, &ids, leading, by);
        changed.into_iter().map(|t| TimelineTrackClips { track: t.0, clips: clips_of(&tl, t) }).collect()
    }

    /// The tracks [`CanvasSession::trim_timeline_clips`] would change, with their clips as it
    /// would leave them, changing nothing. Empty if it would change nothing.
    pub fn timeline_trims_preview(
        &self,
        ids: Vec<u64>,
        leading: bool,
        by: i32,
        policy: TimelineEdgePolicy,
    ) -> Vec<TimelineTrackClips> {
        let ids = ids_of(ids);
        let st = self.shared.lock();
        let Some(mut tl) = st.doc.timeline.clone() else { return Vec::new() };
        if any_locked(&tl, &ids) {
            return Vec::new();
        }
        let edge = if leading { Edge::Start } else { Edge::End };
        let changed = tl.trim_clips(&ids, edge, i64::from(by), policy.policy());
        changed.into_iter().map(|t| TimelineTrackClips { track: t.0, clips: clips_of(&tl, t) }).collect()
    }

    /// Copies of clips `ids` (on one level or several: some inside unfolded groups, say), each
    /// drawing a copy of its layers, put down as far along as all of them span on the timeline,
    /// so the copies keep their timing with one another: on each level from the lowest of its
    /// picked clips' tracks up as they were, the clips there moving along to make room, a group
    /// growing to hold its copies (a cycling group's cycle growing). One "Duplicate Clips" undo
    /// step. Returns the copies (none if
    /// one is on a locked track, or there is no room for the layers).
    pub fn duplicate_timeline_clips(&self, ids: Vec<u64>) -> Vec<u64> {
        let ids = ids_of(ids);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let Some(tl) = st.doc.timeline.as_ref() else { return Vec::new() };
        if ids.is_empty() || any_locked(tl, &ids) {
            return Vec::new();
        }
        let (Some(levels), Some(span)) = (levels_of(tl, &ids), span_of(tl, &ids)) else { return Vec::new() };
        // each level's copies, where they go on it, and the layers they copy
        let mut plans = Vec::new();
        for level in &levels {
            let (Some(copied), Some((track, _))) = (tl.copy_clips(level), tl.picked_span(level)) else {
                return Vec::new();
            };
            let Some(first) = level.iter().filter_map(|id| tl.clip(*id).map(|c| c.start)).min() else {
                return Vec::new();
            };
            let layers: Vec<_> =
                copied.roots().into_iter().filter_map(|r| Some((r, st.doc.unit_layers(r)?))).collect();
            plans.push((copied, track, first + span, layers));
        }
        let units = plans
            .iter()
            .flat_map(|(.., layers)| layers.iter().flat_map(|(_, ls)| ls))
            .filter(|l| l.kind != LayerKind::Group)
            .count();
        if !st.doc.has_room(units) {
            return Vec::new();
        }
        let before = st.doc.clone();
        let mut pairs = Vec::new();
        let mut made = Vec::new();
        for (copied, track, at, layers) in &plans {
            let mut roots = HashMap::new();
            for (root, layers) in layers {
                if let Some((copy, p)) = st.doc.insert_copy(layers, *root) {
                    roots.insert(*root, copy);
                    pairs.extend(p);
                }
            }
            let pasted = st.doc.timeline.as_mut().and_then(|tl| {
                let m = tl.paste_clips(copied, *track, *at, &roots)?;
                tl.grow_groups_pushing(*track);
                Some(m)
            });
            let Some(pasted) = pasted else {
                st.doc = before;
                return Vec::new();
            };
            made.extend(pasted);
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("Duplicate Clips");
            canvas.duplicate_layers(pairs, before);
        }
        st.revision += 1;
        self.shared.frame(st);
        made.into_iter().map(|c| c.0).collect()
    }
}

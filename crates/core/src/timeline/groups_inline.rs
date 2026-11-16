//! Group clips unfolded in the panel: each unfolded group's own tracks shown as rows under the
//! track holding it, their clips where they play on the timeline (in the group's first cycle,
//! the later cycles' repeats shown beside them), so they are picked, moved and trimmed there as on
//! any track. And a cycling group's repeats counted and set by dragging its end.

use make2d_engine::timeline::{ClipContent, ClipId, Group, Timeline, Track};

use super::{TimelineTrackInfo, clip_info};
use crate::CanvasSession;

/// The most repeats of a group's clips shown beside them.
const MOST_REPEATS: usize = 200;

/// One row of the full view's tracks: a track, and where it sits among the groups unfolded.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineRow {
    /// Its clips as on the timeline (inside groups, in their first cycle).
    pub track: TimelineTrackInfo,
    /// How many unfolded groups it is inside (0: a track of the level shown).
    pub depth: u32,
    /// The group clip whose own track it is (0: none).
    pub group: u64,
    /// The frames its clips play in on the timeline: from `start` up to (not including) `end`,
    /// every group it is inside's span (in their first cycles) put together.
    pub start: u32,
    pub end: u32,
    /// A track above it, holding a group it is inside, is locked, or hidden.
    pub locked_above: bool,
    pub hidden_above: bool,
    /// Where its clips play again in a later cycle of a group it is inside, as far as they show.
    pub repeats: Vec<TimelineRepeat>,
}

/// A clip playing again in a later cycle of a group it is inside.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimelineRepeat {
    pub clip: u64,
    pub start: u32,
    pub duration: u32,
}

/// Where a level's frames are on the timeline once: where its frame 0 is, and the timeline frames
/// it shows in (from, up to).
#[derive(Clone, Copy)]
struct Place {
    offset: i64,
    from: i64,
    to: i64,
}

/// Group `g` (clip `start` long `duration` on a level placed at each of `places`): its own level's
/// places, a cycle at a time, the first first; empty where it shows nowhere.
fn inside(places: &[Place], start: u32, duration: u32, g: &Group) -> Vec<Place> {
    let mut out = Vec::new();
    for p in places {
        let begin = p.offset + i64::from(start);
        let span = i64::from(duration);
        let (every, times) = match g.cycle {
            Some(c) => {
                let len = i64::from(c.length.max(1));
                (len, (span + len - 1) / len)
            }
            None => (span, 1),
        };
        for k in 0..times.max(1) {
            let at = begin + k * every;
            let from = p.from.max(at);
            let to = p.to.min(begin + span).min(at + every);
            if from < to {
                out.push(Place { offset: at, from, to });
            }
            if out.len() > MOST_REPEATS {
                return out;
            }
        }
    }
    out
}

/// Rows for `tracks` (a level placed at each of `places`, the first where its clips are), bottom
/// first, each unfolded group's rows just below the track holding it, into `out`.
#[allow(clippy::too_many_arguments)]
fn rows_of(
    tl: &Timeline,
    tracks: &[Track],
    places: &[Place],
    depth: u32,
    group: u64,
    locked_above: bool,
    hidden_above: bool,
    out: &mut Vec<TimelineRow>,
) {
    let Some(first) = places.first().copied() else { return };
    let offset = u32::try_from(first.offset.max(0)).unwrap_or(0);
    for t in tracks {
        // the groups on it, latest first so the earliest's rows end up nearest it when the rows
        // are shown top first
        for c in t.clips.iter().rev() {
            if let ClipContent::Group(g) = &c.content
                && !g.collapsed
            {
                let inner = inside(places, c.start, c.duration, g);
                rows_of(
                    tl,
                    &g.tracks,
                    &inner,
                    depth + 1,
                    c.id.0,
                    locked_above || t.locked,
                    hidden_above || t.hidden,
                    out,
                );
            }
        }
        let mut repeats = Vec::new();
        for p in places.iter().skip(1) {
            for c in &t.clips {
                let from = p.from.max(p.offset + i64::from(c.start));
                let to = p.to.min(p.offset + i64::from(c.end()));
                if from < to && repeats.len() < MOST_REPEATS {
                    repeats.push(TimelineRepeat {
                        clip: c.id.0,
                        start: u32::try_from(from).unwrap_or(0),
                        duration: u32::try_from(to - from).unwrap_or(1),
                    });
                }
            }
        }
        out.push(TimelineRow {
            track: TimelineTrackInfo {
                id: t.id.0,
                name: t.name.clone(),
                hidden: t.hidden,
                locked: t.locked,
                clips: t.clips.iter().map(|c| clip_info(c, offset)).collect(),
                opacity: t.opacity,
                soloed: tl.soloed(t.id),
            },
            depth,
            group,
            start: u32::try_from(first.from.max(0)).unwrap_or(0),
            end: u32::try_from(first.to.clamp(0, i64::from(u32::MAX))).unwrap_or(u32::MAX),
            locked_above,
            hidden_above,
            repeats,
        });
    }
}

/// Whether clip `id` is on a locked track, or one inside a group on a locked track; none if it
/// isn't there.
fn held(tl: &Timeline, id: ClipId) -> Option<bool> {
    let mut track = tl.track_of_clip(id)?;
    loop {
        if tl.track(track)?.locked {
            return Some(true);
        }
        match tl.groups_of_track(track)?.last().map(|g| g.id) {
            Some(g) => track = tl.track_of_clip(g)?,
            None => return Some(false),
        }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The full view's rows, bottom first: the tracks shown (the open group's own, or the
    /// timeline's), each unfolded group clip's own tracks just below the track holding it, at any
    /// depth. Their clips are where they play on the timeline, so the clip edits taking timeline
    /// frames work on them as on any track.
    pub fn timeline_rows(&self) -> Vec<TimelineRow> {
        let st = self.shared.lock();
        let Some(tl) = st.doc.timeline.as_ref() else { return Vec::new() };
        let open = st.open_groups().last().copied();
        let whole = Place { offset: 0, from: 0, to: i64::from(u32::MAX) };
        let (tracks, place) = match open.and_then(|g| Some((tl.clip(g)?, tl.origin(g)?))) {
            Some((c @ make2d_engine::timeline::Clip { content: ClipContent::Group(g), .. }, at)) => {
                let at = i64::from(at);
                (&g.tracks, Place { offset: at, from: at, to: at + i64::from(c.duration) })
            }
            _ => (&tl.tracks, whole),
        };
        let mut out = Vec::new();
        rows_of(tl, tracks, &[place], 0, 0, false, false, &mut out);
        out
    }

    /// Group clip `id` shown folded, or its tracks unfolded under its own (kept with the canvas,
    /// not an undo step). False if it isn't a group or is that already.
    pub fn set_timeline_group_folded(&self, id: u64, folded: bool) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(tl) = st.doc.timeline.as_mut() else { return false };
        if !tl.set_folded(ClipId(id), folded) {
            return false;
        }
        st.revision += 1;
        self.shared.frame(st);
        true
    }

    /// How many times group clip `id` plays its cycle, and the most it has room for on its track
    /// (as [count, most]); none if it doesn't cycle.
    pub fn timeline_group_cycles(&self, id: u64) -> Option<Vec<u32>> {
        let st = self.shared.lock();
        let tl = st.doc.timeline.as_ref()?;
        Some(vec![tl.cycles(ClipId(id))?, tl.cycles_room(ClipId(id))?])
    }

    /// Group clip `id` made `count` cycles long, as far as its track has room: one "Cycles" undo
    /// step. False if it doesn't cycle, is that long already, or is on a locked track (or one
    /// inside a group on a locked track).
    pub fn set_timeline_group_cycles(&self, id: u64, count: u32) -> bool {
        let changed = self
            .change("Cycles", |d| {
                let tl = d.timeline.as_mut()?;
                let id = ClipId(id);
                (!held(tl, id)?).then_some(())?;
                tl.set_cycles(id, count).then_some(())
            })
            .is_some();
        if changed {
            self.redraw_timeline();
        }
        changed
    }

    /// The most frames cycling group clip `id` may last on its track (up to the next clip there);
    /// none if it doesn't cycle.
    pub fn timeline_group_room(&self, id: u64) -> Option<u32> {
        let st = self.shared.lock();
        st.doc.timeline.as_ref()?.group_room(ClipId(id))
    }

    /// Cycling group clip `id` made `frames` long from its start, as far as its track has room,
    /// its last cycle cut short where that falls inside one: one "Cycles" undo step. False if it
    /// doesn't cycle, is that long already, or is on a locked track (or one inside a group on a
    /// locked track).
    pub fn set_timeline_group_length(&self, id: u64, frames: u32) -> bool {
        let changed = self
            .change("Cycles", |d| {
                let tl = d.timeline.as_mut()?;
                let id = ClipId(id);
                (!held(tl, id)?).then_some(())?;
                tl.set_group_span(id, frames).then_some(())
            })
            .is_some();
        if changed {
            self.redraw_timeline();
        }
        changed
    }
}

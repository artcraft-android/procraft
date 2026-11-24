//! Clips dragged about the grid and let go: put down where they are dropped as the tracks view's
//! were (`place_timeline_clip`), the timeline growing to hold one dropped past its end; and one
//! dropped between two tracks, which makes a track there for it.

use procraft_engine::timeline::{ClipId, TrackId};

use super::picked::{ids_of, placed, placed_together};
use crate::CanvasSession;

#[uniffi::export]
impl CanvasSession {
    /// As `place_timeline_clip`, the timeline made as long as its clips where that is longer:
    /// one "Move Clip" undo step.
    pub fn drop_timeline_clip(&self, id: u64, track: u64, start: u32, later: bool) -> bool {
        self.change("Move Clip", |d| {
            let tl = d.timeline.as_mut()?;
            let before = tl.clone();
            if !placed(tl, id, track, start, later) || *tl == before {
                return None;
            }
            super::edits::hold_clips(tl);
            Some(())
        })
        .is_some()
    }

    /// As `place_timeline_clips`, the timeline made as long as its clips where that is longer:
    /// one "Move Clips" undo step.
    pub fn drop_timeline_clips(&self, ids: Vec<u64>, primary: u64, track: u64, by: i32, later: bool) -> bool {
        let ids = ids_of(ids);
        self.change("Move Clips", |d| {
            let tl = d.timeline.as_mut()?;
            let before = tl.clone();
            placed_together(tl, &ids, primary, track, by, later)?;
            if *tl == before {
                return None;
            }
            super::edits::hold_clips(tl);
            Some(())
        })
        .is_some()
    }

    /// Clip `id` put on a new track made for it just above track `beside` (`below`: just below
    /// it), on that track's level (the timeline's own, or the group's it is in), starting on
    /// timeline frame `start`: one "Move Clip" undo step. Returns the new track; 0 if it can't
    /// go there (its own track locked, a group into itself, no such track).
    pub fn drop_timeline_clip_on_new_track(&self, id: u64, beside: u64, below: bool, start: u32) -> u64 {
        self.change("Move Clip", |d| {
            let tl = d.timeline.as_mut()?;
            // a group can't go onto a track of its own
            if tl.groups_of_track(TrackId(beside))?.iter().any(|g| g.id == ClipId(id)) {
                return None;
            }
            let count = tl.every_track().len();
            let made = tl.add_track_above(TrackId(beside), &format!("Track {}", count + 1))?;
            if below {
                // just above it is one place past it: back under it
                let groups = tl.groups_of_track(TrackId(beside))?;
                let level = match groups.last().map(|g| &g.content) {
                    Some(procraft_engine::timeline::ClipContent::Group(g)) => &g.tracks,
                    _ => &tl.tracks,
                };
                let at = level.iter().position(|t| t.id == TrackId(beside))?;
                tl.move_track(made, at);
            }
            if !placed(tl, id, made.0, start, true) {
                return None;
            }
            super::edits::hold_clips(tl);
            Some(made.0)
        })
        .unwrap_or(0)
    }
}

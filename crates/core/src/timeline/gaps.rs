//! The empty frames between a track's clips, for the grid's menu on an empty frame: how many
//! there are in the run a frame is in, and that run made longer, shorter or closed up, the clips
//! after it moving to suit.

use procraft_engine::timeline::TrackId;

use crate::CanvasSession;

/// A run of empty frames on a track.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimelineGap {
    /// Its first frame, on the timeline.
    pub start: u32,
    /// How many frames it is; 0 where no clip comes after it (`open`), when it goes on to
    /// wherever the timeline ends.
    pub length: u32,
    pub open: bool,
}

#[uniffi::export]
impl CanvasSession {
    /// The run of empty frames the timeline's frame `frame` is in on track `track`; none where a
    /// clip is there, or it is before the first frame of a group the track is in.
    pub fn timeline_gap(&self, track: u64, frame: u32) -> Option<TimelineGap> {
        let st = self.shared.lock();
        let tl = st.doc.timeline.as_ref()?;
        let level = tl.level_start(TrackId(track))?;
        let (start, length) = tl.gap_at(TrackId(track), frame.checked_sub(level)?)?;
        Some(TimelineGap { start: start + level, length: length.unwrap_or(0), open: length.is_none() })
    }

    /// That run made `length` frames long (0: closed up), the clips after it on the track moving
    /// to suit and the timeline growing to hold them, as an undo step. False where nothing
    /// changes (a clip is on that frame, none comes after the run, it is that long already), or
    /// the track is locked.
    pub fn set_timeline_gap(&self, track: u64, frame: u32, length: u32) -> bool {
        self.change("Empty Frames", |d| {
            let tl = d.timeline.as_mut()?;
            let id = TrackId(track);
            if tl.track(id)?.locked {
                return None;
            }
            let at = frame.checked_sub(tl.level_start(id)?)?;
            tl.set_gap(id, at, length).then(|| super::edits::hold_clips(tl))
        })
        .is_some()
    }
}

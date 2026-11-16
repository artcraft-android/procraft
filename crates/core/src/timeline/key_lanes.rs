//! The tracks panel's key lanes: keys picked a few at a time, each channel apart, on as many clips
//! as the lanes show, a sound's volume and the camera's among them, and moved, stretched,
//! reversed, taken away or eased as one undo step, with what a drag would do shown before it is
//! let go.

use make2d_engine::timeline::{ClipId, KeyChannel, KeyEdit, KeyLane, LaneKey, Timeline};

use super::{TimelineChannel, TimelineEasing};
use crate::CanvasSession;

/// A lane of keys: one of a clip's channels, a sound's volume, or the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum TimelineKeyLane {
    Clip { id: u64, channel: TimelineChannel },
    Volume { id: u64 },
    Camera,
}

/// A key picked in a lane: the lane and the timeline's frame it is on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimelineKeyPick {
    pub lane: TimelineKeyLane,
    pub frame: u32,
}

/// A key in a lane: its timeline frame and how it eases into the next.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimelineLaneKey {
    pub lane: TimelineKeyLane,
    pub frame: u32,
    pub easing: TimelineEasing,
}

/// What is done to the keys picked.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Enum)]
pub enum TimelineKeyEdit {
    /// Moved this many frames (earlier when negative), as far as every one of them has room in its
    /// clip (the camera's, in the timeline).
    Move { by: i32 },
    /// Each one's distance from timeline frame `anchor` times `factor`, rounded to a frame: no
    /// farther than each one's clip has room, never two landing on one frame.
    Stretch { anchor: u32, factor: f64 },
    /// Each clip's (and the camera's) mirrored across the frames they span on it, played
    /// backwards.
    Reverse,
    /// Taken away.
    Remove,
    /// Easing into the next key as `easing`.
    Ease { easing: TimelineEasing },
}

/// The lanes an edit changed, their keys as it leaves them, and where the picked keys ended up.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct TimelineKeysEdited {
    pub lanes: Vec<TimelineKeyLane>,
    pub keys: Vec<TimelineLaneKey>,
    pub picks: Vec<TimelineKeyPick>,
}

fn channel(c: TimelineChannel) -> KeyChannel {
    match c {
        TimelineChannel::Position => KeyChannel::Position,
        TimelineChannel::Scale => KeyChannel::Scale,
        TimelineChannel::Rotation => KeyChannel::Rotation,
        TimelineChannel::Opacity => KeyChannel::Opacity,
        TimelineChannel::Adjustment => KeyChannel::Adjustment,
    }
}

fn of_channel(c: KeyChannel) -> TimelineChannel {
    match c {
        KeyChannel::Position => TimelineChannel::Position,
        KeyChannel::Scale => TimelineChannel::Scale,
        KeyChannel::Rotation => TimelineChannel::Rotation,
        KeyChannel::Opacity => TimelineChannel::Opacity,
        KeyChannel::Adjustment => TimelineChannel::Adjustment,
    }
}

fn lane(l: TimelineKeyLane) -> KeyLane {
    match l {
        TimelineKeyLane::Clip { id, channel: c } => KeyLane::Clip(ClipId(id), channel(c)),
        TimelineKeyLane::Volume { id } => KeyLane::Volume(ClipId(id)),
        TimelineKeyLane::Camera => KeyLane::Camera,
    }
}

fn of_lane(l: KeyLane) -> TimelineKeyLane {
    match l {
        KeyLane::Clip(id, c) => TimelineKeyLane::Clip { id: id.0, channel: of_channel(c) },
        KeyLane::Volume(id) => TimelineKeyLane::Volume { id: id.0 },
        KeyLane::Camera => TimelineKeyLane::Camera,
    }
}

/// Lane `l`'s keys in `tl`, as the apps show them (those before the timeline's start left out).
fn lane_keys(tl: &Timeline, l: TimelineKeyLane) -> Vec<TimelineLaneKey> {
    tl.lane_keys(lane(l))
        .unwrap_or_default()
        .into_iter()
        .filter_map(|(f, e)| Some(TimelineLaneKey { lane: l, frame: u32::try_from(f).ok()?, easing: TimelineEasing::of(&e) }))
        .collect()
}

/// Lane `l` is on a locked track (the camera never is).
fn locked(tl: &Timeline, l: TimelineKeyLane) -> bool {
    match l {
        TimelineKeyLane::Clip { id, .. } | TimelineKeyLane::Volume { id } => {
            tl.track_of_clip(ClipId(id)).and_then(|t| tl.track(t)).is_none_or(|t| t.locked)
        }
        TimelineKeyLane::Camera => false,
    }
}

/// `edit` done to keys `picks` in `tl` (those on a locked track left as they are): the lanes it
/// changed, their keys after, and where the picks ended up; None if nothing changed.
fn edited(tl: &mut Timeline, picks: &[TimelineKeyPick], edit: TimelineKeyEdit) -> Option<TimelineKeysEdited> {
    let picks: Vec<TimelineKeyPick> = picks.iter().copied().filter(|p| !locked(tl, p.lane)).collect();
    let keys: Vec<LaneKey> = picks.iter().map(|p| (lane(p.lane), i64::from(p.frame))).collect();
    let edit = match edit {
        TimelineKeyEdit::Move { by } => KeyEdit::Move(by),
        TimelineKeyEdit::Stretch { anchor, factor } => KeyEdit::Stretch { anchor: i32::try_from(anchor).ok()?, factor },
        TimelineKeyEdit::Reverse => KeyEdit::Reverse,
        TimelineKeyEdit::Remove => KeyEdit::Remove,
        TimelineKeyEdit::Ease { easing } => KeyEdit::Ease(easing.easing()),
    };
    let ended = tl.edit_lane_keys(&keys, &edit)?;
    let mut lanes: Vec<TimelineKeyLane> = Vec::new();
    for p in &picks {
        if !lanes.contains(&p.lane) {
            lanes.push(p.lane);
        }
    }
    Some(TimelineKeysEdited {
        keys: lanes.iter().flat_map(|&l| lane_keys(tl, l)).collect(),
        lanes,
        picks: ended
            .into_iter()
            .filter_map(|(l, f)| Some(TimelineKeyPick { lane: of_lane(l), frame: u32::try_from(f).ok()? }))
            .collect(),
    })
}

#[uniffi::export]
impl CanvasSession {
    /// `edit` done to keys `picks`, on as many clips, sounds and the camera as they are on (each
    /// channel's apart: a rotation key picked on a frame leaves the opacity key there), as one
    /// undo step named for it. A key moved onto another of its lane's takes its place. Where the
    /// picked keys ended up (none once taken away); None if nothing changed (a locked track, keys
    /// already as far as they go).
    pub fn edit_timeline_keys(&self, picks: Vec<TimelineKeyPick>, edit: TimelineKeyEdit) -> Option<Vec<TimelineKeyPick>> {
        let name = match edit {
            TimelineKeyEdit::Move { .. } => "Move Keys",
            TimelineKeyEdit::Stretch { .. } => "Stretch Keys",
            TimelineKeyEdit::Reverse => "Reverse Keys",
            TimelineKeyEdit::Remove => "Remove Keys",
            TimelineKeyEdit::Ease { .. } => "Easing",
        };
        let ended = self.change(name, |d| edited(d.timeline.as_mut()?, &picks, edit))?;
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
        Some(ended.picks)
    }

    /// The lanes [`CanvasSession::edit_timeline_keys`] would change, their keys as it would leave
    /// them, and where the picked ones would go, changing nothing: what a drag shows before it is
    /// let go. None if it would change nothing.
    pub fn timeline_keys_edit_preview(
        &self,
        picks: Vec<TimelineKeyPick>,
        edit: TimelineKeyEdit,
    ) -> Option<TimelineKeysEdited> {
        let st = self.shared.lock();
        let mut tl = st.doc.timeline.clone()?;
        drop(st);
        edited(&mut tl, &picks, edit)
    }

    /// Lane `lane`'s keys, in frame order: for a sound's volume and the camera, whose keys the
    /// panel otherwise knows only by frame.
    pub fn timeline_lane_keys(&self, lane: TimelineKeyLane) -> Vec<TimelineLaneKey> {
        let st = self.shared.lock();
        st.doc.timeline.as_ref().map(|tl| lane_keys(tl, lane)).unwrap_or_default()
    }
}

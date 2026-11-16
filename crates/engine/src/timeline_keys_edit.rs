//! Keys edited a few at a time, as the panel's key lanes pick them: each channel's keys apart,
//! moved, stretched about a frame, reversed, taken away or eased. The keys picked can be on
//! several clips at once, a sound's volume and the camera's among them.

use std::collections::{BTreeMap, BTreeSet};

use super::*;

/// One of a clip's keyed channels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyChannel {
    Position,
    Scale,
    Rotation,
    Opacity,
    Adjustment,
}

impl KeyChannel {
    /// Every channel, in the order the lanes show them.
    pub const ALL: [KeyChannel; 5] =
        [KeyChannel::Position, KeyChannel::Scale, KeyChannel::Rotation, KeyChannel::Opacity, KeyChannel::Adjustment];
}

/// A key picked: its channel and its clip-relative frame.
pub type KeyPick = (KeyChannel, i32);

/// A lane of keys: one of a clip's channels, a sound's volume, or the camera.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum KeyLane {
    Clip(ClipId, KeyChannel),
    Volume(ClipId),
    Camera,
}

/// A key picked in a lane, on a timeline frame (the document's, through the groups' starts).
pub type LaneKey = (KeyLane, i64);

/// What is done to the keys picked.
#[derive(Clone, Debug, PartialEq)]
pub enum KeyEdit {
    /// Moved this many frames, as far as the clip has room.
    Move(i32),
    /// Each one's distance from frame `anchor` times `factor` (above 0), rounded to a frame: no
    /// farther than the clip has room, and never so close that two of them land on one frame.
    Stretch { anchor: i32, factor: f64 },
    /// Mirrored across the frames they span, each channel's easings turned round so its motion
    /// plays backwards.
    Reverse,
    /// Taken away (the pivot with the last key that moves, scales or turns the clip).
    Remove,
    /// Easing into the next key as this.
    Ease(Easing),
}

/// What an edit does to each key picked.
enum Mapping {
    /// Each one moved from its frame to this one.
    To(Box<dyn Fn(i32) -> i32>),
    Remove,
    Ease(Easing),
}

/// What `edit` does to keys on `frames` (none: nothing to do) kept within `span`: keys already
/// outside it go no farther out.
fn mapping(frames: &BTreeSet<i32>, edit: &KeyEdit, span: (i32, i32)) -> Option<Mapping> {
    let (&first, &last) = (frames.first()?, frames.last()?);
    let (lo, hi) = span;
    Some(match edit {
        KeyEdit::Move(by) => {
            let by = (*by).clamp(lo.saturating_sub(first).min(0), hi.saturating_sub(last).max(0));
            if by == 0 {
                return None;
            }
            Mapping::To(Box::new(move |f: i32| f.saturating_add(by)))
        }
        KeyEdit::Stretch { anchor, factor } => {
            let anchor = *anchor;
            let bounded: Vec<(i64, (i64, i64))> =
                frames.iter().map(|&f| (i64::from(f), (i64::from(lo), i64::from(hi)))).collect();
            let factor = stretch_factor(&bounded, i64::from(anchor), *factor)?;
            Mapping::To(Box::new(move |f: i32| anchor.saturating_add(scaled_i32(f.saturating_sub(anchor), factor))))
        }
        KeyEdit::Reverse => {
            if first == last {
                return None;
            }
            Mapping::To(Box::new(move |f: i32| first.saturating_add(last).saturating_sub(f)))
        }
        KeyEdit::Remove => Mapping::Remove,
        KeyEdit::Ease(easing) => Mapping::Ease(easing.clone()),
    })
}

impl ClipKeys {
    /// `edit` done to the keys `picks` names (those that aren't there left out), within frames
    /// `span` (the clip's first and last, inclusive): keys already outside it go no farther out.
    /// A key moved onto another of its channel's that wasn't picked takes its place. The picked
    /// keys where they ended up (none once taken away); None if nothing changed.
    pub fn edit_keys(&mut self, picks: &[KeyPick], edit: &KeyEdit, span: (i32, i32)) -> Option<Vec<KeyPick>> {
        let picks: BTreeSet<KeyPick> = picks.iter().copied().filter(|&(c, f)| self.frames(c).contains(&f)).collect();
        let frames: BTreeSet<i32> = picks.iter().map(|&(_, f)| f).collect();
        let to = match mapping(&frames, edit, span)? {
            Mapping::To(to) => to,
            Mapping::Remove => {
                for &(c, f) in &picks {
                    self.remove_key(c, f);
                }
                if !self.posed() {
                    self.pivot = None;
                }
                return Some(Vec::new());
            }
            Mapping::Ease(easing) => {
                let mut changed = false;
                for &(c, f) in &picks {
                    changed |= self.ease_key(c, f, &easing);
                }
                return changed.then(|| picks.into_iter().collect());
            }
        };
        if picks.iter().all(|&(_, f)| to(f) == f) {
            return None;
        }
        let reverse = matches!(edit, KeyEdit::Reverse);
        let mut ended = Vec::new();
        for channel in KeyChannel::ALL {
            let on: Vec<i32> = picks.iter().filter(|&&(c, _)| c == channel).map(|&(_, f)| f).collect();
            if on.is_empty() {
                continue;
            }
            let moved = match channel {
                KeyChannel::Position => retime(&mut self.position, &on, &to, reverse),
                KeyChannel::Scale => retime(&mut self.scale, &on, &to, reverse),
                KeyChannel::Rotation => retime(&mut self.rotation, &on, &to, reverse),
                KeyChannel::Opacity => retime(&mut self.opacity, &on, &to, reverse),
                KeyChannel::Adjustment => retime(&mut self.adjustment, &on, &to, reverse),
            };
            ended.extend(moved.into_iter().map(|f| (channel, f)));
        }
        Some(ended)
    }

    /// The frames channel `channel` has keys on, in order.
    pub fn frames(&self, channel: KeyChannel) -> Vec<i32> {
        fn of<T>(c: &Channel<T>) -> Vec<i32> {
            c.keys.iter().map(|k| k.frame).collect()
        }
        match channel {
            KeyChannel::Position => of(&self.position),
            KeyChannel::Scale => of(&self.scale),
            KeyChannel::Rotation => of(&self.rotation),
            KeyChannel::Opacity => of(&self.opacity),
            KeyChannel::Adjustment => of(&self.adjustment),
        }
    }

    /// Channel `channel`'s keys: each one's frame and how it eases into the next.
    pub fn eased(&self, channel: KeyChannel) -> Vec<(i32, Easing)> {
        match channel {
            KeyChannel::Position => self.position.eased(),
            KeyChannel::Scale => self.scale.eased(),
            KeyChannel::Rotation => self.rotation.eased(),
            KeyChannel::Opacity => self.opacity.eased(),
            KeyChannel::Adjustment => self.adjustment.eased(),
        }
    }

    fn remove_key(&mut self, channel: KeyChannel, frame: i32) -> bool {
        match channel {
            KeyChannel::Position => self.position.remove(frame),
            KeyChannel::Scale => self.scale.remove(frame),
            KeyChannel::Rotation => self.rotation.remove(frame),
            KeyChannel::Opacity => self.opacity.remove(frame),
            KeyChannel::Adjustment => self.adjustment.remove(frame),
        }
    }

    fn ease_key(&mut self, channel: KeyChannel, frame: i32, easing: &Easing) -> bool {
        match channel {
            KeyChannel::Position => self.position.ease_key(frame, easing),
            KeyChannel::Scale => self.scale.ease_key(frame, easing),
            KeyChannel::Rotation => self.rotation.ease_key(frame, easing),
            KeyChannel::Opacity => self.opacity.ease_key(frame, easing),
            KeyChannel::Adjustment => self.adjustment.ease_key(frame, easing),
        }
    }
}

impl<T: Clone> Channel<T> {
    /// [`ClipKeys::edit_keys`] for one channel on its own (a sound's volume, the camera): `edit`
    /// done to its keys on `frames` within `span`. The frames they ended on (none once taken
    /// away); None if nothing changed.
    pub fn edit_picked(&mut self, frames: &[i32], edit: &KeyEdit, span: (i32, i32)) -> Option<Vec<i32>> {
        let picked: BTreeSet<i32> = frames.iter().copied().filter(|f| self.keys.iter().any(|k| k.frame == *f)).collect();
        match mapping(&picked, edit, span)? {
            Mapping::To(to) => {
                if picked.iter().all(|&f| to(f) == f) {
                    return None;
                }
                let on: Vec<i32> = picked.into_iter().collect();
                Some(retime(self, &on, &to, matches!(edit, KeyEdit::Reverse)))
            }
            Mapping::Remove => {
                for &f in &picked {
                    self.remove(f);
                }
                Some(Vec::new())
            }
            Mapping::Ease(easing) => {
                let mut changed = false;
                for &f in &picked {
                    changed |= self.ease_key(f, &easing);
                }
                changed.then(|| picked.into_iter().collect())
            }
        }
    }

    /// Each key's frame and how it eases into the next.
    pub fn eased(&self) -> Vec<(i32, Easing)> {
        self.keys.iter().map(|k| (k.frame, k.easing.clone())).collect()
    }

    fn ease_key(&mut self, frame: i32, easing: &Easing) -> bool {
        match self.keys.iter_mut().find(|k| k.frame == frame) {
            Some(k) if k.easing != *easing => {
                k.easing = easing.clone();
                true
            }
            _ => false,
        }
    }
}

/// Where a lane's keys are counted from on the timeline and the frames they can be on there
/// (first and last, inclusive).
struct LanePlace {
    origin: i64,
    span: (i64, i64),
}

impl Timeline {
    /// [`ClipKeys::edit_keys`] on clip `id`'s keys, within its own frames.
    pub fn edit_keys(&mut self, id: ClipId, picks: &[KeyPick], edit: &KeyEdit) -> Option<Vec<KeyPick>> {
        let clip = self.clip_mut(id)?;
        let last = i32::try_from(clip.duration.saturating_sub(1)).unwrap_or(i32::MAX);
        clip.keys.edit_keys(picks, edit, (0, last))
    }

    /// Lane `lane`'s keys: each one's timeline frame and how it eases into the next. None if the
    /// lane isn't there (a clip gone, a volume on a clip that isn't a sound).
    pub fn lane_keys(&self, lane: KeyLane) -> Option<Vec<(i64, Easing)>> {
        let place = self.lane_place(lane)?;
        let keys = match lane {
            KeyLane::Clip(id, channel) => self.clip(id)?.keys.eased(channel),
            KeyLane::Volume(id) => match &self.clip(id)?.content {
                ClipContent::Audio(a) => a.volume_keys.eased(),
                _ => return None,
            },
            KeyLane::Camera => self.camera.eased(),
        };
        Some(keys.into_iter().map(|(f, e)| (place.origin + i64::from(f), e)).collect())
    }

    fn lane_place(&self, lane: KeyLane) -> Option<LanePlace> {
        match lane {
            KeyLane::Clip(id, _) | KeyLane::Volume(id) => {
                let origin = i64::from(self.origin(id)?);
                let duration = i64::from(self.clip(id)?.duration.max(1));
                Some(LanePlace { origin, span: (origin, origin + duration - 1) })
            }
            KeyLane::Camera => Some(LanePlace { origin: 0, span: (0, i64::from(self.duration.max(1)) - 1) }),
        }
    }

    /// `edit` done to keys picked in lanes `picks` (timeline frames; those that aren't there left
    /// out), on as many clips, sounds and the camera as they are on. Each clip's keys stay within
    /// its frames and the camera's within the timeline's; a move or stretch goes only as far as
    /// every one of them has room, so they keep in step. A reverse mirrors each clip's (and the
    /// camera's) picked keys across the frames they span on it. The picked keys where they ended
    /// up (none once taken away; those the edit left where they were, there); None if nothing
    /// changed.
    pub fn edit_lane_keys(&mut self, picks: &[LaneKey], edit: &KeyEdit) -> Option<Vec<LaneKey>> {
        // the picks that are there, by where they are kept: a clip's keys, a sound's volume, the camera
        #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
        enum Holder {
            Clip(ClipId),
            Volume(ClipId),
            Camera,
        }
        let mut held: BTreeMap<Holder, (LanePlace, BTreeSet<LaneKey>)> = BTreeMap::new();
        for &(lane, frame) in picks {
            if !self.lane_keys(lane).is_some_and(|keys| keys.iter().any(|(f, _)| *f == frame)) {
                continue;
            }
            let Some(place) = self.lane_place(lane) else { continue };
            let holder = match lane {
                KeyLane::Clip(id, _) => Holder::Clip(id),
                KeyLane::Volume(id) => Holder::Volume(id),
                KeyLane::Camera => Holder::Camera,
            };
            held.entry(holder).or_insert((place, BTreeSet::new())).1.insert((lane, frame));
        }
        if held.is_empty() {
            return None;
        }
        // a move or a stretch goes as far as all of them have room for
        let edit = match edit {
            KeyEdit::Move(by) => {
                let (mut least, mut most) = (i64::MIN, i64::MAX);
                for (place, keys) in held.values() {
                    let first = keys.iter().map(|&(_, f)| f).min()?;
                    let last = keys.iter().map(|&(_, f)| f).max()?;
                    least = least.max((place.span.0 - first).min(0));
                    most = most.min((place.span.1 - last).max(0));
                }
                let by = i64::from(*by).clamp(least, most);
                if by == 0 {
                    return None;
                }
                KeyEdit::Move(i32::try_from(by).ok()?)
            }
            KeyEdit::Stretch { anchor, factor } => {
                let bounded: Vec<(i64, (i64, i64))> = held
                    .values()
                    .flat_map(|(place, keys)| keys.iter().map(move |&(_, f)| (f, place.span)))
                    .collect();
                KeyEdit::Stretch { anchor: *anchor, factor: stretch_factor(&bounded, i64::from(*anchor), *factor)? }
            }
            other => other.clone(),
        };
        let mut ended = Vec::new();
        let mut changed = false;
        for (holder, (place, keys)) in held {
            let local = |f: i64| i32::try_from(f - place.origin).ok();
            let span = (local(place.span.0)?, local(place.span.1)?);
            let edit = match &edit {
                KeyEdit::Stretch { anchor, factor } => {
                    KeyEdit::Stretch { anchor: local(i64::from(*anchor))?, factor: *factor }
                }
                other => other.clone(),
            };
            let back = |f: i32| place.origin + i64::from(f);
            match holder {
                Holder::Clip(id) => {
                    let picks: Vec<KeyPick> = keys
                        .iter()
                        .filter_map(|&(lane, f)| match lane {
                            KeyLane::Clip(_, channel) => Some((channel, local(f)?)),
                            _ => None,
                        })
                        .collect();
                    let Some(clip) = self.clip_mut(id) else { continue };
                    if let Some(went) = clip.keys.edit_keys(&picks, &edit, span) {
                        changed = true;
                        ended.extend(went.into_iter().map(|(c, f)| (KeyLane::Clip(id, c), back(f))));
                    } else {
                        ended.extend(keys.iter().copied());
                    }
                }
                Holder::Volume(id) => {
                    let frames: Vec<i32> = keys.iter().filter_map(|&(_, f)| local(f)).collect();
                    let Some(ClipContent::Audio(a)) = self.clip_mut(id).map(|c| &mut c.content) else { continue };
                    if let Some(went) = a.volume_keys.edit_picked(&frames, &edit, span) {
                        changed = true;
                        ended.extend(went.into_iter().map(|f| (KeyLane::Volume(id), back(f))));
                    } else {
                        ended.extend(keys.iter().copied());
                    }
                }
                Holder::Camera => {
                    let frames: Vec<i32> = keys.iter().filter_map(|&(_, f)| local(f)).collect();
                    if let Some(went) = self.camera.edit_picked(&frames, &edit, span) {
                        changed = true;
                        ended.extend(went.into_iter().map(|f| (KeyLane::Camera, back(f))));
                    } else {
                        ended.extend(keys.iter().copied());
                    }
                }
            }
        }
        ended.sort();
        changed.then_some(ended)
    }
}

/// `factor` as far as stretching keys on `frames` about `anchor` keeps each within its own span
/// (first and last frames, inclusive; those already outside it going no farther out) and on frames
/// of their own; None if it isn't a stretch.
fn stretch_factor(frames: &[(i64, (i64, i64))], anchor: i64, factor: f64) -> Option<f64> {
    if !(factor.is_finite() && factor > 0.0) {
        return None;
    }
    let mut factor = factor;
    for &(f, span) in frames {
        let d = f - anchor;
        if d == 0 {
            continue;
        }
        let bound = if d > 0 { span.1.max(f) } else { span.0.min(f) };
        factor = factor.min((bound - anchor) as f64 / d as f64);
    }
    // a frame apart at least, so rounding never lands two on one frame
    let ordered: Vec<i64> = frames.iter().map(|&(f, _)| f).collect::<BTreeSet<i64>>().into_iter().collect();
    if let Some(gap) = ordered.windows(2).filter_map(|w| Some(*w.get(1)? - *w.first()?)).min() {
        factor = factor.max(1.0 / gap as f64);
    }
    Some(factor)
}

/// The keys of `c` on `picked` frames moved to `to(frame)`, each taking the place of one already
/// there; reversed, each one eases as the one that led into it did, played backwards, and a
/// motion path's handles change sides. The frames they ended on.
fn retime<T: Clone>(c: &mut Channel<T>, picked: &[i32], to: &dyn Fn(i32) -> i32, reverse: bool) -> Vec<i32> {
    let (mut moving, kept): (Vec<Key<T>>, Vec<Key<T>>) =
        std::mem::take(&mut c.keys).into_iter().partition(|k| picked.contains(&k.frame));
    c.keys = kept;
    if reverse {
        let easings: Vec<Easing> = moving.iter().map(|k| k.easing.clone()).collect();
        let last = easings.last().cloned().unwrap_or_default();
        for (i, key) in moving.iter_mut().enumerate() {
            key.easing = match i.checked_sub(1).and_then(|j| easings.get(j)) {
                Some(e) => reversed(e),
                None => last.clone(),
            };
            if let Some(p) = key.path.as_mut() {
                std::mem::swap(&mut p.before, &mut p.after);
            }
        }
    }
    for key in &mut moving {
        key.frame = to(key.frame);
    }
    // landing on one frame, the later of them stays (they were in frame order)
    let mut ended = Vec::new();
    for key in moving {
        let at = c.keys.partition_point(|k| k.frame < key.frame);
        ended.push(key.frame);
        match c.keys.get_mut(at) {
            Some(k) if k.frame == key.frame => *k = key,
            _ => c.keys.insert(at, key),
        }
    }
    ended.sort_unstable();
    ended.dedup();
    ended
}

/// `e` played backwards.
fn reversed(e: &Easing) -> Easing {
    match e {
        Easing::Linear | Easing::Smooth => e.clone(),
        Easing::CubicBezier(a, b) => Easing::CubicBezier([1.0 - b[0], 1.0 - b[1]], [1.0 - a[0], 1.0 - a[1]]),
        Easing::Reversed(inner) => (**inner).clone(),
        _ => Easing::Reversed(Box::new(e.clone())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use KeyChannel::{Opacity, Position, Rotation};

    fn key<T>(frame: i32, value: T, easing: Easing) -> Key<T> {
        Key { frame, value, easing, path: None }
    }

    /// Opacity keys on 0, 4 and 8 and rotation keys on 4 and 6.
    fn keys() -> ClipKeys {
        ClipKeys {
            opacity: Channel {
                keys: vec![key(0, 0.0, Easing::Linear), key(4, 0.5, Easing::Hold), key(8, 1.0, Easing::Smooth)],
            },
            rotation: Channel { keys: vec![key(4, 0.0, Easing::Linear), key(6, 1.0, Easing::Linear)] },
            ..ClipKeys::default()
        }
    }

    #[test]
    fn a_move_takes_one_channel_and_stops_at_the_clip() {
        let mut k = keys();
        // the opacity key on 4 only, two frames on: the rotation key on 4 stays
        assert_eq!(k.edit_keys(&[(Opacity, 4)], &KeyEdit::Move(2), (0, 9)), Some(vec![(Opacity, 6)]));
        assert_eq!(k.frames(Opacity), [0, 6, 8]);
        assert_eq!(k.frames(Rotation), [4, 6]);
        // past the clip's last frame: as far as it goes
        assert_eq!(k.edit_keys(&[(Opacity, 6), (Rotation, 6)], &KeyEdit::Move(20), (0, 9)), Some(vec![
            (Rotation, 9),
            (Opacity, 9)
        ]));
        // already at the end: nothing
        assert_eq!(k.edit_keys(&[(Opacity, 9)], &KeyEdit::Move(1), (0, 9)), None);
        // a key that isn't there is left out
        assert_eq!(k.edit_keys(&[(Position, 2)], &KeyEdit::Move(1), (0, 9)), None);
    }

    #[test]
    fn a_key_moved_onto_another_takes_its_place() {
        let mut k = keys();
        assert_eq!(k.edit_keys(&[(Opacity, 0)], &KeyEdit::Move(4), (0, 9)), Some(vec![(Opacity, 4)]));
        assert_eq!(k.frames(Opacity), [4, 8]);
        assert_eq!(k.opacity.keys.first().map(|k| k.value), Some(0.0));
    }

    #[test]
    fn a_stretch_scales_about_its_anchor() {
        let mut k = keys();
        let all = [(Opacity, 0), (Opacity, 4), (Opacity, 8)];
        assert_eq!(k.edit_keys(&all, &KeyEdit::Stretch { anchor: 0, factor: 0.5 }, (0, 20)).map(|p| p.len()), Some(3));
        assert_eq!(k.frames(Opacity), [0, 2, 4]);
        // no closer than a frame apart
        let all = [(Opacity, 0), (Opacity, 2), (Opacity, 4)];
        let _ = k.edit_keys(&all, &KeyEdit::Stretch { anchor: 0, factor: 0.1 }, (0, 20));
        assert_eq!(k.frames(Opacity), [0, 1, 2]);
        // no farther than the clip
        let all = [(Opacity, 0), (Opacity, 1), (Opacity, 2)];
        let _ = k.edit_keys(&all, &KeyEdit::Stretch { anchor: 0, factor: 50.0 }, (0, 9));
        assert_eq!(k.frames(Opacity), [0, 5, 9]);
        // from the last key back
        let all = [(Opacity, 0), (Opacity, 5), (Opacity, 9)];
        let _ = k.edit_keys(&all, &KeyEdit::Stretch { anchor: 9, factor: 0.5 }, (0, 9));
        assert_eq!(k.frames(Opacity), [4, 7, 9]);
        assert_eq!(k.edit_keys(&all, &KeyEdit::Stretch { anchor: 9, factor: -1.0 }, (0, 9)), None);
    }

    #[test]
    fn reversed_keys_play_backwards() {
        let with_paths = |mut c: Channel<f32>| {
            for key in &mut c.keys {
                key.path = Some(PathHandles { mode: HandleMode::Broken, before: [1.0, 0.0], after: [2.0, 0.0] });
            }
            c
        };
        let mut k = keys();
        k.opacity = with_paths(k.opacity);
        let all = [(Opacity, 0), (Opacity, 4), (Opacity, 8)];
        assert!(k.edit_keys(&all, &KeyEdit::Reverse, (0, 9)).is_some());
        let values: Vec<(i32, f32)> = k.opacity.keys.iter().map(|k| (k.frame, k.value)).collect();
        assert_eq!(values, [(0, 1.0), (4, 0.5), (8, 0.0)]);
        // 4 → 8 held at 0.5 before; now 0 → 4 is that span backwards, 4 → 8 was 0 → 4's linear
        let easings: Vec<Easing> = k.opacity.keys.iter().map(|k| k.easing.clone()).collect();
        assert_eq!(easings, [Easing::Reversed(Box::new(Easing::Hold)), Easing::Linear, Easing::Smooth]);
        for f in 0..=8 {
            assert_eq!(keys().opacity.value_at(8.0 - f as f32), k.opacity.value_at(f as f32), "frame {f}");
        }
        assert_eq!(k.opacity.keys.first().and_then(|k| k.path).map(|p| p.before), Some([2.0, 0.0]));
        // reversing twice gives it back
        assert!(k.edit_keys(&all, &KeyEdit::Reverse, (0, 9)).is_some());
        assert_eq!(k.opacity, with_paths(keys().opacity));
        // a lone frame doesn't reverse
        assert_eq!(k.edit_keys(&[(Opacity, 4), (Rotation, 4)], &KeyEdit::Reverse, (0, 9)), None);
    }

    #[test]
    fn keys_across_channels_reverse_over_their_whole_span() {
        let mut k = keys();
        let picks = [(Opacity, 0), (Rotation, 6)];
        assert_eq!(k.edit_keys(&picks, &KeyEdit::Reverse, (0, 9)), Some(vec![(Rotation, 0), (Opacity, 6)]));
        assert_eq!(k.frames(Opacity), [4, 6, 8]);
        assert_eq!(k.frames(Rotation), [0, 4]);
    }

    #[test]
    fn removing_takes_one_channel_and_the_pivot_with_the_last_pose_key() {
        let mut k = keys();
        k.pivot = Some([5.0, 5.0]);
        assert_eq!(k.edit_keys(&[(Rotation, 4)], &KeyEdit::Remove, (0, 9)), Some(vec![]));
        assert_eq!(k.frames(Opacity), [0, 4, 8]);
        assert_eq!(k.pivot, Some([5.0, 5.0]));
        assert_eq!(k.edit_keys(&[(Rotation, 6)], &KeyEdit::Remove, (0, 9)), Some(vec![]));
        assert_eq!(k.pivot, None);
        assert_eq!(k.edit_keys(&[(Rotation, 6)], &KeyEdit::Remove, (0, 9)), None);
    }

    #[test]
    fn easing_changes_only_the_picked() {
        let mut k = keys();
        assert!(k.edit_keys(&[(Rotation, 4)], &KeyEdit::Ease(Easing::Hold), (0, 9)).is_some());
        assert_eq!(k.rotation.keys.first().map(|k| k.easing.clone()), Some(Easing::Hold));
        assert_eq!(k.opacity.keys.get(1).map(|k| k.easing.clone()), Some(Easing::Hold));
        assert_eq!(k.edit_keys(&[(Rotation, 4)], &KeyEdit::Ease(Easing::Hold), (0, 9)), None);
    }

    #[test]
    fn a_timeline_edits_within_the_clip() {
        let mut tl = Timeline::new(24, 20);
        let track = tl.add_track("A");
        let id = tl.add_drawing(track, 0, LayerId(1), "1");
        let id = id.unwrap_or(ClipId(0));
        if let Some(c) = tl.clip_mut(id) {
            c.duration = 5;
            c.keys = keys();
        }
        assert_eq!(tl.edit_keys(id, &[(Rotation, 6)], &KeyEdit::Move(3)), None);
        assert_eq!(tl.edit_keys(id, &[(Opacity, 0)], &KeyEdit::Move(3)), Some(vec![(Opacity, 3)]));
        assert_eq!(tl.edit_keys(ClipId(99), &[(Opacity, 3)], &KeyEdit::Remove), None);
    }

    /// Two drawings on one track, frames 0–9 and 10–19, each with `keys()`; a sound on another
    /// track, frames 2–11, its volume keyed on its frames 0 and 5; the camera keyed on 3 and 12.
    fn two_clips_a_sound_and_the_camera() -> (Timeline, ClipId, ClipId, ClipId) {
        let mut tl = Timeline::new(24, 30);
        let track = tl.add_track("A");
        let a = tl.add_drawing(track, 0, LayerId(1), "1").unwrap_or(ClipId(0));
        let b = tl.add_drawing(track, 10, LayerId(2), "2").unwrap_or(ClipId(0));
        for (id, start) in [(a, 0), (b, 10)] {
            if let Some(c) = tl.clip_mut(id) {
                c.start = start;
                c.duration = 10;
                c.keys = keys();
            }
        }
        let sounds = tl.add_track("S");
        let s = ClipId(tl.next_clip);
        tl.next_clip += 1;
        let volume_keys = Channel { keys: vec![key(0, 0.2, Easing::Linear), key(5, 1.0, Easing::Linear)] };
        if let Some(t) = tl.tracks.iter_mut().find(|t| t.id == sounds) {
            t.clips.push(Clip {
                id: s,
                name: "s".into(),
                start: 2,
                duration: 10,
                opacity: 1.0,
                blend: BlendMode::Normal,
                mask: MaskMode::None,
                adjustment: false,
                disabled: false,
                label: None,
                keys: ClipKeys::default(),
                content: ClipContent::Audio(AudioClip {
                    media: MediaId(1),
                    trim_start: 0.0,
                    offset: 0.0,
                    volume: 1.0,
                    muted: false,
                    volume_keys,
                }),
            });
        }
        tl.camera = Channel { keys: vec![key(3, CameraPose::still(10.0, 10.0), Easing::Linear), key(12, CameraPose::still(10.0, 10.0), Easing::Smooth)] };
        (tl, a, b, s)
    }

    fn frames_of(tl: &Timeline, lane: KeyLane) -> Vec<i64> {
        tl.lane_keys(lane).unwrap_or_default().into_iter().map(|(f, _)| f).collect()
    }

    #[test]
    fn keys_on_two_clips_the_camera_and_a_sound_move_together_as_far_as_all_have_room() {
        let (mut tl, a, b, s) = two_clips_a_sound_and_the_camera();
        assert_eq!(frames_of(&tl, KeyLane::Clip(b, Opacity)), [10, 14, 18]);
        assert_eq!(frames_of(&tl, KeyLane::Volume(s)), [2, 7]);
        let picks = [
            (KeyLane::Clip(a, Opacity), 4),
            (KeyLane::Clip(b, Rotation), 16),
            (KeyLane::Volume(s), 7),
            (KeyLane::Camera, 12),
        ];
        // two frames on: each moves in its own clip (or the timeline), the rest stay
        let ended = tl.edit_lane_keys(&picks, &KeyEdit::Move(2));
        assert_eq!(ended, Some(vec![
            (KeyLane::Clip(a, Opacity), 6),
            (KeyLane::Clip(b, Rotation), 18),
            (KeyLane::Volume(s), 9),
            (KeyLane::Camera, 14),
        ]));
        assert_eq!(frames_of(&tl, KeyLane::Clip(a, Opacity)), [0, 6, 8]);
        assert_eq!(frames_of(&tl, KeyLane::Clip(b, Rotation)), [14, 18]);
        assert_eq!(frames_of(&tl, KeyLane::Clip(b, Opacity)), [10, 14, 18]);
        assert_eq!(frames_of(&tl, KeyLane::Volume(s)), [2, 9]);
        assert_eq!(frames_of(&tl, KeyLane::Camera), [3, 14]);
        // b ends on 19, so they all go one frame more, no farther
        let picks = ended.unwrap_or_default();
        let ended = tl.edit_lane_keys(&picks, &KeyEdit::Move(5));
        assert_eq!(ended.as_ref().map(|e| e.iter().map(|&(_, f)| f).collect::<Vec<_>>()), Some(vec![7, 19, 10, 15]));
        // back: as far as a's key has room, seven frames
        let picks = ended.unwrap_or_default();
        let ended = tl.edit_lane_keys(&picks, &KeyEdit::Move(-20)).unwrap_or_default();
        assert_eq!(ended.iter().map(|&(_, f)| f).collect::<Vec<_>>(), [0, 12, 3, 8]);
    }

    #[test]
    fn keys_on_several_lanes_stretch_within_each_ones_room_and_reverse_each_on_its_own() {
        let (mut tl, a, b, _) = two_clips_a_sound_and_the_camera();
        // a's opacity 0, 4, 8 and the camera's 12, about 0, twice as far: the camera's 12 would go
        // to 24 (room up to 29), a's 8 to 16 (room up to 9): no farther than 9/8
        let picks = [(KeyLane::Clip(a, Opacity), 0), (KeyLane::Clip(a, Opacity), 4), (KeyLane::Clip(a, Opacity), 8), (KeyLane::Camera, 12)];
        let ended = tl.edit_lane_keys(&picks, &KeyEdit::Stretch { anchor: 0, factor: 2.0 }).unwrap_or_default();
        assert_eq!(ended.iter().map(|&(_, f)| f).collect::<Vec<_>>(), [0, 5, 9, 14]);
        // reversed: a's across 0–9 and b's across its own 14–16, apart
        let picks = [(KeyLane::Clip(a, Opacity), 0), (KeyLane::Clip(a, Opacity), 9), (KeyLane::Clip(b, Rotation), 14), (KeyLane::Clip(b, Rotation), 16)];
        assert!(tl.edit_lane_keys(&picks, &KeyEdit::Reverse).is_some());
        let values = |id: ClipId, ch: KeyChannel| -> Vec<(i32, f32)> {
            tl.clip(id)
                .map(|c| match ch {
                    Opacity => c.keys.opacity.keys.iter().map(|k| (k.frame, k.value)).collect(),
                    _ => c.keys.rotation.keys.iter().map(|k| (k.frame, k.value)).collect(),
                })
                .unwrap_or_default()
        };
        assert_eq!(values(a, Opacity), [(0, 1.0), (5, 0.5), (9, 0.0)]);
        assert_eq!(values(b, Rotation), [(4, 1.0), (6, 0.0)]);
        // shrunk toward the camera's key on 14: it stays where it is, still picked
        let picks = [(KeyLane::Clip(a, Opacity), 5), (KeyLane::Camera, 14)];
        let ended = tl.edit_lane_keys(&picks, &KeyEdit::Stretch { anchor: 14, factor: 0.5 });
        assert_eq!(ended, Some(vec![(KeyLane::Clip(a, Opacity), 9), (KeyLane::Camera, 14)]));
    }

    #[test]
    fn keys_on_several_lanes_ease_and_go_together() {
        let (mut tl, a, _, s) = two_clips_a_sound_and_the_camera();
        let picks = [(KeyLane::Clip(a, Rotation), 4), (KeyLane::Volume(s), 2), (KeyLane::Camera, 3)];
        assert!(tl.edit_lane_keys(&picks, &KeyEdit::Ease(Easing::Hold)).is_some());
        for lane in [KeyLane::Clip(a, Rotation), KeyLane::Volume(s), KeyLane::Camera] {
            assert_eq!(tl.lane_keys(lane).and_then(|k| k.first().map(|k| k.1.clone())), Some(Easing::Hold), "{lane:?}");
        }
        assert_eq!(tl.edit_lane_keys(&picks, &KeyEdit::Ease(Easing::Hold)), None);
        assert_eq!(tl.edit_lane_keys(&picks, &KeyEdit::Remove), Some(vec![]));
        assert_eq!(frames_of(&tl, KeyLane::Clip(a, Rotation)), [6]);
        assert_eq!(frames_of(&tl, KeyLane::Volume(s)), [7]);
        assert_eq!(frames_of(&tl, KeyLane::Camera), [12]);
        // keys that aren't there, or a volume on a drawing, are left out
        assert_eq!(tl.edit_lane_keys(&[(KeyLane::Camera, 3), (KeyLane::Volume(a), 4)], &KeyEdit::Move(1)), None);
    }
}

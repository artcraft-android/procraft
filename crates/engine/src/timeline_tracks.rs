//! Tracks reordered within their level, one track shown alone (solo), and where playback goes next
//! when part of the timeline is looped.

use std::collections::HashSet;

use super::*;

/// Every track's id at every depth, each level bottom first, a group's own tracks after the
/// track holding it.
fn every_track(tracks: &[Track], out: &mut Vec<TrackId>) {
    for t in tracks {
        out.push(t.id);
        for c in &t.clips {
            if let ClipContent::Group(g) = &c.content {
                every_track(&g.tracks, out);
            }
        }
    }
}

impl Timeline {
    /// Every track's id, at every depth.
    pub fn every_track(&self) -> Vec<TrackId> {
        let mut out = Vec::new();
        every_track(&self.tracks, &mut out);
        out
    }

    /// Track `id` moved to place `to` among the tracks of its own level (bottom first; past the
    /// top is the top). False if it isn't there or is there already.
    pub fn move_track(&mut self, id: TrackId, to: usize) -> bool {
        let Some(level) = level_of_track(&mut self.tracks, id) else { return false };
        let Some(from) = level.iter().position(|t| t.id == id) else { return false };
        let to = to.min(level.len().saturating_sub(1));
        if from == to {
            return false;
        }
        let track = level.remove(from);
        level.insert(to, track);
        true
    }

    /// What showing track `id` alone touches: the tracks it keeps shown (itself, and each track
    /// holding a group clip it is inside, so it can draw) and those it leaves as they are (the
    /// tracks inside its own group clips). None if there is no such track.
    fn solo_scope(&self, id: TrackId) -> Option<(HashSet<TrackId>, HashSet<TrackId>)> {
        let groups = self.groups_of_track(id)?;
        let mut kept: HashSet<TrackId> = groups.iter().filter_map(|g| self.track_of_clip(g.id)).collect();
        kept.insert(id);
        let mut inner = Vec::new();
        for c in &self.track(id)?.clips {
            if let ClipContent::Group(g) = &c.content {
                every_track(&g.tracks, &mut inner);
            }
        }
        Some((kept, inner.into_iter().collect()))
    }

    /// Whether track `id` shows alone: it and the tracks holding the groups it is in shown, every
    /// other track hidden (those inside its own groups aside), with one other track at least.
    pub fn soloed(&self, id: TrackId) -> bool {
        let Some((kept, inner)) = self.solo_scope(id) else { return false };
        let mut others = 0;
        for t in self.every_track() {
            if inner.contains(&t) {
                continue;
            }
            let Some(hidden) = self.track(t).map(|t| t.hidden) else { continue };
            if kept.contains(&t) {
                if hidden {
                    return false;
                }
            } else {
                others += 1;
                if !hidden {
                    return false;
                }
            }
        }
        others > 0
    }

    /// Every track's hidden flag, to put back after a solo.
    pub fn hidden_flags(&self) -> Vec<(TrackId, bool)> {
        self.every_track().into_iter().filter_map(|id| Some((id, self.track(id)?.hidden))).collect()
    }

    /// Track `id` shown alone (see [`Timeline::soloed`]). False if nothing changed.
    pub fn solo(&mut self, id: TrackId) -> bool {
        let Some((kept, inner)) = self.solo_scope(id) else { return false };
        let flags: Vec<(TrackId, bool)> =
            self.every_track().into_iter().filter(|t| !inner.contains(t)).map(|t| (t, !kept.contains(&t))).collect();
        self.set_hidden(&flags)
    }

    /// Each listed track hidden or shown as listed; tracks no longer there are passed over. False
    /// if every one was so already.
    pub fn set_hidden(&mut self, flags: &[(TrackId, bool)]) -> bool {
        let mut changed = false;
        for (id, hidden) in flags {
            if let Some(t) = self.track_mut(*id)
                && t.hidden != *hidden
            {
                t.hidden = *hidden;
                changed = true;
            }
        }
        changed
    }

    /// Where playback goes from frame `at` playing `step` (1 forwards, -1 backwards): within the
    /// loop range if there is one (from its start when `at` is outside it), else the whole
    /// timeline; at an end it loops, turns back or stops as [`Timeline::playback`] says. The next
    /// frame and the step from it, none when it stops.
    pub fn next_played(&self, at: u32, step: i32) -> Option<(u32, i32)> {
        let (from, to) = self.loop_range.filter(|(a, b)| a < b && *b <= self.duration).unwrap_or((0, self.duration));
        if to <= from {
            return None;
        }
        if at < from || at >= to {
            return Some((from, 1));
        }
        let step = if step < 0 { -1 } else { 1 };
        let next = i64::from(at) + i64::from(step);
        if (i64::from(from)..i64::from(to)).contains(&next) {
            return Some((u32::try_from(next).ok()?, step));
        }
        match self.playback {
            Playback::Loop => Some((if step > 0 { from } else { to - 1 }, step)),
            Playback::PingPong if to - from > 1 => {
                let back = i64::from(at) - i64::from(step);
                Some((u32::try_from(back).ok()?, -step))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn three() -> (Timeline, [TrackId; 3]) {
        let mut tl = Timeline::new(24, 24);
        let a = tl.add_track("A");
        let b = tl.add_track("B");
        let c = tl.add_track("C");
        (tl, [a, b, c])
    }

    /// Group clip `g`'s own tracks, bottom first.
    fn inside(tl: &Timeline, g: ClipId) -> Vec<TrackId> {
        match tl.clip(g).map(|c| &c.content) {
            Some(ClipContent::Group(gr)) => gr.tracks.iter().map(|t| t.id).collect(),
            _ => Vec::new(),
        }
    }

    fn order(tl: &Timeline) -> Vec<TrackId> {
        tl.tracks.iter().map(|t| t.id).collect()
    }

    #[test]
    fn a_track_moves_within_its_level() {
        let (mut tl, [a, b, c]) = three();
        assert!(tl.move_track(a, 2));
        assert_eq!(order(&tl), vec![b, c, a]);
        assert!(!tl.move_track(a, 99), "already on top");
        assert!(tl.move_track(c, 0));
        assert_eq!(order(&tl), vec![c, b, a]);
        assert!(!tl.move_track(TrackId(999), 0));
    }

    #[test]
    fn a_track_inside_a_group_moves_among_the_groups_tracks() {
        let (mut tl, [a, _, _]) = three();
        let x = tl.add_drawing(a, 0, LayerId(1), "x").unwrap();
        let g = tl.group(&[x], "G").unwrap();
        let inner = tl.add_group_track(g, "Inner").unwrap();
        let first = inside(&tl, g)[0];
        assert!(tl.move_track(inner, 0));
        assert_eq!(inside(&tl, g), vec![inner, first]);
        assert_eq!(tl.tracks.len(), 3, "the top level is as it was");
    }

    #[test]
    fn solo_hides_the_others_and_keeps_the_groups_around_it() {
        let (mut tl, [a, b, c]) = three();
        assert!(!tl.soloed(b));
        assert!(tl.solo(b));
        assert!(tl.soloed(b));
        assert!(!tl.soloed(a));
        assert_eq!(tl.tracks.iter().map(|t| t.hidden).collect::<Vec<_>>(), vec![true, false, true]);
        assert!(!tl.solo(b), "already alone");

        // inside a group: the track holding the group stays shown
        let x = tl.add_drawing(c, 0, LayerId(1), "x").unwrap();
        let g = tl.group(&[x], "G").unwrap();
        let inner = inside(&tl, g)[0];
        let second = tl.add_group_track(g, "Second").unwrap();
        assert!(tl.solo(inner));
        assert!(tl.soloed(inner));
        assert!(!tl.track(c).unwrap().hidden && tl.track(b).unwrap().hidden && tl.track(second).unwrap().hidden);

        // the tracks inside a soloed track's group are left as they are: the track holding the
        // group shows alone already
        assert!(tl.soloed(c));
        assert!(!tl.solo(c));
        tl.track_mut(a).unwrap().hidden = false;
        assert!(!tl.soloed(c));
        assert!(tl.solo(c));
        assert!(tl.soloed(c));
        assert!(tl.track(a).unwrap().hidden && tl.track(second).unwrap().hidden && !tl.track(inner).unwrap().hidden);
    }

    #[test]
    fn hidden_flags_go_back() {
        let (mut tl, [a, b, _]) = three();
        tl.track_mut(a).unwrap().hidden = true;
        let kept = tl.hidden_flags();
        assert!(tl.solo(b));
        assert!(tl.set_hidden(&kept));
        assert_eq!(tl.hidden_flags(), kept);
        assert!(!tl.set_hidden(&kept));
    }

    #[test]
    fn playback_stays_in_the_loop_range() {
        let mut tl = Timeline::new(24, 24);
        assert_eq!(tl.next_played(5, 1), Some((6, 1)));
        assert_eq!(tl.next_played(23, 1), Some((0, 1)));
        tl.loop_range = Some((4, 8));
        assert_eq!(tl.next_played(0, 1), Some((4, 1)), "from outside it starts at the range's start");
        assert_eq!(tl.next_played(6, 1), Some((7, 1)));
        assert_eq!(tl.next_played(7, 1), Some((4, 1)));
        tl.playback = Playback::PingPong;
        assert_eq!(tl.next_played(7, 1), Some((6, -1)));
        assert_eq!(tl.next_played(4, -1), Some((5, 1)));
        tl.playback = Playback::OneShot;
        assert_eq!(tl.next_played(7, 1), None);
        tl.loop_range = Some((8, 30));
        assert_eq!(tl.next_played(23, 1), None, "a range past the end is the whole timeline");
    }
}

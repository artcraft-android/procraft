//! A group clip shown folded or unfolded in the panel, and a cycling group's repeats counted:
//! how many it plays, as many more as its track has room for, and that many set.

use super::*;

/// The most repeats a cycling group is given by dragging its end.
pub const MOST_CYCLES: u32 = 999;

impl Timeline {
    /// Group clip `id` shown folded (`folded`) or with its tracks unfolded under its own in the
    /// panel. False if it isn't a group or is that already.
    pub fn set_folded(&mut self, id: ClipId, folded: bool) -> bool {
        match self.clip_mut(id).map(|c| &mut c.content) {
            Some(ClipContent::Group(g)) if g.collapsed != folded => {
                g.collapsed = folded;
                true
            }
            _ => false,
        }
    }

    /// How many times group clip `id` plays its cycle over its span (a last one cut short counts);
    /// none if it isn't a group that cycles.
    pub fn cycles(&self, id: ClipId) -> Option<u32> {
        let clip = self.clip(id)?;
        let ClipContent::Group(Group { cycle: Some(c), .. }) = &clip.content else { return None };
        Some(clip.duration.div_ceil(c.length.max(1)).max(1))
    }

    /// The most whole cycles group clip `id` has room for: up to the next clip on its track (with
    /// none, [`MOST_CYCLES`]); never fewer than one. None if it isn't a group that cycles.
    pub fn cycles_room(&self, id: ClipId) -> Option<u32> {
        let clip = self.clip(id)?;
        let ClipContent::Group(Group { cycle: Some(c), .. }) = &clip.content else { return None };
        let length = c.length.max(1);
        let track = self.track(self.track_of_clip(id)?)?;
        let next = track.clips.iter().find(|n| n.start > clip.start).map(|n| n.start);
        let most = next.map_or(MOST_CYCLES, |n| (n - clip.start) / length);
        Some(most.clamp(1, MOST_CYCLES))
    }

    /// Group clip `id` made `count` cycles long (as many as it has room for, at least one), from
    /// its start: the groups it is in grow to hold it, and the timeline too. False if it isn't a
    /// group that cycles, or is that long already.
    pub fn set_cycles(&mut self, id: ClipId, count: u32) -> bool {
        let (Some(room), Some(length)) = (self.cycles_room(id), self.cycle_length(id)) else { return false };
        let duration = count.clamp(1, room).saturating_mul(length);
        let Some(clip) = self.clip_mut(id) else { return false };
        if clip.duration == duration {
            return false;
        }
        clip.duration = duration;
        if let Some(track) = self.track_of_clip(id) {
            self.cover(track);
        }
        self.fit();
        true
    }

    /// The most frames cycling group clip `id` may last: up to the next clip on its track (with
    /// none, [`MOST_CYCLES`] cycles); never fewer than one. None if it isn't a group that cycles.
    pub fn group_room(&self, id: ClipId) -> Option<u32> {
        let length = self.cycle_length(id)?;
        let clip = self.clip(id)?;
        let track = self.track(self.track_of_clip(id)?)?;
        let next = track.clips.iter().find(|n| n.start > clip.start).map(|n| n.start);
        let most = next.map_or(MOST_CYCLES.saturating_mul(length), |n| n - clip.start);
        Some(most.clamp(1, MOST_CYCLES.saturating_mul(length)))
    }

    /// Cycling group clip `id` made `frames` long from its start (as long as it has room for, a
    /// frame at least), its last cycle cut short where that falls inside one: the groups it is in
    /// grow to hold it, and the timeline too. False if it isn't a group that cycles, or is that
    /// long already.
    pub fn set_group_span(&mut self, id: ClipId, frames: u32) -> bool {
        let Some(room) = self.group_room(id) else { return false };
        let duration = frames.clamp(1, room);
        let Some(clip) = self.clip_mut(id) else { return false };
        if clip.duration == duration {
            return false;
        }
        clip.duration = duration;
        if let Some(track) = self.track_of_clip(id) {
            self.cover(track);
        }
        self.fit();
        true
    }

    /// The frames in one of group clip `id`'s cycles, if it cycles.
    fn cycle_length(&self, id: ClipId) -> Option<u32> {
        match &self.clip(id)?.content {
            ClipContent::Group(Group { cycle: Some(c), .. }) => Some(c.length.max(1)),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drawing(id: u64, start: u32, duration: u32) -> Clip {
        Clip { id: ClipId(id), start, ..Clip::drawing("d", LayerId(id), start, duration) }
    }

    fn track(id: u64, clips: Vec<Clip>) -> Track {
        Track { id: TrackId(id), name: format!("T{id}"), hidden: false, locked: false, opacity: 1.0, look: Default::default(), clips }
    }

    /// A group of one two-frame drawing at frame 2, cycling every `cycle` frames over `duration`,
    /// with a drawing at frame 20 after it on the same track.
    fn cycling(cycle: u32, duration: u32) -> Timeline {
        let inner = track(2, vec![drawing(10, 0, 2)]);
        let group = Clip {
            content: ClipContent::Group(Group {
                tracks: vec![inner],
                collapsed: false,
                cycle: Some(Cycle { length: cycle, keys: CycleKeys::Repeat }),
            }),
            ..drawing(5, 2, duration)
        };
        let mut tl = Timeline::new(12, 1);
        tl.tracks = vec![track(1, vec![group, drawing(1, 20, 2)])];
        tl.next_track = 50;
        tl.next_clip = 500;
        tl.fit();
        tl
    }

    #[test]
    fn a_group_folds_and_unfolds() {
        let mut tl = cycling(3, 6);
        assert!(tl.set_folded(ClipId(5), true));
        assert!(!tl.set_folded(ClipId(5), true), "already");
        assert!(matches!(&tl.clip(ClipId(5)).unwrap().content, ClipContent::Group(g) if g.collapsed));
        assert!(tl.set_folded(ClipId(5), false));
        assert!(!tl.set_folded(ClipId(1), true), "not a group");
    }

    #[test]
    fn cycles_are_counted_with_a_short_last_one() {
        assert_eq!(cycling(3, 6).cycles(ClipId(5)), Some(2));
        assert_eq!(cycling(3, 7).cycles(ClipId(5)), Some(3));
        assert_eq!(cycling(3, 7).cycles(ClipId(1)), None, "a drawing doesn't cycle");
        // the drawing after it starts on frame 20: 18 frames, six cycles of three
        assert_eq!(cycling(3, 6).cycles_room(ClipId(5)), Some(6));
        assert_eq!(cycling(4, 6).cycles_room(ClipId(5)), Some(4));
    }

    #[test]
    fn a_count_of_cycles_sets_its_length_as_far_as_it_has_room() {
        let mut tl = cycling(3, 7);
        assert!(tl.set_cycles(ClipId(5), 4));
        assert_eq!(tl.clip(ClipId(5)).map(|c| (c.start, c.duration)), Some((2, 12)));
        assert!(!tl.set_cycles(ClipId(5), 4), "already");
        assert!(tl.set_cycles(ClipId(5), 40));
        assert_eq!(tl.clip(ClipId(5)).map(|c| c.duration), Some(18), "up to the next clip");
        assert!(tl.set_cycles(ClipId(5), 0));
        assert_eq!(tl.clip(ClipId(5)).map(|c| c.duration), Some(3), "one at least");
        assert!(!tl.set_cycles(ClipId(1), 2), "a drawing has no cycles");
    }

    #[test]
    fn a_cycling_group_ends_part_way_through_a_cycle() {
        // starts on frame 2, cycles every 3, the next clip on frame 20: 18 frames of room
        let mut tl = cycling(3, 6);
        assert_eq!(tl.group_room(ClipId(5)), Some(18));
        assert_eq!(tl.group_room(ClipId(1)), None, "a drawing doesn't cycle");
        assert!(tl.set_group_span(ClipId(5), 7));
        assert_eq!(tl.clip(ClipId(5)).map(|c| c.duration), Some(7));
        assert_eq!(tl.cycles(ClipId(5)), Some(3), "a last cycle cut short counts");
        assert!(!tl.set_group_span(ClipId(5), 7), "already");
        assert!(tl.set_group_span(ClipId(5), 99));
        assert_eq!(tl.clip(ClipId(5)).map(|c| c.duration), Some(18), "up to the next clip");
        assert!(tl.set_group_span(ClipId(5), 0));
        assert_eq!(tl.clip(ClipId(5)).map(|c| c.duration), Some(1), "a frame at least");
        assert!(!tl.set_group_span(ClipId(1), 4));
    }

    #[test]
    fn the_timeline_and_an_outer_group_grow_to_hold_the_cycles() {
        let mut tl = cycling(3, 6);
        tl.tracks[0].clips.pop();
        // the cycling group inside another
        let cycling_group = tl.tracks[0].clips.remove(0);
        let outer = Clip {
            content: ClipContent::Group(Group {
                tracks: vec![track(3, vec![cycling_group])],
                collapsed: false,
                cycle: None,
            }),
            ..drawing(6, 0, 8)
        };
        tl.tracks[0].clips.push(outer);
        assert!(tl.set_cycles(ClipId(5), 10));
        assert_eq!(tl.clip(ClipId(5)).map(|c| c.duration), Some(30));
        assert_eq!(tl.clip(ClipId(6)).map(|c| c.duration), Some(32), "the outer group covers it");
        assert!(tl.duration >= 32);
    }
}

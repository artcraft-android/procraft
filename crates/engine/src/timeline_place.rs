//! A clip put down over others, pushing them aside; clips picked together moved to
//! other tracks, trimmed and duplicated as one.

use std::collections::{HashMap, HashSet};

use super::*;

impl Timeline {
    /// Clip `id` put on track `track` (on any level: into a group clip or out of one) starting
    /// exactly on frame `start` of that track's level, the clips there that it would cover pushed
    /// aside: later when `later`, else earlier. Each clip pushed goes just past what it meets (the
    /// moved clip, or a clip pushed before it), so the push ripples on only as far as it has to.
    /// Pushed earlier past frame 0, they are pushed later instead. A group it goes
    /// into grows to cover it. False if it can't go there (a group into itself).
    pub fn place_clip(&mut self, id: ClipId, track: TrackId, start: u32, later: bool) -> bool {
        if self.groups_of_track(track).is_none_or(|gs| gs.iter().any(|g| g.id == id)) {
            return false;
        }
        let Some(duration) = self.clip(id).map(|c| c.duration) else { return false };
        let Some(dest) = self.track(track) else { return false };
        let staying: Vec<&Clip> = dest.clips.iter().filter(|c| c.id != id).collect();
        let Some(moves) = pushed(&staying, &[(start, start + duration)], later)
            .or_else(|| pushed(&staying, &[(start, start + duration)], true))
        else {
            return false;
        };
        let Some((level, t, c)) = level_of_clip(&mut self.tracks, id) else { return false };
        let Some(clip) = level.get_mut(t).map(|tr| tr.clips.remove(c)) else { return false };
        let Some(dest) = self.track_mut(track) else { return false };
        for c in &mut dest.clips {
            if let Some(&to) = moves.get(&c.id) {
                c.start = to;
            }
        }
        dest.clips.push(Clip { start, ..clip });
        dest.clips.sort_by_key(|c| c.start);
        self.cover(track);
        true
    }

    /// How far clips `ids` (each on its own level) can go `by` frames along (earlier when
    /// negative) while also going from their tracks to those as many places up or down as track
    /// `track` is from clip `primary`'s (its level's: `primary` and `track` on one level), and
    /// the tracks they would go to, in the order of `ids`. The nearest shift to `by` where each
    /// fits between the clips left behind there, at frame 0 or later. None if a track they would
    /// go to isn't there or is locked, one of theirs is locked, or one of them isn't there.
    pub fn move_room(&self, ids: &[ClipId], primary: ClipId, track: TrackId, by: i64) -> Option<(i64, Vec<TrackId>)> {
        let rows = self.rows_between(primary, track)?;
        let picked: HashSet<ClipId> = ids.iter().copied().collect();
        let mut goes = Vec::new();
        for id in ids {
            let (to, clip) = self.moved_to(*id, rows)?;
            let ahead: Vec<(u32, u32)> =
                to.clips.iter().filter(|c| !picked.contains(&c.id)).map(|c| (c.start, c.end())).collect();
            goes.push((to.id, clip.start, clip.end(), ahead));
        }
        let fits = |d: i64| {
            goes.iter().all(|(_, s, e, ahead)| {
                let (s, e) = (i64::from(*s) + d, i64::from(*e) + d);
                s >= 0 && ahead.iter().all(|(a, b)| s >= i64::from(*b) || e <= i64::from(*a))
            })
        };
        // the shifts where one of them just meets a clip there, or frame 0, and the one asked for
        let mut tries = vec![by];
        for (_, s, e, ahead) in &goes {
            tries.push(-i64::from(*s));
            for (a, b) in ahead {
                tries.push(i64::from(*b) - i64::from(*s));
                tries.push(i64::from(*a) - i64::from(*e));
            }
        }
        let best = tries.into_iter().filter(|d| fits(*d)).min_by_key(|d| ((d - by).abs(), d.abs()))?;
        Some((best, goes.into_iter().map(|g| g.0).collect()))
    }

    /// Clips `ids` moved as [`Timeline::move_room`] says: to the tracks as many places up or down
    /// as track `track` is from clip `primary`'s, shifted along by the nearest it allows to `by`
    /// frames. The groups they go into or leave keep covering their clips. Returns how far they
    /// went along, or none if they can't go there.
    pub fn move_clips(&mut self, ids: &[ClipId], primary: ClipId, track: TrackId, by: i64) -> Option<i64> {
        let (by, tracks) = self.move_room(ids, primary, track, by)?;
        // on a copy, so that nothing changes if one of them can't be moved after all
        let mut next = self.clone();
        let mut taken = Vec::new();
        for id in ids {
            let (level, t, c) = level_of_clip(&mut next.tracks, *id)?;
            let mut clip = level.get_mut(t).map(|tr| tr.clips.remove(c))?;
            clip.start = u32::try_from(i64::from(clip.start) + by).ok()?;
            taken.push(clip);
        }
        for (clip, to) in taken.into_iter().zip(&tracks) {
            let dest = next.track_mut(*to)?;
            let at = dest.clips.partition_point(|x| x.start < clip.start);
            dest.clips.insert(at, clip);
        }
        let mut covered = tracks;
        covered.dedup();
        for t in covered {
            next.cover(t);
        }
        *self = next;
        Some(by)
    }

    /// The `edge` of each of clips `ids` dragged `by` frames (earlier when negative) as
    /// [`Timeline::trim`] does with `policy`, one after another in time on each track (each from
    /// where the trims before it left it). Returns the tracks something changed on.
    pub fn trim_clips(&mut self, ids: &[ClipId], edge: Edge, by: i64, policy: EdgePolicy) -> Vec<TrackId> {
        let mut order: Vec<(u32, ClipId)> = ids.iter().filter_map(|id| Some((self.origin(*id)?, *id))).collect();
        order.sort();
        let mut changed = Vec::new();
        for (_, id) in order {
            let Some(clip) = self.clip(id) else { continue };
            let from = match edge {
                Edge::Start => clip.start,
                Edge::End => clip.end(),
            };
            let to = u32::try_from((i64::from(from) + by).max(0)).unwrap_or(u32::MAX);
            let Some(track) = self.track_of_clip(id) else { continue };
            if self.trim(id, edge, to, policy) && !changed.contains(&track) {
                changed.push(track);
            }
        }
        changed
    }

    /// Clips `ids` (all on one level) played in the opposite order: each mirrored across the
    /// frames they span together, its length kept, so the first of them becomes the last and the
    /// gaps between them turn round with them. A clip's keys, counted from its own start, go with
    /// it. All of them move or none does: false, and nothing changed, if they aren't all on one
    /// level, one is on a locked track, one would land on a clip that isn't among them, or
    /// mirroring leaves every one where it was.
    pub fn reverse_clips(&mut self, ids: &[ClipId]) -> bool {
        let picked: HashSet<ClipId> = ids.iter().copied().collect();
        let Some(&first) = ids.first() else { return false };
        let Some((level, _, _)) = level_of_clip(&mut self.tracks, first) else { return false };
        let on_level: Vec<&Clip> = level.iter().flat_map(|t| &t.clips).filter(|c| picked.contains(&c.id)).collect();
        if on_level.len() != picked.len() {
            return false;
        }
        let Some(lo) = on_level.iter().map(|c| c.start).min() else { return false };
        let hi = on_level.iter().map(|c| c.end()).max().unwrap_or(lo);
        let mut moved = false;
        let mut next: Vec<(usize, Vec<Clip>)> = Vec::new();
        for (t, track) in level.iter().enumerate() {
            if !track.clips.iter().any(|c| picked.contains(&c.id)) {
                continue;
            }
            if track.locked {
                return false;
            }
            let mut clips = track.clips.clone();
            for c in clips.iter_mut().filter(|c| picked.contains(&c.id)) {
                let start = lo + (hi - c.end());
                moved |= start != c.start;
                c.start = start;
            }
            clips.sort_by_key(|c| c.start);
            if clips.windows(2).any(|w| w[0].end() > w[1].start) {
                return false;
            }
            next.push((t, clips));
        }
        if !moved {
            return false;
        }
        for (t, clips) in next {
            level[t].clips = clips;
        }
        true
    }

    /// Where clips `ids` (all on one level) start being copied just after: the lowest of their
    /// tracks, and the frame (on that level) one past the last of them. None if one isn't there
    /// or they aren't all on one level.
    pub fn picked_span(&self, ids: &[ClipId]) -> Option<(TrackId, u32)> {
        let level = self.level_tracks(*ids.first()?)?;
        let mut low = None;
        let mut end = 0;
        let mut found = 0;
        for (t, track) in level.iter().enumerate() {
            for c in track.clips.iter().filter(|c| ids.contains(&c.id)) {
                low = Some(low.map_or(t, |l: usize| l.min(t)));
                end = end.max(c.end());
                found += 1;
            }
        }
        let wanted: HashSet<_> = ids.iter().collect();
        (found == wanted.len()).then_some(())?;
        Some((level.get(low?)?.id, end))
    }

    /// Clips `ids` (each on its own level) put down `by` frames along (earlier when negative) on
    /// the tracks as many places up or down as track `track` is from clip `primary`'s, each
    /// starting exactly there, the clips there they would cover pushed aside: later when `later`,
    /// else earlier (later where there is no room before them), each as far as it must, as
    /// [`Timeline::place_clip`] does for one. The groups they go into keep covering their clips.
    /// Returns the tracks something changed on (theirs and those they go to), or none if a track
    /// they would go to isn't there or is locked, one of theirs is, or one would start before
    /// frame 0.
    pub fn place_clips(
        &mut self,
        ids: &[ClipId],
        primary: ClipId,
        track: TrackId,
        by: i64,
        later: bool,
    ) -> Option<Vec<TrackId>> {
        let rows = self.rows_between(primary, track)?;
        let picked: HashSet<ClipId> = ids.iter().copied().collect();
        // where each goes, checked before anything changes
        let mut goes = Vec::new();
        for id in ids {
            let (to, clip) = self.moved_to(*id, rows)?;
            let from = self.track_of_clip(*id)?;
            let start = u32::try_from(i64::from(clip.start) + by).ok()?;
            goes.push((*id, from, to.id, start, clip.duration));
        }
        let mut next = self.clone();
        let mut taken = Vec::new();
        for (id, ..) in &goes {
            let (level, t, c) = level_of_clip(&mut next.tracks, *id)?;
            taken.push(level.get_mut(t).map(|tr| tr.clips.remove(c))?);
        }
        let mut changed: Vec<TrackId> = Vec::new();
        for (_, from, to, ..) in &goes {
            for t in [*from, *to] {
                if !changed.contains(&t) {
                    changed.push(t);
                }
            }
        }
        // on each track they go to, what stays there pushed aside from all of theirs landing on it
        let mut dests: Vec<TrackId> = Vec::new();
        for g in &goes {
            if !dests.contains(&g.2) {
                dests.push(g.2);
            }
        }
        for dest in &dests {
            let spans: Vec<(u32, u32)> =
                goes.iter().filter(|g| g.2 == *dest).map(|g| (g.3, g.3 + g.4)).collect();
            let staying: Vec<Clip> =
                next.track(*dest)?.clips.iter().filter(|c| !picked.contains(&c.id)).cloned().collect();
            let refs: Vec<&Clip> = staying.iter().collect();
            let moves = pushed(&refs, &spans, later).or_else(|| pushed(&refs, &spans, true))?;
            for c in &mut next.track_mut(*dest)?.clips {
                if let Some(&to) = moves.get(&c.id) {
                    c.start = to;
                }
            }
        }
        for (clip, (_, _, to, start, _)) in taken.into_iter().zip(&goes) {
            let dest = next.track_mut(*to)?;
            dest.clips.push(Clip { start: *start, ..clip });
            dest.clips.sort_by_key(|c| c.start);
        }
        for t in &dests {
            next.cover(*t);
        }
        next.fit();
        *self = next;
        Some(changed)
    }

    /// Clips `ids` (each on group clip `group`'s level) put into it together, each `by` frames
    /// along from where it is (earlier when negative; no earlier than the group's start), as
    /// [`Timeline::move_into_group`] does for one, earliest first. Returns the tracks they went
    /// onto, or none (nothing changed) if one can't go in.
    pub fn move_clips_into_group(&mut self, ids: &[ClipId], group: ClipId, by: i64) -> Option<Vec<TrackId>> {
        if ids.contains(&group) {
            return None;
        }
        let mut order: Vec<(u32, ClipId)> = ids.iter().map(|id| Some((self.origin(*id)?, *id))).collect::<Option<_>>()?;
        order.sort();
        let mut next = self.clone();
        let mut onto = Vec::new();
        for (_, id) in order {
            let clip = next.clip_mut(id)?;
            clip.start = u32::try_from((i64::from(clip.start) + by).max(0)).ok()?;
            onto.push(next.move_into_group(id, group)?);
        }
        *self = next;
        Some(onto)
    }

    /// The group clips holding track `id` grown to cover their clips, innermost first, each
    /// pushing the clips after it on its own track along as far as it grows, and the timeline to
    /// cover them: after clips are put down on a group's own track from outside the engine
    /// (pasted, say). A group that cycles grows its cycle to hold them, playing as
    /// many cycles as before (and as much of a last one cut short).
    pub fn grow_groups_pushing(&mut self, id: TrackId) {
        let mut track = id;
        while let Some(group) = self.groups_of_track(track).and_then(|gs| gs.last().map(|g| g.id)) {
            let (needed, cycle) = match self.clip(group).map(|g| &g.content) {
                Some(ClipContent::Group(g)) => (
                    g.tracks.iter().flat_map(|t| t.clips.last()).map(Clip::end).max(),
                    g.cycle.map(|c| c.length.max(1)),
                ),
                _ => (None, None),
            };
            let grown = match (needed, cycle, self.clip(group)) {
                (Some(end), None, Some(g)) if g.duration < end => Some(end),
                (Some(end), Some(length), Some(g)) if length < end => {
                    let span = (g.duration / length).saturating_mul(end).saturating_add(g.duration % length);
                    if let Some(ClipContent::Group(Group { cycle: Some(c), .. })) =
                        self.clip_mut(group).map(|g| &mut g.content)
                    {
                        c.length = end;
                    }
                    Some(span)
                }
                _ => None,
            };
            if let (Some(span), Some(g)) = (grown, self.clip(group)) {
                let to = g.start + span;
                self.trim(group, Edge::End, to, EdgePolicy::Push);
            }
            let Some(next) = self.track_of_clip(group) else { break };
            track = next;
        }
        self.fit();
    }

    /// How many places up (down when negative) track `track` is from clip `id`'s, on one level.
    fn rows_between(&self, id: ClipId, track: TrackId) -> Option<i64> {
        let level = self.level_tracks(id)?;
        let from = level.iter().position(|t| t.clips.iter().any(|c| c.id == id))?;
        let to = level.iter().position(|t| t.id == track)?;
        Some(i64::try_from(to).ok()? - i64::try_from(from).ok()?)
    }

    /// The track `rows` places up (down when negative) from clip `id`'s on its level, and the
    /// clip; none if there is none there, or either track is locked.
    fn moved_to(&self, id: ClipId, rows: i64) -> Option<(&Track, &Clip)> {
        let level = self.level_tracks(id)?;
        let from = level.iter().position(|t| t.clips.iter().any(|c| c.id == id))?;
        let to = usize::try_from(i64::try_from(from).ok()? + rows).ok()?;
        let (here, there) = (level.get(from)?, level.get(to)?);
        if here.locked || there.locked {
            return None;
        }
        Some((there, here.clips.iter().find(|c| c.id == id)?))
    }
}

/// Where clips `staying` go when frames `spans` (each a start and one past the end) are put down
/// among them, by id (only those that move): each one they cover, or that a clip pushed before it
/// would cover, pushed past it, later when `later` else earlier. None if pushing earlier would go
/// before frame 0.
fn pushed(staying: &[&Clip], spans: &[(u32, u32)], later: bool) -> Option<HashMap<ClipId, u32>> {
    let mut taken = spans.to_vec();
    let mut order: Vec<&&Clip> = staying.iter().collect();
    if later {
        order.sort_by_key(|c| c.start);
    } else {
        order.sort_by_key(|c| std::cmp::Reverse(c.start));
    }
    let mut moves = HashMap::new();
    for c in order {
        let mut start = c.start;
        while let Some(&(a, b)) = taken.iter().find(|(a, b)| start.max(*a) < (start + c.duration).min(*b)) {
            start = if later { b } else { a.checked_sub(c.duration)? };
        }
        taken.push((start, start + c.duration));
        if start != c.start {
            moves.insert(c.id, start);
        }
    }
    Some(moves)
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

    fn timeline(tracks: Vec<Track>) -> Timeline {
        let mut tl = Timeline::new(12, 1);
        tl.tracks = tracks;
        tl.next_track = 50;
        tl.next_clip = 500;
        tl.fit();
        tl
    }

    fn spans(tl: &Timeline, t: usize) -> Vec<(u64, u32, u32)> {
        tl.tracks.get(t).map_or_else(Vec::new, |t| t.clips.iter().map(|c| (c.id.0, c.start, c.end())).collect())
    }

    #[test]
    fn a_clip_put_down_pushes_the_clips_it_covers_aside() {
        // 0..4, 6..10, 14..16 on the lower track; a 3-frame clip above
        let base = timeline(vec![
            track(1, vec![drawing(1, 0, 4), drawing(2, 6, 4), drawing(3, 14, 2)]),
            track(2, vec![drawing(9, 0, 3)]),
        ]);
        // over the middle one, pushing later: it goes just past, the last one has room
        let mut tl = base.clone();
        assert!(tl.place_clip(ClipId(9), TrackId(1), 7, true));
        assert_eq!(spans(&tl, 0), [(1, 0, 4), (9, 7, 10), (2, 10, 14), (3, 14, 16)]);
        assert!(tl.tracks[1].clips.is_empty());
        // pushing later ripples on through what it then meets
        let mut tl = base.clone();
        assert!(tl.place_clip(ClipId(9), TrackId(1), 12, true));
        assert_eq!(spans(&tl, 0), [(1, 0, 4), (2, 6, 10), (9, 12, 15), (3, 15, 17)]);
        assert_eq!(tl.duration, base.duration, "the last one parked past the end");
        // pushing earlier: the middle one goes before it, then the first one before that
        let mut tl = base.clone();
        assert!(tl.place_clip(ClipId(9), TrackId(1), 8, false));
        assert_eq!(spans(&tl, 0), [(1, 0, 4), (2, 4, 8), (9, 8, 11), (3, 14, 16)]);
        // no room before frame 0: pushed later instead
        let mut tl = base.clone();
        assert!(tl.place_clip(ClipId(9), TrackId(1), 1, false));
        assert_eq!(spans(&tl, 0), [(9, 1, 4), (1, 4, 8), (2, 8, 12), (3, 14, 16)]);
        // into free room: nothing else moves
        let mut tl = base;
        assert!(tl.place_clip(ClipId(9), TrackId(1), 10, true));
        assert_eq!(spans(&tl, 0), [(1, 0, 4), (2, 6, 10), (9, 10, 13), (3, 14, 16)]);
    }

    #[test]
    fn a_clip_moved_along_its_own_track_pushes_its_neighbours() {
        let mut tl = timeline(vec![track(1, vec![drawing(1, 0, 4), drawing(2, 4, 4), drawing(3, 8, 4)])]);
        // the first onto the second: it is pushed later, into the third, which moves on
        assert!(tl.place_clip(ClipId(1), TrackId(1), 3, true));
        assert_eq!(spans(&tl, 0), [(1, 3, 7), (2, 7, 11), (3, 11, 15)]);
    }

    #[test]
    fn picked_clips_move_to_other_tracks_as_far_as_they_fit() {
        // picked: 1 (track 1) and 2 (track 2); track 3 has a clip at 4..8
        let base = timeline(vec![
            track(1, vec![drawing(1, 0, 2)]),
            track(2, vec![drawing(2, 2, 2)]),
            track(3, vec![drawing(3, 4, 4)]),
        ]);
        let ids = [ClipId(1), ClipId(2)];
        // one track up: 2 goes onto track 3, where 2..4 is free
        let (by, to) = base.move_room(&ids, ClipId(1), TrackId(2), 0).unwrap();
        assert_eq!((by, to), (0, vec![TrackId(2), TrackId(3)]));
        // asked 3 along, 2 would sit on 5..7 over the clip there; 0 and 6 (8..10) fit, both 3
        // away: the smaller shift wins. Asked 5, 6 is nearer
        assert_eq!(base.move_room(&ids, ClipId(1), TrackId(2), 3).map(|r| r.0), Some(0));
        assert_eq!(base.move_room(&ids, ClipId(1), TrackId(2), 5).map(|r| r.0), Some(6));
        // two up: track 4 isn't there
        assert_eq!(base.move_room(&ids, ClipId(1), TrackId(3), 0), None);
        let mut tl = base.clone();
        assert_eq!(tl.move_clips(&ids, ClipId(1), TrackId(2), 5), Some(6));
        assert_eq!(spans(&tl, 0), []);
        assert_eq!(spans(&tl, 1), [(1, 6, 8)]);
        assert_eq!(spans(&tl, 2), [(3, 4, 8), (2, 8, 10)]);
        // a locked track they would go onto refuses them
        let mut tl = base;
        if let Some(t) = tl.tracks.get_mut(2) {
            t.locked = true;
        }
        assert_eq!(tl.move_clips(&ids, ClipId(1), TrackId(2), 0), None);
    }

    #[test]
    fn picked_clips_trim_together() {
        let base = timeline(vec![
            track(1, vec![drawing(1, 0, 4), drawing(2, 4, 4)]),
            track(2, vec![drawing(3, 2, 3)]),
        ]);
        let ids = [ClipId(1), ClipId(2), ClipId(3)];
        // ends two later, pushing: 1's push moves 2 along, which then grows too
        let mut tl = base.clone();
        assert_eq!(tl.trim_clips(&ids, Edge::End, 2, EdgePolicy::Push), [TrackId(1), TrackId(2)]);
        assert_eq!(spans(&tl, 0), [(1, 0, 6), (2, 6, 12)]);
        assert_eq!(spans(&tl, 1), [(3, 2, 7)]);
        // stopping: 1 can't grow into 2; 2 and 3 do
        let mut tl = base.clone();
        tl.trim_clips(&ids, Edge::End, 2, EdgePolicy::Clamp);
        assert_eq!(spans(&tl, 0), [(1, 0, 4), (2, 4, 10)]);
        assert_eq!(spans(&tl, 1), [(3, 2, 7)]);
        // starts a frame later
        let mut tl = base;
        tl.trim_clips(&ids, Edge::Start, 1, EdgePolicy::Clamp);
        assert_eq!(spans(&tl, 0), [(1, 1, 4), (2, 5, 8)]);
        assert_eq!(spans(&tl, 1), [(3, 3, 5)]);
    }

    #[test]
    fn picked_clips_put_down_push_aside_what_they_cover() {
        // picked: 1 (track 1, 0..2) and 2 (track 2, 0..2); track 3 has 3 at 4..8, track 2 has 4 at
        // 4..6 (not picked)
        let base = timeline(vec![
            track(1, vec![drawing(1, 0, 2)]),
            track(2, vec![drawing(2, 0, 2), drawing(4, 4, 2)]),
            track(3, vec![drawing(3, 4, 4)]),
        ]);
        let ids = [ClipId(1), ClipId(2)];
        // along their own tracks, 5 later: 2 lands on 5..7 over 4, which is pushed on to 7..9
        let mut tl = base.clone();
        let changed = tl.place_clips(&ids, ClipId(1), TrackId(1), 5, true).unwrap();
        assert_eq!(changed, [TrackId(1), TrackId(2)]);
        assert_eq!(spans(&tl, 0), [(1, 5, 7)]);
        assert_eq!(spans(&tl, 1), [(2, 5, 7), (4, 7, 9)]);
        // a track up, 4 later: 1 onto track 2 at 4..6 (4 pushed later to 6..8), 2 onto track 3
        // at 4..6 (3 pushed later to 6..10)
        let mut tl = base.clone();
        tl.place_clips(&ids, ClipId(1), TrackId(2), 4, true).unwrap();
        assert_eq!(spans(&tl, 0), []);
        assert_eq!(spans(&tl, 1), [(1, 4, 6), (4, 6, 8)]);
        assert_eq!(spans(&tl, 2), [(2, 4, 6), (3, 6, 10)]);
        // pushing earlier: what they cover goes before them
        let mut tl = base.clone();
        tl.place_clips(&ids, ClipId(1), TrackId(2), 4, false).unwrap();
        assert_eq!(spans(&tl, 1), [(4, 2, 4), (1, 4, 6)]);
        assert_eq!(spans(&tl, 2), [(3, 0, 4), (2, 4, 6)]);
        // before frame 0, or onto a track that isn't there: refused, nothing changes
        let mut tl = base.clone();
        assert_eq!(tl.place_clips(&ids, ClipId(1), TrackId(1), -1, true), None);
        assert_eq!(tl.place_clips(&ids, ClipId(1), TrackId(3), 0, true), None);
        assert_eq!(tl, base);
    }

    #[test]
    fn picked_clips_go_into_a_group_together() {
        let group = Clip {
            content: ClipContent::Group(Group {
                tracks: vec![track(9, vec![drawing(90, 0, 2)])],
                collapsed: false,
                cycle: None,
            }),
            ..drawing(8, 0, 4)
        };
        let base = timeline(vec![track(1, vec![group]), track(2, vec![drawing(1, 1, 2)]), track(3, vec![drawing(2, 2, 3)])]);
        let mut tl = base.clone();
        let onto = tl.move_clips_into_group(&[ClipId(2), ClipId(1)], ClipId(8), 0).unwrap();
        assert_eq!(onto.len(), 2);
        assert_eq!(tl.group_of(ClipId(1)), Some(ClipId(8)));
        assert_eq!(tl.group_of(ClipId(2)), Some(ClipId(8)));
        assert_eq!(tl.origin(ClipId(2)), Some(2), "at the frame it was at");
        assert_eq!(tl.clip(ClipId(8)).map(|c| c.duration), Some(5), "the group grows to cover it");
        // dropped two frames on: each goes in two frames later than it was
        let mut tl = base.clone();
        tl.move_clips_into_group(&[ClipId(2), ClipId(1)], ClipId(8), 2).unwrap();
        assert_eq!((tl.origin(ClipId(1)), tl.origin(ClipId(2))), (Some(3), Some(4)));
        assert_eq!(tl.clip(ClipId(8)).map(|c| c.duration), Some(7));
        // one that can't go in (the group itself): none of them do
        let mut tl = base.clone();
        assert_eq!(tl.move_clips_into_group(&[ClipId(1), ClipId(8)], ClipId(8), 0), None);
        assert_eq!(tl, base);
    }

    #[test]
    fn a_group_grows_over_its_clips_pushing_what_follows_it() {
        let group = Clip {
            content: ClipContent::Group(Group {
                tracks: vec![track(9, vec![drawing(90, 0, 2), drawing(91, 5, 2)])],
                collapsed: false,
                cycle: None,
            }),
            ..drawing(8, 0, 4)
        };
        let mut tl = timeline(vec![track(1, vec![group, drawing(1, 4, 2), drawing(2, 8, 1)])]);
        tl.grow_groups_pushing(TrackId(9));
        // everything after it along by as much as it grew, the gap between them kept
        assert_eq!(spans(&tl, 0), [(8, 0, 7), (1, 7, 9), (2, 11, 12)]);
    }

    #[test]
    fn a_cycling_group_grows_its_cycle_over_its_clips() {
        // a 3-frame cycle played twice and a frame more; a copy put down at 3 runs to 5
        let group = Clip {
            content: ClipContent::Group(Group {
                tracks: vec![track(9, vec![drawing(90, 0, 2), drawing(91, 3, 2)])],
                collapsed: false,
                cycle: Some(Cycle { length: 3, keys: CycleKeys::Repeat }),
            }),
            ..drawing(8, 0, 7)
        };
        let mut tl = timeline(vec![track(1, vec![group, drawing(1, 7, 2)])]);
        tl.grow_groups_pushing(TrackId(9));
        let length = match tl.clip(ClipId(8)).map(|c| &c.content) {
            Some(ClipContent::Group(Group { cycle: Some(c), .. })) => Some(c.length),
            _ => None,
        };
        assert_eq!(length, Some(5), "the cycle holds the copy");
        // still two cycles and a frame, the clip after it pushed along by as much
        assert_eq!(spans(&tl, 0), [(8, 0, 11), (1, 11, 13)]);
        assert!(tl.duration >= 13, "the timeline covers them");
    }

    #[test]
    fn the_span_of_picked_clips() {
        let tl = timeline(vec![
            track(1, vec![drawing(1, 0, 4)]),
            track(2, vec![drawing(2, 2, 6)]),
            track(3, vec![drawing(3, 0, 9)]),
        ]);
        assert_eq!(tl.picked_span(&[ClipId(2), ClipId(1)]), Some((TrackId(1), 8)));
        assert_eq!(tl.picked_span(&[ClipId(2), ClipId(77)]), None);
    }
    fn starts(tl: &Timeline, track: usize) -> Vec<(u64, u32, u32)> {
        tl.tracks[track].clips.iter().map(|c| (c.id.0, c.start, c.duration)).collect()
    }

    #[test]
    fn reversed_clips_swap_places_in_time_with_their_lengths_kept() {
        // a frame, a frame held for three, a frame: mirrored across the five frames they span
        let mut tl = timeline(vec![track(1, vec![drawing(1, 2, 1), drawing(2, 3, 3), drawing(3, 6, 1)])]);
        assert!(tl.reverse_clips(&[ClipId(1), ClipId(2), ClipId(3)]));
        assert_eq!(starts(&tl, 0), [(3, 2, 1), (2, 3, 3), (1, 6, 1)]);
        // and back again
        assert!(tl.reverse_clips(&[ClipId(3), ClipId(2), ClipId(1)]));
        assert_eq!(starts(&tl, 0), [(1, 2, 1), (2, 3, 3), (3, 6, 1)]);
    }

    #[test]
    fn reversed_clips_turn_their_gaps_round_and_leave_the_others() {
        // a gap of two after the first picked clip becomes a gap of two before the last
        let mut tl = timeline(vec![track(1, vec![drawing(9, 0, 1), drawing(1, 1, 1), drawing(2, 4, 2), drawing(8, 10, 1)])]);
        assert!(tl.reverse_clips(&[ClipId(1), ClipId(2)]));
        assert_eq!(starts(&tl, 0), [(9, 0, 1), (2, 1, 2), (1, 5, 1), (8, 10, 1)]);
    }

    #[test]
    fn clips_reversed_on_several_tracks_mirror_across_all_they_span() {
        let mut tl = timeline(vec![
            track(1, vec![drawing(1, 0, 2), drawing(2, 2, 1)]),
            track(2, vec![drawing(3, 4, 2)]),
        ]);
        assert!(tl.reverse_clips(&[ClipId(1), ClipId(2), ClipId(3)]));
        // they span frames 0 to 6: what ended last now starts first
        assert_eq!(starts(&tl, 0), [(2, 3, 1), (1, 4, 2)]);
        assert_eq!(starts(&tl, 1), [(3, 0, 2)]);
    }

    #[test]
    fn nothing_is_reversed_onto_another_clip_on_a_locked_track_or_alone() {
        // the clip between them isn't picked: the longer one would land on it
        let tracks = vec![track(1, vec![drawing(1, 0, 3), drawing(7, 3, 1), drawing(2, 4, 1)])];
        let mut tl = timeline(tracks.clone());
        assert!(!tl.reverse_clips(&[ClipId(1), ClipId(2)]));
        assert_eq!(tl.tracks, tracks);
        // one alone has nothing to change places with
        assert!(!tl.reverse_clips(&[ClipId(1)]));
        // a clip that isn't there
        assert!(!tl.reverse_clips(&[ClipId(1), ClipId(99)]));
        // a locked track's clips stay put
        let mut tl = timeline(vec![track(1, vec![drawing(1, 0, 1), drawing(2, 1, 2)])]);
        tl.tracks[0].locked = true;
        assert!(!tl.reverse_clips(&[ClipId(1), ClipId(2)]));
        assert_eq!(starts(&tl, 0), [(1, 0, 1), (2, 1, 2)]);
    }
}

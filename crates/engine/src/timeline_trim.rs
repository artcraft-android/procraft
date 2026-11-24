//! A clip's edges dragged: either edge to a frame, and what that does on reaching the clip next to
//! it (stopping there, pushing the clips beyond along, or trimming into it). And several clips
//! moved along their tracks together.

use std::collections::HashSet;

use super::*;

/// One edge of a clip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Edge {
    /// Its first frame.
    Start,
    /// One past its last frame.
    End,
}

impl Timeline {
    /// Clip `id`'s `edge` dragged to frame `to` on its own level, the other edge staying put, as
    /// `policy` says on reaching the clip next to it:
    ///
    /// - [`EdgePolicy::Clamp`]: it stops there.
    /// - [`EdgePolicy::Push`]: the clips beyond that edge move with it, both ways, the gaps
    ///   between them kept (those before a start as far as the first of them has room).
    /// - [`EdgePolicy::TrimNeighbour`]: it eats into that clip, down to its last frame; nothing
    ///   else moves.
    ///
    /// A start dragged takes the clip's keys along, so its motion stays where it showed; a group's
    /// own clips stay where they showed too (so its start goes no later than the first of them),
    /// and a sound or video starts that much later or earlier in its file (no earlier than the
    /// file's start). A group that cycles keeps its start. An end goes no later than a sound or
    /// video's file lasts. The clip is always a frame long at least.
    ///
    /// False if nothing changed.
    pub fn trim(&mut self, id: ClipId, edge: Edge, to: u32, policy: EdgePolicy) -> bool {
        let fps = f64::from(self.fps.max(1));
        let left = self.clip(id).and_then(|c| self.media_frames_left(c));
        let Some((level, t, c)) = level_of_clip(&mut self.tracks, id) else { return false };
        let Some(track) = level.get_mut(t) else { return false };
        let on = track.id;
        let changed = match edge {
            Edge::Start => trim_start(track, c, to, policy, fps),
            Edge::End => trim_end(track, c, to, policy, fps, left),
        };
        if changed {
            self.cover(on);
        }
        changed
    }
}

impl Timeline {
    /// Makes room for clip `id`'s end to go as late as frame `to` of its own level (one past its
    /// last frame): the clips after it on its track are moved later, the nearest first and each
    /// only as far as it must, so the empty frames between them are taken up before a clip
    /// further on moves at all. Returns how late its end can go now: `to`, or sooner for a sound
    /// or video, which lasts no longer than what is left of its file; its own end, when asked for
    /// no later.
    pub fn open_up_after(&mut self, id: ClipId, to: u32) -> Option<u32> {
        let clip = self.clip(id)?;
        let most = self.media_frames_left(clip).map_or(u32::MAX, |left| clip.start.saturating_add(left.max(1)));
        let to = to.min(most).max(clip.end());
        let track = self.track_of_clip(id)?;
        let (level, t, c) = level_of_clip(&mut self.tracks, id)?;
        let mut next = to;
        for x in level.get_mut(t)?.clips.iter_mut().skip(c + 1) {
            if x.start >= next {
                break;
            }
            x.start = next;
            next = x.end();
        }
        // (a group whose own clips went later lasts long enough to hold them)
        self.cover(track);
        Some(to)
    }

    /// The run of empty frames that frame `at` (of the track's own level) is in on track `id`:
    /// its first frame and how many it is, the count none where no clip comes after it (it goes
    /// on to wherever the timeline ends). None where a clip is on that frame.
    pub fn gap_at(&self, id: TrackId, at: u32) -> Option<(u32, Option<u32>)> {
        let track = self.track(id)?;
        if track.clips.iter().any(|c| c.start <= at && at < c.end()) {
            return None;
        }
        let start = track.clips.iter().filter(|c| c.end() <= at).map(Clip::end).max().unwrap_or(0);
        let next = track.clips.iter().filter(|c| c.start > at).map(|c| c.start).min();
        Some((start, next.map(|n| n - start)))
    }

    /// That run made `length` frames long (0: closed up), every clip after it on the track
    /// moving earlier or later to suit. False where frame `at` has a clip on it, no clip comes
    /// after the run, or it is that long already.
    pub fn set_gap(&mut self, id: TrackId, at: u32, length: u32) -> bool {
        let Some((start, Some(now))) = self.gap_at(id, at) else { return false };
        if now == length {
            return false;
        }
        let Some(track) = self.track_mut(id) else { return false };
        for c in track.clips.iter_mut().filter(|c| c.start >= start + now) {
            c.start = (c.start - now).saturating_add(length);
        }
        true
    }

    /// Makes room for clip `id`'s start to go as early as frame `to` of its own level: the clips
    /// before it on its track are moved earlier, the nearest first and each only as far as it
    /// must, so the empty frames between them are taken up before a clip further back moves, and
    /// none goes before the first frame. (Where [`EdgePolicy::Push`] moves every clip before a
    /// start together, the gaps between them kept, and so stops as soon as the first of them is
    /// on the first frame.) Returns the earliest frame the start can go to now: `to`, or later
    /// where the clips before it have no more room; its own start, when asked for no earlier.
    pub fn close_up_before(&mut self, id: ClipId, to: u32) -> Option<u32> {
        let (level, t, c) = level_of_clip(&mut self.tracks, id)?;
        let track = level.get_mut(t)?;
        // packed from the first frame, the clips before it take this many
        let taken: u32 = track.clips[..c].iter().map(|x| x.duration).sum();
        let to = to.max(taken).min(track.clips[c].start);
        // each ends no later than where the one after it now starts
        let mut next = to;
        for x in track.clips[..c].iter_mut().rev() {
            if x.end() > next {
                x.start = next - x.duration;
            }
            next = x.start;
        }
        Some(to)
    }

    /// Clips `ids` each made to last through the empty frames after it on its track: up to the
    /// next clip there or, with none after it, to the end of the timeline it is on (the
    /// document's; a group's own, which is as long as the group lasts, or as one cycle of it). Its
    /// start, and what it holds from there, stay put; a sound or video goes no further than its
    /// file lasts. One that has no empty frame after it is left as it is. Whether any grew.
    pub fn fill_out(&mut self, ids: &[ClipId]) -> bool {
        let mut grew = false;
        for &id in ids {
            let Some(end) = self.clip(id).map(Clip::end) else { continue };
            let Some(groups) = self.track_of_clip(id).and_then(|track| self.groups_of_track(track)) else { continue };
            let last = match groups.last() {
                None => self.duration,
                Some(group) => match &group.content {
                    ClipContent::Group(Group { cycle: Some(cycle), .. }) => cycle.length.min(group.duration),
                    _ => group.duration,
                },
            };
            // (stopping at the clip after it, as an edge dragged there does)
            grew |= last > end && self.trim(id, Edge::End, last, EdgePolicy::Clamp);
        }
        grew
    }

    /// How far clips `ids` can go together along their own tracks toward `by` frames (later, or
    /// earlier when negative): as far as asked, or until the first of them meets a clip that
    /// isn't one of them, or frame 0. Clips that aren't there are left out.
    pub fn shift_room(&self, ids: &[ClipId], by: i64) -> i64 {
        fn room(tracks: &[Track], ids: &HashSet<ClipId>, by: &mut i64) {
            for track in tracks {
                for (c, clip) in track.clips.iter().enumerate() {
                    if ids.contains(&clip.id) {
                        if *by > 0 {
                            // the next clip stays: no further than it
                            if let Some(next) = track.clips.get(c + 1).filter(|n| !ids.contains(&n.id)) {
                                *by = (*by).min(i64::from(next.start) - i64::from(clip.end()));
                            }
                        } else if *by < 0 {
                            let before = c.checked_sub(1).and_then(|p| track.clips.get(p));
                            if before.is_none_or(|p| !ids.contains(&p.id)) {
                                let floor = before.map_or(0, Clip::end);
                                *by = (*by).max(i64::from(floor) - i64::from(clip.start));
                            }
                        }
                    }
                    if let ClipContent::Group(g) = &clip.content {
                        room(&g.tracks, ids, by);
                    }
                }
            }
        }
        let ids: HashSet<ClipId> = ids.iter().copied().collect();
        let mut by = by;
        room(&self.tracks, &ids, &mut by);
        by
    }

    /// Clips `ids` moved together along their own tracks by as much of `by` frames as
    /// [`Timeline::shift_room`] allows, each keeping its length and what is between them. A group
    /// one of them is in grows to cover it. Returns how far they went (0: nowhere).
    pub fn shift_clips(&mut self, ids: &[ClipId], by: i64) -> i64 {
        fn shift(tracks: &mut [Track], ids: &HashSet<ClipId>, by: i64, moved: &mut Vec<TrackId>) {
            for track in tracks {
                for clip in &mut track.clips {
                    if ids.contains(&clip.id) {
                        clip.start = u32::try_from(i64::from(clip.start) + by).unwrap_or(0);
                        moved.push(track.id);
                    }
                    if let ClipContent::Group(g) = &mut clip.content {
                        shift(&mut g.tracks, ids, by, moved);
                    }
                }
            }
        }
        let by = self.shift_room(ids, by);
        if by == 0 {
            return 0;
        }
        let ids: HashSet<ClipId> = ids.iter().copied().collect();
        let mut moved = Vec::new();
        shift(&mut self.tracks, &ids, by, &mut moved);
        if moved.is_empty() {
            return 0;
        }
        moved.dedup();
        for track in moved {
            self.cover(track);
        }
        by
    }
}

/// The end of the clip at index `c` of `track` to `to`; `left`, how many frames a sound or video's
/// file has from where the clip starts in it.
fn trim_end(track: &mut Track, c: usize, to: u32, policy: EdgePolicy, fps: f64, left: Option<u32>) -> bool {
    let Some(clip) = track.clips.get(c) else { return false };
    let (start, old) = (clip.start, clip.end());
    let longest = left.map_or(u32::MAX, |l| start.saturating_add(l)).max(start + 1);
    let mut to = to.clamp(start + 1, longest);
    let next = track.clips.get(c + 1).map(|n| (n.start, start_range(n, fps)));
    if let Some((next_start, range)) = next
        && to > next_start
    {
        match policy {
            EdgePolicy::Clamp => to = next_start,
            EdgePolicy::Push => {}
            EdgePolicy::TrimNeighbour => {
                // as far into it as its start can go
                to = range.map_or(next_start, |(_, latest)| to.min(latest));
                if let Some(n) = track.clips.get_mut(c + 1) {
                    set_start(n, to, fps);
                }
            }
        }
    }
    if policy == EdgePolicy::Push {
        shift_after(track, c, old, to);
    }
    if let Some(clip) = track.clips.get_mut(c) {
        clip.duration = to - start;
    }
    to != old
}

/// The start of the clip at index `c` of `track` to `to`, its end kept.
fn trim_start(track: &mut Track, c: usize, to: u32, policy: EdgePolicy, fps: f64) -> bool {
    let Some(clip) = track.clips.get(c) else { return false };
    let old = clip.start;
    let Some((earliest, latest)) = start_range(clip, fps) else { return false };
    let mut to = to.clamp(earliest, latest.max(earliest));
    let before = c.checked_sub(1).and_then(|p| track.clips.get(p)).map(|p| (p.start, p.end()));
    match policy {
        EdgePolicy::Clamp => to = to.max(before.map_or(0, |(_, end)| end)),
        EdgePolicy::Push => {
            // the clips before it come along, as far back as the first of them has room
            let room = track.clips.first().filter(|_| c > 0).map_or(u32::MAX, |first| first.start);
            to = to.max(old.saturating_sub(room));
            for earlier in track.clips.iter_mut().take(c) {
                earlier.start = (earlier.start + to).saturating_sub(old);
            }
        }
        EdgePolicy::TrimNeighbour => {
            if let Some((start, end)) = before
                && to < end
            {
                to = to.max(start + 1);
                if let Some(p) = c.checked_sub(1).and_then(|p| track.clips.get_mut(p)) {
                    p.duration = to - p.start;
                }
            }
        }
    }
    if let Some(clip) = track.clips.get_mut(c) {
        set_start(clip, to, fps);
    }
    to != old
}

/// The earliest and latest `clip`'s start can go by what it holds, its end kept; none if its
/// start can't be dragged at all (a group that cycles: its cycle counts from its start).
fn start_range(clip: &Clip, fps: f64) -> Option<(u32, u32)> {
    let last = clip.end().saturating_sub(1);
    let into_file = |trim: f64| {
        // whole frames into its file, a hair over so a trim summed from frames counts them all
        let frames = (trim * fps + 1e-6).floor().max(0.0);
        if frames >= f64::from(clip.start) { 0 } else { clip.start - frames as u32 }
    };
    match &clip.content {
        ClipContent::Drawing(_) => Some((0, last)),
        ClipContent::Group(g) if g.cycle.is_some() => None,
        ClipContent::Group(g) => {
            // no later than its first clip, which would be cut off
            let first = g.tracks.iter().flat_map(|t| t.clips.first()).map(|c| c.start).min();
            Some((0, first.map_or(last, |f| last.min(clip.start.saturating_add(f)))))
        }
        ClipContent::Audio(a) => Some((into_file(a.trim_start), last)),
        ClipContent::Video(v) => Some((into_file(v.trim_start), last)),
    }
}

/// `clip` starting on `start` (within its [`start_range`]) and ending where it did: its keys that
/// much earlier or later, so it moves as it showed; a group's clips where they showed; a sound or
/// video from that much later or earlier in its file.
fn set_start(clip: &mut Clip, start: u32, fps: f64) {
    let end = clip.end();
    let by = i64::from(start) - i64::from(clip.start);
    if by == 0 {
        return;
    }
    clip.keys = shifted_keys(&clip.keys, by);
    match &mut clip.content {
        ClipContent::Drawing(_) => {}
        ClipContent::Group(g) => {
            for inner in g.tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
                inner.start = u32::try_from(i64::from(inner.start) - by).unwrap_or(0);
            }
        }
        ClipContent::Audio(a) => {
            a.trim_start = (a.trim_start + by as f64 / fps).max(0.0);
            let keys = a.volume_keys.keys.iter();
            a.volume_keys = Channel {
                keys: keys
                    .map(|k| Key { frame: i32::try_from(i64::from(k.frame) - by).unwrap_or(i32::MIN), ..k.clone() })
                    .collect(),
            };
        }
        ClipContent::Video(v) => v.trim_start = (v.trim_start + by as f64 / fps).max(0.0),
    }
    clip.start = start;
    clip.duration = end.saturating_sub(start).max(1);
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

    /// Three drawings with gaps: 2..5, 8..12, 14..16.
    fn three() -> Timeline {
        timeline(vec![track(1, vec![drawing(1, 2, 3), drawing(2, 8, 4), drawing(3, 14, 2)])])
    }

    fn spans(tl: &Timeline) -> Vec<(u32, u32)> {
        tl.tracks[0].clips.iter().map(|c| (c.start, c.end())).collect()
    }

    #[test]
    fn an_end_stops_at_the_next_clip() {
        let mut tl = three();
        assert!(tl.trim(ClipId(2), Edge::End, 20, EdgePolicy::Clamp));
        assert_eq!(spans(&tl), [(2, 5), (8, 14), (14, 16)]);
        // already there: nothing to change
        assert!(!tl.trim(ClipId(2), Edge::End, 30, EdgePolicy::Clamp));
        // and never under a frame long
        assert!(tl.trim(ClipId(2), Edge::End, 0, EdgePolicy::Clamp));
        assert_eq!(spans(&tl), [(2, 5), (8, 9), (14, 16)]);
    }

    #[test]
    fn an_end_pushes_the_clips_after_both_ways() {
        let mut tl = three();
        let long = tl.duration;
        assert!(tl.trim(ClipId(1), Edge::End, 9, EdgePolicy::Push));
        assert_eq!(spans(&tl), [(2, 9), (12, 16), (18, 20)]);
        assert_eq!(tl.duration, long, "pushed past the end, they stay there");
        assert!(tl.trim(ClipId(1), Edge::End, 3, EdgePolicy::Push));
        assert_eq!(spans(&tl), [(2, 3), (6, 10), (12, 14)]);
    }

    #[test]
    fn an_end_trims_into_the_next_clip_but_leaves_it_a_frame() {
        let mut tl = three();
        tl.tracks[0].clips[1].keys.opacity.set(2, 0.5, Easing::Linear);
        assert!(tl.trim(ClipId(1), Edge::End, 10, EdgePolicy::TrimNeighbour));
        assert_eq!(spans(&tl), [(2, 10), (10, 12), (14, 16)]);
        // the key it had on frame 10 is still on frame 10
        assert_eq!(tl.tracks[0].clips[1].keys.opacity.keys[0].frame, 0);
        assert!(tl.trim(ClipId(1), Edge::End, 40, EdgePolicy::TrimNeighbour));
        assert_eq!(spans(&tl), [(2, 11), (11, 12), (14, 16)]);
    }

    #[test]
    fn a_start_keeps_its_end_and_where_its_keys_show() {
        let mut tl = three();
        tl.tracks[0].clips[1].keys.position.set(1, [10.0, 0.0], Easing::Linear);
        let at_nine = tl.tracks[0].clips[1].keys.position.value_at(1.0);
        assert!(tl.trim(ClipId(2), Edge::Start, 6, EdgePolicy::Clamp));
        assert_eq!(spans(&tl), [(2, 5), (6, 12), (14, 16)]);
        // frame 9 was its second frame, and is now its fourth
        assert_eq!(tl.tracks[0].clips[1].keys.position.keys[0].frame, 3);
        assert_eq!(tl.tracks[0].clips[1].keys.position.value_at(3.0), at_nine);
        // stops at the clip before, and at its own last frame
        assert!(tl.trim(ClipId(2), Edge::Start, 0, EdgePolicy::Clamp));
        assert_eq!(spans(&tl), [(2, 5), (5, 12), (14, 16)]);
        assert!(tl.trim(ClipId(2), Edge::Start, 30, EdgePolicy::Clamp));
        assert_eq!(spans(&tl), [(2, 5), (11, 12), (14, 16)]);
    }

    #[test]
    fn a_start_pushes_the_clips_before_as_far_as_they_have_room() {
        let mut tl = three();
        // the first clip has two frames of room before it
        assert!(tl.trim(ClipId(3), Edge::Start, 3, EdgePolicy::Push));
        assert_eq!(spans(&tl), [(0, 3), (6, 10), (12, 16)]);
        // and back the other way they follow
        assert!(tl.trim(ClipId(3), Edge::Start, 15, EdgePolicy::Push));
        assert_eq!(spans(&tl), [(3, 6), (9, 13), (15, 16)]);
        // the first clip's own start has nothing to push
        assert!(tl.trim(ClipId(1), Edge::Start, 0, EdgePolicy::Push));
        assert_eq!(spans(&tl), [(0, 6), (9, 13), (15, 16)]);
    }

    #[test]
    fn a_start_trims_into_the_clip_before() {
        let mut tl = three();
        assert!(tl.trim(ClipId(2), Edge::Start, 3, EdgePolicy::TrimNeighbour));
        assert_eq!(spans(&tl), [(2, 3), (3, 12), (14, 16)]);
        assert!(!tl.trim(ClipId(2), Edge::Start, 0, EdgePolicy::TrimNeighbour));
    }

    #[test]
    fn a_group_keeps_its_clips_where_they_showed() {
        let inner = track(2, vec![drawing(10, 2, 3)]);
        let group = Clip {
            content: ClipContent::Group(Group { tracks: vec![inner], collapsed: false, cycle: None }),
            ..drawing(5, 10, 8)
        };
        let mut tl = timeline(vec![track(1, vec![group])]);
        let showed = |tl: &Timeline| tl.origin(ClipId(10));
        assert_eq!(showed(&tl), Some(12));
        // earlier: its clip counts from further in
        assert!(tl.trim(ClipId(5), Edge::Start, 6, EdgePolicy::Clamp));
        assert_eq!(spans(&tl), [(6, 18)]);
        assert_eq!(showed(&tl), Some(12));
        // later: no further than its first clip
        assert!(tl.trim(ClipId(5), Edge::Start, 16, EdgePolicy::Clamp));
        assert_eq!(spans(&tl), [(12, 18)]);
        assert_eq!(showed(&tl), Some(12));
    }

    #[test]
    fn a_cycling_group_keeps_its_start_and_is_a_wall_to_a_trim() {
        let inner = track(2, vec![drawing(10, 0, 2)]);
        let cycle = Some(Cycle { length: 2, keys: CycleKeys::Repeat });
        let group = Clip {
            content: ClipContent::Group(Group { tracks: vec![inner], collapsed: false, cycle }),
            ..drawing(5, 10, 8)
        };
        let mut tl = timeline(vec![track(1, vec![drawing(1, 0, 4), group])]);
        assert!(!tl.trim(ClipId(5), Edge::Start, 6, EdgePolicy::Clamp));
        assert!(tl.trim(ClipId(1), Edge::End, 14, EdgePolicy::TrimNeighbour));
        assert_eq!(spans(&tl), [(0, 10), (10, 18)]);
    }

    #[test]
    fn a_sound_starts_later_in_its_file_and_no_earlier_than_its_start() {
        let mut tl = Timeline::new(12, 48);
        tl.add_media(Media {
            id: MediaId(7),
            name: "s.wav".to_owned(),
            kind: MediaKind::Audio,
            seconds: 2.0,
            extension: "wav".to_owned(),
            size: [0, 0],
        });
        let (_, clip) = tl.add_sound(MediaId(7), 6).unwrap();
        let trim = |tl: &Timeline| match &tl.clip(clip).unwrap().content {
            ClipContent::Audio(a) => a.trim_start,
            _ => f64::NAN,
        };
        assert_eq!(tl.clip(clip).map(|c| (c.start, c.end())), Some((6, 30)));
        assert!(tl.trim(clip, Edge::Start, 12, EdgePolicy::Clamp));
        assert_eq!(tl.clip(clip).map(|c| (c.start, c.end())), Some((12, 30)));
        assert!((trim(&tl) - 0.5).abs() < 1e-9);
        // back out no further than where its file starts
        assert!(tl.trim(clip, Edge::Start, 0, EdgePolicy::Clamp));
        assert_eq!(tl.clip(clip).map(|c| (c.start, c.end())), Some((6, 30)));
        assert!(trim(&tl).abs() < 1e-9);
        // and its end no later than its file lasts
        assert!(!tl.trim(clip, Edge::End, 60, EdgePolicy::Push));
    }

    #[test]
    fn clips_move_together_until_one_meets_a_clip_left_behind() {
        let mut tl = timeline(vec![
            track(1, vec![drawing(1, 2, 3), drawing(2, 8, 4), drawing(3, 14, 2)]),
            track(2, vec![drawing(4, 0, 2), drawing(5, 3, 2), drawing(6, 20, 1)]),
        ]);
        let both = |tl: &Timeline| -> Vec<Vec<(u32, u32)>> {
            tl.tracks.iter().map(|t| t.clips.iter().map(|c| (c.start, c.end())).collect()).collect()
        };
        // later: clip 2 has two frames before clip 3, clip 5 fifteen before clip 6
        assert_eq!(tl.shift_room(&[ClipId(2), ClipId(5)], 9), 2);
        assert_eq!(tl.shift_clips(&[ClipId(2), ClipId(5)], 9), 2);
        assert_eq!(both(&tl), [vec![(2, 5), (10, 14), (14, 16)], vec![(0, 2), (5, 7), (20, 21)]]);
        // neighbours picked together keep what is between them, and go as one
        assert_eq!(tl.shift_clips(&[ClipId(2), ClipId(3)], 5), 5);
        assert_eq!(both(&tl), [vec![(2, 5), (15, 19), (19, 21)], vec![(0, 2), (5, 7), (20, 21)]]);
        assert_eq!(tl.duration, 21);
        // earlier: clip 4 is on frame 0 already, so nothing picked with it moves
        assert_eq!(tl.shift_clips(&[ClipId(4), ClipId(2)], -3), 0);
        assert_eq!(tl.shift_clips(&[ClipId(5), ClipId(2)], -30), -3);
        assert_eq!(both(&tl), [vec![(2, 5), (12, 16), (19, 21)], vec![(0, 2), (2, 4), (20, 21)]]);
        // a clip that isn't there is left out
        assert_eq!(tl.shift_clips(&[ClipId(99)], 4), 0);
    }

    #[test]
    fn a_clip_in_a_group_grows_the_group() {
        let inner = track(2, vec![drawing(10, 0, 3)]);
        let group = Clip {
            content: ClipContent::Group(Group { tracks: vec![inner], collapsed: false, cycle: None }),
            ..drawing(5, 4, 3)
        };
        let mut tl = timeline(vec![track(1, vec![group])]);
        assert!(tl.trim(ClipId(10), Edge::End, 9, EdgePolicy::Clamp));
        assert_eq!(spans(&tl), [(4, 13)]);
        assert_eq!(tl.duration, 13);
    }
    #[test]
    fn room_is_made_before_a_clip_by_closing_the_gaps_nearest_it_first() {
        let spans = |tl: &Timeline| tl.tracks[0].clips.iter().map(|c| (c.start, c.duration)).collect::<Vec<_>>();
        let made = || timeline(vec![track(1, vec![drawing(1, 1, 2), drawing(2, 5, 1), drawing(3, 8, 2), drawing(4, 10, 1)])]);
        // a frame of room wanted before the clip at 8: the gap just before it has two
        let mut tl = made();
        assert_eq!(tl.close_up_before(ClipId(3), 7), Some(7));
        assert_eq!(spans(&tl), [(1, 2), (5, 1), (8, 2), (10, 1)]);
        // four wanted: the clip before it goes back past its own gap, the first stays
        let mut tl = made();
        assert_eq!(tl.close_up_before(ClipId(3), 4), Some(4));
        assert_eq!(spans(&tl), [(1, 2), (3, 1), (8, 2), (10, 1)]);
        // more than there is: the clips before it packed from the first frame, and no further
        let mut tl = made();
        assert_eq!(tl.close_up_before(ClipId(3), 0), Some(3));
        assert_eq!(spans(&tl), [(0, 2), (2, 1), (8, 2), (10, 1)]);
        assert_eq!(tl.close_up_before(ClipId(3), 0), Some(3), "nothing more to close up");
        // asked for no earlier than it starts, nothing moves; a clip that isn't there
        let mut tl = made();
        assert_eq!(tl.close_up_before(ClipId(3), 9), Some(8));
        assert_eq!(spans(&tl), [(1, 2), (5, 1), (8, 2), (10, 1)]);
        assert_eq!(tl.close_up_before(ClipId(99), 0), None);
    }

    #[test]
    fn room_is_made_after_a_clip_by_taking_up_the_gaps_nearest_it_first() {
        let spans = |tl: &Timeline| tl.tracks[0].clips.iter().map(|c| (c.start, c.duration)).collect::<Vec<_>>();
        let made = || timeline(vec![track(1, vec![drawing(1, 0, 2), drawing(2, 4, 1), drawing(3, 5, 2), drawing(4, 10, 1)])]);
        // two frames more: the gap after it has them, and nothing moves
        let mut tl = made();
        assert_eq!(tl.open_up_after(ClipId(1), 4), Some(4));
        assert_eq!(spans(&tl), [(0, 2), (4, 1), (5, 2), (10, 1)]);
        // four more: the two clips it reaches go two later, into the gap after them; the last stays
        let mut tl = made();
        assert_eq!(tl.open_up_after(ClipId(1), 6), Some(6));
        assert_eq!(spans(&tl), [(0, 2), (6, 1), (7, 2), (10, 1)]);
        // more than every gap: all of them pushed, each just past the one before
        let mut tl = made();
        assert_eq!(tl.open_up_after(ClipId(1), 9), Some(9));
        assert_eq!(spans(&tl), [(0, 2), (9, 1), (10, 2), (12, 1)]);
        // asked for no later than it ends, nothing moves
        let mut tl = made();
        assert_eq!(tl.open_up_after(ClipId(1), 1), Some(2));
        assert_eq!(spans(&tl), [(0, 2), (4, 1), (5, 2), (10, 1)]);
        assert_eq!(tl.open_up_after(ClipId(99), 5), None);
    }

    #[test]
    fn a_run_of_empty_frames_is_made_longer_or_shorter_or_closed() {
        let spans = |tl: &Timeline| tl.tracks[0].clips.iter().map(|c| (c.start, c.duration)).collect::<Vec<_>>();
        let mut tl = timeline(vec![track(1, vec![drawing(1, 2, 1), drawing(2, 6, 2), drawing(3, 9, 1)])]);
        // the frames before the first clip, between two, and after the last
        assert_eq!(tl.gap_at(TrackId(1), 0), Some((0, Some(2))));
        assert_eq!(tl.gap_at(TrackId(1), 4), Some((3, Some(3))));
        assert_eq!(tl.gap_at(TrackId(1), 8), Some((8, Some(1))));
        assert_eq!(tl.gap_at(TrackId(1), 30), Some((10, None)), "no clip after it: it has no end");
        assert_eq!(tl.gap_at(TrackId(1), 6), None, "a clip is there");
        // one frame shorter: every clip after it a frame earlier, those before it as they were
        assert!(tl.set_gap(TrackId(1), 4, 2));
        assert_eq!(spans(&tl), [(2, 1), (5, 2), (8, 1)]);
        assert!(!tl.set_gap(TrackId(1), 4, 2), "already");
        // longer
        assert!(tl.set_gap(TrackId(1), 3, 5));
        assert_eq!(spans(&tl), [(2, 1), (8, 2), (11, 1)]);
        // closed
        assert!(tl.set_gap(TrackId(1), 5, 0));
        assert_eq!(spans(&tl), [(2, 1), (3, 2), (6, 1)]);
        assert!(tl.set_gap(TrackId(1), 0, 0));
        assert_eq!(spans(&tl), [(0, 1), (1, 2), (4, 1)]);
        // nothing after it to move, a frame with a clip on it, a track that isn't there
        assert!(!tl.set_gap(TrackId(1), 20, 3));
        assert!(!tl.set_gap(TrackId(1), 1, 3));
        assert!(!tl.set_gap(TrackId(9), 0, 3));
    }
}

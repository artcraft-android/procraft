//! The timeline's length: set to a frame count (shorter than its clips if wanted), fitted to them,
//! and frames added before its first frame or after its last.

use super::*;

impl Timeline {
    /// One past the last frame any clip shows on (0 with no clips).
    pub fn content_end(&self) -> u32 {
        self.tracks.iter().flat_map(|t| t.clips.last()).map(Clip::end).max().unwrap_or(0)
    }

    /// `frames` long, at least a frame. It may end before its clips: what is
    /// past the end stays where it is but neither plays, exports nor is drawn on (see
    /// [`Timeline::trim`] and the moves, which leave the length as it is). False if it is that
    /// long already.
    pub fn set_length(&mut self, frames: u32) -> bool {
        let frames = frames.max(1);
        if frames == self.duration {
            return false;
        }
        self.duration = frames;
        true
    }

    /// As long as its clips, at least a frame. False if it is already.
    pub fn fit_to_content(&mut self) -> bool {
        self.set_length(self.content_end())
    }

    /// `count` frames added before the first (`at_start`) or after the last. Before the first,
    /// everything moves along by `count`: every clip on the timeline's own tracks (their keys,
    /// a group's own clips and a sound's place in its file going with them) and the camera's
    /// keys. False for none.
    pub fn add_frames(&mut self, at_start: bool, count: u32) -> bool {
        if count == 0 {
            return false;
        }
        self.duration = self.duration.saturating_add(count);
        if !at_start {
            return true;
        }
        for clip in self.tracks.iter_mut().flat_map(|t| &mut t.clips) {
            clip.start = clip.start.saturating_add(count);
        }
        let by = i32::try_from(count).unwrap_or(i32::MAX);
        for key in &mut self.camera.keys {
            key.frame = key.frame.saturating_add(by);
        }
        true
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

    /// Two tracks: drawings at 2..5 and 8..12 on one, 0..6 on the other; 20 frames long.
    fn two() -> Timeline {
        let mut tl = Timeline::new(12, 20);
        tl.tracks = vec![track(1, vec![drawing(1, 2, 3), drawing(2, 8, 4)]), track(2, vec![drawing(3, 0, 6)])];
        tl.next_track = 50;
        tl.next_clip = 500;
        tl
    }

    #[test]
    fn the_length_may_end_before_the_clips() {
        let mut tl = two();
        assert_eq!(tl.content_end(), 12);
        assert!(tl.set_length(30));
        assert_eq!(tl.duration, 30);
        assert!(!tl.set_length(30));
        assert!(tl.set_length(0));
        assert_eq!(tl.duration, 1, "a frame at least");
        assert!(tl.set_length(4));
        assert_eq!(tl.duration, 4, "shorter than the clips, as it may be");
        assert_eq!(tl.content_end(), 12, "the clips past the end are kept");
        assert_eq!(tl.tracks[0].clips[1].start, 8);
        assert!(tl.fit_to_content());
        assert_eq!(tl.duration, 12, "fitted, it reaches the clips again");
    }

    #[test]
    fn a_short_timeline_plays_only_to_its_end_and_moves_leave_it_short() {
        let mut tl = two();
        assert!(tl.set_length(4));
        tl.playback = Playback::Loop;
        assert_eq!(tl.next_played(2, 1), Some((3, 1)));
        assert_eq!(tl.next_played(3, 1), Some((0, 1)), "round from its last frame, not the clips'");
        assert_eq!(tl.next_played(9, 1), Some((0, 1)), "from past the end, back to the start");
        tl.playback = Playback::OneShot;
        assert_eq!(tl.next_played(3, 1), None);
        // a clip parked past the end, and trimmed there: the end stays where it was put
        assert!(tl.move_clip(ClipId(1), TrackId(1), 14));
        assert!(tl.trim(ClipId(1), Edge::End, 20, EdgePolicy::Clamp));
        assert_eq!(tl.duration, 4);
        assert_eq!(tl.content_end(), 20);
    }

    #[test]
    fn fitting_to_the_content() {
        let mut tl = two();
        assert!(tl.fit_to_content());
        assert_eq!(tl.duration, 12);
        assert!(!tl.fit_to_content());
        let mut empty = Timeline::new(12, 40);
        assert!(empty.fit_to_content());
        assert_eq!(empty.duration, 1);
    }

    #[test]
    fn frames_added_at_the_end_move_nothing() {
        let mut tl = two();
        assert!(tl.add_frames(false, 6));
        assert_eq!(tl.duration, 26);
        assert_eq!(tl.tracks[0].clips[0].start, 2);
        assert!(!tl.add_frames(false, 0));
    }

    #[test]
    fn frames_added_at_the_start_move_everything_along() {
        let mut tl = two();
        tl.tracks[0].clips[0].keys.opacity.keys.push(Key { frame: 1, value: 0.5, easing: Easing::Linear, path: None });
        tl.camera.keys.push(Key {
            frame: 9,
            value: CameraPose::still(100.0, 100.0),
            easing: Easing::Linear,
            path: None,
        });
        let before = tl.plan(9);
        assert!(tl.add_frames(true, 4));
        assert_eq!(tl.duration, 24);
        let starts: Vec<u32> = tl.tracks.iter().flat_map(|t| &t.clips).map(|c| c.start).collect();
        assert_eq!(starts, [6, 12, 4]);
        assert_eq!(tl.tracks[0].clips[0].keys.opacity.keys[0].frame, 1, "a clip's keys are its own frames");
        assert_eq!(tl.camera.keys[0].frame, 13);
        assert_eq!(tl.plan(13), before, "what showed on frame 9 shows on frame 13");
    }

    #[test]
    fn a_groups_own_clips_stay_in_it() {
        let mut tl = two();
        let ids = [ClipId(1), ClipId(2)];
        let group = tl.group(&ids, "G").unwrap();
        let inner: Vec<u32> = match &tl.clip(group).unwrap().content {
            ClipContent::Group(g) => g.tracks.iter().flat_map(|t| &t.clips).map(|c| c.start).collect(),
            _ => panic!("a group"),
        };
        let start = tl.clip(group).unwrap().start;
        assert!(tl.add_frames(true, 3));
        assert_eq!(tl.clip(group).unwrap().start, start + 3);
        let now: Vec<u32> = match &tl.clip(group).unwrap().content {
            ClipContent::Group(g) => g.tracks.iter().flat_map(|t| &t.clips).map(|c| c.start).collect(),
            _ => panic!("a group"),
        };
        assert_eq!(now, inner);
    }
}

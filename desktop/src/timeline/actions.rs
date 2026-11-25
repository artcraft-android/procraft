//! What the grid's menus, handles and drags do to the timeline, as the iPad's `TimelineTool`
//! does it (ios/ProCraft/TimelineGrid.swift, TimelineGridMove.swift, TimelineGridAdd.swift,
//! TimelineMenus.swift, TimelinePicked.swift, TimelineGroupRows.swift): each one core call, the
//! grid reading the timeline again after it, and what to tell when it can't be done.

use std::collections::HashMap;

use procraft_core::{CanvasSession, ThumbnailImage, TimelineClipInfo, TimelineMask, TimelineTrackInfo};

use super::TimelineGrid;

impl TimelineGrid {
    /// Something to tell (a warning, or not), shown by the app once the grid has been drawn.
    pub(super) fn tell(&mut self, text: impl Into<String>, warning: bool) {
        self.said = Some((text.into(), warning));
    }

    /// The timeline changed: read again on the next frame.
    pub(super) fn changed(&mut self) {
        self.read_at = None;
    }

    /// Track `t` or a track above it holding a group it is inside is locked: its clips stay as
    /// they are.
    pub(super) fn held(&self, t: &TimelineTrackInfo) -> bool {
        t.locked || self.rows.segment_of(t.id).is_some_and(|(_, s)| s.row.locked_above)
    }

    /// Clip `id` and the track (as the rows show it) it is on.
    pub(super) fn clip(&self, id: u64) -> Option<(TimelineTrackInfo, TimelineClipInfo)> {
        self.rows
            .tracks()
            .find_map(|(_, s)| s.track().clips.iter().find(|c| c.id == id).map(|c| (s.track().clone(), c.clone())))
    }

    /// The track (as the rows show it) with id `id`.
    pub(super) fn track(&self, id: u64) -> Option<TimelineTrackInfo> {
        self.rows.segment_of(id).map(|(_, s)| s.track().clone())
    }

    /// The clipboard holds frames.
    pub(super) fn can_paste(&self) -> bool {
        self.panel.as_ref().is_some_and(|p| p.clipboard.clips > 0)
    }

    /// The frames picked together, as a list.
    pub(super) fn chosen(&self) -> Vec<u64> {
        self.selecting.as_ref().map(|s| s.iter().copied().collect()).unwrap_or_default()
    }

    // MARK: Lengthening

    /// Its track's clips as clip `id`'s start (`leading`) or end dragged to `frame` would leave
    /// them; empty if it would change nothing.
    pub(super) fn trim_preview(
        &self,
        session: &CanvasSession,
        id: u64,
        leading: bool,
        frame: i64,
    ) -> Vec<TimelineClipInfo> {
        session.timeline_lengthen_preview(id, leading, i32::try_from(frame).unwrap_or(i32::MAX))
    }

    /// That edge dragged there: one undo step.
    pub(super) fn trim(&mut self, session: &CanvasSession, id: u64, leading: bool, frame: i64) {
        self.stop(session, false);
        session.lengthen_timeline_clip(id, leading, i32::try_from(frame).unwrap_or(i32::MAX));
        self.changed();
    }

    /// The tracks every picked frame's same edge dragged `by` frames would change, with their
    /// clips then.
    pub(super) fn trims_preview(
        &self,
        session: &CanvasSession,
        leading: bool,
        by: i32,
    ) -> HashMap<u64, Vec<TimelineClipInfo>> {
        let mut out = HashMap::new();
        for t in session.timeline_lengthens_preview(self.chosen(), leading, by) {
            out.entry(t.track).or_insert(t.clips);
        }
        out
    }

    /// Every picked frame's same edge dragged `by` frames: one undo step.
    pub(super) fn trim_selected(&mut self, session: &CanvasSession, leading: bool, by: i32) {
        if by == 0 {
            return;
        }
        self.stop(session, false);
        session.lengthen_timeline_clips(self.chosen(), leading, by);
        self.changed();
    }

    // MARK: Frames

    /// The drawings of frames `ids` made blank, the frames staying where they are: one undo step.
    pub(super) fn clear_frames(&mut self, session: &CanvasSession, ids: Vec<u64>) {
        if ids.is_empty() {
            return;
        }
        self.stop(session, false);
        if !session.clear_timeline_clips(ids) {
            self.tell("Only drawings on tracks that aren't locked can be cleared.", true);
        }
        self.changed();
    }

    /// Frames `ids` put in the opposite order in time: one undo step.
    pub(super) fn flip_frames(&mut self, session: &CanvasSession, ids: Vec<u64>) {
        if ids.len() < 2 {
            return;
        }
        self.stop(session, false);
        if !session.reverse_timeline_clips(ids) {
            self.tell("They can't be flipped: a frame between them isn't picked, or a track is locked.", true);
        }
        self.changed();
    }

    /// Frames `ids` copied with their keys, to paste at the playhead; what is picked stays picked.
    pub(super) fn copy_frames(&mut self, session: &CanvasSession, ids: Vec<u64>) {
        if ids.is_empty() {
            return;
        }
        let n = ids.len();
        if session.copy_timeline_clips(ids) {
            if n == 1 {
                self.tell("Copied. Paste puts it at the playhead on the chosen track.", false);
            } else {
                self.tell(
                    format!("{n} frames copied. Paste puts them at the playhead, from the chosen track up."),
                    false,
                );
            }
        }
        self.changed();
    }

    /// Frames `ids` copied, then taken off with their drawings: one undo step.
    pub(super) fn cut_frames(&mut self, session: &CanvasSession, ids: Vec<u64>) {
        if ids.is_empty() {
            return;
        }
        self.stop(session, false);
        self.picked = None;
        if !session.cut_timeline_clips(ids) {
            self.tell("One of them is on a locked track.", true);
        }
        self.changed();
    }

    /// Frames `ids` taken off with their drawings: one undo step.
    pub(super) fn delete_frames(&mut self, session: &CanvasSession, ids: Vec<u64>) {
        if ids.is_empty() {
            return;
        }
        self.stop(session, false);
        self.picked = None;
        if !session.delete_timeline_clips(ids) {
            self.tell("One of them is on a locked track.", true);
        }
        self.changed();
    }

    /// Copies of frames `ids` just after the last of them: one undo step. Returns the copies.
    pub(super) fn duplicate_frames(&mut self, session: &CanvasSession, ids: Vec<u64>) -> Vec<u64> {
        if ids.is_empty() {
            return Vec::new();
        }
        self.stop(session, false);
        let made = session.duplicate_timeline_clips(ids);
        if made.is_empty() {
            self.tell("Only frames on tracks that aren't locked, with room for their layers, can be duplicated.", true);
        }
        self.changed();
        made
    }

    /// A new blank frame on `frame` of track `track` (just after a frame there), the frames after
    /// it moving along; the playhead goes onto it.
    pub(super) fn insert_frame(&mut self, session: &CanvasSession, frame: u32, track: u64) {
        self.select_track(session, track);
        self.go_to(session, frame);
        if let Some(said) = self.add_frame(session) {
            self.tell(said, true);
        }
        self.changed();
    }

    /// The clips copied, at the playhead on the track drawn on (those above on the tracks above);
    /// the first pasted is picked.
    pub(super) fn paste_clips(&mut self, session: &CanvasSession) {
        self.stop(session, false);
        let made = session.paste_timeline_clips();
        if made.is_empty() {
            let locked = self.info().and_then(|i| i.track).and_then(|t| self.track(t)).is_some_and(|t| t.locked);
            self.tell(if locked { "The track is locked." } else { "There's no room for more layers." }, true);
        } else if self.selecting.is_none() {
            self.picked = made.first().copied();
        }
        self.changed();
    }

    /// What was copied put on `frame` of track `track`.
    pub(super) fn paste_at(&mut self, session: &CanvasSession, frame: u32, track: u64) {
        self.select_track(session, track);
        self.go_to(session, frame);
        self.paste_clips(session);
    }

    /// Whether any of clips `ids` has empty frames after it to last through (and none is on a
    /// locked track).
    pub(super) fn can_extend(&self, session: &CanvasSession, ids: &[u64]) -> bool {
        !ids.is_empty() && session.timeline_clips_extend(ids.to_vec())
    }

    /// Clips `ids` each made to last through the empty frames after it: one undo step.
    pub(super) fn extend(&mut self, session: &CanvasSession, ids: Vec<u64>) {
        if ids.is_empty() {
            return;
        }
        self.stop(session, false);
        let one = ids.len() == 1;
        if !session.extend_timeline_clips(ids) {
            self.tell(
                if one {
                    "There's no empty space after this clip to fill, or its track is locked."
                } else {
                    "There's no empty space after these clips to fill, or one is on a locked track."
                },
                true,
            );
        }
        self.changed();
    }

    /// Whether every one of clips `ids` is turned off.
    pub(super) fn all_disabled(&self, ids: &[u64]) -> bool {
        !ids.is_empty() && ids.iter().all(|id| self.clip(*id).is_some_and(|(_, c)| c.disabled))
    }

    /// Clips `ids` all turned off, or all on again: one undo step.
    pub(super) fn set_clips_disabled(&mut self, session: &CanvasSession, ids: Vec<u64>, disabled: bool) {
        if !session.set_timeline_clips_disabled(ids, disabled) {
            self.tell("They are on locked tracks.", true);
        }
        self.changed();
    }

    pub(super) fn set_clip_disabled(&mut self, session: &CanvasSession, id: u64, disabled: bool) {
        session.set_timeline_clip_disabled(id, disabled);
        self.changed();
    }

    /// The playhead brought onto block `clip` if it isn't on one of its frames (its first, or its
    /// last when the playhead is past it): keys are set on the playhead's frame.
    pub(super) fn bring_playhead(&mut self, session: &CanvasSession, clip: &TimelineClipInfo) {
        let frame = self.frame();
        if clip.duration == 0 || (frame >= clip.start && frame < clip.start + clip.duration) {
            return;
        }
        self.go_to(session, if frame < clip.start { clip.start } else { clip.start + clip.duration - 1 });
        self.changed();
    }

    // MARK: Empty frames

    /// The run of empty frames `frame` is in on track `track`: its first frame, how many frames
    /// it is, and whether no frame comes after it (then it has no length to change); none where a
    /// frame is there.
    pub(super) fn gap(&self, session: &CanvasSession, track: u64, frame: u32) -> Option<(u32, u32, bool)> {
        session.timeline_gap(track, frame).map(|g| (g.start, g.length, g.open))
    }

    /// That run made `length` frames long (0: closed), the frames after it moving to suit: one
    /// undo step.
    pub(super) fn set_gap(&mut self, session: &CanvasSession, track: u64, frame: u32, length: u32) {
        self.stop(session, false);
        session.set_timeline_gap(track, frame, length);
        self.changed();
    }

    // MARK: Tracks

    pub(super) fn set_track_label(&mut self, session: &CanvasSession, id: u64, label: Option<u32>) {
        session.set_timeline_track_label(id, label);
        self.changed();
    }

    pub(super) fn set_track_blend(&mut self, session: &CanvasSession, id: u64, blend: u32) {
        if !session.set_timeline_track_blend(id, blend) && self.track(id).is_some_and(|t| t.locked) {
            self.tell("The track is locked.", true);
        }
        self.changed();
    }

    pub(super) fn set_track_mask(&mut self, session: &CanvasSession, id: u64, mask: TimelineMask) {
        if !session.set_timeline_track_mask(id, mask) && self.track(id).is_some_and(|t| t.locked) {
            self.tell("The track is locked.", true);
        }
        self.changed();
    }

    /// Track `id`'s opacity: shown at once, one undo step when `done`.
    pub(super) fn set_track_opacity(&mut self, session: &CanvasSession, id: u64, opacity: f32, done: bool) {
        session.set_timeline_track_opacity(id, opacity, done);
        self.changed();
    }

    pub(super) fn rename_track(&mut self, session: &CanvasSession, id: u64, name: String) {
        session.rename_timeline_track(id, name);
        self.changed();
    }

    pub(super) fn toggle_locked(&mut self, session: &CanvasSession, t: &TimelineTrackInfo) {
        session.set_timeline_track(t.id, t.hidden, !t.locked);
        self.changed();
    }

    pub(super) fn delete_track(&mut self, session: &CanvasSession, id: u64) {
        self.stop(session, false);
        self.picked = None;
        session.delete_timeline_track(id);
        self.changed();
    }

    // MARK: Blocks

    /// Clip `id`'s own opacity: shown at once, one undo step when `done`.
    pub(super) fn set_clip_opacity(&mut self, session: &CanvasSession, id: u64, opacity: f32, done: bool) {
        session.set_timeline_clip_opacity(id, opacity, done);
        self.changed();
    }

    pub(super) fn set_clip_blend(&mut self, session: &CanvasSession, id: u64, blend: u32) {
        session.set_timeline_clip_blend(id, blend);
        self.changed();
    }

    pub(super) fn set_clip_label(&mut self, session: &CanvasSession, id: u64, label: Option<u32>) {
        session.set_timeline_clip_label(id, label);
        self.changed();
    }

    pub(super) fn set_clip_mask(&mut self, session: &CanvasSession, id: u64, mask: TimelineMask) {
        session.set_timeline_clip_mask(id, mask);
        self.changed();
    }

    pub(super) fn rename_clip(&mut self, session: &CanvasSession, id: u64, name: String) {
        session.rename_timeline_clip(id, name);
        self.changed();
    }

    /// Clip `id` cut in two at the playhead.
    pub(super) fn split(&mut self, session: &CanvasSession, id: u64) {
        self.stop(session, false);
        if !session.split_timeline_clip(id) {
            self.tell("Move the playhead inside the clip to split it there.", true);
        }
        self.changed();
    }

    /// Clip `id` taken out of its group, onto the level the group is on.
    pub(super) fn move_out_of_group(&mut self, session: &CanvasSession, id: u64) {
        self.stop(session, false);
        if !session.move_timeline_clip_out_of_group(id) {
            self.tell("It can't be moved out: its track is locked.", true);
        }
        self.changed();
    }

    // MARK: Groups

    /// The frames picked together made into a group, which is picked; the lasso goes off.
    pub(super) fn make_group(&mut self, session: &CanvasSession) {
        let ids = self.chosen();
        self.selecting = None;
        if ids.is_empty() {
            return;
        }
        self.stop(session, false);
        let made = session.group_timeline_clips(ids);
        if made == 0 {
            self.tell("Only clips shown together can be grouped.", true);
        } else {
            self.picked = Some(made);
        }
        self.changed();
    }

    pub(super) fn ungroup(&mut self, session: &CanvasSession, id: u64) {
        self.picked = None;
        session.ungroup_timeline_clip(id);
        self.changed();
    }

    /// Group clip `id` folded, or its tracks unfolded under its own.
    pub(super) fn fold(&mut self, session: &CanvasSession, id: u64, folded: bool) {
        session.set_timeline_group_folded(id, folded);
        self.changed();
    }

    /// Group `id` plays its first `length` frames over and over (0: once).
    pub(super) fn set_cycle(&mut self, session: &CanvasSession, id: u64, length: u32, continue_across: bool) {
        session.set_timeline_cycle(id, length, continue_across);
        self.changed();
    }

    /// How many times cycling group `id` plays its cycle, and the most it has room for.
    pub(super) fn cycles(&self, session: &CanvasSession, id: u64) -> Option<(u32, u32)> {
        match session.timeline_group_cycles(id)?.as_slice() {
            [count, most] => Some((*count, *most)),
            _ => None,
        }
    }

    /// Cycling group `id` made `count` cycles long: one undo step.
    pub(super) fn set_cycles(&mut self, session: &CanvasSession, id: u64, count: u32) {
        self.stop(session, false);
        session.set_timeline_group_cycles(id, count.max(1));
        self.changed();
    }

    /// Cycling group `id` made `frames` long, its last cycle cut short where that falls inside
    /// one: one undo step.
    pub(super) fn set_group_length(&mut self, session: &CanvasSession, id: u64, frames: u32) {
        self.stop(session, false);
        session.set_timeline_group_length(id, frames.max(1));
        self.changed();
    }

    // MARK: Moving

    /// Block `id` dropped on `track` starting on `start`, what it covers pushed aside: one undo
    /// step.
    pub(super) fn drop_clip(&mut self, session: &CanvasSession, id: u64, track: u64, start: i64, later: bool) {
        self.stop(session, false);
        session.drop_timeline_clip(id, track, u32::try_from(start.max(0)).unwrap_or(0), later);
        self.changed();
    }

    /// Blocks `ids` dropped `by` frames along, on the tracks as many rows from their own as
    /// `track` is from `primary`'s: one undo step.
    pub(super) fn drop_clips(
        &mut self,
        session: &CanvasSession,
        ids: Vec<u64>,
        primary: u64,
        track: u64,
        by: i64,
        later: bool,
    ) {
        self.stop(session, false);
        session.drop_timeline_clips(ids, primary, track, i32::try_from(by).unwrap_or(0), later);
        self.changed();
    }

    /// Block `id` dropped on a new track of its own just above (or `below`) track `beside`: one
    /// undo step.
    pub(super) fn drop_on_new_track(&mut self, session: &CanvasSession, id: u64, beside: u64, below: bool, start: i64) {
        self.stop(session, false);
        if session.drop_timeline_clip_on_new_track(id, beside, below, u32::try_from(start.max(0)).unwrap_or(0)) == 0 {
            self.tell("It can't go there.", true);
        }
        self.changed();
    }

    // MARK: The timeline's length

    pub(super) fn set_length(&mut self, session: &CanvasSession, frames: u32) {
        self.stop(session, false);
        session.set_timeline_length(frames);
        self.changed();
    }

    pub(super) fn fit_to_content(&mut self, session: &CanvasSession) {
        self.stop(session, false);
        session.fit_timeline_to_content();
        self.changed();
    }

    pub(super) fn add_frames(&mut self, session: &CanvasSession, at_start: bool, count: u32) {
        self.stop(session, false);
        session.add_timeline_frames(at_start, count);
        self.changed();
    }

    // MARK: Add new

    /// A picture the user picks as a new frame at the playhead on the chosen track (where a frame
    /// is already, just after it), fitted to the canvas: one undo step.
    pub(super) fn add_picture(&mut self, session: &CanvasSession) {
        const PICTURES: [&str; 8] = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"];
        let Some(path) =
            crate::files::dialog().set_title("Add a picture").add_filter("Pictures", &PICTURES).pick_file()
        else {
            return;
        };
        let image = std::fs::read(&path).ok().and_then(|bytes| image::load_from_memory(&bytes).ok());
        let Some(image) = image else {
            self.tell("This picture can't be opened.", true);
            return;
        };
        let rgba = image.to_rgba8();
        self.add_picture_frame(
            session,
            ThumbnailImage { width: rgba.width(), height: rgba.height(), rgba: rgba.into_raw() },
        );
    }

    /// `image` as a new frame at the playhead: one undo step.
    pub(super) fn add_picture_frame(&mut self, session: &CanvasSession, image: ThumbnailImage) {
        self.stop(session, false);
        if session.insert_image_frame(image).is_none() {
            let locked = self.info().and_then(|i| i.track).and_then(|t| self.track(t)).is_some_and(|t| t.locked);
            self.tell(if locked { "The track is locked." } else { "There's no room for another layer." }, true);
        }
        self.changed();
    }
}

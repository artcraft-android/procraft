//! What the timeline panel's clip and track pages change: a clip's opacity, blend mode, colour
//! label, name and whether it shows; a track's opacity and place among the others, and showing it
//! alone; the part of the timeline played over and over; the onion skins either side.

use procraft_engine::document::BlendMode;
use procraft_engine::timeline::{ClipId, Timeline, TrackId};

use crate::CanvasSession;
use crate::animation::PlayStep;

/// The frames playing goes round: from `start` up to (not including) `end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimelineLoop {
    pub start: u32,
    pub end: u32,
}

/// The onion skins either side of the drawing drawn on: how many, and the colour each side is
/// screened with (0xRRGGBB; black leaves them as they are).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimelineOnion {
    pub before: u32,
    pub after: u32,
    pub before_color: u32,
    pub after_color: u32,
}

/// An sRGB colour (0..1 each) as 0xRRGGBB.
pub(super) fn packed(c: [f32; 3]) -> u32 {
    let ch = |v: f32| (if v.is_finite() { v.clamp(0.0, 1.0) } else { 0.0 } * 255.0).round() as u32;
    (ch(c[0]) << 16) | (ch(c[1]) << 8) | ch(c[2])
}

/// 0xRRGGBB as an sRGB colour (0..1 each).
fn unpacked(rgb: u32) -> [f32; 3] {
    let ch = |shift: u32| ((rgb >> shift) & 0xFF) as f32 / 255.0;
    [ch(16), ch(8), ch(0)]
}

/// Clip `id` in `tl` changed by `f`, unless it is on a locked track. Whether `f` changed it.
fn clip_edit(tl: &mut Timeline, id: u64, f: impl FnOnce(&mut procraft_engine::timeline::Clip) -> bool) -> bool {
    let id = ClipId(id);
    let Some(track) = tl.track_of_clip(id) else { return false };
    if tl.track(track).is_none_or(|t| t.locked) {
        return false;
    }
    tl.clip_mut(id).is_some_and(f)
}

#[uniffi::export]
impl CanvasSession {
    /// Clip `id`'s own opacity (0..1; its keyed opacity, where it has keys, goes over it): one
    /// "Clip Opacity" undo step when `done`. False on a locked track.
    pub fn set_timeline_clip_opacity(&self, id: u64, opacity: f32, done: bool) -> bool {
        let opacity = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { 1.0 };
        self.change_live("Clip Opacity", done, |d| {
            d.timeline.as_mut().is_some_and(|tl| {
                clip_edit(tl, id, |c| {
                    c.opacity = opacity;
                    true
                })
            })
        })
    }

    /// Clip `id` blended by mode `blend` (a blend mode id, see `blend_modes`), as an undo step.
    /// False if it already is, or is on a locked track.
    pub fn set_timeline_clip_blend(&self, id: u64, blend: u32) -> bool {
        let Some(mode) = BlendMode::from_id(blend) else { return false };
        self.change("Clip Blend Mode", |d| {
            clip_edit(d.timeline.as_mut()?, id, |c| {
                let changed = c.blend != mode;
                c.blend = mode;
                changed
            })
            .then_some(())
        })
        .is_some()
    }

    /// Clip `id` turned off (it doesn't show, play or onion skin, and stays where it is) or on
    /// again, as an undo step. False if it already is, or is on a locked track.
    pub fn set_timeline_clip_disabled(&self, id: u64, disabled: bool) -> bool {
        self.change(if disabled { "Turn Clip Off" } else { "Turn Clip On" }, |d| {
            clip_edit(d.timeline.as_mut()?, id, |c| {
                let changed = c.disabled != disabled;
                c.disabled = disabled;
                changed
            })
            .then_some(())
        })
        .is_some()
    }

    /// Clip `id` coloured `label` (0xRRGGBB) in the panel, or as it is by default, as an undo step.
    /// False if it already is.
    pub fn set_timeline_clip_label(&self, id: u64, label: Option<u32>) -> bool {
        let label = label.map(unpacked);
        self.change("Clip Colour", |d| {
            let c = d.timeline.as_mut()?.clip_mut(ClipId(id))?;
            let changed = c.label.map(packed) != label.map(packed);
            c.label = label;
            changed.then_some(())
        })
        .is_some()
    }

    /// Clip `id` renamed, as an undo step.
    pub fn rename_timeline_clip(&self, id: u64, name: String) -> bool {
        let name: String = name.trim().chars().take(256).collect();
        if name.is_empty() {
            return false;
        }
        self.change("Rename Clip", |d| {
            let c = d.timeline.as_mut()?.clip_mut(ClipId(id))?;
            (c.name != name).then(|| c.name = name)
        })
        .is_some()
    }

    /// Track `id`'s opacity (0..1), over each of its clips' own: one "Track Opacity" undo step
    /// when `done`.
    pub fn set_timeline_track_opacity(&self, id: u64, opacity: f32, done: bool) -> bool {
        let opacity = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { 1.0 };
        self.change_live("Track Opacity", done, |d| {
            d.timeline.as_mut().and_then(|tl| tl.track_mut(TrackId(id))).is_some_and(|t| {
                t.opacity = opacity;
                true
            })
        })
    }

    /// Track `id` moved to place `to` among the tracks beside it (bottom first), as an undo step:
    /// it draws over the ones below it now, and a clip's mask comes from the track below it now.
    pub fn move_timeline_track(&self, id: u64, to: u32) -> bool {
        let to = usize::try_from(to).unwrap_or(usize::MAX);
        self.change("Move Track", |d| d.timeline.as_mut()?.move_track(TrackId(id), to).then_some(())).is_some()
    }

    /// Track `id` shown alone, every other track hidden (but those holding groups it is in, and
    /// those inside its own groups), as an undo step; asked again while that still stands, the
    /// tracks shown and hidden as they were before (or, with nothing kept from before, every one
    /// shown). Asked for another track while one shows alone, that one shows alone instead, what
    /// was kept from before the first staying kept. False if nothing changed.
    pub fn solo_timeline_track(&self, id: u64) -> bool {
        let id = TrackId(id);
        let (standing, kept) = {
            let st = self.shared.lock();
            let Some(tl) = st.doc.timeline.as_ref() else { return false };
            let kept = st.timeline_solo.as_ref().filter(|(t, _)| tl.soloed(*t)).map(|(t, k)| (*t, k.clone()));
            (tl.soloed(id), kept)
        };
        // shown alone again: back as they were (or every one shown, with nothing kept)
        let back = match (&kept, standing) {
            (Some((t, flags)), true) if *t == id => Some(Some(flags.clone())),
            (None, true) => Some(None),
            _ => None,
        };
        let soloing = back.is_none();
        let mut before = None;
        let changed = self
            .change(if soloing { "Solo Track" } else { "Show Tracks" }, |d| {
                let tl = d.timeline.as_mut()?;
                match back {
                    None => {
                        before = Some(tl.hidden_flags());
                        tl.solo(id).then_some(())
                    }
                    Some(flags) => {
                        let flags = flags.unwrap_or_else(|| tl.every_track().into_iter().map(|t| (t, false)).collect());
                        tl.set_hidden(&flags).then_some(())
                    }
                }
            })
            .is_some();
        let mut st = self.shared.lock();
        if changed {
            st.timeline_solo = soloing.then(|| (id, kept.map(|(_, k)| k).or(before).unwrap_or_default()));
        }
        changed
    }

    /// The frames playing goes round, if part of the timeline is looped.
    pub fn timeline_loop(&self) -> Option<TimelineLoop> {
        let st = self.shared.lock();
        let (start, end) = st.doc.timeline.as_ref()?.loop_range?;
        Some(TimelineLoop { start, end })
    }

    /// Playing goes round frames `range` (kept within the timeline), or the whole timeline, as an
    /// undo step. False if it already does.
    pub fn set_timeline_loop(&self, range: Option<TimelineLoop>) -> bool {
        self.change(if range.is_some() { "Loop Playback" } else { "Stop Looping" }, |d| {
            let tl = d.timeline.as_mut()?;
            let range = range
                .map(|r| (r.start.min(tl.duration.saturating_sub(1)), r.end.min(tl.duration)))
                .filter(|(a, b)| a < b);
            (tl.loop_range != range).then(|| tl.loop_range = range)
        })
        .is_some()
    }

    /// Where playing goes from frame `at`, moving `step` (1 on, -1 back): round the looped frames
    /// if some are (starting from the first of them when `at` is outside), else the whole
    /// timeline, as its playback says at the end. None once a single pass ends.
    pub fn timeline_next_step(&self, at: u32, step: i32) -> Option<PlayStep> {
        let st = self.shared.lock();
        let (at, step) = st.doc.timeline.as_ref()?.next_played(at, step)?;
        Some(PlayStep { at, step })
    }

    /// The onion skins either side of the drawing drawn on.
    pub fn timeline_onion(&self) -> Option<TimelineOnion> {
        let st = self.shared.lock();
        let o = st.doc.timeline.as_ref()?.onion;
        Some(TimelineOnion {
            before: o.count[0],
            after: o.count[1],
            before_color: packed(o.colors[0]),
            after_color: packed(o.colors[1]),
        })
    }

    /// The onion skins either side changed (live, as sliders move: not an undo step).
    pub fn set_timeline_onion(&self, onion: TimelineOnion) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(tl) = st.doc.timeline.as_mut() else { return };
        let most = procraft_engine::animation::MAX_ONION_FRAMES;
        tl.onion.count = [onion.before.min(most), onion.after.min(most)];
        tl.onion.colors = [unpacked(onion.before_color), unpacked(onion.after_color)];
        st.revision += 1;
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
    }
}

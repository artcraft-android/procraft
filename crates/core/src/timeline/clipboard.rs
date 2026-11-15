//! The timeline clipboard: clips (with their keys and their drawings' layers) or keys on their
//! own, copied or cut and pasted at the playhead on the track drawn on, each paste one undo step.
//!
//! The clipboard keeps the layers of the drawings it holds as they were when copied, and the
//! canvas keeps their textures (a cut drawing's too, though its layers are gone), so a paste copies
//! them as they are then: as they were cut, or as they are now if they are still there.

use std::collections::HashMap;

use make2d_engine::document::{Layer, LayerId, LayerKind};
use make2d_engine::timeline::{ClipId, CopiedClips};

use crate::{CanvasSession, State};

/// Clips copied, and their drawings' layers, by drawing (in the clips' order).
#[derive(Clone, Debug)]
pub(crate) struct CopiedTimelineClips {
    clips: CopiedClips,
    layers: Vec<(LayerId, Vec<Layer>)>,
}

/// What the timeline clipboard holds, for the apps' menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimelineClipboardInfo {
    /// How many clips (0: none).
    pub clips: u32,
    /// Keys are held.
    pub keys: bool,
}

#[uniffi::export]
impl CanvasSession {
    /// What the timeline clipboard holds.
    pub fn timeline_clipboard(&self) -> TimelineClipboardInfo {
        let st = self.shared.lock();
        TimelineClipboardInfo {
            clips: st.timeline_clips.as_ref().map_or(0, |c| u32::try_from(c.clips.clips.len()).unwrap_or(u32::MAX)),
            keys: st.timeline_keys.is_some(),
        }
    }

    /// Clips `ids` (on tracks of one level) copied with their keys, in place of what the
    /// clipboard held. False if one isn't there or they aren't on one level.
    pub fn copy_timeline_clips(&self, ids: Vec<u64>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        st.copy_clips(&ids.into_iter().map(ClipId).collect::<Vec<_>>())
    }

    /// Clips `ids` copied, then taken off their tracks with their drawings' layers, as one undo
    /// step.
    pub fn cut_timeline_clips(&self, ids: Vec<u64>) -> bool {
        let ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
        {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            if !st.copy_clips(&ids) {
                return false;
            }
        }
        let cut = self
            .change("Cut Clips", |d| {
                let tl = d.timeline.as_mut()?;
                if ids.iter().any(|id| tl.track_of_clip(*id).and_then(|t| tl.track(t)).is_none_or(|t| t.locked)) {
                    return None;
                }
                let gone: Vec<_> = ids.iter().filter_map(|id| tl.remove(*id, false)).collect();
                for root in gone.iter().flat_map(|c| c.roots()) {
                    d.delete_layer(root);
                }
                Some(())
            })
            .is_some();
        if cut {
            self.follow();
        }
        cut
    }

    /// The clips copied put down at the playhead, the lowest on the track drawn on and the others
    /// on the tracks above it (made, if there are none), each drawing a copy of its layers, as one
    /// undo step. The clips there move along to make room. Returns the clips pasted (none if there
    /// is nothing to paste, a track is locked or there is no room for the layers).
    pub fn paste_timeline_clips(&self) -> Vec<u64> {
        let made = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            st.paste_clips()
        };
        if !made.is_empty() {
            self.follow();
        }
        made.into_iter().map(|c| c.0).collect()
    }

    /// Clip `id`'s keys copied (only those on the playhead's frame, if `here`), in place of what
    /// the clipboard held. False if it has none there.
    pub fn copy_timeline_keys(&self, id: u64, here: bool) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(tl) = st.doc.timeline.as_ref() else { return false };
        let at = if here {
            match tl.local_frame(ClipId(id), frame).and_then(|f| i32::try_from(f).ok()) {
                Some(f) => Some(f),
                None => return false,
            }
        } else {
            None
        };
        let Some(keys) = tl.copy_keys(ClipId(id), at) else { return false };
        st.timeline_keys = Some(keys);
        st.timeline_clips = None;
        st.hold_clipboard();
        true
    }

    /// The keys copied laid on the clip at the playhead on the track drawn on, the first on the
    /// playhead's frame, as one undo step. False with no keys copied or no clip there.
    pub fn paste_timeline_keys(&self) -> bool {
        let (target, at, keys) = {
            let st = self.shared.lock();
            let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
            let target = st.doc.timeline.as_ref().zip(st.drawn_track()).and_then(|(tl, t)| {
                let clip = super::clip_on(tl, t, frame)?.id;
                Some((clip, i32::try_from(tl.local_frame(clip, frame)?).ok()?))
            });
            let Some(((target, at), keys)) = target.zip(st.timeline_keys.clone()) else { return false };
            (target, at, keys)
        };
        self.change("Paste Keys", |d| d.timeline.as_mut()?.paste_keys(target, &keys, at).then_some(())).is_some()
    }
}

impl CanvasSession {
    /// After clips come or go: the current layer follows the playhead.
    fn follow(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.follow_playhead();
        self.shared.frame(st);
    }
}

impl State {
    /// Clips `ids` into the clipboard; false if they can't be copied.
    fn copy_clips(&mut self, ids: &[ClipId]) -> bool {
        let Some(clips) = self.doc.timeline.as_ref().and_then(|tl| tl.copy_clips(ids)) else { return false };
        let layers = clips.roots().into_iter().filter_map(|r| Some((r, self.doc.unit_layers(r)?))).collect();
        self.timeline_clips = Some(CopiedTimelineClips { clips, layers });
        self.timeline_keys = None;
        self.hold_clipboard();
        true
    }

    /// The canvas keeps the textures of the layers the clipboard holds.
    fn hold_clipboard(&mut self) {
        let held = self.timeline_clips.as_ref().map_or(Vec::new(), |c| {
            c.layers.iter().flat_map(|(_, ls)| ls).filter(|l| l.kind != LayerKind::Group).map(|l| l.id).collect()
        });
        if let Some(c) = self.canvas.as_mut() {
            c.hold_layers(held);
        }
    }

    /// The clipboard's clips pasted (see [`CanvasSession::paste_timeline_clips`]).
    fn paste_clips(&mut self) -> Vec<ClipId> {
        let Some(copied) = self.timeline_clips.clone() else { return Vec::new() };
        let frame = self.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(track) = self.drawn_track() else { return Vec::new() };
        let Some(level) = self.doc.timeline.as_ref().and_then(|tl| tl.level_start(track)) else { return Vec::new() };
        let units = copied.layers.iter().flat_map(|(_, ls)| ls).filter(|l| l.kind != LayerKind::Group).count();
        if !self.doc.has_room(units) {
            return Vec::new();
        }
        let before = self.doc.clone();
        let mut roots = HashMap::new();
        let mut pairs = Vec::new();
        for (root, layers) in &copied.layers {
            if let Some((copy, p)) = self.doc.insert_copy(layers, *root) {
                roots.insert(*root, copy);
                pairs.extend(p);
            }
        }
        let at = frame.saturating_sub(level);
        let made = self.doc.timeline.as_mut().and_then(|tl| tl.paste_clips(&copied.clips, track, at, &roots));
        let Some(made) = made else {
            self.doc = before;
            return Vec::new();
        };
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.name_next_step("Paste Clips");
            canvas.duplicate_layers(pairs, before);
        }
        self.revision += 1;
        made
    }
}

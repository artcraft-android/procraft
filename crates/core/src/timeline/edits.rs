//! Clips moved into and out of group clips, a sound's level keyed over time, and the layers
//! panel's + on a timeline (a layer in the drawing at the playhead).

use make2d_engine::timeline::{ClipContent, ClipId, Easing};

use crate::CanvasSession;

#[uniffi::export]
impl CanvasSession {
    /// Clip `id` taken out of the group clip it is in, onto the group's level where it shows (the
    /// group's keys folded into it), as an undo step. The track it lands on is drawn on.
    pub fn move_timeline_clip_out_of_group(&self, id: u64) -> bool {
        let onto = self.change("Move Out of Group", |d| d.timeline.as_mut()?.move_out_of_group(ClipId(id)));
        self.landed(onto)
    }

    /// Clip `id` put into group clip `group` (on the same level) where it is, as an undo step:
    /// on the group's topmost track with room there, or a new one. The group grows to cover it.
    pub fn move_timeline_clip_into_group(&self, id: u64, group: u64) -> bool {
        let onto = self.change("Move Into Group", |d| d.timeline.as_mut()?.move_into_group(ClipId(id), ClipId(group)));
        self.landed(onto)
    }

    /// Sound clip `id`'s level keyed at the playhead to `volume` (0..1): shown at once, one
    /// "Level Key" undo step when `done`. False if the playhead isn't on it.
    pub fn set_timeline_volume_key(&self, id: u64, volume: f32, done: bool) -> bool {
        if !volume.is_finite() {
            return false;
        }
        let frame = self.shared.lock().canvas.as_ref().map_or(0, |c| c.timeline_frame());
        self.change_live("Level Key", done, |d| {
            let Some(tl) = d.timeline.as_mut() else { return false };
            let Some(local) = tl.local_frame(ClipId(id), frame).and_then(|f| i32::try_from(f).ok()) else {
                return false;
            };
            let Some(clip) = tl.clip_mut(ClipId(id)) else { return false };
            if local < 0 || local >= i32::try_from(clip.duration).unwrap_or(i32::MAX) {
                return false;
            }
            let ClipContent::Audio(a) = &mut clip.content else { return false };
            let easing =
                a.volume_keys.keys.iter().find(|k| k.frame == local).map_or(Easing::Linear, |k| k.easing.clone());
            a.volume_keys.set(local, volume.clamp(0.0, make2d_engine::media::MAX_VOLUME), easing);
            true
        })
    }

    /// Takes away sound clip `id`'s level key at the playhead, as an undo step.
    pub fn remove_timeline_volume_key(&self, id: u64) -> bool {
        let frame = self.shared.lock().canvas.as_ref().map_or(0, |c| c.timeline_frame());
        self.change("Remove Level Key", |d| {
            let tl = d.timeline.as_mut()?;
            let local = i32::try_from(tl.local_frame(ClipId(id), frame)?).ok()?;
            match &mut tl.clip_mut(ClipId(id))?.content {
                ClipContent::Audio(a) => a.volume_keys.remove(local).then_some(()),
                _ => None,
            }
        })
        .is_some()
    }
}

impl CanvasSession {
    /// After a clip moved into or out of a group: the track it went on is drawn on, and the canvas
    /// drawn again.
    fn landed(&self, onto: Option<make2d_engine::timeline::TrackId>) -> bool {
        let Some(track) = onto else { return false };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.timeline_track = Some(track);
        st.follow_playhead();
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        self.shared.frame(st);
        true
    }

    /// The layers panel's + on a timeline: a new empty layer in the drawing at the playhead on
    /// the track drawn on (a drawing of one layer becoming a group of two), or with no drawing
    /// there, a new drawing. Returns the layer, or 0.
    pub(crate) fn add_timeline_layer(&self) -> u64 {
        let root = {
            let st = self.shared.lock();
            let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
            st.doc.timeline.as_ref().zip(st.drawn_track()).and_then(|(tl, t)| {
                match &super::clip_on(tl, t, frame)?.content {
                    ClipContent::Drawing(d) => Some(d.root),
                    _ => None,
                }
            })
        };
        match root {
            Some(root) => self.change("new layer", |d| d.add_layer_to_drawing(root)).map_or(0, |l| l.0),
            None => {
                let clip = self.add_timeline_drawing();
                let st = self.shared.lock();
                match st.doc.timeline.as_ref().and_then(|tl| tl.clip(ClipId(clip))).map(|c| &c.content) {
                    Some(ClipContent::Drawing(d)) => d.root.0,
                    _ => 0,
                }
            }
        }
    }
}

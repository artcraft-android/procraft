//! An adjustment clip's values keyed on the timeline: the Keys panel lists them as plain rows at
//! the playhead, and a row moved keys the whole adjustment there.

use make2d_engine::filter::ParamUnit;
use make2d_engine::timeline::ClipId;

use crate::CanvasSession;
use crate::timeline::TimelineEasing;

/// How a value of an adjustment reads.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum AdjustmentUnit {
    /// As it is.
    Plain,
    /// Canvas pixels.
    Pixels,
    /// A share, shown as a percentage (its value times 100).
    Percent,
    /// An angle in radians, shown in degrees.
    Degrees,
    /// Photographic stops (each one doubles the light).
    Stops,
}

/// One value of an adjustment clip's adjustment at the playhead, as a row shows it.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AdjustmentValue {
    pub name: String,
    pub value: f32,
    /// The range its row slides over.
    pub low: f32,
    pub high: f32,
    pub unit: AdjustmentUnit,
}

#[uniffi::export]
impl CanvasSession {
    /// Clip `id`'s adjustment values at the playhead (as its keys ease them, else its layer's);
    /// none unless its drawing is an adjustment layer.
    pub fn timeline_adjustment_values(&self, id: u64) -> Vec<AdjustmentValue> {
        let st = self.shared.lock();
        let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(local) = st.doc.timeline.as_ref().and_then(|tl| tl.local_frame(ClipId(id), frame)) else {
            return Vec::new();
        };
        let Some(filter) = st.doc.clip_adjustment_at(ClipId(id), local as f32) else { return Vec::new() };
        filter
            .params()
            .into_iter()
            .map(|p| AdjustmentValue {
                name: p.name.to_owned(),
                value: p.value,
                low: p.low,
                high: p.high,
                unit: match p.unit {
                    ParamUnit::Plain => AdjustmentUnit::Plain,
                    ParamUnit::Pixels => AdjustmentUnit::Pixels,
                    ParamUnit::Percent => AdjustmentUnit::Percent,
                    ParamUnit::Degrees => AdjustmentUnit::Degrees,
                    ParamUnit::Stops => AdjustmentUnit::Stops,
                },
            })
            .collect()
    }

    /// Clip `id`'s adjustment keyed at the playhead with value `index` (in
    /// [`Self::timeline_adjustment_values`]'s order) set to `value`, the others as they are
    /// there; a new key eases into the next as `easing`. Shown at once, one "Key" undo step when
    /// `done`. Keys of another kind of adjustment (the layer changed since) are let go.
    pub fn set_timeline_adjustment_value(
        &self,
        id: u64,
        index: u32,
        value: f32,
        easing: TimelineEasing,
        done: bool,
    ) -> bool {
        let frame = self.shared.lock().canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let clip = ClipId(id);
        let changed = self.change_live("Key", done, |d| {
            d.mark_adjustment_clips();
            let Some(local) = d.timeline.as_ref().and_then(|tl| tl.local_frame(clip, frame)) else { return false };
            let Some(now) = d.clip_adjustment_at(clip, local as f32) else { return false };
            let Ok(at) = i32::try_from(local) else { return false };
            let new = now.with_param(index as usize, value);
            let Some(c) = d.timeline.as_mut().and_then(|tl| tl.clip_mut(clip)) else { return false };
            let keys = &mut c.keys.adjustment;
            if keys.keys.iter().any(|k| !k.value.same_kind(&new)) {
                keys.keys.clear();
            }
            // a key already there keeps how it eases
            let easing = keys.keys.iter().find(|k| k.frame == at).map_or_else(|| easing.easing(), |k| k.easing.clone());
            keys.set(at, new, easing);
            true
        });
        if changed {
            self.redraw_timeline();
        }
        changed
    }
}

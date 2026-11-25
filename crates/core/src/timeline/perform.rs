//! A move performed: while the timeline plays a frame at a time, a clip is dragged about the
//! canvas (made bigger or smaller and turned, with two fingers), and how it is on each frame is
//! keyed there. When the take ends its keys are evened out as much as is asked, thinned to as few
//! as keep the path, and the whole take is one undo step.

use procraft_engine::timeline::{ClipContent, ClipId, Easing};

use crate::CanvasSession;

#[uniffi::export]
impl CanvasSession {
    /// During a take: on timeline frame `frame` clip `id` is moved `x` across and `y` down
    /// (canvas pixels, as its Position keys count), and, where given, scaled `scale` times and
    /// turned `rotation` degrees: shown at once. Part of the take's one undo step (see
    /// `perform_timeline_end`). False if the frame isn't one of the clip's, or its track is locked.
    pub fn perform_timeline_pose(
        &self,
        id: u64,
        frame: u32,
        x: f32,
        y: f32,
        scale: Option<f32>,
        rotation: Option<f32>,
    ) -> bool {
        if !(x.is_finite() && y.is_finite()) {
            return false;
        }
        // what it turns about, if nothing has set it yet: the middle of what is painted in it
        let middle = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.flush();
            let root = st.doc.timeline.as_ref().and_then(|t| t.clip(ClipId(id))).and_then(|c| match &c.content {
                ClipContent::Drawing(d) => Some(d.root),
                _ => None,
            });
            root.and_then(|r| st.painted_middle(r))
        };
        self.change_live("Perform", false, |d| {
            let centre = [d.width() as f32 / 2.0, d.height() as f32 / 2.0];
            let Some(tl) = d.timeline.as_mut() else { return false };
            let id = ClipId(id);
            if tl.track_of_clip(id).and_then(|t| tl.track(t)).is_none_or(|t| t.locked) {
                return false;
            }
            let Some(origin) = tl.origin(id) else { return false };
            let Some(clip) = tl.clip_mut(id) else { return false };
            let Ok(local) = i32::try_from(i64::from(frame) - i64::from(origin)) else { return false };
            if local < 0 || i64::from(local) >= i64::from(clip.duration) {
                return false;
            }
            let k = &mut clip.keys;
            k.position.set(local, [x, y], Easing::Linear);
            if let Some(s) = scale.filter(|s| s.is_finite()) {
                let s = s.clamp(0.01, 100.0);
                k.scale.set(local, [s, s], Easing::Linear);
            }
            if let Some(deg) = rotation.filter(|r| r.is_finite()) {
                k.rotation.set(local, deg.to_radians(), Easing::Linear);
            }
            if k.posed() && k.pivot.is_none() {
                k.pivot = Some(middle.unwrap_or(centre));
            }
            true
        })
    }

    /// The take ends: clip `id`'s keys on timeline frames `first` to `last` are evened out
    /// (`smoothing` 0, as performed, to 1, the most), thinned to those that keep its path within
    /// `tolerance` canvas pixels (more, the smoother), and the path carried through them in a
    /// curve where smoothing is asked. The take becomes one "Perform" undo step. False if there is
    /// no such clip.
    pub fn perform_timeline_end(&self, id: u64, first: u32, last: u32, tolerance: f32, smoothing: f32) -> bool {
        let smoothing = if smoothing.is_finite() { smoothing.clamp(0.0, 1.0) } else { 0.0 };
        let tolerance = if tolerance.is_finite() { tolerance.max(0.0) } else { 0.0 } * (1.0 + 2.0 * smoothing);
        let passes = (smoothing * 8.0).round() as u32;
        self.change_live("Perform", true, |d| {
            let Some(tl) = d.timeline.as_mut() else { return false };
            let id = ClipId(id);
            let Some(origin) = tl.origin(id) else { return false };
            let Some(clip) = tl.clip_mut(id) else { return false };
            let local = |f: u32| i32::try_from(i64::from(f) - i64::from(origin)).unwrap_or(i32::MAX);
            let (a, b) = (local(first), local(last));
            let k = &mut clip.keys;
            k.position.even_out(a, b, passes);
            k.position.thin(a, b, tolerance);
            k.position.curve(a, b, smoothing);
            // (its size within a hundredth, its turn within half a degree)
            k.scale.even_out(a, b, passes);
            k.scale.thin(a, b, 0.01 * (1.0 + 2.0 * smoothing));
            k.rotation.thin(a, b, 0.5_f32.to_radians() * (1.0 + 2.0 * smoothing));
            true
        })
    }
}

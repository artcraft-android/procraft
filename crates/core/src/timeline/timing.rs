//! Retiming a timeline to another frame rate with its timing kept, its sync markers, and the
//! drawing whose layers the layers panel lists.

use crate::CanvasSession;

#[uniffi::export]
impl CanvasSession {
    /// Plays at `fps` frames a second with its timing kept (every clip, key, marker and the length
    /// scaled, so 12 fps on ones becomes 24 fps on twos), the playhead with it, as one undo step.
    /// False if the rate stays as it is. (`set_timeline_settings` changes the rate alone, every
    /// clip staying on its frames.)
    pub fn retime_timeline(&self, fps: u32) -> bool {
        let (frame, old) = {
            let st = self.shared.lock();
            let Some(tl) = st.doc.timeline.as_ref() else { return false };
            (st.canvas.as_ref().map_or(0, |c| c.timeline_frame()), tl.fps.max(1))
        };
        let Some(new) = self.change("Change Frame Rate", |d| {
            let tl = d.timeline.as_mut()?;
            tl.retime(fps).then_some(tl.fps)
        }) else {
            return false;
        };
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let last = st.doc.timeline.as_ref().map_or(0, |t| t.duration.saturating_sub(1));
        let scaled = (f64::from(frame) * f64::from(new) / f64::from(old)).round();
        if let Some(c) = st.canvas.as_mut() {
            c.set_timeline_frame(if scaled >= f64::from(last) { last } else { scaled.max(0.0) as u32 });
        }
        st.follow_playhead();
        self.shared.frame(st);
        true
    }

    /// The sync markers' frames, in order.
    pub fn timeline_markers(&self) -> Vec<u32> {
        self.shared.lock().doc.timeline.as_ref().map_or(Vec::new(), |t| t.markers.clone())
    }

    /// A sync marker on frame `frame` (tapped while playing, the frame playing), as an undo step.
    /// False if there is one there.
    pub fn add_timeline_marker(&self, frame: u32) -> bool {
        self.change("Add Marker", |d| d.timeline.as_mut()?.add_marker(frame).then_some(())).is_some()
    }

    /// The marker on frame `from` moved to frame `to`, as an undo step.
    pub fn move_timeline_marker(&self, from: u32, to: u32) -> bool {
        self.change("Move Marker", |d| d.timeline.as_mut()?.move_marker(from, to).then_some(())).is_some()
    }

    /// Takes away the marker on frame `frame`, as an undo step.
    pub fn remove_timeline_marker(&self, frame: u32) -> bool {
        self.change("Remove Marker", |d| d.timeline.as_mut()?.remove_marker(frame).then_some(())).is_some()
    }

    /// Takes away every marker, as one undo step.
    pub fn clear_timeline_markers(&self) -> bool {
        self.change("Clear Markers", |d| {
            let tl = d.timeline.as_mut()?;
            (!tl.markers.is_empty()).then(|| tl.markers.clear())
        })
        .is_some()
    }

    /// On a timeline, the drawing the layers panel lists the layers of: the current layer's (the
    /// drawing at the playhead on the track drawn on, as the current layer follows it). None off
    /// a timeline: every layer is listed.
    pub fn timeline_drawing_shown(&self) -> Option<u64> {
        let st = self.shared.lock();
        st.doc.timeline.as_ref()?;
        st.doc.top_of(st.doc.current()).map(|l| l.0)
    }
}

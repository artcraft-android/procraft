//! The timeline's length as the panel changes it: its end dragged on the ruler, fitted to its
//! clips, and frames added before its first frame or after its last. Each is one undo step.

use super::MOST_FRAMES;
use crate::CanvasSession;

#[uniffi::export]
impl CanvasSession {
    /// One past the last frame any clip shows on (0 with no clips or no timeline): where Fit to
    /// Content puts the end.
    pub fn timeline_content_end(&self) -> u32 {
        let st = self.shared.lock();
        st.doc.timeline.as_ref().map_or(0, |tl| tl.content_end())
    }

    /// The timeline `frames` long (at least a frame, shorter than its clips if so: they stay,
    /// past its end) and no longer than the most a timeline lasts, as a "Length" undo step; the
    /// playhead kept within it. False if it is that long.
    pub fn set_timeline_length(&self, frames: u32) -> bool {
        let changed =
            self.change("Length", |d| d.timeline.as_mut()?.set_length(frames.min(MOST_FRAMES)).then_some(())).is_some();
        if changed {
            self.keep_playhead_within(0);
        }
        changed
    }

    /// The timeline as long as its clips, as a "Fit to Content" undo step. False if it is.
    pub fn fit_timeline_to_content(&self) -> bool {
        let changed = self.change("Fit to Content", |d| d.timeline.as_mut()?.fit_to_content().then_some(())).is_some();
        if changed {
            self.keep_playhead_within(0);
        }
        changed
    }

    /// `count` frames added before the first frame (`at_start`: everything moves along, the
    /// playhead with it) or after the last, as an "Add Frames" undo step; no more than the most a
    /// timeline lasts. False for none.
    pub fn add_timeline_frames(&self, at_start: bool, count: u32) -> bool {
        let mut added = 0;
        let changed = self
            .change("Add Frames", |d| {
                let tl = d.timeline.as_mut()?;
                added = count.min(MOST_FRAMES.saturating_sub(tl.duration));
                tl.add_frames(at_start, added).then_some(())
            })
            .is_some();
        if changed {
            self.keep_playhead_within(if at_start { added } else { 0 });
        }
        changed
    }
}

impl CanvasSession {
    /// The playhead moved on by `by` frames and kept within the timeline.
    pub(super) fn keep_playhead_within(&self, by: u32) {
        let frame = {
            let st = self.shared.lock();
            st.canvas.as_ref().map_or(0, |c| c.timeline_frame())
        };
        self.set_timeline_frame(frame.saturating_add(by));
    }
}

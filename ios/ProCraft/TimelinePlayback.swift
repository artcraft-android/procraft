import SwiftUI
import UIKit

// The full view's playback and length: the looped frames drawn on the ruler (held, then dragged
// across it) and their ends dragged; the timeline's end dragged on the ruler; stepping to the
// previous or next drawing; and the Frames page, opened from the frame counter or the end grip,
// with the length fitted to the clips, frames added before or after, the frames showing or the
// open group looped.

extension TimelineTool {
    private var session: CanvasSession { canvasModel.session }

    /// The timeline `frames` long (shorter than its clips, if so: what is past the end stays,
    /// shaded): one undo step.
    func setLength(_ frames: UInt32) {
        stopPlaying()
        if session.setTimelineLength(frames: frames) {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        canvasModel.refresh()
    }

    /// The timeline as long as its clips: one undo step.
    func fitToContent() {
        stopPlaying()
        _ = session.fitTimelineToContent()
        canvasModel.refresh()
    }

    /// `count` frames added before the first frame (everything moving along) or after the last:
    /// one undo step.
    func addFrames(atStart: Bool, count: UInt32) {
        stopPlaying()
        _ = session.addTimelineFrames(atStart: atStart, count: count)
        canvasModel.refresh()
    }

    /// The playhead to the start of the drawing before or after it on the track drawn on (a group
    /// counts as a drawing; sounds and videos don't).
    func stepDrawing(_ by: Int) {
        guard let clips = track?.clips else { return }
        let at = frame
        let starts = clips.filter { $0.kind != .audio && $0.kind != .video }.map(\.start)
        let next = by > 0 ? starts.filter { $0 > at }.min() : starts.filter { $0 < at }.max()
        if let next {
            goTo(next)
            media.blip(next)
        }
    }

}

// MARK: - The ruler

// MARK: - The header

// MARK: - The Frames page


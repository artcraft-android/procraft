import SwiftUI
import UIKit

// A clip put down over others pushes them aside; clips picked together in select
// mode do too, moved along their tracks or to others, go into a group clip together, trim and
// duplicate as one (see "Moving a clip" and "Picking clips together" in docs/design/timeline.md).

extension TimelineTool {
    private var session: CanvasSession { canvasModel.session }

    /// Where clip `id` would land put on track `track` at `start`, the clips there it would cover
    /// pushed aside (`later`, else earlier), and that track's clips then; none if it can't go
    /// there. Changes nothing.
    func placing(_ id: UInt64, on track: UInt64, start: Int, later: Bool) -> TimelinePlacement? {
        session.timelinePlacePreview(id: id, track: track, start: UInt32(max(start, 0)), later: later)
    }

    /// Whether any of clips `ids` has empty frames after it to last through (and none is on a
    /// locked track): what Extend would do something to.
    func canExtend(_ ids: [UInt64]) -> Bool {
        !ids.isEmpty && session.timelineClipsExtend(ids: ids)
    }

    /// Clips `ids` each made to last through the empty frames after it on its track, up to the
    /// next clip there or the end of the timeline: one undo step.
    func extend(_ ids: [UInt64]) {
        guard !ids.isEmpty else { return }
        stopPlaying()
        if session.extendTimelineClips(ids: ids) {
            UISelectionFeedbackGenerator().selectionChanged()
        } else {
            canvasModel.showNotice(
                ids.count == 1 ? "There's no empty space after this clip to fill, or its track is locked."
                    : "There's no empty space after these clips to fill, or one is on a locked track.",
                warning: true
            )
        }
        canvasModel.refresh()
    }

}


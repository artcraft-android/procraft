import SwiftUI
import UIKit

// Group clips unfolded in the full view: each unfolded group's own tracks as rows under the track
// holding it, indented a step a group, their clips where they play on the timeline so they are
// picked, moved and trimmed as on any track. Outside the group's frames the row is shaded, and
// where a cycling group plays its clips again they show faintly beside them. A cycling group's end
// is dragged to set how long it plays, snapping to whole cycles and stopping part way through one
// in between. While a group clip is dragged, the rows of its tracks follow it.

/// How far a row's name steps in for each group it is inside.
private let indentStep: CGFloat = 14

/// A cycling group's end being dragged: how many frames it lasted when the drag began, how many
/// it would now, the most it has room for, and the frames in a cycle.
struct CycleDrag {
    var clip: UInt64
    var from: Int
    var frames: Int
    var most: Int
    var length: Int

    /// How it reads: whole cycles as "×3", a last one cut short as "×2 +1" (frames into it).
    var tag: String {
        let whole = frames / max(length, 1)
        let over = frames % max(length, 1)
        return over == 0 ? "×\(whole)" : "×\(whole) +\(over)"
    }
}

extension TimelineTool {
    /// Where track `id` sits among the unfolded groups, if it is shown.
    func place(_ id: UInt64) -> TimelineRow? {
        rowPlaces[id]
    }

    /// How far track `id`'s name steps in: a step for each group it is inside.
    func indent(_ id: UInt64) -> CGFloat {
        CGFloat(place(id)?.depth ?? 0) * indentStep
    }

    /// Track `t` or a track above it holding a group it is inside is locked: its clips stay as
    /// they are.
    func held(_ t: TimelineTrackInfo) -> Bool {
        t.locked || place(t.id)?.lockedAbove == true
    }

    /// Whether group clip `id` shows its tracks unfolded under its own.
    func unfolded(_ id: UInt64) -> Bool {
        rowPlaces.values.contains { $0.group == id }
    }

    /// Group clip `id` folded, or its tracks unfolded under its own.
    func fold(_ id: UInt64, _ folded: Bool) {
        if canvasModel.session.setTimelineGroupFolded(id: id, folded: folded) {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        canvasModel.refresh()
    }

    /// How many times cycling group `id` plays its cycle, and the most it has room for.
    func cycles(_ id: UInt64) -> (count: Int, most: Int)? {
        guard let n = canvasModel.session.timelineGroupCycles(id: id), n.count == 2 else { return nil }
        return (Int(n[0]), Int(n[1]))
    }

    /// Cycling group `id` made `count` cycles long: one undo step.
    func setCycles(_ id: UInt64, _ count: Int) {
        stopPlaying()
        _ = canvasModel.session.setTimelineGroupCycles(id: id, count: UInt32(max(count, 1)))
        canvasModel.refresh()
    }

    /// The most frames cycling group `id` may last on its track.
    func groupRoom(_ id: UInt64) -> Int? {
        canvasModel.session.timelineGroupRoom(id: id).map { Int($0) }
    }

    /// Cycling group `id` made `frames` long, its last cycle cut short where that falls inside
    /// one: one undo step.
    func setGroupLength(_ id: UInt64, _ frames: Int) {
        stopPlaying()
        _ = canvasModel.session.setTimelineGroupLength(id: id, frames: UInt32(max(frames, 1)))
        canvasModel.refresh()
    }

    /// Whether track `id` is a row inside group clip `group`, at any depth (one of its own
    /// tracks, or a track of a group inside it).
    func inside(_ id: UInt64, group: UInt64) -> Bool {
        var g = place(id)?.group ?? 0
        var seen = 0
        while g != 0, seen < 64 {
            if g == group { return true }
            guard let (holding, _) = clip(g) else { return false }
            g = place(holding.id)?.group ?? 0
            seen += 1
        }
        return false
    }
}


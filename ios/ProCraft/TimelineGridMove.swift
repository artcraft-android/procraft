import SwiftUI
import UIKit

// Moving blocks about the grid timeline (see TimelineGrid.swift). A block held opens its menu;
// dragged on from the hold, the menu goes and the block is picked up: it goes along by whole
// frames and from row to row with the finger, the frames it would cover showing pushed aside,
// and stays where it is let go. With the lasso on, the picked blocks go together. Over the line
// between two rows (one block alone), a line shows across the grid: dropped there, the block gets
// a new track of its own at that place. Near either end of the bar the frames slide under it.

/// The line between two rows a dragged block is over: the track it is beside, whether the new
/// track goes below that one (else above), and where the line is among the rows.
struct GridBetween: Equatable {
    var beside: UInt64
    var below: Bool
    var y: CGFloat
}

/// A block (or the blocks picked together) being dragged somewhere else.
struct GridMove {
    /// The block under the finger, and every block going with it (itself, alone).
    var clip: UInt64
    var ids: [UInt64]
    /// Its track and first frame as the drag began, and the frame under the playhead then.
    var track: UInt64
    var start: Int
    var scrollFrom: CGFloat
    /// How many frames along it has gone, and the track it is over or the line between two.
    var by = 0
    var over: UInt64?
    var between: GridBetween?
    /// The tracks as dropping it here would leave them; and whether it can be dropped here.
    var shown: [UInt64: [TimelineClipInfo]] = [:]
    var valid = false
}

extension TimelineTool {
    private var session: CanvasSession { canvasModel.session }

    /// Block `id` dropped on `track` starting on `start`, what it covers pushed aside: one undo
    /// step.
    func drop(_ id: UInt64, on track: UInt64, start: Int, later: Bool) {
        stopPlaying()
        _ = session.dropTimelineClip(id: id, track: track, start: UInt32(clamping: max(start, 0)), later: later)
        canvasModel.refresh()
    }

    /// Blocks `ids` dropped `by` frames along, on the tracks as many rows from their own as
    /// `track` is from `primary`'s: one undo step.
    func drop(_ ids: [UInt64], primary: UInt64, on track: UInt64, by: Int, later: Bool) {
        stopPlaying()
        _ = session.dropTimelineClips(ids: ids, primary: primary, track: track, by: Int32(clamping: by), later: later)
        canvasModel.refresh()
    }

    /// Block `id` dropped on a new track of its own just above (or `below`) track `beside`: one
    /// undo step.
    func dropOnNewTrack(_ id: UInt64, beside: UInt64, below: Bool, start: Int) {
        stopPlaying()
        let made = session.dropTimelineClipOnNewTrack(id: id, beside: beside, below: below, start: UInt32(clamping: max(start, 0)))
        if made == 0 {
            canvasModel.showNotice("It can't go there.", warning: true)
        }
        canvasModel.refresh()
    }
}

extension GridBody {
    /// The finger that held a block has moved on: the block (with the others picked, the lasso
    /// on) is picked up. False if there is nothing to pick up.
    func moveBegan() -> Bool {
        guard let held = grid.heldClip, let (t, c) = tool.clip(held), !tool.held(t), !tool.isPlaying else { return false }
        var ids = [c.id]
        if let chosen = tool.selecting {
            guard chosen.contains(c.id) else { return false }
            ids = Array(chosen)
        }
        grid.closeMenus(tool)
        tool.picked = nil
        grid.move = GridMove(clip: c.id, ids: ids, track: t.id, start: Int(c.start), scrollFrom: CGFloat(tool.frame))
        // (the frames stand where they are until it nears an end of the bar)
        grid.dragged = CGFloat(tool.frame)
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
        return true
    }

    /// The line between rows at height `y` of them, if it is near one: above the first row and
    /// below the last count.
    private func line(at y: CGFloat, frame: Int) -> GridBetween? {
        guard let first = rows.list.first, let last = rows.list.last else { return nil }
        let near = min(7, first.height / 4)
        if y < first.y + near {
            return GridBetween(beside: first.track(at: frame).id, below: false, y: first.y)
        }
        if y > last.y + last.height - near {
            return GridBetween(beside: last.track(at: frame).id, below: true, y: last.y + last.height)
        }
        // (the line over a row: a track made above that row's, among its own: inside a group,
        // where the row is a group's)
        return rows.list.dropFirst().first { abs(y - $0.y) <= near }.map {
            GridBetween(beside: $0.track(at: frame).id, below: false, y: $0.y)
        }
    }

    /// The block dragged on: where it would land now, and the tracks as that would leave them.
    func moved(_ drag: DragGesture.Value, _ place: GridPlace) {
        guard var m = grid.move else { return }
        let here = place.inRows(drag.location)
        let sz = place.sizes
        grid.moveAgain = { moved(drag, place) }
        slide(nearEdge: drag.location.x, last: place.last)
        // the frames slid under it count as far as it has gone
        let slid = (grid.dragged ?? m.scrollFrom) - m.scrollFrom
        let by = max(Int((drag.translation.width / sz.pitch + slid).rounded()), -m.start)
        var between: GridBetween?
        var over: UInt64?
        let under = Int(((here.x - place.origin) / sz.pitch).rounded(.down))
        if m.ids.count == 1, let line = line(at: here.y, frame: under) {
            between = line
        } else if let row = rows.index(at: min(max(here.y, 0), max(rows.height - 1, 0))) {
            over = rows[row].track(at: under).id
        }
        guard by != m.by || between != m.between || over != m.over || (m.shown.isEmpty && !m.valid) else { return }
        m.by = by
        m.between = between
        m.over = over
        m.shown = [:]
        m.valid = false
        let session = tool.canvasModel.session
        let start = UInt32(clamping: m.start + by)
        if between != nil {
            m.valid = true
        } else if let over, m.ids.count == 1 {
            if let landed = session.timelinePlacePreview(id: m.clip, track: over, start: start, later: by >= 0) {
                m.shown[over] = landed.clips
                // (off its own track, which shows without it)
                if over != m.track, let from = tool.tracks.first(where: { $0.id == m.track }) {
                    m.shown[m.track] = from.clips.filter { $0.id != m.clip }
                }
                m.valid = true
            }
        } else if let over {
            for t in session.timelinePlaceClipsPreview(ids: m.ids, primary: m.clip, track: over, by: Int32(clamping: by), later: by >= 0) {
                m.shown[t.track] = t.clips
                m.valid = true
            }
        }
        UISelectionFeedbackGenerator().selectionChanged()
        grid.move = m
    }

    /// Near either end of the bar, the frames slide under the dragged block for as long as it is
    /// held there.
    private func slide(nearEdge x: CGFloat, last: Int) {
        let zone: CGFloat = 44
        grid.moveSlide = x < Grid.labelsWidth + zone ? -1 : x > width - zone ? 1 : 0
        guard grid.moveSlide != 0, grid.moveTask == nil else { return }
        let grid = grid
        grid.moveTask = Task { @MainActor in
            while !Task.isCancelled, grid.move != nil, grid.moveSlide != 0 {
                let at = (grid.dragged ?? 0) + grid.moveSlide * 0.22
                grid.dragged = min(max(at, 0), CGFloat(last))
                grid.moveAgain?()
                try? await Task.sleep(for: .milliseconds(33))
            }
            grid.moveTask = nil
        }
    }

    /// The block let go: it stays where it showed, as one undo step; the frames slid under it
    /// stay slid.
    func moveEnded() {
        guard let m = grid.move else { return }
        grid.move = nil
        grid.moveAgain = nil
        grid.moveSlide = 0
        grid.moveTask?.cancel()
        grid.moveTask = nil
        let slid = grid.dragged.map { UInt32($0.rounded()) }
        grid.dragged = nil
        if m.valid {
            if let line = m.between {
                tool.dropOnNewTrack(m.clip, beside: line.beside, below: line.below, start: m.start + m.by)
            } else if let over = m.over, over != m.track || m.by != 0 {
                if m.ids.count == 1 {
                    tool.drop(m.clip, on: over, start: m.start + m.by, later: m.by >= 0)
                } else {
                    tool.drop(m.ids, primary: m.clip, on: over, by: m.by, later: m.by >= 0)
                }
            }
        } else {
            UINotificationFeedbackGenerator().notificationOccurred(.warning)
        }
        if let slid, slid != tool.frame {
            tool.goTo(slid)
        }
    }
}

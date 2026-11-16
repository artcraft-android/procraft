import SwiftUI
import UIKit

// A clip put down over others pushes them aside, as Koi's does; clips picked together in select
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

    /// Clip `id` put on track `track` at `start`, pushing aside what it covers: one undo step.
    func placeClip(_ id: UInt64, on track: UInt64, start: Int, later: Bool) {
        stopPlaying()
        _ = session.placeTimelineClip(id: id, track: track, start: UInt32(max(start, 0)), later: later)
        canvasModel.refresh()
    }

    /// How far the clips picked together would go along moved `by` frames toward the tracks as
    /// far up or down as `track` is from clip `primary`'s, and where each would go; none if they
    /// can't go there.
    func moveRoom(primary: UInt64, to track: UInt64, by: Int) -> TimelineMoveRoom? {
        session.timelineMoveRoom(ids: Array(selecting ?? []), primary: primary, track: track, by: Int32(clamping: by))
    }

    /// The clips picked together moved to other tracks as `moveRoom` says: one undo step.
    func moveSelected(primary: UInt64, to track: UInt64, by: Int) {
        stopPlaying()
        if !session.moveTimelineClips(ids: Array(selecting ?? []), primary: primary, track: track, by: Int32(clamping: by)) {
            canvasModel.showNotice("They don't fit there.", warning: true)
        }
        canvasModel.refresh()
    }

    /// The tracks the clips picked together would change put down `by` frames along on the tracks
    /// as far up or down as `track` is from clip `primary`'s, pushing aside what they cover
    /// (`later`, else earlier), with their clips then; empty if they can't go there. Changes
    /// nothing.
    func placingSelected(primary: UInt64, to track: UInt64, by: Int, later: Bool) -> [UInt64: [TimelineClipInfo]] {
        let tracks = session.timelinePlaceClipsPreview(ids: Array(selecting ?? []), primary: primary, track: track,
                                                       by: Int32(clamping: by), later: later)
        return Dictionary(tracks.map { ($0.track, $0.clips) }, uniquingKeysWith: { a, _ in a })
    }

    /// The clips picked together put down as `placingSelected` says: one undo step.
    func placeSelected(primary: UInt64, to track: UInt64, by: Int, later: Bool) {
        stopPlaying()
        if !session.placeTimelineClips(ids: Array(selecting ?? []), primary: primary, track: track,
                                       by: Int32(clamping: by), later: later) {
            canvasModel.showNotice("They can't go there.", warning: true)
        }
        canvasModel.refresh()
    }

    /// The clips picked together put into group clip `group`, each `by` frames along from where
    /// it is: one undo step.
    func moveSelectedIntoGroup(_ group: UInt64, by: Int) {
        stopPlaying()
        if session.moveTimelineClipsIntoGroup(ids: Array(selecting ?? []), group: group, by: Int32(clamping: by)) {
            UISelectionFeedbackGenerator().selectionChanged()
        } else {
            canvasModel.showNotice("Only clips beside the group, on tracks that aren't locked, can go into it together.", warning: true)
        }
        canvasModel.refresh()
    }

    /// The tracks a trim of every picked clip's start (`leading`) or end by `by` frames would
    /// change, with their clips as it would leave them. Changes nothing.
    func trimsPreview(leading: Bool, by: Int) -> [TimelineTrackClips] {
        session.timelineTrimsPreview(ids: Array(selecting ?? []), leading: leading, by: Int32(clamping: by), policy: edgePolicy)
    }

    /// Every picked clip's start (`leading`) or end dragged `by` frames, as the edge policy says
    /// at the clip next to each: one undo step.
    func trimSelected(leading: Bool, by: Int) {
        guard by != 0 else { return }
        stopPlaying()
        _ = session.trimTimelineClips(ids: Array(selecting ?? []), leading: leading, by: Int32(clamping: by), policy: edgePolicy)
        canvasModel.refresh()
    }

    /// Copies of the clips picked together just after the last of them, from the lowest of their
    /// tracks up: one undo step. The copies are picked in their place.
    func duplicateSelected() {
        let ids = Array(selecting ?? [])
        guard !ids.isEmpty else { return }
        stopPlaying()
        let made = session.duplicateTimelineClips(ids: ids)
        if made.isEmpty {
            canvasModel.showNotice("Only clips on tracks that aren't locked, with room for their layers, can be duplicated together.", warning: true)
        } else {
            selecting = Set(made)
        }
        canvasModel.refresh()
    }
}

/// An edge of the clips picked together being dragged: the clip whose grip it is, which edge,
/// where that edge started, how many frames it asks for, the tracks as the trim would leave them,
/// and the frame it has snapped onto.
struct PickedTrim {
    var clip: UInt64
    var leading: Bool
    var from: Int
    var by: Int
    var tracks: [UInt64: [TimelineClipInfo]]
    var snap: Int?
}

/// In select mode, a grip on each edge of every picked clip (wide enough to hold one): dragged,
/// every picked clip's same edge goes as many frames, each as the edge policy says at the clip
/// next to it, shown as it goes; let go, one undo step. Laid over the tracks, above select mode's
/// strokes.
struct PickedTrimGrips: View {
    let tool: TimelineTool
    /// The tracks as shown, top first.
    let rows: [TimelineTrackInfo]
    let pitch: CGFloat
    let rowHeight: CGFloat
    /// Where a dragged start (or end) snaps near frame `at`, if anywhere: what a single clip's
    /// edge snaps onto.
    let snapped: (_ at: CGFloat, _ end: Bool, _ clip: UInt64) -> Int?
    @Binding var trim: PickedTrim?

    var body: some View {
        let chosen = tool.selecting ?? []
        ZStack(alignment: .topLeading) {
            ForEach(Array(rows.enumerated()), id: \.element.id) { row, t in
                if !tool.held(t) {
                    let clips = (trim?.tracks[t.id] ?? t.clips).filter { chosen.contains($0.id) }
                    ForEach(clips, id: \.id) { c in
                        let w = CGFloat(c.duration) * pitch
                        if w >= 30 {
                            // a cycling group's start stays put
                            if c.cycle == 0 {
                                grip(c, leading: true)
                                    .offset(x: CGFloat(c.start) * pitch + 1, y: CGFloat(row) * rowHeight + 3)
                            }
                            grip(c, leading: false)
                                .offset(x: CGFloat(c.start + c.duration) * pitch - 23, y: CGFloat(row) * rowHeight + 3)
                        }
                    }
                }
            }
        }
    }

    private func grip(_ c: TimelineClipInfo, leading: Bool) -> some View {
        Capsule()
            .fill(Color.white)
            .overlay(Capsule().stroke(Pc.accent, lineWidth: 1))
            .frame(width: 5, height: rowHeight - 20)
            .frame(width: 22, height: rowHeight - 6)
            .contentShape(Rectangle())
            .highPriorityGesture(
                // measured on the screen: the clips move under the finger as they trim
                DragGesture(minimumDistance: 2, coordinateSpace: .global)
                    .onChanged { drag in dragged(c, leading: leading, dx: drag.translation.width) }
                    .onEnded { _ in
                        guard let t = trim else { return }
                        trim = nil
                        tool.trimSelected(leading: t.leading, by: t.by)
                    }
            )
    }

    /// Clip `c`'s edge dragged `dx` from where it was: the frames asked for (the edge snapped),
    /// and the tracks as trimming every picked clip's edge that far would leave them.
    private func dragged(_ c: TimelineClipInfo, leading: Bool, dx: CGFloat) {
        let same = trim.map { $0.clip == c.id && $0.leading == leading } ?? false
        let from = same ? trim?.from ?? 0 : Int(leading ? c.start : c.start + c.duration)
        let at = CGFloat(from) + dx / pitch
        let snap = snapped(at, !leading, c.id)
        let by = max(snap ?? Int(at.rounded()), 0) - from
        var next = PickedTrim(clip: c.id, leading: leading, from: from, by: by, tracks: same ? trim?.tracks ?? [:] : [:])
        if !same || trim?.by != by {
            next.tracks = by == 0 ? [:] : Dictionary(tool.trimsPreview(leading: leading, by: by).map { ($0.track, $0.clips) },
                                                    uniquingKeysWith: { a, _ in a })
        }
        // the line shows only when the edge really is on the frame it snapped to
        let shown = next.tracks.values.joined().first { $0.id == c.id }
        let edge = shown.map { Int(leading ? $0.start : $0.start + $0.duration) } ?? from
        next.snap = snap == edge ? snap : nil
        if let s = next.snap, s != trim?.snap {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        trim = next
    }
}

/// Where the clips picked together will land on the tracks they are being moved to: a dashed
/// outline each.
struct PickedMoveOutlines: View {
    /// The tracks as shown, top first.
    let rows: [TimelineTrackInfo]
    /// The track each picked clip goes onto, and how far along.
    let goes: [UInt64: UInt64]
    let by: Int
    let pitch: CGFloat
    let rowHeight: CGFloat

    var body: some View {
        let clips = rows.flatMap(\.clips).filter { goes[$0.id] != nil }
        ZStack(alignment: .topLeading) {
            ForEach(clips, id: \.id) { c in
                if let to = goes[c.id], let row = rows.firstIndex(where: { $0.id == to }) {
                    RoundedRectangle(cornerRadius: 8, style: .circular)
                        .strokeBorder(Pc.accent, style: StrokeStyle(lineWidth: 1.5, dash: [5, 4]))
                        .frame(width: max(CGFloat(c.duration) * pitch - 2, 3), height: rowHeight - 6)
                        .offset(x: CGFloat(Int(c.start) + by) * pitch + 1, y: CGFloat(row) * rowHeight + 3)
                }
            }
        }
        .allowsHitTesting(false)
    }
}

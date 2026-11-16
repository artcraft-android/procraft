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
private var shadeFill: Color { Look.shared.light ? Color.black.opacity(0.10) : Color.black.opacity(0.22) }
private var repeatEdge: Color { Look.shared.light ? Color(argb: 0xFF9A_9A9A) : Color(argb: 0xFF6E_6E6E) }

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

    /// Clip `id` is inside a group: the group open, or one unfolded.
    func inGroup(_ id: UInt64) -> Bool {
        guard let (t, _) = clip(id) else { return false }
        return info?.openGroups.isEmpty == false || (place(t.id)?.group ?? 0) != 0
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

/// A row inside unfolded groups: shaded outside the frames its group plays its clips in, and its
/// clips' repeats in a cycling group's later cycles drawn faintly where they play.
struct GroupRowShade: View {
    let row: TimelineRow
    let width: CGFloat
    let height: CGFloat
    let pitch: CGFloat

    var body: some View {
        Canvas { ctx, size in
            let from = CGFloat(row.start) * pitch
            let to = min(CGFloat(row.end) * pitch, size.width)
            if from > 0 {
                ctx.fill(Path(CGRect(x: 0, y: 0, width: from, height: size.height)), with: .color(shadeFill))
            }
            if to < size.width {
                ctx.fill(Path(CGRect(x: to, y: 0, width: size.width - to, height: size.height)), with: .color(shadeFill))
            }
            // the frames its group plays its clips in, edged on either side
            for x in [from, to] where x > 0 && x < size.width {
                ctx.fill(Path(CGRect(x: x - 0.5, y: 0, width: 1, height: size.height)), with: .color(repeatEdge.opacity(0.6)))
            }
            for r in row.repeats {
                let rect = CGRect(x: CGFloat(r.start) * pitch + 1, y: 3, width: max(CGFloat(r.duration) * pitch - 2, 3), height: size.height - 6)
                let shape = Path(roundedRect: rect, cornerRadius: 8)
                ctx.fill(shape, with: .color(repeatEdge.opacity(0.18)))
                ctx.stroke(shape, with: .color(repeatEdge), style: StrokeStyle(lineWidth: 1, dash: [4, 3]))
            }
        }
        .frame(width: width, height: height)
        .allowsHitTesting(false)
    }
}

/// The steps in front of a row's name, a line for each group it is inside.
struct GroupRowGuides: View {
    let depth: UInt32

    var body: some View {
        HStack(spacing: 0) {
            ForEach(0 ..< Int(depth), id: \.self) { _ in
                Rectangle()
                    .fill(Pc.accent.opacity(0.45))
                    .frame(width: 2)
                    .padding(.vertical, 6)
                    .frame(width: indentStep, alignment: .center)
            }
        }
        .padding(.leading, 6)
        .allowsHitTesting(false)
    }
}

/// The disclosure on a group clip: tapped, its tracks unfold under its own, or fold away.
struct GroupFoldButton: View {
    let tool: TimelineTool
    let clip: TimelineClipInfo
    let tint: Color

    var body: some View {
        let open = tool.unfolded(clip.id)
        Image(systemName: "chevron.right")
            .font(.system(size: 11, weight: .bold))
            .foregroundStyle(tint)
            .rotationEffect(.degrees(open ? 90 : 0))
            .frame(width: 20, height: 28)
            .contentShape(Rectangle())
            .onTapGesture { tool.fold(clip.id, open) }
            .accessibilityLabel(open ? "Fold group" : "Unfold group")
    }
}

/// The grip on a picked cycling group's end: dragged, the group grows or shrinks a frame at a
/// time, snapping to whole cycles (within reach) with a tick of haptics, as long as its track has
/// room for, shown as it goes; let go, one undo step.
struct CycleGrip: View {
    let tool: TimelineTool
    let clip: TimelineClipInfo
    let pitch: CGFloat
    let height: CGFloat
    @Binding var drag: CycleDrag?
    /// How near a whole cycle's end, in points, the end snaps onto it.
    private let reach: CGFloat = 10

    var body: some View {
        Capsule()
            .fill(Color.white)
            .frame(width: 5, height: height - 14)
            .frame(width: 22, height: height)
            .contentShape(Rectangle())
            .overlay(alignment: .top) {
                if let d = drag, d.clip == clip.id {
                    Text(verbatim: d.tag)
                        .font(.system(size: 12, weight: .semibold).monospacedDigit())
                        .foregroundStyle(Color.white)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Capsule().fill(Pc.accent))
                        .fixedSize()
                        .offset(y: -18)
                }
            }
            .frame(maxWidth: .infinity, alignment: .trailing)
            .highPriorityGesture(
                // measured on the screen: the clip grows and shrinks under the finger
                DragGesture(minimumDistance: 2, coordinateSpace: .global)
                    .onChanged { g in
                        let d = drag?.clip == clip.id ? drag : tool.groupRoom(clip.id).map {
                            CycleDrag(clip: clip.id, from: Int(clip.duration), frames: Int(clip.duration), most: $0,
                                      length: Int(max(clip.cycle, 1)))
                        }
                        guard var next = d else { return }
                        let at = CGFloat(next.from) + g.translation.width / max(pitch, 0.01)
                        // a whole cycle's end, if near enough; else the frame under the finger
                        let whole = max((at / CGFloat(next.length)).rounded(), 1) * CGFloat(next.length)
                        let snapped = abs(whole - at) * pitch <= reach
                        let frames = min(max(Int(snapped ? whole : at.rounded()), 1), max(next.most, next.from))
                        if frames != next.frames, frames % next.length == 0 {
                            UISelectionFeedbackGenerator().selectionChanged()
                        }
                        next.frames = frames
                        drag = next
                    }
                    .onEnded { _ in
                        guard let d = drag, d.clip == clip.id else { return }
                        drag = nil
                        if d.frames != d.from {
                            tool.setGroupLength(clip.id, d.frames)
                        }
                    }
            )
    }
}

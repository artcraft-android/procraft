import SwiftUI
import UIKit

// Keys on the grid timeline (see TimelineGrid.swift). A block with keys has a mark under each
// keyed frame. A track with keys has a chevron after its name: turned, a thin row for each kind
// of key its blocks have (moved, scaled, turned, faded; a sound's level) stands under it, a mark
// on each keyed frame, to tap, hold, drag along or take in with the lasso. Animate in the header
// keys with the transform tool (autokey); with it on, a record button arms a take: the next touch
// on the canvas plays the frames on while the block under the playhead follows the finger, keyed
// on each frame, and what was performed is thinned to as few keys as keep its path.

/// A key held on the grid: which, and where along the bar its menu stands.
struct GridKeySpot: Equatable {
    var key: TimelineKeyPick
    var x: CGFloat
}

extension KeyLaneRow {
    /// What its row is called.
    var said: String {
        switch self {
        case .track: ""
        case .volume: "Volume"
        case let .channel(_, channel):
            switch channel {
            case .position: "Position"
            case .scale: "Scale"
            case .rotation: "Rotation"
            case .opacity: "Opacity"
            case .adjustment: "Adjustment"
            }
        }
    }
}

extension TimelineTool {
    /// The rows of keys track `t` has: one for each kind any block on it is keyed in, and its
    /// sounds' level.
    func keyRows(of t: TimelineTrackInfo) -> [KeyLaneRow] {
        var keyed = Set<TimelineChannel>()
        for c in t.clips {
            for k in keys[c.id] ?? [] {
                keyed.insert(k.channel)
            }
        }
        var rows: [KeyLaneRow] = laneOrder.filter { keyed.contains($0) }.map { .channel(t.id, $0) }
        if t.clips.contains(where: { lanes.volumeKeys[$0.id] != nil }) {
            rows.append(.volume(t.id))
        }
        return rows
    }

    /// Whether any block on track `t` has keys.
    func keyed(_ t: TimelineTrackInfo) -> Bool {
        t.clips.contains { !(keys[$0.id] ?? []).isEmpty || lanes.volumeKeys[$0.id] != nil }
    }

    /// Track `id`'s rows of keys put out under it, or away.
    func toggleKeyRows(_ id: UInt64) {
        if gridKeyTracks.contains(id) {
            gridKeyTracks.remove(id)
            // (keys picked on rows no longer showing aren't acted on, and come back with them)
        } else {
            gridKeyTracks.insert(id)
        }
        UISelectionFeedbackGenerator().selectionChanged()
    }

    /// The key of rows `rows` (those sharing one row of the grid) on timeline frame `frame`, if
    /// there is one.
    func key(in rows: [KeyLaneRow], at frame: Int) -> TimelineKeyPick? {
        for lane in rows {
            if let k = lanes.shownKeys(of: self, lane).first(where: { Int($0.frame) == frame }) {
                return TimelineKeyPick(lane: k.lane, frame: k.frame)
            }
        }
        return nil
    }

    /// The playhead brought onto block `clip` if it isn't on one of its frames (its first, or its
    /// last when the playhead is past it): keys are set on the playhead's frame, so a block's Keys
    /// card opened from anywhere else could set none.
    func bringPlayhead(onto clip: TimelineClipInfo) {
        guard clip.duration > 0, frame < clip.start || frame >= clip.start + clip.duration else { return }
        goTo(frame < clip.start ? clip.start : clip.start + clip.duration - 1)
    }

    /// A key of `channel` for block `clip` on the playhead's frame, holding what it shows there;
    /// one already there is taken away instead. One undo step.
    func toggleKey(_ clip: UInt64, _ channel: TimelineChannel) {
        let here = (keys[clip] ?? []).contains { $0.frame == frame && $0.channel == channel }
        if here {
            lanes.removeKey(of: channel, at: frame, on: clip)
            return
        }
        guard let pose = pose(clip) else { return }
        let values: [Float] = switch channel {
        case .position: [pose.x, pose.y]
        case .scale: [pose.scaleX, pose.scaleY]
        case .rotation: [pose.rotation]
        case .opacity: [pose.opacity]
        case .adjustment: []
        }
        guard !values.isEmpty else { return }
        setKey(clip, channel, values, done: true)
    }
}

extension TimelineKeyLanes {
    /// Row `lane`'s keys as they show (as a drag under way would leave them).
    func shownKeys(of tool: TimelineTool, _ lane: KeyLaneRow) -> [TimelineLaneKey] {
        // (each block's keys are a lane of their own to the drag)
        var out: [TimelineLaneKey] = []
        var byLane: [TimelineKeyLane: [TimelineLaneKey]] = [:]
        for k in tool.keyLaneKeys(lane) {
            byLane[k.lane, default: []].append(k)
        }
        for (one, keys) in byLane {
            out += shownKeys(one, keys)
        }
        return out.sorted { $0.frame < $1.frame }
    }

    /// Key `key` taken in with the others picked, or out: by the lasso, which leaves the playhead
    /// where it is.
    func toggle(_ key: TimelineKeyPick) {
        toggleAll([TimelineLaneKey(lane: key.lane, frame: key.frame, easing: .linear)])
    }

    /// Key `key` is one of those picked that show; if it isn't, it alone is picked. For a menu or
    /// a drag that acts on the keys picked.
    func take(_ key: TimelineKeyPick) {
        if !shown.contains(key) {
            if !picks.isEmpty {
                toggleAll(picks.map { TimelineLaneKey(lane: $0.lane, frame: $0.frame, easing: .linear) })
            }
            toggle(key)
        }
    }
}

// MARK: - The rows

/// The rows of keys, in the frames' own space: a strip for each, a mark on each keyed frame; and
/// the marks under the keyed frames of the blocks themselves.
struct GridKeyRows: View {
    let tool: TimelineTool
    let rows: GridRows
    let sizes: GridSizes
    let lo: Int
    let hi: Int

    var body: some View {
        let picked = tool.lanes.shownPicks
        let here = tool.frame
        ZStack(alignment: .topLeading) {
            // a mark under each keyed frame of a block
            Canvas { ctx, _ in
                var marks = Path()
                for (r, t) in rows.tracks {
                    let row = rows[r]
                    let y = row.y + (row.height + sizes.cellHeight) / 2 - 4.5
                    for c in t.clips {
                        var done = Set<UInt32>()
                        for k in tool.keys[c.id] ?? [] where Int(k.frame) >= lo && Int(k.frame) <= hi && done.insert(k.frame).inserted {
                            let x = CGFloat(Int(k.frame) - lo) * sizes.pitch + sizes.cellWidth / 2
                            marks.move(to: CGPoint(x: x, y: y - 3.2))
                            marks.addLine(to: CGPoint(x: x + 3.2, y: y))
                            marks.addLine(to: CGPoint(x: x, y: y + 3.2))
                            marks.addLine(to: CGPoint(x: x - 3.2, y: y))
                            marks.closeSubpath()
                        }
                    }
                }
                ctx.stroke(marks, with: .color(.white), lineWidth: 1.6)
                ctx.fill(marks, with: .color(Pc.accent))
            }
            .frame(width: CGFloat(hi - lo + 1) * sizes.pitch, height: max(rows.height, 1))
            .offset(x: CGFloat(lo) * sizes.pitch)
            .opacity(sizes.pitch >= 9 ? 1 : 0)
            ForEach(rows.list.filter { !$0.lanes.isEmpty }) { row in
                let from = row.segments.first.map { max($0.start, lo) } ?? lo
                Grid.slot.opacity(0.55)
                    .frame(width: CGFloat(hi - from + 1) * sizes.pitch, height: row.height - 6)
                    .clipShape(RoundedRectangle(cornerRadius: 5, style: .continuous))
                    .offset(x: CGFloat(from) * sizes.pitch, y: row.y + 3)
                ForEach(row.lanes, id: \.self) { lane in
                    ForEach(tool.lanes.shownKeys(of: tool, lane).filter { Int($0.frame) >= lo && Int($0.frame) <= hi }, id: \.self) { k in
                        KeyMark(
                            easing: k.easing, picked: picked.contains(TimelineKeyPick(lane: k.lane, frame: k.frame)), here: k.frame == here
                        )
                        .frame(width: 14, height: 14)
                        .offset(x: CGFloat(k.frame) * sizes.pitch + sizes.cellWidth / 2 - 7, y: row.y + row.height / 2 - 7)
                    }
                }
            }
            // keys on two frames or more picked: a grip either side, dragged to spread them out or
            // close them up in time about the far end
            if let grips = Self.grips(tool: tool, rows: rows) {
                ForEach([true, false], id: \.self) { leading in
                    RoundedRectangle(cornerRadius: 3, style: .continuous)
                        .fill(Pc.accent)
                        .frame(width: 6, height: grips.bottom - grips.top - 8)
                        .offset(x: grips.x(leading: leading, sizes: sizes) - 3, y: grips.top + 4)
                }
            }
        }
        .allowsHitTesting(false)
    }

    /// Where the grips stand beside the keys picked: the first and last frames they are on, and
    /// the rows of keys from top to bottom.
    struct Grips {
        var first: Int
        var last: Int
        var top: CGFloat
        var bottom: CGFloat

        /// A grip's middle, in the frames' space: a little clear of the keys' own marks.
        func x(leading: Bool, sizes: GridSizes) -> CGFloat {
            leading ? CGFloat(first) * sizes.pitch + sizes.cellWidth / 2 - 18 : CGFloat(last) * sizes.pitch + sizes.cellWidth / 2 + 18
        }

        /// Which grip is at `point` of the frames' space, if one is: the first's (true) or the last's.
        func grip(at point: CGPoint, sizes: GridSizes) -> Bool? {
            guard point.y >= top, point.y < bottom else { return nil }
            if abs(point.x - x(leading: false, sizes: sizes)) <= 11 {
                return false
            }
            return abs(point.x - x(leading: true, sizes: sizes)) <= 11 ? true : nil
        }
    }

    static func grips(tool: TimelineTool, rows: GridRows) -> Grips? {
        let frames = tool.lanes.shownPicks.map { Int($0.frame) }
        let keyed = rows.list.filter { !$0.lanes.isEmpty }
        guard let first = frames.min(), let last = frames.max(), first < last, let top = keyed.first, let bottom = keyed.last else { return nil }
        return Grips(first: first, last: last, top: top.y, bottom: bottom.y + bottom.height)
    }
}

/// A row of keys' name, in the names' column: tapped, all its keys are picked, or none.
struct GridKeyLabel: View {
    let tool: TimelineTool
    /// The rows of keys on its row of the grid (one, but where groups' tracks share it).
    let lanes: [KeyLaneRow]
    let depth: Int
    let height: CGFloat

    var body: some View {
        let keys = lanes.flatMap { tool.keyLaneKeys($0) }
        let all = !keys.isEmpty && Set(keys.map { TimelineKeyPick(lane: $0.lane, frame: $0.frame) }).isSubset(of: tool.lanes.picks)
        Text(verbatim: lanes.first?.said ?? "")
            .font(.system(size: 11.5, weight: .medium))
            .foregroundStyle(all ? Pc.accent : Grid.ink)
            .lineLimit(1)
            .frame(height: height)
            .padding(.leading, 50 + CGFloat(depth) * 14)
            .contentShape(Rectangle())
            .onTapGesture { tool.lanes.toggleAll(keys) }
            .accessibilityAddTraits(.isButton)
    }
}

// MARK: - Menus

/// A key's menu (and the other keys picked with it): how they ease into the next, copying and
/// pasting, playing them backwards, taking them away.
struct GridKeyMenu: View {
    private enum Beside {
        case easing, curve
    }

    let tool: TimelineTool
    let key: TimelineKeyPick
    let room: CGFloat
    let close: () -> Void

    @State private var beside: Beside?

    private static let easings: [(TimelineEasing, String)] = [
        (.linear, "Linear"), (.hold, "Hold"), (.smooth, "Smooth"), (.easeIn, "Ease in"), (.easeOut, "Ease out"), (.easeInOut, "Ease in and out"),
    ]

    /// How the key eases into the next now.
    private var easing: TimelineEasing? {
        switch key.lane {
        case let .clip(id, channel): tool.keys[id]?.first { $0.frame == key.frame && $0.channel == channel }?.easing
        case let .volume(id): tool.lanes.volumeKeys[id]?.first { $0.frame == key.frame }?.easing
        case .camera: nil
        }
    }

    var body: some View {
        let _ = tool.revision
        let several = tool.lanes.shown.count > 1
        let clip = key.lane.clip
        HStack(alignment: .bottom, spacing: 6) {
            GridCard {
                GridMenuRow(icon: PcIcons.gridBlend, label: "Easing", chevron: true, lit: beside != nil) { beside = beside == nil ? .easing : nil }
                GridMenuLine()
                if let clip {
                    GridMenuRow(icon: PcIcons.gridCopy, label: "Copy keys") {
                        close()
                        tool.goTo(key.frame)
                        tool.copyKeys(clip, here: true)
                    }
                    GridMenuRow(icon: PcIcons.gridPaste, label: "Paste keys", dim: !tool.clipboard.keys) {
                        close()
                        tool.pasteKeys()
                    }
                }
                GridMenuRow(icon: PcIcons.gridFlip, label: "Reverse", dim: !several) {
                    close()
                    tool.lanes.apply(.reverse)
                }
                GridMenuLine()
                GridMenuRow(icon: PcIcons.trash, label: several ? "Delete keys" : "Delete key", tint: Grid.danger) {
                    close()
                    tool.lanes.apply(.remove)
                }
            }
            switch beside {
            case .easing:
                let now = easing
                GridCard {
                    ForEach(Self.easings, id: \.1) { easing, name in
                        GridChoiceRow(label: name, chosen: now == easing) { tool.lanes.apply(.ease(easing: easing)) }
                    }
                    // (the curves kept in the curve editor, for this key)
                    if let clip, !SavedCurves.shared.all.isEmpty {
                        GridMenuLine()
                        let spec = tool.easing(clip, at: key.frame)
                        ForEach(SavedCurves.shared.all) { curve in
                            GridChoiceRow(label: curve.name, chosen: spec == curve.spec) {
                                tool.setEasing(clip, at: key.frame, curve.spec, done: true)
                            }
                        }
                    }
                    if clip != nil {
                        GridMenuLine()
                        GridMenuRow(icon: PcIcons.gridOptions, label: "Edit curve…", chevron: true) {
                            tool.goTo(key.frame)
                            beside = .curve
                        }
                    }
                }
            case .curve:
                if let clip {
                    GridCard(width: 320) {
                        ScrollViewReader { scroll in
                            ScrollView(.vertical, showsIndicators: false) {
                                // (a kept curve's row or its actions, brought into view when it is made or opened)
                                EasingPage(tool: tool, clip: clip, frame: key.frame, reveal: { id in
                                    withAnimation(.easeOut(duration: 0.2)) { scroll.scrollTo(id, anchor: .bottom) }
                                }) { beside = .easing }
                                    .padding(.horizontal, 10)
                            }
                            .frame(height: min(max(room - 60, 220), 520))
                        }
                    }
                }
            case nil:
                EmptyView()
            }
        }
    }
}

/// What a frame's menu opens for its keys: a key of each kind on the playhead's frame put there
/// or taken away, its opacity there, and its keys copied, pasted or all taken away.
struct GridKeysCard: View {
    let tool: TimelineTool
    let clip: TimelineClipInfo
    let close: () -> Void

    var body: some View {
        let _ = tool.revision
        let here = (tool.keys[clip.id] ?? []).filter { $0.frame == tool.frame }
        let inside = tool.frame >= clip.start && tool.frame < clip.start + clip.duration
        let kinds: [(TimelineChannel, String)] = [(.position, "Key position here"), (.scale, "Key scale here"), (.rotation, "Key rotation here")]
        GridCard(width: 262) {
            ForEach(kinds, id: \.1) { channel, label in
                GridChoiceRow(label: label, chosen: here.contains { $0.channel == channel }) {
                    if inside {
                        tool.toggleKey(clip.id, channel)
                    }
                }
                .opacity(inside ? 1 : 0.45)
            }
            // where it is on the playhead's frame, set by hand: each keys it there
            if let pose = tool.pose(clip.id) {
                let size = tool.canvasModel.session.info()
                let (w, h) = (Float(size.width), Float(size.height))
                Group {
                    GridMenuSlider(label: "Move across", value: (pose.x / w + 1) / 2, text: { "\(Int(((($0 * 2) - 1) * w).rounded())) px" }) { v, done in
                        tool.setKey(clip.id, .position, [(v * 2 - 1) * w, pose.y], done: done)
                    }
                    GridMenuSlider(label: "Move down", value: (pose.y / h + 1) / 2, text: { "\(Int(((($0 * 2) - 1) * h).rounded())) px" }) { v, done in
                        tool.setKey(clip.id, .position, [pose.x, (v * 2 - 1) * h], done: done)
                    }
                    // (a tenth to ten times its size, evenly by ratio)
                    GridMenuSlider(
                        label: "Scale", value: log10(min(max(pose.scaleX, 0.1), 10)) / 2 + 0.5,
                        text: { "\(wholeNumber(pow(10, ($0 - 0.5) * 2) * 100)) %" }
                    ) { v, done in
                        let s = pow(10, (v - 0.5) * 2)
                        tool.setKey(clip.id, .scale, [s, s], done: done)
                    }
                    GridMenuSlider(label: "Rotation", value: (pose.rotation + 360) / 720, text: { "\(wholeNumber($0 * 720 - 360))°" }) { v, done in
                        tool.setKey(clip.id, .rotation, [v * 720 - 360], done: done)
                    }
                    GridMenuSlider(label: "Opacity", value: pose.opacity) { v, done in
                        tool.setKey(clip.id, .opacity, [v], done: done)
                    }
                }
                .opacity(inside ? 1 : 0.45)
                .allowsHitTesting(inside)
            }
            // an adjustment frame's own values, each keyed there as it is slid
            AdjustmentKeyRows(model: tool.canvasModel, tool: tool, clip: clip.id, frame: tool.frame)
                .padding(.horizontal, 8)
            GridMenuLine()
            GridMenuRow(icon: PcIcons.gridCopy, label: "Copy keys", dim: (tool.keys[clip.id] ?? []).isEmpty) {
                close()
                tool.copyKeys(clip.id, here: false)
            }
            GridMenuRow(icon: PcIcons.gridPaste, label: "Paste keys", dim: !tool.clipboard.keys) {
                close()
                tool.pasteKeys()
            }
            GridMenuRow(icon: PcIcons.trash, label: "Remove all keys", tint: Grid.danger, dim: (tool.keys[clip.id] ?? []).isEmpty) {
                close()
                tool.removeKeys(clip.id)
            }
        }
    }
}

// MARK: - Perform

/// Over the canvas while a take is armed: the first touch starts it. The frames play on one at a
/// time while the finger is down, the block under the playhead on the chosen track going where
/// the finger takes it (a second finger makes it bigger or smaller and turns it) and keyed there
/// on each; lifted (or at the block's last frame), the take ends, evened out and thinned to the
/// keys that keep its path, as one undo step.
struct GridPerformOverlay: View {
    let model: CanvasModel

    /// The take under way: the block, how it was as the finger came down, how the fingers have
    /// changed that since, the frames it has, and what plays them.
    @State private var take: Take?

    private struct Take {
        var clip: UInt64
        var pose: TimelinePose
        var first: UInt32
        var last: UInt32
        var end: UInt32
        var moved: CGSize = .zero
        /// With a second finger down at some time: how many times as big, and turned how far
        /// (degrees clockwise).
        var scale: CGFloat?
        var turn: CGFloat?
        var playing: Task<Void, Never>?
    }

    var body: some View {
        let tool = model.timeline
        if tool.performArmed, tool.on {
            PerformPad(
                began: { begin() },
                changed: { from, to, scale, turn in
                    // (how far the finger has gone, in the canvas's own pixels)
                    if let a = model.windowToCanvas(from), let b = model.windowToCanvas(to) {
                        let pixels = model.pixels
                        take?.moved = CGSize(width: (b.x - a.x) * pixels, height: (b.y - a.y) * pixels)
                    }
                    if let scale, let turn {
                        take?.scale = scale
                        take?.turn = turn * 180 / .pi
                    }
                },
                ended: { end() }
            )
            .overlay(alignment: .top) {
                Text(verbatim: take == nil ? "Touch the canvas and drag to perform the move" : "Performing…")
                    .font(.system(size: 14, weight: .semibold))
                    .foregroundStyle(.white)
                    .padding(.horizontal, 12)
                    .frame(height: 28)
                    .background(Capsule().fill(autokeyRed))
                    .offset(y: Pc.barHeight + 46)
                    .allowsHitTesting(false)
            }
        }
    }

    private func begin() {
        guard take == nil else { return }
        let tool = model.timeline
        guard let clip = tool.currentClip, clip.kind != .audio, let pose = tool.pose(clip.id) else {
            model.showNotice("Put the playhead on the frame to move first.", warning: true)
            tool.performArmed = false
            return
        }
        tool.stopPlaying()
        tool.performing = true
        let first = tool.frame
        var made = Take(clip: clip.id, pose: pose, first: first, last: first, end: clip.start + clip.duration - 1)
        let session = model.session
        let fps = Double(max(tool.info?.fps ?? 12, 1))
        made.playing = Task { @MainActor in
            var at = first
            while !Task.isCancelled {
                guard let now = take else { return }
                _ = session.performTimelinePose(
                    id: now.clip, frame: at,
                    x: now.pose.x + Float(now.moved.width), y: now.pose.y + Float(now.moved.height),
                    scale: now.scale.map { now.pose.scaleX * Float($0) }, rotation: now.turn.map { now.pose.rotation + Float($0) }
                )
                take?.last = at
                tool.scrub(at)
                try? await Task.sleep(for: .seconds(1 / fps))
                if at >= now.end {
                    end()
                    return
                }
                at += 1
            }
        }
        take = made
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    }

    private func end() {
        guard let done = take else { return }
        take = nil
        done.playing?.cancel()
        let tool = model.timeline
        // within a five-hundredth of the canvas's longer side of where it was dragged, and as
        // much evened out as the settings ask
        let size = model.session.info()
        let smoothing = UserDefaults.standard.object(forKey: "timelinePerformSmoothing") as? Double ?? 0.5
        _ = model.session.performTimelineEnd(
            id: done.clip, first: done.first, last: done.last, tolerance: Float(max(size.width, size.height)) / 500, smoothing: Float(smoothing)
        )
        tool.performing = false
        tool.performArmed = false
        tool.endScrub(done.last)
        model.refresh()
        model.followAutokeyBox()
        // its keys' rows out, to see what was made
        if let track = tool.clip(done.clip)?.0.id {
            tool.gridKeyTracks.insert(track)
        }
        UINotificationFeedbackGenerator().notificationOccurred(.success)
    }
}

/// Takes the touches of a take: the first finger down, where it has gone since (both in the
/// window), and, once a second is down too, how far apart the two are against how they began and
/// how far the line between them has turned (radians, clockwise).
private struct PerformPad: UIViewRepresentable {
    let began: () -> Void
    let changed: (_ from: CGPoint, _ to: CGPoint, _ scale: CGFloat?, _ turn: CGFloat?) -> Void
    let ended: () -> Void

    func makeUIView(context: Context) -> Pad {
        let pad = Pad()
        pad.isMultipleTouchEnabled = true
        pad.backgroundColor = .clear
        pad.pad = self
        return pad
    }

    func updateUIView(_ pad: Pad, context: Context) {
        pad.pad = self
    }

    final class Pad: UIView {
        var pad: PerformPad?
        private var first: UITouch?
        private var second: UITouch?
        private var start: CGPoint = .zero
        /// The two fingers as the second came down: how far apart, and the line between them's
        /// angle; and how far it has turned since, counted through the half turn.
        private var spread: CGFloat = 1
        private var angle: CGFloat = 0
        private var turned: CGFloat = 0
        private var scale: CGFloat?

        private func pair() -> (spread: CGFloat, angle: CGFloat)? {
            guard let a = first?.location(in: nil), let b = second?.location(in: nil) else { return nil }
            return (max(hypot(b.x - a.x, b.y - a.y), 1), atan2(b.y - a.y, b.x - a.x))
        }

        private func say() {
            guard let first else { return }
            if let now = pair() {
                // (the short way round from where it was last, so a turn past half a turn counts on)
                var step = now.angle - angle
                while step > .pi { step -= 2 * .pi }
                while step < -.pi { step += 2 * .pi }
                turned += step
                angle = now.angle
                scale = now.spread / spread
            }
            pad?.changed(start, first.location(in: nil), scale, scale == nil ? nil : turned)
        }

        override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent?) {
            for t in touches {
                if first == nil {
                    first = t
                    start = t.location(in: nil)
                    turned = 0
                    scale = nil
                    pad?.began()
                } else if second == nil, t !== first {
                    second = t
                    if let now = pair() {
                        // (from how they are now: what a finger lifted and put down again changed stays)
                        spread = now.spread / (scale ?? 1)
                        angle = now.angle
                    }
                }
            }
            say()
        }

        override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent?) {
            say()
        }

        override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent?) {
            lifted(touches)
        }

        override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent?) {
            lifted(touches)
        }

        private func lifted(_ touches: Set<UITouch>) {
            if let b = second, touches.contains(b) {
                second = nil
            }
            if let a = first, touches.contains(a) {
                first = nil
                second = nil
                pad?.ended()
            }
        }
    }
}

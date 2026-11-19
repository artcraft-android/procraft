import SwiftUI
import UIKit

// The key lanes in the tracks panel, under the camera's lane while the picked clip's keys are open
// (or a sound's page): a lane for each channel any clip on its track has keys on, and one for its
// sounds' volume, each clip's keys where they fall on the timeline. Keys picked on another track's
// clips keep that track's lanes showing too, each track's under its name, so keys on clips of
// several tracks are picked and changed together. A mark is tapped to pick it
// (or take it out), the playhead going to it; a lane's name, to pick all its keys. The camera's
// lane above picks its keys the same way, so keys on several clips, a sound and the camera are
// picked together. A picked mark dragged moves every picked key by whole frames, as far as all of
// them have room; two or more frames picked, the grips either side stretch them about the far
// one. The header then nudges, reverses or deletes them (and eases the camera's and a sound's,
// which have no Keys page to do it on). Each channel's keys are their own:
// a rotation key picked leaves the opacity key on its frame. A drag shows what it would do (the
// core works it out on a copy) and is one undo step when let go; one the system takes away shows
// no more.

/// How tall a key lane is, and the name over a track's lanes when more than one track's show.
let keyLaneHeight: CGFloat = 26
let keyLaneTrackHeight: CGFloat = 18

private var laneFill: Color { Look.shared.light ? Color(argb: 0xFFDA_DADA) : Color(argb: 0xFF31_3131) }
private var spanFill: Color { Look.shared.light ? Color(argb: 0x14000000) : Color(argb: 0x14FF_FFFF) }
private var ink: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }
private var inkDim: Color { Look.shared.light ? Color(argb: 0x8039_3939) : Color(argb: 0x80C5_C5C5) }
private let rowLineFill = Color(argb: 0x09FF_FFFF)
private var trackNameFill: Color { Look.shared.light ? Color(argb: 0xFFCF_CFCF) : Color(argb: 0xFF2A_2A2A) }

/// The channels in the order the lanes show them, and their names.
private let laneOrder: [TimelineChannel] = [.position, .scale, .rotation, .opacity, .adjustment]

func laneName(_ channel: TimelineChannel) -> String {
    switch channel {
    case .position: "Position"
    case .scale: "Scale"
    case .rotation: "Rotation"
    case .opacity: "Opacity"
    case .adjustment: "Adjustment"
    }
}

/// A row of the key lanes: one channel of every clip on track `track`, its sounds' volume, or the
/// track's name over its lanes.
enum KeyLaneRow: Hashable {
    case track(UInt64)
    case channel(UInt64, TimelineChannel)
    case volume(UInt64)

    var track: UInt64 {
        switch self {
        case let .track(t), let .channel(t, _), let .volume(t): t
        }
    }

    var height: CGFloat {
        if case .track = self { keyLaneTrackHeight } else { keyLaneHeight }
    }
}

extension TimelineKeyLane {
    /// The clip the lane's keys are on (none: the camera's).
    var clip: UInt64? {
        switch self {
        case let .clip(id, _): id
        case let .volume(id): id
        case .camera: nil
        }
    }
}

/// The keys picked in the lanes, and what a drag on them would do.
@Observable
final class TimelineKeyLanes {
    @ObservationIgnored private unowned let model: CanvasModel

    /// The keys picked, in any lane: a track with keys picked on its clips shows its lanes while
    /// the lanes show at all.
    private(set) var picks: Set<TimelineKeyPick> = []
    /// A drag under way: the lanes it changes, their keys as it would leave them, and where the
    /// picked ones would go.
    private(set) var preview: TimelineKeysEdited?
    /// The camera's keys and each sound's volume keys, with how they ease (the clips' own come
    /// with the timeline's keys).
    private(set) var cameraKeys: [TimelineLaneKey] = []
    private(set) var volumeKeys: [UInt64: [TimelineLaneKey]] = [:]

    /// The drag under way: the keys it takes, and what it asks of them.
    @ObservationIgnored private var dragging: (picks: [TimelineKeyPick], edit: TimelineKeyEdit?)?

    init(model: CanvasModel) {
        self.model = model
    }

    private var tool: TimelineTool { model.timeline }

    /// After a refresh: the camera's and the sounds' keys as the timeline panel's reading gives
    /// them (each kept unless it changed), and picks on keys gone let go.
    func refreshed(keys: [UInt64: [TimelineKeyInfo]], camera: [TimelineLaneKey], volumes: [UInt64: [TimelineLaneKey]]) {
        if cameraKeys != camera {
            cameraKeys = camera
        }
        if volumeKeys != volumes {
            volumeKeys = volumes
        }
        guard !picks.isEmpty else { return }
        var there = Set(cameraKeys.map { TimelineKeyPick(lane: $0.lane, frame: $0.frame) })
        for k in volumes.values.joined() {
            there.insert(TimelineKeyPick(lane: k.lane, frame: k.frame))
        }
        for (id, ks) in keys {
            for k in ks {
                there.insert(TimelineKeyPick(lane: .clip(id: id, channel: k.channel), frame: k.frame))
            }
        }
        let kept = picks.intersection(there)
        if kept != picks {
            picks = kept
        }
    }

    func clear() {
        picks = []
        preview = nil
        dragging = nil
    }

    /// The keys picked in lanes showing now (the camera's always shows).
    var shown: Set<TimelineKeyPick> {
        let lanes = tool.keyLanesShown
        return picks.filter { $0.lane == .camera || lanes.contains($0.lane) }
    }

    /// The keys picked as a drag would leave them: where it takes those it moves, the rest as they are.
    var shownPicks: Set<TimelineKeyPick> {
        guard let preview else { return shown }
        let moved = Set(preview.lanes)
        return Set(preview.picks).union(shown.filter { !moved.contains($0.lane) })
    }

    /// Lane `lane`'s keys as shown: as a drag would leave them while one is under way.
    func shownKeys(_ lane: TimelineKeyLane, _ keys: [TimelineLaneKey]) -> [TimelineLaneKey] {
        guard let preview, preview.lanes.contains(lane) else { return keys }
        return preview.keys.filter { $0.lane == lane }
    }

    /// Key `key` tapped: taken in with the others, or out. Taken in, the playhead goes to it, and a
    /// clip's key picks its clip.
    func tap(_ key: TimelineKeyPick) {
        // a drag the system ended without letting go shows no more
        cancelDrag()
        if picks.contains(key) {
            picks.remove(key)
        } else {
            picks.insert(key)
            tool.goTo(key.frame)
            if case let .clip(id, _) = key.lane, tool.picked != id {
                tool.picked = id
            }
        }
        UISelectionFeedbackGenerator().selectionChanged()
    }

    /// Every key of `keys` (a row's) picked, or, all of them picked already, none of them.
    func toggleAll(_ keys: [TimelineLaneKey]) {
        cancelDrag()
        let row = Set(keys.map { TimelineKeyPick(lane: $0.lane, frame: $0.frame) })
        if row.isSubset(of: picks) {
            picks.subtract(row)
        } else {
            picks.formUnion(row)
        }
        UISelectionFeedbackGenerator().selectionChanged()
    }

    /// `edit` done to the keys picked that show, as one undo step; they stay picked where they went.
    func apply(_ edit: TimelineKeyEdit) {
        let taken = shown
        guard !taken.isEmpty else { return }
        commit(edit, picks: Array(taken))
    }

    /// Clip `id`'s key of `channel` at frame `frame` taken away, the others there kept: one undo
    /// step.
    func removeKey(of channel: TimelineChannel, at frame: UInt32, on id: UInt64) {
        commit(.remove, picks: [TimelineKeyPick(lane: .clip(id: id, channel: channel), frame: frame)])
    }

    private func commit(_ edit: TimelineKeyEdit, picks taken: [TimelineKeyPick]) {
        model.timeline.stopPlaying()
        if let ended = model.session.editTimelineKeys(picks: taken, edit: edit) {
            picks = picks.subtracting(taken).union(ended)
        } else {
            // nothing changed: one of them has no room that way, or they ease so already
            UINotificationFeedbackGenerator().notificationOccurred(.warning)
        }
        model.refresh()
    }

    /// Key `key` dragged: it, or every key picked that shows if it is one of them, `by` frames
    /// along. Shown, not done, until `endDrag`.
    func drag(_ key: TimelineKeyPick, by: Int) {
        if dragging == nil {
            if !shown.contains(key) {
                picks = [key]
            }
            dragging = (Array(shown), nil)
        }
        show(by == 0 ? nil : .move(by: Int32(by)))
    }

    /// A stretch grip dragged: the picked keys' distance from frame `anchor` times `factor`.
    func stretch(anchor: UInt32, factor: Double) {
        if dragging == nil {
            let taken = shown
            guard !taken.isEmpty else { return }
            dragging = (Array(taken), nil)
        }
        show(abs(factor - 1) < 0.001 ? nil : .stretch(anchor: anchor, factor: factor))
    }

    private func show(_ edit: TimelineKeyEdit?) {
        guard var d = dragging else { return }
        if let edit, let asked = d.edit, same(edit, asked) {
            return
        }
        d.edit = edit
        dragging = d
        let next = edit.flatMap { model.session.timelineKeysEditPreview(picks: d.picks, edit: $0) }
        if next?.picks != preview?.picks {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        preview = next
    }

    /// The drag let go: what it showed, done as one undo step.
    func endDrag() {
        guard let d = dragging else { return }
        dragging = nil
        let went = preview != nil
        preview = nil
        if went, let edit = d.edit {
            commit(edit, picks: d.picks)
        }
    }

    /// The drag taken away before it was let go (the system's own gesture, a page shut over it):
    /// nothing done, nothing shown.
    func cancelDrag() {
        guard dragging != nil || preview != nil else { return }
        dragging = nil
        preview = nil
    }

    private func same(_ a: TimelineKeyEdit, _ b: TimelineKeyEdit) -> Bool {
        switch (a, b) {
        case let (.move(x), .move(y)): x == y
        case let (.stretch(a1, f1), .stretch(a2, f2)): a1 == a2 && abs(f1 - f2) < 0.005
        default: false
        }
    }
}

extension TimelineTool {
    /// The track whose keys the lanes show first: the picked clip's, while its keys are open (or a
    /// sound's page), not picking clips together.
    var keyLanesTrack: TimelineTrackInfo? {
        guard selecting == nil, let id = picked, let (t, c) = clip(id) else { return nil }
        guard keysOpen || (clipOpen && c.kind == .audio) else { return nil }
        return t
    }

    /// The tracks whose lanes show: the picked clip's, then each other track with keys picked on
    /// its clips, top first as the tracks stand.
    var keyLanesTracks: [TimelineTrackInfo] {
        guard let first = keyLanesTrack else { return [] }
        var held = Set<UInt64>()
        for p in lanes.picks {
            if let id = p.lane.clip, let (t, _) = clip(id) {
                held.insert(t.id)
            }
        }
        held.remove(first.id)
        return [first] + tracks.reversed().filter { held.contains($0.id) }
    }

    /// The rows the lanes show, in order, each track's under its name when more than one track's
    /// show: each channel a clip on the track has keys on, then its sounds' volume.
    var keyLaneRows: [KeyLaneRow] {
        let shown = keyLanesTracks
        var rows: [KeyLaneRow] = []
        for t in shown {
            var keyed = Set<TimelineChannel>()
            for c in t.clips {
                for k in keys[c.id] ?? [] {
                    keyed.insert(k.channel)
                }
            }
            var own: [KeyLaneRow] = laneOrder.filter { keyed.contains($0) }.map { .channel(t.id, $0) }
            if t.clips.contains(where: { lanes.volumeKeys[$0.id] != nil }) {
                own.append(.volume(t.id))
            }
            if shown.count > 1, !own.isEmpty {
                rows.append(.track(t.id))
            }
            rows += own
        }
        return rows
    }

    /// Row `row`'s keys, every clip's on its track, in frame order (none for a track's name).
    func keyLaneKeys(_ row: KeyLaneRow) -> [TimelineLaneKey] {
        guard let t = tracks.first(where: { $0.id == row.track }) else { return [] }
        var out: [TimelineLaneKey] = []
        for c in t.clips {
            switch row {
            case .track:
                break
            case let .channel(_, channel):
                for k in keys[c.id] ?? [] where k.channel == channel {
                    out.append(TimelineLaneKey(lane: .clip(id: c.id, channel: channel), frame: k.frame, easing: k.easing))
                }
            case .volume:
                out.append(contentsOf: lanes.volumeKeys[c.id] ?? [])
            }
        }
        return out.sorted { $0.frame < $1.frame }
    }

    /// Every key of track `id`'s lanes.
    func keyLaneTrackKeys(_ id: UInt64) -> [TimelineLaneKey] {
        keyLaneRows.filter { $0.track == id }.flatMap { keyLaneKeys($0) }
    }

    /// The lanes showing under the camera's.
    var keyLanesShown: Set<TimelineKeyLane> {
        var out = Set<TimelineKeyLane>()
        for t in keyLanesTracks {
            for c in t.clips {
                for channel in laneOrder {
                    out.insert(.clip(id: c.id, channel: channel))
                }
                if c.kind == .audio {
                    out.insert(.volume(id: c.id))
                }
            }
        }
        return out
    }

    /// How tall the lanes stand together.
    var keyLanesHeight: CGFloat {
        keyLaneRows.reduce(0) { $0 + $1.height }
    }

    /// Keys picked in the lanes (the camera's among them), for the header to act on.
    var keysPicked: Bool {
        !lanes.shown.isEmpty
    }
}

/// The lanes' names in the track names' column. A name is tapped to pick all its keys.
struct KeyLanesHeader: View {
    let tool: TimelineTool

    var body: some View {
        let rows = tool.keyLaneRows
        if !rows.isEmpty {
            let picks = tool.lanes.shown
            VStack(spacing: 0) {
                ForEach(rows, id: \.self) { row in
                    if case let .track(id) = row {
                        TrackLaneName(tool: tool, track: id)
                    } else {
                        laneLabel(row, picks: picks)
                    }
                }
            }
        }
    }

    private func laneLabel(_ row: KeyLaneRow, picks: Set<TimelineKeyPick>) -> some View {
        let keys = tool.keyLaneKeys(row)
        let picked = keys.contains { picks.contains(TimelineKeyPick(lane: $0.lane, frame: $0.frame)) }
        let name = if case let .channel(_, c) = row { laneName(c) } else { "Volume" }
        return HStack(spacing: 8) {
            Capsule().fill(Pc.accent.opacity(0.7)).frame(width: 3, height: 14)
            Text(verbatim: name)
                .font(.system(size: 13, weight: picked ? .semibold : .regular))
                .foregroundStyle(picked ? Pc.accent : ink)
            Spacer(minLength: 0)
            Text(verbatim: keys.count == 1 ? "1 key" : "\(keys.count) keys")
                .font(.system(size: 11).monospacedDigit())
                .foregroundStyle(inkDim)
        }
        .padding(.leading, 22)
        .padding(.trailing, 12)
        .frame(height: keyLaneHeight)
        .background(laneFill)
        .overlay(alignment: .bottom) { rowLineFill.frame(height: 1) }
        .contentShape(Rectangle())
        .onTapGesture { tool.lanes.toggleAll(keys) }
    }
}

/// A track's name over its lanes, with more than one track's showing: tapped, every key in them
/// picked (or, all picked, none).
private struct TrackLaneName: View {
    let tool: TimelineTool
    let track: UInt64

    var body: some View {
        let name = tool.tracks.first { $0.id == track }?.name ?? ""
        let picks = tool.lanes.shown
        let keys = tool.keyLaneTrackKeys(track)
        let picked = keys.contains { picks.contains(TimelineKeyPick(lane: $0.lane, frame: $0.frame)) }
        Text(verbatim: name)
            .font(.system(size: 11, weight: .semibold))
            .foregroundStyle(picked ? Pc.accent : inkDim)
            .lineLimit(1)
            .padding(.leading, 12)
            .frame(maxWidth: .infinity, minHeight: keyLaneTrackHeight, maxHeight: keyLaneTrackHeight, alignment: .leading)
            .background(trackNameFill)
            .contentShape(Rectangle())
            .onTapGesture { tool.lanes.toggleAll(keys) }
    }
}

/// The lanes along the timeline, `pitch` points a frame.
struct KeyLanes: View {
    let tool: TimelineTool
    let width: CGFloat
    let pitch: CGFloat
    let frames: Int

    var body: some View {
        let rows = tool.keyLaneRows
        if !rows.isEmpty {
            VStack(spacing: 0) {
                ForEach(rows, id: \.self) { row in
                    if case .track = row {
                        trackNameFill.frame(width: width, height: keyLaneTrackHeight)
                    } else if let t = tool.tracks.first(where: { $0.id == row.track }) {
                        // the clips whose frames the row's keys can go on
                        let volume = if case .volume = row { true } else { false }
                        let spans = t.clips.filter { volume ? $0.kind == .audio : $0.kind != .audio }.map { ($0.start, $0.duration) }
                        LaneRow(tool: tool, keys: tool.keyLaneKeys(row), spans: spans, width: width, pitch: pitch,
                                frames: frames, height: keyLaneHeight, fill: laneFill)
                    }
                }
            }
            .frame(width: width, alignment: .topLeading)
        }
    }
}

/// One lane of marks along the timeline: `keys` as the document has them (shown as a drag would
/// leave them while one is under way), clips' frames shaded under them. A mark is tapped to pick
/// it, the lane elsewhere to let every key go and move the playhead there; a drag begun on a mark
/// moves it, or every key picked with it.
struct LaneRow: View {
    let tool: TimelineTool
    let keys: [TimelineLaneKey]
    let spans: [(UInt32, UInt32)]
    let width: CGFloat
    let pitch: CGFloat
    let frames: Int
    let height: CGFloat
    let fill: Color

    /// The key a drag on the lane began on.
    @State private var dragKey: TimelineKeyPick?
    /// A drag on the lane under way, for one the system takes away to show no more.
    @GestureState private var held = false

    var body: some View {
        let byLane = Dictionary(grouping: keys, by: \.lane)
        let shown = byLane.flatMap { lane, ks in tool.lanes.shownKeys(lane, ks) }
        let picks = tool.lanes.shownPicks
        ZStack(alignment: .leading) {
            fill
                .frame(width: width, height: height)
                .contentShape(Rectangle())
                .onTapGesture { location in
                    // elsewhere on the lane: the keys let go, the playhead there
                    tool.lanes.clear()
                    let f = min(max(Int(location.x / pitch), 0), max(frames - 1, 0))
                    tool.goTo(UInt32(f))
                }
            ForEach(Array(spans.enumerated()), id: \.offset) { _, span in
                spanFill
                    .frame(width: CGFloat(span.1) * pitch, height: height)
                    .offset(x: CGFloat(span.0) * pitch)
                    .allowsHitTesting(false)
            }
            // a line from each key to the next of its own lane
            Path { p in
                for ks in Dictionary(grouping: shown, by: \.lane).values {
                    let ordered = ks.sorted { $0.frame < $1.frame }
                    for (a, b) in zip(ordered, ordered.dropFirst()) {
                        p.move(to: CGPoint(x: (CGFloat(a.frame) + 0.5) * pitch, y: height / 2))
                        p.addLine(to: CGPoint(x: (CGFloat(b.frame) + 0.5) * pitch, y: height / 2))
                    }
                }
            }
            .stroke(inkDim, style: StrokeStyle(lineWidth: 1))
            .frame(width: width, height: height)
            .allowsHitTesting(false)
            ForEach(shown, id: \.self) { k in
                let key = TimelineKeyPick(lane: k.lane, frame: k.frame)
                KeyMark(easing: k.easing, picked: picks.contains(key), here: k.frame == tool.frame)
                    .frame(width: 22, height: height)
                    .contentShape(Rectangle())
                    .offset(x: (CGFloat(k.frame) + 0.5) * pitch - 11)
                    .onTapGesture { tool.lanes.tap(key) }
            }
        }
        .frame(width: width, height: height, alignment: .leading)
        .overlay(alignment: .bottom) { rowLineFill.frame(height: 1) }
        // a drag begun on a key moves it (the lane takes it, not the mark: the mark is drawn anew
        // where the drag shows it, which would end a drag of its own)
        .gesture(
            DragGesture(minimumDistance: 4)
                .updating($held) { _, held, _ in held = true }
                .onChanged { drag in
                    let key = dragKey ?? keyAt(drag.startLocation.x)
                    guard let key else { return }
                    dragKey = key
                    tool.lanes.drag(key, by: Int((drag.translation.width / pitch).rounded()))
                }
                .onEnded { _ in
                    guard dragKey != nil else { return }
                    dragKey = nil
                    tool.lanes.endDrag()
                }
        )
        .onChange(of: held) { _, now in
            // ended without onEnded: the system took the drag away
            guard !now else { return }
            DispatchQueue.main.async {
                guard dragKey != nil else { return }
                dragKey = nil
                tool.lanes.cancelDrag()
            }
        }
        .onDisappear {
            // the lane gone with the drag still on it: nothing more will end it
            guard dragKey != nil else { return }
            dragKey = nil
            tool.lanes.cancelDrag()
        }
    }

    /// The key under point `x` of the lane, as the document has them (not as a drag shows them).
    private func keyAt(_ x: CGFloat) -> TimelineKeyPick? {
        let reach: CGFloat = 11
        let near = keys.min { abs((CGFloat($0.frame) + 0.5) * pitch - x) < abs((CGFloat($1.frame) + 0.5) * pitch - x) }
        guard let near, abs((CGFloat(near.frame) + 0.5) * pitch - x) <= reach else { return nil }
        return TimelineKeyPick(lane: near.lane, frame: near.frame)
    }
}

/// A key's mark: a diamond, a square when it holds, a dot when it eases.
struct KeyMark: View {
    let easing: TimelineEasing
    let picked: Bool
    /// On the playhead's frame.
    let here: Bool

    var body: some View {
        let fill = picked ? Pc.accent : (here ? ink : inkDim)
        let edge = picked ? Color.white : Color.clear
        switch easing {
        case .hold:
            Rectangle().fill(fill).frame(width: 9, height: 9).overlay(Rectangle().strokeBorder(edge, lineWidth: 1.5))
        case .smooth, .easeIn, .easeOut, .easeInOut:
            Circle().fill(fill).frame(width: 10, height: 10).overlay(Circle().strokeBorder(edge, lineWidth: 1.5))
        case .linear:
            Rectangle().fill(fill).frame(width: 9, height: 9).overlay(Rectangle().strokeBorder(edge, lineWidth: 1.5))
                .rotationEffect(.degrees(45))
        }
    }
}

/// With keys on two frames or more picked (the camera's lane and the key lanes together), a grip
/// either side of them: dragged, they stretch or shrink in time about the far end, a readout
/// saying how much. A grip that would stand off the start or end of the timeline stands at it.
struct KeyLaneGrips: View {
    let tool: TimelineTool
    let width: CGFloat
    let pitch: CGFloat

    /// A stretch grip being dragged: which, and the picked keys' first and last frames as it began.
    @State private var stretching: (leading: Bool, first: UInt32, last: UInt32)?
    @State private var factor: Double = 1
    @GestureState private var held = false

    var body: some View {
        let frames = tool.lanes.shownPicks.map(\.frame)
        let height = cameraLaneHeight + tool.keyLanesHeight
        // while one is dragged the grips stay, even should the keys it shows stand on one frame
        let picked = frames.min().flatMap { first in frames.max().map { (first, $0) } }.flatMap { $0.0 < $0.1 ? $0 : nil }
        if let span = picked ?? stretching.map({ ($0.first, $0.last) }) {
            let (first, last) = span
            ZStack(alignment: .topLeading) {
                ForEach([true, false], id: \.self) { leading in
                    grip(leading: leading, first: first, last: last, height: height)
                }
            }
            .frame(width: width, height: height, alignment: .topLeading)
        }
    }

    private func grip(leading: Bool, first: UInt32, last: UInt32, height: CGFloat) -> some View {
        // clear of the marks (a mark takes taps 11 points either side of it), but on the timeline
        let wanted = (CGFloat(leading ? first : last) + 0.5) * pitch + (leading ? -18 : 18)
        let x = min(max(wanted, 7), width - 7)
        return ZStack {
            Capsule().fill(Pc.accent).frame(width: 4, height: max(height - 8, 12))
        }
        .frame(width: 14, height: height)
        .contentShape(Rectangle())
        .overlay(alignment: .top) {
            if stretching?.leading == leading {
                Text(verbatim: "\(Int((factor * 100).rounded()))%")
                    .font(.system(size: 11, weight: .semibold).monospacedDigit())
                    .foregroundStyle(Color.white)
                    .padding(.horizontal, 5)
                    .padding(.vertical, 2)
                    .background(Capsule().fill(Pc.accent))
                    .fixedSize()
                    .offset(y: -2)
            }
        }
        .offset(x: x - 7)
        .highPriorityGesture(
            DragGesture(minimumDistance: 2, coordinateSpace: .global)
                .updating($held) { _, held, _ in held = true }
                .onChanged { drag in
                    let s = stretching ?? (leading, first, last)
                    stretching = s
                    // the far end stays; the near one follows the finger
                    let anchor = leading ? s.last : s.first
                    let span = Double(Int(leading ? s.first : s.last) - Int(anchor))
                    let wanted = (span + Double(drag.translation.width / pitch)) / span
                    factor = max(wanted, 0.01)
                    tool.lanes.stretch(anchor: anchor, factor: factor)
                }
                .onEnded { _ in
                    stretching = nil
                    factor = 1
                    tool.lanes.endDrag()
                }
        )
        .onChange(of: held) { _, now in
            guard !now else { return }
            DispatchQueue.main.async {
                guard stretching != nil else { return }
                stretching = nil
                factor = 1
                tool.lanes.cancelDrag()
            }
        }
        .onDisappear {
            // gone with the drag still on it: nothing more will end it
            guard stretching?.leading == leading else { return }
            stretching = nil
            factor = 1
            tool.lanes.cancelDrag()
        }
    }
}

/// What the header offers with keys picked in the lanes: nudging them a frame either way,
/// reversing and deleting them, and letting them go. How a clip's keys ease is the Keys page's
/// (its Easing…), so the header has no word for it: only with a key of the camera's or of a
/// sound's volume among them, which have no such page, does it offer how they ease.
struct KeyPickWords: View {
    let tool: TimelineTool

    var body: some View {
        let picks = tool.lanes.shown
        let frames = Set(picks.map(\.frame))
        let pageless = picks.contains { pick in
            if case .clip = pick.lane { false } else { true }
        }
        Text(verbatim: picks.count == 1 ? "1 key" : "\(picks.count) keys")
            .font(.system(size: 15))
            .foregroundStyle(inkDim)
            .lineLimit(1)
        BarWord(label: "◀︎") { tool.lanes.apply(.move(by: -1)) }
        BarWord(label: "▶︎") { tool.lanes.apply(.move(by: 1)) }
        if pageless {
            Menu {
                let easings: [(TimelineEasing, String)] = [
                    (.linear, "Linear"), (.smooth, "Smooth"), (.easeIn, "Ease In"), (.easeOut, "Ease Out"),
                    (.easeInOut, "Ease In and Out"), (.hold, "Hold"),
                ]
                ForEach(easings, id: \.1) { easing, label in
                    Button(label) { tool.lanes.apply(.ease(easing: easing)) }
                }
            } label: {
                Text(verbatim: "Easing")
                    .font(.system(size: 17))
                    .foregroundStyle(ink)
                    .frame(height: 30)
            }
        }
        BarWord(label: "Reverse", dim: frames.count < 2) {
            if frames.count > 1 {
                tool.lanes.apply(.reverse)
            }
        }
        BarWord(label: "Delete") { tool.lanes.apply(.remove) }
        Rectangle().fill(inkDim).frame(width: 1, height: 18)
        BarWord(label: "Done", bold: true) { tool.lanes.clear() }
    }
}

/// The Keys page's way to take away keys at the playhead: every channel's at once, or, with more
/// than one keyed there, one channel's.
struct RemoveKeysHere: View {
    let tool: TimelineTool
    let clip: UInt64
    let frame: UInt32
    /// Called before keys go, for the page to let go of a pose it holds.
    let removing: () -> Void

    var body: some View {
        let here = laneOrder.filter { c in (tool.keys[clip] ?? []).contains { $0.frame == frame && $0.channel == c } }
        if here.count > 1 {
            Menu {
                Button("Every Channel") {
                    removing()
                    tool.removeKeys(clip)
                }
                ForEach(here, id: \.self) { channel in
                    Button(laneName(channel)) {
                        removing()
                        tool.lanes.removeKey(of: channel, at: frame, on: clip)
                    }
                }
            } label: {
                Text(verbatim: "Remove Keys Here…")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                    .padding(.leading, 16)
                    .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
                    .contentShape(Rectangle())
            }
        } else {
            TlAction(label: "Remove Key Here") {
                removing()
                tool.removeKeys(clip)
            }
        }
    }
}

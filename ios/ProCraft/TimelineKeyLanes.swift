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

private var ink: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }
private var inkDim: Color { Look.shared.light ? Color(argb: 0x8039_3939) : Color(argb: 0x80C5_C5C5) }
private let rowLineFill = Color(argb: 0x09FF_FFFF)

/// The channels in the order the lanes show them, and their names.
let laneOrder: [TimelineChannel] = [.position, .scale, .rotation, .opacity, .adjustment]

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

    /// The tracks whose lanes show: the picked clip's, then each other track with keys picked on
    /// its clips, top first as the tracks stand.
    var keyLanesTracks: [TimelineTrackInfo] {
        tracks.reversed().filter { gridKeyTracks.contains($0.id) }
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


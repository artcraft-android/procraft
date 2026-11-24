import SwiftUI

// The grid timeline's sizes and rows (see TimelineGrid.swift). The grid zooms: a frame's cell and
// a row get smaller together as it is pinched out, then the frames alone go on narrowing until
// the whole timeline fits; pinched in past where it starts, the frames alone get wider. So
// nothing here is a fixed number of points: every view takes its sizes from `GridSizes`, and
// where a row stands from `GridRows`.

/// How big the grid's cells and rows are at a zoom (1: as it starts).
struct GridSizes: Equatable {
    /// A frame's cell and a row as the grid starts: from one frame to the next, the cell's own
    /// size, and a row's height.
    static let pitch: CGFloat = 44
    static let cellWidth: CGFloat = 40
    static let cellHeight: CGFloat = 31
    static let rowHeight: CGFloat = 44
    static let corner: CGFloat = 7
    /// The widest a frame gets, as a zoom; and how small a row gets (zoomed out further, the
    /// frames alone narrow).
    static let most: CGFloat = 2
    static let leastRows: CGFloat = 0.5
    /// The narrowest a frame ever is, in points.
    static let leastPitch: CGFloat = 0.75

    var zoom: CGFloat = 1

    /// From one frame's cell to the next.
    var pitch: CGFloat { Self.pitch * zoom }
    /// A row's height: smaller with the frames down to half, never taller than it starts.
    var rowHeight: CGFloat { (Self.rowHeight * min(max(zoom, Self.leastRows), 1)).rounded() }
    /// The space after a cell, narrowing with it.
    var gap: CGFloat { (Self.pitch - Self.cellWidth) * min(zoom, 1) }
    var cellWidth: CGFloat { pitch - gap }
    var cellHeight: CGFloat { (rowHeight * Self.cellHeight / Self.rowHeight).rounded() }
    var corner: CGFloat { min(Self.corner, cellWidth / 3, cellHeight / 4) }
    /// How much smaller than it starts a row's own furniture (a track's name) is drawn.
    var rowScale: CGFloat { rowHeight / Self.rowHeight }
    /// Whether a frame is wide enough to show its drawing.
    var pictures: Bool { cellWidth >= 20 }
}

/// A track's frames on a row, and the frames of the timeline it has: a track of the timeline's
/// own, all of them; a track inside a group, the frames the group lasts.
struct GridSegment {
    var track: TimelineTrackInfo
    var start: Int
    var end: Int
    /// The group clip whose own track it is (0: none), and every group it is inside, outermost
    /// first.
    var group: UInt64
    var groups: [UInt64]
}

/// One row of the grid, and where it stands among the rows. A track of the timeline's own has a
/// row to itself. The tracks inside groups share theirs: the expanded groups on one track have
/// their first tracks side by side on the first row under it, their second on the next, and so
/// on, each over the frames its group lasts.
struct GridRow: Identifiable {
    /// The tracks on it, in time order (one, but under a track holding several expanded groups).
    var segments: [GridSegment]
    /// How many groups its tracks are inside.
    var depth: Int
    /// A row of keys under its tracks' row, not of frames: one kind of key (moved, scaled…, or
    /// sounds' volume) of every block on each of the row's tracks that has that kind. Empty: a row
    /// of frames.
    var lanes: [KeyLaneRow] = []
    var lane: KeyLaneRow? { lanes.first }
    /// Its top edge among all the rows, and its height.
    var y: CGFloat = 0
    var height: CGFloat = 0

    var id: String {
        let track = segments.first?.track.id ?? 0
        guard let lane else { return "\(track)" }
        switch lane {
        case let .channel(_, channel): return "\(track)-\(channel)"
        case .volume: return "\(track)-volume"
        case .track: return "\(track)-name"
        }
    }

    /// The track whose frames `frame` is among; where it is among none's, the nearest's.
    func track(at frame: Int) -> TimelineTrackInfo {
        segment(at: frame).track
    }

    func segment(at frame: Int) -> GridSegment {
        if let s = segments.first(where: { frame >= $0.start && frame < $0.end }) {
            return s
        }
        let near = segments.min { a, b in
            min(abs(frame - a.start), abs(frame - a.end)) < min(abs(frame - b.start), abs(frame - b.end))
        }
        // (a row always has a track)
        return near ?? segments[0]
    }

    /// Whether `frame` is one of a track's on it (a row of keys has no frames).
    func has(_ frame: Int) -> Bool {
        lanes.isEmpty && segments.contains { frame >= $0.start && frame < $0.end }
    }
}

/// The grid's rows, top first, each where it stands.
struct GridRows {
    private(set) var list: [GridRow] = []
    /// How tall they all are together.
    private(set) var height: CGFloat = 0

    /// How tall a row of keys is before the grid is zoomed out.
    static let keysHeight: CGFloat = 28

    /// Rows for the panel's `rows` (top first: each track followed by the tracks of the groups on
    /// it that are expanded, a step deeper), one under the other. `keys` gives the rows of keys
    /// that are out under a track.
    init(rows: [TimelineRow], sizes: GridSizes, keys: (TimelineTrackInfo) -> [KeyLaneRow] = { _ in [] }) {
        var y: CGFloat = 0
        for row in Self.level(rows[...], depth: 0, groups: []) {
            var row = row
            row.y = y
            row.height = sizes.rowHeight
            y += row.height
            list.append(row)
            // its tracks' rows of keys: one row for each kind any of them has (the tracks of
            // several groups sharing the row share those too)
            let out = row.segments.flatMap { keys($0.track) }
            for kind in Self.kinds where out.contains(where: { Self.same($0, kind) }) {
                var under = GridRow(segments: row.segments, depth: row.depth, lanes: out.filter { Self.same($0, kind) })
                under.y = y
                under.height = max((Self.keysHeight * sizes.rowScale).rounded(), 16)
                y += under.height
                list.append(under)
            }
        }
        height = y
    }

    /// The kinds of row of keys, in the order they stand.
    private static let kinds: [KeyLaneRow] = laneOrder.map { .channel(0, $0) } + [.volume(0)]

    /// Whether two rows of keys are of one kind, whatever tracks they are of.
    private static func same(_ a: KeyLaneRow, _ b: KeyLaneRow) -> Bool {
        switch (a, b) {
        case let (.channel(_, x), .channel(_, y)): x == y
        case (.volume, .volume): true
        default: false
        }
    }

    private static func segment(_ r: TimelineRow, _ groups: [UInt64]) -> GridSegment {
        GridSegment(track: r.track, start: Int(r.start), end: Int(clamping: r.end), group: r.group, groups: groups)
    }

    /// The row at the start of `rows`, with the rows deeper than it that follow it: its groups'.
    private static func cut(_ rows: ArraySlice<TimelineRow>) -> (row: TimelineRow, inside: ArraySlice<TimelineRow>, rest: ArraySlice<TimelineRow>)? {
        guard let first = rows.first else { return nil }
        var end = rows.startIndex + 1
        while end < rows.endIndex, rows[end].depth > first.depth {
            end += 1
        }
        return (first, rows[(rows.startIndex + 1) ..< end], rows[end...])
    }

    /// The rows of one level's tracks, each with its own row, and the rows of its groups under it.
    private static func level(_ rows: ArraySlice<TimelineRow>, depth: Int, groups: [UInt64]) -> [GridRow] {
        var out: [GridRow] = []
        var left = rows
        while let (row, inside, rest) = cut(left) {
            out.append(GridRow(segments: [segment(row, groups)], depth: depth))
            out += shared(inside, depth: depth + 1, groups: groups)
            left = rest
        }
        return out
    }

    /// The rows of the tracks of the groups on one track: the groups' first tracks on one row,
    /// their second on the next, each followed by the rows of the groups on those tracks.
    private static func shared(_ rows: ArraySlice<TimelineRow>, depth: Int, groups: [UInt64]) -> [GridRow] {
        var byGroup: [(id: UInt64, tracks: [(row: TimelineRow, inside: ArraySlice<TimelineRow>)])] = []
        var left = rows
        while let (row, inside, rest) = cut(left) {
            if let i = byGroup.firstIndex(where: { $0.id == row.group }) {
                byGroup[i].tracks.append((row, inside))
            } else {
                byGroup.append((row.group, [(row, inside)]))
            }
            left = rest
        }
        var out: [GridRow] = []
        for k in 0 ..< (byGroup.map(\.tracks.count).max() ?? 0) {
            let there = byGroup.filter { k < $0.tracks.count }
            let segments = there.map { segment($0.tracks[k].row, groups + [$0.id]) }.sorted { $0.start < $1.start }
            out.append(GridRow(segments: segments, depth: depth))
            for g in there {
                out += shared(g.tracks[k].inside, depth: depth + 1, groups: groups + [g.id])
            }
        }
        return out
    }

    var count: Int { list.count }
    var isEmpty: Bool { list.isEmpty }

    subscript(_ index: Int) -> GridRow { list[index] }

    /// The row at height `y` of all the rows; none off them.
    func index(at y: CGFloat) -> Int? {
        guard y >= 0 else { return nil }
        return list.firstIndex { y >= $0.y && y < $0.y + $0.height }
    }

    /// The row track `id` is on.
    func index(of id: UInt64?) -> Int? {
        guard let id else { return nil }
        return list.firstIndex { $0.lanes.isEmpty && $0.segments.contains { $0.track.id == id } }
    }

    /// Every track on the rows, with its row, top first.
    var tracks: [(row: Int, track: TimelineTrackInfo)] {
        list.enumerated().flatMap { r, row in row.lanes.isEmpty ? row.segments.map { (r, $0.track) } : [] }
    }

    /// The bottom of the last row that has a track inside group `id`: how far down the group's
    /// own rows go. None where it has none showing.
    func bottom(ofGroup id: UInt64) -> CGFloat? {
        list.last { $0.segments.contains { $0.groups.contains(id) } }.map { $0.y + $0.height }
    }
}

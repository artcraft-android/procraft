import SwiftUI
import UIKit

// The grid timeline's cells (see TimelineGrid.swift): every frame's empty slot, each clip as a
// block over the frames it lasts, and the handles on the frame picked (or on the last of the
// frames picked together) that are dragged to make it last longer or shorter.

/// The rows' cells in the frames' own space: frame `f`'s cell starts `f` pitches along, so the
/// whole of it slides by one offset as the frames are dragged.
struct GridCells: View {
    let tool: TimelineTool
    let info: TimelineInfo
    let grid: GridState
    /// The tracks' rows, top first.
    let rows: GridRows
    /// The frames built: those on the bar and some either side.
    let lo: Int
    let hi: Int

    private var sz: GridSizes { grid.sizes }

    /// How far down its row a cell starts.
    private func top(_ row: GridRow) -> CGFloat {
        (row.height - sz.cellHeight) / 2
    }

    var body: some View {
        let sz = self.sz
        ZStack(alignment: .topLeading) {
            // every frame's slot, empty
            Canvas { ctx, _ in
                var slots = Path()
                let corner = CGSize(width: sz.corner, height: sz.corner)
                for r in rows.list where r.lanes.isEmpty {
                    // (a track inside a group has slots only for the frames the group lasts)
                    for s in r.segments {
                        let first = max(s.start, lo)
                        let last = min(s.end - 1, hi)
                        guard first <= last else { continue }
                        // (too narrow to tell one from the next, they are drawn as one strip)
                        guard sz.pitch >= 5 else {
                            slots.addRect(CGRect(
                                x: CGFloat(first - lo) * sz.pitch, y: r.y + top(r), width: CGFloat(last - first + 1) * sz.pitch, height: sz.cellHeight
                            ))
                            continue
                        }
                        for f in first ... last {
                            slots.addRoundedRect(
                                in: CGRect(x: CGFloat(f - lo) * sz.pitch, y: r.y + top(r), width: sz.cellWidth, height: sz.cellHeight),
                                cornerSize: corner
                            )
                        }
                    }
                }
                ctx.fill(slots, with: .color(Grid.slot))
                // where a repeating group plays a frame again: its ghost
                var ghosts = Path()
                for r in rows.list where r.lanes.isEmpty {
                    for s in r.segments {
                        for again in tool.place(s.track.id)?.repeats ?? [] where Int(again.start) <= hi && Int(again.start + again.duration) > lo {
                            ghosts.addRoundedRect(
                                in: CGRect(
                                    x: CGFloat(Int(again.start) - lo) * sz.pitch, y: r.y + top(r),
                                    width: CGFloat(again.duration) * sz.pitch - sz.gap, height: sz.cellHeight
                                ),
                                cornerSize: corner
                            )
                        }
                    }
                }
                ctx.fill(ghosts, with: .color(Grid.block.opacity(0.45)))
            }
            .frame(width: CGFloat(hi - lo + 1) * sz.pitch, height: max(rows.height, sz.rowHeight))
            .offset(x: CGFloat(lo) * sz.pitch)
            .allowsHitTesting(false)
            ForEach(rows.list.filter { $0.lanes.isEmpty }) { r in
                ForEach(r.segments, id: \.track.id) { s in
                    ForEach(shown(s.track).filter { Int($0.start) <= hi && Int($0.start + $0.duration) > lo }, id: \.id) { c in
                        // an expanded group: an outline round it and its own tracks' rows
                        if c.kind == .group, let bottom = rows.bottom(ofGroup: c.id) {
                            let place = span(c)
                            let lit = tool.selecting == nil && tool.picked == c.id
                            RoundedRectangle(cornerRadius: sz.corner + 3, style: .continuous)
                                .strokeBorder(lit ? Pc.accent : Grid.box.opacity(0.7), lineWidth: lit ? 2 : 1.2)
                                .frame(width: place.width + 6, height: bottom - r.y - top(r) + 3)
                                .offset(x: place.x - 3, y: r.y + top(r) - 3)
                                .allowsHitTesting(false)
                        }
                        block(c, on: r, track: s.track)
                    }
                }
            }
            // the rows of keys, and the marks under blocks' keyed frames
            GridKeyRows(tool: tool, rows: rows, sizes: sz, lo: lo, hi: hi)
                .zIndex(1.5)
            handles
        }
    }

    private var edges: GridEdges { GridEdges(tool: tool, grid: grid, rows: rows) }

    private func shown(_ t: TimelineTrackInfo) -> [TimelineClipInfo] {
        edges.shown(t)
    }

    private func span(_ c: TimelineClipInfo) -> (x: CGFloat, width: CGFloat) {
        edges.span(c)
    }

    @ViewBuilder
    private func block(_ c: TimelineClipInfo, on row: GridRow, track t: TimelineTrackInfo) -> some View {
        let sz = self.sz
        let place = span(c)
        let chosen = tool.selecting?.contains(c.id) == true
        let picked = tool.selecting == nil && tool.picked == c.id
        // held for its menu: lifted a little off the bar
        let lifted = tool.selecting == nil && tool.clipMenu?.clip == c.id
        let dragged = grid.trim?.clip == c.id
        // dragged somewhere else: lifted off the bar where it would land (faint where it is,
        // while it can't be dropped or is over the line between two rows)
        let moving = grid.move?.ids.contains(c.id) == true
        let nowhere = moving && (grid.move?.valid != true || grid.move?.between != nil)
        // the frames of it on the bar
        let from = max(lo - Int(c.start), 0)
        let to = min(hi - Int(c.start), Int(((place.width + sz.gap) / sz.pitch).rounded(.up)) - 1)
        let shape = RoundedRectangle(cornerRadius: sz.corner, style: .continuous)
        Group {
            if c.kind == .audio || c.kind == .video {
                GridMediaBlock(
                    tool: tool, grid: grid, clip: c, fps: info.fps, sizes: sz, tint: c.label.map { Color(rgb: $0) }, from: from, to: to
                )
            } else if c.kind == .group {
                GridGroupBlock(
                    clip: c, open: tool.unfolded(c.id), shots: tool.groupShots[c.id] ?? [], pictures: tool.thumbnails, paper: tool.paper,
                    tint: c.label.map { Color(rgb: $0) }, fps: info.fps, from: from, to: to, sizes: sz
                )
            } else {
                GridFrame(
                    clip: c, picture: c.root != 0 ? tool.thumbnails[c.root] : nil, paper: tool.paper,
                    tint: c.label.map { Color(rgb: $0) }, from: from, to: to, sizes: sz
                )
            }
        }
        .frame(width: place.width, height: sz.cellHeight)
        .clipShape(shape)
        .overlay {
            if chosen || picked || moving {
                RoundedRectangle(cornerRadius: sz.corner + 2, style: .continuous)
                    .strokeBorder(Pc.accent, lineWidth: 2.5)
                    .padding(-2.5)
            }
        }
        // touched by the stroke under way: glowing
        .overlay {
            if chosen, !grid.stroke.isEmpty {
                shape.fill(Pc.accent.opacity(0.28))
            }
        }
        // (a little bigger all round, however long it is)
        .scaleEffect(x: lifted || moving ? 1 + 5 / max(place.width, 1) : 1, y: lifted || moving ? 1.1 : 1)
        .shadow(color: lifted || moving ? .black.opacity(0.5) : .clear, radius: 6, y: 2)
        .opacity(nowhere ? 0.4 : t.hidden || tool.place(t.id)?.hiddenAbove == true ? 0.45 : c.disabled ? 0.4 : 1)
        .offset(x: place.x, y: row.y + top(row))
        .zIndex(moving ? 3 : lifted || picked || dragged ? 1 : 0)
        // the frames an edge pushes along slide to their new places; the one dragged follows
        // the finger as it is
        .animation(dragged ? nil : .easeOut(duration: 0.14), value: c.start)
        .animation(dragged ? nil : .easeOut(duration: 0.14), value: c.duration)
        .allowsHitTesting(false)
    }

    // MARK: Handles

    /// The handles on the picked frame's edges (or on the last of the frames picked together):
    /// pictures only, the grid's own touch taking the drags (see `GridEdges.handle(at:)`).
    @ViewBuilder
    private var handles: some View {
        if let h = edges.handled, !tool.isPlaying {
            let place = span(h.clip)
            let row = rows[h.row]
            let y = row.y
            // while one is dragged, only it shows
            let leading = grid.trim?.leading ?? grid.groupTrim?.leading
            // a cycling group's start stays put
            if leading != false, h.clip.cycle == 0 {
                GridHandle(leading: true, cellHeight: sz.cellHeight, rowHeight: row.height)
                    .offset(x: place.x - GridHandle.width + GridHandle.overlap, y: y)
                    .zIndex(2)
            }
            if leading != true {
                GridHandle(leading: false, cellHeight: sz.cellHeight, rowHeight: row.height)
                    .offset(x: place.x + place.width - GridHandle.overlap, y: y)
                    .zIndex(2)
            }
            // a repeating group's end dragged: how many times it plays now
            if let tr = grid.trim, tr.cycle > 0, let now = tr.clips.first(where: { $0.id == tr.clip }) {
                Text(verbatim: CycleDrag(clip: tr.clip, from: 0, frames: Int(now.duration), most: 0, length: tr.cycle).tag)
                    .font(.system(size: 12, weight: .semibold).monospacedDigit())
                    .foregroundStyle(Color.white)
                    .padding(.horizontal, 7)
                    .frame(height: 20)
                    .background(Capsule().fill(Pc.accent))
                    .fixedSize()
                    .offset(x: place.x + place.width + 14, y: y + (row.height - 20) / 2)
                    .zIndex(3)
            }
        }
    }
}

/// The frames' edges as they show and as they are dragged: which clip has handles, where they
/// are, and what dragging one does.
struct GridEdges {
    let tool: TimelineTool
    let grid: GridState
    /// The tracks' rows, top first.
    let rows: GridRows

    private var sz: GridSizes { grid.sizes }
    private var gap: CGFloat { sz.gap }
    private var pitch: CGFloat { sz.pitch }

    /// Track `t`'s clips as they show: as an edge being dragged would leave them, else as they are.
    func shown(_ t: TimelineTrackInfo) -> [TimelineClipInfo] {
        // (a block dragged elsewhere: the tracks as dropping it would leave them)
        if let clips = grid.move?.shown[t.id] {
            return clips
        }
        if let tr = grid.trim, tr.track == t.id, !tr.clips.isEmpty {
            return tr.clips
        }
        return grid.groupTrim?.tracks[t.id] ?? t.clips
    }

    /// Where clip `c` shows: its left edge and width in the frames' space. An edge of it being
    /// dragged follows the finger between frames, as far as the drag can take it.
    func span(_ c: TimelineClipInfo) -> (x: CGFloat, width: CGFloat) {
        var x0 = CGFloat(c.start) * pitch
        var x1 = CGFloat(c.start + c.duration) * pitch
        if let tr = grid.trim, tr.clip == c.id {
            let raw = CGFloat(tr.from) * pitch + tr.dx
            // within half a frame of the frame it is on (held at a limit, it stays on the frame it
            // got to), and never shorter than a frame
            let near = min(max(raw, (CGFloat(tr.frame) - 0.5) * pitch), (CGFloat(tr.frame) + 0.5) * pitch)
            if tr.leading {
                if Int(c.start) == tr.frame {
                    x0 = max(min(near, x1 - pitch), 0)
                } else if raw < x0, Int(c.start + c.duration) > tr.end {
                    // the frames before it have no more room: it grows at its end by as much as
                    // its start falls short of the finger
                    let wanted = CGFloat(tr.end) * pitch + (x0 - raw)
                    x1 = min(max(wanted, x1 - pitch / 2), x1 + pitch / 2)
                }
            } else if Int(c.start + c.duration) == tr.frame {
                x1 = max(near, x0 + pitch)
            }
        }
        return (x0, max(x1 - x0 - gap, min(8, sz.cellWidth)))
    }

    /// The clip with handles on its edges, its row and whether it stands for the frames picked
    /// together: with the lasso on, the last of them in time (the topmost of those ending
    /// together); else the picked one.
    var handled: (clip: TimelineClipInfo, track: TimelineTrackInfo, row: Int, group: Bool)? {
        // (none while a block is dragged elsewhere)
        guard grid.move == nil else { return nil }
        if let chosen = tool.selecting {
            guard !chosen.isEmpty, grid.stroke.isEmpty else { return nil }
            var best: (TimelineClipInfo, TimelineTrackInfo, Int)?
            for (r, t) in rows.tracks where !tool.held(t) {
                for c in shown(t) where chosen.contains(c.id) {
                    if best.map({ c.start + c.duration > $0.0.start + $0.0.duration }) ?? true {
                        best = (c, t, r)
                    }
                }
            }
            return best.map { ($0.0, $0.1, $0.2, true) }
        }
        guard let id = tool.picked else { return nil }
        for (r, t) in rows.tracks where !tool.held(t) {
            if let c = shown(t).first(where: { $0.id == id }) {
                return (c, t, r, false)
            }
        }
        return nil
    }

    /// Which handle is at `point` of the frames' space, if one is: the start's (true) or the
    /// end's. A handle takes a finger's width beyond the edge it is on.
    func handle(at point: CGPoint) -> Bool? {
        guard let h = handled, !tool.isPlaying else { return nil }
        let y = rows[h.row].y
        guard point.y >= y, point.y < y + rows[h.row].height else { return nil }
        let place = span(h.clip)
        let reach = GridHandle.width - GridHandle.overlap
        // on a frame too narrow for both, each keeps its own half
        let inside = min(GridHandle.overlap + 4, place.width / 2)
        if point.x >= place.x + place.width - inside, point.x <= place.x + place.width + reach {
            return false
        }
        if h.clip.cycle == 0, point.x >= place.x - reach, point.x <= place.x + inside {
            return true
        }
        return nil
    }

    /// The handle on the start (`leading`) or the end dragged `dx` from where it was.
    func dragged(leading: Bool, dx: CGFloat) {
        // (the drag under way keeps the edge it began on)
        if let tr = grid.trim {
            edgeDragged(clip: tr.clip, track: tr.track, from: tr.from, end: tr.end, leading: tr.leading, dx: dx)
        } else if let g = grid.groupTrim {
            groupDragged(clip: g.clip, steps: g.steps, from: g.from, leading: g.leading, dx: dx)
        } else if let h = handled {
            let from = Int(leading ? h.clip.start : h.clip.start + h.clip.duration)
            if h.group {
                // an end goes as far as every picked frame before it on its track grows, and its own
                let chosen = tool.selecting ?? []
                let before = h.track.clips.filter { chosen.contains($0.id) && $0.start <= h.clip.start }.count
                groupDragged(clip: h.clip.id, steps: leading ? 1 : max(before, 1), from: from, leading: leading, dx: dx)
            } else {
                edgeDragged(clip: h.clip.id, track: h.track.id, from: from, end: Int(h.clip.start + h.clip.duration), leading: leading, dx: dx)
            }
        }
    }

    /// The picked frame's edge dragged `dx` from where it was: the frame asked for, its track as
    /// that would leave it, and the limit holding it short of there if one does.
    private func edgeDragged(clip: UInt64, track: UInt64, from: Int, end: Int, leading: Bool, dx: CGFloat) {
        let first = grid.trim == nil
        if first {
            tool.stopPlaying()
            tool.edgeDragging = true
            grid.closeMenus(tool)
        }
        var tr = grid.trim ?? GridTrim(clip: clip, track: track, leading: leading, from: from, end: end, frame: from, dx: 0)
        tr.dx = dx
        // a repeating group's end: how long it plays, up to the next block on its track
        if first, !leading, let c = tool.clip(clip)?.1, c.cycle > 0 {
            tr.cycle = Int(c.cycle)
            tr.most = tool.groupRoom(clip) ?? Int(c.duration)
        }
        if tr.cycle > 0 {
            cycleDragged(&tr, dx: dx, first: first)
            grid.trim = tr
            return
        }
        let raw = CGFloat(tr.from) + dx / pitch
        // (a start may be asked before the first frame: what it can't have there goes on its end)
        let asked = leading ? Int(raw.rounded()) : max(Int(raw.rounded()), 0)
        let before = shown(of: tr)
        if asked != tr.frame || first {
            tr.frame = asked
            tr.clips = asked == tr.from ? [] : tool.gridTrimPreview(tr.clip, leading: leading, to: asked)
        }
        // where the edge got to: short of what was asked, something holds it there
        let got = edge(of: tr)
        if shown(of: tr) != before {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        let held = abs(raw - CGFloat(got)) > 0.5
        let limit: CGFloat? = !held ? nil : limitLine(got, leading: leading)
        if limit != nil, tr.limit == nil {
            UIImpactFeedbackGenerator(style: .rigid).impactOccurred()
        }
        tr.limit = limit
        grid.trim = tr
    }

    /// A repeating group's end dragged `dx`: it plays as many frames, a tick as it passes each
    /// whole cycle, no further than there is room for.
    private func cycleDragged(_ tr: inout GridTrim, dx: CGFloat, first: Bool) {
        guard let (t, c) = tool.clip(tr.clip) else { return }
        let start = Int(c.start)
        let asked = Int((CGFloat(tr.from) + dx / pitch).rounded())
        let frames = min(max(asked - start, 1), max(tr.most, 1))
        let before = tr.frame
        tr.frame = start + frames
        tr.clips = t.clips.map { x in
            guard x.id == c.id else { return x }
            var x = x
            x.duration = UInt32(frames)
            return x
        }
        let held = asked - start > tr.most
        if held, tr.limit == nil {
            UIImpactFeedbackGenerator(style: .rigid).impactOccurred()
        } else if tr.frame != before || first {
            // (firmer where a whole cycle ends)
            if frames % tr.cycle == 0 {
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
            } else {
                UISelectionFeedbackGenerator().selectionChanged()
            }
        }
        tr.limit = held ? limitLine(tr.frame, leading: false) : nil
    }

    /// Where the line of an edge held on frame `got` stands, in frames: a start's just before it;
    /// an end's (a frame can be no shorter than one) through the frame it ends on.
    private func limitLine(_ got: Int, leading: Bool) -> CGFloat {
        leading ? CGFloat(got) - gap / 2 / pitch : CGFloat(got) - (pitch - sz.cellWidth / 2) / pitch
    }

    /// The frames the dragged clip starts and ends on, as the drag has left it so far.
    private func shown(of tr: GridTrim) -> [Int] {
        guard let c = tr.clips.first(where: { $0.id == tr.clip }) else { return [] }
        return [Int(c.start), Int(c.start + c.duration)]
    }

    /// The frame the dragged edge is on, as the drag has left it so far.
    private func edge(of tr: GridTrim) -> Int {
        guard let c = tr.clips.first(where: { $0.id == tr.clip }) else { return tr.from }
        return Int(tr.leading ? c.start : c.start + c.duration)
    }

    /// The handle of the frames picked together dragged `dx`: every one of them a frame longer
    /// (or shorter) for each `steps` columns it goes, the tracks as that would leave them.
    private func groupDragged(clip: UInt64, steps: Int, from: Int, leading: Bool, dx: CGFloat) {
        if grid.groupTrim == nil {
            tool.stopPlaying()
            tool.edgeDragging = true
            grid.closeMenus(tool)
        }
        var g = grid.groupTrim ?? GridGroupTrim(clip: clip, leading: leading, from: from, steps: steps, by: 0)
        let by = Int((dx / (pitch * CGFloat(g.steps))).rounded())
        if by != g.by {
            g.by = by
            g.tracks = by == 0 ? [:] : tool.gridTrimsPreview(leading: leading, by: by)
            let got = g.tracks.values.joined().first { $0.id == g.clip }.map { Int(leading ? $0.start : $0.start + $0.duration) } ?? g.from
            // short of where it was asked to go: held there
            let held = got != g.from + by * g.steps
            if held, g.limit == nil {
                UIImpactFeedbackGenerator(style: .rigid).impactOccurred()
            } else if !held {
                UISelectionFeedbackGenerator().selectionChanged()
            }
            g.limit = held ? limitLine(got, leading: leading) : nil
        }
        grid.groupTrim = g
    }

    /// The handle let go: the edge stays where the drag left it, as one undo step.
    func ended() {
        tool.endEdgeDrag()
        if let tr = grid.trim {
            grid.trim = nil
            if tr.cycle > 0 {
                if tr.frame != tr.from, let c = tool.clip(tr.clip)?.1 {
                    tool.setGroupLength(tr.clip, tr.frame - Int(c.start))
                }
            } else if tr.frame != tr.from, !tr.clips.isEmpty {
                tool.gridTrim(tr.clip, leading: tr.leading, to: tr.frame)
            }
        }
        if let g = grid.groupTrim {
            grid.groupTrim = nil
            if g.by != 0, !g.tracks.isEmpty {
                tool.gridTrimSelected(leading: g.leading, by: g.by)
            }
        }
    }
}

/// A clip over the frames it lasts. A drawing shows its picture on the canvas's paper in each
/// frame, the first as it is and the held ones shaded; a group, a sound or a video is a plain
/// block with its name.
private struct GridFrame: View {
    let clip: TimelineClipInfo
    let picture: UIImage?
    let paper: Color
    /// Its colour label, if it has one: a block's fill.
    let tint: Color?
    /// The frames of it drawn, counted from its first: those on the bar.
    let from: Int
    let to: Int
    let sizes: GridSizes

    var body: some View {
        let pitch = sizes.pitch
        let cell = sizes.cellWidth
        if clip.kind == .drawing {
            Canvas { ctx, size in
                ctx.fill(Path(CGRect(origin: .zero, size: size)), with: .color(paper))
                if let picture, sizes.pictures, from <= to {
                    let image = ctx.resolve(Image(uiImage: picture))
                    let scale = min(cell / max(image.size.width, 1), size.height / max(image.size.height, 1))
                    let w = image.size.width * scale
                    let h = image.size.height * scale
                    for f in from ... to {
                        ctx.draw(image, in: CGRect(x: CGFloat(f) * pitch + (cell - w) / 2, y: (size.height - h) / 2, width: w, height: h))
                    }
                }
                // held past its first frame
                if size.width > cell + 1 {
                    ctx.fill(Path(CGRect(x: cell, y: 0, width: size.width - cell, height: size.height)), with: .color(Grid.heldShade))
                }
            }
        } else {
            ZStack(alignment: .leading) {
                tint ?? Grid.block
                HStack(spacing: 5) {
                    PcIconView(clip.kind == .group ? PcIcons.layers : PcIcons.gridOptions, size: 15, color: Grid.inkBright)
                    Text(verbatim: clip.name)
                        .font(.system(size: 12))
                        .foregroundStyle(Grid.inkBright)
                        .lineLimit(1)
                }
                .padding(.horizontal, 7)
                // (its name follows the bar's edge no further than its own first frame)
                .offset(x: CGFloat(from) * pitch)
            }
        }
    }
}

/// A handle on a frame's edge: a tab in the accent with an arrowhead. The grid's touch drags it.
private struct GridHandle: View {
    /// How wide it reaches for the finger, and how much of that lies over the frame's own edge.
    static let width: CGFloat = 28
    static let overlap: CGFloat = 6

    let leading: Bool
    let cellHeight: CGFloat
    let rowHeight: CGFloat

    var body: some View {
        ZStack {
            RoundedRectangle(cornerRadius: min(6, cellHeight / 4), style: .continuous)
                .fill(Pc.accent)
                .frame(width: 16, height: cellHeight - 3)
            PcIconView(leading ? PcIcons.chevronLeft : PcIcons.chevronRight, size: 14, color: .white)
        }
        // against the frame's edge
        .frame(width: Self.width, height: rowHeight, alignment: leading ? .trailing : .leading)
        .allowsHitTesting(false)
    }
}

/// A group over the frames it lasts. Collapsed, each of its frames shows everything in it there,
/// one drawing over another; expanded (its own tracks' rows showing under it), it is a plain
/// strip heading them. Either way its name, and a chevron that turns it over.
struct GridGroupBlock: View {
    let clip: TimelineClipInfo
    let open: Bool
    /// What it shows on each of its frames (a collapsed group's: see `TimelineGroupShot`).
    let shots: [TimelineGroupShot]
    let pictures: [UInt64: UIImage]
    let paper: Color
    let tint: Color?
    let fps: UInt32
    /// The frames of it drawn, counted from its first: those on the bar.
    let from: Int
    let to: Int
    let sizes: GridSizes

    var body: some View {
        let pitch = sizes.pitch
        let cell = sizes.cellWidth
        // (a video inside it: drawn again as its pictures come in)
        let videos = GridVideoPictures.shared
        let _ = videos.read
        Canvas { ctx, size in
            let merged = !open && sizes.pictures && !shots.isEmpty
            ctx.fill(Path(CGRect(origin: .zero, size: size)), with: .color(merged ? paper : tint ?? Grid.block))
            if merged, from <= to {
                for shot in shots {
                    let first = max(Int(shot.start) - Int(clip.start), from)
                    let last = min(Int(shot.start + shot.frames) - Int(clip.start) - 1, to)
                    guard first <= last else { continue }
                    // everything it shows there, bottom first: a drawing's picture, a video's at
                    // the time each frame shows
                    for item in shot.items {
                        for f in first ... last {
                            let picture: UIImage?
                            if item.root != 0 {
                                picture = pictures[item.root]
                            } else if !item.path.isEmpty {
                                let into = Double(f + Int(clip.start) - Int(shot.start)) / Double(max(fps, 1))
                                picture = videos.picture(item.path, at: item.seconds + into, within: 0.2)
                            } else {
                                picture = nil
                            }
                            guard let picture else { continue }
                            let image = ctx.resolve(Image(uiImage: picture))
                            let scale = min(cell / max(image.size.width, 1), size.height / max(image.size.height, 1))
                            let (w, h) = (image.size.width * scale, image.size.height * scale)
                            ctx.draw(image, in: CGRect(x: CGFloat(f) * pitch + (cell - w) / 2, y: (size.height - h) / 2, width: w, height: h))
                        }
                    }
                }
                if let tint {
                    ctx.fill(Path(CGRect(origin: .zero, size: size)), with: .color(tint.opacity(0.3)))
                }
            }
            // where it plays its frames again: a notch at each repeat
            if clip.cycle > 0 {
                var notches = Path()
                var f = Int(clip.cycle)
                while f < Int(clip.duration) {
                    notches.addRect(CGRect(x: CGFloat(f) * pitch - (pitch - cell) / 2 - 0.75, y: 0, width: 1.5, height: size.height))
                    f += Int(clip.cycle)
                }
                ctx.fill(notches, with: .color(Pc.accent.opacity(0.85)))
            }
        }
        .overlay(alignment: .topLeading) {
            if sizes.rowScale > 0.7 {
                HStack(spacing: 3) {
                    PcIconView(open ? PcIcons.chevronDown : PcIcons.chevronRight, size: 11, color: .white)
                    Text(verbatim: clip.name)
                        .font(.system(size: 9.5, weight: .semibold))
                        .foregroundStyle(Color.white)
                        .lineLimit(1)
                }
                .padding(.horizontal, 5)
                .frame(height: 15)
                .background(Capsule().fill(Color.black.opacity(0.55)))
                .padding(.leading, 3)
                .padding(.top, 2)
                .offset(x: CGFloat(from) * pitch)
            }
        }
    }
}

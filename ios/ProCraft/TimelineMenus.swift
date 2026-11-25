import SwiftUI
import UIKit

// The timeline panel's clip and track pages. A clip's opacity, blend mode, colour label and name,
// whether it shows, and playing round its frames; a track's page (held on its name): its opacity,
// showing it alone, locking, renaming, moving and deleting it; a track's name held and dragged up
// or down to move it among the others. Also the looped frames shown on the ruler, and the onion
// skins either side set apart in Settings.

/// A track dragged up or down by its name: which, and how far (points; down is positive).
struct TrackDrag: Equatable {
    var track: UInt64
    var dy: CGFloat
}

/// The colours onion skins are screened with (black leaves them as they are).
let onionColors: [(String, UInt32)] = [
    ("Red", 0xE63333), ("Orange", 0xE68A1A), ("Green", 0x339933), ("Blue", 0x3366E6), ("Purple", 0x9933CC), ("None", 0),
]

extension Color {
    /// An opaque colour from 0xRRGGBB.
    init(rgb: UInt32) {
        self.init(argb: 0xFF00_0000 | (rgb & 0xFF_FFFF))
    }
}

extension TimelineTool {
    private var session: CanvasSession { canvasModel.session }

    /// Clip `id`'s own opacity (with autokey on, keyed at the playhead): shown at once, one undo
    /// step when `done`.
    func setClipOpacity(_ id: UInt64, _ opacity: Float, done: Bool) {
        if autokeyOpacity(id, opacity, done: done) {
            return
        }
        _ = session.setTimelineClipOpacity(id: id, opacity: opacity, done: done)
        if done {
            canvasModel.refresh()
        }
    }

    func setClipBlend(_ id: UInt64, _ blend: UInt32) {
        _ = session.setTimelineClipBlend(id: id, blend: blend)
        canvasModel.refresh()
    }

    func setClipDisabled(_ id: UInt64, _ disabled: Bool) {
        _ = session.setTimelineClipDisabled(id: id, disabled: disabled)
        canvasModel.refresh()
    }

    /// Whether every one of clips `ids` is turned off.
    func allDisabled(_ ids: [UInt64]) -> Bool {
        !ids.isEmpty && ids.allSatisfy { clip($0)?.1.disabled == true }
    }

    /// Clips `ids` (those picked together) all turned off, or all on again: one undo step.
    func setClipsDisabled(_ ids: [UInt64], _ disabled: Bool) {
        if !session.setTimelineClipsDisabled(ids: ids, disabled: disabled) {
            canvasModel.showNotice("They are on locked tracks.", warning: true)
        }
        canvasModel.refresh()
    }

    func setClipLabel(_ id: UInt64, _ label: UInt32?) {
        _ = session.setTimelineClipLabel(id: id, label: label)
        canvasModel.refresh()
    }

    func renameClip(_ id: UInt64, _ name: String) {
        _ = session.renameTimelineClip(id: id, name: name)
        canvasModel.refresh()
    }

    /// Track `id`'s opacity: shown at once, one undo step when `done`.
    func setTrackOpacity(_ id: UInt64, _ opacity: Float, done: Bool) {
        _ = session.setTimelineTrackOpacity(id: id, opacity: opacity, done: done)
        if done {
            canvasModel.refresh()
        }
    }

    func renameTrack(_ id: UInt64, _ name: String) {
        _ = session.renameTimelineTrack(id: id, name: name)
        canvasModel.refresh()
    }

    /// Track `id` moved to place `to` among the tracks shown (bottom first).
    func moveTrack(_ id: UInt64, to: Int) {
        if session.moveTimelineTrack(id: id, to: UInt32(max(to, 0))) {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        canvasModel.refresh()
    }

    /// Track `id` shown alone, or, shown alone already, the tracks back as they were.
    func solo(_ id: UInt64) {
        stopPlaying()
        if session.soloTimelineTrack(id: id) {
            UIImpactFeedbackGenerator(style: .medium).impactOccurred()
        }
        canvasModel.refresh()
    }

    /// The onion skins either side changed (not an undo step, as the other settings).
    func setOnion(_ edit: (inout TimelineOnion) -> Void) {
        guard var o = onion else { return }
        edit(&o)
        session.setTimelineOnion(onion: o)
        refresh()
    }

}

// MARK: - The clip page's rows

// MARK: - The track page

/// A track's name held opens its page; dragged on from there, it moves up or down among the
/// others, an accent line where it will go, and lands there when let go.
struct TrackHeaderGestures: ViewModifier {
    let tool: TimelineTool
    let track: TimelineTrackInfo
    let rowHeight: CGFloat

    @State private var held = false
    /// The page was opened by the hold under way: the lift that ends it counts as a tap too (a
    /// tap has no time limit), and must not close the page it just opened.
    @State private var openedByHold = false

    func body(content: Content) -> some View {
        // rows top first
        let rows = tool.tracks.reversed().map(\.id)
        let row = rows.firstIndex(of: track.id) ?? 0
        let drag = tool.trackDrag
        let mine = drag?.track == track.id
        // the row the dragged track would go to, and which side of it the line shows
        let from = drag.flatMap { d in rows.firstIndex(of: d.track) }
        let target = drag.flatMap { d in from.map { clampedRow($0, dy: d.dy, count: rows.count) } }
        let lineHere = !mine && target == row && from != row
        content
            .overlay(alignment: (from ?? 0) > row ? .top : .bottom) {
                if lineHere {
                    Pc.accent.frame(height: 3)
                }
            }
            .offset(y: mine ? drag?.dy ?? 0 : 0)
            .scaleEffect(mine ? 1.03 : 1)
            .shadow(color: mine ? .black.opacity(0.4) : .clear, radius: 6, y: 2)
            .zIndex(mine ? 1 : 0)
            .simultaneousGesture(TapGesture().onEnded {
                if openedByHold {
                    openedByHold = false
                    return
                }
                tool.trackOpen = nil
            })
            .gesture(
                LongPressGesture(minimumDuration: 0.35)
                    .sequenced(before: DragGesture(minimumDistance: 0, coordinateSpace: .global))
                    .onChanged { value in
                        guard case .second(true, let moved) = value, !tool.isPlaying else { return }
                        if !held {
                            held = true
                            tool.settingsOpen = false
                            tool.trackOpen = track.id
                            openedByHold = true
                            UIImpactFeedbackGenerator(style: .light).impactOccurred()
                        }
                        guard let moved, rows.count > 1 else { return }
                        let dy = moved.translation.height
                        if tool.trackDrag != nil || abs(dy) >= 4 {
                            tool.trackOpen = nil
                            let before = tool.trackDrag.map { clampedRow(row, dy: $0.dy, count: rows.count) }
                            tool.trackDrag = TrackDrag(track: track.id, dy: dy)
                            if let before, before != clampedRow(row, dy: dy, count: rows.count) {
                                UISelectionFeedbackGenerator().selectionChanged()
                            }
                        }
                    }
                    .onEnded { _ in
                        held = false
                        guard let d = tool.trackDrag, d.track == track.id else { return }
                        // dragged: no tap comes at the lift to take the flag back
                        openedByHold = false
                        tool.trackDrag = nil
                        let to = clampedRow(row, dy: d.dy, count: rows.count)
                        if to != row {
                            // back to the core's order, bottom first
                            tool.moveTrack(track.id, to: rows.count - 1 - to)
                        }
                    }
            )
    }

    /// The row (top first) a track on row `row` dragged `dy` goes to.
    private func clampedRow(_ row: Int, dy: CGFloat, count: Int) -> Int {
        min(max(row + Int((dy / rowHeight).rounded()), 0), max(count - 1, 0))
    }
}

// MARK: - The ruler

// MARK: - Settings


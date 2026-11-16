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

/// The colours a clip is labelled with in the panel (0xRRGGBB; none, the panel's own).
private let clipLabels: [(String, UInt32?)] = [
    ("None", nil), ("Red", 0xFF453A), ("Orange", 0xFF9F0A), ("Yellow", 0xFFD60A), ("Green", 0x30D158),
    ("Blue", 0x0A84FF), ("Purple", 0xBF5AF2), ("Gray", 0x8E8E93),
]

/// The colours onion skins are screened with (black leaves them as they are).
private let onionColors: [(String, UInt32)] = [
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

    /// The name of blend mode `id`, none for Normal.
    func blendName(_ id: UInt32) -> String? {
        guard let name = canvasModel.blendModeList.first(where: { $0.id == id })?.name, name != "Normal" else { return nil }
        return name
    }

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

    /// Playing goes round frames `start..<end`, or the whole timeline.
    func setLoop(_ range: TimelineLoop?) {
        _ = session.setTimelineLoop(range: range)
        canvasModel.refresh()
    }

    /// The onion skins either side changed (not an undo step, as the other settings).
    func setOnion(_ edit: (inout TimelineOnion) -> Void) {
        guard var o = onion else { return }
        edit(&o)
        session.setTimelineOnion(onion: o)
        refresh()
    }

    /// Whether playing goes round exactly clip `c`'s frames.
    func loops(_ c: TimelineClipInfo) -> Bool {
        loop == TimelineLoop(start: c.start, end: c.start + c.duration)
    }
}

// MARK: - The clip page's rows

/// The picked clip's look: its opacity, blend mode, colour label and whether it shows; playing
/// round its frames; renaming it. A locked track's clips keep their look.
struct ClipLookRows: View {
    let tool: TimelineTool
    let clip: TimelineClipInfo

    /// The opacity as its slider moves, before it is let go; the blend modes listed.
    @State private var opacity: Float?
    @State private var blendsOpen = false

    var body: some View {
        let locked = tool.clip(clip.id)?.0.locked == true
        VStack(alignment: .leading, spacing: 0) {
            if locked {
                Text(verbatim: "The track is locked: unlock it to change the clip.")
                    .font(.system(size: 13))
                    .foregroundStyle(Pc.slideLabel)
                    .padding(.horizontal, 14)
                    .padding(.bottom, 10)
                    .fixedSize(horizontal: false, vertical: true)
            } else {
                let shown = opacity ?? tool.opacityShown(clip)
                TlSliderGroup {
                    TlSlider(
                        label: tool.autokey ? "Opacity, Keyed Here" : "Opacity", value: "\(wholeNumber(shown * 100)) %", share: shown,
                        onEnd: {
                            if let o = opacity {
                                tool.setClipOpacity(clip.id, o, done: true)
                            }
                            opacity = nil
                        }
                    ) { v in
                        opacity = v
                        tool.setClipOpacity(clip.id, v, done: false)
                    }
                }
                TlGroup {
                    HStack(spacing: 0) {
                        Text(verbatim: "Blend Mode")
                            .font(.system(size: 17))
                            .foregroundStyle(Pc.panelTitle)
                        Spacer(minLength: 8)
                        Text(verbatim: tool.blendName(clip.blend) ?? "Normal")
                            .font(.system(size: 17))
                            .foregroundStyle(blendsOpen ? Pc.accent : Pc.slideValue)
                            .lineLimit(1)
                    }
                    .padding(.horizontal, 16)
                    .frame(height: 44)
                    .contentShape(Rectangle())
                    .onTapGesture { blendsOpen.toggle() }
                    if blendsOpen {
                        TlRowLine()
                        let modes = tool.canvasModel.blendModeList
                        ForEach(modes.indices, id: \.self) { i in
                            if i > 0 {
                                TlRowLine()
                            }
                            TlChoice(label: modes[i].name, chosen: clip.blend == modes[i].id) {
                                tool.setClipBlend(clip.id, modes[i].id)
                            }
                        }
                    }
                    TlRowLine()
                    TlToggle(label: "Show Clip", on: !clip.disabled) { tool.setClipDisabled(clip.id, !clip.disabled) }
                }
                .padding(.top, 10)
            }
            TlGroup {
                LabelSwatches(chosen: clip.label) { tool.setClipLabel(clip.id, $0) }
                TlRowLine()
                TlToggle(label: "Loop Playback Over It", on: tool.loops(clip)) {
                    tool.setLoop(tool.loops(clip) ? nil : TimelineLoop(start: clip.start, end: clip.start + clip.duration))
                }
                TlRowLine()
                TlRenameRow(name: clip.name) { tool.renameClip(clip.id, $0) }
            }
            .padding(.top, 10)
        }
        .padding(.bottom, 10)
    }
}

/// A sound or video clip's rows its page shares with a drawing's: its colour label, whether it
/// shows (and plays), playing round its frames, renaming it. A locked track's clips keep showing.
struct MediaClipMarkRows: View {
    let tool: TimelineTool
    let clip: TimelineClipInfo

    var body: some View {
        let locked = tool.clip(clip.id)?.0.locked == true
        TlGroup {
            LabelSwatches(chosen: clip.label) { tool.setClipLabel(clip.id, $0) }
            if !locked {
                TlRowLine()
                TlToggle(label: clip.kind == .audio ? "Play Sound" : "Show Clip", on: !clip.disabled) {
                    tool.setClipDisabled(clip.id, !clip.disabled)
                }
            }
            TlRowLine()
            TlToggle(label: "Loop Playback Over It", on: tool.loops(clip)) {
                tool.setLoop(tool.loops(clip) ? nil : TimelineLoop(start: clip.start, end: clip.start + clip.duration))
            }
            TlRowLine()
            TlRenameRow(name: clip.name) { tool.renameClip(clip.id, $0) }
        }
        .padding(.top, 10)
    }
}

/// A clip's colour label: a swatch for each, the one chosen ringed.
private struct LabelSwatches: View {
    let chosen: UInt32?
    let choose: (UInt32?) -> Void

    var body: some View {
        HStack(spacing: 0) {
            ForEach(clipLabels.indices, id: \.self) { i in
                let (name, rgb) = clipLabels[i]
                ZStack {
                    if let rgb {
                        Circle().fill(Color(rgb: rgb)).frame(width: 22, height: 22)
                    } else {
                        Circle().strokeBorder(Pc.slideLabel, lineWidth: 1.5).frame(width: 22, height: 22)
                        Rectangle().fill(Pc.slideLabel).frame(width: 1.5, height: 22).rotationEffect(.degrees(45))
                    }
                    if chosen == rgb {
                        Circle().strokeBorder(Pc.accent, lineWidth: 2.5).frame(width: 31, height: 31)
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 44)
                .contentShape(Rectangle())
                .onTapGesture { choose(rgb) }
                .accessibilityLabel(Text(verbatim: name))
            }
        }
        .padding(.horizontal, 6)
    }
}

/// "Rename…", which becomes a field for the new name: Return renames.
struct TlRenameRow: View {
    let name: String
    let rename: (String) -> Void

    @State private var editing: String?
    @FocusState private var focused: Bool

    var body: some View {
        if let text = editing {
            TextField("", text: Binding(get: { text }, set: { editing = $0 }))
                .font(.system(size: 17))
                .foregroundStyle(Pc.panelTitle)
                .tint(Pc.accent)
                .autocorrectionDisabled()
                .submitLabel(.done)
                .focused($focused)
                .onSubmit {
                    rename(text)
                    editing = nil
                }
                .onAppear {
                    focused = true
                    // the whole name chosen, so typing replaces it (as the layers panel's does)
                    DispatchQueue.main.async {
                        UIApplication.shared.sendAction(#selector(UIResponder.selectAll(_:)), to: nil, from: nil, for: nil)
                    }
                }
                .onChange(of: focused) { _, now in
                    if !now {
                        editing = nil
                    }
                }
                .padding(.horizontal, 16)
                .frame(height: 44)
        } else {
            TlAction(label: "Rename…") { editing = name }
        }
    }
}

// MARK: - The track page

/// Track `track`'s page: its opacity; showing it alone, locking it; renaming it, moving it up or
/// down, deleting it.
struct TrackPage: View {
    let tool: TimelineTool
    let track: TimelineTrackInfo

    /// The opacity as its slider moves, before it is let go.
    @State private var opacity: Float?

    var body: some View {
        // shown top first; the core's order is bottom first
        let index = tool.tracks.firstIndex { $0.id == track.id } ?? 0
        let count = tool.tracks.count
        let shown = opacity ?? track.opacity
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                PanelTitle(text: track.name, bottom: 14)
                Spacer(minLength: 8)
                Text(verbatim: "Done")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(Pc.accent)
                    .padding(.horizontal, 14)
                    .frame(height: 44)
                    .contentShape(Rectangle())
                    .onTapGesture { tool.trackOpen = nil }
            }
            TlSliderGroup {
                TlSlider(
                    label: "Opacity", value: "\(wholeNumber(shown * 100)) %", share: shown,
                    onEnd: {
                        if let o = opacity {
                            tool.setTrackOpacity(track.id, o, done: true)
                        }
                        opacity = nil
                    }
                ) { v in
                    opacity = v
                    tool.setTrackOpacity(track.id, v, done: false)
                }
            }
            TlGroup {
                TlToggle(label: "Show Alone", on: track.soloed) { tool.solo(track.id) }
                TlRowLine()
                TlToggle(label: "Show Track", on: !track.hidden) { tool.toggleHidden(track) }
                TlRowLine()
                TlToggle(label: "Lock Track", on: track.locked) { tool.toggleLocked(track) }
                TlRowLine()
                TlRenameRow(name: track.name) { tool.renameTrack(track.id, $0) }
            }
            .padding(.top, 10)
            TlGroup {
                if index < count - 1 {
                    TlAction(label: "Move Up") { tool.moveTrack(track.id, to: index + 1) }
                    TlRowLine()
                }
                if index > 0 {
                    TlAction(label: "Move Down") { tool.moveTrack(track.id, to: index - 1) }
                    TlRowLine()
                }
                if count > 1 {
                    TlAction(label: "Delete Track") {
                        tool.trackOpen = nil
                        tool.deleteTrack(track.id)
                    }
                } else {
                    Text(verbatim: "The only track can't be deleted.")
                        .font(.system(size: 15))
                        .foregroundStyle(Pc.slideLabel)
                        .padding(.leading, 16)
                        .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                }
            }
            .padding(.top, 10)
        }
    }
}

/// A track's name held opens its page; dragged on from there, it moves up or down among the
/// others, an accent line where it will go, and lands there when let go.
struct TrackHeaderGestures: ViewModifier {
    let tool: TimelineTool
    let track: TimelineTrackInfo
    let rowHeight: CGFloat

    @State private var held = false

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
            .simultaneousGesture(TapGesture().onEnded { tool.trackOpen = nil })
            .gesture(
                LongPressGesture(minimumDuration: 0.35)
                    .sequenced(before: DragGesture(minimumDistance: 0, coordinateSpace: .global))
                    .onChanged { value in
                        guard case .second(true, let moved) = value, !tool.isPlaying else { return }
                        if !held {
                            held = true
                            tool.settingsOpen = false
                            tool.keysOpen = false
                            tool.clipOpen = false
                            tool.cameraOpen = false
                            tool.trackOpen = track.id
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

/// The looped frames on the ruler: a band across them, and a line down the tracks at each end.
struct TimelineLoopBand: View {
    let loop: TimelineLoop?
    let pitch: CGFloat
    let ruler: CGFloat
    let height: CGFloat

    var body: some View {
        if let loop {
            let x = CGFloat(loop.start) * pitch
            let w = CGFloat(loop.end - loop.start) * pitch
            ZStack(alignment: .topLeading) {
                Pc.accent.opacity(0.28)
                    .frame(width: w, height: ruler)
                Pc.accent.opacity(0.7).frame(width: 1.5, height: height)
                Pc.accent.opacity(0.7).frame(width: 1.5, height: height).offset(x: w - 1.5)
            }
            .offset(x: x)
            .allowsHitTesting(false)
        }
    }
}

// MARK: - Settings

/// The onion skin drawings before and after the one drawn on, each counted apart.
struct OnionCountSliders: View {
    let tool: TimelineTool
    let most: Int

    var body: some View {
        let o = tool.onion ?? TimelineOnion(before: 0, after: 0, beforeColor: 0, afterColor: 0)
        let word = { (n: UInt32) in n == 0 ? "None" : Int(n) == most ? "Max" : "\(n)" }
        TlSlider(label: "Onion skins before", value: word(o.before), share: Float(o.before) / Float(most)) { v in
            tool.setOnion { $0.before = UInt32(wholeNumber(v * Float(most))) }
        }
        TlSlider(label: "Onion skins after", value: word(o.after), share: Float(o.after) / Float(most)) { v in
            tool.setOnion { $0.after = UInt32(wholeNumber(v * Float(most))) }
        }
    }
}

/// The colours the onion skins before and after are screened with; and the looped frames, with
/// playing round the whole timeline again.
struct OnionColorsAndLoop: View {
    let tool: TimelineTool

    var body: some View {
        let o = tool.onion ?? TimelineOnion(before: 0, after: 0, beforeColor: 0, afterColor: 0)
        TlGroup {
            colorRow("Before", o.beforeColor) { c in tool.setOnion { $0.beforeColor = c } }
            TlRowLine()
            colorRow("After", o.afterColor) { c in tool.setOnion { $0.afterColor = c } }
            if let l = tool.loop {
                TlRowLine()
                TlAction(label: "Stop Looping Frames \(l.start + 1)–\(l.end)") { tool.setLoop(nil) }
            }
        }
        .padding(.top, 10)
    }

    private func colorRow(_ label: String, _ chosen: UInt32, choose: @escaping (UInt32) -> Void) -> some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 15))
                .foregroundStyle(Pc.slideLabel)
                .frame(width: 58, alignment: .leading)
            ForEach(onionColors.indices, id: \.self) { i in
                let (name, rgb) = onionColors[i]
                ZStack {
                    if rgb == 0 {
                        Circle().strokeBorder(Pc.slideLabel, lineWidth: 1.5).frame(width: 20, height: 20)
                    } else {
                        Circle().fill(Color(rgb: rgb)).frame(width: 20, height: 20)
                    }
                    if chosen == rgb {
                        Circle().strokeBorder(Pc.accent, lineWidth: 2.5).frame(width: 29, height: 29)
                    }
                }
                .frame(maxWidth: .infinity, minHeight: 44)
                .contentShape(Rectangle())
                .onTapGesture { choose(rgb) }
                .accessibilityLabel(Text(verbatim: "\(label): \(name)"))
            }
        }
        .padding(.leading, 16)
        .padding(.trailing, 6)
    }
}

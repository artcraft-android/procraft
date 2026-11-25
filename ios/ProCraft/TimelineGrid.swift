import SwiftUI
import UIKit

// The timeline as a grid: every track a row of frame cells under a ruler of frame numbers, the
// playhead standing still in the middle of the bar while the frames slide under it. A drawing held
// for more than a frame is one block, its first frame bright and the held ones dimmed. The frame
// under the playhead, tapped, gets a handle on each edge: dragged, it lasts longer or shorter,
// the frames beyond it moving along. The lasso picks frames together with a stroke drawn across
// them, to lengthen, flip, clear or delete as one. A frame or a track's name held opens its menu.
//
// It shows the same timeline the strip and the tracks view did (TimelineBar.swift), through the
// same `TimelineTool`; what those had and it hasn't (keys, the camera, sounds' and groups' own
// pages, markers) still plays and renders.

/// The grid's fixed sizes (points) and colours; its cells' and rows' sizes, which zoom, are
/// `GridSizes` (TimelineGridRows.swift).
enum Grid {
    static let rulerHeight: CGFloat = 26
    static let headerHeight: CGFloat = 46
    /// How far in from the bar's left edge the cells show whole: before it they fade out under
    /// the tracks' names, and touches there are the names'.
    static let labelsWidth: CGFloat = 176
    /// How many empty frames show past the timeline's end (the core's `PAST_END`).
    static let pastEnd = 64
    static let space = "grid"

    private static func pick(_ dark: UInt32, _ light: UInt32) -> Color {
        Color(argb: Look.shared.light ? light : dark)
    }

    static var panel: Color { pick(0xFF21_2121, 0xFFE4_E4E4) }
    static var slot: Color { pick(0xFF16_1616, 0xFFD1_D1D1) }
    static var line: Color { pick(0xFF47_4747, 0xFFC0_C0C0) }
    static var capsule: Color { pick(0xFF35_3535, 0xFFCB_CBCB) }
    static var ink: Color { pick(0xFFC9_C9C9, 0xFF39_3939) }
    static var inkBright: Color { pick(0xFFF2_F2F2, 0xFF1C_1C1C) }
    static var inkDim: Color { pick(0xFF75_7575, 0xFF8C_8C8C) }
    static var box: Color { pick(0xFF86_8686, 0xFF8A_8A8A) }
    /// A clip that isn't a drawing (a group, a sound, a video).
    static var block: Color { pick(0xFF4E_4E4E, 0xFFB2_B2B2) }
    /// What stands behind a sound's waveform, and behind a video's pictures until they are read.
    static var soundGround: Color { pick(0xFF30_3030, 0xFFBE_BEBE) }
    static var videoGround: Color { pick(0xFF3C_3836, 0xFFB0_ACA8) }
    /// Laid over a drawing's held frames, so they read as the same drawing going on.
    static let heldShade = Color(argb: 0x9940_4040)
    static var card: Color { pick(0x8C2C_2C2C, 0xB3F2_F2F2) }
    static let danger = Color(argb: 0xFFFF_453A)

    /// What is written on a track's highlight colour.
    static let onLabel = Color(argb: 0xE600_0000)
}

extension TimelineTool {
    private var session: CanvasSession { canvasModel.session }

    /// The timeline's settings (its frame rate, playback, onion skins, length), on a page standing
    /// on the bar: opened from Actions, since the grid's header has no word for them.
    func openSettings() {
        canvasModel.closePanels()
        stopPlaying()
        settingsOpen = true
    }

    /// A frame's edge let go (or its drag taken away).
    func endEdgeDrag() {
        edgeDragging = false
    }

    /// One frame on or back from the playhead (an empty one past the end too).
    func stepFrame(_ by: Int) {
        let last = Int(info?.duration ?? 1) - 1 + Grid.pastEnd
        let to = min(max(Int(frame) + by, 0), last)
        guard to != Int(frame) else { return }
        picked = nil
        goTo(UInt32(to))
        media.blip(UInt32(to))
    }

    /// Its track's clips as clip `id`'s start (`leading`) or end dragged to `frame` would leave
    /// them; empty if it would change nothing. An end made later takes up the empty frames after
    /// it first and pushes only the frames it reaches; made earlier, nothing else moves. A start
    /// pushes the frames before it into the empty frames before them, and, asked earlier than
    /// they have room for (`frame` may be before the first), the clip lasts that much longer at
    /// its end instead.
    func gridTrimPreview(_ id: UInt64, leading: Bool, to frame: Int) -> [TimelineClipInfo] {
        session.timelineLengthenPreview(id: id, leading: leading, frame: Int32(clamping: frame))
    }

    /// That edge dragged there: one undo step.
    func gridTrim(_ id: UInt64, leading: Bool, to frame: Int) {
        stopPlaying()
        _ = session.lengthenTimelineClip(id: id, leading: leading, frame: Int32(clamping: frame))
        canvasModel.refresh()
    }

    /// The tracks every picked frame's same edge dragged `by` frames would change, with their
    /// clips then, the frames beyond each moving along.
    func gridTrimsPreview(leading: Bool, by: Int) -> [UInt64: [TimelineClipInfo]] {
        let tracks = session.timelineLengthensPreview(ids: Array(selecting ?? []), leading: leading, by: Int32(clamping: by))
        return Dictionary(tracks.map { ($0.track, $0.clips) }, uniquingKeysWith: { a, _ in a })
    }

    /// Every picked frame's same edge dragged `by` frames: one undo step.
    func gridTrimSelected(leading: Bool, by: Int) {
        guard by != 0 else { return }
        stopPlaying()
        _ = session.lengthenTimelineClips(ids: Array(selecting ?? []), leading: leading, by: Int32(clamping: by))
        canvasModel.refresh()
    }

    func setTrackLabel(_ id: UInt64, _ label: UInt32?) {
        _ = session.setTimelineTrackLabel(id: id, label: label)
        canvasModel.refresh()
    }

    func setTrackBlend(_ id: UInt64, _ blend: UInt32) {
        if !session.setTimelineTrackBlend(id: id, blend: blend), tracks.first(where: { $0.id == id })?.locked == true {
            canvasModel.showNotice("The track is locked.", warning: true)
        }
        canvasModel.refresh()
    }

    func setTrackMask(_ id: UInt64, _ mask: TimelineMask) {
        if !session.setTimelineTrackMask(id: id, mask: mask), tracks.first(where: { $0.id == id })?.locked == true {
            canvasModel.showNotice("The track is locked.", warning: true)
        }
        canvasModel.refresh()
    }

    /// The drawings of frames `ids` made blank, the frames staying where they are: one undo step.
    func clearFrames(_ ids: [UInt64]) {
        guard !ids.isEmpty else { return }
        stopPlaying()
        if !session.clearTimelineClips(ids: ids) {
            canvasModel.showNotice("Only drawings on tracks that aren't locked can be cleared.", warning: true)
        }
        canvasModel.refresh()
    }

    /// Frames `ids` put in the opposite order in time: one undo step.
    func flipFrames(_ ids: [UInt64]) {
        guard ids.count > 1 else { return }
        stopPlaying()
        if session.reverseTimelineClips(ids: ids) {
            UISelectionFeedbackGenerator().selectionChanged()
        } else {
            canvasModel.showNotice("They can't be flipped: a frame between them isn't picked, or a track is locked.", warning: true)
        }
        canvasModel.refresh()
    }

    /// Frames `ids` copied with their keys, to paste at the playhead; what is picked stays picked.
    func copyFrames(_ ids: [UInt64]) {
        guard !ids.isEmpty else { return }
        if session.copyTimelineClips(ids: ids) {
            canvasModel.showNotice(ids.count == 1 ? "Copied. Paste puts it at the playhead on the chosen track."
                : "\(ids.count) frames copied. Paste puts them at the playhead, from the chosen track up.", warning: false)
        }
        canvasModel.refresh()
    }

    /// Frames `ids` copied, then taken off with their drawings: one undo step.
    func cutFrames(_ ids: [UInt64]) {
        guard !ids.isEmpty else { return }
        stopPlaying()
        picked = nil
        if !session.cutTimelineClips(ids: ids) {
            canvasModel.showNotice("One of them is on a locked track.", warning: true)
        }
        canvasModel.refresh()
    }

    /// Frames `ids` taken off with their drawings: one undo step.
    func deleteFrames(_ ids: [UInt64]) {
        guard !ids.isEmpty else { return }
        stopPlaying()
        picked = nil
        if !session.deleteTimelineClips(ids: ids) {
            canvasModel.showNotice("One of them is on a locked track.", warning: true)
        }
        canvasModel.refresh()
    }

    /// Copies of frames `ids` just after the last of them: one undo step. Returns the copies.
    @discardableResult
    func duplicateFrames(_ ids: [UInt64]) -> [UInt64] {
        guard !ids.isEmpty else { return [] }
        stopPlaying()
        let made = session.duplicateTimelineClips(ids: ids)
        if made.isEmpty {
            canvasModel.showNotice("Only frames on tracks that aren't locked, with room for their layers, can be duplicated.", warning: true)
        }
        canvasModel.refresh()
        return made
    }

    /// A new blank frame just after `clip` on `track`, the frames after it moving along; the
    /// playhead goes onto it.
    func insertFrame(after clip: TimelineClipInfo, on track: TimelineTrackInfo) {
        selectTrack(track.id)
        goTo(clip.start)
        addDrawing()
    }

    /// What was copied put at `clip`'s first frame on `track`.
    func paste(at clip: TimelineClipInfo, on track: TimelineTrackInfo) {
        selectTrack(track.id)
        goTo(clip.start)
        pasteClips()
    }
}

/// An edge of the frame under the playhead being dragged: whose and which, the frame that edge
/// started on, the frame the clip ended on (one past its last) as the drag began, the frame asked
/// for now (a start's may be before the first), how far the finger has gone, its track's clips as
/// the drag would leave them (empty: as they are), and the frame a limit holds the edge at.
struct GridTrim {
    var clip: UInt64
    var track: UInt64
    var leading: Bool
    var from: Int
    var end: Int
    var frame: Int
    var dx: CGFloat
    var clips: [TimelineClipInfo] = []
    var limit: CGFloat?
    /// A repeating group's end: how many frames it plays again and again (0: no such thing), and
    /// the most frames it has room for.
    var cycle = 0
    var most = 0
}

/// An edge of the frames picked together being dragged: whose handle, which edge, the frame it
/// started on, how many columns the handle goes for each frame every picked frame grows, how many
/// frames they have grown, the tracks as that would leave them, and a limit's frame.
struct GridGroupTrim {
    var clip: UInt64
    var leading: Bool
    var from: Int
    var steps: Int
    var by: Int
    var tracks: [UInt64: [TimelineClipInfo]] = [:]
    var limit: CGFloat?
}

/// What a finger down on the frames is doing.
enum GridTouch: Equatable {
    /// Not yet told apart: a tap, a hold, or the start of a drag.
    case undecided
    /// Dragged sideways: the frames slide under the playhead.
    case scroll
    /// Dragged with the lasso on: a stroke picking the frames it crosses.
    case stroke
    /// Down on a frame's handle (its start's, or its end's): dragged, the frame lasts longer or
    /// shorter.
    case handle(leading: Bool)
    /// Held still: a menu opened.
    case held
    /// Dragged on from a hold on a block: the block goes with the finger.
    case move
    /// Dragged on from a hold on a key: it (and the keys picked with it) go along.
    case keys
    /// Down on a grip beside the keys picked: dragged, they spread out or close up in time.
    case stretch
    /// Dragged up or down where there are more tracks than show: they scroll.
    case pan
    /// Nothing of the grid's (a track's name).
    case ignore
}

/// What the grid shows that isn't the timeline's own: where it is scrolled to while dragged, an
/// edge being dragged, the lasso's stroke, the picked frames' menu.
@Observable
final class GridState {
    /// The frame under the playhead while the frames are dragged (between frames too); none when
    /// they rest on the playhead's own.
    var dragged: CGFloat?
    var trim: GridTrim?
    var groupTrim: GridGroupTrim?
    /// The lasso's stroke under way, and the last one fading out.
    var stroke: [CGPoint] = []
    var faded: [CGPoint] = []
    var fading = false
    /// The picked frames' menu, open: where along the bar it stands.
    var groupMenu: CGFloat?
    /// The end flap's menu, open: where along the bar it stands. And the timeline's length as the
    /// flap is dragged, before it is let go.
    var flapMenu: CGFloat?
    var lengthDraft: Int?
    /// The frames that alone play: a group's the grid was fitted to with three taps.
    var playOnly: Range<UInt32>?
    /// A key held, its menu open (see TimelineGridKeys.swift).
    var keyMenu: GridKeySpot?
    /// A block being dragged somewhere else (see TimelineGridMove.swift).
    var move: GridMove?
    /// An empty frame held, its menu open; the Add new card open; and what it asked to be picked.
    var emptyMenu: GridEmptySpot?
    var adding = false
    var asking: GridAsk?
    /// How far the tracks are scrolled up, where there are more of them than show.
    var rowsOffset: CGFloat = 0
    /// How far the grid is zoomed (1: as it starts, a frame 44 points wide), kept from one canvas
    /// to the next.
    var zoom: CGFloat = {
        let kept = UserDefaults.standard.double(forKey: "timelineGridZoom")
        return kept > 0 ? CGFloat(kept) : 1
    }()

    /// Its cells' and rows' sizes as it is zoomed.
    var sizes: GridSizes { GridSizes(zoom: zoom) }

    @ObservationIgnored var touch: GridTouch?
    /// Where the touch under way came down: a touch from somewhere else is a new one, the last
    /// having been taken away without ending.
    @ObservationIgnored var touchStart: CGPoint?
    @ObservationIgnored var scrollFrom: CGFloat = 0
    @ObservationIgnored var panFrom: CGFloat = 0
    @ObservationIgnored var scrubbed: UInt32 = 0
    /// The frames the stroke under way has crossed, and what was picked as it began.
    @ObservationIgnored var touched: Set<UInt64> = []
    @ObservationIgnored var touchedKeys: Set<TimelineKeyPick> = []
    @ObservationIgnored var strokeBefore: Set<UInt64>?
    /// The block the finger down is holding, if it held one. And of a block dragged: which way
    /// the frames are sliding under it near an end of the bar, what slides them, and what works
    /// out again where it would land once they have.
    @ObservationIgnored var heldClip: UInt64?
    /// The key the finger down is holding, if it held one, and the frame it was on.
    @ObservationIgnored var heldKey: TimelineKeyPick?
    /// The keys picked being stretched in time: the frame they stretch about, and the frame the
    /// grip dragged began on.
    @ObservationIgnored var stretching: (anchor: Int, from: Int)?
    /// The taps on a group in a row, for the third.
    @ObservationIgnored var taps: (clip: UInt64, count: Int, at: Date) = (0, 0, .distantPast)
    /// The sounds and videos read for their blocks, for one state of the timeline.
    @ObservationIgnored var mediaRead: (revision: Int, clips: [UInt64: TimelineMediaClip]) = (-1, [:])
    @ObservationIgnored var moveSlide: CGFloat = 0
    @ObservationIgnored var moveTask: Task<Void, Never>?
    @ObservationIgnored var moveAgain: (() -> Void)?
    /// Two fingers are on the frames (see TimelineGridZoom.swift): the zoom as they came down,
    /// whether they have moved together far enough to carry the grid along, and how far they had
    /// by then.
    @ObservationIgnored var pinching = false
    /// The fingers of a pinch are lifting: what the drag of one of them says is still the pinch's.
    @ObservationIgnored var afterPinch = false
    @ObservationIgnored var panning = false
    @ObservationIgnored var zoomFrom: CGFloat = 1
    @ObservationIgnored var panShift: CGSize = .zero
    @ObservationIgnored var zoomTask: Task<Void, Never>?

    /// Every menu standing on the bar closed.
    func closeMenus(_ tool: TimelineTool) {
        if tool.clipMenu != nil {
            tool.clipMenu = nil
        }
        if tool.trackOpen != nil {
            tool.trackOpen = nil
        }
        if tool.settingsOpen {
            tool.settingsOpen = false
        }
        if groupMenu != nil {
            groupMenu = nil
        }
        if flapMenu != nil {
            flapMenu = nil
        }
        if emptyMenu != nil {
            emptyMenu = nil
        }
        if keyMenu != nil {
            keyMenu = nil
        }
        if adding {
            adding = false
        }
    }

    func menuOpen(_ tool: TimelineTool) -> Bool {
        tool.clipMenu != nil || tool.trackOpen != nil || tool.settingsOpen || groupMenu != nil || flapMenu != nil || emptyMenu != nil || adding
            || keyMenu != nil
    }

    /// The frames dragged sideways `translation` from where they were: the frame now under the
    /// playhead shown as it plays.
    func scrolled(_ translation: CGFloat, last: Int, tool: TimelineTool) {
        if dragged == nil {
            playOnly = nil
            scrubbed = tool.frame
            if tool.picked != nil {
                tool.picked = nil
            }
        }
        let at = min(max(scrollFrom - translation / sizes.pitch, 0), CGFloat(last))
        dragged = at
        let frame = UInt32(at.rounded())
        if frame != scrubbed {
            scrubbed = frame
            tool.scrub(frame)
        }
    }

    /// The drag let go, having been going to carry them `predicted`: they settle on a frame,
    /// which is drawn on.
    func scrollEnded(_ predicted: CGFloat, last: Int, tool: TimelineTool) {
        guard dragged != nil else { return }
        let target = min(max(scrollFrom - predicted / sizes.pitch, 0), CGFloat(last)).rounded()
        withAnimation(Motion.frameSlide) { dragged = target }
        tool.endScrub(UInt32(target))
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { [weak self] in
            // (unless they are being dragged again)
            if let self, self.touch != .scroll {
                self.dragged = nil
            }
        }
    }

    /// The stroke lifted: it fades away.
    func endStroke() {
        faded = stroke
        stroke = []
        touched = []
        fading = false
        withAnimation(.easeOut(duration: 0.35)) { fading = true }
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { [weak self] in
            if self?.stroke.isEmpty == true {
                self?.faded = []
            }
        }
    }
}

/// The timeline, as the grid.
struct GridTimeline: View {
    let model: CanvasModel
    let tool: TimelineTool
    let info: TimelineInfo
    let screen: CGSize

    @State private var grid = GridState()
    /// How tall the tracks show, as the grabber left them (0: every track, up to half the screen).
    @AppStorage("timelineGridHeight") private var keptHeight = 0.0
    @State private var heightFrom: CGFloat?
    /// When the grabber was last tapped, for the second tap.
    @State private var grabberTapped = Date.distantPast
    @AppStorage("timelineTimecode") private var timecode = false

    var body: some View {
        let width = min(screen.width - 32, 1180)
        let sizes = grid.sizes
        // top first; the core's order is bottom first
        let rows = GridRows(rows: tool.tracks.reversed().compactMap { tool.place($0.id) }, sizes: sizes) { t in
            tool.gridKeyTracks.contains(t.id) ? tool.keyRows(of: t) : []
        }
        // as tall as its rows are before the grid is zoomed out: zoomed out, the bar stays as tall
        // and the rows, smaller, stand in the middle of it
        let all = max(rows.list.reduce(0) { $0 + ($1.lanes.isEmpty ? GridSizes.rowHeight : GridRows.keysHeight) }, GridSizes.rowHeight)
        let most = max((screen.height * 0.5 - Grid.headerHeight - Grid.rulerHeight - 12).rounded(.down), GridSizes.rowHeight)
        let auto = min(all, most)
        let shown = keptHeight > 0 ? min(max(CGFloat(keptHeight), GridSizes.rowHeight), auto) : auto
        // what is left above the bar for a menu standing on it
        let room = max(screen.height - shown - Grid.headerHeight - Grid.rulerHeight - 110, 240)
        ZStack(alignment: .bottom) {
            if grid.menuOpen(tool) {
                // a tap anywhere else closes the menu
                Color.black.opacity(0.001)
                    .contentShape(Rectangle())
                    .onTapGesture { grid.closeMenus(tool) }
            }
            VStack(alignment: .leading, spacing: 0) {
                GridMenus(model: model, tool: tool, info: info, grid: grid, width: width, room: room)
                VStack(spacing: 0) {
                    GridHeader(tool: tool, grid: grid)
                        .frame(height: Grid.headerHeight)
                    Grid.line.frame(height: 1)
                    GridBody(tool: tool, info: info, grid: grid, rows: rows, width: width, height: shown)
                        .padding(.bottom, 9)
                }
                .frame(width: width)
                .background(Grid.panel)
                .clipShape(RoundedRectangle(cornerRadius: 28, style: .continuous))
                .shadow(color: .black.opacity(0.3), radius: 16, y: 6)
                // the whole bar takes its touches: none reach the canvas under it. (Not closing
                // a menu here: the lift that ends the hold that opened one counts as a tap too)
                .contentShape(Rectangle())
                .onTapGesture {}
                // the grabber: dragged up or down, the tracks show taller or shorter; tapped
                // twice, every one of them again
                // (its touches are taken from the moment a finger is down on it, a little above
                // the bar's edge and below it: a tap recognizer beside the drag would hold the
                // drag back until the finger had gone too far for a tap, so taps are told here)
                .overlay(alignment: .top) {
                    Capsule()
                        .fill(heightFrom == nil ? Grid.inkDim : Grid.inkBright)
                        .frame(width: 44, height: 5)
                        .padding(.top, 15)
                        // (as wide as the header leaves clear between its buttons)
                        .frame(width: max(min(130, width / 2 - 270), 66) * 2, height: 36, alignment: .top)
                        .contentShape(Rectangle())
                        .gesture(
                            DragGesture(minimumDistance: 0, coordinateSpace: .global)
                                .onChanged { drag in
                                    let from = heightFrom ?? shown
                                    heightFrom = from
                                    // (a finger that has only come down changes nothing yet)
                                    guard abs(drag.translation.height) >= 1 else { return }
                                    let next = min(max(from - drag.translation.height, GridSizes.rowHeight), min(all, most))
                                    keptHeight = next >= min(all, most) ? 0 : Double(next)
                                }
                                .onEnded { drag in
                                    heightFrom = nil
                                    guard hypot(drag.translation.width, drag.translation.height) < 6 else {
                                        grabberTapped = .distantPast
                                        return
                                    }
                                    let now = Date()
                                    if now.timeIntervalSince(grabberTapped) < 0.4 {
                                        keptHeight = 0
                                        grabberTapped = .distantPast
                                    } else {
                                        grabberTapped = now
                                    }
                                }
                        )
                        .offset(y: -12)
                }
                // where the playhead is in time, over the bar's corner
                .overlay(alignment: .topLeading) {
                    if timecode, !grid.menuOpen(tool) {
                        GridTimecode(frame: tool.frame, fps: info.fps)
                            .offset(x: 4, y: -36)
                    }
                }
                .padding(.bottom, 19)
            }
            .frame(width: width)
            .animation(Motion.easeInOut(0.2), value: rows.count)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottom)
        .modifier(GridImports(model: model, grid: grid))
        // (how tall the bar stands, for what stands above it)
        .onChange(of: shown + Grid.headerHeight + Grid.rulerHeight + 30, initial: true) { _, tall in
            tool.gridHeight = tall
        }
        .onDisappear { tool.gridHeight = 0 }
    }
}

/// The bar's top row: Play, a frame back and on; then the lasso, a new track, a new frame.
private struct GridHeader: View {
    let tool: TimelineTool
    let grid: GridState

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: tool.isPlaying ? "Pause" : "Play")
                .font(.system(size: 17, weight: .bold))
                .foregroundStyle(Grid.ink)
                .lineLimit(1)
                .frame(minWidth: 54, minHeight: 40, alignment: .leading)
                .contentShape(Rectangle())
                .onTapGesture {
                    grid.closeMenus(tool)
                    tool.picked = nil
                    tool.togglePlaying()
                }
            // with Animate on: a take armed, which the next touch on the canvas starts (while the
            // Timeline plugin's Performed moves is on)
            if tool.autokey, PluginsModel.shared.has(.performedMoves) {
                // (a record light: ringed while a take is armed)
                Circle()
                    .fill(autokeyRed)
                    .frame(width: 15, height: 15)
                    .overlay(Circle().strokeBorder(autokeyRed, lineWidth: 2).frame(width: 25, height: 25).opacity(tool.performArmed ? 1 : 0))
                    .frame(width: 40, height: 40)
                    .contentShape(Rectangle())
                    .onTapGesture {
                        grid.closeMenus(tool)
                        tool.stopPlaying()
                        tool.performArmed.toggle()
                    }
                    .accessibilityElement()
                    .accessibilityLabel(Text(verbatim: "Perform"))
                    .accessibilityAddTraits(.isButton)
            }
            // tapped, a frame back or on; held, to the drawing before or after on the chosen track
            GridIconButton(icon: PcIcons.chevronLeft, size: 21, name: "Previous Frame", held: {
                grid.closeMenus(tool)
                tool.picked = nil
                tool.stepDrawing(-1)
            }) {
                grid.closeMenus(tool)
                tool.stepFrame(-1)
            }
            .padding(.leading, 18)
            GridIconButton(icon: PcIcons.chevronRight, size: 21, name: "Next Frame", held: {
                grid.closeMenus(tool)
                tool.picked = nil
                tool.stepDrawing(1)
            }) {
                grid.closeMenus(tool)
                tool.stepFrame(1)
            }
            Spacer(minLength: 0)
            GridIconButton(icon: PcIcons.gridSettings, size: 27, on: tool.settingsOpen, name: "Timeline Settings") {
                let open = !tool.settingsOpen
                grid.closeMenus(tool)
                if open {
                    tool.openSettings()
                }
            }
            // Animate: the transform tool keys what is under the playhead where it is put
            GridIconButton(icon: PcIcons.gridKey, size: 27, on: tool.autokey, name: "Animate") {
                grid.closeMenus(tool)
                tool.performArmed = false
                tool.canvasModel.setAutokey(!tool.autokey)
            }
            GridIconButton(icon: PcIcons.gridLasso, size: 27, on: tool.selecting != nil, name: "Select Frames") {
                grid.closeMenus(tool)
                if tool.selecting == nil {
                    tool.startSelecting()
                } else {
                    tool.selecting = nil
                }
            }
            GridIconButton(icon: PcIcons.gridTrack, size: 27, name: "Add Track") {
                grid.closeMenus(tool)
                tool.picked = nil
                tool.addTrack()
            }
            // tapped, a new frame; held, the Add new card
            GridIconButton(icon: PcIcons.add, size: 38, on: grid.adding, name: "Add Frame", holdFor: 0.2, held: {
                grid.closeMenus(tool)
                tool.picked = nil
                grid.adding = true
            }) {
                grid.closeMenus(tool)
                tool.picked = nil
                tool.addDrawing()
            }
        }
        .padding(.leading, 26)
        .padding(.trailing, 16)
        .padding(.top, 4)
        // a tap on the header beside its buttons closes a menu
        .contentShape(Rectangle())
        .onTapGesture { grid.closeMenus(tool) }
    }
}

/// One of the header's glyphs: lit in the accent while it is on, and for a moment when tapped.
private struct GridIconButton: View {
    let icon: PcIcon
    var size: CGFloat = 24
    var on = false
    let name: String
    /// What holding it does, if anything, and for how long it is held first (as long as a frame
    /// is for its menu, unless said).
    var holdFor = 0.4
    var held: (() -> Void)?
    let action: () -> Void

    @State private var flash = false
    /// It was held: the lift that ends the hold is no tap.
    @State private var wasHeld = false

    var body: some View {
        PcIconView(icon, size: size, color: on || flash ? Pc.accent : Grid.ink)
            .frame(width: 50, height: 40)
            .contentShape(Rectangle())
            .onTapGesture {
                guard !wasHeld else { return }
                flash = true
                action()
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.18) { flash = false }
            }
            .simultaneousGesture(LongPressGesture(minimumDuration: holdFor).onEnded { _ in
                guard let held else { return }
                wasHeld = true
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
                held()
            })
            .simultaneousGesture(DragGesture(minimumDistance: 0).onEnded { _ in
                // (after the tap the lift may count as has been let pass)
                DispatchQueue.main.asyncAfter(deadline: .now() + 0.12) { wasHeld = false }
            })
            .accessibilityElement()
            .accessibilityLabel(Text(verbatim: name))
            .accessibilityAddTraits(.isButton)
    }
}

/// Where the frames stand along the bar: the bar's own x of frame 0's cell, the playhead's cell,
/// the cells' sizes and the rows.
struct GridPlace {
    var origin: CGFloat
    var playhead: CGFloat
    var sizes: GridSizes
    var rows: GridRows
    var last: Int
    /// How far the rows are scrolled up, and how far they can be.
    var scroll: CGFloat = 0
    var over: CGFloat = 0
    /// How far down the bar the rows start, where they are shorter than it (they stand in its
    /// middle).
    var inset: CGFloat = 0

    /// A point of the rows as they show, as a point of all of them.
    func inRows(_ point: CGPoint) -> CGPoint {
        CGPoint(x: point.x, y: point.y + scroll - inset)
    }

    /// The row (top first) and frame at `point` of the rows; none off them.
    func cell(at point: CGPoint) -> (row: Int, frame: Int)? {
        // (the gap after a cell counts as its own)
        let frame = Int(((point.x - origin) / sizes.pitch).rounded(.down))
        guard let row = rows.index(at: point.y), frame >= 0, frame <= last else { return nil }
        return (row, frame)
    }
}

/// The ruler of frame numbers and the tracks' rows, the playhead's outline standing over them.
struct GridBody: View {
    let tool: TimelineTool
    let info: TimelineInfo
    let grid: GridState
    /// The tracks' rows, top first.
    let rows: GridRows
    let width: CGFloat
    /// How tall the rows show: more of them than fit scroll.
    let height: CGFloat

    var body: some View {
        let focus = CGFloat(tool.frame)
        // while it plays the grid holds still, the playhead's outline going over its frames
        let held = tool.isPlaying ? tool.playHeld.map { CGFloat($0) } : nil
        let at = grid.dragged ?? held ?? focus
        let playhead = (width / 2).rounded()
        let last = max(Int(info.duration) - 1, 0) + Grid.pastEnd
        let sz = grid.sizes
        let origin = playhead - sz.cellWidth / 2 - at * sz.pitch
        // as far out as it zooms, the whole timeline shows whichever frame the playhead is on
        let least = GridState.leastZoom(frames: Int(info.duration), room: playhead - Grid.labelsWidth - sz.cellWidth)
        let frames = info.duration
        let shownWidth = width
        let only = grid.playOnly
        let _ = tool.gridFrames = { at in
            // (a group the grid was fitted to plays alone)
            only ?? Self.frames(shownAt: CGFloat(at), of: frames, playhead: playhead, width: shownWidth, sizes: sz)
        }
        // the frames on the bar, and some either side for the slide to a frame tapped
        let lo = min(max(Int((-origin / sz.pitch).rounded(.down)) - 14, 0), last)
        let hi = max(min(Int(((width - origin) / sz.pitch).rounded(.up)) + 14, last), lo)
        let all = max(rows.height, sz.rowHeight)
        // more tracks than show: they scroll up and down
        let over = max(all - height, 0)
        let place = GridPlace(
            origin: origin, playhead: playhead, sizes: sz, rows: rows, last: last, scroll: min(grid.rowsOffset, over), over: over,
            inset: max(((height - all) / 2).rounded(), 0)
        )
        VStack(spacing: 0) {
            GridRuler(tool: tool, grid: grid, fps: Int(info.fps), lo: lo, hi: hi, place: place, width: width)
                // the flap where the timeline ends
                .overlay(alignment: .topLeading) {
                    GridEndFlap(tool: tool, grid: grid, info: info, place: place)
                }
                .clipped()
            rowsArea(place, lo: lo, hi: hi, all: all, head: held == nil ? nil : origin + focus * sz.pitch)
        }
        // two fingers on the frames: zoomed and carried along
        .background(GridPinchProbe(
            began: { grid.pinchBegan(tool: tool) },
            changed: { scale, shift in grid.pinchChanged(scale: scale, shift: shift, least: least, last: last, over: over, tool: tool) },
            ended: { speed, opening in grid.pinchEnded(speed: speed, opening: opening, least: least, last: last, tool: tool) }
        ))
        // a frame tapped or stepped to slides under the playhead, as does the one playing
        // stopped on; dragged, it follows the finger
        .animation(tool.isPlaying || grid.dragged != nil ? nil : Motion.frameSlide, value: at)
        .onChange(of: tool.frame) { _, now in
            // the handles stay with the frame they are on: gone when the playhead leaves it
            if let p = tool.picked, let c = tool.clip(p)?.1, now < c.start || now >= c.start + c.duration {
                tool.picked = nil
            }
        }
        .onChange(of: tool.selecting == nil) { _, off in
            if off {
                grid.groupMenu = nil
                grid.groupTrim = nil
            }
        }
        // the track drawn on is kept in sight where not every track shows
        .onChange(of: info.track) { _, _ in reveal(over: over, animated: true) }
        .onChange(of: rows.count) { _, _ in reveal(over: over, animated: true) }
        .onAppear { reveal(over: over, animated: false) }
    }

    /// The timeline's frames whose cells show whole, right of the tracks' names, with frame `at`
    /// under the playhead; none of them in sight, none.
    static func frames(shownAt at: CGFloat, of frames: UInt32, playhead: CGFloat, width: CGFloat, sizes: GridSizes) -> Range<UInt32>? {
        let origin = playhead - sizes.cellWidth / 2 - at * sizes.pitch
        let first = max(Int(((Grid.labelsWidth - origin) / sizes.pitch).rounded(.up)), 0)
        let last = min(Int(((width - origin - sizes.cellWidth) / sizes.pitch).rounded(.down)), Int(frames) - 1)
        guard first <= last else { return nil }
        return UInt32(first) ..< UInt32(last + 1)
    }

    /// Scrolls the tracks up or down as little as brings the one drawn on wholly into sight.
    private func reveal(over: CGFloat, animated: Bool) {
        guard let row = rows.index(of: info.track) else { return }
        let top = rows[row].y
        let now = min(grid.rowsOffset, over)
        let next = min(max(min(now, top), top + rows[row].height - height), over)
        guard abs(next - grid.rowsOffset) > 0.5 else { return }
        if animated {
            withAnimation(Motion.easeInOut(0.2)) { grid.rowsOffset = max(next, 0) }
        } else {
            grid.rowsOffset = max(next, 0)
        }
    }

    /// (`head`: where the playhead's outline stands while the grid holds still under it, playing)
    private func rowsArea(_ place: GridPlace, lo: Int, hi: Int, all: CGFloat, head: CGFloat?) -> some View {
        let sz = place.sizes
        return ZStack(alignment: .topLeading) {
            GridCells(tool: tool, info: info, grid: grid, rows: rows, lo: lo, hi: hi)
                .offset(x: place.origin)
                .frame(width: width, height: all, alignment: .topLeading)
                // fading out under the tracks' names
                .mask {
                    HStack(spacing: 0) {
                        LinearGradient(
                            stops: [.init(color: .clear, location: 0), .init(color: .clear, location: 0.45), .init(color: .black, location: 1)],
                            startPoint: .leading, endPoint: .trailing
                        )
                        .frame(width: Grid.labelsWidth)
                        Rectangle()
                    }
                }
            // past the timeline's end: shaded, since nothing there plays
            let end = place.origin + CGFloat(grid.lengthDraft ?? Int(info.duration)) * sz.pitch - sz.gap / 2
            if end < width {
                Grid.panel.opacity(0.55)
                    .frame(width: width - max(end, 0), height: all)
                    .overlay(alignment: .leading) { Grid.line.frame(width: 1) }
                    .offset(x: max(end, 0))
                    .allowsHitTesting(false)
            }
            // an empty frame held for its menu: ringed
            if let spot = grid.emptyMenu, let row = rows.index(of: spot.track) {
                RoundedRectangle(cornerRadius: sz.corner + 2, style: .continuous)
                    .strokeBorder(Grid.inkBright.opacity(0.85), lineWidth: 2)
                    .frame(width: sz.cellWidth + 5, height: sz.cellHeight + 5)
                    .offset(x: place.origin + CGFloat(spot.frame) * sz.pitch - 2.5, y: rows[row].y + (rows[row].height - sz.cellHeight) / 2 - 2.5)
                    .allowsHitTesting(false)
            }
            // the playhead: an outline on the chosen track's frame, where it stands whatever
            // slides under it (a frame with its handles out is outlined whole instead)
            if tool.selecting == nil, tool.picked.flatMap({ tool.clip($0) }) == nil,
               let row = rows.index(of: info.track) {
                RoundedRectangle(cornerRadius: sz.corner + 2, style: .continuous)
                    .strokeBorder(Pc.accent, lineWidth: 2.5)
                    .frame(width: sz.cellWidth + 5, height: sz.cellHeight + 5)
                    .offset(x: (head ?? place.playhead - sz.cellWidth / 2) - 2.5, y: rows[row].y + (rows[row].height - sz.cellHeight) / 2 - 2.5)
                    .allowsHitTesting(false)
            }
            // a dragged edge held at a limit: a line down the tracks there
            if let limit = grid.trim?.limit ?? grid.groupTrim?.limit {
                Pc.accent
                    .frame(width: 1.5, height: all)
                    .offset(x: place.origin + limit * sz.pitch - 0.75)
                    .allowsHitTesting(false)
            }
            // a dragged block over the line between two rows: dropped there, a track of its own
            if let line = grid.move?.between {
                Line()
                    .stroke(Pc.accent, style: StrokeStyle(lineWidth: 2, lineCap: .round, dash: [7, 6]))
                    .frame(width: width - Grid.labelsWidth + 40, height: 2)
                    .offset(x: Grid.labelsWidth - 40, y: line.y - 1)
                    .allowsHitTesting(false)
            }
            GridStrokeView(grid: grid)
                .frame(width: width, height: all, alignment: .topLeading)
            // the tracks' names, standing still over the frames
            ForEach(rows.list) { r in
                // (a row several groups' tracks share is named for the group the playhead is in)
                if !r.lanes.isEmpty {
                    GridKeyLabel(tool: tool, lanes: r.lanes, depth: r.depth, height: r.height)
                        .offset(y: r.y)
                } else {
                    let t = r.track(at: Int(tool.frame))
                    GridTrackLabel(tool: tool, track: t, chosen: t.id == info.track, height: r.height)
                        .offset(y: r.y)
                }
            }
        }
        .frame(width: width, height: all, alignment: .topLeading)
        .offset(y: place.inset - place.scroll)
        .frame(width: width, height: height, alignment: .topLeading)
        .clipped()
        .contentShape(Rectangle())
        .coordinateSpace(name: Grid.space)
        // a finger held still on a frame: its menu (see `GridHoldProbe`)
        .background(GridHoldProbe { point in pressed(at: point, place) })
        // one gesture for every touch on the frames (a tap, a drag sideways or up and down, a
        // stroke, a handle), so each is told from the others as it goes
        .gesture(
            DragGesture(minimumDistance: 0, coordinateSpace: .named(Grid.space))
                .onChanged { touched($0, place) }
                .onEnded { lifted($0, place) }
        )
    }

    // MARK: A finger on the frames

    private func touched(_ drag: DragGesture.Value, _ place: GridPlace) {
        // two fingers down: they zoom and move the grid, and this finger alone does nothing more
        if grid.pinching || grid.afterPinch {
            grid.touch = .ignore
            grid.touchStart = drag.startLocation
            return
        }
        let edges = GridEdges(tool: tool, grid: grid, rows: rows)
        if grid.touch != nil, grid.touchStart.map({ hypot($0.x - drag.startLocation.x, $0.y - drag.startLocation.y) > 3 }) ?? true {
            // the last touch never ended (the system took it): what it was doing is dropped
            grid.touch = nil
            grid.stroke = []
            if grid.trim != nil || grid.groupTrim != nil {
                tool.endEdgeDrag()
                grid.trim = nil
                grid.groupTrim = nil
            }
            if grid.move != nil {
                grid.move = nil
                grid.moveTask?.cancel()
                grid.moveTask = nil
                grid.dragged = nil
            }
        }
        // (as points of all the rows, however far they are scrolled)
        let start = place.inRows(drag.startLocation)
        let here = place.inRows(drag.location)
        if grid.touch == nil {
            grid.touchStart = drag.startLocation
            // over the tracks' names, the touch is theirs
            guard start.x >= Grid.labelsWidth - 12 else {
                grid.touch = .ignore
                return
            }
            // a touch on the frames while they play stops them there
            if tool.isPlaying {
                grid.touch = .ignore
                tool.stopPlaying(stay: true)
                return
            }
            // down on a grip beside the keys picked: they stretch about the far end
            if let grips = GridKeyRows.grips(tool: tool, rows: rows),
               let leading = grips.grip(at: CGPoint(x: start.x - place.origin, y: start.y), sizes: place.sizes) {
                grid.touch = .stretch
                grid.stretching = (leading ? grips.last : grips.first, leading ? grips.first : grips.last)
                grid.closeMenus(tool)
                return
            }
            // down on a handle: its edge follows the finger
            if let leading = edges.handle(at: CGPoint(x: start.x - place.origin, y: start.y)) {
                grid.touch = .handle(leading: leading)
                return
            }
            grid.touch = .undecided
            grid.scrollFrom = grid.dragged ?? CGFloat(tool.frame)
            grid.panFrom = place.scroll
        }
        switch grid.touch {
        case .undecided:
            let moved = drag.translation
            guard hypot(moved.width, moved.height) > 6 else { return }
            if tool.selecting != nil {
                grid.touch = .stroke
                grid.closeMenus(tool)
                grid.faded = []
                grid.stroke = [start]
                grid.touched = []
                grid.touchedKeys = []
                grid.strokeBefore = tool.selecting
                take(start, place)
                stroked(to: here, place)
            } else if abs(moved.width) >= abs(moved.height) || place.over <= 0 {
                grid.touch = .scroll
                grid.closeMenus(tool)
                grid.scrolled(moved.width, last: place.last, tool: tool)
            } else {
                grid.touch = .pan
                grid.closeMenus(tool)
                grid.rowsOffset = min(max(grid.panFrom - moved.height, 0), place.over)
            }
        case .scroll:
            grid.scrolled(drag.translation.width, last: place.last, tool: tool)
        case .pan:
            grid.rowsOffset = min(max(grid.panFrom - drag.translation.height, 0), place.over)
        case .stroke:
            stroked(to: here, place)
        case .handle(let leading):
            // (a tap on a handle does nothing)
            if grid.trim != nil || grid.groupTrim != nil || abs(drag.translation.width) > 1 {
                edges.dragged(leading: leading, dx: drag.translation.width)
            }
        case .held:
            // dragged on from the hold: the block held is picked up, and its menu goes; a key
            // held goes along its row, with the keys picked
            if hypot(drag.translation.width, drag.translation.height) > 8 {
                if let key = grid.heldKey {
                    grid.closeMenus(tool)
                    grid.touch = .keys
                    tool.lanes.drag(key, by: Int((drag.translation.width / place.sizes.pitch).rounded()))
                } else if moveBegan() {
                    grid.touch = .move
                    moved(drag, place)
                }
            }
        case .keys:
            if let key = grid.heldKey {
                tool.lanes.drag(key, by: Int((drag.translation.width / place.sizes.pitch).rounded()))
            }
        case .stretch:
            // as far from the far end as the finger has taken the grip, against how far it was
            if let (anchor, from) = grid.stretching, from != anchor {
                let now = CGFloat(from) + drag.translation.width / place.sizes.pitch
                let factor = Double((now - CGFloat(anchor)) / CGFloat(from - anchor))
                tool.lanes.stretch(anchor: UInt32(anchor), factor: max(factor, 0.05))
            }
        case .move:
            moved(drag, place)
        default:
            break
        }
    }

    private func lifted(_ drag: DragGesture.Value, _ place: GridPlace) {
        let was = grid.pinching || grid.afterPinch ? .ignore : grid.touch
        grid.touch = nil
        if !grid.pinching {
            grid.afterPinch = false
        }
        switch was {
        case .undecided:
            tapped(place.inRows(drag.startLocation), place)
        case .scroll:
            grid.scrollEnded(drag.predictedEndTranslation.width, last: place.last, tool: tool)
        case .pan:
            // carried on a little as it was going
            withAnimation(Motion.easeOut(0.25)) {
                grid.rowsOffset = min(max(grid.panFrom - drag.predictedEndTranslation.height, 0), place.over)
            }
        case .stroke:
            grid.endStroke()
        case .handle:
            GridEdges(tool: tool, grid: grid, rows: rows).ended()
        case .move:
            moveEnded()
        case .keys, .stretch:
            tool.lanes.endDrag()
            grid.stretching = nil
        default:
            break
        }
        grid.heldClip = nil
        grid.heldKey = nil
    }

    /// The clip at `point`, with its row's track.
    func clip(at point: CGPoint, _ place: GridPlace) -> (TimelineTrackInfo, TimelineClipInfo)? {
        // (on a row of groups' tracks, only the frames a group lasts are anyone's)
        guard let cell = place.cell(at: point), rows[cell.row].has(cell.frame) else { return nil }
        let t = rows[cell.row].track(at: cell.frame)
        let f = UInt32(cell.frame)
        guard let c = t.clips.first(where: { $0.start <= f && f < $0.start + $0.duration }) else { return nil }
        return (t, c)
    }

    /// A tap: with the lasso on, the frame there is picked with the others or let go; else the
    /// playhead goes to it on its track, or, there already, the frame's handles come out or go.
    private func tapped(_ point: CGPoint, _ place: GridPlace) {
        if grid.menuOpen(tool) {
            grid.closeMenus(tool)
            return
        }
        // a row of keys: the key there picked with the others or let go; elsewhere on it, all let go
        if let cell = place.cell(at: point), !rows[cell.row].lanes.isEmpty {
            if let key = tool.key(in: rows[cell.row].lanes, at: cell.frame) {
                tool.lanes.tap(key)
            } else {
                tool.lanes.clear()
            }
            return
        }
        guard let cell = place.cell(at: point), rows[cell.row].has(cell.frame) else {
            tool.picked = nil
            return
        }
        let t = rows[cell.row].track(at: cell.frame)
        let found = clip(at: point, place)?.1
        if tool.selecting == nil, !tool.isPlaying, let found, found.kind == .group {
            // its chevron: its own tracks' rows shown, or put away
            if point.x - (place.origin + CGFloat(found.start) * place.sizes.pitch) < 30 {
                tool.fold(found.id, tool.unfolded(found.id))
                return
            }
            // tapped three times: the grid is zoomed to it, and it alone plays
            let now = Date()
            let again = grid.taps.clip == found.id && now.timeIntervalSince(grid.taps.at) < 0.45
            grid.taps = (found.id, again ? grid.taps.count + 1 : 1, now)
            if grid.taps.count == 3 {
                grid.taps.count = 0
                fit(found, playhead: place.playhead)
                return
            }
        } else if grid.playOnly != nil {
            grid.playOnly = nil
        }
        if tool.selecting != nil {
            if let found {
                tool.selecting?.formSymmetricDifference([found.id])
                UISelectionFeedbackGenerator().selectionChanged()
            }
            return
        }
        guard !tool.isPlaying else {
            tool.stopPlaying()
            return
        }
        if t.id == info.track, UInt32(cell.frame) == tool.frame {
            guard let found, !tool.held(t) else {
                tool.picked = nil
                return
            }
            tool.picked = tool.picked == found.id ? nil : found.id
            UISelectionFeedbackGenerator().selectionChanged()
        } else {
            tool.picked = nil
            tool.selectTrack(t.id)
            if UInt32(cell.frame) != tool.frame {
                tool.goTo(UInt32(cell.frame))
            }
        }
    }

    /// The grid zoomed so that group `c` fills it, its middle frame under the playhead: playing
    /// then goes round its frames alone, until the grid is slid or zoomed again.
    private func fit(_ c: TimelineClipInfo, playhead: CGFloat) {
        let room = max(playhead - Grid.labelsWidth - 8, 44)
        let zoom = room / (CGFloat(max(c.duration, 1)) / 2) / GridSizes.pitch
        tool.picked = nil
        tool.goTo(c.start + c.duration / 2)
        grid.ease(zoomTo: min(max(zoom, GridSizes.leastPitch / GridSizes.pitch), GridSizes.most))
        grid.playOnly = c.start ..< c.start + c.duration
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }

    /// A finger held still at `point` of the rows as they show (a drag only says where it is once
    /// it moves, so the hold comes from `GridHoldProbe`): unless it is already dragging something
    /// or on a handle, the frame there is held.
    private func pressed(at point: CGPoint, _ place: GridPlace) {
        guard grid.touch == nil || grid.touch == .undecided, point.x >= Grid.labelsWidth - 12 else { return }
        let inRows = place.inRows(point)
        guard GridEdges(tool: tool, grid: grid, rows: rows).handle(at: CGPoint(x: inRows.x - place.origin, y: inRows.y)) == nil else { return }
        // the lift that ends the hold is no tap
        grid.touch = .held
        grid.touchStart = point
        grid.heldClip = nil
        held(at: inRows, place)
    }

    /// A finger held still on a frame: its menu, standing on the bar over it; with the lasso on,
    /// the picked frames' menu when it is one of them.
    private func held(at point: CGPoint, _ place: GridPlace) {
        guard !tool.isPlaying else { return }
        // a key: its menu (with the keys picked, if it is one of them)
        if let cell = place.cell(at: point), !rows[cell.row].lanes.isEmpty {
            if let key = tool.key(in: rows[cell.row].lanes, at: cell.frame) {
                tool.lanes.take(key)
                grid.heldKey = key
                grid.closeMenus(tool)
                grid.keyMenu = GridKeySpot(key: key, x: point.x)
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
            }
            return
        }
        guard let (_, c) = clip(at: point, place) else {
            // an empty frame: its own menu
            if tool.selecting == nil, let cell = place.cell(at: point), rows[cell.row].has(cell.frame) {
                grid.closeMenus(tool)
                grid.emptyMenu = GridEmptySpot(track: rows[cell.row].track(at: cell.frame).id, frame: UInt32(cell.frame), x: point.x)
                UIImpactFeedbackGenerator(style: .light).impactOccurred()
            }
            return
        }
        grid.heldClip = c.id
        if let chosen = tool.selecting {
            guard chosen.contains(c.id) else { return }
            grid.groupMenu = point.x
        } else {
            tool.trackOpen = nil
            tool.settingsOpen = false
            tool.clipMenu = ClipMenu(clip: c.id, x: point.x)
        }
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }

    /// The stroke on to `point`: every frame between the last point and this one is picked, or,
    /// picked already, let go (each once a stroke).
    private func stroked(to point: CGPoint, _ place: GridPlace) {
        let last = grid.stroke.last ?? point
        let far = hypot(point.x - last.x, point.y - last.y)
        let steps = max(Int(far / 4), 1)
        for i in 1 ... steps {
            let t = CGFloat(i) / CGFloat(steps)
            take(CGPoint(x: last.x + (point.x - last.x) * t, y: last.y + (point.y - last.y) * t), place)
        }
        grid.stroke.append(point)
    }

    private func take(_ point: CGPoint, _ place: GridPlace) {
        // (across a row of keys, each key it crosses)
        if let cell = place.cell(at: point), !rows[cell.row].lanes.isEmpty {
            if let key = tool.key(in: rows[cell.row].lanes, at: cell.frame), grid.touchedKeys.insert(key).inserted {
                tool.lanes.toggle(key)
            }
            return
        }
        guard let (t, c) = clip(at: point, place), !tool.held(t), grid.touched.insert(c.id).inserted else { return }
        tool.selecting?.formSymmetricDifference([c.id])
        UISelectionFeedbackGenerator().selectionChanged()
    }
}

/// A line along the middle of its frame.
private struct Line: Shape {
    func path(in rect: CGRect) -> Path {
        Path { p in
            p.move(to: CGPoint(x: rect.minX, y: rect.midY))
            p.addLine(to: CGPoint(x: rect.maxX, y: rect.midY))
        }
    }
}

/// Tells of a finger held still on the view it is behind: where (in that view), after 0.4 s.
/// SwiftUI's drag gesture says nothing of a finger until it moves, and its long press doesn't say
/// where it is, so a press recognizer of UIKit's own watches the window for touches that come down
/// here, beside whatever else takes them.
private struct GridHoldProbe: UIViewRepresentable {
    let held: (CGPoint) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeUIView(context: Context) -> Probe {
        let view = Probe()
        view.isUserInteractionEnabled = false
        view.coordinator = context.coordinator
        context.coordinator.held = held
        return view
    }

    func updateUIView(_ view: Probe, context: Context) {
        context.coordinator.held = held
    }

    static func dismantleUIView(_ view: Probe, coordinator: Coordinator) {
        coordinator.detach()
    }

    final class Probe: UIView {
        weak var coordinator: Coordinator?

        override func didMoveToWindow() {
            super.didMoveToWindow()
            coordinator?.attach(from: self)
        }
    }

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        var held: (CGPoint) -> Void = { _ in }
        private weak var probe: UIView?
        private weak var window: UIWindow?
        private var press: UILongPressGestureRecognizer?

        func attach(from view: UIView) {
            probe = view
            guard view.window !== window else { return }
            detach()
            guard let found = view.window else { return }
            let made = UILongPressGestureRecognizer(target: self, action: #selector(pressed(_:)))
            made.minimumPressDuration = 0.4
            made.allowableMovement = 6
            // whatever else the touch is doing goes on as it was
            made.cancelsTouchesInView = false
            made.delaysTouchesBegan = false
            made.delaysTouchesEnded = false
            made.delegate = self
            found.addGestureRecognizer(made)
            window = found
            press = made
        }

        func detach() {
            if let press {
                window?.removeGestureRecognizer(press)
            }
            press = nil
            window = nil
        }

        @objc private func pressed(_ press: UILongPressGestureRecognizer) {
            guard press.state == .began, let probe else { return }
            held(press.location(in: probe))
        }

        /// Only touches that come down on the view it is behind.
        func gestureRecognizer(_ g: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
            guard let probe else { return false }
            return probe.bounds.contains(touch.location(in: probe))
        }

        func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
            true
        }
    }
}

/// The frame numbers, sliding with the frames: the playhead's bright (in the accent with the
/// lasso on). Where the frames are too narrow for every number, every second, fifth, tenth… is
/// written with a tick at the rest; narrower still, the seconds are. Dragged, the frames slide; a
/// number tapped, the playhead goes to it.
private struct GridRuler: View {
    let tool: TimelineTool
    let grid: GridState
    let fps: Int
    let lo: Int
    let hi: Int
    let place: GridPlace
    let width: CGFloat

    /// How many frames from one number to the next, so that each has `room` points: every
    /// frame's where they fit.
    private static func every(_ pitch: CGFloat, room: CGFloat) -> Int {
        [1, 2, 5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000].first { CGFloat($0) * pitch >= room } ?? 10000
    }

    var body: some View {
        let current = Int(tool.frame)
        let lit = tool.selecting != nil ? Pc.accent : Grid.inkBright
        let sz = place.sizes
        let pitch = sz.pitch
        let fps = max(fps, 1)
        let every = Self.every(pitch, room: 26)
        // numbers rarer than four a second say less than the seconds do
        let seconds = every >= 5 && every * 4 >= fps
        // (in seconds: how many from one written to the next)
        let apart = [1, 2, 5, 10, 15, 30, 60, 120, 300, 600].first { CGFloat($0 * fps) * pitch >= 44 } ?? 1200
        Canvas { ctx, size in
            let mid = size.height / 2 + 3
            func x(_ f: Int) -> CGFloat { CGFloat(f - lo) * pitch + sz.cellWidth / 2 }
            func write(_ text: String, at f: Int, lit here: Bool) {
                let number = Text(verbatim: text)
                    .font(.system(size: 11, weight: here ? .bold : .regular).monospacedDigit())
                    .foregroundColor(here ? lit : Grid.inkDim)
                ctx.draw(number, at: CGPoint(x: x(f), y: mid))
            }
            var ticks = Path()
            func tick(_ f: Int, _ tall: CGFloat) {
                ticks.addRect(CGRect(x: x(f) - 0.5, y: size.height - tall - 2, width: 1, height: tall))
            }
            // (a number too near the playhead's own gives way to it)
            func clear(_ f: Int) -> Bool { abs(CGFloat(f - current) * pitch) >= 30 }
            if seconds {
                // a tick each second (each half where there is room), a number where they fit
                let half = fps % 2 == 0 && CGFloat(fps) * pitch >= 28 ? fps / 2 : fps
                var f = (lo / half) * half
                while f <= hi {
                    if f >= lo, f != current {
                        let second = f % fps == 0 ? f / fps : nil
                        if let second, second % apart == 0 {
                            if clear(f) {
                                write("\(second)s", at: f, lit: false)
                            }
                        } else {
                            tick(f, second != nil ? 5 : 3)
                        }
                    }
                    f += half
                }
            } else {
                for f in lo ... hi where f != current {
                    if (f + 1) % every == 0 {
                        if every == 1 || clear(f) {
                            write("\(f + 1)", at: f, lit: false)
                        }
                    } else if pitch >= 5 {
                        tick(f, 3)
                    }
                }
            }
            ctx.fill(ticks, with: .color(Grid.inkDim.opacity(0.7)))
            if current >= lo, current <= hi {
                write("\(current + 1)", at: current, lit: true)
            }
        }
        .frame(width: CGFloat(hi - lo + 1) * sz.pitch, height: Grid.rulerHeight)
        .offset(x: CGFloat(lo) * sz.pitch + place.origin)
        .frame(width: width, height: Grid.rulerHeight, alignment: .topLeading)
        .clipped()
        .contentShape(Rectangle())
        .onTapGesture { location in
            let frame = Int(((location.x - place.origin) / sz.pitch).rounded(.down))
            guard frame >= 0, frame <= place.last, UInt32(frame) != tool.frame else { return }
            grid.closeMenus(tool)
            tool.picked = nil
            tool.goTo(UInt32(frame))
        }
        .gesture(
            DragGesture(minimumDistance: 6)
                .onChanged { drag in
                    guard grid.trim == nil, grid.groupTrim == nil, !grid.pinching else { return }
                    if tool.isPlaying {
                        tool.stopPlaying(stay: true)
                    }
                    if grid.touch != .scroll {
                        grid.touch = .scroll
                        grid.scrollFrom = grid.dragged ?? CGFloat(tool.frame)
                        grid.closeMenus(tool)
                    }
                    grid.scrolled(drag.translation.width, last: place.last, tool: tool)
                }
                .onEnded { drag in
                    guard grid.touch == .scroll else { return }
                    grid.touch = nil
                    grid.scrollEnded(drag.predictedEndTranslation.width, last: place.last, tool: tool)
                }
        )
    }
}

/// The lasso's stroke: a glowing line while it is drawn, fading once lifted.
private struct GridStrokeView: View {
    let grid: GridState

    var body: some View {
        ZStack(alignment: .topLeading) {
            if grid.stroke.count > 1 {
                line(grid.stroke)
            }
            if grid.faded.count > 1 {
                line(grid.faded).opacity(grid.fading ? 0 : 1)
            }
        }
        .allowsHitTesting(false)
    }

    private func line(_ points: [CGPoint]) -> some View {
        let path = Path { $0.addLines(points) }
        return ZStack {
            path.stroke(Pc.accent, style: StrokeStyle(lineWidth: 5, lineCap: .round, lineJoin: .round))
                .shadow(color: Pc.accent, radius: 6)
            path.stroke(Color.white.opacity(0.9), style: StrokeStyle(lineWidth: 2, lineCap: .round, lineJoin: .round))
        }
    }
}

/// A track's name in its capsule, with its checkbox: tapped, the track is drawn on; its box
/// tapped, it shows or hides (held, it shows alone); held, its menu opens; held and dragged, it
/// moves up or down among the others.
private struct GridTrackLabel: View {
    let tool: TimelineTool
    let track: TimelineTrackInfo
    let chosen: Bool
    /// Its row's height: in a row shorter than rows start, it is drawn that much smaller.
    let height: CGFloat

    var body: some View {
        let scale = height / GridSizes.rowHeight
        let label = track.label.map { Color(rgb: $0) }
        let text = label != nil ? Grid.onLabel : chosen ? Grid.inkBright : Grid.ink
        HStack(spacing: 6) {
            CheckBox(checked: !track.hidden, fill: label != nil ? Grid.onLabel : track.soloed ? Pc.accent : chosen ? Grid.inkBright : Grid.box)
                .frame(width: 26, height: 30)
                .contentShape(Rectangle())
                .onTapGesture { tool.toggleHidden(track) }
                // before the name's hold, which would open the track's menu too
                .highPriorityGesture(LongPressGesture(minimumDuration: 0.4).onEnded { _ in tool.solo(track.id) })
            Text(verbatim: track.name)
                .font(.system(size: 14, weight: .medium))
                .foregroundStyle(text)
                .lineLimit(1)
            if track.locked {
                PcIconView(PcIcons.lock, size: 14, color: text)
            }
            // its blocks have keys: their rows put out under it, or away
            if tool.keyed(track) {
                let out = tool.gridKeyTracks.contains(track.id)
                PcIconView(out ? PcIcons.chevronDown : PcIcons.chevronRight, size: 13, color: out ? Pc.accent : text)
                    .frame(width: 22, height: 30)
                    .contentShape(Rectangle())
                    .onTapGesture { tool.toggleKeyRows(track.id) }
                    .accessibilityLabel(Text(verbatim: out ? "Hide keys" : "Show keys"))
            }
        }
        .padding(.leading, 6)
        .padding(.trailing, tool.keyed(track) ? 6 : 14)
        .frame(height: 32)
        .background(Capsule().fill(label ?? (chosen ? Grid.capsule : Color.clear)))
        // a highlighted track drawn on: ringed, since its capsule is its colour either way
        .overlay {
            if chosen, label != nil {
                Capsule().strokeBorder(Grid.inkBright.opacity(0.75), lineWidth: 1.5)
            }
        }
        .frame(maxWidth: Grid.labelsWidth - 34, alignment: .leading)
        .fixedSize(horizontal: true, vertical: false)
        .contentShape(Capsule())
        .onTapGesture { tool.selectTrack(track.id) }
        .modifier(TrackHeaderGestures(tool: tool, track: track, rowHeight: height))
        .scaleEffect(scale, anchor: .leading)
        .frame(height: height)
        .padding(.leading, 12 + tool.indent(track.id) * scale)
        .opacity(track.hidden ? 0.6 : 1)
    }
}

import SwiftUI
import UIKit

// The timeline at the bottom of the canvas, once the artwork is on one: the tool that reads it from
// the core and changes it, which the grid (TimelineGrid.swift and the files beside it) shows.

private let cardHeight: CGFloat = 40
private let barGap: CGFloat = 25
private let barInset: CGFloat = 26
private let rowHeight: CGFloat = 44
private let rulerHeight: CGFloat = 24
private let thumbSide: UInt32 = 96
private let mostHold = 60
private let mostOnion = 12

private var barFill: Color { Look.shared.light ? Color(argb: 0xFFE4_E4E4) : Color(argb: 0xFF2A_2A2A) }
private var trackFill: Color { Look.shared.light ? Color(argb: 0xFFD0_D0D0) : Color(argb: 0xFF47_4747) }
private var trackEdge: Color { Look.shared.light ? Color(argb: 0xFFBD_BDBD) : Color(argb: 0xFF5C_5C5C) }
private var ink: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }
private var inkDim: Color { Look.shared.light ? Color(argb: 0x8039_3939) : Color(argb: 0x80C5_C5C5) }
private let cardEdge = Color(argb: 0xFF9A_9A9A)
private let rowLineFill = Color(argb: 0x09FF_FFFF)

/// A held clip's short menu: whose, and where along the bar (points from its left edge) it
/// points down at.
struct ClipMenu: Equatable {
    var clip: UInt64
    var x: CGFloat
}

/// The timeline as the core keeps it: its tracks and clips, the playhead, playing.
@Observable
final class TimelineTool {
    @ObservationIgnored private unowned let model: CanvasModel

    private(set) var info: TimelineInfo?
    /// Bottom first, as the core gives them: an unfolded group's own just below the track
    /// holding it (see TimelineGroupRows.swift).
    private(set) var tracks: [TimelineTrackInfo] = []
    /// Where each track shown sits among the unfolded groups, by track.
    private(set) var rowPlaces: [UInt64: TimelineRow] = [:]
    /// Each drawing's picture, by its layer: the whole canvas.
    private(set) var thumbnails: [UInt64: UIImage] = [:]
    /// What each folded group clip shows on each of its frames: the grid draws its block's
    /// pictures from the drawings there.
    private(set) var groupShots: [UInt64: [TimelineGroupShot]] = [:]
    /// How many drawings' pictures have been drawn for the panel, and the seconds spent drawing
    /// them (the refresh-cost debug scene counts them).
    @ObservationIgnored private(set) var thumbnailsDrawn = 0
    @ObservationIgnored private(set) var picturesTime = 0.0
    /// The clip picked in the full view (its edge trims it, the header acts on it).
    var picked: UInt64? {
        didSet {
            if picked != oldValue {
                autokeyPickedChanged()
            }
            if clipMenu != nil, clipMenu?.clip != picked {
                clipMenu = nil
            }
        }
    }
    /// Playing (or scrubbing): the frame shown.
    private(set) var shownFrame: UInt32?
    private(set) var isPlaying = false
    /// The frame standing under the grid's playhead while it plays: the grid holds still there,
    /// the playhead's outline going over the frames instead.
    private(set) var playHeld: UInt32?
    /// The frames the grid shows with frame `at` under its playhead (the grid sets it as it is
    /// laid out; none of the timeline's in sight, none): playing goes round them.
    @ObservationIgnored var gridFrames: ((UInt32) -> Range<UInt32>?)?
    /// The settings' card open over the bar.
    var settingsOpen = false { didSet { if settingsOpen { trackOpen = nil; clipMenu = nil } } }
    /// How new keys ease into the next.
    var keyEasing: TimelineEasing = .smooth { didSet { autokeyEasingChanged() } }
    /// Autokey: the transform tool keys the pose of the drawing at the playhead.
    var autokey = false
    /// The tracks whose key rows are out under them; and whether a move is about to be performed
    /// (the next touch on the canvas starts the take) or is being.
    var gridKeyTracks: Set<UInt64> = []
    var performArmed = false
    var performing = false
    /// How tall the grid's bar stands from the bottom of the screen: what stands on the bottom
    /// edge with it (the transform tool's bar, while Animate keys with it) stands above it.
    var gridHeight: CGFloat = 0
    /// Each clip's keys, by clip.
    private(set) var keys: [UInt64: [TimelineKeyInfo]] = [:]
    /// A block held: its menu standing on the bar over it. The other menus close as it opens, and
    /// it closes as they open.
    var clipMenu: ClipMenu? {
        didSet {
            if clipMenu != nil {
                settingsOpen = false
                trackOpen = nil
            }
        }
    }
    /// The lasso on: the frames picked together (a tap takes one in or out, a stroke every one it
    /// crosses), to move, lengthen, group, copy, cut or delete as one. None: it is off.
    var selecting: Set<UInt64>?
    /// The track whose menu is open (its name held), and the track being dragged up or down by
    /// its name, how far (see TimelineMenus.swift).
    var trackOpen: UInt64? { didSet { if trackOpen != nil { clipMenu = nil } } }
    var trackDrag: TrackDrag?
    /// The onion skins either side, and what the timeline clipboard holds.
    private(set) var onion: TimelineOnion?
    private(set) var clipboard = TimelineClipboardInfo(clips: 0, keys: false)
    /// Counts reads from the session, for what is read through it rather than kept here (a pose).
    private(set) var revision = 0
    /// The keys picked in the rows of keys.
    let lanes: TimelineKeyLanes
    /// A frame's edge being dragged.
    var edgeDragging = false

    @ObservationIgnored private var playing: Task<Void, Never>?
    @ObservationIgnored private var reading: Task<Void, Never>?
    /// How many onion skins each side had before they were last turned off.
    @ObservationIgnored private var onionKept: (UInt32, UInt32) = (1, 1)
    /// The number of the panel's last reading (see `refresh`); the camera's and the sounds' lane
    /// keys it gave; each drawing's picture version, and the version of the picture drawn of each.
    @ObservationIgnored private var panelRead: UInt64 = 0
    @ObservationIgnored private var laneVolumes: [UInt64: [TimelineLaneKey]] = [:]
    @ObservationIgnored private var pictures: [TimelinePicture] = []
    @ObservationIgnored private var pictureVersions: [UInt64: UInt64] = [:]

    init(model: CanvasModel) {
        self.model = model
        lanes = TimelineKeyLanes(model: model)
    }

    /// The canvas it is the timeline of.
    var canvasModel: CanvasModel { model }

    /// What stands behind a clip's picture: the canvas's own background (white while it is
    /// hidden), so the picture reads as the drawing does on the canvas.
    var paper: Color {
        model.background.visible ? Color(rgb: model.backgroundRgb()) : .white
    }

    /// Its sounds and videos.
    var media: TimelineMediaTool { model.media }

    var on: Bool { info != nil }

    /// The frame in the middle: the one played or scrubbed past, else the playhead.
    var frame: UInt32 { shownFrame ?? info?.frame ?? 0 }

    /// The track drawn on.
    var track: TimelineTrackInfo? {
        guard let id = info?.track else { return tracks.first }
        return tracks.first { $0.id == id }
    }

    func clip(_ id: UInt64) -> (TimelineTrackInfo, TimelineClipInfo)? {
        for t in tracks {
            if let c = t.clips.first(where: { $0.id == id }) {
                return (t, c)
            }
        }
        return nil
    }

    /// The clip of the track drawn on at the playhead.
    var currentClip: TimelineClipInfo? {
        let at = frame
        return track?.clips.first { $0.start <= at && at < $0.start + $0.duration }
    }

    /// Reads it again from the session, after any change: in one call, which gives nothing when
    /// nothing the panel shows has changed since it last read it, and each part kept as it is
    /// unless it changed (so the views showing it aren't made again).
    func refresh() {
        if let panel = model.session.timelinePanel(known: panelRead) {
            panelRead = panel.generation
            take(panel)
        }
        guard info != nil else {
            if !tracks.isEmpty {
                tracks = []
            }
            if picked != nil {
                picked = nil
            }
            if trackOpen != nil {
                trackOpen = nil
            }
            if settingsOpen {
                settingsOpen = false
            }
            stopPlaying()
            return
        }
        if let p = picked, clip(p) == nil {
            picked = nil
        }
        // clips that have gone are no longer picked with the others
        if let chosen = selecting {
            let kept = chosen.filter { clip($0) != nil }
            if kept != chosen {
                selecting = kept
            }
        }
        if let t = trackOpen, !tracks.contains(where: { $0.id == t }) {
            trackOpen = nil
        }
        lanes.refreshed(keys: keys, camera: [], volumes: laneVolumes)
        revision += 1
        readThumbnails()
    }

    /// The panel's reading kept: each part set only where it differs from what is shown.
    private func take(_ p: TimelinePanel) {
        if info != p.info {
            info = p.info
        }
        let shown = p.rows.map(\.track)
        if tracks != shown {
            tracks = shown
        }
        let places = Dictionary(p.rows.map { ($0.track.id, $0) }, uniquingKeysWith: { a, _ in a })
        if rowPlaces != places {
            rowPlaces = places
        }
        if clipboard != p.clipboard {
            clipboard = p.clipboard
        }
        if onion != p.onion {
            onion = p.onion
        }
        let read = Dictionary(p.keys.map { ($0.id, $0.keys) }, uniquingKeysWith: { a, _ in a })
        if keys != read {
            keys = read
        }
        laneVolumes = Dictionary(p.volumes.map { ($0.id, $0.keys) }, uniquingKeysWith: { a, _ in a })
        pictures = p.pictures
        let shots = Dictionary(p.groups.map { ($0.id, $0.shots) }, uniquingKeysWith: { a, _ in a })
        if groupShots != shots {
            groupShots = shots
        }
    }

    /// Where clip `id` is at the playhead, as its keys put it.
    func pose(_ id: UInt64) -> TimelinePose? {
        model.session.timelinePose(id: id)
    }

    /// A key of `channel` at the playhead for clip `id`: shown at once, one undo step when `done`.
    func setKey(_ id: UInt64, _ channel: TimelineChannel, _ values: [Float], done: Bool) {
        stopPlaying()
        _ = model.session.setTimelineKey(id: id, channel: channel, values: values, easing: keyEasing, done: done)
        if done {
            model.refresh()
        }
    }

    func removeKeys(_ id: UInt64) {
        _ = model.session.removeTimelineKeys(id: id)
        model.refresh()
    }

    /// The clip whose motion path shows over the canvas: the picked one while its keys are open;
    /// on the grid, the one under the playhead on the chosen track while Animate is on or that
    /// track's key rows are out.
    var pathClip: UInt64? {
        guard autokey || (track.map { gridKeyTracks.contains($0.id) } ?? false) else { return nil }
        return currentClip?.id
    }

    /// That clip's motion path.
    var path: MotionPath? {
        guard let id = pathClip else { return nil }
        _ = keys
        return model.session.timelineMotionPath(id: id)
    }

    /// How clip `id`'s keys at `frame` ease into the next.
    func easing(_ id: UInt64, at frame: UInt32) -> EasingSpec? {
        _ = keys
        return model.session.timelineKeyEasing(id: id, frame: frame)
    }

    /// Clip `id`'s keys at `frame` ease as `spec`: shown at once, one undo step when `done`.
    func setEasing(_ id: UInt64, at frame: UInt32, _ spec: EasingSpec, done: Bool) {
        _ = model.session.setTimelineKeyEasing(id: id, frame: frame, s: spec, done: done)
        if done {
            model.refresh()
        }
    }

    /// Handles on clip `id`'s position key at `frame`, along the way from the key before it to
    /// the key after: the path curves through it.
    func bendPath(_ id: UInt64, at frame: UInt32) {
        guard let keys = model.session.timelineMotionPath(id: id)?.keys, let i = keys.firstIndex(where: { $0.frame == frame }) else { return }
        let before = keys[max(i - 1, 0)], after = keys[min(i + 1, keys.count - 1)]
        var dx = (after.x - before.x) / 6, dy = (after.y - before.y) / 6
        if dx == 0, dy == 0 {
            // a lone key: handles across, a quarter of the canvas
            dx = Float(model.session.info().width) / 8
            dy = 0
        }
        _ = model.session.setTimelinePathHandles(id: id, frame: frame, handles: [-dx, -dy, dx, dy], done: true)
        model.refresh()
    }

    /// The simple easing a key at `frame` has, to keep it when the key is moved.
    private func keyEasingAt(_ id: UInt64, _ frame: UInt32) -> TimelineEasing {
        (keys[id] ?? []).first { $0.frame == frame && $0.channel == .position }?.easing ?? keyEasing
    }

    /// Clip `id`'s position key at `frame` moved to canvas point `to` (the pivot there): the
    /// playhead goes to it first. Shown at once, one undo step when `done`.
    func moveKey(_ id: UInt64, at frame: UInt32, by delta: CGPoint, from start: TimelinePose, done: Bool) {
        if info?.frame != frame {
            model.session.setTimelineFrame(frame: frame)
        }
        let values = [start.x + Float(delta.x), start.y + Float(delta.y)]
        _ = model.session.setTimelineKey(id: id, channel: .position, values: values, easing: keyEasingAt(id, frame), done: done)
        if done {
            model.refresh()
        }
    }

    func setHandles(_ id: UInt64, at frame: UInt32, _ handles: [Float], done: Bool) {
        _ = model.session.setTimelinePathHandles(id: id, frame: frame, handles: handles, done: done)
        if done {
            model.refresh()
        }
    }

    /// Draws again the pictures of the drawings whose version has changed since theirs was drawn
    /// (or that have none yet); a picture that can't be drawn is tried again on the next read.
    private func readThumbnails() {
        let stale = pictures.filter { pictureVersions[$0.root] != $0.version }
        guard !stale.isEmpty else { return }
        reading?.cancel()
        let session = model.session
        reading = Task { @MainActor [weak self] in
            for picture in stale {
                let id = picture.root
                let (made, took) = await Task.detached(priority: .utility) {
                    let start = CFAbsoluteTimeGetCurrent()
                    let made = session.frameThumbnail(id: id, maxSide: thumbSide).flatMap { CanvasModel.image($0) }
                    return (made, CFAbsoluteTimeGetCurrent() - start)
                }.value
                guard let self, !Task.isCancelled else { return }
                self.thumbnailsDrawn += 1
                self.picturesTime += took
                self.thumbnails[id] = made
                self.pictureVersions[id] = made == nil ? nil : picture.version
            }
        }
    }

    /// The artwork on a timeline (Animation Assist's frames becoming its drawings), or off it.
    func setOn(_ on: Bool) {
        stopPlaying()
        _ = on ? model.session.startTimeline() : model.session.stopTimeline()
        model.refresh()
    }

    func change(_ edit: (inout TimelineSettings) -> Void) {
        guard let i = info else { return }
        var s = TimelineSettings(
            fps: i.fps, duration: i.duration, exposure: i.exposure, playback: i.playback,
            onionFrames: i.onionFrames, onionOpacity: i.onionOpacity
        )
        edit(&s)
        model.session.setTimelineSettings(s: s)
        info = model.session.timelineInfo()
    }

    /// Draws on frame `frame` (playing stops): a layer of the drawing there becomes current. It
    /// may be an empty frame a little past the timeline's end, which the grid shows.
    func goTo(_ frame: UInt32) {
        stopPlaying()
        model.session.restTimelineFrame(frame: frame)
        model.refresh()
    }

    /// Scrubbing: frame `frame` shows as it plays.
    func scrub(_ frame: UInt32) {
        if shownFrame == nil {
            stopPlaying()
        }
        if shownFrame != frame {
            media.blip(frame)
        }
        shownFrame = frame
        model.session.setTimelinePlaying(frame: frame)
    }

    /// Scrubbing ends on `frame`, which is drawn on.
    func endScrub(_ frame: UInt32) {
        shownFrame = nil
        model.session.setTimelinePlaying(frame: nil)
        goTo(frame)
    }

    func selectTrack(_ id: UInt64) {
        guard info?.track != id else { return }
        model.session.selectTimelineTrack(id: id)
        model.refresh()
    }

    func addDrawing() {
        stopPlaying()
        if model.session.addTimelineDrawing() == 0 {
            model.showNotice(track?.locked == true ? "The track is locked." : "There's no room for another layer.", warning: true)
        }
        model.refresh()
    }

    func addTrack() {
        stopPlaying()
        _ = model.session.addTimelineTrack()
        model.refresh()
    }

    func deleteTrack(_ id: UInt64) {
        _ = model.session.deleteTimelineTrack(id: id)
        model.refresh()
    }

    func toggleHidden(_ t: TimelineTrackInfo) {
        _ = model.session.setTimelineTrack(id: t.id, hidden: !t.hidden, locked: t.locked)
        model.refresh()
    }

    func toggleLocked(_ t: TimelineTrackInfo) {
        _ = model.session.setTimelineTrack(id: t.id, hidden: t.hidden, locked: !t.locked)
        model.refresh()
    }

    func split(_ clip: UInt64) {
        if !model.session.splitTimelineClip(id: clip) {
            model.showNotice("Move the playhead inside the clip to split it there.", warning: true)
        }
        model.refresh()
    }

    /// Clip `id` taken out of the group open, onto the level the group is on.
    func moveOutOfGroup(_ id: UInt64) {
        if !model.session.moveTimelineClipOutOfGroup(id: id) {
            model.showNotice("It can't be moved out: its track is locked.", warning: true)
        }
        model.refresh()
    }

    /// Clip `id` masked by the clip on the track below as `mask`.
    func setMask(_ id: UInt64, _ mask: TimelineMask) {
        _ = model.session.setTimelineClipMask(id: id, mask: mask)
        model.refresh()
    }

    /// Starts picking clips together, the picked one first.
    func startSelecting() {
        stopPlaying()
        selecting = Set(picked.map { [$0] } ?? [])
        settingsOpen = false
    }

    /// The clips picked together made into a group, which is picked.
    func makeGroup() {
        let ids = Array(selecting ?? [])
        selecting = nil
        guard !ids.isEmpty else { return }
        let made = model.session.groupTimelineClips(ids: ids)
        if made == 0 {
            model.showNotice("Only clips shown together can be grouped.", warning: true)
        } else {
            picked = made
        }
        model.refresh()
    }

    func ungroup(_ id: UInt64) {
        picked = nil
        _ = model.session.ungroupTimelineClip(id: id)
        model.refresh()
    }

    /// Group `id` plays its first `length` frames over and over (0: once).
    func setCycle(_ id: UInt64, length: UInt32, continueAcross: Bool) {
        _ = model.session.setTimelineCycle(id: id, length: length, continueAcross: continueAcross)
        model.refresh()
    }

    /// The clips copied, at the playhead on the track drawn on (those above on the tracks above);
    /// the first pasted is picked.
    func pasteClips() {
        stopPlaying()
        let made = model.session.pasteTimelineClips()
        if made.isEmpty {
            model.showNotice(track?.locked == true ? "The track is locked." : "There's no room for more layers.", warning: true)
        } else {
            picked = made.first
        }
        model.refresh()
    }

    /// Clip `id`'s keys on the playhead's frame (`here`) or all of them, copied.
    func copyKeys(_ id: UInt64, here: Bool) {
        if model.session.copyTimelineKeys(id: id, here: here) {
            model.showNotice("Keys copied. Paste lays them on the clip at the playhead.")
        }
        model.refresh()
    }

    /// The keys copied, onto the clip at the playhead on the track drawn on, from the playhead on.
    func pasteKeys() {
        stopPlaying()
        if !model.session.pasteTimelineKeys() {
            model.showNotice("Move the playhead onto a clip to paste keys onto it.", warning: true)
        }
        model.refresh()
    }

    /// Plays at `fps` with its timing kept: every clip, key and marker moves to match.
    func retime(_ fps: UInt32) {
        stopPlaying()
        _ = model.session.retimeTimeline(fps: fps)
        model.refresh()
    }

    /// Plays, or stops on the frame it got to.
    func togglePlaying() {
        isPlaying ? stopPlaying(stay: true) : play()
    }

    private func play() {
        guard let first = info, first.duration > 0 else { return }
        let session = model.session
        settingsOpen = false
        // the frames the grid shows are the ones played; with none of the timeline's in sight
        // (the playhead resting past its end), those showing from its first frame
        var here = frame
        var shown = gridFrames?(here)
        if gridFrames != nil, shown == nil {
            goTo(0)
            here = 0
            shown = gridFrames?(0)
        }
        let range = shown ?? 0 ..< first.duration
        isPlaying = true
        playHeld = here
        playing?.cancel()
        playing = Task { @MainActor [weak self] in
            var at = range.contains(here) ? here : range.lowerBound
            var step: Int32 = 1
            // each frame on time by the clock, so the sounds stay with the pictures
            var due = ContinuousClock.now + .seconds(self?.media.play(from: at) ?? 0)
            while !Task.isCancelled {
                guard let self, let info = self.info else { return }
                self.shownFrame = at
                session.setTimelinePlaying(frame: at)
                due += .seconds(1 / Double(max(info.fps, 1)))
                try? await Task.sleep(until: due, clock: .continuous)
                guard let next = session.timelineNextStepWithin(at: at, step: step, first: range.lowerBound, end: range.upperBound) else {
                    // a single pass ended: the playhead stays on its last frame
                    self.stopPlaying(stay: true)
                    return
                }
                due += .seconds(self.media.follow(from: at, to: next.at, step: next.step))
                at = next.at
                step = next.step
            }
        }
    }

    /// Playing stops. With `stay`, the playhead stays on the frame it stopped on, rather than
    /// going back to the one drawn on before.
    func stopPlaying(stay: Bool = false) {
        let at = isPlaying ? shownFrame : nil
        playing?.cancel()
        playing = nil
        isPlaying = false
        playHeld = nil
        media.stop()
        if shownFrame != nil {
            shownFrame = nil
            model.session.setTimelinePlaying(frame: nil)
        }
        if stay, let at {
            goTo(at)
        }
    }

    /// Onion skins off, or back to as many as each side had (its colours as they were).
    func toggleOnionSkin() {
        guard let o = onion else { return }
        if o.before > 0 || o.after > 0 {
            onionKept = (o.before, o.after)
            setOnion { $0.before = 0; $0.after = 0 }
        } else {
            let (before, after) = onionKept
            setOnion { $0.before = before; $0.after = after }
        }
    }
}

/// The timeline at the bottom of the canvas: simple mode's strip, or every track.
struct TimelineBar: View {
    let model: CanvasModel
    let screen: CGSize

    var body: some View {
        if let info = model.timeline.info {
            GridTimeline(model: model, tool: model.timeline, info: info, screen: screen)
        }
    }
}

// MARK: - Every track

/// The space the bar's pages stand in, and where along it the header's words that open one
/// are, by name: a page's arrow points down at its word.
private let barSpace = "timelineBar"

extension View {
}

// MARK: - Motion path

/// The picked clip's motion path over the canvas while its keys are open: the path its pivot
/// takes, a dot at each position key, and a bent key's two handles on lines to it. A key's dot is
/// tapped to go to its frame or dragged to move the key there; a handle's is dragged to bend the
/// path, the other handle mirroring it. Only the dots take touches: the canvas keeps the rest.
struct MotionPathOverlay: View {
    let model: CanvasModel

    /// The path as a drag changes it, before the change is let go.
    @State private var live: MotionPath?
    /// The drag under way: the key's frame, and where it (or the handle) and the pose started.
    @State private var dragging: (frame: UInt32, start: CGPoint, pose: TimelinePose?)?

    var body: some View {
        let tool = model.timeline
        if let id = tool.pathClip, !tool.isPlaying, tool.shownFrame == nil,
           let path = live ?? tool.path, !path.keys.isEmpty {
            let screen = { (x: Float, y: Float) in model.canvasToWindow(CGPoint(x: CGFloat(x), y: CGFloat(y))) }
            ZStack(alignment: .topLeading) {
                Canvas { context, _ in
                    var line = Path()
                    for i in 0 ..< path.points.count / 2 {
                        let at = screen(path.points[i * 2], path.points[i * 2 + 1])
                        if i == 0 { line.move(to: at) } else { line.addLine(to: at) }
                    }
                    context.stroke(line, with: .color(.white.opacity(0.7)), lineWidth: 4)
                    context.stroke(line, with: .color(Pc.accent), lineWidth: 2)
                    for key in path.keys {
                        let at = screen(key.x, key.y)
                        if let h = key.handles, h.count == 4 {
                            for (dx, dy) in [(h[0], h[1]), (h[2], h[3])] {
                                let end = screen(key.x + dx, key.y + dy)
                                var arm = Path()
                                arm.move(to: at)
                                arm.addLine(to: end)
                                context.stroke(arm, with: .color(Pc.accent), lineWidth: 1.5)
                                dot(context, at: end, radius: 5)
                            }
                        }
                        dot(context, at: at, radius: 6.5, current: key.frame == tool.frame)
                    }
                }
                .allowsHitTesting(false)
                ForEach(path.keys, id: \.frame) { key in
                    if let h = key.handles, h.count == 4 {
                        grip(at: screen(key.x + h[0], key.y + h[1])) { change(id, key, handle: 0, $0, done: $1) }
                        grip(at: screen(key.x + h[2], key.y + h[3])) { change(id, key, handle: 1, $0, done: $1) }
                    }
                    grip(at: screen(key.x, key.y), tap: { tool.goTo(key.frame) }) { move(id, key, $0, done: $1) }
                }
            }
            // the view as it is, if it hasn't moved since the canvas opened
            .onAppear {
                if model.viewMatrix.isEmpty {
                    model.refreshView()
                }
            }
        }
    }

    /// Key `key` dragged to window point `to`: the playhead goes to it, and its position key moves
    /// by as much as the drag has gone (canvas pixels), one undo step when let go.
    private func move(_ id: UInt64, _ key: PathKey, _ to: CGPoint?, done: Bool) {
        let tool = model.timeline
        guard let to, let at = model.windowToCanvas(CGPoint(x: to.x * model.pixels, y: to.y * model.pixels)) else { return }
        if dragging == nil {
            tool.goTo(key.frame)
            dragging = (key.frame, at, tool.pose(id))
        }
        guard let d = dragging, let pose = d.pose else { return }
        // as the clip's keys count it, through the groups it is in
        let by = model.session.timelineMotionDelta(id: id, frame: d.frame, dx: Float(at.x - d.start.x), dy: Float(at.y - d.start.y))
        tool.moveKey(id, at: d.frame, by: CGPoint(x: CGFloat(by.first ?? 0), y: CGFloat(by.last ?? 0)), from: pose, done: done)
        settle(id, done: done)
    }

    /// A white dot ringed in the accent colour (filled with it on the playhead's key).
    private func dot(_ context: GraphicsContext, at: CGPoint, radius r: CGFloat, current: Bool = false) {
        context.fill(Path(ellipseIn: CGRect(x: at.x - r, y: at.y - r, width: r * 2, height: r * 2)), with: .color(Pc.accent))
        let inner = r - 2
        context.fill(
            Path(ellipseIn: CGRect(x: at.x - inner, y: at.y - inner, width: inner * 2, height: inner * 2)),
            with: .color(current ? Pc.accent : .white)
        )
    }

    /// Where a dot at `at` takes touches: a tap (if it has one), or a drag to the window point
    /// given (and whether it is let go).
    private func grip(at: CGPoint, tap: (() -> Void)? = nil, drag: @escaping (CGPoint?, Bool) -> Void) -> some View {
        Color.clear
            .frame(width: 40, height: 40)
            .contentShape(Circle())
            .gesture(
                DragGesture(minimumDistance: 0, coordinateSpace: .global)
                    .onChanged { g in
                        if dragging != nil || hypot(g.translation.width, g.translation.height) > 4 {
                            drag(g.location, false)
                        }
                    }
                    .onEnded { g in
                        if dragging == nil, hypot(g.translation.width, g.translation.height) <= 4 {
                            tap?()
                        } else {
                            drag(g.location, true)
                        }
                    }
            )
            .position(at)
    }

    /// Handle `handle` (0 toward the key before, 1 toward the key after) of key `key` dragged to
    /// window point `to`; the other mirrors it. One undo step when let go.
    private func change(_ id: UInt64, _ key: PathKey, handle: Int, _ to: CGPoint?, done: Bool) {
        guard let to, let at = model.windowToCanvas(CGPoint(x: to.x * model.pixels, y: to.y * model.pixels)) else { return }
        if dragging == nil {
            dragging = (key.frame, at, nil)
        }
        let dx = Float(at.x) - key.x, dy = Float(at.y) - key.y
        let handles = handle == 0 ? [dx, dy, -dx, -dy] : [-dx, -dy, dx, dy]
        model.timeline.setHandles(id, at: key.frame, handles, done: done)
        settle(id, done: done)
    }

    /// The path read again as it changes; let go, back to the timeline's own.
    private func settle(_ id: UInt64, done: Bool) {
        if done {
            dragging = nil
            live = nil
        } else {
            live = model.session.timelineMotionPath(id: id)
        }
    }
}

// MARK: - Easing

/// The small easing editor: how the keys at the playhead ease into the next, as a curve, its
/// kind, that kind's numbers, and how often it repeats (arriving each time, or looping back),
/// maybe backwards. Every change shows at once; a slider let go or a tap is one undo step.
struct EasingPage: View {
    let tool: TimelineTool
    let clip: UInt64
    let frame: UInt32
    /// Brings what has this id into view, where the page scrolls.
    var reveal: ((AnyHashable) -> Void)?
    let onBack: () -> Void

    /// The easing as a slider moves it, before it is let go.
    @State private var live: EasingSpec?

    private static let kinds: [(EasingKind, String)] = [
        (.linear, "Linear"), (.smooth, "Smooth"), (.cubicBezier, "Ease"), (.hold, "Hold"),
        (.bounce, "Bounce"), (.elastic, "Elastic"), (.steps, "Steps"), (.elasticSteps, "Springy Steps"),
        (.cyclic, "Cyclic"), (.random, "Random"), (.curve, "Custom"),
    ]

    /// Each kind's numbers: name, least, most.
    private static func controls(_ kind: EasingKind) -> [(String, Float, Float)] {
        switch kind {
        case .cubicBezier: [("Start pull", 0, 1), ("Start lift", -0.5, 1.5), ("End pull", 0, 1), ("End lift", -0.5, 1.5)]
        case .bounce: [("First bounce", 0.01, 1), ("Bounciness", 0.01, 0.99)]
        case .elastic: [("Wave length", 0.01, 1), ("Attack", 0, 2), ("Decay", 0, 1), ("Swing", 0, 1)]
        case .steps: [("Step length", 0.01, 1), ("Smoothing", 0, 1)]
        case .elasticSteps: [("Step length", 0.01, 1), ("Springiness", 0, 1)]
        case .cyclic: [("Cycle length", 0.01, 1), ("Sharpness", 0, 1), ("Turn", 0.01, 0.99), ("Drift", 0, 1)]
        case .random: [("Frequency", 0, 1), ("Magnitude", 0, 2), ("Seed", 0, 1), ("Fade", 0, 1), ("Warp", 0, 1), ("Tilt", 0, 1)]
        case .linear, .hold, .smooth, .curve: []
        }
    }

    var body: some View {
        let spec = live ?? tool.easing(clip, at: frame) ?? defaultEasing(kind: .linear)
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                PcIconView(PcIcons.chevronLeft, size: 34, color: Pc.slideValue)
                    .frame(width: 44, height: 34)
                    .contentShape(Rectangle())
                    .onTapGesture(perform: onBack)
                Text(verbatim: "Easing")
                    .font(.system(size: 23, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                Spacer(minLength: 0)
            }
            .frame(height: 34)
            .padding(.top, 6)
            // (a curve of your own is drawn by its points, and has more room for it)
            let drawn = spec.kind == .curve
            EasingCurveView(spec: spec) { next, done in
                drag(next)
                if done {
                    end()
                }
            }
            .frame(height: drawn ? 170 : 110)
            .padding(.top, 10)
            .padding(.bottom, drawn ? 4 : 10)
            if drawn {
                Text(verbatim: "Tap to add a point. Drag a point to shape the curve, or off the top or bottom to remove it.")
                    .font(.system(size: 12))
                    .foregroundStyle(inkDim)
                    .fixedSize(horizontal: false, vertical: true)
                    .padding(.horizontal, 6)
                    .padding(.bottom, 8)
            }
            LazyVGrid(columns: Array(repeating: GridItem(.flexible(), spacing: 4), count: 3), spacing: 4) {
                ForEach(Self.kinds, id: \.1) { kind, label in
                    let chosen = spec.kind == kind
                    Text(verbatim: label)
                        .font(.system(size: 14, weight: chosen ? .semibold : .regular))
                        .foregroundStyle(chosen ? Color.white : Pc.panelTitle)
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                        .frame(maxWidth: .infinity, minHeight: 30)
                        .background(RoundedRectangle(cornerRadius: 9, style: .circular).fill(chosen ? Pc.accent : Pc.actionsGroup))
                        .contentShape(Rectangle())
                        .onTapGesture {
                            var next = defaultEasing(kind: kind)
                            next.repeats = spec.repeats
                            next.looping = spec.looping
                            next.reversed = spec.reversed
                            commit(next)
                        }
                }
            }
            let controls = Self.controls(spec.kind)
            TlSliderGroup {
                ForEach(Array(controls.enumerated()), id: \.offset) { i, control in
                    let (label, least, most) = control
                    let value = i < spec.params.count ? spec.params[i] : least
                    TlSlider(label: label, value: String(format: "%.2f", value), share: (value - least) / (most - least),
                             onEnd: { end() }) { v in
                        var next = spec
                        while next.params.count <= i {
                            next.params.append(least)
                        }
                        next.params[i] = least + v * (most - least)
                        drag(next)
                    }
                }
                TlSlider(label: "Repeats", value: "\(spec.repeats)", share: Float(Int(spec.repeats) - 1) / 7, onEnd: { end() }) { v in
                    var next = spec
                    next.repeats = UInt32(1 + wholeNumber(v * 7))
                    drag(next)
                }
            }
            .padding(.top, 10)
            TlGroup {
                TlToggle(label: "Loop back", on: spec.looping) {
                    var next = spec
                    next.looping.toggle()
                    commit(next)
                }
                TlRowLine()
                TlToggle(label: "Backwards", on: spec.reversed) {
                    var next = spec
                    next.reversed.toggle()
                    commit(next)
                }
            }
            .padding(.top, 10)
            MyCurves(spec: spec, reveal: reveal) { commit($0) }
                .padding(.top, 14)
                .padding(.bottom, 10)
        }
        .onChange(of: frame) { live = nil }
    }

    private func drag(_ next: EasingSpec) {
        live = next
        tool.setEasing(clip, at: frame, next, done: false)
    }

    private func end() {
        guard let spec = live else { return }
        tool.setEasing(clip, at: frame, spec, done: true)
        live = nil
    }

    private func commit(_ next: EasingSpec) {
        live = nil
        tool.setEasing(clip, at: frame, next, done: true)
    }
}

/// An easing's curve: time across, how far along up, the start and end levels faint. A drawn
/// curve (`.curve`) shows the points it goes through, and they are its controls: one dragged
/// moves between its neighbours, a touch away from them all adds one there, and one let go
/// well above or below the frame is taken away. The first and last stay where they are.
private struct EasingCurveView: View {
    let spec: EasingSpec
    /// The easing as a point is moved, added or taken away, and whether the finger is up.
    var edit: ((EasingSpec, Bool) -> Void)?

    /// The point held (its place among the curve's), and whether it is far enough off the frame
    /// to be taken away when let go.
    @State private var held: Int?
    @State private var off = false
    /// The curve's points while one is held: what the touch has made of them so far (a touch can
    /// be over before the view is drawn again with them).
    @State private var working: [[Float]]?

    /// How near a touch is to a point to take it, how far off the frame takes one away, and the
    /// least two points are apart in time.
    private static let reach: CGFloat = 26
    private static let away: CGFloat = 30
    private static let apart: Float = 0.03

    /// A drawn curve's points as the engine reads them: in time's order, the ends pinned.
    private static func points(_ spec: EasingSpec) -> [[Float]] {
        var pts = stride(from: 0, to: spec.params.count - 1, by: 2).map {
            [min(max(spec.params[$0], 0), 1), min(max(spec.params[$0 + 1], -0.5), 1.5)]
        }
        pts.sort { $0[0] < $1[0] }
        guard pts.count >= 2 else { return [[0, 0], [1, 1]] }
        pts[0] = [0, 0]
        pts[pts.count - 1] = [1, 1]
        return pts
    }

    private static func place(_ p: [Float], _ size: CGSize) -> CGPoint {
        CGPoint(x: size.width * CGFloat(p[0]), y: size.height * CGFloat(1.5 - p[1]) / 2)
    }

    private func with(_ pts: [[Float]]) -> EasingSpec {
        var next = spec
        next.params = pts.flatMap { $0 }
        return next
    }

    private func touched(_ g: DragGesture.Value, _ size: CGSize) {
        guard let edit, size.width > 0, size.height > 0 else { return }
        var pts = working ?? Self.points(spec)
        let x = Float(g.location.x / size.width)
        let y = min(max(Float(1.5 - 2 * g.location.y / size.height), -0.5), 1.5)
        if held == nil {
            let inner = pts.indices.dropFirst().dropLast()
            let far = { (i: Int) -> CGFloat in
                let p = Self.place(pts[i], size)
                return hypot(p.x - g.startLocation.x, p.y - g.startLocation.y)
            }
            if let near = inner.min(by: { far($0) < far($1) }), far(near) <= Self.reach {
                held = near
            } else {
                // a new one, where there is room between two for it
                let i = pts.firstIndex { $0[0] > x } ?? pts.count - 1
                guard i > 0, pts[i][0] - pts[i - 1][0] >= 2 * Self.apart else { return }
                pts.insert([x, y], at: i)
                held = i
            }
        }
        guard let i = held, i > 0, i < pts.count - 1 else { return }
        let least = pts[i - 1][0] + Self.apart
        pts[i] = [min(max(x, least), max(pts[i + 1][0] - Self.apart, least)), y]
        off = g.location.y < -Self.away || g.location.y > size.height + Self.away
        working = pts
        edit(with(pts), false)
    }

    private func lifted() {
        defer {
            held = nil
            off = false
            working = nil
        }
        guard let edit, let i = held, var pts = working else { return }
        if off, i > 0, i < pts.count - 1 {
            pts.remove(at: i)
        }
        edit(with(pts), true)
    }

    var body: some View {
        let samples = easingCurve(s: spec, samples: 96)
        let drawn = spec.kind == .curve && edit != nil
        GeometryReader { place in
            Canvas { ctx, size in
                // -0.5 to 1.5 up the frame, so overshoots show
                let y = { (v: Float) in size.height * CGFloat(1.5 - min(max(v, -0.5), 1.5)) / 2 }
                for level: Float in [0, 1] {
                    ctx.fill(Path(CGRect(x: 0, y: y(level), width: size.width, height: 1)), with: .color(inkDim.opacity(0.4)))
                }
                var line = Path()
                for (i, v) in samples.enumerated() {
                    let p = CGPoint(x: size.width * CGFloat(i) / CGFloat(max(samples.count - 1, 1)), y: y(v))
                    i == 0 ? line.move(to: p) : line.addLine(to: p)
                }
                ctx.stroke(line, with: .color(Pc.accent.opacity(drawn && off ? 0.5 : 1)), lineWidth: 2.5)
                guard drawn else { return }
                let pts = Self.points(spec)
                for (i, p) in pts.enumerated() {
                    let at = Self.place(p, size)
                    let pinned = i == 0 || i == pts.count - 1
                    let r: CGFloat = pinned ? 4 : (i == held ? 9 : 7)
                    let dot = Path(ellipseIn: CGRect(x: at.x - r, y: at.y - r, width: 2 * r, height: 2 * r))
                    if pinned {
                        ctx.fill(dot, with: .color(inkDim))
                    } else {
                        let faint = i == held && off
                        ctx.fill(dot, with: .color(Pc.accent.opacity(faint ? 0.3 : 1)))
                        ctx.stroke(dot, with: .color(Color.white.opacity(faint ? 0.3 : 1)), lineWidth: 2)
                    }
                }
            }
            .contentShape(Rectangle())
            .gesture(
                drawn
                    ? DragGesture(minimumDistance: 0)
                    .onChanged { touched($0, place.size) }
                    .onEnded { _ in lifted() }
                    : nil
            )
        }
        .background(RoundedRectangle(cornerRadius: 14, style: .circular).fill(Pc.actionsGroup))
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .circular))
        // it stays up while one of the page's sliders is dragged: the curve is what they change
        .background {
            GeometryReader { place in
                let frame = place.frame(in: .global)
                Color.clear
                    .onAppear { TlSliding.shared.kept = frame }
                    .onChange(of: frame) { _, now in TlSliding.shared.kept = now }
            }
        }
        .onDisappear { TlSliding.shared.kept = nil }
    }
}

struct TlToggle: View {
    let label: String
    let on: Bool
    let action: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(Pc.panelTitle)
            Spacer(minLength: 0)
            PcSwitch(on: on)
        }
        .padding(.leading, 16)
        .padding(.trailing, 6)
        .frame(height: 44)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

// MARK: - Pieces

struct TlRowLine: View {
    var body: some View {
        rowLineFill.frame(height: 2)
    }
}

struct TlGroup<Rows: View>: View {
    @ViewBuilder let rows: () -> Rows

    var body: some View {
        VStack(spacing: 0) {
            rows()
        }
        .frame(maxWidth: .infinity)
        .background(Pc.actionsGroup)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .circular))
    }
}

struct TlSliderGroup<Rows: View>: View {
    var top: CGFloat = 8
    @ViewBuilder let rows: () -> Rows

    var body: some View {
        VStack(spacing: 11.5) {
            rows()
        }
        .padding(.top, top)
        .frame(maxWidth: .infinity)
        .background(Pc.actionsGroup)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .circular))
    }
}

/// The slider being dragged on a page standing on the bar, if one is: where its row is on the
/// screen. The page shows only that row meanwhile (see `TlPopover`), and with it `kept`: a part
/// of the page that shows what the slider changes (the easing editor's curve), where it is on the
/// screen while it is up.
@Observable
final class TlSliding {
    static let shared = TlSliding()
    var row: CGRect?
    var kept: CGRect?
}

/// A setting's name and value over a bar dragged or tapped along. Dragged (not only tapped) on a
/// page standing on the bar, the rest of the page gets out of the way until it is let go.
struct TlSlider: View {
    let label: String
    let value: String
    let share: Float
    var onEnd: () -> Void = {}
    let onChange: (Float) -> Void

    @GestureState private var dragged = false

    var body: some View {
        VStack(spacing: 0) {
            HStack(spacing: 0) {
                Text(verbatim: label)
                    .foregroundStyle(Pc.slideLabel)
                    .lineLimit(1)
                Spacer(minLength: 0)
                Text(verbatim: value)
                    .foregroundStyle(Pc.slideValue)
                    .lineLimit(1)
            }
            .font(.system(size: 17))
            .padding(.top, 6.5)
            GeometryReader { place in
                let w = place.size.width
                let x = CGFloat(min(max(share, 0), 1)) * w
                ZStack(alignment: .leading) {
                    Rectangle().fill(Pc.slideTrack).frame(height: 3)
                    Rectangle().fill(Pc.accent).frame(width: x, height: 3)
                    Capsule().fill(Color.white)
                        .frame(width: 16, height: 10)
                        .offset(x: min(max(x - 8, 0), max(w - 16, 0)))
                }
                .frame(width: w, height: 22)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .updating($dragged) { drag, held, _ in
                            if abs(drag.translation.width) > 4 {
                                held = true
                            }
                        }
                        .onChanged { onChange(Float(min(max($0.location.x / max(w, 1), 0), 1))) }
                        .onEnded { _ in onEnd() }
                )
                .onChange(of: dragged) { _, now in
                    // its whole row: the name and value above the bar, and the padding either side
                    let bar = place.frame(in: .global)
                    TlSliding.shared.row = now ? CGRect(x: bar.minX - 16, y: bar.minY - 30, width: bar.width + 32, height: 58) : nil
                }
                .onDisappear {
                    if dragged {
                        TlSliding.shared.row = nil
                    }
                }
            }
            .frame(height: 22)
        }
        .padding(.horizontal, 16)
        .frame(height: 55, alignment: .top)
    }
}

import SwiftUI
import UIKit

// The timeline at the bottom of the canvas, once the artwork is on one: in simple mode a flipbook
// strip of one track, as Animation Assist's was; otherwise every track, its clips laid out along a
// ruler of frames, with the playhead across them.

private let simpleWideWidth: CGFloat = 657
private let simpleUprightWidth: CGFloat = 446
private let simpleHeight: CGFloat = 96
private let cardHeight: CGFloat = 40
private let barGap: CGFloat = 25
private let barInset: CGFloat = 26
/// The full view: a frame's width to start with and at its widest (it is pinched wider or
/// narrower), a track's height, the ruler's, the track names' column.
private let startPitch: CGFloat = 30
private let mostPitch: CGFloat = 72
/// How near (points) a dragged clip's edge comes to another clip's, a marker or the playhead
/// before it snaps onto it.
private let snapReach: CGFloat = 10
private let rowHeight: CGFloat = 44
private let rulerHeight: CGFloat = 24
private let headerWidth: CGFloat = 196
/// How many tracks the timeline grows to show as they are added (dragged, it shows more).
private let mostRowsShown = 3
private let thumbSide: UInt32 = 96
private let mostHold = 60
private let mostExposure = 12
private let mostFps = 120
private let mostOnion = 12

private var barFill: Color { Look.shared.light ? Color(argb: 0xFFE4_E4E4) : Color(argb: 0xFF2A_2A2A) }
private var laneFill: Color { Look.shared.light ? Color(argb: 0xFFD6_D6D6) : Color(argb: 0xFF33_3333) }
private var laneChosen: Color { Look.shared.light ? Color(argb: 0xFFCC_CCCC) : Color(argb: 0xFF3B_3B3B) }
private var clipFill: Color { Look.shared.light ? Color(argb: 0xFFB8_B8B8) : Color(argb: 0xFF52_5252) }
private var clipEdge: Color { Look.shared.light ? Color(argb: 0xFFA0_A0A0) : Color(argb: 0xFF66_6666) }
private var trackFill: Color { Look.shared.light ? Color(argb: 0xFFD0_D0D0) : Color(argb: 0xFF47_4747) }
private var trackEdge: Color { Look.shared.light ? Color(argb: 0xFFBD_BDBD) : Color(argb: 0xFF5C_5C5C) }
private var ink: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }
private var inkDim: Color { Look.shared.light ? Color(argb: 0x8039_3939) : Color(argb: 0x80C5_C5C5) }
private let heldCard = Color(argb: 0xFFA3_A3A3)
private let cardEdge = Color(argb: 0xFF9A_9A9A)
private let rowLineFill = Color(argb: 0x09FF_FFFF)

/// The Clip page's parts, one shown at a time so the page stays short over the canvas: how the
/// clip looks, how it is masked, a group's cycle and tracks, and what is done with the clip.
enum ClipTab: Hashable {
    case look, mask, group, edit
}

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
    /// Each drawing's picture, by its layer: the whole canvas, as simple mode's cards show it.
    private(set) var thumbnails: [UInt64: UIImage] = [:]
    /// Each drawing's picture cut to what is painted on it, as the full view's clips show it; a
    /// drawing with nothing on it has none.
    private(set) var clipPictures: [UInt64: UIImage] = [:]
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
    var settingsOpen = false { didSet { if settingsOpen { trackOpen = nil; framesOpen = false; clipMenu = nil } } }
    var optionsOpen = false
    /// The picked clip's keys, open over the panel.
    var keysOpen = false { didSet { if keysOpen { trackOpen = nil; framesOpen = false; clipMenu = nil } } }
    /// How new keys ease into the next.
    var keyEasing: TimelineEasing = .smooth { didSet { autokeyEasingChanged() } }
    /// Autokey: the transform tool keys the pose of the drawing at the playhead.
    var autokey = false
    /// Each clip's keys, by clip.
    private(set) var keys: [UInt64: [TimelineKeyInfo]] = [:]
    /// The picked clip's options open over the panel: its mask, a group's cycle.
    var clipOpen = false { didSet { if clipOpen { trackOpen = nil; framesOpen = false; clipMenu = nil } } }
    /// Which part of the Clip page shows: it stays as it was left while the canvas is open.
    var clipTab: ClipTab = .look
    /// A clip held: its short menu (copy, cut, duplicate, split, delete) standing on the bar,
    /// pointing down at the clip. The other pages close as it opens, and it closes as they open.
    var clipMenu: ClipMenu? {
        didSet {
            if clipMenu != nil {
                settingsOpen = false
                keysOpen = false
                clipOpen = false
                cameraOpen = false
                trackOpen = nil
                framesOpen = false
            }
        }
    }
    /// Select mode: the clips picked together (a tap takes one in or out, a stroke across the
    /// tracks every clip it crosses), to move, group, copy, cut or delete as one. None: not
    /// selecting.
    var selecting: Set<UInt64>?
    /// The camera's frame shown over the canvas, dragged to key the camera at the playhead.
    var cameraOpen = false { didSet { if cameraOpen { trackOpen = nil; framesOpen = false; clipMenu = nil } } }
    /// The track whose page is open over the panel (held on its name), and the track being
    /// dragged up or down by its name, how far (see TimelineMenus.swift).
    var trackOpen: UInt64? { didSet { if trackOpen != nil { clipMenu = nil } } }
    var trackDrag: TrackDrag?
    /// The Frames page open over the panel; a loop being drawn on the ruler or an end of one
    /// dragged, and the timeline's end being dragged, shown before they are let go; the ruler
    /// held to draw a loop (its scrub then stands aside); which frames the full view shows (see
    /// TimelinePlayback.swift).
    var framesOpen = false { didSet { if framesOpen { trackOpen = nil; clipMenu = nil } } }
    var loopDraft: TimelineLoop?
    var lengthDraft: UInt32?
    @ObservationIgnored let rulerHold = TimelineRulerHold()
    @ObservationIgnored var shownFrames: () -> Range<UInt32>? = { nil }
    /// How far the full view's tracks are scrolled sideways (points), read from their scroll
    /// view as `shownFrames` is.
    @ObservationIgnored var scrolledBy: () -> CGFloat = { 0 }
    /// Where the header's words that open a page stand along the bar (points from its left
    /// edge), by name, as the header last laid them out.
    var headerWords: [String: CGFloat] = [:]
    /// The frames playing goes round, if some are looped; the onion skins either side.
    private(set) var loop: TimelineLoop?
    private(set) var onion: TimelineOnion?
    /// The camera at the playhead, and the frames it has keys on.
    private(set) var camera: TimelineCamera?
    private(set) var cameraKeys: [UInt32] = []
    /// The sync markers' frames, in order, and what the timeline clipboard holds.
    private(set) var markers: [UInt32] = []
    private(set) var clipboard = TimelineClipboardInfo(clips: 0, keys: false)
    /// Counts reads from the session, for what is read through it rather than kept here (a pose).
    private(set) var revision = 0
    /// The keys picked in the picked clip's key lanes.
    let lanes: TimelineKeyLanes
    /// The full view's zoom, a frame's width as it was last pinched to; how tall its tracks show
    /// (none: up to three of them); what a clip's dragged edge does at the next clip; and whether
    /// dragged clips and edges snap. All kept from one canvas to the next.
    private(set) var pitch: CGFloat = {
        let kept = UserDefaults.standard.double(forKey: "timelinePitch")
        return kept > 0 ? CGFloat(kept) : startPitch
    }()
    private(set) var shownHeight: CGFloat? = (UserDefaults.standard.object(forKey: "timelineHeight") as? Double).map { CGFloat($0) }
    private(set) var edgePolicy: TimelineEdgePolicy = {
        switch UserDefaults.standard.string(forKey: "timelineEdgePolicy") {
        case "stop": .stop
        case "trim": .trim
        default: .push
        }
    }()
    private(set) var snapping = UserDefaults.standard.object(forKey: "timelineSnapping") as? Bool ?? true
    /// A clip's edge being dragged, and whether a second finger has turned the edge policy over
    /// for that drag (see TimelineTrimFlip.swift).
    var edgeDragging = false
    var edgeFlipped = false

    @ObservationIgnored private var playing: Task<Void, Never>?
    @ObservationIgnored private var reading: Task<Void, Never>?
    /// How many onion skins each side had before they were last turned off.
    @ObservationIgnored private var onionKept: (UInt32, UInt32) = (1, 1)
    /// The number of the panel's last reading (see `refresh`); the camera's and the sounds' lane
    /// keys it gave; each drawing's picture version, and the version of the picture drawn of each.
    @ObservationIgnored private var panelRead: UInt64 = 0
    @ObservationIgnored private var laneCamera: [TimelineLaneKey] = []
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
            if optionsOpen {
                optionsOpen = false
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
        if picked == nil {
            if keysOpen {
                keysOpen = false
            }
            // with clips copied, the Clip page stays open to paste them
            if clipOpen, clipboard.clips == 0 {
                clipOpen = false
            }
        }
        if let t = trackOpen, !tracks.contains(where: { $0.id == t }) {
            trackOpen = nil
        }
        lanes.refreshed(keys: keys, camera: laneCamera, volumes: laneVolumes)
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
            let before = tracks.count
            tracks = shown
            // tracks added (not a canvas opening): show them
            if before > 0, shown.count > before {
                showAddedRows()
            }
        }
        let places = Dictionary(p.rows.map { ($0.track.id, $0) }, uniquingKeysWith: { a, _ in a })
        if rowPlaces != places {
            rowPlaces = places
        }
        if camera != p.camera {
            camera = p.camera
        }
        if cameraKeys != p.cameraKeys {
            cameraKeys = p.cameraKeys
        }
        if markers != p.markers {
            markers = p.markers
        }
        if clipboard != p.clipboard {
            clipboard = p.clipboard
        }
        if loop != p.loopFrames {
            loop = p.loopFrames
        }
        if onion != p.onion {
            onion = p.onion
        }
        let read = Dictionary(p.keys.map { ($0.id, $0.keys) }, uniquingKeysWith: { a, _ in a })
        if keys != read {
            keys = read
        }
        laneCamera = p.cameraLane
        laneVolumes = Dictionary(p.volumes.map { ($0.id, $0.keys) }, uniquingKeysWith: { a, _ in a })
        pictures = p.pictures
    }

    /// Where clip `id` is at the playhead, as its keys put it.
    func pose(_ id: UInt64) -> TimelinePose? {
        model.session.timelinePose(id: id)
    }

    /// A frame's width as the full view shows it: as pinched, but no narrower than fits all
    /// `frames` in `width` (a short timeline never narrower than it starts), nor wider than is any
    /// use.
    func pitchShown(frames: Int, width: CGFloat) -> CGFloat {
        let least = min(width / CGFloat(max(frames, 1) + 8), startPitch)
        return min(max(pitch, least), mostPitch)
    }

    /// Zooms the full view to `wanted` points a frame, within what `pitchShown` allows.
    func zoom(to wanted: CGFloat, frames: Int, width: CGFloat) {
        let least = min(width / CGFloat(max(frames, 1) + 8), startPitch)
        let next = min(max(wanted, least), mostPitch)
        guard next != pitch else { return }
        pitch = next
        UserDefaults.standard.set(Double(next), forKey: "timelinePitch")
    }

    /// How tall the tracks show (none: up to three of them): the ruler, the camera's lane and
    /// the tracks. The key lanes stand on top of that while they are open, so opening them takes
    /// no room from the tracks.
    func setShownHeight(_ height: CGFloat?) {
        shownHeight = height
        if let height {
            UserDefaults.standard.set(Double(height), forKey: "timelineHeight")
        } else {
            UserDefaults.standard.removeObject(forKey: "timelineHeight")
        }
    }

    /// Tracks were added: the tracks show taller to take them in, up to `mostRowsShown` of them,
    /// unless the grabber already left them that tall.
    private func showAddedRows() {
        guard let height = shownHeight else { return }
        let wanted = rulerHeight + cameraLaneHeight + CGFloat(min(tracks.count, mostRowsShown)) * rowHeight
        if height < wanted {
            setShownHeight(wanted)
        }
    }

    func setEdgePolicy(_ policy: TimelineEdgePolicy) {
        edgePolicy = policy
        let name = switch policy {
        case .stop: "stop"
        case .push: "push"
        case .trim: "trim"
        }
        UserDefaults.standard.set(name, forKey: "timelineEdgePolicy")
        if policy != .stop {
            UserDefaults.standard.set(name, forKey: "timelineFreeEdgePolicy")
        }
    }

    func setSnapping(_ on: Bool) {
        snapping = on
        UserDefaults.standard.set(on, forKey: "timelineSnapping")
    }

    /// Clip `id`'s start (`leading`) or end dragged to timeline frame `frame`, as the edge policy
    /// says at the next clip (turned over if a second finger did): one undo step.
    func trim(_ id: UInt64, leading: Bool, to frame: Int) {
        stopPlaying()
        _ = model.session.trimTimelineClip(id: id, leading: leading, frame: UInt32(max(frame, 0)), policy: dragPolicy)
        model.refresh()
    }

    /// Its track's clips as that trim would leave them, nothing changed; empty if it would do
    /// nothing.
    func trimPreview(_ id: UInt64, leading: Bool, to frame: Int) -> [TimelineClipInfo] {
        model.session.timelineTrimPreview(id: id, leading: leading, frame: UInt32(max(frame, 0)), policy: dragPolicy)
    }

    /// The frame clip `id` would start on if moved to track `track` near `start`; none if it
    /// can't go there.
    func landing(_ id: UInt64, on track: UInt64, start: Int) -> Int? {
        model.session.timelineMoveLanding(id: id, track: track, start: UInt32(max(start, 0))).map { Int($0) }
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

    /// The picked clip's motion path, while its keys are open.
    var path: MotionPath? {
        guard keysOpen, let id = picked else { return nil }
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

    func straightenPath(_ id: UInt64, at frame: UInt32) {
        _ = model.session.setTimelinePathHandles(id: id, frame: frame, handles: nil, done: true)
        model.refresh()
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

    /// The simple easing a key at `frame` has, to keep it when the key is moved.
    private func keyEasingAt(_ id: UInt64, _ frame: UInt32) -> TimelineEasing {
        (keys[id] ?? []).first { $0.frame == frame && $0.channel == .position }?.easing ?? keyEasing
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
                let (made, cut, took) = await Task.detached(priority: .utility) {
                    let start = CFAbsoluteTimeGetCurrent()
                    let whole = session.frameThumbnail(id: id, maxSide: thumbSide)
                    let made = whole.flatMap { CanvasModel.image($0) }
                    let cut = whole.flatMap { ClipPicture.cut($0, of: id, from: session) }
                    return (made, cut, CFAbsoluteTimeGetCurrent() - start)
                }.value
                guard let self, !Task.isCancelled else { return }
                self.thumbnailsDrawn += 1
                self.picturesTime += took
                self.thumbnails[id] = made
                self.clipPictures[id] = cut
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

    func setSimple(_ simple: Bool) {
        settingsOpen = false
        optionsOpen = false
        model.session.setTimelineSimple(on: simple)
        refresh()
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

    /// Draws on frame `frame` (playing stops): a layer of the drawing there becomes current.
    func goTo(_ frame: UInt32) {
        stopPlaying()
        model.session.setTimelineFrame(frame: frame)
        model.refresh()
    }

    /// Scrubbing: frame `frame` shows as it plays.
    func scrub(_ frame: UInt32) {
        if shownFrame == nil {
            stopPlaying()
            optionsOpen = false
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

    /// Clip `clip` picked: its track drawn on, and the playhead onto it unless it is there. While
    /// selecting, it is taken in with the others or out instead.
    func pick(_ clip: TimelineClipInfo, on track: TimelineTrackInfo) {
        if selecting != nil {
            selecting?.formSymmetricDifference([clip.id])
            return
        }
        clipMenu = nil
        stopPlaying()
        picked = clip.id
        if info?.track != track.id {
            model.session.selectTimelineTrack(id: track.id)
        }
        let at = frame
        if at < clip.start || at >= clip.start + clip.duration {
            model.session.setTimelineFrame(frame: clip.start)
        }
        model.refresh()
    }

    /// Clip `clip` held: picked, with its short menu standing on the bar at `x` along it.
    func openClipMenu(_ clip: TimelineClipInfo, on track: TimelineTrackInfo, x: CGFloat) {
        guard selecting == nil else { return }
        pick(clip, on: track)
        clipMenu = ClipMenu(clip: clip.id, x: x)
        UIImpactFeedbackGenerator(style: .light).impactOccurred()
    }

    func addDrawing() {
        stopPlaying()
        optionsOpen = false
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

    func setDuration(_ clip: UInt64, _ frames: Int, push: Bool) {
        _ = model.session.setTimelineClipDuration(id: clip, duration: UInt32(max(frames, 1)), push: push)
        model.refresh()
    }

    func move(_ clip: UInt64, to track: UInt64, start: Int) {
        _ = model.session.moveTimelineClip(id: clip, track: track, start: UInt32(max(start, 0)))
        model.refresh()
    }

    func split(_ clip: UInt64) {
        optionsOpen = false
        if !model.session.splitTimelineClip(id: clip) {
            model.showNotice("Move the playhead inside the clip to split it there.", warning: true)
        }
        model.refresh()
    }

    /// Clip `id` taken out of the group open, onto the level the group is on.
    func moveOutOfGroup(_ id: UInt64) {
        if model.session.moveTimelineClipOutOfGroup(id: id) {
            // shown where it went
            open(model.session.timelineInfo()?.openGroups.dropLast().last?.id)
            picked = id
        }
        model.refresh()
    }

    /// Clip `id` dropped onto group clip `group`: put inside it.
    func moveIntoGroup(_ id: UInt64, _ group: UInt64) {
        if model.session.moveTimelineClipIntoGroup(id: id, group: group) {
            picked = group
        }
        model.refresh()
    }

    func duplicate(_ clip: UInt64) {
        optionsOpen = false
        if model.session.duplicateTimelineClip(id: clip) == 0 {
            model.noRoomForLayers()
        }
        model.refresh()
    }

    func delete(_ clip: UInt64, close: Bool) {
        optionsOpen = false
        picked = nil
        _ = model.session.deleteTimelineClip(id: clip, close: close)
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
        clipOpen = false
        keysOpen = false
        settingsOpen = false
        cameraOpen = false
    }

    /// The clips picked together copied with their keys, to paste at the playhead.
    func copySelected() {
        let ids = Array(selecting ?? [])
        guard !ids.isEmpty else { return }
        selecting = nil
        if model.session.copyTimelineClips(ids: ids) {
            model.showNotice(ids.count == 1 ? "Copied. Paste puts it at the playhead on the chosen track."
                : "\(ids.count) clips copied. Paste puts them at the playhead, from the chosen track up.", warning: false)
        }
        model.refresh()
    }

    /// The clips picked together copied, then taken off with their drawings: one undo step.
    func cutSelected() {
        let ids = Array(selecting ?? [])
        guard !ids.isEmpty else { return }
        picked = nil
        if model.session.cutTimelineClips(ids: ids) {
            selecting = nil
        } else {
            model.showNotice("One of them is on a locked track.", warning: true)
        }
        model.refresh()
    }

    /// The clips picked together taken off with their drawings: one undo step.
    func deleteSelected() {
        let ids = Array(selecting ?? [])
        guard !ids.isEmpty else { return }
        picked = nil
        if model.session.deleteTimelineClips(ids: ids) {
            selecting = nil
        } else {
            model.showNotice("One of them is on a locked track.", warning: true)
        }
        model.refresh()
    }

    /// How far the clips picked together can go toward `by` frames along their tracks, before
    /// one meets a clip left behind.
    func shiftRoom(_ by: Int) -> Int {
        Int(model.session.timelineShiftRoom(ids: Array(selecting ?? []), by: Int32(clamping: by)))
    }

    /// The clips picked together moved `by` frames along their tracks: one undo step.
    func shiftSelected(_ by: Int) {
        guard by != 0 else { return }
        _ = model.session.shiftTimelineClips(ids: Array(selecting ?? []), by: Int32(clamping: by))
        model.refresh()
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

    /// Opens group clip `id` (its tracks shown in the panel), or with none, goes back to the
    /// timeline's own tracks.
    func open(_ id: UInt64?) {
        stopPlaying()
        picked = nil
        keysOpen = false
        clipOpen = false
        _ = model.session.openTimelineGroup(id: id)
        model.refresh()
    }

    /// Group `id` plays its first `length` frames over and over (0: once).
    func setCycle(_ id: UInt64, length: UInt32, continueAcross: Bool) {
        _ = model.session.setTimelineCycle(id: id, length: length, continueAcross: continueAcross)
        model.refresh()
    }

    /// A camera key at the playhead: shown at once, one undo step when `done`.
    func setCameraKey(x: Float, y: Float, zoom: Float, rotation: Float, done: Bool) {
        stopPlaying()
        _ = model.session.setTimelineCameraKey(x: x, y: y, zoom: zoom, rotation: rotation, easing: keyEasing, done: done)
        camera = model.session.timelineCamera()
        if done {
            model.refresh()
        }
    }

    func removeCameraKey() {
        _ = model.session.removeTimelineCameraKey()
        model.refresh()
    }

    /// Clip `id` copied with its keys, to paste at the playhead.
    func copyClip(_ id: UInt64) {
        _ = model.session.copyTimelineClips(ids: [id])
        clipOpen = false
        model.showNotice("Copied. Paste puts it at the playhead on the chosen track.", warning: false)
        model.refresh()
    }

    /// Clip `id` copied, then taken off with its drawing.
    func cutClip(_ id: UInt64) {
        stopPlaying()
        clipOpen = false
        picked = nil
        if !model.session.cutTimelineClips(ids: [id]) {
            model.showNotice("The track is locked.", warning: true)
        }
        model.refresh()
    }

    /// The clips copied, at the playhead on the track drawn on (those above on the tracks above);
    /// the first pasted is picked.
    func pasteClips() {
        stopPlaying()
        clipOpen = false
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

    /// A sync marker at the frame showing (playing, the one playing).
    func dropMarker() {
        _ = model.session.addTimelineMarker(frame: frame)
        model.refresh()
    }

    func moveMarker(_ from: UInt32, to: UInt32) {
        _ = model.session.moveTimelineMarker(from: from, to: to)
        model.refresh()
    }

    func removeMarker(_ at: UInt32) {
        _ = model.session.removeTimelineMarker(frame: at)
        model.refresh()
    }

    func clearMarkers() {
        _ = model.session.clearTimelineMarkers()
        model.refresh()
    }

    /// Played frames show through the camera, or the canvas as it is.
    func setCameraView(_ on: Bool) {
        model.session.setTimelineCameraView(on: on)
        info = model.session.timelineInfo()
    }

    func togglePlaying() {
        isPlaying ? stopPlaying() : play()
    }

    private func play() {
        guard let first = info, first.duration > 0 else { return }
        let session = model.session
        settingsOpen = false
        optionsOpen = false
        isPlaying = true
        playing?.cancel()
        playing = Task { @MainActor [weak self] in
            var at = first.frame < first.duration ? first.frame : 0
            // from the first looped frame when the playhead is outside them
            if let l = self?.loop, at < l.start || at >= l.end {
                at = l.start
            }
            var step: Int32 = 1
            // each frame on time by the clock, so the sounds stay with the pictures
            var due = ContinuousClock.now + .seconds(self?.media.play(from: at) ?? 0)
            while !Task.isCancelled {
                guard let self, let info = self.info else { return }
                self.shownFrame = at
                session.setTimelinePlaying(frame: at)
                due += .seconds(1 / Double(max(info.fps, 1)))
                try? await Task.sleep(until: due, clock: .continuous)
                guard let next = session.timelineNextStep(at: at, step: step) else {
                    self.isPlaying = false
                    self.media.stop()
                    return
                }
                due += .seconds(self.media.follow(from: at, to: next.at, step: next.step))
                at = next.at
                step = next.step
            }
        }
    }

    func stopPlaying() {
        playing?.cancel()
        playing = nil
        isPlaying = false
        media.stop()
        if shownFrame != nil {
            shownFrame = nil
            model.session.setTimelinePlaying(frame: nil)
        }
    }

    /// The previous or next drawing on the track drawn on.
    func step(_ by: Int) {
        guard let clips = track?.clips, !clips.isEmpty else { return }
        let at = frame
        let next = by > 0 ? clips.first { $0.start > at } : clips.last { $0.start + $0.duration <= at }
        if let next {
            goTo(next.start)
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

/// A drawing's picture for its clip in the full view: cut to what is painted on it, so a small
/// drawing fills its clip's picture rather than being a speck on the whole canvas.
enum ClipPicture {
    /// How many pixels across the cut picture is wanted at, and the most times larger the canvas
    /// is drawn again to get them for a small drawing.
    private static let wanted: CGFloat = 72
    private static let mostLarger = 6

    /// `whole` (layer `id`'s picture of the whole canvas) cut to what is painted on it. A drawing
    /// painted on only a small part of the canvas is drawn again larger first, so its picture
    /// stays sharp. None when nothing is painted.
    static func cut(_ whole: ThumbnailImage, of id: UInt64, from session: CanvasSession) -> UIImage? {
        var from = whole
        var box = painted(whole)
        if box == nil {
            // nothing shows this small, but a thin stroke can fall between the pixels a small
            // picture is made from (each of its pixels is 4 × 4 taps, two canvas pixels each):
            // at an eighth of the canvas every pixel of it is in one, so nothing is missed
            let canvas = session.info()
            let fine = min(max((max(canvas.width, canvas.height) + 7) / 8, thumbSide), 1024)
            guard fine > thumbSide, let finer = session.frameThumbnail(id: id, maxSide: fine) else { return nil }
            from = finer
            box = painted(finer)
        }
        guard let box else { return nil }
        // too few pixels of it: drawn again larger
        let side = CGFloat(max(from.width, from.height))
        let most = CGFloat(thumbSide) * CGFloat(mostLarger)
        let larger = min((side * wanted / max(box.width, box.height)).rounded(.up), most)
        if larger > side, let again = session.frameThumbnail(id: id, maxSide: UInt32(larger)), let found = painted(again) {
            return crop(again, to: found)
        }
        return crop(from, to: box)
    }

    /// The box around the pixels of `t` that aren't see-through: with a little room about it,
    /// never much narrower than it is tall (or the other way), and never less than a twelfth of
    /// the picture, so a dot keeps some of the canvas around it.
    private static func painted(_ t: ThumbnailImage) -> CGRect? {
        let w = Int(t.width), h = Int(t.height)
        guard w > 0, h > 0, t.rgba.count >= w * h * 4 else { return nil }
        var x0 = w, y0 = h, x1 = -1, y1 = -1
        t.rgba.withUnsafeBytes { (bytes: UnsafeRawBufferPointer) in
            for y in 0 ..< h {
                let row = y * w * 4
                for x in 0 ..< w where bytes[row + x * 4 + 3] > 0 {
                    x0 = min(x0, x)
                    x1 = max(x1, x)
                    y0 = min(y0, y)
                    y1 = max(y1, y)
                }
            }
        }
        guard x1 >= x0, y1 >= y0 else { return nil }
        var box = CGRect(x: x0, y: y0, width: x1 - x0 + 1, height: y1 - y0 + 1)
        let long = max(box.width, box.height)
        let least = max(CGFloat(max(w, h)) / 12, long * 0.6)
        let room = long * 0.08 + 1
        box = box.insetBy(dx: -max(room, (least - box.width) / 2), dy: -max(room, (least - box.height) / 2))
        box = box.integral.intersection(CGRect(x: 0, y: 0, width: w, height: h))
        return box.isEmpty ? nil : box
    }

    private static func crop(_ t: ThumbnailImage, to box: CGRect) -> UIImage? {
        guard let image = CanvasModel.image(t)?.cgImage?.cropping(to: box) else { return nil }
        return UIImage(cgImage: image)
    }
}

/// The timeline at the bottom of the canvas: simple mode's strip, or every track.
struct TimelineBar: View {
    let model: CanvasModel
    let screen: CGSize

    var body: some View {
        if let info = model.timeline.info {
            if info.simple {
                SimpleTimeline(model: model, tool: model.timeline, info: info, screen: screen)
            } else {
                TracksTimeline(model: model, tool: model.timeline, info: info, screen: screen)
            }
        }
    }
}

// MARK: - Simple mode

/// One track as a flipbook strip: Play, Settings, Tracks and Add Drawing over the track's frames,
/// the playhead's in the middle; a drawing's card, grey cards for the frames it is held, an empty
/// card where the track has nothing.
private struct SimpleTimeline: View {
    let model: CanvasModel
    let tool: TimelineTool
    let info: TimelineInfo
    let screen: CGSize

    var body: some View {
        let width = screen.width > screen.height ? simpleWideWidth : simpleUprightWidth
        VStack(alignment: .leading, spacing: 0) {
            if tool.settingsOpen {
                TlPopover(width: 300, arrowX: 150) { TimelineSettingsPage(tool: tool, info: info) }
                    .offset(x: width / 2 - 150)
            } else if tool.optionsOpen, let clip = tool.currentClip {
                TlPopover(width: 260, arrowX: 130) { DrawingOptionsPage(tool: tool, clip: clip, frame: info.frame) }
                    .offset(x: width / 2 - 130)
            }
            ZStack(alignment: .topLeading) {
                HStack(spacing: 0) {
                    BarWord(label: tool.isPlaying ? "Pause" : "Play", bold: true) { tool.togglePlaying() }
                    Spacer(minLength: 0)
                    BarWord(label: "Settings") {
                        tool.settingsOpen.toggle()
                        tool.optionsOpen = false
                    }
                    BarWord(label: "Tracks") { tool.setSimple(false) }
                        .padding(.leading, 23)
                    BarWord(label: "Add Drawing") { tool.addDrawing() }
                        .padding(.leading, 23)
                }
                .padding(.horizontal, barInset)
                .padding(.top, 8)
                FrameStrip(model: model, tool: tool, info: info, width: width - 24)
                    .offset(x: 12, y: 42)
                Capsule()
                    .fill(Pc.accent)
                    .frame(width: 24, height: 3)
                    .offset(x: width / 2 - 12, y: 89)
            }
            .frame(width: width, height: simpleHeight, alignment: .topLeading)
            .background(RoundedRectangle(cornerRadius: 30, style: .circular).fill(barFill))
            // the whole bar takes its touches: none reach the canvas under it
            .contentShape(Rectangle())
            .onTapGesture {}
            .padding(.bottom, barGap)
        }
        .frame(width: width)
    }
}

/// The track drawn on, frame by frame, the middle one the playhead's. Dragged, it scrubs; let go,
/// it settles on a frame, which is drawn on. A tap on the middle card opens the drawing's options.
private struct FrameStrip: View {
    let model: CanvasModel
    let tool: TimelineTool
    let info: TimelineInfo
    let width: CGFloat

    @State private var dragged: CGFloat?
    @State private var from: CGFloat = 0
    @State private var shown: UInt32 = 0

    var body: some View {
        let canvas = model.session.info()
        let aspect = canvas.width > 0 && canvas.height > 0 ? min(max(CGFloat(canvas.width) / CGFloat(canvas.height), 0.25), 2) : 0.66
        let cardWidth = cardHeight * aspect + 1
        let pitch = cardWidth + 1
        let count = Int(info.duration)
        let last = CGFloat(max(count - 1, 0))
        let focus = CGFloat(tool.frame)
        let at = dragged ?? focus
        let clips = tool.track?.clips ?? []
        ZStack(alignment: .topLeading) {
            let lo = max(Int((at - width / 2 / pitch).rounded(.down)) - 1, 0)
            let hi = min(Int((at + width / 2 / pitch).rounded(.up)) + 1, count - 1)
            if lo <= hi {
                ForEach(lo...hi, id: \.self) { f in
                    let x = width / 2 - cardWidth / 2 + pitch * (CGFloat(f) - at)
                    let clip = clips.first { $0.start <= f && f < $0.start + $0.duration }
                    let card = RoundedRectangle(cornerRadius: 6, style: .circular)
                    Group {
                        if let clip {
                            let held = UInt32(f) != clip.start
                            ZStack {
                                held ? heldCard : Color.white
                                if !held, let picture = tool.thumbnails[clip.root] {
                                    Image(uiImage: picture).resizable().scaledToFit()
                                }
                            }
                            .clipShape(card)
                            .overlay(card.strokeBorder(cardEdge, lineWidth: 1))
                        } else {
                            card.strokeBorder(cardEdge.opacity(0.6), style: StrokeStyle(lineWidth: 1, dash: [3, 3]))
                        }
                    }
                    .frame(width: cardWidth, height: cardHeight)
                    .contentShape(card)
                    .onTapGesture {
                        if UInt32(f) == tool.frame, clip != nil {
                            tool.optionsOpen.toggle()
                            tool.settingsOpen = false
                        } else {
                            tool.optionsOpen = false
                            tool.goTo(UInt32(f))
                        }
                    }
                    .offset(x: x, y: 2)
                }
            }
        }
        .frame(width: width, height: 44, alignment: .topLeading)
        .clipShape(RoundedRectangle(cornerRadius: 18, style: .circular))
        .background(RoundedRectangle(cornerRadius: 18, style: .circular).fill(trackFill))
        .overlay(RoundedRectangle(cornerRadius: 18, style: .circular).strokeBorder(trackEdge, lineWidth: 1))
        .animation(tool.isPlaying || dragged != nil ? nil : Motion.frameSlide, value: focus)
        .gesture(
            DragGesture(minimumDistance: 6)
                .onChanged { drag in
                    guard count > 1 else { return }
                    if dragged == nil {
                        from = focus
                        shown = tool.frame
                    }
                    let p = min(max(from - drag.translation.width / pitch, 0), last)
                    dragged = p
                    let i = UInt32(p.rounded())
                    if i != shown {
                        shown = i
                        tool.scrub(i)
                    }
                }
                .onEnded { drag in
                    guard dragged != nil else { return }
                    let target = min(max(from - drag.predictedEndTranslation.width / pitch, 0), last).rounded()
                    withAnimation(Motion.frameSlide) { dragged = target }
                    tool.endScrub(UInt32(target))
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.35) { dragged = nil }
                }
        )
    }
}

/// The drawing at the playhead: how long it is held, Split here (on a held frame), Duplicate,
/// Delete.
private struct DrawingOptionsPage: View {
    let tool: TimelineTool
    let clip: TimelineClipInfo
    let frame: UInt32

    @State private var hold: Int?

    var body: some View {
        let shown = hold ?? Int(clip.duration)
        VStack(alignment: .leading, spacing: 0) {
            PanelTitle(text: "Drawing", bottom: 15)
            TlSliderGroup(top: 4) {
                TlSlider(
                    label: "Held for", value: shown == 1 ? "1 frame" : "\(shown) frames",
                    share: Float(shown - 1) / Float(mostHold - 1),
                    onEnd: {
                        if let h = hold {
                            tool.setDuration(clip.id, h, push: true)
                        }
                        hold = nil
                    }
                ) { v in
                    hold = 1 + wholeNumber(v * Float(mostHold - 1))
                }
            }
            .padding(.bottom, 10)
            TlGroup {
                if frame > clip.start {
                    TlAction(label: "Split Here") { tool.split(clip.id) }
                    TlRowLine()
                }
                TlAction(label: "Duplicate") { tool.duplicate(clip.id) }
                TlRowLine()
                TlAction(label: "Delete") { tool.delete(clip.id, close: true) }
            }
        }
    }
}

// MARK: - Every track

/// Every track, top first: a header row (Play, the frame, what acts on the picked clip, Settings,
/// Simple, + Track, Add Drawing), then the tracks' names beside a ruler and their clips, the
/// playhead across them.
private struct TracksTimeline: View {
    let model: CanvasModel
    let tool: TimelineTool
    let info: TimelineInfo
    let screen: CGSize

    /// How tall the tracks showed when the grabber's drag began.
    @State private var heightFrom: CGFloat?

    var body: some View {
        let width = min(screen.width - 32, 1180)
        let rows = min(max(tool.tracks.count, 1), mostRowsShown)
        // as tall as the grabber left it: no shorter than the ruler, the camera's lane and one
        // track, no taller than every track or half the screen. The key lanes stand on top of
        // that (the grabber's height is kept without them), so opening them leaves the tracks
        // the rows they had, down to one where half the screen has no room for more
        let lanes = tool.keyLanesHeight
        let fixed = rulerHeight + cameraLaneHeight
        let least = fixed + rowHeight + lanes
        let every = fixed + CGFloat(max(tool.tracks.count, 1)) * rowHeight + lanes
        let most = max(min(every, (screen.height * 0.5).rounded()), least)
        let bodyHeight = min(max((tool.shownHeight ?? fixed + CGFloat(rows) * rowHeight) + lanes, least), most)
        // what is left above the bar for a page standing on it
        let room = max(screen.height - bodyHeight - 150, 240)
        VStack(alignment: .leading, spacing: 0) {
            if let menu = tool.clipMenu, let clip = tool.clip(menu.clip)?.1 {
                let at = stand(menu.x, page: 220, on: width)
                TlPopover(width: 220, arrowX: at.arrow, room: room) { ClipMenuPage(tool: tool, clip: clip, frame: tool.frame) }
                    .offset(x: at.left)
            } else if let id = tool.trackOpen, let track = tool.tracks.first(where: { $0.id == id }) {
                TlPopover(width: 300, arrowX: headerWidth / 2, room: room) { TrackPage(tool: tool, track: track) }
                    .offset(x: 10)
            } else if tool.settingsOpen {
                let at = stand(tool.headerWords["settings"] ?? width - 150, page: 300, on: width)
                TlPopover(width: 300, arrowX: at.arrow, room: room) { TimelineSettingsPage(tool: tool, info: info) }
                    .offset(x: at.left)
            } else if tool.keysOpen, let id = tool.picked {
                TlPopover(width: 320, arrowX: 160, room: room) { KeysPage(model: model, tool: tool, clip: id, frame: tool.frame) }
                    .offset(x: max(width / 2 - 160, 0))
            } else if tool.clipOpen {
                let clip = tool.picked.flatMap { tool.clip($0)?.1 }
                TlPopover(width: 300, arrowX: 150, room: room) {
                    if let clip, clip.kind == .audio || clip.kind == .video {
                        MediaClipPage(model: model, clip: clip, fps: info.fps)
                    } else {
                        ClipPage(tool: tool, clip: clip, frame: tool.frame)
                    }
                }
                .offset(x: max(width / 2 - 150, 0))
            } else if tool.cameraOpen, let camera = tool.camera {
                // over its lane's name, which opens it, as a track's page is over its name
                TlPopover(width: 300, arrowX: headerWidth / 2, room: room) { CameraPage(model: model, tool: tool, camera: camera, info: info) }
                    .offset(x: 10)
            } else if tool.framesOpen {
                let at = stand(tool.headerWords["frames"] ?? 190, page: 300, on: width)
                TlPopover(width: 300, arrowX: at.arrow, room: room) { TimelineFramesPage(tool: tool, info: info) }
                    .offset(x: at.left)
            }
            VStack(spacing: 6) {
                header
                    .padding(.horizontal, 18)
                    .padding(.top, 12)
                ScrollView(.vertical, showsIndicators: false) {
                    HStack(alignment: .top, spacing: 0) {
                        VStack(spacing: 0) {
                            Color.clear.frame(height: rulerHeight)
                            CameraLaneHeader(tool: tool)
                            KeyLanesHeader(tool: tool)
                            ForEach(tool.tracks.reversed(), id: \.id) { t in
                                TrackHeader(tool: tool, track: t, chosen: t.id == info.track)
                            }
                        }
                        .frame(width: headerWidth)
                        TracksArea(tool: tool, info: info, shownWidth: width - 20 - headerWidth)
                    }
                }
                .frame(height: bodyHeight)
                .clipShape(RoundedRectangle(cornerRadius: 16, style: .circular))
                .padding(.horizontal, 10)
                .padding(.bottom, 10)
            }
            .frame(width: width)
            .background(RoundedRectangle(cornerRadius: 26, style: .circular).fill(barFill))
            .contentShape(Rectangle())
            .onTapGesture {}
            // the grabber: dragged up or down, the tracks show taller or shorter; tapped twice,
            // as tall as they start
            .overlay(alignment: .top) {
                Capsule()
                    .fill(inkDim)
                    .frame(width: 44, height: 4)
                    .frame(width: 132, height: 14)
                    .contentShape(Rectangle())
                    .onTapGesture(count: 2) { tool.setShownHeight(nil) }
                    .gesture(
                        DragGesture(minimumDistance: 2, coordinateSpace: .global)
                            .onChanged { drag in
                                let from = heightFrom ?? bodyHeight
                                heightFrom = from
                                tool.setShownHeight(min(max(from - drag.translation.height, least), most) - lanes)
                            }
                            .onEnded { _ in heightFrom = nil }
                    )
            }
            .padding(.bottom, barGap - 6)
        }
        .frame(width: width)
        .coordinateSpace(name: barSpace)
        .onPreferenceChange(HeaderWordX.self) { tool.headerWords = $0 }
    }

    /// Where a page `page` wide stands on the bar (`width` wide) to point down at `x` along it:
    /// its left edge, and its arrow from there.
    private func stand(_ x: CGFloat, page: CGFloat, on width: CGFloat) -> (left: CGFloat, arrow: CGFloat) {
        let left = min(max(x - page / 2, 0), max(width - page, 0))
        return (left, min(max(x - left, 30), page - 30))
    }

    /// The header's words: all of them where the bar has room; else with Settings and Simple
    /// folded into More; and where even that is too wide, scrolled sideways, fading at the edge
    /// they run off so it shows there are more.
    private var header: some View {
        ViewThatFits(in: .horizontal) {
            headerWords(folded: false)
            headerWords(folded: true)
            ScrollView(.horizontal, showsIndicators: false) {
                headerWords(folded: true).padding(.trailing, 30)
            }
            .mask(
                HStack(spacing: 0) {
                    Rectangle()
                    LinearGradient(colors: [.black, .clear], startPoint: .leading, endPoint: .trailing).frame(width: 30)
                }
            )
        }
    }

    private func headerWords(folded: Bool) -> some View {
        HStack(spacing: 22) {
            // in a group: back out of it, and its name
            if let open = info.openGroups.last {
                BarWord(label: "‹ Back", bold: true) { tool.open(info.openGroups.dropLast().last?.id) }
                Text(verbatim: open.name)
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(Pc.accent)
                    .lineLimit(1)
                Rectangle().fill(inkDim).frame(width: 1, height: 18)
            }
            BarWord(label: tool.isPlaying ? "Pause" : "Play", bold: true) { tool.togglePlaying() }
            if tool.isPlaying {
                // a sync marker where it plays now
                BarWord(label: "Mark", bold: true, tint: markerFill) { tool.dropMarker() }
            }
            TimelineStepWords(tool: tool)
            TimelineFrameCounter(tool: tool, duration: info.duration)
                .headerWord("frames")
            Spacer(minLength: 0)
            if let chosen = tool.selecting {
                // picking clips together: what to do with them, and done
                Text(verbatim: chosen.isEmpty ? "Tap or draw across clips" : chosen.count == 1 ? "1 clip" : "\(chosen.count) clips")
                    .font(.system(size: 15))
                    .foregroundStyle(inkDim)
                    .lineLimit(1)
                BarWord(label: "Group", dim: chosen.isEmpty) { tool.makeGroup() }
                // each through the empty frames after it, to the next clip or the end
                BarWord(label: "Extend", dim: chosen.isEmpty) { tool.extend(Array(chosen)) }
                BarWord(label: "Copy", dim: chosen.isEmpty) { tool.copySelected() }
                BarWord(label: "Duplicate", dim: chosen.isEmpty) { tool.duplicateSelected() }
                BarWord(label: "Cut", dim: chosen.isEmpty) { tool.cutSelected() }
                BarWord(label: "Delete", dim: chosen.isEmpty) { tool.deleteSelected() }
                Rectangle().fill(inkDim).frame(width: 1, height: 18)
                BarWord(label: "Done", bold: true) { tool.selecting = nil }
            } else if tool.keysPicked {
                KeyPickWords(tool: tool)
            } else {
                let pickedOne = tool.picked.flatMap { tool.clip($0) } != nil
                // the clipboard's clips paste from the Clip page, a clip picked or not
                if pickedOne || tool.clipboard.clips > 0 {
                    if pickedOne {
                        BarWord(label: "Keys", bold: tool.keysOpen) {
                            tool.keysOpen.toggle()
                            tool.clipOpen = false
                            tool.settingsOpen = false
                            tool.cameraOpen = false
                        }
                    }
                    BarWord(label: "Clip", bold: tool.clipOpen) {
                        tool.clipOpen.toggle()
                        tool.keysOpen = false
                        tool.settingsOpen = false
                        tool.cameraOpen = false
                    }
                    Rectangle().fill(inkDim).frame(width: 1, height: 18)
                }
                AutokeyWord(model: model)
                BarWord(label: "Select") { tool.startSelecting() }
                // (the camera's page opens from its lane's name, under the ruler)
                Rectangle().fill(inkDim).frame(width: 1, height: 18)
                if folded {
                    // the bar too narrow for every word: the two least used behind one
                    Menu {
                        Button("Settings") { openSettings() }
                        if info.openGroups.isEmpty {
                            Button("Simple View") { tool.setSimple(true) }
                        }
                    } label: {
                        Text(verbatim: "More")
                            .font(.system(size: 17, weight: tool.settingsOpen ? .bold : .regular))
                            .foregroundStyle(ink)
                            .lineLimit(1)
                            .frame(height: 30)
                    }
                    .headerWord("settings")
                } else {
                    BarWord(label: "Settings", bold: tool.settingsOpen) { openSettings() }
                        .headerWord("settings")
                    if info.openGroups.isEmpty {
                        BarWord(label: "Simple") { tool.setSimple(true) }
                    }
                }
                // everything that is added to the timeline, under one word
                TimelineAddMenu(model: model)
            }
        }
    }

    private func openSettings() {
        tool.settingsOpen.toggle()
        tool.keysOpen = false
        tool.clipOpen = false
        tool.cameraOpen = false
    }
}

/// The space the bar's pages stand in, and where along it the header's words that open one
/// are, by name: a page's arrow points down at its word.
private let barSpace = "timelineBar"

private struct HeaderWordX: PreferenceKey {
    static let defaultValue: [String: CGFloat] = [:]

    static func reduce(value: inout [String: CGFloat], nextValue: () -> [String: CGFloat]) {
        value.merge(nextValue()) { _, new in new }
    }
}

extension View {
    /// Tells the bar where this header word is, as `name`.
    func headerWord(_ name: String) -> some View {
        background(GeometryReader { place in
            Color.clear.preference(key: HeaderWordX.self, value: [name: place.frame(in: .named(barSpace)).midX])
        })
    }
}

/// A short word for how a clip is masked, if it is.
private func maskTag(_ mask: TimelineMask) -> String? {
    switch mask {
    case .none: nil
    case .clipping: "Clipped"
    case .alpha: "Alpha Matte"
    case .luminance: "Luma Matte"
    }
}

/// The picked clip's options: how the clip on the track below masks it; a group's cycle, and
/// opening or ungrouping it; picking more clips with it, splitting, duplicating and deleting it;
/// copying or cutting it, and pasting what was copied at the playhead. With no clip picked, only
/// the paste.
private struct ClipPage: View {
    let tool: TimelineTool
    let clip: TimelineClipInfo?
    let frame: UInt32

    /// The cycle's length as its slider moves, before it is let go.
    @State private var cycle: UInt32?

    private static let masks: [(TimelineMask, String)] =
        [(.none, "No Mask"), (.clipping, "Clipping Mask"), (.alpha, "Alpha Matte"), (.luminance, "Luma Matte")]

    var body: some View {
        if let clip {
            options(clip)
        } else {
            VStack(alignment: .leading, spacing: 0) {
                PanelTitle(text: "Clipboard", bottom: 14)
                TlGroup { paste }
            }
        }
    }

    @ViewBuilder
    private var paste: some View {
        let n = tool.clipboard.clips
        TlAction(label: n == 1 ? "Paste Clip at Playhead" : "Paste \(n) Clips at Playhead") { tool.pasteClips() }
    }

    @ViewBuilder
    private func options(_ clip: TimelineClipInfo) -> some View {
        let group = clip.kind == .group
        let drawing = clip.kind == .drawing
        let inside = frame > clip.start && frame < clip.start + clip.duration
        // one part at a time, so the page stays short over the canvas; a part this clip hasn't
        // got (a drawing has no Group) shows its look
        let tab: ClipTab = tool.clipTab == .group && !group ? .look : tool.clipTab
        let tabs: [(ClipTab, String)] = [(.look, "Look"), (.mask, "Mask")] + (group ? [(.group, "Group")] : []) + [(.edit, "Edit")]
        VStack(alignment: .leading, spacing: 0) {
            PanelTitle(text: clip.name, bottom: 10)
            TlSegments(choices: tabs, chosen: tab) { tool.clipTab = $0 }
                .padding(.bottom, 10)
            switch tab {
            case .look:
                ClipLookRows(tool: tool, clip: clip)
            case .mask:
                Text(verbatim: "Masked by the clip on the track below")
                    .font(.system(size: 13))
                    .foregroundStyle(Pc.slideLabel)
                    .padding(.leading, 14)
                    .padding(.bottom, 10)
                TlGroup {
                    ForEach(Self.masks.indices, id: \.self) { i in
                        let (mask, label) = Self.masks[i]
                        if i > 0 {
                            TlRowLine()
                        }
                        TlChoice(label: label, chosen: clip.mask == mask) { tool.setMask(clip.id, mask) }
                    }
                }
            case .group:
                let length = cycle ?? clip.cycle
                let most = max(clip.duration, 2)
                TlSliderGroup {
                    TlSlider(
                        label: "Cycle", value: length == 0 ? "Off" : length == 1 ? "1 frame" : "\(length) frames",
                        share: Float(length) / Float(most),
                        onEnd: {
                            if let c = cycle {
                                tool.setCycle(clip.id, length: c, continueAcross: clip.continueAcross)
                            }
                            cycle = nil
                        }
                    ) { v in
                        cycle = UInt32(wholeNumber(v * Float(most)))
                    }
                }
                if clip.cycle > 0 {
                    TlGroup {
                        TlChoice(label: "Keys Repeat Each Cycle", chosen: !clip.continueAcross) {
                            tool.setCycle(clip.id, length: clip.cycle, continueAcross: false)
                        }
                        TlRowLine()
                        TlChoice(label: "Keys Continue Across", chosen: clip.continueAcross) {
                            tool.setCycle(clip.id, length: clip.cycle, continueAcross: true)
                        }
                    }
                    .padding(.top, 10)
                }
                TlGroup {
                    let open = tool.unfolded(clip.id)
                    TlAction(label: open ? "Fold Group" : "Unfold Group") { tool.fold(clip.id, open) }
                    TlRowLine()
                    TlAction(label: "Open Group") { tool.open(clip.id) }
                    TlRowLine()
                    TlAction(label: "Ungroup") { tool.ungroup(clip.id) }
                }
                .padding(.top, 10)
            case .edit:
                TlGroup {
                    TlAction(label: "Copy") { tool.copyClip(clip.id) }
                    TlRowLine()
                    TlAction(label: "Cut") { tool.cutClip(clip.id) }
                    if tool.clipboard.clips > 0 {
                        TlRowLine()
                        paste
                    }
                }
                TlGroup {
                    TlAction(label: "Select More Clips…") { tool.startSelecting() }
                    // (offered where there are empty frames after it to last through)
                    if tool.canExtend([clip.id]) {
                        TlRowLine()
                        TlAction(label: "Extend to Fill Space") { tool.extend([clip.id]) }
                    }
                    if drawing {
                        if inside {
                            TlRowLine()
                            TlAction(label: "Split Here") { tool.split(clip.id) }
                        }
                        TlRowLine()
                        TlAction(label: "Duplicate") { tool.duplicate(clip.id) }
                    }
                    if tool.inGroup(clip.id) {
                        TlRowLine()
                        TlAction(label: "Move Out of Group") { tool.moveOutOfGroup(clip.id) }
                    }
                    TlRowLine()
                    TlAction(label: "Delete") { tool.delete(clip.id, close: false) }
                }
                .padding(.top, 10)
            }
        }
    }
}

/// A held clip's short menu, standing on the bar and pointing down at it: copy, cut and paste;
/// extend through the empty frames after it, duplicate, split at the playhead, delete. The rest
/// is on its Clip page, which it opens.
private struct ClipMenuPage: View {
    let tool: TimelineTool
    let clip: TimelineClipInfo
    let frame: UInt32

    var body: some View {
        let group = clip.kind == .group
        let inside = frame > clip.start && frame < clip.start + clip.duration
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                PanelTitle(text: clip.name, bottom: 10)
                Spacer(minLength: 8)
                Text(verbatim: "Done")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(Pc.accent)
                    .padding(.horizontal, 14)
                    .frame(height: 44)
                    .contentShape(Rectangle())
                    .onTapGesture { tool.clipMenu = nil }
            }
            TlGroup {
                TlAction(label: "Copy") {
                    tool.clipMenu = nil
                    tool.copyClip(clip.id)
                }
                TlRowLine()
                TlAction(label: "Cut") {
                    tool.clipMenu = nil
                    tool.cutClip(clip.id)
                }
                if tool.clipboard.clips > 0 {
                    TlRowLine()
                    TlAction(label: "Paste at Playhead") {
                        tool.clipMenu = nil
                        tool.pasteClips()
                    }
                }
            }
            TlGroup {
                if tool.canExtend([clip.id]) {
                    TlAction(label: "Extend to Fill Space") {
                        tool.clipMenu = nil
                        tool.extend([clip.id])
                    }
                    TlRowLine()
                }
                if !group {
                    if inside {
                        TlAction(label: "Split Here") {
                            tool.clipMenu = nil
                            tool.split(clip.id)
                        }
                        TlRowLine()
                    }
                    TlAction(label: "Duplicate") {
                        tool.clipMenu = nil
                        tool.duplicate(clip.id)
                    }
                    TlRowLine()
                }
                TlAction(label: "Delete") {
                    tool.clipMenu = nil
                    tool.delete(clip.id, close: false)
                }
            }
            .padding(.top, 10)
            TlGroup {
                TlAction(label: "Clip Page…") { tool.clipOpen = true }
            }
            .padding(.top, 10)
        }
    }
}

/// The camera at the playhead: played frames shown through it or not, and its key there taken
/// away. Its frame over the canvas is dragged to key it.
private struct CameraPage: View {
    let model: CanvasModel
    let tool: TimelineTool
    let camera: TimelineCamera
    let info: TimelineInfo

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            PanelTitle(text: "Camera at frame \(tool.frame + 1)", bottom: 4)
            Text(verbatim: "Drag the frame to move it, a corner to zoom, the knob to turn.")
                .font(.system(size: 13))
                .foregroundStyle(Pc.slideLabel)
                .padding(.horizontal, 14)
                .padding(.bottom, 10)
                .fixedSize(horizontal: false, vertical: true)
            TlGroup {
                HStack(spacing: 0) {
                    Text(verbatim: "Play Through Camera")
                        .font(.system(size: 17))
                        .foregroundStyle(Pc.panelTitle)
                    Spacer(minLength: 0)
                    CheckBox(checked: info.cameraView, fill: Pc.accent)
                }
                .padding(.horizontal, 16)
                .frame(height: 44)
                .contentShape(Rectangle())
                .onTapGesture { tool.setCameraView(!info.cameraView) }
                TlRowLine()
                TlAction(label: "Frame Whole Canvas Here") {
                    let size = model.session.info()
                    tool.setCameraKey(x: Float(size.width) / 2, y: Float(size.height) / 2, zoom: 1, rotation: 0, done: true)
                }
                if camera.keyed {
                    TlRowLine()
                    TlAction(label: "Remove Camera Key Here") { tool.removeCameraKey() }
                }
            }
        }
    }
}

/// A track's name, and its lock and visibility; a tap draws on it.
private struct TrackHeader: View {
    let tool: TimelineTool
    let track: TimelineTrackInfo
    let chosen: Bool

    var body: some View {
        HStack(spacing: 10) {
            Text(verbatim: track.name)
                .font(.system(size: 15, weight: chosen ? .semibold : .regular))
                .foregroundStyle(chosen ? Pc.accent : ink)
                .lineLimit(1)
            Spacer(minLength: 0)
            PcIconView(PcIcons.lock, size: 18, color: track.locked ? ink : inkDim.opacity(0.5))
                .frame(width: 26, height: 30)
                .contentShape(Rectangle())
                .onTapGesture { tool.toggleLocked(track) }
            // held: the track shown alone, or the tracks back as they were
            CheckBox(checked: !track.hidden, fill: track.soloed ? Pc.accent : ink)
                .frame(width: 26, height: 30)
                .contentShape(Rectangle())
                .onTapGesture { tool.toggleHidden(track) }
                // before the name's hold, which would open the track's page too
                .highPriorityGesture(LongPressGesture(minimumDuration: 0.4).onEnded { _ in tool.solo(track.id) })
        }
        .padding(.leading, 14 + tool.indent(track.id))
        .padding(.trailing, 8)
        .frame(height: rowHeight)
        .background(chosen ? laneChosen : laneFill)
        // inside unfolded groups: a step in for each
        .overlay(alignment: .leading) { GroupRowGuides(depth: tool.place(track.id)?.depth ?? 0) }
        .overlay(alignment: .bottom) { rowLineFill.frame(height: 1) }
        .contentShape(Rectangle())
        .onTapGesture { tool.selectTrack(track.id) }
        .modifier(TrackHeaderGestures(tool: tool, track: track, rowHeight: rowHeight))
    }
}

/// The ruler and the tracks' clips, scrolling sideways together, with the playhead across them.
/// The ruler is tapped or dragged to move the playhead; the tracks are pinched to zoom. A clip is
/// tapped to pick it and dragged to move it (along, or to another track), its edges snapping to
/// the other clips', the markers and the playhead, with an outline where it will land. The picked
/// one's edges are dragged to trim it, the track showing what that does as it goes.
private struct TracksArea: View {
    let tool: TimelineTool
    let info: TimelineInfo
    /// How wide the area shows: the lanes reach at least this far.
    let shownWidth: CGFloat

    /// A clip being moved: how far, the frame and row it is over, the frame it would land on
    /// there (none: it can't go there), the group clip it would go into, the frame an edge of
    /// it has snapped onto, which way it pushes the clips it covers aside, and that row's clips as
    /// they would be then (see TimelinePicked.swift).
    private struct Moving {
        var clip: UInt64
        var dx: CGFloat
        var dy: CGFloat
        var start: Int
        var row: Int
        var landing: Int?
        var into: UInt64?
        var snap: Int?
        var later = true
        var pushed: [TimelineClipInfo] = []
    }

    /// An edge being dragged: whose and which, the frame it started on and the frame asked for,
    /// its track's clips as the trim would leave them (empty: as they are), and the frame it has
    /// snapped onto.
    private struct Trimming {
        var clip: UInt64
        var track: UInt64
        var leading: Bool
        var from: Int
        var frame: Int
        var clips: [TimelineClipInfo]
        var snap: Int?
        /// The frame it asked to snap onto, kept so the preview can be asked again.
        var asked: Int?
    }

    /// A finger down on the tracks in select mode. On a clip already picked, it moves them all:
    /// how many frames it asks for, how many they have room for, and the frame the clip under the
    /// finger has snapped onto. Anywhere else it is a stroke: its points, and the clips it has
    /// taken in or out so far (each once).
    private struct Stroke {
        var from: CGPoint
        var grabbed: TimelineClipInfo?
        var points: [CGPoint] = []
        var touched: Set<UInt64> = []
        var asked = 0
        var room = 0
        var snap: Int?
        /// Moved up or down: the row the grabbed clip is over, the track it goes to (none: they
        /// stay on their own) and the track each picked clip goes to.
        var to: Int?
        var track: UInt64?
        var goes: [UInt64: UInt64] = [:]
        /// Which way they push the clips they land on (later unless dragged earlier), the tracks
        /// as that would leave them (see TimelinePicked.swift), and the group clip they would go
        /// into instead.
        var later = true
        var pushed: [UInt64: [TimelineClipInfo]] = [:]
        var into: UInt64?
    }

    @State private var moving: Moving?
    @State private var trimming: Trimming?
    /// The clip whose menu the hold under way opened: the lift that ends the hold counts as a tap
    /// too (a tap has no time limit), and must not close the menu it just opened.
    @State private var heldClip: UInt64?
    /// Whether an edge's drag is still under way: one the system cancels clears its preview.
    @GestureState private var edgeHeld = false
    @State private var stroke: Stroke?
    /// An edge of the clips picked together being dragged (see TimelinePicked.swift).
    @State private var pickedTrim: PickedTrim?
    @State private var scrubbing = false
    /// A cycling group's end being dragged (see TimelineGroupRows.swift).
    @State private var cycling: CycleDrag?

    var body: some View {
        let frames = Int(info.duration)
        let pitch = tool.pitchShown(frames: frames, width: shownWidth)
        // the end being dragged: the ruler shaded past it, and room beyond it, and beyond the
        // clips parked past the end
        let ends = max(frames, Int(tool.lengthDraft ?? 0), tool.clipsEnd)
        let width = max(CGFloat(ends + 8) * pitch, shownWidth)
        let shown = tool.tracks.reversed() as [TimelineTrackInfo]
        ScrollView(.horizontal, showsIndicators: false) {
            ZStack(alignment: .topLeading) {
                VStack(spacing: 0) {
                    ruler(frames: Int(tool.lengthDraft ?? info.duration), width: width, pitch: pitch)
                    CameraLane(tool: tool, width: width, pitch: pitch, frames: frames)
                    KeyLanes(tool: tool, width: width, pitch: pitch, frames: frames)
                    ForEach(Array(shown.enumerated()), id: \.element.id) { row, t in
                        lane(t, row: row, rows: shown, width: width, pitch: pitch)
                            // a row whose clips are being dragged draws over the rows below it
                            .zIndex(raised(t) ? 1 : 0)
                    }
                }
                // the sync markers: a line down through the lanes from each one's flag
                let lanes = tool.keyLanesHeight
                let below = cameraLaneHeight + lanes + CGFloat(shown.count) * rowHeight
                TimelinePastEnd(end: tool.lengthDraft ?? info.duration, pitch: pitch, width: width, height: below)
                    .offset(y: rulerHeight)
                TimelineMarkerLines(markers: tool.markers, pitch: pitch, height: below)
                    .offset(y: rulerHeight)
                TimelineMarkerFlags(tool: tool, frames: frames, pitch: pitch, height: rulerHeight)
                TimelineLoopBand(loop: tool.loopDraft ?? tool.loop, pitch: pitch, ruler: rulerHeight, height: rulerHeight + below)
                TimelineLengthGrip(tool: tool, pitch: pitch, height: rulerHeight)
                TimelineLoopGrips(tool: tool, pitch: pitch, height: rulerHeight)
                KeyLaneGrips(tool: tool, width: width, pitch: pitch)
                    .offset(y: rulerHeight)
                // where the clip being moved will land
                if let m = moving, m.into == nil, let landing = m.landing, let c = tool.clip(m.clip)?.1 {
                    RoundedRectangle(cornerRadius: 8, style: .circular)
                        .strokeBorder(Pc.accent, style: StrokeStyle(lineWidth: 1.5, dash: [5, 4]))
                        .frame(width: clipWidth(c, pitch), height: rowHeight - 6)
                        .offset(x: CGFloat(landing) * pitch + 1, y: rulerHeight + cameraLaneHeight + lanes + CGFloat(m.row) * rowHeight + 3)
                        .allowsHitTesting(false)
                }
                if tool.selecting != nil {
                    // select mode: the tracks take a tap or a stroke that picks clips, or a drag
                    // on a picked one that moves them all
                    Color.clear
                        .frame(width: width, height: CGFloat(shown.count) * rowHeight)
                        .contentShape(Rectangle())
                        .gesture(
                            DragGesture(minimumDistance: 0)
                                .onChanged { stroked($0, rows: shown, pitch: pitch) }
                                .onEnded { strokeEnded($0) }
                        )
                        .offset(y: rulerHeight + cameraLaneHeight)
                    if let points = stroke?.points, stroke?.grabbed == nil, points.count > 1 {
                        Path { p in p.addLines(points) }
                            .stroke(Pc.accent, style: StrokeStyle(lineWidth: 2.5, lineCap: .round, lineJoin: .round))
                            .shadow(color: Pc.accent, radius: 5)
                            .frame(width: width, height: CGFloat(shown.count) * rowHeight, alignment: .topLeading)
                            .offset(y: rulerHeight + cameraLaneHeight)
                            .allowsHitTesting(false)
                    }
                    if let s = stroke, s.track != nil {
                        PickedMoveOutlines(rows: shown, goes: s.goes, by: s.room, pitch: pitch, rowHeight: rowHeight)
                            .offset(y: rulerHeight + cameraLaneHeight + lanes)
                    }
                    if stroke == nil {
                        PickedTrimGrips(tool: tool, rows: shown, pitch: pitch, rowHeight: rowHeight,
                                        snapped: { at, end, clip in snapped(at, onto: snaps(end: end, not: clip, rows: shown), pitch: pitch) },
                                        trim: $pickedTrim)
                            .offset(y: rulerHeight + cameraLaneHeight + lanes)
                    }
                }
                // the edge something has snapped onto
                if let f = moving?.snap ?? trimming?.snap ?? stroke?.snap ?? pickedTrim?.snap {
                    Pc.accent
                        .frame(width: 1, height: rulerHeight + below)
                        .offset(x: CGFloat(f) * pitch - 0.5)
                        .allowsHitTesting(false)
                }
                // the playhead: its frame lit faintly down the lanes where frames are wide enough
                // to tell apart, a line down its middle, and a tab on the ruler with its number
                let x = (CGFloat(tool.frame) + 0.5) * pitch
                if pitch >= 12 {
                    Pc.accent.opacity(0.12)
                        .frame(width: pitch, height: below)
                        .offset(x: CGFloat(tool.frame) * pitch, y: rulerHeight)
                        .allowsHitTesting(false)
                }
                Pc.accent
                    .frame(width: 2, height: rulerHeight + below - 8)
                    .offset(x: x - 1, y: 8)
                    .allowsHitTesting(false)
                Text(verbatim: "\(tool.frame + 1)")
                    .font(.system(size: 10, weight: .bold).monospacedDigit())
                    .foregroundStyle(Color.white)
                    .padding(.horizontal, 5)
                    .frame(minWidth: 18, minHeight: 15)
                    .background(RoundedRectangle(cornerRadius: 5, style: .circular).fill(Pc.accent))
                    .fixedSize()
                    // about the playhead's middle, and not off the ruler's start
                    .frame(width: 0)
                    .offset(x: max(x, 11), y: 1)
                    .allowsHitTesting(false)
            }
            .frame(width: width, alignment: .topLeading)
            .background(ZoomPinch(tool: tool, frames: frames, shownWidth: shownWidth))
            .background(TimelineShownFrames(tool: tool, pitch: pitch))
            .background(TrimFlipProbe(tool: tool, dragging: tool.edgeDragging))
        }
        .frame(width: shownWidth)
        .overlay(alignment: .top) { TrimFlipBadge(tool: tool) }
        .onChange(of: tool.edgeFlipped) { retrim() }
        .onChange(of: edgeHeld) { _, held in
            // a drag the system took away: nothing changes
            if !held, trimming != nil {
                trimming = nil
                tool.endEdgeDrag()
            }
        }
    }

    /// How many frames apart the ruler's numbers are: often enough to read, far enough apart not
    /// to run together.
    private func numbered(_ pitch: CGFloat) -> Int {
        let fps = max(Int(info.fps), 1)
        let steps = [1, 2, 5, 10, fps / 2, fps, fps * 2, fps * 5, fps * 10, fps * 30, fps * 60].filter { $0 > 0 }.sorted()
        return steps.first { CGFloat($0) * pitch >= 56 } ?? (steps.last ?? 1)
    }

    private func ruler(frames: Int, width: CGFloat, pitch: CGFloat) -> some View {
        let every = numbered(pitch)
        // a tick a frame while they have room, else one at each number and half way
        let tick = pitch >= 5 ? 1 : max(every / 2, 1)
        return Canvas { ctx, size in
            for f in stride(from: 0, through: frames, by: tick) {
                let x = CGFloat(f) * pitch
                let major = f % every == 0
                ctx.fill(Path(CGRect(x: x, y: major ? 12 : 17, width: 1, height: major ? 12 : 7)), with: .color(inkDim))
                if major, f < frames {
                    ctx.draw(Text(verbatim: "\(f + 1)").font(.system(size: 10).monospacedDigit()).foregroundColor(inkDim),
                             at: CGPoint(x: x + 3, y: 6), anchor: .leading)
                }
            }
            // past the end, shaded
            let end = CGFloat(frames) * pitch
            ctx.fill(Path(CGRect(x: end, y: 0, width: size.width - end, height: size.height)), with: .color(Color.black.opacity(0.12)))
        }
        .frame(width: width, height: rulerHeight)
        .background(laneFill)
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { drag in
                    // a second finger on it while an edge is dragged only turns the policy over
                    guard trimming == nil else { return }
                    // held a moment, then dragged: a loop drawn across it instead
                    guard !tool.rulerHold.changed(drag.location, frame: frameAt(drag.location.x, pitch), tool: tool) else { return }
                    scrubbing = true
                    tool.scrub(frameAt(drag.location.x, pitch))
                }
                .onEnded { drag in
                    scrubbing = false
                    guard !tool.rulerHold.ended(tool: tool) else { return }
                    tool.endScrub(frameAt(drag.location.x, pitch))
                }
        )
    }

    private func frameAt(_ x: CGFloat, _ pitch: CGFloat) -> UInt32 {
        UInt32(min(max(Int(x / pitch), 0), Int(info.duration) - 1))
    }

    /// How wide a clip shows: its frames, with a hair between it and the next, and never too
    /// narrow to see.
    private func clipWidth(_ c: TimelineClipInfo, _ pitch: CGFloat) -> CGFloat {
        max(CGFloat(c.duration) * pitch - 2, 3)
    }

    private func lane(_ t: TimelineTrackInfo, row: Int, rows: [TimelineTrackInfo], width: CGFloat, pitch: CGFloat) -> some View {
        // an edge of one of its clips dragged: its clips as that would leave them
        let clips = trimming.flatMap { $0.track == t.id && !$0.clips.isEmpty ? $0.clips : nil }
            ?? pickedTrim?.tracks[t.id] ?? pushedAside(on: t, rows: rows) ?? pickedPushed(on: t) ?? t.clips
        // a row inside a group clip being dragged follows it
        let follow = following(t, pitch: pitch)
        return ZStack(alignment: .topLeading) {
            (t.id == info.track ? laneChosen : laneFill)
                .frame(width: width, height: rowHeight)
                .contentShape(Rectangle())
                .onTapGesture { location in
                    guard trimming == nil else { return }
                    tool.picked = nil
                    tool.selectTrack(t.id)
                    tool.goTo(frameAt(location.x, pitch))
                }
            // a line at each frame, under the clips: stronger where the ruler has a number
            FrameLines(frames: Int(width / pitch), pitch: pitch, every: numbered(pitch), height: rowHeight, ink: ink)
                .equatable()
                .allowsHitTesting(false)
            ZStack(alignment: .topLeading) {
                if let place = tool.place(t.id), place.depth > 0 {
                    GroupRowShade(row: place, width: width, height: rowHeight, pitch: pitch)
                }
                ForEach(clips, id: \.id) { c in
                    clipView(c, on: t, row: row, rows: rows, pitch: pitch)
                }
            }
            .offset(follow)
        }
        .frame(width: width, height: rowHeight, alignment: .topLeading)
        .overlay(alignment: .bottom) { rowLineFill.frame(height: 1) }
        .opacity(t.hidden || tool.place(t.id)?.hiddenAbove == true ? 0.45 : 1)
    }

    private func clipView(_ c: TimelineClipInfo, on t: TimelineTrackInfo, row: Int, rows: [TimelineTrackInfo], pitch: CGFloat) -> some View {
        let chosen = tool.selecting?.contains(c.id) == true
        let picked = tool.selecting == nil ? tool.picked == c.id : chosen
        let move = moving?.clip == c.id ? moving : nil
        // snapped, it sits on the frame it snapped to; else it follows the finger. Picked with
        // others and dragged, as far as they all have room for
        let together = chosen && stroke?.grabbed != nil ? CGFloat(stroke?.room ?? 0) * pitch : 0
        let moved = (move.map { $0.snap != nil ? CGFloat($0.start - Int(c.start)) * pitch : $0.dx } ?? 0) + together
        // a cycling group's end dragged: as many cycles long as it would be
        let w = cycling.flatMap { $0.clip == c.id ? max(CGFloat($0.frames) * pitch - 2, 3) : nil } ?? clipWidth(c, pitch)
        let media = c.kind == .audio || c.kind == .video
        // its start being dragged: how much later (or earlier) in its file it would start
        let into = media && trimming?.clip == c.id && trimming?.leading == true
            ? Double(Int(c.start) - Int(t.clips.first { $0.id == c.id }?.start ?? c.start)) / Double(max(info.fps, 1))
            : 0
        let shape = RoundedRectangle(cornerRadius: 8, style: .circular)
        return ZStack(alignment: .leading) {
            // in its colour label's colour, if it has one
            shape.fill(picked ? Pc.accent.opacity(0.85) : c.label.map { Color(rgb: $0).opacity(0.75) } ?? clipFill)
            shape.strokeBorder(picked ? Pc.accent : clipEdge, lineWidth: 1)
            if c.kind == .group {
                // a group: an inner edge, as of the clips inside it
                shape.inset(by: 3).strokeBorder(picked ? Color.white.opacity(0.6) : clipEdge, lineWidth: 1)
                if moving?.into == c.id || stroke?.into == c.id {
                    // the clip (or the clips picked together) being moved would go into it
                    shape.strokeBorder(Pc.accent, lineWidth: 3)
                }
            }
            if media {
                MediaClipFace(media: tool.media, clip: c, picked: picked, fps: info.fps, pitch: pitch, height: rowHeight - 6, into: into)
            } else if c.duration > 1, pitch >= 10 {
                // held for more than a frame: a notch along its top at each frame it is held on
                Path { p in
                    for f in 1 ..< Int(c.duration) {
                        p.addRect(CGRect(x: CGFloat(f) * pitch - 1.5, y: 1, width: 1, height: 3))
                    }
                }
                .fill(picked ? Color.white.opacity(0.55) : ink.opacity(0.35))
                .allowsHitTesting(false)
            }
            let keyed = !(tool.keys[c.id] ?? []).isEmpty
            HStack(spacing: 6) {
                if c.kind == .group, w > 40, tool.selecting == nil {
                    GroupFoldButton(tool: tool, clip: c, tint: picked ? Color.white : ink)
                }
                // its picture, cut to what is painted on it, on the canvas's own background; a
                // drawing with nothing on it yet has none
                if c.root != 0, w > 26, let picture = tool.clipPictures[c.root] {
                    ZStack {
                        tool.paper
                        Image(uiImage: picture).resizable().scaledToFit().padding(1)
                    }
                    // shorter over its keys' marks, which run along the clip's bottom
                    .frame(width: min(30, w - 10), height: rowHeight - (keyed ? 20 : 14))
                    .clipShape(RoundedRectangle(cornerRadius: 4, style: .circular))
                }
                if !media, let title = clipTitle(c.name, width: w - (picked && w >= 44 ? 30 : 0)) {
                    Text(verbatim: title.text)
                        .font(.system(size: 13).monospacedDigit())
                        .foregroundStyle(picked ? Color.white.opacity(title.given ? 1 : 0.8) : title.given ? ink : inkDim)
                        .lineLimit(1)
                    // how it is masked, and a group's cycle
                    let tags = [c.disabled ? "Off" : nil, tool.blendName(c.blend), maskTag(c.mask), c.cycle > 0 ? "↻ \(c.cycle)" : nil].compactMap { $0 }
                    if !tags.isEmpty, w > 150 {
                        Text(verbatim: tags.joined(separator: "  "))
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(picked ? Color.white.opacity(0.8) : inkDim)
                            .lineLimit(1)
                    }
                }
            }
            // clear of the grips on its edges, and of its keys' marks
            .padding(.leading, picked && w >= 44 ? 20 : 5)
            .padding(.trailing, picked && w >= 44 ? 20 : 0)
            .padding(.bottom, keyed ? 6 : 0)
            if tool.picked == c.id, tool.selecting == nil, !tool.held(t) {
                // its edges, held to trim it (a sound or video's start moving into its file or back
                // out of it); a cycling group's start stays put while its end sets how many cycles
                // it plays
                if c.cycle == 0, w >= 44 {
                    edgeGrip(c, on: t, leading: true, rows: rows, pitch: pitch)
                }
                if c.kind == .group, c.cycle > 0 {
                    CycleGrip(tool: tool, clip: c, pitch: pitch, height: rowHeight - 6, drag: $cycling)
                } else {
                    edgeGrip(c, on: t, leading: false, rows: rows, pitch: pitch)
                }
            }
        }
        .frame(width: w, height: rowHeight - 6)
        // turned off, faint
        .opacity(c.disabled ? 0.4 : 1)
        // its keys, a diamond on each keyed frame along the bottom
        .overlay(alignment: .bottomLeading) {
            let frames = Set((tool.keys[c.id] ?? []).map(\.frame)).sorted()
            ZStack(alignment: .bottomLeading) {
                ForEach(frames, id: \.self) { f in
                    Rectangle()
                        .fill(picked ? Color.white : Pc.accent)
                        .frame(width: 6, height: 6)
                        .rotationEffect(.degrees(45))
                        .offset(x: (CGFloat(Int(f) - Int(c.start)) + 0.5) * pitch - 4, y: -2)
                }
            }
            .allowsHitTesting(false)
        }
        .offset(x: CGFloat(c.start) * pitch + 1 + moved, y: 3 + (move?.dy ?? 0) + pickedRows(c, row: row, rows: rows))
        .zIndex(move != nil || together != 0 ? 1 : 0)
        .shadow(color: move != nil ? .black.opacity(0.35) : .clear, radius: 8)
        .onTapGesture {
            if let held = heldClip {
                heldClip = nil
                if held == c.id { return }
            }
            if trimming == nil { tool.pick(c, on: t) }
        }
        // a group folds or unfolds its tracks with a double tap
        .simultaneousGesture(TapGesture(count: 2).onEnded { if c.kind == .group, tool.selecting == nil, trimming == nil { tool.fold(c.id, tool.unfolded(c.id)) } })
        // held still: its short menu, standing on the bar over it (as a track's page opens from
        // its name held)
        .simultaneousGesture(
            tool.selecting != nil ? nil : LongPressGesture(minimumDuration: 0.4, maximumDistance: 6).onEnded { _ in
                guard trimming == nil, moving == nil, !tool.isPlaying else { return }
                // the middle of as much of it as shows, along the bar
                let from = max(CGFloat(c.start) * pitch + 1 - tool.scrolledBy(), 0)
                let to = min(CGFloat(c.start) * pitch + 1 + w - tool.scrolledBy(), shownWidth)
                tool.openClipMenu(c, on: t, x: 10 + headerWidth + (from + max(to, from)) / 2)
                heldClip = tool.clipMenu?.clip == c.id ? c.id : nil
            }
        )
        .gesture(
            // a locked track's clips stay where they are
            tool.held(t) || tool.selecting != nil ? nil : DragGesture(minimumDistance: 8)
                .onChanged { drag in
                    dragged(c, row: row, rows: rows, by: drag.translation, pitch: pitch)
                }
                .onEnded { _ in
                    guard let m = moving else { return }
                    moving = nil
                    if let group = m.into {
                        tool.moveIntoGroup(c.id, group)
                    } else if m.landing != nil, m.row != row || m.start != Int(c.start) {
                        tool.placeClip(c.id, on: rows[m.row].id, start: m.start, later: m.later)
                    }
                }
        )
    }

    /// What a clip says of its name where it has `width` to say it in. A name someone gave it
    /// (`given`) shows whole or not at all. A name the timeline gave it ("Drawing 12") shows whole
    /// where there is room and as its number alone where there isn't: the number is what tells it
    /// from its neighbours, and cut short every one of them read "Drawing…".
    private func clipTitle(_ name: String, width: CGFloat) -> (text: String, given: Bool)? {
        let number = name.hasPrefix("Drawing ") ? name.dropFirst(8) : ""
        if !number.isEmpty, number.allSatisfy(\.isNumber) {
            if width > 132 {
                return (name, false)
            }
            return width > 54 ? (String(number), false) : nil
        }
        return width > 80 ? (name, true) : nil
    }

    /// The frames a dragged start (or an end, `end`) may snap onto: the other clips' edges, the
    /// playhead's frame, the markers', and the timeline's own start and end.
    private func snaps(end: Bool, not clip: UInt64, rows: [TimelineTrackInfo]) -> [Int] {
        var frames = [0, Int(info.duration)]
        for c in rows.flatMap(\.clips) where c.id != clip {
            frames.append(Int(c.start))
            frames.append(Int(c.start + c.duration))
        }
        // an end is one past a clip's last frame: on the playhead's frame, it stops just after it
        let after = end ? 1 : 0
        frames.append(Int(tool.frame) + after)
        frames.append(contentsOf: tool.markers.map { Int($0) + after })
        return frames
    }

    /// The frame of `frames` nearest `at` (frames, between them too), if within reach.
    private func snapped(_ at: CGFloat, onto frames: [Int], pitch: CGFloat) -> Int? {
        guard tool.snapping, let near = frames.min(by: { abs(CGFloat($0) - at) < abs(CGFloat($1) - at) }) else { return nil }
        return abs(CGFloat(near) - at) * pitch <= snapReach ? near : nil
    }

    /// Clip `c` of row `row` dragged `by`: where it would start and on which row, snapped by
    /// either edge, and where it would land there.
    private func dragged(_ c: TimelineClipInfo, row: Int, rows: [TimelineTrackInfo], by: CGSize, pitch: CGFloat) {
        // a second finger dragged while an edge is: it only turns the policy over
        guard trimming == nil else { return }
        // held, then dragged on: it moves, and the menu the hold opened gets out of the way
        if tool.clipMenu != nil {
            tool.clipMenu = nil
        }
        // dragged: no tap comes at the lift to take the hold back
        heldClip = nil
        let at = CGFloat(c.start) + by.width / pitch
        let to = min(max(row + Int((by.height / rowHeight).rounded()), 0), rows.count - 1)
        let length = Int(c.duration)
        let byStart = snapped(at, onto: snaps(end: false, not: c.id, rows: rows), pitch: pitch)
        let byEnd = snapped(at + CGFloat(length), onto: snaps(end: true, not: c.id, rows: rows), pitch: pitch)
        var start = Int(at.rounded())
        var snap: Int?
        // whichever edge is nearer what it snaps onto
        if let s = byStart, byEnd.map({ abs(CGFloat(s) - at) <= abs(CGFloat($0) - at - CGFloat(length)) }) ?? true {
            start = s
            snap = s
        } else if let e = byEnd, e >= length {
            start = e - length
            snap = e
        }
        start = max(start, 0)
        var next = Moving(clip: c.id, dx: by.width, dy: by.height, start: start, row: to, snap: snap)
        // the clips it covers pushed the way it went: later unless dragged earlier
        next.later = abs(by.width) < abs(by.height) || by.width >= 0
        if let m = moving, m.start == start, m.row == to, m.later == next.later {
            next.landing = m.landing
            next.into = m.into
            next.pushed = m.pushed
        } else {
            // dropped onto a group clip on another track: into it
            let onto = to == row ? nil : rows[to].clips.first {
                $0.kind == .group && $0.id != c.id && Int($0.start) <= start && start < Int($0.start + $0.duration)
            }
            next.into = tool.held(rows[to]) ? nil : onto?.id
            let placing = tool.held(rows[to]) ? nil : tool.placing(c.id, on: rows[to].id, start: start, later: next.later)
            next.landing = placing.map { Int($0.start) }
            next.pushed = placing?.clips ?? []
        }
        if let snap, snap != moving?.snap {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        moving = next
    }

    /// The clip at `point` of the tracks (from the first track's top left corner), if one is.
    private func clip(at point: CGPoint, rows: [TimelineTrackInfo], pitch: CGFloat) -> TimelineClipInfo? {
        let row = Int((point.y / rowHeight).rounded(.down))
        guard point.y >= 0, rows.indices.contains(row), point.x >= 0 else { return nil }
        let frame = UInt32(point.x / pitch)
        return rows[row].clips.first { $0.start <= frame && frame < $0.start + $0.duration }
    }

    /// A finger on the tracks in select mode, at `drag.location`. Down on a clip already picked,
    /// it moves them all by whole frames (the clip under it snapping by either edge, as far as
    /// they all have room for); anywhere else, every clip the stroke crosses is taken in with the
    /// others, or out, once.
    private func stroked(_ drag: DragGesture.Value, rows: [TimelineTrackInfo], pitch: CGFloat) {
        var s = stroke ?? {
            let under = clip(at: drag.startLocation, rows: rows, pitch: pitch)
            return Stroke(from: drag.startLocation, grabbed: under.flatMap { tool.selecting?.contains($0.id) == true ? $0 : nil })
        }()
        if let c = s.grabbed {
            let at = CGFloat(c.start) + drag.translation.width / pitch
            let length = Int(c.duration)
            let others = rows.map { t in
                TimelineTrackInfo(id: t.id, name: t.name, hidden: t.hidden, locked: t.locked,
                                  clips: t.clips.filter { tool.selecting?.contains($0.id) != true },
                                  opacity: t.opacity, soloed: t.soloed)
            }
            let byStart = snapped(at, onto: snaps(end: false, not: c.id, rows: others), pitch: pitch)
            let byEnd = snapped(at + CGFloat(length), onto: snaps(end: true, not: c.id, rows: others), pitch: pitch)
            var start = Int(at.rounded())
            var snap: Int?
            if let b = byStart, byEnd.map({ abs(CGFloat(b) - at) <= abs(CGFloat($0) - at - CGFloat(length)) }) ?? true {
                start = b
                snap = b
            } else if let e = byEnd {
                start = e - length
                snap = e
            }
            // never before the first frame: no picked clip starts before it
            let picked = tool.selecting ?? []
            let earliest = rows.flatMap(\.clips).filter { picked.contains($0.id) }.map { Int($0.start) }.min() ?? 0
            let asked = max(start - Int(c.start), -earliest)
            // dragged up or down a row or more: onto the tracks that far from theirs
            let from = rows.firstIndex { $0.clips.contains { $0.id == c.id } } ?? 0
            let to = min(max(from + Int((drag.translation.height / rowHeight).rounded()), 0), rows.count - 1)
            // the clips they land on pushed the way they went: later unless dragged
            // earlier
            let later = abs(drag.translation.width) < abs(drag.translation.height) || drag.translation.width >= 0
            if asked != s.asked || to != (s.to ?? from) || later != s.later || s.to == nil {
                s.asked = asked
                s.to = to
                s.later = later
                // dropped onto a group clip on another track (not one of them): into it, where they are
                let onto = to == from || tool.held(rows[to]) ? nil : rows[to].clips.first {
                    $0.kind == .group && !picked.contains($0.id) && Int($0.start) <= start && start < Int($0.start + $0.duration)
                }
                s.into = onto?.id
                let pushed = onto == nil ? tool.placingSelected(primary: c.id, to: rows[to].id, by: asked, later: later) : [:]
                if pushed.isEmpty {
                    // into a group they follow the finger along; else they can't go there and
                    // stay as they are
                    s.track = nil
                    s.goes = [:]
                    s.pushed = [:]
                    s.room = onto == nil ? 0 : asked
                } else {
                    s.track = rows[to].id
                    s.room = asked
                    s.pushed = pushed
                    var goes: [UInt64: UInt64] = [:]
                    for (track, clips) in pushed {
                        for x in clips where picked.contains(x.id) { goes[x.id] = track }
                    }
                    s.goes = to == from ? [:] : goes
                }
            }
            // the line shows only when they really got there
            let line = s.room == asked ? snap : nil
            if let line, line != s.snap {
                UISelectionFeedbackGenerator().selectionChanged()
            }
            s.snap = line
        } else {
            // every clip between the last point and this one, a few points apart
            let last = s.points.last ?? drag.location
            let far = hypot(drag.location.x - last.x, drag.location.y - last.y)
            let steps = max(Int(far / 4), 1)
            for i in 1 ... steps {
                let t = CGFloat(i) / CGFloat(steps)
                let p = CGPoint(x: last.x + (drag.location.x - last.x) * t, y: last.y + (drag.location.y - last.y) * t)
                if let c = clip(at: p, rows: rows, pitch: pitch), s.touched.insert(c.id).inserted {
                    tool.selecting?.formSymmetricDifference([c.id])
                    UISelectionFeedbackGenerator().selectionChanged()
                }
            }
            s.points.append(drag.location)
        }
        stroke = s
    }

    /// The finger lifted: the clips moved, if they were being; a picked clip only tapped is taken
    /// out from the others.
    private func strokeEnded(_ drag: DragGesture.Value) {
        guard let s = stroke else { return }
        stroke = nil
        guard let c = s.grabbed else { return }
        if hypot(drag.translation.width, drag.translation.height) < 6 {
            tool.selecting?.remove(c.id)
        } else if let group = s.into {
            tool.moveSelectedIntoGroup(group, by: s.asked)
        } else if let track = s.track, s.room != 0 || s.goes.isEmpty == false {
            tool.placeSelected(primary: c.id, to: track, by: s.room, later: s.later)
        }
    }

    /// Track `t`'s clips as the clips picked together being moved would leave them, pushed aside
    /// (the picked clips themselves as they are, following the finger); none if they aren't
    /// being moved onto it, or it isn't changed.
    private func pickedPushed(on t: TimelineTrackInfo) -> [TimelineClipInfo]? {
        guard let s = stroke, s.grabbed != nil, s.into == nil, let shown = s.pushed[t.id] else { return nil }
        let picked = tool.selecting ?? []
        return shown.filter { !picked.contains($0.id) } + t.clips.filter { picked.contains($0.id) }
    }

    /// How far track `t`'s clips show moved while a group clip they are inside is being dragged:
    /// as far as the group.
    private func following(_ t: TimelineTrackInfo, pitch: CGFloat) -> CGSize {
        guard let m = moving, let g = tool.clip(m.clip)?.1, g.kind == .group, tool.inside(t.id, group: g.id) else { return .zero }
        let x = m.snap != nil ? CGFloat(m.start - Int(g.start)) * pitch : m.dx
        return CGSize(width: x, height: m.dy)
    }

    /// Whether track `t`'s row draws over the others: a clip of it being dragged, a clip picked
    /// with others while they are, or it follows a group clip being dragged.
    private func raised(_ t: TimelineTrackInfo) -> Bool {
        if let m = moving {
            return t.clips.contains { $0.id == m.clip } || following(t, pitch: 1) != .zero
        }
        if stroke?.grabbed != nil, let picked = tool.selecting {
            return t.clips.contains { picked.contains($0.id) }
        }
        return false
    }

    /// Track `t`'s clips as the clip being moved onto it would leave them, pushed aside (that clip
    /// itself as it is, following the finger); none if no clip is being moved onto it.
    private func pushedAside(on t: TimelineTrackInfo, rows: [TimelineTrackInfo]) -> [TimelineClipInfo]? {
        guard let m = moving, m.into == nil, m.landing != nil, !m.pushed.isEmpty, rows.indices.contains(m.row),
              rows[m.row].id == t.id else { return nil }
        return m.pushed.filter { $0.id != m.clip } + t.clips.filter { $0.id == m.clip }
    }

    /// How far down (up, negative) picked clip `c` of row `row` shows while the clips picked
    /// together are being moved to other tracks: as far as the row it goes to.
    private func pickedRows(_ c: TimelineClipInfo, row: Int, rows: [TimelineTrackInfo]) -> CGFloat {
        guard let to = stroke?.goes[c.id], let r = rows.firstIndex(where: { $0.id == to }) else { return 0 }
        return CGFloat(r - row) * rowHeight
    }

    /// A grip on the picked clip's start or end, dragged to trim it.
    private func edgeGrip(_ c: TimelineClipInfo, on t: TimelineTrackInfo, leading: Bool, rows: [TimelineTrackInfo], pitch: CGFloat) -> some View {
        Capsule()
            .fill(Color.white)
            .frame(width: 5, height: rowHeight - 20)
            .frame(width: 22, height: rowHeight - 6)
            .contentShape(Rectangle())
            .frame(maxWidth: .infinity, alignment: leading ? .leading : .trailing)
            .highPriorityGesture(
                // measured on the screen: the clip itself moves under the finger as it trims
                DragGesture(minimumDistance: 2, coordinateSpace: .global)
                    .updating($edgeHeld) { _, held, _ in held = true }
                    .onChanged { drag in
                        trim(c, on: t, leading: leading, rows: rows, by: drag.translation.width, pitch: pitch)
                    }
                    .onEnded { _ in
                        guard let tr = trimming else { return }
                        trimming = nil
                        tool.trim(tr.clip, leading: tr.leading, to: tr.frame)
                        tool.endEdgeDrag()
                    }
            )
    }

    /// Clip `c`'s edge dragged `dx` from where it was: the frame asked for (snapped), and its
    /// track's clips as the trim would leave them.
    private func trim(_ c: TimelineClipInfo, on t: TimelineTrackInfo, leading: Bool, rows: [TimelineTrackInfo], by dx: CGFloat, pitch: CGFloat) {
        let from = trimming?.clip == c.id ? trimming?.from ?? 0 : Int(leading ? c.start : c.start + c.duration)
        let at = CGFloat(from) + dx / pitch
        let snap = snapped(at, onto: snaps(end: !leading, not: c.id, rows: rows), pitch: pitch)
        let frame = max(snap ?? Int(at.rounded()), 0)
        if trimming == nil {
            tool.edgeDragging = true
        }
        var next = Trimming(clip: c.id, track: t.id, leading: leading, from: from, frame: frame, clips: trimming?.clips ?? [], asked: snap)
        if trimming?.frame != frame || trimming?.clip != c.id {
            next.clips = frame == from ? [] : tool.trimPreview(c.id, leading: leading, to: frame)
        }
        // the line shows only when the edge really is on the frame it snapped to
        let edge = (next.clips.first { $0.id == c.id }).map { Int(leading ? $0.start : $0.start + $0.duration) } ?? from
        next.snap = snap == edge ? snap : nil
        if let s = next.snap, s != trimming?.snap {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        trimming = next
    }

    /// The edge policy turned over (or back) mid-drag: the track as the trim would now leave it.
    private func retrim() {
        guard var tr = trimming else { return }
        tr.clips = tr.frame == tr.from ? [] : tool.trimPreview(tr.clip, leading: tr.leading, to: tr.frame)
        let edge = (tr.clips.first { $0.id == tr.clip }).map { Int(tr.leading ? $0.start : $0.start + $0.duration) } ?? tr.from
        tr.snap = tr.asked == edge ? tr.asked : nil
        trimming = tr
    }
}

/// A lane's frame lines, under its clips: one at each frame where frames are wide enough to tell
/// apart, and a stronger one every `every` frames, where the ruler has a number. Made again only
/// when the zoom or the length changes.
private struct FrameLines: View, Equatable {
    let frames: Int
    let pitch: CGFloat
    let every: Int
    let height: CGFloat
    let ink: Color

    var body: some View {
        let step = max(every, 1)
        ZStack(alignment: .topLeading) {
            if pitch >= 10 {
                Path { p in
                    for f in 1 ..< max(frames, 1) where f % step != 0 {
                        p.addRect(CGRect(x: CGFloat(f) * pitch, y: 0, width: 1, height: height))
                    }
                }
                .fill(ink.opacity(0.05))
            }
            Path { p in
                for f in stride(from: step, to: max(frames, 1), by: step) {
                    p.addRect(CGRect(x: CGFloat(f) * pitch, y: 0, width: 1, height: height))
                }
            }
            .fill(ink.opacity(0.12))
        }
        .frame(width: CGFloat(max(frames, 1)) * pitch, height: height, alignment: .topLeading)
    }
}

/// A pinch on the tracks zooms them: the frames wider or narrower, the frame under the fingers
/// staying under them. It sits behind the tracks to find the scroll view they are in, and takes
/// that one's pinches.
private struct ZoomPinch: UIViewRepresentable {
    let tool: TimelineTool
    let frames: Int
    let shownWidth: CGFloat

    func makeCoordinator() -> Coordinator {
        Coordinator(tool: tool)
    }

    func makeUIView(context: Context) -> Probe {
        let view = Probe()
        view.isUserInteractionEnabled = false
        view.coordinator = context.coordinator
        return view
    }

    func updateUIView(_ view: Probe, context: Context) {
        context.coordinator.frames = frames
        context.coordinator.shownWidth = shownWidth
    }

    final class Probe: UIView {
        weak var coordinator: Coordinator?

        override func didMoveToWindow() {
            super.didMoveToWindow()
            coordinator?.attach(from: self)
        }
    }

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        let tool: TimelineTool
        var frames = 1
        var shownWidth: CGFloat = 0
        private weak var scroll: UIScrollView?
        private var pinch: UIPinchGestureRecognizer?
        /// The frame width the pinch began at, the frame under the fingers then, and where the
        /// scroll view should be for it to stay there.
        private var fromPitch: CGFloat = 0
        private var anchor: CGFloat = 0
        private var wanted: CGFloat = 0

        init(tool: TimelineTool) {
            self.tool = tool
        }

        /// Takes the pinches of the scroll view `view` is in.
        func attach(from view: UIView) {
            var at = view.superview
            while let v = at, !(v is UIScrollView) {
                at = v.superview
            }
            guard let found = at as? UIScrollView, found !== scroll else { return }
            if let pinch {
                scroll?.removeGestureRecognizer(pinch)
            }
            let made = UIPinchGestureRecognizer(target: self, action: #selector(pinched(_:)))
            made.delegate = self
            found.addGestureRecognizer(made)
            scroll = found
            pinch = made
        }

        @objc private func pinched(_ g: UIPinchGestureRecognizer) {
            guard let scroll, g.numberOfTouches >= 2 || g.state != .changed else { return }
            // in the scroll view's own space, which is its content's
            let x = g.location(in: scroll).x
            switch g.state {
            case .began:
                fromPitch = tool.pitchShown(frames: frames, width: shownWidth)
                anchor = x / max(fromPitch, 0.01)
            case .changed:
                tool.zoom(to: fromPitch * g.scale, frames: frames, width: shownWidth)
                let pitch = tool.pitchShown(frames: frames, width: shownWidth)
                let onScreen = x - scroll.contentOffset.x
                let width = max(CGFloat(frames + 8) * pitch, shownWidth)
                wanted = min(max(anchor * pitch - onScreen, 0), max(width - scroll.bounds.width, 0))
                scroll.contentOffset.x = wanted
                // and again once the tracks are laid out at their new width
                DispatchQueue.main.async { [weak self, weak scroll] in
                    guard let self, let scroll else { return }
                    scroll.contentOffset.x = self.wanted
                }
            default:
                break
            }
        }

        func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
            true
        }

        /// Two fingers down while an edge is dragged turn its policy over rather than zoom.
        func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
            !tool.edgeDragging
        }
    }
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
        if let id = tool.picked, tool.info?.simple == false, !tool.isPlaying, tool.shownFrame == nil,
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

// MARK: - Camera

/// The camera's frame over the canvas while the camera is open: where the output frame looks at
/// the playhead, with a grip at each corner and a knob on a stem off its top edge. Its edge is
/// dragged to move it, a corner to zoom (the frame keeps the canvas's shape), the knob to turn
/// it; each drag keys the camera at the playhead (one undo step when let go). Only the edge and
/// the grips take touches: inside it the canvas is drawn on as ever.
struct CameraFrameOverlay: View {
    let model: CanvasModel

    private enum Grip {
        case move, corner, turn
    }

    /// The drag under way: what it does, where it started (canvas pixels), and the camera then.
    @State private var dragging: (grip: Grip, start: CGPoint, camera: TimelineCamera)?

    var body: some View {
        let tool = model.timeline
        if tool.cameraOpen, tool.info?.simple == false, !tool.isPlaying, tool.shownFrame == nil,
           let camera = tool.camera, camera.corners.count == 8 {
            let corners = (0 ..< 4).map {
                model.canvasToWindow(CGPoint(x: CGFloat(camera.corners[$0 * 2]), y: CGFloat(camera.corners[$0 * 2 + 1])))
            }
            let (stemFoot, knob) = Self.knob(corners)
            let edge = Path { p in
                p.addLines(corners)
                p.closeSubpath()
            }
            ZStack(alignment: .topLeading) {
                Canvas { context, _ in
                    var stem = Path()
                    stem.move(to: stemFoot)
                    stem.addLine(to: knob)
                    for (path, width) in [(edge, 4.0), (stem, 3.5)] {
                        context.stroke(path, with: .color(.white.opacity(0.7)), lineWidth: width)
                    }
                    context.stroke(edge, with: .color(Pc.accent), style: StrokeStyle(lineWidth: 2, dash: camera.keyed ? [] : [8, 6]))
                    context.stroke(stem, with: .color(Pc.accent), lineWidth: 1.5)
                    for at in corners + [knob] {
                        let r: CGFloat = 7
                        context.fill(Path(ellipseIn: CGRect(x: at.x - r, y: at.y - r, width: r * 2, height: r * 2)), with: .color(Pc.accent))
                        context.fill(Path(ellipseIn: CGRect(x: at.x - r + 2, y: at.y - r + 2, width: r * 2 - 4, height: r * 2 - 4)), with: .color(.white))
                    }
                }
                .allowsHitTesting(false)
                // the edge, a band either side of it
                Color.clear
                    .contentShape(edge.strokedPath(StrokeStyle(lineWidth: 26)))
                    .gesture(drag(.move, camera))
                ForEach(0 ..< 4, id: \.self) { i in
                    grip(at: corners[i]).gesture(drag(.corner, camera))
                }
                grip(at: knob).gesture(drag(.turn, camera))
            }
            .onAppear {
                if model.viewMatrix.isEmpty {
                    model.refreshView()
                }
            }
        }
    }

    /// Where the turning knob's stem leaves the top edge, and the knob: out from the edge's
    /// middle, away from the frame.
    private static func knob(_ c: [CGPoint]) -> (CGPoint, CGPoint) {
        let foot = CGPoint(x: (c[0].x + c[1].x) / 2, y: (c[0].y + c[1].y) / 2)
        let middle = CGPoint(x: (c[0].x + c[2].x) / 2, y: (c[0].y + c[2].y) / 2)
        let dx = foot.x - middle.x, dy = foot.y - middle.y
        let length = max(hypot(dx, dy), 1)
        return (foot, CGPoint(x: foot.x + dx / length * 34, y: foot.y + dy / length * 34))
    }

    private func grip(at: CGPoint) -> some View {
        Color.clear
            .frame(width: 40, height: 40)
            .contentShape(Circle())
            .position(at)
    }

    /// Window point `p` (points) on the canvas.
    private func canvasPoint(_ p: CGPoint) -> CGPoint? {
        model.windowToCanvas(CGPoint(x: p.x * model.pixels, y: p.y * model.pixels))
    }

    private func drag(_ grip: Grip, _ camera: TimelineCamera) -> some Gesture {
        DragGesture(minimumDistance: 2, coordinateSpace: .global)
            .onChanged { g in
                guard let at = canvasPoint(g.location) else { return }
                if dragging == nil, let start = canvasPoint(g.startLocation) {
                    dragging = (grip, start, camera)
                }
                key(at, done: false)
            }
            .onEnded { g in
                if let at = canvasPoint(g.location) {
                    key(at, done: true)
                }
                dragging = nil
            }
    }

    /// The camera keyed as the drag to canvas point `at` puts it.
    private func key(_ at: CGPoint, done: Bool) {
        guard let d = dragging else { return }
        let c = d.camera
        let middle = CGPoint(x: CGFloat(c.x), y: CGFloat(c.y))
        var (x, y, zoom, rotation) = (c.x, c.y, c.zoom, c.rotation)
        switch d.grip {
        case .move:
            x += Float(at.x - d.start.x)
            y += Float(at.y - d.start.y)
        case .corner:
            // the frame as far out as the corner is: the further, the wider it looks
            let from = hypot(d.start.x - middle.x, d.start.y - middle.y)
            let to = max(hypot(at.x - middle.x, at.y - middle.y), 1)
            zoom = c.zoom * Float(from / to)
        case .turn:
            // the frame turns with the knob, which turns the canvas in it the other way
            let from = atan2(d.start.y - middle.y, d.start.x - middle.x)
            let to = atan2(at.y - middle.y, at.x - middle.x)
            rotation = c.rotation - Float((to - from) * 180 / .pi)
        }
        model.timeline.setCameraKey(x: x, y: y, zoom: zoom, rotation: rotation, done: done)
    }
}

// MARK: - Keys

/// The picked clip's pose at the playhead: each slider keys its channel there as it moves (one
/// undo step when let go), new keys easing as chosen; and the keys here taken away.
private struct KeysPage: View {
    let model: CanvasModel
    let tool: TimelineTool
    let clip: UInt64
    let frame: UInt32

    /// The pose as a slider moves it, before it is let go.
    @State private var live: TimelinePose?
    /// The easing editor open in the panel's place.
    @State private var easingOpen = false

    private static let still = TimelinePose(x: 0, y: 0, scaleX: 1, scaleY: 1, rotation: 0, opacity: 1)
    private static let leastScale: Float = 0.1
    private static let scaleSpan: Float = 40

    var body: some View {
        if easingOpen, (tool.keys[clip] ?? []).contains(where: { $0.frame == frame }) {
            EasingPage(tool: tool, clip: clip, frame: frame) { easingOpen = false }
        } else {
            keys
        }
    }

    @ViewBuilder
    private var keys: some View {
        let canvas = model.session.info()
        let (w, h) = (Float(max(canvas.width, 1)), Float(max(canvas.height, 1)))
        // read again whenever the timeline is (a key dragged on the canvas)
        let _ = tool.revision
        let pose = live ?? tool.pose(clip) ?? Self.still
        let keyedHere = (tool.keys[clip] ?? []).contains { $0.frame == frame }
        let easings: [(TimelineEasing, String)] = [(.linear, "Linear"), (.smooth, "Smooth"), (.easeInOut, "Ease"), (.hold, "Hold")]
        VStack(alignment: .leading, spacing: 0) {
            PanelTitle(text: "Keys at frame \(frame + 1)", bottom: 14)
            HStack(spacing: 4) {
                ForEach(easings, id: \.1) { easing, label in
                    let chosen = tool.keyEasing == easing
                    Text(verbatim: label)
                        .font(.system(size: 15, weight: chosen ? .semibold : .regular))
                        .foregroundStyle(chosen ? Color.white : Pc.panelTitle)
                        .frame(maxWidth: .infinity, minHeight: 32)
                        .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(chosen ? Pc.accent : Color.clear))
                        .contentShape(Rectangle())
                        .onTapGesture { tool.keyEasing = easing }
                }
            }
            .padding(4)
            .background(Pc.actionsGroup)
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .circular))
            TlSliderGroup {
                TlSlider(label: "Move across", value: "\(Int(pose.x.rounded())) px", share: (pose.x / w + 1) / 2,
                         onEnd: { end(.position) }) { v in
                    drag(.position) { $0.x = (v * 2 - 1) * w }
                }
                TlSlider(label: "Move down", value: "\(Int(pose.y.rounded())) px", share: (pose.y / h + 1) / 2,
                         onEnd: { end(.position) }) { v in
                    drag(.position) { $0.y = (v * 2 - 1) * h }
                }
                TlSlider(label: "Scale", value: "\(wholeNumber(pose.scaleX * 100)) %",
                         share: log(max(pose.scaleX, Self.leastScale) / Self.leastScale) / log(Self.scaleSpan),
                         onEnd: { end(.scale) }) { v in
                    let s = Self.leastScale * pow(Self.scaleSpan, v)
                    drag(.scale) {
                        $0.scaleX = s
                        $0.scaleY = s
                    }
                }
                TlSlider(label: "Rotation", value: "\(wholeNumber(pose.rotation))°", share: (pose.rotation + 360) / 720,
                         onEnd: { end(.rotation) }) { v in
                    drag(.rotation) { $0.rotation = v * 720 - 360 }
                }
                TlSlider(label: "Opacity", value: "\(wholeNumber(pose.opacity * 100)) %", share: pose.opacity,
                         onEnd: { end(.opacity) }) { v in
                    drag(.opacity) { $0.opacity = v }
                }
            }
            .padding(.top, 10)
            // an adjustment clip's own values
            AdjustmentKeyRows(model: model, tool: tool, clip: clip, frame: frame)
            if keyedHere {
                let moveHere = (tool.keys[clip] ?? []).contains { $0.frame == frame && $0.channel == .position }
                let bent = tool.path?.keys.first { $0.frame == frame }?.handles != nil
                TlGroup {
                    TlAction(label: "Easing…") { easingOpen = true }
                    if moveHere {
                        TlRowLine()
                        TlAction(label: bent ? "Straighten Path Here" : "Bend Path Here") {
                            bent ? tool.straightenPath(clip, at: frame) : tool.bendPath(clip, at: frame)
                        }
                    }
                    TlRowLine()
                    RemoveKeysHere(tool: tool, clip: clip, frame: frame) { live = nil }
                }
                .padding(.top, 10)
            }
            // the keys copied, and pasted onto the clip at the playhead from its frame on
            let hasKeys = !(tool.keys[clip] ?? []).isEmpty
            if hasKeys || tool.clipboard.keys {
                TlGroup {
                    if keyedHere {
                        TlAction(label: "Copy Keys Here") { tool.copyKeys(clip, here: true) }
                        TlRowLine()
                    }
                    if hasKeys {
                        TlAction(label: "Copy All Keys") { tool.copyKeys(clip, here: false) }
                    }
                    if tool.clipboard.keys {
                        if hasKeys {
                            TlRowLine()
                        }
                        TlAction(label: "Paste Keys Here") {
                            live = nil
                            tool.pasteKeys()
                        }
                    }
                }
                .padding(.top, 10)
            }
        }
        .onChange(of: frame) { live = nil }
    }

    private func values(_ p: TimelinePose, _ channel: TimelineChannel) -> [Float] {
        switch channel {
        case .position: [p.x, p.y]
        case .scale: [p.scaleX, p.scaleY]
        case .rotation: [p.rotation]
        case .opacity: [p.opacity]
        case .adjustment: []
        }
    }

    /// A slider moved: the pose shown with `edit`, keyed at once.
    private func drag(_ channel: TimelineChannel, _ edit: (inout TimelinePose) -> Void) {
        var p = live ?? tool.pose(clip) ?? Self.still
        edit(&p)
        live = p
        tool.setKey(clip, channel, values(p, channel), done: false)
    }

    /// A slider let go: its key is one undo step.
    private func end(_ channel: TimelineChannel) {
        guard let p = live else { return }
        tool.setKey(clip, channel, values(p, channel), done: true)
        live = nil
    }
}

// MARK: - Easing

/// The small easing editor: how the keys at the playhead ease into the next, as a curve, its
/// kind, that kind's numbers, and how often it repeats (arriving each time, or looping back),
/// maybe backwards. Every change shows at once; a slider let go or a tap is one undo step.
private struct EasingPage: View {
    let tool: TimelineTool
    let clip: UInt64
    let frame: UInt32
    let onBack: () -> Void

    /// The easing as a slider moves it, before it is let go.
    @State private var live: EasingSpec?

    private static let kinds: [(EasingKind, String)] = [
        (.linear, "Linear"), (.smooth, "Smooth"), (.cubicBezier, "Ease"), (.hold, "Hold"),
        (.bounce, "Bounce"), (.elastic, "Elastic"), (.steps, "Steps"), (.elasticSteps, "Springy Steps"),
        (.cyclic, "Cyclic"), (.random, "Random"), (.curve, "Overshoot"),
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
            EasingCurveView(spec: spec)
                .frame(height: 110)
                .padding(.vertical, 10)
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

/// An easing's curve: time across, how far along up, the start and end levels faint.
private struct EasingCurveView: View {
    let spec: EasingSpec

    var body: some View {
        let samples = easingCurve(s: spec, samples: 96)
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
            ctx.stroke(line, with: .color(Pc.accent), lineWidth: 2.5)
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

// MARK: - Settings

/// The timeline's settings: playback, frames a second, length, how long new drawings are held;
/// the onion skins, on a page of their own opened from their row; and taking the artwork off the
/// timeline.
private struct TimelineSettingsPage: View {
    let tool: TimelineTool
    let info: TimelineInfo

    /// A new frame rate keeps the animation's timing (every clip and key moved to match) rather
    /// than its frames; the rate as its slider moves, before it is let go.
    @AppStorage("timelineKeepTiming") private var keepTiming = true
    @State private var fps: UInt32?
    /// The onion skins' page open in the settings' place.
    @State private var onionOpen = false

    var body: some View {
        Group {
            if onionOpen {
                OnionSkinsPage(tool: tool, info: info, most: mostOnion) { onionOpen = false }
            } else {
                settings
            }
        }
        #if DEBUG
        .onAppear {
            if ScreenCapture.onionOpens {
                onionOpen = true
            }
        }
        #endif
    }

    @ViewBuilder
    private var settings: some View {
        let ways: [(AnimationPlayback, String)] = [(.loop, "Loop"), (.pingPong, "Ping-Pong"), (.oneShot, "One Shot")]
        let longest = max(Int(info.duration) * 2, 240)
        VStack(alignment: .leading, spacing: 0) {
            PanelTitle(text: "Timeline", bottom: 18)
            TlGroup {
                ForEach(ways.indices, id: \.self) { i in
                    let (way, label) = ways[i]
                    let chosen = info.playback == way
                    if i > 0 {
                        TlRowLine()
                    }
                    HStack(spacing: 0) {
                        Text(verbatim: label)
                            .font(.system(size: 17))
                            .foregroundStyle(chosen ? Color.white : Pc.panelTitle)
                        Spacer(minLength: 0)
                        if chosen {
                            PcIconView(PcIcons.menuTick, size: 22, color: .white)
                        }
                    }
                    .padding(.horizontal, 16)
                    .frame(height: 40)
                    .background(chosen ? Pc.accent : Color.clear)
                    .contentShape(Rectangle())
                    .onTapGesture { tool.change { $0.playback = way } }
                }
            }
            TlSliderGroup {
                let shownFps = fps ?? info.fps
                TlSlider(
                    label: "Frames Per Second", value: "\(shownFps)", share: Float(Int(shownFps) - 1) / Float(mostFps - 1),
                    onEnd: {
                        if let f = fps {
                            tool.retime(f)
                        }
                        fps = nil
                    }
                ) { v in
                    let f = UInt32(1 + wholeNumber(v * Float(mostFps - 1)))
                    if keepTiming {
                        fps = f
                    } else {
                        tool.change { $0.fps = f }
                    }
                }
                TlSlider(label: "Length", value: "\(info.duration) frames", share: Float(Int(info.duration) - 1) / Float(longest - 1)) { v in
                    tool.change { $0.duration = UInt32(1 + wholeNumber(v * Float(longest - 1))) }
                }
                TlSlider(
                    label: "New drawings held for", value: info.exposure == 1 ? "1 frame" : "\(info.exposure) frames",
                    share: Float(Int(info.exposure) - 1) / Float(mostExposure - 1)
                ) { v in
                    tool.change { $0.exposure = UInt32(1 + wholeNumber(v * Float(mostExposure - 1))) }
                }
            }
            .padding(.top, 10)
            // the onion skins: their counts, opacity and colours are a page of their own
            TlGroup {
                HStack(spacing: 0) {
                    Text(verbatim: "Onion Skins")
                        .font(.system(size: 17))
                        .foregroundStyle(Pc.panelTitle)
                        .lineLimit(1)
                    Spacer(minLength: 8)
                    Text(verbatim: OnionSkinsPage.summary(tool.onion))
                        .font(.system(size: 17))
                        .foregroundStyle(Pc.slideValue)
                        .lineLimit(1)
                    PcIconView(PcIcons.chevronRight, size: 26, color: Pc.slideLabel)
                        .frame(width: 26, height: 26)
                }
                .padding(.leading, 16)
                .padding(.trailing, 8)
                .frame(height: 44)
                .contentShape(Rectangle())
                .onTapGesture { onionOpen = true }
                if let l = tool.loop {
                    TlRowLine()
                    TlAction(label: "Stop Looping Frames \(l.start + 1)–\(l.end)") { tool.setLoop(nil) }
                }
            }
            .padding(.top, 10)
            if !info.simple {
                // what a clip's dragged edge does when it reaches the next clip
                let policies: [(TimelineEdgePolicy, String)] = [(.stop, "Stop"), (.push, "Push"), (.trim, "Trim")]
                VStack(alignment: .leading, spacing: 4) {
                    Text(verbatim: "A dragged edge, at the next clip")
                        .font(.system(size: 13))
                        .foregroundStyle(Pc.slideLabel)
                        .padding(.leading, 12)
                        .padding(.top, 6)
                    HStack(spacing: 4) {
                        ForEach(policies, id: \.1) { policy, label in
                            let chosen = tool.edgePolicy == policy
                            Text(verbatim: label)
                                .font(.system(size: 15, weight: chosen ? .semibold : .regular))
                                .foregroundStyle(chosen ? Color.white : Pc.panelTitle)
                                .frame(maxWidth: .infinity, minHeight: 32)
                                .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(chosen ? Pc.accent : Color.clear))
                                .contentShape(Rectangle())
                                .onTapGesture { tool.setEdgePolicy(policy) }
                        }
                    }
                    .padding(4)
                }
                .background(Pc.actionsGroup)
                .clipShape(RoundedRectangle(cornerRadius: 14, style: .circular))
                .padding(.top, 10)
            }
            TlGroup {
                if !info.simple {
                    HStack(spacing: 0) {
                        Text(verbatim: "Snap Dragged Clips")
                            .font(.system(size: 17))
                            .foregroundStyle(Pc.panelTitle)
                            .lineLimit(1)
                        Spacer(minLength: 8)
                        CheckBox(checked: tool.snapping, fill: Pc.accent)
                    }
                    .padding(.horizontal, 16)
                    .frame(height: 44)
                    .contentShape(Rectangle())
                    .onTapGesture { tool.setSnapping(!tool.snapping) }
                    TlRowLine()
                }
                HStack(spacing: 0) {
                    Text(verbatim: "Keep Timing When FPS Changes")
                        .font(.system(size: 17))
                        .foregroundStyle(Pc.panelTitle)
                        .lineLimit(1)
                        .minimumScaleFactor(0.8)
                    Spacer(minLength: 8)
                    CheckBox(checked: keepTiming, fill: Pc.accent)
                }
                .padding(.horizontal, 16)
                .frame(height: 44)
                .contentShape(Rectangle())
                .onTapGesture { keepTiming.toggle() }
                if !tool.markers.isEmpty {
                    TlRowLine()
                    TlAction(label: tool.markers.count == 1 ? "Clear the Marker" : "Clear \(tool.markers.count) Markers") {
                        tool.clearMarkers()
                    }
                }
            }
            .padding(.top, 10)
        }
    }
}

// MARK: - Pieces

struct BarWord: View {
    let label: String
    var bold = false
    var dim = false
    var tint: Color?
    let action: () -> Void

    var body: some View {
        Text(verbatim: label)
            .font(.system(size: 17, weight: bold ? .bold : .regular))
            .foregroundStyle(tint ?? (dim ? inkDim : ink))
            .lineLimit(1)
            .frame(height: 30)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }
}

/// A panel standing on the bar, its arrow pointing down at `arrowX` from its left edge.
struct TlPopover<Content: View>: View {
    let width: CGFloat
    let arrowX: CGFloat
    /// How tall it may stand; a page taller than that scrolls. None: as tall as its page.
    var room: CGFloat?
    @ViewBuilder let content: () -> Content

    var body: some View {
        Group {
            if let room {
                Capped(height: room - 16 - Pc.arrowHeight) {
                    ViewThatFits(in: .vertical) {
                        VStack(alignment: .leading, spacing: 0) { content() }
                        ScrollView(.vertical, showsIndicators: false) {
                            VStack(alignment: .leading, spacing: 0) { content() }
                        }
                    }
                }
            } else {
                VStack(alignment: .leading, spacing: 0) {
                    content()
                }
            }
        }
        .padding(8)
        .frame(width: width, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 24, style: .circular).fill(Pc.popoverFill))
        .padding(.bottom, Pc.arrowHeight)
        .background(alignment: .bottomLeading) {
            Path { p in
                let h = Pc.arrowHeight
                p.move(to: CGPoint(x: arrowX - h, y: 0))
                p.addLine(to: CGPoint(x: arrowX, y: h))
                p.addLine(to: CGPoint(x: arrowX + h, y: 0))
                p.closeSubpath()
            }
            .fill(Pc.popoverFill)
            .frame(width: width, height: Pc.arrowHeight)
        }
        // while one of its sliders is dragged, only that slider shows: the page stands over the
        // canvas, and what the slider changes is seen there as it goes (and with it, what the
        // page itself shows of the change: the easing's curve)
        .mask {
            GeometryReader { place in
                let frame = place.frame(in: .global)
                let row = TlSliding.shared.row.flatMap { frame.intersects($0) ? $0 : nil }
                let kept = row == nil ? nil : TlSliding.shared.kept.flatMap { frame.intersects($0) ? $0 : nil }
                ZStack(alignment: .topLeading) {
                    Rectangle().opacity(row == nil ? 1 : 0)
                    if let row {
                        RoundedRectangle(cornerRadius: 16, style: .circular)
                            .frame(width: row.width, height: row.height)
                            .offset(x: row.minX - frame.minX, y: row.minY - frame.minY)
                    }
                    if let kept {
                        RoundedRectangle(cornerRadius: 14, style: .circular)
                            .frame(width: kept.width, height: kept.height)
                            .offset(x: kept.minX - frame.minX, y: kept.minY - frame.minY)
                    }
                }
                .animation(.easeOut(duration: 0.15), value: row == nil)
            }
        }
        .onAppear { TlSliding.shared.row = nil }
        .padding(.bottom, 2)
        .contentShape(Rectangle())
        .onTapGesture {}
    }
}

/// Its one view as tall as that view is when offered `height` at most: a page that fits is its
/// own height, one that doesn't is `height` (and scrolls).
private struct Capped: Layout {
    let height: CGFloat

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        subviews.first?.sizeThatFits(ProposedViewSize(width: proposal.width, height: height)) ?? .zero
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        subviews.first?.place(at: bounds.origin, proposal: ProposedViewSize(width: bounds.width, height: bounds.height))
    }
}

struct PanelTitle: View {
    let text: String
    let bottom: CGFloat

    var body: some View {
        Text(verbatim: text)
            .font(.system(size: 23, weight: .bold))
            .foregroundStyle(Pc.panelTitle)
            .lineLimit(1)
            .padding(.leading, 14)
            .padding(.top, 11)
            .padding(.bottom, bottom)
    }
}

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

/// A row of a choice, filled with the accent colour and ticked when it is the one chosen.
struct TlChoice: View {
    let label: String
    let chosen: Bool
    let action: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(chosen ? Color.white : Pc.panelTitle)
                .lineLimit(1)
            Spacer(minLength: 0)
            if chosen {
                PcIconView(PcIcons.menuTick, size: 22, color: .white)
            }
        }
        .padding(.horizontal, 16)
        .frame(height: 40)
        .background(chosen ? Pc.accent : Color.clear)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

struct TlAction: View {
    let label: String
    let action: () -> Void

    var body: some View {
        Text(verbatim: label)
            .font(.system(size: 17))
            .foregroundStyle(Pc.panelTitle)
            .lineLimit(1)
            .padding(.leading, 16)
            .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
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

/// A row of choices, one lit: a page's parts, or a setting of a few.
struct TlSegments<Choice: Hashable>: View {
    let choices: [(Choice, String)]
    let chosen: Choice
    let choose: (Choice) -> Void

    var body: some View {
        HStack(spacing: 4) {
            ForEach(choices, id: \.0) { choice, label in
                let lit = choice == chosen
                Text(verbatim: label)
                    .font(.system(size: 15, weight: lit ? .semibold : .regular))
                    .foregroundStyle(lit ? Color.white : Pc.panelTitle)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, minHeight: 32)
                    .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(lit ? Pc.accent : Color.clear))
                    .contentShape(Rectangle())
                    .onTapGesture { choose(choice) }
            }
        }
        .padding(4)
        .background(Pc.actionsGroup)
        .clipShape(RoundedRectangle(cornerRadius: 14, style: .circular))
    }
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

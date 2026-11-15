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
/// The full view: a frame's width, a track's height, the ruler's, the track names' column.
private let framePitch: CGFloat = 30
private let rowHeight: CGFloat = 44
private let rulerHeight: CGFloat = 24
private let headerWidth: CGFloat = 196
private let mostRowsShown = 4
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

/// The timeline as the core keeps it: its tracks and clips, the playhead, playing.
@Observable
final class TimelineTool {
    @ObservationIgnored private unowned let model: CanvasModel

    private(set) var info: TimelineInfo?
    /// Bottom first, as the core gives them.
    private(set) var tracks: [TimelineTrackInfo] = []
    /// Each drawing's picture, by its layer.
    private(set) var thumbnails: [UInt64: UIImage] = [:]
    /// The clip picked in the full view (its edge trims it, the header acts on it).
    var picked: UInt64?
    /// Playing (or scrubbing): the frame shown.
    private(set) var shownFrame: UInt32?
    private(set) var isPlaying = false
    var settingsOpen = false
    var optionsOpen = false
    /// The picked clip's keys, open over the panel.
    var keysOpen = false
    /// How new keys ease into the next.
    var keyEasing: TimelineEasing = .smooth
    /// Each clip's keys, by clip.
    private(set) var keys: [UInt64: [TimelineKeyInfo]] = [:]
    /// The picked clip's options open over the panel: its mask, a group's cycle.
    var clipOpen = false
    /// Clips being gathered into a group (a tap takes one in or out), while grouping.
    var grouping: Set<UInt64>?
    /// The camera's frame shown over the canvas, dragged to key the camera at the playhead.
    var cameraOpen = false
    /// The camera at the playhead, and the frames it has keys on.
    private(set) var camera: TimelineCamera?
    private(set) var cameraKeys: [UInt32] = []
    /// The sync markers' frames, in order, and what the timeline clipboard holds.
    private(set) var markers: [UInt32] = []
    private(set) var clipboard = TimelineClipboardInfo(clips: 0, keys: false)
    /// Counts reads from the session, for what is read through it rather than kept here (a pose).
    private(set) var revision = 0

    @ObservationIgnored private var playing: Task<Void, Never>?
    @ObservationIgnored private var reading: Task<Void, Never>?
    @ObservationIgnored private var onionKept: UInt32 = 1

    init(model: CanvasModel) {
        self.model = model
    }

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

    /// Reads it again from the session, after any change.
    func refresh() {
        let session = model.session
        info = session.timelineInfo()
        guard info != nil else {
            tracks = []
            picked = nil
            settingsOpen = false
            optionsOpen = false
            stopPlaying()
            return
        }
        tracks = session.timelineTracks()
        if let p = picked, clip(p) == nil {
            picked = nil
        }
        if picked == nil {
            keysOpen = false
            // with clips copied, the Clip page stays open to paste them
            clipOpen = clipOpen && session.timelineClipboard().clips > 0
        }
        camera = session.timelineCamera()
        cameraKeys = session.timelineCameraKeys()
        markers = session.timelineMarkers()
        clipboard = session.timelineClipboard()
        var read: [UInt64: [TimelineKeyInfo]] = [:]
        for t in tracks {
            for c in t.clips {
                let k = session.timelineKeys(id: c.id)
                if !k.isEmpty {
                    read[c.id] = k
                }
            }
        }
        keys = read
        revision += 1
        readThumbnails()
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

    private func readThumbnails() {
        reading?.cancel()
        let session = model.session
        let roots = tracks.flatMap { $0.clips.map(\.root) }.filter { $0 != 0 }
        reading = Task { @MainActor [weak self] in
            for id in roots {
                let made = await Task.detached(priority: .utility) {
                    session.frameThumbnail(id: id, maxSide: thumbSide).flatMap { CanvasModel.image($0) }
                }.value
                guard let self, !Task.isCancelled else { return }
                self.thumbnails[id] = made
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
    /// grouping, it is taken into the group or out of it instead.
    func pick(_ clip: TimelineClipInfo, on track: TimelineTrackInfo) {
        if grouping != nil {
            grouping?.formSymmetricDifference([clip.id])
            return
        }
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
            model.showNotice("Move the playhead inside the drawing to split it there.", warning: true)
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

    /// Starts gathering clips into a group, the picked one first.
    func startGrouping() {
        grouping = Set(picked.map { [$0] } ?? [])
        clipOpen = false
        keysOpen = false
        settingsOpen = false
    }

    /// The clips gathered made into a group, which is picked.
    func makeGroup() {
        let ids = Array(grouping ?? [])
        grouping = nil
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
            var step: Int32 = 1
            while !Task.isCancelled {
                guard let self, let info = self.info else { return }
                self.shownFrame = at
                session.setTimelinePlaying(frame: at)
                try? await Task.sleep(for: .milliseconds(1000 / max(Int(info.fps), 1)))
                guard let next = animationNextStep(playback: info.playback, len: info.duration, at: at, step: step) else {
                    self.isPlaying = false
                    return
                }
                at = next.at
                step = next.step
            }
        }
    }

    func stopPlaying() {
        playing?.cancel()
        playing = nil
        isPlaying = false
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

    func toggleOnionSkin() {
        change { s in
            if s.onionFrames > 0 {
                onionKept = s.onionFrames
                s.onionFrames = 0
            } else {
                s.onionFrames = onionKept
            }
        }
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

    var body: some View {
        let width = min(screen.width - 32, 1180)
        let rows = min(max(tool.tracks.count, 1), mostRowsShown)
        let bodyHeight = rulerHeight + cameraLaneHeight + CGFloat(rows) * rowHeight
        VStack(alignment: .leading, spacing: 0) {
            if tool.settingsOpen {
                TlPopover(width: 300, arrowX: 150) { TimelineSettingsPage(tool: tool, info: info) }
                    .offset(x: width - 300 - 120)
            } else if tool.keysOpen, let id = tool.picked {
                TlPopover(width: 320, arrowX: 160) { KeysPage(model: model, tool: tool, clip: id, frame: tool.frame) }
                    .offset(x: max(width / 2 - 160, 0))
            } else if tool.clipOpen {
                TlPopover(width: 300, arrowX: 150) { ClipPage(tool: tool, clip: tool.picked.flatMap { tool.clip($0)?.1 }, frame: tool.frame) }
                    .offset(x: max(width / 2 - 150, 0))
            } else if tool.cameraOpen, let camera = tool.camera {
                TlPopover(width: 300, arrowX: 150) { CameraPage(model: model, tool: tool, camera: camera, info: info) }
                    .offset(x: max(width - 300 - 220, 0))
            }
            VStack(spacing: 6) {
                header
                    .padding(.horizontal, 18)
                    .padding(.top, 8)
                ScrollView(.vertical, showsIndicators: false) {
                    HStack(alignment: .top, spacing: 0) {
                        VStack(spacing: 0) {
                            Color.clear.frame(height: rulerHeight)
                            CameraLaneHeader(tool: tool)
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
            .padding(.bottom, barGap - 6)
        }
        .frame(width: width)
    }

    private var header: some View {
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
            Text(verbatim: "\(tool.frame + 1) / \(info.duration)")
                .font(.system(size: 15).monospacedDigit())
                .foregroundStyle(inkDim)
            Spacer(minLength: 0)
            if let gathered = tool.grouping {
                Text(verbatim: gathered.count == 1 ? "1 clip" : "\(gathered.count) clips")
                    .font(.system(size: 15))
                    .foregroundStyle(inkDim)
                BarWord(label: "Cancel") { tool.grouping = nil }
                BarWord(label: "Make Group", bold: true, dim: gathered.isEmpty) { tool.makeGroup() }
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
                BarWord(label: "Camera", bold: tool.cameraOpen) {
                    tool.cameraOpen.toggle()
                    tool.keysOpen = false
                    tool.clipOpen = false
                    tool.settingsOpen = false
                }
                BarWord(label: "Settings") {
                    tool.settingsOpen.toggle()
                    tool.keysOpen = false
                    tool.clipOpen = false
                    tool.cameraOpen = false
                }
                if info.openGroups.isEmpty {
                    BarWord(label: "Simple") { tool.setSimple(true) }
                }
                BarWord(label: "+ Track") { tool.addTrack() }
                BarWord(label: "Add Drawing") { tool.addDrawing() }
            }
        }
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
/// opening or ungrouping it; grouping it with others, splitting, duplicating and deleting it;
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
        VStack(alignment: .leading, spacing: 0) {
            PanelTitle(text: clip.name, bottom: 4)
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
            if group {
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
                .padding(.top, 10)
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
            }
            TlGroup {
                if group {
                    TlAction(label: "Open Group") { tool.open(clip.id) }
                    TlRowLine()
                    TlAction(label: "Ungroup") { tool.ungroup(clip.id) }
                    TlRowLine()
                }
                TlAction(label: "Group With Other Clips…") { tool.startGrouping() }
                if drawing {
                    if inside {
                        TlRowLine()
                        TlAction(label: "Split Here") { tool.split(clip.id) }
                    }
                    TlRowLine()
                    TlAction(label: "Duplicate") { tool.duplicate(clip.id) }
                }
                TlRowLine()
                TlAction(label: "Delete") { tool.delete(clip.id, close: false) }
            }
            .padding(.top, 10)
            TlGroup {
                TlAction(label: "Copy") { tool.copyClip(clip.id) }
                TlRowLine()
                TlAction(label: "Cut") { tool.cutClip(clip.id) }
                if tool.clipboard.clips > 0 {
                    TlRowLine()
                    paste
                }
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
            CheckBox(checked: !track.hidden, fill: ink)
                .frame(width: 26, height: 30)
                .contentShape(Rectangle())
                .onTapGesture { tool.toggleHidden(track) }
        }
        .padding(.leading, 14)
        .padding(.trailing, 8)
        .frame(height: rowHeight)
        .background(chosen ? laneChosen : laneFill)
        .overlay(alignment: .bottom) { rowLineFill.frame(height: 1) }
        .contentShape(Rectangle())
        .onTapGesture { tool.selectTrack(track.id) }
    }
}

/// The ruler and the tracks' clips, scrolling sideways together, with the playhead across them.
/// The ruler is tapped or dragged to move the playhead; a clip is tapped to pick it, dragged to
/// move it (along, or to another track), and the picked one's right edge dragged to hold it longer
/// or shorter.
private struct TracksArea: View {
    let tool: TimelineTool
    let info: TimelineInfo
    /// How wide the area shows: the lanes reach at least this far.
    let shownWidth: CGFloat

    @State private var moving: (clip: UInt64, dx: CGFloat, dy: CGFloat)?
    @State private var trimming: (clip: UInt64, dx: CGFloat)?
    @State private var scrubbing = false

    var body: some View {
        let frames = Int(info.duration)
        let width = max(CGFloat(frames + 8) * framePitch, shownWidth)
        let shown = tool.tracks.reversed() as [TimelineTrackInfo]
        ScrollView(.horizontal, showsIndicators: false) {
            ZStack(alignment: .topLeading) {
                VStack(spacing: 0) {
                    ruler(frames: frames, width: width)
                    CameraLane(tool: tool, width: width, pitch: framePitch, frames: frames)
                    ForEach(Array(shown.enumerated()), id: \.element.id) { row, t in
                        lane(t, row: row, rows: shown, width: width)
                    }
                }
                // the sync markers: a line down through the lanes from each one's flag
                let below = cameraLaneHeight + CGFloat(shown.count) * rowHeight
                TimelineMarkerLines(markers: tool.markers, pitch: framePitch, height: below)
                    .offset(y: rulerHeight)
                TimelineMarkerFlags(tool: tool, frames: frames, pitch: framePitch, height: rulerHeight)
                // the playhead
                let x = (CGFloat(tool.frame) + 0.5) * framePitch
                Pc.accent
                    .frame(width: 2, height: rulerHeight + below)
                    .offset(x: x - 1)
                    .allowsHitTesting(false)
            }
            .frame(width: width, alignment: .topLeading)
        }
        .frame(width: shownWidth)
    }

    private func ruler(frames: Int, width: CGFloat) -> some View {
        let fps = max(Int(info.fps), 1)
        let every = fps >= 12 ? fps / 2 : 5
        return Canvas { ctx, size in
            for f in 0...frames {
                let x = CGFloat(f) * framePitch
                let major = f % every == 0
                ctx.fill(Path(CGRect(x: x, y: major ? 12 : 17, width: 1, height: major ? 12 : 7)), with: .color(inkDim))
                if major, f < frames {
                    ctx.draw(Text(verbatim: "\(f + 1)").font(.system(size: 10).monospacedDigit()).foregroundColor(inkDim),
                             at: CGPoint(x: x + 3, y: 6), anchor: .leading)
                }
            }
            // past the end, shaded
            let end = CGFloat(frames) * framePitch
            ctx.fill(Path(CGRect(x: end, y: 0, width: size.width - end, height: size.height)), with: .color(Color.black.opacity(0.12)))
        }
        .frame(width: width, height: rulerHeight)
        .background(laneFill)
        .contentShape(Rectangle())
        .gesture(
            DragGesture(minimumDistance: 0)
                .onChanged { drag in
                    scrubbing = true
                    tool.scrub(frameAt(drag.location.x))
                }
                .onEnded { drag in
                    scrubbing = false
                    tool.endScrub(frameAt(drag.location.x))
                }
        )
    }

    private func frameAt(_ x: CGFloat) -> UInt32 {
        UInt32(min(max(Int(x / framePitch), 0), Int(info.duration) - 1))
    }

    private func lane(_ t: TimelineTrackInfo, row: Int, rows: [TimelineTrackInfo], width: CGFloat) -> some View {
        ZStack(alignment: .topLeading) {
            (t.id == info.track ? laneChosen : laneFill)
                .frame(width: width, height: rowHeight)
                .contentShape(Rectangle())
                .onTapGesture { location in
                    tool.picked = nil
                    tool.selectTrack(t.id)
                    tool.goTo(frameAt(location.x))
                }
            ForEach(t.clips, id: \.id) { c in
                clipView(c, on: t, row: row, rows: rows)
            }
        }
        .frame(width: width, height: rowHeight, alignment: .topLeading)
        .overlay(alignment: .bottom) { rowLineFill.frame(height: 1) }
        .opacity(t.hidden ? 0.45 : 1)
    }

    private func clipView(_ c: TimelineClipInfo, on t: TimelineTrackInfo, row: Int, rows: [TimelineTrackInfo]) -> some View {
        let picked = tool.picked == c.id || tool.grouping?.contains(c.id) == true
        let trim = trimming?.clip == c.id ? trimming?.dx ?? 0 : 0
        let move = moving?.clip == c.id ? moving : nil
        let w = max(CGFloat(c.duration) * framePitch + trim, framePitch) - 2
        let shape = RoundedRectangle(cornerRadius: 8, style: .circular)
        return ZStack(alignment: .leading) {
            shape.fill(picked ? Pc.accent.opacity(0.85) : clipFill)
            shape.strokeBorder(picked ? Pc.accent : clipEdge, lineWidth: 1)
            if c.kind == .group {
                // a group: an inner edge, as of the clips inside it
                shape.inset(by: 3).strokeBorder(picked ? Color.white.opacity(0.6) : clipEdge, lineWidth: 1)
            }
            HStack(spacing: 6) {
                if c.root != 0, w > 26 {
                    ZStack {
                        Color.white
                        if let picture = tool.thumbnails[c.root] {
                            Image(uiImage: picture).resizable().scaledToFit()
                        }
                    }
                    .frame(width: min(30, w - 10), height: rowHeight - 14)
                    .clipShape(RoundedRectangle(cornerRadius: 4, style: .circular))
                }
                if w > 80 {
                    Text(verbatim: c.name)
                        .font(.system(size: 13))
                        .foregroundStyle(picked ? Color.white : ink)
                        .lineLimit(1)
                    // how it is masked, and a group's cycle
                    let tags = [maskTag(c.mask), c.cycle > 0 ? "↻ \(c.cycle)" : nil].compactMap { $0 }
                    if !tags.isEmpty, w > 150 {
                        Text(verbatim: tags.joined(separator: "  "))
                            .font(.system(size: 11, weight: .semibold))
                            .foregroundStyle(picked ? Color.white.opacity(0.8) : inkDim)
                            .lineLimit(1)
                    }
                }
            }
            .padding(.leading, 5)
            if picked {
                // the edge held to hold it longer or shorter
                Capsule()
                    .fill(Color.white)
                    .frame(width: 5, height: rowHeight - 20)
                    .frame(width: 22, height: rowHeight - 6)
                    .contentShape(Rectangle())
                    .frame(maxWidth: .infinity, alignment: .trailing)
                    .highPriorityGesture(
                        DragGesture(minimumDistance: 2)
                            .onChanged { trimming = (c.id, $0.translation.width) }
                            .onEnded { drag in
                                trimming = nil
                                let frames = Int(c.duration) + Int((drag.translation.width / framePitch).rounded())
                                tool.setDuration(c.id, frames, push: false)
                            }
                    )
            }
        }
        .frame(width: w, height: rowHeight - 6)
        // its keys, a diamond on each keyed frame along the bottom
        .overlay(alignment: .bottomLeading) {
            let frames = Set((tool.keys[c.id] ?? []).map(\.frame)).sorted()
            ZStack(alignment: .bottomLeading) {
                ForEach(frames, id: \.self) { f in
                    Rectangle()
                        .fill(picked ? Color.white : Pc.accent)
                        .frame(width: 7, height: 7)
                        .rotationEffect(.degrees(45))
                        .offset(x: (CGFloat(Int(f) - Int(c.start)) + 0.5) * framePitch - 4.5, y: -3)
                }
            }
            .allowsHitTesting(false)
        }
        .offset(x: CGFloat(c.start) * framePitch + 1 + (move?.dx ?? 0), y: 3 + (move?.dy ?? 0))
        .zIndex(move != nil ? 1 : 0)
        .shadow(color: move != nil ? .black.opacity(0.35) : .clear, radius: 8)
        .onTapGesture { tool.pick(c, on: t) }
        // a group opens with a double tap
        .simultaneousGesture(TapGesture(count: 2).onEnded { if c.kind == .group, tool.grouping == nil { tool.open(c.id) } })
        .gesture(
            DragGesture(minimumDistance: 8)
                .onChanged { moving = (c.id, $0.translation.width, $0.translation.height) }
                .onEnded { drag in
                    moving = nil
                    let start = Int(c.start) + Int((drag.translation.width / framePitch).rounded())
                    let to = min(max(row + Int((drag.translation.height / rowHeight).rounded()), 0), rows.count - 1)
                    tool.move(c.id, to: rows[to].id, start: start)
                }
        )
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
        tool.moveKey(id, at: d.frame, by: CGPoint(x: at.x - d.start.x, y: at.y - d.start.y), from: pose, done: done)
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
                    TlAction(label: "Remove Keys Here") {
                        live = nil
                        tool.removeKeys(clip)
                    }
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
    }
}

private struct TlToggle: View {
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

/// The timeline's settings: playback, frames a second, length, how long new drawings are held,
/// onion skins; and taking the artwork off the timeline.
private struct TimelineSettingsPage: View {
    let tool: TimelineTool
    let info: TimelineInfo

    /// A new frame rate keeps the animation's timing (every clip and key moved to match) rather
    /// than its frames; the rate as its slider moves, before it is let go.
    @AppStorage("timelineKeepTiming") private var keepTiming = true
    @State private var fps: UInt32?

    var body: some View {
        let ways: [(AnimationPlayback, String)] = [(.loop, "Loop"), (.pingPong, "Ping-Pong"), (.oneShot, "One Shot")]
        let onion = Int(info.onionFrames)
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
                TlSlider(
                    label: "Onion skin drawings", value: onion == 0 ? "None" : onion == mostOnion ? "Max" : "\(onion)",
                    share: Float(onion) / Float(mostOnion)
                ) { v in
                    tool.change { $0.onionFrames = UInt32(wholeNumber(v * Float(mostOnion))) }
                }
                TlSlider(label: "Onion skin opacity", value: "\(wholeNumber(info.onionOpacity * 100)) %", share: info.onionOpacity) { v in
                    tool.change { $0.onionOpacity = v }
                }
            }
            .padding(.top, 10)
            TlGroup {
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

private struct BarWord: View {
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
private struct TlPopover<Content: View>: View {
    let width: CGFloat
    let arrowX: CGFloat
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content()
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
        .padding(.bottom, 2)
        .contentShape(Rectangle())
        .onTapGesture {}
    }
}

private struct PanelTitle: View {
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

private struct TlRowLine: View {
    var body: some View {
        rowLineFill.frame(height: 2)
    }
}

private struct TlGroup<Rows: View>: View {
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

private struct TlSliderGroup<Rows: View>: View {
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
private struct TlChoice: View {
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

private struct TlAction: View {
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

/// A setting's name and value over a bar dragged or tapped along.
private struct TlSlider: View {
    let label: String
    let value: String
    let share: Float
    var onEnd: () -> Void = {}
    let onChange: (Float) -> Void

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
                        .onChanged { onChange(Float(min(max($0.location.x / max(w, 1), 0), 1))) }
                        .onEnded { _ in onEnd() }
                )
            }
            .frame(height: 22)
        }
        .padding(.horizontal, 16)
        .frame(height: 55, alignment: .top)
    }
}

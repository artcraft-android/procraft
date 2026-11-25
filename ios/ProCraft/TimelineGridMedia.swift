import AVFoundation
import SwiftUI
import UIKit

// Sounds and videos on the grid timeline (see TimelineGrid.swift). A sound is a dark block with
// its waveform down the middle as bars; a video its own pictures laid side by side along it, its
// sound's waveform (while it still has one of its own) over their lower part. Both follow the
// zoom: the bars keep their width and stand for more or less time, and there are more or fewer
// pictures. And the menu of a block that isn't a drawing: a sound's, a video's, a group's.

/// A video's pictures for its block, small, read as they are wanted and kept: a block asks for
/// the picture at a time and draws it once it is there.
///
/// They are read one at a time, the one asked for last first, and only so many wait: the grid
/// slid quickly along a long video asks for hundreds it has passed by the time they could be
/// read, and each takes the decoder as long as a picture for the canvas does. One that waited too
/// long is dropped, and asked for again if its block shows it again.
@Observable
final class GridVideoPictures {
    static let shared = GridVideoPictures()

    private struct Key: Hashable {
        let path: String
        /// (hundredths of a second into the file)
        let at: Int
    }

    /// How many are kept: past this, those read longest ago are let go, to be read again if shown.
    private static let most = 600
    /// How many wait to be read at most.
    private static let waitingMost = 40

    private var kept: [Key: UIImage] = [:]
    /// How many have been read: a block showing them is drawn again as it changes.
    private(set) var read = 0
    /// Those kept, the one read longest ago first.
    @ObservationIgnored private var order: [Key] = []
    @ObservationIgnored private var asked: Set<Key> = []
    /// Those waiting to be read, the one asked for last at the end, each with how far from its
    /// time the picture may be (seconds).
    @ObservationIgnored private var waiting: [(key: Key, within: Double)] = []
    @ObservationIgnored private var reading = false
    @ObservationIgnored private var readers: [String: AVAssetImageGenerator] = [:]

    /// The picture `seconds` into the video at `path`, if it has been read; asked for, else. One
    /// from up to `within` seconds either side will do (half as long as a picture stands for on
    /// the block, zoomed out): the wider that is, the sooner the decoder finds one.
    func picture(_ path: String, at seconds: Double, within: Double) -> UIImage? {
        let key = Key(path: path, at: Int((max(seconds, 0) * 100).rounded()))
        if let got = kept[key] {
            return got
        }
        guard asked.insert(key).inserted else { return nil }
        waiting.append((key, within))
        if waiting.count > Self.waitingMost {
            asked.remove(waiting.removeFirst().key)
        }
        readNext()
        return nil
    }

    private func readNext() {
        guard !reading, let (key, within) = waiting.popLast() else { return }
        reading = true
        let reader = reader(key.path)
        Task.detached(priority: .utility) { [weak self] in
            let near = CMTime(seconds: min(max(within, 0.02), 30), preferredTimescale: 600)
            reader.requestedTimeToleranceBefore = near
            reader.requestedTimeToleranceAfter = near
            let time = CMTime(seconds: Double(key.at) / 100, preferredTimescale: 600)
            let image = try? await reader.image(at: time).image
            await MainActor.run {
                guard let self else { return }
                self.reading = false
                if let image {
                    self.kept[key] = UIImage(cgImage: image)
                    self.order.append(key)
                    if self.order.count > Self.most {
                        for old in self.order.prefix(Self.most / 4) {
                            self.kept[old] = nil
                            self.asked.remove(old)
                        }
                        self.order.removeFirst(Self.most / 4)
                    }
                    self.read += 1
                }
                self.readNext()
            }
        }
    }

    private func reader(_ path: String) -> AVAssetImageGenerator {
        if let r = readers[path] {
            return r
        }
        let r = AVAssetImageGenerator(asset: AVURLAsset(url: URL(fileURLWithPath: path)))
        r.appliesPreferredTrackTransform = true
        r.maximumSize = CGSize(width: 220, height: 220)
        readers[path] = r
        return r
    }
}

extension GridState {
    /// Sound or video clip `id` as its block shows it, read once for each state of the timeline
    /// (a block is drawn again with every frame the grid slides).
    func media(_ id: UInt64, tool: TimelineTool) -> TimelineMediaClip? {
        if mediaRead.revision != tool.revision {
            mediaRead = (tool.revision, [:])
        }
        if let got = mediaRead.clips[id] {
            return got
        }
        let read = tool.media.clip(id)
        if let read {
            mediaRead.clips[id] = read
        }
        return read
    }
}

/// A sound or a video over the frames it lasts.
struct GridMediaBlock: View {
    let tool: TimelineTool
    let grid: GridState
    let clip: TimelineClipInfo
    let fps: UInt32
    let sizes: GridSizes
    /// Its colour label, if it has one.
    let tint: Color?
    /// The frames of it drawn, counted from its first: those on the bar.
    let from: Int
    let to: Int

    /// From one waveform bar to the next, and a bar's own width.
    private static let step: CGFloat = 3.5
    private static let bar: CGFloat = 2

    var body: some View {
        let info = grid.media(clip.id, tool: tool)
        let video = clip.kind == .video
        // a video's waveform is its own sound's, while it has one
        let wave = info.map { video ? $0.sound : $0.media } ?? 0
        let peaks = wave != 0 ? tool.media.peaks(wave) : []
        let pictures = GridVideoPictures.shared
        let pitch = sizes.pitch
        let perSecond = CGFloat(max(fps, 1)) * pitch
        let start = info?.trimStart ?? 0
        // (drawn again as its pictures come in)
        let _ = pictures.read
        Canvas { ctx, size in
            let x0 = CGFloat(from) * pitch
            let x1 = min(CGFloat(to + 1) * pitch, size.width)
            ctx.fill(Path(CGRect(origin: .zero, size: size)), with: .color(tint ?? (video ? Grid.videoGround : Grid.soundGround)))
            guard x1 > x0 else { return }
            if video, let info, let path = info.path, info.width > 0, info.height > 0 {
                // its pictures side by side, each from the time it stands at: as wide as the
                // picture is for its height, to the nearest doubling of how long that lasts, so
                // the same pictures serve as the grid is zoomed a little
                let wide = size.height * CGFloat(info.width) / CGFloat(info.height)
                let lasts = Double(pow(2, (log2(max(wide / perSecond, 0.001))).rounded()))
                let tile = CGFloat(lasts) * perSecond
                var k = Int(((start + Double(x0 / perSecond)) / lasts).rounded(.down))
                while true {
                    let at = Double(k) * lasts
                    let x = CGFloat(at - start) * perSecond
                    if x >= x1 {
                        break
                    }
                    // (no nearer than they ever were asked for, a fifth of a second: nearer
                    // takes the decoder longer and can't be told apart at this size)
                    if let picture = pictures.picture(path, at: at, within: max(lasts / 2, 0.2)) {
                        let image = ctx.resolve(Image(uiImage: picture))
                        // (filling its tile: what doesn't fit is cut off)
                        let scale = max(tile / max(image.size.width, 1), size.height / max(image.size.height, 1))
                        let (w, h) = (image.size.width * scale, image.size.height * scale)
                        var part = ctx
                        part.clip(to: Path(CGRect(x: x, y: 0, width: tile, height: size.height)))
                        part.draw(image, in: CGRect(x: x + (tile - w) / 2, y: (size.height - h) / 2, width: w, height: h))
                    }
                    k += 1
                }
                // a film's edges
                var holes = Path()
                var x = (x0 / 6).rounded(.down) * 6 + 3
                while x < x1 {
                    holes.addEllipse(in: CGRect(x: x - 0.8, y: 1.6, width: 1.6, height: 1.6))
                    holes.addEllipse(in: CGRect(x: x - 0.8, y: size.height - 3.2, width: 1.6, height: 1.6))
                    x += 6
                }
                ctx.fill(holes, with: .color(Color.black.opacity(0.55)))
            }
            guard !peaks.isEmpty else { return }
            // the waveform: a bar for each few points, as tall as the loudest of the sound it
            // stands for. A sound's down the middle; a video's along its lower part
            let rate = Double(mediaPeaksPerSecond())
            let tall = video ? size.height * 0.42 : size.height - 8
            let mid = video ? size.height - 3 - tall / 2 : size.height / 2
            var bars = Path()
            var x = (x0 / Self.step).rounded(.down) * Self.step
            while x < x1 {
                let a = (start + Double(x / perSecond)) * rate
                let b = (start + Double((x + Self.step) / perSecond)) * rate
                let i = max(Int(a), 0)
                let j = min(max(Int(b), i + 1), peaks.count)
                if i < j {
                    var loudest: Float = 0
                    for n in stride(from: i, to: j, by: max((j - i) / 48, 1)) {
                        loudest = max(loudest, peaks[n])
                    }
                    let h = max(CGFloat(pow(Double(loudest), 0.7)) * tall, 1.5)
                    bars.addRoundedRect(
                        in: CGRect(x: x, y: mid - h / 2, width: Self.bar, height: h),
                        cornerSize: CGSize(width: 1, height: 1)
                    )
                }
                x += Self.step
            }
            let muted = info?.muted == true
            ctx.fill(bars, with: .color(video ? Color.white.opacity(muted ? 0.4 : 0.92) : Grid.inkBright.opacity(muted ? 0.3 : 0.72)))
        }
        .overlay(alignment: .topLeading) {
            // its name, no further along than its first frame on the bar
            if sizes.rowScale > 0.7 {
                Text(verbatim: clip.name)
                    .font(.system(size: 9.5, weight: .semibold))
                    .foregroundStyle(video ? Color.white : Grid.inkBright)
                    .shadow(color: .black.opacity(video ? 0.8 : 0), radius: 1.5)
                    .lineLimit(1)
                    .padding(.leading, 6)
                    .padding(.top, 3)
                    .offset(x: CGFloat(from) * pitch)
            }
        }
    }
}

// MARK: - A block's menu

/// The menu of a block that isn't a drawing (a sound, a video, a group): cut, copy, paste and
/// duplicate; its own opacity or level, name, highlight colour, blend mode and mask; what only
/// its kind has; splitting it, a copy after it, lasting through the empty frames after it; its
/// track's options; deleting it.
struct GridBlockMenu: View {
    private enum Beside {
        case highlight, blend, mask, size, slip, cycle, keys, options
    }

    let tool: TimelineTool
    let grid: GridState
    let track: TimelineTrackInfo
    let clip: TimelineClipInfo
    let room: CGFloat
    let close: () -> Void

    @State private var beside: Beside?

    var body: some View {
        let _ = tool.revision
        let locked = tool.held(track)
        let media = clip.kind == .group ? nil : grid.media(clip.id, tool: tool)
        let sound = clip.kind == .audio
        let video = clip.kind == .video
        let group = clip.kind == .group
        // (as tall as there is room for: the rest scrolls)
        let moves = (tool.place(track.id)?.group ?? 0) != 0
        let rows: CGFloat = (sound ? 11 : video ? 15 : 14) + (moves ? 1 : 0)
        let tall = 46 + 46 + rows * 36 + 72
        HStack(alignment: .top, spacing: 6) {
            GridCard(width: 252) {
                ScrollView(.vertical, showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 0) {
                        GridMenuTools(
                            cut: locked ? nil : {
                                close()
                                tool.cutFrames([clip.id])
                            },
                            copy: {
                                close()
                                tool.copyFrames([clip.id])
                            },
                            paste: tool.clipboard.clips == 0 || locked ? nil : {
                                close()
                                tool.paste(at: clip, on: track)
                            },
                            duplicate: locked || group ? nil : {
                                close()
                                tool.duplicateFrames([clip.id])
                            }
                        )
                        GridMenuLine()
                        if sound, let media {
                            // (its level over time, once it has a key: the slider keys it at the playhead)
                            let keyed = !media.volumeKeys.isEmpty
                            GridMenuSlider(label: keyed ? "Level here" : "Level", value: keyed ? media.level : media.volume) { v, done in
                                if keyed {
                                    tool.media.setLevel(media, v, done: done)
                                } else {
                                    tool.media.setSound(media, volume: v, done: done)
                                }
                            }
                            row(PcIcons.gridKey, media.keyedHere ? "Remove level key" : "Key level here", dim: locked || !within) {
                                if media.keyedHere {
                                    tool.media.removeLevelKey(media)
                                } else {
                                    tool.media.setLevel(media, media.level, key: true, done: true)
                                }
                            }
                            row(PcIcons.gridMask, media.muted ? "Unmute" : "Mute", dim: locked) { tool.media.setSound(media, muted: !media.muted, done: true) }
                        } else {
                            GridMenuSlider(label: "Opacity", value: clip.opacity) { v, done in tool.setClipOpacity(clip.id, v, done: done) }
                        }
                        GridRenameRow(name: clip.name) { tool.renameClip(clip.id, $0) }
                        opens(PcIcons.gridHighlight, "Highlight", .highlight)
                        if !sound {
                            opens(PcIcons.gridBlend, "Blend mode", .blend)
                            opens(PcIcons.gridMask, "Mask", .mask)
                        }
                        GridMenuLine()
                        if video, let media {
                            row(PcIcons.gridOptions, "Separate audio and video", dim: locked || media.sound == 0) {
                                close()
                                tool.separateAudio(clip.id)
                            }
                            // the animatic detector, while Timeline media's switch for it is on
                            if PluginsModel.shared.has(.animaticDetector) {
                                row(PcIcons.gridInsert, "Make timing track…", dim: false) {
                                    close()
                                    tool.media.detecting = clip.id
                                }
                            }
                            opens(PcIcons.gridBlend, "Size", .size)
                        }
                        if sound || video {
                            opens(PcIcons.gridFlip, "Slip", .slip)
                        }
                        if group {
                            let open = tool.unfolded(clip.id)
                            row(PcIcons.layers, open ? "Collapse group" : "Expand group", dim: false) { tool.fold(clip.id, open) }
                            opens(PcIcons.gridFlip, "Cycle", .cycle)
                            row(PcIcons.gridClear, "Ungroup", dim: locked) {
                                close()
                                tool.ungroup(clip.id)
                            }
                        }
                        // a group or a video moves, scales, turns and fades by keys as a frame
                        // does (a sound's level is keyed above)
                        if !sound {
                            GridMenuRow(icon: PcIcons.gridKey, label: "Keys", chevron: true, lit: beside == .keys) {
                                beside = beside == .keys ? nil : .keys
                                // (its keys go on the playhead's frame: the playhead comes to it)
                                if beside == .keys {
                                    tool.bringPlayhead(onto: clip)
                                }
                            }
                        }
                        GridMenuLine()
                        // inside a group: out onto the level the group is on
                        if moves {
                            row(PcIcons.layers, "Move out of group", dim: locked) {
                                close()
                                tool.moveOutOfGroup(clip.id)
                            }
                        }
                        // turned off, it stays where it is but neither shows nor plays
                        row(PcIcons.gridHighlight, clip.disabled ? "Turn on" : "Turn off", dim: locked) {
                            tool.setClipDisabled(clip.id, !clip.disabled)
                        }
                        // (a repeating group isn't cut)
                        row(PcIcons.gridScissors, "Split content", dim: locked || !inside || clip.cycle > 0) {
                            close()
                            tool.goTo(tool.frame)
                            tool.split(clip.id)
                        }
                        row(PcIcons.duplicate, "Duplicate after", dim: locked || group) {
                            close()
                            tool.duplicateFrames([clip.id])
                        }
                        row(PcIcons.gridFill, "Fill duration", dim: !tool.canExtend([clip.id])) {
                            close()
                            tool.extend([clip.id])
                        }
                        opens(PcIcons.gridOptions, "Track options", .options)
                        GridMenuLine()
                        row(PcIcons.trash, "Delete content", tint: Grid.danger, dim: locked) {
                            close()
                            tool.deleteFrames([clip.id])
                        }
                    }
                }
                .frame(height: min(tall, max(room - 40, 200)))
            }
            besideCard(media)
        }
    }

    /// Whether the playhead is on one of the block's frames.
    private var within: Bool {
        tool.frame >= clip.start && tool.frame < clip.start + clip.duration
    }

    /// Whether the playhead is inside the block (past its first frame), where it can be split.
    private var inside: Bool {
        tool.frame > clip.start && tool.frame < clip.start + clip.duration
    }

    private func row(_ icon: PcIcon, _ label: String, tint: Color? = nil, dim: Bool, action: @escaping () -> Void) -> some View {
        GridMenuRow(icon: icon, label: label, tint: tint, dim: dim, action: action)
    }

    private func opens(_ icon: PcIcon, _ label: String, _ what: Beside) -> some View {
        GridMenuRow(icon: icon, label: label, chevron: true, lit: beside == what) { beside = beside == what ? nil : what }
    }

    @ViewBuilder
    private func besideCard(_ media: TimelineMediaClip?) -> some View {
        switch beside {
        case .highlight:
            GridCard {
                ForEach(gridHighlights, id: \.1) { name, rgb in
                    GridChoiceRow(label: name, swatch: Color(rgb: rgb), chosen: clip.label == rgb) { tool.setClipLabel(clip.id, rgb) }
                }
                GridChoiceRow(label: "None", swatch: .some(nil), chosen: clip.label == nil) { tool.setClipLabel(clip.id, nil) }
            }
        case .blend:
            let modes = tool.canvasModel.blendModeList
            GridCard {
                ScrollView(.vertical, showsIndicators: false) {
                    VStack(alignment: .leading, spacing: 0) {
                        ForEach(modes.indices, id: \.self) { i in
                            GridChoiceRow(label: modes[i].name, chosen: clip.blend == modes[i].id) { tool.setClipBlend(clip.id, modes[i].id) }
                        }
                    }
                }
                .frame(height: min(CGFloat(modes.count) * 34, min(max(room - 60, 170), 306)))
            }
        case .mask:
            let masks: [(TimelineMask, String)] = [
                (.none, "None"), (.clipping, "Clipping mask"), (.alpha, "Alpha mask"), (.luminance, "Luminance mask"),
            ]
            GridCard {
                ForEach(masks, id: \.1) { mask, name in
                    GridChoiceRow(label: name, chosen: clip.mask == mask) { tool.setMask(clip.id, mask) }
                }
            }
        case .size:
            if let media {
                let canvas = tool.canvasModel.session.info()
                GridCard {
                    ForEach(Placement.allCases, id: \.self) { p in
                        let rect = p.rect(media, canvas: (Float(canvas.width), Float(canvas.height)))
                        let chosen = rect.map { r in zip(r, media.rect).allSatisfy { abs($0 - $1) < 0.5 } } ?? false
                        GridChoiceRow(label: p.label, chosen: chosen) {
                            if let rect {
                                tool.media.placeVideo(clip.id, rect, done: true)
                            }
                        }
                    }
                }
            }
        case .slip:
            if let media {
                let fps = Double(max(tool.info?.fps ?? 12, 1))
                GridCard(width: 300) {
                    Text(verbatim: "Drag to play another part of the file")
                        .font(.system(size: 13))
                        .foregroundStyle(Grid.ink)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 6)
                    SlipStrip(media: tool.media, c: media, plays: Double(clip.duration) / fps)
                        .padding(.horizontal, 8)
                        .padding(.bottom, 6)
                    // a sound's start a part of a frame later, for sync finer than a frame
                    if media.kind == .audio {
                        GridMenuSlider(
                            label: "Nudge", value: Float(min(max(media.offset * fps, 0), 1)),
                            text: { "\(Int((Double($0) / fps * 1000).rounded())) ms" }
                        ) { v, done in
                            tool.media.setSound(media, offset: Double(v) / fps, done: done)
                        }
                        .padding(.bottom, 4)
                    }
                }
            }
        case .cycle:
            GridCycleCard(tool: tool, clip: clip)
        case .keys:
            GridKeysCard(tool: tool, clip: clip, close: close)
        case .options:
            GridTrackMenu(tool: tool, track: track, room: room, close: close)
        case nil:
            EmptyView()
        }
    }
}

/// A group's cycle: playing its frames over and over, how many times, and whether its own keys
/// run on across the repeats.
struct GridCycleCard: View {
    let tool: TimelineTool
    let clip: TimelineClipInfo

    var body: some View {
        let repeats = clip.cycle > 0
        GridCard(width: 262) {
            GridToggleRow(label: "Repeat", on: repeats) {
                // what it lasts now is what is played again
                tool.setCycle(clip.id, length: repeats ? 0 : clip.duration, continueAcross: clip.continueAcross)
            }
            if repeats, let n = tool.cycles(clip.id) {
                GridDurationRow(label: "Repeats", frames: n.count, locked: false) { tool.setCycles(clip.id, min($0, max(n.most, 1))) }
                GridToggleRow(label: "Keys run on across repeats", on: clip.continueAcross) {
                    tool.setCycle(clip.id, length: clip.cycle, continueAcross: !clip.continueAcross)
                }
            }
        }
    }
}

extension TimelineTool {
    /// Video `id`'s sound taken out as a sound block of its own on a new track above it: one undo
    /// step.
    func separateAudio(_ id: UInt64) {
        stopPlaying()
        if canvasModel.session.separateTimelineAudio(id: id) == 0 {
            canvasModel.showNotice("It has no sound of its own, or its track is locked.", warning: true)
        }
        canvasModel.refresh()
    }
}

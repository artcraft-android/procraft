import SwiftUI
import UniformTypeIdentifiers

// Sounds and videos in the timeline panel: what a sound or video clip shows on its lane (its
// waveform, or a strip of film), its own page (level, nudge, the part of the file it plays,
// where a video sits on the canvas, the animatic detector), and "+ Media" in the header.

private var ink: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }
private var inkDim: Color { Look.shared.light ? Color(argb: 0x8039_3939) : Color(argb: 0x80C5_C5C5) }
/// A sound clip's colour, and a video clip's: cool and warm, apart from the drawings' grey.
private var soundFill: Color { Look.shared.light ? Color(argb: 0xFF9F_C6C4) : Color(argb: 0xFF2F_5755) }
private var soundWave: Color { Look.shared.light ? Color(argb: 0xFF2F_6E6A) : Color(argb: 0xFF8F_D6CF) }
private var filmFill: Color { Look.shared.light ? Color(argb: 0xFFC9_B79C) : Color(argb: 0xFF5A_4A33) }
private var filmHoles: Color { Look.shared.light ? Color(argb: 0x66FF_FFFF) : Color(argb: 0x55FF_FFFF) }

/// How long `seconds` is, as a clock: 0:02.40.
func mediaClock(_ seconds: Double) -> String {
    let s = max(seconds, 0)
    let minutes = Int(s / 60)
    let rest = s - Double(minutes * 60)
    return String(format: "%d:%05.2f", minutes, rest)
}

/// A sound or video clip's face on its lane: the waveform of the part of the sound it plays, or
/// film with its sprocket holes. Picked, its left edge is held to trim where it starts in its file.
struct MediaClipFace: View {
    let media: TimelineMediaTool
    let clip: TimelineClipInfo
    let picked: Bool
    let fps: UInt32
    /// A frame's width on the lane.
    let pitch: CGFloat
    let height: CGFloat

    /// Where it started when its start edge's drag began (the core moves it as the drag goes).
    @State private var trimFrom: UInt32?

    var body: some View {
        let c = media.clip(clip.id)
        let shape = RoundedRectangle(cornerRadius: 8, style: .circular)
        ZStack(alignment: .leading) {
            if clip.kind == .audio {
                shape.fill(picked ? Pc.accent.opacity(0.85) : soundFill)
                if let c {
                    Waveform(
                        peaks: media.peaks(c.media), from: c.trimStart,
                        seconds: Double(clip.duration) / Double(max(fps, 1)) - c.offset,
                        lead: c.offset * Double(max(fps, 1)) * pitch
                    )
                    .fill(picked ? Color.white.opacity(0.85) : soundWave.opacity(c.muted ? 0.35 : 1))
                    .padding(.vertical, 5)
                }
            } else {
                shape.fill(picked ? Pc.accent.opacity(0.85) : filmFill)
                Sprockets().fill(picked ? Color.white.opacity(0.45) : filmHoles)
            }
            HStack(spacing: 5) {
                Text(verbatim: clip.kind == .audio ? (c?.muted == true ? "🔇" : "♪") : "▶︎")
                    .font(.system(size: 11, weight: .bold))
                Text(verbatim: clip.name)
                    .font(.system(size: 12, weight: .semibold))
                    .lineLimit(1)
            }
            .foregroundStyle(picked ? Color.white : ink)
            .padding(.horizontal, 6)
            .padding(.vertical, 1)
            .background(Capsule().fill((picked ? Pc.accent : Color.black).opacity(0.25)))
            .padding(.leading, 6)
            .frame(maxHeight: .infinity, alignment: .top)
            .padding(.top, 3)
            if picked {
                // the start, held to trim into the file or back out of it
                Capsule()
                    .fill(Color.white)
                    .frame(width: 5, height: height - 14)
                    .frame(width: 22, height: height)
                    .contentShape(Rectangle())
                    .highPriorityGesture(
                        DragGesture(minimumDistance: 2, coordinateSpace: .global)
                            .onChanged { drag in
                                let from = trimFrom ?? clip.start
                                trimFrom = from
                                media.trimStart(clip.id, to: start(from, drag.translation.width), done: false)
                            }
                            .onEnded { drag in
                                let from = trimFrom ?? clip.start
                                trimFrom = nil
                                media.trimStart(clip.id, to: start(from, drag.translation.width), done: true)
                            }
                    )
                    .offset(x: -8)
            }
        }
        .clipShape(shape)
    }

    /// The first frame the start edge asks for, dragged `dx` from `from`.
    private func start(_ from: UInt32, _ dx: CGFloat) -> UInt32 {
        UInt32(max(Int(from) + Int((dx / pitch).rounded()), 0))
    }
}

/// A sound's waveform from `from` seconds into it, for `seconds`, mirrored about the middle;
/// starting `lead` points in (a nudged sound).
struct Waveform: Shape {
    let peaks: [Float]
    let from: Double
    let seconds: Double
    var lead: CGFloat = 0

    func path(in rect: CGRect) -> Path {
        var p = Path()
        let perSecond = Double(mediaPeaksPerSecond())
        guard !peaks.isEmpty, seconds > 0, rect.width > lead else { return p }
        let mid = rect.midY
        let width = rect.width - lead
        // a bar every three points, each the loudest peak it covers
        let bars = max(Int(width / 3), 1)
        for b in 0 ..< bars {
            let t0 = from + seconds * Double(b) / Double(bars)
            let t1 = from + seconds * Double(b + 1) / Double(bars)
            let i0 = max(Int(t0 * perSecond), 0)
            let i1 = min(max(Int(t1 * perSecond), i0 + 1), peaks.count)
            guard i0 < i1 else { continue }
            let loud = CGFloat(peaks[i0 ..< i1].max() ?? 0)
            let h = max(loud * rect.height / 2, 0.5)
            let x = rect.minX + lead + CGFloat(b) * width / CGFloat(bars)
            p.addRoundedRect(in: CGRect(x: x, y: mid - h, width: 2, height: h * 2), cornerSize: CGSize(width: 1, height: 1))
        }
        return p
    }
}

/// Film's sprocket holes along the top and bottom of a video clip.
private struct Sprockets: Shape {
    func path(in rect: CGRect) -> Path {
        var p = Path()
        var x = rect.minX + 4
        while x < rect.maxX - 4 {
            for y in [rect.minY + 2, rect.maxY - 6] {
                p.addRoundedRect(in: CGRect(x: x, y: y, width: 5, height: 4), cornerSize: CGSize(width: 1, height: 1))
            }
            x += 10
        }
        return p
    }
}

/// A sound or video clip's page: the part of its file it plays (dragged along to slip it), a
/// sound's level, mute and nudge, a video's place on the canvas and the animatic detector.
struct MediaClipPage: View {
    let model: CanvasModel
    let clip: TimelineClipInfo
    let fps: UInt32

    @State private var volume: Float?
    @State private var nudge: Double?

    var body: some View {
        let media = model.media
        if let c = media.clip(clip.id) {
            let fps = Double(max(fps, 1))
            let plays = Double(clip.duration) / fps
            VStack(alignment: .leading, spacing: 0) {
                PanelTitle(text: clip.name, bottom: 2)
                Text(verbatim: subtitle(c, plays: plays))
                    .font(.system(size: 13).monospacedDigit())
                    .foregroundStyle(Pc.slideLabel)
                    .padding(.leading, 14)
                    .padding(.bottom, 10)
                SlipStrip(media: media, c: c, plays: plays)
                    .padding(.bottom, 10)
                if c.kind == .audio {
                    let keyed = !c.volumeKeys.isEmpty
                    let level = volume ?? (keyed ? c.level : c.volume)
                    let ahead = nudge ?? c.offset
                    TlSliderGroup {
                        TlSlider(
                            label: keyed ? "Level Here" : "Level", value: "\(Int((level * 100).rounded()))%", share: level,
                            onEnd: {
                                if let v = volume {
                                    media.setLevel(c, v, done: true)
                                }
                                volume = nil
                            }
                        ) { v in
                            volume = v
                            media.setLevel(c, v, done: false)
                        }
                        TlSlider(
                            label: "Nudge", value: "+\(Int((ahead * 1000).rounded())) ms",
                            share: Float(ahead * fps),
                            onEnd: {
                                if let n = nudge {
                                    media.setSound(c, offset: n, done: true)
                                }
                                nudge = nil
                            }
                        ) { v in
                            nudge = Double(v) / fps
                            media.setSound(c, offset: Double(v) / fps, done: false)
                        }
                    }
                    TlGroup {
                        HStack(spacing: 0) {
                            Text(verbatim: "Mute")
                                .font(.system(size: 17))
                                .foregroundStyle(Pc.panelTitle)
                            Spacer(minLength: 0)
                            CheckBox(checked: c.muted, fill: Pc.accent)
                        }
                        .padding(.horizontal, 16)
                        .frame(height: 44)
                        .contentShape(Rectangle())
                        .onTapGesture { media.setSound(c, muted: !c.muted, done: true) }
                    }
                    .padding(.top, 10)
                    TlGroup {
                        if c.keyedHere {
                            TlAction(label: "Remove Level Key") { media.removeLevelKey(c) }
                        } else {
                            TlAction(label: keyed ? "Key Level Here" : "Key Level Over Time") { media.setLevel(c, c.level, key: true, done: true) }
                        }
                    }
                    .padding(.top, 10)
                } else {
                    let canvas = model.session.info()
                    TlGroup {
                        ForEach(Placement.allCases, id: \.self) { place in
                            if place != .fit {
                                TlRowLine()
                            }
                            let rect = place.rect(c, canvas: (Float(canvas.width), Float(canvas.height)))
                            TlChoice(label: place.label, chosen: rect.map { near($0, c.rect) } ?? false) {
                                if let rect {
                                    media.placeVideo(c.clip, rect, done: true)
                                }
                            }
                        }
                    }
                    TlGroup {
                        TlAction(label: "Make Timing Track…") { media.detecting = c.clip }
                    }
                    .padding(.top, 10)
                }
                let frame = model.timeline.info?.frame ?? 0
                let inside = frame > clip.start && frame < clip.start + clip.duration
                TlGroup {
                    TlAction(label: "Play From the File's Start") { media.slip(c.clip, to: 0, done: true) }
                    if inside {
                        TlRowLine()
                        TlAction(label: "Split Here") { model.timeline.split(clip.id) }
                    }
                    TlRowLine()
                    TlAction(label: "Duplicate") { model.timeline.duplicate(clip.id) }
                    if model.timeline.info?.openGroups.isEmpty == false {
                        TlRowLine()
                        TlAction(label: "Move Out of Group") { model.timeline.moveOutOfGroup(clip.id) }
                    }
                    TlRowLine()
                    TlAction(label: "Delete") { model.timeline.delete(clip.id, close: false) }
                }
                .padding(.top, 10)
            }
        }
    }

    private func subtitle(_ c: TimelineMediaClip, plays: Double) -> String {
        let what = c.kind == .audio ? "Sound" : "Video \(c.width)×\(c.height)"
        let missing = c.path == nil ? " · file missing" : ""
        return "\(what) · plays \(mediaClock(plays)) of \(mediaClock(c.mediaSeconds))\(missing)"
    }

    private func near(_ a: [Float], _ b: [Float]) -> Bool {
        a.count == b.count && zip(a, b).allSatisfy { abs($0 - $1) < 0.5 }
    }
}

/// Where a video sits on the canvas.
private enum Placement: CaseIterable {
    case fit, fill, actual

    var label: String {
        switch self {
        case .fit: "Fit Canvas"
        case .fill: "Fill Canvas"
        case .actual: "Actual Size"
        }
    }

    func rect(_ c: TimelineMediaClip, canvas: (Float, Float)) -> [Float]? {
        let (w, h) = (Float(c.width), Float(c.height))
        guard w > 0, h > 0 else { return nil }
        let scale: Float = switch self {
        case .fit: min(canvas.0 / w, canvas.1 / h)
        case .fill: max(canvas.0 / w, canvas.1 / h)
        case .actual: 1
        }
        let (sw, sh) = (w * scale, h * scale)
        return [(canvas.0 - sw) / 2, (canvas.1 - sh) / 2, sw, sh]
    }
}

/// The whole file as a strip, the part the clip plays lit: dragged along, the clip plays another
/// part of its file where it is (a slip).
private struct SlipStrip: View {
    let media: TimelineMediaTool
    let c: TimelineMediaClip
    /// How long the clip plays, seconds.
    let plays: Double

    @State private var from: Double?

    var body: some View {
        GeometryReader { place in
            let w = place.size.width
            let whole = max(c.mediaSeconds, 0.001)
            let start = from ?? c.trimStart
            let x = CGFloat(start / whole) * w
            let lit = max(CGFloat(plays / whole) * w, 4)
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 10, style: .circular).fill(Pc.actionsGroup)
                if c.kind == .audio {
                    Waveform(peaks: media.peaks(c.media), from: 0, seconds: whole)
                        .fill(inkDim)
                        .padding(.vertical, 8)
                }
                RoundedRectangle(cornerRadius: 8, style: .circular)
                    .fill(Pc.accent.opacity(0.25))
                    .overlay(RoundedRectangle(cornerRadius: 8, style: .circular).strokeBorder(Pc.accent, lineWidth: 2))
                    .frame(width: min(lit, w))
                    .offset(x: min(x, max(w - lit, 0)))
                if c.kind == .audio {
                    Waveform(peaks: media.peaks(c.media), from: start, seconds: min(plays, whole - start))
                        .fill(soundWave)
                        .frame(width: min(lit, w))
                        .padding(.vertical, 8)
                        .offset(x: min(x, max(w - lit, 0)))
                        .allowsHitTesting(false)
                }
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { drag in
                        let moved = c.trimStart + Double(drag.translation.width / max(w, 1)) * whole
                        from = min(max(moved, 0), max(whole - plays, 0))
                    }
                    .onEnded { _ in
                        if let f = from {
                            media.slip(c.clip, to: f, done: true)
                        }
                        from = nil
                    }
            )
        }
        .frame(height: 54)
        .padding(.horizontal, 4)
    }
}

/// "+ Media" in the panel's header: a sound or a video from Files onto the timeline at the
/// playhead. It also shows why a file couldn't come in, and opens the animatic detector.
struct MediaAddWord: View {
    let model: CanvasModel

    var body: some View {
        @Bindable var media = model.media
        Menu {
            Button("Sound…") { media.picking = .audio }
            Button("Video…") { media.picking = .video }
        } label: {
            Text(verbatim: media.bringing ? "Bringing In…" : "+ Media")
                .font(.system(size: 17))
                .foregroundStyle(media.bringing ? inkDim : ink)
                .lineLimit(1)
                .frame(height: 30)
        }
        .disabled(media.bringing)
        .fileImporter(
            isPresented: Binding(get: { media.picking != nil }, set: { if !$0 { media.picking = nil } }),
            allowedContentTypes: media.picking == .video ? [.movie] : [.audio]
        ) { result in
            let kind = media.picking ?? .audio
            media.picking = nil
            if case let .success(url) = result {
                media.bringIn(url, kind: kind)
            }
        }
        .alert(
            "Couldn't Bring It In",
            isPresented: Binding(get: { media.failed != nil }, set: { if !$0 { media.failed = nil } })
        ) {
            Button("OK", role: .cancel) {}
        } message: {
            Text(verbatim: media.failed ?? "")
        }
        .sheet(item: Binding(get: { media.detecting.map(DetectingClip.init) }, set: { media.detecting = $0?.id })) { d in
            AnimaticSheet(model: model, clip: d.id)
        }
    }
}

private struct DetectingClip: Identifiable {
    let id: UInt64
}

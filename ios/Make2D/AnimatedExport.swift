import AVFoundation
import SwiftUI
import UIKit

private var screenFill: Color { Look.shared.light ? Color(argb: 0xFFF2_F2F2) : Color(argb: 0xFF1F_1F1F) }
private var titleInk: Color { Look.shared.light ? Color(argb: 0xFF3D_3D3D) : Color(argb: 0xFFAA_AAAA) }
private var rowInk: Color { Look.shared.light ? Color(argb: 0xFF59_5959) : Color(argb: 0xFFC1_C1C1) }
private var panelFill: Color { Look.shared.light ? Color(argb: 0xFFEB_EBEB) : Color(argb: 0xFF26_2626) }
private var panelEdge: Color { Look.shared.light ? Color(argb: 0xFFDA_DADA) : Color(argb: 0xFF36_3636) }
private var segmentTrack: Color { Look.shared.light ? Color(argb: 0xFFD5_D5D5) : Color(argb: 0xFF3C_3C3C) }
private var capsuleFill: Color { Look.shared.light ? Color(argb: 0xFFFB_FBFB) : Color(argb: 0xFF1A_1A1A) }
private var capsuleEdge: Color { Look.shared.light ? Color(argb: 0xFFE0_E0E0) : Color(argb: 0xFF37_3737) }
private var sizeInk: Color { Look.shared.light ? Color(argb: 0xFF3F_3F3F) : Color(argb: 0xFF8E_8E8E) }
private var fpsTrack: Color { Look.shared.light ? Color(argb: 0xFFDD_DDDD) : Color(argb: 0xFF33_3333) }
private var doneBlue: Color { Pc.tinted(Color(argb: 0xFF01_90FF)) }
private let panelWidth: CGFloat = 400
/// Web Ready fits this, else the canvas's size (a video as large as it can be, up to the most).
private let webReadySide = 480
private let mostVideoSide = 4096
/// The preview plays at this rate whatever the export's.
private let previewFps = 15

/// The animated formats Share offers.
enum AnimatedFormat {
    case gif, png, mp4, hevc

    var title: String {
        switch self {
        case .gif: "Animated GIF"
        case .png: "Animated PNG"
        case .mp4: "Animated MP4"
        case .hevc: "Animated HEVC"
        }
    }

    var fileExtension: String {
        switch self {
        case .gif: "gif"
        case .png: "png"
        case .mp4, .hevc: "mp4"
        }
    }

    /// Made by the system's encoder a frame at a time (its progress counted here), not by the core.
    var video: Bool {
        self == .mp4 || self == .hevc
    }

    var canBeTransparent: Bool {
        self == .gif || self == .png
    }
}

/// What an animated export is asked to make.
struct AnimatedChoices: Equatable {
    var webReady = false
    var dither = true
    var perFramePalette = false
    var transparent = false
    var fps: Float = 15
}

extension CanvasModel {
    /// The animation as `format` in the exports folder, `choices.fps` frames a second: Web Ready fits
    /// 480 pixels, else the canvas's size. `meter` hears how far a video has come; it stops when
    /// `cancelled` says so. Slow: off the main thread.
    static func exportAnimation(
        _ session: CanvasSession, _ format: AnimatedFormat, _ choices: AnimatedChoices,
        meter: ExportMeter? = nil, cancelled: @escaping () -> Bool = { false }
    ) -> URL? {
        let base = exportFileName(name: session.name(), package: false)
        let url = Sharing.exportsDir.appendingPathComponent(base + "." + format.fileExtension)
        let side = UInt32(choices.webReady ? webReadySide : 0)
        do {
            switch format {
            case .gif:
                try session.exportGif(
                    path: url.path, maxSide: side, fps: choices.fps, dither: choices.dither,
                    perFramePalette: choices.perFramePalette, transparent: choices.transparent
                )
            case .png:
                try session.exportApng(path: url.path, maxSide: side, fps: choices.fps, transparent: choices.transparent)
            case .mp4, .hevc:
                let info = session.info()
                let (width, height) = VideoWriter.fit(
                    width: Int(info.width), height: Int(info.height), maxSide: choices.webReady ? webReadySide : mostVideoSide
                )
                let order = session.animationSequence()
                // an HDR artwork's HEVC keeps its light above white (10-bit HLG)
                if format == .hevc, session.isHdr() {
                    let made = VideoWriter.writeHlg(to: url, width: width, height: height, count: order.count, fps: choices.fps) { n in
                        guard !cancelled() else { return nil }
                        meter?.share = Float(n) / Float(max(order.count, 1))
                        return session.animationFrameHlg(index: order[n], width: UInt32(width), height: UInt32(height))
                    }
                    return made ? url : nil
                }
                let made = VideoWriter.write(
                    to: url, width: width, height: height, count: order.count, hevc: format == .hevc,
                    bitsPerPixel: 4, fps: choices.fps
                ) { n, context in
                    guard !cancelled() else { return false }
                    meter?.share = Float(n) / Float(max(order.count, 1))
                    guard let frame = session.animationFrame(index: order[n], transparent: false, maxSide: UInt32(max(width, height))),
                          let picture = CanvasModel.image(frame)?.cgImage
                    else { return false }
                    context.interpolationQuality = .high
                    context.draw(picture, in: CGRect(x: 0, y: 0, width: width, height: height))
                    return true
                }
                if !made {
                    return nil
                }
            }
            return url
        } catch {
            return nil
        }
    }

    /// Share's animated export opens over everything, the panel fading away.
    func openAnimatedExport(_ format: AnimatedFormat) {
        fadePanels()
        withAnimation(Motion.studioIn) { animatedExport = format }
    }

    /// The artwork's own frame rate, which an animated export starts at: Animation Assist's, else 15.
    var artworkFps: Float {
        Float(session.timelineInfo()?.fps ?? session.animationSettings()?.fps ?? 15)
    }

    /// The animation made and shared, with the export card showing how far it has come.
    func shareAnimation(_ format: AnimatedFormat, _ choices: AnimatedChoices, done: @escaping () -> Void) {
        guard exportShown == nil else { return }
        let session = session
        session.resetExportProgress()
        exportCancelled = false
        exportShown = ProgressShown(progress: 0)
        let meter = ExportMeter()
        let poll = Timer.scheduledTimer(withTimeInterval: 0.05, repeats: true) { [weak self] _ in
            self?.exportShown = ProgressShown(progress: format.video ? meter.share : session.exportProgress())
        }
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let url = Self.exportAnimation(session, format, choices, meter: meter) { self?.exportCancelled == true }
            DispatchQueue.main.async {
                poll.invalidate()
                guard let self else { return }
                if self.exportCancelled {
                    self.exportShown = nil
                } else if let url {
                    self.exportShown = ProgressShown(progress: 1, done: true)
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.45) {
                        self.exportShown = nil
                        done()
                        Sharing.share([url])
                    }
                } else {
                    self.exportShown = nil
                    self.showNotice("The animation could not be exported.", warning: true)
                }
            }
        }
    }
}

/// A file's size as the export screen shows it: "1.3 MB", "850 KB".
private func fileSize(_ bytes: Int) -> String {
    bytes >= 1_000_000 ? String(format: "%.1f MB", Double(bytes) / 1_000_000) : "\(max(bytes / 1000, 1)) KB"
}

/// Sharing the layers as an animation: the frames playing, the settings (the size, the frame rate,
/// and per format dithering and a transparent background), Cancel, and the round Done that exports
/// and shares. The file is made as the settings change, to show its size.
struct AnimatedExport: View {
    let model: CanvasModel
    let format: AnimatedFormat

    @State private var choices = AnimatedChoices()
    /// Where the frame rate slider is, starting at the artwork's own rate.
    @State private var fpsAt: Float = 0
    @State private var bytes: Int?
    @State private var exporting = false
    @State private var settingsOpen = false
    @State private var frames: [UIImage] = []
    @State private var shown = 0

    private var title: String {
        let name = model.session.name().trimmingCharacters(in: .whitespaces)
        return name.isEmpty ? "Untitled Artwork" : name
    }

    private func close() {
        withAnimation(Motion.studioOut) { model.animatedExport = nil }
    }

    private var panel: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(verbatim: format.title)
                .font(.system(size: 24, weight: .bold))
                .foregroundStyle(titleInk)
                .lineLimit(1)
                .frame(height: 36)
                .padding(.leading, 24)
                .padding(.top, 19)
                .padding(.bottom, 11.5)
            segments
            FpsSlider(share: fpsAt, value: exportFpsLabel(fps: choices.fps)) { at in
                fpsAt = at
                choices.fps = exportFpsAt(fraction: at)
            }
            if format == .gif {
                SettingLine(label: "Dithering", on: choices.dither) { choices.dither.toggle() }
                SettingLine(label: "Per frame color palette", on: choices.perFramePalette) { choices.perFramePalette.toggle() }
            }
            if format.canBeTransparent {
                SettingLine(label: "Transparent background", on: choices.transparent) { choices.transparent.toggle() }
            }
            Spacer(minLength: 0)
        }
        .frame(width: panelWidth)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 20, style: .circular).fill(panelFill))
        .overlay(RoundedRectangle(cornerRadius: 20, style: .circular).strokeBorder(panelEdge, lineWidth: 1))
        .contentShape(Rectangle())
        .onTapGesture {}
        .padding(10)
    }

    /// Max Resolution or Web Ready, side by side in a grey track, the chosen one blue.
    private var segments: some View {
        Segments(labels: ["Max Resolution", "Web Ready"], chosen: choices.webReady ? 1 : 0) { choices.webReady = $0 == 1 }
    }

    private var buttons: some View {
        ExportButtons(cancel: close) {
            guard !exporting else { return }
            exporting = true
            model.shareAnimation(format, choices) { close() }
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { exporting = false }
        }
    }

    var body: some View {
        GeometryReader { place in
            let screen = place.size
            let wide = screen.width > screen.height
            let left: CGFloat = wide ? panelWidth + 10 : 0
            let inset: CGFloat = wide ? 41 : 24
            let boxWidth = screen.width - left - inset * 2
            let boxHeight = wide ? min(boxWidth, screen.height - 104) : boxWidth
            ZStack(alignment: .topLeading) {
                screenFill
                    .contentShape(Rectangle())
                    .onTapGesture { settingsOpen = false }
                // the frames, fitted to the box, played over and over
                ZStack {
                    if frames.indices.contains(shown) {
                        Image(uiImage: frames[shown]).resizable().scaledToFit()
                    }
                }
                .frame(width: boxWidth, height: boxHeight)
                .offset(x: left + inset, y: 110)
                .allowsHitTesting(false)
                if wide {
                    // the name and size centred between the panel and the buttons
                    VStack(spacing: 0) {
                        Text(verbatim: title)
                            .font(.system(size: 20, weight: .bold))
                            .foregroundStyle(titleInk)
                            .lineLimit(1)
                            .frame(height: 30)
                        Text(verbatim: bytes.map(fileSize) ?? "")
                            .font(.system(size: 13))
                            .foregroundStyle(sizeInk)
                            .lineLimit(1)
                            .frame(height: 20)
                    }
                    .frame(width: max(screen.width - left - 24 - 224, 0))
                    .offset(x: left + 24, y: 23)
                } else {
                    HStack(spacing: 0) {
                        PcIconView(PcIcons.chevronLeft, size: 28, color: titleInk)
                            .frame(width: 60)
                        // on two lines, as there
                        Text(verbatim: "Export\nSettings")
                            .font(.system(size: 24, weight: .bold))
                            .foregroundStyle(titleInk)
                    }
                    .padding(.trailing, 12)
                    .contentShape(Rectangle())
                    .onTapGesture { settingsOpen.toggle() }
                    .offset(x: 14, y: 20)
                }
                buttons
                    .frame(width: screen.width, alignment: .trailing)
                if wide || settingsOpen {
                    panel
                }
            }
            .task(id: "\(choices.transparent && format.canBeTransparent)-\(Int(min(boxWidth, boxHeight)))") {
                let session = model.session
                let side = UInt32(max(min(boxWidth, boxHeight) * 2, 1))
                let clear = choices.transparent && format.canBeTransparent
                let read = await Task.detached(priority: .userInitiated) { () -> [UIImage] in
                    (0 ..< session.animationFrameCount()).compactMap { i in
                        session.animationFrame(index: i, transparent: clear, maxSide: side).flatMap { CanvasModel.image($0) }
                    }
                }.value
                frames = read
                shown = 0
            }
        }
        .ignoresSafeArea()
        .onAppear {
            fpsAt = exportFpsFraction(fps: model.artworkFps)
            choices.fps = exportFpsAt(fraction: fpsAt)
        }
        .task(id: frames.count) {
            while frames.count > 1, !Task.isCancelled {
                try? await Task.sleep(for: .milliseconds(1000 / previewFps))
                shown = (shown + 1) % max(frames.count, 1)
            }
        }
        // the file as the settings make it, for its size
        .task(id: choices) {
            bytes = nil
            let session = model.session
            let asked = choices
            let made = await Task.detached(priority: .utility) { CanvasModel.exportAnimation(session, format, asked) }.value
            if !Task.isCancelled, let made {
                bytes = (try? made.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? nil
            }
        }
    }
}

/// Choices side by side in a grey track, the chosen one blue.
private struct Segments: View {
    let labels: [String]
    let chosen: Int
    let pick: (Int) -> Void

    var body: some View {
        HStack(spacing: 12) {
            ForEach(labels.indices, id: \.self) { i in
                let on = chosen == i
                Text(verbatim: labels[i])
                    .font(.system(size: 15))
                    .foregroundStyle(on ? Color.white : rowInk)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity, maxHeight: .infinity)
                    .background(Capsule().fill(on ? Pc.accent : Color.clear))
                    .contentShape(Capsule())
                    .onTapGesture { pick(i) }
            }
        }
        .padding(2)
        .frame(width: 384, height: 34)
        .background(Capsule().fill(segmentTrack))
        .padding(.leading, 7.5)
    }
}

/// An export screen's Cancel and its round blue Done.
private struct ExportButtons: View {
    let cancel: () -> Void
    let done: () -> Void

    var body: some View {
        HStack(spacing: 13) {
            Text(verbatim: "Cancel")
                .font(.system(size: 17))
                .foregroundStyle(Color(argb: 0xFFA5_A5A5))
                .padding(.horizontal, 13)
                .frame(height: 43)
                .background(Capsule().fill(capsuleFill))
                .overlay(Capsule().strokeBorder(capsuleEdge, lineWidth: 1))
                .contentShape(Capsule())
                .onTapGesture(perform: cancel)
            PcIconView(PcIcons.doneTick, size: 40, color: .white)
                .frame(width: 44, height: 45)
                .background(Capsule().fill(doneBlue))
                .contentShape(Capsule())
                .onTapGesture(perform: done)
        }
        .padding(.top, 16)
        .padding(.trailing, 16)
    }
}

/// A setting's name and its switch, 52 tall.
private struct SettingLine: View {
    let label: String
    let on: Bool
    let action: () -> Void

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(rowInk)
                .lineLimit(1)
                .frame(width: 307, alignment: .leading)
            PcSwitch(on: on)
        }
        .padding(.leading, 16)
        .padding(.trailing, 4)
        .frame(height: 52)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

/// "Frames per second" and the rate over a 3-tall track blue up to a small white knob; dragged
/// anywhere along it, the knob follows.
private struct FpsSlider: View {
    let share: Float
    let value: String
    let onChange: (Float) -> Void

    @State private var from: Float?

    var body: some View {
        ZStack(alignment: .topLeading) {
            HStack(spacing: 0) {
                Text(verbatim: "Frames per second")
                Spacer(minLength: 0)
                Text(verbatim: value)
            }
            .font(.system(size: 17))
            .foregroundStyle(rowInk)
            .lineLimit(1)
            .padding(.leading, 18)
            .padding(.trailing, 22)
            .padding(.top, 18)
            GeometryReader { place in
                let w = place.size.width
                // the knob's middle from 22.5 in to 23 in
                let x0: CGFloat = 22.5
                let x1 = w - 23
                let x = x0 + (x1 - x0) * CGFloat(min(max(share, 0), 1))
                ZStack(alignment: .leading) {
                    Capsule().fill(fpsTrack).frame(width: w - 36, height: 3).offset(x: 18)
                    Capsule().fill(Pc.accent).frame(width: max(x - 18, 0), height: 3).offset(x: 18)
                    RoundedRectangle(cornerRadius: 4, style: .circular)
                        .fill(Color.white)
                        .shadow(color: .black.opacity(0.19), radius: 1.5, y: 1)
                        .frame(width: 16, height: 10)
                        .offset(x: x - 8)
                }
                .frame(width: w, height: 28)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { drag in
                            let start = from ?? share
                            from = start
                            onChange(min(max(start + Float(drag.translation.width / max(x1 - x0, 1)), 0), 1))
                        }
                        .onEnded { _ in from = nil }
                )
            }
            .frame(height: 28)
            .offset(y: 36)
        }
        .frame(height: 72.5, alignment: .topLeading)
    }
}

/// The frames on one picture for a game engine: as the screen starts, a near-square grid of the
/// frames full size, each held frame once, with its JSON and no background.
extension SpriteSheetChoices {
    static let start = SpriteSheetChoices(
        layout: .grid, columns: 0, rows: 0, padding: 0, trim: false, heldOnce: true, json: true, maxSide: 0, transparent: true
    )
}

extension CanvasModel {
    /// The sprite sheet in the exports folder: its picture, then its JSON if asked for, and what
    /// was made; none if it couldn't be. Slow: off the main thread.
    static func exportSpriteSheet(_ session: CanvasSession, _ choices: SpriteSheetChoices) -> ([URL], SpriteSheetMade)? {
        let base = exportFileName(name: session.name(), package: false)
        let url = Sharing.exportsDir.appendingPathComponent(base + " Sprite Sheet.png")
        guard let made = try? session.exportSpriteSheet(path: url.path, choices: choices) else { return nil }
        return ([url] + (made.jsonPath.map { [URL(fileURLWithPath: $0)] } ?? []), made)
    }

    /// Share's sprite sheet export opens over everything, the panel fading away.
    func openSpriteSheetExport() {
        fadePanels()
        withAnimation(Motion.studioIn) { spriteSheetExport = true }
    }
}

/// Sharing the frames as a sprite sheet: the sheet as the settings make it, the settings (the
/// size, the layout, padding, trimming, held frames, the JSON and the background), Cancel, and the
/// round Done that shares the sheet and its JSON.
struct SpriteSheetExport: View {
    let model: CanvasModel

    @State private var choices = SpriteSheetChoices.start
    @State private var sheet: UIImage?
    @State private var made: SpriteSheetMade?
    @State private var settingsOpen = false
    @State private var exporting = false

    private var title: String {
        let name = model.session.name().trimmingCharacters(in: .whitespaces)
        return name.isEmpty ? "Untitled Artwork" : name
    }

    /// What the sheet is: its size and how many pictures are on it.
    private var said: String {
        guard let made else { return "" }
        let pictures = made.cells == 1 ? "1 frame" : "\(made.cells) frames"
        return "\(made.width) × \(made.height) px · \(pictures)"
    }

    private func close() {
        withAnimation(Motion.studioOut) { model.spriteSheetExport = false }
    }

    /// The settings as a key, so the sheet is made again as they change.
    private var asked: String {
        let c = choices
        return "\(c.layout)-\(c.columns)-\(c.rows)-\(c.padding)-\(c.trim)-\(c.heldOnce)-\(c.json)-\(c.maxSide)-\(c.transparent)"
    }

    private var panel: some View {
        VStack(alignment: .leading, spacing: 0) {
            Text(verbatim: "Sprite Sheet")
                .font(.system(size: 24, weight: .bold))
                .foregroundStyle(titleInk)
                .lineLimit(1)
                .frame(height: 36)
                .padding(.leading, 24)
                .padding(.top, 19)
                .padding(.bottom, 11.5)
            Segments(labels: ["Max Resolution", "Web Ready"], chosen: choices.maxSide == 0 ? 0 : 1) {
                choices.maxSide = $0 == 1 ? UInt32(webReadySide) : 0
            }
            Segments(labels: ["Grid", "Row", "Column"], chosen: [SheetLayout.grid, .across, .down].firstIndex(of: choices.layout) ?? 0) {
                choices.layout = [SheetLayout.grid, .across, .down][$0]
            }
            .padding(.top, 12)
            .padding(.bottom, 6)
            if choices.layout == .grid {
                CountLine(label: "Columns", value: choices.columns, most: 64) { choices.columns = $0 }
                CountLine(label: "Rows", value: choices.rows, most: 64) { choices.rows = $0 }
            }
            CountLine(label: "Padding", value: choices.padding, most: 64, none: "None") { choices.padding = $0 }
            SettingLine(label: "Trim empty space", on: choices.trim) { choices.trim.toggle() }
            SettingLine(label: "Held frames once", on: choices.heldOnce) { choices.heldOnce.toggle() }
            SettingLine(label: "JSON data", on: choices.json) { choices.json.toggle() }
            SettingLine(label: "Transparent background", on: choices.transparent) { choices.transparent.toggle() }
            Spacer(minLength: 0)
        }
        .frame(width: panelWidth)
        .frame(maxHeight: .infinity, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 20, style: .circular).fill(panelFill))
        .overlay(RoundedRectangle(cornerRadius: 20, style: .circular).strokeBorder(panelEdge, lineWidth: 1))
        .contentShape(Rectangle())
        .onTapGesture {}
        .padding(10)
    }

    var body: some View {
        GeometryReader { place in
            let screen = place.size
            let wide = screen.width > screen.height
            let left: CGFloat = wide ? panelWidth + 10 : 0
            let inset: CGFloat = wide ? 41 : 24
            let boxWidth = screen.width - left - inset * 2
            let boxHeight = wide ? min(boxWidth, screen.height - 140) : boxWidth
            ZStack(alignment: .topLeading) {
                screenFill
                    .contentShape(Rectangle())
                    .onTapGesture { settingsOpen = false }
                // the sheet, fitted to the box, its edge drawn
                ZStack {
                    if let sheet {
                        Image(uiImage: sheet)
                            .resizable()
                            .interpolation(.none)
                            .scaledToFit()
                            .overlay(Rectangle().strokeBorder(panelEdge, lineWidth: 1))
                    }
                }
                .frame(width: boxWidth, height: boxHeight)
                .offset(x: left + inset, y: 110)
                .allowsHitTesting(false)
                if wide {
                    VStack(spacing: 0) {
                        Text(verbatim: title)
                            .font(.system(size: 20, weight: .bold))
                            .foregroundStyle(titleInk)
                            .lineLimit(1)
                            .frame(height: 30)
                        Text(verbatim: said)
                            .font(.system(size: 13))
                            .foregroundStyle(sizeInk)
                            .lineLimit(1)
                            .frame(height: 20)
                    }
                    .frame(width: max(screen.width - left - 24 - 224, 0))
                    .offset(x: left + 24, y: 23)
                } else {
                    HStack(spacing: 0) {
                        PcIconView(PcIcons.chevronLeft, size: 28, color: titleInk)
                            .frame(width: 60)
                        Text(verbatim: "Export\nSettings")
                            .font(.system(size: 24, weight: .bold))
                            .foregroundStyle(titleInk)
                    }
                    .padding(.trailing, 12)
                    .contentShape(Rectangle())
                    .onTapGesture { settingsOpen.toggle() }
                    .offset(x: 14, y: 20)
                }
                ExportButtons(cancel: close) {
                    guard !exporting else { return }
                    exporting = true
                    model.shareSpriteSheet(choices) { close() }
                    DispatchQueue.main.asyncAfter(deadline: .now() + 0.5) { exporting = false }
                }
                .frame(width: screen.width, alignment: .trailing)
                if wide || settingsOpen {
                    panel
                }
            }
        }
        .ignoresSafeArea()
        // the sheet as the settings make it, a moment after they stop changing
        .task(id: asked) {
            try? await Task.sleep(for: .milliseconds(250))
            guard !Task.isCancelled else { return }
            let session = model.session
            let wanted = choices
            let result = await Task.detached(priority: .userInitiated) { () -> (UIImage?, SpriteSheetMade)? in
                guard let done = CanvasModel.exportSpriteSheet(session, wanted), let png = done.0.first else { return nil }
                return (UIImage(contentsOfFile: png.path), done.1)
            }.value
            if !Task.isCancelled, let result {
                sheet = result.0
                made = result.1
            }
        }
    }
}

/// A setting that counts: its name, its value (0 is "Auto", or `none`), and − and + in a grey
/// track, 52 tall.
private struct CountLine: View {
    let label: String
    let value: UInt32
    let most: UInt32
    var none = "Auto"
    let change: (UInt32) -> Void

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 17))
                .foregroundStyle(rowInk)
                .lineLimit(1)
            Spacer(minLength: 0)
            Text(verbatim: value == 0 ? none : "\(value)")
                .font(.system(size: 17))
                .foregroundStyle(rowInk)
                .monospacedDigit()
                .padding(.trailing, 12)
            HStack(spacing: 0) {
                step("−", enabled: value > 0) { change(value - 1) }
                Rectangle().fill(panelEdge).frame(width: 1, height: 18)
                step("+", enabled: value < most) { change(value + 1) }
            }
            .frame(height: 32)
            .background(Capsule().fill(segmentTrack))
        }
        .padding(.leading, 16)
        .padding(.trailing, 16)
        .frame(height: 52)
    }

    private func step(_ sign: String, enabled: Bool, _ action: @escaping () -> Void) -> some View {
        Text(verbatim: sign)
            .font(.system(size: 20, weight: .medium))
            .foregroundStyle(enabled ? rowInk : rowInk.opacity(0.35))
            .frame(width: 44, height: 32)
            .contentShape(Rectangle())
            .onTapGesture { if enabled { action() } }
    }
}

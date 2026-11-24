import SwiftUI
import UIKit

// The grid timeline's settings (see TimelineGrid.swift): a card standing on the bar over its
// button, a tab along its bottom for each part (the project's frame rate, length and size; the
// onion skins; how it plays; and what is kept from one canvas to the next), what a row opens
// standing beside it. And two things it turns on or sets: the timecode over the bar's corner,
// and the flap on the ruler at the timeline's end, with its own menu.

/// The frame rates offered by name.
private let frameRates: [(String, UInt32)] = [
    ("Frame by frame", 12), ("Anime", 15), ("Cinema", 24), ("Television", 30), ("PAL", 50), ("NTSC", 60), ("Mobile", 120),
]

/// The most of each the settings take (the core's own).
private let mostFrameRate = 120
private let mostHeld = 12
private let mostSkins = 12
private let mostLength = 100_000

/// A length of `frames` at `fps` as minutes and seconds.
func gridClock(_ frames: UInt32, fps: UInt32) -> String {
    let seconds = Int(frames) / max(Int(fps), 1)
    return String(format: "%02d:%02d", seconds / 60, seconds % 60)
}

/// Frame `frame` at `fps` as a timecode: hours, minutes, seconds, and the frame within the second.
func gridTimecode(_ frame: UInt32, fps: UInt32) -> String {
    let fps = max(Int(fps), 1)
    let seconds = Int(frame) / fps
    return String(format: "%02d:%02d:%02d.%03d", seconds / 3600, seconds / 60 % 60, seconds % 60, Int(frame) % fps)
}

// MARK: - The card

/// The timeline's settings.
struct GridSettingsCard: View {
    static let width: CGFloat = 318

    private enum Tab: String, CaseIterable {
        case project = "Project"
        case onion = "Onion"
        case timeline = "Timeline"
        case prefs = "Prefs"

        var icon: PcIcon {
            switch self {
            case .project: PcIcons.gridProject
            case .onion: PcIcons.gridOnion
            case .timeline: PcIcons.gridOptions
            case .prefs: PcIcons.gridPrefs
            }
        }
    }

    /// What stands beside the card.
    private enum Beside {
        case rates, rate, length
    }

    let model: CanvasModel
    let tool: TimelineTool
    let info: TimelineInfo
    /// How tall the card may stand.
    let room: CGFloat
    let close: () -> Void

    @State private var tab: Tab = .project
    @State private var beside: Beside?
    /// The onion skins shown in its sliders: those before the drawing, or those after.
    @State private var after = false
    @AppStorage("timelineKeepTiming") private var keepTiming = true
    @AppStorage("timelineTimecode") private var timecode = false
    @AppStorage("timelineScrubSound") private var scrubSound = true
    @AppStorage("timelineVectorFrames") private var vectorFrames = false
    /// The frame rate as its slider is dragged, before it is let go.
    @State private var rate: UInt32?
    /// How much a performed move is evened out when its take ends.
    @AppStorage("timelinePerformSmoothing") private var smoothing = 0.5

    var body: some View {
        // (what a row opens stands before the card, which is at the bar's right end)
        HStack(alignment: .bottom, spacing: 6) {
            switch beside {
            case .rates: rates
            case .rate:
                GridCard {
                    GridKeypad(unit: "FPS", from: Int(info.fps), most: mostFrameRate) { n in
                        setRate(UInt32(n))
                        beside = nil
                    }
                }
            case .length:
                GridCard {
                    GridKeypad(unit: "frames", from: Int(info.duration), most: mostLength, note: { gridClock(UInt32($0), fps: info.fps) }) { n in
                        tool.setLength(UInt32(n))
                        beside = nil
                    }
                    GridMenuLine()
                    GridMenuRow(icon: PcIcons.gridFill, label: "Fit to content") {
                        tool.fitToContent()
                        beside = nil
                    }
                }
            case nil:
                EmptyView()
            }
            GridCard(width: Self.width) {
                Text(verbatim: "Timeline Settings")
                    .font(.system(size: 22, weight: .bold))
                    .foregroundStyle(Grid.inkBright)
                    .padding(.horizontal, 18)
                    .padding(.top, 8)
                    .padding(.bottom, 12)
                VStack(alignment: .leading, spacing: 8) {
                    switch tab {
                    case .project: project
                    case .onion: onion
                    case .timeline: timeline
                    case .prefs: prefs
                    }
                }
                .padding(.horizontal, 10)
                .frame(height: min(max(room - 170, 200), 322), alignment: .top)
                tabs
            }
        }
    }

    private func setRate(_ fps: UInt32) {
        guard fps != info.fps else { return }
        if keepTiming {
            tool.retime(fps)
        } else {
            tool.change { $0.fps = fps }
        }
    }

    // MARK: Tabs

    private var tabs: some View {
        HStack(spacing: 0) {
            ForEach(Tab.allCases, id: \.self) { t in
                let color = t == tab ? Pc.accent : Grid.inkBright
                VStack(spacing: 4) {
                    PcIconView(t.icon, size: 24, color: color)
                    Text(verbatim: t.rawValue)
                        .font(.system(size: 11.5))
                        .foregroundStyle(color)
                }
                .frame(maxWidth: .infinity, minHeight: 52)
                .contentShape(Rectangle())
                .onTapGesture {
                    tab = t
                    beside = nil
                }
                .accessibilityElement()
                .accessibilityLabel(Text(verbatim: t.rawValue))
                .accessibilityAddTraits(t == tab ? [.isButton, .isSelected] : .isButton)
            }
        }
        .padding(.horizontal, 8)
        .padding(.top, 6)
    }

    @ViewBuilder
    private var project: some View {
        GridSection {
            GridValueRow(label: "Frames per second", value: "\(rate ?? info.fps)", lit: beside == .rates || beside == .rate) {
                beside = beside == .rates || beside == .rate ? nil : .rates
            }
            // (dragged for any rate; the row above for the named ones)
            GridMenuSlider(label: "Frames per second", value: Float(Int(rate ?? info.fps) - 1) / Float(mostFrameRate - 1), bare: true) { v, done in
                let fps = UInt32(1 + wholeNumber(v * Float(mostFrameRate - 1)))
                if done {
                    rate = nil
                    setRate(fps)
                } else if keepTiming {
                    // (the timing is kept by moving every frame: once, when it is let go)
                    rate = fps
                } else {
                    tool.change { $0.fps = fps }
                }
            }
            .padding(.top, -8)
            .padding(.bottom, 4)
            // (under a second, its frames say more)
            let length = info.duration < info.fps ? "\(info.duration) frames" : gridClock(info.duration, fps: info.fps)
            GridValueRow(label: "Duration", value: length, lit: beside == .length) {
                beside = beside == .length ? nil : .length
            }
        }
        GridSection {
            GridToggleRow(label: "Keep timing when FPS changes", on: keepTiming) { keepTiming.toggle() }
        }
        GridSection {
            // (changing the canvas's size is Crop & Resize's work)
            let size = model.session.info()
            GridValueRow(label: "Resolution", value: "\(size.width) × \(size.height)", lit: false) {
                close()
                model.closePanels()
                model.crop.begin()
            }
        }
    }

    @ViewBuilder
    private var onion: some View {
        let o = tool.onion ?? TimelineOnion(before: 0, after: 0, beforeColor: 0, afterColor: 0)
        let shown = o.before > 0 || o.after > 0
        let count = after ? o.after : o.before
        let color = after ? o.afterColor : o.beforeColor
        GridSection {
            GridToggleRow(label: "Onion skin visibility", on: shown) { tool.toggleOnionSkin() }
        }
        HStack(spacing: 4) {
            side("Backwards", chosen: !after) { after = false }
            side("Forwards", chosen: after) { after = true }
        }
        GridSection {
            GridMenuSlider(label: "Frames", value: Float(count) / Float(mostSkins), text: { "\(wholeNumber($0 * Float(mostSkins)))" }) { v, _ in
                let n = UInt32(wholeNumber(v * Float(mostSkins)))
                tool.setOnion { if after { $0.after = n } else { $0.before = n } }
            }
            GridMenuSlider(label: "Opacity", value: info.onionOpacity) { v, _ in tool.change { $0.onionOpacity = v } }
                .padding(.bottom, 6)
        }
        GridSection {
            // tinted off, the skins show in their own colours
            GridToggleRow(label: "Color tint", on: color != 0) {
                let tint: UInt32 = color != 0 ? 0 : after ? 0x339933 : 0xE63333
                tool.setOnion { if after { $0.afterColor = tint } else { $0.beforeColor = tint } }
            }
            if color != 0 {
                HStack(spacing: 0) {
                    ForEach(onionColors.filter { $0.1 != 0 }, id: \.1) { name, rgb in
                        ZStack {
                            Circle().fill(Color(rgb: rgb)).frame(width: 22, height: 22)
                            if color == rgb {
                                Circle().strokeBorder(Grid.inkBright, lineWidth: 2).frame(width: 30, height: 30)
                            }
                        }
                        .frame(maxWidth: .infinity, minHeight: 40)
                        .contentShape(Rectangle())
                        .onTapGesture { tool.setOnion { if after { $0.afterColor = rgb } else { $0.beforeColor = rgb } } }
                        .accessibilityLabel(Text(verbatim: name))
                    }
                }
                .padding(.horizontal, 8)
            }
        }
    }

    private func side(_ label: String, chosen: Bool, choose: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 14, weight: chosen ? .semibold : .regular))
            .foregroundStyle(chosen ? Color.white : Grid.ink)
            .frame(maxWidth: .infinity, minHeight: 34)
            .background(Capsule().fill(chosen ? Pc.accent : Grid.inkBright.opacity(0.07)))
            .contentShape(Rectangle())
            .onTapGesture(perform: choose)
    }

    @ViewBuilder
    private var timeline: some View {
        let ways: [(AnimationPlayback, String)] = [(.loop, "Loop"), (.pingPong, "Ping Pong"), (.oneShot, "One Shot")]
        GridSection {
            ForEach(ways, id: \.1) { way, label in
                let chosen = info.playback == way
                HStack(spacing: 0) {
                    Text(verbatim: label)
                        .font(.system(size: 16))
                        .foregroundStyle(Grid.inkBright)
                    Spacer(minLength: 0)
                    ZStack {
                        Circle().strokeBorder(chosen ? Pc.accent : Grid.inkBright.opacity(0.8), lineWidth: 1.5).frame(width: 20, height: 20)
                        if chosen {
                            Circle().fill(Pc.accent).frame(width: 12, height: 12)
                        }
                    }
                }
                .padding(.horizontal, 14)
                .frame(height: 44)
                .contentShape(Rectangle())
                .onTapGesture { tool.change { $0.playback = way } }
                .accessibilityElement()
                .accessibilityLabel(Text(verbatim: label))
                .accessibilityAddTraits(chosen ? [.isButton, .isSelected] : .isButton)
            }
        }
        GridSection {
            GridMenuSlider(
                label: "Frames per new drawing", value: Float(Int(info.exposure) - 1) / Float(mostHeld - 1),
                text: { "\(1 + wholeNumber($0 * Float(mostHeld - 1)))" }
            ) { v, _ in
                tool.change { $0.exposure = UInt32(1 + wholeNumber(v * Float(mostHeld - 1))) }
            }
            .padding(.bottom, 6)
        }
        GridSection {
            // (none: a performed move is kept as it was made, corners and all)
            GridMenuSlider(label: "Perform smoothing", value: Float(smoothing)) { v, _ in smoothing = Double(v) }
                .padding(.bottom, 6)
        }
    }

    @ViewBuilder
    private var prefs: some View {
        GridSection {
            GridToggleRow(label: "Timecode", on: timecode) { timecode.toggle() }
        }
        GridSection {
            // (off, moving the playhead by hand is silent; playing still sounds)
            GridToggleRow(label: "Sound while scrubbing", on: scrubSound) { scrubSound.toggle() }
        }
        GridSection {
            GridToggleRow(label: "New frames are vector layers", on: vectorFrames) {
                vectorFrames.toggle()
                model.session.setTimelineVectorFrames(on: vectorFrames)
            }
        }
    }

    /// The frame rates by name, and one typed in.
    private var rates: some View {
        GridCard {
            HStack(spacing: 8) {
                Text(verbatim: "Custom")
                Spacer(minLength: 0)
                Text(verbatim: "\(info.fps)").monospacedDigit()
                GridRateBadge()
                PcIconView(PcIcons.chevronRight, size: 15, color: Grid.inkBright)
            }
            .font(.system(size: 15))
            .foregroundStyle(Grid.inkBright)
            .padding(.horizontal, 16)
            .frame(height: 40)
            .contentShape(Rectangle())
            .onTapGesture { beside = .rate }
            GridMenuLine()
            ForEach(frameRates, id: \.1) { name, fps in
                HStack(spacing: 8) {
                    Text(verbatim: name)
                    Spacer(minLength: 0)
                    Text(verbatim: "\(fps)").monospacedDigit()
                    GridRateBadge()
                }
                .font(.system(size: 15))
                .foregroundStyle(Grid.inkBright)
                .padding(.horizontal, 16)
                .frame(height: 36)
                .background(RoundedRectangle(cornerRadius: 12, style: .continuous).fill(info.fps == fps ? Grid.inkBright.opacity(0.12) : .clear).padding(.horizontal, 6))
                .contentShape(Rectangle())
                .onTapGesture { setRate(fps) }
                .accessibilityElement()
                .accessibilityLabel(Text(verbatim: "\(name), \(fps) frames per second"))
                .accessibilityAddTraits(info.fps == fps ? [.isButton, .isSelected] : .isButton)
            }
        }
    }
}

// MARK: - Pieces

/// "FPS" in a little box after a frame rate.
private struct GridRateBadge: View {
    var body: some View {
        Text(verbatim: "FPS")
            .font(.system(size: 9, weight: .bold))
            .foregroundStyle(Grid.panel)
            .padding(.horizontal, 3.5)
            .padding(.vertical, 2)
            .background(RoundedRectangle(cornerRadius: 3.5, style: .continuous).fill(Grid.inkBright))
    }
}

/// A group of the settings' rows on one rounded ground.
struct GridSection<Content: View>: View {
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content()
        }
        .frame(maxWidth: .infinity, alignment: .leading)
        .background(RoundedRectangle(cornerRadius: 14, style: .continuous).fill(Grid.inkBright.opacity(0.07)))
    }
}

/// A setting and what it is: tapped, something opens to change it.
struct GridValueRow: View {
    let label: String
    let value: String
    let lit: Bool
    let action: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Text(verbatim: label)
                .foregroundStyle(Grid.inkBright)
            Spacer(minLength: 0)
            Text(verbatim: value)
                .foregroundStyle(lit ? Pc.accent : Grid.inkBright)
                .monospacedDigit()
        }
        .font(.system(size: 16))
        .lineLimit(1)
        .padding(.horizontal, 14)
        .frame(height: 44)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
        .accessibilityElement()
        .accessibilityLabel(Text(verbatim: "\(label), \(value)"))
        .accessibilityAddTraits(.isButton)
    }
}

/// A setting that is on or off, with its switch.
struct GridToggleRow: View {
    let label: String
    let on: Bool
    let action: () -> Void

    var body: some View {
        HStack(spacing: 8) {
            Text(verbatim: label)
                .font(.system(size: 16))
                .foregroundStyle(Grid.inkBright)
                .lineLimit(1)
                .minimumScaleFactor(0.8)
            Spacer(minLength: 0)
            GridToggle(on: on)
        }
        .padding(.horizontal, 14)
        .frame(height: 44)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
        .accessibilityElement()
        .accessibilityLabel(Text(verbatim: label))
        .accessibilityValue(Text(verbatim: on ? "On" : "Off"))
        .accessibilityAddTraits(.isButton)
    }
}

/// A switch: a pill in the accent with its knob over when on.
struct GridToggle: View {
    let on: Bool

    var body: some View {
        Capsule()
            .fill(on ? Pc.accent : Grid.inkBright.opacity(0.28))
            .frame(width: 48, height: 28)
            .overlay(alignment: on ? .trailing : .leading) {
                Capsule().fill(Color.white).frame(width: 28, height: 22).padding(.horizontal, 3)
            }
            .animation(.easeOut(duration: 0.14), value: on)
    }
}

/// A number typed on its own keys: what is typed so far over them, and Set to take it.
struct GridKeypad: View {
    let unit: String
    /// The number as it is, shown until one is typed.
    let from: Int
    let most: Int
    /// Something said of the number typed (its length in time, say).
    var note: ((Int) -> String)?
    let set: (Int) -> Void

    @State private var typed = ""

    var body: some View {
        let number = Int(typed)
        let valid = number.map { $0 >= 1 && $0 <= most } ?? false
        VStack(spacing: 6) {
            HStack(alignment: .firstTextBaseline, spacing: 5) {
                Text(verbatim: typed.isEmpty ? "\(from)" : typed)
                    .font(.system(size: 22, weight: .semibold).monospacedDigit())
                    .foregroundStyle(typed.isEmpty ? Grid.inkDim : Grid.inkBright)
                Text(verbatim: unit)
                    .font(.system(size: 13))
                    .foregroundStyle(Grid.ink)
                Spacer(minLength: 0)
                if let note {
                    Text(verbatim: note(number ?? from))
                        .font(.system(size: 13).monospacedDigit())
                        .foregroundStyle(Grid.ink)
                }
            }
            .padding(.horizontal, 8)
            .frame(height: 34)
            ForEach([[7, 8, 9], [4, 5, 6], [1, 2, 3]], id: \.self) { row in
                HStack(spacing: 6) {
                    ForEach(row, id: \.self) { n in
                        key("\(n)") { press(n) }
                    }
                }
            }
            HStack(spacing: 6) {
                key("0") { press(0) }
                key("⌫") {
                    if !typed.isEmpty {
                        typed.removeLast()
                    }
                }
                Text(verbatim: "Set")
                    .font(.system(size: 16, weight: .semibold))
                    .foregroundStyle(Color.white)
                    .frame(maxWidth: .infinity, minHeight: 40)
                    .background(Capsule().fill(valid ? Pc.accent : Pc.accent.opacity(0.35)))
                    .contentShape(Rectangle())
                    .onTapGesture {
                        if let number, valid {
                            set(number)
                        }
                    }
                    .accessibilityAddTraits(.isButton)
            }
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 2)
    }

    private func press(_ n: Int) {
        guard typed.count < 6, !(typed.isEmpty && n == 0) else { return }
        typed.append("\(n)")
        UISelectionFeedbackGenerator().selectionChanged()
    }

    private func key(_ label: String, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 19))
            .foregroundStyle(Grid.inkBright)
            .frame(maxWidth: .infinity, minHeight: 40)
            .background(Capsule().fill(Grid.inkBright.opacity(0.1)))
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
            .accessibilityAddTraits(.isButton)
    }
}

// MARK: - Timecode

/// Where the playhead is, in time, over the bar's corner.
struct GridTimecode: View {
    let frame: UInt32
    let fps: UInt32

    var body: some View {
        Text(verbatim: gridTimecode(frame, fps: fps))
            .font(.system(size: 17).monospacedDigit())
            .foregroundStyle(Grid.ink)
            .padding(.horizontal, 10)
            .frame(height: 28)
            .background(Capsule().fill(Grid.panel.opacity(0.82)))
            .allowsHitTesting(false)
            .accessibilityLabel(Text(verbatim: "Timecode"))
    }
}

// MARK: - The end flap

/// The flap on the ruler where the timeline ends: dragged, the timeline is longer or shorter
/// (what is left past its end stays, shaded, and neither plays nor is exported); tapped, its menu
/// opens.
struct GridEndFlap: View {
    let tool: TimelineTool
    let grid: GridState
    let info: TimelineInfo
    let place: GridPlace

    @State private var from: Int?

    var body: some View {
        let frames = grid.lengthDraft ?? Int(info.duration)
        let dragged = grid.lengthDraft != nil
        let color = dragged || grid.flapMenu != nil ? Pc.accent : Grid.box
        let x = place.origin + CGFloat(frames) * place.sizes.pitch - place.sizes.gap / 2
        ZStack(alignment: .topLeading) {
            color.frame(width: 2, height: Grid.rulerHeight)
            // the flap itself, flying from the top of its pole
            UnevenRoundedRectangle(topLeadingRadius: 0, bottomLeadingRadius: 0, bottomTrailingRadius: 5, topTrailingRadius: 5, style: .continuous)
                .fill(color)
                .frame(width: 13, height: 13)
                .overlay {
                    VStack(spacing: 2.5) {
                        ForEach(0 ..< 2, id: \.self) { _ in
                            Capsule().fill(Grid.panel).frame(width: 6, height: 1.5)
                        }
                    }
                }
                .offset(x: 2, y: 2)
        }
        .frame(width: 30, height: Grid.rulerHeight, alignment: .topLeading)
        .contentShape(Rectangle().offset(x: -8))
        .offset(x: x - 1)
        .onTapGesture {
            let open = grid.flapMenu == nil
            grid.closeMenus(tool)
            if open {
                grid.flapMenu = x
            }
        }
        .highPriorityGesture(
            DragGesture(minimumDistance: 4)
                .onChanged { drag in
                    if from == nil {
                        from = Int(info.duration)
                        tool.stopPlaying()
                        grid.closeMenus(tool)
                    }
                    let next = max((from ?? 1) + Int((drag.translation.width / place.sizes.pitch).rounded()), 1)
                    if next != grid.lengthDraft {
                        grid.lengthDraft = next
                        UISelectionFeedbackGenerator().selectionChanged()
                    }
                }
                .onEnded { _ in
                    if let next = grid.lengthDraft, next != Int(info.duration) {
                        tool.setLength(UInt32(next))
                    }
                    grid.lengthDraft = nil
                    from = nil
                }
        )
        .accessibilityElement()
        .accessibilityLabel(Text(verbatim: "Timeline end"))
        .accessibilityValue(Text(verbatim: "\(frames) frames"))
        .accessibilityAddTraits(.isButton)
    }
}

/// The end flap's menu: the timeline's length typed in or fitted to its frames, and frames added
/// before the first or after the last.
struct GridFlapMenu: View {
    let tool: TimelineTool
    let info: TimelineInfo
    let close: () -> Void

    @State private var typing = false
    @State private var adding = 0

    var body: some View {
        let count = adding > 0 ? adding : Int(info.fps)
        HStack(alignment: .bottom, spacing: 6) {
            GridCard {
                HStack(spacing: 8) {
                    Text(verbatim: "Length")
                    Spacer(minLength: 0)
                    Text(verbatim: "\(info.duration) · \(gridClock(info.duration, fps: info.fps))")
                        .monospacedDigit()
                        .foregroundStyle(typing ? Pc.accent : Grid.inkBright)
                }
                .font(.system(size: 15))
                .foregroundStyle(Grid.inkBright)
                .padding(.horizontal, 16)
                .frame(height: 36)
                .contentShape(Rectangle())
                .onTapGesture { typing.toggle() }
                GridMenuRow(icon: PcIcons.gridFill, label: "Fit to content") {
                    close()
                    tool.fitToContent()
                }
                GridMenuLine()
                GridDurationRow(label: "Frames to add", frames: count, locked: false) { adding = $0 }
                GridMenuRow(icon: PcIcons.gridInsert, label: "Add at start") {
                    close()
                    tool.addFrames(atStart: true, count: UInt32(count))
                }
                GridMenuRow(icon: PcIcons.gridInsert, label: "Add at end") {
                    close()
                    tool.addFrames(atStart: false, count: UInt32(count))
                }
            }
            if typing {
                GridCard {
                    GridKeypad(unit: "frames", from: Int(info.duration), most: mostLength, note: { gridClock(UInt32($0), fps: info.fps) }) { n in
                        close()
                        tool.setLength(UInt32(n))
                    }
                }
            }
        }
    }
}

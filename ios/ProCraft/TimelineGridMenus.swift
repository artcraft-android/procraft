import SwiftUI
import UIKit

// The grid timeline's menus (see TimelineGrid.swift), each a card standing on the bar: a frame's
// (held), a track's (its name held, or Track options in a frame's), and the menu of the frames
// picked together (one of them held with the lasso on). What a row opens stands beside its card.

/// The colours a track's name is highlighted in (0xRRGGBB).
let gridHighlights: [(String, UInt32)] = [
    ("Red", 0xFF453A), ("Orange", 0xFF9F0A), ("Yellow", 0xFFD60A), ("Green", 0x30D158), ("Blue", 0x0A84FF),
    ("Purple", 0xBF5AF2), ("Pink", 0xF050C8), ("Grey", 0x8E8E93),
]

let cardWidth: CGFloat = 228

/// Whichever menu is open, standing on the bar where it belongs: the settings (opened from
/// Actions), a track's over its name, a frame's or the picked frames' over the frame held.
struct GridMenus: View {
    let model: CanvasModel
    let tool: TimelineTool
    let info: TimelineInfo
    let grid: GridState
    let width: CGFloat
    /// How tall a menu may stand.
    let room: CGFloat

    var body: some View {
        Group {
            if tool.settingsOpen {
                // over its button in the header
                GridMenuPlace(x: width - 241, first: GridSettingsCard.width, trailing: true) {
                    GridSettingsCard(model: model, tool: tool, info: info, room: room) { grid.closeMenus(tool) }
                }
            } else if let spot = grid.keyMenu {
                GridMenuPlace(x: spot.x) {
                    GridKeyMenu(tool: tool, key: spot.key, room: room) { grid.closeMenus(tool) }
                }
            } else if grid.adding {
                // over the `+` it was opened from
                GridMenuPlace(x: width - 41, first: GridAddMenu.width, trailing: true) {
                    GridAddMenu(tool: tool, grid: grid) { grid.closeMenus(tool) }
                }
            } else if let spot = grid.emptyMenu, let track = tool.tracks.first(where: { $0.id == spot.track }) {
                GridMenuPlace(x: spot.x) {
                    GridEmptyMenu(tool: tool, track: track, frame: spot.frame, room: room) { grid.closeMenus(tool) }
                }
            } else if let x = grid.flapMenu {
                GridMenuPlace(x: x) {
                    GridFlapMenu(tool: tool, info: info) { grid.closeMenus(tool) }
                }
            } else if let id = tool.trackOpen, let track = tool.tracks.first(where: { $0.id == id }) {
                GridMenuPlace(x: 14 + cardWidth / 2) {
                    GridTrackMenu(tool: tool, track: track, room: room) { grid.closeMenus(tool) }
                }
            } else if let menu = tool.clipMenu, tool.selecting == nil, let (track, clip) = tool.clip(menu.clip) {
                GridMenuPlace(x: menu.x) {
                    if clip.kind == .drawing {
                        GridFrameMenu(tool: tool, track: track, clip: clip, room: room) { grid.closeMenus(tool) }
                    } else {
                        GridBlockMenu(tool: tool, grid: grid, track: track, clip: clip, room: room) { grid.closeMenus(tool) }
                    }
                }
            } else if let x = grid.groupMenu, let chosen = tool.selecting, !chosen.isEmpty {
                GridMenuPlace(x: x) {
                    GridGroupMenu(tool: tool, ids: Array(chosen)) { grid.closeMenus(tool) }
                }
            }
        }
        .frame(width: width, alignment: .leading)
        .padding(.bottom, 8)
        .transition(.opacity)
    }
}

/// Its cards with the first of them centred on `x` along the bar, as near as they all fit on it.
struct GridMenuPlace: Layout {
    let x: CGFloat
    /// How wide the first card is (the last, with `trailing`: what a row opens stands before it).
    var first: CGFloat = cardWidth
    var trailing = false

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let size = subviews.first?.sizeThatFits(.unspecified) ?? .zero
        return CGSize(width: proposal.width ?? size.width, height: size.height)
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        guard let cards = subviews.first else { return }
        let size = cards.sizeThatFits(.unspecified)
        let left = min(max(x - first / 2 - (trailing ? size.width - first : 0), 0), max(bounds.width - size.width, 0))
        cards.place(at: CGPoint(x: bounds.minX + left, y: bounds.minY), proposal: ProposedViewSize(size))
    }
}

// MARK: - The menus

/// A frame's menu: cut, copy, paste and duplicate; a new frame after it, lasting through the
/// empty frames after it, how many frames it lasts; its track's options; clearing and deleting it.
private struct GridFrameMenu: View {
    let tool: TimelineTool
    let track: TimelineTrackInfo
    let clip: TimelineClipInfo
    let room: CGFloat
    let close: () -> Void

    @State private var options = false
    @State private var typing = false
    @State private var keys = false

    var body: some View {
        let drawing = clip.kind == .drawing
        let locked = tool.held(track)
        HStack(alignment: .top, spacing: 6) {
            GridCard {
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
                    duplicate: locked || clip.kind == .group ? nil : {
                        close()
                        tool.duplicateFrames([clip.id])
                    }
                )
                GridMenuLine()
                if drawing {
                    GridMenuRow(icon: PcIcons.gridInsert, label: "Insert frame", dim: locked) {
                        close()
                        tool.insertFrame(after: clip, on: track)
                    }
                }
                GridMenuRow(icon: PcIcons.gridFill, label: "Fill duration", dim: !tool.canExtend([clip.id])) {
                    close()
                    tool.extend([clip.id])
                }
                // inside a group: out onto the level the group is on
                if (tool.place(track.id)?.group ?? 0) != 0 {
                    GridMenuRow(icon: PcIcons.layers, label: "Move out of group", dim: locked) {
                        close()
                        tool.moveOutOfGroup(clip.id)
                    }
                }
                GridMenuLine()
                GridDurationRow(frames: Int(clip.duration), locked: locked, keypad: {
                    typing.toggle()
                    options = false
                    keys = false
                }) { tool.gridTrim(clip.id, leading: false, to: Int(clip.start) + max($0, 1)) }
                GridMenuLine()
                GridMenuRow(icon: PcIcons.gridKey, label: "Keys", chevron: true, lit: keys) {
                    keys.toggle()
                    options = false
                    typing = false
                    // (its keys go on the playhead's frame: the playhead comes to it)
                    if keys {
                        tool.bringPlayhead(onto: clip)
                    }
                }
                GridMenuRow(icon: PcIcons.gridOptions, label: "Track options", chevron: true, lit: options) {
                    options.toggle()
                    typing = false
                    keys = false
                }
                GridMenuLine()
                // turned off, it stays where it is but neither shows nor plays
                GridMenuRow(icon: PcIcons.gridHighlight, label: clip.disabled ? "Show frame" : "Hide frame", dim: locked) {
                    tool.setClipDisabled(clip.id, !clip.disabled)
                }
                if drawing {
                    GridMenuRow(icon: PcIcons.gridClear, label: "Clear frame", dim: locked) {
                        close()
                        tool.clearFrames([clip.id])
                    }
                }
                GridMenuRow(icon: PcIcons.trash, label: "Delete frame", tint: Grid.danger, dim: locked) {
                    close()
                    tool.deleteFrames([clip.id])
                }
            }
            if keys {
                GridKeysCard(tool: tool, clip: clip, close: close)
            } else if options {
                GridTrackMenu(tool: tool, track: track, room: room, close: close)
            } else if typing {
                GridCard {
                    GridKeypad(unit: "frames", from: Int(clip.duration), most: 999) { n in
                        tool.gridTrim(clip.id, leading: false, to: Int(clip.start) + max(n, 1))
                        typing = false
                    }
                }
            }
        }
    }
}

/// A track's menu: cut, copy, paste and duplicate (every frame of it); its opacity, name,
/// highlight colour, blend mode and mask; locking and deleting it.
struct GridTrackMenu: View {
    private enum Sub {
        case highlight, blend, mask
    }

    let tool: TimelineTool
    let track: TimelineTrackInfo
    let room: CGFloat
    let close: () -> Void

    @State private var sub: Sub?

    var body: some View {
        let ids = track.clips.map(\.id)
        let locked = tool.held(track)
        HStack(alignment: .top, spacing: 6) {
            GridCard {
                GridMenuTools(
                    cut: ids.isEmpty || locked ? nil : {
                        close()
                        tool.cutFrames(ids)
                    },
                    copy: ids.isEmpty ? nil : {
                        close()
                        tool.copyFrames(ids)
                    },
                    paste: tool.clipboard.clips == 0 || locked ? nil : {
                        close()
                        tool.selectTrack(track.id)
                        tool.pasteClips()
                    },
                    duplicate: ids.isEmpty || locked ? nil : {
                        close()
                        tool.duplicateFrames(ids)
                    }
                )
                GridMenuLine()
                GridMenuSlider(label: "Opacity", value: track.opacity) { v, done in tool.setTrackOpacity(track.id, v, done: done) }
                GridRenameRow(name: track.name) { tool.renameTrack(track.id, $0) }
                GridMenuRow(icon: PcIcons.gridHighlight, label: "Highlight", chevron: true, lit: sub == .highlight) {
                    sub = sub == .highlight ? nil : .highlight
                }
                GridMenuRow(icon: PcIcons.gridBlend, label: "Blend mode", chevron: true, lit: sub == .blend) {
                    sub = sub == .blend ? nil : .blend
                }
                GridMenuRow(icon: PcIcons.gridMask, label: "Mask", chevron: true, lit: sub == .mask) {
                    sub = sub == .mask ? nil : .mask
                }
                GridMenuRow(icon: PcIcons.lock, label: track.locked ? "Unlock track" : "Lock track") { tool.toggleLocked(track) }
                GridMenuLine()
                // every frame off it, the track staying
                GridMenuRow(icon: PcIcons.gridBroom, label: "Clear track", tint: Grid.danger, dim: ids.isEmpty || locked) {
                    close()
                    tool.deleteFrames(ids)
                }
                // the last track stays
                GridMenuRow(icon: PcIcons.trash, label: "Delete track", tint: Grid.danger, dim: tool.tracks.count <= 1) {
                    close()
                    tool.deleteTrack(track.id)
                }
            }
            switch sub {
            case .highlight:
                GridCard {
                    ForEach(gridHighlights, id: \.1) { name, rgb in
                        GridChoiceRow(label: name, swatch: Color(rgb: rgb), chosen: track.label == rgb) { tool.setTrackLabel(track.id, rgb) }
                    }
                    GridChoiceRow(label: "None", swatch: .some(nil), chosen: track.label == nil) { tool.setTrackLabel(track.id, nil) }
                }
            case .blend:
                let modes = tool.canvasModel.blendModeList
                GridCard {
                    ScrollView(.vertical, showsIndicators: false) {
                        VStack(alignment: .leading, spacing: 0) {
                            ForEach(modes.indices, id: \.self) { i in
                                GridChoiceRow(label: modes[i].name, chosen: track.blend == modes[i].id) {
                                    tool.setTrackBlend(track.id, modes[i].id)
                                }
                            }
                        }
                    }
                    // no taller than the cards beside it, which hang from the same top
                    .frame(height: min(CGFloat(modes.count) * 34, min(max(room - 60, 170), 306)))
                }
            case .mask:
                let masks: [(TimelineMask, String)] = [
                    (.none, "None"), (.clipping, "Clipping mask"), (.alpha, "Alpha mask"), (.luminance, "Luminance mask"),
                ]
                GridCard {
                    ForEach(masks, id: \.1) { mask, name in
                        GridChoiceRow(label: name, chosen: track.mask == mask) { tool.setTrackMask(track.id, mask) }
                    }
                }
            case nil:
                EmptyView()
            }
        }
    }
}

/// The menu of the frames picked together: cut, copy, paste and duplicate; flipping their order,
/// turning them off or on, clearing and deleting them. The lasso stays on after each.
private struct GridGroupMenu: View {
    let tool: TimelineTool
    let ids: [UInt64]
    let close: () -> Void

    var body: some View {
        GridCard {
            GridMenuTools(
                cut: {
                    close()
                    tool.cutFrames(ids)
                },
                copy: {
                    close()
                    tool.copyFrames(ids)
                },
                paste: tool.clipboard.clips == 0 ? nil : {
                    close()
                    tool.pasteClips()
                },
                duplicate: {
                    close()
                    // the copies picked in their place
                    let made = tool.duplicateFrames(ids)
                    if !made.isEmpty {
                        tool.selecting = Set(made)
                    }
                }
            )
            GridMenuLine()
            GridMenuRow(icon: PcIcons.layers, label: "Group frames") {
                close()
                tool.makeGroup()
            }
            GridMenuRow(icon: PcIcons.gridFlip, label: "Flip frames", dim: ids.count < 2) {
                close()
                tool.flipFrames(ids)
            }
            GridMenuRow(icon: PcIcons.gridFill, label: "Fill duration", dim: !tool.canExtend(ids)) {
                close()
                tool.extend(ids)
            }
            GridMenuLine()
            // turned off, they stay where they are but neither show nor play: all of them off
            // while any is on, else all back on
            let allOff = tool.allDisabled(ids)
            GridMenuRow(icon: PcIcons.gridHighlight, label: allOff ? "Turn on" : "Turn off") {
                close()
                tool.setClipsDisabled(ids, !allOff)
            }
            GridMenuRow(icon: PcIcons.gridClear, label: "Clear frames") {
                close()
                tool.clearFrames(ids)
            }
            GridMenuRow(icon: PcIcons.trash, label: "Delete frames", tint: Grid.danger) {
                close()
                tool.deleteFrames(ids)
            }
        }
    }
}

// MARK: - Pieces

/// A menu's card: the canvas showing through it, blurred.
struct GridCard<Content: View>: View {
    var width: CGFloat = cardWidth
    @ViewBuilder let content: () -> Content

    var body: some View {
        VStack(alignment: .leading, spacing: 0) {
            content()
        }
        .padding(.vertical, 8)
        .frame(width: width, alignment: .leading)
        .background(Grid.card)
        .background(.ultraThinMaterial)
        .environment(\.colorScheme, Look.shared.light ? .light : .dark)
        .clipShape(RoundedRectangle(cornerRadius: 22, style: .continuous))
        .shadow(color: .black.opacity(0.28), radius: 18, y: 6)
        // its own touches: none close it or reach the canvas
        .contentShape(Rectangle())
        .onTapGesture {}
    }
}

struct GridMenuLine: View {
    var body: some View {
        Grid.line.opacity(0.7)
            .frame(height: 1)
            .padding(.horizontal, 14)
            .padding(.vertical, 6)
    }
}

/// A row of a menu: a glyph and what it does; dimmed where it can't.
struct GridMenuRow: View {
    var icon: PcIcon?
    let label: String
    var tint: Color?
    var chevron = false
    var dim = false
    /// What it opens is open.
    var lit = false
    let action: () -> Void

    var body: some View {
        let color = dim ? Grid.inkDim : lit ? Pc.accent : tint ?? Grid.inkBright
        HStack(spacing: 10) {
            if let icon {
                PcIconView(icon, size: 20, color: color)
                    .frame(width: 22)
            }
            Text(verbatim: label)
                .font(.system(size: 15))
                .foregroundStyle(color)
                .lineLimit(1)
            Spacer(minLength: 0)
            if chevron {
                PcIconView(PcIcons.chevronRight, size: 15, color: color)
            }
        }
        .padding(.horizontal, 16)
        .frame(height: 36)
        .contentShape(Rectangle())
        .onTapGesture {
            if !dim {
                action()
            }
        }
        .accessibilityElement()
        .accessibilityLabel(Text(verbatim: label))
        .accessibilityAddTraits(.isButton)
    }
}

/// One of a list to choose from: the one chosen ticked.
struct GridChoiceRow: View {
    let label: String
    /// A colour's swatch before its name (none given: no swatch; a nil colour: the "none" ring).
    var swatch: Color??
    let chosen: Bool
    let action: () -> Void

    var body: some View {
        HStack(spacing: 10) {
            if let swatch {
                if let swatch {
                    Circle().fill(swatch).frame(width: 18, height: 18)
                } else {
                    PcIconView(PcIcons.gridHighlight, size: 20, color: Grid.inkBright)
                        .frame(width: 18)
                }
            }
            Text(verbatim: label)
                .font(.system(size: 15))
                .foregroundStyle(Grid.inkBright)
                .lineLimit(1)
            Spacer(minLength: 0)
            if chosen {
                ZStack {
                    Circle().fill(Pc.accent).frame(width: 18, height: 18)
                    PcIconView(PcIcons.tick, size: 13, color: .white)
                }
            }
        }
        .padding(.horizontal, 16)
        .frame(height: 34)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
        .accessibilityElement()
        .accessibilityLabel(Text(verbatim: label))
        .accessibilityAddTraits(chosen ? [.isButton, .isSelected] : .isButton)
    }
}

/// A menu's first row: cut, copy, paste and duplicate, each dimmed where it can't be done.
struct GridMenuTools: View {
    let cut: (() -> Void)?
    let copy: (() -> Void)?
    let paste: (() -> Void)?
    let duplicate: (() -> Void)?

    var body: some View {
        HStack(spacing: 0) {
            tool(PcIcons.gridScissors, "Cut", cut)
            tool(PcIcons.gridCopy, "Copy", copy)
            tool(PcIcons.gridPaste, "Paste", paste)
            tool(PcIcons.duplicate, "Duplicate", duplicate)
        }
        .padding(.horizontal, 6)
    }

    private func tool(_ icon: PcIcon, _ label: String, _ action: (() -> Void)?) -> some View {
        let color = action == nil ? Grid.inkDim : Grid.inkBright
        return VStack(spacing: 3) {
            PcIconView(icon, size: 21, color: color)
            Text(verbatim: label)
                .font(.system(size: 10.5))
                .foregroundStyle(color)
                .lineLimit(1)
                .fixedSize()
        }
        .frame(maxWidth: .infinity, minHeight: 46)
        .contentShape(Rectangle())
        .onTapGesture { action?() }
        .accessibilityElement()
        .accessibilityLabel(Text(verbatim: label))
        .accessibilityAddTraits(.isButton)
    }
}

/// A slider in a menu: its name and value over a thin track, dragged anywhere along it.
struct GridMenuSlider: View {
    let label: String
    let value: Float
    /// Without its name and value over it: the track alone.
    var bare = false
    /// What its value reads as (none given: a percentage).
    var text: ((Float) -> String)?
    /// The value as it is dragged, and whether it was let go there.
    let changed: (Float, Bool) -> Void

    @State private var live: Float?

    var body: some View {
        let shown = live ?? value
        VStack(spacing: 4) {
            if !bare {
                HStack(spacing: 0) {
                    Text(verbatim: label)
                    Spacer(minLength: 0)
                    Text(verbatim: text?(shown) ?? "\(wholeNumber(shown * 100)) %")
                        .monospacedDigit()
                }
                .font(.system(size: 15))
                .foregroundStyle(Grid.inkBright)
            }
            GeometryReader { place in
                let w = max(place.size.width, 1)
                let x = CGFloat(min(max(shown, 0), 1)) * w
                ZStack(alignment: .leading) {
                    Capsule().fill(Grid.line).frame(height: 3)
                    Capsule().fill(Pc.accent).frame(width: x, height: 3)
                    Capsule().fill(Color.white).frame(width: 14, height: 8).offset(x: min(max(x - 7, 0), w - 14))
                }
                .frame(height: 22)
                .contentShape(Rectangle())
                .gesture(
                    DragGesture(minimumDistance: 0)
                        .onChanged { drag in
                            let next = Float(min(max(drag.location.x / w, 0), 1))
                            live = next
                            changed(next, false)
                        }
                        .onEnded { _ in
                            if let live {
                                changed(live, true)
                            }
                            live = nil
                        }
                )
            }
            .frame(height: 22)
        }
        .padding(.horizontal, 16)
        .padding(.top, 6)
        .padding(.bottom, 2)
    }
}

/// "Rename", which becomes a field for the new name: Return renames.
struct GridRenameRow: View {
    let name: String
    let rename: (String) -> Void

    @State private var editing: String?
    @FocusState private var focused: Bool

    var body: some View {
        if let text = editing {
            TextField("", text: Binding(get: { text }, set: { editing = $0 }))
                .font(.system(size: 15))
                .foregroundStyle(Grid.inkBright)
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
                    // the whole name chosen, so typing replaces it
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
                .frame(height: 36)
        } else {
            GridMenuRow(icon: PcIcons.gridRename, label: "Rename") { editing = name }
        }
    }
}

/// How many frames a frame lasts: a step shorter or longer, or the number typed in.
struct GridDurationRow: View {
    var label = "Frame duration"
    let frames: Int
    let locked: Bool
    /// Its number tapped opens keys to type it on, beside the menu (none given: it is typed in
    /// place).
    var keypad: (() -> Void)?
    let set: (Int) -> Void

    @State private var typed: String?
    @FocusState private var focused: Bool

    var body: some View {
        let color = locked ? Grid.inkDim : Grid.inkBright
        HStack(spacing: 0) {
            Text(verbatim: label)
                .font(.system(size: 15))
                .foregroundStyle(color)
                .lineLimit(1)
            Spacer(minLength: 6)
            step("−", by: -1, color: frames > 1 ? color : Grid.inkDim)
            Group {
                if let text = typed {
                    TextField("", text: Binding(get: { text }, set: { typed = $0.filter(\.isNumber) }))
                        .keyboardType(.numberPad)
                        .multilineTextAlignment(.center)
                        .tint(Pc.accent)
                        .submitLabel(.done)
                        .focused($focused)
                        .onSubmit { commit() }
                        .onAppear { focused = true }
                        .onChange(of: focused) { _, now in
                            if !now {
                                commit()
                            }
                        }
                } else {
                    Text(verbatim: "\(frames)")
                }
            }
            .font(.system(size: 13, weight: .medium).monospacedDigit())
            .foregroundStyle(color)
            .frame(width: 34, height: 20)
            .overlay(Capsule().strokeBorder(color.opacity(0.8), lineWidth: 1.2))
            .contentShape(Rectangle())
            .onTapGesture {
                guard !locked else { return }
                if let keypad {
                    keypad()
                } else if typed == nil {
                    typed = "\(frames)"
                }
            }
            step("+", by: 1, color: color)
        }
        .padding(.leading, 16)
        .padding(.trailing, 6)
        .frame(height: 36)
    }

    private func step(_ glyph: String, by: Int, color: Color) -> some View {
        Text(verbatim: glyph)
            .font(.system(size: 18, weight: .medium))
            .foregroundStyle(color)
            .frame(width: 28, height: 34)
            .contentShape(Rectangle())
            .onTapGesture {
                guard !locked, frames + by >= 1 else { return }
                typed = nil
                set(frames + by)
            }
    }

    private func commit() {
        guard let text = typed else { return }
        typed = nil
        if let n = Int(text), n >= 1, n != frames {
            set(min(n, 999))
        }
    }
}

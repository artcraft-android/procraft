import SwiftUI

/// The Texture Packer (Share → Texture Packer): the separate drawings on the
/// canvas as boxes over it, grey until tapped, blue while chosen. The bar under them: Cancel, how
/// many are chosen, Export, and Separation (how close parts must be to make one sprite) and
/// Padding (clear pixels round each). The sprite tapped last shows on a card to be renamed. Export
/// shares each chosen sprite as a PNG of its own, in sRGB. The canvas can be moved and zoomed with
/// a finger or two, but not drawn on.
@Observable
final class TexturePackerState {
    var boxes: [PackerBox] = []
    var separation: UInt32 = 6
    var padding: UInt32 = 0
    var chosen: Set<Int> = []
    var focused: Int?
    var thumbnail: UIImage?
    /// Names given, by the box's place (so a name stays with a box the sliders leave as it was).
    var names: [String: String] = [:]

    static func key(_ b: PackerBox) -> String {
        "\(b.x),\(b.y),\(b.width),\(b.height)"
    }
}

extension CanvasModel {
    /// Share's Texture Packer: the canvas as it shows, without its background, its drawings found.
    func openTexturePacker() {
        fadePanels()
        guard let count = try? session.texturePackerOpen(), count > 0 else {
            session.texturePackerClose()
            showNotice("There are no separate drawings to pack.", warning: true)
            return
        }
        // the boxes are drawn and tapped through where the canvas is now
        refreshView()
        let state = TexturePackerState()
        state.boxes = session.texturePackerBoxes(separation: state.separation, padding: state.padding)
        withAnimation(Motion.studioIn) { texturePacker = state }
    }

    func closeTexturePacker() {
        session.texturePackerClose()
        withAnimation(Motion.studioOut) { texturePacker = nil }
    }

    /// The sliders moved: the boxes made again. Separation changes which boxes there are, so what
    /// was chosen goes; padding only grows them, so it stays.
    func packerResized(_ state: TexturePackerState, keepChosen: Bool) {
        state.boxes = session.texturePackerBoxes(separation: state.separation, padding: state.padding)
        if !keepChosen {
            state.chosen = []
            state.focused = nil
        }
        packerFocus(state, state.focused)
    }

    /// The card shows sprite `index` (none: it goes).
    func packerFocus(_ state: TexturePackerState, _ index: Int?) {
        state.focused = index
        guard let index, state.boxes.indices.contains(index) else {
            state.thumbnail = nil
            return
        }
        state.thumbnail = UIImage(data: session.texturePackerThumbnail(place: state.boxes[index], side: 112))
    }

    func packerName(_ state: TexturePackerState, _ index: Int) -> String {
        state.names[TexturePackerState.key(state.boxes[index])] ?? session.texturePackerDefaultName(index: UInt32(index))
    }

    /// Each chosen sprite as its own PNG, in a folder named after the artwork, to share.
    func exportPacked(_ state: TexturePackerState) {
        let sprites = state.chosen.sorted().filter { state.boxes.indices.contains($0) }.map {
            PackerSprite(place: state.boxes[$0], index: UInt32($0), name: packerName(state, $0))
        }
        guard !sprites.isEmpty else { return }
        let base = exportFileName(name: session.name(), package: false)
        let dir = Sharing.exportsDir.appendingPathComponent(base + " Sprites")
        try? FileManager.default.removeItem(at: dir)
        exporting(failed: "The sprites couldn't be made.") { session in
            let paths = (try? session.texturePackerExport(dir: dir.path, sprites: sprites)) ?? []
            return paths.map { URL(fileURLWithPath: $0) }
        }
    }
}

struct TexturePackerOverlay: View {
    let model: CanvasModel
    let state: TexturePackerState
    let screen: CGSize

    /// The pinch and drag under way: the last scale and place, to move the view by what changed.
    @State private var lastScale: CGFloat?
    @State private var lastDrag: CGSize?
    @FocusState private var naming: Bool
    @State private var typed = ""

    private var pixels: CGFloat { model.pixels }

    private func outline(_ b: PackerBox) -> Path {
        let (x0, y0) = (CGFloat(b.x), CGFloat(b.y))
        let (x1, y1) = (x0 + CGFloat(b.width), y0 + CGFloat(b.height))
        let corners = [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].map { model.canvasToWindow(CGPoint(x: $0.0, y: $0.1)) }
        return Path { p in
            p.addLines(corners)
            p.closeSubpath()
        }
    }

    private func tapped(_ at: CGPoint) {
        naming = false
        guard let c = model.windowToCanvas(CGPoint(x: at.x * pixels, y: at.y * pixels)),
              let hit = model.session.texturePackerHit(boxes: state.boxes, x: Float(c.x), y: Float(c.y)).map(Int.init)
        else { return }
        if state.chosen.contains(hit) {
            state.chosen.remove(hit)
            if state.focused == hit {
                model.packerFocus(state, nil)
            }
        } else {
            state.chosen.insert(hit)
            model.packerFocus(state, hit)
            typed = model.packerName(state, hit)
        }
    }

    /// A step of the view moving: by `pan` (points) and `scale` about `pivot` (points).
    private func moveView(pivot: CGPoint, pan: CGSize, scale: CGFloat) {
        model.session.viewGesture(
            pivotX: Float(pivot.x * pixels), pivotY: Float(pivot.y * pixels),
            panX: Float(pan.width * pixels), panY: Float(pan.height * pixels), scale: Float(scale), rotation: 0
        )
        model.refreshView()
    }

    private var boxesLayer: some View {
        Canvas { context, _ in
            for (i, b) in state.boxes.enumerated() {
                let path = outline(b)
                if state.chosen.contains(i) {
                    context.fill(path, with: .color(Pc.accent.opacity(0.3)))
                    context.stroke(path, with: .color(Pc.accent), lineWidth: 2)
                } else {
                    context.fill(path, with: .color(Color(white: 0.5, opacity: 0.35)))
                    context.stroke(path, with: .color(Color(white: 0.8, opacity: 0.9)), lineWidth: 1.5)
                }
            }
        }
        .allowsHitTesting(false)
    }

    private var touches: some View {
        Color.clear
            .contentShape(Rectangle())
            .gesture(SpatialTapGesture().onEnded { tapped($0.location) })
            .simultaneousGesture(
                DragGesture(minimumDistance: 6)
                    .onChanged { drag in
                        let was = lastDrag ?? .zero
                        lastDrag = drag.translation
                        let pan = CGSize(width: drag.translation.width - was.width, height: drag.translation.height - was.height)
                        moveView(pivot: drag.location, pan: pan, scale: 1)
                    }
                    .onEnded { _ in lastDrag = nil }
            )
            .simultaneousGesture(
                MagnifyGesture()
                    .onChanged { pinch in
                        let was = lastScale ?? 1
                        lastScale = pinch.magnification
                        moveView(pivot: pinch.startLocation, pan: .zero, scale: pinch.magnification / max(was, 0.01))
                    }
                    .onEnded { _ in lastScale = nil }
            )
    }

    private var status: String {
        switch state.chosen.count {
        case 0: "Tap drawings to choose them"
        case 1: "1 sprite chosen"
        default: "\(state.chosen.count) sprites chosen"
        }
    }

    private var bar: some View {
        let chosenAny = !state.chosen.isEmpty
        return VStack(spacing: 10) {
            HStack(spacing: 0) {
                Text(verbatim: "Cancel")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.text)
                    .frame(width: 110, height: 44, alignment: .leading)
                    .contentShape(Rectangle())
                    .onTapGesture { model.closeTexturePacker() }
                Text(verbatim: status)
                    .font(.system(size: 15))
                    .foregroundStyle(Pc.textDim)
                    .lineLimit(1)
                    .frame(maxWidth: .infinity)
                Text(verbatim: "Export")
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(chosenAny ? Pc.accent : Pc.disabled)
                    .frame(width: 110, height: 44, alignment: .trailing)
                    .contentShape(Rectangle())
                    .onTapGesture {
                        naming = false
                        if chosenAny {
                            model.exportPacked(state)
                        }
                    }
            }
            HStack(spacing: 20) {
                OptionSlider(
                    label: "Separation", value: Float(state.separation - 1) / 39, shown: "\(state.separation) px", width: 300
                ) { v in
                    let s = UInt32((v * 39).rounded()) + 1
                    guard s != state.separation else { return }
                    state.separation = s
                    model.packerResized(state, keepChosen: false)
                }
                OptionSlider(
                    label: "Padding", value: Float(state.padding) / 32,
                    shown: state.padding == 0 ? "None" : "\(state.padding) px", width: 300
                ) { v in
                    let p = UInt32((v * 32).rounded())
                    guard p != state.padding else { return }
                    state.padding = p
                    model.packerResized(state, keepChosen: true)
                }
            }
        }
        .padding(.horizontal, 24)
        .padding(.top, 10)
        .frame(width: 668, height: 130, alignment: .top)
        .background(RoundedRectangle(cornerRadius: 33, style: .circular).fill(Pc.panel))
        .contentShape(Rectangle())
        .onTapGesture {}
    }

    /// The sprite tapped last: its picture, its size, and its name to change.
    @ViewBuilder private var card: some View {
        if let i = state.focused, state.boxes.indices.contains(i) {
            let b = state.boxes[i]
            HStack(spacing: 12) {
                Group {
                    if let picture = state.thumbnail {
                        Image(uiImage: picture).resizable().interpolation(.medium).scaledToFit()
                    } else {
                        Color.clear
                    }
                }
                .frame(width: 56, height: 56)
                .background(RoundedRectangle(cornerRadius: 8, style: .circular).fill(Pc.row))
                VStack(alignment: .leading, spacing: 6) {
                    Text(verbatim: "Sprite \(i + 1) · \(b.width) × \(b.height) px")
                        .font(.system(size: 13))
                        .foregroundStyle(Pc.textDim)
                    TextField("", text: $typed)
                        .font(.system(size: 16))
                        .foregroundStyle(Pc.text)
                        .autocorrectionDisabled()
                        .textInputAutocapitalization(.never)
                        .submitLabel(.done)
                        .focused($naming)
                        .padding(.horizontal, 10)
                        .frame(width: 240, height: 32)
                        .background(RoundedRectangle(cornerRadius: 8, style: .circular).fill(Pc.row))
                        .onSubmit { rename(i) }
                        .onChange(of: naming) { _, now in
                            if !now {
                                rename(i)
                            }
                        }
                }
            }
            .padding(14)
            .background(RoundedRectangle(cornerRadius: 22, style: .circular).fill(Pc.panel))
            .contentShape(Rectangle())
            .onTapGesture {}
        }
    }

    /// The name typed kept for sprite `index` (an empty one going back to what it was).
    private func rename(_ index: Int) {
        guard state.boxes.indices.contains(index) else { return }
        let name = typed.trimmingCharacters(in: .whitespacesAndNewlines)
        if name.isEmpty {
            typed = model.packerName(state, index)
        } else {
            state.names[TexturePackerState.key(state.boxes[index])] = name
        }
    }

    var body: some View {
        ZStack(alignment: .bottom) {
            touches
            boxesLayer
            VStack(spacing: 12) {
                card
                bar
            }
            .padding(.bottom, 24)
        }
        .frame(width: screen.width, height: screen.height)
        .onAppear {
            if let shown = state.focused, state.boxes.indices.contains(shown) {
                typed = model.packerName(state, shown)
            }
        }
        .onChange(of: state.focused) { _, now in
            if let now, state.boxes.indices.contains(now) {
                typed = model.packerName(state, now)
            }
        }
    }
}

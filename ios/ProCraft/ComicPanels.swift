import SwiftUI

/// Comic Panels (Actions → Canvas): the canvas made a comic page the first time (one panel inside
/// a margin, white with a black border), then the page fitted under the tool's header with each
/// panel outlined. A line dragged across panels cuts each in two, straight across or down when near
/// it, with a gutter between. With Move chosen, a gutter dragged moves the panels' edges on both
/// sides of it (the page's edge alone, its margin). A pinch zooms. Each panel is a group in the
/// layers: its frame (locked, its border the frame's outline style) and the art above it, clipped
/// to it.
extension CanvasModel {
    /// Opens Comic Panels, making the canvas a page first if it has no panels yet.
    func openComicPanels() {
        endQuickShape()
        fadePanels()
        if session.comicPanels().isEmpty {
            guard session.addComicPage() != nil else {
                showNotice("Comic panels can't be added to this canvas.", warning: true)
                return
            }
            refresh()
        }
        readComicPanels()
        withAnimation(Motion.studioIn) { cuttingPanels = true }
    }

    /// The whole page fitted under the header in a window `size` (points).
    func fitComicPage(_ size: CGSize) {
        let (side, top) = (24 * pixels, (comicHeaderHeight + 16) * pixels)
        session.panelToolFit(
            left: Float(side), top: Float(top), right: Float(size.width * pixels - side), bottom: Float(size.height * pixels - side)
        )
        refreshView()
    }

    func closeComicPanels() {
        withAnimation(Motion.studioOut) { cuttingPanels = false }
        session.fitView()
        refreshView()
    }

    /// The panels' shapes read again (a cut, an undo).
    func readComicPanels() {
        let shapes = session.comicPanels().map(\.points)
        if shapes != panelShapes {
            panelShapes = shapes
        }
    }

    /// Cuts the panels the line from `a` to `b` (points) crosses.
    func cutComicPanels(from a: CGPoint, to b: CGPoint) {
        let cut = session.cutPanelsOnScreen(
            ax: Float(a.x * pixels), ay: Float(a.y * pixels), bx: Float(b.x * pixels), by: Float(b.y * pixels)
        )
        if cut > 0 {
            refresh()
        }
    }
}

/// The header's height, points (as the crop tool's).
private let comicHeaderHeight: CGFloat = 85

/// Comic Panels over the canvas: the panels outlined, the cut being dragged, and the header.
struct PanelCutter: View {
    let model: CanvasModel

    /// The line being dragged (points), while one is.
    @State private var line: (from: CGPoint, to: CGPoint)?
    /// The pinch under way: its last scale.
    @State private var lastScale: CGFloat?
    /// A pinch came during the drag: it cuts nothing.
    @State private var pinched = false
    /// Move is chosen in the header: a drag takes a gutter (or the page's edge) instead of cutting.
    @State private var moving = false
    /// Moving, whether the drag under way took a gutter (none until it is asked).
    @State private var grabbed: Bool?

    /// (`p`, points) as the core takes it: window pixels.
    private func px(_ p: CGPoint) -> (Float, Float) {
        (Float(p.x * pixels), Float(p.y * pixels))
    }

    private func dragChanged(_ drag: DragGesture.Value) {
        guard moving else {
            line = pinched ? nil : (drag.startLocation, drag.location)
            return
        }
        let session = model.session
        if grabbed == nil {
            let (x, y) = px(drag.startLocation)
            grabbed = !pinched && session.gutterBegin(x: x, y: y)
        }
        if grabbed == true {
            let (x, y) = px(drag.location)
            _ = session.gutterDrag(x: x, y: y)
            model.readComicPanels()
        }
    }

    private func dragEnded(_ drag: DragGesture.Value) {
        if grabbed == true {
            model.session.gutterEnd(keep: !pinched)
            model.refresh()
        } else if !moving, !pinched {
            model.cutComicPanels(from: drag.startLocation, to: drag.location)
        }
        line = nil
        grabbed = nil
        pinched = false
    }

    private var pixels: CGFloat { model.pixels }

    private func outline(_ shape: [Float]) -> Path {
        let corners = stride(from: 0, to: shape.count - 1, by: 2).map {
            model.canvasToWindow(CGPoint(x: CGFloat(shape[$0]), y: CGFloat(shape[$0 + 1])))
        }
        return Path { p in
            p.addLines(corners)
            p.closeSubpath()
        }
    }

    /// The cut as it would be made: straight across or down when near it.
    private func cutLine(_ line: (from: CGPoint, to: CGPoint)) -> Path? {
        let ends = model.session.panelCutOnScreen(
            ax: Float(line.from.x * pixels), ay: Float(line.from.y * pixels),
            bx: Float(line.to.x * pixels), by: Float(line.to.y * pixels)
        )
        guard ends.count == 4 else { return nil }
        return Path { p in
            p.move(to: CGPoint(x: CGFloat(ends[0]) / pixels, y: CGFloat(ends[1]) / pixels))
            p.addLine(to: CGPoint(x: CGFloat(ends[2]) / pixels, y: CGFloat(ends[3]) / pixels))
        }
    }

    var body: some View {
        GeometryReader { place in
            cutter
                .onAppear { model.fitComicPage(place.size) }
        }
    }

    private var cutter: some View {
        ZStack(alignment: .topLeading) {
            Canvas { context, _ in
                for shape in model.panelShapes {
                    context.stroke(outline(shape), with: .color(Pc.accent.opacity(0.8)), style: StrokeStyle(lineWidth: 1.5, dash: [6, 4]))
                }
                if let line, let path = cutLine(line) {
                    context.stroke(path, with: .color(.white), style: StrokeStyle(lineWidth: 5, lineCap: .round))
                    context.stroke(path, with: .color(Pc.accent), style: StrokeStyle(lineWidth: 3, lineCap: .round))
                }
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 4)
                    .onChanged(dragChanged)
                    .onEnded(dragEnded)
            )
            .simultaneousGesture(
                MagnifyGesture()
                    .onChanged { pinch in
                        pinched = true
                        line = nil
                        let was = lastScale ?? 1
                        lastScale = pinch.magnification
                        model.session.viewGesture(
                            pivotX: Float(pinch.startLocation.x * pixels), pivotY: Float(pinch.startLocation.y * pixels),
                            panX: 0, panY: 0, scale: Float(pinch.magnification / max(was, 0.01)), rotation: 0
                        )
                        model.refreshView()
                    }
                    .onEnded { _ in lastScale = nil }
            )
            VStack(spacing: 0) {
                ToolHeader(
                    title: "Comic Panels",
                    buttons: [
                        ToolHeaderButton(label: "Cut", chosen: !moving, action: { moving = false }),
                        ToolHeaderButton(label: "Move", chosen: moving, action: { moving = true }),
                        ToolHeaderButton(label: "Undo", action: { model.undo() }),
                    ],
                    onDone: { model.closeComicPanels() }
                )
                Text(verbatim: moving ? "Drag a gutter, or the page's edge, to move it" : "Drag across panels to cut them")
                    .font(.system(size: 15))
                    .foregroundStyle(Pc.textDim)
                    .padding(.top, 8)
                    .allowsHitTesting(false)
            }
        }
    }
}

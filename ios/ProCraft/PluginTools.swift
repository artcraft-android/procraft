import SwiftUI

/// A plugin's tool (Actions → Canvas), such as Comic Panels: the canvas under the tool's header
/// (the whole of it fitted there, for a tool that asks), with what the plugin shows drawn over it.
/// A drag's touches go to the plugin, which answers with what to show and its edits to the layers
/// (each kept one undo step); the header's modes, Undo and Done are the app's, and a pinch zooms,
/// cancelling the drag under way. With the plugin off, what it made stays as it was made: Comic
/// Panels' panels are groups, each a frame and the art clipped to it.
extension CanvasModel {
    /// Opens `tool`, which may change the layers as it opens (Comic Panels makes a page).
    func openPluginTool(_ tool: PluginTool) {
        endQuickShape()
        fadePanels()
        let shown = session.pluginToolOpen(plugins: PluginsModel.shared.store, key: tool.key)
        if let said = shown.said {
            showNotice(said, warning: shown.closed)
        }
        if shown.changed {
            refresh()
        }
        guard !shown.closed else { return }
        toolShapes = shown.shapes
        toolMode = 0
        withAnimation(Motion.studioIn) { pluginTool = tool }
    }

    /// The whole canvas fitted under the header in a window `size` (points).
    func fitToolCanvas(_ size: CGSize) {
        let (side, top) = (24 * pixels, (toolHeaderHeight + 16) * pixels)
        session.pluginToolFit(
            left: Float(side), top: Float(top), right: Float(size.width * pixels - side), bottom: Float(size.height * pixels - side)
        )
        refreshView()
    }

    func closePluginTool() {
        session.pluginToolClose()
        let fitted = pluginTool?.fit == true
        withAnimation(Motion.studioOut) { pluginTool = nil }
        toolShapes = []
        if fitted {
            session.fitView()
            refreshView()
        }
        refresh()
    }

    /// What the open tool shows, asked again (an undo, a redo).
    func readPluginTool() {
        let shown = session.pluginToolAgain()
        if shown.closed {
            withAnimation(Motion.studioOut) { pluginTool = nil }
            toolShapes = []
        } else if shown.shapes != toolShapes {
            toolShapes = shown.shapes
        }
    }

    /// A touch at `at` (points) on the open tool, in its mode chosen: what it shows, and the
    /// layers read again if it changed them as it lets go.
    func touchPluginTool(_ touch: PluginToolTouch, at: CGPoint) {
        let shown = session.pluginToolTouch(touch: touch, x: Float(at.x * pixels), y: Float(at.y * pixels), mode: toolMode)
        toolShapes = shown.shapes
        if let said = shown.said {
            showNotice(said, warning: true)
        }
        if shown.closed {
            withAnimation(Motion.studioOut) { pluginTool = nil }
            toolShapes = []
        }
        if shown.changed, touch != .moved {
            refresh()
        }
    }
}

/// The header's height, points (as the crop tool's).
private let toolHeaderHeight: CGFloat = 85

/// A plugin's tool over the canvas: what it shows, and its header.
struct PluginToolView: View {
    let model: CanvasModel
    let tool: PluginTool

    /// A drag under way: whether a pinch came during it (it is cancelled, and nothing more goes).
    @State private var dragging = false
    @State private var pinched = false
    /// The pinch under way: its last scale.
    @State private var lastScale: CGFloat?

    private func dragChanged(_ drag: DragGesture.Value) {
        guard !pinched else { return }
        if !dragging {
            dragging = true
            model.touchPluginTool(.down, at: drag.startLocation)
        }
        model.touchPluginTool(.moved, at: drag.location)
    }

    private func dragEnded(_ drag: DragGesture.Value) {
        if dragging, !pinched {
            model.touchPluginTool(.up, at: drag.location)
        }
        dragging = false
        pinched = false
    }

    private var pixels: CGFloat { model.pixels }

    private func path(_ shape: PluginToolShape) -> Path {
        let p = shape.points
        let corners = stride(from: 0, to: p.count - 1, by: 2).map {
            model.canvasToWindow(CGPoint(x: CGFloat(p[$0]), y: CGFloat(p[$0 + 1])))
        }
        return Path { path in
            path.addLines(corners)
            if shape.closed {
                path.closeSubpath()
            }
        }
    }

    var body: some View {
        GeometryReader { place in
            content
                .onAppear {
                    if tool.fit {
                        model.fitToolCanvas(place.size)
                    }
                }
        }
    }

    private var content: some View {
        ZStack(alignment: .topLeading) {
            Canvas { context, _ in
                for shape in model.toolShapes {
                    let line = path(shape)
                    if shape.look == 0 {
                        context.stroke(line, with: .color(Pc.accent.opacity(0.8)), style: StrokeStyle(lineWidth: 1.5, dash: [6, 4]))
                    } else {
                        context.stroke(line, with: .color(.white), style: StrokeStyle(lineWidth: 5, lineCap: .round))
                        context.stroke(line, with: .color(Pc.accent), style: StrokeStyle(lineWidth: 3, lineCap: .round))
                    }
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
                        if dragging, !pinched {
                            model.touchPluginTool(.cancelled, at: pinch.startLocation)
                        }
                        pinched = true
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
                    title: tool.label,
                    buttons: tool.modes.indices.map { i in
                        ToolHeaderButton(label: tool.modes[i].label, chosen: model.toolMode == UInt32(i), action: { model.toolMode = UInt32(i) })
                    } + [ToolHeaderButton(label: "Undo", action: { model.undo() })],
                    onDone: { model.closePluginTool() }
                )
                let mode = Int(model.toolMode)
                if tool.modes.indices.contains(mode), !tool.modes[mode].hint.isEmpty {
                    Text(verbatim: tool.modes[mode].hint)
                        .font(.system(size: 15))
                        .foregroundStyle(Pc.textDim)
                        .padding(.top, 8)
                        .allowsHitTesting(false)
                }
            }
        }
    }
}

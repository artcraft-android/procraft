import SwiftUI

// The timeline's sync markers: dropped while it plays ("Mark" in the bar), shown as flags on the
// ruler with a faint line down through the tracks, to time drawings to a beat or a line.

/// The markers' colour: amber, apart from the playhead's blue.
let markerFill = Color(argb: 0xFFF2_A93B)

/// The markers' flags along the ruler. A flag is tapped to go to its frame, dragged along to move
/// it, or held for a menu that takes it away.
struct TimelineMarkerFlags: View {
    let tool: TimelineTool
    /// How many frames the timeline lasts, a frame's width, and the ruler's height.
    let frames: Int
    let pitch: CGFloat
    let height: CGFloat

    @State private var dragging: (frame: UInt32, dx: CGFloat)?

    var body: some View {
        ZStack(alignment: .topLeading) {
            ForEach(tool.markers, id: \.self) { m in
                let dx = dragging?.frame == m ? dragging?.dx ?? 0 : 0
                Flag()
                    .fill(markerFill)
                    .frame(width: 12, height: 14)
                    .shadow(color: .black.opacity(dragging?.frame == m ? 0.35 : 0), radius: 4)
                    .frame(width: 26, height: height, alignment: .bottom)
                    .contentShape(Rectangle())
                    .offset(x: (CGFloat(m) + 0.5) * pitch - 13 + dx)
                    .onTapGesture { tool.goTo(m) }
                    .highPriorityGesture(
                        DragGesture(minimumDistance: 4)
                            .onChanged { dragging = (m, $0.translation.width) }
                            .onEnded { drag in
                                dragging = nil
                                let to = Int(m) + Int((drag.translation.width / pitch).rounded())
                                tool.moveMarker(m, to: UInt32(min(max(to, 0), max(frames - 1, 0))))
                            }
                    )
                    .contextMenu {
                        Button(role: .destructive) {
                            tool.removeMarker(m)
                        } label: {
                            Label("Delete Marker", systemImage: "trash")
                        }
                    }
            }
        }
        .frame(height: height, alignment: .topLeading)
    }
}

/// A faint line down from each marker through the tracks, under the clips.
struct TimelineMarkerLines: View {
    let markers: [UInt32]
    let pitch: CGFloat
    let height: CGFloat

    var body: some View {
        Canvas { ctx, _ in
            for m in markers {
                let x = (CGFloat(m) + 0.5) * pitch
                ctx.fill(Path(CGRect(x: x - 0.5, y: 0, width: 1, height: height)), with: .color(markerFill.opacity(0.55)))
            }
        }
        .frame(height: height)
        .allowsHitTesting(false)
    }
}

/// A pennant pointing down at its frame.
private struct Flag: Shape {
    func path(in r: CGRect) -> Path {
        var p = Path()
        let notch = r.height * 0.32
        p.move(to: CGPoint(x: r.minX, y: r.minY))
        p.addLine(to: CGPoint(x: r.maxX, y: r.minY))
        p.addLine(to: CGPoint(x: r.maxX, y: r.maxY - notch))
        p.addLine(to: CGPoint(x: r.midX, y: r.maxY))
        p.addLine(to: CGPoint(x: r.minX, y: r.maxY - notch))
        p.closeSubpath()
        return p
    }
}

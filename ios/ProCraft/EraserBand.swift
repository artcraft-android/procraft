import SwiftUI

/// Each of the Eraser band's segments: room for "Whole Stroke" and "To Crossing".
private let eraserSegmentWidth: CGFloat = 84

/// Under this, the band leaves its title out for the segmented control.
private let eraserRoomForTitle: CGFloat = 352

/// Under this, the segments take shorter names.
private let eraserRoomForNames: CGFloat = 268

/// The Eraser band under the top bar, sliding down from behind it as ColorDrop's does: while the
/// Erase tool is up on a vector layer, how the eraser works there — taking what its brush covers
/// (Subtract), whole strokes, or a stroke up to where it crosses others. It stays while erasing.
struct EraserBand: View {
    let model: CanvasModel
    let width: CGFloat

    /// How far down it is, kept while it slides away.
    @State private var shown: CGFloat = 0

    private static let modes: [VectorEraser] = [.subtract, .wholeStroke, .toCrossing]

    private var light: Bool {
        Look.shared.light
    }

    private var blue: Color {
        Pc.tinted(Color(argb: light ? 0xFF0A_8CFD : 0xFF00_76FD))
    }

    private var ink: Color {
        Color(argb: light ? 0xFF22_2222 : 0xFFC5_C5C5)
    }

    var body: some View {
        let on = model.eraserBandShown
        let height: CGFloat = 32
        let named = width >= eraserRoomForNames
        HStack(spacing: 13) {
            // on a narrow screen the control has the band to itself
            if width >= eraserRoomForTitle {
                Text(verbatim: "Eraser")
                    .font(.system(size: 16.5, weight: .bold))
                    .foregroundStyle(ink)
                    .lineLimit(1)
            }
            BandSegments(
                labels: named ? ["Subtract", "Whole Stroke", "To Crossing"] : ["Subtract", "Stroke", "Crossing"],
                chosen: Self.modes.firstIndex(of: model.vectorEraser) ?? 0,
                width: named ? eraserSegmentWidth : bandSegmentWidth, ink: ink, blue: blue
            ) { model.chooseVectorEraser(Self.modes[$0]) }
        }
        .frame(width: width, height: height)
        .background(
            Rectangle()
                .fill(Color(argb: light ? 0xD6EE_EEEE : 0xD63B_3B3B))
                .allowsHitTesting(false)
        )
        .offset(y: -(1 - shown) * height)
        .frame(width: width, height: height, alignment: .top)
        .clipped()
        .offset(y: Pc.barHeight)
        .opacity(shown > 0 ? 1 : 0)
        .allowsHitTesting(on)
        .onChange(of: on, initial: true) { _, on in
            withAnimation(.timingCurve(0.42, 0, 0.58, 1, duration: 0.3)) {
                shown = on ? 1 : 0
            }
        }
    }
}

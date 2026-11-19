import SwiftUI

/// The colour panel's row for an HDR artwork: how many stops above white the paint colour is lit,
/// from the colour as chosen ("White") up to the HDR limit's most. The eyedropper sets it to the
/// light it finds.
struct ColorLightRow: View {
    let model: CanvasModel

    var body: some View {
        let stops = model.colorStops
        HStack(spacing: 0) {
            Text(verbatim: "HDR")
                .font(.system(size: 15))
                .foregroundStyle(Pc.panelTitle)
                .frame(width: 44, alignment: .leading)
                .padding(.leading, 13)
            // from the colour towards white: the light above white can't show on a track
            SliderRow(value: stops / hdrLimitMost, colours: [model.colorRgb(), 0xFFFFFF]) { v in
                model.setColorStops(((v * hdrLimitMost) * 10).rounded() / 10)
            }
            Text(verbatim: hdrLimitShown(stops))
                .font(.system(size: 15))
                .foregroundStyle(Pc.panelTitle)
                .lineLimit(1)
                .frame(width: 78, alignment: .trailing)
                .padding(.trailing, 13)
        }
        .frame(height: 44)
        .background(RoundedRectangle(cornerRadius: 10, style: .circular).fill(Pc.valueGroup))
        .padding(.horizontal, 8)
        .padding(.top, 8)
    }
}

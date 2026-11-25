import SwiftUI

/// An adjustment clip's values at the playhead, a plain row each: a row slid keys the whole
/// adjustment there as it moves (one undo step when let go), a new key easing as chosen.
struct AdjustmentKeyRows: View {
    let model: CanvasModel
    let tool: TimelineTool
    let clip: UInt64
    let frame: UInt32

    /// The row being slid and its value, before it is let go.
    @State private var live: (index: Int, value: Float)?

    var body: some View {
        // read again whenever the timeline is
        let _ = tool.revision
        let all = model.session.timelineAdjustmentValues(id: clip)
        // curves' keyed values are its exposure's, an HDR canvas's only
        let values = model.isHdr || all.first?.unit != .stops ? all : []
        if !values.isEmpty {
            TlSliderGroup {
                ForEach(Array(values.enumerated()), id: \.offset) { index, v in
                    let shown = live.flatMap { $0.index == index ? $0.value : nil } ?? v.value
                    TlSlider(label: v.name, value: Self.said(v, shown), share: Self.share(v, shown),
                             onEnd: { end(index) }) { s in
                        let value = v.low + s * (v.high - v.low)
                        live = (index, value)
                        tool.stopPlaying()
                        _ = model.session.setTimelineAdjustmentValue(id: clip, index: UInt32(index), value: value,
                                                                     easing: tool.keyEasing, done: false)
                    }
                }
            }
            .padding(.top, 10)
            .onChange(of: frame) { live = nil }
        }
    }

    /// A row let go: its key is one undo step.
    private func end(_ index: Int) {
        guard let l = live, l.index == index else { return }
        _ = model.session.setTimelineAdjustmentValue(id: clip, index: UInt32(index), value: l.value,
                                                     easing: tool.keyEasing, done: true)
        live = nil
        model.refresh()
    }

    private static func share(_ v: AdjustmentValue, _ value: Float) -> Float {
        let span = v.high - v.low
        return span > 0 ? (value - v.low) / span : 0
    }

    private static func said(_ v: AdjustmentValue, _ value: Float) -> String {
        switch v.unit {
        case .plain: String(format: "%.2f", value)
        case .pixels: "\(Int(value.rounded())) px"
        case .percent: "\(Int((value * 100).rounded())) %"
        case .degrees: "\(Int((value * 180 / .pi).rounded()))°"
        case .stops: String(format: "%+.1f stops", value)
        }
    }
}

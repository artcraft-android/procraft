import SwiftUI

// The camera's lane in the tracks panel, just under the ruler: a diamond on each frame the camera
// has a key. The camera is keyed by dragging its frame over the canvas (the Camera page); here its
// keys are seen along the timeline and gone to.

/// How tall the camera's lane is.
let cameraLaneHeight: CGFloat = 26

private var laneFill: Color { Look.shared.light ? Color(argb: 0xFFDD_DDDD) : Color(argb: 0xFF2F_2F2F) }
private var inkDim: Color { Look.shared.light ? Color(argb: 0x8039_3939) : Color(argb: 0x80C5_C5C5) }
private let rowLineFill = Color(argb: 0x09FF_FFFF)

/// The lane's name in the track names' column; a tap opens the camera's page.
struct CameraLaneHeader: View {
    let tool: TimelineTool

    var body: some View {
        HStack(spacing: 8) {
            Text(verbatim: "Camera")
                .font(.system(size: 13, weight: tool.cameraOpen ? .semibold : .regular))
                .foregroundStyle(tool.cameraOpen ? Pc.accent : inkDim)
            Spacer(minLength: 0)
            if !tool.cameraKeys.isEmpty {
                Text(verbatim: tool.cameraKeys.count == 1 ? "1 key" : "\(tool.cameraKeys.count) keys")
                    .font(.system(size: 11).monospacedDigit())
                    .foregroundStyle(inkDim)
            }
        }
        .padding(.leading, 14)
        .padding(.trailing, 12)
        .frame(height: cameraLaneHeight)
        .background(laneFill)
        .overlay(alignment: .bottom) { rowLineFill.frame(height: 1) }
        .contentShape(Rectangle())
        .onTapGesture { openCamera(tool) }
    }
}

/// The camera's keys along the timeline, a lane like the key lanes (TimelineKeyLanes.swift): a
/// key's mark is tapped to pick it (the playhead going to it), with the clips' keys picked below
/// or alone, and dragged to move the keys picked; the lane elsewhere, to go to that frame. The
/// camera's page opens from the lane's name.
struct CameraLane: View {
    let tool: TimelineTool
    let width: CGFloat
    let pitch: CGFloat
    let frames: Int

    var body: some View {
        LaneRow(tool: tool, keys: tool.lanes.cameraKeys, spans: [], width: width, pitch: pitch, frames: frames,
                height: cameraLaneHeight, fill: laneFill)
    }
}

/// The camera's page open, the others shut.
private func openCamera(_ tool: TimelineTool) {
    tool.cameraOpen = true
    tool.keysOpen = false
    tool.clipOpen = false
    tool.settingsOpen = false
}

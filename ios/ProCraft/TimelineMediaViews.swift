import PhotosUI
import SwiftUI
import UniformTypeIdentifiers

// Sounds and videos in the timeline panel: what a sound or video clip shows on its lane (its
// waveform, or a strip of film), its own page (level, nudge, the part of the file it plays,
// where a video sits on the canvas, the animatic detector), and "+ Media" in the header.

private var ink: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }
private var inkDim: Color { Look.shared.light ? Color(argb: 0x8039_3939) : Color(argb: 0x80C5_C5C5) }
private var soundWave: Color { Look.shared.light ? Color(argb: 0xFF2F_6E6A) : Color(argb: 0xFF8F_D6CF) }

/// A sound's waveform from `from` seconds into it, for `seconds`, mirrored about the middle;
/// starting `lead` points in (a nudged sound).
struct Waveform: Shape {
    let peaks: [Float]
    let from: Double
    let seconds: Double
    var lead: CGFloat = 0

    func path(in rect: CGRect) -> Path {
        var p = Path()
        let perSecond = Double(mediaPeaksPerSecond())
        guard !peaks.isEmpty, seconds > 0, rect.width > lead else { return p }
        let mid = rect.midY
        let width = rect.width - lead
        // a bar every three points, each the loudest peak it covers
        let bars = max(Int(width / 3), 1)
        for b in 0 ..< bars {
            let t0 = from + seconds * Double(b) / Double(bars)
            let t1 = from + seconds * Double(b + 1) / Double(bars)
            let i0 = max(Int(t0 * perSecond), 0)
            let i1 = min(max(Int(t1 * perSecond), i0 + 1), peaks.count)
            guard i0 < i1 else { continue }
            let loud = CGFloat(peaks[i0 ..< i1].max() ?? 0)
            let h = max(loud * rect.height / 2, 0.5)
            let x = rect.minX + lead + CGFloat(b) * width / CGFloat(bars)
            p.addRoundedRect(in: CGRect(x: x, y: mid - h, width: 2, height: h * 2), cornerSize: CGSize(width: 1, height: 1))
        }
        return p
    }
}

/// Where a video sits on the canvas.
enum Placement: CaseIterable {
    case fit, fill, actual

    var label: String {
        switch self {
        case .fit: "Fit Canvas"
        case .fill: "Fill Canvas"
        case .actual: "Actual Size"
        }
    }

    func rect(_ c: TimelineMediaClip, canvas: (Float, Float)) -> [Float]? {
        let (w, h) = (Float(c.width), Float(c.height))
        guard w > 0, h > 0 else { return nil }
        let scale: Float = switch self {
        case .fit: min(canvas.0 / w, canvas.1 / h)
        case .fill: max(canvas.0 / w, canvas.1 / h)
        case .actual: 1
        }
        let (sw, sh) = (w * scale, h * scale)
        return [(canvas.0 - sw) / 2, (canvas.1 - sh) / 2, sw, sh]
    }
}

/// The whole file as a strip, the part the clip plays lit: dragged along, the clip plays another
/// part of its file where it is (a slip).
struct SlipStrip: View {
    let media: TimelineMediaTool
    let c: TimelineMediaClip
    /// How long the clip plays, seconds.
    let plays: Double

    @State private var from: Double?

    var body: some View {
        GeometryReader { place in
            let w = place.size.width
            let whole = max(c.mediaSeconds, 0.001)
            let start = from ?? c.trimStart
            let x = CGFloat(start / whole) * w
            let lit = max(CGFloat(plays / whole) * w, 4)
            ZStack(alignment: .leading) {
                RoundedRectangle(cornerRadius: 10, style: .circular).fill(Pc.actionsGroup)
                if c.kind == .audio {
                    Waveform(peaks: media.peaks(c.media), from: 0, seconds: whole)
                        .fill(inkDim)
                        .padding(.vertical, 8)
                }
                RoundedRectangle(cornerRadius: 8, style: .circular)
                    .fill(Pc.accent.opacity(0.25))
                    .overlay(RoundedRectangle(cornerRadius: 8, style: .circular).strokeBorder(Pc.accent, lineWidth: 2))
                    .frame(width: min(lit, w))
                    .offset(x: min(x, max(w - lit, 0)))
                if c.kind == .audio {
                    Waveform(peaks: media.peaks(c.media), from: start, seconds: min(plays, whole - start))
                        .fill(soundWave)
                        .frame(width: min(lit, w))
                        .padding(.vertical, 8)
                        .offset(x: min(x, max(w - lit, 0)))
                        .allowsHitTesting(false)
                }
            }
            .contentShape(Rectangle())
            .gesture(
                DragGesture(minimumDistance: 1)
                    .onChanged { drag in
                        let moved = c.trimStart + Double(drag.translation.width / max(w, 1)) * whole
                        from = min(max(moved, 0), max(whole - plays, 0))
                    }
                    .onEnded { _ in
                        if let f = from {
                            media.slip(c.clip, to: f, done: true)
                        }
                        from = nil
                    }
            )
        }
        .frame(height: 54)
        .padding(.horizontal, 4)
    }
}


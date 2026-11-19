import SwiftUI
import UIKit

// The full view's playback and length: the looped frames drawn on the ruler (held, then dragged
// across it) and their ends dragged; the timeline's end dragged on the ruler; stepping to the
// previous or next drawing; and the Frames page, opened from the frame counter or the end grip,
// with the length fitted to the clips, frames added before or after, the frames showing or the
// open group looped.

private var playInk: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }
private var playInkDim: Color { Look.shared.light ? Color(argb: 0x8039_3939) : Color(argb: 0x80C5_C5C5) }

extension TimelineTool {
    private var session: CanvasSession { canvasModel.session }

    /// One past the last frame any clip shows on: where Fit to Content puts the end.
    var contentEnd: UInt32 { session.timelineContentEnd() }

    /// One past the last frame any clip in the rows reaches, from the panel's own reading: the
    /// lanes reach past it so that clips parked past the timeline's end can still be reached.
    var clipsEnd: Int {
        tracks.lazy.flatMap(\.clips).map { Int($0.start) + Int($0.duration) }.max() ?? 0
    }

    /// The Frames page open over the panel, the other pages closed.
    func openFrames() {
        keysOpen = false
        clipOpen = false
        settingsOpen = false
        cameraOpen = false
        framesOpen.toggle()
    }

    /// The timeline `frames` long (shorter than its clips, if so: what is past the end stays,
    /// shaded): one undo step.
    func setLength(_ frames: UInt32) {
        stopPlaying()
        if session.setTimelineLength(frames: frames) {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        canvasModel.refresh()
    }

    /// The timeline as long as its clips: one undo step.
    func fitToContent() {
        stopPlaying()
        _ = session.fitTimelineToContent()
        canvasModel.refresh()
    }

    /// `count` frames added before the first frame (everything moving along) or after the last:
    /// one undo step.
    func addFrames(atStart: Bool, count: UInt32) {
        stopPlaying()
        _ = session.addTimelineFrames(atStart: atStart, count: count)
        canvasModel.refresh()
    }

    /// The playhead to the start of the drawing before or after it on the track drawn on (a group
    /// counts as a drawing; sounds and videos don't).
    func stepDrawing(_ by: Int) {
        guard let clips = track?.clips else { return }
        let at = frame
        let starts = clips.filter { $0.kind != .audio && $0.kind != .video }.map(\.start)
        let next = by > 0 ? starts.filter { $0 > at }.min() : starts.filter { $0 < at }.max()
        if let next {
            goTo(next)
            media.blip(next)
        }
    }

    /// Playing goes round the frames the full view shows, kept within the timeline.
    func loopShownFrames() {
        guard let shown = shownFrames(), let duration = info?.duration else { return }
        let start = min(shown.lowerBound, duration - 1)
        let end = min(shown.upperBound, duration)
        if end > start + 1 {
            setLoop(TimelineLoop(start: start, end: end))
        }
    }

    /// Playing goes round group `g`'s frames (as the timeline shows them).
    func loopGroup(_ g: TimelineClipInfo) {
        setLoop(TimelineLoop(start: g.start, end: g.start + g.duration))
    }
}

// MARK: - The ruler

/// The ruler held a moment without moving, then dragged: the looped frames drawn from where it
/// was held to where the finger is, set when it lifts (one undo step). Read from the ruler's own
/// scrub drag, which stands aside once the hold is taken.
final class TimelineRulerHold {
    private var start: CGPoint?
    private var from: UInt32 = 0
    private var moved = false
    private var holding = false
    private var waiting: Task<Void, Never>?

    /// The scrub drag moved to `location` (frame `frame`). True once the hold is taken: the drag
    /// draws a loop rather than scrubbing.
    func changed(_ location: CGPoint, frame: UInt32, tool: TimelineTool) -> Bool {
        guard let start else {
            start = location
            from = frame
            moved = false
            holding = false
            waiting = Task { @MainActor [weak self, weak tool] in
                try? await Task.sleep(for: .milliseconds(400))
                guard let self, let tool, !Task.isCancelled, self.start != nil, !self.moved, !tool.isPlaying else { return }
                self.holding = true
                tool.endScrub(self.from)
                tool.loopDraft = TimelineLoop(start: self.from, end: self.from + 1)
                UIImpactFeedbackGenerator(style: .medium).impactOccurred()
            }
            return false
        }
        if !holding {
            if hypot(location.x - start.x, location.y - start.y) > 8 {
                moved = true
                waiting?.cancel()
            }
            return false
        }
        let draft = TimelineLoop(start: min(from, frame), end: max(from, frame) + 1)
        if draft != tool.loopDraft {
            UISelectionFeedbackGenerator().selectionChanged()
            tool.loopDraft = draft
        }
        return true
    }

    /// The scrub drag ended. True if it drew a loop, which is set (a single frame loops nothing).
    func ended(tool: TimelineTool) -> Bool {
        waiting?.cancel()
        waiting = nil
        start = nil
        guard holding else { return false }
        holding = false
        let draft = tool.loopDraft
        tool.loopDraft = nil
        if let draft, draft.end > draft.start + 1 {
            tool.setLoop(draft)
        }
        return true
    }
}

/// A grip on each end of the looped frames, on the ruler: dragged, that end moves frame by frame
/// (shown as it goes, set when let go: one undo step), never past the other end nor off the
/// timeline. Tapped, the Frames page opens.
struct TimelineLoopGrips: View {
    let tool: TimelineTool
    let pitch: CGFloat
    let height: CGFloat

    @State private var dragging: (end: Bool, from: TimelineLoop)?

    var body: some View {
        if let loop = tool.loopDraft ?? tool.loop, !tool.isPlaying {
            ZStack(alignment: .topLeading) {
                grip(at: loop.start, end: false)
                grip(at: loop.end, end: true)
            }
            .frame(height: height, alignment: .topLeading)
        }
    }

    private func grip(at frame: UInt32, end: Bool) -> some View {
        RoundedRectangle(cornerRadius: 2, style: .circular)
            .fill(Pc.accent)
            .frame(width: 5, height: height - 6)
            .frame(width: 24, height: height)
            .contentShape(Rectangle())
            .offset(x: CGFloat(frame) * pitch - 12)
            .onTapGesture { tool.openFrames() }
            .highPriorityGesture(
                DragGesture(minimumDistance: 2, coordinateSpace: .global)
                    .onChanged { drag in
                        guard let loop = tool.loop ?? tool.loopDraft, let duration = tool.info?.duration else { return }
                        let from = dragging?.from ?? loop
                        dragging = (end, from)
                        let by = Int((drag.translation.width / max(pitch, 0.01)).rounded())
                        var next = from
                        if end {
                            next.end = UInt32(min(max(Int(from.end) + by, Int(from.start) + 2), Int(duration)))
                        } else {
                            next.start = UInt32(min(max(Int(from.start) + by, 0), Int(from.end) - 2))
                        }
                        if next != tool.loopDraft {
                            UISelectionFeedbackGenerator().selectionChanged()
                            tool.loopDraft = next
                        }
                    }
                    .onEnded { _ in
                        let draft = tool.loopDraft
                        dragging = nil
                        tool.loopDraft = nil
                        if let draft, draft != tool.loop {
                            tool.setLoop(draft)
                        }
                    }
            )
    }
}

/// The timeline's end on the ruler: a tab dragged to make it longer or shorter, frame by frame,
/// down to a frame (past the clips' end too; shown as it goes, set when let go: one
/// "Length" undo step). Tapped, the Frames page opens.
struct TimelineLengthGrip: View {
    let tool: TimelineTool
    let pitch: CGFloat
    let height: CGFloat

    @State private var from: UInt32?
    /// Where the clips end, a firmer tick as the end passes it.
    @State private var clipsEnd: UInt32 = 0

    var body: some View {
        if let duration = tool.info?.duration, !tool.isPlaying {
            let at = tool.lengthDraft ?? duration
            let held = tool.lengthDraft != nil
            ZStack {
                UnevenRoundedRectangle(topLeadingRadius: 0, bottomLeadingRadius: 0, bottomTrailingRadius: 5, topTrailingRadius: 5)
                    .fill(held ? Pc.accent : playInk)
                    .frame(width: 9, height: height - 4)
            }
            .frame(width: 30, height: height)
            .contentShape(Rectangle())
            .offset(x: CGFloat(at) * pitch - 4)
            .onTapGesture { tool.openFrames() }
            .highPriorityGesture(
                DragGesture(minimumDistance: 2, coordinateSpace: .global)
                    .onChanged { drag in
                        if from == nil {
                            from = duration
                            clipsEnd = tool.contentEnd
                        }
                        let start = Int(from ?? duration)
                        let by = Int((drag.translation.width / max(pitch, 0.01)).rounded())
                        let next = UInt32(min(max(start + by, 1), 100_000))
                        if next != tool.lengthDraft {
                            // a tick each frame, a firmer one at the clips' end
                            if next == clipsEnd {
                                UIImpactFeedbackGenerator(style: .medium).impactOccurred()
                            } else {
                                UISelectionFeedbackGenerator().selectionChanged()
                            }
                            tool.lengthDraft = next
                        }
                    }
                    .onEnded { _ in
                        let draft = tool.lengthDraft
                        from = nil
                        tool.lengthDraft = nil
                        if let draft, draft != duration {
                            tool.setLength(draft)
                        }
                    }
            )
        }
    }
}

/// Reads which frames the full view's tracks show from the scroll view they are in, for Loop
/// What Shows (`TimelineTool.shownFrames`).
struct TimelineShownFrames: UIViewRepresentable {
    let tool: TimelineTool
    let pitch: CGFloat

    func makeUIView(context: Context) -> Probe {
        let view = Probe()
        view.isUserInteractionEnabled = false
        return view
    }

    func updateUIView(_ view: Probe, context: Context) {
        let pitch = max(pitch, 0.01)
        tool.shownFrames = { [weak view] in
            guard let view else { return nil }
            var at = view.superview
            while let v = at, !(v is UIScrollView) {
                at = v.superview
            }
            guard let scroll = at as? UIScrollView else { return nil }
            let first = max(Int((scroll.contentOffset.x / pitch).rounded()), 0)
            let last = max(Int(((scroll.contentOffset.x + scroll.bounds.width) / pitch).rounded(.down)), first + 1)
            return UInt32(first) ..< UInt32(last)
        }
        tool.scrolledBy = { [weak view] in
            var at = view?.superview
            while let v = at, !(v is UIScrollView) {
                at = v.superview
            }
            return (at as? UIScrollView)?.contentOffset.x ?? 0
        }
    }

    final class Probe: UIView {}
}

// MARK: - The header

/// The previous and next drawing, beside Play.
struct TimelineStepWords: View {
    let tool: TimelineTool

    var body: some View {
        if !tool.isPlaying {
            StepGlyph(forward: false) { tool.stepDrawing(-1) }
                .accessibilityLabel(Text(verbatim: "Previous Drawing"))
            StepGlyph(forward: true) { tool.stepDrawing(1) }
                .accessibilityLabel(Text(verbatim: "Next Drawing"))
        }
    }
}

/// A step to the drawing before or after: an arrowhead up against a bar, so it reads as going to
/// a stop and not as playing (a bare arrowhead beside Play looked like playing backwards and on).
private struct StepGlyph: View {
    let forward: Bool
    let action: () -> Void

    var body: some View {
        Canvas { ctx, size in
            var glyph = Path()
            glyph.move(to: CGPoint(x: 0, y: 0))
            glyph.addLine(to: CGPoint(x: 9.5, y: size.height / 2))
            glyph.addLine(to: CGPoint(x: 0, y: size.height))
            glyph.closeSubpath()
            glyph.addRoundedRect(in: CGRect(x: 11.5, y: 0, width: 2.5, height: size.height), cornerSize: CGSize(width: 1, height: 1))
            ctx.fill(glyph, with: .color(playInk))
        }
        .frame(width: 14, height: 13)
        .scaleEffect(x: forward ? 1 : -1)
        .frame(width: 26, height: 30)
        .contentShape(Rectangle())
        .onTapGesture(perform: action)
    }
}

/// The frame counter, tapped to open the Frames page: the frame the playhead is on (as bright as
/// the header's words), of how many, and how far into the timeline that is in time. While the end
/// is dragged, the length it would be.
struct TimelineFrameCounter: View {
    let tool: TimelineTool
    let duration: UInt32

    var body: some View {
        let lit = tool.framesOpen || tool.lengthDraft != nil
        let fps = Double(max(tool.info?.fps ?? 1, 1))
        (Text(verbatim: "\(tool.frame + 1)").foregroundColor(lit ? Pc.accent : playInk).fontWeight(.semibold)
            + Text(verbatim: " / \(tool.lengthDraft ?? duration)").foregroundColor(lit ? Pc.accent : playInkDim)
            + Text(verbatim: "   \(Self.time(Double(tool.frame) / fps))").foregroundColor(playInkDim))
            .font(.system(size: 15).monospacedDigit())
            .lineLimit(1)
            .frame(height: 30)
            .contentShape(Rectangle())
            .onTapGesture { tool.openFrames() }
            .accessibilityLabel(Text(verbatim: "Frames"))
    }

    /// `seconds` as the counter shows them: to the hundredth under a minute, then minutes too.
    static func time(_ seconds: Double) -> String {
        if seconds < 60 {
            return String(format: "%.2f s", seconds)
        }
        let whole = Int(seconds)
        return String(format: "%d:%02d.%d", whole / 60, whole % 60, Int((seconds - Double(whole)) * 10))
    }
}

// MARK: - The Frames page

/// The timeline's length, fitted to its clips; frames added before the first or after the last;
/// the looped frames: the frames showing, the open group's, or none.
struct TimelineFramesPage: View {
    let tool: TimelineTool
    let info: TimelineInfo

    /// How many frames Add puts in; none chosen yet, a second's worth.
    @State private var count: Int?

    var body: some View {
        let end = tool.contentEnd
        let adding = count ?? Int(max(info.fps, 1))
        VStack(alignment: .leading, spacing: 0) {
            PanelTitle(text: "Frames", bottom: 6)
            Text(verbatim: info.duration == 1 ? "1 frame long" : "\(info.duration) frames long, \(lengthInSeconds)")
                .font(.system(size: 13))
                .foregroundStyle(Pc.slideLabel)
                .padding(.leading, 12)
                .padding(.bottom, 12)
            TlGroup {
                TlAction(label: end > 0 && end != info.duration ? "Fit to Content (\(end) frames)" : "Fit to Content") {
                    tool.fitToContent()
                }
                .opacity(max(end, 1) == info.duration ? 0.45 : 1)
            }
            TlGroup {
                HStack(spacing: 0) {
                    Text(verbatim: "Add")
                        .font(.system(size: 17))
                        .foregroundStyle(Pc.panelTitle)
                    Spacer(minLength: 8)
                    stepper("−") { count = max(adding - 1, 1) }
                    Text(verbatim: adding == 1 ? "1 frame" : "\(adding) frames")
                        .font(.system(size: 15).monospacedDigit())
                        .foregroundStyle(Pc.panelTitle)
                        .frame(minWidth: 82)
                    stepper("+") { count = min(adding + 1, 999) }
                }
                .padding(.leading, 16)
                .padding(.trailing, 6)
                .frame(height: 44)
                TlRowLine()
                TlAction(label: "Add at Start") { tool.addFrames(atStart: true, count: UInt32(adding)) }
                TlRowLine()
                TlAction(label: "Add at End") { tool.addFrames(atStart: false, count: UInt32(adding)) }
            }
            .padding(.top, 10)
            TlGroup {
                TlAction(label: "Loop What Shows") { tool.loopShownFrames() }
                if let group = info.openGroups.last {
                    TlRowLine()
                    TlAction(label: "Loop This Group") { tool.loopGroup(group) }
                }
                if let l = tool.loop {
                    TlRowLine()
                    TlAction(label: "Stop Looping Frames \(l.start + 1)–\(l.end)") { tool.setLoop(nil) }
                }
            }
            .padding(.top, 10)
            Text(verbatim: "Hold the ruler and drag to loop frames; drag the timeline's end to make it longer or shorter.")
                .font(.system(size: 12))
                .foregroundStyle(Pc.slideLabel)
                .fixedSize(horizontal: false, vertical: true)
                .padding(.horizontal, 12)
                .padding(.top, 10)
        }
    }

    private var lengthInSeconds: String {
        let s = Double(info.duration) / Double(max(info.fps, 1))
        return s < 10 ? String(format: "%.1f s", s) : "\(Int(s.rounded())) s"
    }

    private func stepper(_ label: String, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 20, weight: .medium))
            .foregroundStyle(Pc.accent)
            .frame(width: 40, height: 40)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }
}


/// Past the timeline's end, over the lanes: shaded, with a line down at the end.
/// Clips parked out there stay where they are but neither play nor export.
struct TimelinePastEnd: View {
    /// The end, frames; a frame's width; how far across and down the lanes go.
    let end: UInt32
    let pitch: CGFloat
    let width: CGFloat
    let height: CGFloat

    var body: some View {
        let x = CGFloat(end) * pitch
        if x < width {
            ZStack(alignment: .topLeading) {
                Rectangle().fill(Color.black.opacity(0.28))
                    .frame(width: width - x, height: height)
                    .offset(x: x)
                Rectangle().fill(Color.white.opacity(0.3))
                    .frame(width: 1, height: height)
                    .offset(x: x)
            }
            .frame(width: width, height: height, alignment: .topLeading)
            .allowsHitTesting(false)
        }
    }
}

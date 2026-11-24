import SwiftUI
import UIKit
import UIKit.UIGestureRecognizerSubclass

// The grid timeline's zoom (see TimelineGrid.swift): two fingers on the frames pinch them wider
// or narrower about the playhead, and carry them and the rows along as they move together. A
// pinch shut fast as the fingers lift goes out to where the whole timeline shows; one flung open
// goes back to the size the grid starts at. Whatever one finger was doing when the second came
// down (a stroke of the lasso, a drag) is dropped.

extension GridState {
    /// How fast a pinch must be closing as the fingers lift to count as flicked shut, in parts of
    /// the spread it started at a second: the canvas's own (`QUICK_PINCH_SPEED`, touch.rs). And
    /// how fast one must be opening, in parts of the spread it has by then.
    static let quickPinch: CGFloat = -2.5
    static let quickSpread: CGFloat = 3

    /// The zoom at which all `frames` of the timeline show on either side of the playhead, with
    /// `room` points there: never further in than the grid starts. A pinch goes out as far as
    /// that, or as far as the rows get smaller where the timeline fits sooner.
    static func leastZoom(frames: Int, room: CGFloat) -> CGFloat {
        let fit = max(room, 1) / CGFloat(max(frames, 1))
        return min(max(fit, GridSizes.leastPitch) / GridSizes.pitch, 1)
    }

    /// A second finger came down on the frames: the two of them zoom and move the grid from
    /// here, and what the first was doing alone is dropped.
    func pinchBegan(tool: TimelineTool) {
        pinching = true
        panning = false
        playOnly = nil
        zoomTask?.cancel()
        zoomFrom = zoom
        scrollFrom = dragged ?? CGFloat(tool.frame)
        panFrom = rowsOffset
        // a stroke begun by the first finger: what it picked is as it was
        if touch == .stroke || !stroke.isEmpty {
            if let before = strokeBefore {
                tool.selecting = before
            }
            stroke = []
            faded = []
            touched = []
        }
        if trim != nil || groupTrim != nil {
            tool.endEdgeDrag()
            trim = nil
            groupTrim = nil
        }
        // (the drag that tells of one finger may say nothing until it lifts: it is no touch of
        // its own then either)
        touch = .ignore
        afterPinch = true
        closeMenus(tool)
    }

    /// The two fingers `scale` times as far apart as they came down, and moved `shift` together:
    /// the grid is that much wider (no further out than `least`), and the frames and rows go
    /// along with them.
    func pinchChanged(scale: CGFloat, shift: CGSize, least: CGFloat, last: Int, over: CGFloat, tool: TimelineTool) {
        guard pinching else { return }
        let next = min(max(zoomFrom * scale, min(least, GridSizes.leastRows)), GridSizes.most)
        if abs(next - zoom) > 0.0005 {
            zoom = next
        }
        // (a pinch's fingers never close quite evenly: a little drift moves nothing)
        if !panning, hypot(shift.width, shift.height) > 14 {
            panning = true
            panShift = shift
        }
        if panning {
            scrolled(shift.width - panShift.width, last: last, tool: tool)
            rowsOffset = min(max(panFrom - (shift.height - panShift.height), 0), over)
        }
    }

    /// The fingers lifted, the pinch `speed` fast as they did (closing: parts of its first spread
    /// a second, below zero; `opening`: parts of its spread then). Flicked shut, the grid eases
    /// out to `least`, where the whole timeline shows (as a quick pinch fits the canvas to the
    /// screen); flung open while it is smaller than it starts, back to the size it starts at.
    func pinchEnded(speed: CGFloat, opening: CGFloat, least: CGFloat, last: Int, tool: TimelineTool) {
        guard pinching else { return }
        pinching = false
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.3) { [weak self] in
            if self?.pinching == false {
                self?.afterPinch = false
            }
        }
        if dragged != nil {
            scrollEnded(0, last: last, tool: tool)
        }
        if speed < Self.quickPinch {
            ease(zoomTo: least)
        } else if opening > Self.quickSpread, zoom < 1 {
            ease(zoomTo: 1)
        } else {
            keepZoom()
        }
    }

    /// The zoom goes to `target` over a moment, evenly as it reads (by ratio, not by points).
    func ease(zoomTo target: CGFloat, over seconds: Double = 0.34) {
        zoomTask?.cancel()
        let from = zoom
        guard abs(from - target) > 0.001 else {
            keepZoom()
            return
        }
        zoomTask = Task { @MainActor [weak self] in
            let start = ContinuousClock.now
            while !Task.isCancelled {
                let t = min((ContinuousClock.now - start) / .seconds(seconds), 1)
                let eased = t * t * (3 - 2 * t)
                self?.zoom = exp(log(from) + (log(target) - log(from)) * eased)
                if t >= 1 {
                    break
                }
                try? await Task.sleep(for: .milliseconds(8))
            }
            self?.keepZoom()
        }
    }

    /// The zoom is kept for the next canvas.
    func keepZoom() {
        UserDefaults.standard.set(Double(zoom), forKey: "timelineGridZoom")
    }
}

/// Tells of two fingers on the view it is behind: when the second comes down, how far apart they
/// are since (as a part of how far they were) and how far they have moved together, and, as one
/// lifts, how fast they were closing or opening. It watches the window for touches that come down
/// here beside whatever else takes them, as `GridHoldProbe` does, and from the moment the second
/// finger lands rather than once they have moved, so a stroke under way stops there.
struct GridPinchProbe: UIViewRepresentable {
    let began: () -> Void
    let changed: (_ scale: CGFloat, _ shift: CGSize) -> Void
    /// (closing or opening in parts of the first spread a second; opening in parts of the last)
    let ended: (_ speed: CGFloat, _ opening: CGFloat) -> Void

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeUIView(context: Context) -> Probe {
        let view = Probe()
        view.isUserInteractionEnabled = false
        view.coordinator = context.coordinator
        context.coordinator.probe = self
        return view
    }

    func updateUIView(_ view: Probe, context: Context) {
        context.coordinator.probe = self
    }

    static func dismantleUIView(_ view: Probe, coordinator: Coordinator) {
        coordinator.detach()
    }

    final class Probe: UIView {
        weak var coordinator: Coordinator?

        override func didMoveToWindow() {
            super.didMoveToWindow()
            coordinator?.attach(from: self)
        }
    }

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        var probe: GridPinchProbe?
        private weak var view: UIView?
        private weak var window: UIWindow?
        private var fingers: TwoFingers?

        func attach(from view: UIView) {
            self.view = view
            guard view.window !== window else { return }
            detach()
            guard let found = view.window else { return }
            let made = TwoFingers(target: nil, action: nil)
            made.cancelsTouchesInView = false
            made.delaysTouchesBegan = false
            made.delaysTouchesEnded = false
            made.delegate = self
            made.within = view
            made.began = { [weak self] in self?.probe?.began() }
            made.changed = { [weak self] scale, shift in self?.probe?.changed(scale, shift) }
            made.ended = { [weak self] speed, opening in self?.probe?.ended(speed, opening) }
            found.addGestureRecognizer(made)
            window = found
            fingers = made
        }

        func detach() {
            if let fingers {
                window?.removeGestureRecognizer(fingers)
            }
            fingers = nil
            window = nil
        }

        /// Only touches that come down on the view it is behind.
        func gestureRecognizer(_ g: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
            guard let view else { return false }
            return view.bounds.contains(touch.location(in: view))
        }

        func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
            true
        }
    }
}

/// Follows the first two fingers down on a view. It never recognizes (so it takes nothing from
/// any other gesture): it only says what the two are doing.
private final class TwoFingers: UIGestureRecognizer {
    var began: () -> Void = {}
    var changed: (CGFloat, CGSize) -> Void = { _, _ in }
    var ended: (CGFloat, CGFloat) -> Void = { _, _ in }
    weak var within: UIView?

    private var first: UITouch?
    private var second: UITouch?
    private var spread: CGFloat = 1
    private var centre: CGPoint = .zero
    /// The pinch's scale as it went, with when (seconds), for its speed as it ends.
    private var samples: [(at: TimeInterval, scale: CGFloat)] = []

    private func pair() -> (spread: CGFloat, centre: CGPoint)? {
        guard let a = first?.location(in: within), let b = second?.location(in: within) else { return nil }
        return (max(hypot(a.x - b.x, a.y - b.y), 1), CGPoint(x: (a.x + b.x) / 2, y: (a.y + b.y) / 2))
    }

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        for t in touches {
            if first == nil {
                first = t
            } else if second == nil, t !== first {
                second = t
                if let now = pair() {
                    spread = now.spread
                    centre = now.centre
                    samples = [(event.timestamp, 1)]
                    began()
                }
            }
        }
    }

    override func touchesMoved(_ touches: Set<UITouch>, with event: UIEvent) {
        guard second != nil, let now = pair() else { return }
        let scale = now.spread / spread
        samples.append((event.timestamp, scale))
        if samples.count > 120 {
            samples.removeFirst(60)
        }
        changed(scale, CGSize(width: now.centre.x - centre.x, height: now.centre.y - centre.y))
    }

    override func touchesEnded(_ touches: Set<UITouch>, with event: UIEvent) {
        lifted(touches, at: event.timestamp)
    }

    override func touchesCancelled(_ touches: Set<UITouch>, with event: UIEvent) {
        lifted(touches, at: event.timestamp)
    }

    private func lifted(_ touches: Set<UITouch>, at now: TimeInterval) {
        let pinching = second != nil
        var gone = false
        if let a = first, touches.contains(a) {
            first = nil
            gone = true
        }
        if let b = second, touches.contains(b) {
            second = nil
            gone = true
        }
        if pinching, gone {
            // how fast the spread was changing over the last 40 ms: nothing heard meanwhile is
            // fingers held still
            let last = samples.last ?? (now, 1)
            let before = samples.last { $0.at <= now - 0.04 } ?? samples.first ?? last
            let seconds = now - before.at
            let speed = seconds > 0.0005 ? (last.scale - before.scale) / CGFloat(seconds) : 0
            ended(speed, speed / max(last.scale, 0.05))
            // the finger left is no pinch, and no new one starts with it
            first = nil
            second = nil
            samples = []
        }
        if first == nil, second == nil, state == .possible {
            state = .failed
        }
    }

    override func reset() {
        super.reset()
        first = nil
        second = nil
        samples = []
    }
}

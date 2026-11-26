import SwiftUI
import UIKit
import UIKit.UIGestureRecognizerSubclass

/// Keeps SwiftUI's layout, accessibility, and ScrollViewReader targets. A pan drives
/// the visible UIKit offset with the same Rust model used by the other frontends.
struct KineticScrollView<Content: View>: View {
    let axes: Axis.Set
    let showsIndicators: Bool
    let content: () -> Content

    init(_ axes: Axis.Set = .vertical, showsIndicators: Bool = true, @ViewBuilder content: @escaping () -> Content) {
        self.axes = axes
        self.showsIndicators = showsIndicators
        self.content = content
    }

    var body: some View {
        SwiftUI.ScrollView(axes, showsIndicators: showsIndicators) {
            content().background(alignment: .topLeading) {
                ScrollAttachment(horizontal: axes == .horizontal).frame(width: 0, height: 0)
            }
        }
    }
}

/// The brush library uses this to distinguish a pull from a coast.
@MainActor
func isKineticScrollTracking(_ scroll: UIScrollView) -> Bool {
    scroll.isTracking || scroll.gestureRecognizers?.contains {
        $0 is ContentScrollPan && ($0.state == .began || $0.state == .changed)
    } == true
}

private final class ContentScrollPan: UIPanGestureRecognizer {
    var onContact: (() -> Void)?
    var onReset: (() -> Void)?

    override func touchesBegan(_ touches: Set<UITouch>, with event: UIEvent) {
        onContact?()
        super.touchesBegan(touches, with: event)
    }

    override func reset() {
        super.reset()
        onReset?()
    }
}

private struct ScrollAttachment: UIViewRepresentable {
    let horizontal: Bool

    func makeUIView(context: Context) -> Marker {
        let view = Marker()
        view.isUserInteractionEnabled = false
        view.horizontal = horizontal
        return view
    }

    func updateUIView(_ view: Marker, context: Context) {
        view.horizontal = horizontal
        view.findScrollView()
    }

    static func dismantleUIView(_ view: Marker, coordinator: ()) {
        view.detach()
    }

    final class Marker: UIView, UIGestureRecognizerDelegate {
        fileprivate var horizontal = false
        private weak var scroller: UIScrollView?
        private var originalPanEnabled = true
        private var motion: ScrollMotion?
        private var started: CFTimeInterval = 0
        private var caught: (velocity: Float, touched: CFTimeInterval)?
        private var lastWritten: CGFloat?
        private let loop = FrameLoop()
        private var pan: ContentScrollPan?

        override func didMoveToSuperview() {
            super.didMoveToSuperview()
            findScrollView()
        }

        override func didMoveToWindow() {
            super.didMoveToWindow()
            if window == nil { detach() } else { findScrollView() }
        }

        override func layoutSubviews() {
            super.layoutSubviews()
            if scroller == nil { findScrollView() }
        }

        fileprivate func findScrollView() {
            guard window != nil else { return }
            var ancestor = superview
            while let view = ancestor, !(view is UIScrollView) {
                ancestor = view.superview
            }
            guard let scroll = ancestor as? UIScrollView else { return }
            if scroll === scroller {
                scroll.panGestureRecognizer.isEnabled = false
                return
            }
            detach()
            scroller = scroll
            originalPanEnabled = scroll.panGestureRecognizer.isEnabled
            scroll.panGestureRecognizer.isEnabled = false
            let recognizer = ContentScrollPan(target: self, action: #selector(panned(_:)))
            recognizer.maximumNumberOfTouches = 1
            recognizer.allowedScrollTypesMask = [.continuous, .discrete]
            recognizer.delegate = self
            recognizer.onContact = { [weak self] in self?.catchScroll() }
            recognizer.onReset = { [weak self] in self?.returnWhenIdle() }
            scroll.addGestureRecognizer(recognizer)
            pan = recognizer
        }

        fileprivate func detach() {
            stop()
            if let scroll = scroller {
                if let pan {
                    pan.onContact = nil
                    pan.onReset = nil
                    scroll.removeGestureRecognizer(pan)
                }
                scroll.panGestureRecognizer.isEnabled = originalPanEnabled && scroll.isScrollEnabled
            }
            scroller = nil
            pan = nil
        }

        private func axis(_ point: CGPoint) -> CGFloat { horizontal ? point.x : point.y }

        private func bounds(_ scroll: UIScrollView) -> (min: Float, max: Float, viewport: Float) {
            let inset = scroll.adjustedContentInset
            let min = horizontal ? -inset.left : -inset.top
            let end = horizontal ? inset.right : inset.bottom
            let length = horizontal ? scroll.contentSize.width : scroll.contentSize.height
            let viewport = horizontal ? scroll.bounds.width : scroll.bounds.height
            return (Float(min), Float(Swift.max(min, length + end - viewport)), Float(viewport))
        }

        func gestureRecognizerShouldBegin(_ gestureRecognizer: UIGestureRecognizer) -> Bool {
            guard let scroll = scroller, scroll.isScrollEnabled,
                  let pan = gestureRecognizer as? UIPanGestureRecognizer else { return false }
            let range = bounds(scroll)
            let velocity = pan.velocity(in: scroll)
            let cross = horizontal ? velocity.y : velocity.x
            return range.max > range.min && abs(axis(velocity)) >= abs(cross)
        }

        private func catchScroll() {
            let now = CACurrentMediaTime()
            caught = motion.map { (scrollMotionAt(motion: $0, seconds: Float(now - started)).velocity, now) }
            stop()
        }

        private func returnWhenIdle() {
            guard !loop.running, let scroll = scroller, scroll.isScrollEnabled else { return }
            let range = bounds(scroll)
            let position = Float(axis(scroll.contentOffset))
            if position < range.min || position > range.max {
                start(scrollMotion(from: position, velocity: 0, min: range.min, max: range.max, snap: 0))
            }
        }

        @objc private func panned(_ pan: UIPanGestureRecognizer) {
            guard let scroll = scroller, scroll.isScrollEnabled else { stop(); return }
            let range = bounds(scroll)
            let current = Float(axis(scroll.contentOffset))
            switch pan.state {
            case .began, .changed:
                stop()
                let delta = -Float(axis(pan.translation(in: scroll)))
                pan.setTranslation(.zero, in: scroll)
                let next = scrollDrag(position: current, delta: delta, min: range.min, max: range.max, viewport: range.viewport)
                move(scroll, to: CGFloat(next))
            case .ended, .cancelled:
                let speed = pan.state == .ended ? -Float(axis(pan.velocity(in: scroll))) : 0
                var velocity = scrollDragVelocity(position: current, velocity: speed, min: range.min, max: range.max, viewport: range.viewport)
                if let caught {
                    velocity = scrollFlingVelocity(velocity: velocity, previous: caught.velocity,
                                                   pause: Float(CACurrentMediaTime() - caught.touched))
                    self.caught = nil
                }
                start(scrollMotion(from: current, velocity: velocity, min: range.min, max: range.max, snap: 0))
            default:
                break
            }
        }

        private func move(_ scroll: UIScrollView, to position: CGFloat) {
            let current = scroll.contentOffset
            if horizontal {
                scroll.contentOffset = CGPoint(x: position, y: current.y)
            } else {
                scroll.contentOffset = CGPoint(x: current.x, y: position)
            }
        }

        private func start(_ value: ScrollMotion) {
            stop()
            motion = value
            started = CACurrentMediaTime()
            loop.start { [weak self] _ in self?.tick() ?? false }
        }

        private func tick() -> Bool {
            guard let scroll = scroller, scroll.isScrollEnabled, let motion else { stop(); return false }
            if let lastWritten, abs(lastWritten - axis(scroll.contentOffset)) > 0.5 {
                // Reordering, keyboard navigation, and ScrollViewReader can move the list directly.
                stop()
                return false
            }
            let now = CACurrentMediaTime()
            let time = Float(now - started)
            let sample = scrollMotionAt(motion: motion, seconds: time)
            let expected = CGFloat(sample.position)
            move(scroll, to: expected)
            lastWritten = axis(scroll.contentOffset)
            if sample.done {
                stop()
                return false
            }
            if let lastWritten, abs(lastWritten - expected) > 0.5 {
                // The nested brush search header may have consumed some of this frame.
                let range = bounds(scroll)
                self.motion = scrollMotion(from: Float(lastWritten), velocity: sample.velocity, min: range.min, max: range.max, snap: 0)
                started = now
            }
            return true
        }

        private func stop() {
            loop.stop()
            motion = nil
            lastWritten = nil
        }
    }
}

/// Calls its tick once a frame with the seconds since the frame before, until the tick says to
/// stop or it is stopped.
final class FrameLoop: NSObject {
    private var link: CADisplayLink?
    private var last: CFTimeInterval = 0
    private var tick: ((CFTimeInterval) -> Bool)?

    var running: Bool {
        link != nil
    }

    func start(_ tick: @escaping (CFTimeInterval) -> Bool) {
        stop()
        self.tick = tick
        last = CACurrentMediaTime()
        let made = CADisplayLink(target: self, selector: #selector(step))
        made.add(to: .main, forMode: .common)
        link = made
    }

    func stop() {
        link?.invalidate()
        link = nil
        tick = nil
    }

    @objc private func step() {
        let now = CACurrentMediaTime()
        let seconds = now - last
        last = now
        if tick?(seconds) != true {
            stop()
        }
    }
}

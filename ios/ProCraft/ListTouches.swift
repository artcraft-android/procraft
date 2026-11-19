import SwiftUI
import UIKit

/// A list's rows held still and slid along. These are UIKit's: a SwiftUI drag on a row (one of its
/// own, one beside the scroll's, a hold and then a drag) keeps the list under it from scrolling when
/// the finger goes down on that row. They watch from the window for as long as the list is up and
/// take nothing from it: a hold begins only with the finger still, and a slide only with a finger
/// that sets off sideways, which ends any scrolling that finger had begun.
///
/// One lies behind a whole list, and finds the row from where the finger is (`takes` then says
/// which touches are the rows'); or one lies behind each row, and has the touches that go down on
/// it. Points are in this view's own space: behind a whole list, it fills what the rows report
/// their places in.
struct RowTouches: UIViewRepresentable {
    /// Whether a touch that went down on this view is on the rows at all (not on what is over
    /// them). Without it, the touches are those that go down inside this view, in the list it is in.
    var takes: ((UIView) -> Bool)?
    /// A finger held still here: whether it took hold of a row. Then where it goes, and its end.
    var held: (CGPoint) -> Bool = { _ in false }
    var heldMoved: (CGPoint) -> Void = { _ in }
    var heldEnded: () -> Void = {}
    /// Whether a finger setting off sideways from here slides a row. Then its start, how far it has
    /// gone, and its end.
    var slides: (CGPoint) -> Bool = { _ in false }
    var slideBegan: (CGPoint) -> Void = { _ in }
    var slid: (CGFloat) -> Void = { _ in }
    var slideEnded: () -> Void = {}

    final class Anchor: UIView, UIGestureRecognizerDelegate {
        var touches: RowTouches?
        private var hold: UILongPressGestureRecognizer?
        private var slide: UIPanGestureRecognizer?
        private var holding = false
        private var sliding = false
        /// What the finger went down on: the lists it is in stop scrolling as it slides.
        private weak var touched: UIView?

        override func willMove(toWindow window: UIWindow?) {
            super.willMove(toWindow: window)
            // taken off the screen mid-touch, a recognizer says nothing more
            finish()
            for recognizer in ([hold, slide] as [UIGestureRecognizer?]).compactMap({ $0 }) {
                recognizer.view?.removeGestureRecognizer(recognizer)
            }
            hold = nil
            slide = nil
            guard let window else { return }
            let hold = UILongPressGestureRecognizer(target: self, action: #selector(held(_:)))
            hold.minimumPressDuration = 0.5
            let slide = UIPanGestureRecognizer(target: self, action: #selector(slid(_:)))
            slide.maximumNumberOfTouches = 1
            for recognizer in [hold, slide] as [UIGestureRecognizer] {
                recognizer.cancelsTouchesInView = false
                recognizer.delaysTouchesBegan = false
                recognizer.delaysTouchesEnded = false
                recognizer.delegate = self
                window.addGestureRecognizer(recognizer)
            }
            self.hold = hold
            self.slide = slide
        }

        private func finish() {
            if holding {
                holding = false
                touches?.heldEnded()
            }
            if sliding {
                sliding = false
                touches?.slideEnded()
            }
        }

        func gestureRecognizer(_: UIGestureRecognizer, shouldReceive touch: UITouch) -> Bool {
            guard let view = touch.view, let touches, taken(view, touch, by: touches) else { return false }
            touched = view
            return true
        }

        private func taken(_ view: UIView, _ touch: UITouch, by touches: RowTouches) -> Bool {
            if let takes = touches.takes {
                return takes(view)
            }
            guard bounds.contains(touch.location(in: self)) else { return false }
            var list = superview
            while let above = list, !(above is UIScrollView) {
                list = above.superview
            }
            return list.map { view.isDescendant(of: $0) } ?? true
        }

        private func place(_ g: UIGestureRecognizer) -> CGPoint {
            g.location(in: self)
        }

        /// Beside everything, a list's own scrolling too: were it one or the other, whichever began
        /// first would win, and a sideways finger sometimes began a scroll first. A slide ends the
        /// scroll itself instead.
        func gestureRecognizer(_: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith _: UIGestureRecognizer) -> Bool {
            true
        }

        override func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
            guard g === slide, let pan = slide else { return !sliding }
            let moved = pan.translation(in: self)
            let at = place(pan)
            return !holding && abs(moved.x) > abs(moved.y)
                && touches?.slides(CGPoint(x: at.x - moved.x, y: at.y - moved.y)) == true
        }

        @objc private func held(_ g: UILongPressGestureRecognizer) {
            switch g.state {
            case .began:
                holding = touches?.held(place(g)) == true
            case .changed:
                if holding {
                    touches?.heldMoved(place(g))
                }
            case .ended, .cancelled, .failed:
                if holding {
                    holding = false
                    touches?.heldEnded()
                }
            default:
                break
            }
        }

        @objc private func slid(_ g: UIPanGestureRecognizer) {
            let moved = g.translation(in: self)
            switch g.state {
            case .began:
                sliding = true
                stopScrolling(above: touched)
                let at = place(g)
                touches?.slideBegan(CGPoint(x: at.x - moved.x, y: at.y - moved.y))
                touches?.slid(moved.x)
            case .changed:
                if sliding {
                    touches?.slid(moved.x)
                }
            case .ended, .cancelled, .failed:
                if sliding {
                    sliding = false
                    touches?.slideEnded()
                }
            default:
                break
            }
        }
    }

    func makeUIView(context _: Context) -> Anchor {
        let anchor = Anchor()
        anchor.isUserInteractionEnabled = false
        anchor.touches = self
        return anchor
    }

    func updateUIView(_ anchor: Anchor, context _: Context) {
        anchor.touches = self
    }
}

/// Ends the scrolling a finger has begun in the lists a view is in, for a finger that turns out to
/// be going along a row. A list that isn't to scroll now is left as it is.
private func stopScrolling(above view: UIView?) {
    var above = view
    while let view = above {
        if let pan = (view as? UIScrollView)?.panGestureRecognizer, pan.isEnabled {
            pan.isEnabled = false
            pan.isEnabled = true
        }
        above = view.superview
    }
}

/// A slider row's touches, UIKit's for the same reason: a tap anywhere on the row moves the slider
/// there, and so does a finger that sets off sideways; one that sets off up or down is the list's,
/// to scroll with. `moved` hears how far along the row the finger is, `done` that it let go.
struct SliderTouches: UIViewRepresentable {
    let moved: (CGFloat) -> Void
    let done: () -> Void

    final class Pad: UIView, UIGestureRecognizerDelegate {
        var touches: SliderTouches?

        override init(frame: CGRect) {
            super.init(frame: frame)
            let slide = UIPanGestureRecognizer(target: self, action: #selector(slid(_:)))
            slide.maximumNumberOfTouches = 1
            slide.delegate = self
            addGestureRecognizer(slide)
            addGestureRecognizer(UITapGestureRecognizer(target: self, action: #selector(tapped(_:))))
        }

        @available(*, unavailable)
        required init?(coder: NSCoder) {
            fatalError("made in code")
        }

        override func gestureRecognizerShouldBegin(_ g: UIGestureRecognizer) -> Bool {
            guard let pan = g as? UIPanGestureRecognizer else { return true }
            let moved = pan.translation(in: self)
            return abs(moved.x) > abs(moved.y)
        }

        /// Beside the list's scrolling, which the slide then ends (as `RowTouches` do).
        func gestureRecognizer(_: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
            other is UIPanGestureRecognizer && other.view is UIScrollView
        }

        @objc private func slid(_ g: UIPanGestureRecognizer) {
            switch g.state {
            case .began:
                stopScrolling(above: superview)
                touches?.moved(g.location(in: self).x)
            case .changed:
                touches?.moved(g.location(in: self).x)
            case .ended, .cancelled, .failed:
                touches?.done()
            default:
                break
            }
        }

        @objc private func tapped(_ g: UITapGestureRecognizer) {
            touches?.moved(g.location(in: self).x)
            touches?.done()
        }
    }

    func makeUIView(context _: Context) -> Pad {
        let pad = Pad()
        pad.touches = self
        return pad
    }

    func updateUIView(_ pad: Pad, context _: Context) {
        pad.touches = self
    }
}

/// A button row's touches, UIKit's for the same reason, and the row's own size exactly (a SwiftUI
/// button answers a little way past its edges, into the row above or the gap below). `pressed`
/// hears a finger go down on the row and come off it, let go or taken by the list as it starts to
/// scroll; `tapped` that it let go where it went down.
struct PressTouches: UIViewRepresentable {
    let pressed: (Bool) -> Void
    let tapped: () -> Void

    final class Pad: UIView {
        var touches: PressTouches?
        private var start: CGPoint?

        override func touchesBegan(_ touches: Set<UITouch>, with _: UIEvent?) {
            guard start == nil, let touch = touches.first else { return }
            start = touch.location(in: nil)
            self.touches?.pressed(true)
        }

        override func touchesEnded(_ touches: Set<UITouch>, with _: UIEvent?) {
            guard let start, let touch = touches.first else { return }
            self.start = nil
            self.touches?.pressed(false)
            let at = touch.location(in: nil)
            if abs(at.x - start.x) < 10, abs(at.y - start.y) < 10 {
                self.touches?.tapped()
            }
        }

        override func touchesCancelled(_: Set<UITouch>, with _: UIEvent?) {
            guard start != nil else { return }
            start = nil
            touches?.pressed(false)
        }
    }

    func makeUIView(context _: Context) -> Pad {
        let pad = Pad()
        pad.touches = self
        return pad
    }

    func updateUIView(_ pad: Pad, context _: Context) {
        pad.touches = self
    }
}

/// Finds the scroll view a list is in, from inside what it scrolls: `RowTouches` behind a whole
/// list know the rows' touches as those that went down in it.
struct ListFinder: UIViewRepresentable {
    let found: (UIScrollView) -> Void

    final class Probe: UIView {
        var found: ((UIScrollView) -> Void)?

        override func didMoveToWindow() {
            super.didMoveToWindow()
            guard window != nil else { return }
            var above = superview
            while let view = above, !(view is UIScrollView) {
                above = view.superview
            }
            if let list = above as? UIScrollView {
                found?(list)
            }
        }
    }

    func makeUIView(context _: Context) -> Probe {
        let probe = Probe()
        probe.isUserInteractionEnabled = false
        probe.found = found
        return probe
    }

    func updateUIView(_ probe: Probe, context _: Context) {
        probe.found = found
    }
}

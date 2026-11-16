import SwiftUI
import UIKit

// A second finger turning a trim's edge policy over, as Koi's does: while a clip's edge is dragged,
// a second finger down anywhere on the tracks makes that one drag Stop where the policy is Push or
// Trim, and Push (or Trim, whichever was chosen last) where it is Stop. The track shows the trim as
// it now would at once, and a tag over the tracks names the policy in use. It holds until the edge
// is let go: lifting either finger first doesn't turn it back, so letting both go together never
// commits the trim under the other policy.

extension TimelineTool {
    /// What a dragged edge does at the next clip for the drag under way: the edge policy, turned
    /// over if a second finger did.
    var dragPolicy: TimelineEdgePolicy {
        guard edgeFlipped else { return edgePolicy }
        switch edgePolicy {
        case .stop:
            return UserDefaults.standard.string(forKey: "timelineFreeEdgePolicy") == "trim" ? .trim : .push
        case .push, .trim:
            return .stop
        }
    }

    /// A second finger came down while an edge is dragged: its policy turned over, once a drag.
    func flipEdge() {
        guard edgeDragging, !edgeFlipped else { return }
        edgeFlipped = true
        UIImpactFeedbackGenerator(style: .medium).impactOccurred()
    }

    /// The edge let go (or the drag taken away): the policy back as chosen.
    func endEdgeDrag() {
        edgeDragging = false
        edgeFlipped = false
    }
}

/// Takes two fingers down on the scroll view the tracks are in, so a second one while an edge is
/// dragged turns its policy over. While an edge is dragged the scroll view doesn't scroll.
struct TrimFlipProbe: UIViewRepresentable {
    let tool: TimelineTool
    /// Whether an edge is being dragged.
    let dragging: Bool

    func makeCoordinator() -> Coordinator {
        Coordinator(tool: tool)
    }

    func makeUIView(context: Context) -> Probe {
        let view = Probe()
        view.isUserInteractionEnabled = false
        view.coordinator = context.coordinator
        return view
    }

    func updateUIView(_ view: Probe, context: Context) {
        context.coordinator.hold(dragging)
    }

    final class Probe: UIView {
        weak var coordinator: Coordinator?

        override func didMoveToWindow() {
            super.didMoveToWindow()
            coordinator?.attach(from: self)
        }
    }

    final class Coordinator: NSObject, UIGestureRecognizerDelegate {
        let tool: TimelineTool
        private weak var scroll: UIScrollView?
        private var press: UILongPressGestureRecognizer?

        init(tool: TimelineTool) {
            self.tool = tool
        }

        /// Watches the scroll view `view` is in for two fingers down.
        func attach(from view: UIView) {
            var at = view.superview
            while let v = at, !(v is UIScrollView) {
                at = v.superview
            }
            guard let found = at as? UIScrollView, found !== scroll else { return }
            if let press {
                scroll?.removeGestureRecognizer(press)
            }
            let made = UILongPressGestureRecognizer(target: self, action: #selector(pressed(_:)))
            made.numberOfTouchesRequired = 2
            made.minimumPressDuration = 0
            made.allowableMovement = .greatestFiniteMagnitude
            // the first finger's drag goes on as it was
            made.cancelsTouchesInView = false
            made.delaysTouchesBegan = false
            made.delaysTouchesEnded = false
            // a pencil on the edge and a finger count as two
            made.requiresExclusiveTouchType = false
            made.delegate = self
            found.addGestureRecognizer(made)
            scroll = found
            press = made
        }

        /// An edge dragged or let go: the tracks don't scroll under it meanwhile.
        func hold(_ dragging: Bool) {
            scroll?.panGestureRecognizer.isEnabled = !dragging
        }

        /// Two fingers down, or moving: if an edge is being dragged by then, its policy turns
        /// over (two put down together turn it once the edge's drag gets going).
        @objc private func pressed(_ g: UILongPressGestureRecognizer) {
            if g.state == .began || g.state == .changed {
                tool.flipEdge()
            }
        }

        func gestureRecognizer(_ g: UIGestureRecognizer, shouldRecognizeSimultaneouslyWith other: UIGestureRecognizer) -> Bool {
            true
        }
    }
}

/// While a second finger has turned a trim's policy over: the policy the drag now uses, in a tag
/// over the top of the tracks.
struct TrimFlipBadge: View {
    let tool: TimelineTool

    var body: some View {
        if tool.edgeDragging, tool.edgeFlipped {
            let name = switch tool.dragPolicy {
            case .stop: "Stop at Clips"
            case .push: "Push Clips"
            case .trim: "Trim Into Clips"
            }
            Text(verbatim: name)
                .font(.system(size: 12, weight: .bold))
                .foregroundStyle(Color.white)
                .padding(.horizontal, 10)
                .padding(.vertical, 4)
                .background(Capsule().fill(Pc.accent))
                .shadow(color: .black.opacity(0.3), radius: 4)
                .padding(.top, 3)
                .allowsHitTesting(false)
                .transition(.opacity)
        }
    }
}

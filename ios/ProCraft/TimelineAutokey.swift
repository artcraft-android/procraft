import SwiftUI

// Autokey (a keyframe mode): with it on, the transform tool on a timeline moves, scales and
// turns the drawing at the playhead by keying its clip's pose there, rather than moving its pixels
// (the core's `timeline::autokey`). A group or video clip picked in the panel, playing at the
// playhead, is keyed in its place, and the clip page's opacity keys the clip's opacity at the
// playhead. Turned on from the full view's header, it opens the transform tool; while the
// transform keys, a badge over the canvas says which frame it keys, and tapped turns autokey off
// (the transform's toolbar stands over the header then).

/// Autokey's colour, a recording light's.
let autokeyRed = Color(argb: 0xFFFF_453A)

extension CanvasModel {
    /// Autokey on or off. On, a transform moving pixels is kept and the transform opens again to
    /// key the drawing (or the group or video picked) at the playhead; off, a transform keying a
    /// pose ends, its keys kept.
    func setAutokey(_ on: Bool) {
        let posing = session.timelineAutokeyPosing()
        timeline.autokey = on
        timeline.stopPlaying()
        session.setTimelineAutokeyClip(id: timeline.picked)
        session.setTimelineAutokey(on: on, easing: timeline.keyEasing)
        if on {
            if transforming, !posing {
                endTransform(commit: true)
            }
            if !transforming {
                toggleTransform()
            }
        } else if transforming, posing {
            // (the core has ended it already)
            endTransform(commit: true)
        }
        refresh()
    }

    /// After the transform's box changed: a key it made is read again (the panel's keys, undo).
    func autokeyFollow() {
        if transforming, session.timelineAutokeyKeyed() {
            refresh()
        }
    }

    /// The transform keying a pose follows the playhead: its box laid again over the drawing there,
    /// as its keys show it.
    func followAutokeyBox() {
        guard transforming, let box = session.timelineAutokeyBox() else { return }
        transform.moved(box)
    }
}

extension TimelineTool {
    /// The clip picked changed: a group or video picked is what autokey keys, the open transform's
    /// box laid over it.
    func autokeyPickedChanged() {
        let model = canvasModel
        model.session.setTimelineAutokeyClip(id: picked)
        if autokey {
            model.followAutokeyBox()
        }
    }

    /// The Keys page's easing changed: autokey's new keys ease so from now on.
    func autokeyEasingChanged() {
        canvasModel.session.setTimelineAutokeyEasing(easing: keyEasing)
    }

    /// Clip `id`'s opacity as its slider shows it: keyed at the playhead while autokey is on, its
    /// own otherwise.
    func opacityShown(_ clip: TimelineClipInfo) -> Float {
        autokey ? canvasModel.session.timelineClipOpacityHere(id: clip.id) : clip.opacity
    }

    /// With autokey on, clip `id`'s opacity keyed at the playhead (one "Key Opacity" step when
    /// `done`). False when autokey is off, and the opacity is the clip's own to set.
    func autokeyOpacity(_ id: UInt64, _ opacity: Float, done: Bool) -> Bool {
        guard autokey else { return false }
        if !canvasModel.session.autokeyTimelineClipOpacity(id: id, opacity: opacity, done: done), done {
            // not at the playhead, or locked: nothing keyed
            UINotificationFeedbackGenerator().notificationOccurred(.warning)
        }
        if done {
            canvasModel.refresh()
        }
        return true
    }
}

/// The header's Autokey: red, a dot before it, while on.
struct AutokeyWord: View {
    let model: CanvasModel

    var body: some View {
        let on = model.timeline.autokey
        HStack(spacing: 6) {
            if on {
                Circle().fill(autokeyRed).frame(width: 8, height: 8)
            }
            BarWord(label: "Autokey", bold: on, tint: on ? autokeyRed : nil) { model.setAutokey(!on) }
        }
    }
}

/// Over the canvas while the transform keys a pose: the frame it keys. Tapped, autokey goes off
/// (the transform keying ends, its keys kept).
struct AutokeyBadge: View {
    let model: CanvasModel

    var body: some View {
        // (the box read here, so the badge is drawn again as it moves)
        if model.transform.box != nil, model.timeline.autokey, model.session.timelineAutokeyPosing() {
            HStack(spacing: 7) {
                Circle().fill(.white).frame(width: 7, height: 7)
                Text(verbatim: "Keying frame \(model.timeline.frame + 1)")
                    .font(.system(size: 14, weight: .semibold).monospacedDigit())
                    .foregroundStyle(.white)
                Rectangle().fill(.white.opacity(0.5)).frame(width: 1, height: 14)
                Text(verbatim: "Turn Off")
                    .font(.system(size: 14, weight: .regular))
                    .foregroundStyle(.white)
            }
            .padding(.horizontal, 12)
            .frame(height: 28)
            .background(Capsule().fill(autokeyRed))
            .contentShape(Capsule())
            .onTapGesture { model.setAutokey(false) }
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .top)
            .offset(y: Pc.barHeight + 10)
        }
    }
}

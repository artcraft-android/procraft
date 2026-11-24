import SwiftUI

// "My curves" in the curve editor: motion curves kept by name. One is an easing as the editor
// made it (its kind, that kind's numbers, a drawn curve's points), kept by the app and so there
// in every artwork, to put on other keys, rename, duplicate, replace or delete.

struct SavedCurve: Codable, Identifiable, Equatable {
    var id = UUID()
    var name: String
    var kind: String
    var params: [Float]
    var repeats: UInt32
    var looping: Bool
    var reversed: Bool

    /// The kinds by the names they are kept under.
    private static let kinds: [(EasingKind, String)] = [
        (.linear, "linear"), (.hold, "hold"), (.smooth, "smooth"), (.cubicBezier, "cubicBezier"), (.curve, "curve"),
        (.bounce, "bounce"), (.elastic, "elastic"), (.steps, "steps"), (.elasticSteps, "elasticSteps"),
        (.cyclic, "cyclic"), (.random, "random"),
    ]

    init(name: String, _ spec: EasingSpec) {
        self.name = name
        kind = Self.kinds.first { $0.0 == spec.kind }?.1 ?? "linear"
        params = spec.params
        repeats = spec.repeats
        looping = spec.looping
        reversed = spec.reversed
    }

    var spec: EasingSpec {
        EasingSpec(
            kind: Self.kinds.first { $0.1 == kind }?.0 ?? .linear, params: params,
            repeats: max(repeats, 1), looping: looping, reversed: reversed
        )
    }
}

/// The curves kept, in the order they were made (a copy just after what it is of).
@Observable
final class SavedCurves {
    static let shared = SavedCurves()

    private static let key = "timelineCurves"

    private(set) var all: [SavedCurve]

    private init() {
        let kept = UserDefaults.standard.data(forKey: Self.key)
        all = kept.flatMap { try? JSONDecoder().decode([SavedCurve].self, from: $0) } ?? []
    }

    private func keep() {
        guard let data = try? JSONEncoder().encode(all) else { return }
        UserDefaults.standard.set(data, forKey: Self.key)
    }

    /// `base`, or `base` with the first number after it that no curve has.
    private func free(_ base: String, numbered: Bool) -> String {
        let taken = Set(all.map(\.name))
        if !numbered, !taken.contains(base) {
            return base
        }
        var n = numbered ? 1 : 2
        while taken.contains("\(base) \(n)") {
            n += 1
        }
        return "\(base) \(n)"
    }

    /// The name a new curve is offered: "Curve 1" and so on.
    var nextName: String { free("Curve", numbered: true) }

    /// `spec` kept as a new curve named `name` (the next free name for one of only spaces).
    func add(_ spec: EasingSpec, named name: String) {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        all.append(SavedCurve(name: name.isEmpty ? nextName : name, spec))
        keep()
    }

    /// Nothing for a name of only spaces.
    func rename(_ id: UUID, _ name: String) {
        let name = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !name.isEmpty, let i = all.firstIndex(where: { $0.id == id }) else { return }
        all[i].name = name
        keep()
    }

    func duplicate(_ id: UUID) {
        guard let i = all.firstIndex(where: { $0.id == id }) else { return }
        let copy = SavedCurve(name: free("\(all[i].name) copy", numbered: false), all[i].spec)
        all.insert(copy, at: i + 1)
        keep()
    }

    /// Curve `id` becomes `spec`, its name kept.
    func replace(_ id: UUID, with spec: EasingSpec) {
        guard let i = all.firstIndex(where: { $0.id == id }) else { return }
        var next = SavedCurve(name: all[i].name, spec)
        next.id = id
        all[i] = next
        keep()
    }

    func remove(_ id: UUID) {
        all.removeAll { $0.id == id }
        keep()
    }
}

/// The curve editor's "My curves": the easing showing kept as a new one, and each one kept, to
/// put on the key (a tap), or rename, duplicate, replace with what is showing, or delete (its ⋯).
struct MyCurves: View {
    /// The easing the editor shows now.
    let spec: EasingSpec
    /// Brings what has this id into view, where the editor scrolls.
    var reveal: ((AnyHashable) -> Void)?
    let apply: (EasingSpec) -> Void

    /// The curve whose actions show.
    @State private var open: UUID?
    /// The name being asked for (in an alert, which stands clear of the keyboard): a new curve's,
    /// or that of the curve being renamed.
    @State private var asking: Ask?
    @State private var typed = ""

    private enum Ask: Equatable {
        case new
        case rename(UUID)
    }

    private let curves = SavedCurves.shared

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text(verbatim: "My curves")
                .font(.system(size: 13, weight: .semibold))
                .foregroundStyle(Pc.panelTitle.opacity(0.6))
                .padding(.leading, 16)
            TlGroup {
                HStack(spacing: 10) {
                    PcIconView(PcIcons.plusMedium, size: 22, color: Pc.accent)
                        .frame(width: 46)
                    Text(verbatim: "Save this curve")
                        .font(.system(size: 17))
                        .foregroundStyle(Pc.accent)
                    Spacer(minLength: 0)
                }
                .padding(.leading, 12)
                .frame(height: 44)
                .contentShape(Rectangle())
                .onTapGesture {
                    open = nil
                    typed = curves.nextName
                    asking = .new
                }
                .accessibilityElement()
                .accessibilityLabel(Text(verbatim: "Save this curve"))
                .accessibilityAddTraits(.isButton)
                ForEach(curves.all) { curve in
                    TlRowLine()
                    row(curve)
                        .id(curve.id)
                    if open == curve.id {
                        actions(curve)
                            .id("actions \(curve.id)")
                    }
                }
            }
        }
        .alert(asking == .new ? "Save curve" : "Rename curve", isPresented: Binding(get: { asking != nil }, set: { if !$0 { asking = nil } })) {
            TextField("Name", text: $typed)
                .autocorrectionDisabled()
            Button("Cancel", role: .cancel) {}
            Button(asking == .new ? "Save" : "Rename") {
                switch asking {
                case .new:
                    curves.add(spec, named: typed)
                    if let made = curves.all.last?.id {
                        show(made)
                    }
                case let .rename(id): curves.rename(id, typed)
                case nil: break
                }
            }
        }
    }

    /// (once it is laid out)
    private func show(_ id: AnyHashable) {
        DispatchQueue.main.asyncAfter(deadline: .now() + 0.1) { reveal?(id) }
    }

    private func row(_ curve: SavedCurve) -> some View {
        let chosen = curve.spec == spec
        return HStack(spacing: 10) {
            CurveThumb(spec: curve.spec)
                .frame(width: 46, height: 28)
            Text(verbatim: curve.name)
                .font(.system(size: 17, weight: chosen ? .semibold : .regular))
                .foregroundStyle(chosen ? Pc.accent : Pc.panelTitle)
                .lineLimit(1)
            Spacer(minLength: 0)
            PcIconView(PcIcons.more, size: 22, color: open == curve.id ? Pc.accent : Pc.panelTitle.opacity(0.6))
                .frame(width: 44, height: 44)
                .contentShape(Rectangle())
                .onTapGesture {
                    open = open == curve.id ? nil : curve.id
                    if open != nil {
                        show("actions \(curve.id)")
                    }
                }
                .accessibilityElement()
                .accessibilityLabel(Text(verbatim: "\(curve.name) options"))
                .accessibilityAddTraits(.isButton)
        }
        .padding(.leading, 12)
        .frame(height: 44)
        .contentShape(Rectangle())
        .onTapGesture { apply(curve.spec) }
        .accessibilityElement(children: .contain)
        .accessibilityLabel(Text(verbatim: curve.name))
    }

    private func actions(_ curve: SavedCurve) -> some View {
        HStack(spacing: 4) {
            action("Rename") {
                typed = curve.name
                asking = .rename(curve.id)
            }
            action("Duplicate") { curves.duplicate(curve.id) }
            // (what the editor shows now, kept under this name)
            action("Replace", dim: curve.spec == spec) { curves.replace(curve.id, with: spec) }
            action("Delete", tint: Grid.danger) {
                curves.remove(curve.id)
                open = nil
            }
        }
        .padding(.horizontal, 8)
        .padding(.bottom, 8)
    }

    private func action(_ label: String, tint: Color? = nil, dim: Bool = false, _ act: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 13, weight: .medium))
            .foregroundStyle((tint ?? Pc.panelTitle).opacity(dim ? 0.35 : 1))
            .lineLimit(1)
            .minimumScaleFactor(0.8)
            .frame(maxWidth: .infinity, minHeight: 30)
            .background(RoundedRectangle(cornerRadius: 9, style: .circular).fill(Pc.panelTitle.opacity(0.08)))
            .contentShape(Rectangle())
            .onTapGesture {
                if !dim {
                    act()
                }
            }
            .accessibilityElement()
            .accessibilityLabel(Text(verbatim: label))
            .accessibilityAddTraits(.isButton)
    }
}

/// A curve small: time across, how far along up.
struct CurveThumb: View {
    let spec: EasingSpec

    var body: some View {
        let samples = easingCurve(s: spec, samples: 32)
        Canvas { ctx, size in
            let inset: CGFloat = 3
            let y = { (v: Float) in inset + (size.height - 2 * inset) * CGFloat(1.5 - min(max(v, -0.5), 1.5)) / 2 }
            var line = Path()
            for (i, v) in samples.enumerated() {
                let p = CGPoint(x: inset + (size.width - 2 * inset) * CGFloat(i) / CGFloat(max(samples.count - 1, 1)), y: y(v))
                i == 0 ? line.move(to: p) : line.addLine(to: p)
            }
            ctx.stroke(line, with: .color(Pc.accent), style: StrokeStyle(lineWidth: 1.8, lineCap: .round, lineJoin: .round))
        }
        .background(RoundedRectangle(cornerRadius: 7, style: .circular).fill(Pc.panelTitle.opacity(0.08)))
    }
}

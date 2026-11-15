import SwiftUI

private let lightWidth: CGFloat = 300
private var menuText: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }

extension PcIcons {
    /// The light tool: a sun, drawn as the other bar glyphs are (stroked, round-capped, in a 44
    /// frame). The iPad's own until the other apps have the tool and their icon table takes it.
    static let light = PcIcon(name: "light", width: 44.0, height: 44.0, parts: [
        PcIconPart(d: "M27.6 22a5.6 5.6 0 1 1-11.2 0a5.6 5.6 0 1 1 11.2 0z", stroke: 2.3, fill: false, evenOdd: false),
        PcIconPart(
            d: "M31 22h3.4M9.6 22h3.4M22 31v3.4M22 9.6v3.4M28.4 28.4l2.4 2.4M13.2 13.2l2.4 2.4M28.4 15.6l2.4-2.4M13.2 30.8l2.4-2.4",
            stroke: 2.3, fill: false, evenOdd: false
        ),
    ])
}

/// The light tool's panel, hanging from its button: what a stroke does to the paint under the brush
/// (brighten, dim, or tame what is past white) and how strongly. The brush is the paint tool's.
struct LightPanel: View {
    let model: CanvasModel
    let arrowX: CGFloat

    private static let kinds: [(kind: LightKind, name: String)] = [
        (.brighten, "Brighten"), (.dim, "Dim"), (.tame, "Tame"),
    ]

    private func group<Rows: View>(@ViewBuilder _ rows: () -> Rows) -> some View {
        let shape = RoundedRectangle(cornerRadius: 13, style: .circular)
        return VStack(spacing: 0) {
            rows()
        }
        .background(shape.fill(Pc.actionsGroup))
        .clipShape(shape)
    }

    var body: some View {
        let light = model.light
        Popover(width: lightWidth, arrowX: arrowX) {
            VStack(alignment: .leading, spacing: 0) {
                // a 52 header, as the other menus have
                Text(verbatim: "Light")
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                    .padding(.leading, 24)
                    .padding(.top, 22.5)
                    .frame(height: 52, alignment: .topLeading)
                VStack(spacing: 10) {
                    group {
                        ForEach(Array(Self.kinds.enumerated()), id: \.offset) { i, entry in
                            if i > 0 {
                                Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                            }
                            let chosen = light.kind == entry.kind
                            HStack(spacing: 0) {
                                Text(verbatim: entry.name)
                                    .font(.system(size: 17))
                                    .foregroundStyle(chosen ? Pc.accent : menuText)
                                    .lineLimit(1)
                                Spacer(minLength: 0)
                                if chosen {
                                    PcIconView(PcIcons.tick, size: 24, color: Pc.accent)
                                }
                            }
                            .padding(.leading, 15)
                            .padding(.trailing, 12)
                            .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44)
                            .contentShape(Rectangle())
                            .onTapGesture {
                                model.setLight(LightSettings(kind: entry.kind, strength: light.strength))
                            }
                        }
                    }
                    group {
                        PanelSlider(
                            label: "Strength", value: light.strength, shown: "\(wholeNumber(light.strength * 100))%"
                        ) { v in
                            model.setLight(LightSettings(kind: light.kind, strength: v))
                        }
                    }
                }
                .padding(.horizontal, 10)
                .padding(.top, 8)
                .padding(.bottom, 10)
            }
        }
    }
}

/// The light panel's width.
let lightPanelWidth = lightWidth

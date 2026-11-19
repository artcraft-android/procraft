import SwiftUI

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

/// The light tool's settings, under the brush library's Light: what a stroke does to the paint
/// under the brush (brighten, dim, or tame what is past white) and how strongly. The brush is the
/// Pen's, chosen under Brush.
struct LightSettingsView: View {
    let model: CanvasModel

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
            // how far past white Brighten goes: an HDR canvas's only (a standard one stops at white)
            if model.isHdr, light.kind == .brighten {
                group {
                    PanelSlider(
                        label: "HDR limit", value: model.lightLimit / hdrLimitMost,
                        shown: hdrLimitShown(model.lightLimit)
                    ) { v in
                        model.setLightLimit(v * hdrLimitMost)
                    }
                }
            }
        }
        .padding(.horizontal, 10)
        .padding(.bottom, 10)
    }
}

/// The Pen's lasso's settings, under the brush library's Lasso. Outline: the brush paints the edge
/// as it goes, and what it goes round fills under it as it lifts, in the paint colour or the
/// second colour, at a strength of its own (the bar's opacity is the brush's). Without, each shape
/// drawn fills with the paint colour at the bar's opacity. The Eraser's lasso has only its words.
struct LassoSettingsView: View {
    let model: CanvasModel

    private func group<Rows: View>(@ViewBuilder _ rows: () -> Rows) -> some View {
        let shape = RoundedRectangle(cornerRadius: 13, style: .circular)
        return VStack(spacing: 0) {
            rows()
        }
        .background(shape.fill(Pc.actionsGroup))
        .clipShape(shape)
    }

    private func caption(_ text: String) -> some View {
        Text(verbatim: text)
            .font(.system(size: 13))
            .foregroundStyle(Pc.panelGlyph)
            .frame(maxWidth: .infinity, alignment: .leading)
            .fixedSize(horizontal: false, vertical: true)
            .padding(.horizontal, 15)
    }

    /// How strongly the fill shows as it is drawn: Off at none.
    private var previewSlider: some View {
        let preview = model.lassoPreviewOpacity
        return group {
            PanelSlider(
                label: "Preview", value: preview, shown: preview <= 0 ? "Off" : "\(wholeNumber(preview * 100))%"
            ) { v in
                model.setLassoPreviewOpacity(v)
            }
        }
    }

    var body: some View {
        VStack(spacing: 10) {
            if model.tool == .erase {
                previewSlider
                caption("Draw a shape to erase it, at the eraser's opacity.")
            } else {
                let outline = model.lassoOutline
                group {
                    HStack(spacing: 0) {
                        Text(verbatim: "Outline with brush")
                            .font(.system(size: 17))
                            .foregroundStyle(menuText)
                            .lineLimit(1)
                        Spacer(minLength: 0)
                        PcSwitch(on: outline)
                    }
                    .padding(.leading, 15)
                    .padding(.trailing, 12)
                    .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44)
                    .contentShape(Rectangle())
                    .onTapGesture { model.setLassoOutline(!outline) }
                }
                if outline {
                    group {
                        HStack(spacing: 12) {
                            Text(verbatim: "Fill")
                                .font(.system(size: 17))
                                .foregroundStyle(menuText)
                                .lineLimit(1)
                            Spacer(minLength: 0)
                            Picker(
                                "Fill",
                                selection: Binding(get: { model.lassoFillSecondary }, set: { model.setLassoFillSecondary($0) })
                            ) {
                                Text(verbatim: "Color").tag(false)
                                Text(verbatim: "Second color").tag(true)
                            }
                            .pickerStyle(.segmented)
                            .labelsHidden()
                            .fixedSize()
                            .environment(\.colorScheme, Look.shared.light ? .light : .dark)
                        }
                        .padding(.leading, 15)
                        .padding(.trailing, 12)
                        .frame(maxWidth: .infinity, minHeight: 50, maxHeight: 50)
                        Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                        let opacity = model.lassoFillOpacity
                        PanelSlider(
                            label: "Fill opacity", value: opacity, shown: "\(wholeNumber(opacity * 100))%"
                        ) { v in
                            model.setLassoFillOpacity(v)
                        }
                    }
                }
                previewSlider
                caption(outline
                    ? "The brush draws the edge; what it goes round fills under it as you lift."
                    : "Draw a shape to fill it with the color, at the brush's opacity.")
            }
        }
        .padding(.horizontal, 10)
        .padding(.bottom, 14)
    }
}

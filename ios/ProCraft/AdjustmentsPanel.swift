import SwiftUI

private let adjustmentsWidth: CGFloat = 330
private var menuText: Color { Look.shared.light ? Color(argb: 0xFF39_3939) : Color(argb: 0xFFC5_C5C5) }
private var menuDisabledText: Color { Look.shared.light ? Color(argb: 0xFFAA_AAAA) : Color(argb: 0xFF62_6262) }

/// The adjustments menu hanging from its button, in groups as the core lists them: colour, blurs,
/// effects, painting.
struct AdjustmentsPanel: View {
    let model: CanvasModel
    let arrowX: CGFloat
    private let groups = adjustmentGroups()
    /// The filters imported: the menu's last group is one row to them, there only with some.
    private let filters = PluginsModel.shared.store.filters()
    /// The plugins' actions listed here, after the filters.
    private let actions = PluginsModel.shared.store.actionsIn(place: .adjustments)
    /// The menu has turned to them.
    @State private var showFilters = false

    private func row(_ label: String, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 17))
            .foregroundStyle(menuText)
            .lineLimit(1)
            .padding(.leading, 15)
            .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }

    private func group<Rows: View>(@ViewBuilder _ rows: () -> Rows) -> some View {
        let shape = RoundedRectangle(cornerRadius: 13, style: .circular)
        return VStack(spacing: 0) {
            rows()
        }
        .background(shape.fill(Pc.actionsGroup))
        .clipShape(shape)
    }

    private var filterRows: some View {
        group {
            row("Adjustments") { showFilters = false }
            ForEach(filters, id: \.id) { filter in
                Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                row(filter.name) { model.startPluginFilter(filter) }
            }
            ForEach(actions, id: \.key) { action in
                Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                row(action.label) { model.runPluginAction(action.key) }
            }
        }
    }

    var body: some View {
        Popover(width: adjustmentsWidth, arrowX: arrowX) {
            VStack(alignment: .leading, spacing: 0) {
                // a 52 header, the title's capitals from 28.5 below the panel's top; the groups 8
                // into the list under it
                Text(verbatim: showFilters ? "Plugins" : "Adjustments")
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                    .padding(.leading, 24)
                    .padding(.top, 22.5)
                    .frame(height: 52, alignment: .topLeading)
                ScrollView {
                    VStack(spacing: 10) {
                        if showFilters {
                            filterRows
                        } else {
                        // the adjustment chosen next goes on a layer of its own, editable
                        group {
                            HStack(spacing: 0) {
                                Text(verbatim: "Adjustment Layer")
                                    .font(.system(size: 17))
                                    .foregroundStyle(menuText)
                                    .lineLimit(1)
                                Spacer(minLength: 0)
                                if model.adjustAsLayer {
                                    PcIconView(PcIcons.tick, size: 20, color: Pc.accent)
                                }
                            }
                            .padding(.leading, 15)
                            .padding(.trailing, 14)
                            .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
                            .contentShape(Rectangle())
                            .onTapGesture { model.adjustAsLayer.toggle() }
                        }
                        ForEach(Array(groups.enumerated()), id: \.offset) { _, group in
                            VStack(spacing: 0) {
                                ForEach(Array(group.enumerated()), id: \.offset) { i, adjustment in
                                    if i > 0 {
                                        Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                                    }
                                    // as an adjustment layer, those that can't be one are greyed
                                    let takes = !model.adjustAsLayer || adjustmentLive(adjustment: adjustment)
                                    Text(verbatim: adjustmentLabel(adjustment: adjustment))
                                        .font(.system(size: 17))
                                        .foregroundStyle(takes ? menuText : menuDisabledText)
                                        .lineLimit(1)
                                        .padding(.leading, 15)
                                        .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
                                        .contentShape(Rectangle())
                                        .onTapGesture {
                                            if model.adjustAsLayer {
                                                if takes {
                                                    model.addAdjustmentLayer(adjustment)
                                                }
                                            } else {
                                                model.startAdjustment(adjustment)
                                            }
                                        }
                                }
                            }
                            .background(RoundedRectangle(cornerRadius: 13, style: .circular).fill(Pc.actionsGroup))
                            .clipShape(RoundedRectangle(cornerRadius: 13, style: .circular))
                        }
                        if !filters.isEmpty || !actions.isEmpty {
                            group { row("Plugins") { showFilters = true } }
                        }
                        }
                    }
                    .padding(.horizontal, 10)
                    .padding(.top, 8)
                    .padding(.bottom, 10)
                }
                .scrollIndicators(.hidden)
            }
        }
        #if DEBUG
        .onAppear {
            showFilters = ScreenCapture.filtersShow
            if ScreenCapture.adjustAsLayer {
                model.adjustAsLayer = true
            }
        }
        #endif
    }
}

/// The adjustments menu's width.
let adjustmentsPanelWidth = adjustmentsWidth

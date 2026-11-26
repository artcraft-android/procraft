import SwiftUI

/// The list the Layers panel's plus opens for a filter layer (interface 17): the filters the
/// plugins on offer under their headings, in the order they list them. A tap picks one; Cancel
/// closes it.
struct FilterLayerList: View {
    let filters: [PluginLayerFilter]
    let onPick: (String) -> Void
    let onClose: () -> Void

    /// The filters under their headings, each heading once, where its first filter comes.
    private var groups: [(String, [PluginLayerFilter])] {
        var out: [(String, [PluginLayerFilter])] = []
        for f in filters {
            if let at = out.firstIndex(where: { $0.0 == f.group }) {
                out[at].1.append(f)
            } else {
                out.append((f.group, [f]))
            }
        }
        return out
    }

    var body: some View {
        NavigationStack {
            KineticScrollView {
                LazyVStack(alignment: .leading, spacing: 20) {
                    ForEach(groups, id: \.0) { group in
                        VStack(alignment: .leading, spacing: 6) {
                            Text(group.0)
                                .font(.footnote)
                                .foregroundStyle(.secondary)
                                .padding(.horizontal, 16)
                                .accessibilityAddTraits(.isHeader)
                            VStack(spacing: 0) {
                                ForEach(group.1, id: \.id) { filter in
                                    if filter.id != group.1.first?.id { Divider().padding(.leading, 16) }
                                    Button { onPick(filter.id) } label: {
                                        Text(filter.label)
                                            .padding(.horizontal, 16)
                                            .frame(maxWidth: .infinity, minHeight: 44, alignment: .leading)
                                            .contentShape(Rectangle())
                                    }
                                }
                            }
                            .background(Color(uiColor: .secondarySystemGroupedBackground), in: RoundedRectangle(cornerRadius: 10))
                        }
                    }
                }
                .padding(20)
            }
            .background(Color(uiColor: .systemGroupedBackground))
            .navigationTitle("Filter Layer")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel", action: onClose)
                }
            }
        }
    }
}

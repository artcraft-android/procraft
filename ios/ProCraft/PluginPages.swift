import SwiftUI

/// The tag on a card.
private var pluginTag: Color { Look.shared.light ? Color(argb: 0x1400_0000) : Color(argb: 0x1FFF_FFFF) }

/// The plugins, a page of the panel: a card each, drawn from what the list says of them.
struct PluginsPage: View {
    let model: CanvasModel
    let onBack: () -> Void

    @State private var plugins = PluginsModel.shared
    @State private var importing = false

    var body: some View {
        let shown = plugins.plugins
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 0) {
                PageHeader(title: "Plugins", onBack: onBack)
                Spacer(minLength: 0)
                Text(verbatim: "Import")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.accent)
                    .lineLimit(1)
                    .padding(.horizontal, 16)
                    .frame(height: 34)
                    .contentShape(Rectangle())
                    .onTapGesture { importing = true }
                    .padding(.top, 12)
            }
            ScrollView {
                VStack(spacing: 20) {
                    if shown.isEmpty {
                        Text(verbatim: "Nothing imported yet. Import a .procraft-plugin file to add one.")
                            .font(.system(size: 15))
                            .foregroundStyle(Pc.actionsRow)
                            .padding(16)
                            .frame(maxWidth: .infinity, alignment: .leading)
                    }
                    ForEach(shown, id: \.id) { plugin in
                        PluginCard(model: model, plugin: plugin)
                    }
                }
                .padding(.horizontal, 12)
                .padding(.top, 8)
                .padding(.bottom, 13)
            }
            .scrollIndicators(.hidden)
            .frame(maxHeight: 760)
        }
        .fileImporter(isPresented: $importing, allowedContentTypes: [.item]) { result in
            if case let .success(url) = result {
                plugins.offer(url) { model.showNotice($0, warning: true) }
            }
        }
        .onAppear { plugins.read() }
    }
}

/// A plugin: its name and tag, what it does, what is the matter with it, its controls whatever
/// they are, and the rows that turn it on or off and remove it.
private struct PluginCard: View {
    let model: CanvasModel
    let plugin: PluginInfo

    private var line: some View {
        Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
    }

    private var about: some View {
        VStack(alignment: .leading, spacing: 0) {
            HStack(spacing: 8) {
                Text(verbatim: plugin.name)
                    .font(.system(size: 17, weight: .semibold))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                if plugin.experimental {
                    Text(verbatim: "experimental")
                        .font(.system(size: 12, weight: .medium))
                        .foregroundStyle(Pc.actionsRow)
                        .lineLimit(1)
                        .padding(.horizontal, 7)
                        .padding(.vertical, 2)
                        .background(RoundedRectangle(cornerRadius: 9, style: .circular).fill(pluginTag))
                }
            }
            Text(verbatim: plugin.description)
                .font(.system(size: 14))
                .foregroundStyle(Pc.actionsRow)
                .padding(.top, 6)
            if let problem = plugin.problem {
                Text(verbatim: problem)
                    .font(.system(size: 14, weight: .medium))
                    .foregroundStyle(Pc.slideValue)
                    .padding(.top, 4)
            }
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .frame(maxWidth: .infinity, alignment: .leading)
    }

    @ViewBuilder private func control(_ control: PluginControl) -> some View {
        switch control {
        case let .switch(one):
            RowView(row: .toggle(one.label, on: one.on, change: { PluginsModel.shared.setSwitch(one.key, $0) }))
        case let .slider(one):
            PluginSliderRow(slider: one) { PluginsModel.shared.setSlider($0, $1) }
        case let .file(one):
            PluginFileRow(file: one) { model.showNotice($0, warning: true) }
        }
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 22, style: .circular)
        let plugins = PluginsModel.shared
        VStack(spacing: 0) {
            about
            // a filter's own controls are on its panel; what the card shows comes with the list
            if plugin.on, plugin.problem == nil {
                ForEach(plugin.controls.indices, id: \.self) { i in
                    line
                    control(plugin.controls[i])
                }
                // what it does when tapped
                ForEach(plugin.actions, id: \.key) { action in
                    line
                    PluginActionRow(label: action.label) { model.runPluginAction(action.key) }
                }
            }
            line
            RowView(row: .act(plugin.on ? "Turn off" : "Turn on", action: { plugins.setOn(plugin, !plugin.on) }))
            if plugin.source == .file {
                line
                RowView(row: .act("Remove", action: {
                    if !plugins.remove(plugin) {
                        model.showNotice("This plugin couldn't be removed.", warning: true)
                    }
                }))
            }
        }
        .background(shape.fill(Pc.actionsGroup))
        .clipShape(shape)
    }
}

/// One of a plugin's actions: its words in the accent colour, run when tapped.
private struct PluginActionRow: View {
    let label: String
    let action: () -> Void

    var body: some View {
        Text(verbatim: label)
            .font(.system(size: 17))
            .foregroundStyle(Pc.accent)
            .lineLimit(1)
            .padding(.horizontal, 16)
            .frame(maxWidth: .infinity, minHeight: 44, maxHeight: 44, alignment: .leading)
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }
}

/// A plugin's own panel, while its action keeps it open: floating under the bar at the right, its
/// title and Done, then its rows on a card as the Actions panel's are. A slider is told as it lets
/// go, anything else at once.
struct PluginPanelView: View {
    let model: CanvasModel
    let panel: PluginPanel

    private var line: some View {
        Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
    }

    @ViewBuilder private func row(_ i: Int, _ row: PanelRow) -> some View {
        switch row {
        case let .text(text):
            Text(verbatim: text)
                .font(.system(size: 15))
                .foregroundStyle(Pc.actionsRow)
                .padding(.horizontal, 16)
                .padding(.vertical, 11)
                .frame(maxWidth: .infinity, alignment: .leading)
        case let .slider(label, least, most, step, value, unit):
            PanelSliderRow(label: label, least: least, most: most, step: step, value: value, unit: unit) {
                model.touchPluginPanel(i, $0)
            }
            .frame(height: 44)
        case let .switch(label, on):
            RowView(row: .toggle(label, on: on, change: { model.touchPluginPanel(i, $0 ? 1 : 0) }))
        case let .choice(label, options, chosen):
            HStack(spacing: 0) {
                Text(verbatim: label)
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.actionsRow)
                    .lineLimit(1)
                Spacer(minLength: 8)
                Text(verbatim: options.indices.contains(Int(chosen)) ? options[Int(chosen)] : "")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.accent)
                    .lineLimit(1)
            }
            .padding(.horizontal, 16)
            .frame(height: 44)
            .contentShape(Rectangle())
            // a tap picks the next
            .onTapGesture { model.touchPluginPanel(i, Float((Int(chosen) + 1) % max(options.count, 1))) }
        case let .button(label):
            PluginActionRow(label: label) { model.touchPluginPanel(i, 1) }
        case let .keys(label, keys):
            // what it does, then the keys that do it; a tap asks for others
            HStack(spacing: 0) {
                Text(verbatim: label)
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.actionsRow)
                    .lineLimit(1)
                Spacer(minLength: 8)
                Text(verbatim: keys)
                    .font(.system(size: 15))
                    .foregroundStyle(Pc.accent)
                    .lineLimit(1)
            }
            .padding(.horizontal, 16)
            .frame(height: 44)
            .contentShape(Rectangle())
            .onTapGesture { model.touchPluginPanel(i, 1) }
        }
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 13, style: .circular)
        Popover(width: 340, arrowX: 0, side: .none) {
            VStack(spacing: 0) {
                HStack(spacing: 8) {
                    Text(verbatim: panel.title)
                        .font(.system(size: 22, weight: .bold))
                        .foregroundStyle(Pc.panelTitle)
                        .lineLimit(1)
                    Spacer(minLength: 0)
                    Text(verbatim: "Done")
                        .font(.system(size: 17))
                        .foregroundStyle(Pc.accent)
                        .lineLimit(1)
                        .contentShape(Rectangle())
                        .onTapGesture { model.closePluginPanel() }
                }
                .padding(.horizontal, 24)
                .padding(.top, 12)
                .frame(height: 52)
                ScrollView {
                    VStack(spacing: 0) {
                        ForEach(Array(panel.rows.enumerated()), id: \.offset) { i, r in
                            if i > 0 {
                                line
                            }
                            row(i, r)
                        }
                    }
                    .background(shape.fill(Pc.actionsGroup))
                    .clipShape(shape)
                    .padding(10)
                }
                .scrollIndicators(.hidden)
                .frame(maxHeight: 520)
                .fixedSize(horizontal: false, vertical: true)
            }
        }
        // what its rows don't take stays on the panel, never reaching the canvas under it
        .contentShape(Rectangle())
        .onTapGesture {}
    }
}

/// A slider on a plugin's panel: its value whole with whole steps, else to two places, then its
/// unit; told as it lets go.
private struct PanelSliderRow: View {
    let label: String
    let least: Float
    let most: Float
    let step: Float
    let value: Float
    let unit: String
    let onSet: (Float) -> Void

    @State private var moved: Float?

    var body: some View {
        let shown = moved ?? value
        let whole = step >= 1 && step.truncatingRemainder(dividingBy: 1) == 0
        let words = (whole ? "\(Int(shown.rounded()))" : String(format: "%.2f", shown)) + unit
        let span = max(most - least, .ulpOfOne)
        ActionSlideRow(
            label: label,
            value: words,
            share: (shown - least) / span,
            change: { along in
                let to = least + along * span
                let stepped = step > 0 ? least + ((to - least) / step).rounded() * step : to
                moved = min(max(stepped, least), most)
            },
            onDone: {
                if let moved {
                    onSet(moved)
                }
                moved = nil
            }
        )
    }
}

/// A file control on a card (a sound): its label, then the name of what was picked and Clear, or
/// Import; a tap on the row picks another.
private struct PluginFileRow: View {
    let file: PluginFile
    let told: (String) -> Void

    @State private var picking = false

    var body: some View {
        HStack(spacing: 0) {
            Text(verbatim: file.label)
                .font(.system(size: 17))
                .foregroundStyle(Pc.actionsRow)
                .lineLimit(1)
            Spacer(minLength: 0)
            if let chosen = file.chosen {
                Text(verbatim: chosen)
                    .font(.system(size: 15))
                    .foregroundStyle(Pc.slideValue)
                    .lineLimit(1)
                    .truncationMode(.tail)
                    .padding(.leading, 12)
                Text(verbatim: "Clear")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.accent)
                    .lineLimit(1)
                    .padding(.leading, 12)
                    .contentShape(Rectangle())
                    .onTapGesture { PluginsModel.shared.clearFile(file.key) }
            } else {
                Text(verbatim: "Import")
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.accent)
                    .lineLimit(1)
            }
        }
        .padding(.horizontal, 16)
        .frame(height: 44)
        .contentShape(Rectangle())
        .onTapGesture { picking = true }
        .fileImporter(isPresented: $picking, allowedContentTypes: [.audio]) { result in
            if case let .success(url) = result {
                PluginsModel.shared.pickSound(file.key, from: url, told: told)
            }
        }
    }
}

/// A slider on a card: the row follows the finger, and the value goes in as it lets go, as what it
/// sets may move the row itself, unless it is live.
private struct PluginSliderRow: View {
    let slider: PluginSlider
    let onSet: (PluginSlider, Float) -> Void

    @State private var moved: Float?

    var body: some View {
        let value = moved ?? slider.value
        ActionSlideRow(
            label: slider.label,
            value: pluginSliderShown(slider: slider, value: value),
            share: pluginSliderAlong(slider: slider, value: value),
            change: { along in
                let to = pluginSliderAt(slider: slider, along: along)
                if slider.live {
                    onSet(slider, to)
                } else {
                    moved = to
                }
            },
            onDone: {
                if let moved {
                    onSet(slider, moved)
                }
                moved = nil
            }
        )
    }
}

/// The one screen that asks whether to import a plugin file: what it is, and what it can do. Also
/// where a setting that has moved to a plugin is said, once.
struct PluginAlerts: View {
    @State private var plugins = PluginsModel.shared
    /// What went wrong keeping a file.
    @State private var failed: String?

    var body: some View {
        ZStack {
            AlertView(
                content: plugins.movedNotice.map {
                    AlertContent(title: "Now a plugin", message: $0, action: "OK", destructive: false, alone: true)
                },
                onCancel: { plugins.movedNoticeRead() },
                onAction: { plugins.movedNoticeRead() }
            )
            AlertView(
                content: plugins.offer.map {
                    AlertContent(title: $0.name, message: $0.message, action: "Import", destructive: false)
                },
                onCancel: { plugins.declineOffer() },
                onAction: { failed = plugins.acceptOffer() }
            )
            AlertView(
                content: failed.map {
                    AlertContent(title: "Can't import this file", message: $0, action: "OK", destructive: false, alone: true)
                },
                onCancel: { failed = nil },
                onAction: { failed = nil }
            )
        }
    }
}

extension CanvasModel {
    /// The session goes by the plugins that reach a canvas.
    /// A plugin's action `key` run on the artwork, off the main thread (it may take seconds): the
    /// panels close so what it does shows, and what it says, or its stopping, is told.
    func runPluginAction(_ key: String) {
        closePanels()
        let session = session
        let plugins = PluginsModel.shared.store
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            _ = plugins.runAction(key: key, session: session)
            let panel = plugins.panel()
            DispatchQueue.main.async { self?.pluginAnswered(panel) }
        }
    }

    /// Row `row` of the plugin's panel touched with `value`: told to the plugin off the main thread,
    /// and the panel shown as it answers (or closed).
    func touchPluginPanel(_ row: Int, _ value: Float) {
        let session = session
        let plugins = PluginsModel.shared.store
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            _ = plugins.panelTouched(row: UInt32(row), value: value, session: session)
            let panel = plugins.panel()
            DispatchQueue.main.async { self?.pluginAnswered(panel) }
        }
    }

    func closePluginPanel() {
        PluginsModel.shared.store.closePanel()
        pluginPanel = nil
    }

    private func pluginAnswered(_ panel: PluginPanel?) {
        pluginPanel = panel
        if let failure = session.takeFailure() {
            showNotice(failure, warning: true)
        }
        PluginsModel.shared.read()
        refresh()
    }

    func applyPlugins() {
        let plugins = PluginsModel.shared.store
        session.usePlugins(plugins: plugins)
        PluginsModel.shared.pickerSession = session
        session.setInterfaceSize(size: Float(Interface.shared.scale))
        followStrokeSound()
    }

    /// Plays the strokes' sound while a plugin makes it and the canvas is on screen in front.
    func followStrokeSound() {
        StrokeSound.shared.follow(hearsStrokes ? session : nil, on: PluginsModel.shared.store.makesStrokeSound())
    }

    func setPluginFilterSlider(_ key: String, _ value: Float) {
        _ = session.pluginFilterSlider(key: key, value: value, done: false)
        readAdjust()
    }

    /// The finger left the slider `key`: a filter too slow to follow it redraws now.
    func letGoPluginFilter(_ key: String) {
        for control in adjustShown?.plugin?.controls ?? [] {
            if case let .slider(slider) = control, slider.key == key {
                _ = session.pluginFilterSlider(key: key, value: slider.value, done: true)
            }
        }
    }

    func setPluginFilterSwitch(_ key: String, _ on: Bool) {
        session.pluginFilterSwitch(key: key, on: on)
        readAdjust()
    }

    /// A filter that stopped was closed by the core, which says why.
    func checkPluginFilter() {
        guard adjustment == .plugin, session.adjustmentShown().adjustment == nil else { return }
        if let failure = session.takeFailure() {
            showNotice(failure, warning: true)
        }
        adjustMenuAt = nil
        readAdjust()
        refresh()
    }
}

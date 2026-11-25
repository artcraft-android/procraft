import AVFoundation
import Observation
import os
import SwiftUI
import UIKit
import UniformTypeIdentifiers

/// ProCraft on the iPad: the gallery, and an artwork open on the canvas the core draws, painted
/// on with the Pencil and moved with fingers, the interface over it.
@main
struct ProCraftApp: App {
    init() {
        // the log kept in a file too, for Help's Share log
        AppLog.start()
        // the layers may take half of what the app may use (an iPad app is ended well short of the
        // device's memory, so not half the device's as on Android), their textures a quarter: past
        // that, those not worked on are kept compressed off the GPU until needed. None said (the
        // simulator): the core's own 2 GiB, and all the textures kept
        let available = os_proc_available_memory()
        if available > 0 {
            setLayerBudget(bytes: UInt64(available / 2))
            setResidentBudget(bytes: UInt64(available / 4))
            // and timeline frames kept for scrubbing and playing an eighth (the core's own
            // default where none is said)
            setFrameCacheBudget(bytes: UInt64(available / 8))
        }
        // the layers' pixels may take half the device's memory: it sets how many layers a canvas
        // holds (set before Custom Canvas asks), more on a bigger iPad
        setLayerBudget(bytes: ProcessInfo.processInfo.physicalMemory / 2)
        // colour profiles imported (Custom Canvas's Import) are kept, and read again at each start
        setProfileFolder(dir: Self.profiles.path)
        TextFonts.prepare()
        // artworks being opened, noted until they show: one the app ended while opening is asked
        // about when next picked, not opened at once into the same end
        setOpenMarksFile(path: Gallery.openMarks.path)
        // the GPU device and the canvases' pipelines made ahead of the first artwork
        DispatchQueue.global(qos: .userInitiated).async { warmGpu() }
        // and the brushes read: with many brought in, reading them as an artwork opened took seconds
        DispatchQueue.global(qos: .utility).async { onBigStack(qos: .utility) { prepareBrushes(dir: CanvasModel.brushesDir) } }
        // the home screen's widget shows the artwork worked on last
        RecentArtwork.start()
    }

    var body: some Scene {
        WindowGroup {
            RootView()
        }
    }

    /// Where imported colour profiles are kept, in the app's own folder.
    private static var profiles: URL {
        let support = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first
            ?? URL(fileURLWithPath: NSTemporaryDirectory())
        return support.appendingPathComponent("icc")
    }
}

/// An artwork open on the canvas, and its folder in the gallery; opened from the gallery's Share for
/// an animated export alone, which format.
struct OpenArtwork {
    let session: CanvasSession
    let dir: String
    var animated: AnimatedFormat? = nil
}

/// The artwork open on the canvas (none while the gallery shows), and opening one, kept out of the
/// views. A closure that captures a view captures its state as it was then: the gallery kept the
/// root view's open handler, and while it zoomed into an artwork that state held the artwork's
/// model, so every artwork opened from the gallery was kept, all its memory with it, after it was
/// left. What the gallery keeps now (`open`) holds this alone.
@Observable
final class OpenCanvas {
    static let shared = OpenCanvas()

    private(set) var model: CanvasModel?
    /// The artwork open was opened for its animated export alone: when that ends, it closes.
    var exportOnly = false
    /// Pixels to the window's point, as the root view has them.
    @ObservationIgnored var pixels: CGFloat = 2
    /// What the open artwork's session had done by itself when last logged, and when.
    @ObservationIgnored private var activityLogged: (at: Date, activity: Activity)?

    /// An artwork opened from the gallery (or made, or brought in): the gallery zooms into it from
    /// its cell, if it has one showing.
    func open(_ artwork: OpenArtwork) {
        let model = CanvasModel(session: artwork.session, dir: artwork.dir)
        model.onClose = { OpenCanvas.shared.close() }
        GalleryZoomer.shared.open(artwork.session, dir: artwork.dir, window: Interface.shared.window, pixels: pixels)
        activityLogged = nil
        show(model)
        logActivity(front: true, of: model)
        if let format = artwork.animated {
            exportOnly = true
            startExport(format, on: model)
        }
    }

    /// `model` on the canvas in place of the gallery.
    func show(_ model: CanvasModel) {
        self.model = model
    }

    /// Back to the gallery.
    func close() {
        model = nil
        exportOnly = false
    }

    /// Opens `format`'s export over the artwork in `model` once its canvas is made (the frames are
    /// read from it): a model's turntable, else its layers.
    private func startExport(_ format: AnimatedFormat, on model: CanvasModel) {
        Task { @MainActor [weak model] in
            while let open = model, OpenCanvas.shared.model === open, !open.session.canvasMade() {
                try? await Task.sleep(for: .milliseconds(16))
            }
            guard let model, OpenCanvas.shared.model === model else { return }
            if model.session.modelInfo() != nil {
                model.scene.openAnimatedExport(format)
            } else {
                model.openAnimatedExport(format)
            }
        }
    }

    /// Puts in the log what the open artwork's session did by itself while the app was in front (now
    /// going away, not `front`) or away (now back): frames drawn, the render thread's wakeups and
    /// retries, the autosave's looks. Away, all should be none. An artwork just opened (`model`) is
    /// only noted, for the next.
    func logActivity(front: Bool, of model: CanvasModel? = nil) {
        guard let open = model ?? self.model else {
            activityLogged = nil
            return
        }
        let session = open.session
        let now = Date()
        let before = activityLogged
        weak var opened = open
        // off the main thread: the session may be drawing
        DispatchQueue.global(qos: .utility).async {
            let after = session.activity()
            DispatchQueue.main.async {
                if let before {
                    let seconds = Int(now.timeIntervalSince(before.at))
                    let b = before.activity
                    AppLog.info(
                        "activity",
                        "\(front ? "away" : "in front") \(seconds) s: \(after.frames &- b.frames) frames drawn, "
                            + "\(after.wakeups &- b.wakeups) render wakeups, \(after.retries &- b.retries) retries, "
                            + "\(after.autosaveLooks &- b.autosaveLooks) autosave looks"
                    )
                }
                if let opened, OpenCanvas.shared.model === opened {
                    OpenCanvas.shared.activityLogged = (now, after)
                }
            }
        }
    }
}

/// The gallery, or the artwork open in place of it.
struct RootView: View {
    /// The artwork open: only read here (see `OpenCanvas`).
    private var open: CanvasModel? {
        OpenCanvas.shared.model
    }

    @Environment(\.displayScale) private var displayScale
    @Environment(\.scenePhase) private var scenePhase

    @ViewBuilder private var screens: some View {
        let zoomer = GalleryZoomer.shared
        ZStack {
            if open == nil {
                Pc.galleryBackground.ignoresSafeArea()
            }
            if let open {
                CanvasScreen(model: open)
                    .statusBarHidden()
                    .persistentSystemOverlays(.hidden)
                    .defersSystemGestures(on: .all)
            }
            // the gallery, kept over the canvas while it zooms into it; the same gallery all the
            // while, so it keeps its place
            if open == nil || zoomer.zoom?.opening == true {
                // (the holder's, which keeps no view)
                GalleryView(onOpen: OpenCanvas.shared.open)
                    .modifier(ZoomedGallery(t: zoomer.at, zoom: zoomer.zoom))
                    .opacity(zoomer.galleryShown)
                    .allowsHitTesting(zoomer.zoom == nil)
                    .ignoresSafeArea(.container)
            }
            // a canvas left fading away as it shrinks into its cell
            if let zoom = zoomer.zoom, !zoom.opening {
                ClosingPicture(t: zoomer.at, zoom: zoom)
                    .ignoresSafeArea()
            }
        }
    }

    /// The system short of memory, or the app put away: the canvas lets go of what it keeps only to
    /// be quicker (made again when next wanted).
    private func trimMemory() {
        guard let session = open?.session else { return }
        DispatchQueue.global(qos: .utility).async { session.trimMemory() }
    }

    /// Whether `url` is a plugin file, by its name (the old ending still opens).
    private static func isPluginFile(_ url: URL) -> Bool {
        let name = url.lastPathComponent.lowercased()
        return name.hasSuffix(".procraft-plugin") || name.hasSuffix(".procraftplugin")
    }

    /// Asks about the plugin file at `url`: nothing in it runs, and nothing is kept until it is
    /// accepted. What can't be imported says why, on the canvas or the gallery. The system's copy
    /// is moved where it is cleared away in time.
    private func offerPlugin(_ url: URL) {
        var file = url
        if url.pathComponents.contains("Inbox") {
            let moved = FileManager.default.temporaryDirectory.appendingPathComponent("\(UUID().uuidString)-\(url.lastPathComponent)")
            if (try? FileManager.default.moveItem(at: url, to: moved)) != nil {
                file = moved
            }
        }
        PluginsModel.shared.offer(file) { said in
            if let open {
                open.showNotice(said, warning: true)
            } else {
                GalleryModel.shared.message = said
            }
        }
    }

    var body: some View {
        screens
            // an artwork opened from the gallery's Share for its animated export goes back once it ends
            .onChange(of: open?.animatedExport == nil && open?.scene.animatedExport == nil) { _, ended in
                if ended, OpenCanvas.shared.exportOnly, let open {
                    OpenCanvas.shared.exportOnly = false
                    open.onClose()
                }
            }
            .background {
                GeometryReader { place in
                    Color.clear.onChange(of: place.size, initial: true) { _, size in
                        Interface.shared.fit(size)
                    }
                }
                .ignoresSafeArea()
            }
            .overlay { PluginAlerts() }
            .onChange(of: scenePhase) { _, phase in
                if phase == .background {
                    OpenCanvas.shared.logActivity(front: false)
                    trimMemory()
                } else if phase == .active {
                    OpenCanvas.shared.logActivity(front: true)
                }
            }
            .onChange(of: displayScale, initial: true) { _, scale in
                OpenCanvas.shared.pixels = scale
            }
            .onReceive(NotificationCenter.default.publisher(for: UIApplication.didReceiveMemoryWarningNotification)) { _ in
                trimMemory()
            }
            // a file opened with the app is brought into the gallery, which shows: an open artwork
            // is saved and closed first. A plugin is asked about where the app stands: it needs no
            // artwork closed
            .onOpenURL { url in
                // the home screen's widget: straight into the artwork worked on last
                if RecentArtwork.isOpenRecent(url) {
                    RecentArtwork.openMostRecent(unlessOpen: open != nil)
                    return
                }
                if Self.isPluginFile(url) {
                    offerPlugin(url)
                    return
                }
                // the artwork open is left (saved) first; one that can't be saved stays, and the file
                // comes in only if it is left without saving
                guard let open else {
                    GalleryModel.shared.importOpened(url)
                    return
                }
                open.afterLeaving = { GalleryModel.shared.importOpened(url) }
                open.onClose()
            }
        #if DEBUG
        .task {
            ScreenCapture.start { model in
                if let model {
                    OpenCanvas.shared.show(model)
                } else {
                    OpenCanvas.shared.close()
                }
            }
        }
        #endif
    }
}

#if DEBUG
/// For the screen captures the iPad workflow takes on the simulator: launched with `-screen NAME`,
/// the app turns on its side and opens a new canvas the size of the screen with the panel NAME
/// open (`canvas` for none, `gallery` for the gallery), or one of the screens below it: a menu of
/// the library, a page of the brush studio, something open over it, a source editor. Nothing can
/// be touched in a capture, so what a touch would open is opened here.
enum ScreenCapture {
    /// What the studio opens over its page once it is up.
    static var studioShows: ((StudioModel, StudioUi) -> Void)?
    /// The control its page is scrolled to.
    static var studioRow: String?
    /// The page the keypad opens on.
    static var keypadPage = KeypadPage.numeric
    /// What the source editor opens over itself, and what the brush library opens.
    static var sourceShows: String?
    static var libraryShows: String?
    /// The page the actions panel opens on.
    static var actionsPage = ActionsPage.main
    /// What the layers panel opens, and the colour panel.
    static var layersShows: String?
    static var colorsShows: String?
    /// The studio laid out on its side: the simulator stays upright whatever is asked of it.
    static var sideways = false
    /// The screens, by how their names begin, that need the canvas attached.
    private static let canvasScreens = ["liquify", "adjust-", "layers-adjustment", "layers-style", "transform", "selection", "effect-", "crop", "recolor", "layer-opacity", "guide", "text", "reference", "timelapse", "animation", "page-assist", "export-", "timeline", "ribbon", "light", "hdr", "texture-packer", "layers-vector", "puppet", "comic-panels"]
    /// What opens over the gallery.
    static var galleryShows: String?
    /// Animation Assist's settings open on the onion skins' colours.
    static var animationColors = false
    /// The Adjustments menu opens on the filters.
    static var filtersShow = false
    /// A gradient's second stop opens its colours, in this mode.
    static var stopOpens = false
    static var compactMode: ColorMode?
    /// What opens over the QuickMenu.
    static var quickShows: String?
    /// The Adjustments menu opens with Adjustment Layer on; a layer's styles open on this one.
    static var adjustAsLayer = false
    static var styleOpens: Int?
    /// The timeline's settings open on the onion skins' page.
    static var onionOpens = false

    /// The studio's pages, by capture.
    private static let pages: [String: Int] = [
        "studio": 3, "studio-taper": 2, "studio-grain": 4, "studio-stylus": 9, "studio-materials": 11,
        "studio-preview": 12, "studio-about": 13,
    ]

    /// Where a value is in the portrait studio, for a keypad to hang from.
    private static let pill = CGRect(x: 648, y: 951, width: 72, height: 28)

    private static func studio(_ model: CanvasModel, page: Int, shows: ((StudioModel, StudioUi) -> Void)? = nil) {
        AppSettings.shared.studioTab = page
        studioShows = shows
        model.studio.open()
    }

    private static func keypad(_ page: KeypadPage) -> (StudioModel, StudioUi) -> Void {
        { studio, ui in
            keypadPage = page
            ui.keypad = KeypadAsk(
                id: "count", title: "Count", pill: pill, typed: typedNumber(.count, most: 16),
                onValue: { _ in }, extras: keypadExtras(studio, "count")
            )
        }
    }

    private static func source(_ model: CanvasModel, _ kind: SourceKind, shows: String? = nil) {
        sourceShows = shows
        model.studio.open()
        model.studio.sourceEditing = kind
    }

    /// A few artworks for the gallery's captures to show.
    static func seedGallery(_ gallery: GalleryModel) {
        if gallery.entries.isEmpty {
            for size in [(1200, 900), (900, 1200), (1000, 1000)] {
                let session = CanvasSession(width: UInt32(size.0), height: UInt32(size.1))
                try? session.save(dir: Gallery.store.newFolder(folder: nil))
            }
            gallery.reload()
        }
        // never drawn, they have no pictures of their own, and Preview and a spread show one
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        for (artwork, colour) in zip(gallery.artworks, [UIColor.systemTeal, .systemOrange, .systemPurple]) {
            let size = CGSize(width: CGFloat(artwork.width) / 4, height: CGFloat(artwork.height) / 4)
            let png = UIGraphicsImageRenderer(size: size, format: format).pngData { context in
                colour.setFill()
                context.fill(CGRect(origin: .zero, size: size))
                UIColor.white.setFill()
                context.cgContext.fillEllipse(in: CGRect(origin: .zero, size: size).insetBy(dx: size.width * 0.3, dy: size.height * 0.3))
            }
            try? png.write(to: URL(fileURLWithPath: artwork.dir).appendingPathComponent("thumbnail.png"))
        }
        gallery.artworkSaved()
    }

    /// Transform open over a filled layer: on an empty one there is nothing to move.
    private static func transform(_ model: CanvasModel) {
        if let l = model.layers.first(where: { $0.current }) {
            model.fillLayer(l)
        }
        model.toggleTransform()
    }

    /// The layers panel over a few layers, what `shows` opens open.
    private static func layers(_ model: CanvasModel, shows: String) {
        layersShows = shows
        model.addLayer()
        model.addLayer()
        model.addLayer()
        if shows == "picked", let other = model.layers.first(where: { !$0.current }) {
            model.pickLayer(other)
        }
        model.toggle(.layers)
    }

    /// The folder of plugin files the capture packed.
    private static var pluginFiles: String? {
        ProcessInfo.processInfo.environment["CAPTURE_PLUGINS"]
    }

    /// Brings in the plugin files packed for the captures, as a person's are: read, then kept.
    private static func importPlugins() {
        guard let folder = pluginFiles, let names = try? FileManager.default.contentsOfDirectory(atPath: folder) else { return }
        let plugins = PluginsModel.shared
        // the old name still opens
        for name in names where name.hasSuffix(".procraft-plugin") || name.hasSuffix(".procraftplugin") {
            guard let bytes = FileManager.default.contents(atPath: folder + "/" + name) else { continue }
            if plugins.store.inspect(bytes: bytes).refused == nil {
                _ = plugins.store.install(bytes: bytes)
            }
        }
        plugins.read()
    }

    /// The plugins holding a filter turned on, as a person would (the app's own come in off).
    private static func filtersOn() {
        importPlugins()
        let plugins = PluginsModel.shared
        for plugin in plugins.plugins where plugin.holds.contains(.filter) && !plugin.on {
            plugins.setOn(plugin, true)
        }
    }

    /// The plugin called `name`, on.
    private static func apply(_ name: String) -> PluginInfo? {
        importPlugins()
        let plugins = PluginsModel.shared
        guard let plugin = plugins.plugins.first(where: { $0.name == name }) else { return nil }
        plugins.setOn(plugin, true)
        return plugins.plugins.first { $0.name == name }
    }

    /// The interface at `size`, through its plugin.
    private static func sized(_ size: Float) {
        for control in apply("Interface size")?.controls ?? [] {
            if case let .slider(slider) = control {
                PluginsModel.shared.setSlider(slider, size)
            }
        }
    }

    /// Frames of the canvas in four colours recorded, off the main thread; then `then`.
    /// A line on a timeline keyed to fall and turn a quarter over eight frames, seen half way;
    /// returns its clip.
    @discardableResult
    static func timelineKeyed(_ model: CanvasModel) -> UInt64? {
        model.timeline.setOn(true)
        let s = model.session
        let at = { (k: Int) in PenSample(x: 700 + Float(k) * 20, y: 1100, pressure: 0.6, tilt: 0, orientation: 0, timeMs: Double(k) * 8, roll: 0) }
        s.strokeBegin(pointer: .stylus, samples: [at(0)])
        s.strokeAdd(samples: (1 ... 30).map(at))
        s.strokeEnd(samples: [])
        guard let clip = s.timelineTracks().first?.clips.first?.id else { return nil }
        _ = s.setTimelineClipDuration(id: clip, duration: 8, push: true)
        s.setTimelineFrame(frame: 0)
        _ = s.setTimelineKey(id: clip, channel: .position, values: [0, 0], easing: .smooth, done: true)
        _ = s.setTimelineKey(id: clip, channel: .rotation, values: [0], easing: .smooth, done: true)
        s.setTimelineFrame(frame: 7)
        _ = s.setTimelineKey(id: clip, channel: .position, values: [0, 500], easing: .smooth, done: true)
        _ = s.setTimelineKey(id: clip, channel: .rotation, values: [90], easing: .smooth, done: true)
        s.setTimelineFrame(frame: 3)
        model.refresh()
        return clip
    }

    private static func recordFrames(_ model: CanvasModel, then: @escaping () -> Void) {
        let session = model.session
        let lapse = model.timeLapse.lapse
        DispatchQueue.global(qos: .userInitiated).async {
            for colour: [Float] in [[1, 0, 0], [0, 1, 0], [0, 0, 1], [1, 1, 0]] {
                _ = session.setBackgroundColor(r: colour[0], g: colour[1], b: colour[2], done: true)
                lapse.record(from: session)
            }
            DispatchQueue.main.async(execute: then)
        }
    }

    /// The middle pixel of `picture`, as hex.
    private static func middle(of picture: UIImage?) -> String {
        guard let image = picture?.cgImage else { return "none" }
        var pixel: [UInt8] = [0, 0, 0, 0]
        let space = CGColorSpace(name: CGColorSpace.sRGB) ?? CGColorSpaceCreateDeviceRGB()
        pixel.withUnsafeMutableBytes { bytes in
            let context = CGContext(
                data: bytes.baseAddress, width: 1, height: 1, bitsPerComponent: 8, bytesPerRow: 4,
                space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            )
            context?.draw(image, in: CGRect(x: -image.width / 2, y: -image.height / 2, width: image.width, height: image.height))
        }
        return String(format: "%02X%02X%02X", pixel[0], pixel[1], pixel[2])
    }

    /// What only a run can show, said on the screen: frames recorded, made into a video and read
    /// back out of it, as a person's are.
    private static func checkTimeLapse(_ model: CanvasModel) {
        let lapse = model.timeLapse.lapse
        recordFrames(model) {
            DispatchQueue.global(qos: .userInitiated).async {
                let frames = lapse.count
                let out = Sharing.exportsDir.appendingPathComponent("check.mp4")
                let made = lapse.export(to: out, seconds: nil) { _ in true }
                let asset = AVURLAsset(url: out)
                let size = asset.tracks(withMediaType: .video).first?.naturalSize ?? .zero
                let bytes = (try? out.resourceValues(forKeys: [.fileSizeKey]).fileSize) ?? 0
                // the pictures gone, so these come out of the video
                let package = lapse.forPackage()
                let text = "\(frames) frames recorded. Made: \(made), \(asset.duration.seconds) s, \(Int(size.width)) x \(Int(size.height)), \(bytes) bytes. "
                    + "Package: \(package.segments.count) videos, \(package.width) x \(package.height). "
                    + "From the video: first \(middle(of: lapse.frame(0))), last \(middle(of: lapse.frame(frames - 1))), count \(lapse.count)."
                DispatchQueue.main.async { model.saying = ("Time-lapse check", text) }
            }
        }
    }

    /// The actions panel on its Prefs tab, turned to `page`.
    private static func prefs(_ model: CanvasModel, _ page: ActionsPage) {
        actionsPage = page
        model.actionsTab = 4
        model.toggle(.actions)
    }

    /// The colour panel in `mode`, what `shows` opens open.
    private static func colors(_ model: CanvasModel, _ mode: ColorMode, shows: String? = nil) {
        colorsShows = shows
        model.colorMode = mode
        model.toggle(.color)
    }

    private static func library(_ model: CanvasModel, shows: String) {
        libraryShows = shows
        model.toolPressed(.paint)
    }

    /// Every brush library's card, what `shows` opens open.
    private static func libraries(_ model: CanvasModel, shows: String) {
        model.showingLibraries = true
        library(model, shows: shows)
    }

    /// Colourful strokes across the canvas (window points), for the effects to change.
    private static func paintStrokes(_ model: CanvasModel) {
        let s = model.session
        s.setBrushSize(slider: 0.5)
        for (k, hue) in [Float(0.0), 0.58, 0.14].enumerated() {
            model.setColor(hue: hue, saturation: 0.8, brightness: 0.9)
            let y = 420 + Float(k) * 260
            let at = { (i: Int) in
                PenSample(x: 500 + Float(i) * 40, y: y + 80 * sin(Float(i) / 3), pressure: 0.8, tilt: 0, orientation: 0, timeMs: Double(i) * 8, roll: 0)
            }
            s.strokeBegin(pointer: .stylus, samples: [at(0)])
            s.strokeAdd(samples: (1 ... 40).map(at))
            s.strokeEnd(samples: [])
        }
        model.refresh()
    }

    /// The effects' screens: an adjustment open on the strokes (or as an adjustment layer), or the
    /// layers panel over an adjustment layer, or a layer's styles.
    private static func effects(_ name: String, _ model: CanvasModel) {
        paintStrokes(model)
        switch name {
        case "effect-layer":
            model.addAdjustmentLayer(.hsb)
            model.adjust { $0.adjustmentHsb(hue: 0.75, saturation: 0.5, brightness: 0.5) }
        case "layers-adjustment":
            model.addAdjustmentLayer(.gradientMap)
            model.endAdjustment(commit: true)
            model.toggle(.layers)
        case "layers-style":
            if let l = model.layers.first(where: { $0.current }) {
                var effects = model.layerEffects(l)
                if effects.count >= 4 {
                    effects[0].on = true
                    effects[0].size = 6
                    effects[1].on = true
                    effects[2].on = true
                    for i in 0 ..< 3 {
                        model.setLayerEffect(l, i, effects[i], done: true)
                    }
                }
            }
            layersShows = "style"
            styleOpens = 1
            model.toggle(.layers)
        case "layers-style-color":
            // the strokes all one colour, as opaque as they were, and its page open
            if let l = model.layers.first(where: { $0.current }) {
                var effects = model.layerEffects(l)
                if effects.count > 4 {
                    effects[4].on = true
                    (effects[4].r, effects[4].g, effects[4].b) = (0.1, 0.55, 0.35)
                    model.setLayerEffect(l, 4, effects[4], done: true)
                }
            }
            layersShows = "style"
            styleOpens = 4
            model.toggle(.layers)
        default:
            break
        }
    }

    /// The vector layers plugin on, and a layer of its kind over the current one.
    private static func vectorLayer(_ model: CanvasModel) {
        _ = apply("Vector layers")
        if let kind = model.layerKinds().first {
            model.addPluginLayer(kind.key)
        }
    }

    /// `steps` run one after another a fifth of a second apart: strokes given all at once, with no
    /// frame between them, aren't all taken.
    private static func paced(_ steps: [() -> Void]) {
        for (i, step) in steps.enumerated() {
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.2 * Double(i), execute: step)
        }
    }

    /// A stroke through `points` (window pixels) at `pressure` for each.
    private static func stroke(_ s: CanvasSession, _ points: [(Float, Float, Float)]) {
        let samples = points.enumerated().map { k, p in
            PenSample(x: p.0, y: p.1, pressure: p.2, tilt: 0, orientation: 0, timeMs: Double(k) * 8, roll: 0)
        }
        guard let first = samples.first, let last = samples.last else { return }
        s.strokeBegin(pointer: .stylus, samples: [first])
        s.strokeAdd(samples: Array(samples.dropFirst().dropLast()))
        s.strokeEnd(samples: [last])
    }

    /// A wave across the canvas at `y`, the pressure swelling from light to full and back.
    private static func wave(_ s: CanvasSession, y: Float) {
        stroke(s, (0 ... 60).map { k in
            let t = Float(k) / 60
            return (300 + Float(k) * 28, y + 70 * sin(t * 2 * .pi), 0.25 + 0.75 * sin(t * .pi))
        })
    }

    /// A straight line from `from` to `to` at full pressure.
    private static func line(_ s: CanvasSession, _ from: (Float, Float), _ to: (Float, Float)) {
        stroke(s, (0 ... 40).map { k in
            let t = Float(k) / 40
            return (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t, 1)
        })
    }

    /// Ribbon brushes drawn on the canvas: a heart stamped, then as a ribbon, a star stretched
    /// along and a leaf, each with the brush the paint tool has, changed only while drawn (the
    /// studio's edit put back after).
    private static func ribbons(_ model: CanvasModel) {
        let s = model.session
        s.setBrushSize(slider: 0.7)
        s.setBrushOpacity(slider: 1)
        // (shape, ribbon, its scale, colour)
        let rows: [(UInt32, Float, Float, [Float])] = [
            (37, 0, 0.5, [0.8, 0.2, 0.3]), (37, 1, 0.5, [0.8, 0.2, 0.3]),
            (18, 1, 0.75, [0.95, 0.65, 0.1]), (8, 1, 0.5, [0.2, 0.6, 0.3]),
        ]
        paced(rows.enumerated().map { i, row in
            {
                _ = s.studioBegin()
                _ = s.setStudioShippedSource(kind: .shape, index: row.0)
                s.setStudioSetting(key: "ribbon", value: row.1)
                s.setStudioSetting(key: "ribbon_scale", value: row.2)
                s.setColor(r: row.3[0], g: row.3[1], b: row.3[2])
                wave(s, y: 760 + Float(i) * 330)
                s.studioEnd(keep: false)
                model.refresh()
            }
        })
    }

    /// The light tool over bands of colour: brightened down one stripe, dimmed down another, its
    /// panel open.
    private static func lit(_ model: CanvasModel) {
        let s = model.session
        s.setBrushSize(slider: 0.9)
        s.setBrushOpacity(slider: 1)
        let bands: [[Float]] = [[0.8, 0.3, 0.2], [0.2, 0.45, 0.8], [0.3, 0.65, 0.3], [0.85, 0.7, 0.2]]
        var steps: [() -> Void] = []
        for (i, c) in bands.enumerated() {
            for k in 0 ..< 3 {
                let y = 720 + Float(i) * 340 + Float(k) * 80
                steps.append {
                    s.setColor(r: c[0], g: c[1], b: c[2])
                    line(s, (250, y), (2100, y))
                }
            }
        }
        steps.append {
            model.select(.light)
            s.setBrushSize(slider: 0.9)
            s.setBrushOpacity(slider: 1)
            model.setLight(LightSettings(kind: .brighten, strength: 0.6))
            line(s, (800, 640), (800, 2080))
        }
        steps.append {
            model.setLight(LightSettings(kind: .dim, strength: 0.6))
            line(s, (1500, 640), (1500, 2080))
        }
        steps.append {
            model.refresh()
            model.toggle(.brushes)
        }
        paced(steps)
    }

    /// Opens what capture `name` shows, if it is one of these.
    private static func open(_ name: String, in model: CanvasModel) {
        if ExportDebug.scenes.contains(name) {
            ExportDebug.open(name, in: model)
            return
        }
        if MediaDebug.scenes.contains(name) {
            MediaDebug.open(name, in: model)
            return
        }
        if TimelineLanesDebug.scenes.contains(name) {
            TimelineLanesDebug.open(name, in: model)
            return
        }
        if TimelineBigDebug.scenes.contains(name) {
            TimelineBigDebug.open(name, in: model)
            return
        }
        if TimelineGridDebug.scenes.contains(name) {
            TimelineGridDebug.open(name, in: model)
            return
        }
        if let page = pages[name] {
            studio(model, page: page)
            return
        }
        switch name {
        case "studio-roundness":
            studioRow = "roundness"
            studio(model, page: 3)
        case "studio-tilt":
            studioRow = "tilt"
            studio(model, page: 9)
        case "studio-landscape":
            sideways = true
            studio(model, page: 3)
        case "keypad":
            studio(model, page: 3, shows: keypad(.numeric))
        case "keypad-pressure":
            studio(model, page: 3, shows: keypad(.pressure))
        case "keypad-tilt":
            studio(model, page: 3, shows: keypad(.tilt))
        case "blend-list":
            studio(model, page: 4) { studio, ui in
                ui.blendList = BlendListAsk(modes: studio.brushModes, current: 0, centreY: 700) { _ in }
            }
        case "node-menu":
            studio(model, page: 9) { _, ui in
                let points = [CGPoint.zero, CGPoint(x: 0.5, y: 0.6), CGPoint(x: 1, y: 1)]
                let box = CGRect(x: 109, y: 620, width: 607, height: 450)
                ui.nodeMenu = NodeMenuAsk(points: points, index: 1, box: box) { _ in }
            }
        case "pad-menu":
            studio(model, page: 3) { studio, _ in
                studio.padButtonCentre = 176
                studio.padMenuOpen = true
            }
        case "source":
            source(model, .grain)
        case "source-shape":
            source(model, .shape)
        case "source-import":
            source(model, .grain, shows: "import")
        case "repeat":
            source(model, .grain, shows: "repeat")
        case "source-library":
            source(model, .grain, shows: "library")
        case "source-landscape":
            sideways = true
            source(model, .grain)
        case "licences":
            model.licencesOpen = true
        case "canvas-info":
            model.canvasInfoOpen = true
        case "text":
            model.text.add()
        case "text-style":
            model.text.add()
            model.text.openStyle()
        case "guide":
            model.setGuideOn(true)
        case "guide-edit":
            model.guide.open()
        case "guide-symmetry":
            model.guide.open()
            model.guide.choose(.symmetry)
            model.guide.symmetryOptionsOpen = true
        case "recolor":
            model.toggleRecolor()
        case "layer-opacity":
            model.startLayerOpacity()
        case "crop":
            model.crop.begin()
        case "crop-settings":
            model.crop.begin()
            model.crop.settingsOpen = true
        case "adjust-modes":
            model.startAdjustment(.gaussianBlur)
            model.adjustModesOpen = true
        case "colors-floating", "colors-floating-palettes", "colors-floating-harmony", "colors-floating-value":
            model.colorMode = ColorMode.allCases.first { name.hasSuffix($0.label.lowercased()) } ?? .disc
            model.floatingColorsAt = CGPoint(x: 300, y: 160)
        case "animation", "animation-settings", "animation-frame", "animation-colors":
            model.animationAssist.setAssist(true, pages: false)
            for _ in 0 ..< 3 {
                model.addLayer()
            }
            model.animationAssist.settingsOpen = name == "animation-settings" || name == "animation-colors"
            model.animationAssist.frameOptionsOpen = name == "animation-frame"
            if name == "animation-colors" {
                animationColors = true
            }
        case "studio-ribbon":
            // the Shape page at its Ribbon section, the ribbon on (its preview drawn as one)
            studioRow = "ribbon"
            studio(model, page: 3) { studio, _ in
                // a heart's shape to stretch: the round tip has none
                studio.keepSource(.shape, pixels: Data(), shipped: 37)
                studio.set("ribbon", 1)
            }
        case "ribbon":
            ribbons(model)
        case "light":
            lit(model)
        case "texture-packer":
            // three drawings apart and a dot by one, packed, two of them chosen and one renamed
            let s = model.session
            s.setBrushSize(slider: 0.6)
            s.setBrushOpacity(slider: 1)
            var steps: [() -> Void] = []
            let shapes: [((Float, Float), (Float, Float), [Float])] = [
                ((400, 700), (900, 900), [0.85, 0.3, 0.2]),
                ((1300, 650), (1300, 1200), [0.2, 0.45, 0.8]),
                ((500, 1500), (1100, 1400), [0.3, 0.65, 0.3]),
                ((1300, 560), (1302, 560), [0.2, 0.45, 0.8]),
            ]
            for (from, to, c) in shapes {
                steps.append {
                    s.setColor(r: c[0], g: c[1], b: c[2])
                    line(s, from, to)
                }
            }
            steps.append {
                model.openTexturePacker()
                guard let packer = model.texturePacker else { return }
                packer.chosen = [0, 2]
                packer.names[TexturePackerState.key(packer.boxes[0])] = "flag"
                model.packerFocus(packer, 0)
            }
            paced(steps)
        case "puppet", "puppet-bent":
            // the Puppet Warp plugin on, a bar painted, Transform's Puppet over it; bent: two pins
            // put at its ends and the far one taken down
            let s = model.session
            let plugins = PluginsModel.shared
            if let puppet = plugins.plugins.first(where: { $0.id == "io.github.artcraftandroid.puppet-warp" }), !puppet.on {
                plugins.setOn(puppet, true)
            }
            s.setBrushSize(slider: 0.9)
            s.setBrushOpacity(slider: 1)
            s.setColor(r: 0.2, g: 0.35, b: 0.8)
            paced([
                {
                    line(s, (500, 900), (1700, 900))
                    model.refresh()
                },
                {
                    model.toggleTransform()
                    model.transform.choose(.puppet)
                },
                {
                    guard name == "puppet-bent" else { return }
                    _ = s.transformPuppetTap(x: 560, y: 900)
                    _ = s.transformPuppetTap(x: 1640, y: 900)
                    s.transformGrab(handle: .pin(index: 1), x: 1640, y: 900)
                    for k in 1 ... 10 {
                        _ = s.transformDrag(x: 1640 - Float(k) * 20, y: 900 + Float(k) * 40)
                    }
                    if let box = s.transformRelease() {
                        model.transform.moved(box)
                    }
                    model.refreshView()
                },
            ])
        case "comic-panels", "comic-panels-cutting":
            // the Comic Panels plugin on, its tool opened (the canvas made a page), cut down the
            // middle and its left half across; cutting: a line being drawn across the right half
            let plugins = PluginsModel.shared
            if let comic = plugins.plugins.first(where: { $0.id == "io.github.artcraftandroid.comic-panels" }), !comic.on {
                plugins.setOn(comic, true)
            }
            paced([
                {
                    guard let tool = PluginsModel.shared.tools.first else { return }
                    model.openPluginTool(tool)
                },
                {
                    // window points: the page fitted under the header, in the middle across
                    let screen = UIScreen.main.bounds
                    let mid = max(screen.width, screen.height) / 2
                    let drag = { (a: CGPoint, b: CGPoint, up: Bool) in
                        model.touchPluginTool(.down, at: a)
                        model.touchPluginTool(.moved, at: b)
                        if up {
                            model.touchPluginTool(.up, at: b)
                        }
                    }
                    drag(CGPoint(x: mid, y: 100), CGPoint(x: mid + 2, y: 1100), true)
                    drag(CGPoint(x: 40, y: 500), CGPoint(x: mid - 20, y: 520), true)
                    if name == "comic-panels-cutting" {
                        drag(CGPoint(x: mid + 40, y: 700), CGPoint(x: mid + 600, y: 760), false)
                    }
                },
            ])
        case "hdr":
            // brightened past white: the bands' middles glow on a screen with extended range
            model.setLightLimit(3)
            lit(model)
        case "hdr-exposure":
            // the bands lit, then Curves open on its exposure, a stop and a half up
            lit(model)
            DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
                model.toggle(.brushes)
                model.startAdjustment(.curves)
                model.adjust {
                    $0.adjustmentCurveChannel(channel: 4)
                    $0.adjustmentExposure(k: 0, value: 1.5)
                }
            }
        case "hdr-limit":
            // the bands lifted two stops past white, then Curves open on its limit, held to white
            lit(model)
            DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
                model.toggle(.brushes)
                model.startAdjustment(.curves)
                model.adjust {
                    $0.adjustmentExposure(k: 0, value: 2)
                    $0.adjustmentCurveChannel(channel: 5)
                    $0.adjustmentLimit(k: 0, value: 0)
                }
            }
        case "hdr-export-check":
            // what only a run can show: the HDR HEIC, the OpenEXR and the HLG video made, as
            // hdr-check.* in the exports folder, for a look from outside
            lit(model)
            DispatchQueue.main.asyncAfter(deadline: .now() + 8) {
                let session = model.session
                DispatchQueue.global(qos: .userInitiated).async {
                    let dir = Sharing.exportsDir
                    var said: [String] = []
                    let heic = dir.appendingPathComponent("hdr-check.heic")
                    do {
                        let picture = try session.hdrPicture()
                        if let data = HDRImage.heic(picture), (try? data.write(to: heic)) != nil {
                            said.append("HEIC: \(data.count) bytes, brightest \(picture.brightest)")
                        } else {
                            said.append("HEIC: not encoded")
                        }
                    } catch {
                        said.append("HEIC: \(error)")
                    }
                    let exr = dir.appendingPathComponent("hdr-check.exr")
                    do {
                        try session.exportExr(path: exr.path)
                        said.append("EXR: made")
                    } catch {
                        said.append("EXR: \(error)")
                    }
                    // then as an animation: the picture, and an empty frame after it
                    DispatchQueue.main.sync {
                        model.animationAssist.setAssist(true, pages: false)
                        model.addLayer()
                    }
                    let video = CanvasModel.exportAnimation(session, .hevc, AnimatedChoices(fps: 12))
                    let moved = video.flatMap { url -> URL? in
                        let to = dir.appendingPathComponent("hdr-check.mp4")
                        try? FileManager.default.removeItem(at: to)
                        return (try? FileManager.default.moveItem(at: url, to: to)) != nil ? to : nil
                    }
                    said.append(moved == nil ? "HLG: failed" : "HLG: made")
                    let text = said.joined(separator: "\n")
                    NSLog("HDR export check: %@", text)
                    DispatchQueue.main.async { model.saying = ("HDR export check", text) }
                }
            }
        case "hdr-colors":
            // the colour panel's light above white: the paint lit two stops
            lit(model)
            DispatchQueue.main.asyncAfter(deadline: .now() + 4) {
                model.toggle(.brushes)
                model.setColorStops(2)
                colors(model, .value)
            }
        case "timeline-keys", "timeline-path":
            guard let clip = timelineKeyed(model) else { break }
            // its track's rows of keys out
            if let track = model.timeline.clip(clip)?.0.id {
                model.timeline.gridKeyTracks.insert(track)
            }
            if name == "timeline-path" {
                // the last key bent: its handles show
                model.timeline.bendPath(clip, at: 7)
            }
        case "timeline-matte":
            // a broad black stroke below, a red layer above it masked by it (an alpha matte)
            model.addLayer()
            model.timeline.setOn(true)
            let s = model.session
            let tracks = s.timelineTracks()
            guard tracks.count == 2, let below = tracks[0].clips.first, let above = tracks[1].clips.first else { break }
            model.selectLayer(below.root)
            s.setBrushSize(slider: 1)
            s.setColor(r: 0, g: 0, b: 0)
            let at = { (k: Int) in PenSample(x: 500 + Float(k) * 30, y: 900 + Float(k % 6) * 60, pressure: 1, tilt: 0, orientation: 0, timeMs: Double(k) * 8, roll: 0) }
            s.strokeBegin(pointer: .stylus, samples: [at(0)])
            s.strokeAdd(samples: (1 ... 30).map(at))
            s.strokeEnd(samples: [])
            s.setColor(r: 0.86, g: 0.16, b: 0.16)
            _ = s.fillLayer(id: above.root)
            _ = s.setTimelineClipMask(id: above.id, mask: .alpha)
            model.refresh()
            model.timeline.picked = above.id
        case "timeline-group", "timeline-group-open":
            // four drawings, the first two grouped and cycling
            model.animationAssist.setAssist(true, pages: false)
            for _ in 0 ..< 3 {
                model.addLayer()
            }
            model.timeline.setOn(true)
            let s = model.session
            guard let track = s.timelineTracks().first(where: { $0.clips.count >= 2 }) else { break }
            let group = s.groupTimelineClips(ids: [track.clips[0].id, track.clips[1].id])
            _ = s.setTimelineClipDuration(id: group, duration: 6, push: true)
            _ = s.setTimelineCycle(id: group, length: 2, continueAcross: false)
            model.refresh()
            // (open: its own tracks' rows out under it; else put away)
            _ = s.setTimelineGroupFolded(id: group, folded: name != "timeline-group-open")
            model.refresh()
        case "timeline", "timeline-tracks":
            model.animationAssist.setAssist(true, pages: false)
            for _ in 0 ..< 3 {
                model.addLayer()
            }
            model.timeline.setOn(true)
            if name == "timeline-tracks" {
                model.timeline.addTrack()
            }
        case "export-sheet", "export-frames":
            // a ball bouncing across in four drawings, the low one held, and a second track's
            // drawing over frames 2 and 3; then the sprite sheet screen, or both exports made
            let s = model.session
            model.animationAssist.setAssist(true, pages: false)
            let line = { (x: Float, y: Float, r: Float) in
                let at = { (k: Int) -> PenSample in
                    let t = Float(k) / 24 * 2 * .pi
                    return PenSample(x: x + r * cos(t), y: y + r * sin(t), pressure: 0.8, tilt: 0, orientation: 0, timeMs: Double(k) * 8, roll: 0)
                }
                s.strokeBegin(pointer: .stylus, samples: [at(0)])
                s.strokeAdd(samples: (1 ... 24).map(at))
                s.strokeEnd(samples: [])
            }
            s.setColor(r: 0.9, g: 0.2, b: 0.1)
            for (i, y) in [Float(500), 900, 1300, 900].enumerated() {
                if i > 0 {
                    model.addLayer()
                }
                line(500 + Float(i) * 300, y, 120)
            }
            if let low = s.animationFrames().dropFirst(2).first {
                s.setFrameHold(id: low, frames: 2)
            }
            model.timeline.setOn(true)
            let track = s.addTimelineTrack()
            _ = s.renameTimelineTrack(id: track, name: "Shadow")
            s.setTimelineFrame(frame: 1)
            let clip = s.addTimelineDrawing()
            _ = s.setTimelineClipDuration(id: clip, duration: 2, push: false)
            s.setColor(r: 0.2, g: 0.2, b: 0.3)
            line(1100, 1500, 60)
            s.setTimelineFrame(frame: 0)
            _ = s.setName(name: "Bounce")
            model.refresh()
            // the Sprite Sheet and Harmony plugin on: its sprite sheet's screen, or each made
            let plugins = PluginsModel.shared
            if let exports = plugins.plugins.first(where: { $0.id == "io.github.artcraftandroid.frame-exports" }), !exports.on {
                plugins.setOn(exports, true)
            }
            let shares = plugins.store.shares()
            if name == "export-sheet" {
                model.pluginShare = shares.first { $0.label == "Sprite Sheet" }
            } else {
                DispatchQueue.global(qos: .userInitiated).async {
                    var said: [String] = []
                    for share in shares {
                        if let done = CanvasModel.exportPluginShare(s, share) {
                            said.append("\(share.label): \(done.1 ?? ""), \(done.0.map(\.lastPathComponent))")
                        } else {
                            said.append("\(share.label): failed")
                        }
                    }
                    DispatchQueue.main.async { model.saying = ("Export check", said.joined(separator: "\n")) }
                }
            }
        case "export-gif", "export-mp4":
            model.animationAssist.setAssist(true, pages: false)
            for _ in 0 ..< 3 {
                model.addLayer()
            }
            model.animatedExport = name == "export-gif" ? .gif : .mp4
        case "export-check":
            // what only a run can show: each format made, its size, frames back out of the video
            model.animationAssist.setAssist(true, pages: false)
            recordFrames(model) {
                for _ in 0 ..< 3 {
                    model.addLayer()
                }
                let session = model.session
                DispatchQueue.global(qos: .userInitiated).async {
                    var said: [String] = []
                    for format in [AnimatedFormat.gif, .png, .mp4, .hevc] {
                        let url = CanvasModel.exportAnimation(session, format, AnimatedChoices(fps: 12))
                        let bytes = url.flatMap { try? $0.resourceValues(forKeys: [.fileSizeKey]).fileSize } ?? 0
                        var more = ""
                        if format.video, let url {
                            let asset = AVURLAsset(url: url)
                            more = " \(asset.duration.seconds) s, \(asset.tracks(withMediaType: .video).first?.nominalFrameRate ?? 0) fps"
                        }
                        said.append("\(format.title): \(url == nil ? "failed" : "\(bytes) bytes")\(more)")
                    }
                    let text = "\(session.animationFrameCount()) frames.\n" + said.joined(separator: "\n")
                    DispatchQueue.main.async { model.saying = ("Export check", text) }
                }
            }
        case "page-assist":
            model.animationAssist.setAssist(true, pages: true)
            model.addLayer()
        case "import-card":
            model.importShown = ProgressShown(progress: 0.4, detail: "Inking.brushset")
        case "colors-cmyk":
            _ = model.session.setNewProfile(name: cmykProfiles().first ?? "")
            model.refreshCanvasSpace()
            model.setRgb(0x3366CC)
            colors(model, .value)
        case "projection-check":
            // no second display on the simulator: what is said is what the app has to work with
            model.changeProjectCanvas(true)
            let manifest = Bundle.main.object(forInfoDictionaryKey: "UIApplicationSceneManifest") as? [String: Any]
            let configs = (manifest?["UISceneConfigurations"] as? [String: Any])?.keys.sorted().joined(separator: ", ") ?? "none"
            let named = NSClassFromString("ProCraft.ProjectionSceneDelegate") != nil
            let scenes = UIApplication.shared.connectedScenes.map { $0.session.role.rawValue }.joined(separator: ", ")
            model.saying = ("Projection check", "Project canvas on: \(AppSettings.shared.projectCanvas). Configurations: \(configs). Delegate found: \(named). Scenes: \(scenes).")
        case "companion":
            UserDefaults.standard.set(true, forKey: "singleTouchCompanion")
            model.readAppSettings()
            model.companionMode = .move
        case "key-layers":
            _ = model.runShortcut("l", command: false, shift: false, alt: false)
        case "settings-check":
            UserDefaults.standard.set(false, forKey: "singleTouchCompanion")
            model.readAppSettings()
            let file = Bundle.main.url(forResource: "Root", withExtension: "plist", subdirectory: "Settings.bundle")
            let page = file.flatMap { NSDictionary(contentsOf: $0) }
            let rows = (page?["PreferenceSpecifiers"] as? [Any])?.count ?? 0
            let types = ["procraft", "brushset", "abr", "swatches"].map { "\($0): \(UTType(filenameExtension: $0)?.identifier ?? "none")" }
            let kinds = (Bundle.main.object(forInfoDictionaryKey: "CFBundleDocumentTypes") as? [Any])?.count ?? 0
            model.saying = ("Settings check", "Settings page: \(rows) rows. Document kinds: \(kinds).\n" + types.joined(separator: "\n"))
        case "timelapse-tab":
            recordFrames(model) {
                model.refresh()
                model.actionsTab = 3
                model.toggle(.actions)
            }
        case "timelapse-replay":
            recordFrames(model) { model.timeLapse.replaying = true }
        case "timelapse-purge":
            recordFrames(model) { model.timeLapse.setRecording(false) }
        case "timelapse-length":
            model.timeLapse.lengthAsking = true
        case "timelapse-check":
            checkTimeLapse(model)
        case "reference":
            model.reference.setOn(true)
        case "reference-bars":
            model.reference.setOn(true)
            model.reference.toggleBars()
        case "reference-image":
            model.reference.setOn(true)
            model.reference.setMode(.image)
        case "prefs-plugins":
            importPlugins()
            prefs(model, .plugins)
        case "prefs-plugin-on":
            _ = apply("Touch position offset")
            prefs(model, .plugins)
        case "plugin-offer":
            if let folder = pluginFiles {
                PluginsModel.shared.offer(URL(fileURLWithPath: folder + "/extract-lines.procraft-plugin")) { _ in }
            }
        case "adjust-plugins":
            filtersOn()
            filtersShow = true
            model.toggle(.adjustments)
        case "effect-plugin":
            filtersOn()
            if let l = model.layers.first(where: { $0.current }) {
                model.fillLayer(l)
            }
            if let filter = PluginsModel.shared.store.filters().first {
                model.startPluginFilter(filter)
            }
        case "interface-again":
            // brought back at the size it was left at, nothing else asked of it
            break
        case "interface-small":
            sized(0.85)
        case "interface-large":
            sized(1.15)
            model.toggle(.color)
        case "effect-gradient":
            model.startAdjustment(.gradientMap)
        case "effect-gradient-edit":
            model.startAdjustment(.gradientMap)
            model.gradientMap.edit(model.gradientMap.index)
        case _ where name.hasPrefix("effect-gradient-stop"):
            stopOpens = true
            compactMode = ColorMode.allCases.first { name.hasSuffix($0.label.lowercased()) }
            model.startAdjustment(.gradientMap)
            model.gradientMap.edit(model.gradientMap.index)
        case "effect-hsb":
            model.startAdjustment(.hsb)
        case "effect-balance":
            model.startAdjustment(.colorBalance)
        case "effect-curves":
            model.startAdjustment(.curves)
        case "effect-glitch":
            model.startAdjustment(.glitch)
        case "effect-noise":
            model.startAdjustment(.noise)
        case "effect-bloom":
            model.startAdjustment(.bloom)
        case "effect-halftone":
            model.startAdjustment(.halftone)
        case "effect-chromatic":
            model.startAdjustment(.chromaticAberration)
        case "effect-perspective":
            model.startAdjustment(.perspectiveBlur)
        case "effect-layer", "layers-adjustment", "layers-style",
             "layers-style-color":
            effects(name, model)
        case "layers-vector-merge":
            VectorMergeScene.show(model)
        case _ where EffectsFixesScene.names.contains(name):
            EffectsFixesScene.show(name, model)
        case "adjustments-layer":
            adjustAsLayer = true
            model.toggle(.adjustments)
        case "transform":
            transform(model)
        case "transform-snapping":
            transform(model)
            model.transform.popup = .snapping
        case "transform-interpolation":
            transform(model)
            model.transform.popup = .interpolation
        case "transform-warp":
            transform(model)
            model.transform.choose(TransformKind.warp)
        case "transform-numeric":
            transform(model)
            model.transform.numeric = TransformNumericAsk(angle: false, at: CGPoint(x: 560, y: 400), right: false)
        case "selection":
            model.toggleSelecting()
        case "selection-feather":
            model.toggleSelecting()
            model.selection.choose(.automatic)
            model.selection.openPopup(.feather)
        case "selection-saved":
            model.toggleSelecting()
            model.selection.openPopup(.saved)
        case "quick-menu":
            model.toggleQuickMenu(at: CGPoint(x: 372, y: 500))
        case "quick-menus":
            quickShows = "menus"
            model.toggleQuickMenu(at: CGPoint(x: 372, y: 500))
        case "quick-picker":
            quickShows = "picker"
            model.toggleQuickMenu(at: CGPoint(x: 372, y: 500))
        case "copy-paste":
            model.copyLayer()
            model.copyPasteOpen = true
        case "liquify":
            model.startAdjustment(.liquify)
        case "liquify-mode":
            model.startAdjustment(.liquify)
            model.liquifyPopup = 1
        case "liquify-twirl":
            model.startAdjustment(.liquify)
            model.chooseLiquifyKind(.twirlRight)
            model.liquifyPopup = 2
        case "adjust-menu":
            model.startAdjustment(.liquify)
            model.adjustMenuAt = CGPoint(x: 372, y: 500)
        case "prefs":
            prefs(model, .main)
        case "prefs-cursor":
            prefs(model, .cursor)
        case "prefs-pressure":
            prefs(model, .pressure)
        case "gestures":
            model.gestureControlsOpen = true
        case "gestures-hover":
            model.gesturePage = .hover
            model.gestureControlsOpen = true
        case "gestures-general":
            model.gesturePage = .general
            model.gestureControlsOpen = true
        case "colors-classic":
            colors(model, .classic)
        case "colors-harmony":
            colors(model, .harmony)
        case "colors-harmony-menu":
            colors(model, .harmony, shows: "harmony-menu")
        case "colors-value":
            colors(model, .value)
        case "colors-hex":
            colors(model, .value, shows: "hex")
        case "colors-palettes":
            colors(model, .palettes)
        case "colors-add":
            colors(model, .palettes, shows: "add")
        case "colors-palette-menu":
            colors(model, .palettes, shows: "palette-menu")
        case "colors-background":
            model.toggle(.layers)
            model.pickBackground()
        case "layers-many":
            layers(model, shows: "")
        case "layers-swipe":
            layers(model, shows: "swipe")
        case "layers-options":
            layers(model, shows: "options")
        case "layers-blend":
            layers(model, shows: "blend")
        case "layers-menu":
            layers(model, shows: "menu")
        case "layers-rename":
            layers(model, shows: "rename")
        case "layers-picked":
            layers(model, shows: "picked")
        case "library-add":
            library(model, shows: "add")
        case "library-set-menu":
            library(model, shows: "set-menu")
        case "library-icons":
            library(model, shows: "icons")
        case "library-search":
            library(model, shows: "search")
        case "library-swipe":
            library(model, shows: "swipe")
        case "library-recent":
            library(model, shows: "recent")
        case "library-title":
            library(model, shows: "title")
        case "libraries":
            libraries(model, shows: "")
        case "libraries-title":
            libraries(model, shows: "libraries-title")
        case "libraries-add":
            libraries(model, shows: "libraries-add")
        case "libraries-card-menu":
            libraries(model, shows: "libraries-card-menu")
        case "layers-add":
            vectorLayer(model)
            layersShows = "add"
            model.toggle(.layers)
        case "share-plugin":
            vectorLayer(model)
            model.actionsTab = 2
            model.toggle(.actions)
        default:
            break
        }
    }

    /// Shows a canvas in place of the one shown (for scenes that go from one canvas to another).
    static var opener: ((CanvasModel?) -> Void)?

    /// `-import PATH -then NAME`: what the artwork the file opens as shows once it is up: its
    /// `layers`, the actions' `actions-share` or `actions-3d` tab, or (a model) its texture `flat`.
    static func imported(_ model: CanvasModel) {
        let arguments = ProcessInfo.processInfo.arguments
        guard let at = arguments.firstIndex(of: "-then"), at + 1 < arguments.count else { return }
        let name = arguments[at + 1]
        DispatchQueue.main.asyncAfter(deadline: .now() + 2) {
            switch name {
            case "layers":
                model.toggle(.layers)
            case "actions-share", "actions-3d":
                model.actionsTab = name == "actions-share" ? 2 : 3
                model.toggle(.actions)
            case "flat":
                model.scene.setFlat(true)
            default:
                break
            }
        }
    }

    static func start(open: @escaping (CanvasModel?) -> Void) {
        opener = open
        let arguments = ProcessInfo.processInfo.arguments
        // `-import PATH`: the file brought in as one opened with the app is
        if let at = arguments.firstIndex(of: "-import"), at + 1 < arguments.count {
            let url = URL(fileURLWithPath: arguments[at + 1])
            DispatchQueue.main.asyncAfter(deadline: .now() + 1) { GalleryModel.shared.importOpened(url) }
        }
        guard let at = arguments.firstIndex(of: "-screen"), at + 1 < arguments.count else { return }
        let name = arguments[at + 1]
        guard let scene = UIApplication.shared.connectedScenes.first as? UIWindowScene else { return }
        scene.requestGeometryUpdate(.iOS(interfaceOrientations: .landscapeRight)) { _ in }
        if name == "gallery" {
            return
        }
        if name.hasPrefix("gallery-") {
            galleryShows = name
            return
        }
        // after the turn, the screen's size the other way round
        DispatchQueue.main.asyncAfter(deadline: .now() + 1) {
            let bounds = scene.screen.bounds
            let scale = scene.screen.scale
            let width = UInt32(max(bounds.width, bounds.height) * scale)
            let height = UInt32(min(bounds.width, bounds.height) * scale)
            let session = CanvasSession(width: width, height: height)
            // an HDR canvas, its light above white shown in the screen's extended range
            // (the light tool's scene too: the tool is an HDR canvas's only)
            if name.hasPrefix("hdr") || name == "light" {
                session.setNewHdr(hdr: true)
            }
            let model = CanvasModel(session: session, dir: Gallery.store.newFolder(folder: nil))
            open(model)
            switch name {
            case "brushes": model.toolPressed(.paint)
            case "layers": model.toggle(.layers)
            case "colors": model.toggle(.color)
            case "actions": model.toggle(.actions)
            case "adjustments": model.toggle(.adjustments)
            // what works on the canvas waits for the view to attach it
            case _ where Self.canvasScreens.contains(where: { name.hasPrefix($0) }):
                DispatchQueue.main.asyncAfter(deadline: .now() + 2) { Self.open(name, in: model) }
            default: Self.open(name, in: model)
            }
        }
    }
}

extension View {
    /// In a capture of the sideways studio, laid out as it is on its side.
    @ViewBuilder
    func sidewaysInCaptures(_ screen: CGSize) -> some View {
        if ScreenCapture.sideways {
            frame(width: screen.height, height: screen.width)
                .rotationEffect(.degrees(90))
                .frame(width: screen.width, height: screen.height)
        } else {
            self
        }
    }
}
#endif

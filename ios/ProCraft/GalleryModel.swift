import ImageIO
import PDFKit
import SwiftUI
import UIKit

/// A picture brought in as an artwork: its longest side at most this.
private let maxPicture = 4096
/// A PDF's pages are drawn at this resolution, the long side kept within this.
private let pdfDpi: Float = 300
private let pdfMostSide: CGFloat = 4096
/// How long a progress card shows its check, seconds.
private let doneShown = 0.45
/// How often a card's pie is moved on, seconds; of its pie, how much copying a file in takes, how
/// far it creeps while the file is read, and how quickly.
private let cardPoll = 0.05
private let copyShare: Float = 0.6
private let creepTo: Float = 0.95
private let creepStep: Float = 0.03
private let modelExtensions = ["obj", "usdz", "usdc", "usda", "usd"]
private let otherBrushExtensions = ["abr", "gbr", "gih", "vbr", "kpp", "bundle", "sut"]

private let srgbName = "sRGB IEC61966-2.1"
private let p3Name = "Display P3"

extension GalleryEntry {
    /// Its folder, which names it in the gallery.
    var dir: String {
        switch self {
        case let .artwork(artwork): artwork.dir
        case let .stack(dir, _, _): dir
        }
    }
}

/// How well a PDF keeps its pictures: JPEG at these qualities, or exactly.
enum PdfQuality {
    static let good: UInt8 = 60
    static let better: UInt8 = 85
    static let best: UInt8 = 100
}

/// A progress card's state: how far it has come (0..1), whether it is done, and the file it is about.
struct ProgressShown: Equatable {
    let progress: Float
    var done = false
    var detail: String?
}

/// What a file brought in made.
private enum Imported {
    /// An artwork in this folder.
    case made(String)
    /// A picture as a new canvas, not yet kept.
    case picture(CanvasSession)
    /// Something that isn't an artwork, or the reason it can't be read.
    case said(String)
    /// Something that asks about itself (a plugin): nothing more to say here.
    case offered
}

/// The gallery: what it shows, and what its buttons do with the store. An artwork's work goes on a
/// queue of its own; the screen is told when it is done.
@Observable
final class GalleryModel {
    /// The gallery kept while an artwork is open, a stack open staying so.
    static let shared = GalleryModel()

    /// What Share offers, in order.
    static let shareFormats = ["ProCraft", "Procreate", "PSD", "PDF", "JPEG", "PNG", "TIFF"]

    private static let presetsKey = "canvasPresets"

    /// The workspace showing, and every one there is.
    private(set) var workspace = Gallery.workspaces.selected()
    private(set) var workspaces = Gallery.workspaces.list()
    /// What the gallery, or the stack open, shows, as the store orders it.
    private(set) var entries: [GalleryEntry] = []
    /// The stack open: its folder and name.
    private(set) var openStack: (dir: String, name: String)?
    /// Goes up when artworks are saved, so thumbnails read again.
    private(set) var version = 0
    /// What a toast says.
    var message: String?
    /// The New canvas list under the screen's size.
    private(set) var presets: [CanvasPreset]
    /// Custom Canvas is open, on `editingPreset` (its place in the list) or a new preset.
    private(set) var customOpen = false
    private(set) var editingPreset: Int?
    private(set) var exportShown: ProgressShown?
    private(set) var importShown: ProgressShown?

    /// Where each thumbnail is on the screen, for what rises from it.
    @ObservationIgnored var thumbAt: [String: CGRect] = [:]
    /// Where each cell showing is on the window, the grid itself, and a stack's way back: where a
    /// drag lands.
    @ObservationIgnored var cellAt: [String: CGRect] = [:]
    /// Where each stack opened was, for its artworks to fly out of and back into.
    @ObservationIgnored private var stackPlaces: [String: CGRect] = [:]
    /// A stack just opened: where its cell was, for its artworks to fly out of.
    private(set) var flyOutOf: CGRect?
    /// A stack just left: its artworks where they were, flying back into its cell.
    private(set) var flyingBack: (cells: [(artwork: GalleryArtwork, at: CGRect)], to: CGRect)?
    /// Goes up each time a stack is left, for what grows the gallery back.
    private(set) var leftStack = 0
    /// The artwork pressed, shrunk, or lifted when held.
    var pressed: String?
    var lifted: String?
    @ObservationIgnored var gridAt = CGRect.zero
    @ObservationIgnored var backAt = CGRect.zero
    @ObservationIgnored var onOpen: (OpenArtwork) -> Void = { _ in }

    @ObservationIgnored private let work = DispatchQueue(label: "gallery", qos: .userInitiated)
    @ObservationIgnored private var cancelled = false
    @ObservationIgnored private var creep: Timer?

    init() {
        presets = presetsFromJson(saved: UserDefaults.standard.string(forKey: Self.presetsKey))
    }

    /// The artworks showing, not those in stacks.
    var artworks: [GalleryArtwork] {
        entries.compactMap {
            if case let .artwork(artwork) = $0 {
                return artwork
            }
            return nil
        }
    }

    private var folder: String? {
        openStack?.dir
    }

    func reload() {
        entries = Gallery.store.list(folder: folder)
    }

    func artworkSaved() {
        reload()
        version += 1
    }

    /// Runs `change` on the store off the main thread, the gallery read again after it. The store
    /// is the one showing now, whichever shows by the time it runs.
    private func changing(_ change: @escaping (ArtworkStore) -> Void) {
        let store = Gallery.store
        work.async { [weak self] in
            change(store)
            DispatchQueue.main.async { self?.reload() }
        }
    }

    // MARK: workspaces

    /// The workspaces read again, and the one picked shown if it isn't the one showing: another
    /// was picked, or this one has a new name, or is gone. No stack stays open.
    private func readWorkspaces() {
        workspaces = Gallery.workspaces.list()
        let picked = Gallery.workspaces.selected()
        guard picked != workspace else { return }
        Gallery.show(picked)
        workspace = picked
        openStack = nil
        stackPlaces = [:]
        flyOutOf = nil
        flyingBack = nil
        reload()
    }

    func showWorkspace(_ next: Workspace) {
        Gallery.workspaces.select(workspace: next)
        readWorkspaces()
    }

    /// A new, empty workspace named `name`, shown at once.
    func newWorkspace(_ name: String) {
        guard let made = Gallery.workspaces.create(name: name) else {
            message = "This workspace can't be made."
            return
        }
        showWorkspace(made)
    }

    /// Runs `change` on the workspaces off the main thread, once nothing is being written in
    /// them: the save of an artwork just left, and the store's own work.
    private func changingWorkspaces(_ change: @escaping () -> Void) {
        Task { @MainActor in
            while Gallery.closingSaves > 0 {
                try? await Task.sleep(for: .milliseconds(100))
            }
            work.async { [weak self] in
                change()
                DispatchQueue.main.async { self?.readWorkspaces() }
            }
        }
    }

    func renameWorkspace(_ renamed: Workspace, _ name: String) {
        changingWorkspaces { [weak self] in
            if Gallery.workspaces.rename(workspace: renamed, name: name) == nil {
                DispatchQueue.main.async { self?.message = "This workspace can't be renamed." }
            }
        }
    }

    /// `gone` deleted with every artwork in it; if it was showing, the first one shows.
    func deleteWorkspace(_ gone: Workspace) {
        changingWorkspaces { Gallery.workspaces.delete(workspace: gone) }
    }

    func showStack(_ dir: String?, name: String = "") {
        if let dir, openStack == nil, let at = cellAt[dir] {
            stackPlaces[dir] = at
            flyOutOf = at
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.6) { [weak self] in self?.flyOutOf = nil }
        } else if dir == nil, let open = openStack, let to = stackPlaces[open.dir] {
            let cells = artworks.compactMap { a in cellAt[a.dir].map { (artwork: a, at: $0) } }
            flyingBack = (cells, to)
            leftStack += 1
            DispatchQueue.main.asyncAfter(deadline: .now() + 0.4) { [weak self] in self?.flyingBack = nil }
        }
        openStack = dir.map { ($0, name) }
        reload()
    }

    /// `entry` dragged to `to` in the gallery or stack showing (its place in the list without it),
    /// out of its stack if it was in one.
    func move(_ entry: GalleryEntry, to: Int) {
        // at once, so the cells move as the finger lifts
        var shown = entries.filter { $0.dir != entry.dir }
        shown.insert(entry, at: min(max(to, 0), shown.count))
        entries = shown
        let into = folder ?? Gallery.store.root()
        changing { $0.moveEntry(dir: entry.dir, to: Int64(to), into: into) }
    }

    /// An artwork dropped onto `onto` in the gallery: with an artwork it makes a stack, this one
    /// first; a stack takes it in first.
    func put(_ a: GalleryArtwork, onto: GalleryEntry) {
        guard openStack == nil, a.dir != onto.dir else { return }
        changing { $0.putOnto(dir: a.dir, onto: onto.dir) }
    }

    /// Puts artworks the gallery can't show in a stack of their own, after the save of any artwork
    /// just left: a folder being written isn't lost.
    @MainActor
    func recoverLost() async {
        while Gallery.closingSaves > 0 {
            try? await Task.sleep(for: .milliseconds(100))
        }
        let store = Gallery.store
        await withCheckedContinuation { done in
            work.async {
                _ = store.recoverLostArtworks()
                done.resume()
            }
        }
        artworkSaved()
    }

    func stack(_ list: [GalleryArtwork]) {
        guard list.count >= 2, openStack == nil else { return }
        changing { _ = $0.stack(dirs: list.map(\.dir)) }
    }

    func renameStack(_ dir: String, _ name: String) {
        let trimmed = name.trimmingCharacters(in: .whitespaces)
        if openStack?.dir == dir {
            openStack = (dir, trimmed.isEmpty ? "Stack" : trimmed)
        }
        changing { $0.renameStack(dir: dir, name: name) }
    }

    func delete(_ a: GalleryArtwork) {
        changing { $0.delete(dir: a.dir) }
    }

    func duplicate(_ a: GalleryArtwork) {
        changing { _ = $0.duplicate(dir: a.dir) }
    }

    func rename(_ a: GalleryArtwork, _ name: String) {
        guard !name.trimmingCharacters(in: .whitespaces).isEmpty else { return }
        changing { $0.rename(dir: a.dir, name: name) }
    }

    func open(_ a: GalleryArtwork) {
        do {
            onOpen(OpenArtwork(session: try CanvasSession.open(dir: a.dir), dir: a.dir))
        } catch {
            message = "This artwork can't be opened."
        }
    }

    // MARK: new canvases

    /// A new artwork of `width` x `height` pixels (the screen's size), in Display P3.
    func newCanvas(width: UInt32, height: UInt32) {
        let session = CanvasSession(width: max(width, 1), height: max(height, 1))
        _ = session.setNewProfile(name: p3Name)
        onOpen(OpenArtwork(session: session, dir: Gallery.store.newFolder(folder: folder)))
    }

    /// A new artwork of a plugin's kind (`<id>.<key>`), `width` x `height` pixels (the screen's
    /// size), in Display P3, its plugin keeping it.
    func newCanvas(kind: String, width: UInt32, height: UInt32) {
        let (w, h) = (max(width, 1), max(height, 1))
        let session = CanvasSession(width: w, height: h)
        _ = session.setNewProfile(name: p3Name)
        guard let canvas = PluginsModel.shared.store.openCanvas(kind: kind, width: w, height: h, data: nil),
              session.attachPluginCanvas(kind: kind, canvas: canvas)
        else {
            message = "The plugin couldn't make this artwork."
            return
        }
        onOpen(OpenArtwork(session: session, dir: Gallery.store.newFolder(folder: folder)))
    }

    /// A new artwork made as preset `p` says.
    func newCanvas(_ p: CanvasPreset) {
        let rgb = [(p.background >> 16) & 255, (p.background >> 8) & 255, p.background & 255].map { Float($0) / 255 }
        let session = CanvasSession.newCustom(
            width: presetWidth(preset: p), height: presetHeight(preset: p), dpi: p.dpi, background: rgb,
            backgroundVisible: !p.backgroundHidden, name: "Untitled Artwork"
        )
        _ = session.setNewProfile(name: p.profile)
        session.setNewHdr(hdr: p.hdr)
        let dir = Gallery.store.newFolder(folder: folder)
        ArtworkTimeLapse(dir: dir).settings = TimeLapseSettings(
            resolution: p.timeLapseResolution, quality: p.timeLapseQuality, hevc: p.timeLapseHevc
        )
        onOpen(OpenArtwork(session: session, dir: dir))
    }

    private func savePresets(_ list: [CanvasPreset]) {
        presets = list
        UserDefaults.standard.set(presetsJson(presets: list), forKey: Self.presetsKey)
    }

    func openCustom(_ index: Int? = nil) {
        editingPreset = index.flatMap { presets.indices.contains($0) ? $0 : nil }
        customOpen = true
    }

    func closeCustom() {
        customOpen = false
        editingPreset = nil
    }

    /// Custom Canvas's Done: an edited preset is saved in its place; a new one goes at the end of
    /// the list and is made into an artwork. True if an artwork was made.
    func finishCustom(_ p: CanvasPreset) -> Bool {
        let index = editingPreset
        closeCustom()
        if let index, presets.indices.contains(index) {
            var list = presets
            list[index] = p
            savePresets(list)
            return false
        }
        savePresets(presets + [p])
        newCanvas(p)
        return true
    }

    func deletePreset(_ i: Int) {
        guard presets.indices.contains(i) else { return }
        var list = presets
        list.remove(at: i)
        savePresets(list)
    }

    // MARK: pictures

    /// Straight RGBA pixels, at most `maxPicture` on a side, in Display P3 (`p3`) or sRGB; not said,
    /// in Display P3 if the picture is, else sRGB.
    static func decode(_ data: Data, p3 wanted: Bool? = nil) -> (image: ThumbnailImage, p3: Bool)? {
        guard let source = CGImageSourceCreateWithData(data as CFData, nil) else { return nil }
        let options: [CFString: Any] = [
            kCGImageSourceCreateThumbnailFromImageAlways: true,
            kCGImageSourceCreateThumbnailWithTransform: true,
            kCGImageSourceThumbnailMaxPixelSize: maxPicture,
            kCGImageSourceShouldCacheImmediately: true,
        ]
        guard let picture = CGImageSourceCreateThumbnailAtIndex(source, 0, options as CFDictionary) else { return nil }
        let width = picture.width
        let height = picture.height
        let named = picture.colorSpace?.name.map { $0 as String }
        let p3 = wanted ?? (named?.contains("P3") == true)
        guard let space = CGColorSpace(name: p3 ? CGColorSpace.displayP3 : CGColorSpace.sRGB) else { return nil }
        var pixels = [UInt8](repeating: 0, count: width * height * 4)
        let drawn: Bool = pixels.withUnsafeMutableBytes { bytes in
            guard let context = CGContext(
                data: bytes.baseAddress, width: width, height: height, bitsPerComponent: 8,
                bytesPerRow: width * 4, space: space, bitmapInfo: CGImageAlphaInfo.premultipliedLast.rawValue
            ) else { return false }
            context.draw(picture, in: CGRect(x: 0, y: 0, width: width, height: height))
            return true
        }
        guard drawn else { return nil }
        // a bitmap holds them premultiplied: the core takes them straight
        for i in stride(from: 0, to: pixels.count, by: 4) {
            let alpha = Int(pixels[i + 3])
            guard alpha != 0, alpha != 255 else { continue }
            for k in 0 ..< 3 {
                pixels[i + k] = UInt8(min(255, (Int(pixels[i + k]) * 255 + alpha / 2) / alpha))
            }
        }
        return (ThumbnailImage(width: UInt32(width), height: UInt32(height), rgba: Data(pixels)), p3)
    }

    private static func session(of data: Data, title: String) -> CanvasSession? {
        guard let decoded = decode(data) else { return nil }
        let session = CanvasSession.fromImage(image: decoded.image, name: title)
        if decoded.p3 {
            _ = session.setNewProfile(name: p3Name)
        }
        return session
    }

    /// A picture from Photos as a new canvas.
    func newFromPicture(_ data: Data?) {
        guard let data else {
            message = "This picture can't be opened."
            return
        }
        let dir = Gallery.store.newFolder(folder: folder)
        work.async { [weak self] in
            let session = Self.session(of: data, title: "Untitled Artwork")
            DispatchQueue.main.async {
                guard let self else { return }
                if let session {
                    self.onOpen(OpenArtwork(session: session, dir: dir))
                } else {
                    Gallery.store.delete(dir: dir)
                    self.message = "This picture can't be opened."
                }
            }
        }
    }

    // MARK: bringing files in

    /// Files from elsewhere into the gallery, one after another; one artwork alone is opened.
    func importFiles(_ urls: [URL]) {
        guard importShown == nil, exportShown == nil, !urls.isEmpty else { return }
        cancelled = false
        let folder = folder
        let only = urls.count == 1
        work.async { [weak self] in
            for url in urls {
                guard let self, !self.cancelled else { break }
                self.importOne(url, folder: folder, open: only)
            }
            DispatchQueue.main.async {
                self?.creep?.invalidate()
                self?.reload()
            }
        }
    }

    /// A file another app opened here, brought in as Import's; the system's copy goes after it.
    func importOpened(_ url: URL) {
        importFiles([url])
        guard url.pathComponents.contains("Inbox") else { return }
        work.async { try? FileManager.default.removeItem(at: url) }
    }

    /// Files dropped on the gallery, brought in as Import's; the copies in `folder` go after them.
    func importDropped(_ urls: [URL], from folder: URL) {
        importFiles(urls)
        work.async { try? FileManager.default.removeItem(at: folder) }
    }

    /// The import card's Cancel: the card goes at once, and what the import makes is thrown away.
    func cancelImport() {
        cancelled = true
        importShown = nil
    }

    private func showCard(_ name: String, from: Float) {
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.creep?.invalidate()
            var shown = from
            self.importShown = ProgressShown(progress: shown, detail: name)
            self.creep = Timer.scheduledTimer(withTimeInterval: cardPoll, repeats: true) { [weak self] _ in
                guard let self, !self.cancelled else { return }
                shown += (creepTo - shown) * creepStep
                self.importShown = ProgressShown(progress: shown, detail: name)
            }
        }
    }

    private func importOne(_ url: URL, folder: String?, open: Bool) {
        let name = url.lastPathComponent
        showCard(name, from: 0)
        let scoped = url.startAccessingSecurityScopedResource()
        defer {
            if scoped {
                url.stopAccessingSecurityScopedResource()
            }
        }
        // a copy of its own: what reads it may want a path, and the picker's goes away
        let copy = FileManager.default.temporaryDirectory.appendingPathComponent("import-\(UUID().uuidString)-\(name)")
        defer { try? FileManager.default.removeItem(at: copy) }
        do {
            try FileManager.default.copyItem(at: url, to: copy)
        } catch {
            finishImport(.said("This file can't be opened."), name: name, folder: folder, open: open)
            return
        }
        showCard(name, from: copyShare)
        let outcome = read(copy, name: name, folder: folder)
        finishImport(outcome, name: name, folder: folder, open: open)
    }

    /// What the card and the gallery do with what a file made; waits for the check to show.
    private func finishImport(_ outcome: Imported, name: String, folder: String?, open: Bool) {
        var toOpen: OpenArtwork?
        var said: String?
        var quiet = false
        switch outcome {
        case let .said(text):
            said = text
        case .offered:
            quiet = true
        case let .made(dir):
            if cancelled {
                Gallery.store.delete(dir: dir)
            } else if open, let session = try? CanvasSession.open(dir: dir) {
                toOpen = OpenArtwork(session: session, dir: dir)
            }
        case let .picture(session):
            let dir = Gallery.store.newFolder(folder: folder)
            if cancelled {
                Gallery.store.delete(dir: dir)
            } else if open {
                toOpen = OpenArtwork(session: session, dir: dir)
            } else if (try? session.save(dir: dir)) == nil {
                Gallery.store.delete(dir: dir)
                said = "This picture can't be opened."
            }
        }
        let wasCancelled = cancelled
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.creep?.invalidate()
            if let said {
                self.message = said
                self.importShown = nil
            } else if quiet {
                self.importShown = nil
            } else if !wasCancelled {
                self.importShown = ProgressShown(progress: 1, done: true, detail: name)
            }
        }
        if said == nil, !quiet, !wasCancelled {
            Thread.sleep(forTimeInterval: doneShown)
        }
        DispatchQueue.main.async { [weak self] in
            guard let self else { return }
            self.importShown = nil
            self.reload()
            if let toOpen {
                self.onOpen(toOpen)
            }
        }
    }

    /// Reads the file at `copy`, called `name`: what it is is told by its first bytes, and a zip by
    /// the names it holds.
    private func read(_ copy: URL, name: String, folder: String?) -> Imported {
        let handle = try? FileHandle(forReadingFrom: copy)
        let head = (try? handle?.read(upToCount: 4)) ?? Data()
        try? handle?.close()
        let title = (name as NSString).deletingPathExtension.trimmingCharacters(in: .whitespaces)
        let lower = name.lowercased()
        let tag = String(decoding: head, as: UTF8.self)
        // what isn't an artwork's, a brush's or a palette's, by its name: a plugin (offered, as its
        // own page's Import offers it), a font, a colour profile
        if lower.hasSuffix(".procraft-plugin") || lower.hasSuffix(".procraftplugin") {
            // read a moment later: a copy this one's going doesn't take with it
            let kept = FileManager.default.temporaryDirectory.appendingPathComponent("plugin-\(UUID().uuidString)-\(name)")
            guard (try? FileManager.default.copyItem(at: copy, to: kept)) != nil else {
                return .said("This file can't be opened.")
            }
            DispatchQueue.main.async { [weak self] in
                PluginsModel.shared.offer(kept) { self?.message = $0 }
            }
            return .offered
        }
        if [".ttf", ".otf", ".ttc"].contains(where: { lower.hasSuffix($0) }) {
            let added = (try? Data(contentsOf: copy)).flatMap { importTextFont(bytes: $0, fileName: name) }
            return .said(added != nil ? "Font added." : "This font can't be read.")
        }
        if lower.hasSuffix(".icc") || lower.hasSuffix(".icm") {
            let added = (try? Data(contentsOf: copy)).flatMap { importProfile(bytes: $0) }
            return .said(added.map { "\($0) added to the color profiles." } ?? "This color profile can't be read.")
        }
        // another program's palette
        let foreign = tag == "ASEF" || lower.hasSuffix(".aco")
        let zip = head.starts(with: [0x50, 0x4B])
        let kind = zip ? zipFileKind(path: copy.path) : nil
        if kind == .brushes || otherBrushExtensions.contains(where: { lower.hasSuffix(".\($0)") }) {
            return addBrushes(copy, name: name)
        }
        if foreign || kind == .swatches {
            guard let bytes = try? Data(contentsOf: copy),
                  CanvasModel.paletteStore.importFile(bytes: bytes, fileName: name) else {
                return .said("This palette can't be read.")
            }
            return .said("Palette added.")
        }
        // a model to paint on: its file, or a zip holding it with the files it needs
        if modelExtensions.contains(where: { lower.hasSuffix(".\($0)") }) {
            return bringModel(copy, name: name, title: title)
        }
        if case .model = kind {
            return bringModel(copy, name: name, title: title)
        }
        // a model's artwork of ours, whole: its folder as it was saved
        if kind == .modelArtwork {
            let dir = Gallery.store.newFolder(folder: folder)
            guard unpackModelArtwork(path: copy.path, dir: dir) else {
                Gallery.store.delete(dir: dir)
                return .said("This file can't be opened.")
            }
            return .made(dir)
        }
        if tag == "8BPS" {
            return bringPsd(copy, title: title, folder: folder)
        }
        if tag == "%PDF" {
            return bringPdf(copy, title: title, folder: folder)
        }
        if zip {
            return bringPackage(copy, folder: folder)
        }
        guard let data = try? Data(contentsOf: copy),
              let session = Self.session(of: data, title: title.isEmpty ? "Untitled Artwork" : title) else {
            return .said("This file can't be opened.")
        }
        return .picture(session)
    }

    /// A model (an OBJ, a USD, or a zip holding one with its files) as a new
    /// artwork that paints on it, the images its file brings in its texture sets.
    private func bringModel(_ copy: URL, name: String, title: String) -> Imported {
        guard let bytes = try? Data(contentsOf: copy) else {
            return .said("This file can't be opened.")
        }
        do {
            let session = try CanvasSession.openModel(
                file: bytes, fileName: name, files: [], name: title.isEmpty ? "Untitled Artwork" : title, textureSize: 0
            )
            _ = session.loadModelTextures()
            return .picture(session)
        } catch let MobileError.File(reason) {
            // the core says what it was doing, then why it couldn't
            let why = reason.range(of: ": ").map { String(reason[$0.upperBound...]) } ?? ""
            return .said(why.isEmpty ? "This model can't be opened." : "This model can't be opened: \(why).")
        } catch {
            return .said("This model can't be opened.")
        }
    }

    /// Brushes wait as files for the next artwork opened, which has the library.
    private func addBrushes(_ copy: URL, name: String) -> Imported {
        guard let bytes = try? Data(contentsOf: copy), readBrushes(bytes: bytes, fileName: name) != nil else {
            return .said("This file has no brushes in it.")
        }
        let to = Gallery.pendingBrushes.appendingPathComponent("\(UUID().uuidString)-\(name)")
        guard (try? bytes.write(to: to)) != nil else {
            return .said("This file can't be opened.")
        }
        return .said("Brushes added to the library.")
    }

    private func bringPsd(_ copy: URL, title: String, folder: String?) -> Imported {
        let dir = Gallery.store.newFolder(folder: folder)
        let outcome = importPsd(path: copy.path, dir: dir, name: title)
        switch outcome {
        case .imported:
            return .made(dir)
        case let .tooManyLayers(most):
            Gallery.store.delete(dir: dir)
            return .said("This file has more layers than a canvas its size can hold (\(most)).")
        case .unsupported:
            Gallery.store.delete(dir: dir)
            return .said("This kind of PSD can't be opened.")
        case .unreadable:
            Gallery.store.delete(dir: dir)
            return .said("This file can't be opened.")
        }
    }

    /// An artwork file of ours, or a layered package.
    private func bringPackage(_ copy: URL, folder: String?) -> Imported {
        guard let session = try? CanvasSession.openPackage(path: copy.path) else {
            return .said("This file can't be opened.")
        }
        let dir = Gallery.store.newFolder(folder: folder)
        guard (try? session.save(dir: dir)) != nil else {
            Gallery.store.delete(dir: dir)
            return .said("This file can't be opened.")
        }
        // its time-lapse: the settings, and its videos as they are
        let segments = FileManager.default.temporaryDirectory.appendingPathComponent("segments-\(UUID().uuidString)")
        defer { try? FileManager.default.removeItem(at: segments) }
        if let lapse = try? packageTimeLapse(path: copy.path, dir: segments.path) {
            ArtworkTimeLapse(dir: dir).take(lapse)
        }
        return .made(dir)
    }

    /// A PDF made into a new artwork named after its file: each page a layer, drawn on white at
    /// `pdfDpi` (the long side kept within `pdfMostSide`) and fitted to the first page's size.
    private func bringPdf(_ copy: URL, title: String, folder: String?) -> Imported {
        guard let document = PDFDocument(url: copy), document.pageCount > 0, let first = document.page(at: 0) else {
            return .said("This PDF can't be opened.")
        }
        func size(_ page: PDFPage) -> CGSize {
            let box = page.bounds(for: .cropBox).size
            return page.rotation % 180 == 0 ? box : CGSize(width: box.height, height: box.width)
        }
        let start = size(first)
        let scale = min(CGFloat(pdfDpi) / 72, pdfMostSide / max(start.width, start.height, 1))
        let w = max((start.width * scale).rounded(), 1)
        let h = max((start.height * scale).rounded(), 1)
        // more pages than a canvas this size holds as layers: none drawn, as none will be taken
        let most = layersFor(width: UInt32(w), height: UInt32(h))
        guard UInt32(document.pageCount) <= most else {
            return .said("This PDF has more pages than a canvas its size can hold as layers (\(most)).")
        }
        let pages = FileManager.default.temporaryDirectory.appendingPathComponent("pdf-\(UUID().uuidString)")
        try? FileManager.default.createDirectory(at: pages, withIntermediateDirectories: true)
        defer { try? FileManager.default.removeItem(at: pages) }
        let format = UIGraphicsImageRendererFormat()
        format.scale = 1
        format.opaque = true
        let renderer = UIGraphicsImageRenderer(size: CGSize(width: w, height: h), format: format)
        for i in 0 ..< document.pageCount {
            guard let page = document.page(at: i) else { continue }
            let box = size(page)
            // each page fitted into the canvas, in its middle
            let fit = min(w / box.width, h / box.height)
            let data = renderer.pngData { drawing in
                UIColor.white.setFill()
                drawing.fill(CGRect(x: 0, y: 0, width: w, height: h))
                let context = drawing.cgContext
                context.translateBy(x: (w - box.width * fit) / 2, y: (h + box.height * fit) / 2)
                context.scaleBy(x: fit, y: -fit)
                page.draw(with: .cropBox, to: context)
            }
            try? data.write(to: pages.appendingPathComponent("page-\(i + 1).png"))
        }
        let dir = Gallery.store.newFolder(folder: folder)
        let outcome = importPages(
            pages: pages.path, count: UInt32(document.pageCount), width: UInt32(w), height: UInt32(h),
            dpi: pdfDpi, dir: dir, name: title
        )
        switch outcome {
        case .imported:
            return .made(dir)
        case let .tooManyPages(most):
            Gallery.store.delete(dir: dir)
            return .said("This PDF has more pages than a canvas its size can hold as layers (\(most)).")
        case .unreadable:
            Gallery.store.delete(dir: dir)
            return .said("This PDF can't be opened.")
        }
    }

    // MARK: sharing

    /// The export card's Cancel: nothing is shared, and the next artwork isn't begun.
    func cancelExport() {
        cancelled = true
    }

    private static func fileExtension(_ format: String) -> String {
        switch format {
        case "PSD": ".psd"
        case "PDF": ".pdf"
        case "JPEG": ".jpg"
        case "PNG": ".png"
        case "TIFF": ".tif"
        case "Procreate": ".procreate"
        default: ".procraft"
        }
    }

    private static func write(_ a: GalleryArtwork, format: String, to url: URL, quality: UInt8) -> Bool {
        guard let session = try? CanvasSession.open(dir: a.dir), session.prepareOffscreen() else { return false }
        do {
            switch format {
            case "PDF": try session.exportPdf(path: url.path, layers: false, quality: quality)
            case "PSD": try session.exportPsd(path: url.path)
            case "TIFF": try session.exportTiff(path: url.path)
            case "PNG": try session.exportPng(path: url.path)
            case "JPEG": try session.exportJpeg(path: url.path)
            case "Procreate":
                try session.exportPackage(path: url.path, seed: UInt64.random(in: 0 ... UInt64.max), timeLapse: ArtworkTimeLapse(dir: a.dir).forPackage())
            // ours: on a timeline, the folder as saved (a package can't hold the timeline)
            default:
                try session.exportArtwork(
                    dir: a.dir, saveFirst: false, path: url.path, seed: UInt64.random(in: 0 ... UInt64.max),
                    timeLapse: ArtworkTimeLapse(dir: a.dir).forPackage()
                )
            }
            return true
        } catch {
            return false
        }
    }

    /// `list` written as `format` for the share sheet, hanging from `from`.
    /// The artwork as a file for a drag out of the gallery, made when the other app asks for it.
    func fileForDrag(_ a: GalleryArtwork, format: String, done: @escaping (URL?) -> Void) {
        let name = exportFileName(name: a.name, package: false) + Self.fileExtension(format)
        let url = Sharing.exportsDir.appendingPathComponent(name)
        work.async {
            done(Self.write(a, format: format, to: url, quality: PdfQuality.best) ? url : nil)
        }
    }

    func share(_ list: [GalleryArtwork], format: String, quality: UInt8 = PdfQuality.best, from: CGRect?) {
        guard exportShown == nil, importShown == nil else { return }
        cancelled = false
        exportShown = ProgressShown(progress: 0)
        let dir = Sharing.exportsDir
        let suffix = Self.fileExtension(format)
        work.async { [weak self] in
            var used = Set<String>()
            var files: [URL] = []
            for (i, a) in list.enumerated() {
                guard let self, !self.cancelled else { break }
                let base = exportFileName(name: a.name, package: false)
                var name = base
                var n = 2
                while !used.insert(name).inserted {
                    name = "\(base) \(n)"
                    n += 1
                }
                let url = dir.appendingPathComponent(name + suffix)
                if Self.write(a, format: format, to: url, quality: quality) {
                    files.append(url)
                }
                let progress = Float(i + 1) / Float(max(list.count, 1))
                DispatchQueue.main.async { self.exportShown = ProgressShown(progress: progress) }
            }
            DispatchQueue.main.async { self?.exported(files, from: from) }
        }
    }

    private func exported(_ files: [URL], from: CGRect?) {
        if cancelled {
            exportShown = nil
            return
        }
        guard !files.isEmpty else {
            exportShown = nil
            message = "The artwork couldn't be exported."
            return
        }
        exportShown = ProgressShown(progress: 1, done: true)
        DispatchQueue.main.asyncAfter(deadline: .now() + doneShown) { [weak self] in
            self?.exportShown = nil
            Sharing.share(files, from: from)
        }
    }
}

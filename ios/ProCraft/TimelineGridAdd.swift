import PhotosUI
import SwiftUI
import UIKit
import UniformTypeIdentifiers

// The grid timeline's "Add new" (see TimelineGrid.swift): the card that `+` held opens, to bring
// in a photo or a video from the Photos library or any of a sound, a picture or a video from
// Files; the pickers it asks for; and the menu of an empty frame held.

/// What the Add new card asks to be picked.
enum GridAsk: Equatable {
    case photo, video, file
}

/// An empty frame held: its track, the frame, and where along the bar its menu stands.
struct GridEmptySpot: Equatable {
    var track: UInt64
    var frame: UInt32
    var x: CGFloat
}

extension TimelineTool {
    private var session: CanvasSession { canvasModel.session }

    /// The run of empty frames `frame` is in on track `track`; none where a frame is there.
    func gap(_ track: UInt64, at frame: UInt32) -> TimelineGap? {
        session.timelineGap(track: track, frame: frame)
    }

    /// That run made `length` frames long (0: closed), the frames after it moving to suit: one
    /// undo step.
    func setGap(_ track: UInt64, at frame: UInt32, to length: Int) {
        stopPlaying()
        if session.setTimelineGap(track: track, frame: frame, length: UInt32(clamping: max(length, 0))) {
            UISelectionFeedbackGenerator().selectionChanged()
        }
        canvasModel.refresh()
    }

    /// A new blank frame on `frame` of track `track`, the playhead going onto it.
    func insertFrame(at frame: UInt32, on track: UInt64) {
        selectTrack(track)
        goTo(frame)
        addDrawing()
    }

    /// What was copied put on `frame` of track `track`.
    func paste(atFrame frame: UInt32, on track: UInt64) {
        selectTrack(track)
        goTo(frame)
        pasteClips()
    }

    /// A picture's bytes as a new frame at the playhead on the chosen track (where a frame is
    /// already, just after it), fitted to the canvas: one undo step.
    func addPhoto(_ data: Data?, name: String?) {
        stopPlaying()
        // a moving GIF brings its own frames
        if name?.lowercased().hasSuffix(".gif") == true {
            canvasModel.insertPicture(data, name: name, isPrivate: false, transform: false)
            return
        }
        guard let data, let image = GalleryModel.decode(data, p3: CanvasSpace.shared.p3)?.image else {
            canvasModel.showNotice("This picture can't be opened.", warning: true)
            return
        }
        if session.insertImageFrame(image: image) == nil {
            canvasModel.showNotice(track?.locked == true ? "The track is locked." : "There's no room for another layer.", warning: true)
        }
        canvasModel.refresh()
    }

    /// A file from Files: a sound or a video onto the timeline at the playhead, a picture as a
    /// new frame there.
    func addFile(_ url: URL) {
        let type = UTType(filenameExtension: url.pathExtension)
        if type?.conforms(to: .audio) == true {
            media.bringIn(url, kind: .audio)
        } else if type?.conforms(to: .movie) == true {
            media.bringIn(url, kind: .video)
        } else if url.pathExtension.lowercased() == "svg" {
            // (its shapes come in as a vector layer, as from the canvas's own Add)
            canvasModel.insertFile(url, isPrivate: false)
        } else if type?.conforms(to: .image) == true {
            addPhoto(url.readPicked(), name: url.lastPathComponent)
        } else {
            canvasModel.showNotice("Only sounds, pictures and videos can be added.", warning: true)
        }
    }
}

/// The card `+` held opens: a photo or a video from the Photos library, or a file.
struct GridAddMenu: View {
    static let width: CGFloat = 268

    let tool: TimelineTool
    let grid: GridState
    let close: () -> Void

    var body: some View {
        // one file is read at a time
        let busy = tool.media.bringing
        GridCard(width: Self.width) {
            Text(verbatim: busy ? "Bringing in…" : "Add new")
                .font(.system(size: 22, weight: .bold))
                .foregroundStyle(Grid.inkBright)
                .padding(.horizontal, 18)
                .padding(.top, 8)
                .padding(.bottom, 12)
            HStack(spacing: 8) {
                tile(PcIcons.gridPhoto, "Photo", dim: false) { ask(.photo) }
                tile(PcIcons.gridVideo, "Video", dim: busy) { ask(.video) }
            }
            .padding(.horizontal, 10)
            GridSection {
                HStack(spacing: 8) {
                    Text(verbatim: "File")
                        .font(.system(size: 16))
                    Spacer(minLength: 0)
                    PcIconView(PcIcons.gridFile, size: 24, color: busy ? Grid.inkDim : Grid.inkBright)
                }
                .foregroundStyle(busy ? Grid.inkDim : Grid.inkBright)
                .padding(.horizontal, 14)
                .frame(height: 46)
                .contentShape(Rectangle())
                .onTapGesture {
                    if !busy {
                        ask(.file)
                    }
                }
                .accessibilityElement()
                .accessibilityLabel(Text(verbatim: "File"))
                .accessibilityAddTraits(.isButton)
            }
            .padding(.horizontal, 10)
            .padding(.top, 8)
            .padding(.bottom, 4)
        }
    }

    private func ask(_ what: GridAsk) {
        close()
        tool.stopPlaying()
        grid.asking = what
    }

    private func tile(_ icon: PcIcon, _ label: String, dim: Bool, action: @escaping () -> Void) -> some View {
        VStack(spacing: 8) {
            PcIconView(icon, size: 36, color: dim ? Grid.inkDim : Pc.accent)
            Text(verbatim: label)
                .font(.system(size: 16))
                .foregroundStyle(dim ? Grid.inkDim : Grid.inkBright)
        }
        .frame(maxWidth: .infinity, minHeight: 92)
        .background(RoundedRectangle(cornerRadius: 18, style: .continuous).fill(Grid.inkBright.opacity(0.08)))
        .contentShape(Rectangle())
        .onTapGesture {
            if !dim {
                action()
            }
        }
        .accessibilityElement()
        .accessibilityLabel(Text(verbatim: label))
        .accessibilityAddTraits(.isButton)
    }
}

/// The pickers the Add new card asks for, and what a sound or video being brought in can say:
/// why it couldn't be, and the animatic detector's sheet.
struct GridImports: ViewModifier {
    private struct Detecting: Identifiable {
        let id: UInt64
    }

    let model: CanvasModel
    let grid: GridState

    @State private var picked: PhotosPickerItem?
    /// Whether the library was opened for a video (or a photo): what `picked` is.
    @State private var video = false

    func body(content: Content) -> some View {
        @Bindable var media = model.media
        content
            .photosPicker(
                isPresented: Binding(
                    get: { grid.asking == .photo || grid.asking == .video },
                    set: { if !$0, grid.asking != .file { grid.asking = nil } }
                ),
                selection: $picked, matching: grid.asking == .video || video ? .videos : .images, preferredItemEncoding: .current
            )
            .onChange(of: grid.asking) { _, now in
                if now == .photo || now == .video {
                    video = now == .video
                }
            }
            .onChange(of: picked) { _, item in
                guard let item else { return }
                picked = nil
                if video {
                    media.bringIn(photo: item)
                } else {
                    Task { @MainActor in
                        let data = try? await item.loadTransferable(type: Data.self)
                        model.timeline.addPhoto(data, name: nil)
                    }
                }
            }
            .fileImporter(
                isPresented: Binding(get: { grid.asking == .file }, set: { if !$0, grid.asking == .file { grid.asking = nil } }),
                allowedContentTypes: [.audio, .movie, .image]
            ) { result in
                if case let .success(url) = result {
                    model.timeline.addFile(url)
                }
            }
            .alert(
                "Couldn't Bring It In",
                isPresented: Binding(get: { media.failed != nil }, set: { if !$0 { media.failed = nil } })
            ) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(verbatim: media.failed ?? "")
            }
            .sheet(item: Binding(get: { media.detecting.map(Detecting.init) }, set: { media.detecting = $0?.id })) { d in
                AnimaticSheet(model: model, clip: d.id)
            }
    }
}

/// An empty frame's menu: pasting or a new frame there; how many empty frames the run it is in
/// lasts, one of them or all of them taken out (the frames after moving earlier); and its
/// track's options.
struct GridEmptyMenu: View {
    private enum Beside {
        case options, length
    }

    let tool: TimelineTool
    let track: TimelineTrackInfo
    let frame: UInt32
    let room: CGFloat
    let close: () -> Void

    @State private var beside: Beside?

    var body: some View {
        // (read again after each change)
        let _ = tool.revision
        let gap = tool.gap(track.id, at: frame)
        let locked = tool.held(track)
        // with no frame after them, the empty frames have no length to change
        let fixed = locked || gap == nil || gap?.open == true
        let length = Int(gap?.length ?? 0)
        let at = gap?.start ?? frame
        HStack(alignment: .top, spacing: 6) {
            GridCard(width: 248) {
                GridMenuRow(icon: PcIcons.gridPaste, label: "Paste", dim: tool.clipboard.clips == 0 || locked) {
                    close()
                    tool.paste(atFrame: frame, on: track.id)
                }
                GridMenuRow(icon: PcIcons.gridInsert, label: "Insert frame", dim: locked) {
                    close()
                    tool.insertFrame(at: frame, on: track.id)
                }
                GridMenuLine()
                GridDurationRow(frames: length, locked: fixed, keypad: { beside = beside == .length ? nil : .length }) {
                    tool.setGap(track.id, at: at, to: $0)
                }
                GridMenuLine()
                GridMenuRow(icon: PcIcons.gridOptions, label: "Track options", chevron: true, lit: beside == .options) {
                    beside = beside == .options ? nil : .options
                }
                GridMenuLine()
                GridMenuRow(icon: PcIcons.gridRemove, label: "Remove single frame", dim: fixed) {
                    // (the last of them gone, there is no empty frame left to have a menu)
                    if length <= 1 {
                        close()
                    }
                    tool.setGap(track.id, at: at, to: length - 1)
                }
                GridMenuRow(icon: PcIcons.gridBroom, label: "Remove empty section", dim: fixed) {
                    close()
                    tool.setGap(track.id, at: at, to: 0)
                }
            }
            switch beside {
            case .options:
                GridTrackMenu(tool: tool, track: track, room: room, close: close)
            case .length:
                GridCard {
                    GridKeypad(unit: "frames", from: length, most: 999) { n in
                        tool.setGap(track.id, at: at, to: n)
                        beside = nil
                    }
                }
            case nil:
                EmptyView()
            }
        }
    }
}

import SwiftUI

private let panelWidth: CGFloat = 300
private let rowHeight: CGFloat = 44
/// The rows scroll past this many, the last showing half so it is seen there are more.
private let mostRows: CGFloat = 7.5
private let buttonWidth: CGFloat = 74
/// How far a row slides to show Rename and Delete.
private let reveal: CGFloat = buttonWidth * 2 + 3 + 16
private var renameFill: Color { Look.shared.light ? Color(argb: 0xFFC6_C6C6) : Color(argb: 0xFF3A_3A3A) }
private var renameInk: Color { Look.shared.light ? Color(argb: 0xFF57_5757) : Color(argb: 0xFFC7_C7C7) }
private let deleteFill = Color(argb: 0xFFFE_604D)

/// A workspace's name being asked for: a new one's, or `renamed`'s.
struct WorkspaceNaming {
    var renamed: Workspace?

    var asked: NameAsked {
        if let renamed {
            return NameAsked(title: "Rename Workspace", name: renamed.name, action: "Rename")
        }
        return NameAsked(title: "New Workspace", action: "Create")
    }
}

/// A workspace about to be deleted, and how many artworks go with it.
struct WorkspaceDeleting {
    let workspace: Workspace
    let count: UInt32

    var alert: AlertContent {
        let with = count == 0 ? "" : count == 1 ? "1 artwork is deleted with it. " : "\(count) artworks are deleted with it. "
        return AlertContent(
            title: "Delete “\(workspace.name)”?", message: with + "This can't be undone.", action: "Delete", icon: PcIcons.trash
        )
    }
}

/// The widest the workspace's button is on a screen `width` across: what is left in the middle
/// between the app's name and the buttons on the right, the same each side.
func workspaceButtonRoom(_ width: CGFloat) -> CGFloat {
    min(max(width - 584, 90), 300)
}

/// The capsule in the middle of the gallery's title row: the workspace showing, and a chevron for
/// the list a tap on it opens.
struct WorkspaceButton: View {
    let name: String
    /// The widest it is.
    let room: CGFloat
    let action: () -> Void

    var body: some View {
        HStack(spacing: 5) {
            Text(verbatim: name)
                .font(.system(size: 16))
                .foregroundStyle(Pc.galleryText)
                .lineLimit(1)
                .truncationMode(.middle)
                .frame(maxWidth: room - 49)
                .fixedSize()
            PcIconView(PcIcons.chevronDown, size: 12, color: Pc.galleryText)
                .frame(width: 12, height: 12)
        }
        .padding(.horizontal, 16)
        .frame(height: 45)
        .background(Capsule().fill(Pc.capsule))
        .overlay(Capsule().strokeBorder(Pc.capsuleEdge, lineWidth: 1))
        .contentShape(Capsule())
        .onTapGesture(perform: action)
    }
}

/// The workspaces, hanging from their button: a tap on one shows it (the one showing is ticked),
/// and a row slid left shows Rename and Delete, but for the first, which stays as it is. Under
/// them, the row that makes a new one.
struct WorkspacesPanel: View {
    let screen: CGSize
    let workspaces: [Workspace]
    let showing: Workspace
    let onPick: (Workspace) -> Void
    let onNew: () -> Void
    let onRename: (Workspace) -> Void
    let onDelete: (Workspace) -> Void

    /// The row slid open (by its folder), if one is.
    @State private var opened: String?

    /// The rows' ground: the group's wash on the panel, with nothing showing through.
    private var ground: some View {
        ZStack {
            Pc.galleryPopover
            Pc.actionsGroup
        }
    }

    private var rows: some View {
        VStack(spacing: 0) {
            ForEach(workspaces.indices, id: \.self) { i in
                let w = workspaces[i]
                if i > 0 {
                    Rectangle().fill(Pc.actionsSeparator).frame(height: 1)
                }
                WorkspaceRow(
                    workspace: w, showing: w == showing, open: opened == w.root,
                    onOpen: { opened = $0 ? w.root : nil },
                    onPick: {
                        if opened != nil {
                            withAnimation(Motion.swipeSettle) { opened = nil }
                        } else {
                            onPick(w)
                        }
                    },
                    onRename: { onRename(w) },
                    onDelete: { onDelete(w) }
                )
                .frame(height: rowHeight)
            }
        }
    }

    var body: some View {
        let shape = RoundedRectangle(cornerRadius: 18, style: .circular)
        let count = CGFloat(workspaces.count)
        let tall = min(count, mostRows) * (rowHeight + 1) - (count > mostRows ? 0 : 1)
        Popover(width: panelWidth, arrowX: panelWidth / 2, fill: Pc.galleryPopover) {
            VStack(alignment: .leading, spacing: 0) {
                Text(verbatim: "Workspaces")
                    .font(.system(size: 24, weight: .bold))
                    .foregroundStyle(Pc.panelTitle)
                    .padding(.leading, 24)
                    .padding(.vertical, 14)
                ScrollView(.vertical, showsIndicators: false) {
                    rows
                }
                .frame(height: tall)
                .background(ground)
                .clipShape(shape)
                .padding(.horizontal, 10)
                HStack(spacing: 0) {
                    Text(verbatim: "New Workspace")
                        .font(.system(size: 17))
                        .foregroundStyle(Pc.panelTitle)
                    Spacer(minLength: 8)
                    PcIconView(PcIcons.add, size: 30, color: Pc.panelTitle)
                        .frame(width: 30, height: 30)
                }
                .padding(.leading, 16)
                .padding(.trailing, 8)
                .frame(height: rowHeight)
                .background(shape.fill(Pc.actionsGroup))
                .contentShape(Rectangle())
                .onTapGesture(perform: onNew)
                .padding(.horizontal, 10)
                .padding(.top, 8)
                .padding(.bottom, 12)
            }
        }
        .contentShape(Rectangle())
        .onTapGesture {}
        .offset(x: (screen.width - panelWidth) / 2, y: 60)
    }
}

/// A workspace's row: its name, ticked if it is the one showing. One that can be renamed or
/// deleted draws back to the left under a finger to show the buttons for it.
private struct WorkspaceRow: View {
    let workspace: Workspace
    let showing: Bool
    let open: Bool
    let onOpen: (Bool) -> Void
    let onPick: () -> Void
    let onRename: () -> Void
    let onDelete: () -> Void

    @State private var moved: CGFloat?

    private func button(_ label: String, fill: Color, ink: Color, action: @escaping () -> Void) -> some View {
        Text(verbatim: label)
            .font(.system(size: 15, weight: .semibold))
            .foregroundStyle(ink)
            .frame(width: buttonWidth, height: 33)
            .background(RoundedRectangle(cornerRadius: 7, style: .circular).fill(fill))
            .contentShape(Rectangle())
            .onTapGesture(perform: action)
    }

    var body: some View {
        let x = moved ?? (open ? -reveal : 0)
        ZStack(alignment: .trailing) {
            Pc.galleryPopover
            if x < 0 {
                HStack(spacing: 3) {
                    button("Rename", fill: renameFill, ink: renameInk, action: onRename)
                    button("Delete", fill: deleteFill, ink: .white, action: onDelete)
                }
                .padding(.trailing, 8)
            }
            HStack(spacing: 0) {
                Text(verbatim: workspace.name)
                    .font(.system(size: 17))
                    .foregroundStyle(Pc.panelTitle)
                    .lineLimit(1)
                    .truncationMode(.middle)
                Spacer(minLength: 8)
                // drawn back, the room is its name's
                if showing, x == 0 {
                    PcIconView(PcIcons.menuTick, size: 22, color: Pc.accent)
                        .frame(width: 22, height: 22)
                }
            }
            .padding(.leading, 16)
            .padding(.trailing, 14)
            .frame(maxHeight: .infinity)
            .background {
                ZStack {
                    Pc.galleryPopover
                    Pc.actionsGroup
                }
            }
            .contentShape(Rectangle())
            .onTapGesture(perform: onPick)
            // it draws in from the right, its name staying where it is
            .padding(.trailing, -x)
        }
        .clipped()
        // UIKit's, behind the row: a SwiftUI drag on it keeps the list from scrolling
        .background(RowTouches(
            slides: { _ in !workspace.isDefault },
            slid: { by in
                let from: CGFloat = open ? -reveal : 0
                moved = min(max(from + by, -reveal), 0)
            },
            slideEnded: {
                guard let end = moved else { return }
                withAnimation(Motion.swipeSettle) {
                    moved = nil
                    onOpen(end < -reveal / 2)
                }
            }
        ))
    }
}

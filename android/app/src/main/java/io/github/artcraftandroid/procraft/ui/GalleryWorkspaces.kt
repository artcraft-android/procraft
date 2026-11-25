package io.github.artcraftandroid.procraft.ui

import androidx.compose.animation.core.animateFloatAsState
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.CircleShape
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.core.Workspace

// the gallery's workspaces, as the iPad app's (GalleryWorkspaces.swift): galleries of their own,
// the one showing named on a capsule in the middle of the title row; what each is, and their
// folders, are the core's (workspaces.rs)

private val WorkspacesWidth = 300.dp
private val WorkspaceRow = 44.dp
/** The rows scroll past this many, the last showing half so it is seen there are more. */
private const val MOST_ROWS = 7.5f
private val RowButtonWidth = 74.dp
/** How far a row slides to show Rename and Delete. */
private val Reveal = RowButtonWidth * 2 + 3.dp + 16.dp
private val RenameFill get() = if (Pc.light) Color(0xFFC6C6C6) else Color(0xFF3A3A3A)
private val RenameInk get() = if (Pc.light) Color(0xFF575757) else Color(0xFFC7C7C7)
private val DeleteFill = Color(0xFFFE604D)

/** A workspace's name being asked for: a new one's, or [renamed]'s; [typed] as it is typed. */
data class WorkspaceNaming(val renamed: Workspace? = null, val typed: String = renamed?.name ?: "")

/** A workspace about to be deleted, and how many artworks go with it. */
data class WorkspaceDeleting(val workspace: Workspace, val count: Int) {
    val message: String get() {
        val with = when (count) {
            0 -> ""
            1 -> "1 artwork is deleted with it. "
            else -> "$count artworks are deleted with it. "
        }
        return with + "This can't be undone."
    }
}

/** The widest the workspace's button is on a screen [width] across: what is left in the middle between the app's name and the buttons on the right. */
fun workspaceButtonRoom(width: Dp): Dp = (width - 584.dp).coerceIn(90.dp, 300.dp)

/** The capsule in the middle of the gallery's title row: the workspace showing, and a chevron for the list a tap on it opens. */
@Composable
fun WorkspaceButton(name: String, room: Dp, modifier: Modifier = Modifier, onClick: () -> Unit) {
    Row(
        modifier.height(45.dp).clip(CircleShape).background(Pc.Capsule).border(1.dp, Pc.CapsuleEdge, CircleShape)
            .clickable(onClick = onClick).padding(horizontal = 16.dp),
        verticalAlignment = Alignment.CenterVertically,
        horizontalArrangement = Arrangement.spacedBy(5.dp),
    ) {
        Text(
            name,
            color = Pc.GalleryText,
            fontSize = 16.sp,
            maxLines = 1,
            overflow = TextOverflow.MiddleEllipsis,
            modifier = Modifier.widthIn(max = room - 49.dp),
        )
        Icon(PcIcons.ChevronDown, contentDescription = "Workspaces", tint = Pc.GalleryText, modifier = Modifier.size(12.dp))
    }
}

/**
 * The workspaces, hanging from their button: a tap on one shows it (the one showing is ticked), and
 * a row slid left shows Rename and Delete, but for the first, which stays as it is. Under them, the
 * row that makes a new one.
 */
@Composable
fun WorkspacesPanel(
    modifier: Modifier,
    workspaces: List<Workspace>,
    showing: Workspace,
    onPick: (Workspace) -> Unit,
    onNew: () -> Unit,
    onRename: (Workspace) -> Unit,
    onDelete: (Workspace) -> Unit,
) {
    // the row slid open (by its folder), if one is
    var opened by remember { mutableStateOf<String?>(null) }
    val count = workspaces.size.toFloat()
    val tall = WorkspaceRow * minOf(count, MOST_ROWS) + 1.dp * (minOf(count, MOST_ROWS) - if (count > MOST_ROWS) 0f else 1f)
    Popover(WorkspacesWidth, WorkspacesWidth / 2, modifier, fill = Pc.GalleryPopover) {
        Text(
            "Workspaces",
            color = Pc.PanelTitle,
            fontSize = 24.sp,
            fontWeight = FontWeight.Bold,
            modifier = Modifier.padding(start = 24.dp, top = 14.dp, bottom = 14.dp),
        )
        Column(
            Modifier.padding(horizontal = 10.dp).fillMaxWidth().height(tall).clip(RoundedCornerShape(18.dp))
                .background(Pc.ActionsGroup).verticalScroll(rememberScrollState()),
        ) {
            workspaces.forEachIndexed { i, w ->
                if (i > 0) Box(Modifier.fillMaxWidth().height(1.dp).background(Pc.ActionsSeparator))
                WorkspaceRowView(
                    w,
                    showing = w == showing,
                    open = opened == w.root,
                    onOpen = { opened = if (it) w.root else null },
                    onPick = { if (opened != null) opened = null else onPick(w) },
                    onRename = { onRename(w) },
                    onDelete = { onDelete(w) },
                )
            }
        }
        Row(
            Modifier.padding(start = 10.dp, end = 10.dp, top = 8.dp, bottom = 12.dp).fillMaxWidth().height(WorkspaceRow)
                .clip(RoundedCornerShape(18.dp)).background(Pc.ActionsGroup).clickable(onClick = onNew)
                .padding(start = 16.dp, end = 8.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text("New Workspace", color = Pc.PanelTitle, fontSize = 17.sp, modifier = Modifier.weight(1f))
            Icon(PcIcons.Add, contentDescription = null, tint = Pc.PanelTitle, modifier = Modifier.size(30.dp))
        }
    }
}

/**
 * A workspace's row: its name, ticked if it is the one [showing]. One that can be renamed or
 * deleted draws back to the left under a finger to show the buttons for it.
 */
@Composable
private fun WorkspaceRowView(
    w: Workspace,
    showing: Boolean,
    open: Boolean,
    onOpen: (Boolean) -> Unit,
    onPick: () -> Unit,
    onRename: () -> Unit,
    onDelete: () -> Unit,
) {
    val density = LocalDensity.current
    val reveal = with(density) { Reveal.toPx() }
    // how far a finger has it (none: at rest, open or not)
    var moved by remember { mutableStateOf<Float?>(null) }
    val rest by animateFloatAsState(if (open) -reveal else 0f, Motion.swipeSettle(), label = "slid")
    val x = moved ?: rest
    Box(
        Modifier.fillMaxWidth().height(WorkspaceRow).background(Pc.GalleryPopover)
            .then(
                if (w.isDefault) Modifier else Modifier.pointerInput(open) {
                    detectHorizontalDragGestures(
                        onDragEnd = {
                            val end = moved ?: return@detectHorizontalDragGestures
                            moved = null
                            onOpen(end < -reveal / 2)
                        },
                        onDragCancel = { moved = null },
                    ) { change, by ->
                        change.consume()
                        moved = ((moved ?: if (open) -reveal else 0f) + by).coerceIn(-reveal, 0f)
                    }
                },
            ),
        contentAlignment = Alignment.CenterEnd,
    ) {
        if (x < 0f) {
            Row(Modifier.padding(end = 8.dp), horizontalArrangement = Arrangement.spacedBy(3.dp)) {
                RowButton("Rename", RenameFill, RenameInk, onRename)
                RowButton("Delete", DeleteFill, Color.White, onDelete)
            }
        }
        // it draws in from the right, its name staying where it is
        val shown = with(density) { (-x).toDp() }
        Row(
            Modifier.fillMaxSize().padding(end = shown).background(Pc.GalleryPopover).background(Pc.ActionsGroup)
                .clickable(onClick = onPick).padding(start = 16.dp, end = 14.dp),
            verticalAlignment = Alignment.CenterVertically,
        ) {
            Text(
                w.name,
                color = Pc.PanelTitle,
                fontSize = 17.sp,
                maxLines = 1,
                overflow = TextOverflow.MiddleEllipsis,
                modifier = Modifier.weight(1f),
            )
            // drawn back, the room is its name's
            if (showing && x == 0f) {
                Spacer(Modifier.width(8.dp))
                Icon(PcIcons.MenuTick, contentDescription = "Showing", tint = Pc.Accent, modifier = Modifier.size(22.dp))
            }
        }
    }
}

@Composable
private fun RowButton(label: String, fill: Color, ink: Color, onClick: () -> Unit) {
    Box(
        Modifier.size(RowButtonWidth, 33.dp).clip(RoundedCornerShape(7.dp)).background(fill).clickable(onClick = onClick),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = ink, fontSize = 15.sp, fontWeight = FontWeight.SemiBold, maxLines = 1)
    }
}

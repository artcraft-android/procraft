package io.github.artcraftandroid.procraft.ui

import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.ExperimentalFoundationApi
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.combinedClickable
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.PaddingValues
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.requiredSize
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.lazy.LazyColumn
import androidx.compose.foundation.lazy.grid.GridCells
import androidx.compose.foundation.lazy.grid.LazyVerticalGrid
import androidx.compose.foundation.lazy.grid.items
import androidx.compose.foundation.lazy.items
import androidx.compose.foundation.lazy.itemsIndexed
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.TransformOrigin
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalDensity
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.IntOffset
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.AssetLibraryInfo
import io.github.artcraftandroid.procraft.core.AssetScope
import io.github.artcraftandroid.procraft.core.PsdImportMode
import java.io.File
import kotlinx.coroutines.launch

private val AssetsWidth = 440.dp
private val FoldersWidth = 168.dp
private val AssetGap = 8.dp
private val CellFill = Color(0x14FFFFFF)
// a library's card, as the desktop measures it
private val CardHeight = 210.dp
private val CardGap = 8.dp
private val CardCorner = 16.dp
private val CardFolderRow = 19.dp
private val CardAssetHeight = 52.dp
/** Menus stay below the top bar. */
private val MenuTop = 57.dp

/**
 * The asset library: the library in use's folders on the left, the shown folder's assets as
 * thumbnails on the right. Tapping an asset puts it on the canvas ready to transform; the plus
 * captures what is on the canvas or brings pictures, PSDs, asset files and whole projects in, and a
 * new asset is named right away. The title opens every library's card.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
fun AssetsPanel(vm: CanvasViewModel, arrowX: Dp, modifier: Modifier = Modifier) {
    if (vm.showingAssetLibraries) {
        AssetLibrariesPanel(vm, arrowX, modifier)
        return
    }
    val libraries = vm.assetLibraries
    val current = vm.assetLibraryPlace
    val folders = libraries.getOrNull(current)?.folders.orEmpty()
    val shown = vm.shownAssetFolder.coerceIn(0, (folders.size - 1).coerceAtLeast(0))
    val assets = folders.getOrNull(shown)?.assets.orEmpty()
    val context = LocalContext.current
    val density = LocalDensity.current
    val scope = rememberCoroutineScope()
    var adding by remember { mutableStateOf(false) }
    var titleMenu by remember { mutableStateOf(false) }
    var folderMenu by remember { mutableStateOf<Int?>(null) }
    var assetMenu by remember { mutableStateOf<String?>(null) }
    var moveFor by remember { mutableStateOf<String?>(null) }
    var thumbFor by remember { mutableStateOf<String?>(null) }
    var thumbModeFor by remember { mutableStateOf<String?>(null) }
    var pickingProject by remember { mutableStateOf(false) }
    // the camera writes its picture to a file of ours, which goes in like a picked photo
    val photo = remember {
        val dir = File(context.cacheDir, "exports").apply { mkdirs() }
        FileProvider.getUriForFile(context, context.packageName + ".files", File(dir, "asset-camera.jpg"))
    }
    val takePhoto = rememberLauncherForActivityResult(ActivityResultContracts.TakePicture()) { taken ->
        if (taken) vm.importAssetImage(photo)
    }
    val pickFile = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri -> uri?.let { vm.importPickedFile(it) } }
    val pickThumb = rememberLauncherForActivityResult(ActivityResultContracts.GetContent()) { uri ->
        val id = thumbFor
        thumbFor = null
        if (uri != null && id != null) vm.setAssetThumbnail(id, uri)
    }
    // the folders an asset can be moved to, across the libraries; the one shown left out
    val many = libraries.size > 1
    val targets = libraries.flatMapIndexed { li, lib ->
        lib.folders.mapIndexedNotNull { fi, folder ->
            if (li == current && fi == shown) null
            else Triple(li, fi, if (many) "${lib.name} / ${folder.name}" else folder.name)
        }
    }
    val left = with(density) { 6.dp.roundToPx() }
    val below = with(density) { MenuTop.roundToPx() }

    Popover(AssetsWidth, arrowX, modifier) {
        val renaming = vm.renamingAssetLibrary == current
        PanelHeader(
            if (renaming) "" else vm.assetLibraryName,
            start = 20.dp,
            end = 12.dp,
            height = 52.dp,
            chevronOn = titleMenu,
            onTitle = { titleMenu = !titleMenu },
            menu = {
                if (renaming) InlineName(vm.assetLibraryName, Pc.PanelTitle, 24.sp) { vm.renameAssetLibrary(current, it) }
                AssetTitleMenu(vm, titleMenu) { titleMenu = false }
            },
        ) {
            Box(Modifier.padding(top = 12.dp).size(41.dp, 40.dp).clickable { adding = true }, contentAlignment = Alignment.Center) {
                Icon(PcIcons.PlusMedium, contentDescription = "Add", tint = Pc.PanelGlyph, modifier = Modifier.requiredSize(41.dp))
                // hangs below the plus, their right edges in line
                LibraryMenu(adding, TransformOrigin(1f, 0f), MenuPlace { a, size -> IntOffset(a.right - size.width, a.bottom) }, onDismiss = { adding = false }, overPanel = true) {
                    MenuItem(PcIcons.MenuCamera, "Capture current layer") {
                        adding = false
                        vm.captureAsset(AssetScope.CURRENT_LAYER)
                    }
                    MenuItem(PcIcons.MenuGroup, "Capture group") {
                        adding = false
                        vm.captureAsset(AssetScope.GROUP)
                    }
                    MenuItem(PcIcons.MenuSelectAll, "Capture picked layers") {
                        adding = false
                        vm.captureAsset(AssetScope.PICKED)
                    }
                    MenuItem(PcIcons.MenuPhoto, "Capture canvas") {
                        adding = false
                        vm.captureAsset(AssetScope.CANVAS)
                    }
                    MenuItem(PcIcons.MenuCamera, "Capture selection") {
                        adding = false
                        vm.captureAsset(AssetScope.SELECTION)
                    }
                    MenuDivider(11.dp)
                    MenuItem(PcIcons.MenuCamera, "Camera") {
                        adding = false
                        runCatching { takePhoto.launch(photo) }.onFailure { vm.message = "No camera app is available." }
                    }
                    MenuItem(PcIcons.MenuPhoto, "Import file...") {
                        adding = false
                        pickFile.launch(arrayOf("image/*", "application/zip", "application/octet-stream", "*/*"))
                    }
                    MenuItem(PcIcons.MenuLibraries, "Import from project...") {
                        adding = false
                        vm.readProjects()
                        pickingProject = true
                    }
                    MenuDivider(11.dp)
                    MenuItem(PcIcons.MenuNewLibrary, "New library") {
                        adding = false
                        vm.newAssetLibraryAndOpen()
                    }
                    MenuItem(PcIcons.MenuNewSet, "New folder") {
                        adding = false
                        vm.newAssetFolder()
                    }
                }
                // the artworks an asset can be made from
                LibraryMenu(pickingProject, TransformOrigin(1f, 0f), MenuPlace { a, size -> IntOffset(a.right - size.width, a.bottom) }, onDismiss = { pickingProject = false }, overPanel = true) {
                    if (vm.projectChoices.isEmpty()) {
                        MenuItem(PcIcons.MenuLibraries, "No artworks yet") { pickingProject = false }
                    } else {
                        LazyColumn(Modifier.fillMaxWidth().heightIn(max = 320.dp)) {
                            items(vm.projectChoices) { a ->
                                MenuItem(PcIcons.MenuFolder, a.name) {
                                    pickingProject = false
                                    vm.importAssetProject(a)
                                }
                            }
                        }
                    }
                }
            }
        }
        if (folders.all { it.assets.isEmpty() }) {
            Box(Modifier.weight(1f).fillMaxWidth(), contentAlignment = Alignment.TopCenter) {
                Column(Modifier.padding(top = 60.dp, start = 32.dp, end = 32.dp), horizontalAlignment = Alignment.CenterHorizontally) {
                    Text("This library has no assets yet", color = Pc.PanelTitle, fontSize = 17.sp, fontWeight = FontWeight.SemiBold, maxLines = 2)
                    Text("Capture or import with + to add some.", color = Pc.PanelGlyph, fontSize = 14.sp, modifier = Modifier.padding(top = 8.dp))
                }
            }
        } else {
            Row(Modifier.fillMaxWidth().weight(1f)) {
                LazyColumn(
                    Modifier.width(FoldersWidth).fillMaxHeight().background(Pc.SetColumn),
                    contentPadding = PaddingValues(12.dp),
                    verticalArrangement = Arrangement.spacedBy(8.dp),
                ) {
                    itemsIndexed(folders) { f, folder ->
                        val selected = f == shown
                        Box {
                            Row(
                                Modifier.fillMaxWidth().height(50.dp).clip(RoundedCornerShape(12.dp))
                                    .background(if (selected) Pc.RowSelected else Pc.SetCell)
                                    .combinedClickable(
                                        onClick = { vm.shownAssetFolder = f },
                                        onLongClick = { folderMenu = f },
                                    ),
                                verticalAlignment = Alignment.CenterVertically,
                            ) {
                                val ink = if (selected) Color.White else Pc.PanelTitle
                                Box(Modifier.padding(start = 4.dp).size(40.dp), contentAlignment = Alignment.Center) {
                                    Icon(PcIcons.Folder, contentDescription = null, tint = ink, modifier = Modifier.requiredSize(26.dp))
                                }
                                val nameModifier = Modifier.padding(start = 1.dp, end = 8.dp)
                                if (vm.renamingAssetFolder == f) {
                                    InlineName(folder.name, ink, 14.sp, nameModifier) { vm.renameAssetFolder(f, it) }
                                } else {
                                    Text(
                                        folder.name,
                                        color = ink,
                                        fontSize = 14.sp,
                                        fontWeight = FontWeight.SemiBold,
                                        lineHeight = 16.sp,
                                        maxLines = 2,
                                        overflow = TextOverflow.Ellipsis,
                                        modifier = nameModifier,
                                    )
                                }
                            }
                            // left of the folder, centred on it
                            LibraryMenu(folderMenu == f, TransformOrigin(1f, 0.5f), MenuPlace(below) { a, size -> IntOffset(a.left - size.width - left, a.center.y - size.height / 2) }, onDismiss = { folderMenu = null }) {
                                MenuItem(PcIcons.MenuRename, "Rename") {
                                    folderMenu = null
                                    vm.renamingAssetFolder = f
                                }
                                MenuItem(PcIcons.MenuShare, "Share") {
                                    folderMenu = null
                                    scope.launch { vm.shareAssetFolder(current, f)?.let { shareFile(context, it, "application/octet-stream") } }
                                }
                                if (folders.size > 1) {
                                    MenuDivider(11.dp)
                                    MenuItem(PcIcons.MenuTrash, "Delete", Pc.MenuDanger) {
                                        folderMenu = null
                                        vm.deletingAssetFolder = f
                                    }
                                }
                            }
                        }
                    }
                }
                // another folder's assets replace these at once
                androidx.compose.runtime.key(shown) {
                    val gridState = androidx.compose.foundation.lazy.grid.rememberLazyGridState()
                    // a new asset named right away may be past the end: bring it into sight
                    LaunchedEffect(vm.renamingAsset) {
                        val id = vm.renamingAsset ?: return@LaunchedEffect
                        assets.indexOfFirst { it.id == id }.takeIf { it >= 0 }?.let { gridState.scrollToItem(it) }
                    }
                    LazyVerticalGrid(
                        columns = GridCells.Fixed(2),
                        state = gridState,
                        modifier = Modifier.weight(1f).fillMaxHeight().background(Pc.BrushColumn),
                        contentPadding = PaddingValues(12.dp),
                        horizontalArrangement = Arrangement.spacedBy(AssetGap),
                        verticalArrangement = Arrangement.spacedBy(AssetGap),
                    ) {
                        items(assets, key = { it.id }) { asset ->
                            val preview = vm.assetPreviews[asset.id]
                            LaunchedEffect(asset.id, preview == null) { if (preview == null) vm.loadAssetPreview(asset.id) }
                            Box {
                                Column(
                                    Modifier.fillMaxWidth().clip(RoundedCornerShape(10.dp))
                                        .combinedClickable(onClick = { vm.placeAsset(asset.id) }, onLongClick = { assetMenu = asset.id }),
                                ) {
                                    Box(
                                        Modifier.fillMaxWidth().aspectRatio(1.25f).clip(RoundedCornerShape(10.dp)).background(CellFill),
                                        contentAlignment = Alignment.Center,
                                    ) {
                                        preview?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize().padding(6.dp)) }
                                    }
                                    if (vm.renamingAsset == asset.id) {
                                        InlineName(asset.name, Pc.PanelTitle, 13.sp, Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 3.dp)) { vm.renameAsset(asset.id, it) }
                                    } else {
                                        Text(
                                            asset.name,
                                            color = Pc.PanelTitle,
                                            fontSize = 13.sp,
                                            fontWeight = FontWeight.SemiBold,
                                            maxLines = 1,
                                            overflow = TextOverflow.Ellipsis,
                                            textAlign = TextAlign.Center,
                                            modifier = Modifier.fillMaxWidth().padding(horizontal = 4.dp, vertical = 5.dp),
                                        )
                                    }
                                }
                                LibraryMenu(assetMenu == asset.id, TransformOrigin(1f, 0.5f), MenuPlace(below) { a, size -> IntOffset(a.left - size.width - left, a.center.y - size.height / 2) }, onDismiss = { assetMenu = null }) {
                                    MenuItem(PcIcons.MenuRename, "Rename") {
                                        assetMenu = null
                                        vm.renamingAsset = asset.id
                                    }
                                    MenuItem(PcIcons.MenuDuplicate, "Duplicate") {
                                        assetMenu = null
                                        vm.duplicateAsset(asset.id)
                                    }
                                    MenuItem(PcIcons.MenuFolder, "Move to folder") {
                                        assetMenu = null
                                        if (targets.isNotEmpty()) moveFor = asset.id
                                    }
                                    MenuItem(PcIcons.MenuShare, "Share") {
                                        assetMenu = null
                                        scope.launch { vm.shareAsset(asset.id)?.let { shareFile(context, it, "application/octet-stream") } }
                                    }
                                    MenuItem(PcIcons.MenuPhoto, "Set thumbnail") {
                                        assetMenu = null
                                        thumbModeFor = asset.id
                                    }
                                    MenuDivider()
                                    MenuItem(PcIcons.MenuTrash, "Delete", Pc.MenuDanger) {
                                        assetMenu = null
                                        vm.deletingAsset = asset.id
                                    }
                                }
                                if (moveFor == asset.id) {
                                    LibraryMenu(true, TransformOrigin(1f, 0.5f), MenuPlace(below) { a, size -> IntOffset(a.left - size.width - left, a.center.y - size.height / 2) }, onDismiss = { moveFor = null }) {
                                        targets.forEach { (li, fi, label) ->
                                            MenuItem(PcIcons.Folder, label) {
                                                moveFor = null
                                                vm.moveAsset(asset.id, li, fi)
                                            }
                                        }
                                    }
                                }
                                if (thumbModeFor == asset.id) {
                                    LibraryMenu(true, TransformOrigin(1f, 0.5f), MenuPlace(below) { a, size -> IntOffset(a.left - size.width - left, a.center.y - size.height / 2) }, onDismiss = { thumbModeFor = null }) {
                                        MenuItem(PcIcons.MenuPhoto, "From canvas") {
                                            thumbModeFor = null
                                            vm.setAssetThumbnailFromCanvas(asset.id)
                                        }
                                        MenuItem(PcIcons.MenuCamera, "From current layer") {
                                            thumbModeFor = null
                                            vm.setAssetThumbnailFromLayer(asset.id)
                                        }
                                        MenuItem(PcIcons.MenuDuplicate, "From clipboard") {
                                            thumbModeFor = null
                                            vm.setAssetThumbnailFromClipboard(asset.id)
                                        }
                                        MenuItem(PcIcons.MenuFolder, "From file") {
                                            thumbModeFor = null
                                            thumbFor = asset.id
                                            pickThumb.launch("image/*")
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    Alert(
        vm.deletingAssetFolder,
        title = { "Delete this folder?" },
        message = { "It can't be brought back." },
        action = "Delete",
        onCancel = { vm.deletingAssetFolder = null },
        onAction = { vm.deleteAssetFolder(it) },
    )
    Alert(
        vm.deletingAsset,
        title = { "Delete this asset?" },
        message = { "It can't be brought back." },
        action = "Delete",
        onCancel = { vm.deletingAsset = null },
        onAction = { vm.deleteAsset(it) },
    )
    Alert(
        vm.psdPending,
        title = { "Import PSD" },
        message = { "Bring the layers in as one group, or each as its own asset?" },
        action = "Group",
        onCancel = { vm.psdPending = null },
        onAction = { vm.importPsd(PsdImportMode.GROUP) },
        icon = PcIcons.MenuFolder,
        destructive = false,
        extra = "Explode",
        onExtra = { vm.importPsd(PsdImportMode.EXPLODE) },
    )
}

/** The library's title menu: rename it, share it, or go back to every library's card. */
@Composable
private fun AssetTitleMenu(vm: CanvasViewModel, open: Boolean, onDismiss: () -> Unit) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    LibraryMenu(open, TransformOrigin(0f, 0f), MenuPlace { a, _ -> IntOffset(a.left, a.bottom) }, onDismiss = onDismiss, overPanel = true) {
        MenuItem(PcIcons.MenuRename, "Rename") {
            onDismiss()
            vm.renamingAssetLibrary = vm.assetLibraryPlace
        }
        MenuItem(PcIcons.MenuShare, "Share") {
            onDismiss()
            scope.launch { vm.shareAssetLibrary(vm.assetLibraryPlace)?.let { shareFile(context, it, "application/octet-stream") } }
        }
        MenuDivider(11.dp)
        MenuItem(PcIcons.MenuLibraries, "Back to libraries") {
            onDismiss()
            vm.showingAssetLibraries = true
        }
    }
}

/**
 * The asset popover showing every library's card (tapped, that library is brought into use; held,
 * its menu), with "+" for a new one.
 */
@Composable
private fun AssetLibrariesPanel(vm: CanvasViewModel, arrowX: Dp, modifier: Modifier) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var adding by remember { mutableStateOf(false) }
    var cardMenu by remember { mutableStateOf<Int?>(null) }
    Popover(AssetsWidth, arrowX, modifier) {
        PanelHeader(
            "Asset libraries",
            start = 20.dp,
            end = 12.dp,
            height = 52.dp,
            chevron = false,
        ) {
            Box(Modifier.padding(top = 12.dp).size(41.dp, 40.dp).clickable { adding = true }, contentAlignment = Alignment.Center) {
                Icon(PcIcons.PlusMedium, contentDescription = "Add", tint = Pc.PanelGlyph, modifier = Modifier.requiredSize(41.dp))
                LibraryMenu(adding, TransformOrigin(1f, 0f), MenuPlace { a, size -> IntOffset(a.right - size.width, a.bottom) }, onDismiss = { adding = false }, overPanel = true) {
                    MenuItem(PcIcons.MenuNewLibrary, "Create new library") {
                        adding = false
                        vm.newAssetLibrary()
                    }
                }
            }
        }
        LazyVerticalGrid(
            GridCells.Fixed(2),
            Modifier.weight(1f, fill = false),
            contentPadding = PaddingValues(start = 12.dp, end = 12.dp, bottom = 12.dp),
            horizontalArrangement = Arrangement.spacedBy(CardGap),
            verticalArrangement = Arrangement.spacedBy(CardGap),
        ) {
            items(vm.assetLibraries.size) { i ->
                val library = vm.assetLibraries[i]
                Box {
                    AssetLibraryCard(vm, i, library, onOpen = { vm.openAssetLibrary(i) }, onMenu = { cardMenu = i })
                    LibraryMenu(cardMenu == i, TransformOrigin(0.5f, 0f), MenuPlace { a, size -> IntOffset(a.left + (a.width - size.width) / 2, a.top + a.height / 4) }, onDismiss = { cardMenu = null }) {
                        MenuItem(PcIcons.MenuRename, "Rename") {
                            cardMenu = null
                            vm.renamingAssetLibrary = i
                        }
                        MenuItem(PcIcons.MenuShare, "Share") {
                            cardMenu = null
                            scope.launch { vm.shareAssetLibrary(i)?.let { shareFile(context, it, "application/octet-stream") } }
                        }
                        if (vm.assetLibraries.size > 1) {
                            MenuDivider(11.dp)
                            MenuItem(PcIcons.MenuTrash, "Delete", Pc.MenuDanger) {
                                cardMenu = null
                                vm.deletingAssetLibrary = i
                            }
                        }
                    }
                }
            }
        }
    }
    Alert(
        vm.deletingAssetLibrary,
        title = { "Delete this library?" },
        message = { "Its folders and assets go with it." },
        action = "Delete",
        onCancel = { vm.deletingAssetLibrary = null },
        onAction = { vm.deleteAssetLibrary(it) },
    )
}

/**
 * A library's card: its name over its folders down the left and a few assets of one of them beside
 * them; the one in use outlined in blue.
 */
@OptIn(ExperimentalFoundationApi::class)
@Composable
private fun AssetLibraryCard(vm: CanvasViewModel, i: Int, library: AssetLibraryInfo, onOpen: () -> Unit, onMenu: () -> Unit) {
    val shape = RoundedCornerShape(CardCorner)
    Column(
        Modifier.fillMaxWidth().height(CardHeight).clip(shape).background(Pc.SetCell)
            .then(if (library.current) Modifier.border(2.dp, Pc.Accent, shape) else Modifier)
            .combinedClickable(onClick = onOpen, onLongClick = onMenu)
            .padding(10.dp),
    ) {
        if (vm.renamingAssetLibrary == i) {
            InlineName(library.name, Pc.PanelTitle, 14.sp) { vm.renameAssetLibrary(i, it) }
        } else {
            Text(library.name, color = Pc.PanelTitle, fontSize = 14.sp, fontWeight = FontWeight.SemiBold, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        // the one in use's shown folder; another's first with an asset in it
        val chosen = if (library.current) {
            vm.shownAssetFolder.coerceIn(0, (library.folders.size - 1).coerceAtLeast(0))
        } else {
            library.folders.indexOfFirst { it.assets.isNotEmpty() }.coerceAtLeast(0)
        }
        Row(Modifier.padding(top = 8.dp).fillMaxSize()) {
            Column(Modifier.weight(0.45f), verticalArrangement = Arrangement.spacedBy(3.dp)) {
                library.folders.forEachIndexed { f, folder ->
                    Row(
                        Modifier.fillMaxWidth().height(CardFolderRow).clip(RoundedCornerShape(5.dp))
                            .background(if (f == chosen) Pc.RowSelected else Pc.PopoverFill.copy(alpha = 0.35f)),
                        verticalAlignment = Alignment.CenterVertically,
                    ) {
                        val ink = if (f == chosen) Color.White else Pc.PanelTitle
                        Icon(PcIcons.Folder, contentDescription = null, tint = ink, modifier = Modifier.requiredSize(14.dp))
                        Text(folder.name, color = ink, fontSize = 8.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
                    }
                }
            }
            Column(Modifier.weight(0.55f).padding(start = 6.dp), verticalArrangement = Arrangement.spacedBy(2.dp)) {
                library.folders.getOrNull(chosen)?.assets?.forEach { asset ->
                    val preview = vm.assetPreviews[asset.id]
                    LaunchedEffect(asset.id, preview == null) { if (preview == null) vm.loadAssetPreview(asset.id) }
                    Row(Modifier.fillMaxWidth().height(CardAssetHeight), verticalAlignment = Alignment.CenterVertically) {
                        Box(Modifier.size(CardAssetHeight - 8.dp).clip(RoundedCornerShape(6.dp)).background(CellFill), contentAlignment = Alignment.Center) {
                            preview?.let { Image(it, contentDescription = null, contentScale = ContentScale.Fit, modifier = Modifier.fillMaxSize().padding(3.dp)) }
                        }
                        Text(asset.name, color = Pc.PanelTitle, fontSize = 9.sp, maxLines = 1, overflow = TextOverflow.Ellipsis, modifier = Modifier.padding(start = 5.dp))
                    }
                }
            }
        }
    }
}

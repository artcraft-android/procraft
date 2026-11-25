package io.github.artcraftandroid.procraft.ui

import android.content.ContentResolver
import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import android.net.Uri
import android.provider.OpenableColumns
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.PickVisualMediaRequest
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.clickable
import androidx.compose.foundation.horizontalScroll
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxHeight
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.heightIn
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.widthIn
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.selection.toggleable
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.CircularProgressIndicator
import androidx.compose.material3.DropdownMenu
import androidx.compose.material3.DropdownMenuItem
import androidx.compose.material3.Icon
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
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
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.semantics.Role
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.text.TextStyle
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.window.Dialog
import androidx.compose.ui.window.DialogProperties
import io.github.artcraftandroid.procraft.AppLog
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.CanvasSession
import io.github.artcraftandroid.procraft.core.StorePackageCover
import io.github.artcraftandroid.procraft.core.StorePackageDetails
import io.github.artcraftandroid.procraft.core.StorePackageImageChoices
import io.github.artcraftandroid.procraft.core.ThumbnailImage
import io.github.artcraftandroid.procraft.core.storeSquarePng
import io.github.artcraftandroid.procraft.core.writeCopyPng
import io.github.artcraftandroid.procraft.core.storeSharedFile
import io.github.artcraftandroid.procraft.core.openAssetPackageCover
import java.io.File
import java.nio.file.Files
import java.util.zip.ZipEntry
import java.util.zip.ZipOutputStream
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

private val categories = listOf("Brushes", "Papers and Textures", "Colour", "Layers", "Interface", "Effects", "Canvas", "Other")
private val licences = listOf("CC0-1.0", "CC BY 4.0", "CC BY-SA 4.0", "MIT", "All rights reserved")
private const val makerLimit = 4

@Composable
internal fun StorePackageOverlay(vm: CanvasViewModel) {
    val file = vm.storePackageSharedFile
    val set = vm.storePackageSet
    if (file == null && set == null) return
    val session = vm.session
    val name = if (set != null) vm.brushSets.getOrNull(set)?.name else file?.nameWithoutExtension
    val library = vm.brushLibraries.indexOfFirst { it.current }
    fun close() {
        vm.storePackageSet = null
        vm.storePackageSharedFile = null
    }
    DisposableEffect(file) { onDispose { file?.delete() } }
    if (session == null || name == null || library < 0) {
        LaunchedEffect(Unit) { close() }
        return
    }
    val shared = file?.let { remember(it) { ChosenAsset(it, name, true) } }
    StorePackageDialog(vm, session, library.toUInt(), (set ?: 0).toUInt(), name, shared, allowBrushSet = set != null, close = ::close)
}

private data class PreparedPreview(
    val png: ByteArray,
    val picture: Bitmap,
    val iconPng: ByteArray,
    val icon: Bitmap,
    val template: String?,
    val iconTemplate: String?,
    val title: String?,
)
private data class ChosenAsset(val file: File, val name: String, val brushes: Boolean)
private data class MakerImage(val file: File, val picture: Bitmap)
private enum class ArtworkTarget { COVER, ICON, GALLERY }

@Composable
private fun StorePackageDialog(
    vm: CanvasViewModel,
    session: CanvasSession,
    library: UInt,
    set: UInt,
    setName: String,
    shared: ChosenAsset?,
    allowBrushSet: Boolean,
    close: () -> Unit,
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val scratch = remember(context) { File(context.cacheDir, "store-package-" + System.nanoTime()) }
    var name by remember { mutableStateOf(setName) }
    var author by remember { mutableStateOf("") }
    var description by remember { mutableStateOf("") }
    var tags by remember { mutableStateOf("") }
    var category by remember { mutableStateOf(categories.first()) }
    var licence by remember { mutableStateOf(licences[1]) }
    var useAsset by remember { mutableStateOf(shared != null) }
    var asset by remember { mutableStateOf(shared) }
    var template by remember { mutableStateOf<String?>(null) }
    var iconTemplate by remember { mutableStateOf<String?>(null) }
    var showTitle by remember { mutableStateOf(true) }
    var cover by remember { mutableStateOf<StorePackageCover?>(null) }
    var preview by remember { mutableStateOf<PreparedPreview?>(null) }
    var preparing by remember { mutableStateOf(true) }
    var rendering by remember { mutableStateOf(false) }
    var reading by remember { mutableStateOf(false) }
    var exporting by remember { mutableStateOf(false) }
    var exported by remember { mutableStateOf<File?>(null) }
    var customCover by remember { mutableStateOf<MakerImage?>(null) }
    var customIcon by remember { mutableStateOf<MakerImage?>(null) }
    var generated by remember { mutableStateOf(emptyList<Bitmap>()) }
    var removedPages by remember { mutableStateOf(emptySet<Int>()) }
    var galleryLoading by remember { mutableStateOf(true) }
    var galleryError by remember { mutableStateOf(false) }
    var imageTarget by remember { mutableStateOf(ArtworkTarget.COVER) }
    var images by remember { mutableStateOf(emptyList<MakerImage>()) }
    var error by remember { mutableStateOf<String?>(null) }
    val busy = reading || exporting
    val contentsOnly = useAsset && asset?.brushes == true

    // File picker work can outlive its activity until the result comes back.
    DisposableEffect(scratch) {
        onDispose { scratch.deleteRecursively() }
    }

    val saveArchive = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/zip")) { uri ->
        val source = exported
        if (uri == null || source == null) return@rememberLauncherForActivityResult
        exporting = true
        scope.launch {
            val saved = withContext(Dispatchers.IO) {
                runCatching {
                    requireNotNull(context.contentResolver.openOutputStream(uri, "w")).use { output ->
                        source.inputStream().use { it.copyTo(output) }
                    }
                }.onFailure { AppLog.e("store", "saving a package ZIP", it) }.isSuccess
            }
            exporting = false
            if (saved) close() else error = "The ZIP couldn't be saved."
        }
    }

    val chooseAsset = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri == null) return@rememberLauncherForActivityResult
        reading = true
        error = null
        scope.launch {
            val chosen = withContext(Dispatchers.IO) { readAsset(context, uri, scratch) }
            if (chosen == null) {
                error = "Choose a ProCraft brush or asset share file."
            } else {
                if (name.isBlank() || name == setName || name == asset?.name) name = chosen.name
                asset?.file?.delete()
                asset = chosen
                if (category in categories.take(2)) category = categories[if (chosen.brushes) 0 else 1]
                preview = null
                preparing = true
            }
            reading = false
        }
    }

    fun accept(target: ArtworkTarget, pictures: List<MakerImage>, asked: Int) {
        if (pictures.isEmpty()) {
            error = "Couldn't read an image from that source."
            return
        }
        when (target) {
            ArtworkTarget.COVER -> {
                customCover?.file?.delete()
                customCover = pictures.first()
            }
            ArtworkTarget.ICON -> {
                customIcon?.file?.delete()
                customIcon = pictures.first()
            }
            ArtworkTarget.GALLERY -> images = images + pictures
        }
        if (pictures.size < asked) error = "One or more gallery images couldn't be read."
    }

    fun choose(target: ArtworkTarget, uris: List<Uri>) {
        if (uris.isEmpty()) return
        val picked = uris.take(if (target == ArtworkTarget.GALLERY) makerLimit - images.size else 1)
        reading = true
        error = null
        scope.launch {
            val found = withContext(Dispatchers.IO) {
                picked.mapNotNull { readMakerImage(context.contentResolver, it, scratch, square = target == ArtworkTarget.ICON) }
            }
            accept(target, found, picked.size)
            reading = false
        }
    }

    val photo = rememberLauncherForActivityResult(ActivityResultContracts.PickVisualMedia()) { uri ->
        if (uri != null) choose(imageTarget, listOf(uri))
    }
    val photos = rememberLauncherForActivityResult(ActivityResultContracts.PickMultipleVisualMedia(makerLimit)) {
        choose(imageTarget, it)
    }
    val file = rememberLauncherForActivityResult(ActivityResultContracts.OpenDocument()) { uri ->
        if (uri != null) choose(imageTarget, listOf(uri))
    }
    val files = rememberLauncherForActivityResult(ActivityResultContracts.OpenMultipleDocuments()) {
        choose(imageTarget, it)
    }
    val photoRequest = PickVisualMediaRequest(ActivityResultContracts.PickVisualMedia.ImageOnly)

    fun fromPhotos(target: ArtworkTarget) {
        imageTarget = target
        if (target == ArtworkTarget.GALLERY && makerLimit - images.size > 1) photos.launch(photoRequest)
        else photo.launch(photoRequest)
    }

    fun fromFiles(target: ArtworkTarget) {
        imageTarget = target
        if (target == ArtworkTarget.GALLERY) files.launch(arrayOf("image/*"))
        else file.launch(arrayOf("image/*"))
    }

    fun takeImage(target: ArtworkTarget, snapshot: Boolean) {
        error = null
        reading = true
        val (uri, copied) = if (snapshot) null to null else vm.storeClipboardImage()
        scope.launch {
            val chosen = withContext(Dispatchers.IO) {
                val square = target == ArtworkTarget.ICON
                if (uri != null) readMakerImage(context.contentResolver, uri, scratch, square)
                else {
                    val pixels = if (snapshot) session.canvasThumbnail(1600u) else copied
                    pixels?.let { readCanvasImage(it, scratch, square) }
                }
            }
            accept(target, listOfNotNull(chosen), 1)
            reading = false
        }
    }

    LaunchedEffect(session, library, set, useAsset, asset) {
        generated = emptyList()
        removedPages = emptySet()
        galleryError = false
        galleryLoading = !useAsset || asset != null
        if (!galleryLoading) return@LaunchedEffect
        val path = asset?.file?.path?.takeIf { useAsset }
        val title = name
        val pages = withContext(Dispatchers.IO) {
            runCatching { session.storePackageGallery(library, set, path, title).map { it.toStoreBitmap() } }
                .onFailure { AppLog.e("store", "preparing gallery pages", it) }
        }
        generated = pages.getOrDefault(emptyList())
        galleryError = pages.isFailure
        galleryLoading = false
    }

    LaunchedEffect(session, library, set, useAsset, asset) {
        cover = null
        preview = null
        template = null
        iconTemplate = null
        rendering = false
        preparing = !contentsOnly
        if (contentsOnly) return@LaunchedEffect
        val assetPath = asset?.file?.path
        cover = withContext(Dispatchers.Default) {
            runCatching {
                if (useAsset) assetPath?.let(::openAssetPackageCover) else session.prepareStorePackageCover(library, set, setName)
            }.onFailure { AppLog.e("store", "preparing a package cover", it) }.getOrNull()
        }
        preparing = false

    }

    DisposableEffect(cover) {
        val prepared = cover
        onDispose { prepared?.destroy() }
    }

    val effectiveTitle = name.trim().takeIf { showTitle && it.isNotEmpty() }
    LaunchedEffect(cover, template, iconTemplate, effectiveTitle) {
        val art = cover ?: return@LaunchedEffect
        val layout = template
        val title = effectiveTitle
        val iconLayout = iconTemplate ?: layout
        rendering = true
        delay(110)
        val made = withContext(Dispatchers.Default) {
            runCatching {
                val png = art.renderPng(layout, title)
                val iconPng = art.renderSquarePng(iconLayout, title)
                val painting = BitmapFactory.decodeByteArray(png, 0, png.size)
                val icon = BitmapFactory.decodeByteArray(iconPng, 0, iconPng.size)
                if (painting != null && icon != null) PreparedPreview(png, painting, iconPng, icon, layout, iconLayout, title) else null
            }.onFailure { AppLog.e("store", "rendering a package cover", it) }.getOrNull()
        }
        preview = made
        if (made == null) error = "The cover couldn't be made."
        rendering = false
    }

    val usesAutomaticCover = customCover == null && !contentsOnly
    val currentPreview = preview?.takeIf {
        it.template == template && it.iconTemplate == (iconTemplate ?: template) && it.title == effectiveTitle
    }
    val automaticReady = !preparing && !rendering && currentPreview != null
    val usesAutomaticIcon = customIcon == null && !contentsOnly && (preparing || cover != null)
    val coverReady = (!usesAutomaticCover && !usesAutomaticIcon) || automaticReady
    val ready = !busy && coverReady && !galleryLoading && !galleryError &&
        name.isNotBlank() && description.isNotBlank() && tags.split(',').any { it.isNotBlank() } &&
        (!useAsset || asset != null)

    LaunchedEffect(name, author, description, category, licence, tags, currentPreview, customCover, customIcon, removedPages, images, useAsset, asset) {
        if (!exporting) exported = null
    }

    StoreDialog(
        "Store Details", 520.dp, 720.dp, !busy, close, actionLabel = "Export", actionEnabled = ready, onAction = export@{
            val automatic = currentPreview
            val coverImage = customCover
            val iconImage = customIcon
            val details = StorePackageDetails(name, author, description, category, licence, tags)
            val original = asset?.file?.path?.takeIf { useAsset }
            val maker = images.map { it.file.path }
            exporting = true
            exported = null
            error = null
            scope.launch {
                val made = withContext(Dispatchers.IO) {
                    runCatching {
                        val coverPng = coverImage?.file?.readBytes() ?: automatic?.png ?: byteArrayOf()
                        val iconPng = iconImage?.file?.readBytes() ?: automatic?.iconPng ?: storeSquarePng(coverPng)
                        val choices = StorePackageImageChoices(coverPng, iconPng, removedPages.sorted().map { it.toUInt() }, maker)
                        exportArchive(context, session, library, set, original, details, choices)
                    }
                }
                exporting = false
                made.onSuccess { exported = it }.onFailure { problem ->
                    AppLog.e("store", "exporting a package", problem)
                    error = problem.message ?: "The package couldn't be exported."
                }
            }
        },
    ) {
        if (exporting) Text("Preparing package…", color = ROW_INK, modifier = Modifier.padding(16.dp))
        exported?.let { output ->
            Row(Modifier.fillMaxWidth().padding(horizontal = 16.dp), horizontalArrangement = Arrangement.End) {
                StoreTextAction("Save ZIP", !busy) { saveArchive.launch(output.name) }
                StoreTextAction("Share ZIP", !busy) {
                    shareFile(context, output, "application/zip")
                    close()
                }
            }
        }
        error?.let { Text(it, color = if (Pc.light) Color(0xFFD32B21) else Color(0xFFFF453A),
            modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)) }
        Column(Modifier.weight(1f).verticalScroll(rememberScrollState()).padding(horizontal = 16.dp)) {
            GroupHead("ARTWORK")
            Row(Modifier.fillMaxWidth().padding(bottom = 8.dp), horizontalArrangement = Arrangement.spacedBy(12.dp)) {
                ArtworkPreview("Cover", customCover?.picture ?: preview?.picture, usesAutomaticCover && (preparing || rendering), Modifier.weight(4f), 4f / 3f)
                ArtworkPreview("Square icon", customIcon?.picture ?: preview?.icon ?: customCover?.picture,
                    customIcon == null && !contentsOnly && (preparing || rendering), Modifier.weight(3f), 1f)
            }
            StoreFormGroup {
                ArtworkSourceRow("Cover image", ArtworkTarget.COVER, !busy, ::fromPhotos, ::fromFiles, ::takeImage)
                GroupRule()
                ArtworkSourceRow("Square icon", ArtworkTarget.ICON, !busy, ::fromPhotos, ::fromFiles, ::takeImage)
                if (customCover != null) {
                    GroupRule()
                    StoreFormRow(if (contentsOnly) "Remove custom cover" else "Use automatic cover", labelColor = ACCENT,
                        enabled = !busy, onClick = { customCover?.file?.delete(); customCover = null })
                }
                if (customIcon != null) {
                    GroupRule()
                    StoreFormRow("Use automatic icon", labelColor = ACCENT, enabled = !busy,
                        onClick = { customIcon?.file?.delete(); customIcon = null })
                }
                val templates = remember(cover) { cover?.templates().orEmpty() }
                val recommendation = remember(cover) { cover?.recommendation() }
                val auto = recommendation?.let { "Auto · " + it } ?: "Auto"
                if (templates.size > 1 && (customCover == null || customIcon == null)) {
                    if (customCover == null) {
                        GroupRule()
                        PackageChoice("Cover layout", template ?: auto, listOf(auto) + templates, !busy) {
                            template = it.takeUnless { value -> value == auto }
                        }
                    }
                    if (customIcon == null) {
                        GroupRule()
                        val match = "Match cover layout"
                        PackageChoice("Icon layout", iconTemplate ?: match, listOf(match) + templates, !busy) {
                            iconTemplate = it.takeUnless { value -> value == match }
                        }
                    }
                    GroupRule()
                    StoreFormRow("Show title on cover") {
                        StoreSwitch(showTitle, Modifier
                            .semantics { contentDescription = "Show title on cover" }
                            .toggleable(value = showTitle, enabled = !busy, role = Role.Switch, onValueChange = { showTitle = it }))
                    }
                }
            }
            GroupHead("GALLERY IMAGES")
            if (galleryLoading) GroupNote("Preparing the contents pages…", last = true)
            if (galleryError) GroupNote("The contents pages couldn't be prepared. Choose another shared file or reopen Share.", danger = true, last = true)
            if (generated.isNotEmpty() || images.isNotEmpty()) {
                Row(Modifier.horizontalScroll(rememberScrollState()).padding(bottom = 8.dp),
                    horizontalArrangement = Arrangement.spacedBy(8.dp)) {
                    generated.forEachIndexed { at, bitmap ->
                        if (at !in removedPages) GalleryTile(bitmap, "Contents page " + (at + 1), !busy) {
                            removedPages = removedPages + at
                        }
                    }
                    images.forEachIndexed { at, image ->
                        GalleryTile(image.picture, "Gallery image " + (at + 1), !busy) {
                            images = images.filterIndexed { index, _ -> index != at }
                            image.file.delete()
                        }
                    }
                }
            }
            StoreFormGroup {
                ArtworkSourceRow("Add gallery images", ArtworkTarget.GALLERY, !busy && images.size < makerLimit,
                    ::fromPhotos, ::fromFiles, ::takeImage)
                if (removedPages.isNotEmpty()) {
                    GroupRule()
                    StoreFormRow("Restore contents pages", labelColor = ACCENT, enabled = !busy, onClick = { removedPages = emptySet() })
                }
            }
            GroupHead("DETAILS")
            Column {
                StoreFormGroup {
                    StoreFormRow("Name") { PackageField("Name", name, { name = it }, !busy) }
                    GroupRule()
                    StoreFormRow("Author") { PackageField("Author", author, { author = it }, !busy) }
                    GroupRule()
                    val kind = if (useAsset) "Shared file" else "Brush set"
                    if (allowBrushSet) {
                        PackageChoice("Kind", kind, listOf("Brush set", "Shared file"), !busy) {
                            val next = it == "Shared file"
                            useAsset = next
                            preparing = true
                            preview = null
                            val before = if (next) setName else asset?.name
                            if (name == before || name.isBlank()) name = if (next) asset?.name ?: setName else setName
                            if (category in categories.take(2)) category = categories[if (next && asset?.brushes == false) 1 else 0]
                        }
                    } else {
                        StoreFormRow("Kind") { Text(kind, color = ROW_INK, fontSize = 17.sp) }
                    }
                    if (useAsset) {
                        GroupRule()
                        StoreFormRow("Shared file", value = asset?.name ?: "Choose a file", chevron = true,
                            enabled = !busy, onClick = { chooseAsset.launch(arrayOf("*/*")) })
                    }
                    GroupRule()
                    PackageChoice("Category", category, categories, !busy) { category = it }
                    GroupRule()
                    PackageChoice("Licence", licence, licences, !busy) { licence = it }
                }
            }
            Column {
                GroupHead("DESCRIPTION")
                StoreFormGroup {
                    PackageField("Description", description, { description = it }, !busy,
                        Modifier.fillMaxWidth().padding(16.dp), multiline = true)
                }
            }
            Column {
                GroupHead("TAGS")
                StoreFormGroup {
                    PackageField("Tags, separated by commas", tags, { tags = it }, !busy, Modifier.fillMaxWidth().padding(16.dp))
                }
                GroupNote("Add a name, description, and at least one tag to export.", last = true)
            }
        }
    }
}

@Composable
internal fun StoreDialog(
    title: String,
    width: Dp,
    height: Dp? = null,
    canClose: Boolean = true,
    onClose: () -> Unit,
    actionLabel: String? = null,
    actionEnabled: Boolean = false,
    onAction: () -> Unit = {},
    content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit,
) {
    Dialog(
        onDismissRequest = { if (canClose) onClose() },
        properties = DialogProperties(dismissOnBackPress = canClose, dismissOnClickOutside = canClose, usePlatformDefaultWidth = false),
    ) {
        val bounds = if (height == null) Modifier else Modifier.heightIn(max = height).fillMaxHeight()
        Surface(Modifier.padding(horizontal = 16.dp, vertical = 18.dp).widthIn(max = width).fillMaxWidth().then(bounds),
            shape = RoundedCornerShape(14.dp), color = BG, contentColor = TITLE, shadowElevation = 20.dp) {
            Column {
                Box(Modifier.fillMaxWidth().height(56.dp)) {
                    Text(title, color = TITLE, fontSize = 17.sp, fontWeight = FontWeight.SemiBold,
                        modifier = Modifier.align(Alignment.Center))
                    Box(Modifier.align(Alignment.CenterStart)) { StoreTextAction("Cancel", canClose, onClose) }
                    if (actionLabel != null) {
                        Box(Modifier.align(Alignment.CenterEnd)) { StoreTextAction(actionLabel, actionEnabled, onAction) }
                    }
                }
                Box(Modifier.fillMaxWidth().height(1.dp).background(SEP))
                content()
            }
        }
    }
}

@Composable
private fun StoreTextAction(label: String, enabled: Boolean, onClick: () -> Unit) {
    Box(Modifier.height(48.dp).clickable(enabled = enabled, role = Role.Button, onClick = onClick)
        .padding(horizontal = 16.dp), contentAlignment = Alignment.Center) {
        Text(label, color = if (enabled) ACCENT else IDLE, fontSize = 17.sp, fontWeight = FontWeight.Medium)
    }
}

@Composable
internal fun StoreFormGroup(content: @Composable androidx.compose.foundation.layout.ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(GROUP_CORNER)).background(GROUP_FILL), content = content)
}

@Composable
private fun ArtworkPreview(label: String, picture: Bitmap?, loading: Boolean, modifier: Modifier, ratio: Float) {
    Column(modifier) {
        Box(Modifier.fillMaxWidth().aspectRatio(ratio).clip(RoundedCornerShape(GROUP_CORNER)).background(TILE_FILL),
            contentAlignment = Alignment.Center) {
            if (picture != null) Image(picture.asImageBitmap(), label, Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
            else if (loading) CircularProgressIndicator(Modifier.size(24.dp), color = ACCENT)
            else Text(label, color = ROW_INK, fontSize = 13.sp)
        }
        Text(label, color = ROW_INK, fontSize = 13.sp, modifier = Modifier.padding(top = 8.dp))
    }
}

@Composable
private fun GalleryTile(picture: Bitmap, label: String, enabled: Boolean, onRemove: () -> Unit) {
    Box(Modifier.size(108.dp, 81.dp).clip(RoundedCornerShape(GROUP_CORNER)).background(TILE_FILL)) {
        Image(picture.asImageBitmap(), label, Modifier.fillMaxSize(), contentScale = ContentScale.Fit)
        Box(Modifier.align(Alignment.TopEnd).size(36.dp).background(Color.Black.copy(alpha = 0.65f))
            .semantics { contentDescription = "Remove " + label }
            .clickable(enabled = enabled, role = Role.Button, onClick = onRemove), contentAlignment = Alignment.Center) {
            Text("×", color = Color.White, fontSize = 23.sp)
        }
    }
}

@Composable
private fun ArtworkSourceRow(label: String, target: ArtworkTarget, enabled: Boolean,
    photos: (ArtworkTarget) -> Unit, files: (ArtworkTarget) -> Unit, take: (ArtworkTarget, Boolean) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        StoreFormRow(label, chevron = true, enabled = enabled, onClick = { open = true })
        Box(Modifier.align(Alignment.BottomEnd).widthIn(max = 244.dp).fillMaxWidth()) {
            DropdownMenu(expanded = open && enabled, onDismissRequest = { open = false }, containerColor = BG) {
                for ((source, action) in listOf<Pair<String, () -> Unit>>(
                    "Photo Library" to { photos(target) },
                    "Files" to { files(target) },
                    "Clipboard" to { take(target, false) },
                    "Current Project" to { take(target, true) },
                )) {
                    DropdownMenuItem(text = { Text(source, fontSize = 17.sp, color = TITLE) }, onClick = {
                        open = false
                        action()
                    })
                }
            }
        }
    }
}

@Composable
internal fun StoreFormRow(
    label: String,
    subtitle: String? = null,
    value: String? = null,
    chevron: Boolean = false,
    enabled: Boolean = true,
    labelColor: Color = TITLE,
    onClick: (() -> Unit)? = null,
    trailing: (@Composable () -> Unit)? = null,
) {
    val action = if (onClick == null) Modifier else Modifier.clickable(enabled = enabled, role = Role.Button, onClick = onClick)
    Row(Modifier.fillMaxWidth().then(action).heightIn(min = GROUP_ROW).padding(horizontal = GROUP_RULE_INSET),
        horizontalArrangement = Arrangement.spacedBy(8.dp), verticalAlignment = Alignment.CenterVertically) {
        Column(Modifier.weight(1f)) {
            Text(label, color = if (enabled) labelColor else IDLE, fontSize = 17.sp)
            if (subtitle != null) Text(subtitle, color = ROW_INK, fontSize = 15.sp, maxLines = 1, overflow = TextOverflow.Ellipsis)
        }
        if (value != null) {
            Text(value, color = ROW_INK, fontSize = 17.sp, maxLines = 1, overflow = TextOverflow.Ellipsis,
                textAlign = TextAlign.End, modifier = Modifier.weight(1f))
        }
        if (chevron) Icon(PcIcons.ChevronRight, null, Modifier.size(13.dp), tint = IDLE)
        trailing?.invoke()
    }
}

@Composable
private fun PackageField(
    label: String,
    value: String,
    update: (String) -> Unit,
    enabled: Boolean,
    modifier: Modifier = Modifier.widthIn(max = 260.dp),
    multiline: Boolean = false,
) {
    BasicTextField(value, update, modifier.semantics { contentDescription = label }, enabled = enabled, singleLine = !multiline,
        minLines = if (multiline) 3 else 1, maxLines = if (multiline) 8 else 1,
        textStyle = TextStyle(color = TITLE, fontSize = 17.sp),
        cursorBrush = SolidColor(ACCENT),
        decorationBox = { enter ->
            Box {
                if (value.isEmpty()) Text(label, color = IDLE, fontSize = 15.sp)
                enter()
            }
        })
}

@Composable
private fun PackageChoice(label: String, selected: String, choices: List<String>, enabled: Boolean, change: (String) -> Unit) {
    var open by remember { mutableStateOf(false) }
    Box {
        StoreFormRow(label, value = selected, chevron = true, enabled = enabled, onClick = { open = true })
        Box(Modifier.align(Alignment.BottomEnd).widthIn(max = 244.dp).fillMaxWidth()) {
            DropdownMenu(expanded = open && enabled, onDismissRequest = { open = false },
                modifier = Modifier.heightIn(max = 360.dp), containerColor = BG) {
                for (choice in choices) {
                    DropdownMenuItem(text = { Text(choice, color = if (choice == selected) ACCENT else TITLE) }, onClick = {
                        open = false
                        if (choice != selected) change(choice)
                    })
                }
            }
        }
    }
}

private fun readAsset(context: Context, uri: Uri, scratch: File): ChosenAsset? = runCatching {
    val resolver = context.contentResolver
    val displayName = resolver.query(uri, arrayOf(OpenableColumns.DISPLAY_NAME), null, null, null)
        ?.use { if (it.moveToFirst()) it.getString(0) else null }
    val extension = displayName?.substringAfterLast('.')?.lowercase()?.takeIf { it in listOf("asset", "assetset", "assetlibrary", "brush", "brushset", "brushlibrary") } ?: return null
    scratch.mkdirs()
    val file = File.createTempFile("asset-", "." + extension, scratch)
    try {
        requireNotNull(resolver.openInputStream(uri)).use { input -> file.outputStream().use { input.copyTo(it) } }
        val info = storeSharedFile(file.path, displayName)
        if (info != null) ChosenAsset(file, info.name, info.brushes) else null.also { file.delete() }
    } catch (error: Exception) {
        file.delete()
        throw error
    }
}.onFailure { AppLog.e("store", "reading an asset pack", it) }.getOrNull()

private fun readMakerImage(resolver: ContentResolver, uri: Uri, scratch: File, square: Boolean): MakerImage? = runCatching {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, bounds) }
    if (bounds.outWidth <= 0 || bounds.outHeight <= 0) return null
    val options = BitmapFactory.Options().apply { inSampleSize = 1 }
    val edge = maxOf(bounds.outWidth, bounds.outHeight)
    while (edge / options.inSampleSize > 1600) options.inSampleSize *= 2
    val decoded = resolver.openInputStream(uri)?.use { BitmapFactory.decodeStream(it, null, options) } ?: return null
    storeMakerImage(scratch, square) { file ->
        file.outputStream().use { decoded.compress(Bitmap.CompressFormat.PNG, 100, it) }
    }
}.onFailure { AppLog.e("store", "reading an artwork image", it) }.getOrNull()

private fun readCanvasImage(pixels: ThumbnailImage, scratch: File, square: Boolean): MakerImage? =
    storeMakerImage(scratch, square) { writeCopyPng(pixels, byteArrayOf(), it.path) }

private fun storeMakerImage(scratch: File, square: Boolean, write: (File) -> Boolean): MakerImage? = runCatching {
    scratch.mkdirs()
    val file = File.createTempFile("artwork-", ".png", scratch)
    var result: MakerImage? = null
    try {
        if (!write(file)) return null
        if (square) {
            val png = storeSquarePng(file.readBytes())
            if (png.isEmpty()) return null
            file.writeBytes(png)
        }
        val picture = BitmapFactory.decodeFile(file.path) ?: return null
        result = MakerImage(file, picture)
        result
    } finally {
        if (result == null) file.delete()
    }
}.getOrNull()

/** Wraps the same files exported on desktop in one document for Android's share sheet. */
private fun exportArchive(
    context: Context,
    session: CanvasSession,
    library: UInt,
    set: UInt,
    asset: String?,
    details: StorePackageDetails,
    artwork: StorePackageImageChoices,
): File {
    val exports = File(context.cacheDir, "exports").apply { mkdirs() }
    val staged = Files.createTempDirectory(exports.toPath(), "store-").toFile()
    try {
        val packageDir = File(session.exportStorePackageArtwork(staged.path, library, set, asset, details, artwork))
        val output = File(staged, packageDir.name + ".zip")
        zipStorePackage(packageDir, output)
        packageDir.deleteRecursively()
        return output
    } catch (error: Exception) {
        staged.deleteRecursively()
        throw error
    }
}

internal fun zipStorePackage(packageDir: File, output: File) {
    val files = requireNotNull(packageDir.listFiles()).filter { it.isFile }.sortedBy { it.name }
    ZipOutputStream(output.outputStream().buffered()).use { zip ->
        for (file in files) {
            zip.putNextEntry(ZipEntry(file.name))
            file.inputStream().use { it.copyTo(zip) }
            zip.closeEntry()
        }
    }
}

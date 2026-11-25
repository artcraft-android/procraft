package io.github.artcraftandroid.procraft.ui

import android.content.ClipData
import android.content.ClipboardManager
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.verticalScroll
import androidx.compose.material3.Icon
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.saveable.rememberSaveable
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.graphics.ColorFilter
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.text.style.TextOverflow
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.core.content.FileProvider
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.BrushInfo
import java.io.File
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext

internal sealed class BrushShare(val title: String, val kind: String) {
    class Single(val brush: BrushInfo) : BrushShare(brush.name, "Brush")
    class Set(val index: Int, name: String, val icon: Int) : BrushShare(name, "Brush set")
    class Library(name: String) : BrushShare(name, "Brush library")

    suspend fun write(vm: CanvasViewModel): File? = when (this) {
        is Single -> vm.brushFileToShare(brush)
        is Set -> vm.brushFileToShare(null, index)
        is Library -> vm.libraryFileToShare()
    }
}

@Composable
internal fun BrushShareOverlay(vm: CanvasViewModel) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var pendingSavePath by rememberSaveable { mutableStateOf<String?>(null) }
    val save = rememberLauncherForActivityResult(ActivityResultContracts.CreateDocument("application/octet-stream")) { uri ->
        val file = pendingSavePath?.let(::File)
        pendingSavePath = null
        if (file != null) scope.launch {
            try {
                if (uri != null) withContext(Dispatchers.IO) {
                    val output = context.contentResolver.openOutputStream(uri, "w") ?: throw java.io.IOException()
                    output.use { out -> file.inputStream().use { it.copyTo(out) } }
                }
            } catch (_: java.io.IOException) {
                vm.showNotice("The shared file couldn't be saved.", warning = true)
            } catch (_: SecurityException) {
                vm.showNotice("The selected location couldn't be opened.", warning = true)
            } finally {
                file.delete()
            }
        }
    }
    val target = vm.brushShare ?: return
    val sample = when (target) {
        is BrushShare.Single -> target.brush
        is BrushShare.Set -> vm.brushSets.getOrNull(target.index)?.brushes?.firstOrNull()
        is BrushShare.Library -> null
    }
    LaunchedEffect(sample) { sample?.let { vm.loadPreview(it) } }
    val art = sample?.let { vm.previews[it.set.toString() + "-" + it.index] }
    val icon = when (target) {
        is BrushShare.Single -> PcIcons.Brush
        is BrushShare.Set -> PcIcons.SetIcons.getOrElse(target.icon) { PcIcons.BrushSet }
        is BrushShare.Library -> PcIcons.MenuLibraries
    }
    fun close() { vm.brushShare = null }
    fun write(then: (File) -> Unit) {
        close()
        scope.launch { target.write(vm)?.let(then) }
    }
    StoreDialog("Share", 400.dp, onClose = ::close) {
        Column(Modifier.verticalScroll(rememberScrollState()).padding(16.dp), verticalArrangement = Arrangement.spacedBy(16.dp)) {
            Row(horizontalArrangement = Arrangement.spacedBy(12.dp), verticalAlignment = Alignment.CenterVertically) {
                Box(Modifier.size(56.dp).clip(RoundedCornerShape(13.dp)).background(TILE_FILL), contentAlignment = Alignment.Center) {
                    if (art == null) Icon(icon, null, Modifier.size(28.dp), tint = IDLE)
                    else Image(art, null, Modifier.fillMaxSize().padding(5.dp), colorFilter = ColorFilter.tint(TITLE))
                }
                Column {
                    Text(target.title, color = TITLE, fontSize = 17.sp, fontWeight = FontWeight.SemiBold,
                        maxLines = 2, overflow = TextOverflow.Ellipsis)
                    Text(target.kind, color = ROW_INK, fontSize = 15.sp)
                }
            }
            StoreFormGroup {
                StoreFormRow("Store", "Package, artwork, and listing details", chevron = true, onClick = {
                    when (target) {
                        is BrushShare.Set -> {
                            close()
                            vm.storePackageSet = target.index
                        }
                        else -> write { vm.storePackageSharedFile = it }
                    }
                })
                GroupRule()
                StoreFormRow("Share File", chevron = true, onClick = {
                    write { shareFile(context, it, "application/octet-stream") }
                })
                GroupRule()
                StoreFormRow("Save to Files", chevron = true, onClick = {
                    write {
                        pendingSavePath = it.path
                        save.launch(it.name)
                    }
                })
                GroupRule()
                StoreFormRow("Copy File", chevron = true, onClick = {
                    write {
                        val uri = FileProvider.getUriForFile(context, context.packageName + ".files", it)
                        context.getSystemService(ClipboardManager::class.java)
                            .setPrimaryClip(ClipData.newUri(context.contentResolver, it.name, uri))
                    }
                })
            }
        }
    }
}

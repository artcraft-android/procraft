package io.github.artcraftandroid.procraft.ui

import android.content.Context
import android.graphics.Bitmap
import android.graphics.BitmapFactory
import androidx.compose.foundation.Image
import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
import androidx.compose.foundation.shape.CircleShape
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
import androidx.compose.ui.graphics.FilterQuality
import androidx.compose.ui.graphics.asImageBitmap
import androidx.compose.ui.layout.ContentScale
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.lifecycle.viewModelScope
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.canvas.SpriteSheetStart
import io.github.artcraftandroid.procraft.canvas.exportHarmonyScene
import io.github.artcraftandroid.procraft.canvas.exportSpriteSheet
import io.github.artcraftandroid.procraft.canvas.shareSpriteSheet
import io.github.artcraftandroid.procraft.core.SheetLayout
import io.github.artcraftandroid.procraft.core.SpriteSheetMade
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

private val SheetRowInk get() = if (Pc.light) Color(0xFF595959) else Color(0xFFC1C1C1)
private val SheetPanelEdge get() = if (Pc.light) Color(0xFFDADADA) else Color(0xFF363636)
private val SheetSegmentTrack get() = if (Pc.light) Color(0xFFD5D5D5) else Color(0xFF3C3C3C)

/** Web Ready's longest side for a frame, as the animated exports'. */
private const val SHEET_WEB_READY_SIDE = 480u

/** The most columns, rows or pixels of padding. */
private const val MOST_COUNT = 64u

/** How long the settings rest before the sheet is made again, ms. */
private const val SETTLE_MS = 250L

/** The preview's longest side at most: a sheet of many large frames is shown smaller. */
private const val PREVIEW_MOST = 4096

/** The layouts, in the order their segments show them. */
private val Layouts = listOf(SheetLayout.GRID, SheetLayout.ACROSS, SheetLayout.DOWN)

/** Opens Share's sprite sheet export over everything, the panel fading away. */
fun CanvasViewModel.openSpriteSheetExport() {
    fadePanels()
    spriteSheetExport = true
}

/**
 * Sharing the frames as a sprite sheet, as the iPad's: the sheet as the settings make it, the
 * settings (the size, the layout, padding, trimming, held frames, the JSON and the background),
 * Cancel, and the round Done that shares the sheet and its JSON. The sheet is made again a moment
 * after the settings stop changing.
 */
@Composable
fun SpriteSheetExport(vm: CanvasViewModel) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    var choices by remember { mutableStateOf(SpriteSheetStart) }
    var sheet by remember { mutableStateOf<Bitmap?>(null) }
    var made by remember { mutableStateOf<SpriteSheetMade?>(null) }
    var exporting by remember { mutableStateOf(false) }
    // the sheet as the settings make it, a moment after they stop changing
    LaunchedEffect(choices) {
        delay(SETTLE_MS)
        val (files, what) = vm.exportSpriteSheet(choices) ?: return@LaunchedEffect
        val picture = withContext(Dispatchers.IO) { files.firstOrNull()?.let(::previewPicture) } ?: return@LaunchedEffect
        sheet = picture
        made = what
    }
    ExportScreen(
        title = "Sprite Sheet",
        name = vm.artworkTitle(),
        size = made?.let(::said),
        exporting = exporting,
        onCancel = { vm.spriteSheetExport = false },
        onDone = {
            exporting = true
            scope.launch {
                // the export card shows how far it has come, and says if it couldn't be made
                val files = vm.shareSpriteSheet(choices)
                exporting = false
                if (files.isNotEmpty()) {
                    vm.spriteSheetExport = false
                    // the JSON beside the picture is no picture: saved as a plain file
                    runCatching { shareFiles(context, files, if (files.size > 1) "application/octet-stream" else "image/png") }
                }
            }
        },
        preview = { _ ->
            // the sheet, fitted to the box, its pixels kept sharp, its edge drawn
            sheet?.let {
                Image(
                    it.asImageBitmap(),
                    contentDescription = "Sprite sheet",
                    contentScale = ContentScale.Fit,
                    filterQuality = FilterQuality.None,
                    modifier = Modifier.aspectRatio(it.width.toFloat() / it.height.coerceAtLeast(1)).border(1.dp, SheetPanelEdge),
                )
            }
        },
    ) {
        Segments(listOf("Max Resolution", "Web Ready"), if (choices.maxSide == 0u) 0 else 1) {
            choices = choices.copy(maxSide = if (it == 1) SHEET_WEB_READY_SIDE else 0u)
        }
        Spacer(Modifier.height(12.dp))
        Segments(listOf("Grid", "Row", "Column"), Layouts.indexOf(choices.layout).coerceAtLeast(0)) { choices = choices.copy(layout = Layouts[it]) }
        Spacer(Modifier.height(6.dp))
        if (choices.layout == SheetLayout.GRID) {
            CountLine("Columns", choices.columns) { choices = choices.copy(columns = it) }
            CountLine("Rows", choices.rows) { choices = choices.copy(rows = it) }
        }
        CountLine("Padding", choices.padding, none = "None") { choices = choices.copy(padding = it) }
        SettingRow("Trim empty space", choices.trim) { choices = choices.copy(trim = it) }
        SettingRow("Held frames once", choices.heldOnce) { choices = choices.copy(heldOnce = it) }
        SettingRow("JSON data", choices.json) { choices = choices.copy(json = it) }
        SettingRow("Transparent background", choices.transparent) { choices = choices.copy(transparent = it) }
    }
}

/** Share > Harmony Scene: the animation as a Harmony scene folder, zipped, offered to share. */
internal fun shareHarmonyScene(vm: CanvasViewModel, context: Context) {
    // made outside the panel, which may be closed meanwhile; cancelled or failed (that is told), nothing is shared
    vm.viewModelScope.launch {
        vm.exportHarmonyScene()?.let { runCatching { shareFile(context, it, "application/zip") } }
    }
}

/** What the sheet is: its size and how many pictures are on it. */
private fun said(made: SpriteSheetMade): String {
    val pictures = if (made.cells == 1u) "1 frame" else "${made.cells} frames"
    return "${made.width} × ${made.height} px · $pictures"
}

/** The sheet's PNG at [file] as the preview shows it, read smaller to fit [PREVIEW_MOST]. */
private fun previewPicture(file: File): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(file.path, bounds)
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / sample > PREVIEW_MOST) sample *= 2
    return BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
}

/** A setting that counts, 52 tall: its name, its value (0 is "Auto", or [none]), and − and + in a grey track. */
@Composable
private fun CountLine(label: String, value: UInt, none: String = "Auto", onChange: (UInt) -> Unit) {
    Row(Modifier.fillMaxWidth().height(52.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = SheetRowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
        Text(if (value == 0u) none else "$value", color = SheetRowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(end = 12.dp))
        Row(Modifier.height(32.dp).clip(CircleShape).background(SheetSegmentTrack), verticalAlignment = Alignment.CenterVertically) {
            Step("−", enabled = value > 0u) { onChange(value - 1u) }
            Box(Modifier.width(1.dp).height(18.dp).background(SheetPanelEdge))
            Step("+", enabled = value < MOST_COUNT) { onChange(value + 1u) }
        }
    }
}

/** One of a count's steps, 44 × 32. */
@Composable
private fun Step(sign: String, enabled: Boolean, onClick: () -> Unit) {
    Box(Modifier.size(44.dp, 32.dp).clickable(enabled = enabled, onClick = onClick), contentAlignment = Alignment.Center) {
        Text(sign, color = if (enabled) SheetRowInk else SheetRowInk.copy(alpha = 0.35f), fontSize = 20.sp, fontWeight = FontWeight.Medium)
    }
}

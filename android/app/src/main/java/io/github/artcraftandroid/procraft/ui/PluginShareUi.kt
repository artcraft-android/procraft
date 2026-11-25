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
import io.github.artcraftandroid.procraft.canvas.exportPluginShare
import io.github.artcraftandroid.procraft.canvas.sharePluginExport
import io.github.artcraftandroid.procraft.core.PluginControl
import io.github.artcraftandroid.procraft.core.PluginShare
import io.github.artcraftandroid.procraft.core.PluginSlider
import io.github.artcraftandroid.procraft.core.pluginSliderChoice
import io.github.artcraftandroid.procraft.core.pluginSliderChosen
import io.github.artcraftandroid.procraft.core.pluginSliderShown
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch
import kotlinx.coroutines.withContext
import java.io.File

private val SheetRowInk get() = if (Pc.light) Color(0xFF595959) else Color(0xFFC1C1C1)
private val SheetPanelEdge get() = if (Pc.light) Color(0xFFDADADA) else Color(0xFF363636)
private val SheetSegmentTrack get() = if (Pc.light) Color(0xFFD5D5D5) else Color(0xFF3C3C3C)

/** How long the settings rest before what it makes is made again, ms. */
private const val SETTLE_MS = 250L

/** The preview's longest side at most: a sheet of many large frames is shown smaller. */
private const val PREVIEW_MOST = 4096

/**
 * Share's [share], made of the animation by a plugin (a sprite sheet, a Harmony scene): its screen
 * if it has settings, over everything, the panel fading away; otherwise made and shared at once.
 */
internal fun CanvasViewModel.openPluginShare(share: PluginShare, context: Context) {
    if (share.controls.isEmpty()) {
        // made outside the panel, which may be closed meanwhile; cancelled or failed (that is told), nothing is shared
        viewModelScope.launch {
            val files = sharePluginExport(share)
            if (files.isNotEmpty()) runCatching { shareFiles(context, files, mimeOf(files)) }
        }
        return
    }
    fadePanels()
    pluginShare = share
}

/** What [files] are shared as: one picture or zip as itself, anything else as plain files. */
private fun mimeOf(files: List<File>): String = when {
    files.size != 1 -> "application/octet-stream"
    files[0].extension.equals("png", ignoreCase = true) -> "image/png"
    files[0].extension.equals("zip", ignoreCase = true) -> "application/zip"
    else -> "application/octet-stream"
}

/** The settings of [share] as a key, so what it makes is made again as they change. */
private fun asked(share: PluginShare): String = share.controls.joinToString("-") {
    when (it) {
        is PluginControl.Switch -> "${it.control.on}"
        is PluginControl.Slider -> "${it.control.value}"
        else -> ""
    }
}

/**
 * A plugin's export with settings (a sprite sheet), as the iPad's (AnimatedExport.swift
 * `PluginShareScreen`): what it makes as they stand, the settings as its plugin lists them (a
 * slider with names as choices side by side, another as a count with − and +, a switch as a
 * switch), Cancel, and the round Done that shares what it makes. What it makes is made again a
 * moment after the settings stop changing. The settings are the plugin's own, kept with it as on
 * its card.
 */
@Composable
fun PluginShareExport(vm: CanvasViewModel, share: PluginShare) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    // the export as its controls are set now (read again as the plugins change)
    val current = vm.pluginShares.firstOrNull { it.key == share.key } ?: share
    var picture by remember { mutableStateOf<Bitmap?>(null) }
    var said by remember { mutableStateOf<String?>(null) }
    var exporting by remember { mutableStateOf(false) }
    // what it makes as the settings stand, a moment after they stop changing
    LaunchedEffect(asked(current)) {
        delay(SETTLE_MS)
        val (files, what) = vm.exportPluginShare(current) ?: return@LaunchedEffect
        val png = files.firstOrNull { it.extension.equals("png", ignoreCase = true) }
        val shown = withContext(Dispatchers.IO) {
            try {
                png?.let(::previewPicture)
            } finally {
                files.firstOrNull()?.parentFile?.deleteRecursively()
            }
        }
        picture = shown
        said = what
    }
    ExportScreen(
        title = current.label,
        name = vm.artworkTitle(),
        size = said,
        exporting = exporting,
        onCancel = { vm.pluginShare = null },
        onDone = {
            exporting = true
            scope.launch {
                // the export card shows how far it has come, and says if it couldn't be made
                val files = vm.sharePluginExport(current)
                exporting = false
                if (files.isNotEmpty()) {
                    vm.pluginShare = null
                    runCatching { shareFiles(context, files, mimeOf(files)) }
                }
            }
        },
        preview = { _ ->
            // what it makes, fitted to the box, its pixels kept sharp, its edge drawn
            picture?.let {
                Image(
                    it.asImageBitmap(),
                    contentDescription = current.label,
                    contentScale = ContentScale.Fit,
                    filterQuality = FilterQuality.None,
                    modifier = Modifier.aspectRatio(it.width.toFloat() / it.height.coerceAtLeast(1)).border(1.dp, SheetPanelEdge),
                )
            }
        },
    ) {
        for (control in current.controls) {
            when (control) {
                is PluginControl.Switch -> SettingRow(control.control.label, control.control.on) { vm.setPluginSwitch(control.control.key, it) }
                is PluginControl.Slider -> {
                    val one = control.control
                    if (one.names.isNotEmpty()) {
                        Text(
                            one.label,
                            color = SheetRowInk.copy(alpha = 0.7f),
                            fontSize = 15.sp,
                            modifier = Modifier.padding(start = 16.dp, top = 10.dp, bottom = 6.dp),
                        )
                        Segments(one.names, pluginSliderChosen(one, one.value).toInt()) { vm.setPluginSlider(one, pluginSliderChoice(one, it.toUInt())) }
                        Spacer(Modifier.height(6.dp))
                    } else {
                        CountLine(one) { vm.setPluginSlider(one, it) }
                    }
                }
                else -> {}
            }
        }
    }
}

/** The PNG at [file] as the preview shows it, read smaller to fit [PREVIEW_MOST]. */
private fun previewPicture(file: File): Bitmap? {
    val bounds = BitmapFactory.Options().apply { inJustDecodeBounds = true }
    BitmapFactory.decodeFile(file.path, bounds)
    var sample = 1
    while (maxOf(bounds.outWidth, bounds.outHeight) / sample > PREVIEW_MOST) sample *= 2
    return BitmapFactory.decodeFile(file.path, BitmapFactory.Options().apply { inSampleSize = sample })
}

/** A slider of a plugin's as a count, 52 tall: its name, its value as the plugin shows it, and − and + a step each in a grey track. */
@Composable
private fun CountLine(slider: PluginSlider, onChange: (Float) -> Unit) {
    Row(Modifier.fillMaxWidth().height(52.dp).padding(horizontal = 16.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(slider.label, color = SheetRowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.weight(1f))
        Text(pluginSliderShown(slider, slider.value), color = SheetRowInk, fontSize = 17.sp, maxLines = 1, modifier = Modifier.padding(end = 12.dp))
        Row(Modifier.height(32.dp).clip(CircleShape).background(SheetSegmentTrack), verticalAlignment = Alignment.CenterVertically) {
            Step("−", enabled = slider.value > slider.least) { onChange((slider.value - slider.step).coerceAtLeast(slider.least)) }
            Box(Modifier.width(1.dp).height(18.dp).background(SheetPanelEdge))
            Step("+", enabled = slider.value < slider.most) { onChange((slider.value + slider.step).coerceAtMost(slider.most)) }
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

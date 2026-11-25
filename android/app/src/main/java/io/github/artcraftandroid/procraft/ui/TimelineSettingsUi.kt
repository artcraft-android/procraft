package io.github.artcraftandroid.procraft.ui

import androidx.compose.foundation.background
import androidx.compose.foundation.border
import androidx.compose.foundation.clickable
import androidx.compose.foundation.gestures.awaitEachGesture
import androidx.compose.foundation.gestures.awaitFirstDown
import androidx.compose.foundation.gestures.detectHorizontalDragGestures
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Box
import androidx.compose.foundation.layout.BoxScope
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.ColumnScope
import androidx.compose.foundation.layout.Row
import androidx.compose.foundation.layout.RowScope
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.layout.size
import androidx.compose.foundation.layout.width
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
import androidx.compose.runtime.rememberUpdatedState
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.draw.drawBehind
import androidx.compose.ui.geometry.CornerRadius
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.text.font.FontWeight
import androidx.compose.ui.unit.Dp
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import io.github.artcraftandroid.procraft.canvas.CanvasViewModel
import io.github.artcraftandroid.procraft.core.AnimationPlayback
import kotlin.math.roundToInt

// The timeline's settings, as the iPad's card (ios/ProCraft/TimelineGridSettings.swift): a card with
// a tab along its bottom for each part (the project's frame rate, length and size; the onion skins;
// how it plays; the app's own timeline settings), what a row opens standing beside it. Opened by
// vm.openTimelineSettings() (Actions → Canvas → Timeline Settings) or vm.toggleTimelineSettings()
// (the grid's header button); it stands vm.timelineSettingsLift over the screen's bottom right, and a
// press anywhere else closes it.

private val CardWidth = 318.dp
private val BesideWidth = 236.dp
private val Rows = 322.dp
private val RowHeight = 44.dp
private val CardFill get() = if (Pc.light) Color(0xFFEDEDED) else Color(0xFF2C2C2C)
private val CardEdge get() = if (Pc.light) Color(0xFFD6D6D6) else Color(0xFF3C3C3C)
private val SectionFill get() = if (Pc.light) Color(0xFFE1E1E1) else Color(0x12FFFFFF)
private val InkDim get() = if (Pc.light) Color(0xFF7A7A7A) else Color(0xFF8E8E8E)
private val KeyFill get() = if (Pc.light) Color(0xFFD6D6D6) else Color(0x1AFFFFFF)
private val LitRow get() = if (Pc.light) Color(0xFFD2D2D2) else Color(0x1FFFFFFF)
private val LineFill get() = if (Pc.light) Color(0xFFD0D0D0) else Color(0x14FFFFFF)

/** The frame rates offered by name. */
private val FrameRates = listOf("Frame by frame" to 12, "Anime" to 15, "Cinema" to 24, "Television" to 30, "PAL" to 50, "NTSC" to 60, "Mobile" to 120)

/** The onion skins' tints (none: they show in their own colours); a side's as first turned on. */
private val OnionColors = listOf("Red" to 0xE63333, "Orange" to 0xE68A1A, "Green" to 0x339933, "Blue" to 0x3366E6, "Purple" to 0x9933CC)
private const val BEFORE_TINT = 0xE63333
private const val AFTER_TINT = 0x339933

private enum class SettingsTab(val label: String, val icon: ImageVector) {
    PROJECT("Project", PcIcons.InfoDimensions),
    ONION("Onion", PcIcons.InfoLayers),
    TIMELINE("Timeline", PcIcons.InfoVideo),
    PREFS("Prefs", PcIcons.TabPrefs),
}

/** What stands beside the card: the frame rates by name, the frame rate's keys, the length's. */
private enum class Beside { RATES, RATE, LENGTH }

/** A length of [frames] at [fps] as minutes and seconds. */
fun timelineClock(frames: Int, fps: Int): String {
    val seconds = frames / fps.coerceAtLeast(1)
    return "%02d:%02d".format(seconds / 60, seconds % 60)
}

/** Frame [frame] at [fps] as a timecode: hours, minutes, seconds, and the frame within the second. */
fun timelineTimecode(frame: Int, fps: Int): String {
    val f = fps.coerceAtLeast(1)
    val seconds = frame / f
    return "%02d:%02d:%02d.%03d".format(seconds / 3600, seconds / 60 % 60, seconds % 60, frame % f)
}

/** The timeline's settings card, standing on the screen's bottom right. */
@Composable
fun BoxScope.TimelineSettingsCard(vm: CanvasViewModel) {
    val info = vm.timelineSettingsInfo ?: return
    var tab by remember { mutableStateOf(SettingsTab.PROJECT) }
    var beside by remember { mutableStateOf<Beside?>(null) }
    // a press anywhere else closes it
    Box(Modifier.fillMaxSize().pointerInput(Unit) { awaitEachGesture { awaitFirstDown().consume(); vm.closeTimelineSettings() } })
    Row(
        Modifier.align(Alignment.BottomEnd).padding(end = 16.dp, bottom = 16.dp + vm.timelineSettingsLift),
        verticalAlignment = Alignment.Bottom,
        horizontalArrangement = Arrangement.spacedBy(6.dp),
    ) {
        when (beside) {
            Beside.RATES -> Card(BesideWidth) { Rates(vm) { beside = Beside.RATE } }
            Beside.RATE -> Card(BesideWidth) {
                Keypad("FPS", info.fps.toInt(), CanvasViewModel.TIMELINE_MOST_FPS, timed = null) {
                    vm.setTimelineRate(it)
                    beside = null
                }
            }
            Beside.LENGTH -> Card(BesideWidth) {
                Keypad("frames", info.duration.toInt(), CanvasViewModel.TIMELINE_MOST_LENGTH, timed = info.fps.toInt()) {
                    vm.setTimelineLength(it)
                    beside = null
                }
                Box(Modifier.fillMaxWidth().height(1.dp).background(LineFill))
                MenuRow("Fit to content", height = 40.dp) {
                    vm.fitTimelineToContent()
                    beside = null
                }
            }
            null -> {}
        }
        Card(CardWidth) {
            Text("Timeline Settings", color = Pc.PanelTitle, fontSize = 22.sp, fontWeight = FontWeight.Bold, modifier = Modifier.padding(start = 10.dp, top = 6.dp, bottom = 12.dp))
            Column(Modifier.fillMaxWidth().height(Rows).verticalScroll(rememberScrollState()), verticalArrangement = Arrangement.spacedBy(8.dp)) {
                when (tab) {
                    SettingsTab.PROJECT -> ProjectTab(vm, beside) { beside = if (beside == it || (it == Beside.RATES && beside == Beside.RATE)) null else it }
                    SettingsTab.ONION -> OnionTab(vm)
                    SettingsTab.TIMELINE -> TimelineTab(vm)
                    SettingsTab.PREFS -> PrefsTab(vm)
                }
            }
            Row(Modifier.fillMaxWidth().padding(top = 6.dp)) {
                for (t in SettingsTab.entries) {
                    val ink = if (t == tab) Pc.Accent else Pc.PanelTitle
                    Column(
                        Modifier.weight(1f).height(52.dp).clickable {
                            tab = t
                            beside = null
                        },
                        horizontalAlignment = Alignment.CenterHorizontally,
                        verticalArrangement = Arrangement.Center,
                    ) {
                        Icon(t.icon, contentDescription = null, tint = ink, modifier = Modifier.size(30.dp))
                        Text(t.label, color = ink, fontSize = 11.5.sp)
                    }
                }
            }
        }
    }
}

/** A card standing on the screen: its presses go no further. */
@Composable
private fun Card(width: Dp, content: @Composable ColumnScope.() -> Unit) {
    Column(
        Modifier.width(width).clip(RoundedCornerShape(22.dp)).background(CardFill).border(1.dp, CardEdge, RoundedCornerShape(22.dp))
            .pointerInput(Unit) { detectTapGestures { } }.padding(8.dp),
        content = content,
    )
}

/** A group of the settings' rows on one rounded ground. */
@Composable
private fun Section(content: @Composable ColumnScope.() -> Unit) {
    Column(Modifier.fillMaxWidth().clip(RoundedCornerShape(14.dp)).background(SectionFill), content = content)
}

@Composable
private fun ColumnScope.ProjectTab(vm: CanvasViewModel, beside: Beside?, open: (Beside) -> Unit) {
    val info = vm.timelineSettingsInfo ?: return
    val fps = info.fps.toInt()
    // the frame rate as its slider is dragged with the timing kept, before it is let go
    var rate by remember { mutableStateOf<Int?>(null) }
    val most = CanvasViewModel.TIMELINE_MOST_FPS
    Section {
        ValueRow("Frames per second", "${rate ?: fps}", lit = beside == Beside.RATES || beside == Beside.RATE) { open(Beside.RATES) }
        // (dragged for any rate; the row above for the named ones)
        Slider(null, null, ((rate ?: fps) - 1) / (most - 1).toFloat(), height = 36.dp, onEnd = {
            rate?.let { vm.setTimelineRate(it) }
            rate = null
        }) { v ->
            val to = 1 + (v * (most - 1)).roundToInt()
            // (the timing is kept by moving every frame: once, when it is let go)
            if (vm.timelineKeepTiming) rate = to else if (to != fps) vm.changeTimeline { it.copy(fps = to.toUInt()) }
        }
        // under a second, its frames say more
        val duration = info.duration.toInt()
        val length = if (duration < fps) "$duration frames" else timelineClock(duration, fps)
        ValueRow("Duration", length, lit = beside == Beside.LENGTH) { open(Beside.LENGTH) }
    }
    Section { ToggleRow("Keep timing when FPS changes", vm.timelineKeepTiming) { vm.changeTimelineKeepTiming(!vm.timelineKeepTiming) } }
    // (changing the canvas's size is Crop & Resize's work)
    val (w, h) = vm.canvasSize()
    Section { ValueRow("Resolution", "$w × $h", lit = false) { vm.timelineResolution() } }
}

@Composable
private fun ColumnScope.OnionTab(vm: CanvasViewModel) {
    val info = vm.timelineSettingsInfo ?: return
    val o = vm.timelineSettingsOnion ?: return
    // the onion skins shown in its sliders: those before the drawing, or those after
    var after by remember { mutableStateOf(false) }
    val count = (if (after) o.after else o.before).toInt()
    val colour = (if (after) o.afterColor else o.beforeColor).toInt()
    Section { ToggleRow("Onion skin visibility", o.before > 0u || o.after > 0u) { vm.toggleTimelineOnion() } }
    Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(4.dp)) {
        Side("Backwards", !after) { after = false }
        Side("Forwards", after) { after = true }
    }
    Section {
        Spacer(Modifier.height(4.dp))
        val most = CanvasViewModel.TIMELINE_MOST_SKINS
        Slider("Frames", "$count", count / most.toFloat()) { v ->
            val n = (v * most).roundToInt().toUInt()
            vm.changeTimelineOnion { if (after) it.copy(after = n) else it.copy(before = n) }
        }
        Slider("Opacity", "${(info.onionOpacity * 100).roundToInt()}%", info.onionOpacity) { v -> vm.changeTimeline { it.copy(onionOpacity = v) } }
        Spacer(Modifier.height(6.dp))
    }
    val tint = { rgb: Int -> vm.changeTimelineOnion { if (after) it.copy(afterColor = rgb.toUInt()) else it.copy(beforeColor = rgb.toUInt()) } }
    Section {
        // tinted off, the skins show in their own colours
        ToggleRow("Color tint", colour != 0) { tint(if (colour != 0) 0 else if (after) AFTER_TINT else BEFORE_TINT) }
        if (colour != 0) {
            Row(Modifier.fillMaxWidth().padding(horizontal = 8.dp)) {
                for ((_, rgb) in OnionColors) {
                    Box(Modifier.weight(1f).height(40.dp).clickable { tint(rgb) }, contentAlignment = Alignment.Center) {
                        Box(Modifier.size(22.dp).clip(CircleShape).background(Color(0xFF000000.toInt() or rgb)))
                        if (colour == rgb) Box(Modifier.size(30.dp).border(2.dp, Pc.PanelTitle, CircleShape))
                    }
                }
            }
        }
    }
}

@Composable
private fun RowScope.Side(label: String, chosen: Boolean, choose: () -> Unit) {
    Box(
        Modifier.weight(1f).height(34.dp).clip(CircleShape).background(if (chosen) Pc.Accent else SectionFill).clickable(onClick = choose),
        contentAlignment = Alignment.Center,
    ) {
        Text(label, color = if (chosen) Color.White else Pc.PanelTitle, fontSize = 14.sp, fontWeight = if (chosen) FontWeight.SemiBold else FontWeight.Normal)
    }
}

/** The Timeline tab: how it plays, frames per new drawing, Perform smoothing. */
@Composable
private fun ColumnScope.TimelineTab(vm: CanvasViewModel) {
    val info = vm.timelineSettingsInfo ?: return
    Section {
        for ((way, label) in listOf(AnimationPlayback.LOOP to "Loop", AnimationPlayback.PING_PONG to "Ping Pong", AnimationPlayback.ONE_SHOT to "One Shot")) {
            val chosen = info.playback == way
            Row(
                Modifier.fillMaxWidth().height(RowHeight).clickable { vm.changeTimeline { it.copy(playback = way) } }.padding(horizontal = 14.dp),
                verticalAlignment = Alignment.CenterVertically,
            ) {
                Text(label, color = Pc.PanelTitle, fontSize = 16.sp, modifier = Modifier.weight(1f))
                Box(Modifier.size(20.dp).border(1.5.dp, if (chosen) Pc.Accent else InkDim, CircleShape), contentAlignment = Alignment.Center) {
                    if (chosen) Box(Modifier.size(12.dp).clip(CircleShape).background(Pc.Accent))
                }
            }
        }
    }
    Section {
        Spacer(Modifier.height(4.dp))
        val most = CanvasViewModel.TIMELINE_MOST_HELD
        val held = info.exposure.toInt().coerceIn(1, most)
        Slider("Frames per new drawing", "$held", (held - 1) / (most - 1).toFloat()) { v ->
            val to = 1 + (v * (most - 1)).roundToInt()
            if (to != held) vm.changeTimeline { it.copy(exposure = to.toUInt()) }
        }
        Spacer(Modifier.height(6.dp))
    }
    Section {
        // (none: a performed move is kept as it was made, corners and all)
        Spacer(Modifier.height(4.dp))
        val smoothing = vm.timelinePerformSmoothing
        Slider("Perform smoothing", "${(smoothing * 100).roundToInt()}%", smoothing, onEnd = vm::keepTimelinePerformSmoothing) { vm.changeTimelinePerformSmoothing(it) }
        Spacer(Modifier.height(6.dp))
    }
}

@Composable
private fun ColumnScope.PrefsTab(vm: CanvasViewModel) {
    Section { ToggleRow("Timecode", vm.timelineTimecode) { vm.changeTimelineTimecode(!vm.timelineTimecode) } }
    // (off, moving the playhead by hand is silent; playing still sounds)
    Section { ToggleRow("Sound while scrubbing", vm.timelineScrubSound) { vm.changeTimelineScrubSound(!vm.timelineScrubSound) } }
    Section { ToggleRow("New frames are vector layers", vm.timelineVectorFrames) { vm.changeTimelineVectorFrames(!vm.timelineVectorFrames) } }
}

/** The frame rates by name, and Custom, which opens the keys to type one in. */
@Composable
private fun ColumnScope.Rates(vm: CanvasViewModel, custom: () -> Unit) {
    val fps = vm.timelineSettingsInfo?.fps?.toInt() ?: return
    MenuRow("Custom", fps = fps, opens = true, height = 40.dp, onClick = custom)
    Box(Modifier.fillMaxWidth().height(1.dp).background(LineFill))
    for ((name, rate) in FrameRates) {
        Box(Modifier.fillMaxWidth().clip(RoundedCornerShape(12.dp)).background(if (rate == fps) LitRow else Color.Transparent)) {
            MenuRow(name, fps = rate, height = 36.dp) { vm.setTimelineRate(rate) }
        }
    }
}

/** A row of a card beside the settings: its name, a frame rate with "FPS" in a little box after it, and a chevron for a row opening more. */
@Composable
private fun MenuRow(label: String, fps: Int? = null, opens: Boolean = false, height: Dp, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(height).clickable(onClick = onClick).padding(horizontal = 10.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Pc.PanelTitle, fontSize = 15.sp, modifier = Modifier.weight(1f), maxLines = 1)
        if (fps != null) {
            Text("$fps", color = Pc.PanelTitle, fontSize = 15.sp)
            Spacer(Modifier.width(8.dp))
            Box(Modifier.clip(RoundedCornerShape(3.5.dp)).background(Pc.PanelTitle).padding(horizontal = 3.5.dp, vertical = 2.dp)) {
                Text("FPS", color = CardFill, fontSize = 9.sp, fontWeight = FontWeight.Bold)
            }
        }
        if (opens) {
            Spacer(Modifier.width(6.dp))
            Icon(PcIcons.ChevronRight, contentDescription = null, tint = Pc.PanelTitle, modifier = Modifier.size(15.dp))
        }
    }
}

/**
 * A number typed on the card's own keys: what is typed so far over them ([from] until one is typed,
 * with its length in time at [timed] frames a second for a length), and Set to take it, from 1 to [most].
 */
@Composable
private fun Keypad(unit: String, from: Int, most: Int, timed: Int?, set: (Int) -> Unit) {
    var typed by remember { mutableStateOf("") }
    val number = typed.toIntOrNull()
    val valid = number != null && number in 1..most
    Column(Modifier.fillMaxWidth().padding(horizontal = 2.dp), verticalArrangement = Arrangement.spacedBy(6.dp)) {
        Row(Modifier.fillMaxWidth().height(34.dp).padding(horizontal = 6.dp), verticalAlignment = Alignment.CenterVertically) {
            Text(typed.ifEmpty { "$from" }, color = if (typed.isEmpty()) InkDim else Pc.PanelTitle, fontSize = 22.sp, fontWeight = FontWeight.SemiBold)
            Spacer(Modifier.width(5.dp))
            Text(unit, color = InkDim, fontSize = 13.sp, modifier = Modifier.weight(1f))
            if (timed != null) Text(timelineClock(number ?: from, timed), color = InkDim, fontSize = 13.sp)
        }
        for (row in listOf(listOf("7", "8", "9"), listOf("4", "5", "6"), listOf("1", "2", "3"), listOf("0", "⌫", "Set"))) {
            Row(Modifier.fillMaxWidth(), horizontalArrangement = Arrangement.spacedBy(6.dp)) {
                for (key in row) {
                    val isSet = key == "Set"
                    val fill = when {
                        isSet && valid -> Pc.Accent
                        isSet -> Pc.Accent.copy(alpha = 0.35f)
                        else -> KeyFill
                    }
                    Box(
                        Modifier.weight(1f).height(40.dp).clip(CircleShape).background(fill).clickable {
                            when (key) {
                                "Set" -> if (valid) number?.let(set)
                                "⌫" -> typed = typed.dropLast(1)
                                // no more than six, none starting with 0
                                else -> if (typed.length < 6 && !(typed.isEmpty() && key == "0")) typed += key
                            }
                        },
                        contentAlignment = Alignment.Center,
                    ) {
                        Text(key, color = if (isSet) Color.White else Pc.PanelTitle, fontSize = if (isSet) 16.sp else 19.sp, fontWeight = if (isSet) FontWeight.SemiBold else FontWeight.Normal)
                    }
                }
            }
        }
    }
}

/** A setting and what it is (in the accent while what it opens shows): tapped, something opens to change it. */
@Composable
private fun ValueRow(label: String, value: String, lit: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(RowHeight).clickable(onClick = onClick).padding(horizontal = 14.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Pc.PanelTitle, fontSize = 16.sp, modifier = Modifier.weight(1f), maxLines = 1)
        Text(value, color = if (lit) Pc.Accent else Pc.PanelTitle, fontSize = 16.sp, maxLines = 1)
    }
}

/** A setting that is on or off, with its switch. */
@Composable
private fun ToggleRow(label: String, on: Boolean, onClick: () -> Unit) {
    Row(Modifier.fillMaxWidth().height(RowHeight).clickable(onClick = onClick).padding(start = 14.dp, end = 8.dp), verticalAlignment = Alignment.CenterVertically) {
        Text(label, color = Pc.PanelTitle, fontSize = 16.sp, modifier = Modifier.weight(1f), maxLines = 1)
        Switch(on, enabled = true)
    }
}

/**
 * A slider: a setting's name and value over its bar ([label] given; 55 tall), or the bar alone in
 * [height]; blue up to a small white knob, dragged or tapped along.
 */
@Composable
private fun Slider(label: String?, value: String?, fraction: Float, height: Dp = 55.dp, onEnd: () -> Unit = {}, onChange: (Float) -> Unit) {
    val change by rememberUpdatedState(onChange)
    val end by rememberUpdatedState(onEnd)
    Column(Modifier.fillMaxWidth().height(height).padding(horizontal = 16.dp), verticalArrangement = if (label == null) Arrangement.Center else Arrangement.Top) {
        if (label != null) {
            Row(Modifier.fillMaxWidth().padding(top = 6.5.dp)) {
                Text(label, color = Pc.SlideLabel, fontSize = 16.sp, modifier = Modifier.weight(1f), maxLines = 1)
                Text(value.orEmpty(), color = Pc.SlideValue, fontSize = 16.sp, maxLines = 1)
            }
        }
        Box(
            Modifier.fillMaxWidth().height(22.dp)
                .pointerInput(Unit) {
                    detectTapGestures {
                        change((it.x / size.width).coerceIn(0f, 1f))
                        end()
                    }
                }
                .pointerInput(Unit) {
                    detectHorizontalDragGestures(onDragStart = { change((it.x / size.width).coerceIn(0f, 1f)) }, onDragEnd = { end() }, onDragCancel = { end() }) { c, _ ->
                        c.consume()
                        change((c.position.x / size.width).coerceIn(0f, 1f))
                    }
                }
                .drawBehind {
                    val y = size.height / 2f
                    val h = 3.dp.toPx()
                    val x = fraction.coerceIn(0f, 1f) * size.width
                    drawRect(Pc.SlideTrack, Offset(0f, y - h / 2f), Size(size.width, h))
                    drawRect(Pc.Accent, Offset(0f, y - h / 2f), Size(x, h))
                    val knob = Size(16.dp.toPx(), 10.dp.toPx())
                    drawRoundRect(Color.White, Offset((x - knob.width / 2f).coerceIn(0f, size.width - knob.width), y - knob.height / 2f), knob, CornerRadius(knob.height / 2f))
                },
        )
    }
}

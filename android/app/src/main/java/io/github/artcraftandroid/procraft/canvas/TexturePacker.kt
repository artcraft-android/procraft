package io.github.artcraftandroid.procraft.canvas

import android.graphics.BitmapFactory
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.mutableStateMapOf
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.ImageBitmap
import androidx.compose.ui.graphics.asImageBitmap
import io.github.artcraftandroid.procraft.core.PackerBox
import io.github.artcraftandroid.procraft.core.PackerSprite

/**
 * The Texture Packer (Share > Texture Packer), as the iPad app's: the separate drawings on the
 * canvas as boxes over it, grey until tapped, blue while chosen; how close parts must be to make
 * one sprite (Separation) and the clear pixels round each (Padding); the sprite tapped last shown
 * on a card to be renamed. Export shares each chosen sprite as a PNG of its own, in sRGB. Finding
 * and cutting the sprites is the core's (texture_packer.rs); its look is ui/TexturePackerUi.kt.
 */
class TexturePackerState {
    var boxes by mutableStateOf<List<PackerBox>>(emptyList())
    var separation by mutableIntStateOf(SEPARATION)
    var padding by mutableIntStateOf(PADDING)
    /** The boxes chosen, by their place in [boxes]. */
    var chosen by mutableStateOf<Set<Int>>(emptySet())
    /** The box tapped last, shown on the card. */
    var focused by mutableStateOf<Int?>(null)
    var thumbnail by mutableStateOf<ImageBitmap?>(null)
    /** Names given, by the box's place (so a name stays with a box the sliders leave as it was). */
    val names = mutableStateMapOf<String, String>()

    companion object {
        /** What it opens on: drawings 6 pixels apart are one sprite, none padded. */
        const val SEPARATION = 6
        const val PADDING = 0
        /** The card's picture is cut this many pixels across. */
        const val THUMBNAIL = 112

        fun key(b: PackerBox) = "${b.x},${b.y},${b.width},${b.height}"
    }
}

/** Share's Texture Packer: the canvas as it shows, without its background, its drawings found. */
fun CanvasViewModel.openTexturePacker() {
    fadePanels()
    val s = session ?: return
    val count = runCatching { s.texturePackerOpen() }.getOrNull()
    if (count == null || count == 0u) {
        s.texturePackerClose()
        showNotice("There are no separate drawings to pack.", warning = true)
        return
    }
    // the boxes are drawn and tapped through where the canvas is now
    refreshView()
    texturePacker = TexturePackerState().apply { boxes = s.texturePackerBoxes(separation.toUInt(), padding.toUInt()) }
}

fun CanvasViewModel.closeTexturePacker() {
    session?.texturePackerClose()
    texturePacker = null
}

/**
 * The sliders moved: the boxes made again. Separation changes which boxes there are, so what was
 * chosen goes; padding only grows them, so it stays.
 */
fun CanvasViewModel.packerResized(state: TexturePackerState, keepChosen: Boolean) {
    val s = session ?: return
    state.boxes = s.texturePackerBoxes(state.separation.toUInt(), state.padding.toUInt())
    if (!keepChosen) {
        state.chosen = emptySet()
        state.focused = null
    }
    packerFocus(state, state.focused)
}

/** The card shows sprite [index] (none: it goes). */
fun CanvasViewModel.packerFocus(state: TexturePackerState, index: Int?) {
    state.focused = index
    val b = index?.let { state.boxes.getOrNull(it) }
    val png = b?.let { session?.texturePackerThumbnail(it, TexturePackerState.THUMBNAIL.toUInt()) }
    state.thumbnail = png?.takeIf { it.isNotEmpty() }?.let { BitmapFactory.decodeByteArray(it, 0, it.size)?.asImageBitmap() }
}

/** Sprite [index]'s name: the one given, or the one it shares under until renamed. */
fun CanvasViewModel.packerName(state: TexturePackerState, index: Int): String =
    state.boxes.getOrNull(index)?.let { state.names[TexturePackerState.key(it)] }
        ?: session?.texturePackerDefaultName(index.toUInt()).orEmpty()

/** A tap at window pixel [at]: the smallest box under it chosen and shown on the card, or let go if it was chosen. */
fun CanvasViewModel.packerTapped(state: TexturePackerState, at: Offset) {
    val s = session ?: return
    val c = windowToCanvas(at) ?: return
    val hit = s.texturePackerHit(state.boxes, c.x, c.y)?.toInt() ?: return
    if (hit in state.chosen) {
        state.chosen = state.chosen - hit
        if (state.focused == hit) packerFocus(state, null)
    } else {
        state.chosen = state.chosen + hit
        packerFocus(state, hit)
    }
}

/** A step of the view moving: by [pan] and [zoom] about [pivot] (window pixels). */
fun CanvasViewModel.packerMoveView(pivot: Offset, pan: Offset, zoom: Float) {
    session?.viewGesture(pivot.x, pivot.y, pan.x, pan.y, zoom, 0f)
    refreshView()
}

/** The chosen sprites, in reading order, each with its name. */
fun CanvasViewModel.packedSprites(state: TexturePackerState): List<PackerSprite> =
    state.chosen.sorted().mapNotNull { i -> state.boxes.getOrNull(i)?.let { PackerSprite(it, i.toUInt(), packerName(state, i)) } }

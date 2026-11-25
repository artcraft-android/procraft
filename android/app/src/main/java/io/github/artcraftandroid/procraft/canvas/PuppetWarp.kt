package io.github.artcraftandroid.procraft.canvas

import android.content.SharedPreferences
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import io.github.artcraftandroid.procraft.core.PuppetSettings

/**
 * Transform's Puppet, as the iPad app's: how its mesh is laid (how far past what is painted it
 * reaches, how fine it is) and whether its pins turn as it bends, and whether its mesh is shown;
 * kept between transforms and launches. Its pins are put, chosen, moved and turned by the shared
 * touches on the canvas: a tap on its mesh puts a pin, on a pin chooses it (or, chosen, takes it
 * away), and a drag moves a pin or, on the chosen one's ring, turns it.
 */
class PuppetWarp internal constructor(private val prefs: SharedPreferences) {
    var settings by mutableStateOf(
        PuppetSettings(
            prefs.getFloat(EXPANSION, 4f),
            prefs.getInt(DENSITY, 40).toUInt(),
            prefs.getBoolean(AUTO_ROTATE, true),
        ),
    )
        private set

    /** The puppet's mesh is drawn over the canvas as it bends. */
    var showsMesh by mutableStateOf(prefs.getBoolean(SHOW_MESH, true))
        private set

    internal fun keep(next: PuppetSettings) {
        settings = next
        prefs.edit()
            .putFloat(EXPANSION, next.expansion)
            .putInt(DENSITY, next.density.toInt())
            .putBoolean(AUTO_ROTATE, next.autoRotate)
            .apply()
    }

    fun toggleMesh() {
        showsMesh = !showsMesh
        prefs.edit().putBoolean(SHOW_MESH, showsMesh).apply()
    }

    private companion object {
        const val EXPANSION = "puppetExpansion"
        const val DENSITY = "puppetDensity"
        const val AUTO_ROTATE = "puppetAutoRotate"
        const val SHOW_MESH = "puppetShowMesh"
    }
}

/** How a puppet's mesh is laid and its pins turn from now on (an open one's laid again). */
fun CanvasViewModel.changePuppet(next: PuppetSettings) {
    if (next.autoRotate != puppet.settings.autoRotate) showDone("Auto-rotate pins " + if (next.autoRotate) "on" else "off")
    puppet.keep(next)
    transformMoved(session?.transformPuppetSettings(next))
}

/** Takes the puppet's chosen pin away. */
fun CanvasViewModel.deletePuppetPin() {
    transformPopup = TransformPopup.NONE
    transformMoved(session?.transformPuppetDeletePin())
}

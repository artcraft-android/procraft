package io.github.artcraftandroid.procraft.canvas

import android.content.Context
import android.content.SharedPreferences
import io.github.artcraftandroid.procraft.core.Plugins

/** The plugins kept in the app's folder, those the app comes with added there first if it never added them, each value as kept in [prefs], the interface settings: a key nothing has is passed over. */
fun keptPlugins(context: Context, prefs: SharedPreferences): Plugins =
    Plugins.withBundled(java.io.File(context.filesDir, "plugins").path).also { plugins ->
        prefs.all.forEach { (key, value) -> plugins.readKept(key, value.toString()) }
        // Existing installations had the curves plugin bundled but off. Enable it only when
        // no explicit choice was saved; never override a user's previous on/off preference.
        plugins.plugins().firstOrNull { it.id == "io.github.artcraftandroid.quickshape-curves" }?.let { curves ->
            if (!prefs.contains(curves.onKey) && !curves.on) plugins.setOn(curves.id, true)
        }
    }

package io.github.artcraftandroid.procraft.canvas

import android.content.Context
import android.content.SharedPreferences
import androidx.compose.runtime.mutableStateListOf
import io.github.artcraftandroid.procraft.core.EasingKind
import io.github.artcraftandroid.procraft.core.EasingSpec
import org.json.JSONArray
import org.json.JSONObject

// "My curves", as the iPad app's (TimelineCurves.swift, SavedCurves): motion curves kept by name.
// One is an easing as the easing editor made it (its kind, that kind's numbers, a drawn curve's
// points, how often it repeats, looping or backwards), kept by the app in its preferences and so
// there in every artwork, to put on other keys, rename, duplicate, replace or delete.

/** A curve kept by [name]; [id] says which it is while the app runs (not kept). */
data class SavedCurve(val id: Long, val name: String, val spec: EasingSpec)

/** A kept curve as a key's Easing menu offers it: [chosen] when it is the one the key has. */
data class EasingChoice(val name: String, val spec: EasingSpec, val chosen: Boolean)

/** The curves kept, in the order they were made (a copy just after what it is of). */
class SavedCurves private constructor(private val prefs: SharedPreferences) {
    /** Read by what is drawn from them, so it is drawn again as they change. */
    val all = mutableStateListOf<SavedCurve>()
    private var nextId = 0L

    init {
        val kept = runCatching { JSONArray(prefs.getString(KEY, null) ?: "[]") }.getOrNull() ?: JSONArray()
        for (i in 0 until kept.length()) {
            read(kept.optJSONObject(i) ?: continue)?.let { all.add(it) }
        }
    }

    private fun made(name: String, spec: EasingSpec) = SavedCurve(nextId++, name, spec)

    private fun read(o: JSONObject): SavedCurve? {
        val kind = KINDS.firstOrNull { it.second == o.optString("kind") }?.first ?: return null
        val name = o.optString("name").trim().ifEmpty { return null }
        val numbers = o.optJSONArray("params") ?: JSONArray()
        val params = (0 until numbers.length()).map { numbers.optDouble(it, 0.0).toFloat() }
        val repeats = o.optInt("repeats", 1).coerceAtLeast(1).toUInt()
        return made(name, EasingSpec(kind, params, repeats, o.optBoolean("looping"), o.optBoolean("reversed")))
    }

    private fun keep() {
        val list = JSONArray()
        for (c in all) {
            list.put(
                JSONObject()
                    .put("name", c.name)
                    .put("kind", KINDS.firstOrNull { it.first == c.spec.kind }?.second ?: "linear")
                    .put("params", JSONArray().apply { c.spec.params.forEach { put(it.toDouble()) } })
                    .put("repeats", c.spec.repeats.toInt().coerceAtLeast(1))
                    .put("looping", c.spec.looping)
                    .put("reversed", c.spec.reversed),
            )
        }
        prefs.edit().putString(KEY, list.toString()).apply()
    }

    /** [base], or [base] with the first number after it that no curve has. */
    private fun free(base: String, numbered: Boolean): String {
        val taken = all.map { it.name }.toSet()
        if (!numbered && base !in taken) return base
        var n = if (numbered) 1 else 2
        while ("$base $n" in taken) n++
        return "$base $n"
    }

    /** The name a new curve is offered: "Curve 1" and so on. */
    val nextName: String get() = free("Curve", numbered = true)

    fun get(id: Long): SavedCurve? = all.firstOrNull { it.id == id }

    /** [spec] kept as a new curve named [name] (the next free name for one of only spaces); its id. */
    fun add(spec: EasingSpec, name: String): Long {
        val curve = made(name.trim().ifEmpty { nextName }, spec)
        all.add(curve)
        keep()
        return curve.id
    }

    /** Nothing for a name of only spaces. */
    fun rename(id: Long, name: String) {
        val i = all.indexOfFirst { it.id == id }
        if (i < 0 || name.isBlank()) return
        all[i] = all[i].copy(name = name.trim())
        keep()
    }

    fun duplicate(id: Long) {
        val i = all.indexOfFirst { it.id == id }
        if (i < 0) return
        all.add(i + 1, made(free("${all[i].name} copy", numbered = false), all[i].spec))
        keep()
    }

    /** Curve [id] becomes [spec], its name kept. */
    fun replace(id: Long, spec: EasingSpec) {
        val i = all.indexOfFirst { it.id == id }
        if (i < 0) return
        all[i] = all[i].copy(spec = spec)
        keep()
    }

    fun remove(id: Long) {
        all.removeAll { it.id == id }
        keep()
    }

    /** The kept curves for a key's Easing menu (under the named easings), the one [now] is marked. */
    fun easingChoices(now: EasingSpec?): List<EasingChoice> = all.map { EasingChoice(it.name, it.spec, it.spec == now) }

    companion object {
        private const val KEY = "timelineCurves"

        /** The kinds by the names they are kept under (the iPad's). */
        private val KINDS = listOf(
            EasingKind.LINEAR to "linear", EasingKind.HOLD to "hold", EasingKind.SMOOTH to "smooth",
            EasingKind.CUBIC_BEZIER to "cubicBezier", EasingKind.CURVE to "curve", EasingKind.BOUNCE to "bounce",
            EasingKind.ELASTIC to "elastic", EasingKind.STEPS to "steps", EasingKind.ELASTIC_STEPS to "elasticSteps",
            EasingKind.CYCLIC to "cyclic", EasingKind.RANDOM to "random",
        )

        @Volatile
        private var shared: SavedCurves? = null

        /** The app's own. */
        fun of(context: Context): SavedCurves = shared ?: synchronized(this) {
            shared ?: SavedCurves(context.applicationContext.getSharedPreferences("timeline", Context.MODE_PRIVATE)).also { shared = it }
        }
    }
}

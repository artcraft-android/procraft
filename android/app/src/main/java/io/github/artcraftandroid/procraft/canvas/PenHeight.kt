package io.github.artcraftandroid.procraft.canvas

import android.view.MotionEvent

/**
 * How high a hovering pen is, a share of its reach (0 on the glass, 1 at the top), from its
 * [distance] axis and that axis's range ([least] to [most]) as its device says; none for a pen
 * that doesn't say (its cursor then shows whole at any height).
 */
internal fun penHeight(distance: Float, least: Float?, most: Float?): Float? {
    if (least == null || most == null || !distance.isFinite() || !least.isFinite() || !most.isFinite()) return null
    val span = most - least
    if (span <= 0f) return null
    return ((distance - least) / span).coerceIn(0f, 1f)
}

/** How high the pen of hover event [e] is (see [penHeight]). */
internal fun penHeight(e: MotionEvent): Float? {
    val range = e.device?.getMotionRange(MotionEvent.AXIS_DISTANCE, e.source) ?: return null
    return penHeight(e.getAxisValue(MotionEvent.AXIS_DISTANCE, 0), range.min, range.max)
}

package io.github.artcraftandroid.procraft.ui

import androidx.compose.ui.graphics.Color
import androidx.compose.ui.graphics.PathFillType
import androidx.compose.ui.graphics.SolidColor
import androidx.compose.ui.graphics.StrokeCap
import androidx.compose.ui.graphics.StrokeJoin
import androidx.compose.ui.graphics.vector.ImageVector
import androidx.compose.ui.graphics.vector.addPathNodes
import androidx.compose.ui.unit.dp

// The grid timeline's own glyphs (the lasso, its menus' rows, Add new), as the iPad's Icons.swift
// draws them in a 24-point frame: each part stroked, filled, or both.

internal object GridIcons {
    private class Part(val d: String, val width: Float, val fill: Boolean = false, val evenOdd: Boolean = false)

    private fun icon(name: String, vararg parts: Part): ImageVector {
        val b = ImageVector.Builder(name = name, defaultWidth = 24.dp, defaultHeight = 24.dp, viewportWidth = 24f, viewportHeight = 24f)
        for (p in parts) {
            b.addPath(
                pathData = addPathNodes(p.d),
                pathFillType = if (p.evenOdd) PathFillType.EvenOdd else PathFillType.NonZero,
                fill = if (p.fill) SolidColor(Color.White) else null,
                stroke = if (p.width > 0f) SolidColor(Color.White) else null,
                strokeLineWidth = p.width,
                strokeLineCap = StrokeCap.Round,
                strokeLineJoin = StrokeJoin.Round,
            )
        }
        return b.build()
    }

    private const val BOX = "M6 5.5H18A2 2 0 0 1 20 7.5V16.5A2 2 0 0 1 18 18.5H6A2 2 0 0 1 4 16.5V7.5A2 2 0 0 1 6 5.5z"
    private const val SQUARE = "M6 5H18A2 2 0 0 1 20 7V17A2 2 0 0 1 18 19H6A2 2 0 0 1 4 17V7A2 2 0 0 1 6 5z"
    private const val PICTURE = "M5.5 5.5H18.5A2 2 0 0 1 20.5 7.5V16.5A2 2 0 0 1 18.5 18.5H5.5A2 2 0 0 1 3.5 16.5V7.5A2 2 0 0 1 5.5 5.5z"

    val Lasso by lazy {
        icon(
            "grid-lasso",
            Part("M12 4.5C7.3 4.5 3.5 7 3.5 10.2C3.5 13.4 7.3 15.9 12 15.9C16.7 15.9 20.5 13.4 20.5 10.2C20.5 7 16.7 4.5 12 4.5z", 1.9f),
            Part("M7.2 15.2C5.6 16 4.9 17.4 5.4 18.6C5.9 19.8 7.6 20 8.4 19C9.2 18 8.6 16.6 7.2 16.4", 1.9f),
        )
    }
    val Insert by lazy { icon("grid-insert", Part(BOX, 1.7f), Part("M12 9V15", 1.7f), Part("M9 12H15", 1.7f)) }
    val Fill by lazy {
        icon("grid-fill", Part("M14 5.5H6A2 2 0 0 0 4 7.5V16.5A2 2 0 0 0 6 18.5H14", 1.7f), Part("M9 12H20", 1.7f), Part("M16.5 8.5L20 12L16.5 15.5", 1.7f))
    }
    val Options by lazy { icon("grid-options", Part(BOX, 1.7f), Part("M8 10H16", 1.7f), Part("M8 14H13", 1.7f)) }
    val Blend by lazy { icon("grid-blend", Part(SQUARE, 1.7f), Part("M12 8.6A3.4 3.4 0 0 1 12 15.4A3.4 3.4 0 0 1 12 8.6z", 0f, fill = true)) }
    val Mask by lazy { icon("grid-mask", Part(SQUARE, 1.7f), Part("M12 8.6A3.4 3.4 0 0 1 12 15.4A3.4 3.4 0 0 1 12 8.6z", 1.5f)) }
    val Highlight by lazy { icon("grid-highlight", Part("M12 4.5A7.5 7.5 0 0 1 12 19.5A7.5 7.5 0 0 1 12 4.5z", 1.7f), Part("M6.8 17.2L17.2 6.8", 1.7f)) }
    val Rename by lazy { icon("grid-rename", Part("M5 19L6 15L16.5 4.5L19.5 7.5L9 18z", 1.7f)) }
    val Clear by lazy { icon("grid-clear", Part("M13.5 5.5L18.5 10.5L11 18H7L4.5 15.5z", 1.7f), Part("M9 10L14 15", 1.7f), Part("M13 18.5H20", 1.7f)) }
    val Flip by lazy {
        icon("grid-flip", Part("M5 9H19", 1.7f), Part("M15.5 5.5L19 9L15.5 12.5", 1.7f), Part("M19 15H5", 1.7f), Part("M8.5 11.5L5 15L8.5 18.5", 1.7f))
    }
    val Scissors by lazy {
        icon(
            "grid-scissors",
            Part("M7 14.5A2.5 2.5 0 0 1 7 19.5A2.5 2.5 0 0 1 7 14.5z", 1.6f),
            Part("M17 14.5A2.5 2.5 0 0 1 17 19.5A2.5 2.5 0 0 1 17 14.5z", 1.6f),
            Part("M8.5 15L16.5 4.5", 1.6f),
            Part("M15.5 15L7.5 4.5", 1.6f),
        )
    }
    val Copy by lazy {
        icon(
            "grid-copy",
            Part("M10 8.5H17A1.5 1.5 0 0 1 18.5 10V18A1.5 1.5 0 0 1 17 19.5H10A1.5 1.5 0 0 1 8.5 18V10A1.5 1.5 0 0 1 10 8.5z", 1.6f),
            Part("M15.5 8.5V6A1.5 1.5 0 0 0 14 4.5H7A1.5 1.5 0 0 0 5.5 6V14A1.5 1.5 0 0 0 7 15.5H8.5", 1.6f),
        )
    }
    val Paste by lazy {
        icon(
            "grid-paste",
            Part("M9 6H7.5A1.5 1.5 0 0 0 6 7.5V18A1.5 1.5 0 0 0 7.5 19.5H16.5A1.5 1.5 0 0 0 18 18V7.5A1.5 1.5 0 0 0 16.5 6H15", 1.6f),
            Part("M10 4.5H14A1 1 0 0 1 15 5.5V6.5A1 1 0 0 1 14 7.5H10A1 1 0 0 1 9 6.5V5.5A1 1 0 0 1 10 4.5z", 1.6f),
        )
    }
    val Key by lazy { icon("grid-key", Part("M12 4.2L19.8 12L12 19.8L4.2 12z", 1.9f), Part("M12 9L15 12L12 15L9 12z", 0f, fill = true)) }
    val Photo by lazy {
        icon("grid-photo", Part(PICTURE, 1.7f), Part("M4 16L8.6 11.6L12 14.6L14.8 12.4L20 16.6", 1.7f), Part("M15.8 8A1.3 1.3 0 1 0 15.8 10.6A1.3 1.3 0 1 0 15.8 8z", 0f, fill = true))
    }
    val File by lazy {
        icon("grid-file", Part("M3.5 7.5A2 2 0 0 1 5.5 5.5H9.3L11.3 7.8H18.5A2 2 0 0 1 20.5 9.8V16.5A2 2 0 0 1 18.5 18.5H5.5A2 2 0 0 1 3.5 16.5z", 1.7f), Part("M3.8 11H20.2", 1.7f))
    }
    val Remove by lazy { icon("grid-remove", Part(BOX, 1.7f), Part("M9 12H15", 1.7f)) }
    val Broom by lazy { icon("grid-broom", Part("M19.5 4.5L13 11", 1.9f), Part("M10.4 9.6L14.4 13.6L9.4 19.4L4.6 14.6z", 0f, fill = true)) }
}

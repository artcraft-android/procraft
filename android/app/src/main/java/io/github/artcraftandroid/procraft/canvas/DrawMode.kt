package io.github.artcraftandroid.procraft.canvas

/** How the Pen or Eraser draws, as the brush library's top offers it: with the brush, its lasso, or light (the Pen on an HDR canvas). */
enum class DrawMode(val label: String) {
    BRUSH("Brush"),
    LASSO("Lasso"),
    LIGHT("Light"),
}

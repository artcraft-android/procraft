# The stock brush library

ProCraft comes with 394 stock brushes in 37 sets, and with the brushes of its earlier versions in a
set of their own, Legacy, after them.

## How the stock brushes are made

- **Settings.** Each stock brush is ProCraft's own variant of a familiar default brush type. Its
  settings use the brush studio's controls. The stock builder varies the initial slider values by
  1% to 3%, up or down, using the brush name to keep each build reproducible. It preserves blend
  modes and rendering modes. Individual presets have further adjustments based on how they draw
  with ProCraft's textures.
- **Names.** Brushes with plain descriptive names (pencil grades, Technical Pen, Soft Airbrush,
  Charcoal Block and the like) keep them. Every brush with a distinctive name has a name of its own.
- **Shapes and grains.** The app loads its 288 remade shapes and grains at their original
  dimensions from [`assets/stock-sources`](../assets/stock-sources). Three have full-resolution
  edits in [`assets/stock`](../assets/stock) for the water flicks' black level, the chipboard
  grain's midtones and the hard tip's stray edge pixels. Other stock textures are drawn in code.
  Brushes that use the same stock texture share its pixels.
- **Previews.** The library draws each brush's preview with ProCraft's own engine, from the brush's
  own settings.

No brush file, shape or grain picture, preview or text from elsewhere is in ProCraft.

The settings live in `crates/core/src/stock_brushes.rs`, written by `write_stock_brushes` in
`crates/core/src/stock_library.rs`, which builds the brushes from them.

## Your brushes and the Legacy set

Brushes are known by where they came from, so the update changes nothing about them:

- The brushes of earlier versions ship as they were, in the Legacy set.
- A brush you changed keeps its changes.
- Artworks that remember a brush still find it.
- The first time the stock sets join a library kept by an earlier version, its sets of earlier
  brushes go into the Legacy set, after the stock sets. This only applies to sets left as they
  shipped: under their own name, and holding nothing but brushes that shipped in them. Sets you
  renamed or added brushes to stay where they are. Your tools, Recent brushes and QuickMenu
  buttons follow the brushes they were on.
- Restore default brushes puts deleted stock and Legacy brushes back where they ship.

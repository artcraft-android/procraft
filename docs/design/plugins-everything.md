Status: steps 1 to 3 built, on branch `plugins/builtin-plugins`: built-in plugins
(`crates/core/src/builtin_plugins.rs`, 2026-11-23), and Pixelate, Dither and Vignette moved into
plugin files (2026-11-24). Put onto the second upstream merge (branch
`plugins/on-upstream-2026-11-24`), step 3's live filters gave way to upstream's filter layers
(interface 17, as decided): Dither and Vignette are filter layers in the `pixel-effects` plugin,
and Pixelate is Infinite Plugins'. Exports took interface 23 there, and mesh warps and tools 24.
Step 4 built too (2026-11-24, `crates/core/src/plugin_exports.rs`): exports of the animation, with
the sprite sheet and the Harmony scene moved into the `frame-exports` plugin. Step 5 built
(2026-11-24, `crates/core/src/plugin_warps.rs`, `plugin_tools.rs`): mesh warps and tools, with
Puppet Warp and Comic Panels moved into the `puppet-warp` and `comic-panels` plugins. Step 6 built
the same day: upstream's features behind thirteen built-ins. All six steps are done.

The goal: every feature beyond a plain painting app is a plugin on the Plugins page, with a card,
Turn on and Turn off, and as much of its code as can really move into a plugin file does. With
every plugin off the app is a canvas, brushes, layers, selection, transform, colour, the gallery
and export.

Decided:

- **Extend the plugin system** so more of the code really moves out of the app, rather than only
  gating features with switches.
- **About ten plugins** for this fork's features, one per area, smaller parts as switches inside.
- **An artwork opens fully with a plugin off.** It shows and plays as it was saved; only the tools
  and panels that edit that feature are hidden.
- **Upstream's features too**, not only this fork's.

## What can move and what can't

A plugin's code today is a WebAssembly module run by wasmi, an interpreter (the iPad allows no code
made at run time). At a hook it sees only numbers and hands numbers back; with an action and a
permission it can reach a few app functions. Filters are WGSL the app runs over a layer, once, with
the result clamped to 0..1.

That decides most of it:

- **Can't move into a plugin file:** anything that is GPU compositing every frame, platform media,
  or the document model the renderer reads. The timeline (playback, mattes, cameras, frame caches),
  sound and video (AVFoundation, MediaCodec), HDR (half-float canvases, EDR, the surface), layer
  styles and adjustment layers' compositing, native vector layers' crisp zoom and fill, ribbon
  stamps, smudge. Interpreted WebAssembly is far too slow for these, and has no GPU or media access.
- **Can move:** computation that runs now and then and hands back a result: the sprite scan behind
  Texture Packer, the sprite-sheet and Harmony scene writers, Puppet Warp's mesh and its solve,
  Comic Panels' cutting, easings, tweening (already a plugin), and per-pixel effects written as WGSL
  (pixelate, dither, vignette) once filters can be live.

And "an artwork opens fully with its plugin off" means the code that *shows* a feature has to stay
in the app even where its editing moves out. So there are two kinds of plugin.

### Built-in plugins (new)

A built-in plugin is one of the app's own features registered as a plugin: the same manifest and
card, controls and switches, on the same page, but its code is the app's, compiled in. Its manifest
says `"holds": ["builtin"]`. It can't be imported or shared as a file, and Remove only turns it off.
Turning it off hides its tools, panels, menu rows and Share entries on every platform; the document
model and renderer are untouched, so artworks that use it open, show and play as saved.

This is the existing Lasso Fill pattern made general: rather than one hook per feature (`lasso_fill`,
answered by a one-line `yes!`), the core keeps a registry of built-in plugins, and each part they
hide is a `Feature`. One call answers whether a feature shows:

- core: `Plugins::has(feature)`, and `Plugins::features()` for all that show, in one call.
- Swift: `PluginsModel.shared.has(.timeline)`, read again whenever the plugins change.
- Kotlin: `vm.has(Feature.VECTOR_LAYERS)`. Desktop: `prefs.plugins.has(Feature::PixelArt)`, handed
  to each panel every frame as Lasso Fill's is.

Built-ins are listed by `Plugins::with_bundled`, the constructor the three apps use, first on the
page. Ids start `procraft.`, which plugin files can't take. Their on or off and switches are kept as
any plugin's are, under `<id>.on` and `<id>.<switch>`.

Built-ins start **on**, unlike the plugin files the app comes with. Upstream's rule is that with
no plugin touched the app is as it always was; for a built-in that means on, since these are
features the app already had. Turning one off is the choice.

### File plugins (grown, interface 9 onward)

The `.procraft-plugin` files there are today, given more reach so real feature code can live in them:

| Interface | Adds | So these move into plugin files |
|---|---|---|
| 17 (upstream's) | **Filter layers:** a filter that is a layer, redrawn as the layers under it change (this took the place of step 3's live filters) | Dither, Vignette (Pixelate is Infinite Plugins') |
| 23 | **Exporters:** a Share entry; the app hands the plugin frames, layers and timing as it asks, and the plugin hands back files to write, PNG encoding done by a utility | Sprite sheet, Harmony scene (Texture Packer stays built in) |
| 24 | **Mesh warps:** the app hands a layer's outline and pins, the plugin hands back a triangle mesh and where its points go; the GPU warps the pixels | Puppet Warp |
| 24 | **Tools:** a tool on the bar whose touches go to the plugin, which answers with edits to the layers (new group, polygon frame, clip) through a `layers.edit` permission | Comic Panels' cutting and gutters |

With a file plugin off, what it already made stays: comic panels stay groups, a warp is already
pixels. A filter layer shows nothing until its plugin is on again (upstream's way, as decided).

## This fork's features, as plugins

| Plugin | Kind | Switches inside | Moves to a file later |
|---|---|---|---|
| Timeline | built-in | Motion paths, Mattes and group clips, Camera, Performed moves, My curves | Easings (My curves) |
| Timeline media | built-in | Sounds, Videos, Timing tracks, Animatic detector | |
| HDR | built-in | HDR canvases, Light tool, Exposure and Limit curves | |
| Effects | built-in | Adjustment layers, Layer styles | (Pixelate, Dither and Vignette moved to the `pixel-effects` file, step 3) |
| Vector layers | built-in | Node editing, SVG/PDF share | |
| Texture Packer | built-in | | (Sprite Sheet and Harmony Scene moved to the `frame-exports` file, step 4) |
| Ribbon and pixel brushes | built-in | Ribbon brushes, Pixel-art brushes | |
| Comic Panels | file (`comic-panels`, step 5) | | |
| Workspaces | built-in | | |
| Puppet Warp | file (`puppet-warp`, step 5) | | |

The existing `effects` filter plugin (Extract lines, Outline, Posterise) stays as it is; the new
Effects built-in takes a different id.

Lasso Fill and Stroke preview opacity are already plugins.

### What each one hides, as built

| Feature | iPad | Android | Desktop |
|---|---|---|---|
| Text | Actions › Add › Add text; a text layer's Edit Text; Add text in the QuickMenu; undo no longer reopens text to edit, and text being edited is ended | the same | the same |
| 3D painting | a model brought in (gallery Import, Files, a drop) says to turn the plugin on; a model artwork of ours opens and paints as saved | the same; a model's Edit lighting & environment | the same; a model's Edit lighting & environment |
| Animation Assist | Actions › Canvas › Animation Assist, and Page Assist on its switch; the frames' bar under the canvas (playing stops); Animation Assist, Page Assist and next and previous frame in the QuickMenu. An artwork with Animation Assist on shows the frame it is on | the same | the same |
| Drawing guides | Actions › Canvas › Drawing Guide and Edit Drawing Guide; a layer's Drawing Assist; the guides' and Drawing Assist's QuickMenu actions and the perspective guide's key; a gesture set to assisted drawing and its Gesture controls page. A guide that is on is paused: neither drawn nor followed, kept as saved | the same | the same |
| Time-lapse | Actions' Video tab (Replay, Recording, Export); the new canvas's Time-lapse settings. Nothing is recorded while off; what was recorded stays | the same | the same |
| Gallery stacks | the gallery's Stack (Select's bar and its panel); an artwork dragged onto another or onto a stack moves instead of stacking. Stacks still open, and artworks still come out of them | the same (Select's bar and More) | the same (Select's bar and More) |
| Clipping groups | a layer's Clipping Mask (with the shared ways in); clipped layers stay clipped and show so | the same | the same |
| PSD import, PSD export | a PSD brought in (gallery Import, Files, a drop) says to turn the plugin on; Share › PSD in Actions and in the gallery's Share; an artwork dragged out as PSD goes out as ProCraft | the same | the same |
| Palettes | the colour panel's Palettes tab (the panel on it goes to Disc), in the small pickers too; the default palette's swatches under the other modes; a palette brought in (gallery Import, a drop) says to turn the plugin on | the same | the same, the floating panel too; the small pickers (onion skins, a canvas's background, gradient stops) still offer Palettes |
| Liquify | Adjustments' Liquify; its QuickMenu action | the same | the same |
| QuickShape | a stroke held still snapping (draw and hold off) and a gesture set to QuickShape; Gesture controls' QuickShape page; QuickShape and QuickShape stroke in the QuickMenu | the same | the same |
| Gradients | Adjustments' Gradient Map (as an adjustment layer too); its QuickMenu action | the same | the same |
| Recolor | its QuickMenu action, its only way in | the same | the same |
| Timeline | Actions › Canvas › Timeline and its settings; the timeline bar and the motion path on the canvas (an artwork on a timeline still opens on its frame) | Actions › Canvas › Timeline and Timeline Settings; the timeline grid's bar and its settings card | the same |
| Performed moves | the grid's Perform button and an armed take | the same | the same |
| My curves | My curves in the curve editor | the same, and the kept curves in a key's Easing menu | the same, and the kept curves in a key's Easing menu |
| Sounds, Videos | the Add card's Video tile; sounds and videos in its file picker | the same | none there |
| Animatic detector | Make timing track… on a video clip | the same | none there |
| HDR | the new-canvas HDR switch (a new canvas is standard) | none there | none there |
| Light tool | the Light draw mode; the Light tool left for the brush | the same | the Light tool left for the brush (no way in there) |
| HDR curves, colours, exports | Curves' Exposure and Limit; the colour panel's light row; HDR HEIC and OpenEXR | | |
| Adjustment layers | the Adjustments menu's Adjustment Layer; Adjustment settings in layer options | the same | the same |
| Layer styles | Layer Style in layer options | the same | the same |
| Vector layers | + held; SVG brought in (says to turn the plugin on); a vector layer's own rows | + held; SVG brought in | + held; SVG brought in |
| Vector PDF | Share | Share | Share |
| Texture Packer | Share | the same | the same |
| Ribbon brushes | the brush studio's Ribbon | the same | the same |
| Pixel art | the studio's Pixel art; Pixelated in Selection, Transform and Crop, off while hidden | the same | the same |
| Comic Panels (the plugin's tool, step 5) | Actions › Canvas › Comic panels | the same | the same |
| Workspaces | the gallery's workspace button; the gallery goes back to the first workspace | the same | the same |
| Puppet Warp (the plugin's warp, step 5) | Transform's Puppet (a remembered Puppet opens as Freeform) | the same | the same |

A brush whose Pixelated or Ribbon is set keeps painting that way with the plugin off: the setting
is the brush's, as an artwork's content is.

## Upstream's features

Upstream keeps adding features as code in the app, so each one gated here is a seam to keep
through every merge from `procraft-upstream`. The gate calls are small and sit at the UI edges, but
merges will touch them. Thirteen built-ins, listed after this fork's (`UPSTREAM` in
`builtin_plugins.rs`), confirmed on 2026-11-24:

| Plugin | Id | Switches inside |
|---|---|---|
| Text | `procraft.text` | |
| 3D painting | `procraft.3d-painting` | |
| Liquify | `procraft.liquify` | |
| QuickShape | `procraft.quickshape` | |
| Gradients | `procraft.gradients` | |
| Recolor | `procraft.recolor` | |
| Animation Assist | `procraft.animation-assist` | Page Assist |
| Drawing guides and symmetry | `procraft.drawing-guides` | |
| Time-lapse | `procraft.time-lapse` | |
| Gallery stacks | `procraft.gallery-stacks` | |
| Clipping groups | `procraft.clipping-groups` | |
| PSD | `procraft.psd` | PSD import, PSD export |
| Palettes | `procraft.palettes` | |

Tweening stays the file plugin it is; it lives in Animation Assist's bar, so it goes with it.

Everywhere, a built-in off also hides its QuickMenu actions (the picker doesn't list them and a
menu's button for one does nothing) and its keyboard shortcuts. The core says which feature each
way in belongs to, so the apps filter with one call: `quick_action_feature`, `adjustment_feature`,
`layer_option_feature` and `gesture_purpose_feature`.

Two of them pause what an artwork has on rather than leave it running with no way to change it
(as decided): with Drawing guides off the guide is neither drawn nor followed
(`CanvasSession::set_guide_paused`, the artwork keeping its guide as saved), and with Time-lapse
off nothing is recorded.

### What each one hides, as built

| Feature | iPad | Android | Desktop |
|---|---|---|---|

Stays the app's, not a plugin: the canvas and view, the brush engine and brush library, layers,
selection, transform, colour, undo, the gallery, PNG/JPEG export, and the Plugins page itself.

## Steps

1. **Built-in plugins:** the registry, `holds: ["builtin"]`, the gate call in core and its bindings,
   the card (Remove hidden), Plugins page grouping, tests. Lasso Fill's hook left as it is.
2. **This fork's features behind the ten built-ins:** iPad first, then Android and desktop. Each feature's
   entry points (bar buttons, Actions rows, panel tabs, Share entries, new-canvas choices) asking
   its gate. Opening artworks untouched.
3. **Filter layers:** done. Dither and Vignette are filter layers (upstream's interface 17) in the
   bundled `pixel-effects` plugin (off until turned on, as bundled files are); Pixelate is Infinite
   Plugins' filter layer. Their native code is gone. An artwork with a native Pixelate, Dither or
   Vignette layer no longer opens (decided: there are none). (Built first as live filters,
   interface 9, a kind of adjustment layer; upstream's filter layers took their place.)
4. **Interface 23, exporters:** done. Sprite Sheet and Harmony Scene are the bundled
   `frame-exports` plugin (off until turned on); their native code is gone. The app draws the
   frames (or each track alone) and puts pictures together; the plugin lays them out and writes
   every file's text. Share's sheet for an export is built from the plugin's controls. Texture
   Packer stays built in (as decided): it is an on-canvas tool, not an export, and is the
   built-in `procraft.texture-packer`. Desktop and Android have the iPad's screen for a plugin's
   export (`desktop/src/plugin_share.rs`, `ui/PluginShareUi.kt`) in place of the native Sprite
   Sheet and Harmony screens the upstream merge's ports had.
5. **Interface 24, mesh warps and tools:** done. Puppet Warp and Comic Panels are the bundled
   `puppet-warp` and `comic-panels` plugins (off until turned on); their built-ins and native code
   are gone. Puppet Warp: the app hands over what is painted as cells (at most 262,144), the
   plugin lays the mesh (outline traced, Ruppert-refined) and bends it by its pins (as-rigid-as-
   possible); the app keeps the mesh, finds pins on it, draws it and warps the pixels on the GPU as
   before. Interpreted, the solve was far too slow for a drag (52 ms a frame at the default
   density, and the densest setting ran out of time), so a new utility, banded least squares
   (`plugin_band.rs`), factors and solves natively what the plugin's code builds: now about 6 ms a
   frame (25 at the densest), 45 ms to put a pin, 0.4 s to lay a mesh. Comic Panels: a tool whose
   touches go to the plugin, answering with shapes to show and edits (a panel's shape, a new
   panel) the app makes as one step through the `layers.edit` permission; the frames, their
   drawing and the clipping stay the app's (`procraft_engine::panels`), so a page opens and shows
   as saved with the plugin off. Desktop and Android have the iPad's window for a plugin's tool
   (`desktop/src/plugin_tool.rs`, `ui/PluginToolUi.kt`) in place of their native Comic Panels,
   and their Transform offers Puppet only while a plugin lays its mesh.
6. **Upstream's features behind built-ins**, one area at a time.

Steps 1 and 2 give the whole page of plugins with every feature switchable. Steps 3 to 5 each move
real code out and can be done in any order. Step 6 is independent of 3 to 5.

## Open questions

- Built-in plugins on Android and desktop: the upstream merge ported many iPad features there; each
  asks its built-in as on the iPad (the table above). HDR canvases, their curves, colour row and
  exports aren't there yet (desktop has no timeline media either), so those switches change
  nothing there.

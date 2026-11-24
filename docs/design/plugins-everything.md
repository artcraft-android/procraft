# Everything as plugins

Status: proposal, 2026-11-23. Nothing here is built yet.

The goal: every feature beyond a plain painting app is a plugin on the Plugins page, with a card,
Turn on and Turn off, and as much of its code as can really move into a plugin file does. With
every plugin off the app is a canvas, brushes, layers, selection, transform, colour, the gallery
and export.

Decided with Soren:

- **Extend the plugin system** so more of the code really moves out of the app, rather than only
  gating features with switches.
- **About ten plugins** for Soren's features, one per area, smaller parts as switches inside.
- **An artwork opens fully with a plugin off.** It shows and plays as it was saved; only the tools
  and panels that edit that feature are hidden.
- **Upstream's features too**, not only Soren's.

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
answered by a one-line `yes!`), the core keeps a registry of built-in ids and switches, and one call
answers "is this on":

- core: `Plugins::builtin_on(id, switch)`, exported through uniffi as `builtin_on`.
- Swift: `PluginsModel.on(.timeline)`, `PluginsModel.on(.timeline, .media)`.
- Kotlin and desktop the same.

Bundled built-ins start **off**, as every bundled plugin does. That's the rule upstream wrote for
the page ("with none on, the app is as it always was"). **Open question:** whether Soren's own build
should start them on.

### File plugins (grown, interface 9 onward)

The `.procraft-plugin` files there are today, given more reach so real feature code can live in them:

| Interface | Adds | So these move into plugin files |
|---|---|---|
| 9 | **Live filters:** a filter that is a layer (an adjustment layer whose effect is the plugin's WGSL), redrawn as the layers under it change; its controls keyable on the timeline; not clamped on HDR canvases | Pixelate, Dither, Vignette (and upstream's simpler adjustments, later) |
| 10 | **Exporters:** a Share entry; the app hands the plugin frames, layers and timing as it asks, and the plugin hands back files to write, PNG encoding done by a utility | Sprite sheet, Harmony scene, Texture Packer |
| 11 | **Mesh warps:** the app hands a layer's outline and pins, the plugin hands back a triangle mesh and where its points go; the GPU warps the pixels | Puppet Warp |
| 11 | **Tools:** a tool on the bar whose touches go to the plugin, which answers with edits to the layers (new group, polygon frame, clip) through a `layers.edit` permission | Comic Panels' cutting and gutters |

With a file plugin off, what it already made stays: a live filter's layer keeps showing the pixels
it last drew (as a plugin layer does today), comic panels stay groups, a warp is already pixels.

## Soren's features, as plugins

| Plugin | Kind | Switches inside | Moves to a file later |
|---|---|---|---|
| Timeline | built-in | Motion paths, Mattes and group clips, Camera, Performed moves, My curves | Easings (My curves) |
| Timeline media | built-in | Sounds, Videos, Timing tracks, Animatic detector | |
| HDR | built-in | HDR canvases, Light tool, Exposure and Limit curves | |
| Effects | built-in, then file | Adjustment layers, Layer styles; Pixelate, Dither, Vignette | Pixelate, Dither, Vignette (9) |
| Vector layers | built-in | Node editing, SVG/PDF share | |
| Exports | built-in, then file | Sprite sheet, Harmony scene, Texture Packer | all three (10) |
| Ribbon and pixel brushes | built-in | Ribbon brushes, Pixel-art brushes | |
| Comic Panels | built-in, then file | | cutting (11) |
| Workspaces | built-in | | |
| Puppet Warp | built-in, then file | | mesh and solve (11) |

The existing `effects` filter plugin (Extract lines, Outline, Posterise) stays as it is; the new
Effects built-in takes a different id.

Lasso Fill and Stroke preview opacity are already plugins.

## Upstream's features

Upstream keeps adding features as code in the app, so each one gated here is a seam to keep
through every merge from `procraft-upstream`. The gate calls are small and sit at the UI edges, but
merges will touch them. Proposed built-ins:

- Text
- 3D painting
- Liquify
- QuickShape
- Gradients
- Recolor
- Animation Assist (Tweening already a file plugin inside it)
- Drawing guides and symmetry
- Time-lapse
- Gallery stacks
- Clipping groups
- PSD import and export
- Palettes

Stays the app's, not a plugin: the canvas and view, the brush engine and brush library, layers,
selection, transform, colour, undo, the gallery, PNG/JPEG export, and the Plugins page itself.

## Steps

1. **Built-in plugins:** the registry, `holds: ["builtin"]`, the gate call in core and its bindings,
   the card (Remove hidden), Plugins page grouping, tests. Lasso Fill's hook left as it is.
2. **Soren's features behind the ten built-ins:** iPad first, then Android and desktop. Each feature's
   entry points (bar buttons, Actions rows, panel tabs, Share entries, new-canvas choices) asking
   its gate. Opening artworks untouched.
3. **Interface 9, live filters:** Pixelate, Dither and Vignette rewritten as WGSL in a file plugin;
   their native code deleted.
4. **Interface 10, exporters:** sprite sheet, Harmony and Texture Packer as a file plugin.
5. **Interface 11, mesh warps and tools:** Puppet Warp and Comic Panels.
6. **Upstream's features behind built-ins**, one area at a time.

Steps 1 and 2 give the whole page of plugins with every feature switchable. Steps 3 to 5 each move
real code out and can be done in any order. Step 6 is independent of 3 to 5.

## Open questions

- Do built-ins start on in Soren's own build, or off as the bundled rule says?
- Built-in plugins on Android and desktop: those apps lack parts of the timeline, media and HDR
  interfaces today; their cards would show only what each app has.
- Keyed live-filter controls need the timeline's key lanes to take a plugin's controls, which ties
  step 3 to the Timeline built-in.

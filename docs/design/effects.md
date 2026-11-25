# Adjustment layers and layer styles

Status: as built, Nov 15, 2026. Adjustment layers and layer styles live in the engine (`Layer::adjustment`, `Layer::style`, `filter::Filter`), the GPU canvas (`composite_adjustment`, `composite_clipped`, `styled`) and the core (the adjustment session, `layer_style.rs`). Their GPU tests are in `crates/gpu/tests/effects.rs`; the engine's in `crates/engine/src/effects_tests.rs`, the session's in `crates/core/src/effects_tests.rs`. On a timeline an adjustment layer is a clip whose values can be keyed: see "Adjustment clips" in `timeline.md`.

## What they are

- **An adjustment layer** is a paint layer with no pixels of its own and a `LayerAdjustment` (one of ProCraft's adjustments, with its curves or gradient table). As the canvas composites, what is below it in its group (all below, if the group passes through) is redrawn through the adjustment and laid back as far as its opacity and mask let it, in its blend mode.
- **Layer styles** are an outline, a drop shadow and outer and inner glows, drawn round what a paint layer shows (with what is clipped to it) as it composites, and a Color (Nov 16, 2026), which makes all the layer shows one colour, as opaque as it was (a colour and an opacity, no size or softness; the inner glow lies over it; `LayerStyle::color`, `fs_style`'s `d`). Its pixels stay as they are.

## Merging

Merging bakes both in, as painting apps usually do: the merged layer looks as the layers did, and what it holds is plain pixels.

- **Picked layers** merge into the lowest paint layer among them that isn't an adjustment layer. An adjustment layer above it redraws the merged-so-far pixels through its adjustment (through its mask, at its opacity, in its mode) in its place in the order. One below that layer has nothing among them to redraw: it stays as it is.
- **Merge Down** works on an adjustment layer too: it is baked into the paint layer below.
- **Styles** go into the pixels before the merge: each layer's mask first, then its styles drawn round what it shows, then its opacity, as it composites. The lower layer's styles are baked too, and the merged layer keeps none.
- Undo keeps whole copies of the layers changed: an adjustment or styles change the lower layer beyond what the upper one painted.

## PSD export

- An HSB adjustment layer is written as Photoshop's Hue/Saturation layer (`hue2`), curves as Curves (`curv`, 17 points a channel sampled from the table, the curve for all three first), colour balance as Color Balance (`blnc`). Each keeps its mask, opacity, blend mode and clipping. ProCraft's HSB turns hue in luma and chroma rather than in HSL, so a Photoshop reading is close, not exact.
- Every other adjustment (blurs, noise, gradient map, pixelate, dither, vignette and the rest) is flattened: a pixel layer of what it draws, at full strength and unmasked, with everything above it hidden (`Document::adjustment_alone`, read by `Canvas::read_adjustment_alone`). That layer carries the adjustment layer's mask, opacity and mode, so it looks as the adjustment did.
- ProCraft's own PSD reader leaves adjustment layers out, as it always has.

## Painting a mask

Painting on an adjustment layer's mask shows as the stroke goes: while one is painted, the mask with the stroke laid in is drawn into a texture of its own (`live_mask`), which the adjustment reads in place of the mask.

## What a stroke redraws

A pointwise adjustment (colour changes, noise, halftone, dither, vignette) redraws only what changed. One that reads its neighbours (`Filter::reach`: a blur's three sigmas and its halvings, sharpen's one pixel, a motion blur's half length, a pixelate cell) spreads a change that far, so the canvas composites the changed part grown by the sum of their reaches, and that again round it, so each reads what is below it composited afresh. The outer band, drawn from parts below that weren't composited again, is put back from a copy of the picture as it was. The blur's own passes draw only that region. A zoom blur, a radial chromatic aberration and a glitch can reach anywhere: with one, the whole canvas composites, as before. So does any blurring adjustment layer on a timeline, since its keys change how far it reads.

## Curves' exposure and limit

On an HDR canvas Curves lists Exposure under Gamma, Red, Green and Blue (folded into Curves rather than added as an adjustment of its own). Picked, its sliders take the curve's place: Exposure (−5 to +5 stops), Above (only what is at least that bright, by its brightest channel) and Soft (how far below that it fades in). The light is multiplied as one value per pixel, before the curves, so hue holds; past white it is kept on an HDR canvas and scaled back to white, as one value, on a standard one. `Filter::Curves { exposure, above, soft }` carries it, so an adjustment layer keeps it and a timeline keys it (Exposure, Above and Soft are Curves' rows in the Keys panel, an HDR canvas's only). A Curves written before it had an exposure reads with none. In a PSD, Curves with an exposure is flattened, as Photoshop's curves can't say it.

Under Exposure is **Limit** (folded in the same way): **Peak** (white to +5 stops above it, the end of the track Off, as it starts) and **Knee** (0 to 95% of the peak). After the curves, what is brighter than the knee (by its brightest channel, encoded) is eased towards the peak, the other channels scaled with it so hue holds; below the knee nothing moves. `Filter::Curves` carries `peak` (none at `LIMIT_OFF`, 6) and `knee`, keyed and kept as the exposure is; a PSD flattens Curves with a limit too.

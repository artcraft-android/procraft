# Krita brushes

A ProCraft plugin that is a brush engine (see the guide's "Brush engines of its own"): it reads
Krita's brush presets (`.kpp`) and bundles (`.bundle`) as they are imported, and draws their
brushes as Krita's pixel brush draws them. It comes with the app, off.

## What it draws

- Tips: auto tips (circle or rectangle; default, soft-by-curve or Gaussian edge; ratio, fades,
  spikes, density and randomness, drawn afresh from a few variants), and pictures: PNGs, GIMP
  brushes (`.gbr`) and pipes (`.gih`, a cell picked per dab by each dimension's selection: constant,
  incremental, random, pressure, angular, velocity, tilt).
- Spacing: fixed or automatic, by the dab's size across and down its own frame (or the same every
  way), scaled by the spacing option.
- Options and their sensors: size, opacity, flow, rotation, ratio, spacing, scatter (both ways,
  along or across), mirror, softness, darken, mix, hue, saturation and value, the texture's strength;
  sensors pressure, X and Y tilt, tilt direction and elevation, speed, drawing angle, rotation,
  distance, time, fade, fuzzy per dab and per stroke (perspective counts as 1); each through its
  curve, several combined by multiply, add, max, min or difference.
- Painting: wash (dabs build towards their opacity, the stroke keeping near the opacity it has
  averaged) or build-up; the preset's blend mode; eraser presets erase.
- Texture: the pattern made into Krita's mask (brightness, contrast, invert, neutral point,
  cutoff), scaled, offset (or at random per stroke), combined with each dab by its texturing mode at
  the strength its sensors give, softly where it says.
- Masking brushes: a second brush along the same path, its size a share of the main brush's, its
  coverage combined with the main stroke's by its composite op.

The Krita brush studio (the plugin's action, opened by the app's brush studio for its brushes) shows
a brush's Krita settings a page at a time: tip, spacing, size, opacity and flow, turn and squash,
scatter and mirror, colour and texture, each option with the sensor it follows and its curve.

The app keeps how each brush looks (its tips drawn, its texture's mask) with the plugin, so only a
brush's first stroke ever waits for them; after that the plugin only reads the brush's settings.
In the app, the app's utilities unpack bundles and decode pictures, much faster than the plugin's own
readers (which its tests use) run in it.

## Not yet

- Krita's other engines (colour smudge, deform, and the rest): those presets come in as the app
  reads them.
- Colour picture tips (stamped in their own colours), lightness and gradient tips and texturing
  modes: drawn as masks, and the two texturing modes as subtract.
- Sharpness (threshold and pixel snapping), airbrush (dabs by time while the pen holds still),
  perspective.
- Dabs are rotated, scaled and textured on the GPU from tips drawn once, not drawn afresh at each
  dab's own pixels as Krita draws them.

## Where its knowledge comes from

Krita is free software under the GNU GPL. This plugin's code is its own: what it does was learned
from Krita's manual (the brush engines, options, sensors and texture pages) and from reading Krita's
sources for facts only (formulas, defaults and file layouts), none of it copied or translated:

- presets, sensors and curve options: how options read their settings, how sensors' values go
  through curves and combine, and each sensor's range;
- the auto tip's mask generators and how density and randomness thin a dab;
- spacing (fixed, automatic as a share of the square root of the size, anisotropic) and the walk
  from one dab to the next;
- the pixel brush's dab: size, rotation, ratio, scatter, mirror, opacity and flow (wash vs build-up,
  the averaged opacity of a washed stroke), darken, mix and HSV;
- GIMP brush, pipe and pattern files and how pipes pick cells;
- the texture's mask and its texturing modes; the masking brush and its composite ops.

The file formats (PNG, zip, base64, a small XML reader) are written here from their public
specifications.

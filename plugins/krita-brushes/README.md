# Krita brushes

A ProCraft plugin that is a brush engine (see the guide's "Brush engines of its own"): it reads
Krita's brush presets (`.kpp`) and bundles (`.bundle`) as they are imported, and draws their
brushes as Krita's pixel brush and colour smudge brush draw them, and its quick, tangent normal,
sketch, spray, bristle, hatching, curve, particle and grid brushes as far as dabs draw them. It comes
with the app, off.

## What it draws

- Tips: auto tips (circle or rectangle; default, soft-by-curve or Gaussian edge; ratio, fades,
  spikes, density and randomness, drawn afresh from a few variants), and pictures: PNGs, GIMP
  brushes (`.gbr`) and pipes (`.gih`, a cell picked per dab by each dimension's selection: constant,
  incremental, random, pressure, angular, velocity, tilt), applied as the preset says: as a mask,
  stamped in their own colours, as a lightness map (its strength an option of its own) or as a
  gradient map (from the colour to the secondary colour), a colour picture's grey through its
  middle point, brightness and contrast; older presets' scale and adjustments read as Krita reads
  them.
- Spacing: fixed or automatic, by the dab's size across and down its own frame (or the same every
  way), scaled by the spacing option.
- Options and their sensors: size, opacity, flow, rotation, ratio, spacing, scatter (both ways,
  along or across), mirror, softness, darken, mix, hue, saturation and value, the texture's strength;
  sensors pressure, X and Y tilt, tilt direction and elevation, speed, drawing angle, rotation,
  distance, time, fade, fuzzy per dab and per stroke (perspective counts as 1); each through its
  curve, several combined by multiply, add, max, min or difference.
- Painting: wash (dabs build towards their opacity, the stroke keeping near the opacity it has
  averaged) or build-up; the preset's blend mode; eraser presets erase. Sharpness: each dab's mask
  made hard at its threshold (what lies within its softness kept), dabs moved onto whole pixels as
  far as it says. Airbrush: dabs by time as well as by spacing (or by time alone), the app handing
  the pen again while it holds still.
- Texture: the pattern made into Krita's mask (brightness, contrast, invert, neutral point,
  cutoff), scaled, offset (or at random per stroke), combined with each dab by its texturing mode at
  the strength its sensors give, softly where it says.
- Masking brushes: a second brush along the same path, its size a share of the main brush's, its
  coverage combined with the main stroke's by its composite op.
- Colour smudge: each dab moves the paint under where the last lay to where it lies (smearing), or
  that paint made one colour first (dulling, from the share of it its smudge radius says, weighed by
  the tip in the newer engine), the colour mixed in at its colour rate (the gradient option moving
  it towards the secondary colour; an eraser's is clear, taking the paint away), laid as far as its
  smudge rate says (in place of what is there, or over it without smearing alpha), picked up from
  every layer in overlay mode, by the older engine's sums or the newer's; the first dab only marks where the
  next picks up from.
- Other engines: the quick brush (hard round dabs of its diameter and spacing), the tangent normal
  brush (a pixel brush painting the pen's lean, or the stroke's way, or the pen's turn, as a normal
  map's colour, its channels swizzled as it says), the sketch brush (at each move a line from where
  it was, and lines towards the points it passed within its tip's reach, as likely as its density
  and their distance say, offset, magnetised, their colour and opacity at random if it says, each
  line a thin dab) and the spray brush (at each of its dabs its particles, as many as its count or
  as its coverage says, scattered by its distribution, squashed, turned and scaled, each an
  ellipse, a rectangle, a pixel or the brush's own tip, sized, turned and coloured at random as it
  says); the bristle brush (each pixel of its tip a bristle, kept as likely as its density says, as
  long as the tip is opaque there; at each move each bristle a line a pixel wide from where it was,
  or would have been, to where it is, the bristles spread by its scale and size, shaken by its
  randomness, sheared as the pen presses, turned by the rotation option, cut by the pressure's
  threshold, their ink running out pixel by pixel along its curve (soaked up from the colours
  under where the stroke starts if it says), taking their opacity and saturation, by its
  weights or not); the hatching brush (at each dab of its tip, the lines of a hatching anchored to
  the canvas at its origin, a separation apart, as thick as it says, at its angle, crosshatched as
  its style and the crosshatching option say, its separation stepped by the separation option, on
  whole pixels unless it says, over the secondary colour if it says, kept inside the tip); the curve
  brush (a line from where the pen was, and a curve through the points it last passed, at the curves'
  opacity); the particle brush (its particles pulled towards the pen by their pull and gravity, a
  dot for each at each of its steps) and the grid brush (the cells of a grid anchored to the canvas
  within its diameter, divided, inset by their borders, each an ellipse, a box or a line, in the
  colour moved at random or picking up the colour under their middle). As in Krita, the quick brush
  takes only its size and spacing options; the sketch, spray and bristle brushes their size,
  opacity and rotation; the hatching brush its size and opacity besides its own; the curve brush its
  opacity besides its own; the particle and grid brushes none.

The app draws each brush's preview with the plugin, and QuickShape, layers of a plugin's own laying
their strokes again and painting on a 3D model go through it too; brush files the app writes keep
the plugin's data.

The Krita brush studio (the plugin's action, opened by the app's brush studio for its brushes) shows
a brush's Krita settings a page at a time, the pages its engine has: a pixel brush's tip, spacing,
size, opacity and flow, turn and squash, scatter and mirror, colour and texture; a colour smudge
brush's smudge length and mode, smudge radius, colour rate and gradient; a tangent normal brush's
tilt; a sketch brush's lines; a spray brush's area, particles, their dynamics and colours; a quick
brush's diameter and spacing. Each option comes with the sensor it follows and its curve.

The app keeps how each brush looks (its tips drawn, its texture's mask) with the plugin, so only a
brush's first stroke ever waits for them; after that the plugin only reads the brush's settings.
In the app, the app's utilities unpack bundles and decode pictures, much faster than the plugin's own
readers (which its tests use) run in it.

## Not yet

- Krita's other engines come in as the app reads them. Some lay no dabs at all, so a brush engine
  of dabs can't draw them: the shape brush (it fills the outline the stroke makes), the filter brush
  (it filters what is under it), the deform brush (it moves the pixels under it) and the clone brush
  (it copies pixels from elsewhere). The dyna and chalk brushes could be drawn as dabs and aren't
  yet.
- The bristle brush keeps at most 4,096 bristles, draws its lines smooth whatever its antialiasing
  and compositing say, and takes its ink as it runs out in pieces within a 64th of it (Krita at
  each pixel). The hatching brush keeps its lines inside each dab's tip in
  pieces, each as opaque as the tip where it lies, halved down to a pixel where the tip's edge
  crosses them (Krita masks each pixel), so a thick line's end at the edge is cut square across it.
  The curve brush lays a move's line and curve over each
  other; the particle brush lays nothing more while the pen holds still; the grid brush taking the
  colour under its cells lays them in no other colour and without a background.
- The sketch brush draws each move's lines over each other (Krita draws them into one dab first,
  then lays that at the opacity option's), keeps the last 4,096 points it passed, and takes its tip
  as round for its reach; the spray brush's particle pictures, its sampled colours and its
  background fill aren't drawn.
- The lightness and gradient texturing modes take the texture's grey alone (a pattern's
  see-through parts are taken as opaque).
- Colour smudge's paint thickness (lightness tips' height). Its overlay mode picks up from the
  image as last shown, so within one batch of the pen's moves it doesn't see what it has just laid
  (Krita keeps its own copy of the image up to date dab by dab).
- Pictures a preset takes from Krita's own resources without carrying them (nor its bundle): a
  tip of one is Krita's default tip, as Krita stands it in (round, 40 px across, the studio saying
  so); a masking brush or texture of one is left out.
- The 1-pixel brush's sharp lines, drawn as dabs.
- Perspective counts as 1 (the app has no perspective assistant), as in Krita without one; a
  gradient map takes the gradient from the colour to the secondary colour (the app has no current
  gradient).
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
- the texture's mask and its texturing modes; the masking brush and its composite ops;
- the colour smudge brush: its smudge rate, colour rate, smudge radius and gradient options, its
  dulling and smearing modes and how its older and newer engines combine them;
- the quick, tangent normal, sketch, spray, bristle, hatching, curve, particle and grid brushes:
  their settings, and how they lay their dabs, lines, particles and cells.

The file formats (PNG, zip, base64, a small XML reader) are written here from their public
specifications.

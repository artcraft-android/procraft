# Texture Packer

Status: as built, Nov 15, 2026. Ported from Koi Studio's Texture Packer (`SpriteScanner`, `TexturePackerViews`), with a Make2D-native panel.

Share → Texture Packer cuts the separate drawings on the canvas into PNGs of their own, for a game engine's sprites.

- **Finding them** (`make2d_engine::sprite_scan`): the canvas as it shows now (an animation's frame drawn on), without its background, is read once (`CanvasSession::texture_packer_open`). Every pixel more than 8/255 opaque is joined to its four neighbours; each group's box is a drawing, unless both its sides are under 4 pixels (a stray). Boxes fewer than **Separation** clear pixels apart (1 to 40, default 6; at 1 only touching ones, a corner too) are joined, so an i's dot or a bubble's tail stays with its drawing. **Padding** (0 to 32) grows each box, kept on the canvas. Boxes come in reading order: top to bottom, then left to right.
- **Choosing**: the boxes show over the canvas, grey until tapped, blue while chosen; the smallest box under a tap is the one taken. The canvas moves and zooms with a finger or two but isn't drawn on. The sprite tapped last shows on a card with its picture, its number and size, and its name to change (default `Sprite_01`…, in reading order). Separation changes which boxes there are, so it lets go of what was chosen; Padding only grows them, so it keeps it. Names are kept by the box's place.
- **Sharing** (`texture_packer_export`): each chosen sprite is written as `<name>.png` in a folder named after the artwork, then shared together. Pixels are straight alpha, converted to sRGB from the canvas's profile (game engines take a PNG's colours to be sRGB; an HDR canvas's light above white is rolled off first). Names lose what can't be in a file name (`/ \ :`, control characters); an empty one, or one starting with a dot, is its default; one used before gets `_2`, `_3`… (ignoring case), so none is written over.

Desktop and Android have no panel for it yet; the core calls are the same.

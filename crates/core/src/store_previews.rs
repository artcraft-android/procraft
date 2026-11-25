//! Store listing artwork. The package picker prepares brush probes or paper scans in `cover`.
//! The brush specimen pages pair each named brush with an impression on warm paper. The original
//! automatic card and collage entry points also serve callers that do not use the cover picker.

use std::path::Path;
use std::sync::OnceLock;

use image::{Rgba, RgbaImage};
use procraft_engine::brush::Brush;
use procraft_engine::geom::Vec2;
use procraft_engine::input::Sample;
use procraft_engine::zip::ZipReader;

use crate::CanvasSession;
use crate::brushes::Painted;
use crate::text::{TextAlignment, TextInfo};

mod cover;
pub(crate) mod quick_look;
pub use cover::{CoverTemplate, StoreCover, asset_pack_store_cover};
pub(crate) use cover::{plugin_cover, plugin_icon};

/// The picture a preview is: 4:3, the shape the store's cards show, large enough for its band.
pub const PREVIEW_WIDTH: u32 = 1200;
pub const PREVIEW_HEIGHT: u32 = 900;

/// The store's own backdrop, which every picture is laid on: the dark one its sheet has, a shade
/// lighter in the middle so the strokes have somewhere to sit, and a grain over it so it reads
/// as a surface rather than a fill.
const BACKDROP: [u8; 3] = [0x21, 0x21, 0x24];

/// How strongly the backdrop's grain shows, in levels of the 255.
const GRAIN: f32 = 3.5;

/// The ink a set's strokes are drawn in: white, as the app's own library shows a brush's stroke.
const BONE: [u8; 3] = [0xF4, 0xF1, 0xEA];

/// The two families of colour a card's supporting strokes take beside the bone one, by its seed:
/// one warm and one cool, so a card has colour without turning into a swatch chart.
const WARM: [[u8; 3]; 2] = [
    [0xE4, 0xC9, 0xCA], // pale rose
    [0xE7, 0xD1, 0xB8], // pale clay
];
const COOL: [[u8; 3]; 2] = [
    [0xB7, 0xCD, 0xE2], // pale blue
    [0xC7, 0xC5, 0xE0], // pale lavender
];

/// The colours words are set in: a set's name, a brush's name under its stroke, and the dimmer
/// one a page's heading is.
const TITLE_INK: [f32; 3] = [0.94, 0.92, 0.88];
#[cfg(test)]
const CAPTION: [f32; 3] = [0.84, 0.84, 0.86];
const HEADING: [f32; 3] = [0.55, 0.55, 0.58];

/// How much of the picture stays clear around what it holds, as a share of its shorter side.
const MARGIN: f32 = 0.085;

/// The frame the layouts are worked out in (pixels); the picture is drawn at any size from them.
const W: f32 = PREVIEW_WIDTH as f32;
const H: f32 = PREVIEW_HEIGHT as f32;

/// The proportions of a sample stroke's box, from the canvas it is drawn on.
const STROKE_ASPECT: f32 = procraft_engine::preview::CANVAS.1 / procraft_engine::preview::CANVAS.0;

/// The proportions of a collage's pictures: a page, upright.
const PICTURE_ASPECT: f32 = 4.0 / 3.0;

/// How many brushes a card shows at most: a focus, two under it and one across from them.
const MOST_CARD: usize = 4;

/// How many brushes a set of nine or more shows on its card, and how many pictures a pack's
/// collage holds.
pub const MOST_SHOWN: usize = 10;
pub const MOST_PICTURES: usize = 8;

/// The name of the folder entries a set may hold among its brushes (they are not brushes).
const FOLDER_MARK: &str = "\u{1F4C2}";

/// The canvas a brush's sample stroke is drawn on, which is the stroke's own size.
fn stroke_canvas() -> (u32, u32) {
    (procraft_engine::preview::CANVAS.0 as u32, procraft_engine::preview::CANVAS.1 as u32)
}

// ---------- what a brush looks like, and which ones are worth showing ----------

/// A brush's numbers that tell it apart from another at a glance, each 0..1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BrushShape {
    /// How thick its sample stroke is drawn, against the canvas.
    pub size: f32,
    pub hardness: f32,
    pub roundness: f32,
    /// How far its stamps wander about the path.
    pub scatter: f32,
    pub spacing: f32,
    /// Whether it takes a grain, and whether its stamp is an image of its own.
    pub grain: f32,
    pub shape: f32,
    /// Whether it is a dual brush.
    pub dual: f32,
}

impl BrushShape {
    /// How different the two look, 0 for brushes that would draw the same kind of stroke.
    fn distance(&self, other: &BrushShape) -> f32 {
        let pairs = [
            (self.size, other.size),
            (self.hardness, other.hardness),
            (self.roundness, other.roundness),
            (self.scatter, other.scatter),
            (self.spacing, other.spacing),
            (self.grain, other.grain),
            (self.shape, other.shape),
            (self.dual, other.dual),
        ];
        pairs.iter().map(|(a, b)| (a - b) * (a - b)).sum::<f32>().sqrt()
    }
}

/// What a brush's stroke shows of it, as [`BrushShape`] has it.
fn shape_of(brush: &Brush) -> BrushShape {
    let share = |value: f32| if value.is_finite() { value.clamp(0.0, 1.0) } else { 0.0 };
    let takes = |image: Option<u32>, source: bool| if image.is_some() || source { 1.0 } else { 0.0 };
    BrushShape {
        size: share(procraft_engine::preview::size_share(brush)),
        hardness: share(brush.hardness),
        roundness: share(brush.shape.roundness),
        scatter: share(brush.shape.scatter / 2.0 + brush.shape.throw),
        spacing: share(brush.spacing),
        grain: takes(brush.grain.image, brush.grain.source.is_some()),
        shape: takes(brush.shape.image, brush.shape.source.is_some()),
        dual: if brush.secondary.is_some() { 1.0 } else { 0.0 },
    }
}

/// The brushes of `shapes` a preview shows, in the order it shows them: all of them up to
/// [`MOST_SHOWN`], and past that the first, the middle and the last of the set, then whichever
/// ones look least like those already chosen.
pub fn select_brushes(shapes: &[BrushShape], seed: u64) -> Vec<usize> {
    let count = shapes.len();
    if count <= MOST_SHOWN {
        return (0..count).collect();
    }
    let mut chosen = vec![0, count / 2, count - 1];
    let mut rng = Rng::new(seed ^ 0x5B_1E);
    while chosen.len() < MOST_SHOWN {
        let mut best: Option<(usize, f32)> = None;
        for at in 0..count {
            if chosen.contains(&at) {
                continue;
            }
            let Some(shape) = shapes.get(at) else { continue };
            // the nearest of what is chosen already: the least like all of it goes next
            let nearest = chosen
                .iter()
                .filter_map(|&other| shapes.get(other))
                .map(|other| shape.distance(other))
                .fold(f32::MAX, f32::min);
            // (a tie at the end of a set of identical brushes goes to a seeded one)
            let nearest = nearest + rng.around(1e-6);
            if best.is_none_or(|(_, score)| nearest > score) {
                best = Some((at, nearest));
            }
        }
        match best {
            Some((at, _)) => chosen.push(at),
            None => break,
        }
    }
    chosen
}

// ---------- the strokes' arrangement ----------

/// One stroke of a composed picture: which of the shown brushes draws it, where its box sits (as
/// shares of the picture), how it is turned, which ink it takes, whether it is mirrored, and how
/// strongly it is laid down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stroke {
    pub brush: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub angle: f32,
    pub ink: [u8; 3],
    pub flip: bool,
    /// 1 for a stroke laid down whole, less for the soft one under the rest.
    pub opacity: f32,
}

#[cfg(test)]
impl Stroke {
    /// The box this stroke's picture takes up once turned, as (left, top, right, bottom).
    fn bounds(&self) -> (f32, f32, f32, f32) {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let ex = (self.width * cos.abs() + self.height * sin.abs()) / 2.0;
        let ey = (self.width * sin.abs() + self.height * cos.abs()) / 2.0;
        (self.x - ex, self.y - ey, self.x + ex, self.y + ey)
    }
}

/// The clear space a picture keeps at each side, as shares of its width and height.
fn margins() -> (f32, f32) {
    (MARGIN * H / W, MARGIN)
}

/// Moves a stroke's centre in so that its turned box keeps the paper's margin clear.
fn keep_inside(stroke: &mut Stroke) {
    let (mx, my) = margins();
    let (sin, cos) = stroke.angle.to_radians().sin_cos();
    let ex = (stroke.width * cos.abs() + stroke.height * sin.abs()) / 2.0;
    let ey = (stroke.width * sin.abs() + stroke.height * cos.abs()) / 2.0;
    // (one wider than the margins can't be kept clear of both edges, and stays where it is)
    if 2.0 * ex <= 1.0 - 2.0 * mx {
        stroke.x = stroke.x.clamp(mx + ex, 1.0 - mx - ex);
    }
    if 2.0 * ey <= 1.0 - 2.0 * my {
        stroke.y = stroke.y.clamp(my + ey, 1.0 - my - ey);
    }
}

/// Whether every stroke laid down whole stays inside the picture's margins (the soft one under
/// them may run off the edge).
#[cfg(test)]
fn clear(strokes: &[Stroke]) -> bool {
    let (mx, my) = margins();
    strokes.iter().filter(|stroke| stroke.opacity >= 1.0).all(|stroke| {
        let (left, top, right, bottom) = stroke.bounds();
        left >= mx - 1e-4 && top >= my - 1e-4 && right <= 1.0 - mx + 1e-4 && bottom <= 1.0 - my + 1e-4
    })
}

/// A stroke of a card's arrangement, in the frame's own pixels.
fn stroke(brush: usize, x: f32, y: f32, width: f32, angle: f32, ink: [u8; 3], flip: bool) -> Stroke {
    Stroke {
        brush,
        x: x / W,
        y: y / H,
        width: width / W,
        height: width * STROKE_ASPECT / H,
        angle,
        ink,
        flip,
        opacity: 1.0,
    }
}

/// The colour a card's marks take beside its bone stroke, from its seed: a warm ink and a cool one.
fn warm_ink(seed: u64) -> [u8; 3] {
    WARM.get(((seed >> 8) % WARM.len() as u64) as usize).copied().unwrap_or(BONE)
}

fn cool_ink(seed: u64) -> [u8; 3] {
    let seed = seed ^ 0x5EED_0FC0_1D5E;
    COOL.get(((seed >> 8) % COOL.len() as u64) as usize).copied().unwrap_or(BONE)
}

/// The colour a set's strokes are drawn in when the engine can't paint: the warm one.
fn accent_ink(seed: u64) -> [u8; 3] {
    warm_ink(seed)
}

/// Where a set's strokes go, the focus first: a broad stroke under the rest, running off the edge
/// in its own ink, and the set's brushes laid over it down the sheet, the focus largest. Drawn in
/// reverse, the soft one goes down first and the focus last.
pub fn stroke_placements(count: usize, seed: u64) -> Vec<Stroke> {
    let accent = accent_ink(seed);
    let mut rng = Rng::new(seed);
    let at = |rng: &mut Rng, brush: usize, x: f32, y: f32, width: f32, angle: f32, ink: [u8; 3], flip: bool| {
        let (x, y) = (x + rng.around(10.0), y + rng.around(9.0));
        let (width, angle) = (width + rng.around(10.0), angle + rng.around(2.0));
        let mut stroke = stroke(brush, x, y, width, angle, ink, flip);
        keep_inside(&mut stroke);
        stroke
    };
    // the broad one under everything: at the stroke's own size, turned across the frame so it
    // runs off two of its edges
    let ghost =
        Stroke { opacity: 0.42, flip: seed.is_multiple_of(2), ..stroke(0, 540.0, 610.0, 1060.0, -24.0, accent, false) };
    match count {
        0 => Vec::new(),
        1 => {
            let hero = at(&mut rng, 0, 660.0, 392.0, 860.0, 12.0, BONE, false);
            let second = at(&mut rng, 0, 930.0, 640.0, 600.0, -46.0, accent, true);
            vec![hero, second, ghost]
        }
        2 => {
            let hero = at(&mut rng, 1, 660.0, 392.0, 860.0, 12.0, BONE, false);
            vec![hero, ghost]
        }
        3 => {
            let hero = at(&mut rng, 1, 660.0, 392.0, 860.0, 12.0, BONE, false);
            let second = at(&mut rng, 2, 930.0, 640.0, 600.0, -46.0, accent, true);
            vec![hero, second, ghost]
        }
        _ => {
            let hero = at(&mut rng, 1, 660.0, 392.0, 860.0, 12.0, BONE, false);
            let second = at(&mut rng, 2, 930.0, 640.0, 600.0, -46.0, accent, true);
            let third = at(&mut rng, 3, 330.0, 760.0, 320.0, 6.0, accent, false);
            let mut strokes = vec![hero, second, third, ghost];
            strokes.truncate(count.min(MOST_CARD));
            strokes
        }
    }
}

// ---------- a pack's collage ----------

/// One picture of a collage: which of the pack's pictures it shows, where its page sits, and how
/// it is turned.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tile {
    pub picture: usize,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub angle: f32,
}

#[cfg(test)]
impl Tile {
    fn bounds(&self) -> (f32, f32, f32, f32) {
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let ex = (self.width * cos.abs() + self.height * sin.abs()) / 2.0;
        let ey = (self.width * sin.abs() + self.height * cos.abs()) / 2.0;
        (self.x - ex, self.y - ey, self.x + ex, self.y + ey)
    }
}

/// Where a collage's pictures go: the one in focus, then a page at each corner, one in front of
/// the focus and two tucked behind. (Centre, width and turn, as shares of the picture.)
const TILE_SLOTS: [(f32, f32, f32, f32); 8] = [
    (0.54, 0.50, 0.400, -2.5),
    (0.235, 0.31, 0.205, -11.0),
    (0.795, 0.29, 0.180, 12.0),
    (0.240, 0.71, 0.190, 8.0),
    (0.800, 0.70, 0.200, -8.0),
    (0.400, 0.74, 0.150, 5.0),
    (0.645, 0.24, 0.140, -14.0),
    (0.680, 0.75, 0.135, 10.0),
];

/// The pictures a collage of `count` shows, in the order it draws them: the ones behind first, the
/// one in focus over them, the one in front last.
pub fn picture_tiles(count: usize, seed: u64) -> Vec<Tile> {
    let mut rng = Rng::new(seed ^ 0x0C01_1A6E);
    let mut tiles: Vec<Tile> = TILE_SLOTS
        .iter()
        .enumerate()
        .take(count.min(TILE_SLOTS.len()))
        .map(|(picture, &(x, y, width, angle))| {
            let width = width * W + rng.around(6.0);
            Tile {
                picture,
                x: (x * W + rng.around(7.0)) / W,
                y: (y * H + rng.around(6.0)) / H,
                width: width / W,
                height: width * PICTURE_ASPECT / H,
                angle: angle + rng.around(2.0),
            }
        })
        .collect();
    tiles.sort_by_key(|tile| match tile.picture {
        0 => 1,
        5 => 2,
        _ => 0,
    });
    tiles
}

/// Whether `name` is a picture a collage can show.
fn is_picture(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    [".png", ".jpg", ".jpeg"].iter().any(|end| lower.ends_with(end))
}

/// Whether `name` is the thumbnail a pack keeps of an asset.
fn is_thumbnail(name: &str) -> bool {
    name.rsplit('/').next().is_some_and(|file| {
        let lower = file.to_ascii_lowercase();
        lower == "thumbnail.png" || lower == "thumbnail.jpg" || lower == "thumbnail.jpeg"
    })
}

/// Whether `name` is the first of an asset's pictures, which is the colour a paper is made of.
fn is_first_picture(name: &str) -> bool {
    name.rsplit_once("/layers/").is_some_and(|(_, file)| {
        let lower = file.to_ascii_lowercase();
        lower == "1.png" || lower == "1.jpg" || lower == "1.jpeg"
    })
}

/// The folder an asset's picture is in: a pack shows one picture of each.
fn folder_of(name: &str) -> &str {
    let folder = name.rsplit_once('/').map_or("", |(folder, _)| folder);
    folder.strip_suffix("/layers").unwrap_or(folder)
}

/// Which of a pack's pictures `names` (its picture entries, in the archive's own order) the
/// collage shows, at most `most` of them: one thumbnail an asset where the pack has them, else one
/// picture an asset, else whatever is left. Sorted by name, so the same pack always shows the same
/// pictures in the same order.
pub fn pack_picture_choices(names: &[String], most: usize) -> Vec<usize> {
    let mut sorted: Vec<usize> = (0..names.len()).filter(|&at| names.get(at).is_some_and(|n| is_picture(n))).collect();
    sorted.sort_by(|&a, &b| names.get(a).cmp(&names.get(b)));
    let mut chosen: Vec<usize> = Vec::new();
    collect_pictures(names, &sorted, &mut chosen, most, is_thumbnail);
    collect_pictures(names, &sorted, &mut chosen, most, is_first_picture);
    // a pack whose pictures are named some other way still shows them
    collect_pictures(names, &sorted, &mut chosen, most, |_| true);
    chosen
}

/// Adds to `chosen`, up to `most`, one picture an asset that `want` takes.
fn collect_pictures(names: &[String], sorted: &[usize], chosen: &mut Vec<usize>, most: usize, want: fn(&str) -> bool) {
    let mut shown: Vec<&str> = chosen.iter().filter_map(|&at| names.get(at).map(|name| folder_of(name))).collect();
    for &at in sorted {
        if chosen.len() >= most {
            return;
        }
        if chosen.contains(&at) {
            continue;
        }
        let Some(name) = names.get(at).map(String::as_str) else { continue };
        if !want(name) {
            continue;
        }
        let folder = folder_of(name);
        if shown.contains(&folder) {
            continue;
        }
        shown.push(folder);
        chosen.push(at);
    }
}

// ---------- pixels on the paper ----------

/// A source's pixels, premultiplied RGBA8, for reading between them as it is drawn.
struct Matte {
    width: usize,
    height: usize,
    pixels: Vec<[u8; 4]>,
}

impl Matte {
    /// `image`'s pixels, which are premultiplied already (a sample stroke off the canvas).
    fn kept(image: &RgbaImage) -> Matte {
        Matte::of(image, false)
    }

    /// `image`'s straight colours premultiplied (a pack's pictures).
    fn straight(image: &RgbaImage) -> Matte {
        Matte::of(image, true)
    }

    fn of(image: &RgbaImage, straight: bool) -> Matte {
        let pixels = image
            .as_raw()
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&[r, g, b, a]| {
                if !straight {
                    return [r, g, b, a];
                }
                let over = |c: u8| ((u16::from(c) * u16::from(a) + 127) / 255) as u8;
                [over(r), over(g), over(b), a]
            })
            .collect();
        Matte { width: image.width() as usize, height: image.height() as usize, pixels }
    }

    /// Whether every pixel is white where it shows: a stroke the canvas drew, which takes an ink.
    fn white(&self) -> bool {
        self.pixels.iter().all(|&[r, g, b, a]| [r, g, b].iter().all(|&c| c.abs_diff(a) <= 2))
    }

    fn pixel(&self, x: isize, y: isize) -> [f32; 4] {
        if x < 0 || y < 0 || x >= self.width as isize || y >= self.height as isize {
            return [0.0; 4];
        }
        let at = y as usize * self.width + x as usize;
        self.pixels.get(at).map_or([0.0; 4], |&[r, g, b, a]| [r.into(), g.into(), b.into(), a.into()])
    }

    /// The pixels at (`x`, `y`), read between them: premultiplied colour and alpha, 0..255.
    fn sample(&self, x: f32, y: f32) -> [f32; 4] {
        let x = x - 0.5;
        let y = y - 0.5;
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let (x0, y0) = (x0 as isize, y0 as isize);
        let along = |a: [f32; 4], b: [f32; 4], t: f32| {
            let mut out = [0.0; 4];
            for (at, (a, b)) in out.iter_mut().zip(a.iter().zip(b)) {
                *at = a + (b - a) * t;
            }
            out
        };
        let top = along(self.pixel(x0, y0), self.pixel(x0 + 1, y0), fx);
        let bottom = along(self.pixel(x0, y0 + 1), self.pixel(x0 + 1, y0 + 1), fx);
        along(top, bottom, fy)
    }
}

/// How rounded a page's corners are, as a share of the picture's shorter side; the paper margin a
/// collage's page keeps around its picture; and how hard a page's shadow falls.
const PAGE_CORNER: f32 = 0.012;
const PAGE_MARGIN: f32 = 0.0055;
const PAGE_SHADOW: f32 = 0.22;
const STROKE_SHADOW: f32 = 0.15;
/// How far a shadow spreads, and how far down the picture it falls, as shares of the picture.
const SHADOW_BLUR: f32 = 0.015;
const SHADOW_DOWN: (f32, f32) = (0.005, 0.009);

/// A stroke whose brush could not draw one: the app's own wave, drawn as a mask, so a picture is
/// still made.
fn wave(width: usize, height: usize) -> RgbaImage {
    let mut image = RgbaImage::new(width as u32, height as u32);
    let (w, h) = (width.max(2) as f32, height.max(2) as f32);
    for y in 0..height {
        for x in 0..width {
            let along = (x as f32 + 0.5) / w;
            let taper = smoothstep(0.0, 0.12, along) * smoothstep(0.0, 0.12, 1.0 - along);
            let swell = 0.55 + 0.45 * (along * std::f32::consts::PI).sin();
            let middle = h / 2.0 + h * 0.16 * (along * 6.6).sin();
            let reach = h * 0.11 * swell;
            let across = ((y as f32 + 0.5 - middle) / reach).abs();
            let edge = (1.0 - across * across).clamp(0.0, 1.0);
            let alpha = (taper * edge * 235.0).round().clamp(0.0, 255.0) as u8;
            image.put_pixel(x as u32, y as u32, Rgba([alpha, alpha, alpha, alpha]));
        }
    }
    image
}

/// One thing drawn on a picture: a source's pixels or a flat shape, in a box turned by an angle,
/// its corners rounded, a soft shadow under it.
struct Layer<'a> {
    source: Option<&'a Matte>,
    /// A white source is drawn in this ink; any other keeps its own colours.
    tint: Option<[u8; 3]>,
    /// What a flat shape is filled with.
    fill: [u8; 4],
    cx: f32,
    cy: f32,
    hw: f32,
    hh: f32,
    angle: f32,
    corner: f32,
    shadow: f32,
    /// How strongly it is laid down, 1 whole.
    opacity: f32,
}

impl Layer<'_> {
    /// The ink a white source is drawn in, worked out once: a source of its own colours keeps them.
    fn marked(&self) -> Option<[u8; 3]> {
        self.tint.filter(|_| self.source.is_none_or(Matte::white))
    }

    /// The premultiplied pixel at a point of the layer's own box.
    fn at(&self, lx: f32, ly: f32, tint: Option<[u8; 3]>) -> [f32; 4] {
        let inside = rounded(lx, ly, self.hw, self.hh, self.corner);
        if inside <= 0.0 {
            return [0.0; 4];
        }
        let Some(source) = self.source else {
            let [r, g, b, a] = self.fill.map(f32::from);
            return [r * inside, g * inside, b * inside, a * inside];
        };
        let (sw, sh) = (source.width as f32, source.height as f32);
        let sample = source.sample((lx + self.hw) / (2.0 * self.hw) * sw, (ly + self.hh) / (2.0 * self.hh) * sh);
        let ink = match tint {
            Some(ink) => {
                let alpha = sample[3] / 255.0;
                [f32::from(ink[0]) * alpha, f32::from(ink[1]) * alpha, f32::from(ink[2]) * alpha, sample[3]]
            }
            None => sample,
        };
        let mut out = [0.0; 4];
        for (at, value) in out.iter_mut().zip(ink) {
            *at = value * inside;
        }
        if self.opacity < 1.0 {
            for value in &mut out {
                *value *= self.opacity;
            }
        }
        out
    }

    /// Draws it over `picture`.
    fn draw(&self, picture: &mut RgbaImage) {
        let tint = self.marked();
        let (sin, cos) = self.angle.to_radians().sin_cos();
        let ex = self.hw * cos.abs() + self.hh * sin.abs();
        let ey = self.hw * sin.abs() + self.hh * cos.abs();
        let blur = SHADOW_BLUR * picture.height() as f32;
        let reach = if self.shadow > 0.0 { blur * 2.0 } else { 0.0 };
        if self.shadow > 0.0 {
            self.under(picture, (sin, cos), tint, ex, ey, blur);
        }
        let width = picture.width() as usize;
        let height = picture.height() as usize;
        let x0 = (self.cx - ex - reach).floor().max(0.0) as usize;
        let y0 = (self.cy - ey - reach).floor().max(0.0) as usize;
        let x1 = (self.cx + ex + reach).ceil().min(width as f32).max(0.0) as usize;
        let y1 = (self.cy + ey + reach).ceil().min(height as f32).max(0.0) as usize;
        let pixels = picture.as_mut().as_chunks_mut::<4>().0;
        for y in y0..y1 {
            for x in x0..x1 {
                let dx = x as f32 + 0.5 - self.cx;
                let dy = y as f32 + 0.5 - self.cy;
                let pixel = self.at(dx * cos + dy * sin, -dx * sin + dy * cos, tint);
                if pixel[3] <= 0.0 {
                    continue;
                }
                let Some(dst) = pixels.get_mut(y * width + x) else { continue };
                let over = 1.0 - pixel[3] / 255.0;
                for at in 0..3 {
                    let under = dst.get(at).copied().map(f32::from).unwrap_or(0.0);
                    if let Some(slot) = dst.get_mut(at) {
                        let value = pixel.get(at).copied().unwrap_or(0.0) + under * over;
                        *slot = value.round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }

    /// The shadow it casts: its coverage a quarter of the picture across and blurred, laid under
    /// it, a little down and to the right.
    fn under(
        &self,
        picture: &mut RgbaImage,
        (sin, cos): (f32, f32),
        tint: Option<[u8; 3]>,
        ex: f32,
        ey: f32,
        blur: f32,
    ) {
        let scale = if picture.height() >= 400 { 0.25 } else { 1.0 };
        let size = picture.height() as f32 / H;
        let offset = (SHADOW_DOWN.0 * W * size, SHADOW_DOWN.1 * H * size);
        let (left, top) = (self.cx - ex - blur, self.cy - ey - blur);
        let across = ((2.0 * (ex + blur)) * scale).ceil().max(1.0) as usize;
        let down = ((2.0 * (ey + blur)) * scale).ceil().max(1.0) as usize;
        let mut mask = vec![0.0f32; across * down];
        for y in 0..down {
            for x in 0..across {
                let dx = left + (x as f32 + 0.5) / scale - self.cx;
                let dy = top + (y as f32 + 0.5) / scale - self.cy;
                let alpha = self.at(dx * cos + dy * sin, -dx * sin + dy * cos, tint)[3] / 255.0;
                if let Some(cell) = mask.get_mut(y * across + x) {
                    *cell = alpha;
                }
            }
        }
        let radius = (blur * scale).round().max(0.0) as usize;
        box_blur(&mut mask, across, down, radius);
        let width = picture.width() as usize;
        let height = picture.height() as usize;
        let x0 = (left + offset.0 - blur).floor().max(0.0) as usize;
        let y0 = (top + offset.1 - blur).floor().max(0.0) as usize;
        let x1 = (left + offset.0 + 2.0 * ex + blur).ceil().min(width as f32).max(0.0) as usize;
        let y1 = (top + offset.1 + 2.0 * ey + blur).ceil().min(height as f32).max(0.0) as usize;
        let pixels = picture.as_mut().as_chunks_mut::<4>().0;
        for y in y0..y1 {
            for x in x0..x1 {
                let u = (x as f32 + 0.5 - offset.0 - left) * scale - 0.5;
                let v = (y as f32 + 0.5 - offset.1 - top) * scale - 0.5;
                let alpha = sample_grid(&mask, across, down, u, v) * self.shadow;
                let Some(dst) = pixels.get_mut(y * width + x) else { continue };
                let light = (1.0 - alpha).clamp(0.0, 1.0);
                for at in 0..3 {
                    if let Some(slot) = dst.get_mut(at) {
                        *slot = (f32::from(*slot) * light).round().clamp(0.0, 255.0) as u8;
                    }
                }
            }
        }
    }
}

/// How much of a box's pixel at (`lx`, `ly`) is inside its rounded corners, 0..1.
fn rounded(lx: f32, ly: f32, hw: f32, hh: f32, corner: f32) -> f32 {
    let reach = corner.clamp(0.0, hw.min(hh));
    let dx = lx.abs() - (hw - reach);
    let dy = ly.abs() - (hh - reach);
    let cornered = (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.min(dy).max(0.0) - reach;
    // (the last pixel of an edge is feathered rather than cut square)
    (0.5 - cornered.max(lx.abs() - hw).max(ly.abs() - hh)).clamp(0.0, 1.0)
}

/// `mask` (0..1, `width` across) as it is `radius` cells out from every cell, twice over.
fn box_blur(mask: &mut [f32], width: usize, height: usize, radius: usize) {
    if radius == 0 || width == 0 || height == 0 {
        return;
    }
    let mut pass = vec![0.0f32; mask.len()];
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            let mut cells = 0.0;
            for step in -(radius as isize)..=(radius as isize) {
                let from = (x as isize + step).clamp(0, width as isize - 1) as usize;
                sum += mask.get(y * width + from).copied().unwrap_or(0.0);
                cells += 1.0;
            }
            if let Some(cell) = pass.get_mut(y * width + x) {
                *cell = sum / cells;
            }
        }
    }
    for y in 0..height {
        for x in 0..width {
            let mut sum = 0.0;
            let mut cells = 0.0;
            for step in -(radius as isize)..=(radius as isize) {
                let from = (y as isize + step).clamp(0, height as isize - 1) as usize;
                sum += pass.get(from * width + x).copied().unwrap_or(0.0);
                cells += 1.0;
            }
            if let Some(cell) = mask.get_mut(y * width + x) {
                *cell = sum / cells;
            }
        }
    }
}

/// `grid` (0..1 values, `width` across) read between its cells at (`x`, `y`).
fn sample_grid(grid: &[f32], width: usize, height: usize, x: f32, y: f32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |x: isize, y: isize| {
        if x < 0 || y < 0 || x >= width as isize || y >= height as isize {
            return 0.0;
        }
        grid.get(y as usize * width + x as usize).copied().unwrap_or(0.0)
    };
    let (x0, y0) = (x0 as isize, y0 as isize);
    let top = at(x0, y0) * (1.0 - fx) + at(x0 + 1, y0) * fx;
    let bottom = at(x0, y0 + 1) * (1.0 - fx) + at(x0 + 1, y0 + 1) * fx;
    top * (1.0 - fy) + bottom * fy
}

/// A smooth step from 0 at `edge0` to 1 at `edge1`.
fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The store's dark backdrop in the frame a picture is, lifted a shade towards the middle so the
/// strokes have somewhere to sit; `grained` gives it the grain of a surface, for a picture that is
/// mostly backdrop (the grain is most of what a page of strokes would weigh, so they go without).
fn backdrop(size: (u32, u32), grained: bool) -> RgbaImage {
    let (width, height) = (size.0.max(1), size.1.max(1));
    let mut picture = RgbaImage::from_pixel(width, height, Rgba([BACKDROP[0], BACKDROP[1], BACKDROP[2], 255]));
    let (cx, cy) = (width as f32 / 2.0, height as f32 / 2.0);
    let far = (cx * cx + cy * cy).sqrt().max(1.0);
    let pixels = picture.as_mut().as_chunks_mut::<4>().0;
    for y in 0..height as usize {
        for x in 0..width as usize {
            let dx = x as f32 + 0.5 - cx;
            let dy = y as f32 + 0.5 - cy;
            let far_off = smoothstep(0.1, 1.0, (dx * dx + dy * dy).sqrt() / far);
            let grain = if grained { grain(x as u32, y as u32) } else { 0.0 };
            let light = 8.0 * (1.0 - far_off) - 4.0 * far_off + grain;
            let Some(pixel) = pixels.get_mut(y * width as usize + x) else { continue };
            for channel in pixel.iter_mut().take(3) {
                *channel = (f32::from(*channel) + light).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    picture
}

/// The backdrop's grain at (`x`, `y`), from -half to half of [`GRAIN`]: the same everywhere.
fn grain(x: u32, y: u32) -> f32 {
    let mut hash = (u64::from(x) << 32) ^ u64::from(y) ^ 0x9E37_79B9_7F4A_7C15;
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xFF51_AFD7_ED55_8CCD);
    hash ^= hash >> 33;
    hash = hash.wrapping_mul(0xC4CE_B9FE_1A85_EC53);
    hash ^= hash >> 33;
    ((hash % 1000) as f32 / 1000.0 - 0.5) * GRAIN
}

/// `strokes` (sample strokes, white on clear) composed into a picture on the backdrop: what a set's
/// card is drawn from when the brushes can't paint one.
pub fn strokes_picture(strokes: &[RgbaImage], seed: u64, size: (u32, u32)) -> RgbaImage {
    let plan = stroke_placements(strokes.len(), seed);
    let mut picture = backdrop(size, true);
    for placed in plan.iter().rev() {
        let Some(stroke) = strokes.get(placed.brush) else { continue };
        lay_stroke(&mut picture, stroke, placed, placed.ink);
    }
    picture
}

/// The picture a pack's listing shows: its pictures as a collage, its name written along the
/// bottom.
pub fn pack_card(pictures: &[RgbaImage], seed: u64, name: &str, size: (u32, u32)) -> RgbaImage {
    let mut picture = pictures_picture(pictures, seed, size);
    subtitle(&mut picture, name, "ASSET PACK");
    picture
}

// ---------- the card's painting ----------

/// The shape a gesture's pen takes along its run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathShape {
    /// A long stroke bowing across its run.
    Swash,
    /// A coil, the pen going round as it travels.
    Loop,
    /// A wave, the pen crossing its run a few times.
    Wave,
    /// A dab, short enough that a stamp brush shows its own tip.
    Dab,
}

/// One stroke of a card's painting: which of the shown brushes takes it, which of the card's three
/// inks, the line it runs along from its start to its end, and how it is drawn.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gesture {
    pub brush: usize,
    /// 0 for the bone ink, 1 for the card's warm one, 2 for its cool one.
    pub hue: u8,
    pub shape: PathShape,
    pub from: (f32, f32),
    pub to: (f32, f32),
    /// How far it leaves the straight run: across it for a swash or a wave, around it for a loop.
    pub bow: f32,
    /// How thick it is drawn, against the brush's own preview size.
    pub weight: f32,
    /// How many times a wave crosses its run.
    pub turns: f32,
    /// Whether its pressure falls away from its start rather than swelling in the middle.
    pub fade: bool,
}

impl Gesture {
    /// The pen's path along it, `steps` of them, pressed as the brush's preview is pressed.
    fn samples(&self, brush: &Brush, steps: usize) -> Vec<Sample> {
        let (ax, ay) = self.from;
        let (bx, by) = self.to;
        let (dx, dy) = (bx - ax, by - ay);
        let run = (dx * dx + dy * dy).sqrt().max(1.0);
        // across the run: where a swash bows out and a wave swings
        let (across_x, across_y) = (-dy / run, dx / run);
        let steps = steps.max(2);
        (0..steps)
            .map(|i| {
                let t = i as f32 / (steps - 1) as f32;
                let (x, y) = match self.shape {
                    PathShape::Swash => {
                        let bow = self.bow * 4.0 * t * (1.0 - t);
                        (ax + dx * t + across_x * bow, ay + dy * t + across_y * bow)
                    }
                    PathShape::Loop => {
                        let reach = self.bow.abs().max(28.0);
                        let angle = std::f32::consts::TAU * 1.1 * t + 0.6;
                        (ax + dx * t + reach * angle.cos(), ay + dy * t + reach * angle.sin())
                    }
                    PathShape::Wave => {
                        let swing = (t * self.turns.max(1.0) * std::f32::consts::TAU).sin() * self.bow;
                        (ax + dx * t + across_x * swing, ay + dy * t + across_y * swing)
                    }
                    PathShape::Dab => (ax + dx * t, ay + dy * t),
                };
                let pressure = if self.fade {
                    (1.0 - t).powf(0.75).clamp(0.04, 1.0)
                } else {
                    procraft_engine::preview::pressure_at(brush, t)
                };
                Sample::at(Vec2::new(x, y), pressure, i as f64 * 4.0)
            })
            .collect()
    }

    /// The diameter its brush is drawn at, against the brush's own preview size.
    fn diameter(&self, brush: &Brush) -> f32 {
        (procraft_engine::preview::diameter(brush) * self.weight).clamp(2.0, H / 3.5)
    }
}

/// A place and a manner for one stroke of a card: the first is the focus, the rest support it, and
/// between them they run across the frame and back.
struct Slot {
    shape: PathShape,
    hue: u8,
    from: (f32, f32),
    to: (f32, f32),
    bow: f32,
    weight: f32,
    turns: f32,
    fade: bool,
}

/// The twelve strokes a card can be painted with, in the order they are worth adding: a broad swash
/// across it first, then a crossing stroke, a coil and a wave, and on down to thin scratches and
/// dabs. Every stroke after the focus is a weight and a shape of its own.
#[rustfmt::skip]
const SLOTS: [Slot; 12] = [
    // the focus: a heavy swash up across the card
    Slot { shape: PathShape::Swash, hue: 0, from: (120.0, 770.0), to: (1080.0, 340.0), bow: -120.0, weight: 1.7, turns: 0.0, fade: false },
    // a mid stroke crossing it the other way
    Slot { shape: PathShape::Swash, hue: 2, from: (912.0, 780.0), to: (228.0, 288.0), bow: 108.0, weight: 0.9, turns: 0.0, fade: true },
    // a warm coil over the middle
    Slot { shape: PathShape::Loop, hue: 1, from: (556.0, 640.0), to: (880.0, 560.0), bow: 78.0, weight: 0.6, turns: 0.0, fade: false },
    // a cool wave down the left
    Slot { shape: PathShape::Wave, hue: 2, from: (232.0, 700.0), to: (572.0, 320.0), bow: 52.0, weight: 0.55, turns: 2.6, fade: false },
    // a warm thin scratch right across
    Slot { shape: PathShape::Swash, hue: 1, from: (156.0, 438.0), to: (1058.0, 548.0), bow: 36.0, weight: 0.4, turns: 0.0, fade: false },
    // a cool dab above it
    Slot { shape: PathShape::Dab, hue: 2, from: (428.0, 272.0), to: (472.0, 260.0), bow: 10.0, weight: 0.7, turns: 0.0, fade: true },
    // a bone mid swash under the focus
    Slot { shape: PathShape::Swash, hue: 0, from: (322.0, 656.0), to: (836.0, 296.0), bow: -66.0, weight: 0.85, turns: 0.0, fade: false },
    // a small cool coil at the right
    Slot { shape: PathShape::Loop, hue: 2, from: (902.0, 330.0), to: (1076.0, 432.0), bow: 54.0, weight: 0.5, turns: 0.0, fade: false },
    // a warm thin wave along the foot
    Slot { shape: PathShape::Wave, hue: 1, from: (700.0, 812.0), to: (1064.0, 706.0), bow: 38.0, weight: 0.5, turns: 1.9, fade: true },
    // a warm dab beyond it
    Slot { shape: PathShape::Dab, hue: 1, from: (978.0, 764.0), to: (1014.0, 776.0), bow: 8.0, weight: 0.65, turns: 0.0, fade: true },
    // a cool sweep up the middle, crossing the focus
    Slot { shape: PathShape::Swash, hue: 2, from: (556.0, 776.0), to: (632.0, 296.0), bow: -76.0, weight: 0.7, turns: 0.0, fade: false },
    // a bone dab in the middle of them
    Slot { shape: PathShape::Dab, hue: 0, from: (600.0, 506.0), to: (640.0, 514.0), bow: 8.0, weight: 0.55, turns: 0.0, fade: true },
];

/// How many strokes a card of `count` brushes is painted with: more brushes, more of the frame
/// covered, up to the twelve there are places for.
fn gesture_count(count: usize) -> usize {
    match count {
        0 | 1 => 3,
        2 => 4,
        3 => 5,
        4..=5 => 6,
        6..=7 => 7,
        8..=10 => 8,
        11..=20 => 9,
        21..=48 => 10,
        _ => 12,
    }
}

/// How a card of `count` brushes is painted: [`gesture_count`] strokes, the first the focus, from
/// the set's `shown` brushes (each once, then round again as the strokes run out of brushes), each
/// along a path of its own, jittered by the seed. The ink runs bone, warm and cool in turn after
/// the focus.
pub fn card_gestures(count: usize, shown: usize, seed: u64) -> Vec<Gesture> {
    let mut rng = Rng::new(seed ^ 0x00CA_2D1E);
    let shown = shown.max(1);
    SLOTS
        .iter()
        .take(gesture_count(count).min(SLOTS.len()))
        .enumerate()
        .map(|(at, slot)| {
            let sway = |rng: &mut Rng, v: f32, by: f32| v + rng.around(by);
            let mut gesture = Gesture {
                brush: at % shown,
                hue: slot.hue,
                shape: slot.shape,
                from: (sway(&mut rng, slot.from.0, 8.0), sway(&mut rng, slot.from.1, 8.0)),
                to: (sway(&mut rng, slot.to.0, 8.0), sway(&mut rng, slot.to.1, 8.0)),
                bow: sway(&mut rng, slot.bow, 8.0),
                weight: (sway(&mut rng, slot.weight, 0.08)).max(0.25),
                turns: sway(&mut rng, slot.turns, 0.15),
                fade: slot.fade,
            };
            if gesture.shape == PathShape::Dab {
                gesture.bow = gesture.bow.abs();
            }
            gesture
        })
        .collect()
}

/// The ink a gesture paints with, sRGB 0..1.
fn gesture_ink(gesture: &Gesture, warm: [u8; 3], cool: [u8; 3]) -> [f32; 3] {
    let ink = match gesture.hue {
        1 => warm,
        2 => cool,
        _ => BONE,
    };
    ink.map(|channel| f32::from(channel) / 255.0)
}

/// `painted` laid over `picture`, which is its size.
fn over(picture: &mut RgbaImage, painted: &RgbaImage) {
    let under = picture.as_mut().as_chunks_mut::<4>().0;
    let over = painted.as_raw().as_chunks::<4>().0;
    for (dst, src) in under.iter_mut().zip(over) {
        let alpha = f32::from(src[3]) / 255.0;
        if alpha <= 0.0 {
            continue;
        }
        for at in 0..3 {
            let above = f32::from(src.get(at).copied().unwrap_or(0));
            let below = f32::from(dst.get(at).copied().unwrap_or(0)) * (1.0 - alpha);
            if let Some(slot) = dst.get_mut(at) {
                *slot = (above + below).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
}

/// An item's name written along a picture's bottom, in the dim heading colour, with the short line
/// about it (its brush count, or what it is) across from it.
fn subtitle(picture: &mut RgbaImage, name: &str, kind: &str) {
    let (width, height) = (picture.width() as f32, picture.height() as f32);
    let margin = 0.047 * width;
    let size = 0.019 * height;
    let top = 0.952 * height;
    write_words(picture, name, size, HEADING, margin, top, true, width - 2.0 * margin - 140.0);
    let kind_width = words_width(kind, size, true);
    write_words(picture, kind, size, HEADING, width - margin - kind_width, top, true, 140.0);
}

/// A set's card headed as a specimen sheet is: a small tracked kicker line with the count across
/// from it, the set's name under it, and a hairline where the strokes begin.
fn masthead(picture: &mut RgbaImage, kicker: &str, name: &str, right: &str) {
    let (width, height) = (picture.width() as f32, picture.height() as f32);
    let margin = 0.075 * width;
    let small = 0.0165 * height;
    write_words(picture, kicker, small, HEADING, margin, 0.040 * height, true, 0.4 * width);
    let right_width = words_width(right, small, true);
    write_words(picture, right, small, HEADING, width - margin - right_width, 0.040 * height, true, 0.4 * width);
    let most = width - 2.0 * margin;
    let wanted = 0.062 * height;
    let measured = words_width(name, wanted, false);
    let size = if measured > most && measured > 0.0 { (wanted * most / measured).max(0.030 * height) } else { wanted };
    write_words(picture, name, size, TITLE_INK, margin, 0.088 * height, false, most);
    Layer {
        source: None,
        tint: None,
        fill: [70, 70, 74, 255],
        cx: width / 2.0,
        cy: 0.190 * height,
        hw: width / 2.0 - margin,
        hh: 0.5,
        angle: 0.0,
        corner: 0.0,
        shadow: 0.0,
        opacity: 1.0,
    }
    .draw(picture);
}

/// One stroke fitted into its box on `picture`, turned and inked.
fn lay_stroke(picture: &mut RgbaImage, stroke: &RgbaImage, placed: &Stroke, ink: [u8; 3]) {
    let (width, height) = (picture.width() as f32, picture.height() as f32);
    let (cx, cy) = (placed.x * width, placed.y * height);
    let (bw, bh) = (placed.width * width, placed.height * height);
    lay_fitted(picture, stroke, cx, cy, bw, bh, placed.angle, placed.flip, ink, STROKE_SHADOW, placed.opacity);
}

/// `stroke` as big as fits in `bw` × `bh` about (`cx`, `cy`), keeping its own shape, turned
/// `angle`, drawn in `ink` `opacity` of the way; false if it shows nothing.
fn lay_fitted(
    picture: &mut RgbaImage,
    stroke: &RgbaImage,
    cx: f32,
    cy: f32,
    bw: f32,
    bh: f32,
    angle: f32,
    flip: bool,
    ink: [u8; 3],
    shadow: f32,
    opacity: f32,
) -> bool {
    let Some(stroke) = trimmed(stroke) else { return false };
    let (sw, sh) = (stroke.width().max(1) as f32, stroke.height().max(1) as f32);
    let scale = (bw / sw).min(bh / sh);
    if !scale.is_finite() || scale <= 0.0 {
        return false;
    }
    let (dw, dh) = ((sw * scale).round().max(1.0) as u32, (sh * scale).round().max(1.0) as u32);
    let fitted = image::imageops::resize(&stroke, dw, dh, image::imageops::FilterType::Triangle);
    let fitted = if flip { image::imageops::flip_horizontal(&fitted) } else { fitted };
    let matte = Matte::kept(&fitted);
    Layer {
        source: Some(&matte),
        tint: Some(ink),
        fill: [255, 255, 255, 255],
        cx,
        cy,
        hw: dw as f32 / 2.0,
        hh: dh as f32 / 2.0,
        angle,
        corner: 0.0,
        shadow,
        opacity,
    }
    .draw(picture);
    true
}

/// `image` without the clear space around the stroke in it, a little kept so its edges fade: none
/// for a stroke that shows nothing.
fn trimmed(image: &RgbaImage) -> Option<RgbaImage> {
    let (width, height) = (image.width(), image.height());
    let pixels = image.as_raw().as_chunks::<4>().0;
    let mut bounds: Option<(u32, u32, u32, u32)> = None;
    for y in 0..height {
        for x in 0..width {
            if pixels.get((y * width + x) as usize).map_or(0, |pixel| pixel[3]) <= 2 {
                continue;
            }
            bounds = Some(match bounds {
                Some((left, top, right, bottom)) => (left.min(x), top.min(y), right.max(x), bottom.max(y)),
                None => (x, y, x, y),
            });
        }
    }
    let (left, top, right, bottom) = bounds?;
    let pad = 2;
    let left = left.saturating_sub(pad);
    let top = top.saturating_sub(pad);
    let right = (right + pad).min(width - 1);
    let bottom = (bottom + pad).min(height - 1);
    Some(image::imageops::crop_imm(image, left, top, right - left + 1, bottom - top + 1).to_image())
}

/// `pictures` (a pack's own, in the order they are shown) composed into the collage its listing
/// shows: prints with rounded corners and a paper margin, overlapping, the first in focus.
pub fn pictures_picture(pictures: &[RgbaImage], seed: u64, size: (u32, u32)) -> RgbaImage {
    let tiles = picture_tiles(pictures.len(), seed);
    let mut picture = backdrop(size, false);
    let (width, height) = (size.0 as f32, size.1 as f32);
    let short = width.min(height);
    let corner = PAGE_CORNER * short;
    let margin = (PAGE_MARGIN * short).max(1.0);
    for tile in &tiles {
        let Some(source) = pictures.get(tile.picture) else { continue };
        let (bw, bh) = ((tile.width * width).round().max(2.0), (tile.height * height).round().max(2.0));
        let (cx, cy) = (tile.x * width, tile.y * height);
        Layer {
            source: None,
            tint: None,
            fill: [31, 31, 34, 255],
            cx,
            cy,
            hw: bw / 2.0 + margin,
            hh: bh / 2.0 + margin,
            angle: tile.angle,
            corner: corner + margin,
            shadow: PAGE_SHADOW,
            opacity: 1.0,
        }
        .draw(&mut picture);
        let cover = cover_crop(source, bw as u32, bh as u32);
        let matte = Matte::straight(&cover);
        Layer {
            source: Some(&matte),
            tint: None,
            fill: [255, 255, 255, 255],
            cx,
            cy,
            hw: bw / 2.0,
            hh: bh / 2.0,
            angle: tile.angle,
            corner,
            shadow: 0.0,
            opacity: 1.0,
        }
        .draw(&mut picture);
    }
    picture
}

/// `source` cut to the middle of it at `width` × `height` proportions, then scaled to that size.
fn cover_crop(source: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let (sw, sh) = (source.width().max(1), source.height().max(1));
    let (want_w, want_h) = (width.max(1), height.max(1));
    let scale = (sw as f32 / want_w as f32).min(sh as f32 / want_h as f32);
    let cut_w = (want_w as f32 * scale).round().clamp(1.0, sw as f32) as u32;
    let cut_h = (want_h as f32 * scale).round().clamp(1.0, sh as f32) as u32;
    let cut = image::imageops::crop_imm(source, (sw - cut_w) / 2, (sh - cut_h) / 2, cut_w, cut_h).to_image();
    image::imageops::resize(&cut, want_w, want_h, image::imageops::FilterType::Triangle)
}

/// The picture shown when nothing can be drawn for an item: the app's own sample stroke on the
/// backdrop, a few times over, so a listing still has something to show.
pub fn fallback_picture(seed: u64, size: (u32, u32)) -> RgbaImage {
    let mark = wave(720, 150);
    strokes_picture(std::slice::from_ref(&mark), seed ^ 0x7A_11, size)
}

// ---------- the pages of single strokes ----------

/// How many strokes a page shows: two across and four down.
pub const SHEET_COLUMNS: usize = 2;
pub const SHEET_ROWS: usize = 4;
/// How many strokes one page holds, and the most pages a listing shows after its card: the six
/// pictures a store's item page is given.
pub const SHEET_STROKES: usize = SHEET_COLUMNS * SHEET_ROWS;
pub const MOST_SHEETS: usize = crate::store::STORE_SPECIMEN_LIMIT;

/// The face brush names are set in: the app's own Inter, carried here so a page of strokes reads
/// the same on every machine.
const FACE: &[u8] = include_bytes!("../../../android/app/src/main/assets/fonts/Inter-Regular.ttf");

/// Has the caption face been loaded? False where it can't be: the pages go without their names.
fn face_ready() -> bool {
    static READY: OnceLock<bool> = OnceLock::new();
    *READY.get_or_init(|| {
        let ready = !crate::add_text_font(FACE.to_vec()).is_empty();
        if !ready {
            log::warn!("the caption face couldn't be loaded: the pages of strokes go without their names");
        }
        ready
    })
}

/// `text` laid out at `size`, its box's top left at (`x`, `y`).
fn words(text: &str, size: f32, colour: [f32; 3], x: f32, y: f32, caps: bool) -> TextInfo {
    TextInfo {
        text: text.to_owned(),
        font: "inter".to_owned(),
        style: "Regular".to_owned(),
        size,
        kerning: 0.0,
        tracking: if caps { 0.08 } else { 0.0 },
        leading: 0.0,
        baseline: 0.86,
        opacity: 1.0,
        align: TextAlignment::Left,
        underline: false,
        outline: false,
        vertical: false,
        all_caps: caps,
        r: colour[0],
        g: colour[1],
        b: colour[2],
        x,
        y,
        width: 0.0,
        runs: Vec::new(),
    }
}

/// How wide `text` is drawn, in pixels.
fn words_width(text: &str, size: f32, caps: bool) -> f32 {
    crate::text_box(words(text, size, [1.0; 3], 0.0, size, caps)).get(2).copied().unwrap_or(0.0)
}

/// `text` cut short with dots so that it is no wider than `width`.
fn fitted_words(text: &str, size: f32, caps: bool, width: f32) -> String {
    if size <= 0.0 || words_width(text, size, caps) <= width {
        return text.to_owned();
    }
    let letters: Vec<char> = text.chars().collect();
    let (mut least, mut high) = (0usize, letters.len());
    while least < high {
        let middle = (least + high).div_ceil(2);
        let candidate: String = letters.iter().take(middle).collect::<String>() + "...";
        if words_width(&candidate, size, caps) <= width {
            least = middle;
        } else {
            high = middle - 1;
        }
    }
    let mut kept: String = letters.iter().take(least).collect();
    kept.push_str("...");
    kept
}

/// `text` drawn in the caption face, its top left at (`x`, `top`) of `picture`, cut short with
/// dots rather than wider than `most`.
fn write_words(
    picture: &mut RgbaImage,
    text: &str,
    size: f32,
    colour: [f32; 3],
    x: f32,
    top: f32,
    caps: bool,
    most: f32,
) {
    if text.is_empty() || !face_ready() {
        return;
    }
    let text = fitted_words(text, size, caps, most);
    let width = words_width(&text, size, caps);
    if width <= 0.0 {
        return;
    }
    let (strip_w, strip_h) = (width.ceil() as u32 + 4, (size * 1.8).ceil() as u32 + 4);
    let drawn = crate::draw_text(words(&text, size, colour, 2.0, size * 0.86 + 2.0, caps), strip_w, strip_h);
    blit(picture, strip_w, strip_h, &drawn, x.round() as i32 - 2, top.round() as i32 - 2);
}

/// `image` (premultiplied RGBA8, `width` × `height`) laid at (`x`, `y`) over `picture`.
fn blit(picture: &mut RgbaImage, width: u32, height: u32, image: &[u8], x: i32, y: i32) {
    let (across, down) = (picture.width() as i32, picture.height() as i32);
    let pixels = picture.as_mut().as_chunks_mut::<4>().0;
    for sy in 0..height as i32 {
        let ty = y + sy;
        if ty < 0 || ty >= down {
            continue;
        }
        for sx in 0..width as i32 {
            let tx = x + sx;
            if tx < 0 || tx >= across {
                continue;
            }
            let at = (sy as usize * width as usize + sx as usize) * 4;
            let Some(source) = image.get(at..at + 4) else { continue };
            let Some(alpha) = source.get(3).copied() else { continue };
            if alpha == 0 {
                continue;
            }
            let Some(dst) = pixels.get_mut(ty as usize * across as usize + tx as usize) else { continue };
            let over = 1.0 - f32::from(alpha) / 255.0;
            for channel in 0..3 {
                let under = dst.get(channel).copied().map(f32::from).unwrap_or(0.0);
                if let Some(slot) = dst.get_mut(channel) {
                    let value = f32::from(source.get(channel).copied().unwrap_or(0)) + under * over;
                    *slot = value.round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

/// A page of single strokes, each with its brush's name under it: what a set's listing shows
/// after its card. `title` heads the page, with its own number out of `pages` across from it.
pub fn stroke_sheet(
    entries: &[(RgbaImage, String)],
    title: &str,
    page: usize,
    pages: usize,
    size: (u32, u32),
) -> RgbaImage {
    let mut picture = RgbaImage::from_pixel(size.0, size.1, Rgba([242, 232, 211, 255]));
    let specimen_ink = [0x2e, 0x2b, 0x27];
    let caption = [0.24, 0.21, 0.18];
    let (width, height) = (size.0 as f32, size.1 as f32);
    let margin = 0.047 * width;
    let heading = 0.0185 * height;
    write_words(&mut picture, title, heading, caption, margin, 0.030 * height, true, width - 2.0 * margin - 80.0);
    let number = format!("{} / {}", page + 1, pages.max(1));
    let number_width = words_width(&number, heading, false);
    write_words(&mut picture, &number, heading, caption, width - margin - number_width, 0.030 * height, false, 80.0);
    // the rule under the heading
    Layer {
        source: None,
        tint: None,
        fill: [171, 155, 133, 255],
        cx: width / 2.0,
        cy: 0.083 * height,
        hw: width / 2.0 - margin,
        hh: 0.5,
        angle: 0.0,
        corner: 0.0,
        shadow: 0.0,
        opacity: 1.0,
    }
    .draw(&mut picture);
    let gap = 0.033 * width;
    let cell = (width - 2.0 * margin - gap) / SHEET_COLUMNS as f32;
    let top = 0.108 * height;
    let row = (0.955 * height - top) / SHEET_ROWS as f32;
    let caption_size = 0.021 * height;
    for (at, (stroke, name)) in entries.iter().take(SHEET_STROKES).enumerate() {
        let left = margin + (at % SHEET_COLUMNS) as f32 * (cell + gap);
        let at_top = top + (at / SHEET_COLUMNS) as f32 * row;
        let cx = left + cell / 2.0;
        lay_fitted(
            &mut picture,
            stroke,
            cx,
            at_top + row * 0.40,
            cell - 0.02 * width,
            row * 0.62,
            0.0,
            false,
            specimen_ink,
            0.0,
            1.0,
        );
        let shown = fitted_words(name, caption_size, false, cell - 8.0);
        let name_width = words_width(&shown, caption_size, false);
        let x = left + ((cell - name_width) / 2.0).max(0.0);
        write_words(&mut picture, name, caption_size, caption, x, at_top + row * 0.80, false, cell - 8.0);
    }
    picture
}

// ---------- a pack's pictures ----------

/// The largest entry a preview reads, so one big picture in a pack can't hold it up.
const PICTURE_BYTES_MOST: usize = 24 << 20;
/// The largest side a picture may have before it is left out.
const PICTURE_SIDE_MOST: u32 = 8192;

/// The name a pack gives itself in its `assets.json`, for the sheet's own field.
pub fn pack_name(path: &Path) -> Option<String> {
    let zip = ZipReader::open(path)?;
    if zip.size("assets.json")? > 256 << 10 {
        return None;
    }
    let bytes = zip.read("assets.json")?;
    let value: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let name = value.get("name")?.as_str()?.trim().to_owned();
    (!name.is_empty()).then_some(name)
}

/// The pictures a pack's collage can show, in the order it shows them.
fn pack_pictures(zip: &ZipReader<'_>) -> Vec<RgbaImage> {
    let names: Vec<String> = zip.names().filter(|name| is_picture(name)).map(str::to_owned).collect();
    pack_picture_choices(&names, MOST_PICTURES)
        .into_iter()
        .filter_map(|at| names.get(at))
        .filter_map(|name| picture_of(zip, name))
        .collect()
}

/// Entry `name` of `zip` decoded, if it is a picture and not too big to open.
fn picture_of(zip: &ZipReader<'_>, name: &str) -> Option<RgbaImage> {
    if zip.size(name)? > PICTURE_BYTES_MOST {
        return None;
    }
    let bytes = zip.read(name)?;
    let reader = image::ImageReader::new(std::io::Cursor::new(&bytes)).with_guessed_format().ok()?;
    let (width, height) = reader.into_dimensions().ok()?;
    if width == 0 || height == 0 || width > PICTURE_SIDE_MOST || height > PICTURE_SIDE_MOST {
        return None;
    }
    Some(image::load_from_memory(&bytes).ok()?.to_rgba8())
}

// ---------- what the apps call ----------

/// A composed preview: opaque RGBA8 pixels, rows top to bottom.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct StorePreview {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl StorePreview {
    fn of(picture: &RgbaImage) -> StorePreview {
        StorePreview { width: picture.width(), height: picture.height(), rgba: picture.as_raw().to_vec() }
    }
}

/// The seed a name makes: the same name always makes the same picture.
pub fn seed_of(key: &str) -> u64 {
    let mut hash: u64 = 0xCBF2_9CE4_8422_2325;
    for byte in key.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01B3);
    }
    hash
}

/// The name of the pack at `path`, for the sheet's own field.
#[uniffi::export]
pub fn asset_pack_name(path: String) -> Option<String> {
    pack_name(Path::new(&path))
}

/// The picture a pack's store listing shows: the pack's own pictures as a collage on the store's
/// backdrop, or a drawn sample for a pack holding no picture. None if `path` isn't an archive.
#[uniffi::export]
pub fn asset_pack_store_preview(path: String, key: String) -> Option<StorePreview> {
    let zip = ZipReader::open(Path::new(&path))?;
    let seed = seed_of(&key);
    let pictures = pack_pictures(&zip);
    let picture = if pictures.is_empty() {
        log::warn!("the pack holds no picture to show: its store preview is drawn instead");
        fallback_picture(seed, (PREVIEW_WIDTH, PREVIEW_HEIGHT))
    } else {
        pack_card(&pictures, seed, &key, (PREVIEW_WIDTH, PREVIEW_HEIGHT))
    };
    Some(StorePreview::of(&picture))
}

/// The brushes of `brushes` that are brushes: a set's folder entries are not drawn.
fn brush_indices(brushes: &[Brush]) -> Vec<usize> {
    brushes.iter().enumerate().filter(|(_, brush)| !brush.name.starts_with(FOLDER_MARK)).map(|(at, _)| at).collect()
}

/// The brushes of a set a card shows: all of them, or the ones worth showing past [`MOST_SHOWN`].
fn shown_brushes(brushes: &[Brush], seed: u64) -> Vec<usize> {
    let candidates = brush_indices(brushes);
    let shapes: Vec<BrushShape> = candidates.iter().filter_map(|&at| brushes.get(at)).map(shape_of).collect();
    select_brushes(&shapes, seed).iter().filter_map(|&at| candidates.get(at).copied()).collect()
}

impl CanvasSession {
    /// The sample strokes of the brushes at `indices`, each with its brush's name.
    fn draw_strokes(&self, library: u32, set: u32, brushes: &[Brush], indices: &[usize]) -> Vec<(RgbaImage, String)> {
        let (width, height) = stroke_canvas();
        indices
            .iter()
            .filter_map(|&at| {
                let brush = brushes.get(at)?;
                if let Some(tap) = cover::stamp_specimen(self, brush) {
                    return Some((tap, brush.name.clone()));
                }
                let index = u32::try_from(at).ok()?;
                let sample = self.brush_library_preview(library, set, index, width, height)?;
                Some((RgbaImage::from_raw(sample.width, sample.height, sample.rgba)?, brush.name.clone()))
            })
            .collect()
    }

    fn store_specimens(&self, library: u32, set: u32, brushes: &[Brush], name: &str) -> Vec<StorePreview> {
        let all = brush_indices(brushes);
        let pages = all.len().div_ceil(SHEET_STROKES).min(MOST_SHEETS);
        all.chunks(SHEET_STROKES)
            .take(pages)
            .enumerate()
            .filter_map(|(page, slice)| {
                let entries = self.draw_strokes(library, set, brushes, slice);
                (!entries.is_empty()).then(|| {
                    StorePreview::of(&stroke_sheet(&entries, name, page, pages, (PREVIEW_WIDTH, PREVIEW_HEIGHT)))
                })
            })
            .collect()
    }

    /// The artistic picture a set's listing shows: the brushes at `indices` painted along the
    /// paths a card is composed of, by the brush engine, with the set's name over them.
    fn store_card(&self, brushes: &[Brush], indices: &[usize], seed: u64, name: &str, count: usize) -> RgbaImage {
        let (warm, cool) = (warm_ink(seed), cool_ink(seed));
        let mut chosen: Vec<usize> = indices.iter().take(MOST_SHOWN).copied().collect();
        // the boldest, most textured of them takes the focus: the card leads with a stroke that
        // shows what a brush is made of, not a soft wash that would read as a smudge
        let mark = |at: usize| {
            let shape = brushes.get(at).map(shape_of);
            shape.map_or(0.0, |shape| {
                let texture = shape.grain.max(shape.shape).max(shape.hardness);
                shape.size * (0.3 + 0.7 * texture)
            })
        };
        chosen.sort_by(|a, b| mark(*b).total_cmp(&mark(*a)));
        let gestures = card_gestures(count, chosen.len(), seed);
        // the focus sets the scale: a brush with no grain, shape or hard edge paints a wash, so it
        // is kept thin, and nothing else is over half the focus's size
        let texture = |brush: &Brush| {
            let shape = shape_of(brush);
            0.12 + 0.88 * shape.grain.max(shape.shape).max(shape.hardness)
        };
        let focus = gestures
            .first()
            .zip(chosen.first())
            .and_then(|(gesture, &at)| brushes.get(at).map(|brush| gesture.diameter(brush) * texture(brush)))
            .unwrap_or(0.0);
        // painted last first: the focus goes down over the strokes that support it
        let painted: Vec<Painted> = gestures
            .iter()
            .enumerate()
            .rev()
            .filter_map(|(at, gesture)| {
                let brush = brushes.get(*chosen.get(gesture.brush)?)?.clone();
                let ink = gesture_ink(gesture, warm, cool);
                let samples = gesture.samples(&brush, 56);
                let mut diameter = gesture.diameter(&brush) * texture(&brush);
                if at > 0 && focus > 0.0 {
                    diameter = diameter.min(focus * 0.55);
                }
                Some(Painted { ink, diameter: diameter.max(2.0), samples, brush, second_scale: 1.0 })
            })
            .collect();
        let painting = if painted.is_empty() {
            None
        } else {
            self.painted(&painted, PREVIEW_WIDTH, PREVIEW_HEIGHT)
                .and_then(|rows| RgbaImage::from_raw(PREVIEW_WIDTH, PREVIEW_HEIGHT, rows))
        };
        let mut picture = match painting {
            Some(painting) => {
                let mut picture = backdrop((PREVIEW_WIDTH, PREVIEW_HEIGHT), true);
                over(&mut picture, &painting);
                picture
            }
            None => {
                log::warn!("the set's brushes painted no stroke: its store preview is drawn instead");
                fallback_picture(seed, (PREVIEW_WIDTH, PREVIEW_HEIGHT))
            }
        };
        let brushes = if count == 1 { "1 BRUSH".to_owned() } else { format!("{count} BRUSHES") };
        masthead(&mut picture, "BRUSH SET", name, &brushes);
        picture
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The picture a set's store listing shows: the set's brushes painted across it, the same for
    /// the same `key` (the listing's name) every time. None if there is no such set; a set whose
    /// brushes can't paint still gets a picture, drawn instead.
    pub fn brush_set_store_preview(&self, library: u32, set: u32, key: String) -> Option<StorePreview> {
        let brushes = self.brush_set_brushes(library, set)?;
        let seed = seed_of(&key);
        let indices = shown_brushes(&brushes, seed);
        let count = brush_indices(&brushes).len();
        Some(StorePreview::of(&self.store_card(&brushes, &indices, seed, &key, count)))
    }

    /// Every picture a set's store listing shows: the artistic one first, then a page of single
    /// strokes with their brushes' names for each [`SHEET_STROKES`] of them, up to [`MOST_SHEETS`]
    /// pages. Empty if there is no such set.
    pub fn brush_set_store_previews(&self, library: u32, set: u32, key: String) -> Vec<StorePreview> {
        let Some(brushes) = self.brush_set_brushes(library, set) else { return Vec::new() };
        let seed = seed_of(&key);
        let indices = shown_brushes(&brushes, seed);
        let card = self.store_card(&brushes, &indices, seed, &key, brush_indices(&brushes).len());
        let mut pictures = vec![StorePreview::of(&card)];
        pictures.extend(self.store_specimens(library, set, &brushes, &key));
        pictures
    }
}

/// A seeded source of the small choices a layout makes (splitmix64): the same seed makes the same
/// picture, on any machine.
struct Rng(u64);

impl Rng {
    fn new(seed: u64) -> Rng {
        Rng(seed ^ 0x9E37_79B9_7F4A_7C15)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number from `-spread` to `spread`.
    fn around(&mut self, spread: f32) -> f32 {
        let unit = (self.next() >> 11) as f32 / (1u64 << 53) as f32;
        (unit * 2.0 - 1.0) * spread
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Brushes of every kind, so a set of them has something to tell apart.
    fn shapes(count: usize) -> Vec<BrushShape> {
        (0..count)
            .map(|at| {
                let f = at as f32;
                BrushShape {
                    size: ((f * 0.37).sin() + 1.0) / 2.0,
                    hardness: ((f * 0.11).cos() + 1.0) / 2.0,
                    roundness: 1.0,
                    scatter: (f % 3.0) / 3.0,
                    spacing: ((f * 0.07).sin() + 1.0) / 2.0 * 0.2,
                    grain: if at % 2 == 0 { 1.0 } else { 0.0 },
                    shape: if at % 3 == 0 { 1.0 } else { 0.0 },
                    dual: if at % 5 == 0 { 1.0 } else { 0.0 },
                }
            })
            .collect()
    }

    fn names(list: &[&str]) -> Vec<String> {
        list.iter().map(|name| (*name).to_owned()).collect()
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-store-preview-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// An archive of `entries` at `dir/pack.assetset`.
    fn packed(dir: &Path, entries: &[(&str, Vec<u8>)]) -> std::path::PathBuf {
        let mut zip = procraft_engine::zip::ZipWriter::new();
        for (name, bytes) in entries {
            zip.add(name, bytes);
        }
        let path = dir.join("pack.assetset");
        std::fs::write(&path, zip.finish()).unwrap();
        path
    }

    fn picture(width: u32, height: u32, colour: [u8; 4]) -> Vec<u8> {
        let pixels: Vec<u8> = colour.iter().copied().cycle().take((width * height * 4) as usize).collect();
        crate::project::rgba_png(width, height, &pixels)
    }

    #[test]
    fn a_single_brush_is_drawn_three_times_down_the_sheet() {
        let strokes = stroke_placements(1, seed_of("RSCO Dead Pen"));
        assert_eq!(strokes.len(), 3);
        assert!(strokes.iter().all(|stroke| stroke.brush == 0), "one brush draws all three");
        assert!(clear(&strokes), "{:?}", strokes.iter().map(Stroke::bounds).collect::<Vec<_>>());
        // the soft one under them, running past the edge, and a crisp stroke over it
        let ghost = strokes.last().expect("the soft one");
        assert!(ghost.opacity < 1.0, "the one under the rest is soft");
        assert_eq!(strokes[0].opacity, 1.0);
        assert!(strokes[0].width > strokes[1].width, "the focus is the biggest of the crisp ones");
        assert!(strokes.iter().any(|stroke| stroke.angle > 0.0), "one is turned one way");
        assert!(strokes.iter().any(|stroke| stroke.angle < 0.0), "another the other");
    }

    #[test]
    fn a_set_is_laid_over_the_soft_one_from_its_first_brush() {
        for count in 2..=8 {
            let strokes = stroke_placements(count, seed_of("a set"));
            let laid = count.min(MOST_CARD);
            assert_eq!(strokes.len(), laid, "{count}: a card shows at most {MOST_CARD}");
            let mut drawn: Vec<usize> = strokes.iter().map(|stroke| stroke.brush).collect();
            drawn.sort_unstable();
            assert_eq!(drawn, (0..laid).collect::<Vec<_>>(), "{count}: each brush once");
            assert!(clear(&strokes), "{count}: {:?}", strokes.iter().map(Stroke::bounds).collect::<Vec<_>>());
            // the soft one last (drawn first), in the first brush's own stroke, under the rest
            let ghost = strokes.last().expect("the soft one");
            assert!(ghost.opacity < 1.0 && ghost.brush == 0, "{count}");
            // the focus over it: the bone stroke, the biggest of those laid down whole
            let crisp: Vec<&Stroke> = strokes.iter().filter(|stroke| stroke.opacity >= 1.0).collect();
            assert!(crisp[0].width > crisp[1..].iter().map(|stroke| stroke.width).fold(0.0, f32::max), "{count}");
            assert_eq!(strokes[0].ink, BONE);
            assert!(strokes[1..crisp.len()].iter().all(|stroke| stroke.ink == strokes[1].ink), "{count}: one accent");
        }
    }

    #[test]
    fn a_set_of_fifty_keeps_the_brushes_worth_showing() {
        let plain = BrushShape {
            size: 0.2,
            hardness: 0.8,
            roundness: 1.0,
            scatter: 0.0,
            spacing: 0.05,
            grain: 0.0,
            shape: 0.0,
            dual: 0.0,
        };
        let mut many = vec![plain; 50];
        for (at, size) in [(7, 0.9), (23, 0.55), (41, 0.75)] {
            if let Some(shape) = many.get_mut(at) {
                *shape = BrushShape { size, scatter: 0.6, grain: 1.0, shape: 1.0, ..plain };
            }
        }
        let chosen = select_brushes(&many, seed_of("fifty"));
        assert_eq!(chosen.len(), MOST_SHOWN);
        for expected in [0, 25, 49] {
            assert!(chosen.contains(&expected), "the set's first, middle and last are shown");
        }
        for distinct in [7, 23, 41] {
            assert!(chosen.contains(&distinct), "brush {distinct} is one of the ones worth showing");
        }
        let mut once = chosen.clone();
        once.sort_unstable();
        once.dedup();
        assert_eq!(once.len(), chosen.len(), "no brush shown twice");
        assert_eq!(select_brushes(&many, seed_of("fifty")), chosen, "the same set chooses the same brushes");
        // a set small enough is shown whole
        assert_eq!(select_brushes(&shapes(8), seed_of("eight")), (0..8).collect::<Vec<_>>());
        assert_eq!(select_brushes(&shapes(1), seed_of("one")), vec![0]);
    }

    #[test]
    fn the_same_name_always_makes_the_same_picture() {
        // the seed is the name hashed, the same on any machine
        assert_eq!(seed_of(""), 0xCBF2_9CE4_8422_2325);
        assert_eq!(seed_of("Dry Ink"), seed_of("Dry Ink"));
        assert_ne!(seed_of("Dry Ink"), seed_of("Dry Ink "));
        for count in [1, 2, 3, 5, 8] {
            assert_eq!(stroke_placements(count, seed_of("Dry Ink")), stroke_placements(count, seed_of("Dry Ink")));
            assert_ne!(stroke_placements(count, seed_of("Dry Ink")), stroke_placements(count, seed_of("Rough Paper")));
        }
        assert_eq!(picture_tiles(6, seed_of("Bond Paper")), picture_tiles(6, seed_of("Bond Paper")));
    }

    #[test]
    fn a_pack_shows_one_picture_an_asset_and_never_more_than_eight() {
        let mut entries = Vec::new();
        for at in 0..10 {
            entries.push(format!("items/a{at}/thumbnail.png"));
            entries.push(format!("items/a{at}/layers/1.jpg"));
            entries.push(format!("items/a{at}/layers/2.jpg"));
        }
        entries.push("assets.json".to_owned());
        let chosen = pack_picture_choices(&entries, MOST_PICTURES);
        assert_eq!(chosen.len(), MOST_PICTURES, "a big pack still shows eight");
        let shown: Vec<&str> = chosen.iter().filter_map(|&at| entries.get(at).map(String::as_str)).collect();
        assert!(shown.iter().all(|name| name.ends_with("thumbnail.png")), "{shown:?}");
        assert_eq!(shown.first().copied(), Some("items/a0/thumbnail.png"), "in name order");
        assert_eq!(shown.last().copied(), Some("items/a7/thumbnail.png"));
        // no thumbnails: the first picture of each asset
        let layers = names(&[
            "items/one/layers/1.jpg",
            "items/one/layers/2.jpg",
            "items/two/layers/1.jpg",
            "items/two/layers/2.png",
        ]);
        assert_eq!(pack_picture_choices(&layers, MOST_PICTURES), vec![0, 2]);
        // pictures named some other way are still shown, and no picture is no picture
        let stray = names(&["pictures/one.png", "pictures/two.JPG"]);
        assert_eq!(pack_picture_choices(&stray, 1), vec![0]);
        assert!(pack_picture_choices(&names(&["assets.json", "items/one/artwork.json"]), MOST_PICTURES).is_empty());
    }

    #[test]
    fn a_collage_keeps_its_pages_inside_the_picture_with_the_focus_drawn_last_but_one() {
        let (mx, my) = margins();
        for count in 1..=8 {
            let tiles = picture_tiles(count, seed_of("Bond Paper"));
            assert_eq!(tiles.len(), count);
            let mut shown: Vec<usize> = tiles.iter().map(|tile| tile.picture).collect();
            shown.sort_unstable();
            assert_eq!(shown, (0..count).collect::<Vec<_>>(), "each picture once");
            for tile in &tiles {
                let (left, top, right, bottom) = tile.bounds();
                assert!(
                    left >= mx - 0.01 && top >= my - 0.01 && right <= 1.0 - mx + 0.01 && bottom <= 1.0 - my + 0.01,
                    "{count}: {tile:?}"
                );
            }
            // the corner pages go down first, the focus over them, the front page last
            let focus = tiles.iter().position(|tile| tile.picture == 0).unwrap_or(0);
            assert!(tiles.iter().take(focus).all(|tile| tile.picture != 0));
            if let Some(front) = tiles.iter().position(|tile| tile.picture == 5) {
                assert_eq!(front, tiles.len() - 1, "{count}: the front page is drawn last");
            }
        }
    }

    #[test]
    fn a_card_is_painted_along_paths_of_its_own() {
        // one brush still paints three strokes, each along its own path
        let one = card_gestures(1, 1, seed_of("one"));
        assert_eq!(one.len(), 3);
        assert!(one.iter().all(|gesture| gesture.brush == 0), "the set's one brush paints them all");
        // more brushes, more strokes, up to the twelve places there are
        for (count, wanted) in [(1, 3), (2, 4), (3, 5), (5, 6), (8, 8), (12, 9), (31, 10), (42, 10), (80, 12)] {
            let shown = count.min(MOST_SHOWN);
            let gestures = card_gestures(count, shown, seed_of("a set"));
            assert_eq!(gestures.len(), wanted, "{count} brushes");
            // the set's brushes take the strokes in turn, then again as they run out
            for (at, gesture) in gestures.iter().enumerate() {
                assert_eq!(gesture.brush, at % shown, "{count}: stroke {at}");
            }
            assert_eq!(gestures[0].hue, 0, "the focus is the bone ink");
        }
        // a big set's card gets every kind of mark, and all three inks
        let gestures = card_gestures(42, MOST_SHOWN, seed_of("a set"));
        for shape in [PathShape::Swash, PathShape::Loop, PathShape::Wave, PathShape::Dab] {
            assert!(gestures.iter().any(|gesture| gesture.shape == shape), "{shape:?}");
        }
        for hue in [0, 1, 2] {
            assert!(gestures.iter().any(|gesture| gesture.hue == hue), "ink {hue}");
        }
        // a path runs from where it says to where it says, in time order, pressed 0..1
        let brush = Brush::round_pen();
        let gesture = gestures[0];
        assert_eq!(gesture.shape, PathShape::Swash, "the focus is a swash");
        let samples = gesture.samples(&brush, 32);
        assert_eq!(samples.len(), 32);
        assert!((samples[0].pos.x - gesture.from.0).abs() < 1e-3 && (samples[0].pos.y - gesture.from.1).abs() < 1e-3);
        assert!((samples[31].pos.x - gesture.to.0).abs() < 1e-3 && (samples[31].pos.y - gesture.to.1).abs() < 1e-3);
        assert!(samples.windows(2).all(|pair| pair[1].time_ms > pair[0].time_ms));
        assert!(samples.iter().all(|sample| (0.0..=1.0).contains(&sample.pressure)));
        // the same name paints the same paths
        assert_eq!(card_gestures(42, MOST_SHOWN, seed_of("a set")), card_gestures(42, MOST_SHOWN, seed_of("a set")));
        assert_ne!(card_gestures(42, MOST_SHOWN, seed_of("a set")), card_gestures(42, MOST_SHOWN, seed_of("another")));
    }

    #[test]
    fn a_stroke_is_inked_and_laid_on_the_backdrop_the_same_way_every_time() {
        let stroke = RgbaImage::from_pixel(60, 18, Rgba([255, 255, 255, 255]));
        let seed = seed_of("Dry Ink");
        let picture = strokes_picture(std::slice::from_ref(&stroke), seed, (120, 90));
        assert_eq!((picture.width(), picture.height()), (120, 90));
        assert!(picture.pixels().all(|pixel| pixel.0[3] == 255), "a preview is opaque");
        let corner = picture.get_pixel(0, 0).0;
        assert!(corner.iter().take(3).all(|channel| *channel < 60), "the backdrop is dark: {corner:?}");
        let inked = picture.pixels().filter(|pixel| pixel.0.iter().take(3).any(|channel| *channel > 120)).count();
        assert!(inked > 100, "{inked} pixels are inked");
        assert_eq!(picture.as_raw(), strokes_picture(std::slice::from_ref(&stroke), seed, (120, 90)).as_raw());
        let other = strokes_picture(std::slice::from_ref(&stroke), seed_of("Rough Paper"), (120, 90));
        assert_ne!(picture.as_raw(), other.as_raw(), "another name draws another picture");
        // a picture of nothing is still a picture of the backdrop
        let blank = strokes_picture(&[], seed, (120, 90));
        assert!(blank.pixels().all(|pixel| pixel.0.iter().take(3).all(|channel| *channel < 60)));
    }

    #[test]
    fn a_fallback_is_the_backdrop_with_the_sample_stroke_drawn_on_it() {
        let seed = seed_of("nothing to draw");
        let drawn = fallback_picture(seed, (300, 225));
        assert_eq!((drawn.width(), drawn.height()), (300, 225));
        assert_eq!(drawn.as_raw(), fallback_picture(seed, (300, 225)).as_raw());
        let inked = drawn.pixels().filter(|pixel| pixel.0.iter().take(3).any(|channel| *channel > 120)).count();
        assert!(inked > 50, "{inked} pixels are drawn on");
        assert_ne!(drawn.as_raw(), fallback_picture(seed_of("another"), (300, 225)).as_raw());
    }

    #[test]
    fn a_brush_s_name_is_written_under_its_stroke() {
        assert!(face_ready(), "the caption face is the app's own, carried with the previews");
        let mut picture = backdrop((300, 80), true);
        let before = picture.as_raw().to_vec();
        write_words(&mut picture, "Block Inker", 18.0, CAPTION, 10.0, 20.0, false, 280.0);
        let lit = picture.pixels().filter(|pixel| pixel.0.iter().take(3).any(|channel| *channel > 120)).count();
        assert!(lit > 30, "{lit} pixels of the name");
        // a name too long for its cell is cut short with dots rather than run past it
        let cut = fitted_words("A very long brush name indeed", 18.0, false, 80.0);
        assert!(cut.ends_with("...") && words_width(&cut, 18.0, false) <= 80.0, "{cut}");
        assert_eq!(fitted_words("Ink", 18.0, false, 200.0), "Ink");
        // nothing to write, nothing changes
        let mut empty = backdrop((300, 80), true);
        write_words(&mut empty, "", 18.0, CAPTION, 10.0, 20.0, false, 280.0);
        assert_eq!(empty.as_raw(), &before);
    }

    #[test]
    fn a_page_shows_at_most_the_strokes_it_has_room_for() {
        let stroke = RgbaImage::from_pixel(8, 4, Rgba([255, 255, 255, 255]));
        let entries: Vec<(RgbaImage, String)> =
            (0..SHEET_STROKES + 3).map(|at| (stroke.clone(), format!("Brush {at}"))).collect();
        let page = stroke_sheet(&entries, "A Set", 0, 2, (600, 450));
        assert_eq!((page.width(), page.height()), (600, 450));
        assert_eq!(page.as_raw(), stroke_sheet(&entries, "A Set", 0, 2, (600, 450)).as_raw());
        let second = stroke_sheet(&entries, "A Set", 1, 2, (600, 450));
        assert_ne!(page.as_raw(), second.as_raw(), "each page numbers itself");
    }

    #[test]
    fn a_pack_with_pictures_is_a_collage_and_one_without_is_drawn() {
        let dir = scratch("pack");
        let manifest = br#"{"format":"procraft-assets","version":1,"kind":"set","name":"Washed Paper"}"#.to_vec();
        let collage = packed(
            &dir,
            &[
                ("assets.json", manifest.clone()),
                ("items/one/thumbnail.png", picture(96, 128, [190, 170, 130, 255])),
                ("items/two/thumbnail.png", picture(96, 128, [90, 120, 150, 255])),
            ],
        );
        assert_eq!(pack_name(&collage).as_deref(), Some("Washed Paper"));
        let key = "Washed Paper";
        let preview = asset_pack_store_preview(collage.to_string_lossy().into_owned(), key.to_owned()).unwrap();
        assert_eq!((preview.width, preview.height), (PREVIEW_WIDTH, PREVIEW_HEIGHT));
        let fallback = fallback_picture(seed_of(key), (PREVIEW_WIDTH, PREVIEW_HEIGHT));
        assert_ne!(preview.rgba, fallback.as_raw().to_vec(), "the pack's own pictures are the collage");
        let again = asset_pack_store_preview(collage.to_string_lossy().into_owned(), key.to_owned()).unwrap();
        assert_eq!(preview.rgba, again.rgba, "the same pack makes the same picture");

        // a pack with no picture at all still gets one
        let empty = packed(&dir, &[("assets.json", manifest)]);
        let drawn = asset_pack_store_preview(empty.to_string_lossy().into_owned(), key.to_owned()).unwrap();
        assert_eq!(drawn.rgba, fallback.as_raw().to_vec());
        // and a file that isn't an archive is no pack
        let nowhere = dir.join("missing.assetset");
        assert!(asset_pack_store_preview(nowhere.to_string_lossy().into_owned(), key.to_owned()).is_none());
        assert!(asset_pack_name(nowhere.to_string_lossy().into_owned()).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

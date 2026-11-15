//! Brush engines of plugins' own (interface 4). A plugin that lists `brush_engines` reads brush
//! files itself and draws the brushes it makes of them its own way: the app hands it a stroke's
//! input and lays the dabs it answers, on the GPU, as it lays its own stamps.
//!
//! - **Import.** A brush file whose name ends as an engine on says (`accepts`) goes to it as well as
//!   through the app's own reading. Each brush it makes, a name and data of its own (a preview and
//!   a size if it gives them), joins the brush the app read of the same name, or comes in on a
//!   round brush if there is none: [`Brush::engine`] keeps the engine's name and the data. With the
//!   engine off or gone, the brush draws as the app read it, so the app is as it was.
//! - **Look.** Before a stroke, the app asks the engine how the brush looks ([`BrushLook`]): its tips
//!   (grey pictures, one picked per dab), whether its dabs build up over each other or towards their
//!   opacity, its blend mode, whether it erases, a texture dabs read where they land, and a second
//!   stroke whose coverage combines with the first (a dual brush). Kept for the brush's data while
//!   the plugin stays on.
//! - **Strokes.** A stroke's samples go to the engine as they come ([`EngineHalf`]), and the dabs it
//!   answers become the stroke's stamps: each its place, size, angle, squash, opacity, flow, tip,
//!   mirroring, its own colour if it has one, and how strongly the texture shows in it.
//!
//! Records are the layer items' (see `plugin_layers.rs`): fields of a 16-bit id, a 32-bit length
//! and the bytes, little-endian. What each holds is in `docs/plugins.md`.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard};

use procraft_engine::brush::Rendering;
use procraft_engine::brush::{Brush, OutsideBrush};
use procraft_engine::geom::Vec2;
use procraft_engine::input::Sample;
use procraft_engine::stroke::{OutsideEngine, Stamp, Tint};
use procraft_gpu::{SecondStroke, StrokeStyle, StrokeTool};

use crate::hooks::{Hooks, Shared, at_work};
use crate::plugin_layers::{Record, fields};
use crate::plugin_run::{DAB_BYTES, Stop};

/// A brush engine a plugin has: its key (`<id>.<key>`), what it is called and the endings of the
/// names of the brush files it reads.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrushEngine {
    pub key: String,
    pub label: String,
    pub accepts: Vec<String>,
}

/// A brush an engine made of a file.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct MadeBrush {
    pub engine: String,
    pub name: String,
    pub data: Vec<u8>,
    pub thumbnail: Option<Vec<u8>>,
}

/// The ids of the records' fields.
mod field {
    /// The file handed to an engine: its name, its bytes.
    pub const FILE_NAME: u16 = 1;
    pub const FILE_BYTES: u16 = 2;
    /// The brushes made of it, each a record of its name, data and preview.
    pub const MADE: u16 = 1;
    pub const MADE_NAME: u16 = 1;
    pub const MADE_DATA: u16 = 2;
    pub const MADE_THUMBNAIL: u16 = 3;
    /// A look: its tips (each width, height, pixels), how its dabs build up, its blend mode,
    /// whether it erases, its texture and its second stroke.
    pub const TIP: u16 = 1;
    pub const TIP_WIDTH: u16 = 1;
    pub const TIP_HEIGHT: u16 = 2;
    pub const TIP_PIXELS: u16 = 3;
    pub const BUILD: u16 = 2;
    pub const BLEND: u16 = 3;
    pub const ERASES: u16 = 4;
    pub const TEXTURE: u16 = 5;
    pub const TEXTURE_SCALE: u16 = 4;
    pub const TEXTURE_MODE: u16 = 5;
    pub const SECOND: u16 = 6;
    pub const SECOND_COMBINE: u16 = 3;
    /// A stroke starting: the brush's id, the colour and the secondary colour, the size now and
    /// where it starts, the opacity slider, the tool, the pointer, a seed and the view's turn.
    pub const BEGIN_BRUSH: u16 = 1;
    pub const BEGIN_COLOUR: u16 = 2;
    pub const BEGIN_SECONDARY: u16 = 3;
    pub const BEGIN_DIAMETER: u16 = 4;
    pub const BEGIN_OPACITY: u16 = 5;
    pub const BEGIN_TOOL: u16 = 6;
    pub const BEGIN_POINTER: u16 = 7;
    pub const BEGIN_SEED: u16 = 8;
    pub const BEGIN_TURN: u16 = 9;
    pub const BEGIN_START_DIAMETER: u16 = 10;
}

/// A dab's flags.
const FLIP_ACROSS: u32 = 1;
const FLIP_DOWN: u32 = 2;
const OWN_COLOUR: u32 = 4;
const FOR_SECOND: u32 = 8;

/// The most tips, and the largest side of one, and of a texture.
const MOST_TIP_SIDE: u32 = 4096;
/// The side tips are drawn into on the GPU at most, and the bytes a set may take there (before its
/// mip levels).
const MOST_SET_SIDE: u32 = 1024;
const MOST_SET_BYTES: usize = 48 << 20;
const MOST_TEXTURE_SIDE: u32 = 2048;
/// The looks each plugin keeps by the brushes' data.
pub(crate) const LOOKS_KEPT: usize = 16;

/// The tips a stroke picks from, fitted into squares a power of two a side for the GPU, and their
/// id there.
#[derive(Debug, PartialEq)]
pub(crate) struct TipSet {
    pub id: u64,
    pub side: u32,
    pub layers: Vec<Vec<u8>>,
}

/// How a stroke's stamps build up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Build {
    /// Each laid over those before.
    Over,
    /// Each towards its opacity by its flow, never past it nor back.
    Towards,
}

/// A texture the dabs read where they land, as a grain: a square, one tile of it `scale` canvas
/// pixels to one of its own, combined with each dab by `mode`.
#[derive(Debug, PartialEq)]
pub(crate) struct LookTexture {
    pub id: u64,
    pub side: u32,
    pub pixels: Vec<u8>,
    pub tile: f32,
    pub mode: u32,
}

/// How a brush an engine draws looks (see the module's notes).
#[derive(Debug, PartialEq)]
pub(crate) struct BrushLook {
    pub tips: TipSet,
    pub build: Build,
    pub blend: Option<u32>,
    pub erases: bool,
    pub texture: Option<LookTexture>,
    pub second: Option<SecondLook>,
}

/// A brush look's second stroke: its tips, how they build up, and how its coverage combines with
/// the first's (a dual brush's mode).
#[derive(Debug, PartialEq)]
pub(crate) struct SecondLook {
    pub tips: TipSet,
    pub build: Build,
    pub combine: u32,
}

fn u32_of(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(*bytes.first_chunk::<4>()?))
}

fn f32_of(bytes: &[u8]) -> Option<f32> {
    Some(f32::from_le_bytes(*bytes.first_chunk::<4>()?)).filter(|v| v.is_finite())
}

/// The record `bytes` as a look, the tips' set given `id` (and the second's and the texture's
/// near it); none if it doesn't hold up.
pub(crate) fn read_look(bytes: &[u8], id: u64) -> Option<BrushLook> {
    let mut tips = Vec::new();
    let (mut build, mut blend, mut erases, mut texture, mut second) = (Build::Over, None, false, None, None);
    for (f, value) in fields(bytes) {
        match f {
            field::TIP => tips.push(read_tip(value)?),
            field::BUILD => build = read_build(value),
            field::BLEND => blend = u32_of(value),
            field::ERASES => erases = value.first().is_some_and(|&b| b != 0),
            field::TEXTURE => texture = read_texture(value, id ^ 0x7e47_0001),
            field::SECOND => second = read_second(value, id ^ 0x5eed_0002),
            _ => {}
        }
    }
    Some(BrushLook { tips: tip_set(&tips, id)?, build, blend, erases, texture, second })
}

/// A set's tips, as the GPU takes them.
fn layers(set: &TipSet) -> Vec<&[u8]> {
    set.layers.iter().map(Vec::as_slice).collect()
}

fn read_build(value: &[u8]) -> Build {
    if value.first().is_some_and(|&b| b == 1) { Build::Towards } else { Build::Over }
}

/// A tip: its width, height and grey pixels (255 paints).
fn read_tip(bytes: &[u8]) -> Option<(u32, u32, Vec<u8>)> {
    let (mut w, mut h, mut pixels) = (None, None, None);
    for (f, value) in fields(bytes) {
        match f {
            field::TIP_WIDTH => w = u32_of(value),
            field::TIP_HEIGHT => h = u32_of(value),
            field::TIP_PIXELS => pixels = Some(value),
            _ => {}
        }
    }
    let (w, h, pixels) = (w?, h?, pixels?);
    let fits = (1..=MOST_TIP_SIDE).contains(&w) && (1..=MOST_TIP_SIDE).contains(&h);
    (fits && pixels.len() == (w as usize) * (h as usize)).then(|| (w, h, pixels.to_vec()))
}

fn read_second(bytes: &[u8], id: u64) -> Option<SecondLook> {
    let mut tips = Vec::new();
    let (mut build, mut combine) = (Build::Towards, 1);
    for (f, value) in fields(bytes) {
        match f {
            field::TIP => tips.push(read_tip(value)?),
            field::BUILD => build = read_build(value),
            field::SECOND_COMBINE => combine = u32_of(value)?,
            _ => {}
        }
    }
    Some(SecondLook { tips: tip_set(&tips, id)?, build, combine })
}

fn read_texture(bytes: &[u8], id: u64) -> Option<LookTexture> {
    let (mut scale, mut mode) = (1.0, 1);
    // its picture's fields are a tip's
    let mut parts = Record::default();
    for (f, value) in fields(bytes) {
        match f {
            field::TIP_WIDTH | field::TIP_HEIGHT | field::TIP_PIXELS => {
                parts.field(f, value);
            }
            field::TEXTURE_SCALE => scale = f32_of(value)?,
            field::TEXTURE_MODE => mode = u32_of(value)?,
            _ => {}
        }
    }
    let (w, h, pixels) = read_tip(&parts.bytes())?;
    if !(0.01..=1000.0).contains(&scale) {
        return None;
    }
    // a square a power of two a side, as a grain is read from: one tile its longer side
    let side = w.max(h).next_power_of_two().clamp(16, MOST_TEXTURE_SIDE);
    let pixels = stretched(w, h, &pixels, side);
    Some(LookTexture { id, side, pixels, tile: w.max(h) as f32 * scale, mode })
}

/// The tips as a set for the GPU: each fitted into a square, all one side (a power of two, as small
/// as keeps the largest whole, held within what a set may take).
fn tip_set(tips: &[(u32, u32, Vec<u8>)], id: u64) -> Option<TipSet> {
    let most = procraft_gpu::MOST_TIPS as usize;
    if tips.is_empty() || tips.len() > most {
        return None;
    }
    let largest = tips.iter().map(|(w, h, _)| *w.max(h)).max().unwrap_or(1);
    let mut side = largest.next_power_of_two().clamp(16, MOST_SET_SIDE);
    while side > 16 && (side as usize).pow(2) * tips.len() > MOST_SET_BYTES {
        side /= 2;
    }
    let layers = tips.iter().map(|(w, h, pixels)| fitted(*w, *h, pixels, side)).collect();
    Some(TipSet { id, side, layers })
}

/// `pixels` (`w` x `h`) fitted into a square `side` a side, its longer side across all of it,
/// centred: each pixel the average of those it covers, or between the four nearest where it is
/// drawn larger.
fn fitted(w: u32, h: u32, pixels: &[u8], side: u32) -> Vec<u8> {
    let tip = procraft_engine::abr::AbrTip {
        name: String::new(),
        spacing: None,
        width: w,
        height: h,
        pixels: pixels.to_vec(),
    };
    crate::brush_files::fitted_at(&tip, side as usize)
}

/// `pixels` (`w` x `h`) drawn over a square `side` a side, stretched to fill it (a texture's tile).
fn stretched(w: u32, h: u32, pixels: &[u8], side: u32) -> Vec<u8> {
    let (w, h, side) = (w as usize, h as usize, side as usize);
    let get = |x: usize, y: usize| f32::from(pixels.get(y.min(h - 1) * w + x.min(w - 1)).copied().unwrap_or(0));
    let mut out = vec![0_u8; side * side];
    for y in 0..side {
        let v = ((y as f32 + 0.5) * h as f32 / side as f32 - 0.5).max(0.0);
        let (y0, fy) = (v.floor() as usize, v.fract());
        for x in 0..side {
            let u = ((x as f32 + 0.5) * w as f32 / side as f32 - 0.5).max(0.0);
            let (x0, fx) = (u.floor() as usize, u.fract());
            // wrapping round, as a tile does
            let (x1, y1) = ((x0 + 1) % w, (y0 + 1) % h);
            let top = get(x0, y0) * (1.0 - fx) + get(x1, y0) * fx;
            let bottom = get(x0, y1) * (1.0 - fx) + get(x1, y1) * fx;
            if let Some(v) = out.get_mut(y * side + x) {
                *v = (top * (1.0 - fy) + bottom * fy).round() as u8;
            }
        }
    }
    out
}

/// What a brush's load hands its engine: the brush's id (field 1, u64) and its data (field 2).
fn load_record(id: u64, data: &[u8]) -> Vec<u8> {
    let mut r = Record::default();
    r.field(1, &id.to_le_bytes()).field(2, data);
    r.bytes()
}

/// The id of a brush's data: what its look is kept by, here and on the GPU.
pub(crate) fn data_id(data: &[u8]) -> u64 {
    crate::project::pixels_hash(data) ^ (data.len() as u64).rotate_left(17)
}

/// The brushes the engines on make of a file called `name` (`bytes`): those whose `accepts` has the
/// ending of its name, or every engine for a name without one.
pub(crate) fn make_brushes(hooks: &Hooks, name: &str, bytes: &[u8]) -> Vec<MadeBrush> {
    let ending = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase());
    let mut file = Record::default();
    file.field(field::FILE_NAME, name.as_bytes()).field(field::FILE_BYTES, bytes);
    let file = file.bytes();
    let mut made = Vec::new();
    for (plugin, running, index, key) in hooks.engines_reading(ending.as_deref()) {
        let answer = at_work(&running).brush_import(index, &file);
        match answer {
            Ok(record) => made.extend(read_made(&record, &key)),
            Err(stop) => hooks.stopped(&plugin, stop),
        }
    }
    made
}

/// The brushes in the record an engine `key` answered an import with.
fn read_made(record: &[u8], key: &str) -> Vec<MadeBrush> {
    let mut made = Vec::new();
    for (f, value) in fields(record) {
        if f != field::MADE {
            continue;
        }
        let (mut name, mut data, mut thumbnail) = (None, None, None);
        for (f, value) in fields(value) {
            match f {
                field::MADE_NAME => name = std::str::from_utf8(value).ok().map(|n| n.chars().take(256).collect()),
                field::MADE_DATA => data = Some(value.to_vec()),
                field::MADE_THUMBNAIL => thumbnail = Some(value.to_vec()).filter(|t| t.starts_with(b"\x89PNG")),
                _ => {}
            }
        }
        if let (Some(name), Some(data)) = (name, data) {
            made.push(MadeBrush { engine: key.to_owned(), name, data, thumbnail });
        }
    }
    made
}

/// `brushes` (as the app read a file) with what engines `made` of it: each such brush joins the one
/// of its name not yet joined, or comes in on a round brush after them.
pub(crate) fn join_made(brushes: &mut Vec<Brush>, made: Vec<MadeBrush>) {
    let mut joined = vec![false; brushes.len()];
    for m in made {
        let engine = Some(OutsideBrush { engine: m.engine, data: m.data.into() });
        let at = brushes.iter().zip(&joined).position(|(b, &j)| !j && b.name.trim() == m.name.trim());
        match at.and_then(|i| brushes.get_mut(i).map(|b| (i, b))) {
            Some((i, b)) => {
                b.engine = engine;
                if b.thumbnail.is_none() {
                    b.thumbnail = m.thumbnail.map(Into::into);
                }
                if let Some(j) = joined.get_mut(i) {
                    *j = true;
                }
            }
            None => {
                brushes.push(Brush {
                    name: m.name,
                    thumbnail: m.thumbnail.map(Into::into),
                    engine,
                    ..Brush::round_pen()
                });
                joined.push(true);
            }
        }
    }
}

/// What a stroke tells its engine as it starts, besides its brush.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Starting {
    /// Its colour and secondary colour (sRGB 0..1), its size (canvas pixels) now and where the
    /// brush's size starts, the opacity slider (0..1).
    pub colour: [f32; 3],
    pub secondary: [f32; 3],
    pub diameter: f32,
    pub start_diameter: f32,
    pub opacity: f32,
    /// 0 paint, 1 erase, 2 smudge; 0 finger, 1 pen, 2 mouse.
    pub tool: u8,
    pub pointer: u8,
    pub seed: u32,
    /// How far the canvas is turned on screen (radians).
    pub turn: f32,
}

/// The look of `brush` drawn by its engine, if that is on (asked of it once for its data, then
/// kept), with the engine's plugin and code. One that stops goes off, and the brush draws as the
/// app read it.
pub(crate) fn look_of(hooks: &Hooks, brush: &Brush) -> Option<(Arc<BrushLook>, u64)> {
    let outside = brush.engine.as_ref()?;
    let (plugin, running, index) = hooks.engine(&outside.engine)?;
    let id = data_id(&outside.data);
    if let Some(look) = hooks.look_kept(&outside.engine, id) {
        return Some((look, id));
    }
    let answer = at_work(&running).brush_load(index, &load_record(id, &outside.data));
    let record = match answer {
        Ok(record) => record?,
        Err(stop) => {
            hooks.stopped(&plugin, stop);
            return None;
        }
    };
    let Some(look) = read_look(&record, id) else {
        log::warn!("a brush engine's look of {} doesn't hold up", brush.name);
        return None;
    };
    let look = Arc::new(look);
    hooks.keep_look(&outside.engine, id, Arc::clone(&look));
    Some((look, id))
}

/// A stroke of `brush` drawn by its engine: the halves its first and second strokes take; none if
/// its engine is off or doesn't start it.
pub(crate) fn begin_stroke(
    hooks: &Arc<Hooks>,
    brush: &Brush,
    look: Arc<BrushLook>,
    id: u64,
    starting: Starting,
) -> Option<(EngineHalf, EngineHalf)> {
    let outside = brush.engine.as_ref()?;
    let (plugin, running, index) = hooks.engine(&outside.engine)?;
    let mut record = Record::default();
    let floats = |v: &[f32]| -> Vec<u8> { v.iter().flat_map(|f| f.to_le_bytes()).collect() };
    record
        .field(field::BEGIN_BRUSH, &id.to_le_bytes())
        .field(field::BEGIN_COLOUR, &floats(&starting.colour))
        .field(field::BEGIN_SECONDARY, &floats(&starting.secondary))
        .field(field::BEGIN_DIAMETER, &floats(&[starting.diameter]))
        .field(field::BEGIN_START_DIAMETER, &floats(&[starting.start_diameter]))
        .field(field::BEGIN_OPACITY, &floats(&[starting.opacity]))
        .field(field::BEGIN_TOOL, &[starting.tool])
        .field(field::BEGIN_POINTER, &[starting.pointer])
        .field(field::BEGIN_SEED, &starting.seed.to_le_bytes())
        .field(field::BEGIN_TURN, &floats(&[starting.turn]));
    let record = record.bytes();
    let begun = (|| {
        let mut code = at_work(&running);
        if let Some(number) = code.brush_begin(&record)? {
            return Ok(Some(number));
        }
        // its brush first, then again
        if code.brush_load(index, &load_record(id, &outside.data))?.is_none() {
            return Ok(None);
        }
        code.brush_begin(&record)
    })();
    let number = match begun {
        Ok(number) => number?,
        Err(stop) => {
            hooks.stopped(&plugin, stop);
            return None;
        }
    };
    let core = Arc::new(Mutex::new(StrokeCore {
        hooks: Arc::clone(hooks),
        plugin,
        running,
        number: Some(number),
        colour: starting.colour,
        look,
        first_ms: None,
        fed: 0,
        ended: false,
        laid: [VecDeque::new(), VecDeque::new()],
    }));
    let half = |second| EngineHalf { core: Arc::clone(&core), second, seen: 0 };
    Some((half(false), half(true)))
}

/// A stroke an engine draws: what both its halves share.
struct StrokeCore {
    hooks: Arc<Hooks>,
    plugin: String,
    running: Shared,
    /// Its number with the engine, until it ends or the plugin stops.
    number: Option<i32>,
    colour: [f32; 3],
    look: Arc<BrushLook>,
    /// When its first sample came (ms).
    first_ms: Option<f64>,
    /// How many samples have gone to the engine, and whether it has ended.
    fed: usize,
    ended: bool,
    /// The stamps each half has yet to take: the first stroke's, the second's.
    laid: [VecDeque<Stamp>; 2],
}

impl StrokeCore {
    /// `samples` handed to the engine, its dabs kept for the halves.
    fn feed(&mut self, samples: &[Sample]) {
        let Some(number) = self.number else { return };
        let first = *self.first_ms.get_or_insert(samples.first().map_or(0.0, |s| s.time_ms));
        let mut bytes = Vec::with_capacity(samples.len() * 32);
        for s in samples {
            for v in [s.pos.x, s.pos.y, s.pressure, s.tilt, s.azimuth, s.roll] {
                bytes.extend_from_slice(&v.to_le_bytes());
            }
            bytes.extend_from_slice(&(s.time_ms - first).max(0.0).to_le_bytes());
        }
        let dabs = at_work(&self.running).brush_add(number, &bytes);
        self.take(dabs);
    }

    fn end(&mut self) {
        if self.ended {
            return;
        }
        self.ended = true;
        let Some(number) = self.number else { return };
        let dabs = at_work(&self.running).brush_end(number);
        self.number = None;
        self.take(dabs);
    }

    fn take(&mut self, dabs: Result<Vec<u8>, Stop>) {
        let dabs = match dabs {
            Ok(dabs) => dabs,
            Err(stop) => {
                // off, and the stroke ends where it got to
                self.number = None;
                self.hooks.stopped(&self.plugin, stop);
                return;
            }
        };
        let first_ms = self.first_ms.unwrap_or(0.0);
        for dab in dabs.as_chunks::<DAB_BYTES>().0 {
            let flags = u32_of(dab.get(32..).unwrap_or_default()).unwrap_or(0);
            let second = flags & FOR_SECOND != 0;
            let look = &*self.look;
            let (tips, build) = match (second, &look.second) {
                (false, _) => (look.tips.layers.len(), look.build),
                (true, Some(s)) => (s.tips.layers.len(), s.build),
                (true, None) => continue,
            };
            if let Some(stamp) = stamp_of(dab, self.colour, tips, build, first_ms) {
                self.laid[usize::from(second)].push_back(stamp);
            }
        }
    }
}

impl Drop for StrokeCore {
    fn drop(&mut self) {
        // the engine lets go of a stroke the app let go of unended
        if let Some(number) = self.number.take() {
            let _ = at_work(&self.running).brush_end(number);
        }
    }
}

/// A dab (see the module's notes) as a stamp of a stroke whose brush's colour is `colour`, whose
/// tips number `tips` and whose stamps `build` up so: none if it isn't one.
fn stamp_of(dab: &[u8; DAB_BYTES], colour: [f32; 3], tips: usize, build: Build, first_ms: f64) -> Option<Stamp> {
    let word = |i: usize| -> [u8; 4] { dab.get(i * 4..i * 4 + 4).and_then(|b| b.try_into().ok()).unwrap_or([0; 4]) };
    let float = |i: usize| f32::from_le_bytes(word(i));
    let [x, y, diameter, angle, ratio, opacity, flow] = [0, 1, 2, 3, 4, 5, 6].map(float);
    let (tip, flags) = (u32::from_le_bytes(word(7)), u32::from_le_bytes(word(8)));
    let [r, g, b, strength, tx, ty] = [9, 10, 11, 12, 13, 14].map(float);
    if ![x, y, diameter].iter().all(|v| v.is_finite()) || diameter <= 0.0 || tips == 0 {
        return None;
    }
    let unit = |v: f32, none: f32| if v.is_finite() { v.clamp(0.0, 1.0) } else { none };
    let (opacity, flow) = (unit(opacity, 1.0), unit(flow, 1.0));
    let center = Vec2::new(x, y);
    let mut s = Stamp::round(center, (diameter * 0.5).min(16_384.0), 1.0);
    (s.alpha, s.transfer) = match build {
        Build::Over => (opacity * flow, 1.0),
        Build::Towards => (flow, opacity),
    };
    s.angle = if angle.is_finite() { angle } else { 0.0 };
    s.roundness = if ratio.is_finite() { ratio.clamp(0.05, 1.0) } else { 1.0 };
    s.flip = [flags & FLIP_ACROSS != 0, flags & FLIP_DOWN != 0];
    s.tip = tip.min(tips as u32 - 1);
    if flags & OWN_COLOUR != 0 {
        s.tint = tint_to(colour, [unit(r, 0.0), unit(g, 0.0), unit(b, 0.0)]);
    }
    // the texture read where the dab lands on the canvas, moved as far as the engine says
    let offset = if tx.is_finite() && ty.is_finite() { Vec2::new(tx, ty) } else { Vec2::ZERO };
    s.grain_offset = center + offset;
    s.grain_depth = unit(strength, 1.0);
    s.time_ms = first_ms;
    Some(s)
}

/// Hue (turns), saturation and brightness of `c`, as the stamp shader reckons them.
fn hsb([r, g, b]: [f32; 3]) -> [f32; 3] {
    let hi = r.max(g).max(b);
    let d = hi - r.min(g).min(b);
    if d <= 1e-5 {
        return [0.0, 0.0, hi];
    }
    let h = if hi == r {
        (g - b) / d
    } else if hi == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    [(h / 6.0).rem_euclid(1.0), d / hi, hi]
}

/// The tint taking `from` to `to` exactly: the hue turned to it, the saturation and brightness set.
fn tint_to(from: [f32; 3], to: [f32; 3]) -> Tint {
    let ([h0, ..], [h1, s1, b1]) = (hsb(from), hsb(to));
    Tint { hue: h1 - h0, saturation: [0.0, s1], brightness: [0.0, b1] }
}

/// One of a stroke's two halves (its first stroke, or its second), as the builder of each takes its
/// stamps: the samples go to the engine once, whichever half has them first.
#[derive(Debug)]
pub(crate) struct EngineHalf {
    core: Arc<Mutex<StrokeCore>>,
    second: bool,
    /// How many samples this half has been handed.
    seen: usize,
}

impl std::fmt::Debug for StrokeCore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StrokeCore").field("plugin", &self.plugin).field("number", &self.number).finish_non_exhaustive()
    }
}

impl EngineHalf {
    fn core(&self) -> MutexGuard<'_, StrokeCore> {
        self.core.lock().unwrap_or_else(|e| e.into_inner())
    }
}

impl OutsideEngine for EngineHalf {
    fn add(&mut self, samples: &[Sample]) -> Vec<Stamp> {
        let seen = self.seen + samples.len();
        let second = self.second;
        let mut core = self.core();
        // what the other half hasn't handed over yet
        if core.fed < seen {
            let new = samples.get(samples.len().saturating_sub(seen - core.fed)..).unwrap_or_default();
            core.feed(new);
            core.fed = seen;
        }
        let laid = core.laid.get_mut(usize::from(second)).map(std::mem::take).unwrap_or_default();
        drop(core);
        self.seen = seen;
        laid.into()
    }

    fn finish(&mut self) -> Vec<Stamp> {
        let second = self.second;
        let mut core = self.core();
        core.end();
        core.laid.get_mut(usize::from(second)).map(std::mem::take).unwrap_or_default().into()
    }
}

/// A stroke a plugin's engine draws: how its first stroke and its second (if it has one) are laid,
/// and the halves of the engine's stroke each builder takes.
pub(crate) struct OutsideStroke {
    pub style: StrokeStyle,
    pub second: Option<SecondStroke>,
    pub first: EngineHalf,
    pub second_half: EngineHalf,
}

impl crate::State {
    /// A stroke of `brush` with `tool` on `layer`, drawn by the brush engine that made the brush if
    /// that is on, at `diameter` canvas pixels (the size slider's, the brush scale in it): its
    /// style laid by what the engine says the brush looks like, its tips and texture on the GPU.
    /// None draws it as the app read it.
    pub(crate) fn outside_stroke(
        &mut self,
        brush: &Brush,
        tool: crate::ToolKind,
        layer: procraft_engine::document::LayerId,
        pointer: crate::PointerKind,
        diameter: f32,
        seed: u32,
    ) -> Option<OutsideStroke> {
        brush.engine.as_ref()?;
        let hooks = self.hooks.clone()?;
        let (look, id) = look_of(&hooks, brush)?;
        let sliders = self.sliders(tool);
        let start = brush.diameter(self.starting_sliders(tool).size);
        let opacity = sliders.opacity.clamp(0.0, 1.0);
        // a brush that erases does so with the paint tool
        let stroke_tool = match tool.stroke_tool() {
            StrokeTool::Paint if look.erases => StrokeTool::Erase,
            other => other,
        };
        let alpha_lock = self.doc.layer(layer).is_some_and(|l| l.alpha_lock);
        let paint = |build: Build, opacity: f32| {
            let mut style = StrokeStyle::new(stroke_tool, self.color, opacity);
            style.preserve_alpha = alpha_lock;
            style.secondary = self.secondary;
            style.preview = self.pad_strokes.is_some();
            style.wetness = StrokeStyle::wetness_at(opacity);
            style.rendering = match build {
                Build::Over => Rendering::BuildUp,
                Build::Towards => Rendering::Capped,
            };
            style
        };
        let mut style = paint(look.build, opacity);
        if let Some(blend) = look.blend {
            style.blend = blend.min(procraft_engine::brush::BRUSH_BEHIND);
        }
        let mut second = look.second.as_ref().map(|s| SecondStroke {
            style: paint(s.build, 1.0).as_second(),
            mode: s.combine,
            threshold: -1.0,
        });
        let (canvas, gpu) = (self.canvas.as_mut()?, self.gpu.as_ref()?);
        canvas.use_tip_set(gpu, &mut style, look.tips.id, look.tips.side, &layers(&look.tips));
        // tips the GPU can't hold leave the brush as the app read it
        style.full_shape?;
        if let Some(t) = &look.texture {
            canvas.use_full_grain(gpu, &mut style, t.id, t.side, &t.pixels);
            style.grain_mode = procraft_engine::brush::GrainMode::Moving;
            style.grain_scale = t.tile.max(1.0);
            style.grain_depth = 1.0;
            style.grain_blend = t.mode;
        }
        if let (Some(s), Some(look)) = (second.as_mut(), look.second.as_ref()) {
            canvas.use_tip_set(gpu, &mut s.style, look.tips.id, look.tips.side, &layers(&look.tips));
            if s.style.full_shape.is_none() {
                second = None;
            }
        }
        let starting = Starting {
            colour: self.color,
            secondary: self.secondary,
            diameter,
            start_diameter: start,
            opacity,
            tool: match tool.stroke_tool() {
                StrokeTool::Erase => 1,
                StrokeTool::Smudge => 2,
                _ => 0,
            },
            pointer: match pointer {
                crate::PointerKind::Finger => 0,
                crate::PointerKind::Mouse => 2,
                _ => 1,
            },
            seed,
            turn: self.view.map_or(0.0, |v| v.rotation()),
        };
        let (first, second_half) = begin_stroke(&hooks, brush, look, id, starting)?;
        Some(OutsideStroke { style, second, first, second_half })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tip(w: u32, h: u32) -> Vec<u8> {
        let mut t = Record::default();
        t.field(field::TIP_WIDTH, &w.to_le_bytes())
            .field(field::TIP_HEIGHT, &h.to_le_bytes())
            .field(field::TIP_PIXELS, &vec![255; (w * h) as usize]);
        t.bytes()
    }

    /// A Krita preset of a hard square tip 20 pixels across, spaced 2 diameters apart: a PNG with
    /// only its settings in it.
    pub(crate) fn square_preset() -> Vec<u8> {
        let xml = r#"<Preset name="Squares" paintopid="paintbrush"><param name="brush_definition" type="string"><![CDATA[<Brush type="auto_brush" spacing="2" useAutoSpacing="0" angle="0" density="1" randomness="0"><MaskGenerator type="rect" id="default" diameter="20" ratio="1" hfade="1" vfade="1" spikes="2" antialiasEdges="0"/></Brush>]]></param><param name="PaintOpAction" type="string"><![CDATA[1]]></param><param name="CompositeOp" type="string"><![CDATA[normal]]></param><param name="PressureSize" type="string"><![CDATA[false]]></param><param name="OpacityUseCurve" type="string"><![CDATA[false]]></param><param name="FlowUseCurve" type="string"><![CDATA[false]]></param></Preset>"#;
        let chunk = |kind: &[u8], data: &[u8]| {
            let mut c = (data.len() as u32).to_be_bytes().to_vec();
            c.extend_from_slice(kind);
            c.extend_from_slice(data);
            // the reader doesn't check sums
            c.extend_from_slice(&[0; 4]);
            c
        };
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        let mut header = Vec::new();
        for v in [1_u32, 1] {
            header.extend_from_slice(&v.to_be_bytes());
        }
        header.extend_from_slice(&[8, 0, 0, 0, 0]);
        png.extend(chunk(b"IHDR", &header));
        let mut text = b"preset\0".to_vec();
        text.extend_from_slice(xml.as_bytes());
        png.extend(chunk(b"tEXt", &text));
        png.extend(chunk(b"IEND", &[]));
        png
    }

    /// The packed Krita plugin at `KRITA_PLUGIN`, drawing a preset's squares where Krita lays
    /// them; with it off, the brush draws as read.
    /// `KRITA_PLUGIN=x.procraft-plugin cargo test -p procraft-core krita_squares -- --ignored`
    #[test]
    #[ignore]
    fn krita_squares_lie_where_the_engine_spaces_them() {
        let Ok(path) = std::env::var("KRITA_PLUGIN") else { return };
        let Ok(gpu) = procraft_gpu::Gpu::headless() else { return };
        let s = crate::CanvasSession::new(200, 120);
        {
            let mut st = s.shared.lock();
            st.window_size = (200, 120);
            st.view = Some(procraft_engine::view::View::fit((200, 120), (200, 120)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        let dir = std::env::temp_dir().join(format!("procraft-krita-plugin-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let plugins = crate::Plugins::with_store(dir.to_string_lossy().into_owned());
        assert_eq!(plugins.install(std::fs::read(path).unwrap()), None);
        let id = plugins.plugins().into_iter().find(|p| p.name == "Krita brushes").unwrap().id;
        assert!(plugins.set_on(id.clone(), true));
        s.use_plugins(Arc::clone(&plugins));
        let set = s.import_any_brushes(square_preset(), Some("Squares.kpp".to_owned()), 0).unwrap();
        assert!(s.shared.lock().library[set as usize].brushes[0].engine.is_some(), "made by the engine too");
        assert!(s.select_brush(set, 0));
        s.set_color(0.0, 0.0, 0.0);
        let draw = |s: &crate::CanvasSession| {
            let layer = s.add_layer();
            let at = |x: f32| crate::PenSample {
                x,
                y: 60.0,
                pressure: 1.0,
                tilt: 0.0,
                orientation: 0.0,
                time_ms: f64::from(x),
                roll: 0.0,
            };
            s.stroke_begin(crate::PointerKind::Stylus, vec![at(20.0)]);
            s.stroke_add((21..=180).map(|x| at(x as f32)).collect());
            s.stroke_end(vec![]);
            s.shared.lock().layer_pixels(procraft_engine::document::LayerId(layer)).unwrap()
        };
        let alpha = |rgba: &[u8], x: usize, y: usize| rgba[(y * 200 + x) * 4 + 3];
        let on = draw(&s);
        // a square at the start, 20 across; the next 40 on (twice its size), none between
        assert!(alpha(&on, 20, 60) > 240 && alpha(&on, 28, 52) > 200, "the first square");
        assert!(alpha(&on, 60, 60) > 240, "the next, 40 on");
        assert_eq!(alpha(&on, 40, 60), 0, "nothing between them");
        assert!(plugins.set_on(id, false));
        let off = draw(&s);
        assert!(alpha(&off, 40, 60) > 0, "as the app read it: a line");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_look_holds_its_tips_fitted_into_one_square_side() {
        let mut look = Record::default();
        look.field(field::TIP, &tip(100, 50)).field(field::TIP, &tip(30, 30)).field(field::BUILD, &[1]);
        look.field(field::BLEND, &3_u32.to_le_bytes()).field(field::ERASES, &[1]);
        let look = read_look(&look.bytes(), 7).unwrap();
        assert_eq!(look.tips.side, 128);
        assert_eq!(look.tips.layers.len(), 2);
        assert_eq!((look.build, look.blend, look.erases), (Build::Towards, Some(3), true));
        // the wide tip across all of the square, a quarter of it clear above and below
        let wide = &look.tips.layers[0];
        assert_eq!(wide[64 * 128 + 64], 255);
        assert_eq!(wide[10 * 128 + 64], 0);
        // a tip whose pixels aren't its size, or none, holds nothing up
        let mut bad = Record::default();
        bad.field(field::TIP, &tip(4, 4)[..20]);
        assert!(read_look(&bad.bytes(), 7).is_none());
        assert!(read_look(&[], 7).is_none());
    }

    #[test]
    fn a_dab_becomes_a_stamp_as_its_stroke_builds_up() {
        let mut dab = [0_u8; DAB_BYTES];
        let words: [f32; 7] = [10.0, 20.0, 8.0, 0.5, 0.5, 0.6, 0.25];
        for (i, v) in words.iter().enumerate() {
            dab[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
        }
        dab[28..32].copy_from_slice(&9_u32.to_le_bytes());
        dab[32..36].copy_from_slice(&(FLIP_ACROSS | OWN_COLOUR).to_le_bytes());
        for (i, v) in [0.0_f32, 0.0, 1.0, 0.5].iter().enumerate() {
            dab[36 + i * 4..40 + i * 4].copy_from_slice(&v.to_le_bytes());
        }
        let over = stamp_of(&dab, [1.0, 0.0, 0.0], 4, Build::Over, 0.0).unwrap();
        assert_eq!((over.center, over.radius, over.roundness), (Vec2::new(10.0, 20.0), 4.0, 0.5));
        assert!((over.alpha - 0.15).abs() < 1e-6 && over.transfer == 1.0);
        assert_eq!((over.tip, over.flip, over.grain_depth), (3, [true, false], 0.5));
        let towards = stamp_of(&dab, [1.0, 0.0, 0.0], 4, Build::Towards, 0.0).unwrap();
        assert_eq!((towards.alpha, towards.transfer), (0.25, 0.6));
        // red tinted to blue: the hue turned two thirds, kept full
        assert!((towards.tint.hue - 2.0 / 3.0).abs() < 1e-6);
        assert_eq!((towards.tint.saturation, towards.tint.brightness), ([0.0, 1.0], [0.0, 1.0]));
        // a dab of no size, or with no tips to draw, is none
        dab[8..12].copy_from_slice(&0.0_f32.to_le_bytes());
        assert!(stamp_of(&dab, [1.0; 3], 4, Build::Over, 0.0).is_none());
    }

    #[test]
    fn made_brushes_join_those_read_by_their_names() {
        let mut brushes = vec![Brush { name: "Pencil".into(), ..Brush::round_pen() }];
        let made = |name: &str| MadeBrush { engine: "a.b.c".into(), name: name.into(), data: vec![1], thumbnail: None };
        join_made(&mut brushes, vec![made("Pencil"), made("Ink")]);
        assert_eq!(brushes.len(), 2);
        assert_eq!(brushes[0].engine.as_ref().map(|e| e.engine.as_str()), Some("a.b.c"));
        assert_eq!(brushes[1].name, "Ink");
        assert!(brushes[1].engine.is_some());
    }
}

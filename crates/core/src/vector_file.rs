//! A vector layer's strokes and fills as an artwork's folder keeps them: `vectors/<layer>.json`
//! beside the layer's pixels (which stay what the strokes draw, so the artwork opens and shows at
//! once, and opens as plain pixels where vector layers aren't known).
//!
//! The file holds the layer's strokes and fills in the order they were made. A stroke is its
//! input (packed, as text), its ink, and how it was set up; the brushes, the drawing guides and
//! the inputs are kept once each and named by their place, so strokes drawn with one brush, or
//! pieces cut from one stroke, share them here as they do in memory.

use std::collections::BTreeMap;
use std::sync::Arc;

use make2d_engine::brush::{Brush, Grain, Shape, Taper, Tilt};
use make2d_engine::curve::Curve;
use make2d_engine::document::LayerId;
use make2d_engine::geom::{Homography, Vec2};
use make2d_engine::guide::Guide;
use make2d_engine::input::{Pointer, Sample};
use make2d_engine::stroke::InputSettings;
use make2d_engine::vector::{
    FillMode, Ink, Object, Second, Setup, StrokeId, TracedFrom, VectorContent, VectorFill, VectorStroke,
};
use serde::{Deserialize, Serialize};

use crate::State;
use crate::brush_library::SavedBrush;

/// A vector layer's file as last made: what told its content apart then (where it was, how much
/// it held and its next name), and the file.
pub(crate) type KeptFile = ((usize, usize, u64), Arc<Vec<u8>>);

/// The file's version: one of a later version than this is left alone (the layer opens as its
/// pixels).
const VERSION: u32 = 1;

/// The most a file may hold of each thing, so that one that isn't ours takes no more than this to
/// turn away.
const MOST_OBJECTS: usize = 1 << 20;
const MOST_SAMPLES: usize = 1 << 22;

/// A brush as a stroke keeps it: as the library keeps a brush (its controls, curves and images),
/// and beside that its numbers bit for bit, the brush's own and its second's. The library's form
/// goes through the controls' own scales, which bring a number back a hair off (and leave out
/// what a control doesn't reach): enough to lay a stamp a hair off, and a stroke drawn again
/// must lay every stamp where it was.
#[derive(Serialize, Deserialize)]
struct KeptBrush {
    brush: SavedBrush,
    numbers: Vec<u32>,
    extra: BTreeMap<String, u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second: Option<(Vec<u32>, BTreeMap<String, u32>)>,
}

/// `brush`'s numbers, in an order of their own: each to read, or to set.
fn numbers(brush: &mut Brush) -> Vec<&mut f32> {
    let Brush {
        spacing,
        streamline,
        pressure_smoothing,
        hardness,
        min_size,
        max_size,
        min_opacity,
        max_opacity,
        flow,
        touch_opacity,
        pressure_size,
        pressure_opacity,
        pen_taper,
        touch_taper,
        shape,
        grain,
        tilt,
        ..
    } = brush;
    let mut out = vec![
        spacing,
        streamline,
        pressure_smoothing,
        hardness,
        min_size,
        max_size,
        min_opacity,
        max_opacity,
        flow,
        touch_opacity,
        pressure_size,
        pressure_opacity,
    ];
    for taper in [pen_taper, touch_taper] {
        let Taper { start, end, size, opacity } = taper;
        out.extend([start, end, size, opacity]);
    }
    let Shape { rotation, follow, roundness, scatter, throw, size_jitter, opacity_jitter, .. } = shape;
    out.extend([rotation, follow, roundness, scatter, throw, size_jitter, opacity_jitter]);
    let Grain { scale, depth, .. } = grain;
    out.extend([scale, depth]);
    let Tilt { angle, size, opacity } = tilt;
    out.extend([angle, size, opacity]);
    out
}

/// A brush's numbers and its further controls, bit for bit.
fn exact(brush: &Brush) -> (Vec<u32>, BTreeMap<String, u32>) {
    let mut brush = brush.clone();
    let extra = brush.extra.iter().map(|(key, value)| (key.clone(), value.to_bits())).collect();
    (numbers(&mut brush).into_iter().map(|n| n.to_bits()).collect(), extra)
}

/// `brush` given the numbers kept of it, where they are as many as it has (else it stays as the
/// library's form made it).
fn set_exact(brush: &mut Brush, kept: &[u32], extra: &BTreeMap<String, u32>) {
    let mine = numbers(brush);
    if mine.len() == kept.len() {
        for (number, bits) in mine.into_iter().zip(kept) {
            *number = f32::from_bits(*bits);
        }
    }
    // (its further controls are those it had, no more: making it again may have noted one at
    // the value it has when it isn't noted)
    brush.extra = extra.iter().map(|(key, bits)| (key.clone(), f32::from_bits(*bits))).collect();
}

#[derive(Serialize, Deserialize)]
struct SavedVector {
    version: u32,
    next: u64,
    brushes: Vec<KeptBrush>,
    guides: Vec<Guide>,
    /// Each stroke's input, packed (see [`packed`]).
    samples: Vec<String>,
    objects: Vec<SavedObject>,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
enum SavedObject {
    Stroke(Box<SavedStroke>),
    Fill(SavedFill),
}

#[derive(Serialize, Deserialize)]
struct SavedInput {
    stabilization: f32,
    motion_filtering: f32,
    motion_expression: f32,
    pressure_smoothing: f32,
    pressure_curve: Vec<[f32; 2]>,
    speed_distance: f32,
    tip_attached: bool,
}

#[derive(Serialize, Deserialize)]
struct SavedSetup {
    /// Which of the file's brushes.
    brush: usize,
    diameter: f32,
    /// 0 a finger, 1 a pen, 2 a pen's eraser end, 3 a mouse.
    pointer: u8,
    seed: u32,
    scale: f32,
    input: SavedInput,
    view_turn: f32,
    points_per_pixel: f32,
}

#[derive(Serialize, Deserialize)]
struct SavedStroke {
    id: u64,
    setup: SavedSetup,
    /// Which of the file's inputs.
    samples: usize,
    colour: [f32; 3],
    secondary: [f32; 3],
    opacity: f32,
    #[serde(default)]
    erases: bool,
    #[serde(default)]
    preserve_alpha: bool,
    wetness: f32,
    /// Which of the file's guides drew copies of it, if one did.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    guide: Option<usize>,
    /// A dual brush's second: how it was set up, and its opacity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    second: Option<(SavedSetup, f32)>,
    /// Where it lies, if not where it was drawn: the map's nine numbers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    placement: Option<[f64; 9]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    shown: Option<[f32; 2]>,
    /// An eraser's: the strokes and fills it takes from, if it names them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    takes_from: Option<Vec<u64>>,
}

#[derive(Serialize, Deserialize)]
struct SavedFill {
    id: u64,
    loops: Vec<Vec<[f32; 2]>>,
    colour: [f32; 3],
    opacity: f32,
    /// 0 over, 1 behind, 2 erasing.
    mode: u8,
    #[serde(default)]
    hard: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    traced: Option<SavedTraced>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    takes_from: Option<Vec<u64>>,
}

#[derive(Serialize, Deserialize)]
struct SavedTraced {
    seed: [f32; 2],
    factor: f32,
    sources: Vec<u64>,
}

/// A stroke's input as text: each sample its place, pressure, tilt, azimuth and roll (32-bit
/// floats) and its time (a 64-bit one), little-endian, the lot in base64.
fn packed(samples: &[Sample]) -> String {
    let mut bytes = Vec::with_capacity(samples.len() * 32);
    for s in samples {
        for v in [s.pos.x, s.pos.y, s.pressure, s.tilt, s.azimuth, s.roll] {
            bytes.extend(v.to_le_bytes());
        }
        bytes.extend(s.time_ms.to_le_bytes());
    }
    make2d_engine::base64::encode(&bytes)
}

fn unpacked(text: &str) -> Option<Vec<Sample>> {
    let bytes = make2d_engine::base64::decode(text)?;
    let (samples, rest) = bytes.as_chunks::<32>();
    if !rest.is_empty() || samples.len() > MOST_SAMPLES {
        return None;
    }
    let out: Vec<Sample> = samples
        .iter()
        .map(|sample| {
            let (floats, time) = sample.split_at(24);
            let mut values = floats.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b));
            let mut next = || values.next().unwrap_or(0.0);
            let (x, y, pressure, tilt, azimuth, roll) = (next(), next(), next(), next(), next(), next());
            let time_ms = time.as_chunks::<8>().0.first().map_or(0.0, |b| f64::from_le_bytes(*b));
            Sample { pos: Vec2::new(x, y), pressure, tilt, azimuth, roll, time_ms }
        })
        .collect();
    // (a stroke never kept a sample that was no number)
    out.iter().all(Sample::is_finite).then_some(out)
}

fn pointer_number(pointer: Pointer) -> u8 {
    match pointer {
        Pointer::Finger => 0,
        Pointer::Stylus => 1,
        Pointer::StylusEraser => 2,
        Pointer::Mouse => 3,
    }
}

fn pointer_of(number: u8) -> Pointer {
    match number {
        1 => Pointer::Stylus,
        2 => Pointer::StylusEraser,
        3 => Pointer::Mouse,
        _ => Pointer::Finger,
    }
}

/// Things kept once each and named by their place: one already there (the same one, or one equal
/// to it) is named, else it is added.
struct Once<T> {
    seen: Vec<Arc<T>>,
}

impl<T: PartialEq> Once<T> {
    fn place(&mut self, thing: &Arc<T>) -> usize {
        let at = self.seen.iter().position(|kept| Arc::ptr_eq(kept, thing) || **kept == **thing);
        at.unwrap_or_else(|| {
            self.seen.push(Arc::clone(thing));
            self.seen.len() - 1
        })
    }
}

impl State {
    /// `brush` as a stroke keeps it.
    fn brush_whole(&self, brush: &Brush) -> KeptBrush {
        let (numbers, extra) = exact(brush);
        KeptBrush { brush: self.brush_kept(brush), numbers, extra, second: brush.secondary.as_deref().map(exact) }
    }

    /// A brush a stroke kept, made again as it was.
    fn brush_again(&self, kept: KeptBrush) -> Brush {
        let mut brush = self.brush_made(kept.brush);
        set_exact(&mut brush, &kept.numbers, &kept.extra);
        if let (Some(second), Some((numbers, extra))) = (brush.secondary.as_deref_mut(), kept.second.as_ref()) {
            set_exact(second, numbers, extra);
        }
        brush
    }

    /// Vector layer `layer`'s strokes and fills as its file keeps them; none if it isn't one.
    pub(crate) fn vector_file(&self, layer: LayerId) -> Option<Vec<u8>> {
        let content = self.doc.layer(layer)?.vector.as_ref()?;
        let mut brushes = Once { seen: Vec::<Arc<Brush>>::new() };
        let mut guides = Once { seen: Vec::<Arc<Guide>>::new() };
        let mut inputs: Vec<Arc<Vec<Sample>>> = Vec::new();
        let mut setup = |setup: &Setup| SavedSetup {
            brush: brushes.place(&setup.brush),
            diameter: setup.diameter,
            pointer: pointer_number(setup.pointer),
            seed: setup.seed,
            scale: setup.scale,
            input: SavedInput {
                stabilization: setup.input.stabilization,
                motion_filtering: setup.input.motion_filtering,
                motion_expression: setup.input.motion_expression,
                pressure_smoothing: setup.input.pressure_smoothing,
                pressure_curve: setup.input.pressure_curve.points().iter().map(|p| [p.x, p.y]).collect(),
                speed_distance: setup.input.speed_distance,
                tip_attached: setup.input.tip_attached,
            },
            view_turn: setup.view_turn,
            points_per_pixel: setup.points_per_pixel,
        };
        let mut objects = Vec::with_capacity(content.len());
        for object in content.objects() {
            objects.push(match object {
                Object::Stroke(stroke) => {
                    // (pieces cut from one stroke share its input: kept once)
                    let shared = stroke.samples_shared();
                    let samples = inputs.iter().position(|kept| Arc::ptr_eq(kept, shared)).unwrap_or_else(|| {
                        inputs.push(Arc::clone(shared));
                        inputs.len() - 1
                    });
                    SavedObject::Stroke(Box::new(SavedStroke {
                        id: stroke.id.0,
                        setup: setup(&stroke.setup),
                        samples,
                        colour: stroke.ink.colour,
                        secondary: stroke.ink.secondary,
                        opacity: stroke.ink.opacity,
                        erases: stroke.ink.erases,
                        preserve_alpha: stroke.ink.preserve_alpha,
                        wetness: stroke.ink.wetness,
                        guide: stroke.guide.as_ref().map(|guide| guides.place(guide)),
                        second: stroke.second.as_ref().map(|second| (setup(&second.setup), second.opacity)),
                        placement: (!stroke.placement.is_identity()).then(|| stroke.placement.rows()),
                        shown: stroke.shown.map(|(from, to)| [from, to]),
                        takes_from: stroke.takes_from.as_ref().map(|ids| ids.iter().map(|id| id.0).collect()),
                    }))
                }
                Object::Fill(fill) => SavedObject::Fill(SavedFill {
                    id: fill.id.0,
                    loops: fill.loops.iter().map(|outline| outline.iter().map(|p| [p.x, p.y]).collect()).collect(),
                    colour: fill.colour,
                    opacity: fill.opacity,
                    mode: match fill.mode {
                        FillMode::Over => 0,
                        FillMode::Behind => 1,
                        FillMode::Erase => 2,
                    },
                    hard: fill.hard,
                    traced: fill.traced.as_ref().map(|traced| SavedTraced {
                        seed: [traced.seed.x, traced.seed.y],
                        factor: traced.factor,
                        sources: traced.sources.iter().map(|id| id.0).collect(),
                    }),
                    takes_from: fill.takes_from.as_ref().map(|ids| ids.iter().map(|id| id.0).collect()),
                }),
            });
        }
        let file = SavedVector {
            version: VERSION,
            next: content.next_id().0,
            brushes: brushes.seen.iter().map(|brush| self.brush_whole(brush)).collect(),
            guides: guides.seen.iter().map(|guide| (**guide).clone()).collect(),
            samples: inputs.iter().map(|samples| packed(samples)).collect(),
            objects,
        };
        serde_json::to_vec(&file).ok()
    }

    /// [`State::vector_file`], made once for the layer as it is and kept until it changes (a save
    /// that finds a layer's strokes as they were has nothing to write out again).
    pub(crate) fn vector_file_kept(&mut self, layer: LayerId) -> Option<Arc<Vec<u8>>> {
        let content = self.doc.layer(layer)?.vector.as_ref()?;
        // (the layer's content is shared until it is changed, then made anew: where it is, how
        // much it holds and its next name tell one from the next)
        let told = (Arc::as_ptr(content) as usize, content.len(), content.next_id().0);
        if let Some((kept, bytes)) = self.vector_files.get(&layer)
            && *kept == told
        {
            return Some(Arc::clone(bytes));
        }
        let bytes = Arc::new(self.vector_file(layer)?);
        self.vector_files.insert(layer, (told, Arc::clone(&bytes)));
        Some(bytes)
    }

    /// Layer `layer` opened with a file of strokes and fills (`bytes`): it is a vector layer
    /// holding them. False if the file can't be read (the layer stays its pixels).
    pub(crate) fn vector_opened(&mut self, layer: LayerId, bytes: &[u8]) -> bool {
        let Some(content) = self.vector_read(bytes) else { return false };
        self.doc.set_vector(layer, content)
    }

    /// What a vector layer's file (`bytes`) holds, made again; none for a file that isn't one, is
    /// of a later version, or doesn't hold together (the layer is then its pixels).
    pub(crate) fn vector_read(&self, bytes: &[u8]) -> Option<VectorContent> {
        let file: SavedVector = serde_json::from_slice(bytes).ok()?;
        if file.version > VERSION || file.objects.len() > MOST_OBJECTS {
            return None;
        }
        let brushes: Vec<Arc<Brush>> = file.brushes.into_iter().map(|kept| Arc::new(self.brush_again(kept))).collect();
        let guides: Vec<Arc<Guide>> = file.guides.into_iter().map(Arc::new).collect();
        let inputs: Vec<Arc<Vec<Sample>>> =
            file.samples.iter().map(|text| unpacked(text).map(Arc::new)).collect::<Option<_>>()?;
        let finite = |values: &[f32]| values.iter().all(|v| v.is_finite());
        let setup = |saved: &SavedSetup| -> Option<Setup> {
            let input = &saved.input;
            let numbers = [saved.diameter, saved.scale, saved.view_turn, saved.points_per_pixel];
            let smoothing = [input.stabilization, input.motion_filtering, input.motion_expression];
            if !finite(&numbers) || !finite(&smoothing) || !finite(&[input.pressure_smoothing, input.speed_distance]) {
                return None;
            }
            let curve: Vec<Vec2> = input.pressure_curve.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
            Some(Setup {
                brush: Arc::clone(brushes.get(saved.brush)?),
                diameter: saved.diameter,
                pointer: pointer_of(saved.pointer),
                seed: saved.seed,
                scale: saved.scale,
                input: InputSettings {
                    stabilization: input.stabilization,
                    motion_filtering: input.motion_filtering,
                    motion_expression: input.motion_expression,
                    pressure_smoothing: input.pressure_smoothing,
                    pressure_curve: if curve.len() >= 2 { Curve::new(&curve) } else { Curve::linear() },
                    speed_distance: input.speed_distance,
                    tip_attached: input.tip_attached,
                },
                view_turn: saved.view_turn,
                points_per_pixel: saved.points_per_pixel,
            })
        };
        let mut objects = Vec::with_capacity(file.objects.len());
        for object in file.objects {
            objects.push(match object {
                SavedObject::Stroke(saved) => {
                    let colours = [saved.colour, saved.secondary].concat();
                    if !finite(&colours) || !finite(&[saved.opacity, saved.wetness]) {
                        return None;
                    }
                    let ink = Ink {
                        colour: saved.colour,
                        secondary: saved.secondary,
                        opacity: saved.opacity,
                        erases: saved.erases,
                        preserve_alpha: saved.preserve_alpha,
                        wetness: saved.wetness,
                    };
                    let samples = Arc::clone(inputs.get(saved.samples)?);
                    let mut stroke = VectorStroke::restored(StrokeId(saved.id), setup(&saved.setup)?, samples, ink);
                    stroke.guide = match saved.guide {
                        Some(at) => Some(Arc::clone(guides.get(at)?)),
                        None => None,
                    };
                    stroke.second = match &saved.second {
                        Some((second, opacity)) if opacity.is_finite() => {
                            Some(Second { setup: setup(second)?, opacity: *opacity })
                        }
                        Some(_) => return None,
                        None => None,
                    };
                    if let Some(rows) = saved.placement {
                        let map = Homography::from_rows(rows);
                        if !map.is_finite() || map.determinant().abs() <= 1e-12 {
                            return None;
                        }
                        stroke.placement = map;
                    }
                    stroke.shown = match saved.shown {
                        Some([from, to]) if from.is_finite() && to.is_finite() && to > from => Some((from, to)),
                        Some(_) => return None,
                        None => None,
                    };
                    stroke.takes_from = saved.takes_from.map(|ids| ids.into_iter().map(StrokeId).collect());
                    Object::Stroke(Arc::new(stroke))
                }
                SavedObject::Fill(saved) => {
                    if !finite(&saved.colour) || !saved.opacity.is_finite() {
                        return None;
                    }
                    let loops: Vec<Vec<Vec2>> = saved
                        .loops
                        .iter()
                        .map(|outline| outline.iter().map(|&[x, y]| Vec2::new(x, y)).collect())
                        .collect();
                    if !loops.iter().flatten().all(|p: &Vec2| p.is_finite()) {
                        return None;
                    }
                    Object::Fill(Arc::new(VectorFill {
                        id: StrokeId(saved.id),
                        loops,
                        colour: saved.colour,
                        opacity: saved.opacity,
                        mode: match saved.mode {
                            1 => FillMode::Behind,
                            2 => FillMode::Erase,
                            _ => FillMode::Over,
                        },
                        hard: saved.hard,
                        traced: saved.traced.filter(|t| finite(&[t.seed[0], t.seed[1], t.factor])).map(|t| {
                            TracedFrom {
                                seed: Vec2::new(t.seed[0], t.seed[1]),
                                factor: t.factor,
                                sources: t.sources.into_iter().map(StrokeId).collect(),
                            }
                        }),
                        takes_from: saved.takes_from.map(|ids| ids.into_iter().map(StrokeId).collect()),
                    }))
                }
            });
        }
        Some(VectorContent::restored(objects, file.next))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::CanvasSession;

    #[test]
    fn every_brush_a_stroke_keeps_comes_back_as_it_was() {
        let s = CanvasSession::new(64, 64);
        let st = s.shared.lock();
        let mut tried = 0;
        for set in make2d_engine::library::library() {
            for mut brush in set.brushes {
                // (as a stroke has it: the opacity slider in it)
                brush.touch_opacity = 0.37;
                brush.flow *= 0.613;
                let text = serde_json::to_string(&st.brush_whole(&brush)).unwrap();
                let again = st.brush_again(serde_json::from_str(&text).unwrap());
                if again != brush {
                    let (was, now) = (format!("{brush:#?}"), format!("{again:#?}"));
                    let differ: Vec<String> = was
                        .lines()
                        .zip(now.lines())
                        .filter(|(a, b)| a != b)
                        .map(|(a, b)| format!("{} => {}", a.trim(), b.trim()))
                        .take(8)
                        .collect();
                    panic!("{}: {} came back otherwise: {differ:?}", set.name, brush.name);
                }
                tried += 1;
            }
        }
        assert!(tried > 20, "{tried}");
    }

    #[test]
    fn a_stroke_s_input_is_packed_and_read_back_bit_for_bit() {
        let samples: Vec<Sample> = (0..50)
            .map(|i| Sample {
                pos: Vec2::new(i as f32 * 1.37 + 0.123_456_7, 300.0 - i as f32 * 0.91),
                pressure: (i as f32 * 0.07).sin().abs(),
                tilt: 0.3,
                azimuth: -1.2 + i as f32 * 0.01,
                roll: 0.05,
                time_ms: 123_456.789 + f64::from(i) * 4.166_667,
            })
            .collect();
        assert_eq!(unpacked(&packed(&samples)).unwrap(), samples);
        assert_eq!(unpacked(&packed(&[])).unwrap(), Vec::new());
        // cut short, or holding what is no number, it is none
        assert!(unpacked("AAAA").is_none());
        let bad = Sample { pressure: f32::NAN, ..samples[0] };
        assert!(unpacked(&packed(&[bad])).is_none());
    }
}

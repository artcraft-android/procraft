//! Strokes kept as strokes: what a vector layer holds in place of pixels.
//!
//! A kept stroke is its input and how it was set up (the brush, its size, the seed its randomness
//! is drawn from, the smoothing and the view it was drawn in), not the shape it left. Drawing it
//! again is the brush engine drawing it again ([`VectorStroke::replay`]): the same builder given
//! the same samples lays the same stamps, so a kept stroke looks as the brush painted it, whatever
//! the brush. Where it lies on the canvas is a map of its own ([`VectorStroke::placement`]): moved,
//! turned, resized or pulled out of square, it is the same stamps placed elsewhere
//! ([`Replay::placed_stamps`]), not a stroke drawn anew.
//!
//! What finds, cuts and fills against a stroke goes by the path its stamps follow
//! ([`Replay::path`]), the line the brush's smoothing left of the input.

use std::sync::{Arc, OnceLock};

use crate::brush::Brush;
use crate::geom::{Homography, IRect, Vec2};
use crate::guide::Guide;
use crate::input::{Pointer, Sample};
use crate::stroke::{InputSettings, PathSpot, Stamp, StrokeBuilder};
use crate::vector_fill::{self, Wall};

/// A kept stroke's name in its layer: never reused there, so what refers to a stroke (a fill's
/// walls, an eraser's marks) still means it after others come and go.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StrokeId(pub u64);

/// How a stroke was set up as it began: all its builder was given but the samples. Strokes drawn
/// with one brush share it.
#[derive(Clone, Debug, PartialEq)]
pub struct Setup {
    pub brush: Arc<Brush>,
    /// The stamp's diameter at full pressure, canvas pixels (the brush's scale on its canvas
    /// already in it).
    pub diameter: f32,
    pub pointer: Pointer,
    /// What the stroke's scatter, jitter and random turns are drawn from.
    pub seed: u32,
    /// How much the brush was scaled on its canvas (see [`StrokeBuilder::scaled`]).
    pub scale: f32,
    /// The app's own smoothing and pressure, as they were set.
    pub input: InputSettings,
    /// How far the canvas was turned on screen, radians (see [`StrokeBuilder::on_view_turned`]).
    pub view_turn: f32,
    /// Screen points a canvas pixel spanned (see [`StrokeBuilder::seen_at`]).
    pub points_per_pixel: f32,
}

impl Setup {
    /// A plain setup for `brush` at `diameter`: no scale, no smoothing of the app's, the canvas
    /// upright at one screen point a pixel.
    pub fn new(brush: Arc<Brush>, diameter: f32, pointer: Pointer, seed: u32) -> Setup {
        Setup {
            brush,
            diameter,
            pointer,
            seed,
            scale: 1.0,
            input: InputSettings::default(),
            view_turn: 0.0,
            points_per_pixel: 1.0,
        }
    }

    /// The builder a stroke set up this way begins with: the one that drew it, and the one that
    /// draws it again.
    pub fn builder(&self) -> StrokeBuilder {
        StrokeBuilder::with_seed((*self.brush).clone(), self.diameter, self.pointer, self.seed)
            .scaled(self.scale)
            .with_input(self.input.clone())
            .on_view_turned(self.view_turn)
            .seen_at(self.points_per_pixel)
    }
}

/// What a stroke paints with, as it was when the stroke began: all that how it is drawn takes
/// besides its brush.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ink {
    /// Its colour and the secondary one, 0..1 each, as the canvas keeps them.
    pub colour: [f32; 3],
    pub secondary: [f32; 3],
    /// The stroke's opacity (what its stamps build up to), as the brush made it of the slider.
    pub opacity: f32,
    /// It takes paint away (the eraser's).
    pub erases: bool,
    /// Its layer's alpha lock was on: it only recolours what is there.
    pub preserve_alpha: bool,
    /// How far wet paint's stamps lay, by the opacity slider.
    pub wetness: f32,
}

/// A dual brush's second stroke: how it was set up, and its opacity (as its brush made it of the
/// slider).
#[derive(Clone, Debug, PartialEq)]
pub struct Second {
    pub setup: Setup,
    pub opacity: f32,
}

/// One stroke of a vector layer.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorStroke {
    pub id: StrokeId,
    pub setup: Setup,
    /// The input as the builder was given it, in the stroke's own space (the canvas as it was
    /// when the stroke was drawn). Pieces cut from a stroke share it.
    samples: Arc<Vec<Sample>>,
    /// The whole stroke's path in its own space, worked out once (pieces share it too).
    path: KeptPath,
    /// The canvas pixels its ink can touch as it lies now, worked out once.
    bounds: KeptBounds,
    pub ink: Ink,
    /// The drawing guide whose symmetry drew copies of its stamps, if one did.
    pub guide: Option<Arc<Guide>>,
    /// A dual brush's second stroke, drawn with the same samples.
    pub second: Option<Second>,
    /// An eraser's: the strokes and fills it takes from, by name. None: all that were made before
    /// it, which is so until something it touched is carried elsewhere (see
    /// [`VectorContent::place`]); from then on its marks belong to those it names, and go where
    /// they go.
    pub takes_from: Option<Vec<StrokeId>>,
    /// Where the stroke lies now: its own space to the canvas.
    pub placement: Homography,
    /// The part of the stroke that shows, as a span of its path (from and to, pixels along it in
    /// its own space); none shows all of it. A piece cut from a stroke is the whole stroke drawn
    /// and only its span kept, so a cut leaves both sides as they were: no taper grows on the new
    /// ends and nothing is scattered afresh.
    pub shown: Option<(f32, f32)>,
}

impl VectorStroke {
    /// A stroke beginning, nothing drawn yet, lying where it is drawn.
    pub fn begin(id: StrokeId, setup: Setup, ink: Ink) -> VectorStroke {
        VectorStroke {
            id,
            setup,
            samples: Arc::default(),
            path: KeptPath::default(),
            bounds: KeptBounds::default(),
            ink,
            guide: None,
            second: None,
            takes_from: None,
            placement: Homography::IDENTITY,
            shown: None,
        }
    }

    /// A stroke read back from where it was kept: its input whole, lying where it was drawn (what
    /// else it had is set on it after).
    pub fn restored(id: StrokeId, setup: Setup, samples: Arc<Vec<Sample>>, ink: Ink) -> VectorStroke {
        VectorStroke { samples, ..VectorStroke::begin(id, setup, ink) }
    }

    /// Its input, as shared with the pieces cut from it.
    pub fn samples_shared(&self) -> &Arc<Vec<Sample>> {
        &self.samples
    }

    /// More of the stroke's input, as it is handed to its builder (which skips samples that are
    /// no numbers, so they are not kept either).
    pub fn record(&mut self, samples: &[Sample]) {
        Arc::make_mut(&mut self.samples).extend(samples.iter().filter(|s| s.is_finite()));
        self.path = KeptPath::default();
        self.bounds = KeptBounds::default();
    }

    /// The whole stroke's path in its own space (what [`VectorStroke::shown`] is a span of),
    /// worked out the first time it is asked for. A stroke's setup is not to change once it is.
    pub fn whole_path(&self) -> Arc<Vec<PathSpot>> {
        Arc::clone(self.path.0.get_or_init(|| {
            let mut builder = self.setup.builder().traced();
            builder.add(&self.samples);
            builder.finish();
            Arc::new(builder.trace().to_vec())
        }))
    }

    /// The path of what shows of the stroke, in its own space.
    pub fn path(&self) -> Vec<PathSpot> {
        let whole = self.whole_path();
        match self.shown {
            None => whole.to_vec(),
            Some((from, to)) => span_of(&whole, from, to),
        }
    }

    /// The line the stroke follows, on the canvas.
    pub fn line(&self) -> Vec<Vec2> {
        self.path().iter().filter_map(|spot| self.placement.apply(spot.pos)).collect()
    }

    /// The path of what shows of the stroke as it lies on the canvas: each spot where the
    /// placement puts it, as wide as it makes it (how far along stays as in its own space).
    pub fn placed_path(&self) -> Vec<PathSpot> {
        placed_spots(&self.path(), self.placement)
    }

    /// The canvas rectangle its ink keeps to, near enough (left, top, right, bottom): its path
    /// and twice its width about it, for what its brush scatters. None for a stroke with no path.
    pub fn reach(&self) -> Option<[f32; 4]> {
        let mut reach: Option<[f32; 4]> = None;
        for spot in self.placed_path() {
            let (p, r) = (spot.pos, spot.radius * 2.0 + 1.0);
            let [x0, y0, x1, y1] = reach.unwrap_or([p.x - r, p.y - r, p.x + r, p.y + r]);
            reach = Some([x0.min(p.x - r), y0.min(p.y - r), x1.max(p.x + r), y1.max(p.y + r)]);
        }
        reach
    }

    /// The place on the stroke's path nearest `point` (canvas pixels): see [`Replay::nearest`].
    pub fn nearest(&self, point: Vec2) -> Option<Near> {
        nearest_on(&self.path(), self.placement, point)
    }

    pub fn samples(&self) -> &[Sample] {
        &self.samples
    }

    /// The stroke drawn again: every stamp it ends with, and the path they follow.
    pub fn replay(&self) -> Replay {
        let mut builder = self.setup.builder().traced();
        builder.add(&self.samples);
        let stamps = builder.finish();
        let whole = Arc::clone(self.path.0.get_or_init(|| Arc::new(builder.trace().to_vec())));
        let path = whole.to_vec();
        match self.shown {
            None => Replay { stamps, path, placement: self.placement },
            Some((from, to)) => Replay {
                stamps: stamps.into_iter().filter(|s| s.distance >= from && s.distance <= to).collect(),
                path: span_of(&path, from, to),
                placement: self.placement,
            },
        }
    }

    /// A dual brush's second stroke drawn again: its stamps, none if the stroke has no second.
    /// (Its path is the first's.)
    pub fn replay_second(&self) -> Option<Vec<Stamp>> {
        let mut builder = self.second.as_ref()?.setup.builder();
        builder.add(&self.samples);
        let stamps = builder.finish();
        Some(match self.shown {
            None => stamps,
            Some((from, to)) => stamps.into_iter().filter(|s| s.distance >= from && s.distance <= to).collect(),
        })
    }

    /// The stroke carried by `map` (canvas to canvas): moved, turned, resized, stretched or pulled
    /// out of square, the same stamps as before.
    pub fn place(&mut self, map: Homography) {
        if map.is_finite() && map.determinant().abs() > 1e-12 {
            self.placement = self.placement.then(map);
            self.bounds = KeptBounds::default();
        }
    }

    /// The canvas pixels the stroke's ink can touch as it lies now: every stamp of it, its second's
    /// and the copies its guide's symmetry draws of them. Worked out the first time it is asked
    /// for (the stroke is drawn again to find it), and kept until the stroke is placed elsewhere.
    /// What it has (its guide, its second, the span of it that shows) is not to change once it is.
    pub fn ink_bounds(&self) -> IRect {
        *self.bounds.0.get_or_init(|| {
            let mut all = IRect::EMPTY;
            let stamps = self.replay().placed_stamps().into_iter().chain(self.placed_second().unwrap_or_default());
            for stamp in stamps {
                all = all.union(stamp.bounds());
                for (at, _, _) in self.guide.iter().flat_map(|guide| guide.copies(stamp.center)) {
                    all = all.union(Stamp { center: at, ..stamp }.bounds());
                }
            }
            all
        })
    }

    /// The stroke as drawn with another brush, size or ink: the same input, set up as `setup` and
    /// painting with `ink` (and a dual brush's `second`). What showed of a piece still shows, by
    /// its share of the way along (the path may come out a little longer or shorter).
    pub fn restyled(&self, setup: Setup, ink: Ink, second: Option<Second>) -> VectorStroke {
        let length = |stroke: &VectorStroke| stroke.whole_path().last().map_or(0.0, |spot| spot.distance);
        let was = length(self);
        let mut now = VectorStroke {
            setup,
            ink,
            second,
            shown: None,
            path: KeptPath::default(),
            bounds: KeptBounds::default(),
            ..self.clone()
        };
        if let Some((from, to)) = self.shown
            && was > 0.0
        {
            let by = length(&now) / was;
            now.shown = Some((from * by, to * by));
        }
        now
    }

    /// Places along what shows of the stroke to take hold of it by, on the canvas: its two ends,
    /// and between them where it bends (where a straight line between two of them would stray
    /// more than `tolerance` pixels from it), no closer together than `apart`, and along a stretch
    /// longer than `most` enough more that none is further than that from the next. Each with how
    /// far along the path it is (the stroke's own space) and half the stroke's width there.
    pub fn nodes(&self, tolerance: f32, apart: f32, most: f32) -> Vec<PathSpot> {
        let path = self.placed_path();
        let (Some(first), Some(last)) = (path.first(), path.last()) else { return Vec::new() };
        if path.len() < 3 {
            return if path.len() == 1 { vec![*first] } else { vec![*first, *last] };
        }
        let mut keep = vec![false; path.len()];
        let mut spans = vec![(0, path.len() - 1)];
        while let Some((a, b)) = spans.pop() {
            let (Some(pa), Some(pb)) = (path.get(a), path.get(b)) else { continue };
            let along = pb.pos - pa.pos;
            let length = along.length().max(1e-6);
            let mut worst = (0.0, None);
            for (i, spot) in path.iter().enumerate().take(b).skip(a + 1) {
                let off = ((spot.pos - pa.pos).cross(along)).abs() / length;
                if off > worst.0 {
                    worst = (off, Some(i));
                }
            }
            if let (off, Some(i)) = worst
                && off > tolerance
            {
                if let Some(k) = keep.get_mut(i) {
                    *k = true;
                }
                spans.push((a, i));
                spans.push((i, b));
            }
        }
        let mut nodes = vec![*first];
        for (spot, _) in path.iter().zip(&keep).filter(|(_, kept)| **kept) {
            let near_last = last.pos.distance(spot.pos) < apart;
            if nodes.last().is_some_and(|n| n.pos.distance(spot.pos) >= apart) && !near_last {
                nodes.push(*spot);
            }
        }
        nodes.push(*last);
        if !(most.is_finite() && most > 0.0) {
            return nodes;
        }
        // (how far apart two are is as the canvas has them, which a path carried elsewhere may not
        // measure as its own space does: counted along what is shown)
        let mut gone = 0.0;
        let mut ways = Vec::with_capacity(path.len());
        for (i, spot) in path.iter().enumerate() {
            if let Some(before) = i.checked_sub(1).and_then(|j| path.get(j)) {
                gone += spot.pos.distance(before.pos);
            }
            ways.push(gone);
        }
        let way_of = |node: &PathSpot| {
            path.iter().zip(&ways).find(|(spot, _)| spot.distance >= node.distance).map_or(gone, |(_, way)| *way)
        };
        let mut all: Vec<PathSpot> = Vec::with_capacity(nodes.len());
        for node in nodes {
            if let Some(before) = all.last() {
                let (a, b) = (way_of(before), way_of(&node));
                let pieces = ((b - a) / most).ceil().clamp(1.0, 64.0) as usize;
                for k in 1..pieces {
                    let at = a + (b - a) * k as f32 / pieces as f32;
                    if let Some((spot, _)) = path.iter().zip(&ways).find(|(_, way)| **way >= at) {
                        all.push(*spot);
                    }
                }
            }
            all.push(node);
        }
        all
    }

    /// The stroke pulled into another shape: the place `along` its path (pixels, its own space)
    /// carried `by` on the canvas, and the rest of it between `from` and `to` along the path
    /// carried less the further from there it is, to nothing at those two (where `from` or `to` is
    /// the place itself, an end, all of it that side goes the whole way). Its input is what is
    /// moved, so it is the same stroke drawn along another line.
    pub fn reshaped(&self, along: f32, from: f32, to: f32, by: Vec2) -> VectorStroke {
        let whole = self.whole_path();
        let length = whole.last().map_or(0.0, |spot| spot.distance);
        // how far the input had come at each sample, as a share of the path's length
        let mut gone = 0.0;
        let mut at = Vec::with_capacity(self.samples.len());
        for (i, sample) in self.samples.iter().enumerate() {
            if let Some(before) = i.checked_sub(1).and_then(|j| self.samples.get(j)) {
                gone += sample.pos.distance(before.pos);
            }
            at.push(gone);
        }
        let scale = if gone > 0.0 { length / gone } else { 0.0 };
        // `by` as the stroke's own space has it, about the place taken hold of
        let held = whole
            .iter()
            .min_by(|a, b| (a.distance - along).abs().total_cmp(&(b.distance - along).abs()))
            .map_or(Vec2::ZERO, |spot| spot.pos);
        let own = self
            .placement
            .apply(held)
            .zip(self.placement.inverse())
            .and_then(|(on_canvas, back)| back.apply(on_canvas + by))
            .map_or(by, |moved| moved - held);
        let share = |d: f32| {
            let lin = if d <= along {
                if from >= along { 1.0 } else { ((d - from) / (along - from)).clamp(0.0, 1.0) }
            } else if to <= along {
                1.0
            } else {
                ((to - d) / (to - along)).clamp(0.0, 1.0)
            };
            0.5 - 0.5 * (std::f32::consts::PI * lin).cos()
        };
        let samples: Vec<Sample> = self
            .samples
            .iter()
            .zip(&at)
            .map(|(sample, gone)| Sample { pos: sample.pos + own * share(gone * scale), ..*sample })
            .collect();
        VectorStroke {
            samples: Arc::new(samples),
            path: KeptPath::default(),
            bounds: KeptBounds::default(),
            ..self.clone()
        }
    }

    /// The stroke bent `through` a map of the canvas onto itself that is no straight carrying (a
    /// warp): its input is what is carried, closely enough spaced first that a straight stretch
    /// can bend, so it is the same stroke drawn along another line, as wide as it was. What showed
    /// of a piece still shows.
    pub fn bent(&self, through: &dyn Fn(Vec2) -> Vec2) -> VectorStroke {
        let back = self.placement.inverse();
        let carry = |own: Vec2| {
            let carried = self.placement.apply(own).map(through);
            carried
                .zip(back)
                .and_then(|(on_canvas, back)| back.apply(on_canvas))
                .filter(|p| p.is_finite())
                .unwrap_or(own)
        };
        // (input a few pixels apart at most, the pen's pressure and lean going evenly between)
        let mut samples: Vec<Sample> = Vec::with_capacity(self.samples.len());
        for sample in self.samples.iter() {
            if let Some(before) = samples.last().copied() {
                let pieces = (sample.pos.distance(before.pos) / BEND_APART).ceil().clamp(1.0, 4096.0) as usize;
                for k in 1..pieces {
                    let t = k as f32 / pieces as f32;
                    let between = |a: f32, b: f32| a + (b - a) * t;
                    samples.push(Sample {
                        pos: before.pos.lerp(sample.pos, t),
                        pressure: between(before.pressure, sample.pressure),
                        tilt: between(before.tilt, sample.tilt),
                        azimuth: before.azimuth,
                        roll: between(before.roll, sample.roll),
                        time_ms: before.time_ms + (sample.time_ms - before.time_ms) * f64::from(t),
                    });
                }
            }
            samples.push(*sample);
        }
        for sample in &mut samples {
            sample.pos = carry(sample.pos);
        }
        let mut now = VectorStroke {
            samples: Arc::new(samples),
            path: KeptPath::default(),
            bounds: KeptBounds::default(),
            shown: None,
            ..self.clone()
        };
        // a piece shows between where its two ends went
        if let Some((from, to)) = self.shown {
            let was = self.whole_path();
            let is = now.whole_path();
            let went = |along: f32| {
                let at = was.iter().min_by(|a, b| (a.distance - along).abs().total_cmp(&(b.distance - along).abs()))?;
                let to = carry(at.pos);
                is.iter().min_by(|a, b| a.pos.distance(to).total_cmp(&b.pos.distance(to))).map(|spot| spot.distance)
            };
            let length = |path: &[PathSpot]| path.last().map_or(0.0, |spot| spot.distance);
            let by = if length(&was) > 0.0 { length(&is) / length(&was) } else { 1.0 };
            now.shown = match (went(from), went(to)) {
                (Some(a), Some(b)) if b > a => Some((a, b)),
                _ => Some((from * by, to * by)),
            };
        }
        now
    }

    /// A dual brush's second stroke's stamps as they lie on the canvas (see
    /// [`Replay::placed_stamps`]).
    pub fn placed_second(&self) -> Option<Vec<Stamp>> {
        Some(placed(self.replay_second()?, self.placement))
    }

    /// The span of this stroke from `from` to `to` along its path (pixels, its own space), as a
    /// stroke of its own named `id`: what a cut leaves of it. Within what already shows, if this
    /// is itself a piece; none if nothing is left.
    pub fn piece(&self, id: StrokeId, from: f32, to: f32) -> Option<VectorStroke> {
        let (within_from, within_to) = self.shown.unwrap_or((f32::NEG_INFINITY, f32::INFINITY));
        let (from, to) = (from.max(within_from), to.min(within_to));
        (from.is_finite() && to.is_finite() && to > from).then(|| VectorStroke {
            id,
            shown: Some((from, to)),
            ..self.clone()
        })
    }
}

/// A stroke's whole path, worked out once and shared by the stroke's copies. (It is made of the
/// stroke's input and setup alone, so it takes no part in whether two strokes are the same.)
#[derive(Clone, Default)]
struct KeptPath(Arc<OnceLock<Arc<Vec<PathSpot>>>>);

impl PartialEq for KeptPath {
    fn eq(&self, _: &KeptPath) -> bool {
        true
    }
}

impl std::fmt::Debug for KeptPath {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(if self.0.get().is_some() { "KeptPath(known)" } else { "KeptPath" })
    }
}

/// The canvas pixels a stroke's ink can touch, worked out once for the stroke as it is. A copy of
/// the stroke (which is made to change it) starts without, and works its own out.
#[derive(Default)]
struct KeptBounds(OnceLock<IRect>);

impl Clone for KeptBounds {
    fn clone(&self) -> KeptBounds {
        KeptBounds::default()
    }
}

impl PartialEq for KeptBounds {
    fn eq(&self, _: &KeptBounds) -> bool {
        true
    }
}

impl std::fmt::Debug for KeptBounds {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("KeptBounds")
    }
}

/// How a filled shape goes onto its layer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FillMode {
    /// Over what is there, as paint.
    Over,
    /// Under what is there: a fill traced from lines lies beneath them.
    Behind,
    /// Takes away what is there (the eraser's lasso).
    Erase,
}

/// What a fill was traced from: the point it spread from, how readily it took openings for gaps,
/// and the strokes it lies against.
#[derive(Clone, Debug, PartialEq)]
pub struct TracedFrom {
    pub seed: Vec2,
    pub factor: f32,
    pub sources: Vec<StrokeId>,
}

/// A filled shape on a vector layer.
#[derive(Clone, Debug, PartialEq)]
pub struct VectorFill {
    pub id: StrokeId,
    /// Its outlines on the canvas, each closed back to its first point; inside by the even-odd
    /// rule, so an outline within another is a hole.
    pub loops: Vec<Vec<Vec2>>,
    pub colour: [f32; 3],
    pub opacity: f32,
    pub mode: FillMode,
    /// Pixel art's: it takes whole pixels only, none partly.
    pub hard: bool,
    /// How it was traced, for one that was (the bucket's); none for a shape drawn by hand.
    pub traced: Option<TracedFrom>,
    /// An eraser's shape's: the strokes and fills it takes from (see
    /// [`VectorStroke::takes_from`]).
    pub takes_from: Option<Vec<StrokeId>>,
}

/// One thing a vector layer holds.
#[derive(Clone, Debug)]
pub enum Object {
    Stroke(Arc<VectorStroke>),
    Fill(Arc<VectorFill>),
}

impl Object {
    pub fn id(&self) -> StrokeId {
        match self {
            Object::Stroke(stroke) => stroke.id,
            Object::Fill(fill) => fill.id,
        }
    }

    /// The canvas pixels it can touch as it lies now: a stroke's ink (see
    /// [`VectorStroke::ink_bounds`]), a fill's outlines and a pixel about them.
    pub fn bounds(&self) -> IRect {
        match self {
            Object::Stroke(stroke) => stroke.ink_bounds(),
            Object::Fill(_) => match self.reach() {
                Some([x0, y0, x1, y1]) if [x0, y0, x1, y1].iter().all(|v| v.is_finite()) => {
                    IRect::new(x0.floor() as i32 - 2, y0.floor() as i32 - 2, x1.ceil() as i32 + 2, y1.ceil() as i32 + 2)
                }
                _ => IRect::EMPTY,
            },
        }
    }

    /// Whether it is an eraser's: a stroke that erases, or its lasso's shape.
    pub fn erases(&self) -> bool {
        match self {
            Object::Stroke(stroke) => stroke.ink.erases,
            Object::Fill(fill) => fill.mode == FillMode::Erase,
        }
    }

    /// An eraser's: what it takes from, if it names them (see [`VectorStroke::takes_from`]).
    pub fn takes_from(&self) -> Option<&[StrokeId]> {
        match self {
            Object::Stroke(stroke) => stroke.takes_from.as_deref(),
            Object::Fill(fill) => fill.takes_from.as_deref(),
        }
    }

    fn set_takes_from(&mut self, ids: Option<Vec<StrokeId>>) {
        match self {
            Object::Stroke(stroke) => Arc::make_mut(stroke).takes_from = ids,
            Object::Fill(fill) => Arc::make_mut(fill).takes_from = ids,
        }
    }

    fn set_id(&mut self, id: StrokeId) {
        match self {
            Object::Stroke(stroke) => Arc::make_mut(stroke).id = id,
            Object::Fill(fill) => Arc::make_mut(fill).id = id,
        }
    }

    /// Carried by `map` (canvas to canvas): a stroke placed, a fill's outlines moved.
    fn carry(&mut self, map: Homography) {
        match self {
            Object::Stroke(stroke) => Arc::make_mut(stroke).place(map),
            Object::Fill(fill) => {
                let fill = Arc::make_mut(fill);
                for outline in &mut fill.loops {
                    *outline = outline.iter().filter_map(|p| map.apply(*p)).collect();
                }
                if let Some(traced) = fill.traced.as_mut() {
                    traced.seed = map.apply(traced.seed).unwrap_or(traced.seed);
                }
            }
        }
    }

    /// Bent `through` a map of the canvas onto itself (a warp): a stroke drawn along where its line
    /// goes, a fill's outlines carried point by point, closely enough spaced to bend.
    fn bend(&mut self, through: &dyn Fn(Vec2) -> Vec2) {
        match self {
            Object::Stroke(stroke) => *stroke = Arc::new(stroke.bent(through)),
            Object::Fill(fill) => {
                let fill = Arc::make_mut(fill);
                for outline in &mut fill.loops {
                    let mut bent = Vec::with_capacity(outline.len());
                    for (i, &p) in outline.iter().enumerate() {
                        let next = outline.get((i + 1) % outline.len().max(1)).copied().unwrap_or(p);
                        let pieces = (p.distance(next) / BEND_APART).ceil().clamp(1.0, 4096.0) as usize;
                        bent.extend((0..pieces).map(|k| through(p.lerp(next, k as f32 / pieces as f32))));
                    }
                    bent.retain(|p| p.is_finite());
                    *outline = bent;
                }
                if let Some(traced) = fill.traced.as_mut() {
                    traced.seed = Some(through(traced.seed)).filter(|p| p.is_finite()).unwrap_or(traced.seed);
                }
            }
        }
    }

    /// The canvas rectangle it keeps to (left, top, right, bottom), near enough.
    pub fn reach(&self) -> Option<[f32; 4]> {
        match self {
            Object::Stroke(stroke) => stroke.reach(),
            Object::Fill(fill) => {
                let mut points = fill.loops.iter().flatten();
                let first = points.next()?;
                Some(points.fold([first.x, first.y, first.x, first.y], |[x0, y0, x1, y1], p| {
                    [x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)]
                }))
            }
        }
    }
}

impl PartialEq for Object {
    /// The same thing (one shared is the same without looking).
    fn eq(&self, other: &Object) -> bool {
        match (self, other) {
            (Object::Stroke(a), Object::Stroke(b)) => Arc::ptr_eq(a, b) || a == b,
            (Object::Fill(a), Object::Fill(b)) => Arc::ptr_eq(a, b) || a == b,
            _ => false,
        }
    }
}

/// A run of a stroke's line that still shows: the stroke's place in its layer's order, its name,
/// and the run's points on the canvas, each with how far along the stroke's path it is.
type ShownRun = (usize, StrokeId, Vec<(Vec2, f32)>);

fn reaches_meet(a: [f32; 4], b: [f32; 4]) -> bool {
    a[0] <= b[2] && b[0] <= a[2] && a[1] <= b[3] && b[1] <= a[3]
}

/// What a vector layer holds: its strokes and fills in the order they were made, which is the
/// order they are drawn in (a fill laid behind goes under all that was there before it; an
/// eraser takes from all that was there before it, and nothing made after: or, once something
/// it touched has been carried elsewhere, from just those it names, wherever they are). A copy shares them,
/// so the layer as it was before each change is cheap to keep for undo.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VectorContent {
    objects: Vec<Object>,
    /// The next one's name (see [`StrokeId`]).
    next: u64,
}

impl VectorContent {
    pub fn objects(&self) -> &[Object] {
        &self.objects
    }

    /// What a layer held, read back from where it was kept: its strokes and fills in order, and
    /// the next one's name (moved past any of theirs).
    pub fn restored(objects: Vec<Object>, next: u64) -> VectorContent {
        let past = objects.iter().map(|o| o.id().0.saturating_add(1)).max().unwrap_or(0);
        VectorContent { objects, next: next.max(past) }
    }

    pub fn strokes(&self) -> impl DoubleEndedIterator<Item = &Arc<VectorStroke>> {
        self.objects.iter().filter_map(|o| match o {
            Object::Stroke(stroke) => Some(stroke),
            Object::Fill(_) => None,
        })
    }

    pub fn fills(&self) -> impl DoubleEndedIterator<Item = &Arc<VectorFill>> {
        self.objects.iter().filter_map(|o| match o {
            Object::Fill(fill) => Some(fill),
            Object::Stroke(_) => None,
        })
    }

    pub fn len(&self) -> usize {
        self.objects.len()
    }

    pub fn is_empty(&self) -> bool {
        self.objects.is_empty()
    }

    /// A name for a stroke or fill about to be made here.
    pub fn next_id(&self) -> StrokeId {
        StrokeId(self.next)
    }

    /// A name of its own for `id`, if another here has it (it shouldn't), and the next name moved
    /// past it.
    fn named(&mut self, id: StrokeId) -> StrokeId {
        let id = if id.0 < self.next && self.objects.iter().any(|o| o.id() == id) { StrokeId(self.next) } else { id };
        self.next = self.next.max(id.0.saturating_add(1));
        id
    }

    /// `stroke` kept as the newest.
    pub fn push(&mut self, mut stroke: VectorStroke) {
        stroke.id = self.named(stroke.id);
        self.objects.push(Object::Stroke(Arc::new(stroke)));
    }

    /// `fill` kept as the newest.
    pub fn push_fill(&mut self, mut fill: VectorFill) {
        fill.id = self.named(fill.id);
        self.objects.push(Object::Fill(Arc::new(fill)));
    }

    /// The canvas pixels that differ between what this holds and what `other` (the same layer at
    /// another time) holds: all that the strokes and fills only one of them has, or that are not
    /// the same in both, can touch. What draws the layer again need only draw there.
    pub fn changed_from(&self, other: &VectorContent) -> IRect {
        let differs = |a: &Object, b: &Object| match (a, b) {
            (Object::Stroke(a), Object::Stroke(b)) => !Arc::ptr_eq(a, b),
            (Object::Fill(a), Object::Fill(b)) => !Arc::ptr_eq(a, b),
            _ => true,
        };
        let only_or_changed = |of: &VectorContent, against: &VectorContent| {
            of.objects
                .iter()
                .filter(|o| against.objects.iter().find(|x| x.id() == o.id()).is_none_or(|x| differs(o, x)))
                .fold(IRect::EMPTY, |all, o| all.union(o.bounds()))
        };
        only_or_changed(self, other).union(only_or_changed(other, self))
    }

    /// The strokes and fills named in `ids` given `colour` (an eraser's marks have none to give).
    /// Whether any changed.
    pub fn recolour(&mut self, ids: &[StrokeId], colour: [f32; 3]) -> bool {
        let mut changed = false;
        for object in self.objects.iter_mut().filter(|o| Self::pickable(o) && ids.contains(&o.id())) {
            match object {
                Object::Stroke(stroke) if stroke.ink.colour != colour => {
                    let now =
                        stroke.restyled(stroke.setup.clone(), Ink { colour, ..stroke.ink }, stroke.second.clone());
                    *stroke = Arc::new(now);
                    changed = true;
                }
                Object::Fill(fill) if fill.colour != colour => {
                    Arc::make_mut(fill).colour = colour;
                    changed = true;
                }
                _ => {}
            }
        }
        changed
    }

    /// The stroke named as `stroke` is put in its place (a stroke restyled or reshaped). Whether
    /// there was one.
    pub fn replace_stroke(&mut self, stroke: VectorStroke) -> bool {
        let Some(place) = self.objects.iter_mut().find(|o| matches!(o, Object::Stroke(s) if s.id == stroke.id)) else {
            return false;
        };
        *place = Object::Stroke(Arc::new(stroke));
        true
    }

    /// The canvas pixels the strokes and fills named in `ids` can touch.
    pub fn bounds_of(&self, ids: &[StrokeId]) -> IRect {
        self.objects.iter().filter(|o| ids.contains(&o.id())).fold(IRect::EMPTY, |all, o| all.union(o.bounds()))
    }

    /// This with only the strokes and fills named in `ids` (and the erasers that take from them),
    /// or with `keep` false, without them: the two parts of a layer when some of it is picked up.
    /// An eraser that takes from all made before it is in both, taking from what each has.
    pub fn part(&self, ids: &[StrokeId], keep: bool) -> VectorContent {
        let mut part = self.clone();
        let gone: Vec<StrokeId> = self
            .objects
            .iter()
            .filter(|o| Self::pickable(o) && ids.contains(&o.id()) != keep)
            .map(Object::id)
            .collect();
        for id in gone {
            part.remove(id);
        }
        part
    }

    /// The stroke or fill named `id` gone; whether there was one.
    pub fn remove(&mut self, id: StrokeId) -> bool {
        let before = self.objects.len();
        self.objects.retain(|o| o.id() != id);
        let gone = self.objects.len() != before;
        if gone {
            self.renamed(id, &[]);
        }
        gone
    }

    /// The object named `was` is now those named `now` (pieces of it; none: it is gone): the
    /// erasers that name it name them instead, and one left naming nothing goes, there being
    /// nothing for its marks to be on.
    fn renamed(&mut self, was: StrokeId, now: &[StrokeId]) {
        for object in &mut self.objects {
            if let Some(ids) = object.takes_from().filter(|ids| ids.contains(&was)) {
                let ids = ids.iter().flat_map(|id| if *id == was { now.to_vec() } else { vec![*id] }).collect();
                object.set_takes_from(Some(ids));
            }
        }
        self.objects.retain(|o| !o.takes_from().is_some_and(<[StrokeId]>::is_empty));
    }

    /// Whether the eraser at `eraser` takes from the object at `index`, named `id`: it names it,
    /// or names none and was made after it.
    fn takes(&self, eraser: usize, index: usize, id: StrokeId) -> bool {
        match self.objects.get(eraser) {
            Some(object) if object.erases() => object.takes_from().map_or(eraser > index, |ids| ids.contains(&id)),
            _ => false,
        }
    }

    /// The erasers that name the stroke or fill `id` as one they take from, in the order they
    /// were made: its own marks, which what draws it takes from it alone.
    pub fn erasers_of(&self, id: StrokeId) -> Vec<Object> {
        self.objects
            .iter()
            .filter(|o| o.erases() && o.takes_from().is_some_and(|ids| ids.contains(&id)))
            .cloned()
            .collect()
    }

    /// Whether anything made before the object at `index` reaches where `reach` does.
    fn anything_under(&self, index: usize, reach: [f32; 4]) -> bool {
        self.objects.iter().take(index).any(|o| o.reach().is_some_and(|r| reaches_meet(r, reach)))
    }

    /// Whether a stroke that erases, reaching `reach`, would find anything here to take from.
    pub fn anything_to_erase(&self, reach: [f32; 4]) -> bool {
        self.anything_under(self.objects.len(), reach)
    }

    /// Whether `point` (canvas pixels) of the object at `index`, named `id`, has been erased: an
    /// eraser that takes from it covers the point.
    fn erased(&self, index: usize, id: StrokeId, point: Vec2) -> bool {
        self.objects.iter().enumerate().any(|(at, eraser)| {
            self.takes(at, index, id)
                && match eraser {
                    Object::Stroke(eraser) if eraser.ink.opacity >= 0.5 => {
                        eraser.reach().is_some_and(|r| reaches_meet(r, [point.x, point.y, point.x, point.y]))
                            && eraser.nearest(point).is_some_and(|near| near.off <= near.radius)
                    }
                    Object::Fill(fill) if fill.opacity >= 0.5 => vector_fill::inside(&fill.loops, point),
                    _ => false,
                }
        })
    }

    /// What can be picked: a stroke that paints, a fill that isn't an eraser's. (An eraser's marks
    /// go with what they were made on.)
    fn pickable(object: &Object) -> bool {
        match object {
            Object::Stroke(stroke) => !stroke.ink.erases,
            Object::Fill(fill) => fill.mode != FillMode::Erase,
        }
    }

    /// The stroke or fill at `point` (canvas pixels): the newest whose ink or inside the point is
    /// on; if it is on none, the stroke whose ink is nearest, within `slack` of it (a thin line
    /// is hard to touch exactly, but a touch on one line is never taken for a newer one beside it).
    pub fn object_at(&self, point: Vec2, slack: f32) -> Option<StrokeId> {
        let at = [point.x - slack, point.y - slack, point.x + slack, point.y + slack];
        // how far outside its ink the point is (none: not near it at all)
        let outside = |object: &Object| match object {
            Object::Stroke(stroke) => stroke
                .reach()
                .filter(|r| reaches_meet(*r, at))
                .and_then(|_| stroke.nearest(point))
                .map(|near| (near.off - near.radius).max(0.0)),
            Object::Fill(fill) => vector_fill::inside(&fill.loops, point).then_some(0.0),
        };
        let mut nearest: Option<(f32, StrokeId)> = None;
        for object in self.objects.iter().rev().filter(|o| Self::pickable(o)) {
            let Some(off) = outside(object) else { continue };
            if off <= 0.0 {
                return Some(object.id());
            }
            if off <= slack && nearest.is_none_or(|(best, _)| off < best) {
                nearest = Some((off, object.id()));
            }
        }
        nearest.map(|(_, id)| id)
    }

    /// The strokes and fills a region takes: every one with a point of its line (a fill's
    /// outline) where `within` says the region is.
    pub fn objects_within(&self, within: impl Fn(Vec2) -> bool) -> Vec<StrokeId> {
        let taken = |object: &Object| match object {
            Object::Stroke(stroke) => stroke.line().into_iter().any(&within),
            Object::Fill(fill) => fill.loops.iter().flatten().any(|p| within(*p)),
        };
        self.objects.iter().filter(|o| Self::pickable(o) && taken(o)).map(Object::id).collect()
    }

    /// Whether the object at `index` is an eraser's (a stroke that erases, or its lasso's shape).
    fn erases(&self, index: usize) -> bool {
        match self.objects.get(index) {
            Some(Object::Stroke(stroke)) => stroke.ink.erases,
            Some(Object::Fill(fill)) => fill.mode == FillMode::Erase,
            None => false,
        }
    }

    /// The strokes and fills named in `ids` (all of them, with none given) carried by `map`
    /// (canvas to canvas). An eraser's marks go with what they were made on. Until now an eraser
    /// took from all that was made before it; from here on, each made after the first thing
    /// carried names what it took from (what lay under it), so that it goes on taking from just
    /// those wherever they are, and from nothing that comes to lie under it. One whose every
    /// stroke and fill is carried is carried with them; one that took from some carried and some
    /// not is made two, one staying with those that stay and one going with those that go.
    /// Whether anything was carried.
    pub fn place(&mut self, ids: Option<&[StrokeId]>, map: Homography) -> bool {
        if !map.is_finite() || map.determinant().abs() <= 1e-12 || map.is_identity() {
            return false;
        }
        self.carry_each(ids, &|object| object.carry(map))
    }

    /// The strokes and fills named in `ids` (all of them, with none given) bent `through` a map of
    /// the canvas onto itself that is no straight carrying (a warp): each stroke drawn along where
    /// its line goes, each fill's outline carried. Erasers go with what they were made on, as
    /// [`VectorContent::place`] has them go. Whether anything was bent.
    pub fn bend(&mut self, ids: Option<&[StrokeId]>, through: &dyn Fn(Vec2) -> Vec2) -> bool {
        self.carry_each(ids, &|object| object.bend(through))
    }

    /// The work of [`VectorContent::place`] and [`VectorContent::bend`]: `carry` is what carries
    /// one stroke or fill.
    fn carry_each(&mut self, ids: Option<&[StrokeId]>, carry: &dyn Fn(&mut Object)) -> bool {
        let carried: Vec<StrokeId> = self
            .objects
            .iter()
            .filter(|o| Self::pickable(o) && ids.is_none_or(|ids| ids.contains(&o.id())))
            .map(Object::id)
            .collect();
        let Some(first) = self.objects.iter().position(|o| carried.contains(&o.id())) else { return false };
        // each eraser made after it, that names none yet, names what lies under it
        for at in first + 1..self.objects.len() {
            if !self.erases(at) || self.objects.get(at).is_some_and(|o| o.takes_from().is_some()) {
                continue;
            }
            let reach = self.objects.get(at).and_then(Object::reach);
            let under: Vec<StrokeId> = self
                .objects
                .iter()
                .take(at)
                .filter(|o| !o.erases() && o.reach().zip(reach).is_some_and(|(r, reach)| reaches_meet(r, reach)))
                .map(Object::id)
                .collect();
            if let Some(object) = self.objects.get_mut(at) {
                object.set_takes_from(Some(under));
            }
        }
        let mut placed = Vec::with_capacity(self.objects.len());
        for mut object in std::mem::take(&mut self.objects) {
            if carried.contains(&object.id()) {
                carry(&mut object);
                placed.push(object);
                continue;
            }
            let Some(ids) = object.takes_from().filter(|_| object.erases()) else {
                placed.push(object);
                continue;
            };
            let (going, staying): (Vec<StrokeId>, Vec<StrokeId>) = ids.iter().partition(|id| carried.contains(id));
            match (going.is_empty(), staying.is_empty()) {
                // (one that names nothing has nothing to mark)
                (true, true) => {}
                (true, false) => placed.push(object),
                (false, true) => {
                    carry(&mut object);
                    placed.push(object);
                }
                (false, false) => {
                    let mut gone = object.clone();
                    gone.set_id(StrokeId(self.next));
                    self.next += 1;
                    gone.set_takes_from(Some(going));
                    carry(&mut gone);
                    object.set_takes_from(Some(staying));
                    placed.push(object);
                    placed.push(gone);
                }
            }
        }
        self.objects = placed;
        true
    }

    /// The lines of each stroke that paints, as they still show: a stroke's line broken where an
    /// eraser has since gone over it, each run with its place on the stroke's path (how far along,
    /// its own space) at its points.
    fn shown_runs(&self) -> Vec<ShownRun> {
        let mut out = Vec::new();
        for (index, object) in self.objects.iter().enumerate() {
            let Object::Stroke(stroke) = object else { continue };
            if stroke.ink.erases {
                continue;
            }
            let mut run: Vec<(Vec2, f32)> = Vec::new();
            for spot in stroke.placed_path() {
                let at = spot.pos;
                if self.erased(index, stroke.id, at) {
                    if run.len() > 1 {
                        out.push((index, stroke.id, std::mem::take(&mut run)));
                    }
                    run.clear();
                } else {
                    run.push((at, spot.distance));
                }
            }
            if run.len() > 1 {
                out.push((index, stroke.id, run));
            }
        }
        out
    }

    /// The lines fills stop at: every painted stroke's line where it still shows. What an eraser
    /// took is no wall, so a line erased through has a gap there.
    pub fn walls(&self) -> Vec<Wall> {
        self.shown_runs()
            .into_iter()
            .map(|(_, id, run)| Wall { line: run.into_iter().map(|(at, _)| at).collect(), source: id.0 })
            .collect()
    }

    /// The painted strokes whose ink a point `radius` across at `point` (canvas pixels) touches,
    /// each with where along it (its own space): the newest first.
    fn touched_at(&self, point: Vec2, radius: f32) -> Vec<(StrokeId, f32)> {
        let reach = [point.x - radius, point.y - radius, point.x + radius, point.y + radius];
        let mut out = Vec::new();
        for stroke in self.strokes().rev().filter(|s| !s.ink.erases) {
            if !stroke.reach().is_some_and(|r| reaches_meet(r, reach)) {
                continue;
            }
            if let Some(near) = stroke.nearest(point).filter(|near| near.off <= near.radius + radius) {
                out.push((stroke.id, near.along));
            }
        }
        out
    }

    /// The eraser taking strokes whole: every painted stroke that a tip `radius` across touches
    /// anywhere along `path` (canvas pixels) goes, all of it, and every fill the path goes into.
    /// Whether anything went.
    pub fn erase_whole(&mut self, path: &[Vec2], radius: f32) -> bool {
        let mut gone: Vec<StrokeId> = Vec::new();
        for &point in path {
            gone.extend(self.touched_at(point, radius).into_iter().map(|(id, _)| id));
            gone.extend(
                self.fills()
                    .filter(|f| f.mode != FillMode::Erase && vector_fill::inside(&f.loops, point))
                    .map(|f| f.id),
            );
        }
        let before = self.objects.len();
        self.objects.retain(|o| !gone.contains(&o.id()));
        for id in &gone {
            self.renamed(*id, &[]);
        }
        self.objects.len() != before
    }

    /// The eraser taking a stroke up to where it meets others: each painted stroke a tip `radius`
    /// across touches along `path` loses the stretch touched, from the crossing with another
    /// stroke's line before it to the one after (to its end where there is none). What is left of
    /// it stays as it was. Whether anything went.
    pub fn erase_to_crossings(&mut self, path: &[Vec2], radius: f32) -> bool {
        let mut changed = false;
        for &point in path {
            for (id, along) in self.touched_at(point, radius) {
                changed |= self.cut_between_crossings(id, along);
            }
        }
        changed
    }

    /// Stroke `id` loses the stretch about `along` (pixels along its path, its own space) between
    /// the crossings either side: the rest stays, as one piece or two, where it was in the order.
    fn cut_between_crossings(&mut self, id: StrokeId, along: f32) -> bool {
        let Some(index) = self.objects.iter().position(|o| o.id() == id) else { return false };
        let Some(Object::Stroke(stroke)) = self.objects.get(index).cloned() else { return false };
        let runs = self.shown_runs();
        let own: Vec<&Vec<(Vec2, f32)>> = runs.iter().filter(|(_, of, _)| *of == id).map(|(_, _, run)| run).collect();
        let others: Vec<&Vec<(Vec2, f32)>> =
            runs.iter().filter(|(_, of, _)| *of != id).map(|(_, _, run)| run).collect();
        let path = stroke.path();
        let (Some(first), Some(last)) = (path.first(), path.last()) else { return false };
        let (mut from, mut to) = (first.distance, last.distance);
        for run in &own {
            for (i, mine) in run.windows(2).enumerate() {
                let [(a, da), (b, db)] = mine else { continue };
                // (its own line counts where it comes back across itself, not where it runs on)
                let own_others =
                    own.iter().flat_map(|r| r.windows(2).enumerate().map(move |(j, w)| (std::ptr::eq(*r, *run), j, w)));
                let theirs = others.iter().flat_map(|r| r.windows(2).map(|w| (false, 0, w)));
                for (same_run, j, other) in own_others.chain(theirs) {
                    if same_run && i.abs_diff(j) <= 1 {
                        continue;
                    }
                    let [(c, _), (d, _)] = other else { continue };
                    let Some(t) = crossing(*a, *b, *c, *d) else { continue };
                    let at = da + (db - da) * t;
                    if at <= along {
                        from = from.max(at);
                    } else {
                        to = to.min(at);
                    }
                }
            }
        }
        // what is left either side, each a piece of the stroke under a name of its own
        let (start, end) = stroke.shown.unwrap_or((first.distance, last.distance));
        let mut left = Vec::new();
        for (a, b) in [(start, from), (to, end)] {
            if b - a > 0.5
                && let Some(piece) = stroke.piece(self.next_id(), a, b)
            {
                self.next += 1;
                left.push(Object::Stroke(Arc::new(piece)));
            }
        }
        let pieces: Vec<StrokeId> = left.iter().map(Object::id).collect();
        self.objects.splice(index..=index, left);
        // (the eraser's marks on it are on its pieces now)
        self.renamed(id, &pieces);
        true
    }
}

/// How far along the segment from `a` to `b` the segment from `c` to `d` crosses it (0..1), if
/// it does.
fn crossing(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> Option<f32> {
    let (r, s) = (b - a, d - c);
    let denominator = r.cross(s);
    if denominator.abs() < 1e-9 {
        return None;
    }
    let t = (c - a).cross(s) / denominator;
    let u = (c - a).cross(r) / denominator;
    ((0.0..=1.0).contains(&t) && (0.0..=1.0).contains(&u)).then_some(t)
}

/// `path` from `from` to `to` along it: the spots between, with one made at each end where the
/// span cuts a stretch.
fn span_of(path: &[PathSpot], from: f32, to: f32) -> Vec<PathSpot> {
    let at = |a: &PathSpot, b: &PathSpot, distance: f32| {
        let reach = b.distance - a.distance;
        let t = if reach > 0.0 { ((distance - a.distance) / reach).clamp(0.0, 1.0) } else { 0.0 };
        PathSpot { pos: a.pos.lerp(b.pos, t), distance, radius: a.radius + (b.radius - a.radius) * t }
    };
    let mut out = Vec::new();
    for pair in path.windows(2) {
        let [a, b] = pair else { continue };
        if b.distance < from || a.distance > to {
            continue;
        }
        if out.is_empty() {
            out.push(if a.distance < from { at(a, b, from) } else { *a });
        }
        out.push(if b.distance > to { at(a, b, to) } else { *b });
    }
    // a path of one spot (a tap) is all there or not
    if let [only] = path
        && only.distance >= from
        && only.distance <= to
    {
        out.push(*only);
    }
    out
}

/// A stroke drawn again.
#[derive(Clone, Debug, PartialEq)]
pub struct Replay {
    /// Its stamps as its brush lays them, in the stroke's own space.
    pub stamps: Vec<Stamp>,
    /// The path they follow, in the stroke's own space, in order.
    pub path: Vec<PathSpot>,
    /// The stroke's own space to the canvas.
    pub placement: Homography,
}

/// Where a point is by a stroke: the nearest place on its path.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Near {
    /// How far along the path that place is, pixels in the stroke's own space (what
    /// [`VectorStroke::piece`] cuts at).
    pub along: f32,
    /// How far the point is from it, and half the stroke's width there, canvas pixels: the point
    /// is on the stroke's ink where `off` is no more than `radius`.
    pub off: f32,
    pub radius: f32,
}

impl Replay {
    /// The line the stroke follows, on the canvas.
    pub fn centreline(&self) -> Vec<Vec2> {
        self.path.iter().filter_map(|spot| self.placement.apply(spot.pos)).collect()
    }

    /// Its stamps as they lie on the canvas: each where the placement puts it, as much bigger and
    /// as far turned as the placement makes things there (mirrored if it mirrors), its own shape
    /// kept. A stroke lying where it was drawn gives its stamps as they are.
    pub fn placed_stamps(&self) -> Vec<Stamp> {
        placed(self.stamps.clone(), self.placement)
    }

    /// The canvas pixels its stamps can touch.
    pub fn bounds(&self) -> IRect {
        let own = self.stamps.iter().fold(IRect::EMPTY, |b, s| b.union(s.bounds()));
        if own.is_empty() {
            return IRect::EMPTY;
        }
        let corners = [(own.x0, own.y0), (own.x1, own.y0), (own.x1, own.y1), (own.x0, own.y1)];
        let placed = corners.map(|(x, y)| self.placement.apply(Vec2::new(x as f32, y as f32)));
        let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
        for p in placed {
            let Some(p) = p else { return IRect::EMPTY };
            (x0, y0, x1, y1) = (x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y));
        }
        if ![x0, y0, x1, y1].iter().all(|v| v.is_finite()) {
            return IRect::EMPTY;
        }
        IRect::new(x0.floor() as i32, y0.floor() as i32, x1.ceil() as i32, y1.ceil() as i32)
    }

    /// The place on the stroke's path nearest `point` (canvas pixels); none for a stroke with no
    /// path, or one placed by a map that can't be undone.
    pub fn nearest(&self, point: Vec2) -> Option<Near> {
        nearest_on(&self.path, self.placement, point)
    }
}

/// How far apart, at most, the points a stroke's input or a fill's outline is carried by are when
/// it is bent (pixels): close enough that a straight stretch comes out a curve.
const BEND_APART: f32 = 3.0;

/// `stamps` (a stroke's own space) as `placement` lays them on the canvas: each where it puts it,
/// as big and turned as it makes it there.
pub fn placed(stamps: Vec<Stamp>, placement: Homography) -> Vec<Stamp> {
    if placement.is_identity() {
        return stamps;
    }
    stamps
        .into_iter()
        .filter_map(|stamp| {
            let (center, scale, turn, mirrored) = placement.about(stamp.center)?;
            // (mirrored, a stamp turned one way is turned the other, its shape flipped with it)
            let (angle, flip) = if mirrored {
                (turn - stamp.angle, [!stamp.flip[0], stamp.flip[1]])
            } else {
                (stamp.angle + turn, stamp.flip)
            };
            Some(Stamp {
                center,
                radius: stamp.radius * scale,
                angle,
                flip,
                grain_offset: stamp.grain_offset * scale,
                grain_turn: stamp.grain_turn + turn,
                grain_size: stamp.grain_size * scale,
                distance: stamp.distance * scale,
                ..stamp
            })
        })
        .collect()
}

/// `spots` (a stroke's own space) as `placement` lays them on the canvas.
fn placed_spots(spots: &[PathSpot], placement: Homography) -> Vec<PathSpot> {
    if placement.is_identity() {
        return spots.to_vec();
    }
    spots
        .iter()
        .filter_map(|spot| {
            let (pos, scale, _, _) = placement.about(spot.pos)?;
            Some(PathSpot { pos, radius: spot.radius * scale, ..*spot })
        })
        .collect()
}

/// The place on `path` (a stroke's own space, placed on the canvas by `placement`) nearest
/// `point` (canvas pixels).
fn nearest_on(path: &[PathSpot], placement: Homography, point: Vec2) -> Option<Near> {
    let own = placement.inverse()?.apply(point)?;
    let scale = placement.about(own).map_or(1.0, |(_, scale, _, _)| scale);
    let mut best: Option<Near> = None;
    let mut take = |along: f32, off: f32, radius: f32| {
        let near = Near { along, off: off * scale, radius: radius * scale };
        if best.is_none_or(|b| near.off - near.radius < b.off - b.radius) {
            best = Some(near);
        }
    };
    if let [only] = path {
        take(only.distance, own.distance(only.pos), only.radius);
    }
    for pair in path.windows(2) {
        let [a, b] = pair else { continue };
        let along = b.pos - a.pos;
        let length2 = along.dot(along);
        let t = if length2 > 0.0 { ((own - a.pos).dot(along) / length2).clamp(0.0, 1.0) } else { 0.0 };
        take(
            a.distance + (b.distance - a.distance) * t,
            own.distance(a.pos.lerp(b.pos, t)),
            a.radius + (b.radius - a.radius) * t,
        );
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::Affine;
    use crate::library::library;

    /// A pen's stroke that bends, speeds up and slows, presses harder and lighter and leans as it
    /// goes: `count` samples, a few of them handed over together as a host batches them.
    fn wave(count: usize) -> Vec<Sample> {
        let mut time = 0.0;
        (0..count)
            .map(|i| {
                let t = i as f32 / count as f32;
                // every third sample comes with the one before it
                time += if i % 3 == 2 { 0.1 } else { 4.0 + f64::from(t) * 6.0 };
                let x = 40.0 + 420.0 * t * (0.4 + 0.6 * t);
                let y = 200.0 + 90.0 * (t * 9.0).sin() + 30.0 * (t * 31.0).cos();
                Sample {
                    pos: Vec2::new(x, y),
                    pressure: 0.15 + 0.8 * (t * 5.0).sin().abs(),
                    tilt: 0.2 + 0.9 * t,
                    azimuth: 1.0 + 2.0 * t,
                    roll: 0.5 * t,
                    time_ms: time,
                }
            })
            .collect()
    }

    fn ink() -> Ink {
        Ink {
            colour: [0.1, 0.2, 0.3],
            secondary: [1.0, 1.0, 1.0],
            opacity: 1.0,
            erases: false,
            preserve_alpha: false,
            wetness: 1.0,
        }
    }

    fn setup(brush: Brush, pointer: Pointer) -> Setup {
        Setup {
            scale: 1.5,
            view_turn: 0.4,
            points_per_pixel: 0.8,
            input: InputSettings { stabilization: 0.2, pressure_smoothing: 0.3, ..InputSettings::default() },
            ..Setup::new(Arc::new(brush), 18.0, pointer, 0x1234_5678)
        }
    }

    /// The stroke drawn as the app draws one live: its samples a few at a time, each lot kept as
    /// it is drawn, with the stamps shown ahead of them looked at in between as the screen does.
    fn drawn_live(setup: &Setup, samples: &[Sample]) -> (VectorStroke, Vec<Stamp>) {
        let mut kept = VectorStroke::begin(StrokeId(1), setup.clone(), ink());
        let mut builder = setup.builder();
        for (i, some) in samples.chunks(5).enumerate() {
            kept.record(some);
            if builder.shows_tip() {
                builder.add_tipped(some);
                builder.preview_tip(some);
            } else {
                builder.add(some);
                if i % 2 == 0 {
                    builder.preview_to_input(some);
                } else {
                    builder.preview(some);
                }
            }
        }
        (kept, builder.finish())
    }

    #[test]
    fn every_brush_the_app_comes_with_is_drawn_again_stamp_for_stamp() {
        let samples = wave(160);
        let mut brushes = 0;
        for set in library() {
            for brush in set.brushes {
                for pointer in [Pointer::Stylus, Pointer::Finger] {
                    let setup = setup(brush.clone(), pointer);
                    let (kept, live) = drawn_live(&setup, &samples);
                    let again = kept.replay();
                    assert!(!live.is_empty(), "{}: {} drew nothing", set.name, brush.name);
                    assert_eq!(again.stamps.len(), live.len(), "{}: {} with {pointer:?}", set.name, brush.name);
                    assert!(again.stamps == live, "{}: {} with {pointer:?} came out otherwise", set.name, brush.name);
                    // and again, the same
                    assert!(kept.replay() == again, "{}: {} twice", set.name, brush.name);
                }
                brushes += 1;
            }
        }
        assert!(brushes > 20, "only {brushes} brushes tried");
    }

    #[test]
    fn a_tap_is_kept_as_its_dot() {
        let setup = setup(Brush::round_pen(), Pointer::Stylus);
        let (kept, live) = drawn_live(&setup, &[Sample::at(Vec2::new(30.0, 40.0), 0.6, 0.0)]);
        let again = kept.replay();
        assert_eq!(again.stamps, live);
        assert_eq!(again.stamps.len(), 1);
        assert_eq!(again.path.len(), 1);
        let near = again.nearest(Vec2::new(31.0, 40.0)).unwrap();
        assert!(near.off < near.radius, "{near:?}");
    }

    #[test]
    fn samples_that_are_no_numbers_are_not_kept() {
        let mut kept = VectorStroke::begin(StrokeId(1), setup(Brush::round_pen(), Pointer::Stylus), ink());
        let bad = Sample::at(Vec2::new(f32::NAN, 1.0), 1.0, 0.0);
        kept.record(&[Sample::at(Vec2::new(1.0, 1.0), 1.0, 0.0), bad, Sample::at(Vec2::new(9.0, 1.0), 1.0, 8.0)]);
        assert_eq!(kept.samples().len(), 2);
    }

    #[test]
    fn the_path_runs_the_stroke_s_length_under_its_stamps() {
        let setup = setup(Brush::round_pen(), Pointer::Stylus);
        // (the pen held upright: leaning, it moves its stamps off the path toward its cap)
        let upright: Vec<Sample> = wave(120).into_iter().map(|s| Sample { tilt: 0.0, ..s }).collect();
        let (kept, _) = drawn_live(&setup, &upright);
        let again = kept.replay();
        // in order, from the start, each spot further along than the last
        assert_eq!(again.path.first().map(|s| s.distance), Some(0.0));
        assert!(again.path.windows(2).all(|w| w[1].distance > w[0].distance));
        // as long as its stretches are, and no stamp is further along than it goes
        let walked: f32 = again.path.windows(2).map(|w| w[0].pos.distance(w[1].pos)).sum();
        let length = again.path.last().unwrap().distance;
        assert!((walked - length).abs() < length * 1e-3, "{walked} {length}");
        assert!(again.stamps.iter().all(|s| s.distance <= length + 1e-3));
        // a round pen scatters nothing: every stamp lies on the path
        for stamp in &again.stamps {
            let near = again.nearest(stamp.center).unwrap();
            assert!(near.off < 0.05, "{near:?}");
            assert!((near.along - stamp.distance).abs() < 0.5, "{near:?} {}", stamp.distance);
        }
    }

    #[test]
    fn a_stroke_placed_elsewhere_is_the_same_stamps() {
        let setup = setup(Brush::round_pen(), Pointer::Stylus);
        let (mut kept, _) = drawn_live(&setup, &wave(120));
        let before = kept.replay();
        // twice the size, a quarter turn, moved: one map after another
        kept.place(Homography::from_affine(Affine::scale(2.0)));
        kept.place(Homography::from_affine(Affine::rotate(std::f32::consts::FRAC_PI_2)));
        kept.place(Homography::from_affine(Affine::translate(Vec2::new(500.0, -20.0))));
        let after = kept.replay();
        assert_eq!(after.stamps, before.stamps, "its own stamps as they were");
        assert_eq!(after.path, before.path);
        // its line and its reach go where the map takes them
        let map = Affine::scale(2.0)
            .then(Affine::rotate(std::f32::consts::FRAC_PI_2))
            .then(Affine::translate(Vec2::new(500.0, -20.0)));
        for (placed, own) in after.centreline().iter().zip(before.centreline()) {
            assert!(placed.distance(map.apply(own)) < 1e-2, "{placed:?}");
        }
        let (was, now) = (before.bounds(), after.bounds());
        assert!((now.width() as f32 - was.height() as f32 * 2.0).abs() <= 2.0, "{was:?} {now:?}");
        assert!((now.height() as f32 - was.width() as f32 * 2.0).abs() <= 2.0, "{was:?} {now:?}");
        // found where it lies now, as wide as it is now, at the same place along itself
        let middle = before.path[before.path.len() / 2];
        let near = after.nearest(map.apply(middle.pos)).unwrap();
        assert!(near.off < 0.05 && (near.along - middle.distance).abs() < 0.5, "{near:?}");
        assert!((near.radius - middle.radius * 2.0).abs() < 1e-3, "{near:?}");
        // a map that is no numbers leaves it where it was
        kept.place(Homography::from_affine(Affine { a: f32::NAN, ..Affine::IDENTITY }));
        assert_eq!(kept.replay(), after);
    }

    #[test]
    fn a_piece_of_a_stroke_is_that_span_of_it_as_it_was() {
        // a brush that tapers at both ends and scatters, so a stroke drawn anew would differ
        let brush = library()
            .into_iter()
            .flat_map(|set| set.brushes)
            .find(|b| b.pen_taper != crate::brush::Taper::NONE)
            .unwrap_or_else(Brush::round_pen);
        let setup = setup(brush, Pointer::Stylus);
        let (kept, _) = drawn_live(&setup, &wave(160));
        let whole = kept.replay();
        let length = whole.path.last().unwrap().distance;
        let (from, to) = (length * 0.25, length * 0.6);
        let piece = kept.piece(StrokeId(2), from, to).unwrap();
        let part = piece.replay();
        // its stamps are the whole stroke's in that span, each as it was
        let within: Vec<Stamp> =
            whole.stamps.iter().copied().filter(|s| s.distance >= from && s.distance <= to).collect();
        assert!(!within.is_empty() && within.len() < whole.stamps.len());
        assert_eq!(part.stamps, within);
        // its path starts and ends where it was cut
        assert!((part.path.first().unwrap().distance - from).abs() < 1e-3);
        assert!((part.path.last().unwrap().distance - to).abs() < 1e-3);
        assert!(part.path.windows(2).all(|w| w[1].distance >= w[0].distance));
        // the two sides of a cut, and nothing between them, are the whole stroke
        let rest = kept.piece(StrokeId(3), f32::MIN, from).into_iter().chain(kept.piece(StrokeId(4), to, f32::MAX));
        let others: usize = rest.map(|p| p.replay().stamps.len()).sum();
        let on_the_cuts = whole.stamps.iter().filter(|s| s.distance == from || s.distance == to).count();
        assert_eq!(others + within.len(), whole.stamps.len() + on_the_cuts);
        // a piece of a piece stays within it; one with nothing left is none
        let inner = piece.piece(StrokeId(5), 0.0, length).unwrap();
        assert_eq!(inner.shown, Some((from, to)));
        assert!(piece.piece(StrokeId(6), to + 1.0, length).is_none());
    }

    #[test]
    fn a_layer_s_strokes_are_kept_in_order_under_names_of_their_own() {
        let setup = setup(Brush::round_pen(), Pointer::Stylus);
        let mut content = VectorContent::default();
        assert!(content.is_empty());
        for _ in 0..3 {
            let mut stroke = VectorStroke::begin(content.next_id(), setup.clone(), ink());
            stroke.record(&wave(20));
            content.push(stroke);
        }
        let ids: Vec<u64> = content.strokes().map(|s| s.id.0).collect();
        assert_eq!(ids, [0, 1, 2]);
        // a copy is the same, sharing its strokes, until one of them is added to
        let before = content.clone();
        assert!(before == content);
        assert!(Arc::ptr_eq(before.strokes().next().unwrap(), content.strokes().next().unwrap()));
        // one named as another is given a name of its own
        content.push(VectorStroke::begin(StrokeId(1), setup.clone(), ink()));
        assert_eq!(content.strokes().next_back().map(|s| s.id.0), Some(3));
        assert!(before != content && before.len() == 3);
        // a dual brush's second is drawn with the same samples, by its own brush
        let mut dual = VectorStroke::begin(content.next_id(), setup.clone(), ink());
        dual.record(&wave(40));
        assert!(dual.replay_second().is_none());
        dual.second = Some(Second { setup: Setup { diameter: 30.0, seed: 9, ..setup }, opacity: 1.0 });
        let second = dual.replay_second().unwrap();
        assert!(!second.is_empty() && second != dual.replay().stamps);
    }

    /// A straight stroke from `from` to `to`, 12 across, drawn by a finger with a plain round pen
    /// (or erased with one), kept in `content`; its name there.
    fn straight(content: &mut VectorContent, from: (f32, f32), to: (f32, f32), erases: bool) -> StrokeId {
        let brush = Brush { streamline: 0.0, ..Brush::round_pen() };
        let setup = Setup::new(Arc::new(brush), 12.0, Pointer::Finger, 7);
        let mut stroke = VectorStroke::begin(content.next_id(), setup, Ink { erases, ..ink() });
        let (a, b) = (Vec2::new(from.0, from.1), Vec2::new(to.0, to.1));
        let samples: Vec<Sample> =
            (0..=40).map(|i| Sample::at(a.lerp(b, i as f32 / 40.0), 1.0, f64::from(i) * 8.0)).collect();
        stroke.record(&samples);
        let id = stroke.id;
        content.push(stroke);
        id
    }

    #[test]
    fn a_line_erased_through_is_a_wall_with_a_gap_in_it() {
        let mut content = VectorContent::default();
        let across = straight(&mut content, (50.0, 100.0), (350.0, 100.0), false);
        let down = straight(&mut content, (200.0, 20.0), (200.0, 300.0), false);
        // both lines whole: two walls, each its stroke's
        let walls = content.walls();
        assert_eq!(walls.iter().map(|w| w.source).collect::<Vec<u64>>(), [across.0, down.0]);
        // the eraser across the first, well clear of the second
        assert!(content.anything_to_erase([90.0, 80.0, 110.0, 120.0]));
        assert!(!content.anything_to_erase([360.0, 200.0, 380.0, 220.0]));
        straight(&mut content, (100.0, 80.0), (100.0, 120.0), true);
        let walls = content.walls();
        let of_across: Vec<&Wall> = walls.iter().filter(|w| w.source == across.0).collect();
        assert_eq!(of_across.len(), 2, "one each side of where it was erased");
        let ends: Vec<(f32, f32)> =
            of_across.iter().map(|w| (w.line.first().unwrap().x, w.line.last().unwrap().x)).collect();
        assert!(ends[0].1 < 96.0 && ends[1].0 > 104.0, "{ends:?}");
        assert!(ends[0].1 > 86.0 && ends[1].0 < 114.0, "no more gone than the eraser took: {ends:?}");
        // a stroke made after the eraser isn't erased by it
        let later = straight(&mut content, (60.0, 100.0), (140.0, 100.0), false);
        assert_eq!(content.walls().iter().filter(|w| w.source == later.0).count(), 1);
        // the eraser's lasso does the same
        content.push_fill(VectorFill {
            id: content.next_id(),
            loops: vec![vec![
                Vec2::new(180.0, 150.0),
                Vec2::new(220.0, 150.0),
                Vec2::new(220.0, 200.0),
                Vec2::new(180.0, 200.0),
            ]],
            colour: [0.0; 3],
            opacity: 1.0,
            mode: FillMode::Erase,
            hard: false,
            traced: None,
            takes_from: None,
        });
        assert_eq!(content.walls().iter().filter(|w| w.source == down.0).count(), 2);
    }

    #[test]
    fn the_eraser_takes_whole_strokes_or_up_to_where_they_cross() {
        let mut content = VectorContent::default();
        let across = straight(&mut content, (50.0, 100.0), (350.0, 100.0), false);
        let first = straight(&mut content, (150.0, 20.0), (150.0, 180.0), false);
        let second = straight(&mut content, (250.0, 20.0), (250.0, 180.0), false);
        let whole = content.strokes().next().unwrap().replay();
        // nowhere near anything: nothing goes
        let mut untouched = content.clone();
        assert!(!untouched.erase_whole(&[Vec2::new(300.0, 250.0)], 5.0));
        assert!(!untouched.erase_to_crossings(&[Vec2::new(300.0, 250.0)], 5.0));
        assert!(untouched == content);
        // whole: the stroke touched goes, the others stay
        let mut gone = content.clone();
        assert!(gone.erase_whole(&[Vec2::new(200.0, 103.0)], 5.0));
        assert_eq!(gone.strokes().map(|s| s.id).collect::<Vec<StrokeId>>(), [first, second]);
        // to the crossings: the middle of the first line goes, between the two it crosses
        assert!(content.erase_to_crossings(&[Vec2::new(200.0, 100.0)], 5.0));
        let pieces: Vec<&Arc<VectorStroke>> = content.strokes().filter(|s| s.id != first && s.id != second).collect();
        assert_eq!(pieces.len(), 2);
        assert!(pieces.iter().all(|p| p.id != across), "each piece under a name of its own");
        let spans: Vec<(f32, f32)> = pieces.iter().map(|p| p.shown.unwrap()).collect();
        assert!((spans[0].0, spans[1].1) == (0.0, 300.0), "{spans:?}");
        assert!((spans[0].1 - 100.0).abs() < 1.0 && (spans[1].0 - 200.0).abs() < 1.0, "{spans:?}");
        // they are where the stroke was in the order, and are its stamps as they were
        assert_eq!(content.objects().iter().position(|o| o.id() == pieces[0].id), Some(0));
        let stamps: usize = pieces.iter().map(|p| p.replay().stamps.len()).sum();
        let kept = whole.stamps.iter().filter(|s| s.distance <= spans[0].1 || s.distance >= spans[1].0).count();
        assert_eq!(stamps, kept);
        assert!(pieces[0].replay().stamps.iter().all(|s| whole.stamps.contains(s)));
        // an end piece touched goes to its end; a line that crosses nothing goes whole
        assert!(content.erase_to_crossings(&[Vec2::new(80.0, 100.0)], 5.0));
        assert_eq!(content.strokes().count(), 3);
        let mut alone = VectorContent::default();
        straight(&mut alone, (50.0, 100.0), (350.0, 100.0), false);
        assert!(alone.erase_to_crossings(&[Vec2::new(200.0, 100.0)], 5.0));
        assert!(alone.is_empty());
    }

    #[test]
    fn a_placed_stroke_s_stamps_lie_where_it_does_now() {
        let setup = setup(Brush::round_pen(), Pointer::Stylus);
        let (mut kept, _) = drawn_live(&setup, &wave(120));
        // lying where it was drawn: its stamps as they are
        let own = kept.replay();
        assert_eq!(own.placed_stamps(), own.stamps);
        // twice the size, a quarter turn on, moved
        let map = Affine::scale(2.0)
            .then(Affine::rotate(std::f32::consts::FRAC_PI_2))
            .then(Affine::translate(Vec2::new(500.0, -20.0)));
        kept.place(Homography::from_affine(map));
        let placed = kept.replay().placed_stamps();
        assert_eq!(placed.len(), own.stamps.len());
        for (now, was) in placed.iter().zip(&own.stamps) {
            assert!(now.center.distance(map.apply(was.center)) < 1e-2, "{now:?}");
            assert!((now.radius - was.radius * 2.0).abs() < 1e-3 && (now.distance - was.distance * 2.0).abs() < 1e-2);
            let turned = (now.angle - was.angle - std::f32::consts::FRAC_PI_2).rem_euclid(std::f32::consts::TAU);
            assert!(turned < 1e-3 || turned > std::f32::consts::TAU - 1e-3, "{turned}");
            assert_eq!((now.alpha, now.roundness, now.flip, now.tint), (was.alpha, was.roundness, was.flip, was.tint));
        }
        // mirrored, each stamp's shape is flipped with it
        let (mut flipped, _) = drawn_live(&setup, &wave(120));
        flipped.place(Homography::from_affine(Affine { a: -1.0, e: 600.0, ..Affine::IDENTITY }));
        let mirrored = flipped.replay().placed_stamps();
        assert!(
            mirrored.iter().zip(&own.stamps).all(|(now, was)| now.flip[0] != was.flip[0] && now.radius == was.radius)
        );
        assert!((mirrored[0].center.x - (600.0 - own.stamps[0].center.x)).abs() < 1e-2);
    }

    #[test]
    fn strokes_and_fills_are_picked_and_carried_with_the_eraser_s_marks_on_them() {
        let mut content = VectorContent::default();
        let across = straight(&mut content, (50.0, 100.0), (350.0, 100.0), false);
        let down = straight(&mut content, (200.0, 20.0), (200.0, 300.0), false);
        let fill = content.next_id();
        content.push_fill(VectorFill {
            id: fill,
            loops: vec![vec![
                Vec2::new(20.0, 200.0),
                Vec2::new(120.0, 200.0),
                Vec2::new(120.0, 280.0),
                Vec2::new(20.0, 280.0),
            ]],
            colour: [1.0, 0.0, 0.0],
            opacity: 1.0,
            mode: FillMode::Over,
            hard: false,
            traced: None,
            takes_from: None,
        });
        // the eraser across the first line only, well clear of the rest
        let eraser = straight(&mut content, (100.0, 80.0), (100.0, 120.0), true);
        // picked by a point: the newest thing there, a stroke within a little of its ink
        assert_eq!(content.object_at(Vec2::new(300.0, 103.0), 2.0), Some(across));
        assert_eq!(content.object_at(Vec2::new(200.0, 100.0), 2.0), Some(down), "the newer of the two that cross");
        assert_eq!(content.object_at(Vec2::new(60.0, 240.0), 2.0), Some(fill));
        assert_eq!(content.object_at(Vec2::new(100.0, 90.0), 2.0), None, "an eraser's stroke is not picked");
        assert_eq!(content.object_at(Vec2::new(300.0, 250.0), 2.0), None);
        // a touch on one line is that line, though a newer one lies within the slack of it
        let mut close = VectorContent::default();
        let upper = straight(&mut close, (50.0, 100.0), (350.0, 100.0), false);
        let lower = straight(&mut close, (50.0, 112.0), (350.0, 112.0), false);
        assert_eq!(close.object_at(Vec2::new(200.0, 100.0), 20.0), Some(upper));
        assert_eq!(close.object_at(Vec2::new(200.0, 113.0), 20.0), Some(lower));
        assert_eq!(close.object_at(Vec2::new(200.0, 90.0), 20.0), Some(upper), "off both: the nearer");
        assert_eq!(close.object_at(Vec2::new(200.0, 125.0), 20.0), Some(lower));
        // picked by a region: all with a point in it
        let left = content.objects_within(|p| p.x < 150.0);
        assert_eq!(left, [across, fill]);
        assert!(content.objects_within(|p| p.x > 360.0).is_empty());
        // the first line carried down: the eraser's mark on it goes with it, the rest stays
        let lower = Homography::from_affine(Affine::translate(Vec2::new(0.0, 150.0)));
        let before = content.clone();
        assert!(content.place(Some(&[across]), lower));
        let line_of = |c: &VectorContent, id: StrokeId| c.strokes().find(|s| s.id == id).unwrap().line();
        assert!((line_of(&content, across)[0].y - 250.0).abs() < 0.5);
        assert!((line_of(&content, eraser)[0].y - 230.0).abs() < 0.5, "its eraser went with it");
        assert_eq!(line_of(&content, down), line_of(&before, down));
        assert_eq!(content.walls().iter().filter(|w| w.source == across.0).count(), 2, "still erased through");
        // an eraser over what is carried and what isn't is made two: one stays, taking from what
        // stays, and one goes with what goes
        let mut shared = before.clone();
        straight(&mut shared, (190.0, 100.0), (210.0, 100.0), true);
        let wide = shared.strokes().next_back().unwrap().id;
        assert!(shared.place(Some(&[across]), lower));
        assert!((line_of(&shared, wide)[0].y - 100.0).abs() < 0.5, "the one that stays");
        let gone = shared.strokes().next_back().unwrap();
        assert!(gone.id != wide && gone.ink.erases && (gone.line()[0].y - 250.0).abs() < 0.5, "the one that goes");
        assert_eq!(gone.takes_from.as_deref(), Some(&[across][..]));
        let runs = |c: &VectorContent, id: StrokeId| c.walls().iter().filter(|w| w.source == id.0).count();
        assert_eq!(runs(&shared, across), 3, "erased through twice still, where it lies now");
        assert_eq!(runs(&shared, down), 2, "erased through once still, and not where the other went");
        // carried back, it is as it was drawn: both erasers' marks where they were
        let back = Homography::from_affine(Affine::translate(Vec2::new(0.0, -150.0)));
        assert!(shared.place(Some(&[across]), back));
        assert_eq!((runs(&shared, across), runs(&shared, down)), (3, 2));
        // a stroke carried under an eraser that never touched it isn't erased by it, nor by one
        // made before it
        let mut apart = VectorContent::default();
        straight(&mut apart, (300.0, 380.0), (300.0, 420.0), true);
        let line = straight(&mut apart, (50.0, 100.0), (350.0, 100.0), false);
        straight(&mut apart, (100.0, 380.0), (100.0, 420.0), true);
        assert_eq!(runs(&apart, line), 1);
        assert!(apart.place(Some(&[line]), Homography::from_affine(Affine::translate(Vec2::new(0.0, 300.0)))));
        assert_eq!(runs(&apart, line), 1, "lying under both now, erased by neither");
        assert_eq!(apart.strokes().filter(|s| s.ink.erases).count(), 1, "the later one, naming nothing, is gone");
        // the stroke gone, the eraser that named only it goes too (one on the fill stays); cut,
        // its pieces keep the eraser's marks
        let mut cut = before.clone();
        straight(&mut cut, (100.0, 240.0), (100.0, 260.0), true);
        assert!(cut.place(Some(&[across]), lower));
        assert_eq!(cut.strokes().filter(|s| s.ink.erases).count(), 2);
        let mut whole = cut.clone();
        assert!(whole.erase_whole(&[Vec2::new(300.0, 250.0)], 5.0));
        let left: Vec<_> = whole.strokes().filter(|s| s.ink.erases).collect();
        assert!(left.len() == 1 && left[0].takes_from.as_deref() == Some(&[fill][..]));
        assert!(cut.erase_to_crossings(&[Vec2::new(300.0, 250.0)], 5.0));
        let piece = cut.strokes().find(|s| s.shown.is_some()).unwrap().id;
        let names: Vec<Vec<StrokeId>> = cut.strokes().filter_map(|s| s.takes_from.clone()).collect();
        assert!(names.iter().any(|ids| ids == &[piece]) && names.iter().all(|ids| !ids.contains(&across)), "{names:?}");
        // a fill is carried by its outline; with none named, everything is
        assert!(content.place(Some(&[fill]), Homography::from_affine(Affine::scale(2.0))));
        assert_eq!(content.fills().next().unwrap().loops[0][2], Vec2::new(240.0, 560.0));
        let mut all = before.clone();
        assert!(all.place(None, lower));
        assert!(all.strokes().all(|s| !s.placement.is_identity()));
        // a map that does nothing, or is no map, carries nothing
        assert!(!all.place(None, Homography::IDENTITY));
        assert!(!all.place(Some(&[StrokeId(99)]), lower));
    }

    #[test]
    fn what_changed_between_two_states_of_a_layer_is_where_their_differences_lie() {
        let mut content = VectorContent::default();
        let across = straight(&mut content, (50.0, 100.0), (350.0, 100.0), false);
        let down = straight(&mut content, (200.0, 220.0), (200.0, 300.0), false);
        let before = content.clone();
        assert!(content.changed_from(&before).is_empty(), "nothing changed");
        // a stroke's bounds hold all its ink, and no more than its ink reaches
        let bounds = content.bounds_of(&[across]);
        assert!(bounds.x0 <= 50 && bounds.x1 >= 350 && bounds.y0 <= 96 && bounds.y1 >= 104, "{bounds:?}");
        assert!(bounds.y0 > 80 && bounds.y1 < 120, "{bounds:?}");
        // a stroke more: where it lies
        let more = straight(&mut content, (60.0, 400.0), (90.0, 400.0), false);
        assert_eq!(content.changed_from(&before), content.bounds_of(&[more]));
        assert_eq!(before.changed_from(&content), content.bounds_of(&[more]), "the same either way round");
        // one gone: where it lay; one carried: where it lay and where it lies
        let mut less = content.clone();
        less.remove(down);
        assert_eq!(less.changed_from(&content), content.bounds_of(&[down]));
        let mut moved = content.clone();
        assert!(moved.place(Some(&[across]), Homography::from_affine(Affine::translate(Vec2::new(0.0, 500.0)))));
        let changed = moved.changed_from(&content);
        assert_eq!(changed, content.bounds_of(&[across]).union(moved.bounds_of(&[across])));
        assert!(changed.y1 > 590 && moved.bounds_of(&[across]).y0 > 580);
        // the layer in two parts: what is picked up, and the rest
        let (picked, rest) = (content.part(&[across], true), content.part(&[across], false));
        assert_eq!(picked.strokes().map(|s| s.id).collect::<Vec<_>>(), [across]);
        assert_eq!(rest.strokes().map(|s| s.id).collect::<Vec<_>>(), [down, more]);
    }

    #[test]
    fn a_stroke_is_recoloured_restyled_and_pulled_into_another_shape() {
        let mut content = VectorContent::default();
        let across = straight(&mut content, (50.0, 100.0), (350.0, 100.0), false);
        let eraser = straight(&mut content, (100.0, 80.0), (100.0, 120.0), true);
        let was = Arc::clone(content.strokes().next().unwrap());
        // recoloured: its colour, nothing else; an eraser's marks take none
        assert!(content.recolour(&[across, eraser], [1.0, 0.0, 0.0]));
        assert!(!content.recolour(&[across], [1.0, 0.0, 0.0]), "already that colour");
        let red = Arc::clone(content.strokes().next().unwrap());
        assert_eq!(red.ink.colour, [1.0, 0.0, 0.0]);
        assert_eq!(red.replay().stamps, was.replay().stamps);
        assert_eq!(content.strokes().nth(1).unwrap().ink.colour, was.ink.colour);
        // restyled twice as wide: the same line, wider
        let wide = red.restyled(Setup { diameter: 24.0, ..red.setup.clone() }, red.ink, None);
        assert_eq!(wide.id, across);
        let (thin, thick) = (red.replay(), wide.replay());
        let widest = |replay: &Replay| replay.stamps.iter().map(|s| s.radius).fold(0.0, f32::max);
        assert!((widest(&thick) - widest(&thin) * 2.0).abs() < 0.5, "{} {}", widest(&thick), widest(&thin));
        assert!(wide.ink_bounds().height() > red.ink_bounds().height() + 8);
        assert!(content.replace_stroke(wide));
        assert_eq!(content.strokes().next().unwrap().setup.diameter, 24.0);
        // a straight line has its two ends to hold it by; one that bends, where it bends too
        assert_eq!(red.nodes(2.0, 10.0, f32::INFINITY).len(), 2);
        // (and along it, when its ends are too far apart to bend it by)
        let along = red.nodes(2.0, 10.0, 60.0);
        assert_eq!(along.len(), 6, "{along:?}");
        assert!(along.windows(2).all(|pair| (pair[1].distance - pair[0].distance - 60.0).abs() < 4.0), "{along:?}");
        let mut bent = VectorContent::default();
        let brush = Brush { streamline: 0.0, ..Brush::round_pen() };
        let mut stroke =
            VectorStroke::begin(bent.next_id(), Setup::new(Arc::new(brush), 12.0, Pointer::Finger, 7), ink());
        let corner: Vec<Sample> = (0..=30)
            .map(|i| Sample::at(Vec2::new(50.0 + i as f32 * 5.0, 100.0), 1.0, f64::from(i) * 8.0))
            .chain(
                (1..=30).map(|i| Sample::at(Vec2::new(200.0, 100.0 + i as f32 * 5.0), 1.0, 240.0 + f64::from(i) * 8.0)),
            )
            .collect();
        stroke.record(&corner);
        bent.push(stroke);
        let stroke = Arc::clone(bent.strokes().next().unwrap());
        let nodes = stroke.nodes(2.0, 10.0, f32::INFINITY);
        assert!(nodes.len() >= 3, "{nodes:?}");
        assert!(nodes.iter().any(|n| n.pos.distance(Vec2::new(200.0, 100.0)) < 12.0), "one at the corner: {nodes:?}");
        // the middle of the straight line pulled 40 down: its middle goes, its ends stay
        let length = red.path().last().unwrap().distance;
        let pulled = red.reshaped(length / 2.0, 0.0, length, Vec2::new(0.0, 40.0));
        let line = pulled.line();
        assert!(
            line[0].distance(Vec2::new(50.0, 100.0)) < 1.0
                && line[line.len() - 1].distance(Vec2::new(350.0, 100.0)) < 1.0
        );
        let lowest = line.iter().map(|p| p.y).fold(0.0, f32::max);
        assert!((lowest - 140.0).abs() < 2.0, "{lowest}");
        assert!(line.windows(2).all(|w| w[0].distance(w[1]) < 8.0), "one smooth line still");
        // an end pulled: all of it that side goes with it, the other end stays
        let swung = red.reshaped(length, 0.0, length, Vec2::new(0.0, -60.0));
        let line = swung.line();
        assert!(
            line[0].distance(Vec2::new(50.0, 100.0)) < 1.0
                && line[line.len() - 1].distance(Vec2::new(350.0, 40.0)) < 1.5
        );
        // a stroke placed elsewhere is pulled by what is seen: 40 on the canvas, whatever its size
        let mut placed = (*red).clone();
        placed.place(Homography::from_affine(Affine::scale(2.0)));
        let pulled = placed.reshaped(length / 2.0, 0.0, length, Vec2::new(0.0, 40.0));
        let lowest = pulled.line().iter().map(|p| p.y).fold(0.0, f32::max);
        assert!((lowest - 240.0).abs() < 3.0, "{lowest}");
    }

    #[test]
    fn a_point_is_found_by_the_stroke_it_is_on() {
        let brush = Brush { streamline: 0.0, ..Brush::round_pen() };
        let setup = Setup::new(Arc::new(brush), 12.0, Pointer::Finger, 7);
        let line: Vec<Sample> =
            (0..=40).map(|i| Sample::at(Vec2::new(20.0 + i as f32 * 5.0, 100.0), 1.0, f64::from(i) * 8.0)).collect();
        let (kept, _) = drawn_live(&setup, &line);
        let again = kept.replay();
        let on = again.nearest(Vec2::new(120.0, 103.0)).unwrap();
        assert!((on.off - 3.0).abs() < 0.1 && on.off < on.radius, "{on:?}");
        assert!((on.along - 100.0).abs() < 1.0, "{on:?}");
        let off = again.nearest(Vec2::new(120.0, 140.0)).unwrap();
        assert!(off.off > off.radius, "{off:?}");
        // past its end, the end is the nearest
        let past = again.nearest(Vec2::new(400.0, 100.0)).unwrap();
        assert!((past.along - 200.0).abs() < 1.0, "{past:?}");
    }
}

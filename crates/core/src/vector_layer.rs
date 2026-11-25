//! Vector layers: layers whose strokes are kept as strokes (`procraft_engine::vector`).
//!
//! A stroke on a vector layer is drawn as on any other, by the same builder onto the same pixels;
//! as it ends, what it was drawn from (its input, its brush and how it was set up) is kept on the
//! layer, with the document, as part of the stroke's own undo step. The layer's pixels are then
//! always what its strokes draw, lowest first (the tests make them again from the strokes alone,
//! and check they come out the same).
//!
//! What changes pixels alone (a fill, a filter, a transform, a smudge) leaves a vector layer be
//! until it is rasterized, as it leaves a text layer.

use std::sync::Arc;

use procraft_engine::brush::Brush;
use procraft_engine::document::{Document, LayerId};
use procraft_engine::geom::Vec2;
use procraft_engine::geom::{Homography, IRect};
use procraft_engine::input::Sample;
use procraft_engine::selection::Mask;
use procraft_engine::stroke::{PathSpot, Stamp};
use procraft_engine::vector::{
    FillMode, Ink, Object, ObjectKind, Second, Setup, StrokeId, TracedFrom, VectorContent, VectorFill, VectorStroke,
};
use procraft_engine::vector_fill::Wall;
use procraft_gpu::{Canvas, SecondStroke, StrokeStyle, StrokeTool};

use crate::selection::LassoFill;
use crate::{CanvasSession, State};

/// How the eraser works on a vector layer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, uniffi::Enum)]
pub enum VectorEraser {
    /// It takes away what its brush covers, as on any layer.
    #[default]
    Subtract,
    /// Every stroke it touches goes, all of it.
    WholeStroke,
    /// A stroke it touches goes from where it crosses another before to where it crosses one
    /// after.
    ToCrossing,
}

/// The eraser taking whole strokes, or up to their crossings, as it is dragged: where it has been
/// (canvas pixels) and half its width. What it touched goes when it lifts.
pub(crate) struct VectorErasing {
    layer: LayerId,
    radius: f32,
    /// Where it last was (canvas pixels): it takes what lies between there and where it goes next.
    last: Option<Vec2>,
    /// The document as it was before the eraser took anything; none until it has.
    before: Option<Document>,
}

/// The selected strokes and fills of a vector layer picked up onto a layer of their own, to be
/// shown moving as they are transformed (see [`State::vector_lift`]).
pub(crate) struct VectorLifted {
    /// The layer they were picked up onto.
    lifted: LayerId,
    ids: Vec<StrokeId>,
    /// The selection as it was (it is set aside while they are moved).
    selection: Option<Mask>,
}

/// Whether selection `mask` takes canvas point `p` (at least half of the pixel there).
fn selected_at(mask: &Mask, p: Vec2) -> bool {
    if !(p.x >= 0.0 && p.y >= 0.0 && p.x < mask.width as f32 && p.y < mask.height as f32) {
        return false;
    }
    mask.data.get(p.y as usize * mask.width as usize + p.x as usize).is_some_and(|&m| m >= 128)
}

/// How what is transformed on a vector layer is carried to where the transform put it.
#[derive(Clone)]
pub(crate) enum Carried {
    /// Moved, turned, resized or pulled out of square: one map of the canvas onto itself.
    By(Homography),
    /// Bent: through a warp's mesh, or a puppet's.
    Bent(procraft_engine::transform::Bend),
}

impl Carried {
    /// Carries the strokes and fills named in `ids` (all, with none given); whether any went.
    fn carry(self, content: &mut VectorContent, ids: Option<&[StrokeId]>) -> bool {
        match self {
            Carried::By(map) => content.place(ids, map),
            Carried::Bent(bend) => content.bend(ids, &|p| bend.point(p)),
        }
    }
}

/// The name of the layer a vector layer's selected strokes and fills are picked up onto while they
/// are transformed.
const LIFTED_NAME: &str = "Transforming";

/// A vector layer's warp shown as it will be kept. Any other transform shows the picture of the
/// strokes moved, which is what keeping it gives (a stroke resized is as much wider). A warp bends
/// each stroke along where its line goes, as wide as it was: the picture pulled through the mesh
/// would show the lines thicker where it stretches and thinner where it squeezes, which is not
/// what is kept. So while the mesh is up the strokes themselves are drawn again through it, on the
/// layer shown moving, whose pixels are held meanwhile (nothing drawn so is a step of its own, and
/// they are put back when the warp ends, kept or not: kept, the strokes are then bent for good and
/// drawn once more).
///
/// Bending a stroke is drawing it again from its input along another line, which takes too long
/// to do for every stroke each time a finger moves. So while a handle is held the strokes are
/// drawn roughly: the stamps each was made of, worked out once, put where the mesh takes them,
/// as big as they were (a line much stretched shows its stamps further apart than it will be
/// kept with); and fills, their outlines carried, are laid without reading what is under them.
/// When the handle is let go they are drawn exactly, as keeping the warp leaves them, unless
/// that takes long enough to be felt each time ([`WARP_EXACT_WITHIN`]): a layer of very many
/// strokes stays as it was drawn while dragged, and is exact once kept.
///
/// Either is done once for each frame drawn, however many times the mesh moved since the last.
/// On a layer whose strokes take longer than a frame to draw even roughly ([`WARP_EVERY_FRAME`]),
/// drawing them each time left the lines trailing the mesh by several frames: there, while a
/// handle is held, the picture of the strokes is pulled through the mesh instead (as any layer's
/// is, every frame: the lines keep up with the finger, wider and narrower as the mesh stretches
/// them for as long as it is held), and the strokes are drawn once it is let go.
pub(crate) struct VectorWarp {
    /// The layer drawn on: the vector layer itself, or the one its selected strokes and fills
    /// were picked up onto.
    target: LayerId,
    /// The layers whose picture was being moved before the mesh came up (the target and its mask),
    /// to take up again should the warp be taken back.
    moved: Vec<LayerId>,
    /// What the target holds, unbent, and which of it is bent (none: all of it).
    from: Arc<VectorContent>,
    ids: Option<Vec<StrokeId>>,
    /// Whether all the target holds goes through the mesh together (it does but for a selection
    /// that couldn't be picked up onto a layer of its own): only then is it drawn roughly.
    whole: bool,
    /// The stamps each stroke of `from` is made of, as they lie unbent: worked out the first time
    /// they are wanted, with whether drawing every stroke exactly is quick enough to do each time
    /// a handle is let go.
    stamps: Option<(Arc<StrokeStamps>, bool)>,
    /// What the target shows now: the mesh it was drawn through, and whether exactly.
    shown: Option<(procraft_engine::transform::Bend, bool)>,
    /// When it was last drawn, and how long that took.
    drawn: Option<(std::time::Instant, std::time::Duration)>,
    /// The picture of the strokes is being pulled through the mesh for now (a handle held on a
    /// layer that takes long to draw), as it was last shown: the layer's pixels are then not
    /// held, their picture taken up as in any other transform.
    pulled: Option<procraft_engine::transform::Bend>,
}

/// Each stroke's stamps as they lie on the canvas, and its second's if it has one, by name.
pub(crate) type StrokeStamps = std::collections::HashMap<StrokeId, (Vec<Stamp>, Option<Vec<Stamp>>)>;

/// Drawing a warp's strokes again within this long is done every frame. One that takes longer
/// shows their picture pulled through the mesh while a handle is held, and with none held waits
/// twice as long as it took before the next, so the rest of the app has most of the time.
const WARP_EVERY_FRAME: std::time::Duration = std::time::Duration::from_millis(12);

/// A warp's strokes are drawn exactly each time a handle is let go if that is reckoned to take no
/// longer than this.
const WARP_EXACT_WITHIN: std::time::Duration = std::time::Duration::from_millis(150);

/// `stroke`'s stamps as they lie on the canvas, and its second's: those `given` has for it (a
/// warp drawn as it is dragged), else the stroke drawn again from its input.
fn stamps_of(given: Option<&StrokeStamps>, stroke: &VectorStroke) -> (Vec<Stamp>, Option<Vec<Stamp>>) {
    match given.and_then(|given| given.get(&stroke.id)) {
        Some((first, second)) => (first.clone(), second.clone()),
        None => (stroke.replay().placed_stamps(), stroke.placed_second()),
    }
}

/// A colour being dropped on a vector layer (ColorDrop): the fill it makes is traced again as its
/// threshold, point or colour changes, from the layer as it was, and kept when it ends.
pub(crate) struct VectorPour {
    layer: LayerId,
    /// Where it fills from (canvas pixels); none while that is off the canvas.
    seed: Option<Vec2>,
    colour: [f32; 3],
    /// The threshold it fills with, in thousandths.
    threshold: i32,
    /// The lines it stops at, as they were when it began.
    walls: Vec<Wall>,
    /// The document, and what the layer held, as it began.
    before: Document,
    was: Arc<VectorContent>,
    /// Whether it fills anything as it is now, and whether anything was ever drawn for it.
    filled: bool,
    drawn: bool,
    /// A Continuous drag kept fills it made before this one in `was`.
    kept: bool,
}

/// A fill poured where another was traced takes its place if it covers this much of it; less,
/// and the other stays, with the new one laid over it.
const COVERS: f32 = 0.9;

fn thousandths(threshold: f32) -> i32 {
    if threshold.is_finite() { (threshold.clamp(0.0, 1.0) * 1000.0).round() as i32 } else { 500 }
}

/// How readily a fill on a vector layer takes an opening between lines for a gap and stops at it
/// (0 never, to 1), by the ColorDrop threshold (in thousandths) it is made with. As on any other
/// layer, the further the threshold is slid the further the fill gets: 0.4 at the threshold's
/// middle, where it starts out, no opening a gap at its top, and every one that can be at none.
fn gap_at(threshold: i32) -> f32 {
    let t = (threshold as f32 / 1000.0).clamp(0.0, 1.0);
    if t <= 0.5 { 1.0 - 1.2 * t } else { 0.8 * (1.0 - t) }
}

/// Whether `a` and `b` (a vector layer at two times) draw the same: the same strokes and fills in
/// the same order, a fill made again counting as itself if it has the same outline and colour.
fn same_shapes(a: &VectorContent, b: &VectorContent) -> bool {
    a.len() == b.len()
        && a.objects().iter().zip(b.objects()).all(|pair| match pair {
            (Object::Stroke(a), Object::Stroke(b)) => Arc::ptr_eq(a, b),
            (Object::Fill(a), Object::Fill(b)) => {
                Arc::ptr_eq(a, b) || (a.id == b.id && a.colour == b.colour && a.mode == b.mode && a.loops == b.loops)
            }
            _ => false,
        })
}

/// A canvas of its own, the artwork's size, with one layer: an object's own erasers are drawn on
/// it alone, to find what they leave of the object (see [`State::left_by`]).
pub(crate) struct Scratch {
    doc: Document,
    canvas: Canvas,
}

/// A stroke being drawn on a vector layer: what is kept of it once it ends.
pub(crate) struct OpenVector {
    layer: LayerId,
    stroke: VectorStroke,
    /// Lasso Fill's Outline: what the stroke went round, kept as a fill just under it.
    under: Option<VectorFill>,
}

impl State {
    /// A stroke begins on vector layer `layer`, set up as `setup` and drawn as `style` (with a
    /// dual brush's `second`, as it is set up and drawn): kept from here on. A brush or a guide the
    /// layer's last stroke had is shared with it.
    pub(crate) fn vector_begin(
        &mut self,
        layer: LayerId,
        mut setup: Setup,
        style: &StrokeStyle,
        second: Option<(Setup, f32)>,
    ) {
        let Some(content) = self.doc.layer(layer).and_then(|l| l.vector.as_ref()) else { return };
        let last = content.strokes().next_back();
        if let Some(brush) = last.map(|s| &s.setup.brush).filter(|b| ***b == *setup.brush) {
            setup.brush = Arc::clone(brush);
        }
        let guide = self.doc.stroke_guide();
        let guide = match last.and_then(|s| s.guide.as_ref()).filter(|g| ***g == *guide) {
            Some(same) => Arc::clone(same),
            None => Arc::new(guide.into_owned()),
        };
        let ink = Ink {
            colour: style.color,
            secondary: style.secondary,
            opacity: style.opacity,
            erases: style.tool == StrokeTool::Erase,
            preserve_alpha: style.preserve_alpha,
            wetness: style.wetness,
        };
        let mut stroke = VectorStroke::begin(content.next_id(), setup, ink);
        stroke.guide = Some(guide);
        stroke.second = second.map(|(setup, opacity)| Second { setup, opacity });
        self.vector_open = Some(OpenVector { layer, stroke, under: None });
    }

    /// More of the open stroke's input, as its builder is given it.
    pub(crate) fn vector_record(&mut self, samples: &[Sample]) {
        if let Some(open) = self.vector_open.as_mut() {
            open.stroke.record(samples);
        }
    }

    /// Lasso Fill's Outline on vector layer `layer`: the shape through `outline` (canvas pixels;
    /// with `hard`, pixel art's, on whole pixels) filled with `colour` at `opacity` over what is
    /// there, kept just under the open stroke as it is kept. The pixels it lays are the caller's.
    pub(crate) fn vector_fill_under(
        &mut self,
        layer: LayerId,
        outline: Vec<Vec2>,
        hard: bool,
        colour: [f32; 3],
        opacity: f32,
    ) {
        let Some(open) = self.vector_open.as_mut().filter(|open| open.layer == layer) else { return };
        let Some(content) = self.doc.layer(layer).and_then(|l| l.vector.as_ref()) else { return };
        open.under = Some(VectorFill {
            id: content.next_id(),
            loops: vec![outline],
            colour,
            opacity,
            mode: FillMode::Over,
            hard,
            traced: None,
            takes_from: None,
        });
    }

    /// The open stroke has ended and its pixels are on their way to its layer: it is kept there,
    /// in the same undo step.
    pub(crate) fn vector_keep(&mut self) {
        let Some(OpenVector { layer, stroke, under }) = self.vector_open.take() else { return };
        let before = self.doc.clone();
        // (an eraser that went over nothing took nothing: kept, it would only be clutter)
        let idle = stroke.ink.erases
            && self
                .doc
                .layer(layer)
                .and_then(|l| l.vector.as_ref())
                .is_some_and(|content| !stroke.reach().is_some_and(|reach| content.anything_to_erase(reach)));
        // Lasso Fill's Outline: the fill laid under the stroke, kept just before it
        let filled =
            under.is_some_and(|fill| self.doc.change_vector(layer, |content| content.push_fill(fill)).is_some());
        let kept = !idle && !stroke.samples().is_empty() && self.doc.keep_stroke(layer, stroke);
        if !kept && !filled {
            return;
        }
        // (drawn on the sharp picture too, the stroke's stamps leave the fill out there)
        if filled
            && self.crisp_stroke == Some(layer)
            && let Some(crisp) = self.crisp.as_mut()
        {
            crisp.filled_under = true;
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.record_document(before);
            canvas.collapse_steps();
        }
        self.revision += 1;
    }

    /// `brush`'s images made ready for `style` to read: its shape and grain, and a brush file's
    /// grain and shape at their own sizes.
    pub(crate) fn use_brush_images(&mut self, style: &mut StrokeStyle, brush: &Brush) {
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.use_sources(gpu, style, brush);
        }
        self.use_full_grain(style, brush);
        self.use_full_shape(style, brush);
    }

    /// How a kept stroke is drawn: as [`State::stroke_brush`] made it when it was first drawn,
    /// from the brush and the ink it kept.
    fn vector_style(&mut self, stroke: &VectorStroke) -> StrokeStyle {
        let (brush, ink) = (Arc::clone(&stroke.setup.brush), stroke.ink);
        let tool = if ink.erases { StrokeTool::Erase } else { StrokeTool::Paint };
        let mut style = StrokeStyle::for_brush(&brush, tool, ink.colour, ink.opacity, ink.preserve_alpha);
        style.secondary = ink.secondary;
        style.wetness = ink.wetness;
        self.use_brush_images(&mut style, &brush);
        style
    }

    /// How a kept stroke's second is drawn and combines with the first (drawn as `first`), as
    /// [`State::second_brush`] made it.
    fn vector_second(&mut self, stroke: &VectorStroke, first: &StrokeStyle) -> Option<SecondStroke> {
        let second = stroke.second.as_ref()?;
        let brush = Arc::clone(&second.setup.brush);
        let mut style =
            StrokeStyle::for_brush(&brush, first.tool, first.color, second.opacity, first.preserve_alpha).as_second();
        style.secondary = first.secondary;
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.use_sources(gpu, &mut style, &brush);
        }
        self.use_full_shape(&mut style, &brush);
        let mode = stroke.setup.brush.setting("dual_blend").round().max(0.0) as u32;
        let classic = stroke.setup.brush.setting("classic_combine") >= 0.5;
        Some(SecondStroke { style, mode, threshold: style.alpha_threshold, classic })
    }

    /// Vector layer `layer`'s pixels made again from what it holds alone, in the order it was
    /// made, as one undo step; false if it isn't one, or there is no canvas to draw on. (For the
    /// tests: that a vector layer's strokes and fills make its pixels again exactly.)
    #[cfg(test)]
    pub(crate) fn redraw_vector_layer(&mut self, layer: LayerId) -> bool {
        if !self.doc.is_vector(layer) || self.canvas.is_none() {
            return false;
        }
        self.finish_stroke();
        self.settle();
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
            canvas.name_next_step("Redraw layer");
        }
        self.redraw_vector_steps(layer);
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.collapse_steps();
        }
        self.revision += 1;
        true
    }

    /// The work of [`State::redraw_vector_layer`], as steps of whatever undo step is being made:
    /// the layer cleared, then each stroke drawn and each fill laid, oldest first.
    #[cfg(test)]
    fn redraw_vector_steps(&mut self, layer: LayerId) {
        let Some(content) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return };
        self.draw_vector(layer, &content, None);
    }

    /// Vector layer `layer`'s pixels brought up to what it holds now from what it held (`before`),
    /// as steps of whatever undo step is being made: only where the two differ is drawn again.
    pub(crate) fn redraw_vector_changed(&mut self, layer: LayerId, before: &VectorContent) {
        let Some(content) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return };
        let changed = content.changed_from(before);
        self.draw_vector(layer, &content, Some(changed));
    }

    /// Draws `content` (a vector layer's strokes and fills) onto `layer`, as steps of whatever
    /// undo step is being made: the layer cleared, then each stroke drawn and each fill laid,
    /// oldest first. With `within`, only that rectangle is cleared and drawn again, by the strokes
    /// and fills that reach it, the rest of the layer left as it is: the same pixels there as
    /// drawing all of it gives, unless a stroke that reaches it paints by what is around it (wet
    /// paint, pooled or burnt edges), when all of it is drawn after all.
    pub(crate) fn draw_vector(&mut self, layer: LayerId, content: &VectorContent, within: Option<IRect>) {
        let full = IRect::from_size(self.doc.width(), self.doc.height());
        let mut within = within.map(|rect| rect.intersect(full));
        if within.is_some_and(IRect::is_empty) {
            return;
        }
        let meets =
            |object: &Object, rect: Option<IRect>| rect.is_none_or(|r| !object.bounds().intersect(r).is_empty());
        if within.is_some() {
            let strokes: Vec<Arc<VectorStroke>> = content
                .objects()
                .iter()
                .filter(|o| meets(o, within))
                .filter_map(|o| match o {
                    Object::Stroke(stroke) => Some(Arc::clone(stroke)),
                    Object::Fill(_) => None,
                })
                .collect();
            let around = strokes.iter().any(|stroke| {
                let style = self.vector_style(stroke);
                style.in_place() || style.wet_edges > 0.0 || style.burnt_edges > 0.0
            });
            if around {
                within = None;
            }
        }
        // a selection keeps strokes to itself as they are drawn: these are drawn whole, wherever
        // they lie, so it is set aside until they are (what was waiting to be drawn is, first);
        // drawing a rectangle only, they are kept to it the same way
        let selecting = self.selection.is_some();
        let kept_to: Option<Mask> = within.map(|rect| {
            let mut mask = Mask::empty(full.width(), full.height());
            for y in rect.y0.max(0)..rect.y1.max(0) {
                let row = y as usize * mask.width as usize;
                if let Some(run) = mask.data.get_mut(row + rect.x0.max(0) as usize..row + rect.x1.max(0) as usize) {
                    run.fill(255);
                }
            }
            mask
        });
        if selecting || kept_to.is_some() {
            self.flush();
            if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
                canvas.set_selection(gpu, kept_to.as_ref().map(|m| m.data.as_slice()));
            }
        }
        if let Some(canvas) = self.canvas.as_mut() {
            match within {
                None => canvas.clear_layer(layer),
                Some(rect) => {
                    canvas.write_layer_part(layer, rect, vec![0; rect.width() as usize * rect.height() as usize * 4])
                }
            }
        }
        for object in content.objects() {
            // an eraser that names what it takes from is drawn with each of those, not here
            if (object.erases() && object.takes_from().is_some()) || !meets(object, within) {
                continue;
            }
            // what the object's own erasers leave of it (all of it, if it has none)
            let erasers = if object.erases() { Vec::new() } else { content.erasers_of(object.id()) };
            if let (Object::Stroke(stroke), false) = (object, erasers.is_empty()) {
                // a stroke that paints plainly is drawn alone, erased as its erasers erase, and
                // laid over the rest: just what it was when they were made on it
                let style = self.vector_style(stroke);
                let plain = style.blend == 0 && !style.preserve_alpha && !style.in_place() && !style.luminance;
                if plain && self.lay_stroke_erased(layer, stroke, &erasers, within) {
                    continue;
                }
            }
            // (any other, a stroke that mixes with what is under it or a fill, is kept to what its
            // erasers leave: for a soft eraser a little more than they left, the stroke building
            // up where it is let through)
            let left = (!erasers.is_empty()).then(|| {
                let mut left = self.left_by(&erasers);
                if let Some(kept_to) = &kept_to {
                    for (l, &k) in left.data.iter_mut().zip(&kept_to.data) {
                        *l = (*l).min(k);
                    }
                }
                left
            });
            match object {
                Object::Fill(fill) => {
                    // (a warp drawn as it is dragged lays its fills without reading what is under
                    // them, where it can)
                    let quickly = self.vector_stamps.is_some() && left.is_none() && within.is_none();
                    if !(quickly && self.lay_fill_quickly(layer, fill)) {
                        self.lay_fill(layer, fill, left.as_ref(), within);
                    }
                }
                Object::Stroke(stroke) => {
                    let style = self.vector_style(stroke);
                    let second = self.vector_second(stroke, &style);
                    // drawn only where it is left: kept to that as a stroke is kept to a selection
                    // (what was waiting to be drawn is drawn first)
                    if let Some(left) = &left {
                        self.flush();
                        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
                            canvas.set_selection(gpu, Some(&left.data));
                        }
                    }
                    let (first_stamps, second_stamps) = stamps_of(self.vector_stamps.as_ref(), stroke);
                    let Some(canvas) = self.canvas.as_mut() else { return };
                    // its stamps and the copies its guide draws of them: those that reach the
                    // rectangle, drawing one only
                    let shown_by = self.vector_shown_by;
                    let stamps = |stamps: Vec<Stamp>| {
                        let all = match stroke.guide.as_deref() {
                            Some(guide) => crate::guide::mirrored(guide, &stamps).into_owned(),
                            None => stamps,
                        };
                        let all = match shown_by {
                            Some(by) => procraft_engine::vector::placed(all, by),
                            None => all,
                        };
                        match within {
                            Some(rect) => all.into_iter().filter(|s| !s.bounds().intersect(rect).is_empty()).collect(),
                            None => all,
                        }
                    };
                    match second {
                        Some(second) => {
                            canvas.begin_dual_stroke(layer, style, second);
                            canvas.replace_second_stamps(&stamps(second_stamps.unwrap_or_default()));
                        }
                        None => canvas.begin_stroke(layer, style),
                    }
                    canvas.replace_stamps(&stamps(first_stamps));
                    canvas.end_stroke();
                    if left.is_some() {
                        self.flush();
                        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
                            canvas.set_selection(gpu, kept_to.as_ref().map(|m| m.data.as_slice()));
                        }
                    }
                }
            }
        }
        // (the canvas the erasers were drawn on is let go: it is as big as the artwork)
        self.vector_scratch = None;
        if selecting || kept_to.is_some() {
            self.flush();
            if selecting {
                self.show_selection();
            } else if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
                canvas.set_selection(gpu, None);
            }
        }
    }

    /// On a canvas of its own: `stroke` drawn alone as it paints (or, with none, the layer full),
    /// then the strokes among `erasers` erasing as they erase, each with its own brush. What the
    /// layer holds after (premultiplied RGBA rows) within the rectangle the stroke reaches (with
    /// none, the erasers). None if there is nothing to draw on, or nothing reached.
    fn on_scratch(&mut self, stroke: Option<&VectorStroke>, erasers: &[Object]) -> Option<(IRect, Vec<u8>)> {
        let (w, h) = (self.doc.width(), self.doc.height());
        let gpu = self.gpu.clone()?;
        if self.vector_scratch.as_ref().is_none_or(|s| (s.doc.width(), s.doc.height()) != (w, h)) {
            let mut doc = Document::new(w, h);
            doc.background_visible = false;
            let canvas = Canvas::new(&gpu, &doc).ok()?;
            self.vector_scratch = Some(Scratch { doc, canvas });
        }
        let erasing = erasers.iter().filter_map(|o| match o {
            Object::Stroke(eraser) => Some((&**eraser, StrokeTool::Erase)),
            Object::Fill(_) => None,
        });
        // each stroke as it is drawn, its brush's images made ready on that canvas
        let mut drawn = Vec::new();
        let (mut own, mut theirs) = (IRect::EMPTY, IRect::EMPTY);
        for (kept, tool) in stroke.map(|s| (s, StrokeTool::Paint)).into_iter().chain(erasing) {
            let grains = &mut self.grain_store;
            let scratch = self.vector_scratch.as_mut()?;
            let mut ready = |style: &mut StrokeStyle, brush: &Brush| {
                scratch.canvas.use_sources(&gpu, style, brush);
                if let Some(full) = brush.grain_file_now().and_then(|f| f.full.as_ref())
                    && let Some(pixels) = grains.pixels(full)
                {
                    scratch.canvas.use_full_grain(&gpu, style, full.id, full.side, &pixels);
                }
                if let Some(full) = brush.shape_file_now().map(|f| &f.full)
                    && let Some(pixels) = grains.pixels(full)
                {
                    scratch.canvas.use_full_shape(&gpu, style, full.id, full.side, &pixels);
                }
            };
            let ink = kept.ink;
            let mut style = StrokeStyle::for_brush(&kept.setup.brush, tool, ink.colour, ink.opacity, false);
            style.secondary = ink.secondary;
            style.wetness = ink.wetness;
            ready(&mut style, &kept.setup.brush);
            let second = kept.second.as_ref().map(|second| {
                let mut drawn =
                    StrokeStyle::for_brush(&second.setup.brush, tool, ink.colour, second.opacity, false).as_second();
                drawn.secondary = ink.secondary;
                ready(&mut drawn, &second.setup.brush);
                let mode = kept.setup.brush.setting("dual_blend").round().max(0.0) as u32;
                let classic = kept.setup.brush.setting("classic_combine") >= 0.5;
                SecondStroke { style: drawn, mode, threshold: drawn.alpha_threshold, classic }
            });
            let shown_by = self.vector_shown_by;
            let copies = |stamps: Vec<Stamp>| {
                let all = match kept.guide.as_deref() {
                    Some(guide) => crate::guide::mirrored(guide, &stamps).into_owned(),
                    None => stamps,
                };
                match shown_by {
                    Some(by) => procraft_engine::vector::placed(all, by),
                    None => all,
                }
            };
            let (first_stamps, second_stamps) = stamps_of(self.vector_stamps.as_ref(), kept);
            let stamps = copies(first_stamps);
            let reach = if tool == StrokeTool::Paint { &mut own } else { &mut theirs };
            *reach = stamps.iter().fold(*reach, |reach, stamp| reach.union(stamp.bounds()));
            drawn.push((style, second, stamps, copies(second_stamps.unwrap_or_default())));
        }
        let Scratch { doc, canvas } = self.vector_scratch.as_mut()?;
        let layer = doc.current();
        if stroke.is_some() {
            canvas.wipe_layer(layer);
        } else {
            canvas.fill_layer(layer, [255, 255, 255]);
        }
        for (style, second, stamps, second_stamps) in drawn {
            match second {
                Some(second) => {
                    canvas.begin_dual_stroke(layer, style, second);
                    canvas.replace_second_stamps(&second_stamps);
                }
                None => canvas.begin_stroke(layer, style),
            }
            canvas.replace_stamps(&stamps);
            canvas.end_stroke();
        }
        canvas.render(&gpu, doc, None);
        canvas.forget_history();
        let rect = if stroke.is_some() { own } else { theirs }.intersect(IRect::from_size(w, h));
        if rect.is_empty() {
            return None;
        }
        Some((rect, canvas.read_layer_part(&gpu, layer, rect).ok()?))
    }

    /// How much of each canvas pixel `erasers` (those one stroke or fill names as its own) leave
    /// of it: all (255) where none of them reaches. The eraser's strokes are drawn as they erase
    /// on a canvas of their own that starts full, and what is left of it read back; its lasso's
    /// shapes then take their part.
    fn left_by(&mut self, erasers: &[Object]) -> Mask {
        let (w, h) = (self.doc.width(), self.doc.height());
        let mut left = Mask { width: w, height: h, data: vec![255; w as usize * h as usize] };
        if erasers.iter().any(|o| matches!(o, Object::Stroke(_)))
            && let Some((rect, pixels)) = self.on_scratch(None, erasers)
        {
            let across = rect.width() as usize;
            for (row, line) in pixels.chunks(across * 4).enumerate() {
                let at = (rect.y0.max(0) as usize + row) * w as usize + rect.x0.max(0) as usize;
                let Some(into) = left.data.get_mut(at..at + across) else { continue };
                for (m, px) in into.iter_mut().zip(line.as_chunks::<4>().0) {
                    *m = px[3];
                }
            }
        }
        for object in erasers {
            let Object::Fill(fill) = object else { continue };
            let opacity = fill.opacity.clamp(0.0, 1.0);
            for (m, &cover) in left.data.iter_mut().zip(&self.fill_cover(fill).data) {
                let taken = f32::from(cover) / 255.0 * opacity;
                *m = (f32::from(*m) * (1.0 - taken)).round() as u8;
            }
        }
        left
    }

    /// `stroke` laid on `layer` with its own `erasers`' marks on it: drawn alone on a canvas of
    /// its own, erased there as its erasers erase, and what is left laid over the layer as it is.
    /// False if it couldn't be (it is then to be drawn some other way).
    fn lay_stroke_erased(
        &mut self,
        layer: LayerId,
        stroke: &VectorStroke,
        erasers: &[Object],
        within: Option<IRect>,
    ) -> bool {
        let Some((rect, mut pixels)) = self.on_scratch(Some(stroke), erasers) else {
            // (with a canvas to draw on, it reaches nowhere on it: nothing to lay)
            return self.vector_scratch.is_some();
        };
        // the eraser's lasso's shapes take their part of it
        for object in erasers {
            let Object::Fill(fill) = object else { continue };
            let opacity = fill.opacity.clamp(0.0, 1.0);
            let cover = crate::layer_ops::mask_part(&self.fill_cover(fill), rect);
            for (px, &c) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&cover) {
                let keep = 1.0 - f32::from(c) / 255.0 * opacity;
                for v in px.iter_mut() {
                    *v = (f32::from(*v) * keep).round() as u8;
                }
            }
        }
        // laid only within the rectangle being drawn again, if one is: the rest of it is on the
        // layer already
        let (rect, pixels) = match within.map(|w| w.intersect(rect)) {
            None => (rect, pixels),
            Some(part) if part.is_empty() => return true,
            Some(part) => {
                let (across, wide) = (rect.width() as usize, part.width() as usize);
                let (left, top) = ((part.x0 - rect.x0) as usize, (part.y0 - rect.y0) as usize);
                let mut cropped = Vec::with_capacity(wide * part.height() as usize * 4);
                for row in pixels.chunks(across * 4).skip(top).take(part.height() as usize) {
                    cropped.extend_from_slice(row.get(left * 4..(left + wide) * 4).unwrap_or_default());
                }
                (part, cropped)
            }
        };
        let Some(mut under) = self.layer_part(layer, rect) else { return false };
        // (both premultiplied: what is laid, over what is there)
        for (dst, src) in under.as_chunks_mut::<4>().0.iter_mut().zip(pixels.as_chunks::<4>().0) {
            let through = 255 - u16::from(src[3]);
            for (d, s) in dst.iter_mut().zip(src) {
                *d = (u16::from(*s) + (u16::from(*d) * through + 127) / 255).min(255) as u8;
            }
        }
        let Some(canvas) = self.canvas.as_mut() else { return false };
        canvas.write_layer_part(layer, rect, under);
        true
    }

    /// Lays `fill` on `layer` as [`State::lay_fill`] does, for a picture drawn again and again (a
    /// warp as it is dragged): its cover worked out only across the rectangle it reaches, and laid
    /// on the GPU over, under or out of what is there, nothing read back. (The two round a partly
    /// covered pixel their own ways, a level of 255 apart at most.) False if it can't be laid so
    /// (drawn for the screen), and nothing was.
    fn lay_fill_quickly(&mut self, layer: LayerId, fill: &VectorFill) -> bool {
        if self.vector_shown_by.is_some() {
            return false;
        }
        let full = IRect::from_size(self.doc.width(), self.doc.height());
        let mut points = fill.loops.iter().flatten().filter(|p| p.is_finite());
        let Some(first) = points.next() else { return true };
        let [x0, y0, x1, y1] = points.fold([first.x, first.y, first.x, first.y], |[x0, y0, x1, y1], p| {
            [x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)]
        });
        let rect = IRect::new(x0.floor() as i32 - 1, y0.floor() as i32 - 1, x1.ceil() as i32 + 1, y1.ceil() as i32 + 1)
            .intersect(full);
        if rect.is_empty() {
            return true;
        }
        let (w, h) = (rect.width(), rect.height());
        let corner = Vec2::new(rect.x0 as f32, rect.y0 as f32);
        let mut cover = vec![0u8; w as usize * h as usize];
        for outline in &fill.loops {
            let outline: Vec<Vec2> = outline.iter().map(|p| *p - corner).collect();
            let part = if fill.hard { Mask::polygon_hard(w, h, &outline) } else { Mask::polygon(w, h, &outline) };
            // in one or the other, not both: an outline inside another cuts a hole
            for (a, &b) in cover.iter_mut().zip(&part.data) {
                let (x, y) = (u32::from(*a), u32::from(b));
                *a = (x + y - (2 * x * y + 127) / 255).min(255) as u8;
            }
        }
        let opacity = fill.opacity.clamp(0.0, 1.0);
        let colour = fill.colour.map(|c| c.clamp(0.0, 1.0));
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        // (premultiplied; an eraser's shape has no colour, only how much it takes)
        let [r, g, b] = if fill.mode == FillMode::Erase { [0.0; 3] } else { colour };
        let mut pixels = vec![0u8; cover.len() * 4];
        for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&cover) {
            if m != 0 {
                let k = f32::from(m) / 255.0 * opacity;
                *px = [byte(r * k), byte(g * k), byte(b * k), byte(k)];
            }
        }
        let how = match fill.mode {
            FillMode::Over => procraft_gpu::Lay::Over,
            FillMode::Behind => procraft_gpu::Lay::Under,
            FillMode::Erase => procraft_gpu::Lay::Out,
        };
        let Some(canvas) = self.canvas.as_mut() else { return false };
        canvas.lay_as(layer, rect, pixels, how);
        true
    }

    /// How much of each canvas pixel `fill` covers: inside its outlines by the even-odd rule,
    /// edge pixels partly (or, pixel art's, whole pixels only).
    fn fill_cover(&self, fill: &VectorFill) -> Mask {
        let (w, h) = (self.doc.width(), self.doc.height());
        let mut cover = Mask::empty(w, h);
        for outline in &fill.loops {
            // (drawn for the screen, it is carried where that shows it)
            let carried: Option<Vec<Vec2>> =
                self.vector_shown_by.map(|by| outline.iter().filter_map(|p| by.apply(*p)).collect());
            let outline = carried.as_deref().unwrap_or(outline);
            let part = if fill.hard { Mask::polygon_hard(w, h, outline) } else { Mask::polygon(w, h, outline) };
            // in one or the other, not both: an outline inside another cuts a hole
            for (a, &b) in cover.data.iter_mut().zip(&part.data) {
                let (x, y) = (u32::from(*a), u32::from(b));
                *a = (x + y - (2 * x * y + 127) / 255).min(255) as u8;
            }
        }
        cover
    }

    /// Lays `fill` on `layer`'s pixels as they are now: over them, under them or taken from them,
    /// as it says; only as much of it as `left` leaves (what its own erasers leave of it), if
    /// given. Whether it covered anything.
    fn lay_fill(&mut self, layer: LayerId, fill: &VectorFill, left: Option<&Mask>, within: Option<IRect>) -> bool {
        let mut cover = self.fill_cover(fill);
        if let Some(left) = left {
            for (c, &l) in cover.data.iter_mut().zip(&left.data) {
                *c = ((u16::from(*c) * u16::from(l) + 127) / 255) as u8;
            }
        }
        let Some(rect) = crate::layer_ops::mask_rect(&cover) else { return false };
        // (only within the rectangle being drawn again, if one is)
        let rect = within.map_or(rect, |w| w.intersect(rect));
        if rect.is_empty() {
            return false;
        }
        let Some(mut pixels) = self.layer_part(layer, rect) else { return false };
        let opacity = fill.opacity.clamp(0.0, 1.0);
        let colour = fill.colour.map(|c| c.clamp(0.0, 1.0));
        let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(&crate::layer_ops::mask_part(&cover, rect)) {
            if m == 0 {
                continue;
            }
            // (the layer's pixels are premultiplied)
            let k = f32::from(m) / 255.0 * opacity;
            let [pr, pg, pb, pa] = px;
            let a = f32::from(*pa) / 255.0;
            // how much of the fill shows, and how much of what was there stays
            let (shown, stays) = match fill.mode {
                FillMode::Over => (k, 1.0 - k),
                FillMode::Behind => (k * (1.0 - a), 1.0),
                FillMode::Erase => (0.0, 1.0 - k),
            };
            for (dst, src) in [(&mut *pr, colour[0]), (&mut *pg, colour[1]), (&mut *pb, colour[2])] {
                *dst = byte(src * shown + f32::from(*dst) / 255.0 * stays);
            }
            *pa = byte(shown + a * stays);
        }
        let Some(canvas) = self.canvas.as_mut() else { return false };
        canvas.write_layer_part(layer, rect, pixels);
        true
    }

    /// A change to vector layer `layer`'s content that its pixels follow, as one undo step called
    /// `name`: `change` alters what the layer holds (saying whether it did), and `draw` then brings
    /// the pixels up to it. Whether anything changed.
    fn vector_step(
        &mut self,
        layer: LayerId,
        name: &str,
        change: impl FnOnce(&mut VectorContent) -> bool,
        draw: impl FnOnce(&mut State),
    ) -> bool {
        self.finish_stroke();
        self.settle();
        let before = self.doc.clone();
        if self.doc.change_vector(layer, change) != Some(true) {
            return false;
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
            canvas.name_next_step(name);
        }
        draw(self);
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.record_document(before);
            canvas.collapse_steps();
        }
        self.revision += 1;
        true
    }

    /// What a vector layer holding `was` holds with the region around `seed` (canvas pixels)
    /// filled with `colour`: the region bounded by `walls`, an opening counting as a gap as readily
    /// as `factor` says, kept as a fill laid under what is there. A fill traced there before (one
    /// the seed is inside) gives way to this one if this covers it, all but ([`COVERS`]): the same
    /// region poured again, or a smaller one whose lines have since gone. One that reaches further
    /// (the colour round a shape drawn on it since, or a fill that got out through a gap into more
    /// than this region) stays whole, and this one is laid over it. None if nothing could be
    /// filled.
    fn poured(
        &self,
        was: &VectorContent,
        walls: &[Wall],
        seed: Vec2,
        colour: [f32; 3],
        factor: f32,
    ) -> Option<VectorContent> {
        let traced = procraft_engine::vector_fill::trace(walls, self.doc.width(), self.doc.height(), seed, factor)?;
        let (replaced, under): (Vec<_>, Vec<_>) = was
            .fills()
            .filter(|f| f.traced.is_some() && procraft_engine::vector_fill::inside(&f.loops, seed))
            .partition(|f| procraft_engine::vector_fill::covered(&traced.loops, &f.loops) >= COVERS);
        let (replaced, under): (Vec<StrokeId>, Vec<StrokeId>) =
            (replaced.iter().map(|f| f.id).collect(), under.iter().map(|f| f.id).collect());
        let fill = VectorFill {
            id: was.next_id(),
            loops: traced.loops,
            colour,
            opacity: 1.0,
            mode: FillMode::Behind,
            hard: false,
            traced: Some(TracedFrom {
                seed,
                factor,
                sources: traced.sources.into_iter().map(procraft_engine::vector::StrokeId).collect(),
            }),
            takes_from: None,
        };
        let mut now = was.clone();
        for id in replaced {
            now.remove(id);
        }
        now.push_fill_over(fill, &under);
        Some(now)
    }

    /// ColorDrop on vector layer `layer`: `colour` dropped at `seed` (canvas pixels) with
    /// `threshold` (0..1) fills the region around it that the strokes' lines bound, shown at once.
    /// The lines are those of [`State::vector_fill_lines`]: every vector layer's that shows with
    /// All Layers on, else the reference layer's if another vector layer is it, else this layer's
    /// own.
    /// The drop stays open, to be traced again as its threshold, point or colour changes, until
    /// [`State::vector_pour_end`] makes it one undo step. False if there is nothing to fill from
    /// (no lines at all, and no region found).
    pub(crate) fn vector_pour_begin(&mut self, layer: LayerId, seed: Vec2, colour: [f32; 3], threshold: f32) -> bool {
        self.finish_stroke();
        self.settle();
        let Some(was) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return false };
        let walls: Vec<Wall> = self
            .vector_fill_lines(layer)
            .into_iter()
            .filter_map(|id| self.doc.layer(id).and_then(|l| l.vector.as_ref()))
            .flat_map(|content| content.walls())
            .collect();
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
        }
        let lined = !walls.is_empty();
        self.vector_pour = Some(VectorPour {
            layer,
            seed: Some(seed),
            colour,
            threshold: thousandths(threshold),
            walls,
            before: self.doc.clone(),
            was,
            filled: false,
            drawn: false,
            kept: false,
        });
        // (with lines but no region found yet, it stays open: another threshold may find one)
        if !self.vector_pour_show() && !lined {
            self.vector_pour_end(false, None);
            return false;
        }
        true
    }

    /// The open drop on a vector layer traced again as it now is, and the layer drawn again where
    /// that differs from what it showed. Whether it fills anything.
    fn vector_pour_show(&mut self) -> bool {
        // (set aside while it draws, so nothing that settles the canvas on the way ends it)
        let Some(mut pour) = self.vector_pour.take() else { return false };
        let layer = pour.layer;
        let next =
            pour.seed.and_then(|seed| self.poured(&pour.was, &pour.walls, seed, pour.colour, gap_at(pour.threshold)));
        pour.filled = next.is_some();
        let now = next.unwrap_or_else(|| (*pour.was).clone());
        if let Some(held) = self.doc.layer(layer).and_then(|l| l.vector.clone())
            && !same_shapes(&now, &held)
        {
            let changed = now.changed_from(&held);
            self.doc.set_vector(layer, now.clone());
            self.draw_vector(layer, &now, Some(changed));
            pour.drawn = true;
            self.revision += 1;
        }
        let filled = pour.filled;
        self.vector_pour = Some(pour);
        filled
    }

    /// The open drop on a vector layer traced from `seed` (canvas pixels) instead; from none (off
    /// the canvas) it fills nothing until it comes back. False if no such drop is open.
    pub(crate) fn vector_pour_move(&mut self, seed: Option<Vec2>) -> bool {
        let Some(pour) = self.vector_pour.as_mut() else { return false };
        if pour.seed != seed {
            pour.seed = seed;
            self.vector_pour_show();
        }
        true
    }

    /// The open drop on a vector layer in `colour` instead. False if no such drop is open.
    pub(crate) fn vector_pour_colour(&mut self, colour: [f32; 3]) -> bool {
        let Some(pour) = self.vector_pour.as_mut() else { return false };
        if pour.colour != colour {
            pour.colour = colour;
            self.vector_pour_show();
        }
        true
    }

    /// The open drop on a vector layer traced at `threshold` (0..1) instead: the further it is
    /// slid, the wider an opening the fill gets out through. False if no such drop is open.
    pub(crate) fn vector_pour_threshold(&mut self, threshold: f32) -> bool {
        let Some(pour) = self.vector_pour.as_mut() else { return false };
        let threshold = thousandths(threshold);
        if pour.threshold != threshold {
            pour.threshold = threshold;
            self.vector_pour_show();
        }
        true
    }

    /// Ends the open drop on a vector layer: `keep` makes its fill one undo step, called `name` if
    /// given (else "ColorDrop"); otherwise (or with nothing filled) the layer is as it was, with
    /// no step left of it. False if no such drop is open.
    pub(crate) fn vector_pour_end(&mut self, keep: bool, name: Option<&str>) -> bool {
        let Some(pour) = self.vector_pour.take() else { return false };
        let keep = keep && (pour.filled || pour.kept);
        let Some(canvas) = self.canvas.as_mut() else { return true };
        // nothing was ever drawn for it: there is only its mark to use up
        if !pour.drawn {
            canvas.collapse_steps();
            return true;
        }
        if keep {
            canvas.name_next_step(name.unwrap_or("ColorDrop"));
        }
        canvas.record_document(pour.before);
        canvas.collapse_steps();
        if !keep {
            // all it drew is one step, taken back and forgotten
            canvas.undo();
            self.flush();
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.forget_redo();
            }
        }
        self.revision += 1;
        true
    }

    /// Whether canvas point `at` on vector layer `layer` is unfilled, for a Continuous drag: no
    /// fill there, nor a stroke's ink, on the layer or on the layer whose lines it fills up to.
    pub(crate) fn vector_unfilled(&self, layer: LayerId, at: Vec2) -> bool {
        let clear =
            |id| self.doc.layer(id).and_then(|l| l.vector.as_ref()).is_none_or(|c| c.object_at(at, 0.0).is_none());
        clear(layer) && self.vector_fill_lines(layer).into_iter().all(clear)
    }

    /// The vector layers whose strokes' lines a fill on vector layer `layer` stops at. With All
    /// Layers on (as a fill on any other layer measures every layer that shows): every vector layer
    /// that shows, and `layer` itself, shown or not. Else the reference layer alone if another
    /// vector layer is it, else `layer` alone.
    fn vector_fill_lines(&self, layer: LayerId) -> Vec<LayerId> {
        if self.pour_all_layers {
            return self
                .doc
                .layers()
                .iter()
                .filter(|l| l.vector.is_some() && (l.id == layer || self.doc.shows(l.id)))
                .map(|l| l.id)
                .collect();
        }
        vec![self.doc.reference().filter(|&r| r != layer && self.doc.is_vector(r)).unwrap_or(layer)]
    }

    /// A Continuous drag on vector layer `layer` reached unfilled `seed` (canvas pixels): the open
    /// drop keeps the fill it has made and traces one from here as well (begun here, with
    /// `colour` and `threshold`, if none is open), so the drag ends as one undo step. Whether it
    /// fills here.
    pub(crate) fn vector_pour_more(&mut self, layer: LayerId, seed: Vec2, colour: [f32; 3], threshold: f32) -> bool {
        let Some(pour) = self.vector_pour.as_mut().filter(|p| p.layer == layer) else {
            // (the fill is kept whole: what was selected is let go of first)
            self.finish_stroke();
            self.settle();
            self.let_go_of_selection();
            return self.vector_pour_begin(layer, seed, colour, threshold)
                && self.vector_pour.as_ref().is_some_and(|p| p.filled);
        };
        if pour.filled
            && let Some(held) = self.doc.layer(layer).and_then(|l| l.vector.clone())
        {
            pour.was = held;
            pour.kept = true;
        }
        pour.seed = Some(seed);
        self.vector_pour_show()
    }

    /// Lasso Fill on a vector layer: the shape through `points` (canvas pixels) kept as a fill of
    /// the paint colour over what is there, or (with `erase`) as an eraser's shape, which is kept
    /// only if there is something under it to take from. Whether anything changed.
    pub(crate) fn vector_lasso(
        &mut self,
        layer: LayerId,
        points: Vec<Vec2>,
        hard: bool,
        erase: bool,
        opacity: f32,
    ) -> bool {
        let Some(content) = self.doc.layer(layer).and_then(|l| l.vector.as_ref()) else { return false };
        let fill = VectorFill {
            id: content.next_id(),
            loops: vec![points],
            colour: self.color,
            opacity,
            mode: if erase { FillMode::Erase } else { FillMode::Over },
            hard,
            traced: None,
            takes_from: None,
        };
        let reach = Object::Fill(Arc::new(fill.clone())).reach();
        if self.fill_cover(&fill).is_empty() || (erase && !reach.is_some_and(|r| content.anything_to_erase(r))) {
            return false;
        }
        let kept = fill.clone();
        self.vector_step(
            layer,
            if erase { "lasso erase" } else { "lasso fill" },
            |content| {
                content.push_fill(kept);
                true
            },
            |st| {
                st.lay_fill(layer, &fill, None, None);
            },
        )
    }

    /// How much of each canvas pixel the strokes and fills named in `ids` on vector layer `layer`
    /// take up, as a selection: a stroke's line as wide as its ink (and a little over), a fill's
    /// inside. None if it isn't a vector layer.
    pub(crate) fn vector_footprint(&self, layer: LayerId, ids: &[StrokeId]) -> Option<Mask> {
        let content = self.doc.layer(layer)?.vector.as_ref()?;
        let (w, h) = (self.doc.width(), self.doc.height());
        let mut mask = Mask::empty(w, h);
        // a disc `radius` about `at`, all of it taken
        fn disc(mask: &mut Mask, at: Vec2, radius: f32) {
            let (w, h) = (mask.width, mask.height);
            let (top, bottom) = ((at.y - radius).floor().max(0.0), (at.y + radius).ceil().min(h as f32 - 1.0));
            let mut y = top;
            while y <= bottom {
                let half = (radius * radius - (y + 0.5 - at.y).powi(2)).max(0.0).sqrt();
                let (x0, x1) = ((at.x - half).floor().max(0.0), (at.x + half).ceil().min(w as f32));
                if x1 > x0 {
                    let row = y as usize * w as usize;
                    if let Some(run) = mask.data.get_mut(row + x0 as usize..row + x1 as usize) {
                        run.fill(255);
                    }
                }
                y += 1.0;
            }
        }
        for object in content.objects().iter().filter(|o| ids.contains(&o.id())) {
            match object {
                Object::Stroke(stroke) => {
                    let spots = stroke.placed_path();
                    if let [only] = spots.as_slice() {
                        disc(&mut mask, only.pos, only.radius + 1.5);
                    }
                    // (discs close enough along each stretch to leave no gaps in a thin line)
                    for pair in spots.windows(2) {
                        let [a, b] = pair else { continue };
                        let steps =
                            (a.pos.distance(b.pos) / (a.radius.min(b.radius) * 0.5 + 0.75)).ceil().clamp(1.0, 256.0);
                        for i in 0..=steps as usize {
                            let t = i as f32 / steps;
                            disc(&mut mask, a.pos.lerp(b.pos, t), a.radius + (b.radius - a.radius) * t + 1.5);
                        }
                    }
                }
                Object::Fill(fill) => {
                    for (a, &b) in mask.data.iter_mut().zip(&self.fill_cover(fill).data) {
                        *a = (*a).max(b);
                    }
                }
            }
        }
        Some(mask)
    }

    /// A shape drawn to select with, as the current layer takes it: on a vector layer, the
    /// strokes and fills it touches, whole (none if it touches none); on any other, itself.
    pub(crate) fn snapped_to_objects(&self, shape: Mask) -> Option<Mask> {
        let layer = self.doc.current();
        let Some(content) = self.doc.layer(layer).and_then(|l| l.vector.as_ref()).filter(|_| !self.scene_shown())
        else {
            return Some(shape);
        };
        let taken = content.objects_within(|p| selected_at(&shape, p));
        if taken.is_empty() {
            return None;
        }
        self.vector_footprint(layer, &taken)
    }

    /// The stroke or fill of the current (vector) layer at canvas point `at`, as a selection: the
    /// newest there, a stroke counting a few screen points from its ink. None if there is none.
    pub(crate) fn footprint_at(&self, at: Vec2) -> Option<Mask> {
        let layer = self.doc.current();
        let content = self.doc.layer(layer)?.vector.as_ref()?;
        let slack = 6.0 * self.display_scale / self.view.map_or(1.0, |v| v.zoom()).max(1e-6);
        let picked = content.object_at(at, slack)?;
        self.vector_footprint(layer, &[picked])
    }

    /// The strokes and fills of vector layer `layer` that are selected: those the selection
    /// covers at least half of. None with no selection (all of them are meant then).
    pub(crate) fn vector_selected(&self, layer: LayerId) -> Option<Vec<StrokeId>> {
        let mask = self.selection.as_ref()?;
        let content = self.doc.layer(layer)?.vector.as_ref()?;
        let mostly = |points: &mut dyn Iterator<Item = Vec2>| {
            let (mut all, mut taken) = (0u32, 0u32);
            for p in points {
                all += 1;
                taken += u32::from(selected_at(mask, p));
            }
            all > 0 && taken * 2 >= all
        };
        let selected = |object: &Object| match object {
            Object::Stroke(stroke) => !stroke.ink.erases && mostly(&mut stroke.line().into_iter()),
            Object::Fill(fill) => {
                // a grid of points over it, those inside it
                let Some([x0, y0, x1, y1]) = object.reach() else { return false };
                let mut inside = (0..24 * 24).filter_map(|i| {
                    let p = Vec2::new(
                        x0 + (x1 - x0) * ((i % 24) as f32 + 0.5) / 24.0,
                        y0 + (y1 - y0) * ((i / 24) as f32 + 0.5) / 24.0,
                    );
                    procraft_engine::vector_fill::inside(&fill.loops, p).then_some(p)
                });
                fill.mode != FillMode::Erase && mostly(&mut inside)
            }
        };
        Some(content.objects().iter().filter(|o| selected(o)).map(Object::id).collect())
    }

    /// A change to vector layer `layer`'s content as [`State::vector_step`] makes one, the layer's
    /// pixels drawn again only where what it holds changed.
    fn vector_changed_step(
        &mut self,
        layer: LayerId,
        name: &str,
        change: impl FnOnce(&mut VectorContent) -> bool,
    ) -> bool {
        let Some(was) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return false };
        self.vector_step(layer, name, change, |st| st.redraw_vector_changed(layer, &was))
    }

    /// The selected strokes and fills of vector layer `layer` (all of them with no selection)
    /// carried by `map`, canvas to canvas, as one undo step called `name`: the same stamps placed
    /// elsewhere, the layer drawn again where they lay and where they lie. Whether anything was
    /// carried.
    pub(crate) fn vector_place(&mut self, layer: LayerId, map: Carried, name: &str) -> bool {
        let selected = self.vector_selected(layer);
        self.vector_changed_step(layer, name, |content| map.carry(content, selected.as_deref()))
    }

    /// The selected strokes and fills of vector layer `layer` gone, as one undo step. Whether any
    /// went.
    pub(crate) fn clear_selected_objects(&mut self, layer: LayerId) -> bool {
        let Some(selected) = self.vector_selected(layer) else { return false };
        // (every one of them, not only up to the first that went)
        self.vector_changed_step(layer, "Clear", |content| {
            selected.iter().filter(|id| content.remove(**id)).count() > 0
        })
    }

    /// The selected strokes and fills of vector layer `layer` picked up onto a layer of their own
    /// just above it, to be transformed: what is shown moving is then just them, the rest of the
    /// layer staying whole under them. The layer's pixels are drawn without them meanwhile; what
    /// it holds is as it was until they are set down ([`State::vector_set_down`]). Whether they
    /// were picked up (not with nothing selected, when all of the layer moves as it is, nor with
    /// no room for another layer).
    pub(crate) fn vector_lift(&mut self, layer: LayerId) -> bool {
        self.vector_lifted = None;
        if !self.doc.is_vector(layer) || self.doc.is_locked(layer) || self.scene_shown() || !self.doc.has_room(1) {
            return false;
        }
        let Some(ids) = self.vector_selected(layer).filter(|ids| !ids.is_empty()) else { return false };
        let Some(content) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return false };
        let Some((opacity, blend, clipped)) = self.doc.layer(layer).map(|l| (l.opacity, l.blend, l.clipped)) else {
            return false;
        };
        if self.canvas.is_none() || self.gpu.is_none() {
            return false;
        }
        // (the selection is set aside: nothing is kept to it while they move)
        let selection = self.selection.take();
        self.show_selection();
        let before = self.doc.clone();
        let lifted = self.doc.add_layer();
        self.doc.rename(lifted, LIFTED_NAME);
        self.doc.set_private(lifted, true);
        self.doc.set_opacity(lifted, opacity);
        self.doc.set_blend(lifted, blend);
        self.doc.set_clipped(lifted, clipped);
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
            canvas.record_document(before);
        }
        // the layer without them where they lay, and them alone on theirs
        let (picked, rest) = (content.part(&ids, true), content.part(&ids, false));
        self.draw_vector(layer, &rest, Some(content.bounds_of(&ids)));
        self.draw_vector(lifted, &picked, None);
        self.vector_lifted = Some(VectorLifted { lifted, ids, selection });
        true
    }

    /// The strokes and fills picked up from vector layer `layer` set down again: carried by `map`
    /// (canvas to canvas) and kept there, the whole of it one undo step called `name`; or, with no
    /// map (the transform called off), everything put back as it was, with no step.
    pub(crate) fn vector_set_down(&mut self, layer: LayerId, lifted: VectorLifted, map: Option<Carried>, name: &str) {
        let VectorLifted { lifted, ids, selection } = lifted;
        let was = self.doc.layer(layer).and_then(|l| l.vector.clone());
        match (map, was) {
            (Some(map), Some(was)) => {
                let before = self.doc.clone();
                self.doc.delete_layer(lifted);
                self.doc.set_current(layer);
                self.doc.change_vector(layer, |content| map.carry(content, Some(&ids)));
                // the layer has been without them where they lay: drawn again there, and where they
                // lie now
                if let Some(now) = self.doc.layer(layer).and_then(|l| l.vector.clone()) {
                    let rect = was.bounds_of(&ids).union(now.changed_from(&was));
                    self.draw_vector(layer, &now, Some(rect));
                }
                if let Some(canvas) = self.canvas.as_mut() {
                    canvas.name_next_step(name);
                    canvas.record_document(before);
                }
                // the selection, which is the shape they take up, goes with them in the same step:
                // whatever later lets go of it, undone, brings it back where they are
                if selection.is_some() {
                    self.selection = selection.clone();
                    let before = self.selection.clone();
                    self.selection = self.vector_footprint(layer, &ids);
                    self.record_selection(before, name);
                }
                if let Some(canvas) = self.canvas.as_mut() {
                    canvas.collapse_steps();
                }
                self.revision += 1;
                self.show_selection();
                return;
            }
            _ => {
                // all that was done to pick them up is one step, taken back and forgotten
                if let Some(canvas) = self.canvas.as_mut() {
                    canvas.collapse_steps();
                    canvas.undo();
                }
                self.flush();
                if let Some(canvas) = self.canvas.as_mut() {
                    canvas.forget_redo();
                }
            }
        }
        self.selection = selection;
        self.show_selection();
    }

    /// The open transform of a vector layer has its mesh up: from now on its strokes are shown
    /// drawn through it ([`VectorWarp`]), when the next frame is ([`State::vector_warp_draw`]).
    /// The picture of them that was being moved is put back and the layer's pixels held. False if
    /// there is nothing to show so (no such transform, or nothing to draw on), and the picture is
    /// pulled through the mesh as any layer's is.
    pub(crate) fn vector_warp_begin(&mut self) -> bool {
        if self.vector_warp.is_some() {
            return true;
        }
        let Some(layer) = self.vector_transform else { return false };
        let Some(content) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return false };
        if self.canvas.is_none() || self.gpu.is_none() {
            return false;
        }
        // picked up onto a layer of their own, that layer holds just them: all it holds is bent
        let (target, from, ids) = match &self.vector_lifted {
            Some(lifted) => (lifted.lifted, Arc::new(content.part(&lifted.ids, true)), Some(lifted.ids.clone())),
            None => (layer, content, self.vector_selected(layer)),
        };
        let whole = self.vector_lifted.is_some() || ids.is_none();
        let moved: Vec<LayerId> = std::iter::once(target).chain(self.doc.mask_of(target)).collect();
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.end_transform(false);
            canvas.hold_layer_quietly(target);
        }
        self.vector_warp =
            Some(VectorWarp { target, moved, from, ids, whole, stamps: None, shown: None, drawn: None, pulled: None });
        true
    }

    /// The warp shown ([`VectorWarp`]) brought up to the mesh as it is now, if it moved since it
    /// was last drawn, or its handle was let go and its strokes are to be drawn exactly. Called as
    /// each frame is drawn; one that takes long is put off (unless `now`) until twice as long as
    /// it took has passed since the last, and says so (true: draw another frame soon).
    pub(crate) fn vector_warp_draw(&mut self, now: bool) -> bool {
        let Some((mesh, source, held)) =
            self.transform.as_ref().and_then(|t| Some((t.bend()?, t.source(), t.dragging())))
        else {
            return false;
        };
        let Some(warp) = self.vector_warp.as_ref() else { return false };
        // roughly while a handle is held (and after, where exactly takes too long): but only what
        // goes through the mesh whole can be
        let quick_enough = warp.stamps.as_ref().is_none_or(|(_, exact)| *exact);
        let exactly = !warp.whole || (!held && quick_enough);
        if warp.shown.as_ref().is_some_and(|(shown, exact)| *shown == mesh && (*exact || !exactly)) {
            return false;
        }
        // one that takes long to draw, a handle held: the picture pulled through the mesh instead,
        // every frame (see [`VectorWarp`])
        let slow = warp.whole && warp.drawn.is_some_and(|(_, took)| took > WARP_EVERY_FRAME);
        if held && slow && !now {
            if warp.pulled.as_ref() == Some(&mesh) {
                return false;
            }
            let taken_up = warp.pulled.is_some();
            let moved = warp.moved.clone();
            if !taken_up && let Some(canvas) = self.canvas.as_mut() {
                // the layer's pixels back as they were, and their picture taken up again
                canvas.release_layer(false, None);
                canvas.begin_transform_layers(&moved);
            }
            self.show_placed();
            if let Some(warp) = self.vector_warp.as_mut() {
                warp.pulled = Some(mesh.clone());
                warp.shown = None;
            }
            return false;
        }
        let started = std::time::Instant::now();
        if !now
            && let Some((at, took)) = warp.drawn
            && took > WARP_EVERY_FRAME
            && started.duration_since(at) < took * 2
        {
            return true;
        }
        let Some(mut warp) = self.vector_warp.take() else { return false };
        // (their picture was being pulled through the mesh: put back, and the layer's pixels held
        // again for the strokes to be drawn on)
        if warp.pulled.take().is_some()
            && let Some(canvas) = self.canvas.as_mut()
        {
            canvas.end_transform(false);
            canvas.hold_layer_quietly(warp.target);
        }
        // each stroke's stamps as they lie unbent, the first time: what drawing them roughly
        // starts from, and (by how long they took) how long drawing them exactly would
        if warp.whole && warp.stamps.is_none() {
            let stamps: StrokeStamps = warp.from.strokes().map(|stroke| (stroke.id, stamps_of(None, stroke))).collect();
            // (a bent stroke's input is closer set than the stroke's own: drawing it again takes
            // about half as long again)
            let exact = started.elapsed() * 3 / 2 <= WARP_EXACT_WITHIN;
            warp.stamps = Some((Arc::new(stamps), exact));
        }
        let exactly = !warp.whole || (!held && warp.stamps.as_ref().is_none_or(|(_, exact)| *exact));
        let [top_left, top_right, _, bottom_left] = source;
        let (w, h) = (top_right.x - top_left.x, bottom_left.y - top_left.y);
        let through = |p: Vec2| mesh.point(p);
        let flat = !(w.abs() > 1e-6 && h.abs() > 1e-6);
        // (the transform is set aside while they are drawn: what settles the canvas on the way, a
        // fill reading what is under it, would end it)
        let (transform, of) = (self.transform.take(), self.vector_transform.take());
        match warp.stamps.as_ref().filter(|_| !exactly && !flat) {
            Some((stamps, _)) => {
                // the stamps where the mesh takes them, as big as they were; the fills' outlines
                // carried; all else as it is
                let carried = |stamps: &[Stamp]| -> Vec<Stamp> {
                    stamps
                        .iter()
                        .map(|stamp| Stamp { center: through(stamp.center), ..*stamp })
                        .filter(|stamp| stamp.center.is_finite())
                        .collect()
                };
                let put: StrokeStamps = stamps
                    .iter()
                    .map(|(id, (first, second))| (*id, (carried(first), second.as_deref().map(carried))))
                    .collect();
                let mut content = (*warp.from).clone();
                content.bend_fills(&through);
                self.vector_stamps = Some(put);
                self.draw_vector(warp.target, &content, None);
                self.vector_stamps = None;
            }
            None => {
                let mut bent = (*warp.from).clone();
                Carried::Bent(mesh.clone()).carry(&mut bent, warp.ids.as_deref());
                self.draw_vector(warp.target, &bent, None);
            }
        }
        self.transform = transform;
        self.vector_transform = of;
        warp.shown = Some((mesh, exactly));
        warp.drawn = Some((std::time::Instant::now(), started.elapsed()));
        self.vector_warp = Some(warp);
        false
    }

    /// The warp shown ([`VectorWarp`]) ends: the layer's pixels back as they were before it. Its
    /// layers, to take their picture up again if the transform goes on without a mesh; none if no
    /// warp was shown.
    pub(crate) fn vector_warp_end(&mut self) -> Option<Vec<LayerId>> {
        let warp = self.vector_warp.take()?;
        if let Some(canvas) = self.canvas.as_mut() {
            // (their picture pulled through the mesh for now: that is what there is to put back)
            match warp.pulled {
                Some(_) => canvas.end_transform(false),
                None => canvas.release_layer(false, None),
            }
        }
        Some(warp.moved)
    }

    /// How many kinds of thing vector layer `layer` holds (lines, lassos, fills: see
    /// [`ObjectKind`]): Separate Objects has something to do with two or more.
    pub(crate) fn vector_kinds(&self, layer: LayerId) -> usize {
        let content = self.doc.layer(layer).and_then(|l| l.vector.as_ref());
        content.map_or(0, |c| {
            ObjectKind::ALL.iter().filter(|k| c.objects().iter().any(|o| ObjectKind::of(o) == Some(**k))).count()
        })
    }

    /// Separate Objects: vector layer `layer`'s lines, lassos and fills each given a vector layer
    /// of its own, as one undo step. The layer keeps the first kind it has (its lines, if it has
    /// any); each other kind goes to a new layer just under it, named after it ("Layer 3 Fills"),
    /// lassos over fills: lines over colour, as colouring is stacked. Each layer has the erasers'
    /// marks that were made on what it holds. What showed is the same but where a lasso's shape
    /// lay over a line, which is now over it, and where an eraser took only part of one kind
    /// lying over another (each is erased alone, so the lower shows through the upper). False if
    /// the layer holds one kind only (or is no vector layer, or locked), or the canvas has no room
    /// for the layers.
    pub(crate) fn vector_separate(&mut self, layer: LayerId) -> bool {
        self.finish_stroke();
        self.settle();
        if self.doc.is_locked(layer) || self.scene_shown() || self.canvas.is_none() || self.gpu.is_none() {
            return false;
        }
        let Some(was) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return false };
        let name = self.doc.layer(layer).map(|l| l.name.clone()).unwrap_or_default();
        let mut parts = ObjectKind::ALL.into_iter().zip(was.separated()).filter_map(|(kind, part)| Some((kind, part?)));
        let Some((_, stays)) = parts.next() else { return false };
        let moved: Vec<(ObjectKind, VectorContent)> = parts.collect();
        if moved.is_empty() || !self.doc.has_room(moved.len()) {
            return false;
        }
        // (what is selected on a vector layer is strokes and fills of it: some are leaving it)
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
        }
        self.let_go_of_selection();
        let before = self.doc.clone();
        // each under the one before it
        let mut made = Vec::new();
        let mut above = layer;
        for (kind, part) in moved {
            let Some(new) = self.doc.add_vector_layer_below(above, format!("{name} {}", kind.name()), part.clone())
            else {
                break;
            };
            made.push((new, part));
            above = new;
        }
        self.doc.set_vector(layer, stays);
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.name_next_step("Separate Objects");
            canvas.record_document(before);
        }
        for (new, part) in &made {
            self.draw_vector(*new, part, None);
        }
        self.redraw_vector_changed(layer, &was);
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.collapse_steps();
        }
        self.revision += 1;
        true
    }

    /// The selection let go of, as one undo step, if there is one: before something is drawn on a
    /// vector layer, where what is selected is strokes and fills picked to be moved or cleared, not
    /// a place to paint in.
    pub(crate) fn let_go_of_selection(&mut self) {
        if self.selection.is_some() {
            let before = self.selection.take();
            self.feather_base = None;
            self.record_selection(before, "clear selection");
        }
    }

    /// The eraser, set to take whole strokes or up to their crossings, comes down on vector layer
    /// `layer` at `points` (canvas pixels), `radius` half its width: what it touches goes as it
    /// goes, the whole drag one undo step. Whether anything went yet.
    pub(crate) fn vector_erase_begin(&mut self, layer: LayerId, points: Vec<Vec2>, radius: f32) -> bool {
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
        }
        self.vector_erasing = Some(VectorErasing { layer, radius, last: None, before: None });
        self.vector_erase_along(points)
    }

    /// The eraser at work goes on through `points` (canvas pixels): what it touches on the way
    /// there goes, and the layer is drawn again where that was. Whether anything went.
    fn vector_erase_along(&mut self, points: Vec<Vec2>) -> bool {
        let Some(erasing) = self.vector_erasing.as_mut() else { return false };
        let (layer, radius) = (erasing.layer, erasing.radius);
        // every place on the way, close enough together that nothing it passes over is missed
        let step = (radius * 0.5).max(1.0);
        let mut path = Vec::new();
        for to in points {
            match erasing.last {
                Some(from) => {
                    let n = (from.distance(to) / step).ceil().clamp(1.0, 4096.0);
                    path.extend((1..=n as usize).map(|i| from.lerp(to, i as f32 / n)));
                }
                None => path.push(to),
            }
            erasing.last = Some(to);
        }
        let mode = self.vector_eraser;
        let Some(was) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return false };
        let before = self.doc.clone();
        let took = self.doc.change_vector(layer, |content| match mode {
            VectorEraser::WholeStroke => content.erase_whole(&path, radius),
            VectorEraser::ToCrossing => content.erase_to_crossings(&path, radius),
            VectorEraser::Subtract => false,
        });
        if took != Some(true) {
            return false;
        }
        // the first thing it takes: the document from before it is what undo goes back to
        if let Some(erasing) = self.vector_erasing.as_mut().filter(|e| e.before.is_none()) {
            erasing.before = Some(before);
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.name_next_step("Erase stroke");
            }
        }
        self.redraw_vector_changed(layer, &was);
        self.revision += 1;
        true
    }

    /// More of where the eraser at work taking strokes goes (`samples`, window pixels): none if
    /// there is no such eraser at work (something else takes them), else whether anything went.
    pub(crate) fn vector_erase_on(&mut self, samples: &[crate::PenSample]) -> Option<bool> {
        self.vector_erasing.as_ref()?;
        let points: Vec<Vec2> = self.to_canvas(samples).iter().filter(|s| s.is_finite()).map(|s| s.pos).collect();
        Some(self.vector_erase_along(points))
    }

    /// The eraser lifts: all it took is one undo step. Whether it took anything.
    pub(crate) fn vector_erase_end(&mut self) -> bool {
        let Some(erasing) = self.vector_erasing.take() else { return false };
        let Some(before) = erasing.before else { return false };
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.record_document(before);
            canvas.collapse_steps();
        }
        self.revision += 1;
        true
    }

    /// The eraser's drag is called off: what it took is put back, with no step left of it.
    pub(crate) fn vector_erase_cancel(&mut self) {
        if !self.vector_erase_end() {
            return;
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.undo();
        }
        self.flush();
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.forget_redo();
        }
    }

    /// Vector layer `layer` cleared: its strokes gone with its pixels, as one undo step. False if
    /// it isn't one.
    pub(crate) fn clear_vector_layer(&mut self, layer: LayerId) -> bool {
        let before = self.doc.clone();
        match self.doc.clear_strokes(layer) {
            None => return false,
            Some(false) => return true,
            Some(true) => {}
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
            canvas.name_next_step("Clear layer");
            canvas.clear_layer(layer);
            canvas.record_document(before);
            canvas.collapse_steps();
        }
        self.revision += 1;
        true
    }
}

/// What of the paint brush, as it is set now, the strokes picked on a vector layer are given.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum VectorApply {
    /// The colour (fills take it too).
    Colour,
    /// The brush and its opacity, each stroke keeping its own size.
    Brush,
    /// The size.
    Size,
}

/// A stroke on a vector layer being pulled into another shape by one of its nodes.
pub(crate) struct VectorReshape {
    layer: LayerId,
    /// The stroke as it was when it was taken hold of.
    stroke: Arc<VectorStroke>,
    /// Its nodes then (canvas pixels), and which of them is held.
    nodes: Vec<Vec2>,
    index: usize,
    /// How far along the stroke's path the held node is, and the nodes either side of it (its own
    /// place where it is an end).
    along: f32,
    from: f32,
    to: f32,
    /// How far it has been carried (canvas pixels).
    by: Vec2,
    /// The document as it was; whether anything was drawn since.
    before: Document,
    drawn: bool,
}

impl State {
    /// The strokes and fills picked on the current layer, if it is a vector layer that can be
    /// changed and any are.
    fn vector_picked(&self) -> Option<(LayerId, Vec<StrokeId>)> {
        let layer = self.doc.current();
        if self.scene_shown() || self.doc.is_locked(layer) || self.doc.unseen(layer).is_some() {
            return None;
        }
        let ids = self.vector_selected(layer).filter(|ids| !ids.is_empty())?;
        Some((layer, ids))
    }

    /// A change to the strokes and fills named in `ids` on vector layer `layer`, as
    /// [`State::vector_changed_step`] makes one called `name`; the selection, which is the shape
    /// they take up, follows them in the same step.
    fn vector_picked_step(
        &mut self,
        layer: LayerId,
        ids: &[StrokeId],
        name: &str,
        change: impl FnOnce(&mut VectorContent) -> bool,
    ) -> bool {
        let Some(was) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return false };
        self.vector_step(layer, name, change, |st| {
            st.redraw_vector_changed(layer, &was);
            if st.selection.is_some() {
                let before = st.selection.clone();
                st.selection = st.vector_footprint(layer, ids);
                st.record_selection(before, name);
            }
        })
    }

    /// A colour dropped at `at` (canvas pixels) on vector layer `layer` where there is something
    /// to take it as its own: on what is picked there, all of that takes it; on a stroke's ink,
    /// that stroke does (as a line does a colour dropped on it on any other layer). False if it
    /// landed on neither, and is to fill there.
    pub(crate) fn vector_recolour_at(&mut self, layer: LayerId, at: Vec2, colour: [f32; 3]) -> bool {
        let picked = self
            .vector_selected(layer)
            .filter(|ids| !ids.is_empty() && self.selection.as_ref().is_some_and(|mask| selected_at(mask, at)));
        let under = || {
            let content = self.doc.layer(layer)?.vector.as_ref()?;
            let id = content.object_at(at, 0.0)?;
            content.strokes().any(|s| s.id == id).then(|| vec![id])
        };
        let Some(ids) = picked.or_else(under) else { return false };
        self.vector_changed_step(layer, "Recolor", |content| content.recolour(&ids, colour));
        true
    }

    /// The strokes picked on the current vector layer given the paint brush's colour, its brush
    /// (and opacity) or its size, as `what` says, as one undo step: each is the same stroke drawn
    /// again with it. Whether anything changed.
    pub(crate) fn vector_apply(&mut self, what: VectorApply) -> bool {
        let Some((layer, ids)) = self.vector_picked() else { return false };
        if what == VectorApply::Colour {
            let colour = self.color;
            return self.vector_changed_step(layer, "Recolor", |content| content.recolour(&ids, colour));
        }
        let tool = crate::ToolKind::Paint;
        let (brush, style, diameter) = self.stroke_brush(tool, layer);
        let second = self.second_brush(tool, layer, &brush, &style);
        let diameter = diameter * self.brush_scale();
        if !(diameter.is_finite() && diameter > 0.0) {
            return false;
        }
        let (brush, scale) = (Arc::new(brush), self.brush_scale());
        let second = second.map(|(b, drawn, d)| (Arc::new(b), drawn.style.opacity, d * scale));
        let Some(content) = self.doc.layer(layer).and_then(|l| l.vector.clone()) else { return false };
        let mut changed = Vec::new();
        for stroke in content.strokes().filter(|s| ids.contains(&s.id) && !s.ink.erases) {
            let was = &stroke.setup;
            let now = match what {
                VectorApply::Brush => {
                    // (a dual brush's second keeps its size beside the first's)
                    let by = was.diameter / diameter;
                    let second = second.as_ref().map(|(b, opacity, d)| Second {
                        setup: Setup {
                            brush: Arc::clone(b),
                            diameter: d * by,
                            seed: was.seed ^ 0x5eed_0002,
                            ..was.clone()
                        },
                        opacity: *opacity,
                    });
                    let ink = Ink { opacity: style.opacity, wetness: style.wetness, ..stroke.ink };
                    stroke.restyled(Setup { brush: Arc::clone(&brush), ..was.clone() }, ink, second)
                }
                _ => {
                    if (was.diameter - diameter).abs() < 1e-3 {
                        continue;
                    }
                    let by = diameter / was.diameter.max(1e-6);
                    let second = stroke.second.as_ref().map(|s| Second {
                        setup: Setup { diameter: s.setup.diameter * by, ..s.setup.clone() },
                        opacity: s.opacity,
                    });
                    stroke.restyled(Setup { diameter, ..was.clone() }, stroke.ink, second)
                }
            };
            changed.push(now);
        }
        if changed.is_empty() {
            return false;
        }
        let name = if what == VectorApply::Brush { "Apply Brush" } else { "Apply Size" };
        self.vector_picked_step(layer, &ids, name, |content| {
            changed.into_iter().filter(|stroke| content.replace_stroke(stroke.clone())).count() > 0
        })
    }

    /// The one stroke picked on the current vector layer, which its nodes show on: none unless
    /// just one stroke is picked, and nothing else.
    fn vector_node_stroke(&self) -> Option<(LayerId, Arc<VectorStroke>)> {
        let (layer, ids) = self.vector_picked()?;
        let [id] = ids.as_slice() else { return None };
        let content = self.doc.layer(layer)?.vector.as_ref()?;
        content.strokes().find(|s| s.id == *id).map(|s| (layer, Arc::clone(s)))
    }

    /// The places `stroke` is taken hold of by, as the view shows it now: its ends, where it
    /// bends, and along its long stretches, far enough apart on the screen to tell from each other.
    fn node_spots(&self, stroke: &VectorStroke) -> Vec<PathSpot> {
        // (canvas pixels a point of the screen)
        let point = self.display_scale / self.view.map_or(1.0, |v| v.zoom()).max(1e-6);
        stroke.nodes(1.5 * point, 28.0 * point, 90.0 * point)
    }

    /// The nodes of the one stroke picked on the current vector layer (window pixels, x and y in
    /// turn), by which it is pulled into another shape; with one of them being dragged, as they
    /// are meanwhile. None unless just one stroke is picked.
    pub(crate) fn vector_nodes(&self) -> Vec<f32> {
        let Some(to_screen) = self.view.map(|v| v.to_screen()) else { return Vec::new() };
        let nodes: Vec<Vec2> = match self.vector_reshape.as_ref() {
            Some(reshape) => reshape
                .nodes
                .iter()
                .enumerate()
                .map(|(i, p)| if i == reshape.index { *p + reshape.by } else { *p })
                .collect(),
            None => match self.vector_node_stroke() {
                Some((_, stroke)) => self.node_spots(&stroke).iter().map(|spot| spot.pos).collect(),
                None => return Vec::new(),
            },
        };
        nodes.into_iter().map(|p| to_screen.apply(p)).flat_map(|p| [p.x, p.y]).collect()
    }

    /// Node `index` of the stroke picked on the current vector layer is taken hold of, to be
    /// dragged ([`State::vector_node_drag`]) until it is let go ([`State::vector_node_release`]).
    /// False if there is no such node.
    pub(crate) fn vector_node_grab(&mut self, index: usize) -> bool {
        self.finish_stroke();
        self.settle();
        let Some((layer, stroke)) = self.vector_node_stroke() else { return false };
        let spots = self.node_spots(&stroke);
        let Some(held) = spots.get(index) else { return false };
        let from = index.checked_sub(1).and_then(|i| spots.get(i)).map_or(held.distance, |spot| spot.distance);
        let to = spots.get(index + 1).map_or(held.distance, |spot| spot.distance);
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
        }
        self.vector_reshape = Some(VectorReshape {
            layer,
            stroke,
            nodes: spots.iter().map(|spot| spot.pos).collect(),
            index,
            along: held.distance,
            from,
            to,
            by: Vec2::ZERO,
            before: self.doc.clone(),
            drawn: false,
        });
        true
    }

    /// The node held goes to window point (`x`, `y`): the stroke is drawn again through there, the
    /// stretch of it up to the nodes either side going along the less the further from the one
    /// held, and the rest of it staying. False if none is held.
    pub(crate) fn vector_node_drag(&mut self, x: f32, y: f32) -> bool {
        let to = self.view.and_then(|v| v.screen_to_canvas(Vec2::new(x, y))).filter(|p| p.is_finite());
        // (set aside while it draws, so nothing that settles the canvas on the way ends it)
        let Some(mut reshape) = self.vector_reshape.take() else { return false };
        let layer = reshape.layer;
        if let (Some(to), Some(node)) = (to, reshape.nodes.get(reshape.index))
            && let Some(was) = self.doc.layer(layer).and_then(|l| l.vector.clone())
        {
            reshape.by = to - *node;
            let now = reshape.stroke.reshaped(reshape.along, reshape.from, reshape.to, reshape.by);
            if self.doc.change_vector(layer, |content| content.replace_stroke(now)) == Some(true) {
                self.redraw_vector_changed(layer, &was);
                reshape.drawn = true;
                self.revision += 1;
            }
        }
        self.vector_reshape = Some(reshape);
        true
    }

    /// The node held is let go: with `keep`, the stroke stays as it was pulled, one undo step, and
    /// the selection takes its new shape; otherwise it is put back as it was, with no step. False
    /// if none was held.
    pub(crate) fn vector_node_release(&mut self, keep: bool) -> bool {
        let Some(reshape) = self.vector_reshape.take() else { return false };
        if !reshape.drawn {
            // nothing was drawn for it: there is only its mark to use up
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.collapse_steps();
            }
            return true;
        }
        if keep && self.selection.is_some() {
            let before = self.selection.clone();
            self.selection = self.vector_footprint(reshape.layer, &[reshape.stroke.id]);
            self.record_selection(before, "Reshape");
        }
        if let Some(canvas) = self.canvas.as_mut() {
            if keep {
                canvas.name_next_step("Reshape");
            }
            canvas.record_document(reshape.before);
            canvas.collapse_steps();
            if !keep {
                canvas.undo();
            }
        }
        if !keep {
            self.flush();
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.forget_redo();
            }
        }
        self.revision += 1;
        true
    }
}

impl CanvasSession {
    /// Lasso Fill on the current layer if it is a vector layer: the shape through window points
    /// `window` is kept there as a fill (or an eraser's shape, as `lasso` says). False if the
    /// layer is no vector layer (the shape is then filled as on any other); true if it is, whether
    /// or not it took the shape (a locked layer takes none, nor one with a selection to cut it).
    pub(crate) fn lasso_on_vector(&self, window: &[Vec2], lasso: LassoFill) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let current = st.doc.current();
        if !st.doc.is_vector(current) || st.scene_shown() {
            return false;
        }
        st.finish_stroke();
        st.settle();
        if st.doc.is_locked(current) {
            return true;
        }
        // (a shape drawn here is kept whole: what was selected is let go of first)
        st.let_go_of_selection();
        let hard = st.lasso_pixelated(lasso);
        let Some(points) = st.to_canvas_points(window) else { return true };
        let points = if hard { procraft_engine::pixel::stepped(&points, true) } else { points };
        let opacity = if lasso.tool_opacity { st.sliders(st.tool).opacity } else { st.fill_opacity };
        if st.vector_lasso(current, points, hard, lasso.erase, opacity) {
            self.shared.frame(st);
        }
        true
    }
}

#[uniffi::export]
impl CanvasSession {
    /// A new empty vector layer above the current one, made current: strokes drawn on it are kept
    /// as strokes. 0 when the canvas has no room for another.
    pub fn add_vector_layer(&self) -> u64 {
        self.change("new layer", |d| {
            if !d.has_room(1) {
                return None;
            }
            Some(d.add_vector_layer().0)
        })
        .unwrap_or(0)
    }

    /// Separate Objects: vector layer `id`'s lines, lassos (shapes drawn by hand) and fills
    /// (traced from lines) each given a vector layer of its own, as one undo step: the layer keeps
    /// its lines, and its lassos and its fills go to new layers just under it, lassos over fills,
    /// named after it. False if it holds one kind only, is no vector layer or is locked, or the
    /// canvas has no room for the layers.
    pub fn separate_vector_objects(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let done = st.vector_separate(LayerId(id));
        if done {
            self.shared.frame(st);
        }
        done
    }

    /// Whether layer `id` is a vector layer.
    pub fn is_vector_layer(&self, id: u64) -> bool {
        self.shared.lock().doc.is_vector(LayerId(id))
    }

    /// How many strokes vector layer `id` keeps (0 for any other layer).
    pub fn vector_stroke_count(&self, id: u64) -> u32 {
        let st = self.shared.lock();
        let strokes = st.doc.layer(LayerId(id)).and_then(|l| l.vector.as_ref()).map_or(0, |c| c.strokes().count());
        u32::try_from(strokes).unwrap_or(u32::MAX)
    }

    /// How many fills vector layer `id` keeps (0 for any other layer).
    pub fn vector_fill_count(&self, id: u64) -> u32 {
        let st = self.shared.lock();
        let fills = st.doc.layer(LayerId(id)).and_then(|l| l.vector.as_ref()).map_or(0, |c| c.fills().count());
        u32::try_from(fills).unwrap_or(u32::MAX)
    }

    /// How the eraser works on vector layers.
    pub fn vector_eraser(&self) -> VectorEraser {
        self.shared.lock().vector_eraser
    }

    pub fn set_vector_eraser(&self, mode: VectorEraser) {
        self.shared.lock().vector_eraser = mode;
    }

    /// How many strokes and fills are picked on the current layer (0 unless it is a vector layer
    /// with a selection).
    pub fn vector_picked_count(&self) -> u32 {
        let st = self.shared.lock();
        st.vector_picked().map_or(0, |(_, ids)| u32::try_from(ids.len()).unwrap_or(u32::MAX))
    }

    /// Gives what is picked on the current vector layer the paint brush's colour, its brush or its
    /// size as they are set now (see [`VectorApply`]), as one undo step. Whether anything changed.
    pub fn vector_apply(&self, what: VectorApply) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let changed = st.vector_apply(what);
        if changed {
            self.shared.frame(st);
        }
        changed
    }

    /// The nodes of the one stroke picked on the current vector layer (window pixels, x and y in
    /// turn): where it can be taken hold of and pulled into another shape. None unless just one
    /// stroke is picked.
    pub fn vector_nodes(&self) -> Vec<f32> {
        self.shared.lock().vector_nodes()
    }

    /// Takes hold of node `index` of [`CanvasSession::vector_nodes`], to be dragged with
    /// [`CanvasSession::vector_node_drag`] until [`CanvasSession::vector_node_release`]. False if
    /// there is no such node.
    pub fn vector_node_grab(&self, index: u32) -> bool {
        self.shared.lock().vector_node_grab(index as usize)
    }

    /// The node held goes to window point (`x`, `y`), and the stroke is drawn again through there.
    /// False if none is held.
    pub fn vector_node_drag(&self, x: f32, y: f32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let dragged = st.vector_node_drag(x, y);
        if dragged {
            self.shared.frame(st);
        }
        dragged
    }

    /// Lets go of the node held: `keep` makes the stroke's new shape one undo step, otherwise it is
    /// put back as it was. False if none was held.
    pub fn vector_node_release(&self, keep: bool) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let held = st.vector_node_release(keep);
        if held {
            self.shared.frame(st);
        }
        held
    }

    /// Whether vector layers are drawn again as fine as the screen once the view rests zoomed in
    /// (on at first), instead of showing as their pixels magnified.
    pub fn vector_crisp(&self) -> bool {
        self.shared.lock().vector_crisp
    }

    pub fn set_vector_crisp(&self, on: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.vector_crisp != on {
            st.vector_crisp = on;
            st.crisp_refused = None;
            self.shared.frame(st);
        }
    }
}

/// What the tests reach that the apps don't.
#[cfg(test)]
impl CanvasSession {
    /// Makes vector layer `id`'s pixels again from its strokes, as one undo step; false if it
    /// isn't one.
    pub fn redraw_vector_layer(&self, id: u64) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let redrawn = st.redraw_vector_layer(LayerId(id));
        if redrawn {
            self.shared.frame(st);
        }
        redrawn
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use procraft_engine::document::LayerId;
    use procraft_engine::geom::Vec2;
    use procraft_engine::view::View;
    use procraft_gpu::{Gpu, wgpu};

    use super::{LIFTED_NAME, VectorApply, VectorEraser};
    use crate::selection::LassoFill;
    use crate::touch::{SelectKind, SelectTouch};
    use crate::{CanvasSession, GuideType, PenSample, PointerKind, SelectionJoin, ToolKind};

    /// A session 300 × 300 shown as it is, a pixel a point, with a GPU to draw on; none where
    /// there is no GPU (the test is skipped).
    /// Where brush `origin` of ours from before the stock library (its set and place as they
    /// shipped) is in the library now.
    fn ours(s: &CanvasSession, origin: (u32, u32)) -> (u32, u32) {
        let (set, i) =
            crate::brush_library::place_of(&s.shared.lock().library, (origin.0 as usize, origin.1 as usize));
        (set as u32, i as u32)
    }

    /// Chooses brush `origin` of ours from before the stock library (see [`ours`]).
    fn select_ours(s: &CanvasSession, origin: (u32, u32)) -> bool {
        let (set, i) = ours(s, origin);
        s.select_brush(set, i)
    }

    fn session() -> Option<Arc<CanvasSession>> {
        let gpu = match Gpu::headless() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped: {e}");
                return None;
            }
        };
        let s = CanvasSession::new(300, 300);
        {
            let mut st = s.shared.lock();
            st.window_size = (300, 300);
            st.view = Some(View::fit((300, 300), (300, 300)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        Some(s)
    }

    /// A stroke that bends from left to right about row `y`, pressed harder and lighter as it goes
    /// and leaning more, handed over a few samples at a time.
    fn stroke(s: &CanvasSession, y: f32, turn: f32) {
        let at = |i: usize| {
            let t = i as f32 / 60.0;
            PenSample {
                x: 30.0 + 240.0 * t,
                y: y + 25.0 * (t * 7.0 + turn).sin(),
                pressure: 0.2 + 0.75 * (t * 4.0 + turn).sin().abs(),
                tilt: 0.3 + 0.6 * t,
                orientation: 0.5 + turn + t,
                time_ms: f64::from(i as u32) * 7.0,
                roll: 0.0,
            }
        };
        s.stroke_begin(PointerKind::Stylus, vec![at(0)]);
        for chunk in (1..60).collect::<Vec<usize>>().chunks(4) {
            s.stroke_add(chunk.iter().map(|&i| at(i)).collect());
        }
        s.stroke_end(vec![at(60)]);
    }

    #[test]
    fn a_canvas_in_display_p3_shares_its_shapes_in_srgb_and_takes_svg_colours_into_p3() {
        use procraft_engine::profile::Profile;
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        let colour = [0.2, 0.5, 0.8];
        s.set_color(colour[0], colour[1], colour[2]);
        s.set_brush_size(0.4);
        stroke(&s, 150.0, 0.0);
        let hex = |c: [f32; 3]| {
            let [r, g, b] = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            format!("#{r:02x}{g:02x}{b:02x}")
        };
        // (the stroke's colour as the layer keeps it)
        let kept = {
            let st = s.shared.lock();
            st.doc.layer(LayerId(layer)).unwrap().vector.as_ref().unwrap().strokes().next().unwrap().ink.colour
        };
        // an sRGB canvas: as it is
        let svg = s.vector_svg().unwrap();
        assert!(svg.contains(&hex(kept)), "{svg}");
        // the same in Display P3: brought to sRGB, which SVG and PDF are read as
        s.shared.lock().doc.profile = Profile::DisplayP3;
        let srgb = Profile::DisplayP3.convert(kept, Profile::Srgb);
        assert_ne!(hex(srgb), hex(kept));
        let svg = s.vector_svg().unwrap();
        let near = |text: &str, c: [f32; 3]| {
            // (within a level of 255, for how the two conversions round)
            (-1i32..=1).any(|d| {
                let shifted = c.map(|v| v + d as f32 / 255.0);
                text.contains(&hex(shifted))
                    || (0..3).any(|i| {
                        let mut one = c;
                        one[i] += d as f32 / 255.0;
                        text.contains(&hex(one))
                    })
            })
        };
        assert!(near(&svg, srgb) && !svg.contains(&hex(kept)), "{svg}");
        // an SVG put into it: its sRGB colours as the canvas has them
        let blue = r##"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect x="10" y="10" width="50" height="50" fill="#3366cc"/></svg>"##;
        let added = s.insert_svg(blue.as_bytes().to_vec(), "In".into());
        assert!(added != 0);
        let fill = {
            let st = s.shared.lock();
            st.doc.layer(LayerId(added)).unwrap().vector.as_ref().unwrap().fills().next().unwrap().colour
        };
        let want = Profile::Srgb.convert([0.2, 0.4, 0.8], Profile::DisplayP3);
        assert!(fill.iter().zip(want).all(|(a, b)| (a - b).abs() < 0.01), "{fill:?} {want:?}");
    }

    #[test]
    fn textured_wet_dual_and_mirrored_strokes_are_drawn_again_just_as_they_were() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        // a grain fixed to the paper (Graphite), a wet wash, and a dual brush: the second of set 1
        // made the first's second
        let (pen, joining) = (ours(&s, (1, 0)), ours(&s, (1, 2)));
        assert_eq!(pen.0, joining.0);
        assert!(s.combine_brushes(pen.0, pen.1, joining.1));
        let brushes = [(0, 0, 0.5), (4, 0, 0.35), (1, 0, 0.3)];
        for (i, (set, brush, size)) in brushes.into_iter().enumerate() {
            assert!(select_ours(&s, (set, brush)), "no brush {set}.{brush}");
            s.set_color(0.8 - i as f32 * 0.3, 0.2 + i as f32 * 0.25, 0.4);
            s.set_brush_size(size);
            stroke(&s, 40.0 + i as f32 * 45.0, i as f32);
        }
        // and the textured brush again, mirrored
        assert!(select_ours(&s, (0, 0)));
        s.set_guide_kind(GuideType::Symmetry);
        s.set_guide_on(true);
        s.set_guide_assisted(true);
        stroke(&s, 200.0, 3.0);
        s.set_guide_on(false);
        assert_eq!(s.vector_stroke_count(layer), 4);
        let live = pixels(&s, layer);
        assert!(painted(&live) > 4000, "{}", painted(&live));
        // made again from the strokes alone: the same picture (a pixel where strokes cross may
        // round the other way, by a level of 255)
        assert!(s.redraw_vector_layer(layer));
        let again = pixels(&s, layer);
        assert!(furthest(&again, &live) <= 1, "{}", furthest(&again, &live));
    }

    fn pixels(s: &CanvasSession, layer: u64) -> Vec<u8> {
        s.shared.lock().layer_pixels(LayerId(layer)).unwrap()
    }

    fn painted(rgba: &[u8]) -> usize {
        rgba.as_chunks::<4>().0.iter().filter(|px| px[3] != 0).count()
    }

    /// The most any pixel's colour or coverage differs between two pictures, of 255.
    fn furthest(a: &[u8], b: &[u8]) -> u8 {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
    }

    #[test]
    fn a_vector_layer_s_pixels_are_what_its_strokes_draw() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        assert!(layer != 0 && s.is_vector_layer(layer));
        // strokes of several brushes, one over another, and the eraser through them
        let brushes = [(1, 0), (0, 0), (1, 2), (2, 1), (1, 5)];
        for (i, (set, brush)) in brushes.into_iter().enumerate() {
            assert!(select_ours(&s, (set, brush)), "no brush {set}.{brush}");
            s.set_color(0.9 - i as f32 * 0.2, 0.1 + i as f32 * 0.2, 0.5);
            s.set_brush_size(0.25 + i as f32 * 0.05);
            stroke(&s, 70.0 + i as f32 * 35.0, i as f32);
        }
        s.set_tool(ToolKind::Erase);
        s.stroke_begin(
            PointerKind::Stylus,
            vec![PenSample { x: 150.0, y: 20.0, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: 0.0, roll: 0.0 }],
        );
        s.stroke_add(
            (1..30)
                .map(|i| PenSample {
                    x: 150.0 + i as f32,
                    y: 20.0 + i as f32 * 9.0,
                    pressure: 0.8,
                    tilt: 0.0,
                    orientation: 0.0,
                    time_ms: f64::from(i) * 8.0,
                    roll: 0.0,
                })
                .collect(),
        );
        s.stroke_end(Vec::new());
        assert_eq!(s.vector_stroke_count(layer), 6);
        let live = pixels(&s, layer);
        assert!(painted(&live) > 2000, "{}", painted(&live));
        // made again from the strokes alone: the same picture
        assert!(s.redraw_vector_layer(layer));
        let again = pixels(&s, layer);
        assert_eq!(painted(&again), painted(&live));
        assert_eq!(furthest(&again, &live), 0, "the strokes drawn again differ from the strokes as drawn");
        // redrawing is a step of its own, which changes nothing either way
        s.undo();
        assert_eq!(furthest(&pixels(&s, layer), &live), 0);
        assert_eq!(s.vector_stroke_count(layer), 6);
    }

    #[test]
    fn undo_takes_a_stroke_from_the_layer_with_its_pixels() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        stroke(&s, 100.0, 0.0);
        let one = pixels(&s, layer);
        stroke(&s, 200.0, 1.0);
        let two = pixels(&s, layer);
        assert_eq!(s.vector_stroke_count(layer), 2);
        assert!(painted(&two) > painted(&one));
        // one undo: the second stroke gone from the pixels and from what is kept
        assert_eq!(s.undo().as_deref(), Some("Paint stroke"));
        assert_eq!(s.vector_stroke_count(layer), 1);
        assert_eq!(furthest(&pixels(&s, layer), &one), 0);
        // redo: both back
        assert_eq!(s.redo().as_deref(), Some("Paint stroke"));
        assert_eq!(s.vector_stroke_count(layer), 2);
        assert_eq!(furthest(&pixels(&s, layer), &two), 0);
        // undone again, what is kept is what shows
        s.undo();
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &one), 0);
        assert_eq!(s.undo().as_deref(), Some("Redraw layer"));
        s.undo();
        assert_eq!(s.vector_stroke_count(layer), 0);
        assert_eq!(painted(&pixels(&s, layer)), 0);
        // the layer itself is the step before
        assert!(s.is_vector_layer(layer));
        s.undo();
        assert!(!s.is_vector_layer(layer));
    }

    #[test]
    fn a_stroke_called_off_is_not_kept() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        s.stroke_begin(
            PointerKind::Stylus,
            vec![PenSample { x: 40.0, y: 40.0, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: 0.0, roll: 0.0 }],
        );
        s.stroke_add(vec![PenSample {
            x: 90.0,
            y: 60.0,
            pressure: 1.0,
            tilt: 0.0,
            orientation: 0.0,
            time_ms: 8.0,
            roll: 0.0,
        }]);
        s.stroke_cancel();
        assert_eq!(s.vector_stroke_count(layer), 0);
        assert_eq!(painted(&pixels(&s, layer)), 0);
        // and the next one is kept as itself alone
        stroke(&s, 150.0, 0.0);
        assert_eq!(s.vector_stroke_count(layer), 1);
        let live = pixels(&s, layer);
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &live), 0);
    }

    #[test]
    fn symmetry_s_copies_are_drawn_again_with_their_stroke() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        s.set_guide_kind(GuideType::Symmetry);
        s.set_guide_on(true);
        s.set_guide_assisted(true);
        stroke(&s, 90.0, 0.5);
        let live = pixels(&s, layer);
        // both halves were drawn
        let half = |rgba: &[u8], right: bool| {
            rgba.as_chunks::<4>()
                .0
                .iter()
                .enumerate()
                .filter(|(i, px)| px[3] != 0 && ((i % 300) >= 150) == right)
                .count()
        };
        assert!(half(&live, false) > 200 && half(&live, true) > 200, "{} {}", half(&live, false), half(&live, true));
        assert_eq!(s.vector_stroke_count(layer), 1, "one stroke, its copies with it");
        // the guide changed since: the stroke keeps the one it was drawn with
        s.set_guide_on(false);
        assert!(s.redraw_vector_layer(layer));
        // (where a stroke crosses its own copies, the two were laid a few stamps of each in turn
        // as it was drawn and are laid one whole after the other now: a pixel there may round the
        // other way, by one level of 255)
        assert!(furthest(&pixels(&s, layer), &live) <= 1);
    }

    #[test]
    fn what_changes_pixels_alone_leaves_a_vector_layer_until_it_is_rasterized() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        stroke(&s, 150.0, 0.0);
        let live = pixels(&s, layer);
        // a smudge, a fill of the whole layer: neither taken
        s.set_tool(ToolKind::Smudge);
        stroke(&s, 150.0, 0.0);
        s.set_tool(ToolKind::Paint);
        assert!(!s.fill_layer(layer));
        assert_eq!(s.vector_stroke_count(layer), 1);
        assert_eq!(furthest(&pixels(&s, layer), &live), 0);
        // a copy of the layer keeps the strokes too
        let copy = s.duplicate_layer(layer).unwrap();
        assert!(s.is_vector_layer(copy) && s.vector_stroke_count(copy) == 1);
        assert!(s.redraw_vector_layer(copy));
        assert_eq!(furthest(&pixels(&s, copy), &live), 0);
        // cleared, its strokes go with its pixels, and come back with them
        s.clear_layer();
        assert_eq!((s.vector_stroke_count(copy), painted(&pixels(&s, copy))), (0, 0));
        assert_eq!(s.undo().as_deref(), Some("Clear layer"));
        assert_eq!(s.vector_stroke_count(copy), 1);
        assert_eq!(furthest(&pixels(&s, copy), &live), 0);
        // rasterized, it is pixels like any other, and takes a fill
        assert!(s.rasterize_layer(copy));
        assert!(!s.is_vector_layer(copy) && s.vector_stroke_count(copy) == 0);
        assert_eq!(furthest(&pixels(&s, copy), &live), 0);
        assert!(s.fill_layer(copy));
        // the first is a vector layer still
        assert!(s.is_vector_layer(layer));
    }

    /// A straight stroke between two points, pressed evenly.
    fn straight(s: &CanvasSession, from: (f32, f32), to: (f32, f32)) {
        let at = |i: u32| {
            let t = i as f32 / 30.0;
            PenSample {
                x: from.0 + (to.0 - from.0) * t,
                y: from.1 + (to.1 - from.1) * t,
                pressure: 0.8,
                tilt: 0.0,
                orientation: 0.0,
                time_ms: f64::from(i) * 8.0,
                roll: 0.0,
            }
        };
        s.stroke_begin(PointerKind::Stylus, vec![at(0)]);
        s.stroke_add((1..30).map(at).collect());
        s.stroke_end(vec![at(30)]);
    }

    /// A box drawn as four strokes that cross at its corners, from 80 to 220, in black.
    fn boxed(s: &CanvasSession) {
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        straight(s, (70.0, 80.0), (230.0, 80.0));
        straight(s, (220.0, 70.0), (220.0, 230.0));
        straight(s, (230.0, 220.0), (70.0, 220.0));
        straight(s, (80.0, 230.0), (80.0, 70.0));
    }

    fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let at = (y * 300 + x) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
    }

    #[test]
    fn the_bucket_fills_between_lines_with_a_shape_laid_under_them() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        boxed(&s);
        let lines = pixels(&s, layer);
        let on_a_line = pixel(&lines, 150, 80);
        assert!(on_a_line[3] > 200, "{on_a_line:?}");
        // red poured inside the box: a fill kept, the box's inside red, its outside clear
        assert!(s.pour_drop(150.0, 150.0, 1.0, 0.0, 0.0, 0.5));
        assert_eq!((s.vector_fill_count(layer), s.vector_stroke_count(layer)), (1, 4));
        let filled = pixels(&s, layer);
        assert_eq!(pixel(&filled, 150, 150), [255, 0, 0, 255]);
        assert_eq!(pixel(&filled, 100, 200), [255, 0, 0, 255]);
        assert_eq!(pixel(&filled, 40, 150)[3], 0, "outside the box");
        assert_eq!(pixel(&filled, 150, 250)[3], 0, "outside the box");
        // the lines are over it, as they were
        assert_eq!(pixel(&filled, 150, 80), on_a_line);
        // drawn again from what the layer holds: the same picture
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &filled), 0);
        s.undo();
        // poured again in another colour, the new fill takes the old one's place
        assert!(s.pour_drop(160.0, 140.0, 0.0, 0.0, 1.0, 0.5));
        assert_eq!(s.vector_fill_count(layer), 1);
        let refilled = pixels(&s, layer);
        assert_eq!(pixel(&refilled, 150, 150), [0, 0, 255, 255]);
        assert_eq!(pixel(&refilled, 150, 80), on_a_line);
        // one undo each: the red back, then no fill at all
        assert_eq!(s.undo().as_deref(), Some("ColorDrop"));
        assert_eq!(furthest(&pixels(&s, layer), &filled), 0);
        s.undo();
        assert_eq!(s.vector_fill_count(layer), 0);
        assert_eq!(furthest(&pixels(&s, layer), &lines), 0);
        // outside the box, the fill is the rest of the canvas and leaves the box's inside alone
        assert!(s.pour_drop(20.0, 20.0, 0.0, 1.0, 0.0, 0.5));
        let around = pixels(&s, layer);
        assert_eq!(pixel(&around, 20, 280), [0, 255, 0, 255]);
        assert_eq!(pixel(&around, 150, 150)[3], 0);
    }

    #[test]
    fn with_all_layers_a_fill_stops_at_the_lines_of_every_vector_layer_that_shows() {
        let Some(s) = session() else { return };
        // the lines on one vector layer, the colour poured on another
        let lines = s.add_vector_layer();
        boxed(&s);
        let colours = s.add_vector_layer();
        // on its own layer, which has no lines, the fill is the whole canvas
        assert!(s.pour_drop(150.0, 150.0, 1.0, 0.0, 0.0, 0.5));
        assert_eq!(pixel(&pixels(&s, colours), 40, 150), [255, 0, 0, 255]);
        s.undo();
        assert_eq!(s.vector_fill_count(colours), 0);
        // with All Layers it stops at the other layer's lines: the box's inside, on this layer
        s.set_pour_all_layers(true);
        assert!(s.pour_drop(150.0, 150.0, 1.0, 0.0, 0.0, 0.5));
        assert_eq!((s.vector_fill_count(colours), s.vector_fill_count(lines)), (1, 0));
        let filled = pixels(&s, colours);
        assert_eq!(pixel(&filled, 150, 150), [255, 0, 0, 255]);
        assert_eq!(pixel(&filled, 40, 150)[3], 0, "outside the box");
        assert_eq!(s.undo().as_deref(), Some("ColorDrop"));
        // lines that don't show are no lines
        assert!(s.set_layer_visible(lines, false));
        assert!(s.pour_drop(150.0, 150.0, 1.0, 0.0, 0.0, 0.5));
        assert_eq!(pixel(&pixels(&s, colours), 40, 150), [255, 0, 0, 255]);
    }

    #[test]
    fn a_fill_inside_another_leaves_the_rest_of_it_and_lies_over_it() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        // a box, and green poured round it: the rest of the canvas
        boxed(&s);
        assert!(s.pour_drop(20.0, 20.0, 0.0, 1.0, 0.0, 0.5));
        assert_eq!(pixel(&pixels(&s, layer), 20, 280), [0, 255, 0, 255]);
        // a small box drawn on the green, and red poured inside that: the green stays everywhere
        // else, and the red shows in the small box, over it and under the lines
        s.set_color(0.0, 0.0, 0.0);
        straight(&s, (8.0, 10.0), (52.0, 10.0));
        straight(&s, (50.0, 8.0), (50.0, 52.0));
        straight(&s, (52.0, 50.0), (8.0, 50.0));
        straight(&s, (10.0, 52.0), (10.0, 8.0));
        let lined = pixels(&s, layer);
        assert!(s.pour_drop(30.0, 30.0, 1.0, 0.0, 0.0, 0.5));
        let both = pixels(&s, layer);
        assert_eq!(s.vector_fill_count(layer), 2);
        assert_eq!(pixel(&both, 30, 30), [255, 0, 0, 255]);
        assert_eq!(pixel(&both, 20, 280), [0, 255, 0, 255], "the green still there");
        assert_eq!(pixel(&both, 280, 20), [0, 255, 0, 255]);
        assert_eq!(pixel(&both, 30, 10), pixel(&lined, 30, 10), "the line over both");
        assert_eq!(pixel(&both, 150, 150)[3], 0, "the big box still empty");
        // drawn again from what the layer holds: the same picture
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &both), 0);
        s.undo();
        // one step, which leaves the green as it was
        assert_eq!(s.undo().as_deref(), Some("ColorDrop"));
        assert_eq!(furthest(&pixels(&s, layer), &lined), 0);
        // poured again in the small box, in blue: that takes the red's place, and the green stays
        assert!(s.pour_drop(30.0, 30.0, 1.0, 0.0, 0.0, 0.5));
        assert!(s.pour_drop(32.0, 28.0, 0.0, 0.0, 1.0, 0.5));
        let again = pixels(&s, layer);
        assert_eq!(s.vector_fill_count(layer), 2);
        assert_eq!(pixel(&again, 30, 30), [0, 0, 255, 255]);
        assert_eq!(pixel(&again, 20, 280), [0, 255, 0, 255]);
        // the eraser inside the small box (through the blue and the green under it), then poured
        // there once more: the new fill is whole, made after the eraser's mark, which stays on
        // what it was made on
        s.set_tool(ToolKind::Erase);
        s.set_brush_size(0.05);
        straight(&s, (27.0, 30.0), (33.0, 30.0));
        s.set_tool(ToolKind::Paint);
        let erased = pixels(&s, layer);
        assert!(pixel(&erased, 30, 30)[3] < 255, "erased through both");
        assert_eq!(pixel(&erased, 30, 10), pixel(&lined, 30, 10), "and none of the lines");
        assert_eq!(pixel(&erased, 50, 30), pixel(&lined, 50, 30));
        assert!(s.pour_drop(30.0, 42.0, 1.0, 1.0, 0.0, 0.5));
        let whole = pixels(&s, layer);
        assert_eq!(s.vector_fill_count(layer), 2);
        assert_eq!(pixel(&whole, 30, 30), [255, 255, 0, 255], "filled where the eraser went");
        assert_eq!(pixel(&whole, 30, 42), [255, 255, 0, 255]);
        assert_eq!(pixel(&whole, 20, 280), [0, 255, 0, 255]);
        {
            let st = s.shared.lock();
            let content = st.doc.layer(LayerId(layer)).unwrap().vector.clone().unwrap();
            let fills: Vec<_> = content.fills().map(|f| f.id).collect();
            let eraser = content.strokes().find(|k| k.ink.erases).unwrap();
            let takes = eraser.takes_from.clone().expect("it names what it took from");
            // (the green, made before it; not the yellow, made after)
            assert!(takes.iter().filter(|id| fills.contains(id)).count() == 1, "{takes:?} {fills:?}");
        }
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &whole), 0);
    }

    #[test]
    fn a_continuous_drag_fills_inside_and_outside_a_box_as_one_step() {
        use crate::touch::Fill;
        let sample = |x: f32, y: f32, time_ms: f64| PenSample {
            x,
            y,
            pressure: 0.8,
            tilt: 0.0,
            orientation: 0.0,
            time_ms,
            roll: 0.0,
        };
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        boxed(&s);
        let lines = pixels(&s, layer);
        s.set_touch_filling(Some(Fill { r: 1.0, g: 0.0, b: 0.0, threshold: 0.5, continuous: true }));
        // from inside the box out over its line: the inside and the outside each a fill
        s.touch_down(1, PointerKind::Finger, vec![sample(150.0, 150.0, 1000.0)]);
        assert_eq!(s.vector_fill_count(layer), 1);
        s.touch_move(1, vec![sample(150.0, 40.0, 1030.0)]);
        s.touch_move(1, vec![sample(150.0, 20.0, 1060.0)]);
        s.touch_up(1, vec![sample(150.0, 20.0, 1090.0)]);
        assert_eq!(s.vector_fill_count(layer), 2);
        let filled = pixels(&s, layer);
        assert_eq!(pixel(&filled, 150, 150), [255, 0, 0, 255]);
        assert_eq!(pixel(&filled, 20, 280), [255, 0, 0, 255]);
        assert_eq!(pixel(&filled, 150, 80), pixel(&lines, 150, 80), "the line over them");
        // dragged over again, nothing more is filled
        s.touch_down(1, PointerKind::Finger, vec![sample(150.0, 150.0, 2000.0)]);
        s.touch_move(1, vec![sample(150.0, 20.0, 2060.0)]);
        s.touch_up(1, vec![sample(150.0, 20.0, 2090.0)]);
        assert_eq!(s.vector_fill_count(layer), 2);
        // one undo takes the drag back
        assert_eq!(s.undo().as_deref(), Some("ColorDrop"));
        assert_eq!(s.vector_fill_count(layer), 0);
        assert_eq!(furthest(&pixels(&s, layer), &lines), 0);
        s.set_touch_filling(None);
    }

    #[test]
    fn a_small_gap_in_the_lines_holds_the_fill_and_an_erased_one_lets_it_out() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        // the box's right side stops short of its bottom by a few pixels
        straight(&s, (70.0, 80.0), (230.0, 80.0));
        straight(&s, (220.0, 70.0), (220.0, 217.0));
        straight(&s, (217.0, 220.0), (70.0, 220.0));
        straight(&s, (80.0, 230.0), (80.0, 70.0));
        let lines = pixels(&s, layer);
        // (read as it is, without ending the drop as other reads of a layer would)
        let raw = || {
            let mut st = s.shared.lock();
            st.flush();
            let (canvas, gpu) = (st.canvas.as_ref().unwrap(), st.gpu.as_ref().unwrap());
            canvas.read_layer(gpu, LayerId(layer)).unwrap()
        };
        // dropped and held: the fill shows, stopped at the gap
        assert!(s.pour_begin(150.0, 150.0, 1.0, 0.0, 0.0, 0.5));
        let held = raw();
        assert_eq!(pixel(&held, 150, 150), [255, 0, 0, 255]);
        assert_eq!(pixel(&held, 260, 150)[3], 0, "the fill came out through the gap");
        // its threshold slid all the way up, no opening is a gap: it goes through
        s.pour_threshold(1.0);
        assert_eq!(pixel(&raw(), 260, 150), [255, 0, 0, 255]);
        assert_eq!(s.vector_fill_count(layer), 1);
        // slid back it is held again, and takes another colour
        s.pour_threshold(0.5);
        s.pour_colour(0.0, 0.0, 1.0);
        let back = raw();
        assert_eq!(pixel(&back, 150, 150), [0, 0, 255, 255]);
        assert_eq!(pixel(&back, 260, 150)[3], 0);
        // let go, all of it is one step
        s.pour_end(true, None);
        assert_eq!(furthest(&pixels(&s, layer), &back), 0);
        assert_eq!(s.undo().as_deref(), Some("ColorDrop"));
        assert_eq!(s.vector_fill_count(layer), 0);
        assert_eq!(furthest(&pixels(&s, layer), &lines), 0);
        // called off, nothing is left of it
        assert!(s.pour_begin(150.0, 150.0, 1.0, 0.0, 0.0, 0.5));
        s.pour_threshold(1.0);
        s.pour_end(false, None);
        assert_eq!(s.vector_fill_count(layer), 0);
        assert_eq!(furthest(&pixels(&s, layer), &lines), 0);
        assert_ne!(s.undo().as_deref(), Some("ColorDrop"));
        s.redo();
        // the eraser through a side, wide: that side is no wall there any more
        s.set_tool(ToolKind::Erase);
        s.set_brush_size(0.6);
        straight(&s, (60.0, 150.0), (100.0, 150.0));
        s.set_tool(ToolKind::Paint);
        assert!(s.pour_drop(150.0, 120.0, 1.0, 0.0, 0.0, 0.5));
        assert_eq!(pixel(&pixels(&s, layer), 20, 150), [255, 0, 0, 255], "the fill stopped at a line that was erased");
    }

    #[test]
    fn lasso_fill_and_the_eraser_s_lasso_are_kept_as_shapes() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        let lasso = |erase: bool| LassoFill { erase, tool_opacity: false };
        let square = |x0: f32, y0: f32, x1: f32, y1: f32| vec![x0, y0, x1, y0, x1, y1, x0, y1];
        // the eraser's lasso over nothing is kept as nothing
        s.fill_polygon(square(50.0, 50.0, 100.0, 100.0), lasso(true));
        assert_eq!(s.vector_fill_count(layer), 0);
        // a stroke, and a shape filled over part of it
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.2);
        straight(&s, (40.0, 150.0), (260.0, 150.0));
        s.set_color(0.0, 0.0, 1.0);
        s.fill_polygon(square(100.0, 100.0, 200.0, 200.0), lasso(false));
        assert_eq!(s.vector_fill_count(layer), 1);
        let filled = pixels(&s, layer);
        assert_eq!(pixel(&filled, 150, 150), [0, 0, 255, 255], "over the stroke, as paint");
        assert_eq!(pixel(&filled, 150, 120), [0, 0, 255, 255]);
        assert!(pixel(&filled, 60, 150)[3] > 200 && pixel(&filled, 60, 150)[2] < 50, "the stroke beside it");
        // the eraser's lasso through both
        s.fill_polygon(square(140.0, 90.0, 160.0, 210.0), lasso(true));
        assert_eq!(s.vector_fill_count(layer), 2);
        let erased = pixels(&s, layer);
        assert_eq!(pixel(&erased, 150, 150)[3], 0);
        assert_eq!(pixel(&erased, 120, 150), [0, 0, 255, 255]);
        // an ellipse too
        s.set_color(1.0, 0.0, 0.0);
        s.fill_shape(true, 20.0, 220.0, 120.0, 280.0, lasso(false));
        assert_eq!(s.vector_fill_count(layer), 3);
        let all = pixels(&s, layer);
        assert_eq!(pixel(&all, 70, 250), [255, 0, 0, 255]);
        // drawn again from what the layer holds: the same picture
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &all), 0);
        s.undo();
        // each is a step of its own
        assert_eq!(s.undo().as_deref(), Some("lasso fill"));
        assert_eq!(furthest(&pixels(&s, layer), &erased), 0);
        assert_eq!(s.undo().as_deref(), Some("lasso erase"));
        assert_eq!(s.vector_fill_count(layer), 1);
        assert_eq!(furthest(&pixels(&s, layer), &filled), 0);
        // a stroke drawn after the eraser's shape isn't erased by it
        s.redo();
        s.set_color(0.0, 0.0, 0.0);
        straight(&s, (40.0, 150.0), (260.0, 150.0));
        assert!(pixel(&pixels(&s, layer), 150, 150)[3] > 200);
        assert!(s.redraw_vector_layer(layer));
        assert!(pixel(&pixels(&s, layer), 150, 150)[3] > 200);
    }

    #[test]
    fn the_eraser_takes_whole_strokes_or_up_to_crossings_when_set_to() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        straight(&s, (40.0, 150.0), (260.0, 150.0));
        straight(&s, (100.0, 60.0), (100.0, 240.0));
        straight(&s, (200.0, 60.0), (200.0, 240.0));
        let drawn = pixels(&s, layer);
        // an eraser stroke over nothing is kept as nothing
        s.set_tool(ToolKind::Erase);
        s.set_brush_size(0.05);
        straight(&s, (275.0, 280.0), (290.0, 290.0));
        assert_eq!(s.vector_stroke_count(layer), 3);
        // up to the crossings: the middle of the line across goes, its ends stay as they were
        s.set_vector_eraser(VectorEraser::ToCrossing);
        assert_eq!(s.vector_eraser(), VectorEraser::ToCrossing);
        straight(&s, (150.0, 140.0), (150.0, 160.0));
        assert_eq!(s.vector_stroke_count(layer), 4, "the line across is two pieces now");
        let cut = pixels(&s, layer);
        assert_eq!(pixel(&cut, 150, 150)[3], 0);
        assert_eq!(pixel(&cut, 60, 150), pixel(&drawn, 60, 150));
        assert_eq!(pixel(&cut, 240, 150), pixel(&drawn, 240, 150));
        assert_eq!(pixel(&cut, 100, 100), pixel(&drawn, 100, 100));
        // one undo puts it back whole
        assert_eq!(s.undo().as_deref(), Some("Erase stroke"));
        assert_eq!(s.vector_stroke_count(layer), 3);
        assert_eq!(furthest(&pixels(&s, layer), &drawn), 0);
        // whole strokes: the one touched goes, all of it
        s.set_vector_eraser(VectorEraser::WholeStroke);
        straight(&s, (90.0, 200.0), (110.0, 205.0));
        assert_eq!(s.vector_stroke_count(layer), 2);
        let less = pixels(&s, layer);
        assert_eq!(pixel(&less, 100, 100)[3], 0);
        assert_eq!(pixel(&less, 200, 100), pixel(&drawn, 200, 100));
        // touching nothing, it changes nothing and makes no step
        straight(&s, (20.0, 20.0), (60.0, 30.0));
        assert_eq!(s.undo().as_deref(), Some("Erase stroke"));
        assert_eq!(furthest(&pixels(&s, layer), &drawn), 0);
        // set back, it takes away what its brush covers and is kept as a stroke
        s.set_vector_eraser(VectorEraser::Subtract);
        straight(&s, (150.0, 140.0), (150.0, 160.0));
        assert_eq!(s.vector_stroke_count(layer), 4);
        // (a thin eraser, pressed lightly: it takes some of the line, as it would of any paint)
        let erased = pixels(&s, layer);
        assert!(pixel(&erased, 150, 150)[3] < pixel(&drawn, 150, 150)[3]);
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &erased), 0);
    }

    /// Whether the selection takes canvas pixel (`x`, `y`).
    fn selected(s: &CanvasSession, x: usize, y: usize) -> bool {
        s.shared.lock().selection.as_ref().is_some_and(|m| m.data[y * 300 + x] >= 128)
    }

    /// Where the `n`-th stroke kept on `layer` lies now: its line's first and last points.
    fn ends(s: &CanvasSession, layer: u64, n: usize) -> ((f32, f32), (f32, f32)) {
        let st = s.shared.lock();
        let content = st.doc.layer(LayerId(layer)).unwrap().vector.clone().unwrap();
        let line = content.strokes().nth(n).unwrap().line();
        let (a, b) = (line[0], line[line.len() - 1]);
        ((a.x, a.y), (b.x, b.y))
    }

    /// Two lines across, at rows 100 and 200, and a red square filled between them on the left.
    fn two_lines_and_a_square(s: &CanvasSession) -> u64 {
        let layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        straight(s, (40.0, 100.0), (260.0, 100.0));
        straight(s, (40.0, 200.0), (260.0, 200.0));
        s.set_color(1.0, 0.0, 0.0);
        s.fill_polygon(
            vec![50.0, 130.0, 110.0, 130.0, 110.0, 170.0, 50.0, 170.0],
            LassoFill { erase: false, tool_opacity: false },
        );
        layer
    }

    #[test]
    fn the_selection_tools_take_whole_strokes_and_fills() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        // a rectangle over the left end of the first line: the whole line is taken, nothing else
        s.select_shape(false, 30.0, 80.0, 70.0, 120.0, SelectionJoin::Add);
        assert!(s.has_selection());
        assert!(selected(&s, 50, 100) && selected(&s, 250, 100), "all of the line it touched");
        assert!(!selected(&s, 150, 200) && !selected(&s, 80, 150) && !selected(&s, 150, 60));
        assert_eq!(s.shared.lock().vector_selected(LayerId(layer)).map(|ids| ids.len()), Some(1));
        // a tap on the square adds it; a tap on nothing adds nothing
        s.select_by_colour(80.0, 150.0, 0.5, SelectionJoin::Add, false);
        assert!(selected(&s, 80, 150) && selected(&s, 250, 100));
        s.select_by_colour(200.0, 150.0, 0.5, SelectionJoin::Add, false);
        assert_eq!(s.shared.lock().vector_selected(LayerId(layer)).map(|ids| ids.len()), Some(2));
        // a tap on the line with Remove lets it go
        s.select_by_colour(200.0, 101.0, 0.5, SelectionJoin::Remove, false);
        assert!(!selected(&s, 250, 100) && selected(&s, 80, 150));
        // undo and redo go through each
        s.undo();
        assert!(selected(&s, 250, 100));
        s.redo();
        assert!(!selected(&s, 250, 100));
        // inverted: the two lines are what is selected
        s.select_invert();
        assert_eq!(s.shared.lock().vector_selected(LayerId(layer)).map(|ids| ids.len()), Some(2));
        // a shape that touches nothing selects nothing
        s.select_none();
        s.select_shape(true, 130.0, 20.0, 180.0, 60.0, SelectionJoin::Add);
        assert!(!s.has_selection());
        // cleared with something selected, those go, whole, and one undo brings them back
        s.select_polygon(vec![200.0, 180.0, 240.0, 180.0, 240.0, 220.0, 200.0, 220.0], SelectionJoin::Add);
        let before = pixels(&s, layer);
        s.clear_layer();
        assert_eq!((s.vector_stroke_count(layer), s.vector_fill_count(layer)), (1, 1));
        let cleared = pixels(&s, layer);
        assert_eq!(pixel(&cleared, 150, 200)[3], 0);
        assert_eq!(pixel(&cleared, 150, 100), pixel(&before, 150, 100), "the line not selected still shows");
        assert_eq!(pixel(&cleared, 80, 150), pixel(&before, 80, 150));
        assert_eq!(s.undo().as_deref(), Some("Clear"));
        assert_eq!(s.vector_stroke_count(layer), 2);
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
    }

    #[test]
    fn a_transform_carries_the_selected_strokes_and_leaves_the_rest() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        let before = pixels(&s, layer);
        let (first, second) = (ends(&s, layer, 0), ends(&s, layer, 1));
        // the first line selected and moved 40 down, 10 along
        s.select_shape(false, 30.0, 80.0, 70.0, 120.0, SelectionJoin::Add);
        assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
        s.transform_grab(crate::TransformHandle::Move, 150.0, 100.0);
        s.transform_drag(160.0, 140.0);
        s.transform_release();
        s.transform_end(true);
        assert!(!s.has_selection(), "a kept transform lets go of what it took");
        let moved = ends(&s, layer, 0);
        assert!((moved.0.0 - first.0.0 - 10.0).abs() < 0.5 && (moved.0.1 - first.0.1 - 40.0).abs() < 0.5, "{moved:?}");
        assert_eq!(ends(&s, layer, 1), second, "the other line where it was");
        let after = pixels(&s, layer);
        assert_eq!(pixel(&after, 150, 100)[3], 0, "gone from where it was");
        assert_eq!(pixel(&after, 160, 140), pixel(&before, 150, 100), "the same ink where it is now");
        assert_eq!(pixel(&after, 150, 200), pixel(&before, 150, 200));
        assert_eq!(pixel(&after, 80, 160), pixel(&before, 80, 160), "the square where it was");
        assert_eq!((s.vector_stroke_count(layer), s.vector_fill_count(layer)), (2, 1));
        // what shows is what the layer holds
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &after), 0);
        s.undo();
        // undone (the selection let go, then the move), it is all as it was
        s.undo();
        assert_eq!(s.undo().as_deref(), Some("non-uniform transform"));
        assert_eq!(ends(&s, layer, 0), first);
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        // with nothing selected the whole layer goes: twice the size about its box
        s.select_none();
        let size = s.transform_begin().unwrap();
        s.transform_set_size(size.width * 2.0, size.height * 2.0);
        s.transform_end(true);
        let (wide, low) = (ends(&s, layer, 0), ends(&s, layer, 1));
        assert!(((wide.1.0 - wide.0.0) - (first.1.0 - first.0.0) * 2.0).abs() < 1.5, "{wide:?}");
        assert!(((low.0.1 - wide.0.1) - (second.0.1 - first.0.1) * 2.0).abs() < 1.5, "{wide:?} {low:?}");
        let bigger = pixels(&s, layer);
        // (four times the ink, less what now lies off the canvas)
        assert!(painted(&bigger) > painted(&before) * 2, "{} of {}", painted(&bigger), painted(&before));
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &bigger), 0);
        s.undo();
        s.undo();
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        // a transform called off changes nothing, warped or not
        assert!(s.transform_begin().is_some());
        s.transform_grab(crate::TransformHandle::Move, 150.0, 100.0);
        s.transform_drag(175.0, 100.0);
        s.transform_release();
        s.transform_set_kind(crate::TransformKind::Warp);
        s.transform_end(false);
        assert_eq!(ends(&s, layer, 0), first);
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        // turned a quarter: the line across now runs down
        s.select_shape(false, 30.0, 80.0, 70.0, 120.0, SelectionJoin::Add);
        assert!(s.transform_begin().is_some());
        s.transform_set_angle(90.0);
        s.transform_end(true);
        let turned = ends(&s, layer, 0);
        assert!((turned.0.0 - turned.1.0).abs() < 1.5 && (turned.1.1 - turned.0.1).abs() > 200.0, "{turned:?}");
    }

    /// A folder of its own for a test's artwork, empty.
    fn folder(name: &str) -> String {
        let dir = std::env::temp_dir().join(format!("procraft-vector-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.to_string_lossy().into_owned()
    }

    /// The artwork saved in `dir` opened, with a GPU to draw on as [`session`] gives one.
    fn opened(dir: &str) -> Arc<CanvasSession> {
        let s = CanvasSession::open(dir.to_owned()).unwrap();
        let mut st = s.shared.lock();
        st.window_size = (300, 300);
        st.view = Some(View::fit((300, 300), (300, 300)));
        st.gpu = Some(Gpu::headless().unwrap());
        st.ensure_canvas();
        drop(st);
        s
    }

    #[test]
    fn a_vector_layer_is_saved_with_its_artwork_and_opens_as_it_was() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        // strokes of several brushes, one with symmetry's copies
        for (i, (set, brush)) in [(1, 0), (0, 0), (2, 1), (1, 5)].into_iter().enumerate() {
            assert!(select_ours(&s, (set, brush)));
            s.set_color(0.8 - i as f32 * 0.2, 0.1 + i as f32 * 0.2, 0.4);
            s.set_brush_size(0.2 + i as f32 * 0.05);
            if i == 3 {
                s.set_guide_kind(GuideType::Symmetry);
                s.set_guide_on(true);
                s.set_guide_assisted(true);
            }
            stroke(&s, 70.0 + i as f32 * 45.0, i as f32);
        }
        s.set_guide_on(false);
        s.set_guide_assisted(false);
        // a box to fill, the fill, a lasso's shape, the eraser through it, and its lasso
        assert!(select_ours(&s, (1, 0)));
        boxed(&s);
        assert!(s.pour_drop(150.0, 150.0, 0.2, 0.6, 1.0, 0.5));
        s.set_color(1.0, 0.8, 0.0);
        let lasso = |erase: bool| LassoFill { erase, tool_opacity: false };
        s.fill_polygon(vec![20.0, 240.0, 90.0, 240.0, 90.0, 290.0, 20.0, 290.0], lasso(false));
        s.set_tool(ToolKind::Erase);
        stroke(&s, 160.0, 2.5);
        s.fill_polygon(vec![40.0, 250.0, 60.0, 250.0, 60.0, 280.0, 40.0, 280.0], lasso(true));
        s.set_tool(ToolKind::Paint);
        // a line cut at its crossings, and the first stroke moved and made bigger
        s.set_vector_eraser(VectorEraser::ToCrossing);
        s.set_tool(ToolKind::Erase);
        s.set_brush_size(0.05);
        straight(&s, (150.0, 75.0), (150.0, 85.0));
        s.set_tool(ToolKind::Paint);
        s.set_vector_eraser(VectorEraser::Subtract);
        s.select_by_colour(30.0, 70.0, 0.5, SelectionJoin::Add, false);
        if s.has_selection() && s.transform_begin().is_some() {
            s.transform_grab(crate::TransformHandle::Move, 150.0, 70.0);
            s.transform_drag(155.0, 60.0);
            s.transform_release();
            s.transform_end(true);
        }
        s.select_none();
        let (strokes, fills) = (s.vector_stroke_count(layer), s.vector_fill_count(layer));
        assert!(strokes >= 9 && fills == 3, "{strokes} strokes, {fills} fills");
        let shown = pixels(&s, layer);
        let kept = s.shared.lock().doc.layer(LayerId(layer)).unwrap().vector.clone().unwrap();
        // an ordinary layer beside it, to see that it stays one
        let plain = s.add_layer();
        stroke(&s, 150.0, 0.0);

        let dir = folder("saved");
        s.save(dir.clone()).unwrap();
        assert!(std::path::Path::new(&dir).join("vectors").join(format!("{layer}.json")).exists());
        assert!(!std::path::Path::new(&dir).join("vectors").join(format!("{plain}.json")).exists());
        let again = opened(&dir);
        assert!(again.is_vector_layer(layer) && !again.is_vector_layer(plain));
        assert_eq!((again.vector_stroke_count(layer), again.vector_fill_count(layer)), (strokes, fills));
        // it shows as it did, from the pixels saved with it
        assert_eq!(furthest(&pixels(&again, layer), &shown), 0);
        // what it holds is what it held: every stroke the same stamps, every fill the same shape
        let read = again.shared.lock().doc.layer(LayerId(layer)).unwrap().vector.clone().unwrap();
        assert_eq!(read.len(), kept.len());
        assert_eq!(read.next_id(), kept.next_id());
        for (now, was) in read.strokes().zip(kept.strokes()) {
            assert_eq!(now.id, was.id);
            assert!(now.replay().placed_stamps() == was.replay().placed_stamps(), "stroke {:?}", was.id);
            assert!(now.placed_second() == was.placed_second() && now.shown == was.shown && now.ink == was.ink);
            assert_eq!(now.guide.is_some(), was.guide.is_some());
            assert_eq!(now.takes_from, was.takes_from);
        }
        assert!(read.fills().zip(kept.fills()).all(|(now, was)| now == was));
        // pieces cut from one stroke still share its input
        let pieces: Vec<_> = read.strokes().filter(|s| s.shown.is_some()).collect();
        assert!(pieces.len() == 2 && Arc::ptr_eq(pieces[0].samples_shared(), pieces[1].samples_shared()));
        // drawn again from what was read, it is the picture that was saved
        assert!(again.redraw_vector_layer(layer));
        assert!(furthest(&pixels(&again, layer), &shown) <= 1, "{}", furthest(&pixels(&again, layer), &shown));
        again.undo();
        // it goes on as a vector layer: a stroke more, saved again and opened again
        again.select_layer(layer);
        stroke(&again, 40.0, 1.0);
        assert_eq!(again.vector_stroke_count(layer), strokes + 1);
        again.save(dir.clone()).unwrap();
        assert_eq!(opened(&dir).vector_stroke_count(layer), strokes + 1);
        // rasterized, its strokes' file goes with the next save, and it opens as pixels
        assert!(again.rasterize_layer(layer));
        again.save(dir.clone()).unwrap();
        assert!(!std::path::Path::new(&dir).join("vectors").join(format!("{layer}.json")).exists());
        let plainly = opened(&dir);
        assert!(!plainly.is_vector_layer(layer));
        assert!(painted(&pixels(&plainly, layer)) > 2000);
        // a file that is no such file leaves the layer its pixels
        std::fs::create_dir_all(std::path::Path::new(&dir).join("vectors")).unwrap();
        std::fs::write(std::path::Path::new(&dir).join("vectors").join(format!("{layer}.json")), b"{\"version\":99}")
            .unwrap();
        assert!(!opened(&dir).is_vector_layer(layer));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_eraser_s_marks_go_with_the_strokes_they_were_made_on() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        let lasso = |erase: bool| LassoFill { erase, tool_opacity: false };
        // a yellow square, made first, where a line will be carried to
        s.set_color(1.0, 0.8, 0.0);
        s.fill_polygon(vec![120.0, 180.0, 180.0, 180.0, 180.0, 220.0, 120.0, 220.0], lasso(false));
        // two lines close together, and the eraser down through both
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        straight(&s, (40.0, 100.0), (260.0, 100.0));
        straight(&s, (40.0, 130.0), (260.0, 130.0));
        let whole = pixels(&s, layer);
        s.set_tool(ToolKind::Erase);
        s.set_brush_size(0.3);
        s.set_brush_opacity(1.0);
        straight(&s, (150.0, 85.0), (150.0, 145.0));
        s.set_tool(ToolKind::Paint);
        let erased = pixels(&s, layer);
        // (how much the eraser left of each line where it went through: less than was there)
        // (how much ink there is across where the eraser went, about row `y`: less than there was)
        let ink = |rgba: &[u8], y: usize| -> f32 {
            (y - 9..y + 10)
                .flat_map(|row| (135..166).map(move |x| (x, row)))
                .map(|(x, row)| f32::from(pixel(rgba, x, row)[3]))
                .sum()
        };
        let (left, stayed) = (ink(&erased, 100), ink(&erased, 130));
        assert!(
            left < ink(&whole, 100) * 0.8 && stayed < ink(&whole, 130) * 0.8,
            "{left} {stayed} of {}",
            ink(&whole, 100)
        );
        assert!(pixel(&erased, 60, 100)[3] > 200 && pixel(&erased, 60, 130)[3] > 200);
        // the upper line alone carried down onto the square
        s.select_by_colour(60.0, 100.0, 0.5, SelectionJoin::Add, false);
        assert_eq!(s.shared.lock().vector_selected(LayerId(layer)).map(|ids| ids.len()), Some(1));
        assert!(s.transform_begin().is_some());
        s.transform_grab(crate::TransformHandle::Move, 150.0, 100.0);
        s.transform_drag(150.0, 200.0);
        s.transform_release();
        s.transform_end(true);
        let moved = pixels(&s, layer);
        // it is where it was carried to, and gone from where it was
        assert_eq!(pixel(&moved, 60, 100)[3], 0);
        assert!(pixel(&moved, 60, 200)[3] > 200 && pixel(&moved, 240, 200)[3] > 200);
        let over_square = pixel(&moved, 130, 200);
        assert!(over_square[1] < 80 && over_square[3] == 255, "the line over the square: {over_square:?}");
        // the eraser's mark went with it: there the line is gone still, and the square shows
        // (the ink across the hole now, told from how much of the square's yellow shows through it:
        // as much as there was across it before, near enough: a grain the eraser has is the
        // canvas's, and lies otherwise where the mark is now)
        let through: f32 = (191..210)
            .flat_map(|row| (135..166).map(move |x| (x, row)))
            .map(|(x, row)| 255.0 * (1.0 - f32::from(pixel(&moved, x, row)[1]) / 204.0))
            .sum();
        assert!((through - left).abs() < ink(&whole, 100) * 0.12, "{through} across the hole now, {left} before");
        // and took nothing from the square, which it never touched
        assert_eq!(pixel(&moved, 150, 188), [255, 204, 0, 255]);
        assert_eq!(pixel(&moved, 150, 212), [255, 204, 0, 255]);
        // the line that stayed is erased through still, where it was
        assert!((ink(&moved, 130) - stayed).abs() < stayed * 0.02 + 40.0 && pixel(&moved, 60, 130)[3] > 200);
        // the eraser is two now: one gone with the line carried, naming it alone, and one that
        // stayed with the rest it reached
        let erasers = {
            let st = s.shared.lock();
            let content = st.doc.layer(LayerId(layer)).unwrap().vector.clone().unwrap();
            content.strokes().filter(|k| k.ink.erases).map(|k| k.takes_from.clone()).collect::<Vec<_>>()
        };
        assert_eq!(erasers.len(), 2);
        assert!(erasers.iter().all(Option::is_some), "{erasers:?}");
        assert_eq!(
            erasers.iter().filter(|names| names.as_ref().is_some_and(|n| n.len() == 1)).count(),
            1,
            "{erasers:?}"
        );
        // what shows is what the layer holds
        assert!(s.redraw_vector_layer(layer));
        assert!(furthest(&pixels(&s, layer), &moved) <= 1);
        s.undo();
        // a stroke drawn across the hole now isn't erased by it
        s.set_brush_size(0.1);
        straight(&s, (150.0, 190.0), (150.0, 210.0));
        assert!(pixel(&pixels(&s, layer), 150, 200)[1] < 80, "drawn after the eraser, over its mark");
        s.undo();
        // undone (the selection let go, then the move), it is all as it was
        s.undo();
        s.undo();
        assert_eq!(furthest(&pixels(&s, layer), &erased), 0);
    }

    #[test]
    fn the_eraser_s_lasso_goes_with_its_stroke_too() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        straight(&s, (40.0, 60.0), (260.0, 60.0));
        s.fill_polygon(
            vec![200.0, 50.0, 220.0, 50.0, 220.0, 70.0, 200.0, 70.0],
            LassoFill { erase: true, tool_opacity: false },
        );
        assert_eq!(pixel(&pixels(&s, layer), 210, 60)[3], 0);
        assert!(s.transform_begin().is_some());
        s.transform_grab(crate::TransformHandle::Move, 150.0, 60.0);
        s.transform_drag(150.0, 160.0);
        s.transform_release();
        s.transform_end(true);
        let moved = pixels(&s, layer);
        assert_eq!(pixel(&moved, 210, 160)[3], 0, "its hole with it");
        assert!(pixel(&moved, 100, 160)[3] > 200 && pixel(&moved, 240, 160)[3] > 200);
        assert_eq!(
            painted(&moved.chunks(300 * 4).take(120).flatten().copied().collect::<Vec<u8>>()),
            0,
            "nothing left above"
        );
        // a line drawn where it was is whole: the shape went with the other
        straight(&s, (40.0, 60.0), (260.0, 60.0));
        assert!(pixel(&pixels(&s, layer), 210, 60)[3] > 200);
    }

    #[test]
    fn drawing_on_a_vector_layer_lets_go_of_what_is_selected() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        let before = pixels(&s, layer);
        // the first line selected; a stroke drawn then is drawn whole, anywhere, the selection gone
        s.select_shape(false, 30.0, 80.0, 70.0, 120.0, SelectionJoin::Add);
        assert!(s.has_selection());
        s.set_color(0.0, 0.0, 0.0);
        straight(&s, (40.0, 250.0), (260.0, 250.0));
        assert!(!s.has_selection());
        assert_eq!(s.vector_stroke_count(layer), 3);
        let drawn = pixels(&s, layer);
        assert!(
            pixel(&drawn, 60, 250)[3] > 200 && pixel(&drawn, 240, 250)[3] > 200,
            "all of it, not kept to what was selected"
        );
        // drawn again from what the layer holds: the same
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &drawn), 0);
        s.undo();
        // one undo takes the stroke, the next brings the selection back
        assert_eq!(s.undo().as_deref(), Some("Paint stroke"));
        assert_eq!(s.vector_stroke_count(layer), 2);
        assert!(!s.has_selection());
        s.undo();
        assert!(s.has_selection() && selected(&s, 250, 100));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        // the bucket and the lasso let go of it too, and are kept whole
        assert!(s.pour_drop(150.0, 150.0, 0.0, 1.0, 0.0, 0.5));
        assert!(!s.has_selection());
        assert_eq!(s.vector_fill_count(layer), 2);
        s.select_by_colour(200.0, 200.0, 0.5, SelectionJoin::Add, false);
        assert!(s.has_selection());
        s.fill_polygon(
            vec![200.0, 20.0, 260.0, 20.0, 260.0, 60.0, 200.0, 60.0],
            LassoFill { erase: false, tool_opacity: false },
        );
        assert!(!s.has_selection());
        assert_eq!(s.vector_fill_count(layer), 3);
        assert_eq!(pixel(&pixels(&s, layer), 230, 40)[3], 255);
        // on any other layer a selection is still where painting keeps to
        let plain = s.add_layer();
        s.select_shape(false, 20.0, 20.0, 120.0, 280.0, SelectionJoin::Add);
        straight(&s, (40.0, 150.0), (260.0, 150.0));
        assert!(s.has_selection());
        let kept = pixels(&s, plain);
        assert!(pixel(&kept, 60, 150)[3] > 200 && pixel(&kept, 240, 150)[3] == 0);
    }

    #[test]
    fn a_shared_package_carries_its_vector_layers() {
        let Some(s) = session() else { return };
        let plain = s.add_layer();
        stroke(&s, 60.0, 0.3);
        let layer = s.add_vector_layer();
        for (i, (set, brush)) in [(1, 0), (0, 0), (2, 1)].into_iter().enumerate() {
            assert!(select_ours(&s, (set, brush)));
            s.set_color(0.7 - i as f32 * 0.2, 0.2 + i as f32 * 0.2, 0.5);
            stroke(&s, 90.0 + i as f32 * 50.0, i as f32);
        }
        assert!(select_ours(&s, (1, 0)));
        boxed(&s);
        assert!(s.pour_drop(150.0, 150.0, 0.2, 0.6, 1.0, 0.5));
        s.set_tool(ToolKind::Erase);
        stroke(&s, 150.0, 2.0);
        s.set_tool(ToolKind::Paint);
        s.fill_polygon(
            vec![20.0, 240.0, 90.0, 240.0, 90.0, 290.0, 20.0, 290.0],
            LassoFill { erase: false, tool_opacity: false },
        );
        let (strokes, fills) = (s.vector_stroke_count(layer), s.vector_fill_count(layer));
        assert!(strokes == 8 && fills == 2, "{strokes} strokes, {fills} fills");
        let shown = pixels(&s, layer);
        let kept = s.shared.lock().doc.layer(LayerId(layer)).unwrap().vector.clone().unwrap();
        let _ = plain;

        let file = std::env::temp_dir().join(format!("procraft-vector-shared-{}.procreate", std::process::id()));
        let path = file.to_string_lossy().into_owned();
        s.export_package(path.clone(), 7, None).unwrap();
        let again = CanvasSession::open_package(path.clone()).unwrap();
        {
            let mut st = again.shared.lock();
            st.window_size = (300, 300);
            st.view = Some(View::fit((300, 300), (300, 300)));
            st.gpu = Some(Gpu::headless().unwrap());
            st.ensure_canvas();
        }
        // one of its layers is a vector layer still, holding what the first held
        let vector: Vec<u64> = again.layers().iter().filter(|l| l.vector).map(|l| l.id).collect();
        assert_eq!(vector.len(), 1, "{:?}", again.layers().iter().map(|l| (&l.name, l.vector)).collect::<Vec<_>>());
        let opened = vector[0];
        assert_eq!((again.vector_stroke_count(opened), again.vector_fill_count(opened)), (strokes, fills));
        let read = again.shared.lock().doc.layer(LayerId(opened)).unwrap().vector.clone().unwrap();
        for (now, was) in read.strokes().zip(kept.strokes()) {
            assert!(now.replay().placed_stamps() == was.replay().placed_stamps(), "stroke {:?}", was.id);
        }
        assert!(read.fills().zip(kept.fills()).all(|(now, was)| now == was));
        // it shows as it did, and drawn again from what it holds it is the same picture
        let arrived = pixels(&again, opened);
        assert!(furthest(&arrived, &shown) <= 1, "{}", furthest(&arrived, &shown));
        assert!(again.redraw_vector_layer(opened));
        assert!(furthest(&pixels(&again, opened), &shown) <= 1);
        again.undo();
        // and it goes on being drawn on as one
        again.select_layer(opened);
        stroke(&again, 40.0, 1.0);
        assert_eq!(again.vector_stroke_count(opened), strokes + 1);
        // a package with no vector layer in it carries nothing of the kind
        assert!(s.rasterize_layer(layer));
        s.export_package(path.clone(), 7, None).unwrap();
        let flat = CanvasSession::open_package(path.clone()).unwrap();
        assert!(flat.layers().iter().all(|l| !l.vector));
        let _ = std::fs::remove_file(&file);
    }

    /// One sample at a point, pressed evenly.
    fn at(x: f32, y: f32, ms: f64) -> PenSample {
        PenSample { x, y, pressure: 0.8, tilt: 0.0, orientation: 0.0, time_ms: ms, roll: 0.0 }
    }

    #[test]
    fn the_eraser_takes_strokes_as_it_goes_and_the_whole_drag_is_one_step() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        straight(&s, (40.0, 80.0), (260.0, 80.0));
        straight(&s, (40.0, 150.0), (260.0, 150.0));
        straight(&s, (40.0, 220.0), (260.0, 220.0));
        let drawn = pixels(&s, layer);
        s.set_tool(ToolKind::Erase);
        s.set_brush_size(0.05);
        s.set_vector_eraser(VectorEraser::WholeStroke);
        // the eraser dragged down across the first two lines: each goes as it is reached
        s.stroke_begin(PointerKind::Stylus, vec![at(150.0, 40.0, 0.0)]);
        assert_eq!(s.vector_stroke_count(layer), 3);
        s.stroke_add(vec![at(150.0, 100.0, 8.0)]);
        assert_eq!(s.vector_stroke_count(layer), 2, "the first gone while the pen is still down");
        let one_gone = pixels(&s, layer);
        assert_eq!(pixel(&one_gone, 60, 80)[3], 0);
        assert_eq!(pixel(&one_gone, 60, 150), pixel(&drawn, 60, 150));
        // (a long way in one go: nothing passed over is missed)
        s.stroke_add(vec![at(150.0, 180.0, 16.0)]);
        assert_eq!(s.vector_stroke_count(layer), 1);
        s.stroke_end(vec![at(150.0, 185.0, 24.0)]);
        let two_gone = pixels(&s, layer);
        assert_eq!((pixel(&two_gone, 60, 150)[3], pixel(&two_gone, 240, 80)[3]), (0, 0));
        assert_eq!(pixel(&two_gone, 60, 220), pixel(&drawn, 60, 220));
        // what shows is what the layer holds
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &two_gone), 0);
        s.undo();
        // one undo brings both back, one redo takes both again
        assert_eq!(s.undo().as_deref(), Some("Erase stroke"));
        assert_eq!(s.vector_stroke_count(layer), 3);
        assert_eq!(furthest(&pixels(&s, layer), &drawn), 0);
        s.redo();
        assert_eq!(s.vector_stroke_count(layer), 1);
        s.undo();
        // called off part way, all it took is back and there is nothing of it to undo
        s.stroke_begin(PointerKind::Stylus, vec![at(150.0, 40.0, 0.0)]);
        s.stroke_add(vec![at(150.0, 100.0, 8.0)]);
        assert_eq!(s.vector_stroke_count(layer), 2);
        s.stroke_cancel();
        assert_eq!(s.vector_stroke_count(layer), 3);
        assert_eq!(furthest(&pixels(&s, layer), &drawn), 0);
        assert_eq!(s.undo().as_deref(), Some("Paint stroke"), "the last line, not the drag called off");
        s.redo();
        // touching nothing, a drag leaves no step
        s.stroke_begin(PointerKind::Stylus, vec![at(20.0, 20.0, 0.0)]);
        s.stroke_add(vec![at(30.0, 25.0, 8.0)]);
        s.stroke_end(Vec::new());
        assert_eq!(s.undo().as_deref(), Some("Paint stroke"));
        s.redo();
        // up to the crossings, as it goes, too
        s.set_tool(ToolKind::Paint);
        s.set_brush_size(0.1);
        straight(&s, (100.0, 60.0), (100.0, 240.0));
        straight(&s, (200.0, 60.0), (200.0, 240.0));
        s.set_tool(ToolKind::Erase);
        s.set_brush_size(0.05);
        s.set_vector_eraser(VectorEraser::ToCrossing);
        s.stroke_begin(PointerKind::Stylus, vec![at(150.0, 140.0, 0.0)]);
        s.stroke_add(vec![at(150.0, 160.0, 8.0)]);
        assert_eq!(s.vector_stroke_count(layer), 6, "the middle line in two pieces before the pen lifts");
        assert_eq!(pixel(&pixels(&s, layer), 150, 150)[3], 0);
        s.stroke_end(Vec::new());
        assert!(pixel(&pixels(&s, layer), 60, 150)[3] > 200 && pixel(&pixels(&s, layer), 240, 150)[3] > 200);
    }

    #[test]
    fn what_is_transformed_is_shown_moving_alone_over_the_rest_of_its_layer() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.15);
        // a line across and a line down through it
        straight(&s, (40.0, 100.0), (260.0, 100.0));
        straight(&s, (150.0, 40.0), (150.0, 260.0));
        let before = pixels(&s, layer);
        let layers = s.layers().len();
        // the line across selected (away from the crossing) and taken up to be transformed
        s.select_by_colour(60.0, 100.0, 0.5, SelectionJoin::Add, false);
        assert_eq!(s.shared.lock().vector_selected(LayerId(layer)).map(|ids| ids.len()), Some(1));
        assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
        // it is on a layer of its own for now, and its own layer is whole without it: the line
        // down not cut where they cross
        let shown = s.layers();
        assert_eq!(shown.len(), layers + 1);
        let lifted = shown.iter().find(|l| l.name == LIFTED_NAME).unwrap().id;
        // (read as they are, without ending the transform as other reads of a layer would)
        let raw = |id: u64| {
            let mut st = s.shared.lock();
            st.flush();
            let (canvas, gpu) = (st.canvas.as_ref().unwrap(), st.gpu.as_ref().unwrap());
            canvas.read_layer(gpu, LayerId(id)).unwrap()
        };
        let (rest, picked) = (raw(layer), raw(lifted));
        assert_eq!(pixel(&rest, 60, 100)[3], 0, "not on its layer while it is held");
        assert_eq!(pixel(&rest, 150, 100), pixel(&before, 150, 60), "the line down whole where they crossed");
        assert_eq!(pixel(&rest, 150, 200), pixel(&before, 150, 200));
        assert_eq!(pixel(&picked, 60, 100), pixel(&before, 60, 100), "all of it on the layer of its own");
        assert_eq!(pixel(&picked, 150, 100), pixel(&before, 60, 100), "whole where the other crossed it");
        assert_eq!(pixel(&picked, 150, 60)[3], 0, "none of the line down with it");
        s.transform_grab(crate::TransformHandle::Move, 150.0, 100.0);
        s.transform_drag(150.0, 200.0);
        s.transform_release();
        assert_eq!(s.layers().len(), layers + 1, "still held while it is dragged");
        // kept: set down where it was moved to, the layer of its own gone
        s.transform_end(true);
        assert_eq!(s.layers().len(), layers);
        assert!(s.layers().iter().any(|l| l.id == layer && l.current), "its own layer the current one again");
        let after = pixels(&s, layer);
        assert_eq!(pixel(&after, 60, 100)[3], 0);
        assert_eq!(pixel(&after, 60, 200), pixel(&before, 60, 100));
        assert_eq!(pixel(&after, 150, 100), pixel(&before, 150, 60));
        assert_eq!(s.vector_stroke_count(layer), 2);
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &after), 0);
        s.undo();
        // the selection let go is one step, the transform one more: then all is as it was
        s.undo();
        assert_eq!(s.undo().as_deref(), Some("non-uniform transform"));
        assert_eq!(s.layers().len(), layers);
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        assert!(s.has_selection());
        // called off: put back, nothing left of it to undo
        assert!(s.transform_begin().is_some());
        assert_eq!(s.layers().len(), layers + 1);
        s.transform_grab(crate::TransformHandle::Move, 150.0, 100.0);
        s.transform_drag(170.0, 230.0);
        s.transform_release();
        s.transform_end(false);
        assert_eq!(s.layers().len(), layers);
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        assert!(s.has_selection(), "what was selected still is");
        assert_eq!(s.vector_stroke_count(layer), 2);
        assert_eq!(s.undo().as_deref(), Some("Automatic Selection"), "the selecting, not the transform called off");
    }

    /// What undo would take back next.
    fn undo_name(s: &CanvasSession) -> Option<String> {
        let mut st = s.shared.lock();
        st.flush();
        st.canvas.as_ref().unwrap().undo_name()
    }

    /// How many pixels down column `x`, within 40 of row `around`, are painted.
    fn tall(rgba: &[u8], x: usize, around: usize) -> usize {
        (around - 40..around + 40).filter(|&y| pixel(rgba, x, y)[3] > 128).count()
    }

    #[test]
    fn a_dropped_colour_and_the_brush_as_it_is_set_go_to_what_is_already_drawn() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        let before = pixels(&s, layer);
        // green dropped right on the first line's ink: the line takes it, and nothing is filled
        assert!(s.pour_drop(150.0, 100.0, 0.0, 1.0, 0.0, 0.5));
        s.pour_end(true, None);
        assert_eq!((s.vector_stroke_count(layer), s.vector_fill_count(layer)), (2, 1));
        let green = pixels(&s, layer);
        let p = pixel(&green, 150, 100);
        assert!(p[1] > 200 && p[0] < 40 && p[3] > 200, "{p:?}");
        assert_eq!(pixel(&green, 150, 200), pixel(&before, 150, 200));
        assert_eq!(s.undo().as_deref(), Some("Recolor"));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        s.redo();
        // the second line and the square picked, blue dropped on the square: both take it
        s.select_shape(false, 30.0, 180.0, 70.0, 220.0, SelectionJoin::Add);
        s.select_by_colour(80.0, 150.0, 0.5, SelectionJoin::Add, false);
        assert_eq!(s.vector_picked_count(), 2);
        assert!(s.pour_drop(80.0, 150.0, 0.0, 0.0, 1.0, 0.5));
        s.pour_end(true, None);
        assert_eq!(s.vector_picked_count(), 2, "still picked");
        assert_eq!(s.vector_fill_count(layer), 1);
        let blue = pixels(&s, layer);
        assert_eq!(pixel(&blue, 80, 150), [0, 0, 255, 255]);
        let p = pixel(&blue, 150, 200);
        assert!(p[2] > 200 && p[0] < 40 && p[3] > 200, "{p:?}");
        assert_eq!(pixel(&blue, 150, 100), pixel(&green, 150, 100), "the line not picked keeps its own");
        // the brush made bigger, and its size given to what is picked: that line is drawn wider
        s.set_brush_size(0.4);
        assert!(s.vector_apply(VectorApply::Size));
        let wide = pixels(&s, layer);
        assert!(
            tall(&wide, 150, 200) > tall(&blue, 150, 200) + 4,
            "{} {}",
            tall(&wide, 150, 200),
            tall(&blue, 150, 200)
        );
        assert_eq!(tall(&wide, 150, 100), tall(&blue, 150, 100));
        assert!(selected(&s, 150, 200 + tall(&wide, 150, 200) / 2 - 1), "the selection takes its new shape");
        assert!(!s.vector_apply(VectorApply::Size), "nothing to change twice");
        assert_eq!(s.undo().as_deref(), Some("Apply Size"));
        assert_eq!(furthest(&pixels(&s, layer), &blue), 0);
        assert!(!selected(&s, 150, 200 + tall(&wide, 150, 200) / 2 - 1));
        // the colour as it is set
        s.set_color(1.0, 1.0, 0.0);
        assert!(s.vector_apply(VectorApply::Colour));
        assert_eq!(pixel(&pixels(&s, layer), 80, 150), [255, 255, 0, 255]);
        // another brush: the line is that brush's now, as wide as it was
        let kept = |s: &CanvasSession| {
            let st = s.shared.lock();
            let content = st.doc.layer(LayerId(layer)).unwrap().vector.clone().unwrap();
            let stroke = content.strokes().nth(1).unwrap();
            (stroke.setup.brush.name.clone(), stroke.setup.diameter, stroke.ink.colour)
        };
        let was = kept(&s);
        let other = (0..6u32)
            .flat_map(|set| (0..12u32).map(move |i| (set, i)))
            .find(|&(set, i)| s.select_brush(set, i) && s.shared.lock().brush(ToolKind::Paint).name != was.0)
            .is_some();
        assert!(other, "another brush to give it");
        assert!(s.vector_apply(VectorApply::Brush));
        let now = kept(&s);
        assert_ne!(now.0, was.0);
        assert_eq!((now.1, now.2), (was.1, was.2));
        assert_eq!(s.undo().as_deref(), Some("Apply Brush"));
        assert_eq!(kept(&s), was);
        // with nothing picked there is nothing to give them to
        s.select_none();
        assert!(!s.vector_apply(VectorApply::Colour));
    }

    #[test]
    fn zoomed_in_a_node_dragged_goes_all_the_way_with_the_window_drawn_meanwhile() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        s.shared.lock().view = View::centred(Vec2::new(150.0, 150.0), 1.5, 0.0, (300, 300));
        s.set_vector_crisp(true);
        frame(&s);
        frame(&s);
        assert!(s.shared.lock().crisp.is_some());
        s.select_shape(false, 0.0, 55.0, 300.0, 95.0, SelectionJoin::Add);
        frame(&s);
        let nodes = s.vector_nodes();
        let n = nodes.len() / 2;
        assert!(n >= 3, "{nodes:?}");
        let mid = n / 2;
        let (x, y) = (nodes[mid * 2], nodes[mid * 2 + 1]);
        // dragged down in steps, a frame drawn after each, as the app does
        assert!(s.vector_node_grab(mid as u32));
        for step in 1..=4 {
            assert!(s.vector_node_drag(x, y + 10.0 * step as f32), "step {step}: the drag was let go");
            frame(&s);
        }
        let held = s.vector_nodes();
        assert!((held[mid * 2 + 1] - (y + 40.0)).abs() < 0.01, "{held:?}");
        assert!(s.vector_node_release(true));
        assert_eq!(s.undo().as_deref(), Some("Reshape"));
        assert_eq!(s.vector_stroke_count(layer), 2);
    }

    #[test]
    fn a_picked_stroke_is_pulled_into_another_shape_by_its_nodes() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        let before = pixels(&s, layer);
        assert!(s.vector_nodes().is_empty(), "nothing picked");
        s.select_shape(false, 30.0, 80.0, 70.0, 120.0, SelectionJoin::Add);
        // the line's two ends, and some along it to bend it by
        let nodes = s.vector_nodes();
        let n = nodes.len() / 2;
        assert!(n >= 3, "{nodes:?}");
        assert!((nodes[0] - 40.0).abs() < 4.0 && (nodes[n * 2 - 2] - 260.0).abs() < 4.0, "{nodes:?}");
        let mid =
            (0..n).min_by(|&a, &b| (nodes[a * 2] - 150.0).abs().total_cmp(&(nodes[b * 2] - 150.0).abs())).unwrap();
        let (x, y) = (nodes[mid * 2], nodes[mid * 2 + 1]);
        assert!(mid > 0 && mid < n - 1);
        // the one in the middle dragged 40 down
        assert!(s.vector_node_grab(mid as u32));
        assert!(s.vector_node_drag(x, y + 20.0));
        assert!(s.vector_node_drag(x, y + 40.0));
        let held = s.vector_nodes();
        assert_eq!(held.len(), nodes.len());
        assert!((held[mid * 2 + 1] - (y + 40.0)).abs() < 0.01 && held[0] == nodes[0], "{held:?}");
        assert!(s.vector_node_release(true));
        let bent = pixels(&s, layer);
        let column = x as usize;
        assert!((136..146).any(|row| pixel(&bent, column, row)[3] > 128), "the line goes through where it was pulled");
        assert_eq!(pixel(&bent, column, 100)[3], 0, "and no longer where it was");
        let (first, last) = ends(&s, layer, 0);
        assert!((first.0 - 40.0).abs() < 2.0 && (first.1 - 100.0).abs() < 2.0, "{first:?}");
        assert!((last.0 - 260.0).abs() < 2.0 && (last.1 - 100.0).abs() < 2.0, "{last:?}");
        assert_eq!(pixel(&bent, 150, 200), pixel(&before, 150, 200), "the other line as it was");
        assert_eq!(s.vector_stroke_count(layer), 2);
        // it is still what is picked, in its new shape
        assert!(selected(&s, column, 140) && !selected(&s, column, 100));
        assert_eq!(s.vector_picked_count(), 1);
        // one undo for all of the drag
        assert_eq!(s.undo().as_deref(), Some("Reshape"));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        assert!(selected(&s, column, 100));
        // an end dragged and called off: as it was, with no step left of it
        assert!(s.vector_node_grab(0));
        assert!(s.vector_node_drag(40.0, 60.0));
        assert!(s.vector_node_release(false));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        assert_ne!(undo_name(&s).as_deref(), Some("Reshape"));
        // with the selection tool, a touch that lands on a node drags it; one elsewhere selects
        s.set_touch_selecting(Some(SelectTouch {
            kind: SelectKind::Rectangle,
            join: SelectionJoin::Add,
            threshold: 0.5,
            lasso_fill: false,
            lasso_erase: false,
            lasso_tool: false,
        }));
        // (the node stays put until the pen has gone a little way, then goes as it does)
        s.touch_down(1, PointerKind::Stylus, vec![at(260.0, 100.0, 0.0)]);
        for (i, y) in [130.0, 160.0, 190.0].into_iter().enumerate() {
            s.touch_move(1, vec![at(260.0, y, 30.0 * (i + 1) as f64)]);
        }
        s.touch_up(1, vec![at(260.0, 190.0, 120.0)]);
        let (first, last) = ends(&s, layer, 0);
        assert!((first.1 - 100.0).abs() < 2.0, "{first:?}");
        assert!(last.1 > 125.0 && (last.0 - 260.0).abs() < 4.0, "the end went with the pen: {last:?}");
        assert_eq!(undo_name(&s).as_deref(), Some("Reshape"));
        assert_eq!(s.vector_picked_count(), 1);
    }

    /// The window as a frame shows it now (300 × 300, RGBA rows).
    fn frame(s: &CanvasSession) -> Vec<u8> {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let target = st.gpu.as_ref().unwrap().device.create_texture(&wgpu::TextureDescriptor {
            label: Some("test frame"),
            size: wgpu::Extent3d { width: 300, height: 300, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let view = target.create_view(&wgpu::TextureViewDescriptor::default());
        st.draw_frame(&view, format, (300, 300), Default::default(), std::time::Instant::now());
        let gpu = st.gpu.as_ref().unwrap();
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        procraft_gpu::read_texture(gpu, &target, 300, 300).unwrap()
    }

    #[test]
    fn zoomed_in_a_held_colordrop_undone_leaves_no_fill_on_the_sharp_picture() {
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        // a box whose right side stops short of its bottom
        straight(&s, (70.0, 80.0), (230.0, 80.0));
        straight(&s, (220.0, 70.0), (220.0, 217.0));
        straight(&s, (217.0, 220.0), (70.0, 220.0));
        straight(&s, (80.0, 230.0), (80.0, 70.0));
        s.shared.lock().view = View::centred(Vec2::new(150.0, 150.0), 1.5, 0.0, (300, 300));
        s.set_vector_crisp(true);
        frame(&s);
        frame(&s);
        assert!(s.shared.lock().crisp.is_some());
        let lines = frame(&s);
        // (a window corner: outside the box, on the canvas)
        let corner = |rgba: &[u8]| pixel(rgba, 4, 4);
        assert_eq!(corner(&lines), [255, 255, 255, 255]);
        // dropped, held and slid up so it comes out through the gap and fills the rest, the window
        // drawn all the while as it is in the app: the held fill shows on the sharp picture, and
        // drawing it there doesn't end the drop
        assert!(s.pour_begin(150.0, 150.0, 1.0, 0.0, 0.0, 0.5));
        frame(&s);
        s.pour_threshold(1.0);
        frame(&s);
        let held = frame(&s);
        assert!(s.shared.lock().vector_pour.is_some(), "the drop was let go by drawing the window");
        assert!(s.shared.lock().crisp.is_some());
        assert_eq!(corner(&held), [255, 0, 0, 255]);
        // let go: one step
        s.pour_end(true, None);
        frame(&s);
        assert_eq!(corner(&frame(&s)), [255, 0, 0, 255]);
        // undone: the sharp picture shows the box alone again
        assert_eq!(s.undo().as_deref(), Some("ColorDrop"));
        assert_eq!(s.vector_fill_count(layer), 0);
        frame(&s);
        let undone = frame(&s);
        assert_eq!(corner(&undone), [255, 255, 255, 255], "the fill left on the sharp picture");
        assert!(furthest(&undone, &lines) <= 3, "{}", furthest(&undone, &lines));
        // and redone, it is back
        s.redo();
        frame(&s);
        assert_eq!(corner(&frame(&s)), [255, 0, 0, 255]);
    }

    #[test]
    fn zoomed_in_the_sharp_picture_reaches_past_the_window_for_a_pan_or_zoom_out() {
        let Some(s) = session() else { return };
        let _layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.3);
        straight(&s, (20.0, 150.0), (280.0, 150.0));
        s.shared.lock().view = View::centred(Vec2::new(150.0, 150.0), 4.0, 0.0, (300, 300));
        s.set_vector_crisp(true);
        frame(&s);
        frame(&s);
        // a quarter of the window past each of its edges
        let (from, w, h) = {
            let st = s.shared.lock();
            let crisp = st.crisp.as_ref().unwrap();
            (crisp.to_screen.apply(Vec2::ZERO), crisp.doc.width(), crisp.doc.height())
        };
        assert_eq!((from.x, from.y, w, h), (-75.0, -75.0, 450, 450));
        // zoomed out to 3×: all of the window still lies on it, so it all shows sharp meanwhile
        s.shared.lock().view = View::centred(Vec2::new(150.0, 150.0), 3.0, 0.0, (300, 300));
        let (to_crisp, w) = {
            let st = s.shared.lock();
            let crisp = st.crisp.as_ref().unwrap();
            (st.view.unwrap().to_screen().inverse().unwrap().then(crisp.to_crisp), crisp.doc.width() as f32)
        };
        for (x, y) in [(0.0, 0.0), (300.0, 0.0), (0.0, 300.0), (300.0, 300.0)] {
            let p = to_crisp.apply(Vec2::new(x, y));
            assert!(p.x >= 0.0 && p.y >= 0.0 && p.x <= w && p.y <= w, "window corner {x}, {y} off it: {p:?}");
        }
        frame(&s);
        assert!(s.shared.lock().crisp.is_some());
    }

    #[test]
    fn zoomed_in_a_line_drawn_sharp_is_as_wide_as_on_the_artwork() {
        let Some(s) = session() else { return };
        let _layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.3);
        straight(&s, (60.0, 150.0), (240.0, 150.0));
        // the ink across the line, down the window's middle column
        let ink = |rgba: &[u8]| -> f64 { (0..300).map(|y| 1.0 - f64::from(pixel(rgba, 150, y)[1]) / 255.0).sum() };
        for zoom in [4.0f32, 12.0] {
            s.shared.lock().view = View::centred(Vec2::new(150.0, 150.0), zoom, 0.0, (300, 300));
            s.set_vector_crisp(false);
            let soft = ink(&frame(&s));
            s.set_vector_crisp(true);
            frame(&s);
            let sharp = ink(&frame(&s));
            assert!(s.shared.lock().crisp.is_some());
            // (the round tip's rim and the edge its stamps build up are the artwork's pixels, not
            // the screen's: drawn as the screen's, the line came out two of them thinner)
            assert!((soft - sharp).abs() < soft * 0.03, "at {zoom}×: {soft} on the artwork, {sharp} sharp");
        }
    }

    /// Which columns (by their place in each four) have a pixel unlike the one to its left, among
    /// rows `rows`: a picture magnified four times in squares changes only at every fourth.
    fn changes_at(rgba: &[u8], rows: std::ops::Range<usize>) -> [bool; 4] {
        let mut at = [false; 4];
        for y in rows {
            for x in 1..300 {
                if pixel(rgba, x, y) != pixel(rgba, x - 1, y) {
                    at[x % 4] = true;
                }
            }
        }
        at
    }

    #[test]
    fn zoomed_in_a_vector_layer_is_drawn_again_as_fine_as_the_screen() {
        let Some(s) = session() else { return };
        // a red square on a layer of pixels, and over it a vector layer with a slanting line
        s.set_color(1.0, 0.0, 0.0);
        s.fill_polygon(
            vec![120.0, 150.0, 160.0, 150.0, 160.0, 175.0, 120.0, 175.0],
            LassoFill { erase: false, tool_opacity: false },
        );
        let layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.5);
        straight(&s, (100.0, 105.0), (200.0, 135.0));
        // four times as big, about the middle of it
        s.shared.lock().view = View::centred(Vec2::new(150.0, 140.0), 4.0, 0.0, (300, 300));
        s.set_vector_crisp(false);
        let soft = frame(&s);
        assert_eq!(changes_at(&soft, 0..300).iter().filter(|c| **c).count(), 1, "squares, four pixels a side");
        // on: the view has only just come to rest, so this frame is as before; the next is sharp
        s.set_vector_crisp(true);
        assert_eq!(furthest(&frame(&s), &soft), 0);
        let sharp = frame(&s);
        assert!(s.shared.lock().crisp.is_some());
        // the line's edge now falls on any pixel of the window, not every fourth
        let line_rows = 0..130;
        assert!(
            changes_at(&sharp, line_rows.clone()).iter().filter(|c| **c).count() >= 3,
            "the line as fine as the screen"
        );
        // it is the same line: as much ink, in the same place
        let dark = |rgba: &[u8]| {
            let px = rgba.as_chunks::<4>().0;
            let ink: Vec<(usize, f64)> = px
                .iter()
                .enumerate()
                .map(|(i, p)| (i, 1.0 - f64::from(p[1]) / 255.0))
                .filter(|(_, d)| *d > 0.0)
                .collect();
            let all: f64 = ink.iter().map(|(_, d)| d).sum();
            let (x, y) =
                ink.iter().fold((0.0, 0.0), |(x, y), (i, d)| (x + (i % 300) as f64 * d, y + (i / 300) as f64 * d));
            (all, x / all, y / all)
        };
        // (only where the line is: the square is red, its green none too)
        let top = |rgba: &[u8]| rgba[..130 * 300 * 4].to_vec();
        let (a, b) = (dark(&top(&soft)), dark(&top(&sharp)));
        assert!((a.0 - b.0).abs() < a.0 * 0.12, "{a:?} {b:?}");
        assert!((a.1 - b.1).abs() < 1.5 && (a.2 - b.2).abs() < 1.5, "{a:?} {b:?}");
        // the layer of pixels shows just as it did, squares and all
        for y in 200..300 {
            for x in 0..300 {
                assert_eq!(pixel(&sharp, x, y), pixel(&soft, x, y), "at {x}, {y}");
            }
        }
        assert_eq!(pixel(&sharp, 100, 250), [255, 0, 0, 255]);
        // the artwork itself is untouched by it
        let kept = pixels(&s, layer);
        assert_eq!(kept.len(), 300 * 300 * 4);
        // the picture changed (the line undone): the sharp one is let go, and with nothing on the
        // vector layer none is made
        s.undo();
        let gone = frame(&s);
        assert!(s.shared.lock().crisp.is_none());
        assert!((0..130).all(|y| (0..300).all(|x| pixel(&gone, x, y) == pixel(&gone, 0, 0))), "no line");
        frame(&s);
        assert!(s.shared.lock().crisp.is_none());
        // back: made again, with the line
        s.redo();
        frame(&s);
        frame(&s);
        assert!(s.shared.lock().crisp.is_some());
        let rest = |d: std::time::Duration| s.shared.lock().crisp_rest = d;
        let sharp_at =
            |rgba: &[u8], rows: std::ops::Range<usize>| changes_at(rgba, rows).iter().filter(|c| **c).count() >= 3;
        // a stroke drawn now is drawn on the sharp picture too, as it is made: sharp under the pen
        // (in a corner of the window that neither the line nor the square reaches)
        let band = 136..188;
        let clear = |rgba: &[u8]| band.clone().all(|y| (0..60).all(|x| pixel(rgba, x, y) == pixel(rgba, 0, 136)));
        let sharp_there = |rgba: &[u8]| {
            let mut at = [false; 4];
            for y in band.clone() {
                for x in 1..60 {
                    at[x % 4] |= pixel(rgba, x, y) != pixel(rgba, x - 1, y);
                }
            }
            at.iter().filter(|c| **c).count() >= 3
        };
        assert!(clear(&frame(&s)));
        rest(std::time::Duration::from_secs(3600));
        s.set_color(0.0, 0.0, 1.0);
        s.set_brush_size(0.1);
        s.stroke_begin(PointerKind::Stylus, vec![at(2.0, 150.0, 0.0)]);
        s.stroke_add(vec![at(100.0, 170.0, 10.0), at(180.0, 152.0, 20.0), at(260.0, 172.0, 30.0)]);
        let during = frame(&s);
        assert!(s.shared.lock().crisp_stroke.is_some() && s.shared.lock().crisp.is_some());
        assert!(!clear(&during) && sharp_there(&during), "the stroke shows, sharp, as it is drawn");
        s.stroke_end(vec![at(260.0, 172.0, 40.0)]);
        let drawn = frame(&s);
        assert!(s.shared.lock().crisp.is_some() && s.shared.lock().crisp_stroke.is_none());
        assert_eq!(s.vector_stroke_count(layer), 2);
        assert!(sharp_there(&drawn));
        // made again from what the layer now holds, it is the picture the stroke left
        rest(std::time::Duration::ZERO);
        let again = frame(&s);
        assert!(furthest(&drawn, &again) <= 3, "{}", furthest(&drawn, &again));
        // undone, it is brought up to date before the next frame, and is still the sharp one
        s.undo();
        let undone = frame(&s);
        assert!(s.shared.lock().crisp.is_some());
        assert!(clear(&undone) && sharp_at(&undone, 0..130));
        // a stroke on the layer of pixels is drawn on it too; as it ends, that layer shows its own
        // pixels again, squares and all: just what the artwork's canvas shows there
        let below = s.layers().iter().find(|l| !l.vector).unwrap().id;
        s.select_layer(below);
        s.stroke_begin(PointerKind::Stylus, vec![at(2.0, 158.0, 0.0)]);
        s.stroke_add(vec![at(150.0, 164.0, 10.0), at(260.0, 160.0, 20.0)]);
        assert!(!clear(&frame(&s)));
        assert!(s.shared.lock().crisp_stroke.is_some());
        s.stroke_end(vec![at(260.0, 160.0, 30.0)]);
        let painted = frame(&s);
        assert!(s.shared.lock().crisp.is_some());
        s.set_vector_crisp(false);
        let plain = frame(&s);
        for y in band.clone() {
            for x in 0..60 {
                assert_eq!(pixel(&painted, x, y), pixel(&plain, x, y), "at {x}, {y}");
            }
        }
        assert!(!clear(&plain));
        s.set_vector_crisp(true);
        frame(&s);
        frame(&s);
        // the view moved: it goes on showing, where that part of the artwork now is, over the
        // artwork's own canvas; and is made for the new view once that rests
        s.shared.lock().view = View::centred(Vec2::new(152.0, 141.0), 4.0, 0.0, (300, 300));
        let moved = frame(&s);
        assert!(s.shared.lock().crisp.is_some());
        assert!(sharp_at(&moved, 0..126), "still sharp as the view moves");
        let rested = frame(&s);
        assert!(sharp_at(&rested, 0..126));
        // (the same picture, but at the edges the view came to show, where the artwork's own canvas
        // showed through meanwhile)
        let within = |rgba: &[u8]| -> Vec<u8> {
            (16..284).flat_map(|y| (16..284).flat_map(move |x| pixel(rgba, x, y))).collect()
        };
        assert!(furthest(&within(&moved), &within(&rested)) <= 3, "{}", furthest(&within(&moved), &within(&rested)));
        // a transform under way is shown by the artwork's canvas; the sharp one comes back after
        s.select_layer(layer);
        assert!(s.transform_begin().is_some());
        let held = frame(&s);
        assert!(!sharp_at(&held, 0..126));
        s.transform_end(false);
        assert!(sharp_at(&frame(&s), 0..126));
    }

    #[test]
    fn zoomed_in_a_lasso_s_fill_shows_on_the_sharp_picture_as_it_is_drawn() {
        use crate::selection::StrokeFill;
        let Some(s) = session() else { return };
        let layer = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        // a line near the top, for there to be a sharp picture of: twice as big, it runs along the
        // window's row 20
        straight(&s, (90.0, 85.0), (210.0, 85.0));
        let dir = std::env::temp_dir().join(format!("procraft-lasso-sharp-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let plugins = crate::Plugins::with_store(dir.to_string_lossy().into_owned());
        let (_, file) = crate::bundled::BUNDLED.iter().find(|(name, _)| *name == "selection-lasso-fill").unwrap();
        assert_eq!(plugins.install(file.to_vec()), None);
        s.use_plugins(Arc::clone(&plugins));
        assert!(plugins.set_on("io.github.artcraftandroid.selection-lasso-fill".to_owned(), true));
        s.shared.lock().view = View::centred(Vec2::new(150.0, 150.0), 2.0, 0.0, (300, 300));
        s.set_vector_crisp(true);
        frame(&s);
        frame(&s);
        assert!(s.shared.lock().crisp.is_some());
        // (the sharp picture is not made again while this goes on: what shows is what was put on it)
        s.shared.lock().crisp_rest = std::time::Duration::from_secs(3600);
        // a square gone round with the pen on the window, a move every 4 px, not lifted
        let round = |s: &CanvasSession, ms: f64| {
            s.touch_down(1, PointerKind::Stylus, vec![at(60.0, 60.0, ms)]);
            let corners = [(60.0, 60.0), (240.0, 60.0), (240.0, 240.0), (60.0, 240.0), (60.0, 70.0)];
            let mut t = ms;
            for pair in corners.windows(2) {
                let [(ax, ay), (bx, by)]: [(f32, f32); 2] = [pair[0], pair[1]];
                let steps = ((bx - ax).abs().max((by - ay).abs()) / 4.0).ceil().max(1.0) as usize;
                for k in 1..=steps {
                    let f = k as f32 / steps as f32;
                    t += 4.0;
                    s.touch_move(1, vec![at(ax + (bx - ax) * f, ay + (by - ay) * f, t)]);
                }
            }
            t
        };
        let white = [255, 255, 255, 255];
        s.set_brush_opacity(1.0);
        s.set_color(1.0, 0.0, 0.0);
        s.set_secondary_color(0.0, 0.0, 1.0);
        s.set_lasso_preview_opacity(1.0);
        // Outline: the stroke is drawn on the sharp picture, and what it goes round shows there
        s.set_stroke_fill(Some(StrokeFill { secondary: true, opacity: 1.0 }));
        let t = round(&s, 0.0);
        let during = frame(&s);
        assert!(s.shared.lock().crisp_stroke.is_some(), "drawn on the sharp picture");
        assert_eq!(pixel(&during, 150, 150), [0, 0, 255, 255], "the fill shown under the stroke");
        assert_eq!(pixel(&during, 20, 150), white, "and nowhere else");
        // lifted: the fill is there at once, with no frame between that shows neither
        s.touch_up(1, vec![at(60.0, 70.0, t + 4.0)]);
        let lifted = frame(&s);
        assert_eq!(s.vector_fill_count(layer), 1);
        assert_eq!(pixel(&lifted, 150, 150), [0, 0, 255, 255], "filled as it showed");
        assert_eq!(pixel(&lifted, 20, 150), white);
        s.undo();
        assert_eq!(pixel(&frame(&s), 150, 150), white, "nothing left shown once it is taken back");
        // let go of, a stroke takes what it showed with it
        round(&s, 1000.0);
        assert_eq!(pixel(&frame(&s), 150, 150), [0, 0, 255, 255]);
        s.touch_cancel();
        assert_eq!(pixel(&frame(&s), 150, 150), white);
        s.set_stroke_fill(None);
        // the Pen's own lasso: no stroke is drawn, and its fill shows on the sharp picture all the same
        s.set_touch_selecting(Some(SelectTouch {
            kind: SelectKind::Freehand,
            join: SelectionJoin::Add,
            threshold: 0.5,
            lasso_fill: true,
            lasso_erase: false,
            lasso_tool: true,
        }));
        let t = round(&s, 2000.0);
        let during = frame(&s);
        assert!(s.shared.lock().crisp.is_some());
        assert_eq!(pixel(&during, 150, 150), [255, 0, 0, 255], "shown as it will fill");
        assert_eq!(pixel(&during, 20, 150), white);
        s.touch_up(1, vec![at(60.0, 70.0, t + 4.0)]);
        assert_eq!(s.vector_fill_count(layer), 1);
        assert_eq!(pixel(&frame(&s), 150, 150), [255, 0, 0, 255], "filled");
        s.undo();
        assert_eq!(pixel(&frame(&s), 150, 150), white, "and nothing left shown");
        // the tool put away mid-lasso takes its preview with it
        round(&s, 3000.0);
        assert_eq!(pixel(&frame(&s), 150, 150), [255, 0, 0, 255]);
        s.set_touch_selecting(None);
        assert_eq!(pixel(&frame(&s), 150, 150), white);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The open transform's warp, its two middle columns of points all dragged up 40: what it
    /// holds is bent up in the middle (by three quarters of that), its ends staying.
    fn warp_up(s: &CanvasSession) {
        s.transform_set_kind(crate::TransformKind::Warp);
        for index in [1, 2, 5, 6, 9, 10, 13, 14] {
            s.transform_grab(crate::TransformHandle::Mesh { index }, 150.0, 100.0);
            s.transform_drag(150.0, 60.0);
            s.transform_release();
        }
    }

    #[test]
    fn a_kept_transform_undone_brings_the_selection_back_where_the_strokes_are() {
        let Some(s) = session() else { return };
        let _layer = two_lines_and_a_square(&s);
        // the first line (along y 100) picked and bent up in the middle, kept
        s.select_shape(false, 30.0, 80.0, 70.0, 120.0, SelectionJoin::Add);
        assert!(selected(&s, 150, 100));
        assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
        warp_up(&s);
        s.transform_end(true);
        assert!(!s.has_selection(), "a kept transform lets go of the selection");
        // the selection back: around the line where it is now, not where it was
        assert_eq!(s.undo().as_deref(), Some("clear selection"));
        assert!(selected(&s, 150, 70) && !selected(&s, 150, 100), "the selection is where the line was");
        assert_eq!(s.vector_picked_count(), 1, "and still picks the line");
        // the transform undone: line and selection both back where they were
        assert_eq!(s.undo().as_deref(), Some("warp transform"));
        assert!(selected(&s, 150, 100) && !selected(&s, 150, 70));
        assert_eq!(s.vector_picked_count(), 1);
        // and forward again, the same
        s.redo();
        assert!(selected(&s, 150, 70) && !selected(&s, 150, 100));
        s.redo();
        assert!(!s.has_selection());
    }

    /// A frame drawn with the open warp's strokes drawn for it whatever the last took (a slow one
    /// is otherwise put off a while, which a test run beside others can be).
    fn warp_frame(s: &CanvasSession) -> Vec<u8> {
        s.shared.lock().vector_warp_draw(true);
        frame(s)
    }

    /// Layer `layer`'s pixels as they are shown now, with whatever is open left open.
    fn shown_pixels(s: &CanvasSession, layer: u64) -> Vec<u8> {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        st.flush();
        st.canvas.as_mut().unwrap().read_layer(st.gpu.as_ref().unwrap(), LayerId(layer)).unwrap()
    }

    #[test]
    fn separate_objects_gives_lines_lassos_and_fills_a_vector_layer_each() {
        let Some(s) = session() else { return };
        // a box of lines, red poured inside it (a fill traced from them), a blue shape drawn by
        // hand inside that, and a stroke of the eraser across the lot
        let layer = s.add_vector_layer();
        let separates = |s: &CanvasSession, id: u64| {
            s.layer_options(id).into_iter().find(|r| r.option == crate::LayerOption::SeparateObjects).map(|r| r.enabled)
        };
        boxed(&s);
        // lines alone: nothing to tell apart
        assert_eq!(separates(&s, layer), Some(false));
        assert!(!s.separate_vector_objects(layer));
        assert!(s.pour_drop(150.0, 150.0, 1.0, 0.0, 0.0, 0.5));
        s.set_color(0.0, 0.0, 1.0);
        s.fill_polygon(
            vec![100.0, 100.0, 140.0, 100.0, 140.0, 140.0, 100.0, 140.0],
            LassoFill { erase: false, tool_opacity: false },
        );
        s.set_tool(ToolKind::Erase);
        s.set_brush_size(0.2);
        straight(&s, (60.0, 120.0), (240.0, 120.0));
        s.set_tool(ToolKind::Paint);
        let before = pixels(&s, layer);
        let shown = frame(&s);
        let names = |s: &CanvasSession| s.layers().iter().map(|l| l.name.clone()).collect::<Vec<_>>();
        let was = names(&s);
        let own = was[0].clone();
        assert_eq!(separates(&s, layer), Some(true));
        assert!(s.separate_vector_objects(layer));
        // the layer keeps its lines; its lassos and its fills are on layers of their own under it
        let now = names(&s);
        assert_eq!(now.len(), was.len() + 2);
        assert_eq!(now[..3], [own.clone(), format!("{own} Lassos"), format!("{own} Fills")]);
        let ids: Vec<u64> = s.layers().iter().take(3).map(|l| l.id).collect();
        assert_eq!(ids[0], layer);
        assert!(s.layers().iter().take(3).all(|l| l.vector));
        assert!(s.layers().iter().find(|l| l.id == layer).unwrap().current);
        let kinds = |id: u64| {
            use procraft_engine::vector::ObjectKind;
            let st = s.shared.lock();
            let content = st.doc.layer(LayerId(id)).unwrap().vector.clone().unwrap();
            let count = |kind| content.objects().iter().filter(|o| ObjectKind::of(o) == Some(kind)).count();
            [count(ObjectKind::Line), count(ObjectKind::Lasso), count(ObjectKind::Fill)]
        };
        assert_eq!([kinds(ids[0]), kinds(ids[1]), kinds(ids[2])], [[4, 0, 0], [0, 1, 0], [0, 0, 1]]);
        // together they show what the one layer showed, the eraser's mark on each (where it
        // took only part and one kind lies over another, the lower shows through what is left of
        // the upper: each is erased alone)
        let away = |rgba: &[u8]| -> Vec<u8> {
            (0..300)
                .filter(|y| !(105..135).contains(y))
                .flat_map(|y| (0..300).flat_map(move |x| pixel(rgba, x, y)))
                .collect()
        };
        let apart = frame(&s);
        assert!(furthest(&away(&apart), &away(&shown)) <= 1, "{}", furthest(&away(&apart), &away(&shown)));
        let (lines, lassos, fills) = (pixels(&s, ids[0]), pixels(&s, ids[1]), pixels(&s, ids[2]));
        assert_eq!(pixel(&lines, 150, 80), pixel(&before, 150, 80));
        assert_eq!(pixel(&lines, 150, 150)[3], 0, "no fill left on the lines' layer");
        assert_eq!(pixel(&lassos, 120, 135), [0, 0, 255, 255]);
        assert_eq!(pixel(&lassos, 180, 180)[3], 0);
        assert_eq!(pixel(&fills, 180, 180), [255, 0, 0, 255]);
        assert_eq!(pixel(&fills, 120, 135), [255, 0, 0, 255], "the fill whole under the shape");
        assert_eq!(pixel(&lassos, 120, 120)[3], pixel(&before, 120, 120)[3], "erased as it was");
        assert_eq!(pixel(&fills, 180, 120)[3], pixel(&before, 180, 120)[3]);
        assert!(pixel(&fills, 180, 120)[3] < 255);
        // each is drawn again from what it holds just as it is
        for (id, was) in ids.iter().zip([&lines, &lassos, &fills]) {
            assert!(s.redraw_vector_layer(*id));
            assert_eq!(furthest(&pixels(&s, *id), was), 0);
            s.undo();
        }
        // nothing more to separate on any of them
        assert!(ids.iter().all(|id| separates(&s, *id) == Some(false)));
        // one step
        assert_eq!(s.undo().as_deref(), Some("Separate Objects"));
        assert_eq!(names(&s), was);
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        assert_eq!(furthest(&frame(&s), &shown), 0);
        // no lines: the lassos stay, the fills go under them
        let fills_only = s.add_vector_layer();
        s.set_pour_all_layers(true);
        assert!(s.pour_drop(150.0, 150.0, 0.0, 1.0, 0.0, 0.5));
        s.fill_polygon(vec![10.0, 10.0, 30.0, 10.0, 30.0, 30.0], LassoFill { erase: false, tool_opacity: false });
        let own = s.layers().iter().find(|l| l.id == fills_only).unwrap().name.clone();
        assert!(s.separate_vector_objects(fills_only));
        let at = s.layers().iter().position(|l| l.id == fills_only).unwrap();
        assert_eq!(s.layers()[at + 1].name, format!("{own} Fills"));
        assert_eq!(s.layers().len(), was.len() + 2);
    }

    #[test]
    fn a_warp_is_drawn_roughly_while_a_handle_is_held_and_exactly_when_it_is_let_go() {
        let Some(s) = session() else { return };
        // a box of lines, red poured inside it (a fill laid under them), and a blue shape over
        let layer = s.add_vector_layer();
        boxed(&s);
        assert!(s.pour_drop(150.0, 150.0, 1.0, 0.0, 0.0, 0.5));
        s.set_color(0.0, 0.0, 1.0);
        s.fill_polygon(
            vec![100.0, 100.0, 140.0, 100.0, 140.0, 140.0, 100.0, 140.0],
            LassoFill { erase: false, tool_opacity: false },
        );
        let before = pixels(&s, layer);
        assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
        s.transform_set_kind(crate::TransformKind::Warp);
        let shown =
            |s: &CanvasSession| s.shared.lock().vector_warp.as_ref().and_then(|w| w.shown.as_ref().map(|(_, exact)| *exact));
        // the mesh up and nothing held: drawn exactly (and no differently yet: nothing has moved,
        // but for how a stroke's input is spaced once it is bent)
        warp_frame(&s);
        assert_eq!(shown(&s), Some(true));
        // a handle held and dragged: drawn roughly, each stroke's stamps where the mesh takes them
        s.transform_grab(crate::TransformHandle::Mesh { index: 5 }, 150.0, 150.0);
        s.transform_drag(170.0, 120.0);
        warp_frame(&s);
        assert_eq!(shown(&s), Some(false));
        let rough = shown_pixels(&s, layer);
        assert_ne!(furthest(&rough, &before), 0);
        // let go: exactly, which is what keeping it leaves
        s.transform_release();
        warp_frame(&s);
        assert_eq!(shown(&s), Some(true));
        let exact = shown_pixels(&s, layer);
        // the two are all but the same picture: lines as wide in both, the fills alike
        let differ = rough
            .as_chunks::<4>()
            .0
            .iter()
            .zip(exact.as_chunks::<4>().0)
            .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 96))
            .count();
        assert!(differ * 50 < painted(&exact), "{differ} of {}", painted(&exact));
        let ink =
            |rgba: &[u8]| rgba.as_chunks::<4>().0.iter().filter(|px| px[3] > 128 && px[0] < 64 && px[2] < 64).count();
        assert!(ink(&rough).abs_diff(ink(&exact)) * 20 < ink(&exact), "{} {}", ink(&rough), ink(&exact));
        assert_eq!(pixel(&rough, 180, 180), [255, 0, 0, 255]);
        assert_eq!(pixel(&exact, 180, 180), [255, 0, 0, 255]);
        s.transform_end(true);
        assert_eq!(furthest(&pixels(&s, layer), &exact), 0);
        assert_eq!(s.undo().as_deref(), Some("warp transform"));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
    }

    #[test]
    fn a_puppet_bends_a_vector_layers_strokes_by_its_pins() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        let before = pixels(&s, layer);
        assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
        s.transform_set_kind(crate::TransformKind::Puppet);
        // the first line pinned at its ends, its right end taken down
        s.transform_puppet_tap(50.0, 100.0);
        assert_eq!(s.transform_puppet_tap(250.0, 100.0).map(|b| b.pins.len()), Some(6));
        s.transform_grab(crate::TransformHandle::Pin { index: 1 }, 250.0, 100.0);
        s.transform_drag(250.0, 140.0);
        s.transform_release();
        // shown as its strokes bent, as wide as they were
        let shown = warp_frame(&s);
        assert!(!shown.is_empty());
        assert_eq!(s.shared.lock().vector_warp.as_ref().and_then(|w| w.shown.as_ref().map(|(_, exact)| *exact)), Some(true));
        s.transform_end(true);
        let after = pixels(&s, layer);
        let ink = |rgba: &[u8], x: usize, y: usize| pixel(rgba, x, y)[3] > 128;
        assert!(ink(&after, 250, 140) && !ink(&after, 250, 100), "the right end went down");
        assert!(ink(&after, 50, 100), "the left end stayed");
        assert!(ink(&after, 150, 200) && ink(&after, 80, 150), "the rest as it was");
        assert_eq!(s.undo().as_deref(), Some("puppet warp"));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
    }

    #[test]
    fn a_warp_that_takes_long_to_draw_keeps_up_with_a_held_handle_and_is_kept_the_same() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        let before = pixels(&s, layer);
        // (how long its strokes took to draw last, as said: four frames' worth, or no time)
        let took = |s: &CanvasSession, long: bool| {
            let mut st = s.shared.lock();
            let warp = st.vector_warp.as_mut().unwrap();
            let took = if long { super::WARP_EVERY_FRAME * 4 } else { std::time::Duration::ZERO };
            warp.drawn = Some((std::time::Instant::now() - std::time::Duration::from_secs(5), took));
        };
        let pulled = |s: &CanvasSession| s.shared.lock().vector_warp.as_ref().is_some_and(|w| w.pulled.is_some());
        let drawn_as_a_frame_is = |s: &CanvasSession, long: bool| {
            took(s, long);
            s.shared.lock().vector_warp_draw(false);
            frame(s)
        };
        // the same warp twice: drawn quickly (its strokes at every move), and taking long
        let mut kept = Vec::new();
        for takes_long in [false, true] {
            assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
            s.transform_set_kind(crate::TransformKind::Warp);
            warp_frame(&s);
            s.transform_grab(crate::TransformHandle::Mesh { index: 5 }, 150.0, 150.0);
            s.transform_drag(170.0, 120.0);
            let first = drawn_as_a_frame_is(&s, takes_long);
            assert_eq!(pulled(&s), takes_long);
            // it follows the handle at every move, the layer's picture pulled through the mesh
            s.transform_drag(190.0, 90.0);
            let second = drawn_as_a_frame_is(&s, takes_long);
            assert_eq!(pulled(&s), takes_long);
            assert_ne!(furthest(&first, &second), 0, "the lines went with the handle");
            assert_ne!(furthest(&shown_pixels(&s, layer), &before), 0);
            // (a frame with nothing moved shows the same, drawing nothing again)
            assert_eq!(furthest(&drawn_as_a_frame_is(&s, takes_long), &second), 0);
            // let go: the strokes themselves, bent, as keeping it leaves them
            s.transform_release();
            drawn_as_a_frame_is(&s, takes_long);
            assert!(!pulled(&s));
            let exact = shown_pixels(&s, layer);
            s.transform_end(true);
            assert_eq!(furthest(&pixels(&s, layer), &exact), 0, "kept, it is what was shown");
            kept.push(pixels(&s, layer));
            assert_eq!(s.undo().as_deref(), Some("warp transform"));
            assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        }
        assert_eq!(furthest(&kept[0], &kept[1]), 0, "kept the same, however it was shown meanwhile");
        // ended with the handle still held (kept, then called off): nothing of the picture pulled
        // through the mesh is left
        let step = s.shared.lock().canvas.as_ref().unwrap().undo_step();
        for keep in [true, false] {
            assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
            s.transform_set_kind(crate::TransformKind::Warp);
            warp_frame(&s);
            s.transform_grab(crate::TransformHandle::Mesh { index: 5 }, 150.0, 150.0);
            s.transform_drag(190.0, 90.0);
            drawn_as_a_frame_is(&s, true);
            assert!(pulled(&s));
            s.transform_end(keep);
            assert!(s.shared.lock().vector_warp.is_none());
            if keep {
                assert_eq!(furthest(&pixels(&s, layer), &kept[0]), 0);
                assert_eq!(s.undo().as_deref(), Some("warp transform"));
            }
            assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        }
        s.shared.lock().canvas.as_mut().unwrap().forget_redo();
        assert_eq!(s.shared.lock().canvas.as_ref().unwrap().undo_step(), step);
    }

    #[test]
    fn a_warp_shows_the_strokes_bent_as_keeping_it_leaves_them() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        let before = pixels(&s, layer);
        let plain = frame(&s);
        // the whole layer warped: as a frame is drawn, the layer shows its strokes bent through
        // the mesh, each as wide as it was, which is just what keeping the warp leaves
        assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
        warp_up(&s);
        let shown_frame = warp_frame(&s);
        let shown = shown_pixels(&s, layer);
        assert!((64..76).any(|row| pixel(&shown, 150, row)[3] > 128), "ink where it is bent to");
        assert_eq!(pixel(&shown, 150, 100)[3], 0, "and none where it was");
        // (the mesh as it is, drawn already: another frame draws nothing again)
        assert!(!s.shared.lock().vector_warp_draw(true));
        s.transform_end(true);
        assert_eq!(furthest(&pixels(&s, layer), &shown), 0, "kept, it is what was shown");
        assert_eq!(furthest(&frame(&s), &shown_frame), 0);
        // one step, however many times it was drawn meanwhile
        assert_eq!(s.undo().as_deref(), Some("warp transform"));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        assert!(s.shared.lock().vector_warp.is_none());
        // called off: the layer as it was, with nothing of it left to undo or redo
        let step = s.shared.lock().canvas.as_ref().unwrap().undo_step();
        assert!(s.transform_begin().is_some());
        warp_up(&s);
        warp_frame(&s);
        assert_ne!(furthest(&shown_pixels(&s, layer), &before), 0);
        s.transform_end(false);
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        assert_eq!(s.shared.lock().canvas.as_ref().unwrap().undo_step(), step);
        assert_eq!(furthest(&frame(&s), &plain), 0);
        // the first line alone, picked up onto a layer of its own: shown bent there, the rest of
        // its layer whole under it
        s.select_shape(false, 30.0, 80.0, 70.0, 120.0, SelectionJoin::Add);
        assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
        warp_up(&s);
        let shown_frame = warp_frame(&s);
        assert_eq!(pixel(&shown_pixels(&s, layer), 150, 200), pixel(&before, 150, 200), "the line not picked");
        s.transform_end(true);
        let kept = pixels(&s, layer);
        assert!((64..76).any(|row| pixel(&kept, 150, row)[3] > 128));
        assert_eq!(furthest(&frame(&s), &shown_frame), 0, "kept, it is what was shown");
        // (a transform kept lets go of the selection, a step of its own)
        assert_eq!(s.undo().as_deref(), Some("clear selection"));
        assert_eq!(s.undo().as_deref(), Some("warp transform"));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        s.select_none();
        // the mesh taken back: the picture of the strokes is moved again, as in any other transform
        assert!(s.transform_begin().is_some());
        warp_up(&s);
        warp_frame(&s);
        assert!(s.shared.lock().vector_warp.is_some());
        s.transform_apply(crate::TransformAction::Reset);
        assert!(s.shared.lock().vector_warp.is_none());
        assert_eq!(furthest(&shown_pixels(&s, layer), &before), 0);
        // (and the mesh up again shows them bent again)
        warp_up(&s);
        warp_frame(&s);
        assert_eq!(furthest(&shown_pixels(&s, layer), &shown), 0);
        s.transform_end(true);
        assert_eq!(furthest(&pixels(&s, layer), &shown), 0);
    }

    #[test]
    fn a_warp_bends_the_strokes_and_fills_it_holds() {
        let Some(s) = session() else { return };
        let layer = two_lines_and_a_square(&s);
        let before = pixels(&s, layer);
        // where the `n`-th stroke's line is as it passes column 150
        let middle = |n: usize| {
            let st = s.shared.lock();
            let content = st.doc.layer(LayerId(layer)).unwrap().vector.clone().unwrap();
            let line = content.strokes().nth(n).unwrap().line();
            line.iter().min_by(|a, b| (a.x - 150.0).abs().total_cmp(&(b.x - 150.0).abs())).unwrap().y
        };
        // the first line picked and warped: it is the same line, bent
        s.select_shape(false, 30.0, 80.0, 70.0, 120.0, SelectionJoin::Add);
        assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
        warp_up(&s);
        s.transform_end(true);
        assert_eq!((s.vector_stroke_count(layer), s.vector_fill_count(layer)), (2, 1));
        let (first, last) = ends(&s, layer, 0);
        assert!((first.0 - 40.0).abs() < 3.0 && (first.1 - 100.0).abs() < 3.0, "{first:?}");
        assert!((last.0 - 260.0).abs() < 3.0 && (last.1 - 100.0).abs() < 3.0, "{last:?}");
        assert!((middle(0) - 70.0).abs() < 4.0, "{}", middle(0));
        let bent = pixels(&s, layer);
        assert!((64..76).any(|row| pixel(&bent, 150, row)[3] > 128), "ink where it was bent to");
        assert_eq!(pixel(&bent, 150, 100)[3], 0, "and none where it was");
        assert_eq!(pixel(&bent, 150, 200), pixel(&before, 150, 200), "the line not picked as it was");
        assert_eq!(pixel(&bent, 80, 150), pixel(&before, 80, 150));
        // drawn again from what the layer holds: the same picture
        assert!(s.redraw_vector_layer(layer));
        assert_eq!(furthest(&pixels(&s, layer), &bent), 0);
        s.undo();
        // (a transform kept lets go of the selection, a step of its own)
        assert_eq!(s.undo().as_deref(), Some("clear selection"));
        assert_eq!(s.undo().as_deref(), Some("warp transform"));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
        assert!((middle(0) - 100.0).abs() < 1.0);
        // with nothing picked the whole layer is warped: both lines bend, and the square between
        // them goes up with them, its top no longer straight
        s.select_none();
        assert!(s.transform_begin().is_some(), "{}", s.transform_refusal());
        warp_up(&s);
        s.transform_end(true);
        assert!((middle(0) - 70.0).abs() < 4.0 && (middle(1) - 170.0).abs() < 4.0, "{} {}", middle(0), middle(1));
        let all = pixels(&s, layer);
        // (near the box's side the square has hardly moved; further in it has gone up)
        let top_at =
            |x: usize| (60..200).find(|&row| pixel(&all, x, row)[0] > 200 && pixel(&all, x, row)[3] > 200).unwrap();
        assert!(top_at(52) >= 118, "{}", top_at(52));
        assert!(top_at(105) < top_at(52) - 8, "{} {}", top_at(105), top_at(52));
        assert_eq!(s.undo().as_deref(), Some("warp transform"));
        assert_eq!(furthest(&pixels(&s, layer), &before), 0);
    }

    #[test]
    fn vector_layers_go_out_as_svg_and_pdf_and_an_svg_comes_in_as_a_layer_of_strokes_and_fills() {
        let Some(s) = session() else { return };
        assert!(!s.exports_svg() && !s.exports_vectors(), "nothing to send yet");
        let layer = two_lines_and_a_square(&s);
        assert!(s.exports_svg() && s.exports_vectors());
        let dir = folder("vector-share");
        std::fs::create_dir_all(&dir).unwrap();
        let (svg, pdf) = (format!("{dir}/out.svg"), format!("{dir}/out.pdf"));
        s.export_svg(svg.clone()).unwrap();
        s.export_vector_pdf(pdf.clone()).unwrap();
        let text = std::fs::read_to_string(&svg).unwrap();
        assert!(text.contains("viewBox=\"0 0 300 300\"") && text.contains("<title>"), "{text}");
        assert_eq!(text.matches("<path").count(), 3, "two lines and a square: {text}");
        assert!(std::fs::read(&pdf).unwrap().starts_with(b"%PDF-1.4"));
        // read back in, it is a layer of its own with as much on it, drawn where it was
        let again = s.insert_svg(text.clone().into_bytes(), "Read Back".into());
        assert!(again != 0 && again != layer);
        assert!(s.is_vector_layer(again));
        // (its lines thin at their ends, so each went out as the outline of its ink: a shape)
        assert_eq!((s.vector_stroke_count(again), s.vector_fill_count(again)), (0, 3));
        assert_eq!(s.layers().iter().find(|l| l.id == again).unwrap().name, "Read Back");
        let (was, is) = (pixels(&s, layer), pixels(&s, again));
        assert_eq!(pixel(&is, 80, 150), pixel(&was, 80, 150), "the square");
        assert!(pixel(&is, 150, 100)[3] > 128 && pixel(&is, 150, 200)[3] > 128, "the lines");
        assert_eq!(pixel(&is, 150, 150)[3], 0);
        let ink = |rgba: &[u8]| rgba.as_chunks::<4>().0.iter().filter(|p| p[3] > 128).count() as f32;
        assert!((ink(&is) - ink(&was)).abs() < ink(&was) * 0.1, "{} {}", ink(&is), ink(&was));
        // one undo takes the layer away again
        assert_eq!(s.undo().as_deref(), Some("Insert SVG"));
        assert!(s.layers().iter().all(|l| l.id != again));
        // a picture of another size is fitted onto the canvas: a green box in its top left quarter
        // with a blue line round it ten wide, and a curve
        let picture = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 600 600">
            <g opacity="0.5"><rect x="100" y="100" width="200" height="160" fill="#00ff00" stroke="blue" stroke-width="20"/></g>
            <path d="M100 500 C 200 300, 400 300, 500 500" fill="none" stroke="#ff0000" stroke-width="8"/>
            <text x="10" y="20">not drawn</text>
        </svg>"##;
        let read = s.insert_svg(picture.as_bytes().to_vec(), String::new());
        assert!(read != 0);
        assert_eq!((s.vector_stroke_count(read), s.vector_fill_count(read)), (2, 1));
        let shown = pixels(&s, read);
        // (half as big: the box from 50 to 150 across, 50 to 130 down, its line ten wide)
        let inside = pixel(&shown, 100, 90);
        assert!(inside[1] > 100 && inside[0] < 10 && (inside[3] as i32 - 128).abs() < 6, "{inside:?}");
        let edge = pixel(&shown, 100, 50);
        assert!(edge[2] > 100 && edge[1] < 80, "{edge:?}");
        assert_eq!(pixel(&shown, 100, 40)[3], 0, "outside the line round it");
        assert!(pixel(&shown, 100, 46)[3] > 60 && pixel(&shown, 100, 54)[3] > 60, "the line ten wide");
        // (the curve's top is three quarters of the way to its handles: 350 of 600, so 175)
        let top = (0..300).find(|&row| pixel(&shown, 150, row)[0] > 200 && pixel(&shown, 150, row)[3] > 200).unwrap();
        assert!((top as i32 - 173).abs() <= 3, "{top}");
        // what can't be read gives nothing
        assert_eq!(s.insert_svg(b"not a picture".to_vec(), String::new()), 0);
        assert_eq!(
            s.insert_svg(
                b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"10\" height=\"10\"/>".to_vec(),
                String::new()
            ),
            0
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Run with `--ignored --nocapture`: what a drag of a vector layer's transform costs a frame, on
    /// a canvas a tablet's size: the whole layer moved, some of its strokes picked up and moved,
    /// and its mesh pulled; with a raster layer of the same picture to compare with.
    #[test]
    #[ignore]
    fn transform_lag() {
        let Ok(gpu) = Gpu::headless() else { return };
        let (w, h) = (2048u32, 2732u32);
        let s = CanvasSession::new(w, h);
        {
            let mut st = s.shared.lock();
            st.window_size = (w, h);
            st.view = Some(View::fit((w, h), (w, h)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        let target = {
            let st = s.shared.lock();
            st.gpu.as_ref().unwrap().device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Bgra8UnormSrgb,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            })
        };
        let view = target.create_view(&Default::default());
        let started = std::time::Instant::now();
        let draw = |s: &CanvasSession| {
            let mut st = s.shared.lock();
            let srgb = procraft_engine::profile::Profile::Srgb;
            st.draw_frame(&view, wgpu::TextureFormat::Bgra8UnormSrgb, (w, h), srgb, started);
            st.needs_frame
        };
        let flush = |s: &CanvasSession| {
            if let Some(g) = s.shared.lock().gpu.clone() {
                g.flush();
            }
        };
        let ms = |t: std::time::Instant| t.elapsed().as_secs_f64() * 1000.0;
        let strokes: usize = std::env::var("LAG_STROKES").ok().and_then(|n| n.parse().ok()).unwrap_or(60);
        let paint = |s: &CanvasSession| {
            s.set_color(0.0, 0.0, 0.0);
            s.set_brush_size(0.2);
            for k in 0..strokes {
                let y = 200.0 + (k as f32) * (2300.0 / strokes as f32);
                straight(s, (150.0, y), (1900.0, y + 120.0));
            }
        };
        for kind in ["raster", "vector", "vector selected", "vector warp", "raster warp"] {
            if kind.starts_with("vector") {
                s.add_vector_layer();
            } else {
                s.add_layer();
            }
            paint(&s);
            if kind == "vector selected" {
                s.select_shape(false, 100.0, 150.0, 400.0, 1300.0, SelectionJoin::Add);
            }
            draw(&s);
            flush(&s);
            let t = std::time::Instant::now();
            assert!(s.transform_begin().is_some(), "{kind}: {}", s.transform_refusal());
            if kind.ends_with("warp") {
                s.transform_set_kind(crate::TransformKind::Warp);
            }
            draw(&s);
            flush(&s);
            let begun = ms(t);
            s.transform_grab(crate::TransformHandle::Move, 1000.0, 1300.0);
            let (mut drags, mut draws, mut wholes, mut again) = (Vec::new(), Vec::new(), Vec::new(), 0);
            for i in 0..30 {
                let whole = std::time::Instant::now();
                let t = std::time::Instant::now();
                s.transform_drag(1000.0 + i as f32 * 6.0, 1300.0 + i as f32 * 4.0);
                drags.push(ms(t));
                let t = std::time::Instant::now();
                // (a warp drawn each time, however long the last took, to see how long that is)
                if std::env::var("LAG_WARP_NOW").is_ok() {
                    s.shared.lock().vector_warp_draw(true);
                }
                again += usize::from(draw(&s));
                draws.push(ms(t));
                flush(&s);
                wholes.push(ms(whole));
            }
            let t = std::time::Instant::now();
            s.transform_release();
            draw(&s);
            flush(&s);
            let released = ms(t);
            let t = std::time::Instant::now();
            s.transform_end(true);
            draw(&s);
            flush(&s);
            let ended = ms(t);
            let mid = |v: &mut Vec<f64>| {
                v.sort_by(f64::total_cmp);
                (v[v.len() / 2], v[v.len() - 1])
            };
            let (drag, draw_ms, whole) = (mid(&mut drags), mid(&mut draws), mid(&mut wholes));
            println!(
                "{kind:<16} begin {begun:>7.1}  drag {:>6.2} (worst {:>6.2})  frame {:>6.2} (worst {:>6.2})  with gpu {:>6.2} (worst {:>6.2})  put off {again:>2}  release {released:>7.1}  end {ended:>7.1}",
                drag.0, drag.1, draw_ms.0, draw_ms.1, whole.0, whole.1
            );
            s.select_none();
        }
    }
}

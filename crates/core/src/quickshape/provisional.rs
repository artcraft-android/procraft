//! A heavy shape's stroke (wet paint, paint laid in place, a second) shown while it is moved about:
//! laid whole each frame, wet mixing and second and all, on a canvas a k-th of the canvas's size, k
//! about as many canvas pixels as a screen pixel shows, over its layer as it was, and shown in the
//! layer from there (see `procraft_gpu`'s `provisional.rs`); the canvas's own stroke let go of
//! meanwhile. Laid again at each move at the canvas's resolution, such a shape trailed its handle
//! (a big wet ellipse by seventy pixels on a tablet, a move shown every 30 ms); at a k-th of the
//! pixels every frame shows the latest move. Once it stops moving it shows so until the stroke made
//! for where it stopped is to be had; then the layer is put back as it was and that laid at the
//! canvas's resolution once, as it always was, the picture held until it is laid. Kept, or redrawn
//! for another reason, it is laid so at once.

use procraft_engine::document::{Document, LayerId};
use procraft_engine::geom::IRect;
use procraft_engine::stroke::Stamp;
use procraft_gpu::{Canvas, SecondStroke, StrokeStyle};

use super::{Made, ShapePlots};
use crate::State;

/// The most canvas pixels a pixel of the smaller canvas covers each way.
const MOST_K: u32 = 8;

/// The smaller canvas a moving shape's stroke is laid on (see the module).
pub(super) struct ShapeProvisional {
    canvas: Box<Canvas>,
    doc: Document,
    layer: LayerId,
    /// The stroke as laid there (its images its own), and whether it is begun there.
    style: StrokeStyle,
    second: Option<SecondStroke>,
    begun: bool,
    /// The canvas's stroke as it was begun, to begin again: its layer, style, second and width.
    main: (LayerId, StrokeStyle, Option<SecondStroke>, Option<f32>),
}

impl State {
    /// How many canvas pixels a pixel of the smaller canvas covers each way: about as many as a
    /// screen pixel shows (none fewer than one, nor more than [`MOST_K`]).
    fn provisional_k(&self) -> u32 {
        let fit = procraft_engine::view::View::fit((self.doc.width(), self.doc.height()), self.window_size).zoom();
        let zoom = self.view.map_or(fit, |v| v.zoom());
        if zoom > 0.0 && zoom.is_finite() { ((1.0 / zoom).round() as u32).clamp(1, MOST_K) } else { 1 }
    }

    /// Whether the shape's stroke is shown from a smaller canvas now: while it is moved about, of a
    /// brush whose stroke is heavy to lay again (painting in place, or with a second), not a brush
    /// engine's (it draws its own), nor a plugin layer's, nor one laid into a layer's store.
    fn provisional_wanted(&self) -> bool {
        let Some(edit) = self.shape.as_ref() else { return false };
        let Some(stroke) = edit.stroke.as_ref() else { return false };
        let engine = self.stroke.as_ref().is_none_or(|b| b.is_outside());
        edit.moving()
            && !edit.provisional_refused
            && !engine
            && self.recording.is_none()
            && stroke.style.store == 0
            && (stroke.style.in_place() || stroke.second.is_some())
    }

    /// This frame's part in showing the shape's stroke from a smaller canvas: begun as it starts
    /// moving, once there is a stroke to lay there (the stroke laid here shown till then), laid there
    /// for where it is now, and once it stops, shown so until the stroke made for where it stopped is
    /// laid here in its place. No frame shows neither: each showed only the handles for a tenth of a
    /// second or more on a tablet, as it began moving and as it stopped. True if the frame shows it
    /// so (or the stroke laid here as it was), false to lay it as itself.
    pub(super) fn provisional_frame(&mut self) -> bool {
        let shown = self.shape.as_ref().is_some_and(|e| e.provisional.is_some());
        if !self.provisional_wanted() {
            return shown && self.finish_provisional_shape();
        }
        let Some((first, second)) = self.provisional_stamps() else {
            // (nothing to lay there yet: what shows stays)
            return true;
        };
        if !shown && !self.begin_provisional_shape() {
            // (laid as itself, where it is now)
            if let Some(edit) = self.shape.as_mut() {
                edit.stale = true;
            }
            return false;
        }
        self.lay_provisional(first, second);
        true
    }

    /// The shape's stamps for where it is now, if they are to be had this frame (and are new): a
    /// stroke made for it elsewhere, or the stamps made before moved along (spaced afresh elsewhere
    /// once the move stretches them far: shown stretched till then, every frame its latest move).
    fn provisional_stamps(&mut self) -> Option<(Vec<Stamp>, Option<Vec<Stamp>>)> {
        let mut stamps = None;
        let waited = self.shape.as_ref().and_then(|e| e.replot.as_ref()).map(|r| r.made.try_recv());
        match waited {
            Some(Ok(made)) => {
                // (made for a move since gone past: its stamps moved along to the latest)
                let latest = self.shape.as_ref().is_some_and(|e| e.moves == made.moves);
                let made = self.adopt_made(made);
                if let Some(edit) = self.shape.as_mut() {
                    edit.replot = None;
                    edit.stale |= !latest;
                }
                stamps = latest.then_some(made);
            }
            Some(Err(std::sync::mpsc::TryRecvError::Disconnected)) => {
                if let Some(edit) = self.shape.as_mut() {
                    (edit.replot, edit.stale) = (None, true);
                }
            }
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) | None => {}
        }
        if self.shape.as_ref().is_some_and(|e| e.stale) {
            let near = self.moved_shape_stamps();
            let moved = near.clone().or_else(|| self.moved_shape_stamps_by(f32::INFINITY));
            if near.is_none() && self.shape.as_ref().is_some_and(|e| e.replot.is_none()) {
                if let Some(made) = self.make_shape_elsewhere() {
                    stamps = Some(self.adopt_made(made));
                }
                self.needs_frame = true;
            }
            if let Some(moved) = moved {
                let replot = self.shape.as_mut().and_then(|e| e.replot.take());
                stamps = Some(moved);
                if let Some(edit) = self.shape.as_mut() {
                    edit.stale = false;
                    // (a stroke still being made for it spaces it afresh once made)
                    edit.replot = replot;
                    edit.laid_moves = edit.moves;
                    edit.moved_plots = true;
                }
            } else {
                self.needs_frame = true;
            }
        }
        stamps
    }

    /// The shape stopped moving while shown from a smaller canvas: shown so until the stroke made for
    /// where it stopped is to be had, then that laid here in its place (the picture held as it was
    /// until it is laid, as a stroke laid again is). True: the frame is done with the shape.
    fn finish_provisional_shape(&mut self) -> bool {
        let waited = self.shape.as_ref().and_then(|e| e.replot.as_ref()).map(|r| r.made.try_recv());
        let made = match waited {
            Some(Ok(made)) if self.shape.as_ref().is_some_and(|e| e.moves == made.moves) => Some(made),
            Some(Err(std::sync::mpsc::TryRecvError::Empty)) => {
                self.needs_frame = true;
                return true;
            }
            // (none being made, or made for a move gone past: made afresh)
            _ => {
                if let Some(edit) = self.shape.as_mut() {
                    edit.replot = None;
                }
                self.make_shape_elsewhere()
            }
        };
        let Some(made) = made else {
            self.needs_frame = true;
            return true;
        };
        self.end_provisional_shape();
        if let Some(edit) = self.shape.as_mut() {
            (edit.stale, edit.replot) = (false, None);
        }
        self.lay_made(made);
        true
    }

    /// The shape's stroke can't be shown from a smaller canvas: it is laid as itself while it moves.
    fn refuse_provisional(&mut self) {
        log::warn!("a moving shape's stroke couldn't be shown from a smaller canvas");
        if let Some(edit) = self.shape.as_mut() {
            edit.provisional_refused = true;
        }
    }

    /// Takes the stroke `made` for the shape as its own without laying it: kept to move along, its
    /// builders the stroke's. Its stamps.
    fn adopt_made(&mut self, made: Made) -> (Vec<Stamp>, Option<Vec<Stamp>>) {
        let Made { first: (builder, stamps), second, moves, shape, scale } = made;
        let (second, second_stamps) = match second {
            Some((builder, stamps)) => (Some(builder), Some(stamps)),
            None => (None, None),
        };
        if let Some(edit) = self.shape.as_mut() {
            edit.laid_moves = moves;
            edit.moved_plots = false;
            edit.plots = Some(ShapePlots { shape, scale, primary: stamps.clone(), secondary: second_stamps.clone() });
        }
        self.stroke = Some(Box::new(builder));
        self.second = second.map(Box::new);
        (stamps, second_stamps)
    }

    /// Begins showing the shape's stroke from a smaller canvas, if it isn't yet: the canvas made, the
    /// stroke's images taken there, the canvas's own stroke let go of. False if it can't be.
    fn begin_provisional_shape(&mut self) -> bool {
        if self.shape.as_ref().is_some_and(|e| e.provisional.is_some()) {
            return true;
        }
        let Some(gpu) = self.gpu.clone() else { return false };
        let k = self.provisional_k();
        let mut doc = Document::new(self.doc.width().div_ceil(k), self.doc.height().div_ceil(k));
        let Ok(mut small) = Canvas::new(&gpu, &doc) else {
            self.refuse_provisional();
            return false;
        };
        // (its layer made)
        small.render(&gpu, &mut doc, None);
        let small_layer = doc.current();
        small.draw_scaled([0.0, 0.0], 1.0 / k as f32);
        let Some(stroke) = self.shape.as_ref().and_then(|e| e.stroke.as_ref()) else { return false };
        let (layer, style, second) = (stroke.layer, stroke.style, stroke.second.as_ref().map(|s| s.stroke));
        // the stroke's shapes and grains as the smaller canvas holds them (a brush file's own at
        // their own size aren't there: its built-in ones)
        let mut small_style = StrokeStyle { full_grain: None, full_shape: None, ..style };
        if let Some(builder) = self.stroke.as_ref() {
            small.use_sources(&gpu, &mut small_style, builder.brush());
        }
        let small_second = second.map(|mut s| {
            s.style.full_grain = None;
            s.style.full_shape = None;
            if let Some(builder) = self.second.as_ref() {
                small.use_sources(&gpu, &mut s.style, builder.brush());
            }
            s
        });
        let Some(main) = self.canvas.as_mut() else { return false };
        let across = main.told_brush_across();
        main.set_prediction(&[]);
        main.set_tip(&[]);
        main.cancel_stroke();
        if !main.begin_provisional(&gpu, layer, (&mut small, small_layer), k) {
            begin_again(main, (layer, style, second, across));
            self.refuse_provisional();
            return false;
        }
        if let Some(across) = across {
            small.brush_across(across / k as f32);
        }
        if let Some(edit) = self.shape.as_mut() {
            edit.provisional = Some(Box::new(ShapeProvisional {
                canvas: Box::new(small),
                doc,
                layer: small_layer,
                style: small_style,
                second: small_second,
                begun: false,
                main: (layer, style, second, across),
            }));
        }
        true
    }

    /// Lays the shape's stroke, its stamps `first` and its second's `second`, whole on the smaller
    /// canvas and shows it from there.
    fn lay_provisional(&mut self, first: Vec<Stamp>, second: Option<Vec<Stamp>>) {
        let Some(gpu) = self.gpu.clone() else { return };
        // with a symmetry guide on, its copies too, as the stroke had them and as it is kept
        let guide = self.doc.stroke_guide();
        let first = crate::guide::mirrored(&guide, &first).into_owned();
        let second = second.map(|second| crate::guide::mirrored(&guide, &second).into_owned());
        let Some(mut p) = self.shape.as_mut().and_then(|e| e.provisional.take()) else { return };
        if !p.begun {
            match p.second {
                Some(s) => p.canvas.begin_dual_stroke(p.layer, p.style, s),
                None => p.canvas.begin_stroke(p.layer, p.style),
            }
            p.begun = true;
        }
        // a k-th of the stamps, each as strong as the k it stands for: laid k times smaller, they
        // stand as close in its pixels as the stroke's in the canvas's (all of them, a whole
        // shape's thousands, took a tablet as long as laying it again whole here)
        let k = self.doc.width().div_ceil(p.doc.width()).max(1) as usize;
        let (thin_first, thin_second) = (thinned(&first, k), second.as_deref().map(|s| thinned(s, k)));
        p.canvas.replace_shape_stamps(&thin_first, thin_second.as_deref());
        p.canvas.lay_held();
        p.canvas.render(&gpu, &mut p.doc, None);
        // what it may have changed, a pixel past its stamps' reach, and the smaller canvas's pixel
        let reach = |s: &Stamp| {
            let r = s.radius * s.roundness.max(1.0) * std::f32::consts::SQRT_2 + 2.0;
            IRect::around(s.center, r)
        };
        let k = k as i32;
        let r = first.iter().chain(second.iter().flatten()).fold(IRect::EMPTY, |r, s| r.union(reach(s)));
        let r = if r.is_empty() { r } else { IRect::new(r.x0 - k, r.y0 - k, r.x1 + k, r.y1 + k) };
        if let Some(main) = self.canvas.as_mut() {
            main.show_provisional((&p.canvas, p.layer), r);
        }
        if let Some(edit) = self.shape.as_mut() {
            edit.provisional = Some(p);
        }
    }

    /// Ends showing the shape's stroke from a smaller canvas, if it is: the layer put back as it was
    /// and the canvas's stroke begun again, its stroke to be laid in it. Whether it was shown.
    pub(super) fn end_provisional_shape(&mut self) -> bool {
        let Some(p) = self.shape.as_mut().and_then(|e| e.provisional.take()) else { return false };
        if let Some(main) = self.canvas.as_mut() {
            main.end_provisional();
            begin_again(main, p.main);
        }
        self.needs_frame = true;
        true
    }
}

/// Every `k`th of `stamps` (and the last), each as strong as the `k` laid over each other it stands for.
fn thinned(stamps: &[Stamp], k: usize) -> Vec<Stamp> {
    if k <= 1 {
        return stamps.to_vec();
    }
    let last = stamps.len().saturating_sub(1);
    stamps
        .iter()
        .enumerate()
        .filter(|&(i, _)| i % k == 0 || i == last)
        .map(|(_, s)| Stamp { alpha: 1.0 - (1.0 - s.alpha.clamp(0.0, 1.0)).powi(k as i32), ..*s })
        .collect()
}

/// `main`'s stroke begun again as it was begun: on its layer, in its style, with its second, as wide.
fn begin_again(
    main: &mut Canvas,
    (layer, style, second, across): (LayerId, StrokeStyle, Option<SecondStroke>, Option<f32>),
) {
    match second {
        Some(second) => main.begin_dual_stroke(layer, style, second),
        None => main.begin_stroke(layer, style),
    }
    if let Some(across) = across {
        main.brush_across(across);
    }
}

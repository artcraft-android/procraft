//! Vector layers shown sharp when zoomed in.
//!
//! A vector layer's pixels are the canvas's, so magnified they would show as a magnified picture
//! of its strokes. Zoomed in, the part of the artwork the window shows is made again at the
//! window's own pixels on a canvas of its own: every other layer as the window would show it
//! (magnified), each vector layer drawn again from what it holds, as fine as the screen. That
//! canvas is shown in place of the artwork's, and kept up with it:
//!
//! - a stroke being drawn is drawn on it too, stamp for stamp as it is made, so the picture stays
//!   sharp under the pen;
//! - a lasso's fill shown as it is drawn is shown on it too, where it lies there;
//! - anything else that changes the picture has it brought up to date before the next frame;
//! - while the view moves it goes on showing where that part of the artwork now is, over the
//!   artwork's own canvas, until the view rests and one is made for where it rests.
//!
//! The artwork itself is never touched by it.

use std::time::{Duration, Instant};

use procraft_engine::brush::Brush;
use procraft_engine::document::{Document, LayerId};
use procraft_engine::geom::{Affine, Homography, IRect, Vec2};
use procraft_engine::profile::Profile;
use procraft_engine::stroke::Stamp;
use procraft_engine::vector::{Object, StrokeId, VectorContent, placed};
use procraft_gpu::{Canvas, Gpu, GpuError, LassoPreview, SecondStroke, StrokeEcho, StrokeStyle, StrokeTool};

use crate::grain_store::GrainStore;
use crate::{State, ToolKind};

/// How long the view rests before the sharp picture is made for where it rests (and how long
/// after a stroke drawn on it ends before it is made again whole).
pub(crate) const REST: Duration = if cfg!(test) { Duration::ZERO } else { Duration::from_millis(180) };

/// The most the sharp picture's canvas may take, in bytes: past it the artwork shows as it is.
const MOST_BYTES: u64 = 512 << 20;

/// How far past the window's edges the sharp picture is made, of the window's size each side: as
/// far as there is room for, the first of these that fits. A quarter keeps a pan of that much, or
/// a zoom out to two thirds, sharp while it moves.
const MARGINS: [f32; 3] = [0.25, 0.1, 0.0];

/// What a canvas holds besides its layers, in layers' worth (its stroke, composite and caches).
const CANVAS_LAYERS: u64 = 7;

/// The view a sharp picture is of: the artwork's pixels to the window's, and the window's size.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CrispView {
    view: Affine,
    size: (u32, u32),
}

/// The picture a sharp one was made of: while this is the same, so is what the artwork shows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CrispPicture {
    composites: u64,
    step: Option<u64>,
    revision: u64,
}

/// The part of the artwork the window shows, at the window's pixels.
pub(crate) struct Crisp {
    pub(crate) doc: Document,
    pub(crate) canvas: Canvas,
    /// Its pixels to the window's, for the view it was made of.
    pub(crate) to_screen: Affine,
    /// The artwork's pixels to its own.
    pub(crate) to_crisp: Affine,
    at: CrispView,
    picture: CrispPicture,
    /// The layers it has, as the artwork had them: with others, it is made anew.
    layers: Vec<LayerId>,
    /// Layers of pixels are magnified as squares (the artwork's pixels show as squares there).
    squares: bool,
    /// A stroke was drawn on it as it was made, and ended then: it is made again whole once things
    /// have rested, to be sure it is what the layers hold.
    owed: Option<Instant>,
    /// The lasso's fill the artwork's canvas showed as it was drawn when that was last carried
    /// onto this one (in the artwork's pixels), to carry it again only when it has changed.
    lasso: Option<LassoPreview>,
    /// The stroke being drawn on it has a fill kept under it on its vector layer (Lasso Fill's
    /// Outline), which only drawing that layer again shows: done as the stroke ends, not waited for.
    pub(crate) filled_under: bool,
}

/// How a frame shows the sharp picture.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CrispShown {
    /// Not at all: the artwork's canvas as it is.
    No,
    /// In place of the artwork's canvas.
    Whole,
    /// Over the artwork's canvas, its pixels carried to the window's by this: the view has moved
    /// since it was made.
    Over(Affine),
}

/// `style` made ready to draw `brush` on `canvas`: the brush's shape and grain as that canvas keeps
/// them (a style is ready for one canvas only).
fn ready(canvas: &mut Canvas, gpu: &Gpu, grains: &mut GrainStore, style: &mut StrokeStyle, brush: &Brush) {
    canvas.use_sources(gpu, style, brush);
    if let Some(full) = brush.grain_file_now().and_then(|f| f.full.as_ref())
        && let Some(pixels) = grains.pixels(full)
    {
        canvas.use_full_grain(gpu, style, full.id, full.side, &pixels);
    }
    if let Some(full) = brush.shape_file_now().map(|f| &f.full)
        && let Some(pixels) = grains.pixels(full)
    {
        canvas.use_full_shape(gpu, style, full.id, full.side, &pixels);
    }
}

impl State {
    /// Whether the sharp picture is wanted at all: on, zoomed in, and a vector layer with
    /// something on it showing on a canvas that can be shown that way.
    fn crisp_on(&self, screen: Profile) -> bool {
        self.vector_crisp
            && self.pad_strokes.is_none()
            && self.view.is_some_and(|v| v.zoom() > 1.02)
            && self.canvas.as_ref().is_some_and(|c| !c.busy() && c.play_frame().is_none())
            && self.doc.pixel_layer_ids().any(|id| self.crisp_layer(id).is_some())
            // (a canvas shown through a colour table is shown as it is: the table is its canvas's)
            && crate::colour::table_key(&self.doc, screen).is_none()
    }

    /// Whether something is under way that the sharp picture can't show: what is shown moving or
    /// changing is on the artwork's canvas alone (a transform, an adjustment, a fill held, a
    /// stroke not being drawn on the sharp picture too).
    fn crisp_busy(&self) -> bool {
        self.transform.is_some()
            || self.adjusting
            || self.text_edit.is_some()
            || self.shape.is_some()
            || self.pending_doc.is_some()
            || self.feather_base.is_some()
            || self.selection_before.is_some()
            || self.pour.is_some()
            || self.reveal.is_some()
            || self.plugin_artwork.as_ref().is_some_and(|a| a.down)
            || (self.stroke.is_some() && self.crisp_stroke.is_none())
    }

    /// What vector layer `id` holds, if it is one that shows and holds anything.
    fn crisp_layer(&self, id: LayerId) -> Option<&std::sync::Arc<VectorContent>> {
        let content = self.doc.layer(id)?.vector.as_ref()?;
        (!content.is_empty() && self.doc.unseen(id).is_none()).then_some(content)
    }

    /// What the artwork shows now, all that was waiting to be drawn drawn.
    fn crisp_picture(&mut self) -> Option<CrispPicture> {
        self.flush();
        let canvas = self.canvas.as_ref()?;
        Some(CrispPicture { composites: canvas.composites(), step: canvas.undo_step(), revision: self.revision })
    }

    /// The sharp picture let go of, with any stroke being drawn on it.
    pub(crate) fn crisp_drop(&mut self) {
        self.crisp = None;
        self.crisp_wait = None;
        if self.crisp_stroke.take().is_some()
            && let Some(canvas) = self.canvas.as_mut()
        {
            canvas.stop_echo();
        }
    }

    /// How this frame shows the sharp picture ([`State::crisp`]), brought up to date first: in
    /// place of the artwork's canvas while it is of the view as it is; over it while the view
    /// moves; not at all while there is none (one is made once the view has rested a moment, a
    /// frame asked for then) or something is under way that it can't show.
    pub(crate) fn crisp_frame(&mut self, size: (u32, u32), screen: Profile) -> CrispShown {
        self.crisp_due = None;
        if !self.crisp_on(screen) {
            self.crisp_drop();
            return CrispShown::No;
        }
        // the stroke being drawn, as far as it has come
        self.crisp_echo();
        let (Some(view), Some(picture)) = (self.view, self.crisp_picture()) else { return CrispShown::No };
        let view = CrispView { view: view.to_screen(), size };
        let now = Instant::now();
        let Some(mut crisp) = self.crisp.take() else { return self.crisp_rested(view, picture, now) };
        if crisp.at != view {
            // the view has moved: a stroke being drawn on it goes on on the artwork alone
            if self.crisp_stroke.take().is_some() {
                crisp.canvas.cancel_stroke();
                if let Some(canvas) = self.canvas.as_mut() {
                    canvas.stop_echo();
                }
            }
            // what it shows is still the picture: it goes on showing, where that now is, until
            // one is made for where the view rests
            let still = crisp.picture == picture
                && !self.crisp_busy()
                && self.selection.is_none()
                && self.backdrops.is_empty()
                && self.backdrop_windows.is_empty();
            let over = crisp.to_crisp.inverse().map(|back| back.then(view.view)).filter(|_| still);
            // (kept only while it is still the picture: it is no use once that has changed)
            self.crisp = (crisp.picture == picture).then_some(crisp);
            return match self.crisp_rested(view, picture, now) {
                CrispShown::No => over.map_or(CrispShown::No, CrispShown::Over),
                shown => shown,
            };
        }
        self.crisp_wait = None;
        if self.crisp_stroke.is_some() {
            self.crisp = Some(crisp);
            return CrispShown::Whole;
        }
        if self.crisp_busy() {
            self.crisp = Some(crisp);
            return CrispShown::No;
        }
        // the picture changed some other way, or a stroke drawn on it has rested: made again
        let again =
            crisp.picture != picture || crisp.owed.is_some_and(|since| now.duration_since(since) >= self.crisp_rest);
        if again {
            match self.crisp_redo(crisp, None) {
                Some(made) => crisp = Crisp { picture, owed: None, ..made },
                None => return self.crisp_rested(view, picture, now),
            }
        } else if let Some(since) = crisp.owed {
            self.crisp_due = Some(since + self.crisp_rest);
        }
        self.crisp = Some(crisp);
        CrispShown::Whole
    }

    /// With no sharp picture of `view`: one is made once the view has rested there a moment with
    /// nothing under way (a frame is asked for when that is due), and shown.
    fn crisp_rested(&mut self, view: CrispView, picture: CrispPicture, now: Instant) -> CrispShown {
        if self.crisp_busy() || self.stroke.is_some() || self.view_motion.is_some() {
            self.crisp_wait = None;
            return CrispShown::No;
        }
        if self.crisp_refused == Some((view, picture)) {
            return CrispShown::No;
        }
        match self.crisp_wait {
            Some((waiting, since)) if waiting == view => {
                if now.duration_since(since) >= self.crisp_rest {
                    self.crisp_wait = None;
                    self.crisp = self.crisp_build(view, picture);
                    if self.crisp.is_none() {
                        self.crisp_refused = Some((view, picture));
                        return CrispShown::No;
                    }
                    return CrispShown::Whole;
                }
                self.crisp_due = Some(since + self.crisp_rest);
            }
            _ => {
                self.crisp_wait = Some((view, now));
                self.crisp_due = Some(now + self.crisp_rest);
            }
        }
        CrispShown::No
    }

    /// The sharp picture of the artwork as `view` shows it, and some way past the window's edges
    /// where there is room for it (a pan or a zoom out shows that much more of it, sharp, until
    /// one is made for where the view rests); none if it can't be made (the canvas turned with its
    /// edges in sight, too much to hold, a layer not on the GPU).
    fn crisp_build(&mut self, view: CrispView, picture: CrispPicture) -> Option<Crisp> {
        let gpu = self.gpu.clone()?;
        let to_screen = view.view;
        let back = to_screen.inverse()?;
        let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
        let (ww, wh) = (view.size.0 as f32, view.size.1 as f32);
        // the window's pixels the canvas lies on
        let corners = [(0.0, 0.0), (w, 0.0), (0.0, h), (w, h)].map(|(x, y)| to_screen.apply(Vec2::new(x, y)));
        let low = |v: fn(&Vec2) -> f32| corners.iter().map(v).fold(f32::INFINITY, f32::min);
        let high = |v: fn(&Vec2) -> f32| corners.iter().map(v).fold(f32::NEG_INFINITY, f32::max);
        // turned, those pixels are all canvas only if what is made lies wholly on it
        let upright = (to_screen.b.abs() < 1e-4 && to_screen.c.abs() < 1e-4)
            || (to_screen.a.abs() < 1e-4 && to_screen.d.abs() < 1e-4);
        let on_canvas = |x: f32, y: f32| {
            let p = back.apply(Vec2::new(x, y));
            p.x >= 0.0 && p.y >= 0.0 && p.x <= w && p.y <= h
        };
        let layers: Vec<LayerId> = self.doc.pixel_layer_ids().collect();
        // the most past the window it can be, of the window's size each side; else just the window
        let fits = MARGINS.iter().find_map(|&m| {
            let (mx, my) = ((ww * m).round(), (wh * m).round());
            let (x0, y0) = (low(|p| p.x).floor().max(-mx), low(|p| p.y).floor().max(-my));
            let (x1, y1) = (high(|p| p.x).ceil().min(ww + mx), high(|p| p.y).ceil().min(wh + my));
            if !(x1 > x0 && y1 > y0) {
                return None;
            }
            let around = [(x0, y0), (x1, y0), (x0, y1), (x1, y1)];
            if !upright && !around.into_iter().all(|(x, y)| on_canvas(x, y)) {
                return None;
            }
            let (cw, ch) = ((x1 - x0) as u32, (y1 - y0) as u32);
            let bytes = (layers.len() as u64 + CANVAS_LAYERS) * u64::from(cw) * u64::from(ch) * 4;
            (bytes <= MOST_BYTES && !crate::device::short_of_memory(bytes)).then_some((x0, y0, cw, ch))
        });
        let (x0, y0, cw, ch) = fits?;
        let to_crisp = to_screen.then(Affine::translate(Vec2::new(-x0, -y0)));
        let doc = self.doc.resized(cw, ch);
        let made = gpu.checked(|| -> Result<Canvas, GpuError> {
            let mut canvas = Canvas::unfilled(&gpu, &doc)?;
            canvas.fill_rest(&gpu, &doc);
            // (its strokes as wide as the artwork's: their rims counted in the artwork's pixels)
            canvas.set_artwork_pixel(to_screen.scale_factor());
            Ok(canvas)
        });
        let canvas = match made.and_then(|made| made) {
            Ok(canvas) => canvas,
            Err(e) => {
                log::info!("no sharp picture: {e}");
                return None;
            }
        };
        let crisp = Crisp {
            doc,
            canvas,
            to_screen: Affine::translate(Vec2::new(x0, y0)),
            to_crisp,
            at: view,
            picture,
            layers,
            squares: to_screen.scale_factor() >= self.display_scale,
            owed: None,
            lasso: None,
            filled_under: false,
        };
        self.crisp_redo(crisp, None)
    }

    /// `crisp` brought up to what the artwork's layers hold now: every layer of pixels that shows
    /// magnified again, every vector layer that shows drawn again; or (`only`) just that layer.
    /// None if it can't be (the artwork has other layers than it was made with, a layer is not on
    /// the GPU, the GPU is lost): it is then to be made anew.
    fn crisp_redo(&mut self, mut crisp: Crisp, only: Option<LayerId>) -> Option<Crisp> {
        let gpu = self.gpu.clone()?;
        if !self.doc.pixel_layer_ids().eq(crisp.layers.iter().copied()) {
            return None;
        }
        let (cw, ch) = (crisp.doc.width(), crisp.doc.height());
        let from_crisp = crisp.to_crisp.inverse()?;
        // the layers as they are now (shown or hidden, how they blend), on its own size
        crisp.doc = self.doc.resized(cw, ch);
        let wanted = |id: &LayerId| only.is_none_or(|only| only == *id);
        // (the vector layers that show are drawn; every other layer that shows is magnified as the
        // window would show it, in squares from where the artwork's pixels show as squares)
        let drawn: Vec<(LayerId, std::sync::Arc<VectorContent>)> = self
            .doc
            .pixel_layer_ids()
            .filter(wanted)
            .filter_map(|id| Some((id, std::sync::Arc::clone(self.crisp_layer(id)?))))
            .collect();
        let magnified: Vec<LayerId> = self
            .doc
            .pixel_layer_ids()
            .filter(wanted)
            .filter(|id| self.doc.unseen(*id).is_none() && self.crisp_layer(*id).is_none())
            .collect();
        let main = self.canvas.as_ref()?;
        let (m, squares) = (from_crisp, crisp.squares);
        let canvas = &mut crisp.canvas;
        let went = gpu.checked(|| {
            magnified
                .iter()
                .all(|id| canvas.magnify_layer_from(&gpu, main, *id, [m.a, m.b, m.c, m.d, m.e, m.f], squares))
        });
        if !matches!(went, Ok(true)) {
            return None;
        }
        // the vector layers drawn on it, by all that draws them on the artwork: its canvas and its
        // document stand in for the artwork's meanwhile, what is drawn carried onto it. Nothing is
        // kept to a selection as it is drawn
        if !drawn.is_empty() {
            crisp.canvas.set_selection(&gpu, None);
            let seen = [(0.0, 0.0), (cw as f32, 0.0), (0.0, ch as f32), (cw as f32, ch as f32)]
                .map(|(x, y)| from_crisp.apply(Vec2::new(x, y)));
            let seen = IRect::new(
                seen.iter().map(|p| p.x).fold(f32::INFINITY, f32::min).floor() as i32,
                seen.iter().map(|p| p.y).fold(f32::INFINITY, f32::min).floor() as i32,
                seen.iter().map(|p| p.x).fold(f32::NEG_INFINITY, f32::max).ceil() as i32,
                seen.iter().map(|p| p.y).fold(f32::NEG_INFINITY, f32::max).ceil() as i32,
            );
            let selection = self.selection.take();
            // (a ColorDrop held or a node being dragged on the artwork is set aside meanwhile:
            // drawing a fill settles what is open, and would end it)
            let pour = self.vector_pour.take();
            let reshape = self.vector_reshape.take();
            std::mem::swap(&mut self.doc, &mut crisp.doc);
            if let Some(main) = self.canvas.as_mut() {
                std::mem::swap(main, &mut crisp.canvas);
            }
            self.vector_shown_by = Some(Homography::from_affine(crisp.to_crisp));
            for (layer, content) in &drawn {
                // (only what is in sight)
                let ids: Vec<StrokeId> = content
                    .objects()
                    .iter()
                    .filter(|object| !object.bounds().intersect(seen).is_empty())
                    .map(Object::id)
                    .collect();
                self.draw_vector(*layer, &content.part(&ids, true), None);
            }
            self.flush();
            self.vector_shown_by = None;
            if let Some(main) = self.canvas.as_mut() {
                std::mem::swap(main, &mut crisp.canvas);
            }
            std::mem::swap(&mut self.doc, &mut crisp.doc);
            self.selection = selection;
            self.vector_pour = pour;
            self.vector_reshape = reshape;
            crisp.canvas.forget_history();
        }
        // the selection, to show (and to keep strokes to) as it does on the artwork
        let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
        let taken = self.selection.as_ref().map(|mask| {
            let mut data = vec![0u8; cw as usize * ch as usize];
            for (y, row) in data.chunks_mut(cw.max(1) as usize).enumerate() {
                for (x, taken) in row.iter_mut().enumerate() {
                    let p = from_crisp.apply(Vec2::new(x as f32 + 0.5, y as f32 + 0.5));
                    if p.x >= 0.0 && p.y >= 0.0 && p.x < w && p.y < h {
                        let at = p.y as usize * mask.width as usize + p.x as usize;
                        *taken = mask.data.get(at).copied().unwrap_or(0);
                    }
                }
            }
            data
        });
        crisp.canvas.set_selection(&gpu, taken.as_deref());
        (!gpu.is_lost()).then_some(crisp)
    }

    /// The lasso's fill the artwork's canvas shows as it is drawn (Lasso Fill's Outline, or a
    /// tool's own lasso) shown on the sharp picture too, where its polygon lies there: that
    /// picture is what a frame shows in the artwork's place. Done before each frame, and only
    /// when what the artwork shows has changed since.
    pub(crate) fn crisp_lasso(&mut self) {
        let Some(crisp) = self.crisp.as_mut() else { return };
        let shown = self.canvas.as_ref().and_then(Canvas::lasso_preview);
        if crisp.lasso.as_ref() == shown {
            return;
        }
        crisp.lasso = shown.cloned();
        let by = crisp.to_crisp;
        crisp.canvas.set_lasso_preview(shown.map(|preview| LassoPreview {
            layer: preview.layer,
            points: preview
                .points
                .iter()
                .map(|&[x, y]| {
                    let p = by.apply(Vec2::new(x, y));
                    [p.x, p.y]
                })
                .collect(),
            color: preview.color,
        }));
    }

    /// A stroke begins on `layer` of the artwork, drawn as `style` with `brush` (and a dual
    /// brush's `second`): if the sharp picture is showing, the stroke begins on it too, to be
    /// drawn there stamp for stamp as it is made ([`State::crisp_echo`]). Whether it did: the
    /// artwork's canvas is then to tell of the stroke ([`Canvas::echo_stroke`]).
    pub(crate) fn crisp_stroke_begin(
        &mut self,
        layer: LayerId,
        tool: ToolKind,
        brush: &Brush,
        style: StrokeStyle,
        second: Option<&(Brush, SecondStroke, f32)>,
    ) -> bool {
        // (what the last stroke left to tell is told first)
        self.crisp_echo();
        self.crisp_stroke = None;
        let plain = matches!(tool.stroke_tool(), StrokeTool::Paint | StrokeTool::Erase) && !style.preview;
        // (no sharp picture showing: nothing to bring up to date, and the artwork's waiting work,
        // a stroke laid again over frames as it ended say, is left to go on as it was)
        if !plain || self.crisp.is_none() || self.crisp_busy() || self.view_motion.is_some() {
            return false;
        }
        let (Some(view), Some(picture), Some(gpu)) = (self.view, self.crisp_picture(), self.gpu.clone()) else {
            return false;
        };
        let Some(mut crisp) = self.crisp.take() else { return false };
        if crisp.at.view != view.to_screen() || self.doc.unseen(layer).is_some() {
            self.crisp = Some(crisp);
            return false;
        }
        // (it is to be what the stroke is drawn on: brought up to the picture first)
        if crisp.picture != picture {
            match self.crisp_redo(crisp, None) {
                Some(made) => crisp = Crisp { picture, owed: None, ..made },
                None => return false,
            }
        }
        let mut style = style;
        ready(&mut crisp.canvas, &gpu, &mut self.grain_store, &mut style, brush);
        match second {
            Some((second_brush, drawn, _)) => {
                let mut second_style = drawn.style;
                ready(&mut crisp.canvas, &gpu, &mut self.grain_store, &mut second_style, second_brush);
                crisp.canvas.begin_dual_stroke(layer, style, SecondStroke { style: second_style, ..*drawn });
            }
            None => crisp.canvas.begin_stroke(layer, style),
        }
        self.crisp = Some(crisp);
        self.crisp_stroke = Some(layer);
        true
    }

    /// What the artwork's canvas was given of the stroke being drawn since this was last done is
    /// given to the sharp picture's, each stamp where and as big as that shows it. As the stroke
    /// ends there it ends here: kept, a layer of pixels is magnified again at once (the stroke's
    /// true pixels), a vector layer left as drawn until things rest.
    pub(crate) fn crisp_echo(&mut self) {
        let told = self.canvas.as_mut().map(Canvas::take_echo).unwrap_or_default();
        let Some(layer) = self.crisp_stroke.filter(|_| !told.is_empty()) else { return };
        let Some(mut crisp) = self.crisp.take() else {
            self.crisp_stroke = None;
            return;
        };
        let by = Homography::from_affine(crisp.to_crisp);
        let carried = |stamps: Vec<Stamp>| placed(stamps, by);
        let mut ended = None;
        for what in told {
            let canvas = &mut crisp.canvas;
            match what {
                StrokeEcho::Stamps(stamps) => canvas.add_stamps(&carried(stamps)),
                StrokeEcho::Replace(stamps) => canvas.replace_stamps(&carried(stamps)),
                StrokeEcho::Second(stamps) => canvas.add_second_stamps(&carried(stamps)),
                StrokeEcho::ReplaceSecond(stamps) => canvas.replace_second_stamps(&carried(stamps)),
                StrokeEcho::Tip(stamps) => canvas.set_tip(&carried(stamps)),
                StrokeEcho::SecondTip(stamps) => canvas.set_second_tip(&carried(stamps)),
                StrokeEcho::Ahead(stamps) => canvas.set_ahead(&carried(stamps)),
                StrokeEcho::Shown(shown) => canvas.set_stroke_shown(shown),
                StrokeEcho::Prediction(stamps) => canvas.set_prediction(&carried(stamps)),
                StrokeEcho::DropSecond => canvas.drop_second(),
                StrokeEcho::End => {
                    canvas.end_stroke();
                    ended = Some(true);
                }
                StrokeEcho::Cancel => {
                    canvas.cancel_stroke();
                    ended = Some(false);
                }
            }
        }
        let Some(kept) = ended else {
            self.crisp = Some(crisp);
            return;
        };
        self.crisp_stroke = None;
        if let Some(gpu) = self.gpu.as_ref() {
            crisp.canvas.render(gpu, &mut crisp.doc, None);
            crisp.canvas.forget_history();
        }
        // it now shows the picture as the stroke left it (but for a fill kept under the stroke,
        // which the stroke's stamps don't draw: its layer is then drawn again now, so the fill
        // shows from the frame its preview goes)
        let filled_under = std::mem::take(&mut crisp.filled_under);
        let Some(picture) = self.crisp_picture() else { return };
        if kept && (filled_under || !self.doc.is_vector(layer)) {
            self.crisp = self.crisp_redo(crisp, Some(layer)).map(|made| Crisp { picture, ..made });
        } else {
            self.crisp = Some(Crisp { picture, owed: kept.then(Instant::now).or(crisp.owed), ..crisp });
        }
    }
}

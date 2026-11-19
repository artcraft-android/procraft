//! Selections: shapes drawn on the window become a selection on the canvas, which painting keeps
//! inside, and which can be inverted, feathered, saved, filled or copied to a layer of its own.

use procraft_engine::geom::Vec2;
use procraft_engine::selection::{ColourFlood, Combine, Mask, feather_radius};

use crate::{CanvasSession, State};

/// How a new shape joins the selection there is.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionJoin {
    Add,
    Remove,
}

impl From<SelectionJoin> for Combine {
    fn from(j: SelectionJoin) -> Combine {
        match j {
            SelectionJoin::Add => Combine::Add,
            SelectionJoin::Remove => Combine::Remove,
        }
    }
}

/// How the selection shows: bands over what is not selected, or what is selected tinted (while
/// selecting by colour) or covered (while its threshold is being dragged).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SelectionDisplay {
    Stripes,
    Tint,
    Fill,
}

/// How Lasso Fill fills a shape: painting it, or erasing it (Lasso Erase); at Color Fill's opacity,
/// or at the current tool's opacity slider (`tool_opacity`: the Paint or Erase tool's own lasso).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct LassoFill {
    pub erase: bool,
    pub tool_opacity: bool,
}

/// Lasso Fill with the Pen's brush (Lasso's Outline on): each paint stroke, as it ends, fills the
/// shape its path goes round (closed from its end back to its start) under it, with the paint
/// colour or the secondary one (`secondary`) at `opacity` (0..1), only where the selection
/// reaches, the stroke and its fill one undo step.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct StrokeFill {
    pub secondary: bool,
    pub opacity: f32,
}

/// What a lasso's fill as it is drawn shows in where it erases: grey, as what it takes away.
pub(crate) const ERASE_PREVIEW: [f32; 3] = [0.5, 0.5, 0.5];

/// How wide (window pixels) the stroke a tool's own lasso follows the pen as is: small, so its line
/// keeps close to the pen's path, its points near enough together.
const LASSO_LINE_WIDTH: f32 = 6.0;

/// Points around an ellipse drawn as a polygon.
const ELLIPSE_POINTS: usize = 128;

impl State {
    /// Shows the selection on the canvas (or none when nothing is selected).
    pub(crate) fn show_selection(&mut self) {
        let mask = self.selection.as_ref().filter(|m| !m.is_empty());
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.set_selection(gpu, mask.map(|m| m.data.as_slice()));
        }
        if mask.is_none() {
            self.selection = None;
        }
    }

    /// The selection as it is now becomes one undo step, called `name`, that puts `before` back.
    pub(crate) fn record_selection(&mut self, before: Option<Mask>, name: &str) {
        self.finish_stroke();
        self.settle();
        if self.selection.as_ref().is_some_and(Mask::is_empty) {
            self.selection = None;
        }
        if self.selection == before {
            return;
        }
        let coverage = self.selection.as_ref().map(|m| m.data.as_slice());
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.name_next_step(name);
            canvas.change_selection(gpu, &mut self.doc, coverage, before.map(|m| m.data));
            self.revision += 1;
        }
    }

    /// Joins `shape` to the selection; `record` makes it an undo step (otherwise it is only shown,
    /// as while a colour threshold is dragged).
    pub(crate) fn join(&mut self, shape: Mask, how: SelectionJoin, record: bool) {
        self.feather_base = None;
        let before = self.selection.clone();
        match (self.selection.as_mut(), how) {
            (Some(m), _) => m.combine(&shape, how.into()),
            (None, SelectionJoin::Add) => self.selection = Some(shape),
            (None, SelectionJoin::Remove) => {}
        }
        if record {
            let name = match (how, before.is_some()) {
                (SelectionJoin::Remove, _) => "subtract from selection",
                (SelectionJoin::Add, true) => "add to selection",
                (SelectionJoin::Add, false) => "draw selection",
            };
            self.record_selection(before, name);
        } else {
            self.show_selection();
        }
    }

    /// Takes the selection back from the canvas (after an undo or redo).
    pub(crate) fn sync_selection(&mut self) {
        let (w, h) = (self.doc.width(), self.doc.height());
        self.selection =
            self.canvas.as_ref().and_then(|c| c.selection()).map(|c| Mask { width: w, height: h, data: c.to_vec() });
        self.feather_base = None;
    }

    /// The canvas pixel under window point (`x`, `y`), if there is one: through the view, or on a
    /// shown model the texel of the texture set painted on.
    pub(crate) fn window_texel(&self, x: f32, y: f32) -> Option<(u32, u32)> {
        if self.scene_shown() {
            return self.scene_texel(x, y);
        }
        let p = self.view?.screen_to_canvas(Vec2::new(x, y))?;
        let (w, h) = (self.doc.width() as f32, self.doc.height() as f32);
        (p.x >= 0.0 && p.y >= 0.0 && p.x < w && p.y < h).then_some((p.x as u32, p.y as u32))
    }

    /// Window points to canvas points.
    pub(crate) fn to_canvas_points(&self, window: &[Vec2]) -> Option<Vec<Vec2>> {
        let view = self.view?;
        window.iter().map(|&p| view.screen_to_canvas(p).filter(|c| c.is_finite())).collect()
    }

    /// Whether a shape Lasso Fill draws is pixel art's (`hard`): as Selection's Pixelated says, or
    /// for a tool's own lasso, which fills as its brush paints, as that brush's Pixelated does.
    pub(crate) fn lasso_pixelated(&self, lasso: LassoFill) -> bool {
        if lasso.tool_opacity { self.brush(self.tool).setting("pixelated") >= 0.5 } else { self.selection_pixelated }
    }

    /// Window points `window` made a shape on the canvas (on a shown model, on its texture). For
    /// pixel art (`hard`: [`CanvasSession::set_selection_pixelated`]) its points go onto whole
    /// pixels and it takes whole pixels only, none partly.
    fn window_shape(&mut self, window: &[Vec2], hard: bool) -> Option<Mask> {
        if self.scene_shown() {
            self.scene_mask(window)
        } else if hard {
            // along the pixels' edges all the way round, so it takes just the pixels it goes round
            let points = procraft_engine::pixel::stepped(&self.to_canvas_points(window)?, true);
            Some(Mask::polygon_hard(self.doc.width(), self.doc.height(), &points))
        } else {
            let points = self.to_canvas_points(window)?;
            Some(Mask::polygon(self.doc.width(), self.doc.height(), &points))
        }
    }

    /// The rectangle (or with `ellipse`, the ellipse in it) between window points `a` and `b` as a
    /// shape on the canvas. For pixel art (`hard`) the two corners go onto whole pixels first, the
    /// shape then drawn between them, and it takes whole pixels only: a rectangle is the pixels
    /// between its corners, and an ellipse fits a whole number of pixels across and down.
    fn window_box(&mut self, ellipse: bool, a: Vec2, b: Vec2, hard: bool) -> Option<Mask> {
        if !hard || self.scene_shown() {
            return self.window_shape(&shape_outline(ellipse, a.x, a.y, b.x, b.y), false);
        }
        let view = self.view?;
        let to_screen = view.to_screen();
        let snap = |p: Vec2| {
            let on = view.screen_to_canvas(p).filter(|c| c.is_finite())?;
            Some(to_screen.apply(Vec2::new(on.x.round(), on.y.round())))
        };
        let (a, b) = (snap(a)?, snap(b)?);
        let points = self.to_canvas_points(&shape_outline(ellipse, a.x, a.y, b.x, b.y))?;
        Some(Mask::polygon_hard(self.doc.width(), self.doc.height(), &points))
    }

    /// Fills `mask` on the current layer with the paint colour at `opacity` (or with `erase`, takes
    /// it away from what is there), as one undo step (called `name`, if it has one); false if
    /// nothing could be filled (no layer, a locked one, an empty mask, or erasing with the alpha
    /// locked).
    fn fill_mask(&mut self, mask: &Mask, name: Option<&str>, opacity: f32, erase: bool) -> bool {
        let current = self.doc.current();
        if self.doc.layer(current).is_none() || self.doc.is_locked(current) {
            return false;
        }
        let alpha_lock = self.doc.layer(current).is_some_and(|l| l.alpha_lock);
        if erase && alpha_lock {
            return false;
        }
        // only what the mask covers changes
        let Some(rect) = crate::layer_ops::mask_rect(mask) else { return false };
        let Some(mut pixels) = self.layer_part(current, rect) else { return false };
        fill_pixels(&mut pixels, &crate::layer_ops::mask_part(mask, rect), self.color, opacity, erase, alpha_lock);
        let Some(canvas) = self.canvas.as_mut() else { return false };
        if let Some(name) = name {
            canvas.name_next_step(name);
        }
        canvas.write_layer_part(current, rect, pixels);
        self.flush();
        self.revision += 1;
        true
    }

    /// Lasso Fill's Outline (see [`StrokeFill`]): the paint stroke `open`, about to end, has what
    /// it goes round filled under it, given its layer to write as it goes on (on a vector layer,
    /// kept there as a fill under the stroke). Whether it was: not for other tools, a stroke laid
    /// in place, or a model's.
    pub(crate) fn fill_under_stroke(&mut self, open: &procraft_engine::stroke::StrokeBuilder) -> bool {
        let Some((layer, fill)) = self.stroke_fill_target() else { return false };
        // the path its stamps go along (before jitter threw them), back from where it lifted to
        // where it began
        let mut line = open.clone();
        line.finish();
        let path = line.placed_line();
        if path.len() < 3 {
            return false;
        }
        let (width, height) = (self.doc.width(), self.doc.height());
        // a pixelated brush fills whole pixels, as its lasso does
        let hard = self.brush(self.tool).setting("pixelated") >= 0.5;
        let outline = if hard { procraft_engine::pixel::stepped(&path, true) } else { path };
        let mut shape =
            if hard { Mask::polygon_hard(width, height, &outline) } else { Mask::polygon(width, height, &outline) };
        if let Some(selection) = &self.selection {
            for (a, &b) in shape.data.iter_mut().zip(&selection.data) {
                *a = ((u16::from(*a) * u16::from(b) + 127) / 255) as u8;
            }
        }
        let Some(rect) = crate::layer_ops::mask_rect(&shape) else { return false };
        // the layer as it is under the stroke (drawn apart from it until it ends)
        self.flush();
        let colour = if fill.secondary { self.secondary } else { self.color };
        // a vector layer keeps it as a fill under the stroke (drawn again as laid now: over what
        // is there, no alpha lock)
        let vector = self.doc.is_vector(layer);
        if vector {
            self.vector_fill_under(layer, outline, hard, colour, fill.opacity);
        }
        let alpha_lock = !vector && self.doc.layer(layer).is_some_and(|l| l.alpha_lock);
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return false };
        let Ok(mut pixels) = canvas.read_layer_part(gpu, layer, rect) else { return false };
        fill_pixels(&mut pixels, &crate::layer_ops::mask_part(&shape, rect), colour, fill.opacity, false, alpha_lock);
        canvas.set_stroke_under(rect, pixels);
        true
    }

    /// What a tool's own lasso follows the pen with: a stroke of the current tool's brush as it
    /// would be begun now, the app's input settings and the view's turn and zoom with it, small.
    pub(crate) fn lasso_builder(&self) -> procraft_engine::stroke::StrokeBuilder {
        let brush = self.brush(self.tool).clone();
        let zoom = self.view.map_or(1.0, |v| v.zoom()).max(1e-3);
        procraft_engine::stroke::StrokeBuilder::with_seed(
            brush,
            (LASSO_LINE_WIDTH / zoom).max(0.5),
            procraft_engine::input::Pointer::Stylus,
            0,
        )
        .with_input(self.input.clone())
        .on_view_turned(self.view.map_or(0.0, |v| v.rotation()))
        .seen_at(zoom / self.display_scale)
    }

    /// Where Lasso Fill's Outline fills under the paint stroke open, and how: none for other tools,
    /// a stroke laid in place, one on a plugin's or a locked layer, or a model's.
    fn stroke_fill_target(&self) -> Option<(procraft_engine::document::LayerId, StrokeFill)> {
        let fill = self.stroke_fill?;
        if self.scene_shown() || !self.hooks.as_ref().is_some_and(|hooks| hooks.lasso_fill()) {
            return None;
        }
        let canvas = self.canvas.as_ref()?;
        if canvas.stroke_tool() != Some(procraft_gpu::StrokeTool::Paint) || canvas.is_pencil() {
            return None;
        }
        let layer = canvas.stroke_laid_apart()?;
        // (a plugin's layer makes what is drawn on it its own way)
        let plugins = self.doc.layer(layer).is_none_or(|l| l.plugin.is_some()) || self.recording.is_some();
        (!plugins && !self.doc.is_locked(layer)).then_some((layer, fill))
    }

    /// Lasso Fill's Outline shown under the paint stroke open, as far as it shows drawn: its tip
    /// reaching the input, and on to `predicted` (canvas samples expected next) as its tip is.
    pub(crate) fn preview_stroke_fill(&mut self, predicted: &[procraft_engine::input::Sample]) {
        let Some((layer, fill)) = self.stroke_fill_target() else { return };
        let Some(open) = self.stroke.as_mut() else { return };
        let line = open.shown_line(predicted);
        let colour = if fill.secondary { self.secondary } else { self.color };
        self.preview_lasso(layer, &line, colour, fill.opacity);
    }

    /// A lasso's fill shown as it is drawn on layer `layer`: what `line` (canvas pixels) goes
    /// round, in `colour` at `strength` (as it will fill) times the preview's opacity.
    pub(crate) fn preview_lasso(&mut self, layer: procraft_engine::document::LayerId, line: &[Vec2], colour: [f32; 3], strength: f32) {
        let a = (strength * self.lasso_preview).clamp(0.0, 1.0);
        let [r, g, b] = colour.map(|c| c.clamp(0.0, 1.0) * a);
        let preview = (a > 0.0 && line.len() >= 3).then(|| procraft_gpu::LassoPreview {
            layer,
            points: line.iter().map(|p| [p.x, p.y]).collect(),
            color: [r, g, b, a],
        });
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.set_lasso_preview(preview);
        }
    }

    /// The lasso's fill shown as it was drawn taken away.
    pub(crate) fn end_lasso_preview(&mut self) {
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.set_lasso_preview(None);
        }
    }

    /// The current layer's pixels, after any queued work.
    pub(crate) fn current_pixels(&mut self) -> Option<Vec<u8>> {
        self.finish_stroke();
        self.settle();
        self.flush();
        let current = self.doc.current();
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return None };
        canvas.read_layer(gpu, current).ok()
    }
}

#[uniffi::export]
impl CanvasSession {
    /// A rectangle (or with `ellipse`, the ellipse in it) between window points (`x0`, `y0`) and
    /// (`x1`, `y1`) joins the selection. Drawn on the window, so it turns with the view.
    pub fn select_shape(&self, ellipse: bool, x0: f32, y0: f32, x1: f32, y1: f32, join: SelectionJoin) {
        self.select_window(
            |st| st.window_box(ellipse, Vec2::new(x0, y0), Vec2::new(x1, y1), st.selection_pixelated),
            join,
        );
    }

    /// Selection's Pixelated, for pixel art, from now on: the shapes drawn to select with, and
    /// those Color Fill's Lasso Fill fills, have their points on whole pixels and take whole
    /// pixels only, with no partly covered edge. (Selecting by colour always does; a tool's own
    /// lasso goes by its brush's Pixelated instead.) What is selected already stays as it is, and
    /// feathering still softens it.
    pub fn set_selection_pixelated(&self, on: bool) {
        self.shared.lock().selection_pixelated = on;
    }

    /// The polygon through window points `points` (x, y pairs) joins the selection.
    pub fn select_polygon(&self, points: Vec<f32>, join: SelectionJoin) {
        let window: Vec<Vec2> = points.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
        self.select_window(|st| st.window_shape(&window, st.selection_pixelated), join);
    }

    /// Selects by colour: the pixels of the current layer (or of the reference layer, when another
    /// layer is it, as ColorDrop reads) joined to window point (`x`, `y`) whose colour is within
    /// `threshold` (0..1) of its colour join the selection. While `live` (the
    /// threshold being dragged) each call replaces the last one's shape; the last call is not live.
    pub fn select_by_colour(&self, x: f32, y: f32, threshold: f32, join: SelectionJoin, live: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(seed) = st.window_texel(x, y) else { return };
        let (w, h) = (st.doc.width(), st.doc.height());
        // on a vector layer the tap takes the stroke or fill under it, whole (there is no colour
        // to measure a threshold from)
        if st.doc.is_vector(st.doc.current()) && !st.scene_shown() {
            if st.selection_before.is_none() {
                st.selection_before = Some(st.selection.clone());
            }
            let before = st.selection_before.clone().flatten();
            st.selection = before.clone();
            match st.window_to_canvas(x, y).and_then(|at| st.footprint_at(at)) {
                Some(shape) => st.join(shape, join, false),
                None => st.show_selection(),
            }
            if !live {
                st.selection_before = None;
                st.record_selection(before, "Automatic Selection");
            }
            self.shared.frame(st);
            return;
        }
        // the layer is read and measured from the seed once for the whole drag
        let (pixels, flood) = match st.colour_source.take() {
            Some((px, flood)) if flood.seed() == seed => (px, Some(flood)),
            Some((px, _)) => (px, None),
            None => {
                let current = st.doc.current();
                let source = st.doc.reference().filter(|&r| r != current).unwrap_or(current);
                let Some(px) = st.layer_pixels(source) else { return };
                st.selection_before = Some(st.selection.clone());
                (px, None)
            }
        };
        let Some(flood) = flood.or_else(|| {
            let flood = ColourFlood::new(w, h, &pixels, seed)?;
            if live {
                flood.work_out_reach();
            }
            Some(flood)
        }) else {
            return;
        };
        let shape = flood.mask(threshold);
        let before = st.selection_before.clone().flatten();
        st.selection = before.clone();
        if live {
            st.join(shape, join, false);
            st.colour_source = Some((pixels, flood));
        } else {
            // the drag ends: one step from before it
            st.join(shape, join, false);
            st.selection_before = None;
            st.record_selection(before, "Automatic Selection");
        }
        self.shared.frame(st);
    }

    /// Selection mask visibility, 0..1 (a quarter by default).
    pub fn set_selection_visibility(&self, visibility: f32) {
        let mut st = self.shared.lock();
        st.selection_visibility = if visibility.is_finite() { visibility.clamp(0.0, 1.0) } else { 0.25 };
        self.shared.frame(&mut st);
    }

    /// Brush scaling: relative to the screen (true) or to the canvas.
    pub fn set_brush_scaling(&self, on_screen: bool) {
        self.shared.lock().brush_on_screen = on_screen;
    }

    pub fn set_selection_display(&self, display: SelectionDisplay) {
        let mut st = self.shared.lock();
        st.selection_display = display;
        self.shared.frame(&mut st);
    }

    /// Selects what was not selected, and the other way round.
    pub fn select_invert(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (w, h) = (st.doc.width(), st.doc.height());
        let before = st.selection.clone();
        let mut m = st.selection.take().unwrap_or_else(|| Mask::empty(w, h));
        st.feather_base = None;
        m.invert();
        st.selection = Some(m);
        st.record_selection(before, "invert");
        self.shared.frame(st);
    }

    /// Softens the selection's edges by `amount` (0..1, the feather popover's): each call feathers
    /// the selection as it was before the first, until the selection is changed some other way.
    /// A big selection takes a while, so the canvas is let go of meanwhile (a host may call this
    /// off its main thread, one call at a time); a selection changed some other way meanwhile
    /// stays as it was changed.
    pub fn select_feather(&self, amount: f32) {
        let (base, mut room) = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            // the selection as it was before the first, kept once
            if st.feather_base.is_none() {
                st.feather_base = st.selection.clone().map(|m| (m, Vec::new()));
            }
            let Some((base, room)) = st.feather_base.as_mut() else { return };
            (base.clone(), std::mem::take(room))
        };
        let mut m = base.clone();
        m.feather_in(feather_radius(amount), &mut room);
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some((_, kept)) = st.feather_base.as_mut().filter(|(now, _)| *now == base) else { return };
        *kept = room;
        st.selection = Some(m);
        st.show_selection();
        self.shared.frame(st);
    }

    /// The feather popover closed: its feathering becomes one undo step.
    pub fn select_feather_done(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let Some((base, _)) = st.feather_base.take() {
            st.record_selection(Some(base), "feather");
            self.shared.frame(st);
        }
    }

    /// Selects nothing (painting goes anywhere again).
    pub fn select_none(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let before = st.selection.take();
        st.feather_base = None;
        st.record_selection(before, "clear selection");
        self.shared.frame(st);
    }

    pub fn has_selection(&self) -> bool {
        self.shared.lock().selection.is_some()
    }

    /// Keeps the selection to load again later; returns how many are kept.
    pub fn save_selection(&self) -> u32 {
        let mut st = self.shared.lock();
        if let Some(m) = st.selection.clone() {
            st.saved_selections.push(m);
        }
        u32::try_from(st.saved_selections.len()).unwrap_or(u32::MAX)
    }

    /// Makes a kept selection the selection again.
    pub fn load_selection(&self, index: u32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(m) = st.saved_selections.get(index as usize).cloned() else { return false };
        let before = st.selection.replace(m);
        st.feather_base = None;
        st.record_selection(before, "load selection");
        self.shared.frame(st);
        true
    }

    pub fn delete_saved_selection(&self, index: u32) {
        let mut st = self.shared.lock();
        if (index as usize) < st.saved_selections.len() {
            st.saved_selections.remove(index as usize);
        }
    }

    /// A kept selection as a small picture fitting `max_width` × `max_height`: white where it
    /// selects, black where it does not.
    pub fn saved_selection_thumbnail(
        &self,
        index: u32,
        max_width: u32,
        max_height: u32,
    ) -> Option<crate::layers::ThumbnailImage> {
        let st = self.shared.lock();
        let m = st.saved_selections.get(index as usize)?;
        let (w, h) = (m.width.max(1) as f32, m.height.max(1) as f32);
        let scale = (max_width.max(1) as f32 / w).min(max_height.max(1) as f32 / h);
        let (tw, th) = (((w * scale).round() as u32).max(1), ((h * scale).round() as u32).max(1));
        let mut rgba = Vec::with_capacity((tw * th * 4) as usize);
        for y in 0..th {
            for x in 0..tw {
                // the mean of the mask pixels this one covers, sampled on a small grid
                let mut sum = 0u32;
                for sy in 0..3 {
                    for sx in 0..3 {
                        let mx = ((x as f32 + (sx as f32 + 0.5) / 3.0) / scale) as u32;
                        let my = ((y as f32 + (sy as f32 + 0.5) / 3.0) / scale) as u32;
                        let i =
                            (my.min(m.height.saturating_sub(1)) * m.width + mx.min(m.width.saturating_sub(1))) as usize;
                        sum += u32::from(m.data.get(i).copied().unwrap_or(0));
                    }
                }
                let v = (sum / 9) as u8;
                rgba.extend([v, v, v, 255]);
            }
        }
        Some(crate::layers::ThumbnailImage { width: tw, height: th, rgba })
    }

    pub fn saved_selection_count(&self) -> u32 {
        u32::try_from(self.shared.lock().saved_selections.len()).unwrap_or(u32::MAX)
    }

    /// How strongly a lasso's fill shows as it is drawn, 0..1, times the strength it will fill at:
    /// none at 0, as it will be at 1. Shown on the canvas, at its own resolution, on the layer
    /// (under a paint stroke whose Outline it is).
    pub fn set_lasso_preview_opacity(&self, opacity: f32) {
        self.shared.lock().lasso_preview = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { 0.5 };
    }

    /// Lasso Fill's Outline from now on (see [`StrokeFill`]), or none: paint strokes only paint.
    /// Taken up only while a plugin offers Lasso Fill.
    pub fn set_stroke_fill(&self, fill: Option<StrokeFill>) {
        self.shared.lock().stroke_fill = fill;
    }

    /// How strongly Color Fill fills, by selection or Lasso Fill, 0..1.
    pub fn set_fill_opacity(&self, opacity: f32) {
        self.shared.lock().fill_opacity = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { 1.0 };
    }

    /// Fills the selection on the current layer with the paint colour at Color Fill's opacity
    /// ([`CanvasSession::set_fill_opacity`]), as one undo step.
    pub fn fill_selection(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let current = st.doc.current();
        if st.doc.layer(current).is_none_or(|l| l.plugin.is_some()) || st.doc.is_locked(current) {
            return;
        }
        let Some(mask) = st.selection.clone() else { return };
        let opacity = st.fill_opacity;
        if st.fill_mask(&mask, None, opacity, false) {
            self.shared.frame(st);
        }
    }

    /// Copies what is selected on the current layer to a new layer above it, made current;
    /// returns its id.
    pub fn copy_selection_to_layer(&self) -> Option<u64> {
        // only what the selection's box holds is read back and copied
        let part = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            let mask = st.selection.clone()?;
            match crate::layer_ops::mask_rect(&mask) {
                Some(rect) => {
                    let mut pixels = st.layer_part(st.doc.current(), rect)?;
                    for (px, &m) in
                        pixels.as_chunks_mut::<4>().0.iter_mut().zip(&crate::layer_ops::mask_part(&mask, rect))
                    {
                        let k = u16::from(m);
                        for c in px.iter_mut() {
                            *c = ((u16::from(*c) * k + 127) / 255) as u8;
                        }
                    }
                    Some((rect, pixels))
                }
                None => None,
            }
        };
        let id = self.add_layer_quietly();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if let (Some(canvas), Some(gpu), Some((rect, pixels))) = (st.canvas.as_mut(), st.gpu.as_ref(), part) {
            let _ = canvas.upload_layer_part(gpu, procraft_engine::document::LayerId(id), rect, &pixels);
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(id)
    }
}

impl CanvasSession {
    /// The shape `drawn` on the window joins the selection.
    fn select_window(&self, drawn: impl FnOnce(&mut State) -> Option<Mask>, join: SelectionJoin) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(shape) = drawn(st) else { return };
        // (on a vector layer a shape takes the strokes and fills it touches, whole)
        let Some(shape) = st.snapped_to_objects(shape) else { return };
        st.join(shape, join, true);
        self.shared.frame(st);
    }

    /// Lasso Fill: the shape `drawn` on the window filled on the current layer with the paint
    /// colour (or erased, as `lasso` says), as one undo step, only where the selection there is
    /// reaches (as painting is); the selection is left as it is.
    fn fill_window(&self, drawn: impl FnOnce(&mut State) -> Option<Mask>, lasso: LassoFill) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let Some(mut shape) = drawn(st) else { return };
        if let Some(selection) = &st.selection {
            // as much of each pixel as both cover (a feathered edge stays soft)
            for (a, &b) in shape.data.iter_mut().zip(&selection.data) {
                *a = ((u16::from(*a) * u16::from(b) + 127) / 255) as u8;
            }
        }
        let opacity = if lasso.tool_opacity { st.sliders(st.tool).opacity } else { st.fill_opacity };
        let name = if lasso.erase { "lasso erase" } else { "lasso fill" };
        if st.fill_mask(&shape, Some(name), opacity, lasso.erase) {
            self.shared.frame(st);
        }
    }

    /// Lasso Fill: the polygon through window points `points` (x, y pairs) filled.
    pub(crate) fn fill_polygon(&self, points: Vec<f32>, lasso: LassoFill) {
        let window: Vec<Vec2> = points.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
        // (a vector layer keeps the shape as a shape)
        if self.lasso_on_vector(&window, lasso) {
            return;
        }
        self.fill_window(|st| st.window_shape(&window, st.lasso_pixelated(lasso)), lasso);
    }

    /// Lasso Fill: a rectangle (or with `ellipse`, the ellipse in it) between window points
    /// (`x0`, `y0`) and (`x1`, `y1`) filled.
    pub(crate) fn fill_shape(&self, ellipse: bool, x0: f32, y0: f32, x1: f32, y1: f32, lasso: LassoFill) {
        if self.lasso_on_vector(&shape_outline(ellipse, x0, y0, x1, y1), lasso) {
            return;
        }
        self.fill_window(
            |st| st.window_box(ellipse, Vec2::new(x0, y0), Vec2::new(x1, y1), st.lasso_pixelated(lasso)),
            lasso,
        );
    }
}

/// A path drawn by hand (`points`: x, y in turn, canvas pixels) as Pixelated takes it, for showing
/// it as it is drawn: along the pixels' edges, a pixel across or down at a time, and `closed`,
/// back to where it began the same way (`procraft_engine::pixel::stepped`). The shape it then
/// selects or fills is just the pixels inside this.
#[uniffi::export]
pub fn pixel_path(points: Vec<f32>, closed: bool) -> Vec<f32> {
    let points: Vec<Vec2> = points.as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)).collect();
    procraft_engine::pixel::stepped(&points, closed).iter().flat_map(|p| [p.x, p.y]).collect()
}

/// A rectangle (or with `ellipse`, the ellipse in it) between window points (`x0`, `y0`) and
/// (`x1`, `y1`), as a polygon.
fn shape_outline(ellipse: bool, x0: f32, y0: f32, x1: f32, y1: f32) -> Vec<Vec2> {
    let (a, b) = (Vec2::new(x0, y0), Vec2::new(x1, y1));
    if ellipse {
        let (c, r) = (a.lerp(b, 0.5), (b - a) * 0.5);
        (0..ELLIPSE_POINTS)
            .map(|i| {
                let t = i as f32 / ELLIPSE_POINTS as f32 * std::f32::consts::TAU;
                Vec2::new(c.x + r.x * t.cos(), c.y + r.y * t.sin())
            })
            .collect()
    } else {
        vec![a, Vec2::new(b.x, a.y), b, Vec2::new(a.x, b.y)]
    }
}

/// `pixels` (premultiplied RGBA8) with `colour` laid over them where `mask` (a byte each) covers,
/// at `opacity`; or with `erase`, taken away there. With `alpha_lock`, the colour goes only where
/// there is paint, as much as there is.
fn fill_pixels(pixels: &mut [u8], mask: &[u8], colour: [f32; 3], opacity: f32, erase: bool, alpha_lock: bool) {
    let [r, g, b] = colour.map(|c| c.clamp(0.0, 1.0));
    let opacity = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { 1.0 };
    for (px, &m) in pixels.as_chunks_mut::<4>().0.iter_mut().zip(mask) {
        if m == 0 {
            continue;
        }
        if erase {
            // what is there (premultiplied), less as much as is covered at the opacity
            let keep = 1.0 - f32::from(m) / 255.0 * opacity;
            for c in px.iter_mut() {
                *c = to_byte(f32::from(*c) / 255.0 * keep);
            }
            continue;
        }
        let [pr, pg, pb, pa] = px;
        // the colour over what is there, as much as is covered at the fill's opacity (only
        // where there is paint, with the alpha locked)
        let k = f32::from(m) / 255.0 * opacity;
        let a = f32::from(*pa) / 255.0;
        let (cover, out_a) = if alpha_lock { (k * a, a) } else { (k, k + a * (1.0 - k)) };
        for (dst, src) in [(&mut *pr, r), (&mut *pg, g), (&mut *pb, b)] {
            *dst = to_byte(src * cover + f32::from(*dst) / 255.0 * (1.0 - k));
        }
        *pa = to_byte(out_a);
    }
}

fn to_byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

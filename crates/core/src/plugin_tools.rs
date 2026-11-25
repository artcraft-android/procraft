//! Tools (interface 24): a plugin's tool, opened from Actions › Canvas, whose touches go to the
//! plugin. The app tells it what happens (it was opened, a touch went down, moved, lifted or was
//! cancelled by a pinch, or the artwork changed under it, an undo), with the canvas's size and
//! the shapes it may edit; the plugin answers with what to show over the canvas and edits to the
//! layers, and what becomes of them: shown as they are while a touch goes on, kept as one step
//! undo takes back, or gone back to how the layers were. Its edits need the `layers.edit`
//! permission.
//!
//! The edits are the app's to make, and to draw: today, comic panels (see
//! [`procraft_engine::panels`]), each a group of a frame keeping a shape, drawn white with a border,
//! and the art above it clipped to it. A panel's shape set, or a new panel made. So what a tool
//! made shows and opens as it was with its plugin off or gone.
//!
//! Records here are those `plugin_layers.rs` writes: fields one after another, each a 16-bit id, a
//! 32-bit length and its bytes, little-endian. Numbers are little-endian too; text is UTF-8;
//! points are canvas pixels, x and y by turns (f32 each).
//!
//! What it is told (`procraft_tool_touch(tool, len)`, `tool` by its place in the manifest's
//! `tools`): [`told`]. Its answer (its length, at `procraft_tool_out()`; below 0 for nothing):
//! [`answer`].

use std::sync::Arc;

use procraft_engine::document::{Document, LayerId};
use procraft_engine::geom::{Affine, IRect, Vec2};
use procraft_engine::panels::{self, Point};
use procraft_engine::view::View;

use crate::plugin_layers::{Record, fields};
use crate::plugin_run::Running;
use crate::plugins::Plugins;
use crate::{CanvasSession, State};

/// The fields of what a tool is told.
pub mod told {
    /// What happened (u32): one of [`OPENED`], [`AGAIN`], [`DOWN`], [`MOVED`], [`UP`],
    /// [`CANCELLED`].
    pub const WHAT: u16 = 1;
    /// Where the touch is (a point).
    pub const AT: u16 = 2;
    /// The mode chosen, by its place among the tool's (u32).
    pub const MODE: u16 = 3;
    /// How many canvas pixels a point of the screen spans now (f32): how near a touch is.
    pub const PER_POINT: u16 = 4;
    /// The canvas's width and height (u32 each).
    pub const SIZE: u16 = 5;
    /// 1 if the artwork is an animation (u8).
    pub const ANIMATED: u16 = 6;
    /// A shape it may edit, one field each, bottom first: a record of [`FRAME_LAYER`] (the layer
    /// keeping it, u64) and [`FRAME_POINTS`] (its corners).
    pub const FRAME: u16 = 7;
    pub const FRAME_LAYER: u16 = 1;
    pub const FRAME_POINTS: u16 = 2;

    /// The tool was opened.
    pub const OPENED: u32 = 0;
    /// The artwork changed under it (undo, redo): what it shows may be asked again.
    pub const AGAIN: u32 = 1;
    /// A touch went down, moved, lifted, or was cancelled (a pinch came).
    pub const DOWN: u32 = 2;
    pub const MOVED: u32 = 3;
    pub const UP: u32 = 4;
    pub const CANCELLED: u32 = 5;
}

/// The fields of a tool's answer (some named only for the record: the plugin writes them).
#[allow(dead_code)]
pub mod answer {
    /// A shape to show over the canvas, one field each: a record of [`SHOW_POINTS`],
    /// [`SHOW_CLOSED`] (1: its last corner joins its first, u8) and [`SHOW_LOOK`] (u32: [`GUIDE`]
    /// a thin dashed line, [`DRAWN`] what the touch is drawing).
    pub const SHOW: u16 = 1;
    /// An edit to the layers, one field each: a record of [`EDIT_KIND`] (u32) and what it takes.
    /// [`SHAPE`]: [`EDIT_LAYER`] a frame, [`EDIT_POINTS`] its shape. [`PANEL`]: a new panel of shape
    /// [`EDIT_POINTS`], just above the panel of frame [`EDIT_LAYER`] (0: on top of the stack),
    /// named [`EDIT_NAME`], its border as frame [`EDIT_LIKE`]'s or else [`EDIT_BORDER`] canvas
    /// pixels wide (f32), its art the current layer if [`EDIT_CURRENT`] is 1 (u8); made only as the
    /// edits are kept.
    pub const EDIT: u16 = 2;
    /// What becomes of the edits (u32): [`SHOWN`] (and those shown since the touch began, if no
    /// field), [`KEPT`] as one step, [`BACK`] to how the layers were before them.
    pub const KEEP: u16 = 3;
    /// The step's name, as undo says it.
    pub const STEP: u16 = 4;
    /// What the app says.
    pub const SAID: u16 = 5;
    /// 1 if the tool closes (u8).
    pub const CLOSE: u16 = 6;

    pub const SHOW_POINTS: u16 = 1;
    pub const SHOW_CLOSED: u16 = 2;
    pub const SHOW_LOOK: u16 = 3;
    pub const EDIT_KIND: u16 = 1;
    pub const EDIT_LAYER: u16 = 2;
    pub const EDIT_POINTS: u16 = 3;
    pub const EDIT_NAME: u16 = 4;
    pub const EDIT_LIKE: u16 = 5;
    pub const EDIT_BORDER: u16 = 6;
    pub const EDIT_CURRENT: u16 = 7;

    pub const SHAPE: u32 = 0;
    pub const PANEL: u32 = 1;
    pub const SHOWN: u32 = 0;
    pub const KEPT: u32 = 1;
    pub const BACK: u32 = 2;
    pub const GUIDE: u32 = 0;
    pub const DRAWN: u32 = 1;
}

/// The most corners a shape shown or edited may have, shapes one answer may show, and edits it
/// may make.
const MOST_CORNERS: usize = 256;
const MOST_SHOWN: usize = 4096;
const MOST_EDITS: usize = 1024;

/// What happened with a plugin's tool, as the app tells it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum PluginToolTouch {
    Down,
    Moved,
    Up,
    /// A pinch came while the touch was down: what it did is undone.
    Cancelled,
}

/// A shape a tool shows over the canvas: its corners (canvas pixels, x and y by turns), whether
/// its last joins its first, and how it looks (0: a guide, thin and dashed; 1: what the touch
/// draws).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PluginToolShape {
    pub points: Vec<f32>,
    pub closed: bool,
    pub look: u32,
}

/// What a tool's answer leaves: the shapes to show over the canvas, what to say, whether the
/// layers changed (the layers panel and the canvas read again), and whether it closed.
#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct PluginToolShown {
    pub shapes: Vec<PluginToolShape>,
    pub said: Option<String>,
    pub changed: bool,
    pub closed: bool,
}

/// An edit a tool made.
#[derive(Clone, Debug, PartialEq)]
enum Edit {
    Shape(LayerId, Vec<Point>),
    Panel { above: Option<LayerId>, shape: Vec<Point>, name: String, like: Option<LayerId>, border: f32, current: bool },
}

/// A tool open: a copy of its plugin's code, its place among its plugin's tools, and the step
/// its edits are making, if they are: the document before them and each frame they changed with
/// its shape then.
pub(crate) struct OpenTool {
    key: String,
    index: u32,
    plugin: String,
    plugins: Arc<Plugins>,
    running: Running,
    mode: u32,
    step: Option<(Document, Shown)>,
}

/// The shapes the edits under way changed, each as it was before them.
type Shown = Vec<(LayerId, Vec<Point>)>;

fn f32s(values: impl IntoIterator<Item = f32>) -> Vec<u8> {
    values.into_iter().flat_map(f32::to_le_bytes).collect()
}

fn f32s_of(bytes: &[u8]) -> Vec<f32> {
    bytes.as_chunks::<4>().0.iter().map(|b| f32::from_le_bytes(*b)).collect()
}

fn u32_of(bytes: &[u8]) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
}

fn u64_of(bytes: &[u8]) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(..8)?.try_into().ok()?))
}

/// A shape's corners, if it is one: three to [`MOST_CORNERS`], each a number.
fn shape_of(bytes: &[u8]) -> Option<Vec<Point>> {
    let corners: Vec<Point> = f32s_of(bytes).as_chunks::<2>().0.to_vec();
    let fits = (3..=MOST_CORNERS).contains(&corners.len()) && corners.iter().flatten().all(|v| v.is_finite());
    fits.then_some(corners)
}

/// The shapes and edits an answer holds, what becomes of them, the step's name, what to say,
/// and whether the tool closes; none if it doesn't hold up.
#[allow(clippy::type_complexity)]
fn read_answer(bytes: &[u8]) -> Option<(Vec<PluginToolShape>, Vec<Edit>, u32, Option<String>, Option<String>, bool)> {
    let (mut shapes, mut edits, mut keep, mut step, mut said, mut close) =
        (Vec::new(), Vec::new(), answer::SHOWN, None, None, false);
    let text = |v: &[u8]| std::str::from_utf8(v).ok().map(str::to_owned).filter(|t| !t.trim().is_empty());
    for (id, value) in fields(bytes) {
        match id {
            answer::SHOW => {
                let mut shape = PluginToolShape { points: Vec::new(), closed: false, look: answer::GUIDE };
                for (id, v) in fields(value) {
                    match id {
                        answer::SHOW_POINTS => shape.points = f32s_of(v),
                        answer::SHOW_CLOSED => shape.closed = v.first().is_some_and(|b| *b != 0),
                        answer::SHOW_LOOK => shape.look = u32_of(v)?,
                        _ => {}
                    }
                }
                let corners = shape.points.len() / 2;
                if !shape.points.len().is_multiple_of(2) || !(2..=MOST_CORNERS).contains(&corners) || shapes.len() >= MOST_SHOWN {
                    return None;
                }
                shapes.push(shape);
            }
            answer::EDIT => {
                let (mut kind, mut layer, mut shape, mut name, mut like, mut border, mut current) =
                    (None, 0, None, String::new(), None, 1.0, false);
                for (id, v) in fields(value) {
                    match id {
                        answer::EDIT_KIND => kind = u32_of(v),
                        answer::EDIT_LAYER => layer = u64_of(v)?,
                        answer::EDIT_POINTS => shape = Some(shape_of(v)?),
                        answer::EDIT_NAME => name = text(v).unwrap_or_default(),
                        answer::EDIT_LIKE => like = Some(LayerId(u64_of(v)?)),
                        answer::EDIT_BORDER => border = f32s_of(v).first().copied().filter(|b| b.is_finite())?,
                        answer::EDIT_CURRENT => current = v.first().is_some_and(|b| *b != 0),
                        _ => {}
                    }
                }
                let shape = shape?;
                edits.push(match kind? {
                    answer::SHAPE => Edit::Shape(LayerId(layer), shape),
                    answer::PANEL => {
                        let above = (layer != 0).then_some(LayerId(layer));
                        let name = if name.chars().count() > 60 { name.chars().take(60).collect() } else { name };
                        Edit::Panel { above, shape, name, like, border, current }
                    }
                    _ => return None,
                });
                if edits.len() > MOST_EDITS {
                    return None;
                }
            }
            answer::KEEP => keep = u32_of(value).filter(|k| *k <= answer::BACK)?,
            answer::STEP => step = text(value),
            answer::SAID => said = text(value),
            answer::CLOSE => close = value.first().is_some_and(|b| *b != 0),
            _ => {}
        }
    }
    Some((shapes, edits, keep, step, said, close))
}

/// The frame `points` drawn over where it is, and where it was (`was`), on a canvas `width` ×
/// `height`.
fn frame_part(points: &[Point], was: Option<&[Point]>, width: u32, height: u32) -> Option<(IRect, Vec<u8>)> {
    let now = panels::shape_rect(points, width, height);
    let before = was.and_then(|w| panels::shape_rect(w, width, height));
    let rect = match (now, before) {
        (Some(a), Some(b)) => a.union(b),
        (a, b) => a.or(b)?,
    };
    Some((rect, panels::draw_frame(points, rect)))
}

/// The shape of frame `id` in `doc`.
fn shape(doc: &Document, id: LayerId) -> Option<Vec<Point>> {
    doc.layer(id)?.panel.as_ref().map(|p| p.points.clone())
}

impl State {
    /// What the open tool is told now, `what` happening at canvas point `at`.
    fn tool_told(&self, mode: u32, what: u32, at: Option<Vec2>) -> Vec<u8> {
        let doc = &self.doc;
        let zoom = self.view.map_or(1.0, |v| v.to_screen().scale_factor()).max(1e-6);
        let mut r = Record::default();
        r.field(told::WHAT, &what.to_le_bytes())
            .field(told::MODE, &mode.to_le_bytes())
            .field(told::PER_POINT, &(self.display_scale / zoom).to_le_bytes())
            .field(told::SIZE, &[doc.width().to_le_bytes(), doc.height().to_le_bytes()].concat())
            .field(told::ANIMATED, &[u8::from(doc.timeline.is_some() || doc.animation.is_some())]);
        if let Some(at) = at {
            r.field(told::AT, &f32s([at.x, at.y]));
        }
        for (id, frame) in doc.panel_frames().into_iter().filter(|(id, _)| doc.panel_of(*id).is_some()) {
            let mut f = Record::default();
            f.field(told::FRAME_LAYER, &id.0.to_le_bytes())
                .field(told::FRAME_POINTS, &f32s(frame.points.iter().flatten().copied()));
            r.field(told::FRAME, &f.bytes());
        }
        r.bytes()
    }

    /// The shapes the edits under way set made as they were, their pixels drawn again so; the
    /// document as it was before them, should the panels they made be gone too.
    fn tool_back(&mut self) {
        let Some((before, was)) = self.plugin_tool.as_mut().and_then(|t| t.step.take()) else { return };
        let (w, h) = (self.doc.width(), self.doc.height());
        for (id, shape_was) in &was {
            let now = shape(&self.doc, *id);
            if let (Some((rect, px)), Some(canvas)) =
                (frame_part(shape_was, now.as_deref(), w, h), self.canvas.as_mut())
            {
                canvas.put_layer_region(*id, rect.x0 as u32, rect.y0 as u32, rect.width(), rect.height(), px);
            }
        }
        self.doc = before;
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.invalidate();
        }
    }

    /// `edits` made (`keep`: as one step named `step`, with those shown since the touch began);
    /// whether the layers changed.
    fn tool_edit(&mut self, edits: Vec<Edit>, keep: u32, step: Option<String>) -> bool {
        if keep == answer::BACK {
            let changed = self.plugin_tool.as_ref().is_some_and(|t| t.step.is_some());
            self.tool_back();
            return changed;
        }
        if edits.is_empty() && (keep == answer::SHOWN || self.plugin_tool.as_ref().is_none_or(|t| t.step.is_none())) {
            return false;
        }
        self.finish_stroke();
        self.settle();
        let (w, h) = (self.doc.width(), self.doc.height());
        let before = match self.plugin_tool.as_mut().and_then(|t| t.step.take()) {
            Some(step) => step,
            None => (self.doc.clone(), Vec::new()),
        };
        let (before_doc, mut was) = before;
        // shapes set, drawn at once
        let mut made = Vec::new();
        for edit in edits {
            match edit {
                Edit::Shape(id, points) => {
                    let Some(now) = shape(&self.doc, id) else { continue };
                    if self.doc.panel_of(id).is_none() {
                        continue;
                    }
                    if !was.iter().any(|(f, _)| *f == id) {
                        was.push((id, now.clone()));
                    }
                    self.doc.set_panel_shape(id, points);
                    let points = shape(&self.doc, id).unwrap_or_default();
                    if let (Some((rect, px)), Some(canvas)) =
                        (frame_part(&points, Some(&now), w, h), self.canvas.as_mut())
                    {
                        canvas.put_layer_region(id, rect.x0 as u32, rect.y0 as u32, rect.width(), rect.height(), px);
                    }
                }
                panel @ Edit::Panel { .. } => made.push(panel),
            }
        }
        if keep == answer::SHOWN {
            if let Some(tool) = self.plugin_tool.as_mut() {
                tool.step = Some((before_doc, was));
            }
            return true;
        }
        // kept: the panels made, then every frame changed as one step
        let current = self.doc.current();
        let mut current_now = None;
        let mut new_frames = Vec::new();
        for edit in made {
            let Edit::Panel { above, shape: points, name, like, border, current: becomes } = edit else { continue };
            let above = match above {
                Some(frame) => match self.doc.panel_of(frame) {
                    Some(group) => Some(group),
                    None => continue,
                },
                None => None,
            };
            let style = like
                .and_then(|f| self.doc.layer(f))
                .filter(|l| l.panel.is_some())
                .and_then(|l| l.style)
                .unwrap_or_else(|| panels::border_style(border));
            let Some(panel) = self.doc.add_panel(above, points, style, &name) else { continue };
            new_frames.push(panel.frame);
            if becomes {
                current_now = Some(panel.art);
            }
        }
        self.doc.set_current(current_now.unwrap_or(current));
        let mut parts = Vec::new();
        if let Some(canvas) = self.canvas.as_mut() {
            for (id, shape_was) in &was {
                let Some(now) = shape(&self.doc, *id) else { continue };
                let Some((rect, px)) = frame_part(&now, Some(shape_was), w, h) else { continue };
                // the pixels as they were, for the step to go back to
                if let Some((_, old)) = frame_part(shape_was, Some(&now), w, h) {
                    canvas.put_layer_region(*id, rect.x0 as u32, rect.y0 as u32, rect.width(), rect.height(), old);
                }
                parts.push((*id, rect, px));
            }
        }
        for id in new_frames {
            if let Some((rect, px)) = shape(&self.doc, id).and_then(|s| frame_part(&s, None, w, h)) {
                parts.push((id, rect, px));
            }
        }
        if parts.is_empty() && self.doc.layers().len() == before_doc.layers().len() {
            self.doc = before_doc;
            return false;
        }
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.name_next_step(step.as_deref().unwrap_or("edit layers"));
            canvas.write_layer_parts_and_document(parts, before_doc);
        }
        self.revision += 1;
        true
    }
}

impl CanvasSession {
    /// The open tool told `what` (at window point `at`), and its answer made: what it leaves.
    fn tool_ask(&self, what: u32, at: Option<(f32, f32)>) -> PluginToolShown {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let at = at.and_then(|(x, y)| st.window_to_canvas(x, y));
        let Some(mode) = st.plugin_tool.as_ref().map(|t| t.mode) else {
            return PluginToolShown { closed: true, ..PluginToolShown::default() };
        };
        let told = st.tool_told(mode, what, at);
        let Some(tool) = st.plugin_tool.as_mut() else { return PluginToolShown::default() };
        let index = tool.index;
        let answer = match tool.running.tool_touch(index, &told) {
            Ok(answer) => answer,
            Err(stop) => {
                // stopped: what it was doing goes back, and it closes
                tool.plugins.part_stopped(&tool.plugin, stop);
                st.tool_back();
                st.plugin_tool = None;
                self.shared.frame(st);
                return PluginToolShown {
                    said: Some(stop.text().to_owned()),
                    changed: true,
                    closed: true,
                    ..Default::default()
                };
            }
        };
        let Some((shapes, edits, keep, step, said, close)) = answer.as_deref().and_then(read_answer) else {
            return PluginToolShown::default();
        };
        let changed = st.tool_edit(edits, keep, step);
        if close {
            st.tool_back();
            st.plugin_tool = None;
        }
        self.shared.frame(st);
        PluginToolShown { shapes, said, changed, closed: close }
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Opens tool `key` of `plugins` (as [`Plugins::tools`] lists it): its code started on a copy
    /// of its own, told it was opened. What it shows, or why it couldn't open.
    pub fn plugin_tool_open(&self, plugins: Arc<Plugins>, key: String) -> PluginToolShown {
        let code = match plugins.tool_to_run(&key) {
            Ok(code) => code,
            Err(why) => return PluginToolShown { said: Some(why), closed: true, ..PluginToolShown::default() },
        };
        {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.tool_back();
            st.plugin_tool = Some(OpenTool {
                key,
                index: code.index,
                plugin: code.plugin,
                plugins,
                running: code.running,
                mode: 0,
                step: None,
            });
        }
        self.tool_ask(told::OPENED, None)
    }

    /// The open tool's key, if one is open.
    pub fn plugin_tool_key(&self) -> Option<String> {
        self.shared.lock().plugin_tool.as_ref().map(|t| t.key.clone())
    }

    /// A touch on the open tool at window point (`x`, `y`), with its mode `mode` (by its place
    /// among the tool's) chosen: what it leaves.
    pub fn plugin_tool_touch(&self, touch: PluginToolTouch, x: f32, y: f32, mode: u32) -> PluginToolShown {
        if let Some(tool) = self.shared.lock().plugin_tool.as_mut() {
            tool.mode = mode;
        }
        let what = match touch {
            PluginToolTouch::Down => told::DOWN,
            PluginToolTouch::Moved => told::MOVED,
            PluginToolTouch::Up => told::UP,
            PluginToolTouch::Cancelled => told::CANCELLED,
        };
        self.tool_ask(what, (x.is_finite() && y.is_finite()).then_some((x, y)))
    }

    /// The artwork changed under the open tool (an undo, a redo): what it shows now.
    pub fn plugin_tool_again(&self) -> PluginToolShown {
        self.tool_ask(told::AGAIN, None)
    }

    /// Closes the open tool; what it was doing in a touch goes back.
    pub fn plugin_tool_close(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.tool_back();
        st.plugin_tool = None;
        self.shared.frame(st);
    }

    /// Fits the whole canvas between `left`, `top`, `right` and `bottom` (window pixels), as a
    /// tool that asks for it shows the canvas under its header.
    pub fn plugin_tool_fit(&self, left: f32, top: f32, right: f32, bottom: f32) {
        if ![left, top, right, bottom].iter().all(|v| v.is_finite()) {
            return;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.view_motion = None;
        let size = ((right - left).max(1.0) as u32, (bottom - top).max(1.0) as u32);
        let fitted = View::fit((st.doc.width(), st.doc.height()), size);
        st.view = Some(View::from_affine(fitted.to_screen().then(Affine::translate(Vec2::new(left, top)))));
        self.shared.frame(st);
    }
}

#[cfg(test)]
mod tests {
    use procraft_gpu::Gpu;

    use super::*;
    use crate::plugin_warps::plugins_on;

    /// The Comic Panels plugin, and its tool.
    const COMIC: &str = "io.github.artcraftandroid.comic-panels";

    fn session(side: u32) -> Option<Arc<CanvasSession>> {
        let gpu = match Gpu::headless() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipped: {e}");
                return None;
            }
        };
        let s = CanvasSession::new(side, side);
        {
            let mut st = s.shared.lock();
            st.window_size = (side, side);
            st.view = Some(View::fit((side, side), (side, side)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        Some(s)
    }

    fn pixel(s: &CanvasSession, x: u32, y: u32) -> [u8; 4] {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        let px = canvas.read_composite(gpu).unwrap();
        let i = ((y * st.doc.width() + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    fn layer_pixel(s: &CanvasSession, id: LayerId, x: u32, y: u32) -> [u8; 4] {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        let px = canvas.read_layer(gpu, id).unwrap();
        let i = ((y * st.doc.width() + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    /// The canvas's panels' frames and their shapes, bottom first.
    fn frames(s: &CanvasSession) -> Vec<(LayerId, Vec<Point>)> {
        let st = s.shared.lock();
        st.doc.panel_frames().into_iter().map(|(id, f)| (id, f.points.clone())).collect()
    }

    /// Comic Panels opened on `s`, the plugin on.
    fn open(s: &CanvasSession) -> (Arc<Plugins>, PluginToolShown) {
        let plugins = plugins_on(COMIC);
        let tools = plugins.tools();
        assert_eq!(
            tools.iter().map(|t| (t.label.as_str(), t.modes.len(), t.fit)).collect::<Vec<_>>(),
            [("Comic panels", 2, true)]
        );
        let shown = s.plugin_tool_open(plugins.clone(), format!("{COMIC}.panels"));
        (plugins, shown)
    }

    /// A drag from (`ax`, `ay`) to (`bx`, `by`), window pixels, in `mode`, let go; what it left.
    fn drag(s: &CanvasSession, mode: u32, (ax, ay): (f32, f32), (bx, by): (f32, f32)) -> PluginToolShown {
        s.plugin_tool_touch(PluginToolTouch::Down, ax, ay, mode);
        s.plugin_tool_touch(PluginToolTouch::Moved, bx, by, mode);
        s.plugin_tool_touch(PluginToolTouch::Up, bx, by, mode)
    }

    const CUT: u32 = 0;
    const MOVE: u32 = 1;

    #[test]
    fn opened_it_makes_a_page_whose_panel_is_white_with_a_black_border_and_cuts_undo_in_one_step() {
        let Some(s) = session(400) else { return };
        // the background shown grey, so the panel's white tells apart
        s.set_background_color(0.5, 0.5, 0.5, true);
        let (plugins, shown) = open(&s);
        assert!(shown.changed && !shown.closed && shown.said.is_none());
        assert_eq!(shown.shapes.len(), 1);
        let panels = frames(&s);
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].1[0], [28.0, 28.0]);
        // its art current, clipped to its frame
        let art = s.shared.lock().doc.current();
        assert_eq!(s.shared.lock().doc.clip_base(art), Some(panels[0].0));
        // opened again, it makes none
        s.plugin_tool_close();
        assert!(!s.plugin_tool_open(plugins, format!("{COMIC}.panels")).changed);
        assert_eq!(frames(&s).len(), 1);
        // inside white, the border just outside the frame black, the margin the background
        assert_eq!(pixel(&s, 200, 200)[..3], [255, 255, 255]);
        let edge = pixel(&s, 27, 200);
        assert!(edge[0] < 100, "{edge:?}");
        assert!(pixel(&s, 10, 200)[0].abs_diff(128) < 4);
        // a cut down the middle: a gap of the background between two panels
        let cut = drag(&s, CUT, (200.0, 0.0), (201.0, 400.0));
        assert!(cut.changed);
        assert_eq!(cut.shapes.len(), 2);
        let panels = frames(&s);
        assert_eq!(panels.len(), 2);
        assert!(pixel(&s, 200, 200)[0].abs_diff(128) < 4);
        assert_eq!(pixel(&s, 100, 200)[..3], [255, 255, 255]);
        assert_eq!(pixel(&s, 300, 200)[..3], [255, 255, 255]);
        // the cut frame cleared where it no longer is
        assert_eq!(layer_pixel(&s, panels[0].0, 200, 200)[3], 0);
        // missing every panel cuts nothing
        assert!(!drag(&s, CUT, (1.0, 1.0), (2.0, 2.0)).changed);
        assert_eq!(s.undo().as_deref(), Some("cut panels"));
        assert_eq!(s.plugin_tool_again().shapes.len(), 1);
        assert_eq!(frames(&s).len(), 1);
        assert_eq!(pixel(&s, 200, 200)[..3], [255, 255, 255]);
        assert_eq!(s.undo().as_deref(), Some("comic page"));
        assert!(frames(&s).is_empty());
        assert!(pixel(&s, 200, 200)[0].abs_diff(128) < 4);
    }

    #[test]
    fn a_line_dragged_shows_straightened_until_it_is_let_go() {
        let Some(s) = session(300) else { return };
        open(&s);
        s.plugin_tool_touch(PluginToolTouch::Down, 10.0, 100.0, CUT);
        let shown = s.plugin_tool_touch(PluginToolTouch::Moved, 290.0, 104.0, CUT);
        let line = shown.shapes.iter().find(|shape| shape.look == answer::DRAWN).unwrap();
        assert!(!line.closed && (line.points[1] - line.points[3]).abs() < 0.01, "{line:?}");
        assert_eq!(frames(&s).len(), 1, "nothing cut yet");
        // a pinch cancels it
        assert!(!s.plugin_tool_touch(PluginToolTouch::Cancelled, 290.0, 104.0, CUT).changed);
        assert_eq!(frames(&s).len(), 1);
        assert!(drag(&s, CUT, (10.0, 100.0), (290.0, 104.0)).changed);
        assert_eq!(frames(&s).len(), 2);
    }

    #[test]
    fn a_gutter_dragged_moves_both_panels_edges_live_and_undoes_in_one_step() {
        let Some(s) = session(300) else { return };
        s.set_background_color(0.5, 0.5, 0.5, true);
        open(&s);
        // two panels, 21..147 and 153..279 across
        assert!(drag(&s, CUT, (150.0, 0.0), (150.0, 300.0)).changed);
        let grey = |px: [u8; 4]| px[0].abs_diff(128) < 4;
        assert!(grey(pixel(&s, 150, 150)));
        // nothing to take in the middle of a panel
        assert!(!drag(&s, MOVE, (80.0, 150.0), (120.0, 150.0)).changed);
        s.plugin_tool_touch(PluginToolTouch::Down, 150.0, 150.0, MOVE);
        assert!(s.plugin_tool_touch(PluginToolTouch::Moved, 200.0, 150.0, MOVE).changed);
        // shown as it moves, the gap now at 200
        assert_eq!(pixel(&s, 150, 150)[..3], [255, 255, 255]);
        assert!(grey(pixel(&s, 200, 150)));
        assert!(s.plugin_tool_touch(PluginToolTouch::Up, 200.0, 150.0, MOVE).changed);
        let edges: Vec<(f32, f32)> = frames(&s)
            .iter()
            .map(|(_, p)| p.iter().map(|c| c[0]).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x))))
            .collect();
        assert!(edges.iter().any(|e| (e.1 - 197.0).abs() < 0.5), "{edges:?}");
        assert!(edges.iter().any(|e| (e.0 - 203.0).abs() < 0.5), "{edges:?}");
        assert!(grey(pixel(&s, 200, 150)));
        assert_eq!(pixel(&s, 150, 150)[..3], [255, 255, 255]);
        assert_eq!(s.undo().as_deref(), Some("move gutter"));
        assert!(grey(pixel(&s, 150, 150)));
        assert_eq!(pixel(&s, 200, 150)[..3], [255, 255, 255]);
        s.redo();
        assert!(grey(pixel(&s, 200, 150)));
        // a drag cancelled by a pinch goes back
        s.plugin_tool_touch(PluginToolTouch::Down, 200.0, 150.0, MOVE);
        s.plugin_tool_touch(PluginToolTouch::Moved, 150.0, 150.0, MOVE);
        assert_eq!(pixel(&s, 200, 150)[..3], [255, 255, 255]);
        s.plugin_tool_touch(PluginToolTouch::Cancelled, 150.0, 150.0, MOVE);
        assert!(grey(pixel(&s, 200, 150)));
        assert_eq!(pixel(&s, 150, 150)[..3], [255, 255, 255]);
        // as does one under way as the tool closes
        s.plugin_tool_touch(PluginToolTouch::Down, 200.0, 150.0, MOVE);
        s.plugin_tool_touch(PluginToolTouch::Moved, 150.0, 150.0, MOVE);
        s.plugin_tool_close();
        assert!(grey(pixel(&s, 200, 150)));
    }

    #[test]
    fn a_page_shows_as_it_was_made_with_the_plugin_off_and_its_art_only_inside_its_frame() {
        let Some(s) = session(200) else { return };
        let (plugins, _) = open(&s);
        let art = s.shared.lock().doc.current();
        s.plugin_tool_close();
        // the whole art layer filled red: only the panel shows it
        s.set_color(1.0, 0.0, 0.0);
        assert!(s.fill_layer(art.0));
        assert!(plugins.set_on(COMIC.to_owned(), false));
        assert!(plugins.tools().is_empty());
        let inside = pixel(&s, 100, 100);
        assert!(inside[0] > 240 && inside[1] < 20, "{inside:?}");
        let margin = pixel(&s, 3, 100);
        assert!(margin[0] > 240 && margin[1] > 240, "{margin:?}");
        // and kept so in the file
        let doc = s.shared.lock().doc.clone();
        let back: Document = serde_json::from_str(&serde_json::to_string(&doc).unwrap()).unwrap();
        assert_eq!(back.panel_frames().len(), 1);
        // off, it can't be opened
        let shown = s.plugin_tool_open(plugins, format!("{COMIC}.panels"));
        assert!(shown.closed && shown.said.is_some());
    }

    #[test]
    fn it_says_why_it_makes_no_page_of_an_animation() {
        let Some(s) = session(200) else { return };
        s.shared.lock().doc.animation = Some(Default::default());
        let (_, shown) = open(&s);
        assert!(shown.closed && shown.said.is_some() && !shown.changed);
        assert!(frames(&s).is_empty() && s.plugin_tool_key().is_none());
    }

    fn points(shape: &[Point]) -> Vec<u8> {
        f32s(shape.iter().flatten().copied())
    }

    #[test]
    fn an_answer_is_read_and_one_that_does_not_hold_up_is_refused() {
        let square = [[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        let mut show = Record::default();
        show.field(answer::SHOW_POINTS, &points(&square[..2])).field(answer::SHOW_LOOK, &answer::DRAWN.to_le_bytes());
        let mut edit = Record::default();
        edit.field(answer::EDIT_KIND, &answer::PANEL.to_le_bytes())
            .field(answer::EDIT_LAYER, &0_u64.to_le_bytes())
            .field(answer::EDIT_POINTS, &points(&square))
            .field(answer::EDIT_NAME, b"Panel 1")
            .field(answer::EDIT_CURRENT, &[1]);
        let mut r = Record::default();
        r.field(answer::SHOW, &show.bytes())
            .field(answer::EDIT, &edit.bytes())
            .field(answer::KEEP, &answer::KEPT.to_le_bytes())
            .field(answer::STEP, b"comic page");
        let (shapes, edits, keep, step, said, close) = read_answer(&r.bytes()).unwrap();
        assert_eq!(shapes, [PluginToolShape { points: vec![0.0, 0.0, 10.0, 0.0], closed: false, look: answer::DRAWN }]);
        assert_eq!(
            edits,
            [Edit::Panel {
                above: None,
                shape: square.to_vec(),
                name: "Panel 1".into(),
                like: None,
                border: 1.0,
                current: true
            }]
        );
        assert_eq!((keep, step.as_deref(), said, close), (answer::KEPT, Some("comic page"), None, false));
        // a shape of two corners, a kind this app hasn't, or a keep it hasn't
        let mut bad = Record::default();
        let mut two = Record::default();
        two.field(answer::EDIT_KIND, &answer::SHAPE.to_le_bytes()).field(answer::EDIT_POINTS, &points(&square[..2]));
        bad.field(answer::EDIT, &two.bytes());
        assert!(read_answer(&bad.bytes()).is_none());
        let mut odd = Record::default();
        let mut kind = Record::default();
        kind.field(answer::EDIT_KIND, &9_u32.to_le_bytes()).field(answer::EDIT_POINTS, &points(&square));
        odd.field(answer::EDIT, &kind.bytes());
        assert!(read_answer(&odd.bytes()).is_none());
        let mut keep = Record::default();
        keep.field(answer::KEEP, &7_u32.to_le_bytes());
        assert!(read_answer(&keep.bytes()).is_none());
    }
}

//! A plugin's tool (interface 24), opened from Actions → Canvas, as the iPad's (PluginTools.swift):
//! Comic Panels is one, from its plugin. The canvas under the tool's header (the whole of it fitted
//! there, 24 in from the window's sides and foot, for a tool that asks), with what the plugin shows
//! drawn over it. A drag's presses go to the plugin, which answers with what to show and its edits
//! to the layers (each kept one undo step); the header's modes, Undo and Done are the app's. The
//! wheel, a trackpad's pinch or two fingers zoom, cancelling the drag under way. With the plugin
//! off, what it made stays as it was made: Comic Panels' panels are groups, each a frame and the
//! art clipped to it.

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Shape, Stroke, pos2};
use procraft_core::{CanvasSession, PluginTool, PluginToolShape, PluginToolShown, PluginToolTouch, Plugins};
use std::sync::Arc;

use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::tool_header::{self, Tapped};

/// The hint under the header, as the iPad's dimmed text.
const HINT: Themed = Themed(0x99FFFFFF, 0x99000000);
/// The canvas's room for a tool that fits it: this far in from the window's sides and foot, and
/// below the header (points).
const SIDE: f32 = 24.0;
const UNDER_HEADER: f32 = 16.0;
/// The wheel zooms by this much a notch, and a trackpad's scroll this much a point (as on the
/// canvas, input.rs).
const WHEEL_ZOOM: f32 = 1.15;
const SCROLL_ZOOM: f32 = 0.005;

/// What the tool asked of the interface.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Asked {
    /// The header's Undo.
    Undo,
    /// Something to tell, and whether it is a warning.
    Said(String, bool),
}

/// A drag on the canvas: whether a zoom came during it (it is cancelled, and nothing more goes).
struct Drag {
    zoomed: bool,
}

#[derive(Default)]
pub struct PluginToolWindow {
    /// The tool open, if one is.
    pub open: Option<PluginTool>,
    /// Its mode chosen, by its place among the tool's.
    mode: u32,
    /// What it shows over the canvas.
    pub shapes: Vec<PluginToolShape>,
    /// The window (points) the canvas was last fitted in.
    fitted: Option<Rect>,
    drag: Option<Drag>,
}

impl PluginToolWindow {
    /// Opens `tool` of `plugins`, which may change the layers as it opens (Comic Panels makes a
    /// page). What to tell, and whether it is a warning (it couldn't open, say).
    pub fn begin(
        &mut self,
        session: &CanvasSession,
        plugins: &Arc<Plugins>,
        tool: PluginTool,
    ) -> Option<(String, bool)> {
        let shown = session.plugin_tool_open(Arc::clone(plugins), tool.key.clone());
        let said = shown.said.clone().map(|s| (s, shown.closed));
        if !shown.closed {
            self.open = Some(tool);
            self.mode = 0;
            self.shapes = shown.shapes;
            self.fitted = None;
            self.drag = None;
        }
        said
    }

    /// Closes it; a tool that fitted the canvas leaves it fitted to the window again.
    pub fn end(&mut self, session: &CanvasSession) {
        session.plugin_tool_close();
        self.drag = None;
        self.shapes.clear();
        if self.open.take().is_some_and(|t| t.fit) {
            session.fit_view();
        }
    }

    /// What the open tool shows, asked again (an undo, a redo).
    pub fn again(&mut self, session: &CanvasSession) {
        if self.open.is_none() {
            return;
        }
        let shown = session.plugin_tool_again();
        self.answered(shown);
    }

    /// What an answer leaves: the shapes, or the tool closed. What it said, if anything.
    fn answered(&mut self, shown: PluginToolShown) -> Option<String> {
        if shown.closed {
            self.open = None;
            self.drag = None;
            self.shapes.clear();
        } else {
            self.shapes = shown.shapes;
        }
        shown.said
    }

    /// Shows it while it is open: what the plugin shows, the header and the mode's hint. What the
    /// tool asked, if anything.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) -> Option<Asked> {
        let tool = self.open.clone()?;
        let screen = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        // the whole canvas under the header, fitted again when the window changes
        if tool.fit && self.fitted != Some(screen) {
            let top = screen.top() + tool_header::HEIGHT + UNDER_HEADER;
            session.plugin_tool_fit(
                (screen.left() + SIDE) * ppp,
                top * ppp,
                (screen.right() - SIDE) * ppp,
                (screen.bottom() - SIDE) * ppp,
            );
            self.fitted = Some(screen);
        }
        let mut said = None;
        egui::Area::new(Id::unique("plugin tool")).fixed_pos(screen.min).order(Order::Middle).show(ctx, |ui| {
            // the whole window takes the drags; nothing reaches the canvas
            let response = ui.allocate_rect(screen, Sense::click_and_drag());
            self.zoom(ui, &response, session, ppp);
            said = self.drag(ui, &response, session, ppp);
            self.draw(ui, session, ppp);
        });
        if self.open.is_none() {
            // the plugin closed it as it answered
            return said.map(|s| Asked::Said(s, true));
        }
        let mode = tool.modes.get(self.mode as usize);
        if let Some(hint) = mode.map(|m| m.hint.as_str()).filter(|h| !h.is_empty()) {
            egui::Area::new(Id::unique("plugin tool hint"))
                .fixed_pos(screen.min)
                .order(Order::Foreground)
                .interactable(false)
                .show(ctx, |ui| {
                    let at = pos2(screen.center().x, screen.top() + tool_header::HEIGHT + 8.0);
                    text(ui.painter(), at, Align2::CENTER_TOP, hint, 15.0, Weight::Regular, argb(HINT));
                });
        }
        let buttons: Vec<(&str, bool)> = tool
            .modes
            .iter()
            .enumerate()
            .map(|(i, m)| (m.label.as_str(), i as u32 == self.mode))
            .chain([("Undo", false)])
            .collect();
        let tapped = tool_header::show(ctx, look, &tool.label, &buttons);
        let modes = tool.modes.len();
        match tapped {
            Some(Tapped::Button(i)) if i < modes => {
                self.mode = i as u32;
                said.map(|s| Asked::Said(s, true))
            }
            Some(Tapped::Button(_)) => Some(Asked::Undo),
            Some(Tapped::Done) => {
                self.end(session);
                None
            }
            None if crate::ui::escape(ctx) => {
                self.end(session);
                None
            }
            None => said.map(|s| Asked::Said(s, true)),
        }
    }

    /// The wheel, a trackpad's pinch or two fingers zoom the canvas about where they are; a drag
    /// under way is cancelled.
    fn zoom(&mut self, ui: &egui::Ui, response: &egui::Response, session: &CanvasSession, ppp: f32) {
        if !response.hovered() && !response.dragged() {
            return;
        }
        let (mut by, at, touch) = ui.input(|i| {
            let mut by = 1.0f32;
            for e in &i.events {
                match e {
                    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } => {
                        by *= WHEEL_ZOOM.powf(delta.y);
                    }
                    egui::Event::MouseWheel { delta, .. } => by *= (delta.y * SCROLL_ZOOM).exp(),
                    egui::Event::Zoom(z) => by *= z,
                    _ => {}
                }
            }
            (by, i.pointer.latest_pos(), i.multi_touch())
        });
        let mut pivot = at;
        if let Some(t) = touch {
            by *= t.zoom_delta;
            pivot = Some(t.center_pos);
        }
        if (by - 1.0).abs() < 1e-4 || !by.is_finite() {
            return;
        }
        let Some(p) = pivot else { return };
        // what a drag under way did goes back
        if let Some(d) = self.drag.as_mut()
            && !d.zoomed
        {
            d.zoomed = true;
            let shown = session.plugin_tool_touch(PluginToolTouch::Cancelled, p.x * ppp, p.y * ppp, self.mode);
            self.answered(shown);
        }
        session.view_gesture(p.x * ppp, p.y * ppp, 0.0, 0.0, by.max(0.01), 0.0);
    }

    /// A press at `at` (points) told to the tool in its mode chosen. What it said, if anything.
    fn touch(&mut self, session: &CanvasSession, touch: PluginToolTouch, at: Pos2, ppp: f32) -> Option<String> {
        let shown = session.plugin_tool_touch(touch, at.x * ppp, at.y * ppp, self.mode);
        self.answered(shown)
    }

    /// A drag on the canvas, told to the tool: down where it began, each move, up where it ended
    /// (unless a zoom came during it). What the tool said, if anything.
    fn drag(&mut self, ui: &egui::Ui, response: &egui::Response, session: &CanvasSession, ppp: f32) -> Option<String> {
        let mut said = None;
        if response.drag_started()
            && let Some(from) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
        {
            self.drag = Some(Drag { zoomed: false });
            said = self.touch(session, PluginToolTouch::Down, from, ppp).or(said);
        }
        if response.dragged()
            && self.drag.as_ref().is_some_and(|d| !d.zoomed)
            && let Some(at) = response.interact_pointer_pos()
        {
            said = self.touch(session, PluginToolTouch::Moved, at, ppp).or(said);
        }
        if response.drag_stopped()
            && let Some(d) = self.drag.take()
            && !d.zoomed
        {
            let at = response.interact_pointer_pos().or_else(|| ui.input(|i| i.pointer.latest_pos()));
            if let Some(at) = at {
                said = self.touch(session, PluginToolTouch::Up, at, ppp).or(said);
            }
        }
        said
    }

    /// What the tool shows: a guide thin and dashed; what a drag draws white under the accent,
    /// round at its ends.
    fn draw(&self, ui: &egui::Ui, session: &CanvasSession, ppp: f32) {
        let painter = ui.painter();
        let m = session.view_matrix();
        let get = |i: usize| m.get(i).copied().unwrap_or(0.0);
        let (a, b, c, d, e, f) = (get(0), get(1), get(2), get(3), get(4), get(5));
        let on_screen = |x: f32, y: f32| pos2((a * x + c * y + e) / ppp, (b * x + d * y + f) / ppp);
        for shape in &self.shapes {
            let mut corners: Vec<Pos2> =
                shape.points.as_chunks::<2>().0.iter().map(|[x, y]| on_screen(*x, *y)).collect();
            if shape.closed
                && let Some(&first) = corners.first()
            {
                corners.push(first);
            }
            if shape.look == 0 {
                let outline = Stroke::new(1.5, argb(pc::ACCENT).gamma_multiply(0.8));
                painter.extend(Shape::dashed_line(&corners, outline, 6.0, 4.0));
                continue;
            }
            for (width, colour) in [(5.0, Color32::WHITE), (3.0, argb(pc::ACCENT))] {
                painter.add(Shape::line(corners.clone(), Stroke::new(width, colour)));
                // round ends
                for end in [corners.first(), corners.last()].into_iter().flatten() {
                    painter.circle_filled(*end, width / 2.0, colour);
                }
            }
        }
    }
}

//! Comic Panels, as the iPad app's (ComicPanels.swift), opened from Actions → Canvas: the canvas
//! made a comic page the first time (one panel inside a margin, white with a black border), then
//! the whole page fitted under the tool's header with each panel outlined. A line dragged across
//! panels cuts each in two, straight across or down when near it, with a gutter between. With Move
//! chosen, a gutter dragged moves the panels' edges on both sides of it (the page's edge alone, its
//! margin). The wheel, a trackpad's pinch or two fingers zoom; a zoom during a drag cuts or moves
//! nothing. Each panel is a group in the layers: its frame (locked) and the art above it, clipped
//! to it. The panels themselves are the core's (panels.rs).

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Shape, Stroke, pos2};
use procraft_core::CanvasSession;

use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::tool_header::{self, Tapped};

/// The hint under the header, as the iPad's dimmed text.
const HINT: Themed = Themed(0x99FFFFFF, 0x99000000);
/// The page's room: this far in from the window's sides and foot, and below the header (points).
const SIDE: f32 = 24.0;
const UNDER_HEADER: f32 = 16.0;
/// The wheel zooms by this much a notch, and a trackpad's scroll this much a point (as on the
/// canvas, input.rs).
const WHEEL_ZOOM: f32 = 1.15;
const SCROLL_ZOOM: f32 = 0.005;
/// The notice when the canvas can't be made a page.
pub const CANNOT: &str = "Comic panels can't be added to this canvas.";

/// What the header asked of the interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Asked {
    Undo,
    Closed,
}

/// A drag on the page: where it began and is now (points), whether it took a gutter (Move), and
/// whether a zoom came during it (it does nothing then).
struct Drag {
    from: Pos2,
    to: Pos2,
    grabbed: bool,
    zoomed: bool,
}

#[derive(Default)]
pub struct ComicPanels {
    pub open: bool,
    /// Move is chosen in the header: a drag takes a gutter (or the page's edge) instead of cutting.
    moving: bool,
    /// The window (points) the page was last fitted in.
    fitted: Option<Rect>,
    drag: Option<Drag>,
}

impl ComicPanels {
    /// Opens it, making the canvas a page first if it has no panels yet; false (the canvas as it
    /// was) if it can't be one (animated, say).
    pub fn begin(&mut self, session: &CanvasSession) -> bool {
        if session.comic_panels().is_empty() && session.add_comic_page().is_none() {
            return false;
        }
        self.open = true;
        self.moving = false;
        self.fitted = None;
        self.drag = None;
        true
    }

    /// Closes it, the canvas fitted to the window again.
    pub fn end(&mut self, session: &CanvasSession) {
        if self.drag.take().is_some_and(|d| d.grabbed) {
            session.gutter_end(false);
        }
        self.open = false;
        session.fit_view();
    }

    /// Shows it while it is open: the panels outlined, the cut being dragged, the header and its
    /// hint. What the header asked, if anything.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) -> Option<Asked> {
        if !self.open {
            return None;
        }
        let screen = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        // the whole page under the header, fitted again when the window changes
        if self.fitted != Some(screen) {
            let top = screen.top() + tool_header::HEIGHT + UNDER_HEADER;
            session.panel_tool_fit(
                (screen.left() + SIDE) * ppp,
                top * ppp,
                (screen.right() - SIDE) * ppp,
                (screen.bottom() - SIDE) * ppp,
            );
            self.fitted = Some(screen);
        }
        egui::Area::new(Id::unique("comic panels")).fixed_pos(screen.min).order(Order::Middle).show(ctx, |ui| {
            // the whole window takes the drags; nothing reaches the canvas
            let response = ui.allocate_rect(screen, Sense::click_and_drag());
            self.zoom(ui, &response, session, ppp);
            self.drag(ui, &response, session, ppp);
            self.draw(ui, session, ppp);
        });
        let hint = if self.moving {
            "Drag a gutter, or the page's edge, to move it"
        } else {
            "Drag across panels to cut them"
        };
        egui::Area::new(Id::unique("comic panels hint"))
            .fixed_pos(screen.min)
            .order(Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                let at = pos2(screen.center().x, screen.top() + tool_header::HEIGHT + 8.0);
                text(ui.painter(), at, Align2::CENTER_TOP, hint, 15.0, Weight::Regular, argb(HINT));
            });
        let buttons = [("Cut", !self.moving), ("Move", self.moving), ("Undo", false)];
        let tapped = tool_header::show(ctx, look, "Comic Panels", &buttons);
        let asked = match tapped {
            Some(Tapped::Button(0)) => {
                self.moving = false;
                None
            }
            Some(Tapped::Button(1)) => {
                self.moving = true;
                None
            }
            Some(Tapped::Button(_)) => Some(Asked::Undo),
            Some(Tapped::Done) => Some(Asked::Closed),
            None => crate::ui::escape(ctx).then_some(Asked::Closed),
        };
        if asked == Some(Asked::Closed) {
            self.end(session);
        }
        asked
    }

    /// The wheel, a trackpad's pinch or two fingers zoom the page about where they are; during a
    /// drag, it then does nothing.
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
        // whatever a drag under way began does nothing
        if let Some(d) = self.drag.as_mut() {
            d.zoomed = true;
        }
        session.view_gesture(p.x * ppp, p.y * ppp, 0.0, 0.0, by.max(0.01), 0.0);
    }

    /// A drag on the page: a cut shown as it goes and made as it ends, or with Move a gutter taken
    /// where it began and moved with it.
    fn drag(&mut self, ui: &egui::Ui, response: &egui::Response, session: &CanvasSession, ppp: f32) {
        if response.drag_started()
            && let Some(from) = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos())
        {
            let grabbed = self.moving && session.gutter_begin(from.x * ppp, from.y * ppp);
            self.drag = Some(Drag { from, to: from, grabbed, zoomed: false });
        }
        if response.dragged()
            && let (Some(d), Some(at)) = (self.drag.as_mut(), response.interact_pointer_pos())
        {
            d.to = at;
            if d.grabbed && !d.zoomed {
                session.gutter_drag(at.x * ppp, at.y * ppp);
            }
        }
        if response.drag_stopped()
            && let Some(d) = self.drag.take()
        {
            if d.grabbed {
                session.gutter_end(!d.zoomed);
            } else if !self.moving && !d.zoomed {
                session.cut_panels_on_screen(d.from.x * ppp, d.from.y * ppp, d.to.x * ppp, d.to.y * ppp);
            }
        }
    }

    /// Each panel outlined, dashed, and the cut being dragged as it would be made.
    fn draw(&self, ui: &egui::Ui, session: &CanvasSession, ppp: f32) {
        let painter = ui.painter();
        let m = session.view_matrix();
        let get = |i: usize| m.get(i).copied().unwrap_or(0.0);
        let (a, b, c, d, e, f) = (get(0), get(1), get(2), get(3), get(4), get(5));
        let on_screen = |x: f32, y: f32| pos2((a * x + c * y + e) / ppp, (b * x + d * y + f) / ppp);
        let outline = Stroke::new(1.5, argb(pc::ACCENT).gamma_multiply(0.8));
        for panel in session.comic_panels() {
            let mut corners: Vec<Pos2> =
                panel.points.as_chunks::<2>().0.iter().map(|[x, y]| on_screen(*x, *y)).collect();
            if let Some(&first) = corners.first() {
                corners.push(first);
            }
            painter.extend(Shape::dashed_line(&corners, outline, 6.0, 4.0));
        }
        let Some(drag) = self.drag.as_ref().filter(|d| !self.moving && !d.zoomed && d.from != d.to) else { return };
        let ends = session.panel_cut_on_screen(drag.from.x * ppp, drag.from.y * ppp, drag.to.x * ppp, drag.to.y * ppp);
        let [ax, ay, bx, by] = ends.as_slice() else { return };
        let line = [pos2(*ax, *ay) / ppp, pos2(*bx, *by) / ppp];
        for (width, colour) in [(5.0, Color32::WHITE), (3.0, argb(pc::ACCENT))] {
            painter.line_segment(line, Stroke::new(width, colour));
            // round ends
            for end in line {
                painter.circle_filled(end, width / 2.0, colour);
            }
        }
    }
}

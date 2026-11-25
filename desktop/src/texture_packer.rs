//! The Texture Packer (Share → Texture Packer), as the iPad app's (TexturePacker.swift): the
//! separate drawings on the canvas as boxes over it, grey until clicked, blue while chosen. The bar
//! under them: Cancel, how many are chosen, Export, and Separation (how close parts must be to make
//! one sprite) and Padding (clear pixels round each). The sprite clicked last shows on a card to be
//! renamed. Export writes each chosen sprite as a PNG of its own, in sRGB, into a folder picked.
//! The canvas can be moved and zoomed (a drag, the wheel, a pinch), but not drawn on. Finding and
//! cutting the sprites is the core's (texture_packer.rs).

use std::collections::{BTreeSet, HashMap};

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Shape, Stroke, UiBuilder, pos2, vec2};
use procraft_core::{CanvasSession, PackerBox, PackerSprite};

use crate::adjust_panels::option_slider;
use crate::look::{Themed, Weight, argb, pc, text};

const PANEL: Themed = Themed(0xF7262626, 0xF7E6E6E6);
const ROW: Themed = Themed(0xFF303030, 0xFFD8D8D8);
const TEXT_DIM: Themed = Themed(0x99FFFFFF, 0x99000000);
const DISABLED: Themed = Themed(0x4DFFFFFF, 0x4D000000);
const LABEL: Themed = Themed(0xFF808080, 0xFF595959);
/// A box not chosen: grey, see-through, its edge light.
const BOX_FILL: Color32 = Color32::from_rgba_unmultiplied_const(128, 128, 128, 89);
const BOX_EDGE: Color32 = Color32::from_rgba_unmultiplied_const(204, 204, 204, 230);
/// The bar: 668 wide (less in a narrow window), 130 tall, 24 over the window's foot; its sliders
/// 300 wide, 20 apart.
const BAR_WIDTH: f32 = 668.0;
const BAR_HEIGHT: f32 = 130.0;
const BAR_RISE: f32 = 24.0;
const BAR_SIDE: f32 = 24.0;
const SLIDER_GAP: f32 = 20.0;
/// The card over the bar: its picture 56 square, its name field 240 by 32, 14 round them.
const CARD_GAP: f32 = 12.0;
const CARD_PAD: f32 = 14.0;
const THUMB: f32 = 56.0;
const FIELD: egui::Vec2 = vec2(240.0, 32.0);
/// The card's picture is cut this many pixels across (twice what it shows, for a sharp one).
const THUMB_PIXELS: u32 = 112;
/// How much a wheel's line zooms the canvas, and a trackpad's scrolled point (as the canvas).
const WHEEL_ZOOM: f32 = 1.15;
const SCROLL_ZOOM: f32 = 0.005;
/// What it opens on: drawings 6 pixels apart are one sprite, none padded.
const SEPARATION: u32 = 6;
const PADDING: u32 = 0;
/// What is said when there is nothing to pack.
pub const NOTHING_TO_PACK: &str = "There are no separate drawings to pack.";

/// What the packer asks of the interface.
#[derive(Debug, PartialEq)]
pub enum Asked {
    /// Cancel (or Escape): it closed.
    Closed,
    /// Export: these sprites, to write where the person picks.
    Export(Vec<PackerSprite>),
}

pub struct TexturePacker {
    boxes: Vec<PackerBox>,
    separation: u32,
    padding: u32,
    /// The boxes chosen, by their place in `boxes`.
    chosen: BTreeSet<usize>,
    /// The box clicked last, shown on the card.
    focused: Option<usize>,
    thumbnail: Option<egui::TextureHandle>,
    /// Names given, by the box's place (so a name stays with a box the sliders leave as it was).
    names: HashMap<(u32, u32, u32, u32), String>,
    /// The name being typed on the card.
    typed: String,
}

fn key(b: &PackerBox) -> (u32, u32, u32, u32) {
    (b.x, b.y, b.width, b.height)
}

/// The view as [`CanvasSession::view_matrix`] gives it.
fn matrix(session: &CanvasSession) -> [f32; 6] {
    let v = session.view_matrix();
    v.get(..6).and_then(|m| <[f32; 6]>::try_from(m).ok()).unwrap_or([1.0, 0.0, 0.0, 1.0, 0.0, 0.0])
}

/// Canvas pixel (x, y) in the window (points).
fn to_window([a, b, c, d, e, f]: [f32; 6], ppp: f32, x: f32, y: f32) -> Pos2 {
    pos2(a * x + c * y + e, b * x + d * y + f) / ppp
}

/// Window point `p` on the canvas (pixels).
fn to_canvas([a, b, c, d, e, f]: [f32; 6], ppp: f32, p: Pos2) -> Option<(f32, f32)> {
    let det = a * d - b * c;
    if det.abs() < 1e-9 {
        return None;
    }
    let (x, y) = (p.x * ppp - e, p.y * ppp - f);
    Some(((d * x - c * y) / det, (a * y - b * x) / det))
}

/// Box `b`'s corners in the window (points), round from its top left.
fn corners(m: [f32; 6], ppp: f32, b: &PackerBox) -> Vec<Pos2> {
    let (x0, y0) = (b.x as f32, b.y as f32);
    let (x1, y1) = (x0 + b.width as f32, y0 + b.height as f32);
    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].into_iter().map(|(x, y)| to_window(m, ppp, x, y)).collect()
}

impl TexturePacker {
    /// Opens on the canvas as it shows now, without its background, its drawings found; none if
    /// there are none (say [`NOTHING_TO_PACK`]).
    pub fn open(session: &CanvasSession) -> Option<TexturePacker> {
        if !session.texture_packer_open().is_ok_and(|count| count > 0) {
            session.texture_packer_close();
            return None;
        }
        Some(TexturePacker {
            boxes: session.texture_packer_boxes(SEPARATION, PADDING),
            separation: SEPARATION,
            padding: PADDING,
            chosen: BTreeSet::new(),
            focused: None,
            thumbnail: None,
            names: HashMap::new(),
            typed: String::new(),
        })
    }

    /// The sliders moved: the boxes made again. Separation changes which boxes there are, so what
    /// was chosen goes; padding only grows them, so it stays.
    fn resized(&mut self, ctx: &egui::Context, session: &CanvasSession, keep_chosen: bool) {
        self.boxes = session.texture_packer_boxes(self.separation, self.padding);
        if !keep_chosen {
            self.chosen.clear();
            self.focused = None;
        }
        self.focus(ctx, session, self.focused);
    }

    /// The card shows sprite `index` (none: it goes).
    fn focus(&mut self, ctx: &egui::Context, session: &CanvasSession, index: Option<usize>) {
        self.focused = index;
        self.thumbnail = index.and_then(|i| self.boxes.get(i)).and_then(|b| {
            let png = session.texture_packer_thumbnail(*b, THUMB_PIXELS);
            let (w, h, rgba) = crate::files::png_rgba(&png)?;
            let picture = egui::ColorImage::from_rgba_unmultiplied([w as usize, h as usize], &rgba);
            Some(ctx.load_texture("texture packer sprite", picture, egui::TextureOptions::LINEAR))
        });
        if let Some(i) = index {
            self.typed = self.name(session, i);
        }
    }

    /// Sprite `index`'s name: the one given, or the one it shares under until renamed.
    fn name(&self, session: &CanvasSession, index: usize) -> String {
        let given = self.boxes.get(index).and_then(|b| self.names.get(&key(b)));
        given.cloned().unwrap_or_else(|| session.texture_packer_default_name(index as u32))
    }

    /// The name typed kept for sprite `index` (an empty one going back to what it was).
    fn rename(&mut self, session: &CanvasSession, index: usize) {
        let Some(b) = self.boxes.get(index) else { return };
        let name = self.typed.trim();
        if name.is_empty() {
            self.typed = self.name(session, index);
        } else {
            self.names.insert(key(b), name.to_owned());
        }
    }

    /// The chosen sprites, in reading order, each with its name.
    fn sprites(&self, session: &CanvasSession) -> Vec<PackerSprite> {
        self.chosen
            .iter()
            .filter_map(|&i| {
                let place = *self.boxes.get(i)?;
                Some(PackerSprite { place, index: i as u32, name: self.name(session, i) })
            })
            .collect()
    }

    /// A click at window point `at`: the smallest box under it chosen and shown on the card, or let
    /// go if it was chosen.
    fn clicked(&mut self, ctx: &egui::Context, session: &CanvasSession, at: Pos2) {
        let Some((x, y)) = to_canvas(matrix(session), ctx.pixels_per_point(), at) else { return };
        let Some(hit) = session.texture_packer_hit(self.boxes.clone(), x, y).map(|i| i as usize) else { return };
        if self.chosen.remove(&hit) {
            if self.focused == Some(hit) {
                self.focus(ctx, session, None);
            }
        } else {
            self.chosen.insert(hit);
            self.focus(ctx, session, Some(hit));
        }
    }

    fn status(&self) -> String {
        match self.chosen.len() {
            0 => "Click drawings to choose them".to_owned(),
            1 => "1 sprite chosen".to_owned(),
            n => format!("{n} sprites chosen"),
        }
    }

    /// Shows it over the canvas; what it asks, if anything. Closed, it has let the core's packer go.
    pub fn show(&mut self, ctx: &egui::Context, session: &CanvasSession) -> Option<Asked> {
        let screen = ctx.content_rect();
        let ppp = ctx.pixels_per_point();
        // Escape lets go of the name being typed first, then closes
        let typing = ctx.memory(|m| m.focused().is_some());
        let mut asked = None;
        if !typing && crate::ui::escape(ctx) {
            asked = Some(Asked::Closed);
        }
        egui::Area::new(Id::unique("texture packer")).fixed_pos(screen.min).order(Order::Middle).show(ctx, |ui| {
            // the whole window takes the clicks and the drags; nothing reaches the canvas
            let response = ui.allocate_rect(screen, Sense::click_and_drag());
            self.move_view(ui, &response, session, ppp);
            if response.clicked()
                && let Some(at) = response.interact_pointer_pos()
            {
                self.clicked(ctx, session, at);
            }
            // (where the view is now it has moved)
            let m = matrix(session);
            for (i, b) in self.boxes.iter().enumerate() {
                let (fill, edge) = if self.chosen.contains(&i) {
                    (argb(pc::ACCENT).gamma_multiply(0.3), Stroke::new(2.0, argb(pc::ACCENT)))
                } else {
                    (BOX_FILL, Stroke::new(1.5, BOX_EDGE))
                };
                ui.painter().add(Shape::convex_polygon(corners(m, ppp, b), fill, edge));
            }
        });
        let width = BAR_WIDTH.min(screen.width() - 32.0).max(200.0);
        let bar = Rect::from_min_size(
            pos2(screen.center().x - width / 2.0, screen.bottom() - BAR_RISE - BAR_HEIGHT),
            vec2(width, BAR_HEIGHT),
        );
        if let Some(a) = self.bar(ctx, session, bar) {
            asked = Some(a);
        }
        self.card(ctx, session, bar);
        if asked == Some(Asked::Closed) {
            session.texture_packer_close();
        }
        asked
    }

    /// The view moved: by a drag (one finger, or the mouse), a pinch, or the wheel (as on the
    /// canvas: a wheel's lines zoom, a trackpad scrolls, with Ctrl or ⌘ zooms).
    fn move_view(&self, ui: &egui::Ui, response: &egui::Response, session: &CanvasSession, ppp: f32) {
        let (wheel, touch, pointer) = ui.input(|i| {
            let mut pan = egui::Vec2::ZERO;
            let mut zoom = 1.0f32;
            for e in &i.events {
                match e {
                    egui::Event::MouseWheel { unit: egui::MouseWheelUnit::Line, delta, .. } => {
                        zoom *= WHEEL_ZOOM.powf(delta.y);
                    }
                    egui::Event::MouseWheel { delta, modifiers, .. } if modifiers.command || modifiers.ctrl => {
                        zoom *= (delta.y * ppp * SCROLL_ZOOM).exp();
                    }
                    egui::Event::MouseWheel { delta, .. } => pan += *delta,
                    egui::Event::Zoom(z) => zoom *= z,
                    _ => {}
                }
            }
            ((pan, zoom), i.multi_touch(), i.pointer.latest_pos())
        });
        let mut moved = false;
        if let Some(t) = touch {
            let c = t.center_pos;
            let d = t.translation_delta;
            session.view_gesture(c.x * ppp, c.y * ppp, d.x * ppp, d.y * ppp, t.zoom_delta, 0.0);
            moved = true;
        } else if response.dragged()
            && let Some(p) = response.interact_pointer_pos()
        {
            let d = response.drag_delta();
            session.view_gesture(p.x * ppp, p.y * ppp, d.x * ppp, d.y * ppp, 1.0, 0.0);
            moved = true;
        }
        let (pan, zoom) = wheel;
        if response.hovered()
            && let Some(p) = pointer
            && (pan != egui::Vec2::ZERO || zoom != 1.0)
        {
            session.view_gesture(p.x * ppp, p.y * ppp, pan.x * ppp, pan.y * ppp, zoom, 0.0);
            moved = true;
        }
        if moved {
            ui.ctx().request_repaint();
        }
    }

    /// The bar at `rect`: Cancel, how many are chosen and Export, then the two sliders.
    fn bar(&mut self, ctx: &egui::Context, session: &CanvasSession, rect: Rect) -> Option<Asked> {
        let mut asked = None;
        egui::Area::new(Id::unique("texture packer bar"))
            .fixed_pos(rect.min)
            .constrain(false)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                ui.allocate_rect(rect, Sense::click());
                let painter = ui.painter();
                painter.rect_filled(rect, 33.0, argb(PANEL));
                let row = Rect::from_min_size(
                    pos2(rect.left() + BAR_SIDE, rect.top() + 10.0),
                    vec2(rect.width() - BAR_SIDE * 2.0, 44.0),
                );
                let y = row.center().y;
                text(
                    painter,
                    pos2(row.left(), y),
                    Align2::LEFT_CENTER,
                    "Cancel",
                    17.0,
                    Weight::Regular,
                    argb(pc::TEXT),
                );
                text(
                    painter,
                    pos2(row.center().x, y),
                    Align2::CENTER_CENTER,
                    &self.status(),
                    15.0,
                    Weight::Regular,
                    argb(TEXT_DIM),
                );
                let any = !self.chosen.is_empty();
                let ink = if any { argb(pc::ACCENT) } else { argb(DISABLED) };
                text(painter, pos2(row.right(), y), Align2::RIGHT_CENTER, "Export", 17.0, Weight::SemiBold, ink);
                let cancel = Rect::from_min_size(row.min, vec2(110.0, 44.0));
                if ui.interact(cancel, ui.scope_id().with("cancel"), Sense::click()).clicked() {
                    asked = Some(Asked::Closed);
                }
                let export = Rect::from_min_size(pos2(row.right() - 110.0, row.top()), vec2(110.0, 44.0));
                if ui.interact(export, ui.scope_id().with("export"), Sense::click()).clicked() {
                    // the name being typed kept first
                    if let Some(i) = self.focused {
                        self.rename(session, i);
                    }
                    ui.memory_mut(|m| m.stop_text_input());
                    if any {
                        asked = Some(Asked::Export(self.sprites(session)));
                    }
                }
                let slider = (row.width() - SLIDER_GAP) / 2.0;
                let top = row.bottom() + 10.0;
                let separation = format!("{} px", self.separation);
                let value = (self.separation - 1) as f32 / 39.0;
                if let Some(v) =
                    option_slider(ui, pos2(row.left(), top), slider, "Separation", value, &separation, false, LABEL)
                {
                    let s = (v * 39.0).round() as u32 + 1;
                    if s != self.separation {
                        self.separation = s;
                        self.resized(ctx, session, false);
                    }
                }
                let padding = if self.padding == 0 { "None".to_owned() } else { format!("{} px", self.padding) };
                let at = pos2(row.left() + slider + SLIDER_GAP, top);
                if let Some(v) =
                    option_slider(ui, at, slider, "Padding", self.padding as f32 / 32.0, &padding, false, LABEL)
                {
                    let p = (v * 32.0).round() as u32;
                    if p != self.padding {
                        self.padding = p;
                        self.resized(ctx, session, true);
                    }
                }
            });
        asked
    }

    /// The sprite clicked last, over the bar at `bar`: its picture, its size, and its name to change.
    fn card(&mut self, ctx: &egui::Context, session: &CanvasSession, bar: Rect) {
        let Some((i, b)) = self.focused.and_then(|i| Some((i, *self.boxes.get(i)?))) else { return };
        let size = vec2(CARD_PAD * 2.0 + THUMB + 12.0 + FIELD.x, CARD_PAD * 2.0 + THUMB);
        let rect = Rect::from_min_size(pos2(bar.center().x - size.x / 2.0, bar.top() - CARD_GAP - size.y), size);
        egui::Area::new(Id::unique("texture packer card"))
            .fixed_pos(rect.min)
            .constrain(false)
            .order(Order::Foreground)
            .show(ctx, |ui| {
                ui.allocate_rect(rect, Sense::click());
                let painter = ui.painter();
                painter.rect_filled(rect, 22.0, argb(PANEL));
                let picture = Rect::from_min_size(rect.min + vec2(CARD_PAD, CARD_PAD), vec2(THUMB, THUMB));
                painter.rect_filled(picture, 8.0, argb(ROW));
                if let Some(t) = &self.thumbnail {
                    // fitted inside, as it is shaped
                    let [w, h] = t.size().map(|n| n as f32);
                    let k = (THUMB / w.max(1.0)).min(THUMB / h.max(1.0));
                    let shown = Rect::from_center_size(picture.center(), vec2(w * k, h * k));
                    let whole = Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0));
                    painter.image(t.id(), shown, whole, Color32::WHITE);
                }
                let left = picture.right() + 12.0;
                let words = format!("Sprite {} · {} × {} px", i + 1, b.width, b.height);
                let column_top = picture.center().y - (16.0 + 6.0 + FIELD.y) / 2.0;
                text(painter, pos2(left, column_top), Align2::LEFT_TOP, &words, 13.0, Weight::Regular, argb(TEXT_DIM));
                let field = Rect::from_min_size(pos2(left, column_top + 16.0 + 6.0), FIELD);
                painter.rect_filled(field, 8.0, argb(ROW));
                let output = egui::TextEdit::singleline(&mut self.typed)
                    .id(Id::unique("texture packer name"))
                    .font(Weight::Regular.font(16.0))
                    .text_color(argb(pc::TEXT))
                    .frame(egui::Frame::NONE)
                    .margin(vec2(10.0, 0.0))
                    .align(Align2::LEFT_CENTER)
                    .desired_width(field.width())
                    .char_limit(256)
                    .show(&mut ui.new_child(UiBuilder::new().max_rect(field)));
                if output.response.lost_focus() {
                    self.rename(session, i);
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Modifiers, PointerButton, RawInput};
    use procraft_core::{PenSample, PointerKind};

    use super::*;

    /// A canvas `side` pixels square, shown whole in a window as big (a pixel a point), on a
    /// headless GPU; none without one.
    fn session(side: u32) -> Option<std::sync::Arc<CanvasSession>> {
        let gpu = procraft_gpu::Gpu::headless().map_err(|e| eprintln!("skipped: {e}")).ok()?;
        let session = CanvasSession::new(side, side);
        session.set_display_scale(1.0);
        session.use_gpu(gpu);
        session.show_in(side, side);
        Some(session)
    }

    /// A short line from (`x`, `y`) on the window.
    fn dot(session: &CanvasSession, x: f32, y: f32) {
        let sample = |x, y| PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: 0.0, roll: 0.0 };
        session.stroke_begin(PointerKind::Finger, vec![sample(x, y), sample(x + 4.0, y)]);
        session.stroke_end(vec![]);
    }

    /// The packer shown in a window `side` points square, frame after frame, each with `events`.
    struct Shown {
        ctx: egui::Context,
        side: f32,
        time: f64,
    }

    impl Shown {
        fn frame(&mut self, packer: &mut TexturePacker, session: &CanvasSession, events: Vec<Event>) -> Option<Asked> {
            self.time += 1.0 / 60.0;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(self.side, self.side))),
                time: Some(self.time),
                events,
                ..RawInput::default()
            };
            let mut asked = None;
            let mut output = self.ctx.clone().run_ui(raw, |ui| asked = packer.show(ui.ctx(), session));
            output.textures_delta.clear();
            asked
        }

        /// A click at `at`: the pointer moved there a frame before it presses.
        fn click(&mut self, packer: &mut TexturePacker, session: &CanvasSession, at: Pos2) -> Option<Asked> {
            let press = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(packer, session, vec![Event::PointerMoved(at)]);
            self.frame(packer, session, vec![press(true)]);
            let asked = self.frame(packer, session, vec![press(false)]);
            self.frame(packer, session, Vec::new()).or(asked)
        }
    }

    /// Box `b`'s middle in the window.
    fn middle(session: &CanvasSession, b: &PackerBox) -> Pos2 {
        let c = corners(matrix(session), 1.0, b);
        Rect::from_points(&c).center()
    }

    /// Two drawings apart are two boxes; clicking one chooses it and shows it on the card, named
    /// as it shares; its name changed, Export asks for it under that name, and the core writes it
    /// so. Separation pulled up joins them (what was chosen goes); Cancel closes the core's packer.
    #[test]
    fn drawings_are_chosen_named_and_asked_to_be_exported() {
        let Some(session) = session(600) else { return };
        session.set_brush_size(0.2);
        session.set_color(1.0, 0.0, 0.0);
        dot(&session, 100.0, 100.0);
        dot(&session, 125.0, 110.0);
        let Some(mut packer) = TexturePacker::open(&session) else { panic!("two drawings to pack") };
        assert_eq!(packer.boxes.len(), 2, "{:?}", packer.boxes);
        let ctx = egui::Context::default();
        crate::look::install_fonts(&ctx);
        let mut shown = Shown { ctx, side: 600.0, time: 0.0 };
        shown.frame(&mut packer, &session, Vec::new());
        let second = packer.boxes.get(1).copied().expect("a second box");
        // clicked: chosen, on the card under the name it shares with
        assert_eq!(shown.click(&mut packer, &session, middle(&session, &second)), None);
        assert_eq!(packer.chosen.iter().copied().collect::<Vec<_>>(), [1]);
        assert_eq!(packer.focused, Some(1));
        assert!(packer.thumbnail.is_some(), "its picture on the card");
        assert_eq!(packer.typed, session.texture_packer_default_name(1));
        let card = shown.ctx.memory(|m| m.area_rect(Id::unique("texture packer card")));
        assert!(card.is_some(), "the card shows");
        // renamed
        packer.typed = "  flag ".to_owned();
        packer.rename(&session, 1);
        assert_eq!(packer.name(&session, 1), "flag");
        // a click off every box changes nothing; on the box again it is let go
        shown.click(&mut packer, &session, pos2(500.0, 40.0));
        assert_eq!(packer.chosen.len(), 1);
        shown.click(&mut packer, &session, middle(&session, &second));
        assert!(packer.chosen.is_empty());
        assert_eq!(packer.focused, None);
        // both chosen; Export asks for them in reading order, the renamed one by its name
        let first = packer.boxes.first().copied().expect("a first box");
        shown.click(&mut packer, &session, middle(&session, &first));
        shown.click(&mut packer, &session, middle(&session, &second));
        let bar = shown.ctx.memory(|m| m.area_rect(Id::unique("texture packer bar"))).expect("the bar");
        // 16 in from the sides of a window narrower than it, 24 over its foot
        assert_eq!(bar, Rect::from_min_size(pos2(16.0, 600.0 - BAR_RISE - BAR_HEIGHT), vec2(568.0, BAR_HEIGHT)));
        let export = pos2(bar.right() - BAR_SIDE - 20.0, bar.top() + 10.0 + 22.0);
        let Some(Asked::Export(sprites)) = shown.click(&mut packer, &session, export) else { panic!("Export asked") };
        let names: Vec<&str> = sprites.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, [session.texture_packer_default_name(0).as_str(), "flag"]);
        let dir = std::env::temp_dir().join(format!("desktop-packer-{}", std::process::id()));
        let paths = session.texture_packer_export(dir.to_string_lossy().into_owned(), sprites).unwrap_or_default();
        assert!(paths.iter().any(|p| p.ends_with("flag.png")), "{paths:?}");
        let _ = std::fs::remove_dir_all(&dir);
        // Separation as far as it goes, dragged along its slider: one sprite, nothing chosen
        let slider = pos2(bar.left() + BAR_SIDE + 10.0, bar.top() + 10.0 + 44.0 + 10.0 + 42.0);
        let press = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        shown.frame(&mut packer, &session, vec![Event::PointerMoved(slider)]);
        shown.frame(&mut packer, &session, vec![press(slider, true)]);
        for k in 1..=10 {
            shown.frame(&mut packer, &session, vec![Event::PointerMoved(slider + vec2(k as f32 * 40.0, 0.0))]);
        }
        shown.frame(&mut packer, &session, vec![press(slider + vec2(400.0, 0.0), false)]);
        assert_eq!(packer.separation, 40);
        assert_eq!(packer.boxes.len(), 1);
        assert!(packer.chosen.is_empty());
        // Cancel
        let cancel = pos2(bar.left() + BAR_SIDE + 20.0, bar.top() + 10.0 + 22.0);
        assert_eq!(shown.click(&mut packer, &session, cancel), Some(Asked::Closed));
        assert!(session.texture_packer_boxes(6, 0).is_empty(), "the core's packer let go");
    }

    /// A drag on the canvas moves the view and chooses nothing.
    #[test]
    fn a_drag_moves_the_canvas() {
        let Some(session) = session(400) else { return };
        session.set_brush_size(0.2);
        dot(&session, 100.0, 100.0);
        let Some(mut packer) = TexturePacker::open(&session) else { panic!("a drawing to pack") };
        let ctx = egui::Context::default();
        crate::look::install_fonts(&ctx);
        let mut shown = Shown { ctx, side: 400.0, time: 0.0 };
        // (an area takes presses from its second frame on)
        shown.frame(&mut packer, &session, Vec::new());
        let before = matrix(&session);
        let from = middle(&session, packer.boxes.first().expect("a box"));
        let press = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        shown.frame(&mut packer, &session, vec![Event::PointerMoved(from)]);
        shown.frame(&mut packer, &session, vec![press(from, true)]);
        for k in 1..=10 {
            shown.frame(&mut packer, &session, vec![Event::PointerMoved(from + vec2(k as f32 * 6.0, 0.0))]);
        }
        shown.frame(&mut packer, &session, vec![press(from + vec2(60.0, 0.0), false)]);
        let after = matrix(&session);
        assert!(after[4] > before[4] + 30.0, "moved right: {before:?} {after:?}");
        assert!(packer.chosen.is_empty());
    }
}

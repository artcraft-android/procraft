//! The panels hanging from the top bar, as the Android app's (ui/Popover.kt, ui/AppRoot.kt): a
//! frosted body with an arrow pointing up at its button, a soft shadow around it, and a header.

use egui::emath::TSTransform;
use egui::{Align2, Color32, Pos2, Rect, Sense, Ui, Vec2, pos2};
use procraft_core::InterfacePart;
use resvg::tiny_skia;

use crate::icons;
use crate::look::{Look, Shade, Themed, Weight, argb, fill, pc, round_rect, text};

/// A panel's own material over the canvas: what is behind it shows through, blurred.
pub const POPOVER_FROST: Themed = Themed(0xDE1E1E1E, 0xFFE6E6E6);
pub const POPOVER_SHADOW: Themed = Themed(0x38000000, 0x24000000);
pub const POPOVER_SHADOW_RADIUS: f32 = 51.0;
pub const POPOVER_CORNER: f32 = 35.0;
pub const POPOVER_GAP: f32 = 12.0;
pub const ARROW_HEIGHT: f32 = 12.0;
pub const ARROW_SHIFT: f32 = 2.0;
pub const PANEL_TITLE: Themed = Themed(0xFFC7C7C7, 0xFF393939);
pub const PANEL_GLYPH: Themed = Themed(0xFFC8C8C8, 0xFF393939);
pub const CHEVRON_DISC: Themed = Themed(0xFF4C4C4C, 0xFFCFCFCF);
pub const CHEVRON_GLYPH: Themed = Themed(0xFF2E2E2E, 0xFFE6E6E6);

/// The strip a floating panel's window adds over the panel (its grabber and Close), and under it
/// (its corner, which sizes it).
pub const FLOATING_TOP: f32 = 24.0;
pub const FLOATING_BOTTOM: f32 = 20.0;
const GRABBER: Themed = Themed(0x7AC6C6C6, 0xFFB0B0B0);
/// Close: a disc of white over the window with its cross cut out of it (drawn darker).
const CLOSE_DISC: u32 = 0x26FFFFFF;
const CLOSE_CROSS: u32 = 0x4D000000;

/// Where a panel shows: hanging from its button (where the button shows, in the panels' space),
/// or floating in a window of its own at this rect (in its own space, see
/// [`crate::floating_panels`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Hang {
    Button(Rect),
    Floating(Rect),
}

/// What a floating panel's strips had done to them this frame: the grabber strip pressed (its
/// window moves with the pointer from there), the corner pressed (it is sized), Close tapped.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FloatingChrome {
    pub moving: bool,
    pub sizing: bool,
    pub closed: bool,
}

/// The rect a panel floating in `window` is laid out in as a panel hanging from a button is, its
/// arrow's room above it: its body under the window's top strip, down to its bottom strip.
pub fn floating_rect(window: Rect) -> Rect {
    Rect::from_min_max(
        pos2(window.left(), window.top() + FLOATING_TOP - ARROW_HEIGHT),
        pos2(window.right(), window.bottom() - FLOATING_BOTTOM),
    )
}

/// The area a panel floating in `window` is drawn in: over the canvas, under the panels hanging
/// from the bar, kept to the window and clipped to it as the panel measures it.
pub fn floating_area(ctx: &egui::Context, id: egui::Id, window: Rect) -> egui::Area {
    egui::Area::new(id).fixed_pos(window.min).order(egui::Order::Middle).constrain_to(screen(ctx))
}

/// Draws a floating panel's window at `window` (no arrow): the shadow round it, then its frosted
/// body, whose blur goes into `backdrops`; its layer shown as its space does.
pub fn floating_panel_frame(ui: &Ui, look: &mut Look, slot: &'static str, window: Rect, backdrops: &mut Vec<f32>) {
    let to_window = to_window(ui.ctx());
    ui.ctx().set_transform_layer(ui.layer_id(), to_window);
    floating_frame(ui, look, slot, window, to_window.scaling, backdrops);
}

/// A floating panel's strips at `window`: the grabber in the middle of the top one (the whole strip
/// moves the window) and Close at its right, and two short lines across the lower right corner,
/// which sizes it. What was done to them.
pub fn floating_chrome(ui: &Ui, window: Rect) -> FloatingChrome {
    let grabber = Rect::from_center_size(pos2(window.center().x, window.top() + 12.0), egui::vec2(40.0, 4.0));
    ui.painter().rect_filled(grabber, 2, argb(GRABBER));
    let strip = Rect::from_min_size(window.min, egui::vec2(window.width(), FLOATING_TOP));
    let moving = ui.interact(strip, ui.scope_id().with("floating strip"), Sense::drag()).drag_started();
    let close = Rect::from_center_size(pos2(window.right() - 14.0 - 20.0, window.top() + 22.0), egui::vec2(40.0, 40.0));
    let c = close.center();
    ui.painter().circle_filled(c, 8.5, argb(CLOSE_DISC));
    let stroke = egui::Stroke::new(1.3, argb(CLOSE_CROSS));
    for (a, b) in [(egui::vec2(-3.2, -3.2), egui::vec2(3.2, 3.2)), (egui::vec2(3.2, -3.2), egui::vec2(-3.2, 3.2))] {
        ui.painter().line_segment([c + a, c + b], stroke);
    }
    let closed = ui.interact(close, ui.scope_id().with("floating close"), Sense::click()).clicked();
    let corner = Rect::from_min_size(
        pos2(window.right() - 6.0 - 36.0, window.bottom() - FLOATING_BOTTOM),
        egui::vec2(36.0, FLOATING_BOTTOM),
    );
    let end = pos2(corner.right() - 12.0, corner.bottom() - 6.0);
    let line = egui::Stroke::new(2.0, argb(GRABBER));
    for reach in [10.0, 5.0] {
        ui.painter().line_segment([end - egui::vec2(reach, 0.0), end - egui::vec2(0.0, reach)], line);
    }
    let sizing = ui.interact(corner, ui.scope_id().with("floating corner"), Sense::drag()).drag_started();
    FloatingChrome { moving, sizing, closed }
}

/// A panel hanging from its button, while a plugin lets it float: the grabber at the top of its
/// `body`, and while it is dragged, where the press began and where the pointer is (the window's
/// points).
pub fn undock_grabber(ui: &Ui, body: Rect) -> Option<(Pos2, Pos2)> {
    let grabber = Rect::from_center_size(pos2(body.center().x, body.top() + 6.0), egui::vec2(40.0, 4.0));
    ui.painter().rect_filled(grabber, 2, argb(GRABBER));
    // (narrower than the colour panel's: clear of a title beside it)
    let handle = Rect::from_center_size(pos2(body.center().x, body.top() + 15.0), egui::vec2(100.0, 30.0));
    let held = ui.interact(handle, ui.scope_id().with("undock"), Sense::drag());
    let (pointer, from) = (held.interact_pointer_pos()?, ui.input(|i| i.pointer.press_origin())?);
    held.dragged().then(|| (from, to_window(ui.ctx()) * pointer))
}

/// Where the side bar is, for the panels to keep clear of: whether it shows, and on the right.
#[derive(Clone, Copy, Default)]
struct SideBarPlace {
    shown: bool,
    right: bool,
    centre: f32,
}

/// How big each part of the interface shows next to the rest of it this frame, as the plugins say
/// (in [`procraft_core::InterfacePart`]'s order), for what draws a part to read.
pub fn set_part_scales(ctx: &egui::Context, plugins: &procraft_core::Plugins) {
    use procraft_core::InterfacePart as P;
    put_part_scales(
        ctx,
        [P::Floating, P::Menus, P::SideBar, P::TopBar, P::Panels].map(|p| plugins.interface_part_scale(p)),
    );
}

/// How big each part of the interface shows next to the rest of it this frame, in
/// [`InterfacePart`]'s order.
pub fn put_part_scales(ctx: &egui::Context, scales: [f32; 5]) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("interface part scales"), scales));
}

/// How big `part` of the interface shows next to the rest of it this frame (1: as the rest).
pub fn part_scale(ctx: &egui::Context, part: InterfacePart) -> f32 {
    let scales = ctx.data(|d| d.get_temp::<[f32; 5]>(egui::Id::unique("interface part scales")));
    scales.and_then(|s| s.get(part as usize).copied()).unwrap_or(1.0)
}

/// How far down from the window's top the top bar reaches as it shows: what hangs from it hangs
/// from there.
pub fn bar_shown(ctx: &egui::Context) -> f32 {
    pc::BAR_HEIGHT * part_scale(ctx, InterfacePart::TopBar)
}

/// The panels' own space: each is laid out at its own size, under a top bar of its own height, in
/// the window as it measures it at that size, and shows at the panels' size from where the top
/// bar, as it shows, ends at the window's left edge.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PanelSpace {
    /// From a panel's points to the window's.
    pub to_window: TSTransform,
    /// The window as a panel measures it: a top bar's height above where the panels hang, the
    /// room under it, and as wide as the window.
    pub screen: Rect,
}

impl PanelSpace {
    /// In `window`, under a top bar reaching `bar_bottom` down, shown `scale` times as big.
    pub fn new(window: Rect, bar_bottom: f32, scale: f32) -> PanelSpace {
        let hang = pos2(window.left(), window.top() + bar_bottom);
        let to_window = crate::chrome::scaled_from(hang, scale);
        let measured = Rect::from_min_max(pos2(window.left(), hang.y - pc::BAR_HEIGHT * scale), window.max);
        PanelSpace { to_window, screen: to_window.inverse().mul_rect(measured) }
    }
}

/// A panel is drawn from now on, in `space`; or, with none, no panel is.
pub fn drawing_panel(ctx: &egui::Context, space: Option<PanelSpace>) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("panel space"), space));
}

/// The panels' space while one is drawn.
fn panel_space(ctx: &egui::Context) -> Option<PanelSpace> {
    ctx.data(|d| d.get_temp::<Option<PanelSpace>>(egui::Id::unique("panel space"))).flatten()
}

/// From the points of what is drawn now to the window's: a panel's while one is drawn.
pub fn to_window(ctx: &egui::Context) -> TSTransform {
    panel_space(ctx).map_or(TSTransform::IDENTITY, |s| s.to_window)
}

/// The window as what is drawn now measures it: as a panel does while one is drawn.
pub fn screen(ctx: &egui::Context) -> Rect {
    panel_space(ctx).map_or_else(|| ctx.content_rect(), |s| s.screen)
}

/// Window point `p` (the pointer's, say) where what is drawn now has it.
pub fn in_panel(ctx: &egui::Context, p: Pos2) -> Pos2 {
    to_window(ctx).inverse() * p
}

/// A move of `by` across the window (the pointer's) as what is drawn now has it.
pub fn in_panel_by(ctx: &egui::Context, by: Vec2) -> Vec2 {
    by / to_window(ctx).scaling
}

/// The area a panel at `rect` (arrow included) is drawn in, over the canvas: kept to the window
/// and clipped to it as the panel measures it.
pub fn area(ctx: &egui::Context, id: egui::Id, rect: Rect) -> egui::Area {
    egui::Area::new(id).fixed_pos(rect.min).order(egui::Order::Foreground).constrain_to(screen(ctx))
}

/// The side bar shows (or not) on the right (or the left), its centre kept at `centre` (a share of
/// the window's height), for the panels placed this frame.
pub fn side_bar_at(ctx: &egui::Context, shown: bool, right: bool, centre: f32) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::unique("side bar place"), SideBarPlace { shown, right, centre }));
}

/// Where the side bar shows this frame, if it shows.
pub fn side_bar_shown_at(ctx: &egui::Context) -> Option<Rect> {
    let side = ctx.data(|d| d.get_temp::<SideBarPlace>(egui::Id::unique("side bar place")))?;
    let scale = part_scale(ctx, InterfacePart::SideBar);
    side.shown.then(|| {
        crate::chrome::fit_side_bar(ctx.content_rect(), side.right, scale, bar_shown(ctx), side.centre).shown()
    })
}

/// Where a panel `width` wide goes, hanging from a button centred at `button_x`: its left edge and
/// its arrow's tip from that edge, in the panels' space while one is drawn. It shows centred on
/// the button (the tip `ARROW_SHIFT` right of the button's centre) and inside the window, clear of
/// the side bar as it shows, on whichever side it is.
pub fn place(ctx: &egui::Context, width: f32, button_x: f32) -> (f32, f32) {
    let to_window = to_window(ctx);
    let (k, x) = (to_window.scaling, to_window.translation.x);
    let screen = ctx.content_rect();
    let side = ctx.data(|d| d.get_temp::<SideBarPlace>(egui::Id::unique("side bar place"))).unwrap_or(SideBarPlace {
        shown: true,
        right: false,
        centre: procraft_core::side_bar_first_centre(),
    });
    let side_bar = pc::SIDE_BAR_WIDTH * part_scale(ctx, InterfacePart::SideBar);
    let clear = if side.shown { side_bar + POPOVER_GAP } else { POPOVER_GAP };
    let (min_left, min_right) = if side.right { (POPOVER_GAP, clear) } else { (clear, POPOVER_GAP) };
    // where it shows
    let (tip, shown) = (button_x * k + x + ARROW_SHIFT, width * k);
    let lo = screen.left() + min_left;
    let hi = (screen.right() - min_right - shown).max(lo);
    let left = (tip - shown / 2.0).clamp(lo, hi);
    ((left - x) / k, (tip - left) / k)
}

/// A panel's body (below its arrow) for a panel at `rect` (arrow included).
pub fn body(rect: Rect) -> Rect {
    Rect::from_min_max(pos2(rect.left(), rect.top() + ARROW_HEIGHT), rect.max)
}

/// Draws a panel at `rect` (its arrow on top, the tip `arrow_x` from its left edge): the shadow
/// round it, then its frosted body, whose blur goes into `backdrops` (window pixels). Its layer
/// shows it as the panels' space does, its icons and shapes drawn for that.
pub fn frame(ui: &Ui, look: &mut Look, slot: &'static str, rect: Rect, arrow_x: f32, backdrops: &mut Vec<f32>) {
    let to_window = to_window(ui.ctx());
    ui.ctx().set_transform_layer(ui.layer_id(), to_window);
    look.set_zoom(ui.layer_id(), to_window.scaling);
    let body = body(rect);
    let ppp = ui.ctx().pixels_per_point();
    let shown = to_window.mul_rect(body);
    let edges = [shown.left(), shown.top(), shown.right(), shown.bottom(), POPOVER_CORNER * to_window.scaling];
    backdrops.extend(edges.map(|v| v * ppp));
    backdrops.push(1.0);
    // only round the panel: none of it shows through
    let shadow = egui::epaint::Shadow {
        offset: [0, 0],
        blur: POPOVER_SHADOW_RADIUS as u8,
        spread: 0,
        color: argb(POPOVER_SHADOW),
    };
    ui.painter().add(shadow.as_shape(body, POPOVER_CORNER));
    let size = rect.size();
    look.shape(ui, slot, (arrow_x * 100.0).round() as i32, rect, |pixmap, t| {
        let mut pb = tiny_skia::PathBuilder::new();
        let (half, tip) = (ARROW_HEIGHT, 2.0);
        // a corner the arrow comes near is rounded less, so that the arrow rises from a straight
        // edge rather than hanging over the curve (at a small interface size, or a button near
        // the window's edge)
        let near = |from: f32| (from - half - tip).clamp(0.0, POPOVER_CORNER);
        let corners = [near(arrow_x), near(size.x - arrow_x), POPOVER_CORNER, POPOVER_CORNER];
        round_rect(&mut pb, Rect::from_min_max(pos2(0.0, ARROW_HEIGHT), size.to_pos2()), corners);
        pb.move_to(arrow_x - half - tip, ARROW_HEIGHT + 0.5);
        pb.line_to(arrow_x - tip, tip);
        pb.quad_to(arrow_x, 0.0, arrow_x + tip, tip);
        pb.line_to(arrow_x + half + tip, ARROW_HEIGHT + 0.5);
        pb.close();
        frost(pixmap, pb, t);
    });
}

/// Draws a floating panel at `rect` (no arrow): the shadow round it, then its frosted body, whose
/// blur goes into `backdrops` (window pixels). Its layer shows it `scale` times as big, from its
/// top-left, its icons and shapes drawn for that.
pub fn floating_frame(ui: &Ui, look: &mut Look, slot: &'static str, rect: Rect, scale: f32, backdrops: &mut Vec<f32>) {
    look.set_zoom(ui.layer_id(), scale);
    let ppp = ui.ctx().pixels_per_point();
    let shown = Rect::from_min_size(rect.min, rect.size() * scale);
    let edges = [shown.left(), shown.top(), shown.right(), shown.bottom(), POPOVER_CORNER * scale];
    backdrops.extend(edges.map(|v| v * ppp));
    backdrops.push(1.0);
    let shadow = egui::epaint::Shadow {
        offset: [0, 0],
        blur: POPOVER_SHADOW_RADIUS as u8,
        spread: 0,
        color: argb(POPOVER_SHADOW),
    };
    ui.painter().add(shadow.as_shape(rect, POPOVER_CORNER));
    let size = rect.size();
    look.shape(ui, slot, (size.y * 10.0).round() as i32, rect, |pixmap, t| {
        let mut pb = tiny_skia::PathBuilder::new();
        round_rect(&mut pb, Rect::from_min_size(pos2(0.0, 0.0), size), [POPOVER_CORNER; 4]);
        frost(pixmap, pb, t);
    });
}

/// Draws a panel at `rect` (arrow included) with its arrow on its `left` side, else its right, the
/// tip `arrow_y` from its top, and `corner` rounding: the shadow round it, then its frosted body,
/// whose blur goes into `backdrops` (window pixels). Returns the body.
#[allow(clippy::too_many_arguments)]
pub fn side_frame(
    ui: &Ui,
    look: &mut Look,
    slot: &'static str,
    rect: Rect,
    arrow_y: f32,
    left: bool,
    corner: f32,
    backdrops: &mut Vec<f32>,
) -> Rect {
    let body = side_body(rect, left);
    let ppp = ui.ctx().pixels_per_point();
    backdrops.extend([body.left(), body.top(), body.right(), body.bottom(), corner].map(|v| v * ppp));
    backdrops.push(1.0);
    let shadow = egui::epaint::Shadow {
        offset: [0, 0],
        blur: POPOVER_SHADOW_RADIUS as u8,
        spread: 0,
        color: argb(POPOVER_SHADOW),
    };
    ui.painter().add(shadow.as_shape(body, corner));
    let size = rect.size();
    let key = ((arrow_y * 100.0).round() as i32, left, (corner * 10.0) as i32);
    look.shape(ui, slot, key, rect, |pixmap, t| frost(pixmap, side_outline(size, arrow_y, left, corner), t));
    body
}

/// As [`side_frame`], filled `colour` (nothing shows through). Returns the body.
#[allow(clippy::too_many_arguments)]
pub fn solid_side_frame(
    ui: &Ui,
    look: &mut Look,
    slot: &'static str,
    rect: Rect,
    arrow_y: f32,
    left: bool,
    corner: f32,
    colour: impl Shade,
) -> Rect {
    let colour = colour.value();
    let body = side_body(rect, left);
    let shadow = egui::epaint::Shadow {
        offset: [0, 0],
        blur: POPOVER_SHADOW_RADIUS as u8,
        spread: 0,
        color: argb(POPOVER_SHADOW),
    };
    ui.painter().add(shadow.as_shape(body, corner));
    let size = rect.size();
    let key = ((arrow_y * 100.0).round() as i32, left, (corner * 10.0) as i32, colour);
    look.shape(ui, slot, key, rect, |pixmap, t| {
        fill(pixmap, side_outline(size, arrow_y, left, corner).finish(), argb(colour), t);
    });
    body
}

/// A side-arrowed panel's body: `rect` less its arrow, on its `left` side or its right.
fn side_body(rect: Rect, left: bool) -> Rect {
    if left {
        Rect::from_min_max(pos2(rect.left() + ARROW_HEIGHT, rect.top()), rect.max)
    } else {
        Rect::from_min_max(rect.min, pos2(rect.right() - ARROW_HEIGHT, rect.bottom()))
    }
}

/// The outline of a panel `size` big (arrow included) with its arrow on its `left` side, else its
/// right, the tip `arrow_y` from its top, rounded `corner`: one shape, filled as one.
fn side_outline(size: egui::Vec2, arrow_y: f32, left: bool, corner: f32) -> tiny_skia::PathBuilder {
    let mut pb = tiny_skia::PathBuilder::new();
    let (half, tip, w) = (ARROW_HEIGHT, 2.0, size.x);
    if left {
        round_rect(&mut pb, Rect::from_min_max(pos2(ARROW_HEIGHT, 0.0), size.to_pos2()), [corner; 4]);
        pb.move_to(ARROW_HEIGHT + 0.5, arrow_y - half - tip);
        pb.line_to(tip, arrow_y - tip);
        pb.quad_to(0.0, arrow_y, tip, arrow_y + tip);
        pb.line_to(ARROW_HEIGHT + 0.5, arrow_y + half + tip);
    } else {
        round_rect(&mut pb, Rect::from_min_max(Pos2::ZERO, pos2(w - ARROW_HEIGHT, size.y)), [corner; 4]);
        pb.move_to(w - ARROW_HEIGHT - 0.5, arrow_y - half - tip);
        pb.line_to(w - tip, arrow_y - tip);
        pb.quad_to(w, arrow_y, w - tip, arrow_y + tip);
        pb.line_to(w - ARROW_HEIGHT - 0.5, arrow_y + half + tip);
    }
    pb.close();
    pb
}

/// Draws a panel at `rect` (arrow included) with its arrow `below` it (else above), the tip
/// `arrow_x` from its left edge, rounded `corner` and filled `colour` (nothing shows through): the
/// shadow round it, then the panel. Returns its body.
#[allow(clippy::too_many_arguments)]
pub fn solid_frame(
    ui: &Ui,
    look: &mut Look,
    slot: &'static str,
    rect: Rect,
    arrow_x: f32,
    below: bool,
    corner: f32,
    colour: impl Shade,
) -> Rect {
    let colour = colour.value();
    let body = if below {
        Rect::from_min_max(rect.min, pos2(rect.right(), rect.bottom() - ARROW_HEIGHT))
    } else {
        Rect::from_min_max(pos2(rect.left(), rect.top() + ARROW_HEIGHT), rect.max)
    };
    let shadow = egui::epaint::Shadow {
        offset: [0, 0],
        blur: POPOVER_SHADOW_RADIUS as u8,
        spread: 0,
        color: argb(POPOVER_SHADOW),
    };
    ui.painter().add(shadow.as_shape(body, corner));
    let size = rect.size();
    let key = ((arrow_x * 100.0).round() as i32, colour, below, (corner * 10.0) as i32);
    look.shape(ui, slot, key, rect, |pixmap, t| {
        let mut pb = tiny_skia::PathBuilder::new();
        let (half, tip, h) = (ARROW_HEIGHT, 2.0, size.y);
        if below {
            round_rect(&mut pb, Rect::from_min_max(Pos2::ZERO, pos2(size.x, h - ARROW_HEIGHT)), [corner; 4]);
            pb.move_to(arrow_x - half - tip, h - ARROW_HEIGHT - 0.5);
            pb.line_to(arrow_x - tip, h - tip);
            pb.quad_to(arrow_x, h, arrow_x + tip, h - tip);
            pb.line_to(arrow_x + half + tip, h - ARROW_HEIGHT - 0.5);
        } else {
            round_rect(&mut pb, Rect::from_min_max(pos2(0.0, ARROW_HEIGHT), size.to_pos2()), [corner; 4]);
            pb.move_to(arrow_x - half - tip, ARROW_HEIGHT + 0.5);
            pb.line_to(arrow_x - tip, tip);
            pb.quad_to(arrow_x, 0.0, arrow_x + tip, tip);
            pb.line_to(arrow_x + half + tip, ARROW_HEIGHT + 0.5);
        }
        pb.close();
        let mut paint = tiny_skia::Paint::default();
        let [r, g, b, a] = argb(colour).to_srgba_unmultiplied();
        paint.set_color_rgba8(r, g, b, a);
        paint.anti_alias = true;
        if let Some(path) = pb.finish() {
            pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, t, None);
        }
    });
    body
}

/// A panel's outline filled with its frosted material: the arrow and the body are one shape,
/// filled as one, without an even-odd seam.
fn frost(pixmap: &mut tiny_skia::Pixmap, pb: tiny_skia::PathBuilder, t: tiny_skia::Transform) {
    let mut paint = tiny_skia::Paint::default();
    let [r, g, b, a] = argb(POPOVER_FROST).to_srgba_unmultiplied();
    paint.set_color_rgba8(r, g, b, a);
    paint.anti_alias = true;
    if let Some(path) = pb.finish() {
        pixmap.fill_path(&path, &paint, tiny_skia::FillRule::Winding, t, None);
    }
}

/// Fills a rounded rectangle (corners `[top left, top right, bottom right, bottom left]`) kept as
/// `slot`: for the parts of a panel that follow its rounded corners.
pub fn rounded(ui: &Ui, look: &mut Look, slot: &'static str, rect: Rect, corners: [f32; 4], colour: Color32) {
    let size = rect.size();
    let key = (corners.map(|c| (c * 10.0) as i32), colour.to_array());
    look.shape(ui, slot, key, rect, |pixmap, t| {
        let mut pb = tiny_skia::PathBuilder::new();
        round_rect(&mut pb, Rect::from_min_size(Pos2::ZERO, size), corners);
        fill(pixmap, pb.finish(), colour, t);
    });
}

/// A panel's header: its title (24 bold) `start` in, with the chevron disc after it. Returns
/// whether the title was tapped.
pub fn header(ui: &Ui, look: &mut Look, top: Rect, title: &str, start: f32, chevron: bool, chevron_on: bool) -> bool {
    header_title(ui, look, top, title, start, chevron, chevron_on).0
}

/// As [`header`]; also where the title and its chevron are, for a menu to hang from.
pub fn header_title(
    ui: &Ui,
    look: &mut Look,
    top: Rect,
    title: &str,
    start: f32,
    chevron: bool,
    chevron_on: bool,
) -> (bool, Rect) {
    let painter = ui.painter();
    let at = pos2(top.left() + start, top.top() + 19.0);
    let words = text(painter, at, Align2::LEFT_TOP, title, 24.0, Weight::Bold, argb(PANEL_TITLE));
    if chevron {
        let centre = pos2(words.right() + 10.0 + 8.5, words.center().y - 1.5);
        painter.circle_filled(centre, 8.5, if chevron_on { argb(pc::ACCENT) } else { argb(CHEVRON_DISC) });
        look.icon(ui, &icons::CHEVRON_DOWN, centre, 13.0, argb(CHEVRON_GLYPH));
    }
    let row = if chevron { Rect::from_min_max(words.min, pos2(words.right() + 27.0, words.bottom())) } else { words };
    (ui.interact(row, ui.scope_id().with(("title", title)), Sense::click()).clicked(), row)
}

/// A light-on-dark tint for a selected row, or none.
pub fn row_fill(selected: bool, idle: impl Shade) -> Color32 {
    if selected { argb(pc::ACCENT) } else { argb(idle) }
}

#[cfg(test)]
mod tests {
    use egui::emath::TSTransform;
    use egui::{Pos2, Rect, pos2, vec2};

    use super::{ARROW_SHIFT, POPOVER_GAP, PanelSpace, drawing_panel, place, put_part_scales, side_bar_at};
    use crate::look::pc;

    fn near(a: Pos2, b: Pos2) -> bool {
        (a - b).length() < 1e-3
    }

    #[test]
    fn panels_as_big_as_the_rest_are_laid_out_in_the_window_itself() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        let space = PanelSpace::new(window, pc::BAR_HEIGHT, 1.0);
        assert_eq!((space.to_window, space.screen), (TSTransform::IDENTITY, window));
        // under a bigger top bar they only hang lower
        let space = PanelSpace::new(window, pc::BAR_HEIGHT * 1.5, 1.0);
        assert_eq!(space.to_window, TSTransform::IDENTITY);
        assert_eq!(space.screen.top() + pc::BAR_HEIGHT, pc::BAR_HEIGHT * 1.5);
        assert_eq!((space.screen.left(), space.screen.right(), space.screen.bottom()), (0.0, 1200.0, 800.0));
    }

    #[test]
    fn panels_bigger_or_smaller_hang_from_the_top_bar_as_it_shows_and_measure_the_window_to_its_edges() {
        let window = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        for (bar, scale) in [(1.0, 1.5), (1.25, 0.5), (0.5, 1.5), (1.5, 0.75)] {
            let PanelSpace { to_window, screen } = PanelSpace::new(window, pc::BAR_HEIGHT * bar, scale);
            assert_eq!(to_window.scaling, scale);
            // a panel's top, under its own top bar, shows where the top bar ends
            let top = to_window * pos2(screen.left(), screen.top() + pc::BAR_HEIGHT);
            assert!(near(top, pos2(0.0, pc::BAR_HEIGHT * bar)), "{bar} {scale}: {top:?}");
            assert!(near(to_window * screen.max, window.max), "{bar} {scale}");
        }
    }

    #[test]
    fn a_panel_bigger_or_smaller_points_at_its_button_inside_the_window_clear_of_the_side_bar_as_it_shows() {
        let ctx = egui::Context::default();
        let window = Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 800.0));
        let raw = egui::RawInput { screen_rect: Some(window), ..Default::default() };
        let mut output = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx();
            let width = 384.0;
            for side in [1.0, 1.5] {
                put_part_scales(ctx, [1.0, 1.0, side, 1.0, 1.0]);
                for right in [false, true] {
                    side_bar_at(ctx, true, right, procraft_core::side_bar_first_centre());
                    for scale in [0.5, 1.0, 1.5] {
                        let space = PanelSpace::new(window, pc::BAR_HEIGHT, scale);
                        drawing_panel(ctx, Some(space));
                        for button in [70.0, 600.0, 1150.0] {
                            let local = (space.to_window.inverse() * pos2(button, 0.0)).x;
                            let (left, tip) = place(ctx, width, local);
                            let shown = |x: f32| (space.to_window * pos2(x, 0.0)).x;
                            let case = format!("{side} {right} {scale} {button}");
                            let clear = pc::SIDE_BAR_WIDTH * side + POPOVER_GAP;
                            let (lo, hi) =
                                if right { (POPOVER_GAP, 1200.0 - clear) } else { (clear, 1200.0 - POPOVER_GAP) };
                            assert!(shown(left) >= lo - 1e-3 && shown(left + width) <= hi + 1e-3, "{case}");
                            if button == 600.0 {
                                assert!((shown(left + tip) - (button + ARROW_SHIFT)).abs() < 1e-3, "{case}");
                            }
                        }
                    }
                }
            }
            drawing_panel(ctx, None);
        });
        output.textures_delta.clear();
    }
}

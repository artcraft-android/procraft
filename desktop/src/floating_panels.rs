//! The Layers panel and the brush library torn off into windows of their own, while a plugin lets
//! them float, as the Android app has them (ui/FloatingPanels.kt): each dragged out by a grabber at
//! its top as the colour panel is, then a window over the canvas that stays open while one draws,
//! moved by the strip at its top, sized from its lower right corner and closed by its Close (its
//! button then hangs it from the bar again). Inside, the panel is the same as where it hangs. Where
//! each window is, and how big, is the core's to work out ([`procraft_core::floating_fitted`]), and
//! is kept with the preferences.

use egui::emath::TSTransform;
use egui::{Pos2, Rect, pos2, vec2};
use procraft_core::{FloatingPanel, FloatingRoom, FloatingWindow, Plugins};

/// The panels that may float, each by its place in [`Floating::windows`].
pub const PANELS: [FloatingPanel; 2] = [FloatingPanel::Layers, FloatingPanel::Brushes];

/// How far a panel's grabber is dragged before it floats, and how far down its window's grabber is
/// (at the window's own size): what is held as it is dragged out.
const UNDOCK_DRAG: f32 = 30.0;
const GRAB_Y: f32 = 12.0;

/// A window being moved by its strip or sized from its corner: whose, which, where the pointer was
/// when it was pressed (the window's points), and the window then.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Held {
    panel: FloatingPanel,
    sizing: bool,
    from: Pos2,
    start: FloatingWindow,
}

/// The floating panels' windows, and what is being done to them.
#[derive(Default)]
pub struct Floating {
    /// Each panel's window as it was last left, shown or closed (none: it never floated), by its
    /// place in [`PANELS`].
    pub windows: [Option<FloatingWindow>; 2],
    /// Which the plugins let float now, as they said at their generation.
    allowed: [bool; 2],
    allowed_for: Option<u64>,
    held: Option<Held>,
    /// Something changed that is kept with the preferences.
    pub changed: bool,
}

/// `panel`'s place in [`PANELS`].
fn at(panel: FloatingPanel) -> usize {
    panel as usize
}

impl Floating {
    /// The windows as they were kept, each by its place in [`PANELS`].
    pub fn kept(windows: [Option<FloatingWindow>; 2]) -> Floating {
        Floating { windows, ..Floating::default() }
    }

    /// Which panels float, as `plugins` say now (asked again only when they change).
    pub fn follow(&mut self, plugins: &Plugins) {
        let now = plugins.generation();
        if self.allowed_for != Some(now) {
            self.allowed_for = Some(now);
            self.allowed = PANELS.map(|p| plugins.floats(p));
        }
    }

    /// Whether a plugin lets `panel` float.
    pub fn allowed(&self, panel: FloatingPanel) -> bool {
        self.allowed.get(at(panel)).copied().unwrap_or(false)
    }

    /// `panel`'s window while it shows: torn off and not closed, and a plugin lets it float.
    pub fn shown(&self, panel: FloatingPanel) -> Option<FloatingWindow> {
        let window = self.windows.get(at(panel)).copied().flatten()?;
        (window.open && self.allowed(panel)).then_some(window)
    }

    fn set(&mut self, panel: FloatingPanel, window: FloatingWindow) {
        if let Some(slot) = self.windows.get_mut(at(panel)) {
            *slot = Some(window);
        }
    }

    /// `panel`'s grabber, hanging from its button, dragged from `from` to `pointer` (the window's
    /// points): past [`UNDOCK_DRAG`] it floats, its window as big as it was last left, its grabber
    /// under the pointer and following it until let go; true then (the panel that hung closes).
    pub fn undock(&mut self, ctx: &egui::Context, panel: FloatingPanel, from: Pos2, pointer: Pos2) -> bool {
        if !self.allowed(panel) || pointer.distance(from) <= UNDOCK_DRAG {
            return false;
        }
        let k = scale(ctx);
        let first = procraft_core::floating_at(panel, pointer.x, pointer.y - GRAB_Y * k, k);
        let sized = match self.windows.get(at(panel)).copied().flatten() {
            Some(was) => {
                FloatingWindow { x: pointer.x - was.width * k / 2.0, width: was.width, height: was.height, ..first }
            }
            None => first,
        };
        let start = procraft_core::floating_fitted(panel, sized, room(ctx), k);
        self.set(panel, start);
        self.held = Some(Held { panel, sizing: false, from: pointer, start });
        true
    }

    /// `panel`'s window closed: where it was and its size kept for when it floats again.
    pub fn close(&mut self, panel: FloatingPanel) {
        if let Some(window) = self.windows.get(at(panel)).copied().flatten() {
            self.set(panel, FloatingWindow { open: false, ..window });
            self.changed = true;
        }
    }

    /// `panel`'s strip (or, `sizing`, its corner) pressed with the pointer at `pointer` (the
    /// window's points): its window moves (or is sized) with the pointer until let go.
    pub fn hold(&mut self, panel: FloatingPanel, sizing: bool, pointer: Pos2) {
        if let Some(start) = self.shown(panel) {
            self.held = Some(Held { panel, sizing, from: pointer, start });
        }
    }

    /// A window held follows the pointer while it is down, kept once let go.
    pub fn follow_pointer(&mut self, ctx: &egui::Context) {
        let Some(held) = self.held else { return };
        let (pointer, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        match pointer {
            Some(p) if down => {
                let (k, room) = (scale(ctx), room(ctx));
                let by = p - held.from;
                let window = if held.sizing {
                    procraft_core::floating_resized(held.panel, held.start, by.x, by.y, room, k)
                } else {
                    procraft_core::floating_moved(held.panel, held.start, by.x, by.y, room, k)
                };
                self.set(held.panel, window);
                ctx.request_repaint();
            }
            _ => {
                self.held = None;
                self.changed = true;
            }
        }
    }

    /// Where `window` shows for `panel` now (inside the room, which may have shrunk since it was
    /// left), and the space it is drawn in: laid out at its own size from its top-left, shown at the
    /// floating windows' size. Its rect is in that space.
    pub fn space(
        ctx: &egui::Context,
        panel: FloatingPanel,
        window: FloatingWindow,
    ) -> (Rect, crate::panel::PanelSpace) {
        let k = scale(ctx);
        let window = procraft_core::floating_fitted(panel, window, room(ctx), k);
        let corner = pos2(window.x, window.y);
        let rect = Rect::from_min_size(corner, vec2(window.width, window.height));
        let to_window = TSTransform::from_translation(corner.to_vec2())
            * TSTransform::from_scaling(k)
            * TSTransform::from_translation(-corner.to_vec2());
        (rect, crate::panel::PanelSpace { to_window, screen: to_window.inverse().mul_rect(ctx.content_rect()) })
    }
}

/// How big the floating windows show next to the rest of the interface, as a plugin says.
fn scale(ctx: &egui::Context) -> f32 {
    crate::panel::part_scale(ctx, procraft_core::InterfacePart::Floating)
}

/// Where the floating windows may go: the window, clear of the side bar as the panels are.
fn room(ctx: &egui::Context) -> FloatingRoom {
    let screen = ctx.content_rect();
    let (mut left, mut right) = (screen.left(), screen.right());
    if let Some(bar) = crate::panel::side_bar_shown_at(ctx) {
        if bar.center().x < screen.center().x {
            left = left.max(bar.right() + crate::panel::POPOVER_GAP);
        } else {
            right = right.min(bar.left() - crate::panel::POPOVER_GAP);
        }
    }
    FloatingRoom { left, top: screen.top(), right: right.max(left), bottom: screen.bottom() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_floats_only_while_a_plugin_lets_it_and_it_isn_t_closed() {
        let mut floating = Floating::default();
        let window = FloatingWindow { open: true, x: 10.0, y: 20.0, width: 384.0, height: 520.0 };
        floating.windows = [Some(window), None];
        assert_eq!(floating.shown(FloatingPanel::Layers), None, "no plugin lets it");
        floating.allowed = [true, true];
        assert_eq!(floating.shown(FloatingPanel::Layers), Some(window));
        assert_eq!(floating.shown(FloatingPanel::Brushes), None, "it never floated");
        floating.close(FloatingPanel::Layers);
        assert!(floating.changed);
        assert_eq!(floating.shown(FloatingPanel::Layers), None);
        assert_eq!(floating.windows[0], Some(FloatingWindow { open: false, ..window }), "kept where it was");
        // held, it moves from where it was when pressed
        floating.windows[0] = Some(window);
        floating.hold(FloatingPanel::Layers, false, pos2(50.0, 50.0));
        assert_eq!(floating.held.map(|h| (h.panel, h.sizing, h.start)), Some((FloatingPanel::Layers, false, window)));
        // one that doesn't show isn't held
        floating.held = None;
        floating.hold(FloatingPanel::Brushes, true, pos2(50.0, 50.0));
        assert_eq!(floating.held, None);
    }
}

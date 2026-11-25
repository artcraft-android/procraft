//! The easing editor, as the iPad app's (TimelineBar.swift, `EasingPage` and `EasingCurveView`;
//! TimelineCurves.swift, `MyCurves`): how a key eases into the next, as a curve, its kind, that
//! kind's numbers, how often it repeats (arriving each time, or looping back) and maybe backwards,
//! and under it "My curves". Every change shows at once; a slider or point let go, or a tap, is
//! done (one undo step).
//!
//! A kind's curve is drawn time across, how far along up (-0.5 to 1.5, so overshoots show). Ease's
//! two handles are dragged on it. Custom's points are its controls: one dragged moves between its
//! neighbours, a press away from them adds one there, and one let go well above or below the frame
//! is taken away; the first and last stay (`Easing::Curve`, a natural spline through them).
//!
//! The key rows call it, each frame while it is open:
//!
//! ```ignore
//! // owned by whoever shows it, kept while the app runs
//! let mut editor = EasingEditor::default();
//! let mut curves = SavedCurves::open();
//! // each frame: `easing` is the key's, changed in place
//! match editor.show(ctx, look, rect, &mut easing, &mut curves) {
//!     Some(Done::Live) => { /* show it at once: set_timeline_key_easing(.., EasingSpec::of(&easing), false) */ }
//!     Some(Done::Set) => { /* one undo step: set_timeline_key_easing(.., EasingSpec::of(&easing), true) */ }
//!     Some(Done::Back) => { /* close it (back to the key's menu) */ }
//!     None => {}
//! }
//! ```
//!
//! [`EasingEditor::show`] draws its own panel at `rect` (scrolling inside it when taller);
//! [`EasingEditor::show_in`] draws only what is in it, in a panel of the caller's.
//! [`EasingEditor::height`] is how tall it was last drawn, to size the panel by.
//! [`crate::saved_curves::easing_choices`] lists My curves for a key's Easing menu.

// (opened from a key's rows, which come later: until then only its tests reach all of it)
#![allow(dead_code)]

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use procraft_core::{EasingKind, EasingSpec, default_easing};
use procraft_engine::timeline::Easing;

use crate::alert::{Alert, Answer};
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::panel::PANEL_TITLE;
use crate::saved_curves::SavedCurves;

/// The width it is meant for.
pub const WIDTH: f32 = 320.0;

const FILL: Themed = Themed(0xFF383838, 0xFFE6E6E6);
const GROUP: Themed = Themed(0x420C0C0C, 0xFFE8E8E8);
const ROW_LINE: Themed = Themed(0x09FFFFFF, 0x0F000000);
const SLIDE_LABEL: Themed = Themed(0xFFC1C1C1, 0xFF3D3D3D);
const SLIDE_VALUE: Themed = Themed(0xFFEDEDED, 0xFF3D3D3D);
const SLIDE_TRACK: Themed = Themed(0xFF414141, 0xFFBEBEBE);
const INK_DIM: Themed = Themed(0xFF8E8E8E, 0xFF7A7A7A);
const DANGER: u32 = 0xFFFE604D;
/// Inside the panel's edge.
const INSET: f32 = 8.0;
const CURVE_HEIGHT: f32 = 110.0;
/// A curve of your own is drawn by its points, and has more room for it.
const DRAWN_HEIGHT: f32 = 170.0;
const HINT: &str = "Click to add a point. Drag a point to shape the curve, or off the top or bottom to remove it.";
const ROW: f32 = 44.0;
/// How near a press is to a point to take it, how far off the frame takes one away, and the least
/// two points are apart in time.
const REACH: f32 = 26.0;
const AWAY: f32 = 30.0;
const APART: f32 = 0.03;

/// The kinds, as the iPad names and orders them.
pub const KINDS: [(EasingKind, &str); 11] = [
    (EasingKind::Linear, "Linear"),
    (EasingKind::Smooth, "Smooth"),
    (EasingKind::CubicBezier, "Ease"),
    (EasingKind::Hold, "Hold"),
    (EasingKind::Bounce, "Bounce"),
    (EasingKind::Elastic, "Elastic"),
    (EasingKind::Steps, "Steps"),
    (EasingKind::ElasticSteps, "Springy Steps"),
    (EasingKind::Cyclic, "Cyclic"),
    (EasingKind::Random, "Random"),
    (EasingKind::Curve, "Custom"),
];

/// Each kind's numbers: name, least, most.
fn controls(kind: EasingKind) -> &'static [(&'static str, f32, f32)] {
    match kind {
        EasingKind::CubicBezier => {
            &[("Start pull", 0.0, 1.0), ("Start lift", -0.5, 1.5), ("End pull", 0.0, 1.0), ("End lift", -0.5, 1.5)]
        }
        EasingKind::Bounce => &[("First bounce", 0.01, 1.0), ("Bounciness", 0.01, 0.99)],
        EasingKind::Elastic => {
            &[("Wave length", 0.01, 1.0), ("Attack", 0.0, 2.0), ("Decay", 0.0, 1.0), ("Swing", 0.0, 1.0)]
        }
        EasingKind::Steps => &[("Step length", 0.01, 1.0), ("Smoothing", 0.0, 1.0)],
        EasingKind::ElasticSteps => &[("Step length", 0.01, 1.0), ("Springiness", 0.0, 1.0)],
        EasingKind::Cyclic => {
            &[("Cycle length", 0.01, 1.0), ("Sharpness", 0.0, 1.0), ("Turn", 0.01, 0.99), ("Drift", 0.0, 1.0)]
        }
        EasingKind::Random => &[
            ("Frequency", 0.0, 1.0),
            ("Magnitude", 0.0, 2.0),
            ("Seed", 0.0, 1.0),
            ("Fade", 0.0, 1.0),
            ("Warp", 0.0, 1.0),
            ("Tilt", 0.0, 1.0),
        ],
        EasingKind::Linear | EasingKind::Hold | EasingKind::Smooth | EasingKind::Curve => &[],
    }
}

/// What the editor did to the easing this frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Done {
    /// Changed, and still held (a slider, a point or a handle): show it at once, no undo step yet.
    Live,
    /// Changed and let go, a kind or a curve picked, a switch flipped: one undo step.
    Set,
    /// Its back chevron: done with it.
    Back,
}

/// Which name is being asked for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Ask {
    New,
    Rename(u64),
}

/// A press on the curve, while it is held.
#[derive(Clone, Debug, Default)]
struct Held {
    /// Where it was pressed.
    start: Option<Pos2>,
    /// Custom's point held (its place among the curve's), or Ease's handle (0 or 1).
    held: Option<usize>,
    /// The point is far enough off the frame to be taken away when let go.
    off: bool,
    /// The points as the press has made them so far.
    working: Option<Vec<[f32; 2]>>,
    /// Where the pointer was last.
    last: Option<Pos2>,
}

#[derive(Default)]
pub struct EasingEditor {
    held: Held,
    /// The kept curve whose actions show.
    open: Option<u64>,
    alert: Alert,
    asking: Option<Ask>,
    /// How far down it is scrolled, and how tall its content was last drawn.
    scroll: f32,
    content: f32,
    /// Frames to go scrolled to the foot (a new curve brought into view once it is measured).
    to_foot: u8,
    /// Where the curve was last drawn.
    curve_rect: Option<Rect>,
}

/// A drawn curve's points as the engine reads them: in time's order, the ends pinned.
fn points(spec: &EasingSpec) -> Vec<[f32; 2]> {
    let mut pts: Vec<[f32; 2]> =
        spec.params.as_chunks::<2>().0.iter().map(|[x, y]| [x.clamp(0.0, 1.0), y.clamp(-0.5, 1.5)]).collect();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
    if pts.len() < 2 {
        return vec![[0.0, 0.0], [1.0, 1.0]];
    }
    if let Some(first) = pts.first_mut() {
        *first = [0.0, 0.0];
    }
    if let Some(last) = pts.last_mut() {
        *last = [1.0, 1.0];
    }
    pts
}

/// Ease's two handles.
fn handles(spec: &EasingSpec) -> [[f32; 2]; 2] {
    let p = |i: usize, or: f32| spec.params.get(i).copied().filter(|v| v.is_finite()).unwrap_or(or);
    [[p(0, 0.42), p(1, 0.0)], [p(2, 0.58), p(3, 1.0)]]
}

/// A point of the curve's (time, how far along) in `rect`.
fn place(rect: Rect, [x, y]: [f32; 2]) -> Pos2 {
    pos2(rect.left() + rect.width() * x, rect.top() + rect.height() * (1.5 - y) / 2.0)
}

/// Where `p` is on the curve's frame (time not held to it, how far along held to -0.5..1.5).
fn unplace(rect: Rect, p: Pos2) -> [f32; 2] {
    let x = (p.x - rect.left()) / rect.width().max(1.0);
    let y = 1.5 - 2.0 * (p.y - rect.top()) / rect.height().max(1.0);
    [x, y.clamp(-0.5, 1.5)]
}

impl Held {
    /// Custom's points as a press from `self.start` now at `at` makes them; none if it can't (no
    /// room for a new point there).
    fn drawn(&mut self, spec: &EasingSpec, rect: Rect, at: Pos2) -> Option<EasingSpec> {
        let start = self.start?;
        let mut pts = self.working.clone().unwrap_or_else(|| points(spec));
        let [x, y] = unplace(rect, at);
        if self.held.is_none() {
            let far = |i: usize| pts.get(i).map_or(f32::INFINITY, |p| place(rect, *p).distance(start));
            let near = (1..pts.len().saturating_sub(1)).min_by(|a, b| far(*a).total_cmp(&far(*b)));
            match near {
                Some(near) if far(near) <= REACH => self.held = Some(near),
                _ => {
                    // a new one, where there is room between two for it
                    let i = pts.iter().position(|p| p[0] > x).unwrap_or(pts.len().saturating_sub(1));
                    let before = pts.get(i.checked_sub(1)?)?[0];
                    if pts.get(i)?[0] - before < 2.0 * APART {
                        return None;
                    }
                    pts.insert(i, [x, y]);
                    self.held = Some(i);
                }
            }
        }
        let i = self.held?;
        let least = pts.get(i.checked_sub(1)?)?[0] + APART;
        let most = (pts.get(i + 1)?[0] - APART).max(least);
        *pts.get_mut(i)? = [x.clamp(least, most), y];
        self.off = at.y < rect.top() - AWAY || at.y > rect.bottom() + AWAY;
        self.working = Some(pts.clone());
        Some(EasingSpec { params: pts.into_iter().flatten().collect(), ..spec.clone() })
    }

    /// Custom's points as the press let go leaves them: the point held taken away if it was off
    /// the frame.
    fn lifted(&mut self, spec: &EasingSpec) -> Option<EasingSpec> {
        let held = std::mem::take(self);
        let (i, mut pts) = (held.held?, held.working?);
        if held.off && i > 0 && i + 1 < pts.len() {
            pts.remove(i);
        }
        Some(EasingSpec { params: pts.into_iter().flatten().collect(), ..spec.clone() })
    }

    /// Ease with its handle near where the press began moved to `at`.
    fn handled(&mut self, spec: &EasingSpec, rect: Rect, at: Pos2) -> Option<EasingSpec> {
        let hs = handles(spec);
        if self.held.is_none() {
            let start = self.start?;
            let far = |i: usize| hs.get(i).map_or(f32::INFINITY, |p| place(rect, *p).distance(start));
            let near = if far(1) < far(0) { 1 } else { 0 };
            if far(near) > REACH {
                return None;
            }
            self.held = Some(near);
        }
        let i = self.held?;
        let [x, y] = unplace(rect, at);
        let mut params: Vec<f32> = hs.into_iter().flatten().collect();
        *params.get_mut(2 * i)? = x.clamp(0.0, 1.0);
        *params.get_mut(2 * i + 1)? = y;
        Some(EasingSpec { params, ..spec.clone() })
    }
}

impl EasingEditor {
    /// How tall what it shows was when last drawn (its panel's insets in), to size a panel by.
    pub fn height(&self) -> f32 {
        self.content
    }

    /// The editor in a panel of its own at `rect`, scrolling when what it shows is taller;
    /// `easing` changed in place. See the module's notes for what it gives.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        rect: Rect,
        easing: &mut Easing,
        curves: &mut SavedCurves,
    ) -> Option<Done> {
        let mut done = None;
        egui::Area::new(Id::unique("easing editor")).fixed_pos(rect.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(rect, Sense::click_and_drag());
            ui.painter().rect_filled(rect, 24, argb(FILL));
            done = self.show_in(ui, look, rect, easing, curves);
        });
        done
    }

    /// What the editor shows, in `rect` of a panel the caller draws.
    pub fn show_in(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        rect: Rect,
        easing: &mut Easing,
        curves: &mut SavedCurves,
    ) -> Option<Done> {
        let spec = EasingSpec::of(easing);
        // scrolled by the wheel over it, as far as what it shows goes
        if ui.rect_contains_pointer(rect) {
            self.scroll -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        if self.to_foot > 0 {
            self.to_foot -= 1;
            self.scroll = f32::INFINITY;
        }
        self.scroll = self.scroll.clamp(0.0, (self.content - rect.height()).max(0.0));
        let mut done = None;
        let mut next = None;
        ui.scope(|ui| {
            ui.set_clip_rect(rect.intersect(ui.clip_rect()));
            let (left, right) = (rect.left() + INSET, rect.right() - INSET);
            let mut y = rect.top() + INSET - self.scroll;
            // the back chevron and "Easing"
            y += 6.0;
            let back = Rect::from_min_size(pos2(left - INSET, y), vec2(44.0, 34.0));
            look.icon(ui, &icons::CHEVRON_LEFT, back.center(), 34.0, argb(SLIDE_VALUE));
            text(
                ui.painter(),
                pos2(back.right(), y + 17.0),
                Align2::LEFT_CENTER,
                "Easing",
                23.0,
                Weight::Bold,
                argb(PANEL_TITLE),
            );
            if ui.interact(back, ui.scope_id().with("back"), Sense::click()).clicked() {
                done = Some(Done::Back);
            }
            y += 34.0 + 10.0;
            // the curve
            let drawn = spec.kind == EasingKind::Curve;
            let curve =
                Rect::from_min_max(pos2(left, y), pos2(right, y + if drawn { DRAWN_HEIGHT } else { CURVE_HEIGHT }));
            self.curve_rect = Some(curve);
            if let Some((changed, let_go)) = self.curve(ui, curve, &spec) {
                next = Some(changed);
                done = done.or(Some(if let_go { Done::Set } else { Done::Live }));
            }
            y = curve.bottom();
            if drawn {
                y += 4.0;
                let hint =
                    crate::look::wrapped(ui.painter(), HINT, 12.0, Weight::Regular, argb(INK_DIM), right - left - 12.0);
                let h = hint.size().y;
                ui.painter().galley(pos2(left + 6.0, y), hint, argb(INK_DIM));
                y += h + 8.0;
            } else {
                y += 10.0;
            }
            // the kinds, three across
            let cell = (right - left - 8.0) / 3.0;
            for (k, (kind, label)) in KINDS.iter().enumerate() {
                let at = pos2(left + (cell + 4.0) * (k % 3) as f32, y + 34.0 * (k / 3) as f32);
                let chip = Rect::from_min_size(at, vec2(cell, 30.0));
                let chosen = spec.kind == *kind;
                ui.painter().rect_filled(chip, 9, if chosen { argb(pc::ACCENT) } else { argb(GROUP) });
                let (weight, ink) =
                    if chosen { (Weight::SemiBold, Color32::WHITE) } else { (Weight::Regular, argb(PANEL_TITLE)) };
                text(ui.painter(), chip.center(), Align2::CENTER_CENTER, label, 14.0, weight, ink);
                if ui.interact(chip, ui.scope_id().with(("kind", k)), Sense::click()).clicked() {
                    next = Some(EasingSpec {
                        repeats: spec.repeats,
                        looping: spec.looping,
                        reversed: spec.reversed,
                        ..default_easing(*kind)
                    });
                    done = done.or(Some(Done::Set));
                }
            }
            y += 34.0 * KINDS.len().div_ceil(3) as f32 - 4.0 + 10.0;
            // its numbers, and how often it repeats
            let numbers = controls(spec.kind);
            let group = Rect::from_min_max(pos2(left, y), pos2(right, y + slider_group(numbers.len() + 1)));
            ui.painter().rect_filled(group, 18, argb(GROUP));
            let mut row_top = y + 8.0;
            for (i, (label, least, most)) in numbers.iter().enumerate() {
                let value = spec.params.get(i).copied().unwrap_or(*least);
                let row = Rect::from_min_max(pos2(left, row_top), pos2(right, row_top + 55.0));
                let shown = format!("{value:.2}");
                let (moved, let_go) = slider(ui, row, label, &shown, (value - least) / (most - least), i);
                if let Some(v) = moved {
                    let mut changed = spec.clone();
                    while changed.params.len() <= i {
                        changed.params.push(*least);
                    }
                    if let Some(p) = changed.params.get_mut(i) {
                        *p = least + v * (most - least);
                    }
                    next = Some(changed);
                    done = done.or(Some(Done::Live));
                }
                if let_go {
                    done = Some(done.map_or(Done::Set, |d| if d == Done::Back { d } else { Done::Set }));
                }
                row_top += 55.0 + 11.5;
            }
            let row = Rect::from_min_max(pos2(left, row_top), pos2(right, row_top + 55.0));
            let shown = spec.repeats.to_string();
            let share = (spec.repeats.max(1) - 1) as f32 / 7.0;
            let (moved, let_go) = slider(ui, row, "Repeats", &shown, share, numbers.len());
            if let Some(v) = moved {
                next = Some(EasingSpec { repeats: 1 + (v * 7.0).round() as u32, ..spec.clone() });
                done = done.or(Some(Done::Live));
            }
            if let_go {
                done = Some(done.map_or(Done::Set, |d| if d == Done::Back { d } else { Done::Set }));
            }
            y = group.bottom() + 10.0;
            // looping back, backwards
            let group = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW * 2.0 + 2.0));
            ui.painter().rect_filled(group, 18, argb(GROUP));
            let loop_row = Rect::from_min_size(group.min, vec2(group.width(), ROW));
            if toggle_row(ui, loop_row, "Loop back", spec.looping) {
                next = Some(EasingSpec { looping: !spec.looping, ..spec.clone() });
                done = done.or(Some(Done::Set));
            }
            row_line(ui, loop_row.bottom(), left, right);
            let back_row = Rect::from_min_size(pos2(left, loop_row.bottom() + 2.0), vec2(group.width(), ROW));
            if toggle_row(ui, back_row, "Backwards", spec.reversed) {
                next = Some(EasingSpec { reversed: !spec.reversed, ..spec.clone() });
                done = done.or(Some(Done::Set));
            }
            y = group.bottom() + 14.0;
            // My curves
            if let Some(picked) = self.my_curves(ui, look, &mut y, left, right, &spec, curves) {
                next = Some(picked);
                done = done.or(Some(Done::Set));
            }
            y += 10.0 + INSET;
            self.content = y + self.scroll - rect.top();
        });
        if let Some(answer) = self.alert.show(ui.ctx(), look) {
            let typed = self.alert.typed().unwrap_or_default().to_owned();
            match (answer, self.asking.take()) {
                (Answer::Action, Some(Ask::New)) => {
                    curves.add(spec.clone(), &typed);
                    // brought into view
                    self.to_foot = 2;
                }
                (Answer::Action, Some(Ask::Rename(id))) => curves.rename(id, &typed),
                _ => {}
            }
        }
        if let Some(next) = next
            && next != spec
        {
            *easing = next.easing();
        }
        done
    }

    /// The curve in `rect`, and the presses on it (Ease's handles, Custom's points): the easing
    /// as they change it, and whether it was let go.
    fn curve(&mut self, ui: &Ui, rect: Rect, spec: &EasingSpec) -> Option<(EasingSpec, bool)> {
        let custom = spec.kind == EasingKind::Curve;
        let ease = spec.kind == EasingKind::CubicBezier;
        let response = ui.interact(rect, ui.scope_id().with("curve"), Sense::click_and_drag());
        let down = response.is_pointer_button_down_on();
        let mut changed = None;
        if (custom || ease) && (down || response.clicked()) {
            if self.held.start.is_none() {
                let start = ui.input(|i| i.pointer.press_origin()).or(response.interact_pointer_pos());
                self.held = Held { start, ..Held::default() };
            }
            let at = response.interact_pointer_pos().or(self.held.last);
            self.held.last = at;
            if let Some(at) = at {
                let moved = if custom { self.held.drawn(spec, rect, at) } else { self.held.handled(spec, rect, at) };
                changed = moved.map(|m| (m, false));
            }
        }
        if !down && self.held.start.is_some() {
            // let go
            let was = changed.as_ref().map_or(spec, |c| &c.0).clone();
            let settled = if custom { self.held.lifted(&was) } else { self.held.held.map(|_| was) };
            self.held = Held::default();
            changed = settled.map(|s| (s, true));
        }
        let shown = changed.as_ref().map_or(spec, |c| &c.0);
        let painter = ui.painter();
        painter.rect_filled(rect, 14, argb(GROUP));
        for level in [0.0, 1.0] {
            let y = place(rect, [0.0, level]).y;
            painter.rect_filled(
                Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), 1.0)),
                0,
                argb(INK_DIM).gamma_multiply(0.4),
            );
        }
        let e = shown.easing();
        let n = 96;
        let line: Vec<Pos2> = (0..n)
            .map(|i| {
                let t = i as f32 / (n - 1) as f32;
                place(rect, [t, e.progress(t).clamp(-0.5, 1.5)])
            })
            .collect();
        let faint = custom && self.held.off;
        let accent = argb(pc::ACCENT);
        painter.add(egui::Shape::line(line, Stroke::new(2.5, if faint { accent.gamma_multiply(0.5) } else { accent })));
        if custom {
            let pts = points(shown);
            for (i, p) in pts.iter().enumerate() {
                let at = place(rect, *p);
                let pinned = i == 0 || i + 1 == pts.len();
                if pinned {
                    painter.circle_filled(at, 4.0, argb(INK_DIM));
                } else {
                    let held = self.held.held == Some(i);
                    let fade = if held && self.held.off { 0.3 } else { 1.0 };
                    let r = if held { 9.0 } else { 7.0 };
                    painter.circle_filled(at, r, accent.gamma_multiply(fade));
                    painter.circle_stroke(at, r, Stroke::new(2.0, Color32::WHITE.gamma_multiply(fade)));
                }
            }
        } else if ease {
            let hs = handles(shown);
            for (from, h) in [[0.0, 0.0], [1.0, 1.0]].into_iter().zip(hs) {
                let at = place(rect, h);
                painter.line_segment([place(rect, from), at], Stroke::new(1.5, argb(INK_DIM)));
                painter.circle_filled(at, 7.0, accent);
                painter.circle_stroke(at, 7.0, Stroke::new(2.0, Color32::WHITE));
            }
        }
        changed
    }

    /// "My curves": the easing showing kept as a new one, and each one kept, put on the key with
    /// a click, or renamed, duplicated, replaced with what is showing, or deleted (its ⋯). The
    /// curve clicked, if one was.
    #[allow(clippy::too_many_arguments)]
    fn my_curves(
        &mut self,
        ui: &Ui,
        look: &mut Look,
        y: &mut f32,
        left: f32,
        right: f32,
        spec: &EasingSpec,
        curves: &mut SavedCurves,
    ) -> Option<EasingSpec> {
        let title = argb(PANEL_TITLE);
        text(
            ui.painter(),
            pos2(left + 16.0, *y),
            Align2::LEFT_TOP,
            "My curves",
            13.0,
            Weight::SemiBold,
            title.gamma_multiply(0.6),
        );
        *y += 16.0 + 6.0;
        let open_rows = usize::from(self.open.is_some_and(|id| curves.get(id).is_some()));
        let height = ROW + (ROW + 2.0) * curves.all().len() as f32 + 38.0 * open_rows as f32;
        let group = Rect::from_min_max(pos2(left, *y), pos2(right, *y + height));
        ui.painter().rect_filled(group, 18, argb(GROUP));
        let save = Rect::from_min_size(group.min, vec2(group.width(), ROW));
        look.icon(ui, &icons::PLUS_MEDIUM, pos2(left + 12.0 + 23.0, save.center().y), 22.0, argb(pc::ACCENT));
        text(
            ui.painter(),
            pos2(left + 12.0 + 56.0, save.center().y),
            Align2::LEFT_CENTER,
            "Save this curve",
            17.0,
            Weight::Regular,
            argb(pc::ACCENT),
        );
        if ui.interact(save, ui.scope_id().with("save curve"), Sense::click()).clicked() {
            self.open = None;
            self.asking = Some(Ask::New);
            self.alert.ask_name("Save curve", &curves.next_name(), "Save");
        }
        let mut top = save.bottom();
        let mut picked = None;
        let mut act: Option<(u64, &str)> = None;
        for curve in curves.all() {
            row_line(ui, top, left, right);
            top += 2.0;
            let row = Rect::from_min_size(pos2(left, top), vec2(group.width(), ROW));
            let chosen = curve.spec == *spec;
            let thumb = Rect::from_min_size(pos2(left + 12.0, row.center().y - 14.0), vec2(46.0, 28.0));
            curve_thumb(ui, thumb, &curve.spec);
            let (weight, ink) = if chosen { (Weight::SemiBold, argb(pc::ACCENT)) } else { (Weight::Regular, title) };
            let name_left = thumb.right() + 10.0;
            let more = Rect::from_min_size(pos2(right - 44.0, row.top()), vec2(44.0, 44.0));
            let room = more.left() - name_left;
            let name = fitted(ui, &curve.name, room, weight);
            text(ui.painter(), pos2(name_left, row.center().y), Align2::LEFT_CENTER, &name, 17.0, weight, ink);
            let is_open = self.open == Some(curve.id);
            let more_ink = if is_open { argb(pc::ACCENT) } else { title.gamma_multiply(0.6) };
            look.icon(ui, &icons::MORE, more.center(), 22.0, more_ink);
            // (the ⋯ over the row)
            let tapped = ui.interact(row, ui.scope_id().with(("curve", curve.id)), Sense::click()).clicked();
            if ui.interact(more, ui.scope_id().with(("curve more", curve.id)), Sense::click()).clicked() {
                self.open = if is_open { None } else { Some(curve.id) };
            } else if tapped {
                picked = Some(curve.spec.clone());
            }
            top = row.bottom();
            if is_open {
                let actions = ["Rename", "Duplicate", "Replace", "Delete"];
                let w = (group.width() - 16.0 - 4.0 * 3.0) / 4.0;
                for (k, label) in actions.iter().enumerate() {
                    let button = Rect::from_min_size(pos2(left + 8.0 + (w + 4.0) * k as f32, top), vec2(w, 30.0));
                    let dim = *label == "Replace" && chosen;
                    let ink = if *label == "Delete" { argb(DANGER) } else { title };
                    ui.painter().rect_filled(button, 9, title.gamma_multiply(0.08));
                    let ink = if dim { ink.gamma_multiply(0.35) } else { ink };
                    text(ui.painter(), button.center(), Align2::CENTER_CENTER, label, 13.0, Weight::Medium, ink);
                    if ui.interact(button, ui.scope_id().with(("curve action", curve.id, k)), Sense::click()).clicked()
                        && !dim
                    {
                        act = Some((curve.id, label));
                    }
                }
                top += 38.0;
            }
        }
        match act {
            Some((id, "Rename")) => {
                let name = curves.get(id).map(|c| c.name.clone()).unwrap_or_default();
                self.asking = Some(Ask::Rename(id));
                self.alert.ask_name("Rename curve", &name, "Rename");
            }
            Some((id, "Duplicate")) => {
                curves.duplicate(id);
            }
            Some((id, "Replace")) => curves.replace(id, spec.clone()),
            Some((id, _)) => {
                curves.remove(id);
                self.open = None;
            }
            None => {}
        }
        *y = group.bottom();
        picked
    }

    /// Where the curve was last drawn.
    #[cfg(test)]
    fn curve_rect(&self) -> Rect {
        self.curve_rect.unwrap_or(Rect::NOTHING)
    }
}

/// `name` cut short with an ellipsis to fit `room`.
fn fitted(ui: &Ui, name: &str, room: f32, weight: Weight) -> String {
    let width = |s: &str| crate::look::text_width(ui.painter(), s, 17.0, weight);
    if width(name) <= room {
        return name.to_owned();
    }
    let chars: Vec<char> = name.chars().collect();
    let mut n = chars.len();
    while n > 0 {
        let cut: String = chars.iter().take(n).collect::<String>() + "…";
        if width(&cut) <= room {
            return cut;
        }
        n -= 1;
    }
    "…".into()
}

/// A slider group of `n` sliders: 8 above the first, 11.5 between, 8 below the last.
fn slider_group(n: usize) -> f32 {
    8.0 + 55.0 * n as f32 + 11.5 * (n.saturating_sub(1)) as f32 + 4.0
}

/// The faint 2 pt line between a group's rows, under `y`.
fn row_line(ui: &Ui, y: f32, left: f32, right: f32) {
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, y), pos2(right, y + 2.0)), 0, argb(ROW_LINE));
}

/// A setting's name and value over a bar dragged or clicked along, in `row` (55 tall): where it
/// was set (0..1) while held, and whether it was let go.
fn slider(ui: &Ui, row: Rect, label: &str, value: &str, share: f32, id: usize) -> (Option<f32>, bool) {
    let (left, right) = (row.left() + 16.0, row.right() - 16.0);
    text(ui.painter(), pos2(left, row.top() + 6.5), Align2::LEFT_TOP, label, 17.0, Weight::Regular, argb(SLIDE_LABEL));
    text(
        ui.painter(),
        pos2(right, row.top() + 6.5),
        Align2::RIGHT_TOP,
        value,
        17.0,
        Weight::Regular,
        argb(SLIDE_VALUE),
    );
    let bar = Rect::from_min_max(pos2(left, row.top() + 27.5), pos2(right, row.top() + 49.5));
    let response = ui.interact(bar, ui.scope_id().with(("easing slider", id)), Sense::click_and_drag());
    let y = bar.center().y;
    let x = left + share.clamp(0.0, 1.0) * bar.width();
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(right, y + 1.5)), 0, argb(SLIDE_TRACK));
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(x, y + 1.5)), 0, argb(pc::ACCENT));
    let knob_x = (x - 8.0).clamp(left, right - 16.0);
    ui.painter().rect_filled(Rect::from_min_size(pos2(knob_x, y - 5.0), vec2(16.0, 10.0)), 5, Color32::WHITE);
    let moved = (response.is_pointer_button_down_on() || response.clicked())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|p| ((p.x - left) / bar.width().max(1.0)).clamp(0.0, 1.0));
    (moved, response.drag_stopped() || response.clicked())
}

/// A row with its switch; true when clicked.
fn toggle_row(ui: &Ui, row: Rect, label: &str, on: bool) -> bool {
    let at = pos2(row.left() + 16.0, row.center().y);
    text(ui.painter(), at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(PANEL_TITLE));
    crate::transform::switch(ui, pos2(row.right() - 6.0 - 63.0, row.center().y - 14.0), on, label);
    ui.interact(row, ui.scope_id().with(("easing toggle", label)), Sense::click()).clicked()
}

/// A curve small in `rect`: time across, how far along up.
fn curve_thumb(ui: &Ui, rect: Rect, spec: &EasingSpec) {
    let painter = ui.painter();
    painter.rect_filled(rect, 7, argb(PANEL_TITLE).gamma_multiply(0.08));
    let inner = rect.shrink(3.0);
    let samples = procraft_core::easing_curve(spec.clone(), 32);
    let last = samples.len().saturating_sub(1).max(1) as f32;
    let line: Vec<Pos2> = samples
        .iter()
        .enumerate()
        .map(|(i, v)| {
            pos2(
                inner.left() + inner.width() * i as f32 / last,
                inner.top() + inner.height() * (1.5 - v.clamp(-0.5, 1.5)) / 2.0,
            )
        })
        .collect();
    painter.add(egui::Shape::line(line, Stroke::new(1.8, argb(pc::ACCENT))));
}

#[cfg(test)]
mod tests {
    use egui::{Event, Key, Modifiers};
    use procraft_engine::timeline::{FitMode, RepeatFit};

    use super::*;
    use crate::questions::tests::{Window, press};

    /// The editor on a window, frame after frame, and what it gave.
    struct Shown {
        window: Window,
        editor: EasingEditor,
        easing: Easing,
        curves: SavedCurves,
        /// What it gave, frame by frame since last asked.
        given: Vec<Done>,
    }

    const PANEL: Rect = Rect { min: pos2(100.0, 0.0), max: pos2(100.0 + WIDTH, 800.0) };

    impl Shown {
        fn new(easing: Easing) -> Shown {
            let mut shown = Shown {
                window: Window::new(),
                editor: EasingEditor::default(),
                easing,
                curves: SavedCurves::default(),
                given: Vec::new(),
            };
            // (an area comes in on its second frame)
            shown.frame(Vec::new(), &[]);
            shown.frame(Vec::new(), &[]);
            shown
        }

        /// A frame with `events`; where each of `words` was drawn.
        fn frame(&mut self, events: Vec<Event>, words: &[&str]) -> Vec<Option<Pos2>> {
            let Shown { window, editor, easing, curves, .. } = self;
            let (given, drawn) = window.frame(events, words, |ctx, look| editor.show(ctx, look, PANEL, easing, curves));
            self.given.extend(given);
            drawn
        }

        fn at(&mut self, word: &str) -> Pos2 {
            self.frame(Vec::new(), &[word])[0].unwrap_or_else(|| panic!("{word} isn't drawn"))
        }

        fn click(&mut self, at: Pos2) {
            self.frame(vec![Event::PointerMoved(at)], &[]);
            self.frame(vec![press(at, true)], &[]);
            self.frame(vec![press(at, false)], &[]);
            self.frame(Vec::new(), &[]);
        }

        fn click_word(&mut self, word: &str) {
            let at = self.at(word);
            self.click(at);
        }

        /// Pressed at `from`, dragged to `to` in steps, and let go there.
        fn drag(&mut self, from: Pos2, to: Pos2) {
            self.frame(vec![Event::PointerMoved(from)], &[]);
            self.frame(vec![press(from, true)], &[]);
            for k in 1..=6 {
                let p = from + (to - from) * (k as f32 / 6.0);
                self.frame(vec![Event::PointerMoved(p)], &[]);
            }
            self.frame(vec![press(to, false)], &[]);
            self.frame(Vec::new(), &[]);
        }

        /// The wheel turned over it, as far down as it goes.
        fn scroll_down(&mut self) {
            let over = PANEL.center();
            self.frame(vec![Event::PointerMoved(over)], &[]);
            let wheel = Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: vec2(0.0, -2000.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![wheel], &[]);
            for _ in 0..60 {
                self.frame(Vec::new(), &[]);
            }
        }

        fn taken(&mut self) -> Vec<Done> {
            std::mem::take(&mut self.given)
        }

        /// The alert asking for a name answered with `name` typed over what it offered, and
        /// Return.
        fn name(&mut self, name: &str) {
            self.frame(Vec::new(), &[]);
            std::thread::sleep(std::time::Duration::from_millis(350));
            self.frame(Vec::new(), &[]);
            let rub =
                |key| Event::Key { key, physical_key: None, pressed: true, repeat: false, modifiers: Modifiers::NONE };
            let mut events: Vec<Event> = (0..30).map(|_| rub(Key::Backspace)).collect();
            events.push(Event::Text(name.into()));
            self.frame(events, &[]);
            self.frame(vec![rub(Key::Enter)], &[]);
            // (gone once it has faded)
            std::thread::sleep(std::time::Duration::from_millis(400));
            self.frame(Vec::new(), &[]);
        }

        fn curve_point(&self, p: [f32; 2]) -> Pos2 {
            place(self.editor.curve_rect(), p)
        }
    }

    /// Choosing a kind puts its usual numbers on the key, keeping how it repeats; a slider moves
    /// its number live and is done when let go; Repeats, Loop back and Backwards make it fitted
    /// and reversed.
    #[test]
    fn a_kind_is_chosen_and_its_sliders_and_switches_change_it() {
        let mut shown = Shown::new(Easing::Linear);
        shown.click_word("Bounce");
        assert_eq!(shown.easing, Easing::Bounce { first_step: 0.3, bounciness: 0.8 });
        assert_eq!(shown.taken(), [Done::Set]);
        // Bounciness 0.01..0.99, dragged to its bar's left end
        let label = shown.at("Bounciness");
        let bar_y = label.y + 22.0;
        let value = shown.at("0.80");
        shown.drag(pos2(value.x - 30.0, bar_y), pos2(PANEL.left() + 8.0 + 16.0, bar_y));
        let Easing::Bounce { first_step, bounciness } = shown.easing else { panic!("{:?}", shown.easing) };
        assert_eq!(first_step, 0.3);
        assert!((bounciness - 0.01).abs() < 1e-4, "{bounciness}");
        let given = shown.taken();
        assert!(given.len() >= 2 && given[..given.len() - 1].iter().all(|d| *d == Done::Live), "{given:?}");
        assert_eq!(given.last(), Some(&Done::Set), "let go: done");
        // three times over, back and forth, backwards
        let repeats = shown.at("Repeats");
        shown.click(pos2(PANEL.left() + 8.0 + 16.0 + (WIDTH - 16.0 - 32.0) * 2.0 / 7.0, repeats.y + 22.0));
        shown.click_word("Loop back");
        shown.click_word("Backwards");
        let base = Easing::Bounce { first_step: 0.3, bounciness: shown_bounciness(&shown.easing) };
        let fitted = Easing::Fitted(Box::new(base), RepeatFit { mode: FitMode::Loop, repetitions: 3 });
        assert_eq!(shown.easing, Easing::Reversed(Box::new(fitted)));
        // another kind keeps how it repeats
        shown.click_word("Steps");
        let steps = Easing::Steps { step: 0.25, smoothing: 0.0 };
        let fitted = Easing::Fitted(Box::new(steps), RepeatFit { mode: FitMode::Loop, repetitions: 3 });
        assert_eq!(shown.easing, Easing::Reversed(Box::new(fitted)));
        shown.taken();
        let back = shown.at("Easing") - vec2(60.0, 0.0);
        shown.click(back);
        assert_eq!(shown.taken(), [Done::Back]);
    }

    fn shown_bounciness(e: &Easing) -> f32 {
        match e {
            Easing::Bounce { bounciness, .. } => *bounciness,
            Easing::Reversed(e) | Easing::Fitted(e, _) => shown_bounciness(e),
            _ => f32::NAN,
        }
    }

    fn drawn_points(e: &Easing) -> Vec<[f32; 2]> {
        match e {
            Easing::Curve(p) => p.clone(),
            other => panic!("not a drawn curve: {other:?}"),
        }
    }

    /// Custom's points: one dragged moves between its neighbours, a press away from them adds one,
    /// one let go well below the frame is taken away, and the ends stay.
    #[test]
    fn custom_points_are_dragged_added_and_taken_away() {
        let mut shown = Shown::new(Easing::Linear);
        shown.click_word("Custom");
        assert_eq!(drawn_points(&shown.easing), [[0.0, 0.0], [0.35, 0.28], [0.62, 1.18], [1.0, 1.0]]);
        shown.taken();
        // the first inner point, dragged up and past the next: it stops short of it
        let from = shown.curve_point([0.35, 0.28]);
        let to = shown.curve_point([0.9, 0.5]);
        shown.drag(from, to);
        let pts = drawn_points(&shown.easing);
        assert_eq!(pts.len(), 4);
        assert!((pts[1][0] - (0.62 - APART)).abs() < 1e-3 && (pts[1][1] - 0.5).abs() < 0.02, "{pts:?}");
        assert_eq!((pts[0], pts[3]), ([0.0, 0.0], [1.0, 1.0]), "the ends stay");
        let given = shown.taken();
        assert_eq!(given.last(), Some(&Done::Set));
        assert!(given.contains(&Done::Live));
        // a click away from them adds one there
        let add = shown.curve_point([0.85, 0.2]);
        shown.click(add);
        let pts = drawn_points(&shown.easing);
        assert_eq!(pts.len(), 5, "{pts:?}");
        assert!((pts[3][0] - 0.85).abs() < 0.01 && (pts[3][1] - 0.2).abs() < 0.02, "{pts:?}");
        assert_eq!(shown.taken().last(), Some(&Done::Set));
        // that one, let go well below the frame, is taken away
        let below = shown.editor.curve_rect().bottom() + AWAY + 20.0;
        shown.drag(add, pos2(add.x, below));
        let pts = drawn_points(&shown.easing);
        assert_eq!(pts.len(), 4, "{pts:?}");
        assert!(pts.iter().all(|p| (p[0] - 0.85).abs() > 0.01));
        // a press on an end doesn't move it: a point is added just before it
        let end = shown.curve_point([1.0, 1.0]);
        let before = shown.easing.clone();
        shown.drag(end, end + vec2(-4.0, 40.0));
        assert_eq!(drawn_points(&shown.easing).len(), 5, "a point added just before the end");
        assert_eq!(drawn_points(&shown.easing).last(), Some(&[1.0, 1.0]));
        assert_ne!(shown.easing, before);
    }

    /// Ease's handles are dragged on the curve.
    #[test]
    fn ease_handles_are_dragged_on_the_curve() {
        let mut shown = Shown::new(Easing::Linear);
        shown.click_word("Ease");
        assert_eq!(shown.easing, Easing::CubicBezier([0.42, 0.0], [0.58, 1.0]));
        let from = shown.curve_point([0.58, 1.0]);
        let to = shown.curve_point([0.2, 1.3]);
        shown.drag(from, to);
        let Easing::CubicBezier(a, b) = shown.easing else { panic!() };
        assert_eq!(a, [0.42, 0.0]);
        assert!((b[0] - 0.2).abs() < 0.01 && (b[1] - 1.3).abs() < 0.02, "{b:?}");
        assert_eq!(shown.taken().last(), Some(&Done::Set));
        // a press away from both handles does nothing
        let before = shown.easing.clone();
        let away = shown.curve_point([0.9, -0.3]);
        shown.drag(away, away + vec2(10.0, 10.0));
        assert_eq!(shown.easing, before);
        assert!(shown.taken().is_empty());
    }

    /// The easing showing is saved to My curves by name, renamed from its ⋯, duplicated, replaced
    /// with what is showing, put back on the key with a click, and deleted.
    #[test]
    fn my_curves_are_saved_renamed_and_applied() {
        let mut shown = Shown::new(Easing::Linear);
        shown.click_word("Elastic");
        let elastic = shown.easing.clone();
        // taller than the window: scrolled to My curves
        shown.scroll_down();
        assert!(shown.editor.scroll > 0.0, "scrolled");
        shown.click_word("Save this curve");
        shown.name("Wobbly");
        assert_eq!(shown.curves.all().len(), 1);
        assert_eq!(shown.curves.all()[0].name, "Wobbly");
        assert_eq!(shown.curves.all()[0].easing(), elastic);
        let id = shown.curves.all()[0].id;
        // renamed from its ⋯
        let row = shown.at("Wobbly");
        shown.click(pos2(PANEL.right() - 8.0 - 22.0, row.y));
        shown.click_word("Rename");
        shown.name("Springy");
        assert_eq!(shown.curves.get(id).map(|c| c.name.as_str()), Some("Springy"));
        // duplicated, the copy just after
        shown.click_word("Duplicate");
        assert_eq!(shown.curves.all().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Springy", "Springy copy"]);
        // another easing, then the first curve put on the key
        shown.click_word("Hold");
        assert_eq!(shown.easing, Easing::Hold);
        shown.taken();
        shown.click_word("Springy");
        assert_eq!(shown.easing, elastic);
        assert_eq!(shown.taken(), [Done::Set]);
        // Hold kept in the copy's place (its ⋯ still open), then the copy deleted
        shown.click_word("Hold");
        let copy = shown.at("Springy copy");
        shown.click(pos2(PANEL.right() - 8.0 - 22.0, copy.y));
        shown.click_word("Replace");
        assert_eq!(shown.curves.all()[1].easing(), Easing::Hold);
        assert_eq!(shown.curves.all()[1].name, "Springy copy");
        shown.click_word("Delete");
        assert_eq!(shown.curves.all().iter().map(|c| c.name.as_str()).collect::<Vec<_>>(), ["Springy"]);
        // the key's Easing menu offers it
        let choices = crate::saved_curves::easing_choices(&shown.curves, &elastic);
        assert_eq!(choices.len(), 1);
        assert!(choices[0].chosen && choices[0].easing == elastic);
    }
}

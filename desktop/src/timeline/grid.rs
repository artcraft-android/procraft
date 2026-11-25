//! The grid's bar (see the module's own words): its sizes and colours, as the iPad's `Grid` and
//! `GridSizes` at the size the grid starts at; drawing it; and what a press on it does. One press
//! on the frames is told apart as it goes, as the iPad's one drag gesture does
//! (ios/ProCraft/TimelineGrid.swift): a tap, a hold (its menu; dragged on from there, the block
//! moves), a drag sideways (scrub), up or down (more tracks than show), a lasso's stroke, a
//! handle. A right-click is a hold that doesn't move anything.

use std::collections::{BTreeSet, HashSet};
use std::sync::Arc;

use egui::{Align2, Color32, CornerRadius, Id, Order, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use procraft_core::{CanvasSession, TimelineClipInfo, TimelineClipKind, TimelinePanel, TimelineTrackInfo};

use super::menus::{CARD_WIDTH, Kind};
use super::rows::{GridRow, GridRows};
use super::{PAST_END, TimelineGrid, grid_icons};
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, cubic_bezier, pc, text, text_width};
use crate::prefs::Prefs;

/// From one frame's cell to the next, a cell's own size, a row's height, a cell's corners.
pub(super) const PITCH: f32 = 44.0;
pub(super) const CELL_WIDTH: f32 = 40.0;
pub(super) const CELL_HEIGHT: f32 = 31.0;
pub(super) const ROW_HEIGHT: f32 = 44.0;
const CORNER: u8 = 7;
pub(super) const GAP: f32 = PITCH - CELL_WIDTH;
/// The ruler's and the header's heights, the space under the rows, and the bar's gap from the
/// window's bottom edge and its corners.
pub(super) const RULER_HEIGHT: f32 = 26.0;
pub(super) const HEADER_HEIGHT: f32 = 46.0;
const UNDER_ROWS: f32 = 9.0;
const BAR_GAP: f32 = 19.0;
const BAR_CORNER: u8 = 28;
/// How far over the bar a menu stands.
pub(super) const BAR_GAP_ABOVE: f32 = 8.0;
/// The widest the bar gets.
const MOST_WIDTH: f32 = 1180.0;
/// How far in from the bar's left edge the cells show whole: before it they fade out under the
/// tracks' names, and presses there are the names'.
pub(super) const LABELS_WIDTH: f32 = 176.0;
/// How far in from the bar's ends the frames show (its rounded corners stand there).
const ROWS_INSET: f32 = 8.0;
/// How far a name steps in for each group its track is inside.
const INDENT: f32 = 14.0;
/// A frame tapped or stepped to slides under the playhead in this long (seconds).
const SLIDE: f64 = 0.35;
/// How long a header button is held for what holding it does, `+` for Add new, a frame for its
/// menu, a track's name for its (seconds).
const HOLD: f64 = 0.4;
const ADD_HOLD: f64 = 0.2;
const FRAME_HOLD: f64 = 0.4;
const NAME_HOLD: f64 = 0.35;
/// How far a press goes before it is a drag, not a tap or a hold; how far a hold goes before the
/// block held is picked up.
const TAP_SLOP: f32 = 6.0;
const PICK_UP: f32 = 8.0;
/// How long a tapped header button stays lit, and a lasso's stroke takes to fade (seconds).
const FLASH: f64 = 0.18;
const FADE: f64 = 0.35;

const PANEL: Themed = Themed(0xFF212121, 0xFFE4E4E4);
const SLOT: Themed = Themed(0xFF161616, 0xFFD1D1D1);
const LINE: Themed = Themed(0xFF474747, 0xFFC0C0C0);
const CAPSULE: Themed = Themed(0xFF353535, 0xFFCBCBCB);
const INK: Themed = Themed(0xFFC9C9C9, 0xFF393939);
const INK_BRIGHT: Themed = Themed(0xFFF2F2F2, 0xFF1C1C1C);
const INK_DIM: Themed = Themed(0xFF757575, 0xFF8C8C8C);
const BOX: Themed = Themed(0xFF868686, 0xFF8A8A8A);
/// A clip that isn't a drawing (a group, a sound, a video).
const BLOCK: Themed = Themed(0xFF4E4E4E, 0xFFB2B2B2);
/// Laid over a drawing's held frames, so they read as the same drawing going on.
const HELD_SHADE: u32 = 0x99404040;
/// What is written on a track's highlight colour.
const ON_LABEL: u32 = 0xE6000000;

/// A button of the header.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Button {
    Play,
    Back,
    On,
    Settings,
    Lasso,
    Track,
    Add,
}

/// What a press on the frames (or the ruler) is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
enum Touch {
    #[default]
    None,
    /// On the frames, not yet told apart: a tap, a hold, or the start of a drag.
    Undecided,
    /// On the ruler, not yet dragged: a tap goes to the frame.
    Ruler,
    /// On a track's name: a tap chooses it, a hold opens its menu.
    Name,
    /// On the end flap: dragged, the timeline's length; tapped, its menu.
    Flap { from: u32 },
    /// Dragged sideways: the frames slide under the playhead, from frame `from` there.
    Scroll { from: f32 },
    /// Dragged up or down where there are more tracks than show, from `from` scrolled.
    Pan { from: f32 },
    /// With the lasso on: a stroke picking the frames it crosses.
    Stroke,
    /// On a frame's handle (its start's, or its end's): dragged, it lasts longer or shorter.
    Handle { leading: bool },
    /// Held still: a menu opened.
    Held,
    /// Dragged on from a hold on a block: the block goes with the pointer.
    Move,
    /// Nothing of the frames' (it stopped them playing, or closed a menu).
    Ignore,
}

/// How the frames are moving under the playhead, and the presses on the bar under way.
#[derive(Default)]
pub(super) struct Motion {
    /// The frame under the playhead as shown (between frames while dragged or sliding).
    at: f32,
    /// The frame it last came to rest on; a frame sliding to the playhead: from, to, since.
    focus: Option<f32>,
    slide: Option<(f32, f32, f64)>,
    touch: Touch,
    /// Where and when the press under way came down; the block it held, if it held one.
    origin: Pos2,
    since: f64,
    held_clip: Option<u64>,
    /// The press began with a menu open: a tap only closes it.
    closing: bool,
    /// The frame last shown while scrubbing.
    scrubbed: Option<u32>,
    /// How far the tracks are scrolled up, where there are more of them than show; and the track
    /// last kept in sight.
    rows_offset: f32,
    revealed: Option<u64>,
    /// A header button held: which, since when, and whether its hold has acted.
    press: Option<(Button, f64, bool)>,
    flash: Option<(Button, f64)>,
}

impl Motion {
    /// Everything under way let go: the frames rest where the playhead is.
    pub(super) fn let_go(&mut self) {
        self.focus = None;
        self.slide = None;
        self.touch = Touch::None;
        self.scrubbed = None;
        self.press = None;
        self.held_clip = None;
    }

    /// Playing stopped, the grid having held still at frame `held`: the playhead's frame slides
    /// to it from there.
    pub(super) fn stopped_playing(&mut self, held: f32) {
        self.at = held;
        self.focus = Some(held);
        self.slide = None;
    }

    /// The frame under the playhead at `now`: `target` (the playhead's), slid to from where it
    /// was when it changed.
    fn settle(&mut self, target: f32, now: f64) -> bool {
        if self.focus != Some(target) {
            match self.focus {
                None => self.at = target,
                Some(_) => self.slide = Some((self.at, target, now)),
            }
            self.focus = Some(target);
        }
        let Some((from, to, since)) = self.slide else { return false };
        let k = (((now - since) / SLIDE) as f32).clamp(0.0, 1.0);
        self.at = from + (to - from) * cubic_bezier(0.2, 0.8, 0.5, 1.0, k);
        if k >= 1.0 {
            self.slide = None;
        }
        k < 1.0
    }
}

/// The lasso's stroke under way (window points), and the last one fading out; the frames it has
/// crossed, and what was picked as it began (a second press puts that back).
#[derive(Default)]
pub(super) struct Lasso {
    pub points: Vec<Pos2>,
    faded: Vec<Pos2>,
    faded_at: f64,
    touched: HashSet<u64>,
    before: Option<BTreeSet<u64>>,
}

/// Where everything stands on the bar.
pub(super) struct Place {
    pub bar: Rect,
    header: Rect,
    pub ruler: Rect,
    /// The rows' area as it shows, and the top of the first row (scrolled).
    pub rows_rect: Rect,
    pub top: f32,
    /// The playhead's cell's middle, and frame 0's cell's left edge.
    playhead: f32,
    pub origin: f32,
    /// The last frame there is a cell for.
    pub last: u32,
    /// How far the rows can scroll.
    over: f32,
    pub rows: Arc<GridRows>,
}

impl Place {
    /// Frame `f`'s cell's left edge.
    fn x(&self, f: f32) -> f32 {
        self.origin + f * PITCH
    }

    /// The frame whose cell (and the gap after it) `x` is over, if there is one.
    fn frame_at(&self, x: f32) -> Option<u32> {
        let f = ((x - self.origin) / PITCH).floor();
        (f >= 0.0 && f <= self.last as f32).then_some(f as u32)
    }

    /// The row (top first) at height `y`.
    fn row_at(&self, y: f32) -> Option<usize> {
        if !self.rows_rect.y_range().contains(y) {
            return None;
        }
        self.rows.index_at(y - self.top)
    }

    /// Row `r`'s top edge.
    pub fn row_top(&self, r: usize) -> f32 {
        self.top + self.rows.get(r).map_or(r as f32 * ROW_HEIGHT, |row| row.y)
    }

    /// The top of a cell on row `r`.
    pub fn cell_top(&self, r: usize) -> f32 {
        let height = self.rows.get(r).map_or(ROW_HEIGHT, |row| row.height);
        self.row_top(r) + (height - CELL_HEIGHT) / 2.0
    }

    /// The row and frame of the cell at `p`, if there is one.
    fn cell(&self, p: Pos2) -> Option<(usize, u32)> {
        Some((self.row_at(p.y)?, self.frame_at(p.x)?))
    }

    /// Whether `p` is over the tracks' names.
    fn on_names(&self, p: Pos2) -> bool {
        self.rows_rect.contains(p) && p.x < self.bar.left() + LABELS_WIDTH - 12.0
    }

    /// Where the end flap stands for a timeline `frames` long: its pole's x.
    fn flap_x(&self, frames: u32) -> f32 {
        self.x(frames as f32) - GAP / 2.0
    }

    /// The timeline's frames whose cells show whole right of the tracks' names with frame `at`
    /// under the playhead, of a timeline `frames` long: first, and one past the last.
    fn shown(&self, at: f32, frames: u32) -> Option<(u32, u32)> {
        let origin = self.playhead - CELL_WIDTH / 2.0 - at * PITCH;
        let first = ((self.bar.left() + LABELS_WIDTH - origin) / PITCH).ceil().max(0.0);
        let last = ((self.bar.right() - origin - CELL_WIDTH) / PITCH).floor().min(frames as f32 - 1.0);
        (first <= last).then(|| (first as u32, last as u32 + 1))
    }
}

/// `c` made `alpha` as strong.
fn faded(c: Color32, alpha: f32) -> Color32 {
    if alpha >= 1.0 { c } else { c.gamma_multiply(alpha) }
}

/// A colour label (0xRRGGBB) as a colour.
pub(super) fn label_colour(rgb: u32) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

impl TimelineGrid {
    /// Shows the grid at the bottom of the window while the artwork is on a timeline; returns what
    /// to tell (and whether it is a warning).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        prefs: &Prefs,
    ) -> Option<(String, bool)> {
        if !self.read(ctx, session) {
            return self.said.take();
        }
        self.play_on(ctx, session);
        // the handles stay with the frame they are on: gone when the playhead leaves it
        if let Some((_, c)) = self.picked.and_then(|p| self.clip(p))
            && self.edit.trim.is_none()
        {
            let at = self.frame();
            if at < c.start || at >= c.start + c.duration {
                self.picked = None;
            }
        }
        if self.selecting.is_none() && matches!(self.menu.as_ref().map(|m| m.kind), Some(Kind::Picked)) {
            self.close_menu();
        }
        let place = self.place(ctx);
        let now = ctx.input(|i| i.time);
        // (what stands at the window's foot stands over it)
        let lift = ctx.content_rect().bottom() - place.bar.top();
        let frame = ctx.cumulative_frame_nr();
        ctx.data_mut(|d| d.insert_temp(Id::unique("timeline grid lift"), (lift, frame)));
        egui::Area::new(Id::unique("timeline grid")).fixed_pos(place.bar.min).order(Order::Middle).show(ctx, |ui| {
            // the whole bar takes its presses: none reach the canvas under it
            ui.allocate_rect(place.bar, Sense::click_and_drag());
            crate::screen::claim(ui, place.bar);
            // the bar's ground, under the header's buttons drawn next
            ui.painter().add(
                egui::epaint::Shadow { offset: [0, 6], blur: 16, spread: 0, color: Color32::from_black_alpha(77) }
                    .as_shape(place.bar, CornerRadius::same(BAR_CORNER)),
            );
            ui.painter().rect_filled(place.bar, BAR_CORNER, argb(PANEL));
            if let Some(said) = self.header(ui, look, session, &place, now) {
                self.tell(said, true);
            }
            self.touches(ui, session, &place, now);
            // where the frames stand, now that the presses have moved them
            let moving = match (self.playing.as_ref(), self.motion.touch) {
                (Some(p), _) => {
                    self.motion.at = p.held;
                    false
                }
                (None, Touch::Scroll { .. } | Touch::Move) => false,
                (None, _) => {
                    let target = self.info().map_or(0.0, |i| i.frame as f32);
                    self.motion.settle(target, now)
                }
            };
            if moving {
                ui.ctx().request_repaint();
            }
            self.reveal(&place);
            let place = self.place(ui.ctx());
            self.draw(ui, look, session, &place, prefs, now);
        });
        let place = self.place(ctx);
        self.show_menu(ctx, look, session, &place);
        self.said.take()
    }

    /// Where the bar and everything on it stand in the window, the frames as they are now.
    fn place(&self, ctx: &egui::Context) -> Place {
        let screen = ctx.content_rect();
        let width = (screen.width() - 32.0).clamp(LABELS_WIDTH * 2.0, MOST_WIDTH);
        let all = self.rows.height.max(ROW_HEIGHT);
        let most = (screen.height() * 0.5 - HEADER_HEIGHT - RULER_HEIGHT - 12.0).floor().max(ROW_HEIGHT);
        let shown = all.min(most);
        let height = HEADER_HEIGHT + 1.0 + RULER_HEIGHT + shown + UNDER_ROWS;
        let bar = Rect::from_min_size(
            pos2(screen.center().x - width / 2.0, screen.bottom() - BAR_GAP - height),
            vec2(width, height),
        );
        let header = Rect::from_min_size(bar.min, vec2(width, HEADER_HEIGHT));
        let ruler = Rect::from_min_size(pos2(bar.left(), header.bottom() + 1.0), vec2(width, RULER_HEIGHT));
        let rows_rect = Rect::from_min_size(pos2(bar.left(), ruler.bottom()), vec2(width, shown));
        let over = (all - shown).max(0.0);
        let scroll = self.motion.rows_offset.clamp(0.0, over);
        let playhead = (bar.left() + width / 2.0).round();
        let duration = self.info().map_or(1, |i| i.duration);
        Place {
            bar,
            header,
            ruler,
            rows_rect,
            top: rows_rect.top() - scroll,
            playhead,
            origin: playhead - CELL_WIDTH / 2.0 - self.motion.at * PITCH,
            last: duration.saturating_sub(1) + PAST_END,
            over,
            rows: Arc::clone(&self.rows),
        }
    }

    /// Where not every track shows, the rows scrolled as little as brings the chosen track's
    /// wholly into sight, once each time another is chosen.
    fn reveal(&mut self, place: &Place) {
        let track = self.info().and_then(|i| i.track);
        if track == self.motion.revealed {
            return;
        }
        self.motion.revealed = track;
        let Some(r) = self.rows.index_of(track) else { return };
        let Some(row) = self.rows.get(r) else { return };
        let now = self.motion.rows_offset.clamp(0.0, place.over);
        let to = now.min(row.y).max(row.y + row.height - place.rows_rect.height());
        self.motion.rows_offset = to.clamp(0.0, place.over);
    }

    /// The frame under the playhead as shown.
    pub(super) fn motion_at(&self) -> f32 {
        self.motion.at
    }

    /// The frames slid `by` frames under the playhead (a block dragged near an end of the bar).
    pub(super) fn slide_frames(&mut self, by: f32, last: f32) {
        self.motion.slide = None;
        self.motion.at = (self.motion.at + by).clamp(0.0, last);
    }

    /// The frames come to rest with frame `at` under the playhead, where they are.
    pub(super) fn rest_frames(&mut self, at: f32) {
        self.motion.at = at;
        self.motion.slide = None;
    }

    // MARK: The header

    /// The header: Play, a frame back and on; then the settings, the lasso, a new track and a new
    /// frame.
    fn header(
        &mut self,
        ui: &Ui,
        look: &mut Look,
        session: &CanvasSession,
        place: &Place,
        now: f64,
    ) -> Option<&'static str> {
        let y = place.header.top() + 4.0 + 20.0;
        let ink = argb(INK);
        let painter = ui.painter();
        let word = if self.playing.is_some() { "Pause" } else { "Play" };
        let buttons = header_buttons(place, text_width(painter, word, 17.0, Weight::Bold));
        let [(_, play), (_, back), (_, on), (_, settings), (_, lasso), (_, track), (_, add)] = buttons;
        text(painter, pos2(play.left(), y), Align2::LEFT_CENTER, word, 17.0, Weight::Bold, ink);
        let lit = |b: Button| self.motion.flash.is_some_and(|(f, at)| f == b && now - at < FLASH);
        let tint = |b: Button| if lit(b) { argb(pc::ACCENT) } else { ink };
        look.icon(ui, &icons::CHEVRON_LEFT, back.center(), 21.0, tint(Button::Back));
        look.icon(ui, &icons::CHEVRON_RIGHT, on.center(), 21.0, tint(Button::On));
        settings_glyph(painter, settings.center(), tint(Button::Settings));
        self.settings_button = Some(settings);
        let lasso_ink = if self.selecting.is_some() { argb(pc::ACCENT) } else { tint(Button::Lasso) };
        look.icon(ui, &grid_icons::GRID_LASSO, lasso.center(), 27.0, lasso_ink);
        track_glyph(painter, track.center(), tint(Button::Track));
        let adding = matches!(self.menu.as_ref().map(|m| m.kind), Some(Kind::Add));
        look.icon(ui, &icons::ADD, add.center(), 38.0, if adding { argb(pc::ACCENT) } else { tint(Button::Add) });
        let mut said = None;
        for (button, rect) in buttons {
            let response = ui.interact(rect, ui.scope_id().with(("timeline header", button as u8)), Sense::click());
            // held: ‹ and › go to the drawing before or after; `+` opens Add new
            if response.is_pointer_button_down_on() {
                let held = match self.motion.press {
                    Some((b, since, acted)) if b == button => (since, acted),
                    _ => {
                        self.motion.press = Some((button, now, false));
                        (now, false)
                    }
                };
                let hold = if button == Button::Add { ADD_HOLD } else { HOLD };
                if matches!(button, Button::Back | Button::On | Button::Add) && !held.1 {
                    if now - held.0 >= hold {
                        self.motion.press = Some((button, held.0, true));
                        self.close_menu();
                        self.picked = None;
                        if button == Button::Add {
                            self.open_menu(Kind::Add, rect.center().x);
                        } else {
                            self.step_drawing(session, if button == Button::On { 1 } else { -1 });
                        }
                        self.read_at = None;
                    } else {
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(hold - (now - held.0)));
                    }
                }
            }
            let acted = self.motion.press.is_some_and(|(b, _, acted)| b == button && acted);
            if response.clicked() && !acted {
                self.motion.flash = Some((button, now));
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(FLASH));
                said = self.pressed(ui.ctx(), session, button, place).or(said);
                self.read_at = None;
            }
        }
        if !ui.input(|i| i.pointer.any_down()) {
            self.motion.press = None;
        }
        said
    }

    /// Header button `button` tapped.
    fn pressed(
        &mut self,
        ctx: &egui::Context,
        session: &CanvasSession,
        button: Button,
        place: &Place,
    ) -> Option<&'static str> {
        self.close_menu();
        if button != Button::Lasso && button != Button::Settings {
            self.picked = None;
        }
        match button {
            Button::Play => {
                if self.playing.is_some() {
                    self.stop(session, true);
                } else {
                    self.play_shown(ctx, session, place);
                }
            }
            Button::Back => self.step_frame(session, -1),
            Button::On => self.step_frame(session, 1),
            Button::Settings => {
                self.stop(session, false);
                self.settings_asked = true;
            }
            Button::Lasso => {
                // on, the picked frame first; off, what was picked let go
                if self.selecting.is_none() {
                    self.stop(session, false);
                    self.selecting = Some(self.picked.take().into_iter().collect());
                } else {
                    self.selecting = None;
                    self.edit.group_trim = None;
                }
            }
            Button::Track => self.add_track(session),
            Button::Add => return self.add_frame(session),
        }
        None
    }

    /// Plays the frames the grid shows with the playhead's under it (none of the timeline's in
    /// sight, the playhead resting past its end: those showing from its first frame).
    fn play_shown(&mut self, ctx: &egui::Context, session: &CanvasSession, place: &Place) {
        let Some(info) = self.info() else { return };
        let (duration, here) = (info.duration, info.frame);
        let mut held = here as f32;
        let mut shown = place.shown(held, duration);
        if shown.is_none() {
            session.rest_timeline_frame(0);
            self.read(ctx, session);
            held = 0.0;
            shown = place.shown(0.0, duration);
        }
        let (first, end) = shown.unwrap_or((0, duration));
        self.motion.focus = Some(held);
        self.motion.slide = None;
        self.play(ctx, session, first, end, held);
    }

    // MARK: Presses on the frames

    /// A press on the ruler or the rows, told apart as it goes (see the file's own words).
    fn touches(&mut self, ui: &Ui, session: &CanvasSession, place: &Place, now: f64) {
        let area = Rect::from_min_max(place.ruler.min, place.rows_rect.max);
        let (pressed, secondary, down, released, at, dt) = ui.input(|i| {
            (
                i.pointer.primary_pressed(),
                i.pointer.secondary_pressed(),
                i.pointer.primary_down(),
                i.pointer.primary_released(),
                i.pointer.interact_pos(),
                i.stable_dt.clamp(0.0, 0.1),
            )
        });
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape));
        // a second press, or Escape, drops the stroke under way and what it picked
        if self.motion.touch == Touch::Stroke && (secondary || escape) {
            if let Some(before) = self.stroke.before.take() {
                self.selecting = Some(before);
            }
            self.end_stroke(now);
            self.motion.touch = Touch::Ignore;
        }
        let Some(at) = at else { return };
        if secondary && area.contains(at) && !self.on_cards(at) && self.motion.touch == Touch::None {
            self.right_clicked(session, place, at);
            return;
        }
        if pressed && area.contains(at) && !self.on_cards(at) {
            self.began(session, place, at, now);
        }
        if self.motion.touch == Touch::None {
            return;
        }
        if down || pressed {
            self.dragging(ui, session, place, at, now, dt);
        }
        if released || !down {
            self.lifted(ui, session, place, now);
            self.motion.touch = Touch::None;
            self.motion.held_clip = None;
        }
    }

    /// A press coming down at `at`.
    fn began(&mut self, session: &CanvasSession, place: &Place, at: Pos2, now: f64) {
        self.motion.origin = at;
        self.motion.since = now;
        self.motion.held_clip = None;
        self.motion.closing = self.menu.is_some();
        self.motion.touch = if place.on_names(at) {
            Touch::Name
        } else if self.playing.is_some() {
            // a press on the frames while they play stops them there, and does nothing more
            self.stop(session, true);
            self.read_at = None;
            Touch::Ignore
        } else if place.ruler.contains(at) {
            let duration = self.info().map_or(1, |i| i.duration);
            let flap = place.flap_x(duration);
            if at.x >= flap - 8.0 && at.x <= flap + 22.0 { Touch::Flap { from: duration } } else { Touch::Ruler }
        } else if let Some(leading) = self.handle_at(place, at) {
            Touch::Handle { leading }
        } else {
            Touch::Undecided
        };
    }

    /// The press under way at `at`.
    fn dragging(&mut self, ui: &Ui, session: &CanvasSession, place: &Place, at: Pos2, now: f64, dt: f32) {
        let moved = at - self.motion.origin;
        let far = moved.length();
        let held_for = now - self.motion.since;
        let wait = |hold: f64| {
            if held_for < hold {
                ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(hold - held_for));
            }
        };
        let touch = self.motion.touch;
        match touch {
            Touch::Undecided if far > TAP_SLOP => {
                self.close_menu();
                if self.selecting.is_some() {
                    self.motion.touch = Touch::Stroke;
                    self.stroke = Lasso { before: self.selecting.clone(), ..Lasso::default() };
                    self.stroke.points.push(self.motion.origin);
                    self.take(place, self.motion.origin);
                    self.stroked(place, at);
                } else if moved.x.abs() >= moved.y.abs() || place.over <= 0.0 {
                    self.start_scroll(session, place, moved.x);
                } else {
                    self.motion.touch = Touch::Pan { from: self.motion.rows_offset.clamp(0.0, place.over) };
                }
            }
            Touch::Undecided => {
                if held_for >= FRAME_HOLD {
                    self.motion.touch = Touch::Held;
                    self.held_at(place, self.motion.origin);
                } else {
                    wait(FRAME_HOLD);
                }
            }
            Touch::Ruler if far > TAP_SLOP => {
                self.close_menu();
                self.start_scroll(session, place, moved.x);
            }
            Touch::Name if far <= TAP_SLOP => {
                if held_for >= NAME_HOLD {
                    self.motion.touch = Touch::Ignore;
                    if let Some(t) = self.name_at(place, self.motion.origin) {
                        self.open_menu(Kind::Track { track: t.id }, place.bar.left() + 14.0 + CARD_WIDTH / 2.0);
                    }
                } else {
                    wait(NAME_HOLD);
                }
            }
            // (dragged off it: nothing)
            Touch::Name => self.motion.touch = Touch::Ignore,
            Touch::Flap { from } if far >= 4.0 || self.length_draft.is_some() => {
                if self.length_draft.is_none() {
                    self.stop(session, false);
                    self.close_menu();
                }
                let next = (i64::from(from) + (moved.x / PITCH).round() as i64).max(1);
                self.length_draft = Some(u32::try_from(next).unwrap_or(1));
            }
            Touch::Scroll { from } => self.scrolled(session, place, from, moved.x),
            Touch::Pan { from } => self.motion.rows_offset = (from - moved.y).clamp(0.0, place.over),
            Touch::Stroke => self.stroked(place, at),
            Touch::Handle { leading } => {
                // (a tap on a handle does nothing)
                if self.edit.trim.is_some() || self.edit.group_trim.is_some() || moved.x.abs() > 1.0 {
                    self.edge_dragged(session, leading, moved.x);
                }
            }
            // dragged on from the hold: the block held is picked up, and its menu goes
            Touch::Held if far > PICK_UP => {
                if self.move_began(self.motion.held_clip) {
                    self.motion.touch = Touch::Move;
                    self.moved(session, place, at, self.motion.origin, dt);
                }
            }
            Touch::Move if self.moved(session, place, at, self.motion.origin, dt) => ui.ctx().request_repaint(),
            _ => {}
        }
    }

    /// The press let go.
    fn lifted(&mut self, ui: &Ui, session: &CanvasSession, place: &Place, now: f64) {
        let closing = self.motion.closing;
        match self.motion.touch {
            Touch::Undecided => {
                if closing {
                    self.close_menu();
                } else {
                    self.tapped(session, place, self.motion.origin);
                }
            }
            Touch::Ruler => {
                self.close_menu();
                if !closing
                    && let Some(f) = place.frame_at(self.motion.origin.x)
                    && f != self.frame()
                {
                    self.picked = None;
                    self.go_to(session, f);
                }
            }
            Touch::Name => {
                self.close_menu();
                if !closing {
                    self.name_tapped(session, place, self.motion.origin);
                }
            }
            Touch::Flap { .. } => match self.length_draft.take() {
                Some(next) => {
                    if self.info().is_some_and(|i| i.duration != next) {
                        self.set_length(session, next);
                    }
                }
                None => {
                    let open = !matches!(self.menu.as_ref().map(|m| m.kind), Some(Kind::Flap));
                    self.close_menu();
                    if open {
                        let duration = self.info().map_or(1, |i| i.duration);
                        self.open_menu(Kind::Flap, place.flap_x(duration));
                    }
                }
            },
            Touch::Scroll { .. } => {
                // carried on as far as it was going, then settling on a frame, which is drawn on
                let speed = ui.input(|i| i.pointer.velocity().x);
                let to = (self.motion.at - speed / 4.2 / PITCH).clamp(0.0, place.last as f32).round();
                session.set_timeline_playing(None);
                session.rest_timeline_frame(to as u32);
                self.motion.slide = Some((self.motion.at, to, now));
                self.motion.focus = Some(to);
                self.motion.scrubbed = None;
            }
            Touch::Stroke => self.end_stroke(now),
            Touch::Handle { .. } => self.edge_ended(session),
            Touch::Move => self.move_ended(session),
            _ => {}
        }
        self.read_at = None;
    }

    /// A drag sideways begun: the frames scrub under the playhead.
    fn start_scroll(&mut self, session: &CanvasSession, place: &Place, dx: f32) {
        self.picked = None;
        self.motion.slide = None;
        self.motion.scrubbed = Some(self.frame());
        let from = self.motion.at;
        self.motion.touch = Touch::Scroll { from };
        self.scrolled(session, place, from, dx);
    }

    /// The frames dragged `dx` sideways from frame `from` under the playhead: the frame now under
    /// it shown as it plays.
    fn scrolled(&mut self, session: &CanvasSession, place: &Place, from: f32, dx: f32) {
        let at = (from - dx / PITCH).clamp(0.0, place.last as f32);
        self.motion.at = at;
        let frame = at.round() as u32;
        if self.motion.scrubbed != Some(frame) {
            self.motion.scrubbed = Some(frame);
            session.set_timeline_playing(Some(frame));
        }
    }

    /// The track whose name is at `p`.
    fn name_at(&self, place: &Place, p: Pos2) -> Option<TimelineTrackInfo> {
        let row = self.rows.get(place.row_at(p.y)?)?;
        row.track(self.frame()).cloned()
    }

    /// A tap on a track's name: it is chosen (its box: hidden or shown).
    fn name_tapped(&mut self, session: &CanvasSession, place: &Place, at: Pos2) {
        let Some(r) = place.row_at(at.y) else { return };
        let Some(t) = self.name_at(place, at) else { return };
        let depth = self.rows.get(r).map_or(0, |row| row.depth);
        let check = place.bar.left() + 12.0 + depth as f32 * INDENT + 6.0 + 26.0;
        if at.x < check {
            session.set_timeline_track(t.id, !t.hidden, t.locked);
        } else {
            self.picked = None;
            self.select_track(session, t.id);
        }
    }

    /// The clip at `p`, with its row's track.
    fn clip_at(&self, place: &Place, p: Pos2) -> Option<(TimelineTrackInfo, TimelineClipInfo)> {
        let (r, f) = place.cell(p)?;
        let row = self.rows.get(r).filter(|row| row.has(f))?;
        let t = row.track(f)?;
        let c = t.clips.iter().find(|c| c.start <= f && f < c.start + c.duration)?;
        Some((t.clone(), c.clone()))
    }

    /// A tap at `at`: with the lasso on, the frame there is picked with the others or let go; else
    /// the playhead goes to it on its track, or, there already, the frame's handles come out or
    /// go. A group's chevron expands it or puts its tracks away.
    fn tapped(&mut self, session: &CanvasSession, place: &Place, at: Pos2) {
        let Some((r, f)) = place.cell(at) else {
            self.picked = None;
            return;
        };
        let Some(row) = self.rows.get(r).filter(|row| row.has(f)) else {
            self.picked = None;
            return;
        };
        let Some(t) = row.track(f).cloned() else { return };
        let found = self.clip_at(place, at).map(|(_, c)| c);
        if let Some(c) = found.as_ref().filter(|c| c.kind == TimelineClipKind::Group && self.selecting.is_none())
            && at.x - place.x(c.start as f32) < 30.0
        {
            let open = self.rows.unfolded(c.id);
            self.fold(session, c.id, open);
            return;
        }
        if let Some(chosen) = self.selecting.as_mut() {
            if let Some(c) = found
                && !chosen.remove(&c.id)
            {
                chosen.insert(c.id);
            }
            return;
        }
        let chosen = self.info().and_then(|i| i.track);
        if chosen == Some(t.id) && f == self.frame() {
            self.picked = match found {
                Some(c) if !self.held(&t) && self.picked != Some(c.id) => Some(c.id),
                _ => None,
            };
        } else {
            self.picked = None;
            self.select_track(session, t.id);
            if f != self.frame() {
                self.go_to(session, f);
            }
        }
    }

    /// A press held still at `at` (or right-clicked): the menu of what is there, standing on the
    /// bar over it; with the lasso on, the picked frames' menu when it is one of them.
    fn held_at(&mut self, place: &Place, at: Pos2) {
        if self.playing.is_some() {
            return;
        }
        let Some((_, c)) = self.clip_at(place, at) else {
            // an empty frame: its own menu
            if self.selecting.is_none()
                && let Some((r, f)) = place.cell(at)
                && let Some(row) = self.rows.get(r).filter(|row| row.has(f))
                && let Some(t) = row.track(f)
            {
                let track = t.id;
                self.open_menu(Kind::Empty { track, frame: f }, at.x);
            }
            return;
        };
        self.motion.held_clip = Some(c.id);
        match self.selecting.as_ref() {
            Some(chosen) if chosen.contains(&c.id) => self.open_menu(Kind::Picked, at.x),
            Some(_) => {}
            None => {
                let kind = if c.kind == TimelineClipKind::Drawing {
                    Kind::Frame { clip: c.id }
                } else {
                    Kind::Block { clip: c.id }
                };
                self.open_menu(kind, at.x);
            }
        }
    }

    /// A right-click at `at`: the menu a hold there opens (a track's name's, a frame's, the end
    /// flap's).
    fn right_clicked(&mut self, session: &CanvasSession, place: &Place, at: Pos2) {
        if self.playing.is_some() {
            self.stop(session, true);
        }
        self.close_menu();
        if place.on_names(at) {
            if let Some(t) = self.name_at(place, at) {
                self.open_menu(Kind::Track { track: t.id }, place.bar.left() + 14.0 + CARD_WIDTH / 2.0);
            }
        } else if place.ruler.contains(at) {
            let duration = self.info().map_or(1, |i| i.duration);
            let flap = place.flap_x(duration);
            if at.x >= flap - 8.0 && at.x <= flap + 22.0 {
                self.open_menu(Kind::Flap, flap);
            }
        } else {
            self.held_at(place, at);
            self.motion.held_clip = None;
        }
    }

    /// The stroke on to `to`: every frame between the last point and this one is picked, or,
    /// picked already, let go (each once a stroke).
    fn stroked(&mut self, place: &Place, to: Pos2) {
        let last = self.stroke.points.last().copied().unwrap_or(to);
        let steps = ((to - last).length() / 4.0).max(1.0) as usize;
        for i in 1..=steps {
            let k = i as f32 / steps as f32;
            self.take(place, last + (to - last) * k);
        }
        self.stroke.points.push(to);
    }

    fn take(&mut self, place: &Place, p: Pos2) {
        let Some((t, c)) = self.clip_at(place, p) else { return };
        if self.held(&t) || !self.stroke.touched.insert(c.id) {
            return;
        }
        if let Some(chosen) = self.selecting.as_mut()
            && !chosen.remove(&c.id)
        {
            chosen.insert(c.id);
        }
    }

    /// The stroke lifted: it fades away.
    fn end_stroke(&mut self, now: f64) {
        self.stroke.faded = std::mem::take(&mut self.stroke.points);
        self.stroke.faded_at = now;
        self.stroke.touched.clear();
        self.stroke.before = None;
    }

    // MARK: Drawing

    fn draw(&self, ui: &Ui, look: &mut Look, session: &CanvasSession, place: &Place, prefs: &Prefs, now: f64) {
        let Some(panel) = self.panel.as_ref() else { return };
        let Some(info) = panel.info.as_ref() else { return };
        let painter = ui.painter();
        let bar = place.bar;
        painter.rect_filled(
            Rect::from_min_size(pos2(bar.left(), place.header.bottom()), vec2(bar.width(), 1.0)),
            0,
            argb(LINE),
        );
        let inside = Rect::from_x_y_ranges(
            bar.left() + ROWS_INSET..=bar.right() - ROWS_INSET,
            place.ruler.top()..=place.rows_rect.bottom(),
        );
        let lo = place.frame_at(inside.left()).unwrap_or(0).saturating_sub(1);
        let hi = ((inside.right() - place.origin) / PITCH).ceil().clamp(0.0, place.last as f32) as u32;
        let ruler = painter.with_clip_rect(place.ruler.intersect(inside));
        self.ruler(&ruler, place, lo, hi);
        let cells = painter.with_clip_rect(place.rows_rect.intersect(inside));
        let paper = {
            let b = session.background();
            let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            if b.visible { Color32::from_rgb(c(b.r), c(b.g), c(b.b)) } else { Color32::WHITE }
        };
        for (r, row) in self.rows.list.iter().enumerate() {
            self.row(&cells, place, r, row, panel, lo, hi, paper);
        }
        // past the timeline's end: shaded, since nothing there plays
        let end = place.flap_x(self.length_draft.unwrap_or(info.duration));
        if end < place.rows_rect.right() {
            let shade = Rect::from_x_y_ranges(
                end.max(place.rows_rect.left())..=place.rows_rect.right(),
                place.rows_rect.y_range(),
            );
            cells.rect_filled(shade, 0, argb(PANEL).gamma_multiply(0.55));
            cells.rect_filled(Rect::from_min_size(shade.min, vec2(1.0, shade.height())), 0, argb(LINE));
        }
        // an empty frame held for its menu: ringed
        if let Some(Kind::Empty { track, frame }) = self.menu.as_ref().map(|m| m.kind)
            && let Some(r) = self.rows.index_of(Some(track))
        {
            let ring =
                Rect::from_min_size(pos2(place.x(frame as f32), place.cell_top(r)), vec2(CELL_WIDTH, CELL_HEIGHT));
            cells.rect_stroke(
                ring.expand(2.5),
                CORNER + 2,
                Stroke::new(2.0, argb(INK_BRIGHT).gamma_multiply(0.85)),
                StrokeKind::Inside,
            );
        }
        // the playhead: an outline on the chosen track's frame where it stands, whatever slides
        // under it (while playing, the grid holds still and it goes over the frames); a frame with
        // its handles out is outlined whole instead
        let picked_shows = self.picked.is_some_and(|p| self.clip(p).is_some());
        if self.selecting.is_none()
            && !picked_shows
            && let Some(r) = self.rows.index_of(info.track)
        {
            let x = match self.playing.as_ref() {
                Some(p) => place.x(p.at as f32),
                None => place.playhead - CELL_WIDTH / 2.0,
            };
            let outline = Rect::from_min_size(pos2(x, place.cell_top(r)), vec2(CELL_WIDTH, CELL_HEIGHT)).expand(2.5);
            cells.rect_stroke(outline, CORNER + 2, Stroke::new(2.5, argb(pc::ACCENT)), StrokeKind::Inside);
        }
        self.draw_handles(ui, look, &cells, place);
        self.draw_move_line(&cells, place);
        self.draw_stroke(ui, &cells, now);
        // the cells fade out under the tracks' names
        let names = Rect::from_x_y_ranges(inside.left()..=bar.left() + LABELS_WIDTH, place.rows_rect.y_range());
        fade(&cells, names, argb(PANEL));
        let frame = self.frame();
        for (r, row) in self.rows.list.iter().enumerate() {
            // (a row several groups' tracks share is named for the group the playhead is in)
            if let Some(t) = row.track(frame) {
                let chosen = Some(t.id) == info.track;
                track_label(ui, look, &cells, place, r, row, t, chosen);
            }
        }
        // the end flap, over the ruler
        self.flap(&ruler, place, info.duration);
        // where the playhead is in time, over the bar's corner
        if prefs.timeline_timecode && self.menu.is_none() {
            let words = crate::timeline_settings::timecode(frame, info.fps);
            let wide = text_width(painter, &words, 17.0, Weight::Regular) + 20.0;
            let capsule = Rect::from_min_size(pos2(bar.left() + 4.0, bar.top() - 36.0), vec2(wide, 28.0));
            painter.rect_filled(capsule, 14, argb(PANEL).gamma_multiply(0.82));
            text(painter, capsule.center(), Align2::CENTER_CENTER, &words, 17.0, Weight::Regular, argb(INK));
        }
    }

    /// The ruler of frame numbers: every frame's, the playhead's lit (in the accent with the lasso
    /// on).
    fn ruler(&self, painter: &egui::Painter, place: &Place, lo: u32, hi: u32) {
        let current = self.frame();
        let y = place.ruler.top() + RULER_HEIGHT / 2.0 + 3.0;
        let lit = if self.selecting.is_some() { argb(pc::ACCENT) } else { argb(INK_BRIGHT) };
        for f in lo..=hi {
            let x = place.x(f as f32) + CELL_WIDTH / 2.0;
            let here = f == current;
            let (weight, ink) = if here { (Weight::Bold, lit) } else { (Weight::Regular, argb(INK_DIM)) };
            text(painter, pos2(x, y), Align2::CENTER_CENTER, &(f + 1).to_string(), 11.0, weight, ink);
        }
    }

    /// The flap on the ruler where the timeline ends (as long as it is being dragged to), lit
    /// while dragged or its menu is open.
    fn flap(&self, painter: &egui::Painter, place: &Place, duration: u32) {
        let frames = self.length_draft.unwrap_or(duration);
        let lit = self.length_draft.is_some() || matches!(self.menu.as_ref().map(|m| m.kind), Some(Kind::Flap));
        let colour = if lit { argb(pc::ACCENT) } else { argb(BOX) };
        let x = place.flap_x(frames);
        let top = place.ruler.top();
        painter.rect_filled(Rect::from_min_size(pos2(x - 1.0, top), vec2(2.0, RULER_HEIGHT)), 0, colour);
        let flag = Rect::from_min_size(pos2(x + 1.0, top + 2.0), vec2(13.0, 13.0));
        painter.rect_filled(flag, CornerRadius { nw: 0, sw: 0, ne: 5, se: 5 }, colour);
        for dy in [-2.0, 2.0] {
            let notch = Rect::from_center_size(flag.center() + vec2(0.0, dy), vec2(6.0, 1.5));
            painter.rect_filled(notch, 1, argb(PANEL));
        }
    }

    /// The lasso's stroke: a glowing line while it is drawn, fading once lifted.
    fn draw_stroke(&self, ui: &Ui, painter: &egui::Painter, now: f64) {
        let line = |points: &[Pos2], alpha: f32| {
            if points.len() < 2 {
                return;
            }
            painter.add(Shape::line(points.to_vec(), Stroke::new(9.0, faded(argb(pc::ACCENT), 0.35 * alpha))));
            painter.add(Shape::line(points.to_vec(), Stroke::new(5.0, faded(argb(pc::ACCENT), alpha))));
            painter.add(Shape::line(points.to_vec(), Stroke::new(2.0, faded(Color32::from_white_alpha(230), alpha))));
        };
        line(&self.stroke.points, 1.0);
        let k = ((now - self.stroke.faded_at) / FADE) as f32;
        if !self.stroke.faded.is_empty() && k < 1.0 {
            line(&self.stroke.faded, 1.0 - k.max(0.0));
            ui.ctx().request_repaint();
        }
    }

    /// Row `r`: an empty slot for each frame of each track on it, a block over each clip's, a
    /// group's outline round its own rows, and a mark under each keyed frame.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        painter: &egui::Painter,
        place: &Place,
        r: usize,
        row: &GridRow,
        panel: &TimelinePanel,
        lo: u32,
        hi: u32,
        paper: Color32,
    ) {
        let top = place.cell_top(r);
        for s in &row.segments {
            let first = lo.max(s.row.start);
            let last = hi.min(s.row.end.saturating_sub(1));
            for f in (first <= last).then_some(first..=last).into_iter().flatten() {
                let cell = Rect::from_min_size(pos2(place.x(f as f32), top), vec2(CELL_WIDTH, CELL_HEIGHT));
                painter.rect_filled(cell, CORNER, argb(SLOT));
            }
            // where a repeating group plays a frame again: its ghost
            for again in s.row.repeats.iter().filter(|a| a.start <= hi && a.start + a.duration > lo) {
                let ghost = Rect::from_min_size(
                    pos2(place.x(again.start as f32), top),
                    vec2(again.duration as f32 * PITCH - GAP, CELL_HEIGHT),
                );
                painter.rect_filled(ghost, CORNER, argb(BLOCK).gamma_multiply(0.45));
            }
        }
        for s in &row.segments {
            let t = s.track();
            let dim = if t.hidden || s.row.hidden_above { 0.45 } else { 1.0 };
            for c in self.shown_clips(t).iter().filter(|c| c.start <= hi && c.start + c.duration > lo) {
                // an expanded group: an outline round it and its own tracks' rows
                if c.kind == TimelineClipKind::Group
                    && let Some(bottom) = self.rows.bottom_of_group(c.id)
                {
                    let (x, w) = self.span(c);
                    let lit = self.selecting.is_none() && self.picked == Some(c.id);
                    let outline = Rect::from_min_max(
                        pos2(place.origin + x - 3.0, top - 3.0),
                        pos2(place.origin + x + w + 3.0, place.top + bottom),
                    );
                    let stroke = if lit {
                        Stroke::new(2.0, argb(pc::ACCENT))
                    } else {
                        Stroke::new(1.2, argb(BOX).gamma_multiply(0.7))
                    };
                    painter.rect_stroke(outline, CORNER + 3, stroke, StrokeKind::Inside);
                }
                self.block(painter, place, top, c, dim, lo, hi, paper, panel);
            }
        }
        // a mark under each keyed frame of a block
        let y = place.row_top(r) + (row.height + CELL_HEIGHT) / 2.0 - 4.5;
        for c in row.segments.iter().flat_map(|s| &s.track().clips) {
            let Some(keys) = panel.keys.iter().find(|k| k.id == c.id) else { continue };
            let mut done = HashSet::new();
            for k in keys.keys.iter().filter(|k| k.frame >= lo && k.frame <= hi && done.insert(k.frame)) {
                let x = place.x(k.frame as f32) + CELL_WIDTH / 2.0;
                let d = 3.2;
                let points = vec![pos2(x, y - d), pos2(x + d, y), pos2(x, y + d), pos2(x - d, y)];
                painter.add(Shape::convex_polygon(points, argb(pc::ACCENT), Stroke::new(1.6, Color32::WHITE)));
            }
        }
    }

    /// Clip `c` over the frames it lasts, its cells' tops at `top`. A drawing shows its picture on
    /// the canvas's paper in each frame, the first as it is and the held ones shaded; a collapsed
    /// group each of its frames' drawings, one over another; an expanded group, a sound or a video
    /// is a plain block with its name. Picked, it is outlined; held for its menu or moving, lifted.
    #[allow(clippy::too_many_arguments)]
    fn block(
        &self,
        painter: &egui::Painter,
        place: &Place,
        top: f32,
        c: &TimelineClipInfo,
        dim: f32,
        lo: u32,
        hi: u32,
        paper: Color32,
        panel: &TimelinePanel,
    ) {
        let (x, width) = self.span(c);
        let block = Rect::from_min_size(pos2(place.origin + x, top), vec2(width, CELL_HEIGHT));
        let chosen = self.selecting.as_ref().is_some_and(|s| s.contains(&c.id));
        let picked = self.selecting.is_none() && self.picked == Some(c.id);
        let menu_on = matches!(self.menu.as_ref().map(|m| m.kind), Some(Kind::Frame { clip } | Kind::Block { clip }) if clip == c.id);
        let moving = self.moving.as_ref().is_some_and(|m| m.ids.contains(&c.id));
        let nowhere = moving && self.moving.as_ref().is_some_and(|m| !m.valid || m.between.is_some());
        let alpha = if nowhere {
            0.4
        } else if c.disabled {
            0.4_f32.min(dim)
        } else {
            dim
        };
        // held for its menu, or moving: lifted a little off the bar
        let block = if menu_on || moving { block.expand2(vec2(2.5, 1.5)) } else { block };
        if menu_on || moving {
            painter.add(
                egui::epaint::Shadow { offset: [0, 2], blur: 12, spread: 0, color: Color32::from_black_alpha(128) }
                    .as_shape(block, CornerRadius::same(CORNER)),
            );
        }
        let tint = c.label.map(label_colour);
        // the frames of it on the bar, counted from its first
        let from = lo.saturating_sub(c.start);
        let to = (hi - c.start.min(hi)).min(c.duration.saturating_sub(1));
        let inside = painter.with_clip_rect(block.intersect(painter.clip_rect()));
        let picture = |root: u64, f: u32, a: f32| {
            let Some((_, Some(picture))) = self.pictures.get(&root) else { return };
            let [w, h] = picture.size().map(|v| v.max(1) as f32);
            let k = (CELL_WIDTH / w).min(CELL_HEIGHT / h);
            let cell = Rect::from_min_size(pos2(block.left() + f as f32 * PITCH, top), vec2(CELL_WIDTH, CELL_HEIGHT));
            let shown = Rect::from_center_size(cell.center(), vec2(w * k, h * k));
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            inside.image(picture.id(), shown, uv, faded(Color32::WHITE, a));
        };
        let shots = (c.kind == TimelineClipKind::Group && !self.rows.unfolded(c.id))
            .then(|| panel.groups.iter().find(|g| g.id == c.id))
            .flatten()
            .filter(|g| !g.shots.is_empty());
        match (c.kind, shots) {
            (TimelineClipKind::Drawing, _) => {
                painter.rect_filled(block, CORNER, faded(paper, alpha));
                for f in from..=to {
                    picture(c.root, f, alpha);
                }
                // held past its first frame
                if width > CELL_WIDTH + 1.0 {
                    let held = Rect::from_x_y_ranges(block.left() + CELL_WIDTH..=block.right(), block.y_range());
                    let corners = CornerRadius { nw: 0, sw: 0, ne: CORNER, se: CORNER };
                    painter.rect_filled(held, corners, faded(argb(HELD_SHADE), alpha));
                }
            }
            (TimelineClipKind::Group, Some(g)) => {
                // collapsed: each of its frames shows everything in it there, bottom first
                painter.rect_filled(block, CORNER, faded(paper, alpha));
                for shot in &g.shots {
                    let first = shot.start.saturating_sub(c.start).max(from);
                    let last = (shot.start + shot.frames).saturating_sub(c.start + 1).min(to);
                    for root in &shot.roots {
                        for f in (first <= last).then_some(first..=last).into_iter().flatten() {
                            picture(*root, f, alpha);
                        }
                    }
                }
                if let Some(tint) = tint {
                    painter.rect_filled(block, CORNER, faded(tint.gamma_multiply(0.3), alpha));
                }
            }
            _ => {
                painter.rect_filled(block, CORNER, faded(tint.unwrap_or_else(|| argb(BLOCK)), alpha));
            }
        }
        // where a group plays its frames again: a notch at each repeat
        if c.kind == TimelineClipKind::Group && c.cycle > 0 {
            let mut f = c.cycle;
            while f < c.duration {
                let x = block.left() + f as f32 * PITCH - GAP / 2.0;
                inside.rect_filled(
                    Rect::from_x_y_ranges(x - 0.75..=x + 0.75, block.y_range()),
                    0,
                    argb(pc::ACCENT).gamma_multiply(0.85),
                );
                f += c.cycle;
            }
        }
        // (its name follows the bar's edge no further than its own first frame)
        let name_x = block.left() + from as f32 * PITCH;
        if c.kind == TimelineClipKind::Group {
            // its chevron and name in a dark capsule over its top
            let open = self.rows.unfolded(c.id);
            let words = text_width(painter, &c.name, 9.5, Weight::Bold);
            let capsule = Rect::from_min_size(pos2(name_x + 3.0, top + 2.0), vec2(words + 26.0, 15.0));
            inside.rect_filled(capsule, 7.5, Color32::from_black_alpha(140));
            chevron(&inside, pos2(capsule.left() + 9.0, capsule.center().y), open, Color32::WHITE);
            text(
                &inside,
                pos2(capsule.left() + 17.0, capsule.center().y),
                Align2::LEFT_CENTER,
                &c.name,
                9.5,
                Weight::Bold,
                Color32::WHITE,
            );
        } else if c.kind != TimelineClipKind::Drawing {
            let ink = if tint.is_some() { argb(ON_LABEL) } else { argb(INK_BRIGHT) };
            text(
                &inside,
                pos2(name_x + 7.0, block.center().y),
                Align2::LEFT_CENTER,
                &c.name,
                12.0,
                Weight::Regular,
                faded(ink, alpha),
            );
        }
        if chosen || picked || moving {
            painter.rect_stroke(block.expand(2.5), CORNER + 2, Stroke::new(2.5, argb(pc::ACCENT)), StrokeKind::Inside);
        }
        // touched by the stroke under way: glowing
        if chosen && !self.stroke.points.is_empty() {
            painter.rect_filled(block, CORNER, argb(pc::ACCENT).gamma_multiply(0.28));
        }
    }
}

/// Where the header's buttons are: Play (its word `words` wide), a frame back and on; then the
/// settings, the lasso, a new track and a new frame.
fn header_buttons(place: &Place, words: f32) -> [(Button, Rect); 7] {
    let y = place.header.top() + 4.0 + 20.0;
    let play = Rect::from_min_size(pos2(place.bar.left() + 26.0, y - 20.0), vec2(words.max(54.0), 40.0));
    let back = Rect::from_min_size(pos2(play.right() + 18.0, y - 20.0), vec2(50.0, 40.0));
    let add = Rect::from_min_size(pos2(place.bar.right() - 16.0 - 50.0, y - 20.0), vec2(50.0, 40.0));
    [
        (Button::Play, play),
        (Button::Back, back),
        (Button::On, back.translate(vec2(50.0, 0.0))),
        (Button::Settings, add.translate(vec2(-150.0, 0.0))),
        (Button::Lasso, add.translate(vec2(-100.0, 0.0))),
        (Button::Track, add.translate(vec2(-50.0, 0.0))),
        (Button::Add, add),
    ]
}

#[cfg(test)]
impl TimelineGrid {
    /// The middle of row `r`'s (top first) cell of frame `f` in the window, as the frames stand.
    pub(crate) fn cell_centre(&self, ctx: &egui::Context, r: usize, f: u32) -> Pos2 {
        let place = self.place(ctx);
        pos2(place.x(f as f32) + CELL_WIDTH / 2.0, place.cell_top(r) + CELL_HEIGHT / 2.0)
    }

    /// The middle of the ruler over frame `f`.
    pub(crate) fn ruler_centre(&self, ctx: &egui::Context, f: u32) -> Pos2 {
        let place = self.place(ctx);
        pos2(place.x(f as f32) + CELL_WIDTH / 2.0, place.ruler.center().y)
    }

    /// The middle of the header's button `name`: Play, Back, On, Settings, Lasso, Track or Add.
    pub(crate) fn button_centre(&self, ctx: &egui::Context, name: &str) -> Pos2 {
        let place = self.place(ctx);
        let found = header_buttons(&place, 0.0).into_iter().find(|(b, _)| format!("{b:?}") == name);
        found.map_or(Pos2::ZERO, |(_, r)| r.center())
    }

    /// The middle of row `r`'s track's name.
    pub(crate) fn name_centre(&self, ctx: &egui::Context, r: usize) -> Pos2 {
        let place = self.place(ctx);
        let depth = self.rows.get(r).map_or(0, |row| row.depth) as f32;
        pos2(place.bar.left() + 12.0 + depth * INDENT + 6.0 + 26.0 + 30.0, place.row_top(r) + ROW_HEIGHT / 2.0)
    }

    /// The middle of the handled frame's start's (`leading`) or end's handle.
    pub(crate) fn handle_centre(&self, ctx: &egui::Context, leading: bool) -> Option<Pos2> {
        let place = self.place(ctx);
        let h = self.handled()?;
        let (x, w) = self.span(&h.clip);
        let x = place.origin
            + if leading { x + super::edges::HANDLE_OVERLAP - 8.0 } else { x + w - super::edges::HANDLE_OVERLAP + 8.0 };
        Some(pos2(x, place.cell_top(h.row) + CELL_HEIGHT / 2.0))
    }

    /// The middle of the end flap.
    pub(crate) fn flap_centre(&self, ctx: &egui::Context) -> Pos2 {
        let place = self.place(ctx);
        let x = place.flap_x(self.info().map_or(1, |i| i.duration));
        pos2(x + 7.0, place.ruler.top() + 8.0)
    }

    /// Whether it has a timeline to show.
    pub(crate) fn showing(&self) -> bool {
        self.info().is_some()
    }

    /// The frame under the playhead as shown.
    pub(crate) fn shown_at(&self) -> f32 {
        self.motion.at
    }

    /// The frames picked together (the lasso on), the frame picked, and the menu open.
    pub(crate) fn chosen_now(&self) -> Option<Vec<u64>> {
        self.selecting.as_ref().map(|s| s.iter().copied().collect())
    }

    pub(crate) fn picked_now(&self) -> Option<u64> {
        self.picked
    }

    pub(crate) fn menu_open(&self) -> bool {
        self.menu.is_some()
    }

    /// How many rows the grid shows, and how many tracks are on row `r`.
    pub(crate) fn row_count(&self) -> usize {
        self.rows.len()
    }

    pub(crate) fn tracks_on_row(&self, r: usize) -> usize {
        self.rows.get(r).map_or(0, |row| row.segments.len())
    }

    /// Where the line over row `r` (its top edge) is, at frame `f`.
    pub(crate) fn row_line(&self, ctx: &egui::Context, r: usize, f: u32) -> Pos2 {
        let place = self.place(ctx);
        pos2(place.x(f as f32) + CELL_WIDTH / 2.0, place.row_top(r))
    }

    /// A picture as a new frame at the playhead, as Add new's picture does once it is read.
    pub(crate) fn add_picture_for_test(&mut self, session: &CanvasSession, image: procraft_core::ThumbnailImage) {
        self.add_picture_frame(session, image);
    }
}

/// `rect` filled with `colour` on its left, fading out to nothing at its right edge from 45% of
/// the way.
fn fade(painter: &egui::Painter, rect: Rect, colour: Color32) {
    let mut mesh = egui::Mesh::default();
    let solid = rect.left() + (rect.width() * 0.45).max(0.0);
    let mut quad = |left: f32, right: f32, a: Color32, b: Color32| {
        let i = mesh.vertices.len() as u32;
        mesh.colored_vertex(pos2(left, rect.top()), a);
        mesh.colored_vertex(pos2(right, rect.top()), b);
        mesh.colored_vertex(pos2(right, rect.bottom()), b);
        mesh.colored_vertex(pos2(left, rect.bottom()), a);
        mesh.add_triangle(i, i + 1, i + 2);
        mesh.add_triangle(i, i + 2, i + 3);
    };
    quad(rect.left(), solid, colour, colour);
    quad(solid, rect.right(), colour, Color32::TRANSPARENT);
    painter.add(Shape::mesh(mesh));
}

/// A small chevron at `at`: pointing down when `open`, else right.
fn chevron(painter: &egui::Painter, at: Pos2, open: bool, ink: Color32) {
    let points = if open {
        vec![at + vec2(-3.5, -1.8), at + vec2(0.0, 1.8), at + vec2(3.5, -1.8)]
    } else {
        vec![at + vec2(-1.8, -3.5), at + vec2(1.8, 0.0), at + vec2(-1.8, 3.5)]
    };
    painter.add(Shape::line(points, Stroke::new(1.6, ink)));
}

/// Row `r`'s track `t`'s name in its capsule (the chosen track's lit), with its box, standing
/// still over the frames, stepped in for each group it is inside. (Room is left after the name:
/// what more a track's row holds there is the keys'.)
#[allow(clippy::too_many_arguments)]
fn track_label(
    ui: &Ui,
    look: &mut Look,
    painter: &egui::Painter,
    place: &Place,
    r: usize,
    row: &GridRow,
    t: &TimelineTrackInfo,
    chosen: bool,
) {
    let label = t.label.map(label_colour);
    let ink = match (label, chosen) {
        (Some(_), _) => argb(ON_LABEL),
        (None, true) => argb(INK_BRIGHT),
        (None, false) => argb(INK),
    };
    let alpha = if t.hidden { 0.6 } else { 1.0 };
    let indent = row.depth as f32 * INDENT;
    let most = LABELS_WIDTH - 34.0 - 6.0 - 26.0 - 6.0 - 14.0 - indent - if t.locked { 20.0 } else { 0.0 };
    let name = fitted(painter, &t.name, most.max(20.0));
    let words = text_width(painter, &name, 14.0, Weight::Medium);
    let left = place.bar.left() + 12.0 + indent;
    let width = 6.0 + 26.0 + 6.0 + words + if t.locked { 20.0 } else { 0.0 } + 14.0;
    let middle = place.row_top(r) + row.height / 2.0;
    let capsule = Rect::from_min_size(pos2(left, middle - 16.0), vec2(width, 32.0));
    if let Some(fill) = label.or(chosen.then(|| argb(CAPSULE))) {
        painter.rect_filled(capsule, 16, faded(fill, alpha));
    }
    if chosen && label.is_some() {
        painter.rect_stroke(capsule, 16, Stroke::new(1.5, argb(INK_BRIGHT).gamma_multiply(0.75)), StrokeKind::Inside);
    }
    // its box: ticked while it shows
    let boxed = Rect::from_center_size(pos2(left + 6.0 + 13.0, middle), vec2(16.0, 16.0));
    let fill = match (label, t.soloed, chosen) {
        (Some(_), _, _) => argb(ON_LABEL),
        (None, true, _) => argb(pc::ACCENT),
        (None, false, true) => argb(INK_BRIGHT),
        (None, false, false) => argb(BOX),
    };
    if t.hidden {
        painter.rect_stroke(boxed, 4, Stroke::new(1.5, faded(fill, alpha)), StrokeKind::Inside);
    } else {
        painter.rect_filled(boxed, 4, fill);
        let tick = vec![
            pos2(boxed.left() + 3.5, boxed.center().y),
            pos2(boxed.left() + 6.8, boxed.bottom() - 4.0),
            pos2(boxed.right() - 3.5, boxed.top() + 4.0),
        ];
        painter.add(Shape::line(tick, Stroke::new(1.8, argb(PANEL))));
    }
    let at = pos2(boxed.right() + 7.0, middle);
    let shown = text(painter, at, Align2::LEFT_CENTER, &name, 14.0, Weight::Medium, faded(ink, alpha));
    if t.locked && painter.clip_rect().contains(pos2(shown.right() + 13.0, middle)) {
        look.icon_on(ui, painter, &icons::LOCK, pos2(shown.right() + 13.0, middle), 14.0, faded(ink, alpha));
    }
}

/// `name` cut short with an ellipsis to fit `most` points.
fn fitted(painter: &egui::Painter, name: &str, most: f32) -> String {
    if text_width(painter, name, 14.0, Weight::Medium) <= most {
        return name.to_owned();
    }
    let mut chars: Vec<char> = name.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let cut: String = chars.iter().collect::<String>() + "…";
        if text_width(painter, &cut, 14.0, Weight::Medium) <= most {
            return cut;
        }
    }
    "…".to_owned()
}

/// The settings button's glyph: two sliders, 27 points across.
fn settings_glyph(painter: &egui::Painter, at: Pos2, ink: Color32) {
    let line = Stroke::new(1.8, ink);
    for (dy, knob) in [(-4.5, 4.0), (4.5, -4.0)] {
        let y = at.y + dy;
        painter.line_segment([pos2(at.x - 9.0, y), pos2(at.x + 9.0, y)], line);
        painter.circle(pos2(at.x + knob, y), 3.0, argb(PANEL), line);
    }
}

/// The new track button's glyph: two tracks, a plus by them, 27 points across.
fn track_glyph(painter: &egui::Painter, at: Pos2, ink: Color32) {
    let line = Stroke::new(1.6, ink);
    for dy in [-4.5, 4.5] {
        let r = Rect::from_center_size(pos2(at.x - 2.5, at.y + dy), vec2(15.0, 6.0));
        painter.rect_stroke(r, 2, line, StrokeKind::Middle);
    }
    let plus = pos2(at.x + 9.0, at.y + 4.5);
    painter.line_segment([plus - vec2(3.5, 0.0), plus + vec2(3.5, 0.0)], line);
    painter.line_segment([plus - vec2(0.0, 3.5), plus + vec2(0.0, 3.5)], line);
}

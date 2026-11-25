//! The grid's bar (see the module's own words): its sizes and colours, as the iPad's `Grid` and
//! `GridSizes` at the size the grid starts at; drawing it; and what a press on it does.

use std::collections::HashSet;

use egui::{Align2, Color32, CornerRadius, Id, Order, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use procraft_core::{CanvasSession, TimelineClipInfo, TimelineClipKind, TimelineKeyPick, TimelinePanel, TimelineRow};

use super::keys::{KEYS_HEIGHT, Kind};
use super::{PAST_END, TimelineGrid};
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, cubic_bezier, pc, text, text_width};

/// From one frame's cell to the next, a cell's own size, a row's height, a cell's corners.
pub(super) const PITCH: f32 = 44.0;
pub(super) const CELL_WIDTH: f32 = 40.0;
pub(super) const CELL_HEIGHT: f32 = 31.0;
pub(super) const ROW_HEIGHT: f32 = 44.0;
const CORNER: u8 = 7;
const GAP: f32 = PITCH - CELL_WIDTH;
/// The ruler's and the header's heights, the space under the rows, and the bar's gap from the
/// window's bottom edge and its corners.
pub(super) const RULER_HEIGHT: f32 = 26.0;
pub(super) const HEADER_HEIGHT: f32 = 46.0;
const UNDER_ROWS: f32 = 9.0;
const BAR_GAP: f32 = 19.0;
const BAR_CORNER: u8 = 28;
/// The widest the bar gets.
const MOST_WIDTH: f32 = 1180.0;
/// How far in from the bar's left edge the cells show whole: before it they fade out under the
/// tracks' names, and presses there are the names'.
pub(super) const LABELS_WIDTH: f32 = 176.0;
/// How far in from the bar's ends the frames show (its rounded corners stand there).
const ROWS_INSET: f32 = 8.0;
/// A frame tapped or stepped to slides under the playhead in this long (seconds).
const SLIDE: f64 = 0.35;
/// How long a header button is held for what holding it does (seconds).
pub(super) const HOLD: f64 = 0.4;
/// How long a tapped header button stays lit (seconds).
const FLASH: f64 = 0.18;

pub(super) const PANEL: Themed = Themed(0xFF212121, 0xFFE4E4E4);
const SLOT: Themed = Themed(0xFF161616, 0xFFD1D1D1);
const LINE: Themed = Themed(0xFF474747, 0xFFC0C0C0);
const CAPSULE: Themed = Themed(0xFF353535, 0xFFCBCBCB);
pub(super) const INK: Themed = Themed(0xFFC9C9C9, 0xFF393939);
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
    /// With Animate on: a take armed or let go (Perform).
    Record,
    Back,
    On,
    Settings,
    /// Animate: autokey on or off.
    Animate,
    Track,
    Add,
}

/// A line of the grid, top first: a track's row of frames (its place among the rows, top first),
/// or a row of keys under it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Line {
    Track(usize),
    Keys(usize, Kind),
}

/// What a press on the frames (or the ruler) is doing.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Touch {
    #[default]
    None,
    /// Dragged sideways: the frames slide under the playhead, from frame `from` there.
    Scroll { from: f32 },
    /// Dragged up or down where there are more tracks than show, from `from` scrolled.
    Pan { from: f32 },
    /// Nothing of the frames' (it stopped them playing).
    Ignore,
    /// A key's mark dragged: it and the keys picked with it go along by frames.
    Keys { key: TimelineKeyPick },
    /// A grip beside the keys picked dragged from frame `from`: they stretch about `anchor`.
    Stretch { anchor: u32, from: u32 },
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

/// Where everything stands on the bar.
pub(super) struct Place {
    pub(super) bar: Rect,
    header: Rect,
    ruler: Rect,
    /// The rows' area as it shows, and the top of the first row (scrolled).
    pub(super) rows: Rect,
    pub(super) top: f32,
    /// The playhead's cell's middle, and frame 0's cell's left edge.
    playhead: f32,
    origin: f32,
    /// The last frame there is a cell for.
    last: u32,
    /// The grid's lines, top first: each with its top (down from the first's) and height.
    pub(super) lines: Vec<(Line, f32, f32)>,
    /// How far the rows can scroll.
    over: f32,
}

impl Place {
    /// Frame `f`'s cell's left edge.
    pub(super) fn x(&self, f: f32) -> f32 {
        self.origin + f * PITCH
    }

    /// The frame whose cell (and the gap after it) `x` is over, if there is one.
    pub(super) fn frame_at(&self, x: f32) -> Option<u32> {
        let f = ((x - self.origin) / PITCH).floor();
        (f >= 0.0 && f <= self.last as f32).then_some(f as u32)
    }

    /// The line at height `y`.
    pub(super) fn line_at(&self, y: f32) -> Option<Line> {
        if !self.rows.y_range().contains(y) || y < self.top {
            return None;
        }
        let y = y - self.top;
        self.lines.iter().find(|(_, top, height)| y >= *top && y < top + height).map(|(l, _, _)| *l)
    }

    /// The track's row (top first) at height `y` (none on a row of keys).
    fn row_at(&self, y: f32) -> Option<usize> {
        match self.line_at(y) {
            Some(Line::Track(r)) => Some(r),
            _ => None,
        }
    }

    /// Row `r`'s top edge, down from the first's.
    fn row_offset(&self, r: usize) -> f32 {
        self.lines.iter().find(|(l, _, _)| *l == Line::Track(r)).map_or(0.0, |(_, top, _)| *top)
    }

    /// Row `r`'s top edge.
    pub(super) fn row_top(&self, r: usize) -> f32 {
        self.top + self.row_offset(r)
    }

    /// The top of a cell on row `r`.
    fn cell_top(&self, r: usize) -> f32 {
        self.row_top(r) + (ROW_HEIGHT - CELL_HEIGHT) / 2.0
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

/// The rows, top first: the core reads them bottom first.
pub(super) fn rows_of(panel: &TimelinePanel) -> Vec<&TimelineRow> {
    panel.rows.iter().rev().collect()
}

/// Whether frame `f` is one of row `row`'s (a track inside a group has the frames the group lasts).
fn has(row: &TimelineRow, f: u32) -> bool {
    f >= row.start && f < row.end
}

/// `c` made `alpha` as strong.
fn faded(c: Color32, alpha: f32) -> Color32 {
    if alpha >= 1.0 { c } else { c.gamma_multiply(alpha) }
}

/// A colour label (0xRRGGBB) as a colour.
fn label_colour(rgb: u32) -> Color32 {
    let [_, r, g, b] = rgb.to_be_bytes();
    Color32::from_rgb(r, g, b)
}

impl TimelineGrid {
    /// Shows the grid at the bottom of the window while the artwork is on a timeline; returns what
    /// to tell (a frame that couldn't be made).
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession) -> Option<&'static str> {
        if !self.read(ctx, session) {
            return None;
        }
        self.play_on(ctx, session);
        let place = self.place(ctx);
        let now = ctx.input(|i| i.time);
        let mut said = None;
        egui::Area::new(Id::unique("timeline grid")).fixed_pos(place.bar.min).order(Order::Middle).show(ctx, |ui| {
            // the whole bar takes its presses: none reach the canvas under it
            ui.allocate_rect(place.bar, Sense::click_and_drag());
            crate::screen::claim(ui, place.bar);
            said = self.header(ui, look, session, &place, now);
            self.touches(ui, session, &place, now);
            // where the frames stand, now that the presses have moved them
            let moving = match (self.playing.as_ref(), self.motion.touch) {
                (Some(p), _) => {
                    self.motion.at = p.held;
                    false
                }
                (None, Touch::Scroll { .. }) => false,
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
            self.draw(ui, look, session, &place);
        });
        let place = self.place(ctx);
        super::animate::keep_bar_top(ctx, place.bar.top());
        said = self.key_menu(ctx, look, session).or(said);
        said = self.keys_card(ctx, look, session, &place).or(said);
        self.motion_path(ctx, session, &place);
        said = self.perform(ctx, session, &place).or(said);
        self.autokey_badge(ctx, session);
        said
    }

    /// Where the bar and everything on it stand in the window, the frames as they are now.
    fn place(&self, ctx: &egui::Context) -> Place {
        let screen = ctx.content_rect();
        let width = (screen.width() - 32.0).clamp(LABELS_WIDTH * 2.0, MOST_WIDTH);
        let lines = self.lines();
        let all = lines.last().map_or(0.0, |(_, top, height)| top + height).max(ROW_HEIGHT);
        let most = (screen.height() * 0.5 - HEADER_HEIGHT - RULER_HEIGHT - 12.0).floor().max(ROW_HEIGHT);
        let shown = all.min(most);
        let height = HEADER_HEIGHT + 1.0 + RULER_HEIGHT + shown + UNDER_ROWS;
        let bar = Rect::from_min_size(
            pos2(screen.center().x - width / 2.0, screen.bottom() - BAR_GAP - height),
            vec2(width, height),
        );
        let header = Rect::from_min_size(bar.min, vec2(width, HEADER_HEIGHT));
        let ruler = Rect::from_min_size(pos2(bar.left(), header.bottom() + 1.0), vec2(width, RULER_HEIGHT));
        let rows = Rect::from_min_size(pos2(bar.left(), ruler.bottom()), vec2(width, shown));
        let over = (all - shown).max(0.0);
        let scroll = self.motion.rows_offset.clamp(0.0, over);
        let playhead = (bar.left() + width / 2.0).round();
        let duration = self.info().map_or(1, |i| i.duration);
        Place {
            bar,
            header,
            ruler,
            rows,
            top: rows.top() - scroll,
            playhead,
            origin: playhead - CELL_WIDTH / 2.0 - self.motion.at * PITCH,
            last: duration.saturating_sub(1) + PAST_END,
            lines,
            over,
        }
    }

    /// The grid's lines, top first: each track's row, and under a track whose rows of keys are
    /// out, a row for each kind of key its blocks have.
    fn lines(&self) -> Vec<(Line, f32, f32)> {
        let Some(panel) = self.panel.as_ref() else { return Vec::new() };
        let mut out = Vec::new();
        let mut y = 0.0;
        for (r, row) in rows_of(panel).into_iter().enumerate() {
            out.push((Line::Track(r), y, ROW_HEIGHT));
            y += ROW_HEIGHT;
            if self.keys.out.contains(&row.track.id) {
                for kind in super::keys::kinds(panel, row) {
                    out.push((Line::Keys(r, kind), y, KEYS_HEIGHT));
                    y += KEYS_HEIGHT;
                }
            }
        }
        out
    }

    /// Where not every track shows, the rows scrolled as little as brings the chosen track's
    /// wholly into sight, once each time another is chosen.
    fn reveal(&mut self, place: &Place) {
        let Some(panel) = self.panel.as_ref() else { return };
        let track = panel.info.as_ref().and_then(|i| i.track);
        if track == self.motion.revealed {
            return;
        }
        self.motion.revealed = track;
        let Some(r) = rows_of(panel).iter().position(|row| Some(row.track.id) == track) else { return };
        let top = place.row_offset(r);
        let now = self.motion.rows_offset.clamp(0.0, place.over);
        self.motion.rows_offset = now.min(top).max(top + ROW_HEIGHT - place.rows.height()).clamp(0.0, place.over);
    }

    // MARK: The header

    /// The header: Play, a frame back and on; then the settings, a new track and a new frame.
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
        let autokey = self.animate.autokey;
        let buttons = header_buttons(place, text_width(painter, word, 17.0, Weight::Bold), autokey);
        let at = |b: Button| buttons.iter().find(|(x, _)| *x == b).map_or(Rect::NOTHING, |(_, r)| *r);
        text(painter, pos2(at(Button::Play).left(), y), Align2::LEFT_CENTER, word, 17.0, Weight::Bold, ink);
        let lit = |b: Button| self.motion.flash.is_some_and(|(f, at)| f == b && now - at < FLASH);
        let tint = |b: Button| if lit(b) { argb(pc::ACCENT) } else { ink };
        if autokey {
            super::animate::record_glyph(painter, at(Button::Record).center(), self.animate.armed);
        }
        look.icon(ui, &icons::CHEVRON_LEFT, at(Button::Back).center(), 21.0, tint(Button::Back));
        look.icon(ui, &icons::CHEVRON_RIGHT, at(Button::On).center(), 21.0, tint(Button::On));
        settings_glyph(painter, at(Button::Settings).center(), tint(Button::Settings));
        self.settings_button = Some(at(Button::Settings));
        let animate = if autokey { super::animate::RED } else { tint(Button::Animate) };
        super::animate::animate_glyph(painter, at(Button::Animate).center(), animate, autokey);
        track_glyph(painter, at(Button::Track).center(), tint(Button::Track));
        look.icon(ui, &icons::ADD, at(Button::Add).center(), 38.0, tint(Button::Add));
        let mut said = None;
        for (button, rect) in buttons {
            let response = ui.interact(rect, ui.scope_id().with(("timeline header", button as u8)), Sense::click());
            // held: ‹ and › go to the drawing before or after
            if response.is_pointer_button_down_on() {
                let held = match self.motion.press {
                    Some((b, since, acted)) if b == button => (since, acted),
                    _ => {
                        self.motion.press = Some((button, now, false));
                        (now, false)
                    }
                };
                if matches!(button, Button::Back | Button::On) && !held.1 {
                    if now - held.0 >= HOLD {
                        self.motion.press = Some((button, held.0, true));
                        self.step_drawing(session, if button == Button::On { 1 } else { -1 });
                        self.read_at = None;
                    } else {
                        ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(HOLD - (now - held.0)));
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
            Button::Animate => {
                self.stop(session, false);
                self.animate_asked = Some(!self.animate.autokey);
            }
            Button::Record => {
                self.stop(session, false);
                self.animate.armed = !self.animate.armed;
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

    /// A press on the ruler or the rows: tapped, the playhead goes to the frame (and its track);
    /// dragged sideways, the frames scrub under the playhead and settle on a frame let go; dragged
    /// up or down, the tracks scroll where there are more than show. On the tracks' names, a tap
    /// chooses the track (its box shows or hides it).
    fn touches(&mut self, ui: &Ui, session: &CanvasSession, place: &Place, now: f64) {
        let area = Rect::from_min_max(place.ruler.min, place.rows.max);
        let response = ui.interact(area, ui.scope_id().with("timeline frames"), Sense::click_and_drag());
        let origin = ui.input(|i| i.pointer.press_origin());
        let on_names = |p: Pos2| place.rows.contains(p) && p.x < place.bar.left() + LABELS_WIDTH - 12.0;
        // a press on the frames while they play stops them there, and does nothing more
        if self.playing.is_some() && response.is_pointer_button_down_on() && !origin.is_some_and(on_names) {
            self.stop(session, true);
            self.motion.touch = Touch::Ignore;
            self.read_at = None;
        }
        // a mark held still: its menu
        let down = response.is_pointer_button_down_on() && self.motion.touch == Touch::None && self.playing.is_none();
        let held = self.keys_held(ui, place, now, down);
        if response.drag_started() && self.motion.touch != Touch::Ignore {
            let delta = ui.input(|i| i.pointer.interact_pos().zip(origin).map_or(egui::Vec2::ZERO, |(p, o)| p - o));
            let keys = origin.filter(|o| !on_names(*o)).and_then(|o| self.key_touch(place, o));
            self.motion.touch = if origin.is_some_and(on_names) {
                Touch::Ignore
            } else if let Some(touch) = keys {
                touch
            } else if delta.x.abs() < delta.y.abs()
                && place.over > 0.0
                && origin.is_some_and(|o| place.rows.contains(o))
            {
                Touch::Pan { from: self.motion.rows_offset.clamp(0.0, place.over) }
            } else {
                self.motion.slide = None;
                self.motion.scrubbed = Some(self.frame());
                Touch::Scroll { from: self.motion.at }
            };
        }
        let moved = ui.input(|i| i.pointer.interact_pos().zip(origin).map_or(egui::Vec2::ZERO, |(p, o)| p - o));
        match self.motion.touch {
            Touch::Scroll { from } if response.dragged() => {
                let at = (from - moved.x / PITCH).clamp(0.0, place.last as f32);
                self.motion.at = at;
                let frame = at.round() as u32;
                if self.motion.scrubbed != Some(frame) {
                    self.motion.scrubbed = Some(frame);
                    session.set_timeline_playing(Some(frame));
                }
            }
            Touch::Pan { from } if response.dragged() => {
                self.motion.rows_offset = (from - moved.y).clamp(0.0, place.over);
            }
            Touch::Keys { key } if response.dragged() => {
                // (a hold's menu stays until the mark is taken somewhere)
                if moved.length() > 8.0 {
                    self.keys.menu = None;
                }
                self.keys_dragged(session, key, (moved.x / PITCH).round() as i32);
            }
            Touch::Stretch { anchor, from } if response.dragged() => {
                self.keys_stretched(session, anchor, from, moved.x);
            }
            _ => {}
        }
        if response.drag_stopped() {
            if let Touch::Keys { .. } | Touch::Stretch { .. } = self.motion.touch {
                self.keys_let_go(session);
            }
            if let Touch::Scroll { .. } = self.motion.touch {
                // carried on as far as it was going, then settling on a frame, which is drawn on
                let speed = ui.input(|i| i.pointer.velocity().x);
                // (from where the drag left them: the press's origin is gone once it is let go)
                let at = (self.motion.at - speed / 4.2 / PITCH).clamp(0.0, place.last as f32).round();
                session.set_timeline_playing(None);
                session.rest_timeline_frame(at as u32);
                self.motion.slide = Some((self.motion.at, at, now));
                self.motion.focus = Some(at);
                self.motion.scrubbed = None;
                self.read_at = None;
            }
            self.motion.touch = Touch::None;
        }
        if response.clicked()
            && self.motion.touch != Touch::Ignore
            && !held
            && let Some(at) = response.interact_pointer_pos()
        {
            self.tapped(ui, session, place, at);
            self.read_at = None;
        }
        // a mark right-clicked: its menu
        if response.secondary_clicked()
            && let Some(at) = response.interact_pointer_pos()
            && let Some(key) = self.mark_at(place, at)
        {
            self.open_key_menu(place, key, at);
        }
        if !response.is_pointer_button_down_on() && !response.drag_stopped() && !response.dragged() {
            self.motion.touch = Touch::None;
        }
    }

    /// A tap at `at`: on the ruler, the playhead goes to the frame there; on a frame, the playhead
    /// goes to it on its track; on a track's name, it is chosen (its box, hidden or shown; its
    /// chevron, its rows of keys out or away); on a row of keys, see [`TimelineGrid::keys_tapped`].
    fn tapped(&mut self, ui: &Ui, session: &CanvasSession, place: &Place, at: Pos2) {
        if self.keys_tapped(session, place, at) {
            return;
        }
        let Some(panel) = self.panel.as_ref() else { return };
        let rows = rows_of(panel);
        if place.ruler.contains(at) {
            if let Some(f) = place.frame_at(at.x)
                && f != self.frame()
            {
                self.go_to(session, f);
            }
            return;
        }
        let Some(r) = place.row_at(at.y) else { return };
        let Some(row) = rows.get(r).copied() else { return };
        let track = row.track.id;
        if at.x < place.bar.left() + LABELS_WIDTH - 12.0 {
            let check = place.bar.left() + 12.0 + 6.0 + 26.0;
            let chevron = label_place(ui.painter(), place, r, row, super::keys::keyed(panel, row)).chevron;
            if chevron.is_some_and(|c| c.expand(4.0).contains(at)) {
                if !self.keys.out.remove(&track) {
                    self.keys.out.insert(track);
                }
            } else if at.x < check {
                session.set_timeline_track(track, !row.track.hidden, row.track.locked);
            } else {
                self.select_track(session, track);
            }
            return;
        }
        let Some(f) = place.frame_at(at.x).filter(|f| has(row, *f)) else { return };
        let chosen = self.info().and_then(|i| i.track);
        if chosen == Some(track) && f == self.frame() {
            return;
        }
        self.select_track(session, track);
        if f != self.frame() {
            self.go_to(session, f);
        }
    }

    // MARK: Drawing

    fn draw(&self, ui: &Ui, look: &mut Look, session: &CanvasSession, place: &Place) {
        let Some(panel) = self.panel.as_ref() else { return };
        let Some(info) = panel.info.as_ref() else { return };
        let painter = ui.painter();
        let bar = place.bar;
        painter.add(
            egui::epaint::Shadow { offset: [0, 6], blur: 16, spread: 0, color: Color32::from_black_alpha(77) }
                .as_shape(bar, CornerRadius::same(BAR_CORNER)),
        );
        painter.rect_filled(bar, BAR_CORNER, argb(PANEL));
        painter.rect_filled(
            Rect::from_min_size(pos2(bar.left(), place.header.bottom()), vec2(bar.width(), 1.0)),
            0,
            argb(LINE),
        );
        let inside = Rect::from_x_y_ranges(
            bar.left() + ROWS_INSET..=bar.right() - ROWS_INSET,
            place.ruler.top()..=place.rows.bottom(),
        );
        let rows = rows_of(panel);
        let lo = place.frame_at(inside.left()).unwrap_or(0).saturating_sub(1);
        let hi = ((inside.right() - place.origin) / PITCH).ceil().clamp(0.0, place.last as f32) as u32;
        self.ruler(&painter.with_clip_rect(place.ruler.intersect(inside)), place, lo, hi);
        let cells = painter.with_clip_rect(place.rows.intersect(inside));
        let paper = {
            let b = session.background();
            let c = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            if b.visible { Color32::from_rgb(c(b.r), c(b.g), c(b.b)) } else { Color32::WHITE }
        };
        for (r, row) in rows.iter().enumerate() {
            self.row(&cells, place, r, row, panel, lo, hi, paper);
        }
        self.draw_keys(&cells, place, lo, hi);
        // past the timeline's end: shaded, since nothing there plays
        let end = place.x(info.duration as f32) - GAP / 2.0;
        if end < place.rows.right() {
            let shade = Rect::from_x_y_ranges(end.max(place.rows.left())..=place.rows.right(), place.rows.y_range());
            cells.rect_filled(shade, 0, argb(PANEL).gamma_multiply(0.55));
            cells.rect_filled(Rect::from_min_size(shade.min, vec2(1.0, shade.height())), 0, argb(LINE));
        }
        // the playhead: an outline on the chosen track's frame where it stands, whatever slides
        // under it (while playing, the grid holds still and it goes over the frames)
        if let Some(r) = rows.iter().position(|row| Some(row.track.id) == info.track) {
            let x = match self.playing.as_ref() {
                Some(p) => place.x(p.at as f32),
                None => place.playhead - CELL_WIDTH / 2.0,
            };
            let outline = Rect::from_min_size(pos2(x, place.cell_top(r)), vec2(CELL_WIDTH, CELL_HEIGHT)).expand(2.5);
            cells.rect_stroke(outline, CORNER + 2, Stroke::new(2.5, argb(pc::ACCENT)), StrokeKind::Inside);
        }
        // the cells fade out under the tracks' names
        let names = Rect::from_x_y_ranges(inside.left()..=bar.left() + LABELS_WIDTH, place.rows.y_range());
        fade(&cells, names, argb(PANEL));
        for (r, row) in rows.iter().enumerate() {
            let chosen = Some(row.track.id) == info.track;
            let keyed = super::keys::keyed(panel, row).then(|| self.keys.out.contains(&row.track.id));
            track_label(ui, look, &cells, place, r, row, chosen, keyed);
        }
        self.draw_key_names(&cells, place);
    }

    /// The ruler of frame numbers: every frame's, the playhead's lit.
    fn ruler(&self, painter: &egui::Painter, place: &Place, lo: u32, hi: u32) {
        let current = self.frame();
        let y = place.ruler.top() + RULER_HEIGHT / 2.0 + 3.0;
        for f in lo..=hi {
            let x = place.x(f as f32) + CELL_WIDTH / 2.0;
            let here = f == current;
            let (weight, ink) = if here { (Weight::Bold, argb(INK_BRIGHT)) } else { (Weight::Regular, argb(INK_DIM)) };
            text(painter, pos2(x, y), Align2::CENTER_CENTER, &(f + 1).to_string(), 11.0, weight, ink);
        }
    }

    /// Row `r`: an empty slot for each of its frames, a block over each clip's, and a mark under
    /// each keyed frame.
    #[allow(clippy::too_many_arguments)]
    fn row(
        &self,
        painter: &egui::Painter,
        place: &Place,
        r: usize,
        row: &TimelineRow,
        panel: &TimelinePanel,
        lo: u32,
        hi: u32,
        paper: Color32,
    ) {
        let top = place.cell_top(r);
        let first = lo.max(row.start);
        let last = hi.min(row.end.saturating_sub(1));
        for f in (first <= last).then_some(first..=last).into_iter().flatten() {
            let cell = Rect::from_min_size(pos2(place.x(f as f32), top), vec2(CELL_WIDTH, CELL_HEIGHT));
            painter.rect_filled(cell, CORNER, argb(SLOT));
        }
        // where a repeating group plays a frame again: its ghost
        for again in row.repeats.iter().filter(|a| a.start <= hi && a.start + a.duration > lo) {
            let ghost = Rect::from_min_size(
                pos2(place.x(again.start as f32), top),
                vec2(again.duration as f32 * PITCH - GAP, CELL_HEIGHT),
            );
            painter.rect_filled(ghost, CORNER, argb(BLOCK).gamma_multiply(0.45));
        }
        let dim = if row.track.hidden || row.hidden_above { 0.45 } else { 1.0 };
        for c in row.track.clips.iter().filter(|c| c.start <= hi && c.start + c.duration > lo) {
            self.block(painter, place, top, c, if c.disabled { 0.4 } else { dim }, lo, hi, paper);
        }
        // a mark under each keyed frame of a block
        let y = place.row_top(r) + (ROW_HEIGHT + CELL_HEIGHT) / 2.0 - 4.5;
        for c in &row.track.clips {
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
    /// the canvas's paper in each frame, the first as it is and the held ones shaded; a group, a
    /// sound or a video is a plain block with its name.
    #[allow(clippy::too_many_arguments)]
    fn block(
        &self,
        painter: &egui::Painter,
        place: &Place,
        top: f32,
        c: &TimelineClipInfo,
        alpha: f32,
        lo: u32,
        hi: u32,
        paper: Color32,
    ) {
        let width = (c.duration as f32 * PITCH - GAP).max(8.0);
        let block = Rect::from_min_size(pos2(place.x(c.start as f32), top), vec2(width, CELL_HEIGHT));
        let tint = c.label.map(label_colour);
        // the frames of it on the bar, counted from its first
        let from = lo.saturating_sub(c.start);
        let to = (hi - c.start.min(hi)).min(c.duration.saturating_sub(1));
        if c.kind == TimelineClipKind::Drawing {
            painter.rect_filled(block, CORNER, faded(paper, alpha));
            if let Some((_, Some(picture))) = self.pictures.get(&c.root) {
                let [w, h] = picture.size().map(|v| v.max(1) as f32);
                let k = (CELL_WIDTH / w).min(CELL_HEIGHT / h);
                let inside = painter.with_clip_rect(block.intersect(painter.clip_rect()));
                for f in from..=to {
                    let cell =
                        Rect::from_min_size(pos2(block.left() + f as f32 * PITCH, top), vec2(CELL_WIDTH, CELL_HEIGHT));
                    let shown = Rect::from_center_size(cell.center(), vec2(w * k, h * k));
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    inside.image(picture.id(), shown, uv, faded(Color32::WHITE, alpha));
                }
            }
            // held past its first frame
            if width > CELL_WIDTH + 1.0 {
                let held = Rect::from_x_y_ranges(block.left() + CELL_WIDTH..=block.right(), block.y_range());
                let corners = CornerRadius { nw: 0, sw: 0, ne: CORNER, se: CORNER };
                painter.rect_filled(held, corners, faded(argb(HELD_SHADE), alpha));
            }
        } else {
            painter.rect_filled(block, CORNER, faded(tint.unwrap_or_else(|| argb(BLOCK)), alpha));
            // (its name follows the bar's edge no further than its own first frame)
            let x = block.left() + from as f32 * PITCH + 7.0;
            let named = painter.with_clip_rect(block.intersect(painter.clip_rect()));
            let ink = if tint.is_some() { argb(ON_LABEL) } else { argb(INK_BRIGHT) };
            text(
                &named,
                pos2(x, block.center().y),
                Align2::LEFT_CENTER,
                &c.name,
                12.0,
                Weight::Regular,
                faded(ink, alpha),
            );
        }
    }
}

/// Where the header's buttons are: Play (its word `words` wide), with Animate on (`autokey`) the
/// record button, a frame back and on; then the settings, Animate, a new track and a new frame.
fn header_buttons(place: &Place, words: f32, autokey: bool) -> Vec<(Button, Rect)> {
    let y = place.header.top() + 4.0 + 20.0;
    let play = Rect::from_min_size(pos2(place.bar.left() + 26.0, y - 20.0), vec2(words.max(54.0), 40.0));
    let record = Rect::from_min_size(pos2(play.right(), y - 20.0), vec2(40.0, 40.0));
    let after = if autokey { record.right() } else { play.right() };
    let back = Rect::from_min_size(pos2(after + 18.0, y - 20.0), vec2(50.0, 40.0));
    let add = Rect::from_min_size(pos2(place.bar.right() - 16.0 - 50.0, y - 20.0), vec2(50.0, 40.0));
    let mut all = vec![(Button::Play, play)];
    if autokey {
        all.push((Button::Record, record));
    }
    all.extend([
        (Button::Back, back),
        (Button::On, back.translate(vec2(50.0, 0.0))),
        (Button::Settings, add.translate(vec2(-150.0, 0.0))),
        (Button::Animate, add.translate(vec2(-100.0, 0.0))),
        (Button::Track, add.translate(vec2(-50.0, 0.0))),
        (Button::Add, add),
    ]);
    all
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

    /// The middle of the header's button `name`: Play, Back, On, Settings, Track or Add.
    pub(crate) fn button_centre(&self, ctx: &egui::Context, name: &str) -> Pos2 {
        let place = self.place(ctx);
        let found =
            header_buttons(&place, 0.0, self.animate.autokey).into_iter().find(|(b, _)| format!("{b:?}") == name);
        found.map_or(Pos2::ZERO, |(_, r)| r.center())
    }

    /// The middle of row `r`'s track's name.
    pub(crate) fn name_centre(&self, ctx: &egui::Context, r: usize) -> Pos2 {
        let place = self.place(ctx);
        pos2(place.bar.left() + 12.0 + 6.0 + 26.0 + 30.0, place.row_top(r) + ROW_HEIGHT / 2.0)
    }

    /// The middle of row `r`'s chevron after its track's name (its rows of keys out or away),
    /// where its blocks have keys.
    pub(crate) fn chevron_centre(&self, ctx: &egui::Context, r: usize) -> Option<Pos2> {
        let place = self.place(ctx);
        let panel = self.panel.as_ref()?;
        let row = rows_of(panel).get(r).copied()?;
        let painter = ctx.layer_painter(egui::LayerId::background());
        label_place(&painter, &place, r, row, super::keys::keyed(panel, row)).chevron.map(|c| c.center())
    }

    /// The rows of keys out, top first: the row each is under, and its name.
    pub(crate) fn key_rows(&self, ctx: &egui::Context) -> Vec<(usize, &'static str)> {
        let place = self.place(ctx);
        place
            .lines
            .iter()
            .filter_map(|(l, _, _)| match l {
                Line::Keys(r, kind) => Some((*r, kind.said())),
                Line::Track(_) => None,
            })
            .collect()
    }

    /// The middle of the row of keys `name` under row `r`, over frame `f` (on its name, `f`
    /// none).
    pub(crate) fn key_centre(&self, ctx: &egui::Context, r: usize, name: &str, f: Option<u32>) -> Option<Pos2> {
        let place = self.place(ctx);
        let (_, top, height) = place.lines.iter().find(|(l, _, _)| match l {
            Line::Keys(at, kind) => *at == r && kind.said() == name,
            Line::Track(_) => false,
        })?;
        let x = f.map_or(place.bar.left() + 70.0, |f| place.x(f as f32) + CELL_WIDTH / 2.0);
        Some(pos2(x, place.top + top + height / 2.0))
    }

    /// The middle of the grip before the keys picked (`leading`) or after them, where they span
    /// two frames or more.
    pub(crate) fn grip_centre(&self, ctx: &egui::Context, leading: bool) -> Option<Pos2> {
        let place = self.place(ctx);
        let g = self.grips(&place)?;
        Some(g.rect(&place, leading).center())
    }

    /// Whether it has a timeline to show.
    pub(crate) fn showing(&self) -> bool {
        self.info().is_some()
    }

    /// The frame under the playhead as shown.
    pub(crate) fn shown_at(&self) -> f32 {
        self.motion.at
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

/// Where row `r`'s track's name stands: its capsule, its name (as it fits), and where its blocks
/// have keys (`keyed`), the chevron after it.
struct LabelPlace {
    capsule: Rect,
    name: String,
    chevron: Option<Rect>,
}

fn label_place(painter: &egui::Painter, place: &Place, r: usize, row: &TimelineRow, keyed: bool) -> LabelPlace {
    let t = &row.track;
    let lock = if t.locked { 20.0 } else { 0.0 };
    let chevron = if keyed { 22.0 } else { 0.0 };
    let most = LABELS_WIDTH - 34.0 - 6.0 - 26.0 - 6.0 - 14.0 - lock - chevron;
    let name = fitted(painter, &t.name, most);
    let words = text_width(painter, &name, 14.0, Weight::Medium);
    let left = place.bar.left() + 12.0 + row.depth as f32 * 14.0;
    let end = if keyed { 6.0 } else { 14.0 };
    let width = 6.0 + 26.0 + 6.0 + words + lock + chevron + end;
    let middle = place.row_top(r) + ROW_HEIGHT / 2.0;
    let capsule = Rect::from_min_size(pos2(left, middle - 16.0), vec2(width, 32.0));
    let chevron =
        keyed.then(|| Rect::from_min_size(pos2(capsule.right() - end - chevron, middle - 15.0), vec2(chevron, 30.0)));
    LabelPlace { capsule, name, chevron }
}

/// Row `r`'s track's name in its capsule (the chosen track's lit), with its box, standing still
/// over the frames; where its blocks have keys, a chevron after it (down while its rows of keys
/// are out: `keyed` says whether they are).
#[allow(clippy::too_many_arguments)]
fn track_label(
    ui: &Ui,
    look: &mut Look,
    painter: &egui::Painter,
    place: &Place,
    r: usize,
    row: &TimelineRow,
    chosen: bool,
    keyed: Option<bool>,
) {
    let t = &row.track;
    let label = t.label.map(label_colour);
    let ink = match (label, chosen) {
        (Some(_), _) => argb(ON_LABEL),
        (None, true) => argb(INK_BRIGHT),
        (None, false) => argb(INK),
    };
    let alpha = if t.hidden { 0.6 } else { 1.0 };
    let LabelPlace { capsule, name, chevron } = label_place(painter, place, r, row, keyed.is_some());
    let (left, middle) = (capsule.left(), capsule.center().y);
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
    if let (Some(c), Some(out)) = (chevron, keyed) {
        let icon = if out { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT };
        let tint = if out { argb(pc::ACCENT) } else { faded(ink, alpha) };
        if painter.clip_rect().contains(c.center()) {
            look.icon_on(ui, painter, icon, c.center(), 13.0, tint);
        }
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

//! The timeline's settings, as the iPad's card (ios/ProCraft/TimelineGridSettings.swift): a card
//! with a tab along its bottom for each part, and what a row opens standing beside it.
//!
//! - **Project**: the frames per second (by name, or typed on the card's own keys, and dragged on
//!   the slider under them), "Keep timing when FPS changes" (the timing kept by moving every
//!   frame, once, when the slider is let go), the length (typed, or fitted to the frames), and the
//!   canvas's size, which opens Crop & Resize ([`Said::Crop`]).
//! - **Onion**: shown or not, each side's count, the opacity, each side's tint.
//! - **Timeline**: Loop, Ping Pong, One Shot; frames per new drawing; Perform smoothing.
//! - **Prefs**: the timecode; Sound while scrubbing; New frames are vector layers.
//!
//! Keep timing, the timecode, Sound while scrubbing, New frames are vector layers and Perform
//! smoothing are the app's settings, not the artwork's: kept in [`Prefs`] (`timeline_*`), as the
//! iPad keeps them in its defaults ([`Said::Prefs`] asks for them to be kept). New frames are
//! vector layers goes to the session as it changes, and [`Prefs::apply`] gives it to each canvas
//! opened. The timeline's grid reads the others from [`Prefs`]: `timeline_timecode` (the timecode
//! over its corner), `timeline_scrub_sound` (off: scrubbing is silent, playing still sounds) and
//! `timeline_perform_smoothing` (how much a performed take is evened out, 0..1).
//!
//! **Opening it.** Actions → Canvas → Timeline Settings ([`crate::actions::Act::TimelineSettings`]),
//! or the grid's header button: it calls [`TimelineSettingsCard::toggle`] on the interface's
//! `timeline_settings` field, having set [`TimelineSettingsCard::anchor`] to the button's rect (the
//! card then stands over the button's right end, and a press on the button doesn't count as a
//! press outside the card, which closes it). Without an anchor the card stands over the window's
//! bottom right corner. It shows only while the canvas has a timeline, and closes on Escape.

use egui::{Align2, Color32, Id, Key, Order, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use procraft_core::{AnimationPlayback, CanvasSession, TimelineInfo, TimelineOnion, TimelineSettings};

use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::panel::PANEL_TITLE;
use crate::prefs::Prefs;

/// The card's width, the beside cards' width, and the gap between them.
pub(crate) const WIDTH: f32 = 318.0;
pub(crate) const BESIDE_WIDTH: f32 = 236.0;
const BESIDE_GAP: f32 = 6.0;
/// The title's band over the rows, the rows' inset from the card's sides, the gap between their
/// sections, a row's height, a slider row's, and the bare frame rate slider's.
pub(crate) const TITLE_BAND: f32 = 50.0;
pub(crate) const INSET: f32 = 10.0;
pub(crate) const SECTION_GAP: f32 = 8.0;
pub(crate) const ROW: f32 = 44.0;
pub(crate) const SLIDER_ROW: f32 = 55.0;
pub(crate) const BARE_SLIDER: f32 = 36.0;
/// Above a group of slider rows, and under it.
pub(crate) const SLIDERS_TOP: f32 = 4.0;
const SLIDERS_BOTTOM: f32 = 6.0;
/// The tabs' band at the bottom; the most the rows take (fewer on a short window).
pub(crate) const TABS: f32 = 58.0;
const MOST_CONTENT: f32 = 322.0;
const LEAST_CONTENT: f32 = 200.0;
/// The side capsules (Backwards, Forwards) and the onion swatches' row.
pub(crate) const SIDES: f32 = 34.0;
const SWATCHES: f32 = 40.0;
/// A beside card's margin; the frame rates' rows (Custom's, then the named ones'); a keypad's
/// number line, its keys and their gaps.
pub(crate) const BESIDE_MARGIN: f32 = 8.0;
pub(crate) const CUSTOM_ROW: f32 = 40.0;
pub(crate) const RATE_ROW: f32 = 36.0;
pub(crate) const NUMBER_LINE: f32 = 34.0;
pub(crate) const KEY: f32 = 40.0;
pub(crate) const KEY_GAP: f32 = 6.0;
/// Off the window's edges, and over the anchor.
const MARGIN: f32 = 16.0;
const OVER_ANCHOR: f32 = 8.0;

const CARD_FILL: Themed = Themed(0xFF2C2C2C, 0xFFEDEDED);
const CARD_EDGE: Themed = Themed(0xFF3C3C3C, 0xFFD6D6D6);
const SECTION_FILL: Themed = Themed(0x12FFFFFF, 0xFFE1E1E1);
const INK_DIM: Themed = Themed(0xFF8E8E8E, 0xFF7A7A7A);
const KEY_FILL: Themed = Themed(0x1AFFFFFF, 0xFFD6D6D6);
const LIT_ROW: Themed = Themed(0x1FFFFFFF, 0xFFD2D2D2);
const LINE: Themed = Themed(0x14FFFFFF, 0xFFD0D0D0);
const SLIDER_TRACK: Themed = Themed(0xFF414141, 0xFFBEBEBE);
const BADGE_INK: Themed = Themed(0xFF2C2C2C, 0xFFEDEDED);

/// The frame rates offered by name.
pub(crate) const FRAME_RATES: [(&str, u32); 7] = [
    ("Frame by frame", 12),
    ("Anime", 15),
    ("Cinema", 24),
    ("Television", 30),
    ("PAL", 50),
    ("NTSC", 60),
    ("Mobile", 120),
];

/// The onion skins' tints (none: they show in their own colours).
pub(crate) const ONION_COLOURS: [(&str, u32); 5] =
    [("Red", 0xE63333), ("Orange", 0xE68A1A), ("Green", 0x339933), ("Blue", 0x3366E6), ("Purple", 0x9933CC)];
/// A side's tint as it is first turned on: red before, green after.
const BEFORE_TINT: u32 = 0xE63333;
const AFTER_TINT: u32 = 0x339933;

/// The most of each the settings take (the core's own).
pub(crate) const MOST_FRAME_RATE: u32 = 120;
pub(crate) const MOST_HELD: u32 = 12;
pub(crate) const MOST_SKINS: u32 = 12;
pub(crate) const MOST_LENGTH: u32 = 100_000;
/// The most digits typed on the keys.
const MOST_DIGITS: usize = 6;

/// The card's tabs.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Tab {
    #[default]
    Project,
    Onion,
    Timeline,
    Prefs,
}

const TAB_LIST: [(Tab, &str, &Icon); 4] = [
    (Tab::Project, "Project", &icons::INFO_DIMENSIONS),
    (Tab::Onion, "Onion", &icons::INFO_LAYERS),
    (Tab::Timeline, "Timeline", &icons::INFO_VIDEO),
    (Tab::Prefs, "Prefs", &icons::TAB_PREFS),
];

/// What stands beside the card: the frame rates by name, the frame rate's keys, the length's.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Beside {
    Rates,
    Rate,
    Length,
}

/// What the card asks of the app.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Said {
    /// The canvas's size tapped: the card has closed, Crop & Resize opens.
    Crop,
    /// One of the app's timeline settings in [`Prefs`] changed: it is kept.
    Prefs,
}

/// A length of `frames` at `fps` as minutes and seconds.
pub fn clock(frames: u32, fps: u32) -> String {
    let seconds = frames / fps.max(1);
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

/// Frame `frame` at `fps` as a timecode: hours, minutes, seconds, and the frame within the
/// second (as the grid shows it over its corner while Prefs → Timecode is on).
#[allow(dead_code)] // (the grid's, which isn't on this branch)
pub fn timecode(frame: u32, fps: u32) -> String {
    let fps = fps.max(1);
    let seconds = frame / fps;
    format!("{:02}:{:02}:{:02}.{:03}", seconds / 3600, seconds / 60 % 60, seconds % 60, frame % fps)
}

/// The timeline's settings card.
#[derive(Debug)]
pub struct TimelineSettingsCard {
    open: bool,
    /// Where the button that opens it shows (window points): the card stands over its right end,
    /// and a press on it isn't a press outside the card. None: over the window's bottom right.
    pub anchor: Option<Rect>,
    tab: Tab,
    beside: Option<Beside>,
    /// The onion skins shown in its sliders: those before the drawing, or those after.
    after: bool,
    /// The frame rate as its slider is dragged with the timing kept, before it is let go.
    rate: Option<u32>,
    /// What is typed on the keys beside it so far.
    typed: String,
    /// The onion skins' counts each side, as they were when last turned off.
    onion_kept: (u32, u32),
}

impl Default for TimelineSettingsCard {
    fn default() -> TimelineSettingsCard {
        TimelineSettingsCard {
            open: false,
            anchor: None,
            tab: Tab::default(),
            beside: None,
            after: false,
            rate: None,
            typed: String::new(),
            onion_kept: (1, 1),
        }
    }
}

/// A slider's touch this frame: where it was set (0..1) while held or tapped, and whether it is
/// held.
struct Slid {
    at: Option<f32>,
    held: bool,
}

// (the grid's header button, which isn't on this branch, calls `toggle`; the others are its too)
#[allow(dead_code)]
impl TimelineSettingsCard {
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Opens on the tab it was left on, nothing beside it.
    pub fn open(&mut self) {
        self.open = true;
        self.set_beside(None);
    }

    pub fn close(&mut self) {
        self.open = false;
        self.set_beside(None);
        self.rate = None;
    }

    /// Opens it, or closes it if open (the grid's header button).
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open();
        }
    }

    /// The tab shown.
    pub fn tab(&self) -> Tab {
        self.tab
    }

    /// What stands beside it.
    pub fn beside(&self) -> Option<Beside> {
        self.beside
    }

    /// Its keys are out: the keyboard's digits, Backspace and Enter type on them.
    pub fn takes_keys(&self) -> bool {
        self.open && matches!(self.beside, Some(Beside::Rate | Beside::Length))
    }

    /// Where it shows in `screen`: the card, what stands beside it, and how tall its rows are.
    pub(crate) fn placed(&self, screen: Rect) -> (Rect, Option<Rect>, f32) {
        let content = (screen.height() - 170.0).clamp(LEAST_CONTENT, MOST_CONTENT);
        let height = TITLE_BAND + content + TABS + BESIDE_MARGIN;
        let corner = match self.anchor {
            Some(button) => pos2(button.right(), button.top() - OVER_ANCHOR),
            None => pos2(screen.right() - MARGIN, screen.bottom() - MARGIN),
        };
        let card = Rect::from_min_max(corner - vec2(WIDTH, height), corner);
        // kept on the window
        let shift =
            vec2((screen.left() + MARGIN - card.left()).max(0.0), (screen.top() + MARGIN - card.top()).max(0.0));
        let card = card.translate(shift);
        let beside = self.beside.map(|b| {
            Rect::from_min_max(
                pos2(card.left() - BESIDE_GAP - BESIDE_WIDTH, card.bottom() - beside_height(b)),
                pos2(card.left() - BESIDE_GAP, card.bottom()),
            )
        });
        (card, beside, content)
    }

    fn set_beside(&mut self, beside: Option<Beside>) {
        self.beside = beside;
        self.typed.clear();
    }

    /// Shows it while it is open and the canvas has a timeline (closing it otherwise).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        prefs: &mut Prefs,
    ) -> Option<Said> {
        if !self.open {
            return None;
        }
        let Some(info) = session.timeline_info() else {
            self.close();
            return None;
        };
        let (card, beside_rect, content) = self.placed(ctx.content_rect());
        // a press outside it (and off its button) closes it; so does Escape, what is beside it
        // first
        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| {
                    !card.contains(p)
                        && !beside_rect.is_some_and(|b| b.contains(p))
                        && !self.anchor.is_some_and(|a| a.contains(p))
                })
        });
        if pressed_outside {
            self.close();
            return None;
        }
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            if self.beside.is_some() {
                self.set_beside(None);
            } else {
                self.close();
                return None;
            }
        }
        let mut said = None;
        egui::Area::new(Id::unique("timeline settings")).fixed_pos(card.min).order(Order::Foreground).show(ctx, |ui| {
            ui.allocate_rect(card, Sense::click_and_drag());
            card_ground(ui, card);
            text(
                ui.painter(),
                pos2(card.left() + 18.0, card.top() + 14.0),
                Align2::LEFT_TOP,
                "Timeline Settings",
                22.0,
                Weight::Bold,
                argb(PANEL_TITLE),
            );
            let rows = Rect::from_min_size(
                pos2(card.left() + INSET, card.top() + TITLE_BAND),
                vec2(WIDTH - 2.0 * INSET, content),
            );
            said = match self.tab {
                Tab::Project => self.project(ui, session, &info, prefs, rows),
                Tab::Onion => {
                    self.onion(ui, session, &info, rows);
                    None
                }
                Tab::Timeline => self.timeline(ui, session, &info, prefs, rows),
                Tab::Prefs => self.prefs(ui, session, prefs, rows),
            };
            let tabs = Rect::from_min_size(pos2(card.left() + 8.0, rows.bottom()), vec2(WIDTH - 16.0, TABS));
            self.tabs(ui, look, tabs);
        });
        if said == Some(Said::Crop) {
            self.close();
            return said;
        }
        if let (Some(beside), Some(rect)) = (self.beside, beside_rect) {
            egui::Area::new(Id::unique("timeline settings beside")).fixed_pos(rect.min).order(Order::Foreground).show(
                ctx,
                |ui| {
                    ui.allocate_rect(rect, Sense::click_and_drag());
                    card_ground(ui, rect);
                    let inner = rect.shrink(BESIDE_MARGIN);
                    match beside {
                        Beside::Rates => self.rates(ui, session, &info, prefs, inner),
                        Beside::Rate => {
                            if let Some(n) = self.keypad(ui, inner, "FPS", info.fps, MOST_FRAME_RATE, false, info.fps) {
                                set_rate(session, &info, prefs, n);
                                self.set_beside(None);
                            }
                        }
                        Beside::Length => {
                            if let Some(n) =
                                self.keypad(ui, inner, "frames", info.duration, MOST_LENGTH, true, info.fps)
                            {
                                session.set_timeline_length(n);
                                self.set_beside(None);
                            }
                            let line_y = inner.top() + keypad_height();
                            ui.painter().rect_filled(
                                Rect::from_min_max(pos2(inner.left(), line_y), pos2(inner.right(), line_y + 1.0)),
                                0,
                                argb(LINE),
                            );
                            let fit = Rect::from_min_size(pos2(inner.left(), line_y + 1.0), vec2(inner.width(), KEY));
                            if menu_row(ui, fit, "Fit to content", None, false, "fit") {
                                session.fit_timeline_to_content();
                                self.set_beside(None);
                            }
                        }
                    }
                },
            );
        }
        if self.takes_keys() {
            self.hardware_keys(ctx, session, &info, prefs);
        }
        said
    }

    /// The keyboard's digits, Backspace and Enter on the keys beside the card.
    fn hardware_keys(&mut self, ctx: &egui::Context, session: &CanvasSession, info: &TimelineInfo, prefs: &Prefs) {
        const DIGITS: [Key; 10] = [
            Key::Num0,
            Key::Num1,
            Key::Num2,
            Key::Num3,
            Key::Num4,
            Key::Num5,
            Key::Num6,
            Key::Num7,
            Key::Num8,
            Key::Num9,
        ];
        let (digits, back, enter) = ctx.input(|i| {
            let digits: Vec<u32> =
                DIGITS.iter().zip(0u32..).filter(|(k, _)| i.key_pressed(**k)).map(|(_, n)| n).collect();
            (digits, i.key_pressed(Key::Backspace), i.key_pressed(Key::Enter))
        });
        for n in digits {
            self.press(n);
        }
        if back {
            self.typed.pop();
        }
        if enter {
            let rate = self.beside == Some(Beside::Rate);
            let most = if rate { MOST_FRAME_RATE } else { MOST_LENGTH };
            if let Some(n) = self.typed.parse::<u32>().ok().filter(|n| (1..=most).contains(n)) {
                if rate {
                    set_rate(session, info, prefs, n);
                } else {
                    session.set_timeline_length(n);
                }
                self.set_beside(None);
            }
        }
    }

    /// A digit typed: no more than six, and none starting with 0.
    fn press(&mut self, n: u32) {
        if self.typed.len() < MOST_DIGITS && !(self.typed.is_empty() && n == 0) {
            self.typed.push_str(&n.to_string());
        }
    }

    fn tabs(&mut self, ui: &Ui, look: &mut Look, band: Rect) {
        let cell_width = band.width() / TAB_LIST.len() as f32;
        for (i, (tab, label, icon)) in TAB_LIST.into_iter().enumerate() {
            let cell =
                Rect::from_min_size(pos2(band.left() + cell_width * i as f32, band.top()), vec2(cell_width, TABS));
            let chosen = tab == self.tab;
            let ink = if chosen { argb(pc::ACCENT) } else { argb(PANEL_TITLE) };
            look.icon(ui, icon, pos2(cell.center().x, cell.top() + 20.0), 30.0, ink);
            text(
                ui.painter(),
                pos2(cell.center().x, cell.top() + 37.0),
                Align2::CENTER_TOP,
                label,
                11.5,
                Weight::Regular,
                ink,
            );
            if ui.interact(cell, ui.scope_id().with(("tab", i)), Sense::click()).clicked() {
                self.tab = tab;
                self.set_beside(None);
            }
        }
    }

    fn project(
        &mut self,
        ui: &Ui,
        session: &CanvasSession,
        info: &TimelineInfo,
        prefs: &mut Prefs,
        rows: Rect,
    ) -> Option<Said> {
        let (left, right) = (rows.left(), rows.right());
        let mut y = rows.top();
        let first = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW + BARE_SLIDER + ROW));
        section(ui, first);
        // the frame rate: the row opens the named ones; the slider takes any
        let fps = self.rate.unwrap_or(info.fps);
        let lit = matches!(self.beside, Some(Beside::Rates | Beside::Rate));
        let row = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW));
        if value_row(ui, row, "Frames per second", &fps.to_string(), lit, "fps") {
            self.set_beside(if lit { None } else { Some(Beside::Rates) });
        }
        y += ROW;
        let bar = Rect::from_min_max(pos2(left, y), pos2(right, y + BARE_SLIDER));
        let fraction = (fps.max(1) - 1) as f32 / (MOST_FRAME_RATE - 1) as f32;
        let slid = slider(ui, bar, None, fraction, "fps slider");
        if let Some(at) = slid.at {
            let to = 1 + (at * (MOST_FRAME_RATE - 1) as f32).round() as u32;
            if prefs.timeline_keep_timing {
                // (the timing is kept by moving every frame: once, when it is let go)
                self.rate = Some(to);
            } else if to != info.fps {
                change(session, info, |s| s.fps = to);
            }
        }
        if !slid.held
            && let Some(to) = self.rate.take()
        {
            set_rate(session, info, prefs, to);
        }
        y += BARE_SLIDER;
        // the length: under a second, its frames say more
        let length =
            if info.duration < info.fps { format!("{} frames", info.duration) } else { clock(info.duration, info.fps) };
        let row = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW));
        let lit = self.beside == Some(Beside::Length);
        if value_row(ui, row, "Duration", &length, lit, "duration") {
            self.set_beside(if lit { None } else { Some(Beside::Length) });
        }
        y += ROW + SECTION_GAP;
        let mut said = None;
        let row = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW));
        section(ui, row);
        if toggle_row(ui, row, "Keep timing when FPS changes", prefs.timeline_keep_timing) {
            prefs.timeline_keep_timing = !prefs.timeline_keep_timing;
            said = Some(Said::Prefs);
        }
        y += ROW + SECTION_GAP;
        // (changing the canvas's size is Crop & Resize's work)
        let row = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW));
        section(ui, row);
        let size = session.info();
        if value_row(ui, row, "Resolution", &format!("{} × {}", size.width, size.height), false, "resolution") {
            said = Some(Said::Crop);
        }
        said
    }

    fn onion(&mut self, ui: &Ui, session: &CanvasSession, info: &TimelineInfo, rows: Rect) {
        let o =
            session.timeline_onion().unwrap_or(TimelineOnion { before: 0, after: 0, before_color: 0, after_color: 0 });
        let shown = o.before > 0 || o.after > 0;
        let (count, colour) = if self.after { (o.after, o.after_color) } else { (o.before, o.before_color) };
        let (left, right) = (rows.left(), rows.right());
        let mut y = rows.top();
        let row = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW));
        section(ui, row);
        if toggle_row(ui, row, "Onion skin visibility", shown) {
            self.toggle_onion(session, o);
        }
        y += ROW + SECTION_GAP;
        // the side the sliders and tint below are for
        let half = (rows.width() - 4.0) / 2.0;
        for (i, (label, after)) in [("Backwards", false), ("Forwards", true)].into_iter().enumerate() {
            let capsule = Rect::from_min_size(pos2(left + (half + 4.0) * i as f32, y), vec2(half, SIDES));
            let chosen = after == self.after;
            let fill = if chosen { argb(pc::ACCENT) } else { argb(SECTION_FILL) };
            ui.painter().rect_filled(capsule, SIDES / 2.0, fill);
            let ink = if chosen { Color32::WHITE } else { argb(PANEL_TITLE) };
            let weight = if chosen { Weight::Medium } else { Weight::Regular };
            text(ui.painter(), capsule.center(), Align2::CENTER_CENTER, label, 14.0, weight, ink);
            if ui.interact(capsule, ui.scope_id().with(("side", i)), Sense::click()).clicked() {
                self.after = after;
            }
        }
        y += SIDES + SECTION_GAP;
        let group = Rect::from_min_max(pos2(left, y), pos2(right, y + sliders_height(2)));
        section(ui, group);
        y += SLIDERS_TOP;
        let row = Rect::from_min_max(pos2(left, y), pos2(right, y + SLIDER_ROW));
        let slid = slider(ui, row, Some(("Frames", count.to_string())), count as f32 / MOST_SKINS as f32, "skins");
        if let Some(at) = slid.at {
            let n = (at * MOST_SKINS as f32).round() as u32;
            if n != count {
                let mut to = o;
                if self.after {
                    to.after = n;
                } else {
                    to.before = n;
                }
                session.set_timeline_onion(to);
            }
        }
        y += SLIDER_ROW;
        let row = Rect::from_min_max(pos2(left, y), pos2(right, y + SLIDER_ROW));
        let opacity = info.onion_opacity.clamp(0.0, 1.0);
        let shown_opacity = format!("{}%", (opacity * 100.0).round());
        if let Some(at) = slider(ui, row, Some(("Opacity", shown_opacity)), opacity, "opacity").at
            && at != info.onion_opacity
        {
            change(session, info, |s| s.onion_opacity = at);
        }
        y += SLIDER_ROW + SLIDERS_BOTTOM + SECTION_GAP;
        // tinted off, the skins show in their own colours
        let tall = if colour != 0 { ROW + SWATCHES } else { ROW };
        section(ui, Rect::from_min_max(pos2(left, y), pos2(right, y + tall)));
        let row = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW));
        let tint = |rgb: u32| {
            let mut to = o;
            if self.after {
                to.after_color = rgb;
            } else {
                to.before_color = rgb;
            }
            session.set_timeline_onion(to);
        };
        if toggle_row(ui, row, "Color tint", colour != 0) {
            tint(if colour != 0 {
                0
            } else if self.after {
                AFTER_TINT
            } else {
                BEFORE_TINT
            });
        }
        if colour != 0 {
            y += ROW;
            let cell = (rows.width() - 16.0) / ONION_COLOURS.len() as f32;
            for (i, (_, rgb)) in ONION_COLOURS.into_iter().enumerate() {
                let at = Rect::from_min_size(pos2(left + 8.0 + cell * i as f32, y), vec2(cell, SWATCHES));
                ui.painter().circle_filled(at.center(), 11.0, rgb_colour(rgb));
                if colour == rgb {
                    ui.painter().circle_stroke(at.center(), 14.0, Stroke::new(2.0, argb(PANEL_TITLE)));
                }
                if ui.interact(at, ui.scope_id().with(("swatch", i)), Sense::click()).clicked() {
                    tint(rgb);
                }
            }
        }
    }

    /// Onion skins off, or back to as many as each side had (its colours as they were).
    fn toggle_onion(&mut self, session: &CanvasSession, o: TimelineOnion) {
        let mut to = o;
        if o.before > 0 || o.after > 0 {
            self.onion_kept = (o.before, o.after);
            (to.before, to.after) = (0, 0);
        } else {
            (to.before, to.after) = self.onion_kept;
        }
        session.set_timeline_onion(to);
    }

    fn timeline(
        &mut self,
        ui: &Ui,
        session: &CanvasSession,
        info: &TimelineInfo,
        prefs: &mut Prefs,
        rows: Rect,
    ) -> Option<Said> {
        let (left, right) = (rows.left(), rows.right());
        let mut y = rows.top();
        let ways = [
            (AnimationPlayback::Loop, "Loop"),
            (AnimationPlayback::PingPong, "Ping Pong"),
            (AnimationPlayback::OneShot, "One Shot"),
        ];
        section(ui, Rect::from_min_max(pos2(left, y), pos2(right, y + ROW * ways.len() as f32)));
        for (i, (way, label)) in ways.into_iter().enumerate() {
            let row = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW));
            let chosen = info.playback == way;
            text(
                ui.painter(),
                pos2(row.left() + 14.0, row.center().y),
                Align2::LEFT_CENTER,
                label,
                16.0,
                Weight::Regular,
                argb(PANEL_TITLE),
            );
            let ring = pos2(row.right() - 24.0, row.center().y);
            let ring_ink = if chosen { argb(pc::ACCENT) } else { argb(INK_DIM) };
            ui.painter().circle_stroke(ring, 9.25, Stroke::new(1.5, ring_ink));
            if chosen {
                ui.painter().circle_filled(ring, 6.0, argb(pc::ACCENT));
            }
            if ui.interact(row, ui.scope_id().with(("playback", i)), Sense::click()).clicked() && !chosen {
                change(session, info, |s| s.playback = way);
            }
            y += ROW;
        }
        y += SECTION_GAP;
        section(ui, Rect::from_min_max(pos2(left, y), pos2(right, y + sliders_height(1))));
        let row = Rect::from_min_max(pos2(left, y + SLIDERS_TOP), pos2(right, y + SLIDERS_TOP + SLIDER_ROW));
        let held = info.exposure.clamp(1, MOST_HELD);
        let fraction = (held - 1) as f32 / (MOST_HELD - 1) as f32;
        if let Some(at) = slider(ui, row, Some(("Frames per new drawing", held.to_string())), fraction, "held").at {
            let to = 1 + (at * (MOST_HELD - 1) as f32).round() as u32;
            if to != info.exposure {
                change(session, info, |s| s.exposure = to);
            }
        }
        y += sliders_height(1) + SECTION_GAP;
        // (none: a performed move is kept as it was made, corners and all)
        section(ui, Rect::from_min_max(pos2(left, y), pos2(right, y + sliders_height(1))));
        let row = Rect::from_min_max(pos2(left, y + SLIDERS_TOP), pos2(right, y + SLIDERS_TOP + SLIDER_ROW));
        let smoothing = prefs.timeline_perform_smoothing.clamp(0.0, 1.0);
        let shown = format!("{}%", (smoothing * 100.0).round());
        if let Some(at) = slider(ui, row, Some(("Perform smoothing", shown)), smoothing, "smoothing").at
            && at != prefs.timeline_perform_smoothing
        {
            prefs.timeline_perform_smoothing = at;
            return Some(Said::Prefs);
        }
        None
    }

    fn prefs(&mut self, ui: &Ui, session: &CanvasSession, prefs: &mut Prefs, rows: Rect) -> Option<Said> {
        let (left, right) = (rows.left(), rows.right());
        let mut said = None;
        let settings = [
            "Timecode",
            // (off, moving the playhead by hand is silent; playing still sounds)
            "Sound while scrubbing",
            "New frames are vector layers",
        ];
        for (i, label) in settings.into_iter().enumerate() {
            let y = rows.top() + (ROW + SECTION_GAP) * i as f32;
            let row = Rect::from_min_max(pos2(left, y), pos2(right, y + ROW));
            section(ui, row);
            let on = match i {
                0 => prefs.timeline_timecode,
                1 => prefs.timeline_scrub_sound,
                _ => prefs.timeline_vector_frames,
            };
            if toggle_row(ui, row, label, on) {
                match i {
                    0 => prefs.timeline_timecode = !on,
                    1 => prefs.timeline_scrub_sound = !on,
                    _ => {
                        prefs.timeline_vector_frames = !on;
                        session.set_timeline_vector_frames(!on);
                    }
                }
                said = Some(Said::Prefs);
            }
        }
        said
    }

    /// The frame rates by name, and Custom, which opens the keys to type one in.
    fn rates(&mut self, ui: &Ui, session: &CanvasSession, info: &TimelineInfo, prefs: &Prefs, inner: Rect) {
        let (left, right) = (inner.left(), inner.right());
        let mut y = inner.top();
        let custom = Rect::from_min_max(pos2(left, y), pos2(right, y + CUSTOM_ROW));
        if menu_row(ui, custom, "Custom", Some(info.fps), true, "custom") {
            self.set_beside(Some(Beside::Rate));
            return;
        }
        y += CUSTOM_ROW;
        ui.painter().rect_filled(Rect::from_min_max(pos2(left, y), pos2(right, y + 1.0)), 0, argb(LINE));
        y += 1.0;
        for (i, (name, fps)) in FRAME_RATES.into_iter().enumerate() {
            let row = Rect::from_min_max(pos2(left, y), pos2(right, y + RATE_ROW));
            if info.fps == fps {
                ui.painter().rect_filled(row, 12, argb(LIT_ROW));
            }
            if menu_row(ui, row, name, Some(fps), false, ("rate", i)) {
                set_rate(session, info, prefs, fps);
            }
            y += RATE_ROW;
        }
    }

    /// A number typed on the card's own keys: what is typed so far over them (`from` until one is
    /// typed, with its length in time for a length), and Set to take it, between 1 and `most`.
    fn keypad(&mut self, ui: &Ui, inner: Rect, unit: &str, from: u32, most: u32, timed: bool, fps: u32) -> Option<u32> {
        let number = self.typed.parse::<u32>().ok();
        let valid = number.is_some_and(|n| (1..=most).contains(&n));
        let (left, right) = (inner.left() + 2.0, inner.right() - 2.0);
        let line = inner.top() + NUMBER_LINE / 2.0;
        let (shown, ink) = if self.typed.is_empty() {
            (from.to_string(), argb(INK_DIM))
        } else {
            (self.typed.clone(), argb(PANEL_TITLE))
        };
        let at = text(ui.painter(), pos2(left + 6.0, line), Align2::LEFT_CENTER, &shown, 22.0, Weight::Medium, ink);
        text(
            ui.painter(),
            pos2(at.right() + 5.0, line + 2.0),
            Align2::LEFT_CENTER,
            unit,
            13.0,
            Weight::Regular,
            argb(INK_DIM),
        );
        if timed {
            let note = clock(number.unwrap_or(from), fps);
            text(
                ui.painter(),
                pos2(right - 6.0, line + 2.0),
                Align2::RIGHT_CENTER,
                &note,
                13.0,
                Weight::Regular,
                argb(INK_DIM),
            );
        }
        let keys: [[&str; 3]; 4] = [["7", "8", "9"], ["4", "5", "6"], ["1", "2", "3"], ["0", "⌫", "Set"]];
        let width = (right - left - 2.0 * KEY_GAP) / 3.0;
        let mut set = None;
        for (r, line) in keys.iter().enumerate() {
            let top = inner.top() + NUMBER_LINE + KEY_GAP + (KEY + KEY_GAP) * r as f32;
            for (c, label) in line.iter().enumerate() {
                let key = Rect::from_min_size(pos2(left + (width + KEY_GAP) * c as f32, top), vec2(width, KEY));
                let is_set = *label == "Set";
                let fill = match (is_set, valid) {
                    (true, true) => argb(pc::ACCENT),
                    (true, false) => argb(pc::ACCENT).gamma_multiply(0.35),
                    _ => argb(KEY_FILL),
                };
                ui.painter().rect_filled(key, KEY / 2.0, fill);
                let (ink, size, weight) = if is_set {
                    (Color32::WHITE, 16.0, Weight::Medium)
                } else {
                    (argb(PANEL_TITLE), 19.0, Weight::Regular)
                };
                text(ui.painter(), key.center(), Align2::CENTER_CENTER, label, size, weight, ink);
                if !ui.interact(key, ui.scope_id().with(("key", r, c)), Sense::click()).clicked() {
                    continue;
                }
                match *label {
                    "Set" => set = number.filter(|_| valid),
                    "⌫" => _ = self.typed.pop(),
                    digit => {
                        if let Ok(n) = digit.parse::<u32>() {
                            self.press(n);
                        }
                    }
                }
            }
        }
        set
    }
}

/// The frame rate changed to `fps`: every frame moved to keep the timing, if so set, else the
/// rate alone.
fn set_rate(session: &CanvasSession, info: &TimelineInfo, prefs: &Prefs, fps: u32) {
    if fps == info.fps {
        return;
    }
    if prefs.timeline_keep_timing {
        session.retime_timeline(fps);
    } else {
        change(session, info, |s| s.fps = fps);
    }
}

/// The timeline's settings as `edit` changes them (live: not an undo step).
fn change(session: &CanvasSession, info: &TimelineInfo, edit: impl FnOnce(&mut TimelineSettings)) {
    let mut s = TimelineSettings {
        fps: info.fps,
        duration: info.duration,
        exposure: info.exposure,
        playback: info.playback,
        onion_frames: info.onion_frames,
        onion_opacity: info.onion_opacity,
    };
    edit(&mut s);
    session.set_timeline_settings(s);
}

/// A beside card's height.
pub(crate) fn beside_height(beside: Beside) -> f32 {
    2.0 * BESIDE_MARGIN
        + match beside {
            Beside::Rates => CUSTOM_ROW + 1.0 + RATE_ROW * FRAME_RATES.len() as f32,
            Beside::Rate => keypad_height(),
            Beside::Length => keypad_height() + 1.0 + KEY,
        }
}

/// The keys' height: the number's line over four rows.
pub(crate) fn keypad_height() -> f32 {
    NUMBER_LINE + KEY_GAP + 4.0 * KEY + 3.0 * KEY_GAP + KEY_GAP
}

/// A group of `n` slider rows: a little above the first, and under the last.
pub(crate) fn sliders_height(n: usize) -> f32 {
    SLIDERS_TOP + SLIDER_ROW * n as f32 + SLIDERS_BOTTOM
}

fn rgb_colour(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// The card's ground: rounded, edged.
fn card_ground(ui: &Ui, rect: Rect) {
    ui.painter().rect(rect, 22, argb(CARD_FILL), Stroke::new(1.0, argb(CARD_EDGE)), egui::StrokeKind::Inside);
}

/// A group of rows on one rounded ground.
fn section(ui: &Ui, rect: Rect) {
    ui.painter().rect_filled(rect, 14, argb(SECTION_FILL));
}

/// A setting and what it is (in the accent while what it opens shows); true when tapped.
fn value_row(ui: &Ui, row: Rect, label: &str, value: &str, lit: bool, id: &str) -> bool {
    text(
        ui.painter(),
        pos2(row.left() + 14.0, row.center().y),
        Align2::LEFT_CENTER,
        label,
        16.0,
        Weight::Regular,
        argb(PANEL_TITLE),
    );
    let ink = if lit { argb(pc::ACCENT) } else { argb(PANEL_TITLE) };
    text(
        ui.painter(),
        pos2(row.right() - 14.0, row.center().y),
        Align2::RIGHT_CENTER,
        value,
        16.0,
        Weight::Regular,
        ink,
    );
    ui.interact(row, ui.scope_id().with(("value", id)), Sense::click()).clicked()
}

/// A setting that is on or off, with its switch; true when tapped.
fn toggle_row(ui: &Ui, row: Rect, label: &str, on: bool) -> bool {
    text(
        ui.painter(),
        pos2(row.left() + 14.0, row.center().y),
        Align2::LEFT_CENTER,
        label,
        16.0,
        Weight::Regular,
        argb(PANEL_TITLE),
    );
    crate::transform::switch(ui, pos2(row.right() - 8.0 - 63.0, row.center().y - 14.0), on, ("timeline", label));
    ui.interact(row, ui.scope_id().with(("toggle", label)), Sense::click()).clicked()
}

/// A row of a beside card: its name, and a frame rate after it with "FPS" in a little box (and a
/// chevron, for a row opening more); true when tapped.
fn menu_row(
    ui: &Ui,
    row: Rect,
    label: &str,
    fps: Option<u32>,
    opens: bool,
    id: impl std::hash::Hash + std::fmt::Debug,
) -> bool {
    let y = row.center().y;
    text(
        ui.painter(),
        pos2(row.left() + 10.0, y),
        Align2::LEFT_CENTER,
        label,
        15.0,
        Weight::Regular,
        argb(PANEL_TITLE),
    );
    let mut right = row.right() - 10.0;
    if opens {
        text(ui.painter(), pos2(right, y), Align2::RIGHT_CENTER, "›", 18.0, Weight::Regular, argb(PANEL_TITLE));
        right -= 16.0;
    }
    if let Some(fps) = fps {
        let badge = Rect::from_min_max(pos2(right - 24.0, y - 7.0), pos2(right, y + 7.0));
        ui.painter().rect_filled(badge, 3.5, argb(PANEL_TITLE));
        text(ui.painter(), badge.center(), Align2::CENTER_CENTER, "FPS", 9.0, Weight::Bold, argb(BADGE_INK));
        text(
            ui.painter(),
            pos2(badge.left() - 8.0, y),
            Align2::RIGHT_CENTER,
            &fps.to_string(),
            15.0,
            Weight::Regular,
            argb(PANEL_TITLE),
        );
    }
    ui.interact(row, ui.scope_id().with(("menu row", id)), Sense::click()).clicked()
}

/// A slider in `row`: with a name and value over its bar (a slider row), or the bar alone in the
/// row's middle; the bar blue up to a small white knob. Dragged or tapped along, it goes there.
fn slider(ui: &Ui, row: Rect, named: Option<(&str, String)>, fraction: f32, id: &str) -> Slid {
    let (left, right) = (row.left() + 16.0, row.right() - 16.0);
    let y = match &named {
        Some((label, value)) => {
            let top = row.top() + 6.5;
            text(ui.painter(), pos2(left, top), Align2::LEFT_TOP, label, 16.0, Weight::Regular, argb(PANEL_TITLE));
            text(ui.painter(), pos2(right, top), Align2::RIGHT_TOP, value, 16.0, Weight::Regular, argb(PANEL_TITLE));
            row.top() + 38.5
        }
        None => row.center().y,
    };
    let bar = Rect::from_min_max(pos2(row.left(), y - 11.0), pos2(row.right(), y + 11.0));
    let response = ui.interact(bar, ui.scope_id().with(("slider", id)), Sense::click_and_drag());
    let x = left + fraction.clamp(0.0, 1.0) * (right - left);
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(right, y + 1.5)), 0, argb(SLIDER_TRACK));
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(x, y + 1.5)), 0, argb(pc::ACCENT));
    ui.painter().rect_filled(Rect::from_center_size(pos2(x, y), vec2(16.0, 10.0)), 5, Color32::WHITE);
    let held = response.is_pointer_button_down_on();
    let at = (held || response.clicked())
        .then(|| response.interact_pointer_pos())
        .flatten()
        .map(|p: Pos2| ((p.x - left) / (right - left)).clamp(0.0, 1.0));
    Slid { at, held }
}

#[cfg(test)]
mod tests {
    use egui::{Event, Modifiers, PointerButton, RawInput};

    use super::*;

    /// The card shown on a timeline canvas (no GPU) in a 1200 × 900 window, the mouse clicking at
    /// each of `clicks` (its middle tab first, then each in turn), then `keys` typed; what it said.
    fn drive(
        card: &mut TimelineSettingsCard,
        session: &CanvasSession,
        prefs: &mut Prefs,
        clicks: &[Pos2],
        keys: &[Key],
    ) -> Vec<Said> {
        let (ctx, mut look) = (egui::Context::default(), Look::default());
        crate::look::install_fonts(&ctx);
        let press = |pos, pressed| Event::PointerButton {
            pos,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let key = |key, pressed| Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        };
        let mut steps: Vec<Vec<Event>> = vec![Vec::new(), Vec::new()];
        for at in clicks {
            steps.extend([vec![Event::PointerMoved(*at)], vec![press(*at, true)], vec![press(*at, false)], Vec::new()]);
        }
        for k in keys {
            steps.extend([vec![key(*k, true), key(*k, false)], Vec::new()]);
        }
        let mut said = Vec::new();
        for (i, events) in steps.into_iter().enumerate() {
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 900.0))),
                time: Some(i as f64 / 60.0),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.clone().run_ui(raw, |ui| said.extend(card.show(ui.ctx(), &mut look, session, prefs)));
            output.textures_delta.clear();
        }
        said
    }

    /// The card over the window's bottom right, with no anchor.
    fn card_rect() -> Rect {
        let height = TITLE_BAND + MOST_CONTENT + TABS + BESIDE_MARGIN;
        Rect::from_min_size(pos2(1200.0 - MARGIN - WIDTH, 900.0 - MARGIN - height), vec2(WIDTH, height))
    }

    fn tab_at(t: usize) -> Pos2 {
        let card = card_rect();
        pos2(card.left() + 8.0 + (WIDTH - 16.0) / 4.0 * (t as f32 + 0.5), card.top() + TITLE_BAND + MOST_CONTENT + 20.0)
    }

    /// Prefs: the timecode, Sound while scrubbing and New frames are vector layers are the app's
    /// (each asks to be kept); the last reaches the session at once, a new drawing then a vector
    /// layer. Not on a timeline, the card closes.
    #[test]
    fn prefs_are_the_app_s_and_new_frames_become_vector_layers() {
        let session = CanvasSession::new(200, 150);
        let (mut card, mut prefs) = (TimelineSettingsCard::default(), Prefs::default());
        card.open();
        assert!(drive(&mut card, &session, &mut prefs, &[], &[]).is_empty());
        assert!(!card.is_open(), "no timeline");
        assert!(session.start_timeline());
        card.open();
        let top = card_rect().top() + TITLE_BAND;
        let row = |i: f32| pos2(card_rect().center().x, top + (ROW + SECTION_GAP) * i + ROW / 2.0);
        let said = drive(&mut card, &session, &mut prefs, &[tab_at(3), row(0.0), row(1.0), row(2.0)], &[]);
        assert_eq!(card.tab(), Tab::Prefs);
        assert_eq!(said, vec![Said::Prefs; 3]);
        assert!(prefs.timeline_timecode);
        assert!(!prefs.timeline_scrub_sound);
        assert!(prefs.timeline_vector_frames);
        assert_ne!(session.add_timeline_drawing(), 0);
        let current = session.layers().into_iter().find(|l| l.current).expect("the drawing's layer");
        assert!(current.vector, "a vector layer");
        // the Timeline tab: One Shot, and frames per new drawing at its most
        let said = drive(&mut card, &session, &mut prefs, &[tab_at(2), row(0.0) + vec2(0.0, 2.0 * ROW)], &[]);
        assert!(said.is_empty());
        assert_eq!(session.timeline_info().map(|i| i.playback), Some(AnimationPlayback::OneShot));
        let held = pos2(card_rect().right() - INSET - 16.0, top + 3.0 * ROW + SECTION_GAP + SLIDERS_TOP + 38.5);
        drive(&mut card, &session, &mut prefs, &[held], &[]);
        assert_eq!(session.timeline_info().map(|i| i.exposure), Some(MOST_HELD));
    }

    /// The frame rate typed on the keyboard (Custom's keys out), the timing not kept: the rate
    /// alone changes, the length staying in frames; Escape puts the keys away, then closes it.
    #[test]
    fn a_rate_typed_on_the_keyboard_without_keeping_timing() {
        let session = CanvasSession::new(200, 150);
        assert!(session.start_timeline());
        let before = session.timeline_info().expect("a timeline");
        let mut prefs = Prefs { timeline_keep_timing: false, ..Prefs::default() };
        let mut card = TimelineSettingsCard::default();
        card.open();
        let card_at = card_rect();
        let fps_row = pos2(card_at.center().x, card_at.top() + TITLE_BAND + ROW / 2.0);
        drive(&mut card, &session, &mut prefs, &[fps_row], &[]);
        assert_eq!(card.beside(), Some(Beside::Rates));
        let beside = Rect::from_min_max(
            pos2(card_at.left() - BESIDE_GAP - BESIDE_WIDTH, card_at.bottom() - beside_height(Beside::Rates)),
            pos2(card_at.left() - BESIDE_GAP, card_at.bottom()),
        );
        let custom = pos2(beside.center().x, beside.top() + BESIDE_MARGIN + CUSTOM_ROW / 2.0);
        drive(&mut card, &session, &mut prefs, &[custom], &[Key::Num0, Key::Num4, Key::Num8]);
        assert!(card.takes_keys());
        drive(&mut card, &session, &mut prefs, &[], &[Key::Enter]);
        let info = session.timeline_info().expect("a timeline");
        assert_eq!(info.fps, 48);
        assert_eq!(info.duration, before.duration, "its frames as they were");
        assert_eq!(card.beside(), None);
        drive(&mut card, &session, &mut prefs, &[fps_row], &[Key::Escape]);
        assert_eq!(card.beside(), None);
        assert!(card.is_open());
        drive(&mut card, &session, &mut prefs, &[], &[Key::Escape]);
        assert!(!card.is_open());
    }

    #[test]
    fn clock_and_timecode_read_as_the_ipad_s() {
        assert_eq!(clock(90, 12), "00:07");
        assert_eq!(clock(24 * 75, 24), "01:15");
        assert_eq!(clock(5, 0), "00:05");
        assert_eq!(timecode(24 * 3661 + 5, 24), "01:01:01.005");
    }
}

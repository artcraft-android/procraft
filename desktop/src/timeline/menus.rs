//! The grid's menus, each a row of cards standing on the bar, as the iPad's
//! (ios/ProCraft/TimelineGridMenus.swift, TimelineGridAdd.swift, TimelineGridMedia.swift's block
//! menu and cycle card, TimelineGridSettings.swift's end flap menu): a frame's (held or
//! right-clicked), a block's (a group, a sound or a video), an empty frame's, a track's (its name
//! held or right-clicked, or Track options in another menu), the frames picked together's (one of
//! them held with the lasso on), the end flap's (tapped) and Add new (`+` held). What a row opens
//! stands beside its card. A press anywhere off the cards closes the menu, as Escape does.
//!
//! Each menu is a list of [`Item`]s a card at a time; drawing them gives what was pressed
//! ([`Hit`]), which the menu then does.
//!
//! Left out (media comes later): a sound's level and its key, Mute, Slip, a video's size, its
//! timing track and Separate audio and video; Add new's photo, video and sound.

use egui::{Align2, Color32, FontId, Id, Key, Order, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use procraft_core::{CanvasSession, TimelineClipKind, TimelineMask, TimelineTrackInfo};

use super::grid::{BAR_GAP_ABOVE, Place, label_colour};
use super::{TimelineGrid, grid_icons};
use crate::icons::{self, Icon};
use crate::look::{Look, Themed, Weight, argb, pc, text};

/// A card's usual width, an empty frame's menu's, a block's, the cycle card's.
pub(super) const CARD_WIDTH: f32 = 228.0;
const EMPTY_WIDTH: f32 = 248.0;
const BLOCK_WIDTH: f32 = 252.0;
const CYCLE_WIDTH: f32 = 262.0;
/// The gap between cards; a card's padding above and below its rows.
const CARD_GAP: f32 = 6.0;
const PAD: f32 = 8.0;
/// The rows' heights.
const TOOLS: f32 = 46.0;
const LINE: f32 = 13.0;
const ROW: f32 = 36.0;
const CHOICE: f32 = 34.0;
const SLIDER: f32 = 56.0;
const TOGGLE: f32 = 44.0;
const NUMBER_LINE: f32 = 34.0;
const KEY: f32 = 40.0;
const KEY_GAP: f32 = 6.0;
/// The most digits typed on a keypad; the most frames a frame or a run of empty frames is typed;
/// the longest a timeline is typed.
const MOST_DIGITS: usize = 6;
const MOST_FRAMES: u32 = 999;
const MOST_LENGTH: u32 = 100_000;

const CARD_FILL: Themed = Themed(0xF22C2C2C, 0xF5F2F2F2);
const CARD_EDGE: Themed = Themed(0xFF3C3C3C, 0xFFD6D6D6);
const INK: Themed = Themed(0xFFC9C9C9, 0xFF393939);
const INK_BRIGHT: Themed = Themed(0xFFF2F2F2, 0xFF1C1C1C);
const INK_DIM: Themed = Themed(0xFF757575, 0xFF8C8C8C);
const RULE: Themed = Themed(0xB3474747, 0xB3C0C0C0);
const KEY_FILL: Themed = Themed(0x1AF2F2F2, 0x1A1C1C1C);
/// A row that takes something away.
const DANGER: Color32 = Color32::from_rgb(0xFF, 0x45, 0x3A);

/// The colours a track's name (or a block) is highlighted in (0xRRGGBB).
pub(super) const HIGHLIGHTS: [(&str, u32); 8] = [
    ("Red", 0xFF453A),
    ("Orange", 0xFF9F0A),
    ("Yellow", 0xFFD60A),
    ("Green", 0x30D158),
    ("Blue", 0x0A84FF),
    ("Purple", 0xBF5AF2),
    ("Pink", 0xF050C8),
    ("Grey", 0x8E8E93),
];

const MASKS: [(TimelineMask, &str); 4] = [
    (TimelineMask::None, "None"),
    (TimelineMask::Clipping, "Clipping mask"),
    (TimelineMask::Alpha, "Alpha mask"),
    (TimelineMask::Luminance, "Luminance mask"),
];

/// Which menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Kind {
    /// A drawing's frame held.
    Frame { clip: u64 },
    /// A group's, a sound's or a video's block held.
    Block { clip: u64 },
    /// An empty frame held: its track and the frame.
    Empty { track: u64, frame: u32 },
    /// A track's name held.
    Track { track: u64 },
    /// One of the frames picked together held.
    Picked,
    /// The end flap tapped.
    Flap,
    /// `+` held.
    Add,
}

/// What stands beside a menu's card (or beside the track's card standing beside it).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Beside {
    Options,
    Typing,
    Highlight,
    Blend,
    Mask,
    Cycle,
}

/// The menu standing on the bar.
pub(super) struct Menu {
    pub kind: Kind,
    /// Where along the window its first card stands (its middle).
    pub x: f32,
    pub beside: Option<Beside>,
    /// What stands beside the track's card (a track's own menu's, or the one beside another).
    pub sub: Option<Beside>,
    /// What is typed on the keys beside it so far.
    pub typed: String,
    /// A name being typed in place of Rename: whose (its key) and the text so far.
    pub renaming: Option<(&'static str, String)>,
    focus_asked: bool,
    /// The end flap's frames to add (0: a second's worth).
    pub adding: u32,
    /// How far each card's rows are scrolled, where they don't all fit.
    scroll: Vec<f32>,
}

impl Menu {
    pub fn new(kind: Kind, x: f32) -> Menu {
        Menu {
            kind,
            x,
            beside: None,
            sub: None,
            typed: String::new(),
            renaming: None,
            focus_asked: false,
            adding: 0,
            scroll: Vec::new(),
        }
    }

    /// Its keys are out, or a name is being typed: the keyboard is the menu's.
    pub fn takes_keys(&self) -> bool {
        self.renaming.is_some() || self.beside == Some(Beside::Typing)
    }

    fn set_beside(&mut self, beside: Option<Beside>) {
        self.beside = if self.beside == beside { None } else { beside };
        self.sub = None;
        self.typed.clear();
    }

    fn set_sub(&mut self, sub: Beside) {
        self.sub = if self.sub == Some(sub) { None } else { Some(sub) };
    }
}

/// A colour's swatch before a choice's name.
#[derive(Clone, Copy)]
enum Swatch {
    No,
    Colour(Color32),
    /// "None": the highlight's ring.
    Clear,
}

/// A row of a card.
enum Item {
    /// Cut, copy, paste and duplicate, each with its key, dimmed where it can't be done.
    Tools([(&'static str, bool); 4]),
    Line,
    Row {
        key: &'static str,
        label: String,
        icon: Option<&'static Icon>,
        danger: bool,
        dim: bool,
        lit: bool,
        opens: bool,
    },
    Slider {
        key: &'static str,
        label: String,
        value: f32,
    },
    /// "Rename", which becomes a field for the new name.
    Rename {
        key: &'static str,
        name: String,
    },
    /// How many frames: a step shorter or longer, or the number tapped to type it beside.
    Duration {
        key: &'static str,
        label: String,
        frames: u32,
        locked: bool,
    },
    Choice {
        key: &'static str,
        index: usize,
        label: String,
        swatch: Swatch,
        chosen: bool,
    },
    Toggle {
        key: &'static str,
        label: String,
        on: bool,
    },
    /// A number typed on its own keys (`fps`: its length in time said beside it).
    Keypad {
        key: &'static str,
        unit: &'static str,
        from: u32,
        most: u32,
        fps: Option<u32>,
    },
    /// A setting and what it is, tapped to open more.
    Value {
        key: &'static str,
        label: String,
        value: String,
        lit: bool,
    },
}

impl Item {
    fn height(&self) -> f32 {
        match self {
            Item::Tools(_) => TOOLS,
            Item::Line => LINE,
            Item::Row { .. } | Item::Rename { .. } | Item::Duration { .. } | Item::Value { .. } => ROW,
            Item::Slider { .. } => SLIDER,
            Item::Choice { .. } => CHOICE,
            Item::Toggle { .. } => TOGGLE,
            Item::Keypad { .. } => NUMBER_LINE + KEY_GAP + 4.0 * KEY + 4.0 * KEY_GAP,
        }
    }
}

fn row(key: &'static str, label: impl Into<String>, icon: &'static Icon) -> Item {
    Item::Row { key, label: label.into(), icon: Some(icon), danger: false, dim: false, lit: false, opens: false }
}

fn dim(item: Item, dimmed: bool) -> Item {
    match item {
        Item::Row { key, label, icon, danger, lit, opens, .. } => {
            Item::Row { key, label, icon, danger, dim: dimmed, lit, opens }
        }
        other => other,
    }
}

fn danger(item: Item) -> Item {
    match item {
        Item::Row { key, label, icon, dim, lit, opens, .. } => {
            Item::Row { key, label, icon, danger: true, dim, lit, opens }
        }
        other => other,
    }
}

fn opens(key: &'static str, label: &str, icon: &'static Icon, lit: bool) -> Item {
    Item::Row { key, label: label.into(), icon: Some(icon), danger: false, dim: false, lit, opens: true }
}

/// A card: how wide, its rows.
struct Card {
    width: f32,
    items: Vec<Item>,
}

impl Card {
    fn height(&self) -> f32 {
        2.0 * PAD + self.items.iter().map(Item::height).sum::<f32>()
    }
}

/// What was pressed on a card.
#[derive(Clone, Debug, PartialEq)]
enum Hit {
    Row(&'static str),
    Slid { key: &'static str, value: f32, done: bool },
    Renamed { key: &'static str, name: String },
    Step { key: &'static str, to: u32 },
    Typing(&'static str),
    Choice { key: &'static str, index: usize },
    Keyed { key: &'static str, n: u32 },
}

impl TimelineGrid {
    /// Menu `kind` opened over `x` (window points), whatever else was open closed (a key's menu
    /// and the Keys card among them).
    pub(super) fn open_menu(&mut self, kind: Kind, x: f32) {
        self.settings_asked = false;
        self.keys.menu = None;
        self.keys_card_for = None;
        self.menu = Some(Menu::new(kind, x));
    }

    /// The menu closed.
    pub(super) fn close_menu(&mut self) {
        self.menu = None;
        self.cards.clear();
    }

    /// Whether `p` (window points) is on one of the menu's cards as last drawn.
    pub(super) fn on_cards(&self, p: Pos2) -> bool {
        self.menu.is_some() && self.cards.iter().any(|c| c.contains(p))
    }

    /// The menu open, standing on the bar: its cards drawn, and what was pressed on them done. A
    /// press off them and off the bar, or Escape, closes it.
    pub(super) fn show_menu(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession, place: &Place) {
        let Some(menu) = self.menu.as_ref() else {
            self.cards.clear();
            return;
        };
        // (a press on the bar is the grid's: a tap there closes it)
        let screen = ctx.content_rect();
        crate::screen::claim_window(ctx, screen);
        let off = ctx.input(|i| {
            (i.pointer.any_pressed())
                && i.pointer.interact_pos().is_some_and(|p| !place.bar.contains(p) && !self.on_cards(p))
        });
        if off {
            self.close_menu();
            return;
        }
        if menu.renaming.is_none() && crate::ui::escape_first(ctx) {
            match self.menu.as_mut() {
                Some(m) if m.sub.is_some() => m.sub = None,
                Some(m) if m.beside.is_some() => m.set_beside(None),
                _ => self.close_menu(),
            }
            return;
        }
        let kind = menu.kind;
        let Some(cards) = self.menu_cards(session) else {
            self.close_menu();
            return;
        };
        // laid out as the iPad's `GridMenuPlace`: the first card's middle over `x` (the last's,
        // for Add new), as near as they all fit on the bar, hanging from the same top (standing
        // on the same bottom, the end flap's)
        let total = cards.iter().map(|c| c.width).sum::<f32>() + CARD_GAP * cards.len().saturating_sub(1) as f32;
        let first = cards.first().map_or(CARD_WIDTH, |c| c.width);
        let trailing = kind == Kind::Add;
        let x = menu.x - first / 2.0 - if trailing { total - first } else { 0.0 };
        let left = x.min(place.bar.right() - total).max(place.bar.left());
        let bottom = place.bar.top() - BAR_GAP_ABOVE;
        let room = (bottom - screen.top() - 12.0).max(120.0);
        let tallest = cards.iter().map(Card::height).fold(0.0, f32::max).min(room);
        let mut rects = Vec::new();
        let mut at = left;
        for c in &cards {
            let h = c.height().min(room);
            let top = if kind == Kind::Flap { bottom - h } else { bottom - tallest };
            rects.push(Rect::from_min_size(pos2(at, top), vec2(c.width, h)));
            at += c.width + CARD_GAP;
        }
        let mut hits = Vec::new();
        let Some(menu) = self.menu.as_mut() else { return };
        menu.scroll.resize(cards.len(), 0.0);
        for (i, (card, rect)) in cards.iter().zip(&rects).enumerate() {
            egui::Area::new(Id::unique(("timeline menu card", i))).fixed_pos(rect.min).order(Order::Foreground).show(
                ctx,
                |ui| {
                    ui.allocate_rect(*rect, Sense::click_and_drag());
                    draw_card(ui, look, menu, i, card, *rect, &mut hits);
                },
            );
        }
        // the keyboard's digits on the keys beside it
        if let Some(Item::Keypad { key, most, .. }) =
            cards.iter().flat_map(|c| &c.items).find(|i| matches!(i, Item::Keypad { .. }))
            && let Some(n) = hardware_keys(ctx, menu, *most)
        {
            hits.push(Hit::Keyed { key, n });
        }
        self.cards = rects;
        for hit in hits {
            self.menu_hit(session, kind, hit);
            if self.menu.is_none() {
                break;
            }
        }
    }

    /// The cards of the menu open, as things are now: none where what it is of has gone.
    fn menu_cards(&self, session: &CanvasSession) -> Option<Vec<Card>> {
        let menu = self.menu.as_ref()?;
        let mut cards = Vec::new();
        match menu.kind {
            Kind::Frame { clip } => {
                let (track, c) = self.clip(clip)?;
                let locked = self.held(&track);
                let drawing = c.kind == TimelineClipKind::Drawing;
                let mut items = vec![
                    Item::Tools([
                        ("Cut", !locked),
                        ("Copy", true),
                        ("Paste", self.can_paste() && !locked),
                        ("Duplicate", !locked && c.kind != TimelineClipKind::Group),
                    ]),
                    Item::Line,
                ];
                if drawing {
                    items.push(dim(row("Insert frame", "Insert frame", &grid_icons::GRID_INSERT), locked));
                }
                items.push(dim(
                    row("Fill duration", "Fill duration", &grid_icons::GRID_FILL),
                    !self.can_extend(session, &[clip]),
                ));
                if self.inside_group(track.id) {
                    items.push(dim(row("Move out of group", "Move out of group", &icons::LAYERS), locked));
                }
                items.push(Item::Line);
                items.push(Item::Duration {
                    key: "Frame duration",
                    label: "Frame duration".into(),
                    frames: c.duration,
                    locked,
                });
                items.push(Item::Line);
                items.push(opens("Keys", "Keys", &grid_icons::GRID_KEY, false));
                items.push(opens(
                    "Track options",
                    "Track options",
                    &grid_icons::GRID_OPTIONS,
                    menu.beside == Some(Beside::Options),
                ));
                items.push(Item::Line);
                let off = if c.disabled { "Turn on" } else { "Turn off" };
                items.push(dim(row("Turn off", off, &grid_icons::GRID_HIGHLIGHT), locked));
                if drawing {
                    items.push(dim(row("Clear frame", "Clear frame", &grid_icons::GRID_CLEAR), locked));
                }
                items.push(danger(dim(row("Delete frame", "Delete frame", &icons::TRASH), locked)));
                cards.push(Card { width: CARD_WIDTH, items });
                match menu.beside {
                    Some(Beside::Options) => self.track_cards(menu, &track, &mut cards),
                    Some(Beside::Typing) => cards.push(keypad_card("Frame duration", c.duration, MOST_FRAMES, None)),
                    _ => {}
                }
            }
            Kind::Block { clip } => {
                let (track, c) = self.clip(clip)?;
                let locked = self.held(&track);
                let sound = c.kind == TimelineClipKind::Audio;
                let group = c.kind == TimelineClipKind::Group;
                let at = self.frame();
                let inside = at > c.start && at < c.start + c.duration;
                let mut items = vec![
                    Item::Tools([
                        ("Cut", !locked),
                        ("Copy", true),
                        ("Paste", self.can_paste() && !locked),
                        ("Duplicate", !locked && !group),
                    ]),
                    Item::Line,
                ];
                if !sound {
                    items.push(Item::Slider { key: "Opacity", label: "Opacity".into(), value: c.opacity });
                }
                items.push(Item::Rename { key: "Rename", name: c.name.clone() });
                items.push(opens(
                    "Highlight",
                    "Highlight",
                    &grid_icons::GRID_HIGHLIGHT,
                    menu.beside == Some(Beside::Highlight),
                ));
                if !sound {
                    items.push(opens(
                        "Blend mode",
                        "Blend mode",
                        &grid_icons::GRID_BLEND,
                        menu.beside == Some(Beside::Blend),
                    ));
                    items.push(opens("Mask", "Mask", &grid_icons::GRID_MASK, menu.beside == Some(Beside::Mask)));
                }
                items.push(Item::Line);
                if group {
                    let open = self.rows.unfolded(clip);
                    items.push(row(
                        "Expand group",
                        if open { "Collapse group" } else { "Expand group" },
                        &icons::LAYERS,
                    ));
                    items.push(opens("Cycle", "Cycle", &grid_icons::GRID_FLIP, menu.beside == Some(Beside::Cycle)));
                    items.push(dim(row("Ungroup", "Ungroup", &grid_icons::GRID_CLEAR), locked));
                }
                if !sound {
                    items.push(opens("Keys", "Keys", &grid_icons::GRID_KEY, false));
                }
                items.push(Item::Line);
                if self.inside_group(track.id) {
                    items.push(dim(row("Move out of group", "Move out of group", &icons::LAYERS), locked));
                }
                let off = if c.disabled { "Turn on" } else { "Turn off" };
                items.push(dim(row("Turn off", off, &grid_icons::GRID_HIGHLIGHT), locked));
                // (a repeating group isn't cut)
                items.push(dim(
                    row("Split content", "Split content", &grid_icons::GRID_SCISSORS),
                    locked || !inside || c.cycle > 0,
                ));
                items.push(dim(row("Duplicate after", "Duplicate after", &icons::DUPLICATE), locked || group));
                items.push(dim(
                    row("Fill duration", "Fill duration", &grid_icons::GRID_FILL),
                    !self.can_extend(session, &[clip]),
                ));
                items.push(opens(
                    "Track options",
                    "Track options",
                    &grid_icons::GRID_OPTIONS,
                    menu.beside == Some(Beside::Options),
                ));
                items.push(Item::Line);
                items.push(danger(dim(row("Delete content", "Delete content", &icons::TRASH), locked)));
                cards.push(Card { width: BLOCK_WIDTH, items });
                match menu.beside {
                    Some(Beside::Highlight) => cards.push(highlight_card(c.label)),
                    Some(Beside::Blend) => cards.push(blend_card(c.blend)),
                    Some(Beside::Mask) => cards.push(mask_card(c.mask)),
                    Some(Beside::Cycle) => cards.push(self.cycle_card(session, clip, c.cycle, c.continue_across)),
                    Some(Beside::Options) => self.track_cards(menu, &track, &mut cards),
                    _ => {}
                }
            }
            Kind::Empty { track, frame } => {
                let t = self.track(track)?;
                let gap = self.gap(session, track, frame);
                let locked = self.held(&t);
                // with no frame after them, the empty frames have no length to change
                let fixed = locked || gap.is_none_or(|(_, _, open)| open);
                let length = gap.map_or(0, |(_, length, _)| length);
                let items = vec![
                    dim(row("Paste", "Paste", &grid_icons::GRID_PASTE), !self.can_paste() || locked),
                    dim(row("Insert frame", "Insert frame", &grid_icons::GRID_INSERT), locked),
                    Item::Line,
                    Item::Duration { key: "Empty frames", label: "Empty frames".into(), frames: length, locked: fixed },
                    Item::Line,
                    opens(
                        "Track options",
                        "Track options",
                        &grid_icons::GRID_OPTIONS,
                        menu.beside == Some(Beside::Options),
                    ),
                    Item::Line,
                    dim(row("Remove single frame", "Remove single frame", &grid_icons::GRID_REMOVE), fixed),
                    dim(row("Remove empty section", "Remove empty section", &grid_icons::GRID_BROOM), fixed),
                ];
                cards.push(Card { width: EMPTY_WIDTH, items });
                match menu.beside {
                    Some(Beside::Options) => self.track_cards(menu, &t, &mut cards),
                    Some(Beside::Typing) => cards.push(keypad_card("Empty frames", length, MOST_FRAMES, None)),
                    _ => {}
                }
            }
            Kind::Track { track } => {
                let t = self.track(track)?;
                self.track_cards(menu, &t, &mut cards);
            }
            Kind::Picked => {
                let ids = self.chosen();
                if ids.is_empty() {
                    return None;
                }
                let off = self.all_disabled(&ids);
                let items = vec![
                    Item::Tools([("Cut", true), ("Copy", true), ("Paste", self.can_paste()), ("Duplicate", true)]),
                    Item::Line,
                    row("Group frames", "Group frames", &icons::LAYERS),
                    dim(row("Flip frames", "Flip frames", &grid_icons::GRID_FLIP), ids.len() < 2),
                    dim(row("Fill duration", "Fill duration", &grid_icons::GRID_FILL), !self.can_extend(session, &ids)),
                    Item::Line,
                    row("Turn off", if off { "Turn on" } else { "Turn off" }, &grid_icons::GRID_HIGHLIGHT),
                    row("Clear frames", "Clear frames", &grid_icons::GRID_CLEAR),
                    danger(row("Delete frames", "Delete frames", &icons::TRASH)),
                ];
                cards.push(Card { width: CARD_WIDTH, items });
            }
            Kind::Flap => {
                let info = self.info()?;
                let count = if menu.adding > 0 { menu.adding } else { info.fps.max(1) };
                let length =
                    format!("{} · {}", info.duration, crate::timeline_settings::clock(info.duration, info.fps));
                let items = vec![
                    Item::Value {
                        key: "Length",
                        label: "Length".into(),
                        value: length,
                        lit: menu.beside == Some(Beside::Typing),
                    },
                    row("Fit to content", "Fit to content", &grid_icons::GRID_FILL),
                    Item::Line,
                    Item::Duration {
                        key: "Frames to add",
                        label: "Frames to add".into(),
                        frames: count,
                        locked: false,
                    },
                    row("Add at start", "Add at start", &grid_icons::GRID_INSERT),
                    row("Add at end", "Add at end", &grid_icons::GRID_INSERT),
                ];
                cards.push(Card { width: CARD_WIDTH, items });
                if menu.beside == Some(Beside::Typing) {
                    cards.push(keypad_card("Length", info.duration, MOST_LENGTH, Some(info.fps)));
                }
            }
            Kind::Add => {
                let items = vec![row("Picture", "Picture from a file…", &grid_icons::GRID_PHOTO)];
                cards.push(Card { width: CARD_WIDTH, items });
            }
        }
        Some(cards)
    }

    /// Whether track `id` is a track of a group's (a row inside an expanded group).
    fn inside_group(&self, id: u64) -> bool {
        self.rows.segment_of(id).is_some_and(|(_, s)| s.row.group != 0)
    }

    /// A track's card, and what stands beside it.
    fn track_cards(&self, menu: &Menu, t: &TimelineTrackInfo, cards: &mut Vec<Card>) {
        let empty = t.clips.is_empty();
        let locked = self.held(t);
        let last = self.rows.tracks().count() <= 1;
        let items = vec![
            Item::Tools([
                ("Track cut", !empty && !locked),
                ("Track copy", !empty),
                ("Track paste", self.can_paste() && !locked),
                ("Track duplicate", !empty && !locked),
            ]),
            Item::Line,
            Item::Slider { key: "Track opacity", label: "Opacity".into(), value: t.opacity },
            Item::Rename { key: "Track rename", name: t.name.clone() },
            opens("Track highlight", "Highlight", &grid_icons::GRID_HIGHLIGHT, menu.sub == Some(Beside::Highlight)),
            opens("Track blend", "Blend mode", &grid_icons::GRID_BLEND, menu.sub == Some(Beside::Blend)),
            opens("Track mask", "Mask", &grid_icons::GRID_MASK, menu.sub == Some(Beside::Mask)),
            row("Track lock", if t.locked { "Unlock track" } else { "Lock track" }, &icons::LOCK),
            Item::Line,
            // every frame off it, the track staying
            danger(dim(row("Clear track", "Clear track", &grid_icons::GRID_BROOM), empty || locked)),
            // the last track stays
            danger(dim(row("Delete track", "Delete track", &icons::TRASH), last)),
        ];
        cards.push(Card { width: CARD_WIDTH, items });
        match menu.sub {
            Some(Beside::Highlight) => cards.push(highlight_card(t.label)),
            Some(Beside::Blend) => cards.push(blend_card(t.blend)),
            Some(Beside::Mask) => cards.push(mask_card(t.mask)),
            _ => {}
        }
    }

    /// A group's cycle: playing its frames over and over, how many times, and whether its own keys
    /// run on across the repeats.
    fn cycle_card(&self, session: &CanvasSession, clip: u64, cycle: u32, continue_across: bool) -> Card {
        let repeats = cycle > 0;
        let mut items = vec![Item::Toggle { key: "Repeat", label: "Repeat".into(), on: repeats }];
        if repeats && let Some((count, _)) = self.cycles(session, clip) {
            items.push(Item::Duration { key: "Repeats", label: "Repeats".into(), frames: count, locked: false });
            items.push(Item::Toggle { key: "Run on", label: "Keys run on across repeats".into(), on: continue_across });
        }
        Card { width: CYCLE_WIDTH, items }
    }

    /// What was pressed on the cards of menu `kind` done.
    fn menu_hit(&mut self, session: &CanvasSession, kind: Kind, hit: Hit) {
        // the track's card, wherever it stands
        if let Some(track) = self.menu_track(kind)
            && self.track_hit(session, track, &hit)
        {
            return;
        }
        match kind {
            Kind::Frame { clip } | Kind::Block { clip } => self.clip_hit(session, clip, hit),
            Kind::Empty { track, frame } => self.empty_hit(session, track, frame, hit),
            Kind::Track { .. } => {}
            Kind::Picked => self.picked_hit(session, hit),
            Kind::Flap => self.flap_hit(session, hit),
            Kind::Add => {
                if hit == Hit::Row("Picture") {
                    self.close_menu();
                    self.add_picture(session);
                }
            }
        }
    }

    /// The track whose card the menu shows (its own, or beside it).
    fn menu_track(&self, kind: Kind) -> Option<u64> {
        match kind {
            Kind::Track { track } | Kind::Empty { track, .. } => Some(track),
            Kind::Frame { clip } | Kind::Block { clip } => self.clip(clip).map(|(t, _)| t.id),
            _ => None,
        }
    }

    /// A press on a track's card: true if it was one.
    fn track_hit(&mut self, session: &CanvasSession, track: u64, hit: &Hit) -> bool {
        let Some(t) = self.track(track) else { return false };
        let ids: Vec<u64> = t.clips.iter().map(|c| c.id).collect();
        match hit {
            Hit::Row("Track cut") => {
                self.close_menu();
                self.cut_frames(session, ids);
            }
            Hit::Row("Track copy") => {
                self.close_menu();
                self.copy_frames(session, ids);
            }
            Hit::Row("Track paste") => {
                self.close_menu();
                self.select_track(session, track);
                self.paste_clips(session);
            }
            Hit::Row("Track duplicate") => {
                self.close_menu();
                self.duplicate_frames(session, ids);
            }
            Hit::Slid { key: "Track opacity", value, done } => self.set_track_opacity(session, track, *value, *done),
            Hit::Renamed { key: "Track rename", name } => self.rename_track(session, track, name.clone()),
            Hit::Row("Track highlight") => self.with_menu(|m| m.set_sub(Beside::Highlight)),
            Hit::Row("Track blend") => self.with_menu(|m| m.set_sub(Beside::Blend)),
            Hit::Row("Track mask") => self.with_menu(|m| m.set_sub(Beside::Mask)),
            Hit::Row("Track lock") => self.toggle_locked(session, &t),
            Hit::Row("Clear track") => {
                self.close_menu();
                self.delete_frames(session, ids);
            }
            Hit::Row("Delete track") => {
                self.close_menu();
                self.delete_track(session, track);
            }
            // (the choices beside the track's card: the highlights, blends or masks)
            Hit::Choice { key: "Highlight", index } if self.menu_sub() == Some(Beside::Highlight) => {
                self.set_track_label(session, track, HIGHLIGHTS.get(*index).map(|(_, rgb)| *rgb));
            }
            Hit::Choice { key: "Blend", index } if self.menu_sub() == Some(Beside::Blend) => {
                if let Some(m) = procraft_core::blend_modes().get(*index) {
                    self.set_track_blend(session, track, m.id);
                }
            }
            Hit::Choice { key: "Mask", index } if self.menu_sub() == Some(Beside::Mask) => {
                if let Some((mask, _)) = MASKS.get(*index) {
                    self.set_track_mask(session, track, *mask);
                }
            }
            _ => return false,
        }
        true
    }

    fn menu_sub(&self) -> Option<Beside> {
        self.menu.as_ref().and_then(|m| m.sub)
    }

    fn with_menu(&mut self, edit: impl FnOnce(&mut Menu)) {
        if let Some(m) = self.menu.as_mut() {
            edit(m);
        }
    }

    /// A press on a frame's or a block's menu.
    fn clip_hit(&mut self, session: &CanvasSession, clip: u64, hit: Hit) {
        let Some((track, c)) = self.clip(clip) else { return };
        match hit {
            Hit::Row("Cut") => {
                self.close_menu();
                self.cut_frames(session, vec![clip]);
            }
            Hit::Row("Copy") => {
                self.close_menu();
                self.copy_frames(session, vec![clip]);
            }
            Hit::Row("Paste") => {
                self.close_menu();
                self.paste_at(session, c.start, track.id);
            }
            Hit::Row("Duplicate" | "Duplicate after") => {
                self.close_menu();
                self.duplicate_frames(session, vec![clip]);
            }
            Hit::Row("Insert frame") => {
                self.close_menu();
                self.insert_frame(session, c.start, track.id);
            }
            Hit::Row("Fill duration") => {
                self.close_menu();
                self.extend(session, vec![clip]);
            }
            Hit::Row("Move out of group") => {
                self.close_menu();
                self.move_out_of_group(session, clip);
            }
            Hit::Step { key: "Frame duration", to } | Hit::Keyed { key: "Frame duration", n: to } => {
                if matches!(hit, Hit::Keyed { .. }) {
                    self.with_menu(|m| m.set_beside(None));
                }
                self.trim(session, clip, false, i64::from(c.start) + i64::from(to.max(1)));
            }
            Hit::Typing("Frame duration") => self.with_menu(|m| m.set_beside(Some(Beside::Typing))),
            Hit::Row("Keys") => {
                // (its keys go on the playhead's frame: the playhead comes to it)
                self.close_menu();
                self.bring_playhead(session, &c);
                self.keys_card_for = Some(clip);
            }
            Hit::Row("Track options") => self.with_menu(|m| m.set_beside(Some(Beside::Options))),
            Hit::Row("Turn off") => self.set_clip_disabled(session, clip, !c.disabled),
            Hit::Row("Clear frame") => {
                self.close_menu();
                self.clear_frames(session, vec![clip]);
            }
            Hit::Row("Delete frame" | "Delete content") => {
                self.close_menu();
                self.delete_frames(session, vec![clip]);
            }
            Hit::Slid { key: "Opacity", value, done } => self.set_clip_opacity(session, clip, value, done),
            Hit::Renamed { key: "Rename", name } => self.rename_clip(session, clip, name),
            Hit::Row("Highlight") => self.with_menu(|m| m.set_beside(Some(Beside::Highlight))),
            Hit::Row("Blend mode") => self.with_menu(|m| m.set_beside(Some(Beside::Blend))),
            Hit::Row("Mask") => self.with_menu(|m| m.set_beside(Some(Beside::Mask))),
            Hit::Row("Cycle") => self.with_menu(|m| m.set_beside(Some(Beside::Cycle))),
            Hit::Choice { key: "Highlight", index } => {
                self.set_clip_label(session, clip, HIGHLIGHTS.get(index).map(|(_, rgb)| *rgb));
            }
            Hit::Choice { key: "Blend", index } => {
                if let Some(m) = procraft_core::blend_modes().get(index) {
                    self.set_clip_blend(session, clip, m.id);
                }
            }
            Hit::Choice { key: "Mask", index } => {
                if let Some((mask, _)) = MASKS.get(index) {
                    self.set_clip_mask(session, clip, *mask);
                }
            }
            Hit::Row("Expand group") => {
                let open = self.rows.unfolded(clip);
                self.fold(session, clip, open);
            }
            Hit::Row("Ungroup") => {
                self.close_menu();
                self.ungroup(session, clip);
            }
            Hit::Row("Split content") => {
                self.close_menu();
                self.split(session, clip);
            }
            // the cycle card: what it lasts now is what is played again
            Hit::Row("Repeat") => {
                let length = if c.cycle > 0 { 0 } else { c.duration };
                self.set_cycle(session, clip, length, c.continue_across);
            }
            Hit::Step { key: "Repeats", to } => {
                let most = self.cycles(session, clip).map_or(1, |(_, most)| most.max(1));
                self.set_cycles(session, clip, to.min(most));
            }
            Hit::Row("Run on") => self.set_cycle(session, clip, c.cycle, !c.continue_across),
            _ => {}
        }
    }

    /// A press on an empty frame's menu.
    fn empty_hit(&mut self, session: &CanvasSession, track: u64, frame: u32, hit: Hit) {
        let gap = self.gap(session, track, frame);
        let (at, length) = gap.map_or((frame, 0), |(start, length, _)| (start, length));
        match hit {
            Hit::Row("Paste") => {
                self.close_menu();
                self.paste_at(session, frame, track);
            }
            Hit::Row("Insert frame") => {
                self.close_menu();
                self.insert_frame(session, frame, track);
            }
            Hit::Step { key: "Empty frames", to } => self.set_gap(session, track, at, to),
            Hit::Keyed { key: "Empty frames", n } => {
                self.with_menu(|m| m.set_beside(None));
                self.set_gap(session, track, at, n);
            }
            Hit::Typing("Empty frames") => self.with_menu(|m| m.set_beside(Some(Beside::Typing))),
            Hit::Row("Track options") => self.with_menu(|m| m.set_beside(Some(Beside::Options))),
            Hit::Row("Remove single frame") => {
                // (the last of them gone, there is no empty frame left to have a menu)
                if length <= 1 {
                    self.close_menu();
                }
                self.set_gap(session, track, at, length.saturating_sub(1));
            }
            Hit::Row("Remove empty section") => {
                self.close_menu();
                self.set_gap(session, track, at, 0);
            }
            _ => {}
        }
    }

    /// A press on the picked frames' menu (the lasso stays on after each).
    fn picked_hit(&mut self, session: &CanvasSession, hit: Hit) {
        let ids = self.chosen();
        let Hit::Row(key) = hit else { return };
        self.close_menu();
        match key {
            "Cut" => self.cut_frames(session, ids),
            "Copy" => self.copy_frames(session, ids),
            "Paste" => self.paste_clips(session),
            "Duplicate" => {
                // the copies picked in their place
                let made = self.duplicate_frames(session, ids);
                if !made.is_empty() {
                    self.selecting = Some(made.into_iter().collect());
                }
            }
            "Group frames" => self.make_group(session),
            "Flip frames" => self.flip_frames(session, ids),
            "Fill duration" => self.extend(session, ids),
            "Turn off" => {
                let off = self.all_disabled(&ids);
                self.set_clips_disabled(session, ids, !off);
            }
            "Clear frames" => self.clear_frames(session, ids),
            "Delete frames" => self.delete_frames(session, ids),
            _ => {}
        }
    }

    /// A press on the end flap's menu.
    fn flap_hit(&mut self, session: &CanvasSession, hit: Hit) {
        let fps = self.info().map_or(1, |i| i.fps.max(1));
        let count = self.menu.as_ref().map_or(fps, |m| if m.adding > 0 { m.adding } else { fps });
        match hit {
            Hit::Row("Length") => self.with_menu(|m| m.set_beside(Some(Beside::Typing))),
            Hit::Keyed { key: "Length", n } => {
                self.close_menu();
                self.set_length(session, n);
            }
            Hit::Row("Fit to content") => {
                self.close_menu();
                self.fit_to_content(session);
            }
            Hit::Step { key: "Frames to add", to } => self.with_menu(|m| m.adding = to.max(1)),
            Hit::Row("Add at start") => {
                self.close_menu();
                self.add_frames(session, true, count);
            }
            Hit::Row("Add at end") => {
                self.close_menu();
                self.add_frames(session, false, count);
            }
            _ => {}
        }
    }
}

/// The highlights to choose from, the one chosen ticked.
fn highlight_card(label: Option<u32>) -> Card {
    let mut items: Vec<Item> = HIGHLIGHTS
        .iter()
        .enumerate()
        .map(|(index, (name, rgb))| Item::Choice {
            key: "Highlight",
            index,
            label: (*name).into(),
            swatch: Swatch::Colour(label_colour(*rgb)),
            chosen: label == Some(*rgb),
        })
        .collect();
    items.push(Item::Choice {
        key: "Highlight",
        index: HIGHLIGHTS.len(),
        label: "None".into(),
        swatch: Swatch::Clear,
        chosen: label.is_none(),
    });
    Card { width: CARD_WIDTH, items }
}

/// The blend modes to choose from (it scrolls where they don't all fit).
fn blend_card(blend: u32) -> Card {
    let items = procraft_core::blend_modes()
        .into_iter()
        .enumerate()
        .map(|(index, m)| Item::Choice {
            key: "Blend",
            index,
            label: m.name,
            swatch: Swatch::No,
            chosen: m.id == blend,
        })
        .collect();
    Card { width: CARD_WIDTH, items }
}

fn mask_card(mask: TimelineMask) -> Card {
    let items = MASKS
        .iter()
        .enumerate()
        .map(|(index, (m, name))| Item::Choice {
            key: "Mask",
            index,
            label: (*name).into(),
            swatch: Swatch::No,
            chosen: *m == mask,
        })
        .collect();
    Card { width: CARD_WIDTH, items }
}

fn keypad_card(key: &'static str, from: u32, most: u32, fps: Option<u32>) -> Card {
    Card { width: CARD_WIDTH, items: vec![Item::Keypad { key, unit: "frames", from, most, fps }] }
}

/// The keyboard's digits, Backspace and Enter on the keys beside the menu: the number set, if
/// Enter took one between 1 and `most`.
fn hardware_keys(ctx: &egui::Context, menu: &mut Menu, most: u32) -> Option<u32> {
    if menu.renaming.is_some() {
        return None;
    }
    const DIGITS: [Key; 10] =
        [Key::Num0, Key::Num1, Key::Num2, Key::Num3, Key::Num4, Key::Num5, Key::Num6, Key::Num7, Key::Num8, Key::Num9];
    let (digits, back, enter) = ctx.input(|i| {
        let digits: Vec<u32> = DIGITS.iter().zip(0u32..).filter(|(k, _)| i.key_pressed(**k)).map(|(_, n)| n).collect();
        (digits, i.key_pressed(Key::Backspace), i.key_pressed(Key::Enter))
    });
    for n in digits {
        press(menu, n);
    }
    if back {
        menu.typed.pop();
    }
    enter.then(|| menu.typed.parse::<u32>().ok().filter(|n| (1..=most).contains(n))).flatten()
}

/// A digit typed: no more than six, and none starting with 0.
fn press(menu: &mut Menu, n: u32) {
    if menu.typed.len() < MOST_DIGITS && !(menu.typed.is_empty() && n == 0) {
        menu.typed.push_str(&n.to_string());
    }
}

/// Card `i` of the menu drawn in `rect`, what was pressed on it into `hits`.
fn draw_card(ui: &mut Ui, look: &mut Look, menu: &mut Menu, i: usize, card: &Card, rect: Rect, hits: &mut Vec<Hit>) {
    let painter = ui.painter().clone();
    painter.add(
        egui::epaint::Shadow { offset: [0, 6], blur: 18, spread: 0, color: Color32::from_black_alpha(71) }
            .as_shape(rect, egui::CornerRadius::same(22)),
    );
    painter.rect(rect, 22, argb(CARD_FILL), Stroke::new(1.0, argb(CARD_EDGE)), StrokeKind::Inside);
    // where its rows don't all fit, the wheel scrolls them
    let content = card.height();
    let over = (content - rect.height()).max(0.0);
    let scroll = menu.scroll.get(i).copied().unwrap_or(0.0);
    let scroll = if over > 0.0 && ui.rect_contains_pointer(rect) {
        (scroll - ui.input(|inp| inp.smooth_scroll_delta.y)).clamp(0.0, over)
    } else {
        scroll.clamp(0.0, over)
    };
    if let Some(s) = menu.scroll.get_mut(i) {
        *s = scroll;
    }
    let inner = rect.shrink2(vec2(0.0, PAD));
    let clipped = painter.with_clip_rect(inner);
    let mut y = rect.top() + PAD - scroll;
    for (n, item) in card.items.iter().enumerate() {
        let h = item.height();
        let r = Rect::from_min_size(pos2(rect.left(), y), vec2(rect.width(), h));
        y += h;
        if r.bottom() < inner.top() || r.top() > inner.bottom() {
            continue;
        }
        let id = Id::unique(("timeline menu", i, n));
        // (a row only partly in sight takes no presses)
        let whole = inner.contains_rect(r.shrink(1.0));
        draw_item(ui, look, &clipped, menu, item, r, id, whole, hits);
    }
}

/// One row of a card, in `r`; presses on it (where it shows `whole`) into `hits`.
#[allow(clippy::too_many_arguments)]
fn draw_item(
    ui: &mut Ui,
    look: &mut Look,
    painter: &egui::Painter,
    menu: &mut Menu,
    item: &Item,
    r: Rect,
    id: Id,
    whole: bool,
    hits: &mut Vec<Hit>,
) {
    let clicked = |ui: &Ui, rect: Rect, salt: &str| whole && ui.interact(rect, id.with(salt), Sense::click()).clicked();
    match item {
        Item::Tools(tools) => {
            let inner = r.shrink2(vec2(6.0, 0.0));
            let w = inner.width() / 4.0;
            for (k, (key, on)) in tools.iter().enumerate() {
                let cell = Rect::from_min_size(pos2(inner.left() + w * k as f32, r.top()), vec2(w, r.height()));
                let ink = if *on { argb(INK_BRIGHT) } else { argb(INK_DIM) };
                let icon = match k {
                    0 => &grid_icons::GRID_SCISSORS,
                    1 => &grid_icons::GRID_COPY,
                    2 => &grid_icons::GRID_PASTE,
                    _ => &icons::DUPLICATE,
                };
                look.icon_on(ui, painter, icon, pos2(cell.center().x, cell.top() + 16.0), 21.0, ink);
                let word = key.trim_start_matches("Track ");
                let word = format!("{}{}", word.get(..1).unwrap_or("").to_uppercase(), word.get(1..).unwrap_or(""));
                text(
                    painter,
                    pos2(cell.center().x, cell.top() + 35.0),
                    Align2::CENTER_CENTER,
                    &word,
                    10.5,
                    Weight::Regular,
                    ink,
                );
                if clicked(ui, cell, key) && *on {
                    hits.push(Hit::Row(key));
                }
            }
        }
        Item::Line => {
            let line = Rect::from_min_size(pos2(r.left() + 14.0, r.top() + 6.0), vec2(r.width() - 28.0, 1.0));
            painter.rect_filled(line, 0, argb(RULE));
        }
        Item::Row { key, label, icon, danger, dim, lit, opens } => {
            let ink = if *dim {
                argb(INK_DIM)
            } else if *lit {
                argb(pc::ACCENT)
            } else if *danger {
                DANGER
            } else {
                argb(INK_BRIGHT)
            };
            let mut x = r.left() + 16.0;
            if let Some(icon) = icon {
                look.icon_on(ui, painter, icon, pos2(x + 11.0, r.center().y), 20.0, ink);
                x += 22.0 + 10.0;
            }
            text(painter, pos2(x, r.center().y), Align2::LEFT_CENTER, label, 15.0, Weight::Regular, ink);
            if *opens {
                look.icon_on(ui, painter, &icons::CHEVRON_RIGHT, pos2(r.right() - 22.0, r.center().y), 15.0, ink);
            }
            if clicked(ui, r, key) && !*dim {
                hits.push(Hit::Row(key));
            }
        }
        Item::Slider { key, label, value } => {
            let (left, right) = (r.left() + 16.0, r.right() - 16.0);
            let ink = argb(INK_BRIGHT);
            text(painter, pos2(left, r.top() + 6.0), Align2::LEFT_TOP, label, 15.0, Weight::Regular, ink);
            let shown = format!("{} %", (value * 100.0).round() as i32);
            text(painter, pos2(right, r.top() + 6.0), Align2::RIGHT_TOP, &shown, 15.0, Weight::Regular, ink);
            let y = r.top() + 6.0 + 20.0 + 4.0 + 11.0;
            let bar = Rect::from_min_max(pos2(left, y - 11.0), pos2(right, y + 11.0));
            let x = left + value.clamp(0.0, 1.0) * (right - left);
            painter.rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(right, y + 1.5)), 1, argb(RULE));
            painter.rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(x, y + 1.5)), 1, argb(pc::ACCENT));
            let knob = Rect::from_center_size(pos2(x.clamp(left + 7.0, right - 7.0), y), vec2(14.0, 8.0));
            painter.rect_filled(knob, 4, Color32::WHITE);
            if whole {
                let response = ui.interact(bar, id.with("slider"), Sense::click_and_drag());
                let at = response.interact_pointer_pos().map(|p| ((p.x - left) / (right - left)).clamp(0.0, 1.0));
                if let Some(at) = at
                    && (response.is_pointer_button_down_on() || response.clicked())
                {
                    hits.push(Hit::Slid { key, value: at, done: false });
                }
                if (response.drag_stopped() || response.clicked())
                    && let Some(at) = at
                {
                    hits.push(Hit::Slid { key, value: at, done: true });
                }
            }
        }
        Item::Rename { key, name } => match menu.renaming.as_mut().filter(|(k, _)| k == key) {
            Some((_, typed)) => {
                let field = r.shrink2(vec2(16.0, 6.0));
                let edit = egui::TextEdit::singleline(typed)
                    .frame(egui::Frame::NONE)
                    .font(FontId::proportional(15.0))
                    .text_color(argb(INK_BRIGHT));
                let response = ui.put(field, edit);
                if !menu.focus_asked {
                    response.request_focus();
                    menu.focus_asked = true;
                }
                if response.lost_focus() {
                    let entered = ui.input(|i| i.key_pressed(Key::Enter));
                    if entered && let Some((_, name)) = menu.renaming.take() {
                        hits.push(Hit::Renamed { key, name });
                    }
                    menu.renaming = None;
                }
            }
            None => {
                let ink = argb(INK_BRIGHT);
                look.icon_on(ui, painter, &grid_icons::GRID_RENAME, pos2(r.left() + 27.0, r.center().y), 20.0, ink);
                text(
                    painter,
                    pos2(r.left() + 48.0, r.center().y),
                    Align2::LEFT_CENTER,
                    "Rename",
                    15.0,
                    Weight::Regular,
                    ink,
                );
                if clicked(ui, r, key) {
                    menu.renaming = Some((key, name.clone()));
                    menu.focus_asked = false;
                }
            }
        },
        Item::Duration { key, label, frames, locked } => {
            let ink = if *locked { argb(INK_DIM) } else { argb(INK_BRIGHT) };
            text(painter, pos2(r.left() + 16.0, r.center().y), Align2::LEFT_CENTER, label, 15.0, Weight::Regular, ink);
            let plus = Rect::from_center_size(pos2(r.right() - 6.0 - 14.0, r.center().y), vec2(28.0, 34.0));
            let number = Rect::from_center_size(pos2(plus.left() - 17.0, r.center().y), vec2(34.0, 20.0));
            let minus = Rect::from_center_size(pos2(number.left() - 14.0, r.center().y), vec2(28.0, 34.0));
            let minus_ink = if *frames > 1 { ink } else { argb(INK_DIM) };
            text(painter, minus.center(), Align2::CENTER_CENTER, "−", 18.0, Weight::Medium, minus_ink);
            text(painter, plus.center(), Align2::CENTER_CENTER, "+", 18.0, Weight::Medium, ink);
            painter.rect_stroke(number, 10, Stroke::new(1.2, ink.gamma_multiply(0.8)), StrokeKind::Inside);
            text(painter, number.center(), Align2::CENTER_CENTER, &frames.to_string(), 13.0, Weight::Medium, ink);
            if !*locked {
                if clicked(ui, minus, "minus") && *frames > 1 {
                    hits.push(Hit::Step { key, to: frames - 1 });
                }
                if clicked(ui, plus, "plus") {
                    hits.push(Hit::Step { key, to: frames + 1 });
                }
                if clicked(ui, number, "number") {
                    hits.push(Hit::Typing(key));
                }
            }
        }
        Item::Choice { key, index, label, swatch, chosen } => {
            let ink = argb(INK_BRIGHT);
            let mut x = r.left() + 16.0;
            match swatch {
                Swatch::No => {}
                Swatch::Colour(c) => {
                    painter.circle_filled(pos2(x + 9.0, r.center().y), 9.0, *c);
                    x += 28.0;
                }
                Swatch::Clear => {
                    look.icon_on(ui, painter, &grid_icons::GRID_HIGHLIGHT, pos2(x + 9.0, r.center().y), 20.0, ink);
                    x += 28.0;
                }
            }
            text(painter, pos2(x, r.center().y), Align2::LEFT_CENTER, label, 15.0, Weight::Regular, ink);
            if *chosen {
                let at = pos2(r.right() - 16.0 - 9.0, r.center().y);
                painter.circle_filled(at, 9.0, argb(pc::ACCENT));
                let tick = vec![at + vec2(-4.0, 0.0), at + vec2(-1.0, 3.0), at + vec2(4.5, -3.5)];
                painter.add(Shape::line(tick, Stroke::new(1.8, Color32::WHITE)));
            }
            if clicked(ui, r, "choice") {
                hits.push(Hit::Choice { key, index: *index });
            }
        }
        Item::Toggle { key, label, on } => {
            let ink = argb(INK_BRIGHT);
            text(painter, pos2(r.left() + 14.0, r.center().y), Align2::LEFT_CENTER, label, 16.0, Weight::Regular, ink);
            let pill = Rect::from_center_size(pos2(r.right() - 14.0 - 22.0, r.center().y), vec2(44.0, 26.0));
            let fill = if *on { argb(pc::ACCENT) } else { argb(RULE) };
            painter.rect_filled(pill, 13, fill);
            let knob = if *on { pill.right() - 13.0 } else { pill.left() + 13.0 };
            painter.circle_filled(pos2(knob, pill.center().y), 11.0, Color32::WHITE);
            if clicked(ui, r, "toggle") {
                hits.push(Hit::Row(key));
            }
        }
        Item::Value { key, label, value, lit } => {
            let ink = argb(INK_BRIGHT);
            text(painter, pos2(r.left() + 16.0, r.center().y), Align2::LEFT_CENTER, label, 15.0, Weight::Regular, ink);
            let value_ink = if *lit { argb(pc::ACCENT) } else { ink };
            text(
                painter,
                pos2(r.right() - 16.0, r.center().y),
                Align2::RIGHT_CENTER,
                value,
                15.0,
                Weight::Regular,
                value_ink,
            );
            if clicked(ui, r, "value") {
                hits.push(Hit::Row(key));
            }
        }
        Item::Keypad { key, unit, from, most, fps } => {
            keypad(ui, painter, menu, r, id, whole, (key, unit, *from, *most, *fps), hits);
        }
    }
}

/// A number typed on its own keys: what is typed so far over them (`from` until one is typed),
/// and Set to take it, between 1 and `most`.
#[allow(clippy::too_many_arguments)]
fn keypad(
    ui: &Ui,
    painter: &egui::Painter,
    menu: &mut Menu,
    r: Rect,
    id: Id,
    whole: bool,
    (key, unit, from, most, fps): (&'static str, &str, u32, u32, Option<u32>),
    hits: &mut Vec<Hit>,
) {
    let inner = r.shrink2(vec2(10.0, 2.0));
    let number = menu.typed.parse::<u32>().ok();
    let valid = number.is_some_and(|n| (1..=most).contains(&n));
    let line = inner.top() + NUMBER_LINE / 2.0;
    let (shown, ink) =
        if menu.typed.is_empty() { (from.to_string(), argb(INK_DIM)) } else { (menu.typed.clone(), argb(INK_BRIGHT)) };
    let at = text(painter, pos2(inner.left() + 8.0, line), Align2::LEFT_CENTER, &shown, 22.0, Weight::Bold, ink);
    text(painter, pos2(at.right() + 5.0, line + 2.0), Align2::LEFT_CENTER, unit, 13.0, Weight::Regular, argb(INK));
    if let Some(fps) = fps {
        let note = crate::timeline_settings::clock(number.unwrap_or(from), fps);
        text(
            painter,
            pos2(inner.right() - 8.0, line + 2.0),
            Align2::RIGHT_CENTER,
            &note,
            13.0,
            Weight::Regular,
            argb(INK),
        );
    }
    let keys: [[&str; 3]; 4] = [["7", "8", "9"], ["4", "5", "6"], ["1", "2", "3"], ["0", "⌫", "Set"]];
    let width = (inner.width() - 2.0 * KEY_GAP) / 3.0;
    for (row, labels) in keys.iter().enumerate() {
        let top = inner.top() + NUMBER_LINE + KEY_GAP + (KEY + KEY_GAP) * row as f32;
        for (col, label) in labels.iter().enumerate() {
            let rect = Rect::from_min_size(pos2(inner.left() + (width + KEY_GAP) * col as f32, top), vec2(width, KEY));
            let set = *label == "Set";
            let fill = match (set, valid) {
                (true, true) => argb(pc::ACCENT),
                (true, false) => argb(pc::ACCENT).gamma_multiply(0.35),
                _ => argb(KEY_FILL),
            };
            painter.rect_filled(rect, KEY / 2.0, fill);
            let (ink, size) = if set { (Color32::WHITE, 16.0) } else { (argb(INK_BRIGHT), 19.0) };
            let weight = if set { Weight::Bold } else { Weight::Regular };
            text(painter, rect.center(), Align2::CENTER_CENTER, label, size, weight, ink);
            if !whole || !ui.interact(rect, id.with(("key", row, col)), Sense::click()).clicked() {
                continue;
            }
            match *label {
                "Set" => {
                    if let Some(n) = number.filter(|_| valid) {
                        hits.push(Hit::Keyed { key, n });
                    }
                }
                "⌫" => _ = menu.typed.pop(),
                digit => {
                    if let Ok(n) = digit.parse::<u32>() {
                        press(menu, n);
                    }
                }
            }
        }
    }
}

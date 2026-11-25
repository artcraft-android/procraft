//! A block's Keys card, as the iPad's (`GridKeysCard` in TimelineGridKeys.swift, and
//! AdjustmentKeyRows.swift): opened by a frame's menu's Keys ([`TimelineGrid::keys_card_for`]), it
//! keys the block on the playhead's frame, the playhead brought onto the block first if it isn't.
//!
//! A key of position, scale or rotation put there or taken away (a row each, ticked where there is
//! one); where it is there set by slider (moved across and down, its scale, its turn, its opacity),
//! each keying it as it moves and one "Key" undo step let go; an adjustment clip's own values, a
//! slider each (`set_timeline_adjustment_value`); how new keys ease (autokey's too); and its keys
//! copied, pasted, or those on the playhead's frame taken away. Escape or its × closes it.

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use procraft_core::{
    AdjustmentUnit, CanvasSession, TimelineChannel, TimelineKeyEdit, TimelineKeyLane, TimelineKeyPick,
};

use super::TimelineGrid;
use super::grid::Place;
use super::keys::{EASINGS, card_frame, menu_row};
use crate::look::{Look, Themed, Weight, argb, pc, text};

const WIDTH: f32 = 290.0;
const TITLE: f32 = 46.0;
const ROW: f32 = 40.0;
const SLIDER: f32 = 52.0;
const CHIP: f32 = 30.0;
const INSET: f32 = 8.0;

const LABEL: Themed = Themed(0xFFC1C1C1, 0xFF3D3D3D);
const VALUE: Themed = Themed(0xFFEDEDED, 0xFF3D3D3D);
const TRACK: Themed = Themed(0xFF414141, 0xFFBEBEBE);
const CHIP_FILL: Themed = Themed(0xFF3A3A3A, 0xFFDADADA);

/// The card's own state: the slider held and the value it has set (not yet let go), and how far
/// it is scrolled.
#[derive(Default)]
pub(super) struct Card {
    /// Which block it was opened for (the playhead brought onto it then).
    opened: Option<u64>,
    live: Option<(usize, f32)>,
    scroll: f32,
}

/// A slider of the card: its name, where its value is along it (0..1), and its value as said.
struct Slider {
    label: String,
    share: f32,
    said: String,
}

/// What a row of the card asked.
enum Asked {
    Toggle(TimelineChannel),
    Slide(usize, f32, bool),
    Easing(procraft_core::TimelineEasing),
    Copy,
    Paste,
    Remove,
    Close,
}

/// An adjustment value as its row says it.
fn said(unit: AdjustmentUnit, v: f32) -> String {
    match unit {
        AdjustmentUnit::Plain => format!("{v:.2}"),
        AdjustmentUnit::Pixels => format!("{} px", v.round() as i64),
        AdjustmentUnit::Percent => format!("{} %", (v * 100.0).round() as i64),
        AdjustmentUnit::Degrees => format!("{}°", v.to_degrees().round() as i64),
        AdjustmentUnit::Stops => format!("{v:+.1} stops"),
    }
}

impl TimelineGrid {
    /// The Keys card, while [`TimelineGrid::keys_card_for`] names a block. Returns what to tell.
    pub(super) fn keys_card(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        place: &Place,
    ) -> Option<&'static str> {
        let Some(id) = self.keys_card_for else {
            self.card = Card::default();
            return None;
        };
        let clip = self.panel.as_ref()?.rows.iter().flat_map(|r| r.track.clips.iter()).find(|c| c.id == id).cloned();
        let Some(clip) = clip else {
            self.keys_card_for = None;
            return None;
        };
        // keys are set on the playhead's frame: it is brought onto the block
        if self.card.opened != Some(id) {
            self.card = Card { opened: Some(id), ..Card::default() };
            let frame = self.frame();
            let end = clip.start + clip.duration.max(1);
            if frame < clip.start || frame >= end {
                self.go_to(session, if frame < clip.start { clip.start } else { end - 1 });
                self.read_at = None;
            }
        }
        if crate::ui::escape_first(ctx) {
            self.keys_card_for = None;
            return None;
        }
        let frame = self.frame();
        let inside = frame >= clip.start && frame < clip.start + clip.duration;
        let keys = self.panel.as_ref()?.keys.iter().find(|k| k.id == id).map(|k| k.keys.clone()).unwrap_or_default();
        let here: Vec<TimelineChannel> = keys.iter().filter(|k| k.frame == frame).map(|k| k.channel).collect();
        let pasting = self.panel.as_ref().is_some_and(|p| p.clipboard.keys);
        let info = session.info();
        let (w, h) = (info.width as f32, info.height as f32);
        let pose = session.timeline_pose(id);
        let adjustments = if inside { session.timeline_adjustment_values(id) } else { Vec::new() };
        // the sliders: where it is, then its adjustment's values
        let live = self.card.live;
        let value = |i: usize, v: f32| live.filter(|l| l.0 == i).map_or(v, |l| l.1);
        let mut sliders = Vec::new();
        if let Some(p) = pose.as_ref() {
            let across = value(0, (p.x / w + 1.0) / 2.0);
            let down = value(1, (p.y / h + 1.0) / 2.0);
            let scale = value(2, p.scale_x.clamp(0.1, 10.0).log10() / 2.0 + 0.5);
            let turn = value(3, (p.rotation + 360.0) / 720.0);
            let opacity = value(4, p.opacity);
            sliders.push(Slider {
                label: "Move across".into(),
                share: across,
                said: format!("{} px", ((across * 2.0 - 1.0) * w).round() as i64),
            });
            sliders.push(Slider {
                label: "Move down".into(),
                share: down,
                said: format!("{} px", ((down * 2.0 - 1.0) * h).round() as i64),
            });
            sliders.push(Slider {
                label: "Scale".into(),
                share: scale,
                said: format!("{} %", (10f32.powf((scale - 0.5) * 2.0) * 100.0).round() as i64),
            });
            sliders.push(Slider {
                label: "Rotation".into(),
                share: turn,
                said: format!("{}°", (turn * 720.0 - 360.0).round() as i64),
            });
            sliders.push(Slider {
                label: "Opacity".into(),
                share: opacity,
                said: format!("{} %", (opacity * 100.0).round() as i64),
            });
        }
        let posed = sliders.len();
        for (i, v) in adjustments.iter().enumerate() {
            let span = v.high - v.low;
            let share = if span > 0.0 { (v.value - v.low) / span } else { 0.0 };
            let share = value(posed + i, share);
            sliders.push(Slider { label: v.name.clone(), share, said: said(v.unit, v.low + share * span) });
        }
        let toggles = [
            (TimelineChannel::Position, "Key position here"),
            (TimelineChannel::Scale, "Key scale here"),
            (TimelineChannel::Rotation, "Key rotation here"),
        ];
        let content = TITLE
            + ROW * toggles.len() as f32
            + INSET
            + SLIDER * sliders.len() as f32
            + INSET
            + 22.0
            + CHIP * 2.0
            + 6.0
            + INSET
            + ROW * 3.0
            + INSET;
        // over the bar, at the playhead
        let screen = ctx.content_rect();
        let bottom = place.bar.top() - 8.0;
        let room = (bottom - screen.top() - crate::panel::bar_shown(ctx) - 8.0).max(200.0);
        let height = content.min(room);
        let left = (place.bar.center().x - WIDTH / 2.0).clamp(screen.left() + 8.0, screen.right() - WIDTH - 8.0);
        let card = Rect::from_min_max(pos2(left, bottom - height), pos2(left + WIDTH, bottom));
        self.card.scroll = self.card.scroll.clamp(0.0, (content - height).max(0.0));
        let mut asked = None;
        let name = clip.name.clone();
        let easing = self.keys.easing;
        egui::Area::new(Id::unique("timeline keys card")).fixed_pos(card.min).order(Order::Foreground).show(
            ctx,
            |ui| {
                ui.allocate_rect(card, Sense::click());
                crate::screen::claim(ui, card);
                card_frame(ui, card);
                if ui.rect_contains_pointer(card) {
                    self.card.scroll = (self.card.scroll - ui.input(|i| i.smooth_scroll_delta.y))
                        .clamp(0.0, (content - height).max(0.0));
                }
                let ui = &mut *ui;
                ui.set_clip_rect(card.shrink(1.0));
                let mut y = card.top() - self.card.scroll;
                // the title, and its ×
                text(
                    ui.painter(),
                    pos2(card.left() + 18.0, y + TITLE / 2.0),
                    Align2::LEFT_CENTER,
                    &format!("Keys: {name}"),
                    18.0,
                    Weight::Bold,
                    argb(VALUE),
                );
                let close = Rect::from_center_size(pos2(card.right() - 24.0, y + TITLE / 2.0), vec2(32.0, 32.0));
                let x = Stroke::new(1.8, argb(LABEL));
                ui.painter().line_segment([close.center() - vec2(6.0, 6.0), close.center() + vec2(6.0, 6.0)], x);
                ui.painter().line_segment([close.center() + vec2(-6.0, 6.0), close.center() + vec2(6.0, -6.0)], x);
                if ui.interact(close, ui.scope_id().with("close"), Sense::click()).clicked() {
                    asked = Some(Asked::Close);
                }
                y += TITLE;
                for (i, (channel, label)) in toggles.iter().enumerate() {
                    let row = Rect::from_min_size(pos2(card.left(), y), vec2(WIDTH, ROW));
                    if menu_row(ui, row, label, inside, false, false, false, ("keys card toggle", i)) {
                        asked = Some(Asked::Toggle(*channel));
                    }
                    if here.contains(channel) {
                        tick(ui, pos2(row.right() - 26.0, row.center().y));
                    }
                    y += ROW;
                }
                y += INSET;
                for (i, s) in sliders.iter().enumerate() {
                    let row = Rect::from_min_size(pos2(card.left(), y), vec2(WIDTH, SLIDER));
                    if let Some((at, done)) = slider(ui, row, s, inside, i) {
                        asked = Some(Asked::Slide(i, at, done));
                    }
                    y += SLIDER;
                }
                y += INSET;
                text(
                    ui.painter(),
                    pos2(card.left() + 18.0, y + 10.0),
                    Align2::LEFT_CENTER,
                    "New keys ease",
                    14.0,
                    Weight::Regular,
                    argb(LABEL),
                );
                y += 22.0;
                let chip_width = (WIDTH - 2.0 * 14.0 - 2.0 * 6.0) / 3.0;
                for (i, (e, label)) in EASINGS.iter().enumerate() {
                    let (col, line) = ((i % 3) as f32, (i / 3) as f32);
                    let chip = Rect::from_min_size(
                        pos2(card.left() + 14.0 + col * (chip_width + 6.0), y + line * (CHIP + 6.0)),
                        vec2(chip_width, CHIP),
                    );
                    let on = *e == easing;
                    ui.painter().rect_filled(chip, 8, if on { argb(pc::ACCENT) } else { argb(CHIP_FILL) });
                    let short = if *e == procraft_core::TimelineEasing::EaseInOut { "In and out" } else { label };
                    let ink = if on { Color32::WHITE } else { argb(VALUE) };
                    text(ui.painter(), chip.center(), Align2::CENTER_CENTER, short, 13.0, Weight::Regular, ink);
                    if ui.interact(chip, ui.scope_id().with(("keys card easing", i)), Sense::click()).clicked() {
                        asked = Some(Asked::Easing(*e));
                    }
                }
                y += CHIP * 2.0 + 6.0 + INSET;
                ui.painter().rect_filled(
                    Rect::from_min_size(pos2(card.left() + 15.0, y - 4.0), vec2(WIDTH - 30.0, 1.0)),
                    0,
                    argb(TRACK),
                );
                let rows = [
                    ("Copy Keys", !keys.is_empty(), false, 0),
                    ("Paste Keys", pasting, false, 1),
                    ("Remove Keys Here", !here.is_empty(), true, 2),
                ];
                for (label, enabled, danger, i) in rows {
                    let row = Rect::from_min_size(pos2(card.left(), y), vec2(WIDTH, ROW));
                    if menu_row(ui, row, label, enabled, danger, false, false, ("keys card row", i)) {
                        asked =
                            Some([Asked::Copy, Asked::Paste, Asked::Remove].into_iter().nth(i).unwrap_or(Asked::Close));
                    }
                    y += ROW;
                }
                let _ = look;
            },
        );
        let asked = asked?;
        let mut said = None;
        match asked {
            Asked::Close => self.keys_card_for = None,
            Asked::Toggle(channel) => {
                if here.contains(&channel) {
                    let pick = TimelineKeyPick { lane: TimelineKeyLane::Clip { id, channel }, frame };
                    session.edit_timeline_keys(vec![pick], TimelineKeyEdit::Remove);
                } else if let Some(p) = pose.as_ref() {
                    let values = match channel {
                        TimelineChannel::Position => vec![p.x, p.y],
                        TimelineChannel::Scale => vec![p.scale_x, p.scale_y],
                        _ => vec![p.rotation],
                    };
                    session.set_timeline_key(id, channel, values, self.keys.easing, true);
                }
                self.read_at = None;
            }
            Asked::Slide(i, at, done) => {
                self.stop(session, false);
                self.card.live = (!done).then_some((i, at));
                if let Some(p) = pose.as_ref().filter(|_| i < posed) {
                    let (channel, values) = match i {
                        0 => (TimelineChannel::Position, vec![(at * 2.0 - 1.0) * w, p.y]),
                        1 => (TimelineChannel::Position, vec![p.x, (at * 2.0 - 1.0) * h]),
                        2 => {
                            let s = 10f32.powf((at - 0.5) * 2.0);
                            (TimelineChannel::Scale, vec![s, s])
                        }
                        3 => (TimelineChannel::Rotation, vec![at * 720.0 - 360.0]),
                        _ => (TimelineChannel::Opacity, vec![at]),
                    };
                    session.set_timeline_key(id, channel, values, self.keys.easing, done);
                } else if let Some(v) = i.checked_sub(posed).and_then(|j| adjustments.get(j)) {
                    let value = v.low + at * (v.high - v.low);
                    let index = u32::try_from(i - posed).unwrap_or(0);
                    session.set_timeline_adjustment_value(id, index, value, self.keys.easing, done);
                }
                if done {
                    self.read_at = None;
                }
            }
            Asked::Easing(e) => {
                self.keys.easing = e;
                session.set_timeline_autokey_easing(e);
            }
            Asked::Copy => {
                session.copy_timeline_keys(id, false);
                self.read_at = None;
            }
            Asked::Paste => {
                self.stop(session, false);
                if !session.paste_timeline_keys() {
                    said = Some("Move the playhead onto a clip to paste keys onto it.");
                }
                self.read_at = None;
            }
            Asked::Remove => {
                session.remove_timeline_keys(id);
                self.read_at = None;
            }
        }
        said
    }
}

/// A tick at `at`.
fn tick(ui: &Ui, at: Pos2) {
    let line = vec![at + vec2(-6.0, 0.0), at + vec2(-2.0, 4.5), at + vec2(6.0, -5.0)];
    ui.painter().add(egui::Shape::line(line, Stroke::new(2.0, argb(pc::ACCENT))));
}

/// Slider `s` in `row`: its name and value over a bar dragged or clicked along (dim, and taking
/// nothing, when not `enabled`). Where it was set (0..1) while held, and whether it was let go.
fn slider(ui: &Ui, row: Rect, s: &Slider, enabled: bool, id: usize) -> Option<(f32, bool)> {
    let (left, right) = (row.left() + 18.0, row.right() - 18.0);
    let dim = |c: Color32| if enabled { c } else { c.gamma_multiply(0.45) };
    text(
        ui.painter(),
        pos2(left, row.top() + 5.0),
        Align2::LEFT_TOP,
        &s.label,
        15.0,
        Weight::Regular,
        dim(argb(LABEL)),
    );
    text(
        ui.painter(),
        pos2(right, row.top() + 5.0),
        Align2::RIGHT_TOP,
        &s.said,
        15.0,
        Weight::Regular,
        dim(argb(VALUE)),
    );
    let y = row.top() + 36.0;
    let bar = Rect::from_min_max(pos2(left - 8.0, y - 11.0), pos2(right + 8.0, y + 11.0));
    let x = left + s.share.clamp(0.0, 1.0) * (right - left);
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(right, y + 1.5)), 0, dim(argb(TRACK)));
    ui.painter().rect_filled(Rect::from_min_max(pos2(left, y - 1.5), pos2(x, y + 1.5)), 0, dim(argb(pc::ACCENT)));
    ui.painter().rect_filled(Rect::from_center_size(pos2(x, y), vec2(16.0, 10.0)), 5, dim(Color32::WHITE));
    if !enabled {
        return None;
    }
    let response = ui.interact(bar, ui.scope_id().with(("keys card slider", id)), Sense::click_and_drag());
    let at = |p: Pos2| ((p.x - left) / (right - left).max(1.0)).clamp(0.0, 1.0);
    if response.drag_stopped() || response.clicked() {
        let p = response.interact_pointer_pos().or(ui.input(|i| i.pointer.latest_pos()))?;
        return Some((at(p), true));
    }
    if response.is_pointer_button_down_on() {
        let p = response.interact_pointer_pos()?;
        return Some((at(p), false));
    }
    None
}

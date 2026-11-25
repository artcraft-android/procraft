//! Keys on the grid, as the iPad's (TimelineGridKeys.swift, TimelineKeyLanes.swift). A block's
//! keyed frames have a mark under them (drawn with its row). A track whose blocks have keys has a
//! chevron after its name: clicked, its rows of keys stand under it, one for each kind any of its
//! blocks has (Position, Scale, Rotation, Opacity, Adjustment; and its sounds' level), a mark on
//! each keyed frame: a diamond, a square where it holds, a dot where it eases.
//!
//! A mark clicked is picked (the playhead going to it) or let go; a row's name, all its keys or
//! none; the row elsewhere, every key let go. Right-clicked or held, its menu: Easing (the simple
//! ones, My curves, and the easing editor), copy, paste, reverse and delete, on the keys picked
//! with it. Dragged, it and the keys picked go along by frames, shown from a copy
//! (`timeline_keys_edit_preview`) and one undo step on letting go (`edit_timeline_keys`). With
//! keys on two frames or more picked, a grip either side stretches them in time about the far end.

use std::collections::HashSet;

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};
use procraft_core::{
    CanvasSession, EasingSpec, TimelineChannel, TimelineEasing, TimelineKeyEdit, TimelineKeyLane, TimelineKeyPick,
    TimelineKeysEdited, TimelineLaneKey, TimelinePanel, TimelineRow,
};
use procraft_engine::timeline::Easing;

use super::TimelineGrid;
use super::grid::{CELL_WIDTH, INDENT, LABELS_WIDTH, PITCH, Place};
use super::rows::{GridRow, KeyRow};
use crate::easing_editor::{Done, EasingEditor};
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::saved_curves::SavedCurves;

/// A row of keys' height.
pub(super) const KEYS_HEIGHT: f32 = 28.0;
/// The menu's width, a row's height, and its corners.
const MENU_WIDTH: f32 = 236.0;
const MENU_ROW: f32 = 40.0;
const MENU_CORNER: u8 = 16;

const INK: Themed = Themed(0xFFC5C5C5, 0xFF393939);
const INK_DIM: Themed = Themed(0x80C5C5C5, 0x80393939);
const SLOT: Themed = Themed(0xFF161616, 0xFFD1D1D1);
const CARD: Themed = Themed(0xFA2A2A2A, 0xFAF0F0F0);
const CARD_EDGE: Themed = Themed(0xFF474747, 0xFFC0C0C0);
const CARD_LINE: Themed = Themed(0xFF3A3A3A, 0xFFDDDDDD);
const MENU_INK: Themed = Themed(0xFFF2F2F2, 0xFF1C1C1C);
const MENU_DIM: Themed = Themed(0x66FFFFFF, 0x66000000);
const DANGER: u32 = 0xFFFF453A;

/// A kind of key, and so a row of keys: one of a clip's channels, or a sound's level.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Channel(TimelineChannel),
    Volume,
}

/// The kinds, in the order their rows stand.
pub(super) const KINDS: [Kind; 6] = [
    Kind::Channel(TimelineChannel::Position),
    Kind::Channel(TimelineChannel::Scale),
    Kind::Channel(TimelineChannel::Rotation),
    Kind::Channel(TimelineChannel::Opacity),
    Kind::Channel(TimelineChannel::Adjustment),
    Kind::Volume,
];

impl Kind {
    /// What its row is called.
    pub(super) fn said(self) -> &'static str {
        match self {
            Kind::Channel(TimelineChannel::Position) => "Position",
            Kind::Channel(TimelineChannel::Scale) => "Scale",
            Kind::Channel(TimelineChannel::Rotation) => "Rotation",
            Kind::Channel(TimelineChannel::Opacity) => "Opacity",
            Kind::Channel(TimelineChannel::Adjustment) => "Adjustment",
            Kind::Volume => "Volume",
        }
    }
}

/// The simple easings, as the menu names them.
pub(super) const EASINGS: [(TimelineEasing, &str); 6] = [
    (TimelineEasing::Linear, "Linear"),
    (TimelineEasing::Hold, "Hold"),
    (TimelineEasing::Smooth, "Smooth"),
    (TimelineEasing::EaseIn, "Ease in"),
    (TimelineEasing::EaseOut, "Ease out"),
    (TimelineEasing::EaseInOut, "Ease in and out"),
];

/// Row `row`'s keys of kind `kind`, every block's on its track, in frame order.
pub(super) fn row_keys(panel: &TimelinePanel, row: &TimelineRow, kind: Kind) -> Vec<TimelineLaneKey> {
    let mut out = Vec::new();
    for c in &row.track.clips {
        match kind {
            Kind::Channel(channel) => {
                let Some(keys) = panel.keys.iter().find(|k| k.id == c.id) else { continue };
                out.extend(keys.keys.iter().filter(|k| k.channel == channel).map(|k| TimelineLaneKey {
                    lane: TimelineKeyLane::Clip { id: c.id, channel },
                    frame: k.frame,
                    easing: k.easing,
                }));
            }
            Kind::Volume => {
                if let Some(v) = panel.volumes.iter().find(|v| v.id == c.id) {
                    out.extend(v.keys.iter().copied());
                }
            }
        }
    }
    out.sort_by_key(|k| k.frame);
    out
}

/// The kinds of key the blocks on row `row`'s track have, in the order their rows stand.
pub(super) fn kinds(panel: &TimelinePanel, row: &TimelineRow) -> Vec<Kind> {
    KINDS.into_iter().filter(|k| !row_keys(panel, row, *k).is_empty()).collect()
}

/// Whether any block on row `row`'s track has keys (a sound's level among them).
pub(super) fn keyed(panel: &TimelinePanel, row: &TimelineRow) -> bool {
    row.track.clips.iter().any(|c| {
        panel.keys.iter().any(|k| k.id == c.id && !k.keys.is_empty())
            || panel.volumes.iter().any(|v| v.id == c.id && !v.keys.is_empty())
    })
}

/// A key's menu, and what stands beside it.
pub(crate) struct MarkMenu {
    pub(super) key: TimelineKeyPick,
    /// Where the mark is (window points): the menu stands over it.
    at: Pos2,
    beside: Option<Beside>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Beside {
    /// The easings to pick from.
    Easing,
    /// The easing editor, on how the key's clip eases from its frame (as it is being changed).
    Curve,
}

/// What a row of the menus asks.
#[derive(Clone, Debug, PartialEq)]
enum Asked {
    Easing,
    Ease(TimelineEasing),
    Curve(EasingSpec),
    EditCurve,
    Copy,
    Paste,
    Reverse,
    Delete,
}

/// Keys on the grid: the tracks whose rows of keys are out, the keys picked, what a drag on them
/// would do, a key's menu, and how new keys ease.
pub(crate) struct Keys {
    pub(super) out: HashSet<u64>,
    pub(crate) picks: Vec<TimelineKeyPick>,
    /// A drag under way: its keys, and what it asks of them; the lanes it changes, as it would
    /// leave them.
    dragging: Option<(Vec<TimelineKeyPick>, Option<TimelineKeyEdit>)>,
    pub(crate) preview: Option<TimelineKeysEdited>,
    pub(crate) menu: Option<MarkMenu>,
    editor: EasingEditor,
    /// The easing the editor changes, as it is.
    editing: Option<Easing>,
    /// My curves (opened when first wanted).
    pub(crate) curves: Option<SavedCurves>,
    /// How new keys ease: the Keys card's choice (or a key's menu's), autokey's too.
    pub(crate) easing: TimelineEasing,
}

impl Default for Keys {
    fn default() -> Keys {
        Keys {
            out: HashSet::new(),
            picks: Vec::new(),
            dragging: None,
            preview: None,
            menu: None,
            editor: EasingEditor::default(),
            editing: None,
            curves: None,
            easing: TimelineEasing::Smooth,
        }
    }
}

impl Keys {
    /// My curves, read from where the app keeps them the first time (in tests, none kept).
    pub(super) fn curves(&mut self) -> &mut SavedCurves {
        self.curves.get_or_insert_with(|| if cfg!(test) { SavedCurves::default() } else { SavedCurves::open() })
    }

    /// The lanes whose rows are out (each block's channels on a track out, its sounds' level).
    fn lanes_out(&self, panel: &TimelinePanel) -> Vec<TimelineKeyLane> {
        let mut lanes = Vec::new();
        for row in panel.rows.iter().filter(|r| self.out.contains(&r.track.id)) {
            for c in &row.track.clips {
                for kind in KINDS {
                    lanes.push(match kind {
                        Kind::Channel(channel) => TimelineKeyLane::Clip { id: c.id, channel },
                        Kind::Volume => TimelineKeyLane::Volume { id: c.id },
                    });
                }
            }
        }
        lanes
    }

    /// The keys picked whose rows are out.
    pub(super) fn shown(&self, panel: &TimelinePanel) -> Vec<TimelineKeyPick> {
        let lanes = self.lanes_out(panel);
        self.picks.iter().copied().filter(|p| lanes.contains(&p.lane)).collect()
    }

    /// The keys picked as a drag would leave them.
    pub(super) fn shown_picks(&self, panel: &TimelinePanel) -> Vec<TimelineKeyPick> {
        let shown = self.shown(panel);
        let Some(preview) = self.preview.as_ref() else { return shown };
        let mut out = preview.picks.clone();
        out.extend(shown.into_iter().filter(|p| !preview.lanes.contains(&p.lane)));
        out
    }

    /// Keys `keys` (a row's) as they show: the lanes a drag under way changes, as it would leave
    /// them.
    pub(super) fn shown_keys(&self, keys: Vec<TimelineLaneKey>) -> Vec<TimelineLaneKey> {
        let Some(preview) = self.preview.as_ref() else { return keys };
        let lanes: Vec<TimelineKeyLane> = keys.iter().map(|k| k.lane).collect();
        let mut out: Vec<TimelineLaneKey> = keys.into_iter().filter(|k| !preview.lanes.contains(&k.lane)).collect();
        out.extend(preview.keys.iter().filter(|k| preview.lanes.contains(&k.lane) && lanes.contains(&k.lane)).copied());
        out.sort_by_key(|k| k.frame);
        out
    }

    /// After a reading: the picks on keys no longer there let go.
    pub(super) fn refreshed(&mut self, panel: &TimelinePanel) {
        if self.picks.is_empty() {
            return;
        }
        let mut there = Vec::new();
        for k in &panel.keys {
            there.extend(k.keys.iter().map(|x| TimelineKeyPick {
                lane: TimelineKeyLane::Clip { id: k.id, channel: x.channel },
                frame: x.frame,
            }));
        }
        for v in &panel.volumes {
            there.extend(v.keys.iter().map(|x| TimelineKeyPick { lane: x.lane, frame: x.frame }));
        }
        self.picks.retain(|p| there.contains(p));
    }

    /// Key `key` clicked: taken in with the others, or out. True when taken in.
    fn tap(&mut self, key: TimelineKeyPick) -> bool {
        self.cancel_drag();
        if let Some(i) = self.picks.iter().position(|p| *p == key) {
            self.picks.remove(i);
            false
        } else {
            self.picks.push(key);
            true
        }
    }

    /// Every key of `keys` (a row's) picked, or, all picked already, none of them.
    fn toggle_all(&mut self, keys: &[TimelineLaneKey]) {
        self.cancel_drag();
        let row: Vec<TimelineKeyPick> = keys.iter().map(|k| TimelineKeyPick { lane: k.lane, frame: k.frame }).collect();
        if row.iter().all(|p| self.picks.contains(p)) {
            self.picks.retain(|p| !row.contains(p));
        } else {
            for p in row {
                if !self.picks.contains(&p) {
                    self.picks.push(p);
                }
            }
        }
    }

    /// Key `key` is one of those picked that show; if it isn't, it alone is picked.
    fn take(&mut self, panel: &TimelinePanel, key: TimelineKeyPick) {
        if !self.shown(panel).contains(&key) {
            self.picks = vec![key];
        }
    }

    /// `edit` done to keys `taken` as one undo step: they stay picked where they went. False if
    /// nothing changed (no room that way, or locked).
    fn commit(&mut self, session: &CanvasSession, edit: TimelineKeyEdit, taken: Vec<TimelineKeyPick>) -> bool {
        match session.edit_timeline_keys(taken.clone(), edit) {
            Some(ended) => {
                self.picks.retain(|p| !taken.contains(p));
                for p in ended {
                    if !self.picks.contains(&p) {
                        self.picks.push(p);
                    }
                }
                true
            }
            None => false,
        }
    }

    /// `edit` done to the keys picked that show, as one undo step.
    pub(super) fn apply(&mut self, session: &CanvasSession, panel: &TimelinePanel, edit: TimelineKeyEdit) -> bool {
        let taken = self.shown(panel);
        !taken.is_empty() && self.commit(session, edit, taken)
    }

    /// Key `key` dragged `by` frames: it, or every key picked that shows if it is one of them.
    /// Shown, not done, until [`Keys::end_drag`].
    fn drag(&mut self, session: &CanvasSession, panel: &TimelinePanel, key: TimelineKeyPick, by: i32) {
        if self.dragging.is_none() {
            if !self.shown(panel).contains(&key) {
                self.picks = vec![key];
            }
            self.dragging = Some((self.shown(panel), None));
        }
        self.show_edit(session, (by != 0).then_some(TimelineKeyEdit::Move { by }));
    }

    /// A stretch grip dragged: the picked keys' distance from frame `anchor` times `factor`.
    fn stretch(&mut self, session: &CanvasSession, panel: &TimelinePanel, anchor: u32, factor: f64) {
        if self.dragging.is_none() {
            let taken = self.shown(panel);
            if taken.is_empty() {
                return;
            }
            self.dragging = Some((taken, None));
        }
        let edit = ((factor - 1.0).abs() >= 0.001).then_some(TimelineKeyEdit::Stretch { anchor, factor });
        self.show_edit(session, edit);
    }

    fn show_edit(&mut self, session: &CanvasSession, edit: Option<TimelineKeyEdit>) {
        let Some((picks, asked)) = self.dragging.as_mut() else { return };
        let same = match (edit, *asked) {
            (Some(TimelineKeyEdit::Move { by: a }), Some(TimelineKeyEdit::Move { by: b })) => a == b,
            (
                Some(TimelineKeyEdit::Stretch { anchor: a, factor: f }),
                Some(TimelineKeyEdit::Stretch { anchor: b, factor: g }),
            ) => a == b && (f - g).abs() < 0.005,
            (None, None) => true,
            _ => false,
        };
        if same {
            return;
        }
        *asked = edit;
        let picks = picks.clone();
        self.preview = edit.and_then(|e| session.timeline_keys_edit_preview(picks, e));
    }

    /// The drag let go: what it showed, done as one undo step.
    fn end_drag(&mut self, session: &CanvasSession) -> bool {
        let Some((picks, edit)) = self.dragging.take() else { return false };
        let went = self.preview.take().is_some();
        match edit {
            Some(edit) if went => self.commit(session, edit, picks),
            _ => false,
        }
    }

    /// A drag taken away before it was let go: nothing done, nothing shown.
    pub(super) fn cancel_drag(&mut self) {
        self.dragging = None;
        self.preview = None;
    }
}

/// Where the grips stand beside the keys picked: the first and last frames they are on, and the
/// rows of keys from top to bottom (window points).
#[derive(Clone, Copy, Debug)]
pub(super) struct Grips {
    pub(super) first: u32,
    pub(super) last: u32,
    top: f32,
    bottom: f32,
}

impl Grips {
    /// A grip's middle across: a little clear of the keys' own marks.
    pub(super) fn x(&self, place: &Place, leading: bool) -> f32 {
        if leading {
            place.x(self.first as f32) + CELL_WIDTH / 2.0 - 18.0
        } else {
            place.x(self.last as f32) + CELL_WIDTH / 2.0 + 18.0
        }
    }

    /// Which grip is at `at`, if one is: the first's (true) or the last's.
    fn grip_at(&self, place: &Place, at: Pos2) -> Option<bool> {
        if at.y < self.top || at.y >= self.bottom {
            return None;
        }
        if (at.x - self.x(place, false)).abs() <= 11.0 {
            return Some(false);
        }
        ((at.x - self.x(place, true)).abs() <= 11.0).then_some(true)
    }

    pub(super) fn rect(&self, place: &Place, leading: bool) -> Rect {
        Rect::from_min_max(
            pos2(self.x(place, leading) - 3.0, self.top + 4.0),
            pos2(self.x(place, leading) + 3.0, self.bottom - 4.0),
        )
    }
}

/// A key's mark at `at`: a diamond, a square where it holds, a dot where it eases.
fn mark(painter: &egui::Painter, at: Pos2, easing: TimelineEasing, fill: Color32, edge: Option<Color32>) {
    let stroke = edge.map_or(Stroke::NONE, |c| Stroke::new(1.5, c));
    match easing {
        TimelineEasing::Hold => {
            let r = Rect::from_center_size(at, vec2(9.0, 9.0));
            painter.rect_filled(r, 0, fill);
            painter.rect_stroke(r, 0, stroke, StrokeKind::Inside);
        }
        TimelineEasing::Smooth | TimelineEasing::EaseIn | TimelineEasing::EaseOut | TimelineEasing::EaseInOut => {
            painter.circle(at, 5.0, fill, stroke);
        }
        TimelineEasing::Linear => {
            let d = 6.4;
            let points = vec![at - vec2(0.0, d), at + vec2(d, 0.0), at + vec2(0.0, d), at - vec2(d, 0.0)];
            painter.add(Shape::convex_polygon(points, fill, stroke));
        }
    }
}

impl TimelineGrid {
    /// The rows of keys (as the rows lay them), each with its place among the rows, what it
    /// holds and its top (window points).
    fn key_lines<'a>(&self, place: &'a Place) -> Vec<(usize, &'a GridRow, &'a KeyRow, f32)> {
        place
            .rows
            .list
            .iter()
            .enumerate()
            .filter_map(|(r, row)| row.keys.as_ref().map(|k| (r, row, k, place.row_top(r))))
            .collect()
    }

    /// The keys of row of keys `row` (each of its tracks' blocks'), in frame order, as they show
    /// (a drag under way leaving them where it would).
    fn line_keys(&self, panel: &TimelinePanel, row: &GridRow, keys: &KeyRow) -> Vec<TimelineLaneKey> {
        let mut all: Vec<TimelineLaneKey> = row
            .segments
            .iter()
            .filter(|s| keys.tracks.contains(&s.track().id))
            .flat_map(|s| row_keys(panel, &s.row, keys.kind))
            .collect();
        all.sort_by_key(|k| k.frame);
        all
    }

    /// The row of keys at `at` (window points), with its place among the rows.
    fn key_line_at<'a>(&self, place: &'a Place, at: Pos2) -> Option<(usize, &'a GridRow, &'a KeyRow)> {
        let r = place.row_at(at.y)?;
        let row = place.rows.get(r)?;
        Some((r, row, row.keys.as_ref()?))
    }

    /// Whether `at` is on a row of keys.
    pub(super) fn on_key_row(&self, place: &Place, at: Pos2) -> bool {
        self.key_line_at(place, at).is_some()
    }

    /// The grips beside the keys picked, where they span two frames or more.
    pub(super) fn grips(&self, place: &Place) -> Option<Grips> {
        let panel = self.panel.as_ref()?;
        let picks = self.keys.shown_picks(panel);
        let (first, last) = (picks.iter().map(|p| p.frame).min()?, picks.iter().map(|p| p.frame).max()?);
        let lines = self.key_lines(place);
        let top = lines.first().map(|l| l.3)?;
        let bottom = lines.last().map(|l| l.3 + l.1.height)?;
        (first < last).then_some(Grips { first, last, top, bottom })
    }

    /// Which grip (the first's: true) is at `at`, if one is.
    pub(super) fn grip_at(&self, place: &Place, at: Pos2) -> Option<(u32, u32)> {
        let g = self.grips(place)?;
        let leading = g.grip_at(place, at)?;
        // (stretched about the far end, from the near one)
        Some(if leading { (g.last, g.first) } else { (g.first, g.last) })
    }

    /// The key whose mark is at `at` (window points), if there is one.
    pub(super) fn mark_at(&self, place: &Place, at: Pos2) -> Option<TimelineKeyPick> {
        let panel = self.panel.as_ref()?;
        let (_, row, keys) = self.key_line_at(place, at)?;
        if at.x < place.bar.left() + LABELS_WIDTH - 12.0 {
            return None;
        }
        let f = place.frame_at(at.x)?;
        let shown = self.keys.shown_keys(self.line_keys(panel, row, keys));
        shown.into_iter().find(|k| k.frame == f).map(|k| TimelineKeyPick { lane: k.lane, frame: k.frame })
    }

    /// A mark dragged `by` frames along.
    pub(super) fn keys_dragged(&mut self, session: &CanvasSession, key: TimelineKeyPick, by: i32) {
        let Some(panel) = self.panel.as_ref() else { return };
        self.keys.drag(session, panel, key, by);
    }

    /// A grip dragged: the frame it was at (`from`) taken `dx` points along, about `anchor`.
    pub(super) fn keys_stretched(&mut self, session: &CanvasSession, anchor: u32, from: u32, dx: f32) {
        let Some(panel) = self.panel.as_ref() else { return };
        if from == anchor {
            return;
        }
        let now = from as f32 + dx / PITCH;
        let factor = f64::from((now - anchor as f32) / (from as f32 - anchor as f32)).max(0.05);
        self.keys.stretch(session, panel, anchor, factor);
    }

    /// A drag on the keys let go: done, one undo step.
    pub(super) fn keys_let_go(&mut self, session: &CanvasSession) {
        if self.keys.end_drag(session) {
            self.read_at = None;
        }
    }

    /// Key `key`'s menu opened over its mark (`at`), it taken with the keys picked; the frames'
    /// menu closed.
    pub(super) fn open_key_menu(&mut self, place: &Place, key: TimelineKeyPick, at: Pos2) {
        let Some(panel) = self.panel.as_ref() else { return };
        self.keys.take(panel, key);
        let x = place.x(key.frame as f32) + CELL_WIDTH / 2.0;
        self.keys.menu = Some(MarkMenu { key, at: pos2(x, at.y), beside: None });
        self.keys.editing = None;
        self.close_menu();
    }

    /// A tap on a row of keys at `at`: on a row's name, all its keys picked or none; on a mark,
    /// it picked (the playhead going to it) or let go; elsewhere on the row, every key let go.
    /// False if `at` isn't on a row of keys.
    pub(super) fn keys_tapped(&mut self, session: &CanvasSession, place: &Place, at: Pos2) -> bool {
        let Some(panel) = self.panel.as_ref() else { return false };
        let Some((_, row, keys)) = self.key_line_at(place, at) else { return false };
        if at.x < place.bar.left() + LABELS_WIDTH - 12.0 {
            let all = self.line_keys(panel, row, keys);
            self.keys.toggle_all(&all);
            return true;
        }
        match self.mark_at(place, at) {
            Some(key) => {
                if self.keys.tap(key) && key.frame != self.frame() {
                    self.go_to(session, key.frame);
                }
            }
            None => {
                self.keys.picks.clear();
                self.keys.cancel_drag();
            }
        }
        true
    }

    /// The lasso's stroke over `at` on a row of keys: the key there taken in with the others or
    /// out (once a stroke, kept in `touched`). False off a row of keys.
    pub(super) fn keys_stroked(&mut self, place: &Place, at: Pos2, touched: &mut Vec<TimelineKeyPick>) -> bool {
        if !self.on_key_row(place, at) {
            return false;
        }
        if let Some(key) = self.mark_at(place, at)
            && !touched.contains(&key)
        {
            touched.push(key);
            self.keys.cancel_drag();
            match self.keys.picks.iter().position(|p| *p == key) {
                Some(i) => {
                    self.keys.picks.remove(i);
                }
                None => self.keys.picks.push(key),
            }
        }
        true
    }

    /// The rows of keys over the frames `lo..=hi`: a strip for each, a mark on each keyed frame;
    /// the grips beside the keys picked.
    pub(super) fn draw_keys(&self, painter: &egui::Painter, place: &Place, lo: u32, hi: u32) {
        let Some(panel) = self.panel.as_ref() else { return };
        let picked = self.keys.shown_picks(panel);
        let here = self.frame();
        for (_, row, keys, top) in self.key_lines(place) {
            let height = row.height;
            let from = lo.max(row.segments.first().map_or(0, |s| s.row.start));
            let strip = Rect::from_min_max(
                pos2(place.x(from as f32), top + 3.0),
                pos2(place.x(hi as f32 + 1.0), top + height - 3.0),
            );
            painter.rect_filled(strip, 5, argb(SLOT).gamma_multiply(0.55));
            let y = top + height / 2.0;
            let shown = self.keys.shown_keys(self.line_keys(panel, row, keys));
            for k in shown.iter().filter(|k| k.frame >= lo && k.frame <= hi) {
                let x = place.x(k.frame as f32) + CELL_WIDTH / 2.0;
                let chosen = picked.contains(&TimelineKeyPick { lane: k.lane, frame: k.frame });
                let fill = if chosen {
                    argb(pc::ACCENT)
                } else if k.frame == here {
                    argb(INK)
                } else {
                    argb(INK_DIM)
                };
                mark(painter, pos2(x, y), k.easing, fill, chosen.then_some(Color32::WHITE));
            }
        }
        if let Some(g) = self.grips(place) {
            for leading in [true, false] {
                painter.rect_filled(g.rect(place, leading), 3, argb(pc::ACCENT));
            }
        }
    }

    /// The rows of keys' names, in the names' column (lit where all a row's keys are picked),
    /// stepped in as their tracks' names are.
    pub(super) fn draw_key_names(&self, painter: &egui::Painter, place: &Place) {
        let Some(panel) = self.panel.as_ref() else { return };
        for (_, row, keys, top) in self.key_lines(place) {
            let all = self.line_keys(panel, row, keys);
            let lit = !all.is_empty()
                && all.iter().all(|k| self.keys.picks.contains(&TimelineKeyPick { lane: k.lane, frame: k.frame }));
            let ink = if lit { argb(pc::ACCENT) } else { argb(INK) };
            let at = pos2(place.bar.left() + 50.0 + row.depth as f32 * INDENT, top + row.height / 2.0);
            text(painter, at, Align2::LEFT_CENTER, keys.kind.said(), 11.5, Weight::Medium, ink);
        }
    }

    // MARK: The key's menu

    /// The key's menu, while it is open: what it asks done. Returns what to tell.
    pub(super) fn key_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
    ) -> Option<&'static str> {
        let menu = self.keys.menu.as_ref()?;
        let (key, at, beside) = (menu.key, menu.at, menu.beside);
        let screen = ctx.content_rect();
        let panel = self.panel.as_ref()?;
        let several = self.keys.shown(panel).len() > 1;
        let clip = match key.lane {
            TimelineKeyLane::Clip { id, .. } => Some(id),
            _ => None,
        };
        let pasting = panel.clipboard.keys;
        let now = panel.keys.iter().find(|k| Some(k.id) == clip).and_then(|k| {
            k.keys.iter().find(|x| x.frame == key.frame && Some(x.channel) == channel_of(key.lane)).map(|x| x.easing)
        });
        let now = now.or_else(|| match key.lane {
            TimelineKeyLane::Volume { id } => panel
                .volumes
                .iter()
                .find(|v| v.id == id)
                .and_then(|v| v.keys.iter().find(|x| x.frame == key.frame))
                .map(|x| x.easing),
            _ => None,
        });
        // the menu's rows, and the easings' beside it
        let mut rows: Vec<(String, Asked, bool, bool)> = vec![("Easing".into(), Asked::Easing, true, false)];
        if clip.is_some() {
            rows.push(("Copy Keys".into(), Asked::Copy, true, false));
            rows.push(("Paste Keys".into(), Asked::Paste, pasting, false));
        }
        rows.push(("Reverse".into(), Asked::Reverse, several, false));
        rows.push((if several { "Delete Keys" } else { "Delete Key" }.into(), Asked::Delete, true, true));
        let height = MENU_ROW * rows.len() as f32 + 12.0;
        let left = (at.x - 30.0).clamp(screen.left() + 8.0, screen.right() - MENU_WIDTH - 8.0);
        let bottom = (at.y - 16.0).max(screen.top() + height + 8.0);
        let card = Rect::from_min_max(pos2(left, bottom - height), pos2(left + MENU_WIDTH, bottom));
        let mut asked = None;
        let mut close = false;
        // the rest of the window takes no presses while it is open: one closes it
        egui::Area::new(Id::unique("timeline key menu scrim")).fixed_pos(screen.min).order(Order::Foreground).show(
            ctx,
            |ui| {
                let shield = ui.allocate_rect(screen, Sense::click_and_drag());
                if shield.is_pointer_button_down_on() || shield.clicked() || crate::ui::escape_first(ui.ctx()) {
                    close = true;
                }
            },
        );
        // what stands beside it
        let mut easings: Vec<(String, Asked, bool)> = Vec::new();
        if beside == Some(Beside::Easing) {
            easings.extend(EASINGS.iter().map(|(e, name)| ((*name).to_owned(), Asked::Ease(*e), now == Some(*e))));
            if let Some(id) = clip {
                let spec = session.timeline_key_easing(id, key.frame);
                let curves = crate::saved_curves::easing_choices(
                    self.keys.curves(),
                    &spec.map_or(Easing::Smooth, |s| s.easing()),
                );
                easings.extend(curves.into_iter().map(|c| (c.name, Asked::Curve(EasingSpec::of(&c.easing)), c.chosen)));
                easings.push(("Edit Curve…".into(), Asked::EditCurve, false));
            }
        }
        let beside_rect = match beside {
            Some(Beside::Easing) => {
                let h = MENU_ROW * easings.len() as f32 + 12.0;
                let top = (card.bottom() - h).max(screen.top() + 8.0);
                Some(Rect::from_min_size(pos2(card.right() + 6.0, top), vec2(MENU_WIDTH, h)))
            }
            Some(Beside::Curve) => {
                let h = self.keys.editor.height().clamp(300.0, (card.bottom() - screen.top() - 16.0).max(300.0));
                let top = (card.bottom() - h).max(screen.top() + 8.0);
                Some(Rect::from_min_size(pos2(card.right() + 6.0, top), vec2(crate::easing_editor::WIDTH, h)))
            }
            None => None,
        };
        // kept on the window: moved left as far as it needs
        let over = beside_rect.map_or(0.0, |b| (b.right() - (screen.right() - 8.0)).max(0.0));
        let card = card.translate(vec2(-over, 0.0));
        let beside_rect = beside_rect.map(|b| b.translate(vec2(-over, 0.0)));
        let mut editor_done = None;
        egui::Area::new(Id::unique("timeline key menu")).fixed_pos(screen.min).order(Order::Tooltip).show(ctx, |ui| {
            ui.allocate_rect(card, Sense::click());
            if let Some(b) = beside_rect {
                ui.allocate_rect(b, Sense::click());
            }
            crate::screen::claim(ui, card);
            card_frame(ui, card);
            for (i, (label, what, enabled, danger)) in rows.iter().enumerate() {
                let row = Rect::from_min_size(
                    pos2(card.left(), card.top() + 6.0 + MENU_ROW * i as f32),
                    vec2(card.width(), MENU_ROW),
                );
                let lit = *what == Asked::Easing && beside.is_some();
                if menu_row(ui, row, label, *enabled, *danger, lit, *what == Asked::Easing, ("key menu", i)) {
                    asked = Some(what.clone());
                }
            }
            match (beside, beside_rect) {
                (Some(Beside::Easing), Some(b)) => {
                    crate::screen::claim(ui, b);
                    card_frame(ui, b);
                    for (i, (label, what, chosen)) in easings.iter().enumerate() {
                        let row = Rect::from_min_size(
                            pos2(b.left(), b.top() + 6.0 + MENU_ROW * i as f32),
                            vec2(b.width(), MENU_ROW),
                        );
                        let more = *what == Asked::EditCurve;
                        if menu_row(ui, row, label, true, false, *chosen, more, ("key easing", i)) {
                            asked = Some(what.clone());
                        }
                        if *chosen {
                            ui.painter().circle_filled(pos2(row.right() - 22.0, row.center().y), 4.0, argb(pc::ACCENT));
                        }
                    }
                }
                (Some(Beside::Curve), Some(b)) => {
                    crate::screen::claim(ui, b);
                    ui.painter().rect_filled(b, 24, argb(CARD));
                    if let Some(id) = clip {
                        let mut easing = self.keys.editing.clone().unwrap_or_else(|| {
                            session.timeline_key_easing(id, key.frame).map_or(Easing::Smooth, |s| s.easing())
                        });
                        let mut curves = std::mem::take(self.keys.curves());
                        editor_done = self.keys.editor.show_in(ui, look, b, &mut easing, &mut curves);
                        *self.keys.curves() = curves;
                        self.keys.editing = Some(easing);
                    }
                }
                _ => {}
            }
        });
        let mut said = None;
        // the easing editor: shown at once, one undo step when let go
        if let (Some(done), Some(id)) = (editor_done, clip) {
            let spec = self.keys.editing.as_ref().map(EasingSpec::of);
            match done {
                Done::Live | Done::Set => {
                    if let Some(spec) = spec {
                        session.set_timeline_key_easing(id, key.frame, spec, done == Done::Set);
                    }
                    if done == Done::Set {
                        self.read_at = None;
                    }
                }
                Done::Back => {
                    if let Some(m) = self.keys.menu.as_mut() {
                        m.beside = Some(Beside::Easing);
                    }
                    self.keys.editing = None;
                }
            }
            return None;
        }
        if let Some(asked) = asked {
            said = self.key_menu_asked(session, key, clip, asked);
            self.read_at = None;
        } else if close {
            self.keys.menu = None;
            self.keys.editing = None;
        }
        said
    }

    /// A row of the key's menu tapped.
    fn key_menu_asked(
        &mut self,
        session: &CanvasSession,
        key: TimelineKeyPick,
        clip: Option<u64>,
        asked: Asked,
    ) -> Option<&'static str> {
        let panel = self.panel.as_ref()?;
        let mut close = true;
        let mut said = None;
        match asked {
            Asked::Easing => {
                close = false;
                if let Some(m) = self.keys.menu.as_mut() {
                    m.beside = if m.beside.is_some() { None } else { Some(Beside::Easing) };
                }
            }
            Asked::Ease(easing) => {
                close = false;
                if !self.keys.apply(session, panel, TimelineKeyEdit::Ease { easing }) {
                    said = Some("The keys ease so already.");
                }
                // (new keys ease so too, autokey's among them)
                self.keys.easing = easing;
                session.set_timeline_autokey_easing(easing);
            }
            Asked::Curve(spec) => {
                close = false;
                if let Some(id) = clip {
                    session.set_timeline_key_easing(id, key.frame, spec, true);
                }
            }
            Asked::EditCurve => {
                close = false;
                if key.frame != self.frame() {
                    self.go_to(session, key.frame);
                }
                self.keys.editing = None;
                if let Some(m) = self.keys.menu.as_mut() {
                    m.beside = Some(Beside::Curve);
                }
            }
            Asked::Copy => {
                if let Some(id) = clip {
                    if key.frame != self.frame() {
                        self.go_to(session, key.frame);
                    }
                    session.copy_timeline_keys(id, true);
                }
            }
            Asked::Paste => {
                self.stop(session, false);
                if !session.paste_timeline_keys() {
                    said = Some("Move the playhead onto a clip to paste keys onto it.");
                }
            }
            Asked::Reverse => {
                if !self.keys.apply(session, panel, TimelineKeyEdit::Reverse) {
                    said = Some("Those keys can't be reversed.");
                }
            }
            Asked::Delete => {
                self.keys.apply(session, panel, TimelineKeyEdit::Remove);
            }
        }
        if close {
            self.keys.menu = None;
            self.keys.editing = None;
        }
        said
    }
}

/// The channel a lane is of (none: a sound's level, or the camera).
fn channel_of(lane: TimelineKeyLane) -> Option<TimelineChannel> {
    match lane {
        TimelineKeyLane::Clip { channel, .. } => Some(channel),
        _ => None,
    }
}

/// A card of the menus: filled, edged.
pub(super) fn card_frame(ui: &Ui, rect: Rect) {
    ui.painter().rect_filled(rect, MENU_CORNER, argb(CARD));
    ui.painter().rect_stroke(rect, MENU_CORNER, Stroke::new(1.0, argb(CARD_EDGE)), StrokeKind::Inside);
}

/// A row of a menu: its label (dim when not `enabled`, red when `danger`, the accent when `lit`),
/// a chevron when it opens `more`. True when clicked.
#[allow(clippy::too_many_arguments)]
pub(super) fn menu_row(
    ui: &Ui,
    row: Rect,
    label: &str,
    enabled: bool,
    danger: bool,
    lit: bool,
    more: bool,
    id: (&str, usize),
) -> bool {
    let ink = if !enabled {
        argb(MENU_DIM)
    } else if danger {
        argb(DANGER)
    } else if lit {
        argb(pc::ACCENT)
    } else {
        argb(MENU_INK)
    };
    let response = ui.interact(row, ui.scope_id().with(id), Sense::click());
    if response.hovered() && enabled {
        ui.painter().rect_filled(row.shrink2(vec2(6.0, 2.0)), 8, argb(CARD_LINE));
    }
    text(ui.painter(), pos2(row.left() + 18.0, row.center().y), Align2::LEFT_CENTER, label, 16.0, Weight::Regular, ink);
    if more {
        let x = row.right() - 18.0;
        let y = row.center().y;
        ui.painter()
            .line(vec![pos2(x - 4.0, y - 6.0), pos2(x + 2.0, y), pos2(x - 4.0, y + 6.0)], Stroke::new(1.8, ink));
    }
    enabled && response.clicked()
}

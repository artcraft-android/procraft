//! Moving blocks about the grid, as the iPad's (ios/ProCraft/TimelineGridMove.swift). A block
//! held opens its menu; dragged on from the hold, the menu goes and the block is picked up: it
//! goes along by whole frames and from row to row with the pointer, the frames it would cover
//! showing pushed aside (`timeline_place_preview`), and stays where it is let go. With the lasso
//! on, the picked blocks go together. Over the line between two rows (one block alone) a dashed
//! line shows across the grid: dropped there, the block gets a new track of its own at that
//! place. Near either end of the bar the frames slide under it.

use std::collections::HashMap;

use egui::{Pos2, Shape, Stroke, pos2};
use procraft_core::{CanvasSession, TimelineClipInfo};

use super::TimelineGrid;
use super::grid::{LABELS_WIDTH, PITCH, Place};
use crate::look::{argb, pc};

/// How near an end of the bar a block dragged makes the frames slide under it, and how many
/// frames they slide a second there.
const SLIDE_ZONE: f32 = 44.0;
const SLIDE_SPEED: f32 = 6.6;

/// The line between two rows a dragged block is over: the track it is beside, whether the new
/// track goes below that one (else above), and where the line is among the rows.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Between {
    pub beside: u64,
    pub below: bool,
    pub y: f32,
}

/// A block (or the blocks picked together) being dragged somewhere else.
pub(super) struct Move {
    /// The block under the pointer, and every block going with it (itself, alone).
    pub clip: u64,
    pub ids: Vec<u64>,
    /// Its track and first frame as the drag began, and the frame under the playhead then.
    pub track: u64,
    pub start: i64,
    pub scroll_from: f32,
    /// How many frames along it has gone, and the track it is over or the line between two.
    pub by: i64,
    pub over: Option<u64>,
    pub between: Option<Between>,
    /// The tracks as dropping it here would leave them; whether it can be dropped here; whether
    /// that has been worked out yet.
    pub shown: HashMap<u64, Vec<TimelineClipInfo>>,
    pub valid: bool,
    pub read: bool,
}

impl TimelineGrid {
    /// The pointer that held a block has moved on: the block (with the others picked, the lasso
    /// on) is picked up. False if there is nothing to pick up.
    pub(super) fn move_began(&mut self, held: Option<u64>) -> bool {
        let Some((t, c)) = held.and_then(|id| self.clip(id)) else { return false };
        if self.held(&t) || self.playing.is_some() {
            return false;
        }
        let ids = match self.selecting.as_ref() {
            Some(chosen) if !chosen.contains(&c.id) => return false,
            Some(chosen) => chosen.iter().copied().collect(),
            None => vec![c.id],
        };
        self.menu = None;
        self.picked = None;
        self.moving = Some(Move {
            clip: c.id,
            ids,
            track: t.id,
            start: i64::from(c.start),
            scroll_from: self.motion_at(),
            by: 0,
            over: None,
            between: None,
            shown: HashMap::new(),
            valid: false,
            read: false,
        });
        true
    }

    /// The line between rows at height `y` of them (rows' points), if it is near one: above the
    /// first row and below the last count.
    fn line_at(&self, y: f32, frame: u32) -> Option<Between> {
        let (first, last) = (self.rows.list.first()?, self.rows.list.last()?);
        let near = (first.height / 4.0).min(7.0);
        if y < first.y + near {
            return Some(Between { beside: first.track(frame)?.id, below: false, y: first.y });
        }
        if y > last.y + last.height - near {
            return Some(Between { beside: last.track(frame)?.id, below: true, y: last.y + last.height });
        }
        // (the line over a row: a track made above that row's, among its own: inside a group,
        // where the row is a group's)
        self.rows
            .list
            .iter()
            .skip(1)
            .find(|r| (y - r.y).abs() <= near)
            .and_then(|r| Some(Between { beside: r.track(frame)?.id, below: false, y: r.y }))
    }

    /// The block dragged on to `at`, the pointer having come down at `origin`: where it would land
    /// now, and the tracks as that would leave them. Near an end of the bar the frames slide under
    /// it (`dt` seconds since the last frame).
    pub(super) fn moved(&mut self, session: &CanvasSession, place: &Place, at: Pos2, origin: Pos2, dt: f32) -> bool {
        let Some(mut m) = self.moving.take() else { return false };
        let slide = if at.x < place.bar.left() + LABELS_WIDTH + SLIDE_ZONE {
            -1.0
        } else if at.x > place.bar.right() - SLIDE_ZONE {
            1.0
        } else {
            0.0
        };
        if slide != 0.0 {
            self.slide_frames(slide * SLIDE_SPEED * dt, place.last as f32);
        }
        // the frames slid under it count as far as it has gone
        let slid = self.motion_at() - m.scroll_from;
        let by = (((at.x - origin.x) / PITCH + slid).round() as i64).max(-m.start);
        let y = at.y - place.top;
        let under = u32::try_from((((at.x - place.origin) / PITCH).floor() as i64).max(0)).unwrap_or(0);
        let (mut between, mut over) = (None, None);
        if m.ids.len() == 1
            && let Some(line) = self.line_at(y, under)
        {
            between = Some(line);
        } else if let Some(r) = self.rows.index_at(y.clamp(0.0, (self.rows.height - 1.0).max(0.0))) {
            over = self.rows.get(r).and_then(|row| row.track(under)).map(|t| t.id);
        }
        if m.read && by == m.by && between == m.between && over == m.over {
            self.moving = Some(m);
            return slide != 0.0;
        }
        m.by = by;
        m.between = between;
        m.over = over;
        m.shown.clear();
        m.valid = false;
        m.read = true;
        let start = u32::try_from((m.start + by).max(0)).unwrap_or(0);
        if between.is_some() {
            m.valid = true;
        } else if let Some(over) = over {
            if m.ids.len() == 1 {
                if let Some(landed) = session.timeline_place_preview(m.clip, over, start, by >= 0) {
                    m.shown.insert(over, landed.clips);
                    // (off its own track, which shows without it)
                    if over != m.track
                        && let Some(from) = self.track(m.track)
                    {
                        m.shown.insert(m.track, from.clips.into_iter().filter(|c| c.id != m.clip).collect());
                    }
                    m.valid = true;
                }
            } else {
                let by = i32::try_from(by).unwrap_or(0);
                for t in session.timeline_place_clips_preview(m.ids.clone(), m.clip, over, by, by >= 0) {
                    m.shown.insert(t.track, t.clips);
                    m.valid = true;
                }
            }
        }
        self.moving = Some(m);
        slide != 0.0
    }

    /// The block let go: it stays where it showed, as one undo step; the frames slid under it
    /// stay slid.
    pub(super) fn move_ended(&mut self, session: &CanvasSession) {
        let Some(m) = self.moving.take() else { return };
        let slid = self.motion_at().round().max(0.0);
        self.rest_frames(slid);
        if m.valid {
            if let Some(line) = m.between {
                self.drop_on_new_track(session, m.clip, line.beside, line.below, m.start + m.by);
            } else if let Some(over) = m.over.filter(|o| *o != m.track || m.by != 0) {
                if m.ids.len() == 1 {
                    self.drop_clip(session, m.clip, over, m.start + m.by, m.by >= 0);
                } else {
                    self.drop_clips(session, m.ids.clone(), m.clip, over, m.by, m.by >= 0);
                }
            }
        }
        let slid = slid as u32;
        if slid != self.frame() {
            self.go_to(session, slid);
        }
        self.changed();
    }

    /// Over the line between two rows: a dashed line across the grid there.
    pub(super) fn draw_move_line(&self, painter: &egui::Painter, place: &Place) {
        let Some(line) = self.moving.as_ref().and_then(|m| m.between) else { return };
        let y = place.top + line.y;
        let from = pos2(place.bar.left() + LABELS_WIDTH - 40.0, y);
        let to = pos2(place.bar.right(), y);
        painter.extend(Shape::dashed_line(&[from, to], Stroke::new(2.0, argb(pc::ACCENT)), 7.0, 6.0));
    }
}

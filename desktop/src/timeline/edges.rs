//! The frames' edges as they show and as they are dragged, as the iPad's `GridEdges`
//! (ios/ProCraft/TimelineGridCells.swift): which clip has handles (the frame picked, or the last
//! of the frames picked together), where they are, and what dragging one does. An end made later
//! takes up the empty frames after it first and pushes only the frames it reaches; a start made
//! earlier pushes the frames before it into the empty frames before them, and with no more room
//! the frame lasts that much longer at its end instead. A repeating group's end sets how long it
//! plays, a tag saying how many times.

use std::collections::HashMap;

use egui::{Align2, Color32, Pos2, Rect, Ui, pos2, vec2};
use procraft_core::{CanvasSession, TimelineClipInfo, TimelineTrackInfo};

use super::TimelineGrid;
use super::grid::{CELL_HEIGHT, CELL_WIDTH, GAP, PITCH, Place};
use crate::look::{Look, Weight, argb, pc, text, text_width};

/// How wide a handle reaches for the pointer, and how much of that lies over the frame's own edge.
pub(super) const HANDLE_WIDTH: f32 = 28.0;
pub(super) const HANDLE_OVERLAP: f32 = 6.0;

/// An edge being dragged: of one frame, or of the frames picked together.
#[derive(Default)]
pub(super) struct Edit {
    pub trim: Option<Trim>,
    pub group_trim: Option<GroupTrim>,
}

/// An edge of the frame under the playhead being dragged: whose and which, the frame that edge
/// started on, the frame the clip ended on (one past its last) as the drag began, the frame asked
/// for now (a start's may be before the first), how far the pointer has gone, its track's clips
/// as the drag would leave them (empty: as they are), and the frame a limit holds the edge at.
pub(super) struct Trim {
    pub clip: u64,
    pub track: u64,
    pub leading: bool,
    pub from: i64,
    pub end: i64,
    pub frame: i64,
    pub dx: f32,
    pub clips: Vec<TimelineClipInfo>,
    pub limit: Option<f32>,
    /// A repeating group's end: how many frames it plays again and again (0: no such thing), and
    /// the most frames it has room for.
    pub cycle: u32,
    pub most: u32,
}

/// An edge of the frames picked together being dragged: whose handle, which edge, the frame it
/// started on, how many columns the handle goes for each frame every picked frame grows, how many
/// frames they have grown, the tracks as that would leave them, and a limit's frame.
pub(super) struct GroupTrim {
    pub clip: u64,
    pub leading: bool,
    pub from: i64,
    pub steps: i64,
    pub by: i64,
    pub tracks: HashMap<u64, Vec<TimelineClipInfo>>,
    pub limit: Option<f32>,
}

/// The clip with handles on its edges, its track, its row, and whether it stands for the frames
/// picked together.
pub(super) struct Handled {
    pub clip: TimelineClipInfo,
    pub track: TimelineTrackInfo,
    pub row: usize,
    pub group: bool,
}

/// Where the line of an edge held on frame `got` stands, in frames: a start's just before it; an
/// end's (a frame can be no shorter than one) through the frame it ends on.
fn limit_line(got: i64, leading: bool) -> f32 {
    if leading { got as f32 - GAP / 2.0 / PITCH } else { got as f32 - (PITCH - CELL_WIDTH / 2.0) / PITCH }
}

/// How a repeating group's length reads: whole cycles as "×3", a last one cut short as "×2 +1".
pub(super) fn cycle_tag(frames: u32, length: u32) -> String {
    let length = length.max(1);
    let (whole, over) = (frames / length, frames % length);
    if over == 0 { format!("×{whole}") } else { format!("×{whole} +{over}") }
}

impl TimelineGrid {
    /// Track `t`'s clips as they show: as a block dragged elsewhere or an edge being dragged would
    /// leave them, else as they are.
    pub(super) fn shown_clips<'a>(&'a self, t: &'a TimelineTrackInfo) -> &'a [TimelineClipInfo] {
        if let Some(clips) = self.moving.as_ref().and_then(|m| m.shown.get(&t.id)) {
            return clips;
        }
        if let Some(tr) = self.edit.trim.as_ref().filter(|tr| tr.track == t.id && !tr.clips.is_empty()) {
            return &tr.clips;
        }
        self.edit.group_trim.as_ref().and_then(|g| g.tracks.get(&t.id)).map_or(&t.clips, |c| c)
    }

    /// Where clip `c` shows: its left edge and width in the frames' space (frame 0's cell at 0).
    /// An edge of it being dragged follows the pointer between frames, as far as the drag can take
    /// it.
    pub(super) fn span(&self, c: &TimelineClipInfo) -> (f32, f32) {
        let mut x0 = c.start as f32 * PITCH;
        let mut x1 = (c.start + c.duration) as f32 * PITCH;
        if let Some(tr) = self.edit.trim.as_ref().filter(|tr| tr.clip == c.id) {
            let raw = tr.from as f32 * PITCH + tr.dx;
            // within half a frame of the frame it is on (held at a limit, it stays on the frame it
            // got to), and never shorter than a frame
            let near = raw.max((tr.frame as f32 - 0.5) * PITCH).min((tr.frame as f32 + 0.5) * PITCH);
            if tr.leading {
                if i64::from(c.start) == tr.frame {
                    x0 = near.min(x1 - PITCH).max(0.0);
                } else if raw < x0 && i64::from(c.start + c.duration) > tr.end {
                    // the frames before it have no more room: it grows at its end by as much as
                    // its start falls short of the pointer
                    let wanted = tr.end as f32 * PITCH + (x0 - raw);
                    x1 = wanted.max(x1 - PITCH / 2.0).min(x1 + PITCH / 2.0);
                }
            } else if i64::from(c.start + c.duration) == tr.frame {
                x1 = near.max(x0 + PITCH);
            }
        }
        (x0, (x1 - x0 - GAP).max(8.0))
    }

    /// The clip with handles on its edges: with the lasso on, the last of the picked frames in time
    /// (the topmost of those ending together); else the picked one.
    pub(super) fn handled(&self) -> Option<Handled> {
        // (none while a block is dragged elsewhere)
        if self.moving.is_some() {
            return None;
        }
        if let Some(chosen) = self.selecting.as_ref() {
            if chosen.is_empty() || !self.stroke.points.is_empty() {
                return None;
            }
            let mut best: Option<Handled> = None;
            for (r, s) in self.rows.tracks().filter(|(_, s)| !self.held(s.track())) {
                for c in self.shown_clips(s.track()).iter().filter(|c| chosen.contains(&c.id)) {
                    if best.as_ref().is_none_or(|b| c.start + c.duration > b.clip.start + b.clip.duration) {
                        best = Some(Handled { clip: c.clone(), track: s.track().clone(), row: r, group: true });
                    }
                }
            }
            return best;
        }
        let id = self.picked?;
        self.rows.tracks().filter(|(_, s)| !self.held(s.track())).find_map(|(r, s)| {
            let c = self.shown_clips(s.track()).iter().find(|c| c.id == id)?;
            Some(Handled { clip: c.clone(), track: s.track().clone(), row: r, group: false })
        })
    }

    /// Which handle is at `p` (window points), if one is: the start's (true) or the end's. A
    /// handle takes a pointer's width beyond the edge it is on.
    pub(super) fn handle_at(&self, place: &Place, p: Pos2) -> Option<bool> {
        if self.playing.is_some() {
            return None;
        }
        let h = self.handled()?;
        let row = place.rows.get(h.row)?;
        let y = place.row_top(h.row);
        if p.y < y || p.y >= y + row.height || !place.rows_rect.contains(p) {
            return None;
        }
        let (x, w) = self.span(&h.clip);
        let px = p.x - place.origin;
        let reach = HANDLE_WIDTH - HANDLE_OVERLAP;
        // on a frame too narrow for both, each keeps its own half
        let inside = (HANDLE_OVERLAP + 4.0).min(w / 2.0);
        if px >= x + w - inside && px <= x + w + reach {
            return Some(false);
        }
        if h.clip.cycle == 0 && px >= x - reach && px <= x + inside {
            return Some(true);
        }
        None
    }

    /// The handle on the start (`leading`) or the end dragged `dx` from where it was.
    pub(super) fn edge_dragged(&mut self, session: &CanvasSession, leading: bool, dx: f32) {
        // (the drag under way keeps the edge it began on)
        if let Some(tr) = self.edit.trim.as_ref() {
            let (clip, track, from, end, leading) = (tr.clip, tr.track, tr.from, tr.end, tr.leading);
            self.edge(session, clip, track, from, end, leading, dx);
        } else if let Some(g) = self.edit.group_trim.as_ref() {
            let (clip, steps, from, leading) = (g.clip, g.steps, g.from, g.leading);
            self.group_edge(session, clip, steps, from, leading, dx);
        } else if let Some(h) = self.handled() {
            let from = i64::from(if leading { h.clip.start } else { h.clip.start + h.clip.duration });
            if h.group {
                // an end goes as far as every picked frame before it on its track grows, and its own
                let chosen = self.selecting.clone().unwrap_or_default();
                let before =
                    h.track.clips.iter().filter(|c| chosen.contains(&c.id) && c.start <= h.clip.start).count() as i64;
                self.group_edge(session, h.clip.id, if leading { 1 } else { before.max(1) }, from, leading, dx);
            } else {
                let end = i64::from(h.clip.start + h.clip.duration);
                self.edge(session, h.clip.id, h.track.id, from, end, leading, dx);
            }
        }
    }

    /// The picked frame's edge dragged `dx` from where it was: the frame asked for, its track as
    /// that would leave it, and the limit holding it short of there if one does.
    #[allow(clippy::too_many_arguments)]
    fn edge(&mut self, session: &CanvasSession, clip: u64, track: u64, from: i64, end: i64, leading: bool, dx: f32) {
        let first = self.edit.trim.is_none();
        if first {
            self.stop(session, false);
            self.menu = None;
        }
        let mut tr = self.edit.trim.take().unwrap_or(Trim {
            clip,
            track,
            leading,
            from,
            end,
            frame: from,
            dx: 0.0,
            clips: Vec::new(),
            limit: None,
            cycle: 0,
            most: 0,
        });
        tr.dx = dx;
        // a repeating group's end: how long it plays, up to the next block on its track
        if first
            && !leading
            && let Some((_, c)) = self.clip(clip).filter(|(_, c)| c.cycle > 0)
        {
            tr.cycle = c.cycle;
            tr.most = session.timeline_group_room(clip).unwrap_or(c.duration);
        }
        if tr.cycle > 0 {
            self.cycle_dragged(&mut tr, dx);
            self.edit.trim = Some(tr);
            return;
        }
        let raw = tr.from as f32 + dx / PITCH;
        // (a start may be asked before the first frame: what it can't have there goes on its end)
        let asked = if leading { raw.round() as i64 } else { (raw.round() as i64).max(0) };
        if asked != tr.frame || first {
            tr.frame = asked;
            tr.clips = if asked == tr.from { Vec::new() } else { self.trim_preview(session, clip, leading, asked) };
        }
        // where the edge got to: short of what was asked, something holds it there
        let got = tr
            .clips
            .iter()
            .find(|c| c.id == tr.clip)
            .map_or(tr.from, |c| i64::from(if tr.leading { c.start } else { c.start + c.duration }));
        let held = (raw - got as f32).abs() > 0.5;
        tr.limit = held.then(|| limit_line(got, leading));
        self.edit.trim = Some(tr);
    }

    /// A repeating group's end dragged `dx`: it plays as many frames, no further than there is
    /// room for.
    fn cycle_dragged(&self, tr: &mut Trim, dx: f32) {
        let Some((t, c)) = self.clip(tr.clip) else { return };
        let start = i64::from(c.start);
        let asked = (tr.from as f32 + dx / PITCH).round() as i64;
        let most = i64::from(tr.most.max(1));
        let frames = (asked - start).clamp(1, most);
        tr.frame = start + frames;
        tr.clips = t
            .clips
            .iter()
            .map(|x| {
                let mut x = x.clone();
                if x.id == c.id {
                    x.duration = u32::try_from(frames).unwrap_or(1);
                }
                x
            })
            .collect();
        tr.limit = (asked - start > most).then(|| limit_line(tr.frame, false));
    }

    /// The handle of the frames picked together dragged `dx`: every one of them a frame longer (or
    /// shorter) for each `steps` columns it goes, the tracks as that would leave them.
    fn group_edge(&mut self, session: &CanvasSession, clip: u64, steps: i64, from: i64, leading: bool, dx: f32) {
        if self.edit.group_trim.is_none() {
            self.stop(session, false);
            self.menu = None;
        }
        let mut g = self.edit.group_trim.take().unwrap_or(GroupTrim {
            clip,
            leading,
            from,
            steps,
            by: 0,
            tracks: HashMap::new(),
            limit: None,
        });
        let by = (dx / (PITCH * g.steps as f32)).round() as i64;
        if by != g.by {
            g.by = by;
            g.tracks = if by == 0 {
                HashMap::new()
            } else {
                self.trims_preview(session, leading, i32::try_from(by).unwrap_or(0))
            };
            let got = g
                .tracks
                .values()
                .flatten()
                .find(|c| c.id == g.clip)
                .map_or(g.from, |c| i64::from(if leading { c.start } else { c.start + c.duration }));
            // short of where it was asked to go: held there
            let held = got != g.from + by * g.steps;
            g.limit = held.then(|| limit_line(got, leading));
        }
        self.edit.group_trim = Some(g);
    }

    /// The handle let go: the edge stays where the drag left it, as one undo step.
    pub(super) fn edge_ended(&mut self, session: &CanvasSession) {
        if let Some(tr) = self.edit.trim.take() {
            if tr.cycle > 0 {
                if tr.frame != tr.from
                    && let Some((_, c)) = self.clip(tr.clip)
                {
                    let frames = u32::try_from(tr.frame - i64::from(c.start)).unwrap_or(1);
                    self.set_group_length(session, tr.clip, frames);
                }
            } else if tr.frame != tr.from && !tr.clips.is_empty() {
                self.trim(session, tr.clip, tr.leading, tr.frame);
            }
        }
        if let Some(g) = self.edit.group_trim.take()
            && g.by != 0
            && !g.tracks.is_empty()
        {
            self.trim_selected(session, g.leading, i32::try_from(g.by).unwrap_or(0));
        }
    }

    /// The handles on the handled clip's edges (while one is dragged, only it), a repeating
    /// group's end's tag, and a dragged edge's limit line.
    pub(super) fn draw_handles(&self, ui: &Ui, look: &mut Look, painter: &egui::Painter, place: &Place) {
        if let Some(limit) =
            self.edit.trim.as_ref().and_then(|t| t.limit).or(self.edit.group_trim.as_ref().and_then(|g| g.limit))
        {
            let x = place.origin + limit * PITCH;
            let line = Rect::from_x_y_ranges(x - 0.75..=x + 0.75, place.rows_rect.y_range());
            painter.rect_filled(line, 0, argb(pc::ACCENT));
        }
        if self.playing.is_some() {
            return;
        }
        let Some(h) = self.handled() else { return };
        let (x, w) = self.span(&h.clip);
        let x = place.origin + x;
        let top = place.cell_top(h.row);
        let dragged = self.edit.trim.as_ref().map(|t| t.leading).or(self.edit.group_trim.as_ref().map(|g| g.leading));
        let tab = |painter: &egui::Painter, look: &mut Look, left: f32, leading: bool| {
            let rect = Rect::from_min_size(pos2(left, top + 1.5), vec2(16.0, CELL_HEIGHT - 3.0));
            painter.rect_filled(rect, 6, argb(pc::ACCENT));
            let glyph = if leading { &crate::icons::CHEVRON_LEFT } else { &crate::icons::CHEVRON_RIGHT };
            look.icon_on(ui, painter, glyph, rect.center(), 14.0, Color32::WHITE);
        };
        // (a cycling group's start stays put)
        if dragged != Some(false) && h.clip.cycle == 0 {
            tab(painter, look, x + HANDLE_OVERLAP - 16.0, true);
        }
        if dragged != Some(true) {
            tab(painter, look, x + w - HANDLE_OVERLAP, false);
        }
        // a repeating group's end dragged: how many times it plays now
        if let Some(tr) = self.edit.trim.as_ref().filter(|t| t.cycle > 0)
            && let Some(now) = tr.clips.iter().find(|c| c.id == tr.clip)
        {
            let tag = cycle_tag(now.duration, tr.cycle);
            let wide = text_width(painter, &tag, 12.0, Weight::Bold) + 14.0;
            let capsule = Rect::from_min_size(pos2(x + w + 14.0, top + CELL_HEIGHT / 2.0 - 10.0), vec2(wide, 20.0));
            painter.rect_filled(capsule, 10, argb(pc::ACCENT));
            text(painter, capsule.center(), Align2::CENTER_CENTER, &tag, 12.0, Weight::Bold, Color32::WHITE);
        }
    }
}

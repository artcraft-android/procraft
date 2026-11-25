//! Where the grid's rows stand, as the iPad's `GridRows` (ios/ProCraft/TimelineGridRows.swift):
//! a track of the level shown has a row to itself; under a track holding expanded groups, the
//! groups' tracks share rows (`Segment`: a track and the frames its group lasts), their first
//! tracks side by side on the first row under it, their second on the next, and so on, each over
//! the frames its group lasts. The grid doesn't zoom on desktop, so every row is as tall as a
//! row starts on the iPad.
//!
//! Under a row whose tracks have their rows of keys out stand those rows of keys (as the iPad's
//! `GridRows` lays them): one for each kind of key any of those tracks' blocks has, in the kinds'
//! order, the tracks sharing a row of frames sharing them. A row of keys is a [`GridRow`] too (its
//! `keys` says what it holds), with its row of frames' tracks, at its own height; it has no frames.

use std::collections::HashSet;

use procraft_core::{TimelinePanel, TimelineRow, TimelineTrackInfo};

use super::grid::ROW_HEIGHT;
use super::keys::{KEYS_HEIGHT, KINDS, Kind, kinds};

/// A track's frames on a row, and the frames of the timeline it has: a track of the level shown,
/// all of them; a track inside a group, the frames the group lasts.
#[derive(Clone, Debug)]
pub(super) struct Segment {
    /// The core's row of the track (its clips, where it is among the groups, what holds it).
    pub row: TimelineRow,
    /// Every group it is inside, outermost first.
    pub groups: Vec<u64>,
}

impl Segment {
    pub fn track(&self) -> &TimelineTrackInfo {
        &self.row.track
    }

    /// Whether frame `f` is one of its own (a track inside a group has the frames the group lasts).
    pub fn has(&self, f: u32) -> bool {
        f >= self.row.start && f < self.row.end
    }
}

/// What a row of keys holds: one kind of key, of each of the tracks on its row of frames whose
/// rows of keys are out and whose blocks have that kind (top first).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct KeyRow {
    pub kind: Kind,
    pub tracks: Vec<u64>,
}

/// One row of the grid, and where it stands among the rows.
#[derive(Clone, Debug)]
pub(super) struct GridRow {
    /// The tracks on it, in time order (one, but under a track holding several expanded groups);
    /// a row of keys, those of the row of frames it stands under.
    pub segments: Vec<Segment>,
    /// How many groups its tracks are inside.
    pub depth: u32,
    /// A row of keys under its tracks' row, not of frames: what it holds. None: a row of frames.
    pub keys: Option<KeyRow>,
    /// Its top edge among all the rows, and its height.
    pub y: f32,
    pub height: f32,
}

impl GridRow {
    /// The track whose frames `frame` is among; where it is among none's, the nearest's.
    pub fn segment(&self, frame: u32) -> Option<&Segment> {
        self.segments.iter().find(|s| s.has(frame)).or_else(|| {
            let far = |s: &Segment| frame.abs_diff(s.row.start).min(frame.abs_diff(s.row.end));
            self.segments.iter().min_by_key(|s| far(s))
        })
    }

    pub fn track(&self, frame: u32) -> Option<&TimelineTrackInfo> {
        self.segment(frame).map(Segment::track)
    }

    /// Whether `frame` is one of a track's on it (a row of keys has no frames).
    pub fn has(&self, frame: u32) -> bool {
        self.keys.is_none() && self.segments.iter().any(|s| s.has(frame))
    }

    /// Whether it is a row of frames (not of keys).
    pub fn frames(&self) -> bool {
        self.keys.is_none()
    }
}

/// The grid's rows, top first, each where it stands.
#[derive(Clone, Debug, Default)]
pub(super) struct GridRows {
    pub list: Vec<GridRow>,
    /// How tall they all are together.
    pub height: f32,
    /// The tracks whose rows of keys were out as they were laid.
    pub keys_out: HashSet<u64>,
}

impl GridRows {
    /// Rows for the panel's rows (bottom first, as the core reads them: each unfolded group's
    /// own tracks just before the track holding it), top first, one under the other; under each,
    /// the rows of keys of its tracks in `out` (those whose rows of keys are out).
    pub fn new(panel: &TimelinePanel, out: &HashSet<u64>) -> GridRows {
        let top_first: Vec<TimelineRow> = panel.rows.iter().rev().cloned().collect();
        let mut list = Vec::new();
        let mut y = 0.0;
        for mut row in level(&top_first, 0, &[]) {
            row.y = y;
            row.height = ROW_HEIGHT;
            y += row.height;
            // its tracks' rows of keys: one for each kind any of them has (the tracks of several
            // groups sharing the row share those too)
            let keyed: Vec<(u64, Vec<Kind>)> = row
                .segments
                .iter()
                .filter(|s| out.contains(&s.track().id))
                .map(|s| (s.track().id, kinds(panel, &s.row)))
                .collect();
            let under: Vec<GridRow> = KINDS
                .into_iter()
                .filter_map(|kind| {
                    let tracks: Vec<u64> = keyed.iter().filter(|(_, k)| k.contains(&kind)).map(|(id, _)| *id).collect();
                    (!tracks.is_empty()).then(|| GridRow {
                        segments: row.segments.clone(),
                        depth: row.depth,
                        keys: Some(KeyRow { kind, tracks }),
                        y: 0.0,
                        height: KEYS_HEIGHT,
                    })
                })
                .collect();
            list.push(row);
            for mut keys in under {
                keys.y = y;
                y += keys.height;
                list.push(keys);
            }
        }
        GridRows { list, height: y, keys_out: out.clone() }
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.list.len()
    }

    pub fn get(&self, r: usize) -> Option<&GridRow> {
        self.list.get(r)
    }

    /// The row at height `y` of all the rows; none off them.
    pub fn index_at(&self, y: f32) -> Option<usize> {
        (y >= 0.0).then(|| self.list.iter().position(|r| y >= r.y && y < r.y + r.height)).flatten()
    }

    /// The row (of frames) track `id` is on.
    pub fn index_of(&self, id: Option<u64>) -> Option<usize> {
        let id = id?;
        self.list.iter().position(|r| r.frames() && r.segments.iter().any(|s| s.track().id == id))
    }

    /// The segment of track `id`, with its row (of frames).
    pub fn segment_of(&self, id: u64) -> Option<(usize, &Segment)> {
        self.tracks().find(|(_, s)| s.track().id == id)
    }

    /// Every track on the rows of frames, with its row, top first.
    pub fn tracks(&self) -> impl Iterator<Item = (usize, &Segment)> {
        self.list
            .iter()
            .enumerate()
            .filter(|(_, row)| row.frames())
            .flat_map(|(r, row)| row.segments.iter().map(move |s| (r, s)))
    }

    /// The row of frames that row `r` stands under (itself, for a row of frames).
    #[cfg(test)]
    pub fn frames_row(&self, r: usize) -> usize {
        self.list.get(..=r).and_then(|above| above.iter().rposition(GridRow::frames)).unwrap_or(r)
    }

    /// The bottom of the last row that has a track inside group `id`: how far down the group's
    /// own rows go. None where it has none showing.
    pub fn bottom_of_group(&self, id: u64) -> Option<f32> {
        self.list.iter().rev().find(|r| r.segments.iter().any(|s| s.groups.contains(&id))).map(|r| r.y + r.height)
    }

    /// Whether group clip `id` shows its tracks unfolded under its own.
    pub fn unfolded(&self, id: u64) -> bool {
        self.tracks().any(|(_, s)| s.row.group == id)
    }
}

fn segment(row: &TimelineRow, groups: &[u64]) -> Segment {
    Segment { row: row.clone(), groups: groups.to_vec() }
}

/// The row at the start of `rows`, the rows deeper than it that follow it (its groups'), and the
/// rest.
fn cut(rows: &[TimelineRow]) -> Option<(&TimelineRow, &[TimelineRow], &[TimelineRow])> {
    let (first, after) = rows.split_first()?;
    let end = after.iter().position(|r| r.depth <= first.depth).unwrap_or(after.len());
    let (inside, rest) = after.split_at(end);
    Some((first, inside, rest))
}

/// The rows of one level's tracks, each with its own row, and the rows of its groups under it.
fn level(rows: &[TimelineRow], depth: u32, groups: &[u64]) -> Vec<GridRow> {
    let mut out = Vec::new();
    let mut left = rows;
    while let Some((row, inside, rest)) = cut(left) {
        out.push(GridRow { segments: vec![segment(row, groups)], depth, keys: None, y: 0.0, height: 0.0 });
        out.extend(shared(inside, depth + 1, groups));
        left = rest;
    }
    out
}

/// The rows of the tracks of the groups on one track: the groups' first tracks on one row, their
/// second on the next, each followed by the rows of the groups on those tracks.
fn shared(rows: &[TimelineRow], depth: u32, groups: &[u64]) -> Vec<GridRow> {
    type Tracks<'a> = Vec<(&'a TimelineRow, &'a [TimelineRow])>;
    let mut by_group: Vec<(u64, Tracks)> = Vec::new();
    let mut left = rows;
    while let Some((row, inside, rest)) = cut(left) {
        match by_group.iter_mut().find(|(g, _)| *g == row.group) {
            Some((_, tracks)) => tracks.push((row, inside)),
            None => by_group.push((row.group, vec![(row, inside)])),
        }
        left = rest;
    }
    let most = by_group.iter().map(|(_, t)| t.len()).max().unwrap_or(0);
    let mut out = Vec::new();
    for k in 0..most {
        let there: Vec<_> = by_group.iter().filter_map(|(g, t)| t.get(k).map(|t| (*g, *t))).collect();
        let mut segments: Vec<Segment> =
            there.iter().map(|(g, (row, _))| segment(row, &[groups, &[*g]].concat())).collect();
        segments.sort_by_key(|s| s.row.start);
        out.push(GridRow { segments, depth, keys: None, y: 0.0, height: 0.0 });
        for (g, (_, inside)) in there {
            out.extend(shared(inside, depth + 1, &[groups, &[g]].concat()));
        }
    }
    out
}

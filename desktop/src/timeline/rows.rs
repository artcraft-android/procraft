//! Where the grid's rows stand, as the iPad's `GridRows` (ios/ProCraft/TimelineGridRows.swift):
//! a track of the level shown has a row to itself; under a track holding expanded groups, the
//! groups' tracks share rows (`Segment`: a track and the frames its group lasts), their first
//! tracks side by side on the first row under it, their second on the next, and so on, each over
//! the frames its group lasts. The grid doesn't zoom on desktop, so every row is as tall as a
//! row starts on the iPad.
//!
//! (Rows of keys under a track's row are the keys' to add: a [`GridRow`] is a row of frames.)

use procraft_core::{TimelineRow, TimelineTrackInfo};

use super::grid::ROW_HEIGHT;

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

/// One row of the grid, and where it stands among the rows.
#[derive(Clone, Debug)]
pub(super) struct GridRow {
    /// The tracks on it, in time order (one, but under a track holding several expanded groups).
    pub segments: Vec<Segment>,
    /// How many groups its tracks are inside.
    pub depth: u32,
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

    /// Whether `frame` is one of a track's on it.
    pub fn has(&self, frame: u32) -> bool {
        self.segments.iter().any(|s| s.has(frame))
    }
}

/// The grid's rows, top first, each where it stands.
#[derive(Clone, Debug, Default)]
pub(super) struct GridRows {
    pub list: Vec<GridRow>,
    /// How tall they all are together.
    pub height: f32,
}

impl GridRows {
    /// Rows for the panel's `rows` (bottom first, as the core reads them: each unfolded group's
    /// own tracks just before the track holding it), top first, one under the other.
    pub fn new(rows: &[TimelineRow]) -> GridRows {
        let top_first: Vec<TimelineRow> = rows.iter().rev().cloned().collect();
        let mut list = level(&top_first, 0, &[]);
        let mut y = 0.0;
        for row in &mut list {
            row.y = y;
            row.height = ROW_HEIGHT;
            y += row.height;
        }
        GridRows { list, height: y }
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

    /// The row track `id` is on.
    pub fn index_of(&self, id: Option<u64>) -> Option<usize> {
        let id = id?;
        self.list.iter().position(|r| r.segments.iter().any(|s| s.track().id == id))
    }

    /// The segment of track `id`, with its row.
    pub fn segment_of(&self, id: u64) -> Option<(usize, &Segment)> {
        self.list
            .iter()
            .enumerate()
            .find_map(|(r, row)| row.segments.iter().find(|s| s.track().id == id).map(|s| (r, s)))
    }

    /// Every track on the rows, with its row, top first.
    pub fn tracks(&self) -> impl Iterator<Item = (usize, &Segment)> {
        self.list.iter().enumerate().flat_map(|(r, row)| row.segments.iter().map(move |s| (r, s)))
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
        out.push(GridRow { segments: vec![segment(row, groups)], depth, y: 0.0, height: 0.0 });
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
        out.push(GridRow { segments, depth, y: 0.0, height: 0.0 });
        for (g, (_, inside)) in there {
            out.extend(shared(inside, depth + 1, &[groups, &[g]].concat()));
        }
    }
    out
}

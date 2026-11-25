//! Filling by geometry: the region around a point, bounded by lines, as a shape.
//!
//! The lines (strokes' paths) are triangulated with the open space between them (a constrained
//! Delaunay triangulation, every stretch of a line an edge of it). The fill spreads from the
//! triangle under the point across edges that are no line's, through the widest openings first,
//! and each triangle learns the narrowest opening the fill squeezed through to reach it. A gap in
//! a line is a narrow opening: what lies past it is reached only through that, far narrower than
//! its own openings, and is left out. So gaps close themselves, the same at any zoom, with no
//! distance to set. The method is "Delaunay Painting" (Parakkat, Memari and Cani, Computer
//! Graphics Forum 2022), with a sensitivity (`factor`) in place of the paper's competing colours:
//! a triangle is filled when the fill reaches it through at least `factor` of its own widest
//! opening.

use std::collections::{HashMap, HashSet};

use crate::geom::Vec2;

/// A line fills stop at: its points in order (canvas pixels), and what it is of (a stroke's
/// name, say).
#[derive(Clone, Debug, PartialEq)]
pub struct Wall {
    pub line: Vec<Vec2>,
    pub source: u64,
}

/// A region traced: its outlines (each closed back to its first point; a point is inside by the
/// even-odd rule, so an outline within another is a hole), the walls it lies against, and its
/// area (canvas pixels squared).
#[derive(Clone, Debug, PartialEq)]
pub struct Traced {
    pub loops: Vec<Vec<Vec2>>,
    pub sources: Vec<u64>,
    pub area: f32,
}

type Point = [f64; 2];

/// The value at `i`, or the type's nothing past the end (never reached: every index here is one
/// this file made).
fn at<T: Copy + Default>(values: &[T], i: usize) -> T {
    values.get(i).copied().unwrap_or_default()
}

fn put<T>(values: &mut [T], i: usize, value: T) {
    if let Some(slot) = values.get_mut(i) {
        *slot = value;
    }
}

/// Twice the area of the triangle a, b, c: positive when they turn one way, negative the other.
fn orient(a: Point, b: Point, c: Point) -> f64 {
    let ([ax, ay], [bx, by], [cx, cy]) = (a, b, c);
    (bx - ax) * (cy - ay) - (by - ay) * (cx - ax)
}

/// Whether `p` is inside the circle through a, b and c (in the order triangles are kept).
fn incircle(a: Point, b: Point, c: Point, p: Point) -> bool {
    let (ax, ay) = (a[0] - p[0], a[1] - p[1]);
    let (bx, by) = (b[0] - p[0], b[1] - p[1]);
    let (cx, cy) = (c[0] - p[0], c[1] - p[1]);
    let det = (ax * ax + ay * ay) * (bx * cy - cx * by) - (bx * bx + by * by) * (ax * cy - cx * ay)
        + (cx * cx + cy * cy) * (ax * by - bx * ay);
    det > 0.0
}

fn segments_cross(p0: Point, p1: Point, q0: Point, q1: Point) -> bool {
    orient(p0, p1, q0) * orient(p0, p1, q1) < 0.0 && orient(q0, q1, p0) * orient(q0, q1, p1) < 0.0
}

/// Where two segments cross within both (not where they only touch, share an end or overlap):
/// the point, and how far along each it is (0..1).
fn proper_intersection(p0: Point, p1: Point, q0: Point, q1: Point) -> Option<(Point, f64, f64)> {
    let (d1, d2) = (orient(p0, p1, q0), orient(p0, p1, q1));
    let (d3, d4) = (orient(q0, q1, p0), orient(q0, q1, p1));
    if !(d1 * d2 < 0.0 && d3 * d4 < 0.0) {
        return None;
    }
    let denominator = d1 - d2;
    if denominator.abs() <= f64::EPSILON {
        return None;
    }
    let t_second = d1 / denominator;
    let point = [q0[0] + (q1[0] - q0[0]) * t_second, q0[1] + (q1[1] - q0[1]) * t_second];
    let along = [p1[0] - p0[0], p1[1] - p0[1]];
    let length2 = along[0] * along[0] + along[1] * along[1];
    if length2 <= 0.0 {
        return None;
    }
    let t_first = ((point[0] - p0[0]) * along[0] + (point[1] - p0[1]) * along[1]) / length2;
    Some((point, t_first, t_second))
}

/// A pair of vertices as one number, whichever way round.
fn edge_key(a: usize, b: usize) -> u64 {
    let (low, high) = (a.min(b) as u64, a.max(b) as u64);
    (high << 32) | (low & 0xFFFF_FFFF)
}

fn cell_key(x: i32, y: i32) -> u64 {
    (u64::from(x as u32) << 32) | u64::from(y as u32)
}

/// A wall's stretch between two vertices, and which wall (its place among those given).
type Constraint = (usize, usize, usize);

/// The spacing of the points put in open space: the size of the largest openings the
/// triangulation offers, so also what a gap is measured against.
fn guide_step(width: f64, height: f64) -> f64 {
    (width.max(height) / 64.0).max(16.0)
}

/// The vertices and walls to triangulate: the lines' points (those within a quarter pixel made
/// one), their stretches as walls split where they cross, and an even grid of points in the open
/// space and round the canvas's edge, none nearer a line than half its spacing.
///
/// The grid keeps the triangles of open space near one size: without it empty areas would make
/// triangles of any size, whose widest opening would dwarf every way in, and the mesh itself
/// would read as a gap. A grid point standing in a gap would split it into short edges beside
/// full-width ones, and the fill would walk through: so they keep clear of the lines.
fn build_input(walls: &[Wall], width: f64, height: f64) -> (Vec<Point>, Vec<Constraint>) {
    let sample_step = (width.max(height) / 512.0).max(4.0);
    let grid_step = guide_step(width, height);
    let mut points: Vec<Point> = Vec::new();
    let mut index_by_cell: HashMap<u64, usize> = HashMap::new();
    let mut vertex_index = |points: &mut Vec<Point>, p: Point| -> usize {
        let key = cell_key((p[0] * 4.0).round() as i32, (p[1] * 4.0).round() as i32);
        *index_by_cell.entry(key).or_insert_with(|| {
            points.push(p);
            points.len() - 1
        })
    };
    let mut constraints: Vec<Constraint> = Vec::new();
    let mut samples_by_cell: HashMap<u64, Vec<Point>> = HashMap::new();
    for (wall_index, wall) in walls.iter().enumerate() {
        let mut previous: Option<(usize, Point)> = None;
        let line: Vec<Vec2> = wall.line.iter().copied().filter(|p| p.is_finite()).collect();
        for (i, p) in line.iter().enumerate() {
            let to = [f64::from(p.x), f64::from(p.y)];
            // a long stretch is walked in steps, so the mesh is even along it
            let from = previous.map_or(to, |(_, from)| from);
            let reach = ((to[0] - from[0]).powi(2) + (to[1] - from[1]).powi(2)).sqrt();
            // (points closer than the weld are one; the last is always kept)
            let steps = if previous.is_none() { 1 } else { ((reach / sample_step).ceil() as usize).clamp(1, 4096) };
            for step in 1..=steps {
                let t = step as f64 / steps as f64;
                let position = [from[0] + (to[0] - from[0]) * t, from[1] + (to[1] - from[1]) * t];
                let index = vertex_index(&mut points, position);
                if let Some((before, _)) = previous
                    && before != index
                {
                    constraints.push((before, index, wall_index));
                }
                previous = Some((index, position));
                let cell = cell_key((position[0] / grid_step).floor() as i32, (position[1] / grid_step).floor() as i32);
                samples_by_cell.entry(cell).or_default().push(position);
            }
            let _ = i;
        }
    }
    let clearance = grid_step / 2.0;
    let is_clear = |p: Point| -> bool {
        let (cx, cy) = ((p[0] / grid_step).floor() as i32, (p[1] / grid_step).floor() as i32);
        for dy in -1..=1 {
            for dx in -1..=1 {
                let Some(samples) = samples_by_cell.get(&cell_key(cx + dx, cy + dy)) else { continue };
                if samples.iter().any(|s| (s[0] - p[0]).powi(2) + (s[1] - p[1]).powi(2) < clearance * clearance) {
                    return false;
                }
            }
        }
        true
    };
    let stepped = |to: f64, through: bool| -> Vec<f64> {
        let mut out = Vec::new();
        let mut v = if through { 0.0 } else { grid_step };
        while if through { v <= to } else { v < to } {
            out.push(v);
            v += grid_step;
        }
        out
    };
    let mut guides: Vec<Point> = Vec::new();
    for x in stepped(width, false) {
        for y in stepped(height, false) {
            guides.push([x, y]);
        }
    }
    for x in stepped(width, true) {
        guides.push([x.min(width), 0.0]);
        guides.push([x.min(width), height]);
    }
    for y in stepped(height, true) {
        guides.push([0.0, y.min(height)]);
        guides.push([width, y.min(height)]);
    }
    guides.extend([[0.0, 0.0], [width, 0.0], [0.0, height], [width, height]]);
    for guide in guides {
        if is_clear(guide) {
            vertex_index(&mut points, guide);
        }
    }
    let constraints = split_crossings(constraints, &mut points, (sample_step * 2.0).max(8.0), &mut vertex_index);
    // a hair of jitter, the same every time, takes points off the exact lines and circles that
    // pixel-snapped lines put them on, which the circle test can't tell apart
    for (index, point) in points.iter_mut().enumerate() {
        let mut hash = (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        hash = (hash ^ (hash >> 31)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        point[0] += ((hash & 0xFFFF) as f64 / 65535.0 - 0.5) * 0.02;
        point[1] += (((hash >> 16) & 0xFFFF) as f64 / 65535.0 - 0.5) * 0.02;
    }
    (points, constraints)
}

/// The walls split where one properly crosses another, both onto one vertex there: walls that
/// cross can't both be edges, split ones can.
fn split_crossings(
    constraints: Vec<Constraint>,
    points: &mut Vec<Point>,
    cell_size: f64,
    vertex_index: &mut impl FnMut(&mut Vec<Point>, Point) -> usize,
) -> Vec<Constraint> {
    let mut cells: HashMap<u64, Vec<usize>> = HashMap::new();
    for (index, &(a, b, _)) in constraints.iter().enumerate() {
        let (p0, p1) = (at(points, a), at(points, b));
        let cell = |v: f64| (v / cell_size).floor() as i32;
        let (x0, x1) = (cell(p0[0].min(p1[0])), cell(p0[0].max(p1[0])));
        let (y0, y1) = (cell(p0[1].min(p1[1])), cell(p0[1].max(p1[1])));
        for y in y0..=y1 {
            for x in x0..=x1 {
                cells.entry(cell_key(x, y)).or_default().push(index);
            }
        }
    }
    let mut splits: HashMap<usize, Vec<(f64, usize)>> = HashMap::new();
    let mut tested: HashSet<u64> = HashSet::new();
    // (in the cells' order as numbered, so the same input always splits the same way)
    let mut buckets: Vec<(&u64, &Vec<usize>)> = cells.iter().filter(|(_, bucket)| bucket.len() > 1).collect();
    buckets.sort_by_key(|(key, _)| **key);
    for (_, bucket) in buckets {
        for (i, &one) in bucket.iter().enumerate() {
            for &other in bucket.iter().skip(i + 1) {
                let (first, second) = (one.min(other), one.max(other));
                if first == second || !tested.insert(((first as u64) << 32) | second as u64) {
                    continue;
                }
                let ((a0, a1, _), (b0, b1, _)) = (at(&constraints, first), at(&constraints, second));
                // (walls that share an end meet there: no crossing)
                if a0 == b0 || a0 == b1 || a1 == b0 || a1 == b1 {
                    continue;
                }
                let Some((point, t_first, t_second)) =
                    proper_intersection(at(points, a0), at(points, a1), at(points, b0), at(points, b1))
                else {
                    continue;
                };
                let vertex = vertex_index(points, point);
                if vertex != a0 && vertex != a1 {
                    splits.entry(first).or_default().push((t_first, vertex));
                }
                if vertex != b0 && vertex != b1 {
                    splits.entry(second).or_default().push((t_second, vertex));
                }
            }
        }
    }
    if splits.is_empty() {
        return constraints;
    }
    let mut result = Vec::with_capacity(constraints.len() + splits.len() * 2);
    for (index, &(a, b, wall)) in constraints.iter().enumerate() {
        let Some(cuts) = splits.get_mut(&index) else {
            result.push((a, b, wall));
            continue;
        };
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut previous = a;
        for &(_, vertex) in cuts.iter() {
            if vertex != previous {
                result.push((previous, vertex, wall));
                previous = vertex;
            }
        }
        if previous != b {
            result.push((previous, b, wall));
        }
    }
    result
}

/// The vertices in an order that keeps each next to the last (along a Z-shaped curve through the
/// plane), so finding where each goes is a few steps from where the last went.
fn insertion_order(points: &[Point]) -> Vec<usize> {
    fn spread(value: u32) -> u64 {
        let mut x = u64::from(value);
        x = (x | (x << 16)) & 0x0000_FFFF_0000_FFFF;
        x = (x | (x << 8)) & 0x00FF_00FF_00FF_00FF;
        x = (x | (x << 4)) & 0x0F0F_0F0F_0F0F_0F0F;
        x = (x | (x << 2)) & 0x3333_3333_3333_3333;
        x = (x | (x << 1)) & 0x5555_5555_5555_5555;
        x
    }
    // (lines may run off the canvas: moved over so those still order sensibly)
    let offset = 1_i64 << 20;
    let coordinate = |v: f64| u32::try_from((v.round() as i64).saturating_add(offset).max(0)).unwrap_or(u32::MAX);
    let mut keyed: Vec<(u64, usize)> = points
        .iter()
        .enumerate()
        .map(|(index, p)| (spread(coordinate(p[0])) | (spread(coordinate(p[1])) << 1), index))
        .collect();
    keyed.sort_unstable();
    keyed.into_iter().map(|(_, index)| index).collect()
}

/// A triangulation built a point at a time (Bowyer–Watson), with walls forced in as edges by
/// flipping the edges that cross them. A triangle is three vertices in order, and across its
/// edge from the i-th to the next is its i-th neighbour (none at the outside). Three made-up
/// vertices past the real ones stand far outside, so every real vertex lands inside a triangle.
struct Delaunay {
    points: Vec<Point>,
    real: usize,
    vertices: Vec<usize>,
    neighbors: Vec<Option<usize>>,
    alive: Vec<bool>,
    free: Vec<usize>,
    /// A live triangle at each vertex, to walk round it from.
    vertex_triangle: Vec<Option<usize>>,
    inserted: Vec<bool>,
    /// The walls, by their vertices, with which walls given put each there.
    walls: HashMap<u64, Vec<usize>>,
    seen: Vec<u32>,
    bad_mark: Vec<u32>,
    mark: u32,
    last: usize,
}

impl Delaunay {
    fn new(input: Vec<Point>) -> Delaunay {
        let real = input.len();
        let mut points = input;
        let first = points.first().copied().unwrap_or([0.0, 0.0]);
        let (mut low, mut high) = (first, first);
        for p in &points {
            low = [low[0].min(p[0]), low[1].min(p[1])];
            high = [high[0].max(p[0]), high[1].max(p[1])];
        }
        let centre = [(low[0] + high[0]) * 0.5, (low[1] + high[1]) * 0.5];
        let span = (high[0] - low[0]).max(high[1] - low[1]).max(1.0);
        points.push([centre[0] - span * 24.0, centre[1] - span * 2.0]);
        points.push([centre[0] + span * 24.0, centre[1] - span * 2.0]);
        points.push([centre[0], centre[1] + span * 24.0]);
        let mut made = Delaunay {
            points,
            real,
            vertices: Vec::new(),
            neighbors: Vec::new(),
            alive: Vec::new(),
            free: Vec::new(),
            vertex_triangle: vec![None; real + 3],
            inserted: vec![false; real],
            walls: HashMap::new(),
            seen: Vec::new(),
            bad_mark: Vec::new(),
            mark: 0,
            last: 0,
        };
        made.allocate([real, real + 1, real + 2], [None; 3]);
        made
    }

    fn vertex(&self, t: usize, i: usize) -> usize {
        at(&self.vertices, t * 3 + i % 3)
    }

    fn neighbor(&self, t: usize, i: usize) -> Option<usize> {
        at(&self.neighbors, t * 3 + i % 3)
    }

    fn point(&self, v: usize) -> Point {
        at(&self.points, v)
    }

    fn is_alive(&self, t: usize) -> bool {
        at(&self.alive, t)
    }

    fn allocate(&mut self, corners: [usize; 3], across: [Option<usize>; 3]) -> usize {
        let slot = match self.free.pop() {
            Some(slot) => {
                for i in 0..3 {
                    put(&mut self.vertices, slot * 3 + i, at(&corners, i));
                    put(&mut self.neighbors, slot * 3 + i, at(&across, i));
                }
                put(&mut self.alive, slot, true);
                slot
            }
            None => {
                self.vertices.extend(corners);
                self.neighbors.extend(across);
                self.alive.push(true);
                self.seen.push(0);
                self.bad_mark.push(0);
                self.alive.len() - 1
            }
        };
        for corner in corners {
            put(&mut self.vertex_triangle, corner, Some(slot));
        }
        slot
    }

    fn local_index(&self, v: usize, t: usize) -> Option<usize> {
        (0..3).find(|&i| self.vertex(t, i) == v)
    }

    /// Which of `t`'s edges goes from `a` to `b`, if one does.
    fn edge_slot(&self, a: usize, b: usize, t: usize) -> Option<usize> {
        (0..3).find(|&i| self.vertex(t, i) == a && self.vertex(t, i + 1) == b)
    }

    /// The triangle `p` is in: walked to from the last one touched, every triangle looked at if
    /// the walk goes nowhere. None only when there is no triangle.
    fn locate(&mut self, p: Point) -> Option<usize> {
        let mut t = if self.is_alive(self.last) { self.last } else { self.alive.iter().position(|&a| a)? };
        let cap = self.alive.len() + 64;
        'walk: for _ in 0..cap {
            for i in 0..3 {
                let (a, b) = (self.point(self.vertex(t, i)), self.point(self.vertex(t, i + 1)));
                if orient(a, b, p) < 0.0
                    && let Some(next) = self.neighbor(t, i)
                {
                    t = next;
                    continue 'walk;
                }
            }
            self.last = t;
            return Some(t);
        }
        let found = (0..self.alive.len()).find(|&candidate| {
            self.is_alive(candidate)
                && (0..3).all(|i| {
                    let (a, b) = (self.point(self.vertex(candidate, i)), self.point(self.vertex(candidate, i + 1)));
                    orient(a, b, p) >= -1e-9
                })
        })?;
        self.last = found;
        Some(found)
    }

    fn insert(&mut self, index: usize) {
        let p = self.point(index);
        let Some(start) = self.locate(p) else { return };
        self.mark += 1;
        let mark = self.mark;
        // the hole: the triangle it is in, and every one about it whose circle holds it
        let mut bad = vec![start];
        put(&mut self.seen, start, mark);
        put(&mut self.bad_mark, start, mark);
        let mut cursor = 0;
        while let Some(&t) = bad.get(cursor) {
            cursor += 1;
            for i in 0..3 {
                let Some(next) = self.neighbor(t, i) else { continue };
                if at(&self.seen, next) == mark {
                    continue;
                }
                put(&mut self.seen, next, mark);
                let corners = [0, 1, 2].map(|k| self.point(self.vertex(next, k)));
                if incircle(at(&corners, 0), at(&corners, 1), at(&corners, 2), p) {
                    put(&mut self.bad_mark, next, mark);
                    bad.push(next);
                }
            }
        }
        // the hole's edge, each stretch with what lies past it
        let mut boundary: Vec<(usize, usize, Option<usize>)> = Vec::new();
        for &t in &bad {
            for i in 0..3 {
                let outer = self.neighbor(t, i);
                if outer.is_none_or(|o| at(&self.bad_mark, o) != mark) {
                    boundary.push((self.vertex(t, i), self.vertex(t, i + 1), outer));
                }
            }
        }
        for &t in &bad {
            put(&mut self.alive, t, false);
            self.free.push(t);
        }
        // a fan of triangles from the point to the hole's edge, each joined to the two beside it
        let mut created = Vec::with_capacity(boundary.len());
        for &(from, to, outer) in &boundary {
            let t = self.allocate([from, to, index], [outer, None, None]);
            if let Some(outer) = outer
                && let Some(slot) = self.edge_slot(to, from, outer)
            {
                put(&mut self.neighbors, outer * 3 + slot, Some(t));
            }
            created.push(t);
        }
        for (i, &(from, to, _)) in boundary.iter().enumerate() {
            let across_to = boundary.iter().position(|edge| edge.0 == to).map(|other| at(&created, other));
            let across_from = boundary.iter().position(|edge| edge.1 == from).map(|other| at(&created, other));
            let t = at(&created, i);
            put(&mut self.neighbors, t * 3 + 1, across_to);
            put(&mut self.neighbors, t * 3 + 2, across_from);
        }
        if let Some(&first) = created.first() {
            self.last = first;
        }
        put(&mut self.inserted, index, true);
    }

    /// The triangle with an edge from `v` to `target`, and which edge, found by going round `v`.
    fn triangle_with_edge(&self, v: usize, target: usize) -> Option<(usize, usize)> {
        let start = at(&self.vertex_triangle, v).filter(|&t| self.is_alive(t))?;
        self.local_index(v, start)?;
        let mut t = start;
        for _ in 0..512 {
            let k = self.local_index(v, t)?;
            if self.vertex(t, k + 1) == target {
                return Some((t, k));
            }
            t = self.neighbor(t, k)?;
            if t == start {
                return None;
            }
        }
        None
    }

    /// Makes the stretch from `a` to `b` an edge (flipping away what crosses it) and a wall. One
    /// that can't be forced in is left out: a wall missing softens gap finding there, no more.
    fn enforce(&mut self, (a, b, wall): Constraint) {
        if a == b || !at(&self.inserted, a) || !at(&self.inserted, b) {
            return;
        }
        let key = edge_key(a, b);
        if self.triangle_with_edge(a, b).is_some() {
            self.note_wall(key, wall);
            return;
        }
        let (pa, pb) = (self.point(a), self.point(b));
        // from `a` towards `b`, the edges crossed, each with the stretch's right side first
        let Some((mut t, mut right, mut left)) = self.first_crossed(a, pa, pb) else { return };
        let mut crossed = vec![(right, left)];
        let mut reached = false;
        for _ in 0..4096 {
            if self.walls.contains_key(&edge_key(right, left)) {
                return;
            }
            let Some(slot) = self.edge_slot(right, left, t).or_else(|| self.edge_slot(left, right, t)) else { return };
            let Some(next) = self.neighbor(t, slot) else { return };
            t = next;
            let Some(far) = (0..3).map(|i| self.vertex(t, i)).find(|&v| v != right && v != left) else { return };
            if far == b {
                reached = true;
                break;
            }
            let side = orient(pa, pb, self.point(far));
            if side > 0.0 {
                left = far;
            } else if side < 0.0 {
                right = far;
            } else {
                return;
            }
            crossed.push((right, left));
        }
        if !reached {
            return;
        }
        // flipped away, one at a time; one that can't flip yet (its four corners make no convex
        // shape) waits behind the rest, and the count stops those that never can
        let mut queue = crossed;
        let mut head = 0;
        let mut budget = queue.len() * 64 + 256;
        while let Some(&(u, v)) = queue.get(head) {
            if budget == 0 {
                break;
            }
            budget -= 1;
            head += 1;
            if !segments_cross(pa, pb, self.point(u), self.point(v)) {
                continue;
            }
            let Some((f0, f1)) = self.flip(u, v) else {
                queue.push((u, v));
                continue;
            };
            if f0 != a && f0 != b && f1 != a && f1 != b && segments_cross(pa, pb, self.point(f0), self.point(f1)) {
                queue.push((f0, f1));
            }
        }
        if self.triangle_with_edge(a, b).is_some() {
            self.note_wall(key, wall);
        }
    }

    fn note_wall(&mut self, key: u64, wall: usize) {
        let of = self.walls.entry(key).or_default();
        if !of.contains(&wall) {
            of.push(wall);
        }
    }

    /// The triangle at `a` that the stretch to `pb` leaves through, with the two ends of the edge
    /// it crosses: the one on its right, then the one on its left.
    fn first_crossed(&self, a: usize, pa: Point, pb: Point) -> Option<(usize, usize, usize)> {
        let home = at(&self.vertex_triangle, a).filter(|&t| self.is_alive(t))?;
        let mut t = home;
        for _ in 0..512 {
            let k = self.local_index(a, t)?;
            let (v1, v2) = (self.vertex(t, k + 1), self.vertex(t, k + 2));
            if orient(pa, pb, self.point(v1)) < 0.0 && orient(pa, pb, self.point(v2)) > 0.0 {
                return Some((t, v1, v2));
            }
            t = self.neighbor(t, k)?;
            if t == home {
                return None;
            }
        }
        None
    }

    /// Flips the edge between `u` and `v` to the other diagonal of the two triangles it is
    /// between; none if they make no strictly convex shape (it would fold). The new edge's ends.
    fn flip(&mut self, u: usize, v: usize) -> Option<(usize, usize)> {
        let (t1, i) = self.triangle_with_edge(u, v)?;
        let far1 = self.vertex(t1, i + 2);
        let t2 = self.neighbor(t1, i)?;
        let j = self.edge_slot(v, u, t2)?;
        let far2 = self.vertex(t2, j + 2);
        let (pu, pv, p1, p2) = (self.point(u), self.point(v), self.point(far1), self.point(far2));
        if !(orient(pu, p2, pv) > 0.0
            && orient(pv, p2, p1) < 0.0
            && orient(pu, p1, pv) < 0.0
            && orient(pu, p2, p1) > 0.0)
        {
            return None;
        }
        let across_v_far1 = self.neighbor(t1, i + 1);
        let across_far1_u = self.neighbor(t1, i + 2);
        let across_u_far2 = self.neighbor(t2, j + 1);
        let across_far2_v = self.neighbor(t2, j + 2);
        // the first becomes (u, far2, far1), the second (far2, v, far1)
        for (k, (corner, across)) in
            [(u, across_u_far2), (far2, Some(t2)), (far1, across_far1_u)].into_iter().enumerate()
        {
            put(&mut self.vertices, t1 * 3 + k, corner);
            put(&mut self.neighbors, t1 * 3 + k, across);
        }
        for (k, (corner, across)) in
            [(far2, across_far2_v), (v, across_v_far1), (far1, Some(t1))].into_iter().enumerate()
        {
            put(&mut self.vertices, t2 * 3 + k, corner);
            put(&mut self.neighbors, t2 * 3 + k, across);
        }
        if let Some(outer) = across_v_far1
            && let Some(slot) = self.edge_slot(far1, v, outer)
        {
            put(&mut self.neighbors, outer * 3 + slot, Some(t2));
        }
        if let Some(outer) = across_u_far2
            && let Some(slot) = self.edge_slot(far2, u, outer)
        {
            put(&mut self.neighbors, outer * 3 + slot, Some(t1));
        }
        put(&mut self.vertex_triangle, u, Some(t1));
        put(&mut self.vertex_triangle, v, Some(t2));
        put(&mut self.vertex_triangle, far1, Some(t1));
        put(&mut self.vertex_triangle, far2, Some(t2));
        Some((far1, far2))
    }
}

/// The triangulation as the fill goes over it: the real triangles only, each edge once with its
/// length and whether it is a wall.
struct Graph {
    count: usize,
    vertices: Vec<usize>,
    neighbors: Vec<Option<usize>>,
    edges: Vec<usize>,
    edge_length: Vec<f64>,
    edge_is_wall: Vec<bool>,
    wall_sources: HashMap<usize, Vec<usize>>,
    points: Vec<Point>,
}

impl Graph {
    fn of(delaunay: Delaunay) -> Option<Graph> {
        let mut compact: Vec<Option<usize>> = vec![None; delaunay.alive.len()];
        let mut slots = Vec::new();
        for slot in 0..delaunay.alive.len() {
            if delaunay.is_alive(slot) && (0..3).all(|i| delaunay.vertex(slot, i) < delaunay.real) {
                put(&mut compact, slot, Some(slots.len()));
                slots.push(slot);
            }
        }
        if slots.is_empty() {
            return None;
        }
        let mut graph = Graph {
            count: slots.len(),
            vertices: Vec::with_capacity(slots.len() * 3),
            neighbors: Vec::with_capacity(slots.len() * 3),
            edges: Vec::with_capacity(slots.len() * 3),
            edge_length: Vec::new(),
            edge_is_wall: Vec::new(),
            wall_sources: HashMap::new(),
            points: Vec::new(),
        };
        for (index, &slot) in slots.iter().enumerate() {
            for i in 0..3 {
                let (a, b) = (delaunay.vertex(slot, i), delaunay.vertex(slot, i + 1));
                let neighbor = delaunay.neighbor(slot, i).and_then(|n| at(&compact, n));
                graph.vertices.push(a);
                graph.neighbors.push(neighbor);
                // an edge shared with a triangle already done was noted from that side, the
                // other way round
                let shared = neighbor.filter(|&n| n < index).and_then(|n| {
                    (0..3)
                        .find(|&j| at(&graph.vertices, n * 3 + j) == b && at(&graph.vertices, n * 3 + (j + 1) % 3) == a)
                        .map(|j| at(&graph.edges, n * 3 + j))
                });
                let edge = shared.unwrap_or_else(|| {
                    let (pa, pb) = (delaunay.point(a), delaunay.point(b));
                    graph.edge_length.push(((pb[0] - pa[0]).powi(2) + (pb[1] - pa[1]).powi(2)).sqrt());
                    let sources = delaunay.walls.get(&edge_key(a, b));
                    graph.edge_is_wall.push(sources.is_some());
                    let edge = graph.edge_length.len() - 1;
                    if let Some(sources) = sources {
                        graph.wall_sources.insert(edge, sources.clone());
                    }
                    edge
                });
                graph.edges.push(edge);
            }
        }
        graph.points = delaunay.points;
        Some(graph)
    }

    fn vertex(&self, t: usize, i: usize) -> usize {
        at(&self.vertices, t * 3 + i % 3)
    }

    fn neighbor(&self, t: usize, i: usize) -> Option<usize> {
        at(&self.neighbors, t * 3 + i % 3)
    }

    fn edge(&self, t: usize, i: usize) -> usize {
        at(&self.edges, t * 3 + i % 3)
    }

    fn point(&self, v: usize) -> Point {
        at(&self.points, v)
    }

    fn area(&self, t: usize) -> f64 {
        orient(self.point(self.vertex(t, 0)), self.point(self.vertex(t, 1)), self.point(self.vertex(t, 2))).abs() / 2.0
    }

    fn holds(&self, t: usize, p: Point) -> bool {
        (0..3).all(|i| orient(self.point(self.vertex(t, i)), self.point(self.vertex(t, i + 1)), p) >= -1e-9)
    }

    /// The triangle `p` is in: walked to (never straight back), every one looked at if the walk
    /// goes nowhere. None when `p` is outside them all.
    fn locate(&self, p: Point) -> Option<usize> {
        if self.count == 0 {
            return None;
        }
        let (mut t, mut previous) = (0, None);
        'walk: for _ in 0..self.count + 64 {
            for i in 0..3 {
                if orient(self.point(self.vertex(t, i)), self.point(self.vertex(t, i + 1)), p) < 0.0
                    && let Some(next) = self.neighbor(t, i)
                    && Some(next) != previous
                {
                    previous = Some(t);
                    t = next;
                    continue 'walk;
                }
            }
            if self.holds(t, p) {
                return Some(t);
            }
            break;
        }
        (0..self.count).find(|&candidate| self.holds(candidate, p))
    }

    /// The narrowest opening on the widest way from `seed` to each triangle (nothing for one no
    /// way reaches), across edges that are no wall.
    fn bottlenecks(&self, seed: usize) -> (Vec<f64>, Vec<bool>) {
        let mut weight = vec![0.0; self.count];
        let mut reached = vec![false; self.count];
        put(&mut weight, seed, f64::INFINITY);
        put(&mut reached, seed, true);
        let mut queue = vec![seed];
        let mut head = 0;
        // (a way found wider is taken up again: widths only grow, so it settles)
        while let Some(&t) = queue.get(head) {
            head += 1;
            for j in 0..3 {
                let Some(next) = self.neighbor(t, j) else { continue };
                let edge = self.edge(t, j);
                if at(&self.edge_is_wall, edge) {
                    continue;
                }
                let reach = at(&self.edge_length, edge).min(at(&weight, t));
                if reach > at(&weight, next) {
                    put(&mut weight, next, reach);
                    put(&mut reached, next, true);
                    queue.push(next);
                }
            }
        }
        (weight, reached)
    }

    /// The triangles filled from `seed`: those joined to it that the fill reaches through at
    /// least `factor` of their own widest opening (all the walls bound, at no factor). A
    /// triangle's opening counts for no more than `cap`, the open space's grid step: its cells
    /// are entered across their sides, never their diagonals, and measured by a diagonal every
    /// side would read as a gap.
    fn filled(&self, seed: usize, factor: f64, cap: f64) -> Vec<bool> {
        let (weight, reached) = self.bottlenecks(seed);
        if factor <= 0.0 {
            return reached;
        }
        let widest: Vec<f64> = (0..self.count)
            .map(|t| {
                let open =
                    (0..3).filter(|&j| self.neighbor(t, j).is_some() && !at(&self.edge_is_wall, self.edge(t, j)));
                open.map(|j| at(&self.edge_length, self.edge(t, j))).fold(0.0, f64::max).min(cap)
            })
            .collect();
        // (the grid's edges differ only by the jitter: that mustn't decide a cell at full factor)
        let tolerance = 0.1;
        let mut filled = vec![false; self.count];
        put(&mut filled, seed, true);
        let mut queue = vec![seed];
        let mut head = 0;
        while let Some(&t) = queue.get(head) {
            head += 1;
            for j in 0..3 {
                let Some(next) = self.neighbor(t, j) else { continue };
                if at(&filled, next) || at(&self.edge_is_wall, self.edge(t, j)) {
                    continue;
                }
                if at(&reached, next) && at(&weight, next) + tolerance >= at(&widest, next) * factor {
                    put(&mut filled, next, true);
                    queue.push(next);
                }
            }
        }
        filled
    }

    /// The filled triangles' edge as closed outlines, each a run of vertices.
    fn outlines(&self, filled: &[bool]) -> Vec<Vec<usize>> {
        let mut by_start: HashMap<usize, Vec<usize>> = HashMap::new();
        let mut boundary: Vec<(usize, usize)> = Vec::new();
        for t in (0..self.count).filter(|&t| at(filled, t)) {
            for j in 0..3 {
                if self.neighbor(t, j).is_none_or(|next| !at(filled, next)) {
                    let (from, to) = (self.vertex(t, j), self.vertex(t, j + 1));
                    by_start.entry(from).or_default().push(boundary.len());
                    boundary.push((from, to));
                }
            }
        }
        let mut used = vec![false; boundary.len()];
        let mut loops = Vec::new();
        for start in 0..boundary.len() {
            if at(&used, start) {
                continue;
            }
            put(&mut used, start, true);
            let (first, mut current) = at(&boundary, start);
            let mut outline = vec![first];
            while current != first {
                outline.push(current);
                let next = by_start.get(&current).and_then(|edges| edges.iter().copied().find(|&e| !at(&used, e)));
                let Some(next) = next else { break };
                put(&mut used, next, true);
                current = at(&boundary, next).1;
            }
            if current == first && outline.len() >= 3 {
                loops.push(outline);
            }
        }
        loops
    }
}

/// `points` (a closed outline) kept to the canvas: lines may run past it, a fill never does.
fn clipped(points: Vec<Point>, width: f64, height: f64) -> Vec<Point> {
    fn against(input: Vec<Point>, inside: impl Fn(Point) -> bool, cut: impl Fn(Point, Point) -> Point) -> Vec<Point> {
        let mut out = Vec::with_capacity(input.len() + 4);
        let Some(&last) = input.last() else { return out };
        let mut previous = last;
        for current in input {
            match (inside(current), inside(previous)) {
                (true, true) => out.push(current),
                (true, false) => {
                    out.push(cut(previous, current));
                    out.push(current);
                }
                (false, true) => out.push(cut(previous, current)),
                (false, false) => {}
            }
            previous = current;
        }
        out
    }
    let at_x = |x: f64| move |a: Point, b: Point| [x, a[1] + (b[1] - a[1]) * (x - a[0]) / (b[0] - a[0])];
    let at_y = |y: f64| move |a: Point, b: Point| [a[0] + (b[0] - a[0]) * (y - a[1]) / (b[1] - a[1]), y];
    let points = against(points, |p| p[0] >= 0.0, at_x(0.0));
    let points = against(points, |p| p[0] <= width, at_x(width));
    let points = against(points, |p| p[1] >= 0.0, at_y(0.0));
    against(points, |p| p[1] <= height, at_y(height))
}

/// A closed outline with the points left out that the line through those kept passes within
/// `tolerance` of.
fn simplified(points: &[Point], tolerance: f64) -> Vec<Point> {
    if points.len() < 4 {
        return points.to_vec();
    }
    // from the point furthest from the first, which with the first is always kept
    let first = at(points, 0);
    let far = (0..points.len())
        .max_by(|&i, &j| {
            let d = |k: usize| (at(points, k)[0] - first[0]).powi(2) + (at(points, k)[1] - first[1]).powi(2);
            d(i).total_cmp(&d(j))
        })
        .unwrap_or(0);
    let mut keep = vec![false; points.len()];
    put(&mut keep, 0, true);
    put(&mut keep, far, true);
    // the outline as two runs between the two (the second back round to the first)
    let mut spans = vec![(0, far), (far, points.len())];
    while let Some((a, b)) = spans.pop() {
        if b <= a + 1 {
            continue;
        }
        let (pa, pb) = (at(points, a), at(points, b % points.len()));
        let (dx, dy) = (pb[0] - pa[0], pb[1] - pa[1]);
        let length = (dx * dx + dy * dy).sqrt().max(1e-12);
        let mut worst = (0.0, None);
        for i in a + 1..b {
            let p = at(points, i);
            let off = ((p[0] - pa[0]) * dy - (p[1] - pa[1]) * dx).abs() / length;
            if off > worst.0 {
                worst = (off, Some(i));
            }
        }
        if let (off, Some(i)) = worst
            && off > tolerance
        {
            put(&mut keep, i, true);
            spans.push((a, i));
            spans.push((i, b));
        }
    }
    points.iter().zip(keep).filter(|(_, kept)| *kept).map(|(p, _)| *p).collect()
}

/// The region around `seed` bounded by `walls`, on a canvas `width` × `height`: all of it the
/// walls close in, less what is reached only through gaps. `factor` (0..1) is how readily an
/// opening counts as a gap: at 0 none does, and higher finds more (smaller fills). None if the
/// seed is off the canvas or nothing can be made of it.
pub fn trace(walls: &[Wall], width: u32, height: u32, seed: Vec2, factor: f32) -> Option<Traced> {
    let (w, h) = (f64::from(width.max(1)), f64::from(height.max(1)));
    let seed = [f64::from(seed.x), f64::from(seed.y)];
    if !(seed[0] >= 0.0 && seed[0] < w && seed[1] >= 0.0 && seed[1] < h) {
        return None;
    }
    let (points, constraints) = build_input(walls, w, h);
    if points.len() < 3 {
        return None;
    }
    let order = insertion_order(&points);
    let mut delaunay = Delaunay::new(points);
    for index in order {
        delaunay.insert(index);
    }
    for constraint in constraints {
        delaunay.enforce(constraint);
    }
    let graph = Graph::of(delaunay)?;
    let seed_triangle = graph.locate(seed)?;
    let factor = if factor.is_finite() { f64::from(factor).clamp(0.0, 1.0) } else { 0.0 };
    let filled = graph.filled(seed_triangle, factor, guide_step(w, h));
    let mut loops = Vec::new();
    for outline in graph.outlines(&filled) {
        let points: Vec<Point> = outline.iter().map(|&v| graph.point(v)).collect();
        let kept = simplified(&clipped(points, w, h), 0.5);
        if kept.len() >= 3 {
            loops.push(kept.iter().map(|p| Vec2::new(p[0] as f32, p[1] as f32)).collect::<Vec<Vec2>>());
        }
    }
    if loops.is_empty() {
        return None;
    }
    let mut sources: Vec<u64> = Vec::new();
    let mut area = 0.0;
    for t in (0..graph.count).filter(|&t| at(&filled, t)) {
        area += graph.area(t);
        for j in 0..3 {
            for &wall in graph.wall_sources.get(&graph.edge(t, j)).into_iter().flatten() {
                if let Some(source) = walls.get(wall).map(|w| w.source)
                    && !sources.contains(&source)
                {
                    sources.push(source);
                }
            }
        }
    }
    sources.sort_unstable();
    Some(Traced { loops, sources, area: area as f32 })
}

/// How much of what is inside `of` is inside `loops` too (0 none of it, to 1 all), near enough:
/// by a grid of points over `of`. All of it for an `of` too slight for any point to land in.
pub fn covered(loops: &[Vec<Vec2>], of: &[Vec<Vec2>]) -> f32 {
    const ACROSS: usize = 32;
    let mut points = of.iter().flatten().filter(|p| p.is_finite());
    let Some(first) = points.next() else { return 1.0 };
    let [x0, y0, x1, y1] = points.fold([first.x, first.y, first.x, first.y], |[x0, y0, x1, y1], p| {
        [x0.min(p.x), y0.min(p.y), x1.max(p.x), y1.max(p.y)]
    });
    let (mut within, mut both) = (0u32, 0u32);
    for i in 0..ACROSS * ACROSS {
        let p = Vec2::new(
            x0 + (x1 - x0) * ((i % ACROSS) as f32 + 0.5) / ACROSS as f32,
            y0 + (y1 - y0) * ((i / ACROSS) as f32 + 0.5) / ACROSS as f32,
        );
        if inside(of, p) {
            within += 1;
            both += u32::from(inside(loops, p));
        }
    }
    if within == 0 { 1.0 } else { both as f32 / within as f32 }
}

/// Whether `point` is inside `loops` by the even-odd rule.
pub fn inside(loops: &[Vec<Vec2>], point: Vec2) -> bool {
    let mut crossings = 0;
    for outline in loops {
        for (a, b) in outline.iter().zip(outline.iter().cycle().skip(1)) {
            if (a.y > point.y) != (b.y > point.y) && point.x < a.x + (b.x - a.x) * (point.y - a.y) / (b.y - a.y) {
                crossings += 1;
            }
        }
    }
    crossings % 2 == 1
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wall(source: u64, points: &[(f32, f32)]) -> Wall {
        Wall { line: points.iter().map(|&(x, y)| Vec2::new(x, y)).collect(), source }
    }

    #[test]
    fn how_much_of_one_shape_another_covers() {
        let rect = |x0: f32, y0: f32, x1: f32, y1: f32| {
            vec![Vec2::new(x0, y0), Vec2::new(x1, y0), Vec2::new(x1, y1), Vec2::new(x0, y1)]
        };
        let big = vec![rect(0.0, 0.0, 400.0, 400.0)];
        let small = vec![rect(100.0, 100.0, 200.0, 200.0)];
        // the big one covers all of the small one, the small a sixteenth of the big
        assert_eq!(covered(&big, &small), 1.0);
        assert!((covered(&small, &big) - 1.0 / 16.0).abs() < 0.01, "{}", covered(&small, &big));
        assert_eq!(covered(&small, &small), 1.0);
        // with a hole where the small one is, the big one covers none of it
        let holed = vec![rect(0.0, 0.0, 400.0, 400.0), rect(100.0, 100.0, 200.0, 200.0)];
        assert_eq!(covered(&holed, &small), 0.0);
        // half over it
        let half = vec![rect(150.0, 0.0, 400.0, 400.0)];
        assert!((covered(&half, &small) - 0.5).abs() < 0.05);
        // nothing to cover is covered
        assert_eq!(covered(&small, &[]), 1.0);
    }

    /// A square from 100 to 300, drawn as one closed line.
    fn square() -> Wall {
        wall(7, &[(100.0, 100.0), (300.0, 100.0), (300.0, 300.0), (100.0, 300.0), (100.0, 100.0)])
    }

    #[test]
    fn a_closed_shape_is_filled_to_its_line() {
        let traced = trace(&[square()], 500, 500, Vec2::new(200.0, 200.0), 0.4).unwrap();
        assert!((traced.area - 40_000.0).abs() < 400.0, "{}", traced.area);
        assert_eq!(traced.sources, [7]);
        assert_eq!(traced.loops.len(), 1);
        assert!(inside(&traced.loops, Vec2::new(105.0, 105.0)) && inside(&traced.loops, Vec2::new(295.0, 295.0)));
        assert!(!inside(&traced.loops, Vec2::new(95.0, 200.0)) && !inside(&traced.loops, Vec2::new(305.0, 200.0)));
        // its outline is the square's four corners, near enough
        assert!(traced.loops[0].len() <= 8, "{:?}", traced.loops[0]);
        // and the same every time
        assert_eq!(trace(&[square()], 500, 500, Vec2::new(200.0, 200.0), 0.4).unwrap(), traced);
    }

    #[test]
    fn outside_a_shape_the_fill_is_the_canvas_with_the_shape_a_hole_in_it() {
        let traced = trace(&[square()], 500, 500, Vec2::new(20.0, 20.0), 0.4).unwrap();
        assert!((traced.area - (250_000.0 - 40_000.0)).abs() < 600.0, "{}", traced.area);
        assert_eq!(traced.loops.len(), 2, "the canvas's edge and the hole");
        assert!(inside(&traced.loops, Vec2::new(20.0, 480.0)) && inside(&traced.loops, Vec2::new(498.0, 2.0)));
        assert!(!inside(&traced.loops, Vec2::new(200.0, 200.0)), "the square is left out");
        // off the canvas there is nothing to fill
        assert!(trace(&[square()], 500, 500, Vec2::new(-5.0, 20.0), 0.4).is_none());
        assert!(trace(&[square()], 500, 500, Vec2::new(20.0, 500.0), 0.4).is_none());
    }

    /// The square with a gap `gap` wide in the middle of its right side.
    fn gapped(gap: f32) -> Vec<Wall> {
        let (top, bottom) = (200.0 - gap / 2.0, 200.0 + gap / 2.0);
        vec![wall(1, &[(300.0, top), (300.0, 100.0), (100.0, 100.0), (100.0, 300.0), (300.0, 300.0), (300.0, bottom)])]
    }

    #[test]
    fn a_gap_in_a_line_closes_itself() {
        // (on a canvas this size an opening counts as a gap up to 16 pixels at full sensitivity:
        // 6.4 at this one)
        for gap in [2.0, 4.0, 6.0] {
            let walls = gapped(gap);
            let within = trace(&walls, 500, 500, Vec2::new(200.0, 200.0), 0.4).unwrap();
            assert!((within.area - 40_000.0).abs() < 1200.0, "a gap of {gap}: {}", within.area);
            assert!(!inside(&within.loops, Vec2::new(350.0, 200.0)), "a gap of {gap} leaked");
            // from outside, the shape stays out too
            let without = trace(&walls, 500, 500, Vec2::new(20.0, 20.0), 0.4).unwrap();
            assert!(!inside(&without.loops, Vec2::new(200.0, 200.0)), "a gap of {gap} leaked in");
            assert!((without.area - 210_000.0).abs() < 1500.0, "a gap of {gap}: {}", without.area);
            // with no gap finding, the fill goes through it and takes the canvas
            let all = trace(&walls, 500, 500, Vec2::new(200.0, 200.0), 0.0).unwrap();
            assert!(all.area > 245_000.0, "a gap of {gap}: {}", all.area);
        }
        // a wider one is no gap at that sensitivity, and is at a higher
        let wide = gapped(12.0);
        assert!(trace(&wide, 500, 500, Vec2::new(200.0, 200.0), 0.4).unwrap().area > 245_000.0);
        let closed = trace(&wide, 500, 500, Vec2::new(200.0, 200.0), 1.0).unwrap();
        assert!((closed.area - 40_000.0).abs() < 2000.0, "{}", closed.area);
    }

    #[test]
    fn an_opening_too_wide_to_be_a_gap_lets_the_fill_through() {
        // the right side nearly all missing: an open shape, not a gap
        let open = [wall(
            1,
            &[(300.0, 110.0), (300.0, 100.0), (100.0, 100.0), (100.0, 300.0), (300.0, 300.0), (300.0, 290.0)],
        )];
        let traced = trace(&open, 500, 500, Vec2::new(200.0, 200.0), 0.4).unwrap();
        assert!(traced.area > 240_000.0, "{}", traced.area);
        // the more readily gaps are found, the less the fill takes
        let narrow = gapped(14.0);
        let sizes: Vec<f32> = [0.0, 0.2, 0.6, 1.0]
            .map(|factor| trace(&narrow, 500, 500, Vec2::new(200.0, 200.0), factor).unwrap().area)
            .to_vec();
        assert!(sizes.windows(2).all(|w| w[1] <= w[0] + 1.0), "{sizes:?}");
    }

    #[test]
    fn lines_that_cross_bound_the_cells_between_them() {
        // two across and two down: nine cells, the middle one closed in by all four
        let grid = [
            wall(1, &[(50.0, 200.0), (450.0, 200.0)]),
            wall(2, &[(50.0, 300.0), (450.0, 300.0)]),
            wall(3, &[(200.0, 50.0), (200.0, 450.0)]),
            wall(4, &[(300.0, 50.0), (300.0, 450.0)]),
        ];
        let middle = trace(&grid, 500, 500, Vec2::new(250.0, 250.0), 0.4).unwrap();
        assert!((middle.area - 10_000.0).abs() < 300.0, "{}", middle.area);
        assert_eq!(middle.sources, [1, 2, 3, 4]);
        assert!(!inside(&middle.loops, Vec2::new(250.0, 150.0)) && !inside(&middle.loops, Vec2::new(350.0, 250.0)));
    }

    #[test]
    fn a_shape_inside_another_is_left_out_of_its_fill() {
        let inner = wall(2, &[(180.0, 180.0), (220.0, 180.0), (220.0, 220.0), (180.0, 220.0), (180.0, 180.0)]);
        let traced = trace(&[square(), inner], 500, 500, Vec2::new(120.0, 120.0), 0.4).unwrap();
        assert!((traced.area - (40_000.0 - 1600.0)).abs() < 500.0, "{}", traced.area);
        assert_eq!(traced.loops.len(), 2);
        assert!(!inside(&traced.loops, Vec2::new(200.0, 200.0)));
        assert!(inside(&traced.loops, Vec2::new(250.0, 250.0)));
        assert_eq!(traced.sources, [2, 7]);
    }

    #[test]
    fn nothing_to_fill_against_fills_the_canvas() {
        let traced = trace(&[], 400, 300, Vec2::new(10.0, 10.0), 0.4).unwrap();
        assert!((traced.area - 120_000.0).abs() < 10.0, "{}", traced.area);
        assert!(traced.sources.is_empty());
    }

    #[test]
    fn a_curved_shape_drawn_by_hand_is_filled() {
        // a wobbly circle of many close points, its ends not quite meeting
        let ring: Vec<(f32, f32)> = (0..=358)
            .map(|deg| {
                let a = (deg as f32).to_radians();
                let r = 120.0 + 6.0 * (a * 7.0).sin();
                (250.0 + r * a.cos(), 250.0 + r * a.sin())
            })
            .collect();
        let traced = trace(&[wall(1, &ring)], 500, 500, Vec2::new(250.0, 250.0), 0.4).unwrap();
        let disc = std::f32::consts::PI * 120.0 * 120.0;
        assert!((traced.area - disc).abs() < disc * 0.05, "{} of {disc}", traced.area);
        assert!(!inside(&traced.loops, Vec2::new(250.0, 100.0)));
    }
}

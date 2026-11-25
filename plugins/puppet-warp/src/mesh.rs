//! The triangles a puppet warp lays over what is painted, following its outline.
//!
//! What is painted is grown by the expansion (and a little more, so the outline once simplified
//! still takes in every painted pixel), any hollow it closes is filled, and its outline is traced
//! (marching squares over the distance from the paint) and simplified (Douglas–Peucker) into
//! polygons, one for each piece apart. Each polygon is cut into triangles (ear clipping), made a
//! constrained Delaunay triangulation (Lawson's flips, the outline's edges kept) and refined by
//! Ruppert's method ("A Delaunay refinement algorithm for quality 2-dimensional mesh
//! generation", 1995): a triangle too big or too thin gets a vertex at the centre of its
//! circumcircle, and a stretch of outline such a vertex would crowd is halved instead. So the
//! triangles keep to the outline, about the spacing on a side, smaller where it turns tightly.

use std::collections::{HashMap, VecDeque};

use crate::geom::Vec2;

type Point = [f64; 2];

/// No triangle (across an edge of the outline) or no vertex.
const NONE: u32 = u32::MAX;

/// The most grid nodes the outline is traced over: a bigger drawing is traced over a coarser grid
/// than its pixels.
const MOST_NODES: usize = 2_000_000;
/// How far the traced outline may be simplified, against the spacing.
const SIMPLIFY: f64 = 0.1;
/// The biggest triangle: its circumradius against the spacing. Refined to this, the triangles
/// come out about the spacing on a side, as many as a grid of square cells the spacing across
/// would have (an equilateral triangle the spacing on a side has 1/√3 of it), the biggest about
/// 1.6 times it.
const MOST_RADIUS: f64 = 0.8;
/// The thinnest: its circumradius against its shortest edge (√2: no angle under about 20.7°).
const MOST_RATIO: f64 = std::f64::consts::SQRT_2;
/// Stretches of outline shorter than this share of the spacing aren't halved again.
const LEAST_SPLIT: f64 = 0.15;
/// A node no paint is near (its squared distance).
const FAR: f64 = 1e20;

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

/// Corner or neighbour `k` (taken round, so `k + 1` and `k + 2` are the next two) of a triangle.
fn of(three: [u32; 3], k: usize) -> u32 {
    three.get(k % 3).copied().unwrap_or(NONE)
}

/// Which of the three is `x`.
fn slot(three: [u32; 3], x: u32) -> Option<usize> {
    three.iter().position(|&y| y == x)
}

fn orient(a: Point, b: Point, c: Point) -> f64 {
    (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0])
}

fn dist(a: Point, b: Point) -> f64 {
    (b[0] - a[0]).hypot(b[1] - a[1])
}

fn mid(a: Point, b: Point) -> Point {
    [(a[0] + b[0]) * 0.5, (a[1] + b[1]) * 0.5]
}

/// Whether `d` lies inside the circle through `a`, `b`, `c` (anticlockwise), clear of it.
fn in_circle(a: Point, b: Point, c: Point, d: Point) -> bool {
    let (adx, ady, bdx, bdy, cdx, cdy) = (a[0] - d[0], a[1] - d[1], b[0] - d[0], b[1] - d[1], c[0] - d[0], c[1] - d[1]);
    let t1 = (adx * adx + ady * ady) * (bdx * cdy - cdx * bdy);
    let t2 = (bdx * bdx + bdy * bdy) * (cdx * ady - adx * cdy);
    let t3 = (cdx * cdx + cdy * cdy) * (adx * bdy - bdx * ady);
    t1 + t2 + t3 > 1e-10 * (t1.abs() + t2.abs() + t3.abs())
}

/// The centre of the circle through `a`, `b`, `c`, and its radius.
fn circumcircle(a: Point, b: Point, c: Point) -> Option<(Point, f64)> {
    let (bx, by, cx, cy) = (b[0] - a[0], b[1] - a[1], c[0] - a[0], c[1] - a[1]);
    let d = 2.0 * (bx * cy - by * cx);
    let (b2, c2) = (bx * bx + by * by, cx * cx + cy * cy);
    if d.abs() <= 1e-12 * (b2 + c2) {
        return None;
    }
    let (ux, uy) = ((cy * b2 - by * c2) / d, (bx * c2 - cx * b2) / d);
    let centre = [a[0] + ux, a[1] + uy];
    (centre[0].is_finite() && centre[1].is_finite()).then(|| (centre, ux.hypot(uy)))
}

/// How far `p` is from the stretch `a`–`b`.
fn segment_distance(p: Point, a: Point, b: Point) -> f64 {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx * dx + dy * dy;
    let t = if len > 0.0 { (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / len).clamp(0.0, 1.0) } else { 0.0 };
    dist(p, [a[0] + t * dx, a[1] + t * dy])
}

/// Whether stretches `a`–`b` and `c`–`d` cross, each through the other.
fn cross(a: Point, b: Point, c: Point, d: Point) -> bool {
    orient(a, b, c) * orient(a, b, d) < 0.0 && orient(c, d, a) * orient(c, d, b) < 0.0
}

/// Twice the area a ring closes (positive when it goes anticlockwise, as an outer outline here
/// does).
fn ring_area(ring: &[Point]) -> f64 {
    let n = ring.len();
    (0..n)
        .map(|i| {
            let (a, b) = (at(ring, i), at(ring, (i + 1) % n));
            a[0] * b[1] - b[0] * a[1]
        })
        .sum()
}

/// The box (x0, y0, x1, y1, pixels, the end ones past it) of the pixels of `alpha` (one byte a
/// pixel, `width` across) that aren't clear; none when all are.
pub fn painted(alpha: &[u8], width: usize) -> Option<[usize; 4]> {
    if width == 0 {
        return None;
    }
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0, 0);
    for (y, row) in alpha.chunks_exact(width).enumerate() {
        let Some(first) = row.iter().position(|&a| a != 0) else { continue };
        let last = row.iter().rposition(|&a| a != 0).unwrap_or(first);
        x0 = x0.min(first);
        x1 = x1.max(last + 1);
        y0 = y0.min(y);
        y1 = y + 1;
    }
    (x0 < x1 && y0 < y1).then_some([x0, y0, x1, y1])
}

/// One piece of the mesh: its vertices (source points) and its triangles (anticlockwise as the
/// numbers go, y down), the vertices numbered so each triangle's are near one another.
#[derive(Clone, Debug, Default)]
pub struct Piece {
    pub verts: Vec<Vec2>,
    pub tris: Vec<[u32; 3]>,
}

/// The triangles over the pixels of `alpha` (one byte a pixel, `width` across, its first pixel at
/// source point `origin`) that aren't clear, within `bounds` (from [`painted`]): reaching
/// `expansion` past them, hollows filled, the triangles about `spacing` on a side. A piece for each
/// part apart from the rest.
pub fn lay(alpha: &[u8], width: usize, bounds: [usize; 4], origin: Vec2, expansion: f32, spacing: f32) -> Vec<Piece> {
    let [x0, y0, x1, y1] = bounds;
    if width == 0 || x0 >= x1 || y0 >= y1 || spacing.is_nan() || spacing <= 0.0 {
        return Vec::new();
    }
    let spacing = f64::from(spacing);
    let expansion = f64::from(expansion.max(0.0));
    let tolerance = SIMPLIFY * spacing;
    // the grid the outline is traced over: the pixels, unless that's too many
    let margin = 2.0 * (expansion + tolerance + 4.0);
    let nodes = ((x1 - x0) as f64 + margin) * ((y1 - y0) as f64 + margin);
    let step = (nodes / MOST_NODES as f64).sqrt().ceil().max(1.0) as usize;
    // the outline lies as far out as the expansion and as far again as simplifying may cut in
    let reach = expansion + tolerance + 0.5 * step as f64;
    let field = Field::new(alpha, width, bounds, step, reach);
    let corner = [
        f64::from(origin.x) + x0 as f64 - (field.pad * step) as f64,
        f64::from(origin.y) + y0 as f64 - (field.pad * step) as f64,
    ];
    let scale = step as f64;
    field
        .outlines()
        .into_iter()
        .filter_map(|ring| {
            let ring: Vec<Point> = ring.iter().map(|p| [p[0] * scale, p[1] * scale]).collect();
            let mut mesh = Triangulation::of_polygon(&simplified(&ring, tolerance))?;
            mesh.refine(spacing);
            Some(mesh.piece(corner))
        })
        .collect()
}

/// Over a grid of nodes `step` pixels apart, each node's distance from the paint less the reach:
/// at or below 0 inside the outline.
struct Field {
    cols: usize,
    rows: usize,
    /// How many nodes lie before the first painted pixel's, each way.
    pad: usize,
    values: Vec<f32>,
}

impl Field {
    fn new(alpha: &[u8], width: usize, bounds: [usize; 4], step: usize, reach: f64) -> Field {
        let [x0, y0, x1, y1] = bounds;
        let pad = (reach / step as f64).ceil() as usize + 2;
        let cols = (x1 - x0).div_ceil(step) + 1 + 2 * pad;
        let rows = (y1 - y0).div_ceil(step) + 1 + 2 * pad;
        // every corner of every square of the grid holding paint is on the paint
        let mut d2 = vec![FAR; cols * rows];
        for (y, row) in alpha.chunks_exact(width).enumerate().take(y1).skip(y0) {
            let k = ((y - y0) / step + pad) * cols + pad;
            for (x, &a) in row.iter().enumerate().take(x1).skip(x0) {
                if a != 0 {
                    let k = k + (x - x0) / step;
                    for n in [k, k + 1, k + cols, k + cols + 1] {
                        put(&mut d2, n, 0.0);
                    }
                }
            }
        }
        distance_transform(&mut d2, cols, rows);
        let mut values: Vec<f32> = d2.iter().map(|&d| (d.sqrt() * step as f64 - reach) as f32).collect();
        fill_hollows(&mut values, cols, rows, -reach as f32);
        Field { cols, rows, pad, values }
    }

    fn value(&self, k: usize) -> f32 {
        self.values.get(k).copied().unwrap_or(1.0)
    }

    /// Where the outline crosses grid edge `id` (node units): the edge right of node `id / 2` when
    /// `id` is even, below it when odd.
    fn crossing(&self, id: usize) -> Point {
        let k = id / 2;
        let (i, j) = ((k % self.cols) as f64, (k / self.cols) as f64);
        let (other, dx, dy) = if id.is_multiple_of(2) { (k + 1, 1.0, 0.0) } else { (k + self.cols, 0.0, 1.0) };
        let (a, b) = (f64::from(self.value(k)), f64::from(self.value(other)));
        let t = if (a - b).abs() > 1e-12 { (a / (a - b)).clamp(1e-3, 1.0 - 1e-3) } else { 0.5 };
        [i + t * dx, j + t * dy]
    }

    /// The outer outlines (node units), each a ring anticlockwise as the numbers go; the outlines
    /// of holes are left out.
    fn outlines(&self) -> Vec<Vec<Point>> {
        let (cols, rows) = (self.cols, self.rows);
        if cols < 2 || rows < 2 {
            return Vec::new();
        }
        // each crossing, the one the outline goes on to (inside kept on its left)
        let mut next = vec![NONE; 2 * cols * rows];
        for j in 0..rows - 1 {
            for i in 0..cols - 1 {
                let k = j * cols + i;
                let values = [self.value(k), self.value(k + 1), self.value(k + cols + 1), self.value(k + cols)];
                let inside = values.map(|v| v <= 0.0);
                // the square's corners round (top left, top right, bottom right, bottom left) and
                // its edges round, edge e from corner e to the next
                let (x, y) = (i as f64, j as f64);
                let corners = [[x, y], [x + 1.0, y], [x + 1.0, y + 1.0], [x, y + 1.0]];
                let edges = [2 * k, 2 * (k + 1) + 1, 2 * (k + cols), 2 * k + 1];
                let corner = |c: usize| corners.get(c % 4).copied().unwrap_or_default();
                let edge = |e: usize| edges.get(e % 4).copied().unwrap_or_default();
                let is_in = |c: usize| inside.get(c % 4).copied().unwrap_or(false);
                let mut link = |ea: usize, eb: usize, q: Point| {
                    let (a, b) = (edge(ea), edge(eb));
                    let (from, to) = if orient(self.crossing(a), self.crossing(b), q) > 0.0 { (a, b) } else { (b, a) };
                    put(&mut next, from, to as u32);
                };
                let crossed: Vec<usize> = (0..4).filter(|&e| is_in(e) != is_in(e + 1)).collect();
                match crossed.as_slice() {
                    &[ea, eb] => {
                        let q = (0..4).find(|&c| is_in(c)).map(corner).unwrap_or_default();
                        link(ea, eb, q);
                    }
                    [_, _, _, _] => {
                        // a saddle: the middle decides which corners are joined
                        let centre_in = values.iter().sum::<f32>() <= 0.0;
                        for c in (0..4).filter(|&c| is_in(c) != centre_in) {
                            let q = if is_in(c) { corner(c) } else { [x + 0.5, y + 0.5] };
                            link(c + 3, c, q);
                        }
                    }
                    _ => {}
                }
            }
        }
        let mut seen = vec![false; next.len()];
        let mut rings = Vec::new();
        for start in 0..next.len() {
            if at(&next, start) == NONE || at(&seen, start) {
                continue;
            }
            let mut ring = Vec::new();
            let mut id = start;
            while id != NONE as usize && !at(&seen, id) {
                put(&mut seen, id, true);
                ring.push(self.crossing(id));
                id = at(&next, id) as usize;
            }
            if ring.len() >= 3 && ring_area(&ring) > 0.0 {
                rings.push(ring);
            }
        }
        rings
    }
}

/// Squared distances (in nodes) from the nodes at 0, in place: Felzenszwalb and Huttenlocher,
/// "Distance transforms of sampled functions" (2012), down each column then along each row.
fn distance_transform(d2: &mut [f64], cols: usize, rows: usize) {
    let n = cols.max(rows);
    let (mut f, mut out, mut v, mut z) = (vec![0.0; n], vec![0.0; n], vec![0usize; n], vec![0.0; n + 1]);
    for x in 0..cols {
        for y in 0..rows {
            put(&mut f, y, at(d2, y * cols + x));
        }
        parabolas(&f, rows, &mut out, &mut v, &mut z);
        for y in 0..rows {
            put(d2, y * cols + x, at(&out, y));
        }
    }
    for y in 0..rows {
        for x in 0..cols {
            put(&mut f, x, at(d2, y * cols + x));
        }
        parabolas(&f, cols, &mut out, &mut v, &mut z);
        for x in 0..cols {
            put(d2, y * cols + x, at(&out, x));
        }
    }
}

/// The lower envelope of the parabolas rooted at each of the first `n` of `f`, into `out`.
fn parabolas(f: &[f64], n: usize, out: &mut [f64], v: &mut [usize], z: &mut [f64]) {
    if n == 0 {
        return;
    }
    let meet = |q: usize, p: usize| {
        let (qf, pf) = (q as f64, p as f64);
        ((at(f, q) + qf * qf) - (at(f, p) + pf * pf)) / (2.0 * qf - 2.0 * pf)
    };
    let mut k = 0usize;
    put(v, 0, 0);
    put(z, 0, f64::NEG_INFINITY);
    put(z, 1, f64::INFINITY);
    for q in 1..n {
        let mut s = meet(q, at(v, k));
        while k > 0 && s <= at(z, k) {
            k -= 1;
            s = meet(q, at(v, k));
        }
        k += 1;
        put(v, k, q);
        put(z, k, s);
        put(z, k + 1, f64::INFINITY);
    }
    k = 0;
    for q in 0..n {
        while at(z, k + 1) < q as f64 {
            k += 1;
        }
        let p = at(v, k);
        let d = q as f64 - p as f64;
        put(out, q, d * d + at(f, p));
    }
}

/// Every node outside (above 0) that can't be reached from the grid's edge through others
/// outside (diagonals too) set to `inside`.
fn fill_hollows(values: &mut [f32], cols: usize, rows: usize, inside: f32) {
    let mut reached = vec![false; values.len()];
    let mut todo: Vec<usize> = (0..cols).flat_map(|x| [x, (rows - 1) * cols + x]).collect();
    todo.extend((0..rows).flat_map(|y| [y * cols, y * cols + cols - 1]));
    while let Some(k) = todo.pop() {
        if at(&reached, k) || values.get(k).is_none_or(|&v| v <= 0.0) {
            continue;
        }
        put(&mut reached, k, true);
        let (x, y) = ((k % cols) as isize, (k / cols) as isize);
        for (dx, dy) in [(-1, -1), (0, -1), (1, -1), (-1, 0), (1, 0), (-1, 1), (0, 1), (1, 1)] {
            let (nx, ny) = (x + dx, y + dy);
            if nx >= 0 && ny >= 0 && (nx as usize) < cols && (ny as usize) < rows {
                todo.push(ny as usize * cols + nx as usize);
            }
        }
    }
    for (v, reached) in values.iter_mut().zip(reached) {
        if *v > 0.0 && !reached {
            *v = inside;
        }
    }
}

/// `ring` simplified to within `tolerance` (Douglas–Peucker, kept from crossing itself): at least
/// three points, each one of the ring's.
fn simplified(ring: &[Point], tolerance: f64) -> Vec<Point> {
    let n = ring.len();
    if n < 4 {
        return ring.to_vec();
    }
    let p = |i: usize| at(ring, i % n);
    let far = (1..n).max_by(|&a, &b| dist(p(0), p(a)).total_cmp(&dist(p(0), p(b)))).unwrap_or(n / 2);
    let mut keep = vec![false; n];
    put(&mut keep, 0, true);
    put(&mut keep, far, true);
    // the furthest point of a stretch from its chord (indices taken round)
    let furthest = |lo: usize, hi: usize| {
        let (a, b) = (p(lo), p(hi));
        (lo + 1..hi).map(|k| (k, segment_distance(p(k), a, b))).max_by(|x, y| x.1.total_cmp(&y.1))
    };
    // the two halves keep their furthest points whatever, so there are at least three
    let mut spans = vec![(0, far, true), (far, n, true)];
    while let Some((lo, hi, always)) = spans.pop() {
        if let Some((k, d)) = furthest(lo, hi)
            && (always || d > tolerance)
        {
            put(&mut keep, k % n, true);
            spans.push((lo, k, false));
            spans.push((k, hi, false));
        }
    }
    // where the simplified ring would cross itself, put back points until it doesn't
    for _ in 0..64 {
        let kept: Vec<usize> = (0..n).filter(|&i| at(&keep, i)).collect();
        let points: Vec<Point> = kept.iter().map(|&i| p(i)).collect();
        let crossing = self_crossings(&points);
        if !crossing.contains(&true) {
            return points;
        }
        let mut grew = false;
        for (s, _) in crossing.iter().enumerate().filter(|(_, c)| **c) {
            let lo = at(&kept, s);
            let hi = if s + 1 < kept.len() { at(&kept, s + 1) } else { at(&kept, 0) + n };
            if let Some((k, _)) = furthest(lo, hi) {
                put(&mut keep, k % n, true);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    ring.to_vec()
}

/// For each edge of the closed ring `points` (from each point to the next), whether it crosses
/// another.
fn self_crossings(points: &[Point]) -> Vec<bool> {
    let m = points.len();
    let mut crossing = vec![false; m];
    if m < 4 {
        return crossing;
    }
    let edge = |s: usize| (at(points, s), at(points, (s + 1) % m));
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in points {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let size = ((hi[0] - lo[0]).max(hi[1] - lo[1]) / (m as f64).sqrt()).max(1e-6);
    let cell = |v: f64, o: f64| ((v - o) / size).floor() as i64;
    let mut buckets: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
    for s in 0..m {
        let (a, b) = edge(s);
        for x in cell(a[0].min(b[0]), lo[0])..=cell(a[0].max(b[0]), lo[0]) {
            for y in cell(a[1].min(b[1]), lo[1])..=cell(a[1].max(b[1]), lo[1]) {
                buckets.entry((x, y)).or_default().push(s);
            }
        }
    }
    for list in buckets.values() {
        for (i, &s) in list.iter().enumerate() {
            for &t in list.iter().skip(i + 1) {
                let near = s.abs_diff(t) == 1 || s.abs_diff(t) == m - 1;
                let ((a, b), (c, d)) = (edge(s), edge(t));
                if !near && cross(a, b, c, d) {
                    put(&mut crossing, s, true);
                    put(&mut crossing, t, true);
                }
            }
        }
    }
    crossing
}

/// A triangle: its corners anticlockwise (as the numbers go) and, across from each, the
/// neighbouring triangle or [`NONE`] where that edge is the outline's.
#[derive(Clone, Copy, Debug, Default)]
struct Tri {
    v: [u32; 3],
    n: [u32; 3],
}

/// Where a walk toward a point ended.
enum Walk {
    /// In this triangle, on its edge across from that corner if so.
    In(u32, Option<usize>),
    /// Stopped at the outline: this triangle's edge across from that corner.
    Out(u32, usize),
}

/// A triangulation of one polygon, each edge of the polygon an edge of it.
struct Triangulation {
    pts: Vec<Point>,
    tris: Vec<Tri>,
}

impl Triangulation {
    fn pt(&self, v: u32) -> Point {
        at(&self.pts, v as usize)
    }

    /// Triangle `t`; across the outline ([`NONE`]), one with no corners and no neighbours.
    fn tri(&self, t: u32) -> Tri {
        self.tris.get(t as usize).copied().unwrap_or(Tri { v: [NONE; 3], n: [NONE; 3] })
    }

    fn set(&mut self, t: u32, tri: Tri) {
        put(&mut self.tris, t as usize, tri);
    }

    /// The edge of triangle `t` across from corner `i`: its two ends.
    fn edge(&self, t: u32, i: usize) -> (Point, Point) {
        let tri = self.tri(t);
        (self.pt(of(tri.v, i + 1)), self.pt(of(tri.v, i + 2)))
    }

    /// Triangle `x`'s neighbour `old` made `new`.
    fn relink(&mut self, x: u32, old: u32, new: u32) {
        if let Some(tri) = self.tris.get_mut(x as usize) {
            for n in &mut tri.n {
                if *n == old {
                    *n = new;
                }
            }
        }
    }

    /// The polygon `ring` (anticlockwise, not crossing itself) cut into triangles and made
    /// constrained Delaunay.
    fn of_polygon(ring: &[Point]) -> Option<Triangulation> {
        let m = ring.len();
        if m < 3 || ring_area(ring) <= 0.0 {
            return None;
        }
        let p = |i: usize| at(ring, i);
        let mut prev: Vec<usize> = (0..m).map(|i| (i + m - 1) % m).collect();
        let mut next: Vec<usize> = (0..m).map(|i| (i + 1) % m).collect();
        let ear = |prev: &[usize], next: &[usize], i: usize| {
            let (a, c) = (at(prev, i), at(next, i));
            let (pa, pb, pc) = (p(a), p(i), p(c));
            if orient(pa, pb, pc) <= 0.0 {
                return false;
            }
            // no corner turning inward lies in it
            let mut k = at(next, c);
            while k != a {
                let pk = p(k);
                let reflex = orient(p(at(prev, k)), pk, p(at(next, k))) <= 0.0;
                if reflex
                    && pk != pa
                    && pk != pc
                    && orient(pa, pb, pk) >= 0.0
                    && orient(pb, pc, pk) >= 0.0
                    && orient(pc, pa, pk) >= 0.0
                {
                    return false;
                }
                k = at(next, k);
            }
            true
        };
        let mut is_ear: Vec<bool> = (0..m).map(|i| ear(&prev, &next, i)).collect();
        let mut corners: Vec<[u32; 3]> = Vec::with_capacity(m - 2);
        let (mut left, mut i, mut misses, mut rechecked) = (m, 0usize, 0usize, false);
        while left > 3 {
            let clip = if at(&is_ear, i) {
                Some(i)
            } else {
                i = at(&next, i);
                misses += 1;
                if misses <= left {
                    None
                } else if !rechecked {
                    // ears may have opened that weren't looked at again: look at every corner
                    let mut k = i;
                    for _ in 0..left {
                        put(&mut is_ear, k, ear(&prev, &next, k));
                        k = at(&next, k);
                    }
                    (misses, rechecked) = (0, true);
                    None
                } else {
                    // none (a ring all but straight somewhere): the corner that turns most
                    let mut k = i;
                    let mut best = (i, f64::MIN);
                    for _ in 0..left {
                        let turn = orient(p(at(&prev, k)), p(k), p(at(&next, k)));
                        if turn > best.1 {
                            best = (k, turn);
                        }
                        k = at(&next, k);
                    }
                    Some(best.0)
                }
            };
            let Some(e) = clip else { continue };
            let (a, c) = (at(&prev, e), at(&next, e));
            corners.push([a as u32, e as u32, c as u32]);
            put(&mut next, a, c);
            put(&mut prev, c, a);
            left -= 1;
            put(&mut is_ear, a, ear(&prev, &next, a));
            put(&mut is_ear, c, ear(&prev, &next, c));
            (i, misses, rechecked) = (c, 0, false);
        }
        corners.push([at(&prev, i) as u32, i as u32, at(&next, i) as u32]);
        // neighbours: the triangle across each edge, found by the edge the other way round
        let mut by_edge: HashMap<(u32, u32), (u32, usize)> = HashMap::new();
        for (t, v) in corners.iter().enumerate() {
            for k in 0..3 {
                by_edge.insert((of(*v, k + 1), of(*v, k + 2)), (t as u32, k));
            }
        }
        let tris = corners
            .iter()
            .map(|&v| {
                let n = [0, 1, 2].map(|k| by_edge.get(&(of(v, k + 2), of(v, k + 1))).map_or(NONE, |&(t, _)| t));
                Tri { v, n }
            })
            .collect();
        let mut mesh = Triangulation { pts: ring.to_vec(), tris };
        mesh.make_delaunay();
        Some(mesh)
    }

    /// Lawson's flips until every edge but the outline's is locally Delaunay.
    fn make_delaunay(&mut self) {
        let mut stack: Vec<(u32, usize)> = (0..self.tris.len() as u32).flat_map(|t| [(t, 0), (t, 1), (t, 2)]).collect();
        let mut budget = 64 * self.tris.len() + 1024;
        while let Some((t, i)) = stack.pop() {
            budget = budget.saturating_sub(1);
            if budget == 0 {
                break;
            }
            let tri = self.tri(t);
            let u = of(tri.n, i);
            let Some(j) = slot(self.tri(u).n, t) else { continue };
            let d = self.pt(of(self.tri(u).v, j));
            let [a, b, c] = [i, i + 1, i + 2].map(|k| self.pt(of(tri.v, k)));
            if in_circle(a, b, c, d) && orient(a, b, d) > 0.0 && orient(a, d, c) > 0.0 {
                self.flip(t, i);
                for k in 0..3 {
                    stack.push((t, k));
                    stack.push((u, k));
                }
            }
        }
    }

    /// Flips the edge of triangle `t` across from its corner `i` (a), with the triangle beyond
    /// it: `t` becomes (a, b, d) and that triangle (a, d, c), a first in both.
    fn flip(&mut self, t: u32, i: usize) {
        let tri = self.tri(t);
        let (a, b, c) = (of(tri.v, i), of(tri.v, i + 1), of(tri.v, i + 2));
        let (u, n_ca, n_ab) = (of(tri.n, i), of(tri.n, i + 1), of(tri.n, i + 2));
        let beyond = self.tri(u);
        let Some(j) = slot(beyond.n, t) else { return };
        let (d, n_bd, n_dc) = (of(beyond.v, j), of(beyond.n, j + 1), of(beyond.n, j + 2));
        self.set(t, Tri { v: [a, b, d], n: [n_bd, u, n_ab] });
        self.set(u, Tri { v: [a, d, c], n: [n_dc, n_ca, t] });
        self.relink(n_bd, u, t);
        self.relink(n_ca, t, u);
    }

    /// Flips away every edge across from the new vertex (each triangle's first corner) in
    /// `stack` that isn't locally Delaunay; every triangle it changes is noted in `touched`.
    fn legalize(&mut self, mut stack: Vec<u32>, touched: &mut Vec<u32>) {
        let mut budget = 64 * self.tris.len() + 1024;
        while let Some(t) = stack.pop() {
            touched.push(t);
            budget = budget.saturating_sub(1);
            let tri = self.tri(t);
            let u = of(tri.n, 0);
            let Some(j) = slot(self.tri(u).n, t) else { continue };
            let d = self.pt(of(self.tri(u).v, j));
            let [p, b, c] = tri.v.map(|v| self.pt(v));
            if budget > 0 && in_circle(p, b, c, d) && orient(p, b, d) > 0.0 && orient(p, d, c) > 0.0 {
                self.flip(t, 0);
                stack.push(t);
                stack.push(u);
            }
        }
    }

    fn add(&mut self, p: Point) -> u32 {
        self.pts.push(p);
        (self.pts.len() - 1) as u32
    }

    /// A vertex at `point` inside triangle `t`; the triangles changed.
    fn split_tri(&mut self, t: u32, point: Point) -> Vec<u32> {
        let Tri { v: [a, b, c], n: [na, nb, nc] } = self.tri(t);
        let p = self.add(point);
        let (t1, t2) = (self.tris.len() as u32, self.tris.len() as u32 + 1);
        self.set(t, Tri { v: [p, b, c], n: [na, t1, t2] });
        self.tris.push(Tri { v: [p, c, a], n: [nb, t2, t] });
        self.tris.push(Tri { v: [p, a, b], n: [nc, t, t1] });
        self.relink(nb, t, t1);
        self.relink(nc, t, t2);
        let mut touched = Vec::new();
        self.legalize(vec![t, t1, t2], &mut touched);
        touched
    }

    /// A vertex at `point` on the edge of triangle `t` across from its corner `i` (and of the
    /// triangle beyond, if any); the triangles changed.
    fn split_edge(&mut self, t: u32, i: usize, point: Point) -> Vec<u32> {
        let tri = self.tri(t);
        let (a, b, c) = (of(tri.v, i), of(tri.v, i + 1), of(tri.v, i + 2));
        let (u, nb, nc) = (of(tri.n, i), of(tri.n, i + 1), of(tri.n, i + 2));
        let beyond = self.tri(u);
        let j = slot(beyond.n, t);
        let p = self.add(point);
        let t2 = self.tris.len() as u32;
        let mut touched = Vec::new();
        match j {
            None => {
                self.set(t, Tri { v: [p, a, b], n: [nc, NONE, t2] });
                self.tris.push(Tri { v: [p, c, a], n: [nb, t, NONE] });
                self.relink(nb, t, t2);
                self.legalize(vec![t, t2], &mut touched);
            }
            Some(j) => {
                let (d, n_bd, n_dc) = (of(beyond.v, j), of(beyond.n, j + 1), of(beyond.n, j + 2));
                let u2 = t2 + 1;
                self.set(t, Tri { v: [p, a, b], n: [nc, u2, t2] });
                self.tris.push(Tri { v: [p, c, a], n: [nb, t, u] });
                self.set(u, Tri { v: [p, d, c], n: [n_dc, t2, u2] });
                self.tris.push(Tri { v: [p, b, d], n: [n_bd, u, t] });
                self.relink(nb, t, t2);
                self.relink(n_bd, u, u2);
                self.legalize(vec![t, t2, u, u2], &mut touched);
            }
        }
        touched
    }

    /// Walks from triangle `t` toward `p`.
    fn walk(&self, mut t: u32, p: Point, near: f64) -> Option<Walk> {
        for step in 0..self.tris.len() + 16 {
            let tri = self.tri(t);
            let mut on = None;
            let mut out = None;
            for k in 0..3 {
                let i = (k + step) % 3;
                let (b, c) = self.edge(t, i);
                let side = orient(b, c, p) / dist(b, c).max(1e-12);
                if side < -near {
                    out = Some(i);
                    break;
                }
                if side <= near {
                    on = Some(i);
                }
            }
            match out {
                None => return Some(Walk::In(t, on)),
                Some(i) if of(tri.n, i) == NONE => return Some(Walk::Out(t, i)),
                Some(i) => t = of(tri.n, i),
            }
        }
        None
    }

    /// The edge of `t` along the outline (across from which corner) whose diametral circle
    /// holds the corner across from it, if any at least `least` long.
    fn encroached(&self, t: u32, least: f64) -> Option<usize> {
        let tri = self.tri(t);
        (0..3).find(|&k| {
            let (a, (b, c)) = (self.pt(of(tri.v, k)), self.edge(t, k));
            of(tri.n, k) == NONE
                && (b[0] - a[0]) * (c[0] - a[0]) + (b[1] - a[1]) * (c[1] - a[1]) < 0.0
                && dist(b, c) >= least
        })
    }

    /// Ruppert's refinement: until no triangle's circumradius is over `MOST_RADIUS` of the
    /// spacing or `MOST_RATIO` of its shortest edge (or there's no fixing it, the outline too
    /// fine there), and no stretch of outline is crowded.
    fn refine(&mut self, spacing: f64) {
        let most = spacing * MOST_RADIUS;
        let least = spacing * LEAST_SPLIT;
        let near = spacing * 1e-6;
        // no more vertices than a mesh several times as fine would have
        let area = ring_area(&self.pts) * 0.5;
        let perimeter: f64 =
            (0..self.pts.len()).map(|i| dist(at(&self.pts, i), at(&self.pts, (i + 1) % self.pts.len()))).sum();
        let cap = self.pts.len() + (8.0 * area / (spacing * spacing) + 8.0 * perimeter / spacing) as usize + 64;
        let mut queue: VecDeque<u32> = (0..self.tris.len() as u32).collect();
        let mut queued = vec![true; self.tris.len()];
        while let Some(t) = queue.pop_front() {
            put(&mut queued, t as usize, false);
            if self.pts.len() >= cap {
                break;
            }
            let touched = if let Some(i) = self.encroached(t, least) {
                let (b, c) = self.edge(t, i);
                self.split_edge(t, i, mid(b, c))
            } else {
                let [a, b, c] = self.tri(t).v.map(|v| self.pt(v));
                let shortest = dist(a, b).min(dist(b, c)).min(dist(c, a));
                match circumcircle(a, b, c) {
                    Some((centre, r)) if r > most || r > MOST_RATIO * shortest => {
                        match self.settle(t, centre, least, near) {
                            Some(touched) => touched,
                            None => continue,
                        }
                    }
                    _ => continue,
                }
            };
            queued.resize(self.tris.len(), false);
            for x in touched.into_iter().chain([t]) {
                if !at(&queued, x as usize) {
                    put(&mut queued, x as usize, true);
                    queue.push_back(x);
                }
            }
        }
    }

    /// A vertex for bad triangle `t` at its circumcentre `centre`, or the stretch of outline it
    /// would crowd halved instead; none when that stretch is too short to halve.
    fn settle(&mut self, t: u32, centre: Point, least: f64, near: f64) -> Option<Vec<u32>> {
        let halve = |mesh: &mut Triangulation, x: u32, k: usize| {
            let (b, c) = mesh.edge(x, k);
            (dist(b, c) >= least).then(|| mesh.split_edge(x, k, mid(b, c)))
        };
        match self.walk(t, centre, near)? {
            Walk::Out(x, k) => halve(self, x, k),
            Walk::In(x, on) => {
                // an outline edge about it whose diametral circle it would fall in
                let tri = self.tri(x);
                for y in [x, of(tri.n, 0), of(tri.n, 1), of(tri.n, 2)].into_iter().filter(|&y| y != NONE) {
                    let near_tri = self.tri(y);
                    for k in (0..3).filter(|&k| of(near_tri.n, k) == NONE) {
                        let (b, c) = self.edge(y, k);
                        if (b[0] - centre[0]) * (c[0] - centre[0]) + (b[1] - centre[1]) * (c[1] - centre[1]) < 0.0 {
                            return halve(self, y, k);
                        }
                    }
                }
                if tri.v.iter().any(|&v| dist(self.pt(v), centre) <= near) {
                    return None;
                }
                Some(match on {
                    Some(k) => {
                        // onto the edge exactly
                        let (b, c) = self.edge(x, k);
                        let (dx, dy) = (c[0] - b[0], c[1] - b[1]);
                        let s = (((centre[0] - b[0]) * dx + (centre[1] - b[1]) * dy) / (dx * dx + dy * dy).max(1e-24))
                            .clamp(0.0, 1.0);
                        self.split_edge(x, k, [b[0] + s * dx, b[1] + s * dy])
                    }
                    None => self.split_tri(x, centre),
                })
            }
        }
    }

    /// The vertices on triangles, numbered from the ends of the piece outward (reverse
    /// Cuthill–McKee).
    fn cuthill_mckee(&self) -> Vec<u32> {
        let n = self.pts.len();
        let mut near: Vec<Vec<u32>> = vec![Vec::new(); n];
        for tri in &self.tris {
            for k in 0..3 {
                let (a, b) = (of(tri.v, k), of(tri.v, k + 1));
                if let Some(list) = near.get_mut(a as usize) {
                    list.push(b);
                }
                if let Some(list) = near.get_mut(b as usize) {
                    list.push(a);
                }
            }
        }
        for list in &mut near {
            list.sort_unstable();
            list.dedup();
        }
        let degree = |v: u32| near.get(v as usize).map_or(0, Vec::len);
        // breadth first from `start`, fewest neighbours first: the order, all reached marked
        let breadth = |start: usize, seen: &mut Vec<bool>| {
            let mut order = vec![start as u32];
            put(seen, start, true);
            let mut k = 0;
            while let Some(&v) = order.get(k) {
                k += 1;
                let mut fresh: Vec<u32> =
                    near.get(v as usize).into_iter().flatten().copied().filter(|&w| !at(seen, w as usize)).collect();
                fresh.sort_by_key(|&w| degree(w));
                for w in fresh {
                    put(seen, w as usize, true);
                    order.push(w);
                }
            }
            order
        };
        let mut order: Vec<u32> = Vec::with_capacity(n);
        let mut seen = vec![false; n];
        for start in 0..n {
            if at(&seen, start) || degree(start as u32) == 0 {
                continue;
            }
            // from as far as can be from where it starts: an end of the piece
            let far = breadth(start, &mut vec![false; n]).last().copied().unwrap_or(start as u32) as usize;
            order.extend(breadth(far, &mut seen).into_iter().rev());
        }
        order
    }

    /// The vertices on triangles in order along the piece's longest axis, or (`across`) the axis
    /// square to it.
    fn swept(&self, across: bool) -> Vec<u32> {
        let mut used = vec![false; self.pts.len()];
        for tri in &self.tris {
            for v in tri.v {
                put(&mut used, v as usize, true);
            }
        }
        let on: Vec<u32> = (0..self.pts.len() as u32).filter(|&v| at(&used, v as usize)).collect();
        // the principal axis of the vertices
        let count = on.len().max(1) as f64;
        let (mx, my) = on.iter().fold((0.0, 0.0), |(x, y), &v| (x + self.pt(v)[0] / count, y + self.pt(v)[1] / count));
        let (mut xx, mut xy, mut yy) = (0.0, 0.0, 0.0);
        for &v in &on {
            let (dx, dy) = (self.pt(v)[0] - mx, self.pt(v)[1] - my);
            (xx, xy, yy) = (xx + dx * dx, xy + dx * dy, yy + dy * dy);
        }
        let angle = 0.5 * (2.0 * xy).atan2(xx - yy) + if across { std::f64::consts::FRAC_PI_2 } else { 0.0 };
        let (c, s) = (angle.cos(), angle.sin());
        let mut order = on;
        order.sort_by(|&a, &b| {
            let (p, q) = (self.pt(a), self.pt(b));
            (p[0] * c + p[1] * s).total_cmp(&(q[0] * c + q[1] * s))
        });
        order
    }

    /// What factoring the solver's matrices costs with the vertices in `order`: the sum, over
    /// the vertices, of the square of how far each is numbered from its lowest numbered
    /// neighbour (the length of its rows).
    fn cost(&self, order: &[u32]) -> u64 {
        let mut number = vec![NONE; self.pts.len()];
        for (k, &v) in order.iter().enumerate() {
            put(&mut number, v as usize, k as u32);
        }
        let mut lowest = number.clone();
        for t in &self.tris {
            let k = t.v.map(|v| at(&number, v as usize));
            let least = k.iter().min().copied().unwrap_or(0);
            for v in t.v {
                if let Some(l) = lowest.get_mut(v as usize) {
                    *l = (*l).min(least);
                }
            }
        }
        number.iter().zip(&lowest).filter(|(n, _)| **n != NONE).map(|(&n, &l)| u64::from(n - l).pow(2)).sum()
    }

    /// As a piece (offset by `corner`), its vertices numbered so each triangle's are close
    /// together (what the solver's matrices cost to factor): by reverse Cuthill–McKee, or swept
    /// along the piece or across it, whichever costs least. Vertices on no triangle are left
    /// out.
    fn piece(&self, corner: Point) -> Piece {
        let candidates = [self.cuthill_mckee(), self.swept(false), self.swept(true)];
        let order = candidates.into_iter().min_by_key(|order| self.cost(order)).unwrap_or_default();
        let n = self.pts.len();
        let mut number = vec![NONE; n];
        for (k, &v) in order.iter().enumerate() {
            put(&mut number, v as usize, k as u32);
        }
        let verts = order
            .iter()
            .map(|&v| {
                let p = self.pt(v);
                Vec2::new((p[0] + corner[0]) as f32, (p[1] + corner[1]) as f32)
            })
            .collect();
        let tris = self.tris.iter().map(|t| t.v.map(|v| at(&number, v as usize))).collect();
        Piece { verts, tris }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(mesh: &Triangulation) -> f64 {
        mesh.tris.iter().map(|t| orient(mesh.pt(t.v[0]), mesh.pt(t.v[1]), mesh.pt(t.v[2])) * 0.5).sum()
    }

    /// Every neighbour link goes both ways and shares the edge.
    fn links_hold(mesh: &Triangulation) {
        for (t, tri) in mesh.tris.iter().enumerate() {
            for k in 0..3 {
                let u = of(tri.n, k);
                if u == NONE {
                    continue;
                }
                let j = slot(mesh.tri(u).n, t as u32).unwrap();
                let (b, c) = (of(tri.v, k + 1), of(tri.v, k + 2));
                let (e, f) = (of(mesh.tri(u).v, j + 1), of(mesh.tri(u).v, j + 2));
                assert_eq!((b, c), (f, e), "triangle {t} edge {k}");
            }
        }
    }

    /// An L, a notch cut in its top.
    fn notched() -> Vec<Point> {
        vec![
            [0.0, 0.0],
            [100.0, 0.0],
            [100.0, 40.0],
            [40.0, 40.0],
            [40.0, 100.0],
            [25.0, 100.0],
            [20.0, 60.0],
            [15.0, 100.0],
            [0.0, 100.0],
        ]
    }

    #[test]
    fn a_polygon_is_cut_into_triangles_that_fill_it_exactly() {
        // y down, so anticlockwise as the numbers go is clockwise on screen: the ring as given
        let ring = notched();
        assert!(ring_area(&ring) > 0.0);
        let mut mesh = Triangulation::of_polygon(&ring).unwrap();
        assert_eq!(mesh.tris.len(), ring.len() - 2);
        assert!((area(&mesh) - ring_area(&ring) * 0.5).abs() < 1e-6);
        links_hold(&mesh);
        mesh.refine(10.0);
        links_hold(&mesh);
        assert!((area(&mesh) - ring_area(&ring) * 0.5).abs() < 1e-6, "{}", area(&mesh));
        for t in &mesh.tris {
            let [a, b, c] = t.v.map(|v| mesh.pt(v));
            assert!(orient(a, b, c) > 0.0);
            let (_, r) = circumcircle(a, b, c).unwrap();
            assert!(r <= 10.0 * MOST_RADIUS * 1.0001, "{r}");
        }
        // the notch's sides are edges still
        let piece = mesh.piece([0.0, 0.0]);
        assert_eq!(piece.verts.len(), mesh.pts.len());
    }

    #[test]
    fn the_distance_transform_measures_from_the_nearest_zero() {
        let (cols, rows) = (9, 7);
        let mut d2 = vec![FAR; cols * rows];
        d2[3 * cols + 2] = 0.0;
        d2[cols + 7] = 0.0;
        distance_transform(&mut d2, cols, rows);
        for y in 0..rows {
            for x in 0..cols {
                let want = [(2.0, 3.0), (7.0, 1.0)]
                    .iter()
                    .map(|&(px, py): &(f64, f64)| (x as f64 - px).powi(2) + (y as f64 - py).powi(2))
                    .fold(f64::MAX, f64::min);
                assert!((d2[y * cols + x] - want).abs() < 1e-6, "{x} {y}");
            }
        }
    }

    #[test]
    fn simplifying_keeps_the_ring_from_crossing_itself() {
        // a U, its slot 3 wide, every pixel along it a point: simplified loosely enough, a stretch
        // down one arm would cut across the slot into the other
        let corners: [Point; 8] =
            [[0.0, 0.0], [40.0, 0.0], [40.0, 60.0], [21.5, 60.0], [21.5, 6.0], [18.5, 6.0], [18.5, 60.0], [0.0, 60.0]];
        let mut ring = Vec::new();
        for k in 0..corners.len() {
            let (a, b) = (corners[k], corners[(k + 1) % corners.len()]);
            let n = dist(a, b).ceil() as usize;
            for s in 0..n {
                let t = s as f64 / n as f64;
                // a little wobble, as a traced outline has
                let wobble = ((ring.len() as f64) * 1.7).sin() * 0.3;
                ring.push([a[0] + (b[0] - a[0]) * t + wobble, a[1] + (b[1] - a[1]) * t]);
            }
        }
        for tolerance in [0.5, 2.0, 5.0, 12.0] {
            let simple = simplified(&ring, tolerance);
            assert!(simple.len() >= 3 && simple.len() < ring.len(), "{tolerance}");
            assert!(!self_crossings(&simple).contains(&true), "{tolerance}");
            assert!(Triangulation::of_polygon(&simple).is_some(), "{tolerance}");
        }
    }
}

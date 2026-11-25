//! A puppet warp: a mesh of triangles laid over what is painted, bent by pins.
//!
//! The mesh is laid over the painted pixels (and as far past them as the expansion reaches, with
//! any hollow they close filled) following their outline, in triangles about its spacing on a
//! side and smaller where the outline turns tightly (see [`crate::mesh`]). Each piece apart from
//! the rest is an island; islands bend on their own.
//!
//! Pins hold points of the mesh where they are put. Between them the mesh bends as rigidly as it
//! can, by the two steps of Igarashi, Moscovich and Hughes, "As-rigid-as-possible shape
//! manipulation" (2005): first the shape that keeps each triangle most nearly similar to itself,
//! then each triangle turned (not scaled) to fit that and the mesh fitted to them. A pin given an
//! angle holds the triangles about it at that turn; one left free turns with the bend.
//!
//! Everything here is in source coordinates: canvas pixels where the content was when the
//! transform began. The app keeps the mesh too, to find points on it and to draw it bent.

use crate::geom::Vec2;
use crate::mesh;

/// How the mesh is laid.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshSettings {
    /// How far past the painted pixels the mesh reaches (canvas pixels): more joins nearby
    /// pieces into one and covers thin strands.
    pub expansion: f32,
    /// About how many triangles across the longer side of what is painted: more bends more
    /// smoothly.
    pub density: u32,
}

impl Default for MeshSettings {
    fn default() -> MeshSettings {
        MeshSettings { expansion: 4.0, density: 40 }
    }
}

/// The densities taken, fewest triangles across to most.
pub const DENSITY_RANGE: (u32, u32) = (12, 80);
/// The furthest the mesh reaches past the painted pixels (canvas pixels).
pub const EXPANSION_MAX: f32 = 64.0;

/// How hard a pin holds its point, and a pin's angle the triangles about it, against how hard
/// each triangle keeps its shape.
const PIN_WEIGHT: f64 = 1.0e4;
const TURN_WEIGHT: f64 = 10.0;
/// How far about a pin (in spacings, about how wide a triangle is) its triangles turn with it.
const PIN_REACH: f32 = 1.6;

/// The three edges of a triangle by its corners: each edge's two ends, then the corner across.
const EDGES: [(usize, usize, usize); 3] = [(0, 1, 2), (1, 2, 0), (2, 0, 1)];

fn vert(verts: &[Vec2], v: u32) -> Vec2 {
    verts.get(v as usize).copied().unwrap_or(Vec2::ZERO)
}

fn pick<T: Copy + Default>(of: &[T; 3], k: usize) -> T {
    of.get(k).copied().unwrap_or_default()
}

fn bump(b: &mut [f64], i: usize, v: f64) {
    if let Some(x) = b.get_mut(i) {
        *x += v;
    }
}

#[derive(Clone, Copy, Debug)]
struct Island {
    /// Its vertices and triangles: each island's are together, its vertices numbered so each
    /// triangle's are close.
    verts: (u32, u32),
    tris: (u32, u32),
}

/// The mesh at rest.
#[derive(Clone, Debug)]
pub struct PuppetMesh {
    /// About how wide the triangles were laid (source pixels).
    spacing: f32,
    rest: Vec<Vec2>,
    tris: Vec<[u32; 3]>,
    islands: Vec<Island>,
}

/// What is painted, as the app hands it over: `cols` × `rows` cells (one byte each, not 0 where
/// any pixel in it is painted), each `scale` canvas pixels on a side, the first cell's top left at
/// source point `origin`.
#[derive(Clone, Copy, Debug)]
pub struct Painted<'a> {
    pub cells: &'a [u8],
    pub cols: usize,
    pub rows: usize,
    pub scale: f32,
    pub origin: Vec2,
}

impl PuppetMesh {
    /// The mesh over every painted cell of `painted`; none when none is.
    pub fn build(painted: Painted, settings: MeshSettings) -> Option<PuppetMesh> {
        let Painted { cells, cols, rows, scale, origin } = painted;
        if cols == 0 || cells.len() < cols * rows || !(scale.is_finite() && scale > 0.0) || !origin.is_finite() {
            return None;
        }
        let cells = cells.get(..cols * rows)?;
        let bounds = mesh::painted(cells, cols)?;
        let [x0, y0, x1, y1] = bounds;
        let expansion = if settings.expansion.is_finite() { settings.expansion.clamp(0.0, EXPANSION_MAX) } else { 0.0 };
        let density = settings.density.clamp(DENSITY_RANGE.0, DENSITY_RANGE.1) as f32;
        let side = (x1 - x0).max(y1 - y0) as f32 * scale + 2.0 * expansion;
        let spacing = (side / density).max(2.0);
        let mut mesh = PuppetMesh { spacing, rest: Vec::new(), tris: Vec::new(), islands: Vec::new() };
        // laid in cells, then carried to source pixels
        for piece in mesh::lay(cells, cols, bounds, Vec2::ZERO, expansion / scale, spacing / scale) {
            let (first_vert, first_tri) = (mesh.rest.len() as u32, mesh.tris.len() as u32);
            mesh.rest.extend(piece.verts.into_iter().map(|v| origin + v * scale));
            mesh.tris.extend(piece.tris.iter().map(|t| t.map(|v| v + first_vert)));
            mesh.islands.push(Island {
                verts: (first_vert, mesh.rest.len() as u32),
                tris: (first_tri, mesh.tris.len() as u32),
            });
        }
        (!mesh.tris.is_empty()).then_some(mesh)
    }

    /// The vertices at rest.
    pub fn rest(&self) -> &[Vec2] {
        &self.rest
    }

    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.tris
    }

    /// About how wide the triangles were laid (source pixels).
    pub fn spacing(&self) -> f32 {
        self.spacing
    }

    #[cfg(test)]
    pub fn island_count(&self) -> usize {
        self.islands.len()
    }

    fn tri(&self, t: u32) -> [u32; 3] {
        self.tris.get(t as usize).copied().unwrap_or([0; 3])
    }

    fn corners(&self, verts: &[Vec2], t: u32) -> [Vec2; 3] {
        self.tri(t).map(|v| vert(verts, v))
    }

    #[cfg(test)]
    /// The triangle source point `p` lies in and its weights there; none off the mesh. Where
    /// islands overlap, the first. (The app finds points on the mesh itself; this is for tests.)
    pub fn locate(&self, p: Vec2) -> Option<(u32, [f32; 3])> {
        if !p.is_finite() {
            return None;
        }
        (0..self.tris.len() as u32).find_map(|t| {
            let w = self.weights(t, p);
            w.iter().all(|&w| w >= -1e-4).then_some((t, w))
        })
    }

    #[cfg(test)]
    /// The weights of triangle `tri`'s corners that make point `p`.
    fn weights(&self, tri: u32, p: Vec2) -> [f32; 3] {
        let [a, b, c] = self.corners(&self.rest, tri);
        let d = (b - a).cross(c - a);
        if d.abs() < 1e-12 {
            return [1.0, 0.0, 0.0];
        }
        let wb = (p - a).cross(c - a) / d;
        let wc = (b - a).cross(p - a) / d;
        [1.0 - wb - wc, wb, wc]
    }

    /// The point weights `w` make of triangle `tri` with its vertices at `verts`.
    pub fn at(&self, verts: &[Vec2], tri: u32, w: [f32; 3]) -> Vec2 {
        let [a, b, c] = self.corners(verts, tri);
        let [wa, wb, wc] = w;
        a * wa + b * wb + c * wc
    }

    fn island_of_tri(&self, tri: u32) -> Option<usize> {
        self.islands.iter().position(|i| (i.tris.0..i.tris.1).contains(&tri))
    }

    /// Whether triangle `tri` is on the mesh.
    pub fn has_triangle(&self, tri: u32) -> bool {
        (tri as usize) < self.tris.len()
    }
}

/// A pin: where on the mesh it is (a triangle and the weights of its corners), where that point
/// has been taken (source pixels), and the turn it holds the mesh about it at (radians,
/// clockwise on screen); none to let the mesh turn there as it bends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pin {
    pub tri: u32,
    pub weights: [f32; 3],
    pub at: Vec2,
    pub angle: Option<f32>,
}

impl Pin {
    #[cfg(test)]
    /// A pin at source point `p` on `mesh`, where it is; none off the mesh.
    pub fn new(mesh: &PuppetMesh, p: Vec2, angle: Option<f32>) -> Option<Pin> {
        let (tri, weights) = mesh.locate(p)?;
        Some(Pin { tri, weights, at: mesh.at(&mesh.rest, tri, weights), angle })
    }

    /// Where it is on the mesh at rest.
    pub fn rest(&self, mesh: &PuppetMesh) -> Vec2 {
        mesh.at(&mesh.rest, self.tri, self.weights)
    }
}

/// The mesh bent by its pins: its vertices, and the turn about each pin.
#[derive(Clone, Debug, PartialEq)]
pub struct Bent {
    pub verts: Vec<Vec2>,
    pub angles: Vec<f32>,
}

/// What bends the mesh for a set of pins: each island's two systems, made ready (factored) once
/// for where the pins are on the mesh and which hold an angle, then solved for wherever they
/// are taken.
#[derive(Debug)]
pub struct Solver {
    /// Which pins this was made for: their triangles, weights and whether each holds an angle.
    key: Vec<(u32, [u32; 3], bool)>,
    islands: Vec<Option<System>>,
}

#[derive(Debug)]
struct System {
    /// The pins on the island (indices into the pins).
    pins: Vec<usize>,
    /// For each of the island's triangles, the pin whose turn it takes, if any.
    held_by: Vec<Option<usize>>,
    similar: Band,
    fitted: Band,
}

fn key_of(pins: &[Pin]) -> Vec<(u32, [u32; 3], bool)> {
    pins.iter().map(|p| (p.tri, p.weights.map(f32::to_bits), p.angle.is_some())).collect()
}

/// The pins (indices) on each island.
fn pins_by_island(mesh: &PuppetMesh, pins: &[Pin]) -> Vec<Vec<usize>> {
    let mut on = vec![Vec::new(); mesh.islands.len()];
    for (k, pin) in pins.iter().enumerate() {
        if let Some(list) = mesh.island_of_tri(pin.tri).and_then(|i| on.get_mut(i)) {
            list.push(k);
        }
    }
    on
}

impl Solver {
    /// Made ready for `pins` on `mesh`.
    pub fn new(mesh: &PuppetMesh, pins: &[Pin]) -> Solver {
        let islands = mesh
            .islands
            .iter()
            .zip(pins_by_island(mesh, pins))
            .map(|(island, on)| if on.len() >= 2 { System::new(mesh, *island, pins, on) } else { None })
            .collect();
        Solver { key: key_of(pins), islands }
    }

    /// Whether this was made for pins where `pins` are on the mesh, holding angles alike.
    pub fn fits(&self, pins: &[Pin]) -> bool {
        self.key == key_of(pins)
    }

    /// The mesh bent by `pins` (those this was made for, wherever they now are).
    pub fn solve(&self, mesh: &PuppetMesh, pins: &[Pin]) -> Bent {
        let mut verts = mesh.rest.clone();
        let mut angles: Vec<f32> = pins.iter().map(|p| p.angle.unwrap_or(0.0)).collect();
        for ((island, on), system) in mesh.islands.iter().zip(pins_by_island(mesh, pins)).zip(&self.islands) {
            match (on.as_slice(), system) {
                // no pin: where it was
                ([], _) => {}
                // one: carried whole, turned about it as it holds
                ([only], _) => {
                    let Some(pin) = pins.get(*only) else { continue };
                    let (pivot, turn) = (pin.rest(mesh), pin.angle.unwrap_or(0.0));
                    let (lo, hi) = (island.verts.0 as usize, island.verts.1 as usize);
                    for (v, rest) in verts.iter_mut().zip(&mesh.rest).take(hi).skip(lo) {
                        *v = pin.at + rotate(*rest - pivot, turn);
                    }
                }
                (_, Some(system)) => system.solve(mesh, *island, pins, &mut verts, &mut angles),
                _ => {}
            }
        }
        Bent { verts, angles }
    }
}

impl System {
    fn new(mesh: &PuppetMesh, island: Island, pins: &[Pin], on: Vec<usize>) -> Option<System> {
        let first = island.verts.0;
        let m = (island.verts.1 - first) as usize;
        let tris = mesh.tris.get(island.tris.0 as usize..island.tris.1 as usize)?;
        let local = |v: u32| v.saturating_sub(first) as usize;
        let pin = |p: usize| pins.get(p).copied();
        // which pin's turn each triangle takes: the nearest within reach
        let reach = PIN_REACH * mesh.spacing;
        let held_by: Vec<Option<usize>> = tris
            .iter()
            .map(|t| {
                let [a, b, c] = t.map(|v| vert(&mesh.rest, v));
                let centre = (a + b + c) * (1.0 / 3.0);
                on.iter()
                    .filter_map(|&p| Some((p, pin(p)?.rest(mesh).distance(centre))))
                    .filter(|&(_, d)| d <= reach)
                    .min_by(|a, b| a.1.total_cmp(&b.1))
                    .map(|(p, _)| p)
            })
            .collect();
        // each vertex's lowest numbered neighbour: where its rows of the matrices begin
        let mut lowest: Vec<usize> = (0..m).collect();
        for t in tris {
            let ids = t.map(local);
            let least = ids.iter().min().copied().unwrap_or(0);
            for v in ids {
                if let Some(l) = lowest.get_mut(v) {
                    *l = (*l).min(least);
                }
            }
        }
        let mut similar = Band::new(lowest.iter().flat_map(|&l| [2 * l, 2 * l]).collect());
        let mut fitted = Band::new(lowest);
        for (t, holder) in tris.iter().zip(&held_by) {
            let p = t.map(|v| vert(&mesh.rest, v));
            let ids = t.map(local);
            let holds_turn = holder.and_then(pin).is_some_and(|p| p.angle.is_some());
            for (i, j, o) in EDGES {
                let (pi, pj, po) = (pick(&p, i), pick(&p, j), pick(&p, o));
                let (vi, vj, vo) = (pick(&ids, i), pick(&ids, j), pick(&ids, o));
                // the third corner in terms of the edge: o = i + x (j - i) + y R(j - i)
                let d = pj - pi;
                let len = d.dot(d);
                if len < 1e-12 {
                    continue;
                }
                let r = po - pi;
                let (x, y) = (f64::from(r.dot(d) / len), f64::from(r.dot(perp(d)) / len));
                // its residual across and down, over (vi.x, vi.y, vj.x, vj.y, vo.x, vo.y)
                let across = [(2 * vi, x - 1.0), (2 * vi + 1, -y), (2 * vj, -x), (2 * vj + 1, y), (2 * vo, 1.0)];
                let down = [(2 * vi, y), (2 * vi + 1, x - 1.0), (2 * vj, -y), (2 * vj + 1, -x), (2 * vo + 1, 1.0)];
                similar.add_row(&across, 1.0);
                similar.add_row(&down, 1.0);
                fitted.add_row(&[(vi, -1.0), (vj, 1.0)], 1.0);
                // a pin holding an angle holds its triangles' edges turned so
                if holds_turn {
                    similar.add_row(&[(2 * vi, -1.0), (2 * vj, 1.0)], TURN_WEIGHT);
                    similar.add_row(&[(2 * vi + 1, -1.0), (2 * vj + 1, 1.0)], TURN_WEIGHT);
                }
            }
        }
        for p in on.iter().filter_map(|&p| pin(p)) {
            let ids = mesh.tri(p.tri).map(local);
            let w = p.weights.map(f64::from);
            for axis in 0..2 {
                let row: Vec<(usize, f64)> = ids.iter().zip(w).map(|(&v, w)| (2 * v + axis, w)).collect();
                similar.add_row(&row, PIN_WEIGHT);
            }
            let row: Vec<(usize, f64)> = ids.iter().zip(w).map(|(&v, w)| (v, w)).collect();
            fitted.add_row(&row, PIN_WEIGHT);
        }
        (similar.factor() && fitted.factor()).then_some(System { pins: on, held_by, similar, fitted })
    }

    fn solve(&self, mesh: &PuppetMesh, island: Island, pins: &[Pin], verts: &mut [Vec2], angles: &mut [f32]) {
        let first = island.verts.0;
        let m = (island.verts.1 - first) as usize;
        let Some(tris) = mesh.tris.get(island.tris.0 as usize..island.tris.1 as usize) else { return };
        let local = |v: u32| v.saturating_sub(first) as usize;
        let turn_of = |holder: &Option<usize>| holder.and_then(|h| pins.get(h)).and_then(|p| p.angle);
        let held: Vec<Pin> = self.pins.iter().filter_map(|&p| pins.get(p).copied()).collect();
        // step one: as near similar as can be
        let mut b = vec![0.0f64; 2 * m];
        for pin in &held {
            for (&v, w) in mesh.tri(pin.tri).map(local).iter().zip(pin.weights) {
                let w = PIN_WEIGHT * f64::from(w);
                bump(&mut b, 2 * v, w * f64::from(pin.at.x));
                bump(&mut b, 2 * v + 1, w * f64::from(pin.at.y));
            }
        }
        for (t, holder) in tris.iter().zip(&self.held_by) {
            let Some(turn) = turn_of(holder) else { continue };
            let p = t.map(|v| vert(&mesh.rest, v));
            let ids = t.map(local);
            for (i, j, _) in EDGES {
                let rest = pick(&p, j) - pick(&p, i);
                if rest.dot(rest) < 1e-12 {
                    continue;
                }
                let d = rotate(rest, turn);
                for (axis, v) in [(0, d.x), (1, d.y)] {
                    bump(&mut b, 2 * pick(&ids, j) + axis, TURN_WEIGHT * f64::from(v));
                    bump(&mut b, 2 * pick(&ids, i) + axis, -TURN_WEIGHT * f64::from(v));
                }
            }
        }
        self.similar.solve(&mut b);
        let similar = |v: usize| {
            Vec2::new(b.get(2 * v).copied().unwrap_or(0.0) as f32, b.get(2 * v + 1).copied().unwrap_or(0.0) as f32)
        };
        // step two: each triangle turned to fit that, unscaled, and the mesh fitted to them
        let (mut bx, mut by) = (vec![0.0f64; m], vec![0.0f64; m]);
        let mut turns = vec![(0.0f32, 0.0f32); pins.len()];
        for (t, holder) in tris.iter().zip(&self.held_by) {
            let p = t.map(|v| vert(&mesh.rest, v));
            let ids = t.map(local);
            let fit = fit_turn(p, ids.map(similar));
            if let Some(sum) = holder.and_then(|h| turns.get_mut(h)) {
                sum.0 += fit.sin();
                sum.1 += fit.cos();
            }
            let turn = turn_of(holder).unwrap_or(fit);
            for (i, j, _) in EDGES {
                let rest = pick(&p, j) - pick(&p, i);
                if rest.dot(rest) < 1e-12 {
                    continue;
                }
                let d = rotate(rest, turn);
                let (vi, vj) = (pick(&ids, i), pick(&ids, j));
                bump(&mut bx, vj, f64::from(d.x));
                bump(&mut bx, vi, -f64::from(d.x));
                bump(&mut by, vj, f64::from(d.y));
                bump(&mut by, vi, -f64::from(d.y));
            }
        }
        for pin in &held {
            for (&v, w) in mesh.tri(pin.tri).map(local).iter().zip(pin.weights) {
                let w = PIN_WEIGHT * f64::from(w);
                bump(&mut bx, v, w * f64::from(pin.at.x));
                bump(&mut by, v, w * f64::from(pin.at.y));
            }
        }
        // a free pin's angle is the turn of the triangles about it
        for &p in &self.pins {
            if let (Some(pin), Some(&(s, c)), Some(angle)) = (pins.get(p), turns.get(p), angles.get_mut(p)) {
                *angle = pin.angle.unwrap_or(if s == 0.0 && c == 0.0 { 0.0 } else { s.atan2(c) });
            }
        }
        self.fitted.solve(&mut bx);
        self.fitted.solve(&mut by);
        let out = verts.iter_mut().skip(first as usize).take(m);
        for (v, (x, y)) in out.zip(bx.iter().zip(&by)) {
            let p = Vec2::new(*x as f32, *y as f32);
            if p.is_finite() {
                *v = p;
            }
        }
    }
}

/// The turn (radians, clockwise on screen) that best carries triangle `p` onto triangle `q`.
fn fit_turn(p: [Vec2; 3], q: [Vec2; 3]) -> f32 {
    let centre = |t: [Vec2; 3]| (t[0] + t[1] + t[2]) * (1.0 / 3.0);
    let (pc, qc) = (centre(p), centre(q));
    let (mut s, mut c) = (0.0, 0.0);
    for (a, b) in p.iter().zip(&q) {
        let (a, b) = (*a - pc, *b - qc);
        s += a.cross(b);
        c += a.dot(b);
    }
    if s == 0.0 && c == 0.0 { 0.0 } else { s.atan2(c) }
}

fn rotate(v: Vec2, radians: f32) -> Vec2 {
    let (sin, cos) = radians.sin_cos();
    Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
}

/// `v` turned a quarter (anticlockwise on screen).
fn perp(v: Vec2) -> Vec2 {
    Vec2::new(-v.y, v.x)
}

/// A symmetric matrix kept by its envelope (a skyline): each row from its first entry that may
/// not be 0 to the diagonal, lower half only; once factored, its Cholesky factor, which keeps to
/// the same envelope. Factoring costs about the sum of the squares of the rows' lengths.
#[derive(Debug)]
struct Band {
    /// Each row's first column.
    first: Vec<usize>,
    /// Where each row starts in `a` (and one past the last row's end).
    start: Vec<usize>,
    a: Vec<f64>,
    /// The number the app keeps its factor by, once the app has factored it (in a plugin).
    #[cfg_attr(not(target_arch = "wasm32"), allow(dead_code))]
    kept: Option<i32>,
}

/// The app's band utilities (see `crates/core/src/plugin_band.rs`): a hundred times as fast as
/// this code is interpreted, which the solve on every frame of a drag needs.
#[cfg(target_arch = "wasm32")]
mod app {
    #[link(wasm_import_module = "procraft")]
    unsafe extern "C" {
        pub fn band_factor(first: i32, n: i32, values: i32) -> i32;
        pub fn band_solve(band: i32, b: i32, n: i32) -> i32;
        pub fn band_drop(band: i32);
    }
}

impl Drop for Band {
    fn drop(&mut self) {
        #[cfg(target_arch = "wasm32")]
        if let Some(band) = self.kept {
            unsafe { app::band_drop(band) };
        }
    }
}

impl Band {
    /// All 0, row `i` kept from column `first[i]` (at most `i`).
    fn new(first: Vec<usize>) -> Band {
        let first: Vec<usize> = first.into_iter().enumerate().map(|(i, f)| f.min(i)).collect();
        let mut start = Vec::with_capacity(first.len() + 1);
        let mut at = 0;
        for (i, f) in first.iter().enumerate() {
            start.push(at);
            at += i - f + 1;
        }
        start.push(at);
        Band { first, start, a: vec![0.0; at], kept: None }
    }

    fn n(&self) -> usize {
        self.first.len()
    }

    fn first(&self, i: usize) -> usize {
        self.first.get(i).copied().unwrap_or(i)
    }

    /// Where entry (`i`, `j`) is kept, `first(i)` ≤ `j` ≤ `i`.
    fn index(&self, i: usize, j: usize) -> usize {
        self.start.get(i).copied().unwrap_or(0) + j - self.first(i)
    }

    fn at(&self, i: usize, j: usize) -> f64 {
        if j < self.first(i) {
            return 0.0;
        }
        self.a.get(self.index(i, j)).copied().unwrap_or(0.0)
    }

    fn set(&mut self, i: usize, j: usize, v: f64) {
        if j >= self.first(i) {
            let k = self.index(i, j);
            if let Some(x) = self.a.get_mut(k) {
                *x = v;
            }
        }
    }

    /// Adds `weight` × the row's outer product with itself: one term of a least-squares sum.
    fn add_row(&mut self, row: &[(usize, f64)], weight: f64) {
        for &(i, a) in row {
            for &(j, b) in row {
                if i >= j && j >= self.first(i) && i < self.n() {
                    let k = self.index(i, j);
                    if let Some(x) = self.a.get_mut(k) {
                        *x += weight * a * b;
                    }
                }
            }
        }
    }

    /// The sum of row `i` times row `j` (of the factor so far) over columns `from` to `to`.
    fn dot(&self, i: usize, j: usize, from: usize, to: usize) -> f64 {
        if from >= to {
            return 0.0;
        }
        let (ri, rj) = (self.index(i, from), self.index(j, from));
        match (self.a.get(ri..ri + (to - from)), self.a.get(rj..rj + (to - from))) {
            (Some(x), Some(y)) => x.iter().zip(y).map(|(a, b)| a * b).sum(),
            _ => 0.0,
        }
    }

    /// Factors it (L Lᵀ): by the app, in a plugin; false if it isn't positive definite.
    fn factor(&mut self) -> bool {
        #[cfg(target_arch = "wasm32")]
        {
            let first: Vec<u32> = self.first.iter().map(|&f| f as u32).collect();
            let n = first.len() as i32;
            let band = unsafe { app::band_factor(first.as_ptr() as i32, n, self.a.as_ptr() as i32) };
            self.kept = (band > 0).then_some(band);
            // the factor is the app's: the matrix here is needed no more
            self.a = Vec::new();
            self.kept.is_some()
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.factor_here()
    }

    /// Factors it in place (L Lᵀ); false if it isn't positive definite.
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    fn factor_here(&mut self) -> bool {
        for i in 0..self.n() {
            let fi = self.first(i);
            for j in fi..i {
                let from = fi.max(self.first(j));
                let s = self.at(i, j) - self.dot(i, j, from, j);
                let d = self.at(j, j);
                self.set(i, j, s / d);
            }
            let d = self.at(i, i) - self.dot(i, i, fi, i);
            if !(d > 1e-12 && d.is_finite()) {
                return false;
            }
            self.set(i, i, d.sqrt());
        }
        true
    }

    /// Solves (L Lᵀ) x = b in place, once factored.
    fn solve(&self, b: &mut [f64]) {
        #[cfg(target_arch = "wasm32")]
        if let Some(band) = self.kept {
            // not solved (the app let it go): no numbers, which the app takes as no bend
            if unsafe { app::band_solve(band, b.as_mut_ptr() as i32, b.len() as i32) } != 1 {
                b.fill(f64::NAN);
            }
            return;
        }
        self.solve_here(b);
    }

    /// Solves (L Lᵀ) x = b in place, once factored here.
    fn solve_here(&self, b: &mut [f64]) {
        let n = self.n().min(b.len());
        for i in 0..n {
            let fi = self.first(i);
            let s: f64 = (fi..i).map(|k| self.at(i, k) * b.get(k).copied().unwrap_or(0.0)).sum();
            if let Some(x) = b.get_mut(i) {
                *x = (*x - s) / self.at(i, i);
            }
        }
        for i in (0..n).rev() {
            let xi = b.get(i).copied().unwrap_or(0.0) / self.at(i, i);
            if let Some(x) = b.get_mut(i) {
                *x = xi;
            }
            for k in self.first(i)..i {
                let l = self.at(i, k);
                if let Some(x) = b.get_mut(k) {
                    *x -= l * xi;
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// The mesh over `alpha` (a byte a pixel, `width` across, its first pixel at `origin`).
    pub(crate) fn build(alpha: &[u8], width: usize, origin: Vec2, settings: MeshSettings) -> Option<PuppetMesh> {
        let painted = Painted { cells: alpha, cols: width, rows: alpha.len() / width.max(1), scale: 1.0, origin };
        PuppetMesh::build(painted, settings)
    }

    /// Each edge once, its two vertices.
    pub(crate) fn edges(mesh: &PuppetMesh) -> Vec<[u32; 2]> {
        let mut edges: Vec<[u32; 2]> = mesh
            .triangles()
            .iter()
            .flat_map(|&[a, b, c]| [[a, b], [b, c], [c, a]])
            .map(|[a, b]| [a.min(b), a.max(b)])
            .collect();
        edges.sort_unstable();
        edges.dedup();
        edges
    }

    /// A `w` × `h` alpha with the rectangles `rects` (x0, y0, x1, y1) painted.
    fn alpha(w: usize, h: usize, rects: &[[usize; 4]]) -> Vec<u8> {
        let mut a = vec![0u8; w * h];
        for &[x0, y0, x1, y1] in rects {
            for y in y0..y1 {
                for x in x0..x1 {
                    a[y * w + x] = 255;
                }
            }
        }
        a
    }

    fn settings(expansion: f32, density: u32) -> MeshSettings {
        MeshSettings { expansion, density }
    }

    fn close(a: Vec2, b: Vec2, tolerance: f32) -> bool {
        a.distance(b) <= tolerance
    }

    #[test]
    fn the_mesh_covers_every_painted_pixel() {
        let a = alpha(200, 120, &[[20, 30, 90, 40], [100, 10, 104, 110], [150, 60, 151, 61]]);
        let mesh = build(&a, 200, Vec2::new(5.0, 7.0), settings(0.0, 30)).unwrap();
        for y in 0..120 {
            for x in 0..200 {
                if a[y * 200 + x] != 0 {
                    let p = Vec2::new(5.0 + x as f32 + 0.5, 7.0 + y as f32 + 0.5);
                    let (tri, w) = mesh.locate(p).unwrap_or_else(|| panic!("{x} {y} off the mesh"));
                    assert!(w.iter().all(|&w| (-1e-4..=1.0001).contains(&w)), "{w:?}");
                    assert!(close(mesh.at(&mesh.rest, tri, w), p, 1e-3));
                }
            }
        }
        assert!(build(&[0; 100], 10, Vec2::ZERO, MeshSettings::default()).is_none());
    }

    #[test]
    fn pieces_apart_are_islands_and_expansion_joins_them() {
        let a = alpha(200, 100, &[[10, 10, 60, 90], [80, 10, 130, 90]]);
        let apart = build(&a, 200, Vec2::ZERO, settings(0.0, 40)).unwrap();
        assert_eq!(apart.island_count(), 2);
        let joined = build(&a, 200, Vec2::ZERO, settings(16.0, 40)).unwrap();
        assert_eq!(joined.island_count(), 1);
    }

    #[test]
    fn a_closed_outline_is_filled_and_an_open_one_is_not() {
        // a square ring of line 3 px wide, and the same with a gap in its top
        let ring = [[20, 20, 180, 23], [20, 177, 180, 180], [20, 20, 23, 180], [177, 20, 180, 180]];
        let closed = build(&alpha(200, 200, &ring), 200, Vec2::ZERO, settings(0.0, 40)).unwrap();
        assert!(closed.locate(Vec2::new(100.0, 100.0)).is_some());
        let open = [[20, 20, 80, 23], [120, 20, 180, 23], ring[1], ring[2], ring[3]];
        let open = build(&alpha(200, 200, &open), 200, Vec2::ZERO, settings(0.0, 40)).unwrap();
        assert!(open.locate(Vec2::new(100.0, 100.0)).is_none());
    }

    /// A `w` × `h` alpha with discs (centre x, y, radius) painted.
    fn discs(w: usize, h: usize, discs: &[(f32, f32, f32)]) -> Vec<u8> {
        let mut a = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                if discs.iter().any(|&(cx, cy, r)| p.distance(Vec2::new(cx, cy)) <= r) {
                    a[y * w + x] = 255;
                }
            }
        }
        a
    }

    #[test]
    fn strokes_closer_than_a_triangle_is_wide_stay_apart() {
        // two strokes 6 px apart, in a drawing 400 across at 40 triangles across (each about 10
        // px): a grid of cells that size would have joined them
        let a = alpha(400, 300, &[[0, 20, 10, 280], [16, 20, 26, 280], [300, 140, 400, 160]]);
        let mesh = build(&a, 400, Vec2::ZERO, settings(0.0, 40)).unwrap();
        assert!(mesh.spacing() >= 9.0, "{}", mesh.spacing());
        assert_eq!(mesh.island_count(), 3);
        assert!(mesh.locate(Vec2::new(13.0, 150.0)).is_none(), "the gap between them is off the mesh");
        // a pin on one bends that stroke alone
        let pins: Vec<Pin> =
            [Vec2::new(5.0, 30.0), Vec2::new(5.0, 270.0)].iter().map(|&p| Pin::new(&mesh, p, None).unwrap()).collect();
        let mut moved = pins.clone();
        moved[1].at = moved[1].at + Vec2::new(60.0, 0.0);
        let bent = solve(&mesh, &moved);
        let other = mesh.locate(Vec2::new(21.0, 270.0)).unwrap();
        assert!(close(mesh.at(&bent.verts, other.0, other.1), Vec2::new(21.0, 270.0), 1e-3));
    }

    #[test]
    fn the_mesh_keeps_to_the_outline_in_triangles_about_its_spacing() {
        // a body and a head on it, and an ear off to the side
        let a = discs(300, 300, &[(150.0, 200.0, 80.0), (150.0, 90.0, 50.0), (250.0, 40.0, 20.0)]);
        let expansion = 4.0;
        let mesh = build(&a, 300, Vec2::ZERO, settings(expansion, 30)).unwrap();
        let spacing = mesh.spacing();
        assert_eq!(mesh.island_count(), 2);
        // every vertex near the paint: no further out than the expansion, what simplifying the
        // outline may need and a pixel
        let painted: Vec<Vec2> = (0..300 * 300)
            .filter(|&k| a[k] != 0)
            .map(|k| Vec2::new((k % 300) as f32 + 0.5, (k / 300) as f32 + 0.5))
            .collect();
        let reach = expansion + 0.1 * spacing + 1.5;
        for v in mesh.rest() {
            let d = painted.iter().map(|p| p.distance(*v)).fold(f32::MAX, f32::min);
            assert!(d <= reach, "{v:?} is {d} from the paint");
        }
        // and the outline's vertices close to it all round: a staircase of cells would stand off
        let mut far = 0;
        for &[a, b] in &edges(&mesh) {
            let along = mesh.triangles().iter().filter(|t| t.contains(&a) && t.contains(&b)).count();
            if along == 1 {
                let d = painted.iter().map(|p| p.distance(mesh.rest()[a as usize])).fold(f32::MAX, f32::min);
                if d < expansion - 1.0 {
                    far += 1;
                }
            }
        }
        assert_eq!(far, 0, "outline vertices inside the expansion");
        // no triangle turned over, none far wider than the spacing, few thin ones; as many
        // vertices as a grid of cells the spacing across would have, give or take
        let area = std::f32::consts::PI * (88.0f32.powi(2) + 58.0f32.powi(2) + 28.0f32.powi(2));
        let grid = area / (spacing * spacing);
        let count = mesh.rest().len() as f32;
        assert!(count > 0.6 * grid && count < 1.6 * grid, "{count} vertices against {grid}");
        let mut thin = 0;
        for t in mesh.triangles() {
            let [p, q, r] = t.map(|v| mesh.rest()[v as usize]);
            assert!((q - p).cross(r - p) > 0.0);
            let longest = p.distance(q).max(q.distance(r)).max(r.distance(p));
            assert!(longest <= 1.7 * spacing, "{longest} against {spacing}");
            let least = [(p, q, r), (q, r, p), (r, p, q)]
                .iter()
                .map(|&(a, b, c)| ((b - a).dot(c - a) / (a.distance(b) * a.distance(c))).clamp(-1.0, 1.0).acos())
                .fold(f32::MAX, f32::min);
            if least < 20f32.to_radians() {
                thin += 1;
            }
        }
        assert!(thin * 50 <= mesh.triangles().len(), "{thin} thin of {}", mesh.triangles().len());
    }

    fn bar() -> PuppetMesh {
        // a bar 200 × 40
        build(&alpha(240, 80, &[[20, 20, 220, 60]]), 240, Vec2::ZERO, settings(0.0, 40)).unwrap()
    }

    fn solve(mesh: &PuppetMesh, pins: &[Pin]) -> Bent {
        Solver::new(mesh, pins).solve(mesh, pins)
    }

    fn vertex_near(mesh: &PuppetMesh, p: Vec2) -> usize {
        let nearest =
            (0..mesh.rest().len()).min_by(|&a, &b| mesh.rest()[a].distance(p).total_cmp(&mesh.rest()[b].distance(p)));
        nearest.filter(|&v| close(mesh.rest()[v], p, mesh.spacing())).unwrap()
    }

    #[test]
    fn pins_left_where_they_are_leave_the_mesh_at_rest() {
        let mesh = bar();
        for angle in [None, Some(0.0)] {
            let pins: Vec<Pin> = [Vec2::new(30.0, 40.0), Vec2::new(210.0, 40.0), Vec2::new(120.0, 30.0)]
                .iter()
                .map(|&p| Pin::new(&mesh, p, angle).unwrap())
                .collect();
            let bent = solve(&mesh, &pins);
            for (a, b) in bent.verts.iter().zip(mesh.rest()) {
                assert!(close(*a, *b, 1e-2), "{a:?} {b:?}");
            }
        }
    }

    #[test]
    fn one_pin_carries_its_island_whole() {
        let mesh = bar();
        let mut pin = Pin::new(&mesh, Vec2::new(120.0, 40.0), None).unwrap();
        pin.at = pin.at + Vec2::new(30.0, -12.0);
        let bent = solve(&mesh, &[pin]);
        for (a, b) in bent.verts.iter().zip(mesh.rest()) {
            assert!(close(*a, *b + Vec2::new(30.0, -12.0), 1e-3));
        }
        // turned, it turns about the pin
        pin.angle = Some(std::f32::consts::FRAC_PI_2);
        let bent = solve(&mesh, &[pin]);
        let i = vertex_near(&mesh, Vec2::new(220.0, 40.0));
        let want = pin.at + rotate(mesh.rest()[i] - pin.rest(&mesh), std::f32::consts::FRAC_PI_2);
        assert!(close(bent.verts[i], want, 1e-2), "{:?} {want:?}", bent.verts[i]);
        assert!(close(bent.verts[i], pin.at + Vec2::new(0.0, 100.0), mesh.spacing()), "{:?}", bent.verts[i]);
    }

    #[test]
    fn pins_hold_their_points_where_they_are_taken_and_islands_without_pins_stay() {
        let a = alpha(400, 100, &[[20, 20, 220, 60], [300, 20, 380, 60]]);
        let mesh = build(&a, 400, Vec2::ZERO, settings(0.0, 40)).unwrap();
        assert_eq!(mesh.island_count(), 2);
        let mut pins: Vec<Pin> = [Vec2::new(30.0, 40.0), Vec2::new(120.0, 40.0), Vec2::new(210.0, 40.0)]
            .iter()
            .map(|&p| Pin::new(&mesh, p, None).unwrap())
            .collect();
        pins[2].at = pins[2].at + Vec2::new(-20.0, 60.0);
        let bent = solve(&mesh, &pins);
        for pin in &pins {
            assert!(close(mesh.at(&bent.verts, pin.tri, pin.weights), pin.at, 0.1));
        }
        // the far end bent down, the near one stayed but for a little turn about its pin
        let i = vertex_near(&mesh, Vec2::new(20.0, 20.0));
        assert!(close(bent.verts[i], mesh.rest()[i], 6.0), "{:?}", bent.verts[i]);
        let j = vertex_near(&mesh, Vec2::new(220.0, 40.0));
        assert!(bent.verts[j].y > 80.0, "{:?}", bent.verts[j]);
        // the other island is untouched
        let island = mesh.islands[1];
        for v in island.verts.0..island.verts.1 {
            assert_eq!(bent.verts[v as usize], mesh.rest()[v as usize]);
        }
        // no triangle turned over
        for t in mesh.triangles() {
            let [a, b, c] = t.map(|v| bent.verts[v as usize]);
            let [p, q, r] = t.map(|v| mesh.rest()[v as usize]);
            assert!((b - a).cross(c - a) * (q - p).cross(r - p) > 0.0);
        }
    }

    #[test]
    fn a_pin_holding_an_angle_turns_the_mesh_about_it() {
        let mesh = bar();
        let turn = 0.6;
        let pins = [
            Pin::new(&mesh, Vec2::new(30.0, 40.0), Some(0.0)).unwrap(),
            Pin::new(&mesh, Vec2::new(200.0, 40.0), Some(turn)).unwrap(),
        ];
        let bent = solve(&mesh, &pins);
        // the triangles about the turned pin are turned near enough as far
        let near = mesh.locate(Vec2::new(200.0, 40.0)).unwrap().0;
        let (p, q) = (mesh.corners(mesh.rest(), near), mesh.corners(&bent.verts, near));
        assert!((fit_turn(p, q) - turn).abs() < 0.1, "{}", fit_turn(p, q));
        // a free pin's angle follows the bend
        let mut free = [
            Pin::new(&mesh, Vec2::new(30.0, 40.0), None).unwrap(),
            Pin::new(&mesh, Vec2::new(200.0, 40.0), None).unwrap(),
        ];
        free[1].at = free[1].at + Vec2::new(-40.0, 90.0);
        let bent = solve(&mesh, &free);
        assert!(bent.angles[1] > 0.2, "{:?}", bent.angles);
    }

    #[test]
    fn the_band_solves_as_a_dense_solve_would() {
        // a random symmetric positive definite band, by rows of a least-squares sum
        let (n, w) = (40, 5);
        let mut seed = 7u64;
        let mut next = || {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            ((seed >> 33) as f64 / (1u64 << 31) as f64) - 0.5
        };
        let mut band = Band::new((0..n).map(|i: usize| i.saturating_sub(w)).collect());
        let mut dense = vec![vec![0.0f64; n]; n];
        for i in 0..n {
            let row: Vec<(usize, f64)> =
                (i..(i + w + 1).min(n)).map(|j| (j, next() + if j == i { 2.0 } else { 0.0 })).collect();
            band.add_row(&row, 1.0);
            for &(a, x) in &row {
                for &(b, y) in &row {
                    dense[a][b] += x * y;
                }
            }
        }
        let want: Vec<f64> = (0..n).map(|_| next()).collect();
        let mut b: Vec<f64> = (0..n).map(|i| (0..n).map(|j| dense[i][j] * want[j]).sum()).collect();
        assert!(band.factor());
        band.solve(&mut b);
        for (got, want) in b.iter().zip(&want) {
            assert!((got - want).abs() < 1e-8, "{got} {want}");
        }
    }

    #[test]
    #[ignore = "timing: cargo test --release puppet::tests::timing -- --ignored --nocapture"]
    fn timing() {
        let a = alpha(2000, 2000, &[[100, 100, 1900, 1900]]);
        let started = std::time::Instant::now();
        let mesh = build(&a, 2000, Vec2::ZERO, settings(4.0, DENSITY_RANGE.1)).unwrap();
        println!("laid in {:?}", started.elapsed());
        let mut pins: Vec<Pin> = [Vec2::new(150.0, 150.0), Vec2::new(1850.0, 1850.0), Vec2::new(1000.0, 1000.0)]
            .iter()
            .map(|&p| Pin::new(&mesh, p, None).unwrap())
            .collect();
        let started = std::time::Instant::now();
        let solver = Solver::new(&mesh, &pins);
        let made = started.elapsed();
        pins[1].at = pins[1].at + Vec2::new(-200.0, 50.0);
        let started = std::time::Instant::now();
        let _ = solver.solve(&mesh, &pins);
        println!("{} vertices: made ready in {made:?}, solved in {:?}", mesh.rest().len(), started.elapsed());
    }
}

#[cfg(test)]
mod picture {
    use super::tests::{build, edges};
    use super::*;

    /// Line art (rings 3 px wide) of a figure with long ears, its mesh drawn over it as SVG into
    /// the file `PUPPET_SVG` names.
    #[test]
    #[ignore = "a picture: PUPPET_SVG=mesh.svg cargo test puppet::picture -- --ignored"]
    fn draw_the_mesh() {
        let Ok(path) = std::env::var("PUPPET_SVG") else { return };
        let (w, h) = (600usize, 800usize);
        // ellipses: centre, radii, turn
        let rings = [
            (300.0, 330.0, 120.0, 110.0, 0.0f32),
            (230.0, 130.0, 32.0, 120.0, -0.35),
            (380.0, 140.0, 32.0, 120.0, 0.45),
            (300.0, 600.0, 150.0, 170.0, 0.0),
            (150.0, 560.0, 40.0, 110.0, 0.6),
            (460.0, 520.0, 40.0, 100.0, -0.9),
            (480.0, 300.0, 45.0, 45.0, 0.0),
        ];
        let mut a = vec![0u8; w * h];
        for y in 0..h {
            for x in 0..w {
                let p = Vec2::new(x as f32 + 0.5, y as f32 + 0.5);
                for &(cx, cy, rx, ry, turn) in &rings {
                    let q = rotate(p - Vec2::new(cx, cy), -turn);
                    let r = ((q.x / rx).powi(2) + (q.y / ry).powi(2)).sqrt();
                    if (r - 1.0).abs() * rx.min(ry) < 1.5 {
                        a[y * w + x] = 255;
                    }
                }
            }
        }
        let mesh = build(&a, w, Vec2::ZERO, MeshSettings::default()).unwrap();
        let mut svg =
            format!("<svg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 {w} {h}' width='{w}' height='{h}'>");
        svg += "<rect width='100%' height='100%' fill='#bdbdbd'/>";
        for t in mesh.triangles() {
            let [p, q, r] = t.map(|v| mesh.rest()[v as usize]);
            svg += &format!("<path d='M{} {}L{} {}L{} {}Z' fill='#f4f4f4'/>", p.x, p.y, q.x, q.y, r.x, r.y);
        }
        for &(cx, cy, rx, ry, turn) in &rings {
            svg += &format!(
                "<ellipse cx='{cx}' cy='{cy}' rx='{rx}' ry='{ry}' transform='rotate({} {cx} {cy})' fill='none' stroke='#222' stroke-width='3'/>",
                turn.to_degrees()
            );
        }
        for &[p, q] in &edges(&mesh) {
            let (p, q) = (mesh.rest()[p as usize], mesh.rest()[q as usize]);
            svg += &format!(
                "<line x1='{}' y1='{}' x2='{}' y2='{}' stroke='#333' stroke-width='0.6'/>",
                p.x, p.y, q.x, q.y
            );
        }
        svg += "</svg>";
        std::fs::write(path, svg).unwrap();
    }
}

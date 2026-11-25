//! A puppet warp as the app keeps it: a mesh of triangles laid over what is painted, bent by
//! pins. Laying the mesh and bending it are a plugin's (a mesh warp, `plugin_warps.rs` in the
//! core); here is the mesh it hands back, where points and pins fall on it, and the mesh bent as
//! it is drawn: through its triangles on the GPU, and through them for each point of a vector
//! layer.
//!
//! Everything here is in source coordinates: canvas pixels where the content was when the
//! transform began.

use std::sync::Arc;

use crate::geom::{Homography, Vec2};

fn vert(verts: &[Vec2], v: u32) -> Vec2 {
    verts.get(v as usize).copied().unwrap_or(Vec2::ZERO)
}

/// The mesh at rest.
#[derive(Clone, Debug)]
pub struct PuppetMesh {
    /// About how wide the triangles were laid (source pixels).
    spacing: f32,
    rest: Vec<Vec2>,
    tris: Vec<[u32; 3]>,
    edges: Vec<[u32; 2]>,
    lookup: Lookup,
}

impl PuppetMesh {
    /// The mesh of vertices `rest` (source points) and triangles `tris` (each three of them),
    /// about `spacing` wide; none if it has no triangle, a triangle names a vertex it hasn't, or a
    /// vertex isn't a number.
    pub fn new(rest: Vec<Vec2>, tris: Vec<[u32; 3]>, spacing: f32) -> Option<PuppetMesh> {
        let fits = !tris.is_empty()
            && rest.iter().all(|v| v.is_finite())
            && tris.iter().flatten().all(|&v| (v as usize) < rest.len());
        if !fits {
            return None;
        }
        let spacing = if spacing.is_finite() && spacing > 0.0 { spacing } else { 1.0 };
        let mut edges: Vec<[u32; 2]> =
            tris.iter().flat_map(|&[a, b, c]| [[a, b], [b, c], [c, a]]).map(|[a, b]| [a.min(b), a.max(b)]).collect();
        edges.sort_unstable();
        edges.dedup();
        let lookup = Lookup::new(&rest, &tris, spacing);
        Some(PuppetMesh { spacing, rest, tris, edges, lookup })
    }

    /// The vertices at rest.
    pub fn rest(&self) -> &[Vec2] {
        &self.rest
    }

    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.tris
    }

    /// Each edge once, its two vertices.
    pub fn edges(&self) -> &[[u32; 2]] {
        &self.edges
    }

    /// About how wide the triangles were laid (source pixels).
    pub fn spacing(&self) -> f32 {
        self.spacing
    }

    fn tri(&self, t: u32) -> [u32; 3] {
        self.tris.get(t as usize).copied().unwrap_or([0; 3])
    }

    fn corners(&self, verts: &[Vec2], t: u32) -> [Vec2; 3] {
        self.tri(t).map(|v| vert(verts, v))
    }

    /// The triangle source point `p` lies in and its weights there; none off the mesh. Where
    /// islands overlap, the first.
    pub fn locate(&self, p: Vec2) -> Option<(u32, [f32; 3])> {
        if !p.is_finite() {
            return None;
        }
        let (x, y) = self.lookup.cell_of(p);
        self.lookup.at(x, y).iter().find_map(|&t| {
            let w = self.weights(t, p);
            w.iter().all(|&w| w >= -1e-4).then_some((t, w))
        })
    }

    /// The triangle nearest source point `p` and its weights there, which lie outside 0 to 1 when
    /// `p` is off the mesh (the triangle's own map carried on past it).
    pub fn nearest(&self, p: Vec2) -> Option<(u32, [f32; 3])> {
        if let Some(found) = self.locate(p) {
            return Some(found);
        }
        if self.tris.is_empty() || !p.is_finite() {
            return None;
        }
        let lookup = &self.lookup;
        let (cx, cy) = lookup.cell_of(p);
        let (cx, cy) = (cx as isize, cy as isize);
        // rings of cells further and further out, until a triangle is found and no nearer one can
        // be (a cell r rings out is at least r - 1 cells from the one `p` is in, or nearest)
        let mut best: Option<(f32, u32)> = None;
        for r in 0..=(lookup.cols.max(lookup.rows) as isize) {
            if let Some((d, _)) = best
                && (r as f32 - 1.0) * lookup.size > d
            {
                break;
            }
            for y in (cy - r).max(0)..=(cy + r).min(lookup.rows as isize - 1) {
                for x in (cx - r).max(0)..=(cx + r).min(lookup.cols as isize - 1) {
                    if (y - cy).abs() != r && (x - cx).abs() != r {
                        continue;
                    }
                    for &t in lookup.at(x as usize, y as usize) {
                        let d = triangle_distance(self.corners(&self.rest, t), p);
                        if best.is_none_or(|(b, _)| d < b) {
                            best = Some((d, t));
                        }
                    }
                }
            }
        }
        let (_, tri) = best?;
        Some((tri, self.weights(tri, p)))
    }

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
}

/// How far `p` is from triangle `t` (0 inside it).
fn triangle_distance(t: [Vec2; 3], p: Vec2) -> f32 {
    let [a, b, c] = t;
    let sides = [(a, b), (b, c), (c, a)];
    let turn = (b - a).cross(c - a).signum();
    if sides.iter().all(|&(e, f)| (f - e).cross(p - e) * turn >= 0.0) {
        return 0.0;
    }
    sides
        .iter()
        .map(|&(e, f)| {
            let d = f - e;
            let len = d.dot(d);
            let s = if len > 0.0 { ((p - e).dot(d) / len).clamp(0.0, 1.0) } else { 0.0 };
            p.distance(e + d * s)
        })
        .fold(f32::MAX, f32::min)
}

/// The triangles by square cells over the mesh: each cell, the triangles whose boxes touch it.
#[derive(Clone, Debug, Default)]
struct Lookup {
    corner: Vec2,
    size: f32,
    cols: usize,
    rows: usize,
    /// Each cell's first entry in `tris` (and one past the last cell's last).
    first: Vec<u32>,
    tris: Vec<u32>,
}

impl Lookup {
    /// The most cells along a side.
    const MOST: usize = 512;

    fn new(verts: &[Vec2], tris: &[[u32; 3]], size: f32) -> Lookup {
        let (mut lo, mut hi) = (Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN));
        for v in verts {
            lo = Vec2::new(lo.x.min(v.x), lo.y.min(v.y));
            hi = Vec2::new(hi.x.max(v.x), hi.y.max(v.y));
        }
        if !(lo.x <= hi.x && lo.y <= hi.y) {
            return Lookup::default();
        }
        let size = size.max((hi.x - lo.x).max(hi.y - lo.y) / Self::MOST as f32).max(1e-3);
        let cols = ((hi.x - lo.x) / size) as usize + 1;
        let rows = ((hi.y - lo.y) / size) as usize + 1;
        let mut lookup = Lookup { corner: lo, size, cols, rows, first: vec![0; cols * rows + 1], tris: Vec::new() };
        let span = |lookup: &Lookup, t: &[u32; 3]| {
            let p = t.map(|v| vert(verts, v));
            let (a, b) = (
                lookup.cell_of(Vec2::new(p[0].x.min(p[1].x).min(p[2].x), p[0].y.min(p[1].y).min(p[2].y))),
                lookup.cell_of(Vec2::new(p[0].x.max(p[1].x).max(p[2].x), p[0].y.max(p[1].y).max(p[2].y))),
            );
            (a.1..=b.1).flat_map(move |y| (a.0..=b.0).map(move |x| y * cols + x))
        };
        // counted, then each cell's run filled
        for t in tris {
            for c in span(&lookup, t) {
                if let Some(n) = lookup.first.get_mut(c + 1) {
                    *n += 1;
                }
            }
        }
        for c in 0..cols * rows {
            let before = lookup.first.get(c).copied().unwrap_or(0);
            if let Some(n) = lookup.first.get_mut(c + 1) {
                *n += before;
            }
        }
        let mut fill = lookup.first.clone();
        lookup.tris = vec![0; lookup.first.last().copied().unwrap_or(0) as usize];
        for (k, t) in tris.iter().enumerate() {
            for c in span(&lookup, t) {
                if let Some(at) = fill.get_mut(c) {
                    if let Some(slot) = lookup.tris.get_mut(*at as usize) {
                        *slot = k as u32;
                    }
                    *at += 1;
                }
            }
        }
        lookup
    }

    /// The cell `p` is in, or the nearest.
    fn cell_of(&self, p: Vec2) -> (usize, usize) {
        let q = (p - self.corner) * (1.0 / self.size);
        let clamp = |v: f32, n: usize| (v.max(0.0) as usize).min(n.saturating_sub(1));
        (clamp(q.x, self.cols), clamp(q.y, self.rows))
    }

    fn at(&self, x: usize, y: usize) -> &[u32] {
        let c = y * self.cols + x;
        let (a, b) = (self.first.get(c).copied().unwrap_or(0), self.first.get(c + 1).copied().unwrap_or(0));
        self.tris.get(a as usize..b as usize).unwrap_or(&[])
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

/// The mesh bent by its pins: its vertices, and the turn about each pin (radians, clockwise on
/// screen).
#[derive(Clone, Debug, PartialEq)]
pub struct Bent {
    pub verts: Vec<Vec2>,
    pub angles: Vec<f32>,
}

/// What bends a mesh by its pins: a plugin's mesh warp.
pub trait Bends: Send + Sync + std::fmt::Debug {
    /// `mesh` bent by `pins`: where each of its vertices goes, and the turn about each pin; none if
    /// it can't be.
    fn bend(&self, mesh: &PuppetMesh, pins: &[Pin]) -> Option<Bent>;
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

/// The puppet as it is shown: the mesh, its vertices bent (source pixels), and the map from
/// source to canvas the box puts them through.
#[derive(Debug)]
pub struct PuppetFrame {
    pub mesh: Arc<PuppetMesh>,
    pub bent: Bent,
    pub place: Homography,
}

impl PuppetFrame {
    /// A vertex where it is shown (canvas pixels).
    fn shown(&self, v: Vec2) -> Vec2 {
        self.place.apply(v).unwrap_or(v)
    }

    /// Where source point `p` goes on the canvas: through the triangle it lies in (or the nearest,
    /// carried on past it).
    pub fn point(&self, p: Vec2) -> Vec2 {
        match self.mesh.nearest(p) {
            Some((tri, w)) => self.shown(self.mesh.at(&self.bent.verts, tri, w)),
            None => self.shown(p),
        }
    }

    /// The mesh as triangles to draw, each corner (canvas x, y, source x, y).
    pub fn triangles(&self) -> Vec<[f32; 4]> {
        let rest = &self.mesh.rest;
        self.mesh
            .tris
            .iter()
            .flat_map(|t| {
                t.map(|v| {
                    let (to, from) = (self.shown(vert(&self.bent.verts, v)), vert(rest, v));
                    [to.x, to.y, from.x, from.y]
                })
            })
            .collect()
    }

    /// Its edges as segments (x0, y0, x1, y1 in turn, canvas pixels).
    pub fn edge_lines(&self) -> Vec<f32> {
        self.mesh
            .edges
            .iter()
            .flat_map(|&[a, b]| {
                let (a, b) = (self.shown(vert(&self.bent.verts, a)), self.shown(vert(&self.bent.verts, b)));
                [a.x, a.y, b.x, b.y]
            })
            .collect()
    }

    /// Where on the mesh at rest source point `p` of the mesh as it is bent lies, and how far the
    /// bend has turned it there; none off the bent mesh. Where it folds over itself, the first.
    pub fn find_bent(&self, p: Vec2) -> Option<(Vec2, f32)> {
        (0..self.mesh.tris.len() as u32).find_map(|t| {
            let [a, b, c] = self.mesh.corners(&self.bent.verts, t);
            let d = (b - a).cross(c - a);
            if d.abs() < 1e-9 {
                return None;
            }
            let wb = (p - a).cross(c - a) / d;
            let wc = (b - a).cross(p - a) / d;
            let w = [1.0 - wb - wc, wb, wc];
            w.iter().all(|&w| w >= -1e-4).then(|| {
                let turn = fit_turn(self.mesh.corners(&self.mesh.rest, t), [a, b, c]);
                (self.mesh.at(&self.mesh.rest, t, w), turn)
            })
        })
    }

    /// Where `pin` is shown (canvas pixels).
    pub fn pin_shown(&self, pin: &Pin) -> Vec2 {
        self.shown(pin.at)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A mesh over the rectangle `x`, `y`, `w` × `h` in squares `step` across, each two triangles.
    pub(crate) fn grid(x: f32, y: f32, w: f32, h: f32, step: f32) -> PuppetMesh {
        let (cols, rows) = ((w / step).round() as u32, (h / step).round() as u32);
        let mut rest = Vec::new();
        for j in 0..=rows {
            for i in 0..=cols {
                rest.push(Vec2::new(x + i as f32 * step, y + j as f32 * step));
            }
        }
        let at = |i: u32, j: u32| j * (cols + 1) + i;
        let mut tris = Vec::new();
        for j in 0..rows {
            for i in 0..cols {
                tris.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
                tris.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
            }
        }
        PuppetMesh::new(rest, tris, step).unwrap()
    }

    /// Bends a mesh as a whole, by the turn, scale and move that carry its first two pins where
    /// they are taken (one pin: moved, and turned as it holds), every pin's angle that turn: so
    /// pins land exactly, for tests of what keeps them.
    #[derive(Debug)]
    pub(crate) struct Whole;

    impl Bends for Whole {
        fn bend(&self, mesh: &PuppetMesh, pins: &[Pin]) -> Option<Bent> {
            let (map, turn): (Box<dyn Fn(Vec2) -> Vec2>, f32) = match pins {
                [] => (Box::new(|p| p), 0.0),
                [one] => {
                    let (pivot, turn) = (one.rest(mesh), one.angle.unwrap_or(0.0));
                    let at = one.at;
                    (Box::new(move |p: Vec2| at + rotate(p - pivot, turn)), turn)
                }
                [a, b, ..] => {
                    let (p, q) = (a.rest(mesh), b.rest(mesh));
                    let (from, to) = (q - p, b.at - a.at);
                    let len = from.dot(from);
                    if len < 1e-9 {
                        return None;
                    }
                    // the complex number that carries `from` onto `to`
                    let (c, s) = (from.dot(to) / len, from.cross(to) / len);
                    let at = a.at;
                    (
                        Box::new(move |v: Vec2| {
                            let d = v - p;
                            at + Vec2::new(c * d.x - s * d.y, s * d.x + c * d.y)
                        }),
                        s.atan2(c),
                    )
                }
            };
            let verts = mesh.rest().iter().map(|&v| map(v)).collect();
            let angles = pins.iter().map(|p| p.angle.unwrap_or(turn)).collect();
            Some(Bent { verts, angles })
        }
    }

    fn rotate(v: Vec2, radians: f32) -> Vec2 {
        let (sin, cos) = radians.sin_cos();
        Vec2::new(v.x * cos - v.y * sin, v.x * sin + v.y * cos)
    }

    fn close(a: Vec2, b: Vec2, tolerance: f32) -> bool {
        a.distance(b) <= tolerance
    }

    #[test]
    fn a_mesh_handed_over_is_checked_and_finds_its_points() {
        assert!(PuppetMesh::new(vec![Vec2::ZERO; 3], vec![[0, 1, 3]], 1.0).is_none());
        assert!(PuppetMesh::new(vec![Vec2::ZERO; 3], Vec::new(), 1.0).is_none());
        assert!(
            PuppetMesh::new(vec![Vec2::new(f32::NAN, 0.0), Vec2::ZERO, Vec2::ZERO], vec![[0, 1, 2]], 1.0).is_none()
        );
        let mesh = grid(20.0, 20.0, 200.0, 40.0, 10.0);
        assert_eq!(mesh.edges().len(), 20 * 5 + 21 * 4 + 20 * 4);
        let (tri, w) = mesh.locate(Vec2::new(33.0, 47.0)).unwrap();
        assert!(close(mesh.at(mesh.rest(), tri, w), Vec2::new(33.0, 47.0), 1e-3));
        assert!(mesh.locate(Vec2::new(10.0, 10.0)).is_none());
        // off the mesh, the nearest triangle carried on past it
        let (tri, w) = mesh.nearest(Vec2::new(10.0, 40.0)).unwrap();
        assert!(w.iter().any(|&w| w < 0.0));
        assert!(close(mesh.at(mesh.rest(), tri, w), Vec2::new(10.0, 40.0), 1e-3));
    }

    #[test]
    fn a_frame_carries_points_as_its_triangles_do() {
        let mesh = Arc::new(grid(20.0, 20.0, 200.0, 40.0, 10.0));
        let mut pins = vec![
            Pin::new(&mesh, Vec2::new(30.0, 40.0), None).unwrap(),
            Pin::new(&mesh, Vec2::new(200.0, 40.0), None).unwrap(),
        ];
        pins[1].at = pins[1].at + Vec2::new(0.0, 50.0);
        let bent = Whole.bend(&mesh, &pins).unwrap();
        let place = Homography::from_affine(crate::geom::Affine::translate(Vec2::new(10.0, 0.0)));
        let frame = PuppetFrame { mesh: mesh.clone(), bent, place };
        let tris = frame.triangles();
        assert_eq!(tris.len(), mesh.triangles().len() * 3);
        for c in tris.iter().step_by(7) {
            let p = frame.point(Vec2::new(c[2], c[3]));
            assert!(close(p, Vec2::new(c[0], c[1]), 1e-2), "{p:?} {c:?}");
        }
        // off the mesh, carried on as the nearest triangle is
        assert!(frame.point(Vec2::new(0.0, 40.0)).is_finite());
        assert_eq!(frame.edge_lines().len(), mesh.edges().len() * 4);
        // a point of the bent mesh found where it is at rest
        let bent_pin = frame.mesh.at(&frame.bent.verts, pins[1].tri, pins[1].weights);
        let (rest, _) = frame.find_bent(bent_pin).unwrap();
        assert!(close(rest, Vec2::new(200.0, 40.0), 1e-2), "{rest:?}");
    }
}

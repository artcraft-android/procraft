//! A page's panels as shapes: each a convex polygon, wound so its area (shoelace) is positive, in
//! canvas pixels. A line cuts a panel in two with a gap (the gutter) between the halves; a gutter
//! dragged moves the panels' edges along it on both sides, so the gap between them stays.

/// A point on the canvas, canvas pixels.
pub type Point = [f32; 2];

/// How far from a right angle (degrees) a cut is made straight across or down.
pub const SNAP_DEGREES: f32 = 4.0;

/// The smallest a half of a cut panel may be across (canvas pixels): a cut leaving less makes
/// nothing.
const LEAST_ACROSS: f32 = 8.0;

/// The gaps a new page's panels are cut with, and the page's margin and border, for a canvas
/// `width` × `height`: between panels side by side (a column gap), between rows (wider, as
/// manga's are), round the page, and the border's width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PageMeasures {
    pub column_gap: f32,
    pub row_gap: f32,
    pub margin: f32,
    pub border: f32,
}

impl PageMeasures {
    pub fn for_canvas(width: u32, height: u32) -> PageMeasures {
        let side = width.min(height).max(1) as f32;
        PageMeasures {
            column_gap: (side * 0.02).max(2.0).round(),
            row_gap: (side * 0.035).max(3.0).round(),
            margin: (side * 0.07).max(4.0).round(),
            border: (side * 0.003).clamp(1.0, MOST_BORDER).round(),
        }
    }
}

/// A canvas's one panel inside its margin.
pub fn page_shape(width: u32, height: u32, margin: f32) -> Vec<Point> {
    let (w, h) = (width as f32, height as f32);
    let m = margin.clamp(0.0, w.min(h) / 2.0 - 1.0).max(0.0);
    vec![[m, m], [w - m, m], [w - m, h - m], [m, h - m]]
}

/// Each side of the polygon: a corner and the next, the last going back to the first.
fn sides(points: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    points.iter().copied().zip(points.iter().copied().cycle().skip(1))
}

/// Twice the polygon's area (shoelace): positive when wound as [`PanelFrame::points`] are.
fn twice_area(points: &[Point]) -> f32 {
    sides(points).map(|(p, q)| p[0] * q[1] - q[0] * p[1]).sum()
}

/// `points` wound as frames are, with corners on top of each other dropped.
pub fn tidied(mut points: Vec<Point>) -> Vec<Point> {
    points.retain(|p| p[0].is_finite() && p[1].is_finite());
    points.dedup_by(|a, b| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-3);
    while points.len() > 1
        && points.first().zip(points.last()).is_some_and(|(a, b)| (a[0] - b[0]).hypot(a[1] - b[1]) < 1e-3)
    {
        points.pop();
    }
    if twice_area(&points) < 0.0 {
        points.reverse();
    }
    points
}

/// Each edge of a convex polygon: a corner and the unit normal pointing in.
fn edges(points: &[Point]) -> Vec<(Point, Point)> {
    sides(points)
        .filter_map(|(p, q)| {
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            let len = dx.hypot(dy);
            (len > 1e-6).then(|| (p, [-dy / len, dx / len]))
        })
        .collect()
}

/// The line from `a` to `b` turned about its middle to straight across or down when it is within
/// [`SNAP_DEGREES`] of it.
pub fn snapped(a: Point, b: Point) -> (Point, Point) {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if len < 1e-6 {
        return (a, b);
    }
    let angle = dy.atan2(dx).to_degrees();
    let straight = (angle / 90.0).round() * 90.0;
    if (angle - straight).abs() > SNAP_DEGREES {
        return (a, b);
    }
    let (s, c) = straight.to_radians().sin_cos();
    let mid = [(a[0] + b[0]) / 2.0, (a[1] + b[1]) / 2.0];
    let half = [c * len / 2.0, s * len / 2.0];
    ([mid[0] - half[0], mid[1] - half[1]], [mid[0] + half[0], mid[1] + half[1]])
}

/// The gap a cut from `a` to `b` leaves: `column_gap` for one more down than across (it puts
/// panels side by side), else `row_gap`.
pub fn gap_for(a: Point, b: Point, column_gap: f32, row_gap: f32) -> f32 {
    if (b[1] - a[1]).abs() > (b[0] - a[0]).abs() { column_gap } else { row_gap }
}

/// Whether the segment from `a` to `b` passes through the shape (more than touching it).
pub fn crosses(points: &[Point], a: Point, b: Point) -> bool {
    if points.len() < 3 {
        return false;
    }
    // the part of the segment inside each edge, narrowed edge by edge
    let (mut t0, mut t1) = (0.0f32, 1.0f32);
    for (p, n) in edges(points) {
        let f0 = (a[0] - p[0]) * n[0] + (a[1] - p[1]) * n[1];
        let f1 = (b[0] - p[0]) * n[0] + (b[1] - p[1]) * n[1];
        if f0 < 0.0 && f1 < 0.0 {
            return false;
        }
        if f0 < 0.0 || f1 < 0.0 {
            let t = f0 / (f0 - f1);
            if f0 < 0.0 { t0 = t0.max(t) } else { t1 = t1.min(t) }
        }
    }
    let len = (b[0] - a[0]).hypot(b[1] - a[1]);
    (t1 - t0) * len > 0.5
}

/// The part of the shape where `side` is at least `least`, `side` a signed distance.
fn clip(points: &[Point], side: impl Fn(Point) -> f32, least: f32) -> Vec<Point> {
    let mut out = Vec::with_capacity(points.len() + 1);
    for (p, q) in sides(points) {
        let (sp, sq) = (side(p) - least, side(q) - least);
        if sp >= 0.0 {
            out.push(p);
        }
        if (sp >= 0.0) != (sq >= 0.0) {
            let t = sp / (sp - sq);
            out.push([p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t]);
        }
    }
    tidied(out)
}

/// The shape's least width across: what a half too thin to keep is told by.
fn least_across(points: &[Point]) -> f32 {
    // for each edge, how far the far corner is: a convex shape's width across that edge
    edges(points)
        .iter()
        .map(|(p, n)| points.iter().map(|r| (r[0] - p[0]) * n[0] + (r[1] - p[1]) * n[1]).fold(0.0, f32::max))
        .fold(f32::INFINITY, f32::min)
}

/// The shape cut in two along the line through `a` and `b`, `gap` apart: the two halves, the
/// larger first. None if the line misses it or a half would be too thin.
pub fn cut(points: &[Point], a: Point, b: Point, gap: f32) -> Option<(Vec<Point>, Vec<Point>)> {
    let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
    let len = dx.hypot(dy);
    if points.len() < 3 || len < 1e-3 {
        return None;
    }
    let m = [-dy / len, dx / len];
    let side = |r: Point| (r[0] - a[0]) * m[0] + (r[1] - a[1]) * m[1];
    let half = gap.max(0.0) / 2.0;
    let one = clip(points, side, half);
    let other = clip(points, |r| -side(r), half);
    let keeps = |s: &[Point]| s.len() >= 3 && least_across(s) >= LEAST_ACROSS;
    if !keeps(&one) || !keeps(&other) {
        return None;
    }
    Some(if twice_area(&one) >= twice_area(&other) { (one, other) } else { (other, one) })
}

/// Where the lines through `p` along `d` and through `q` along `e` meet; none if they are
/// parallel.
fn meet(p: Point, d: Point, q: Point, e: Point) -> Option<Point> {
    let cross = d[0] * e[1] - d[1] * e[0];
    if cross.abs() < 1e-9 {
        return None;
    }
    let s = ((q[0] - p[0]) * e[1] - (q[1] - p[1]) * e[0]) / cross;
    Some([p[0] + s * d[0], p[1] + s * d[1]])
}

/// Edge `i` of the shape (from corner `i` to the next) pushed `by` canvas pixels outwards (in,
/// for less than nothing), its neighbours kept on their lines: the two corners it joins slide along
/// them. None if a neighbour runs alongside it.
fn move_edge(points: &[Point], i: usize, by: f32) -> Option<Vec<Point>> {
    let n = points.len();
    if n < 3 || i >= n {
        return None;
    }
    let at = |k: usize| points.get(k % n).copied();
    let (before, p, q, after) = (at(i + n - 1)?, at(i)?, at(i + 1)?, at(i + 2)?);
    let d = [q[0] - p[0], q[1] - p[1]];
    let len = d[0].hypot(d[1]);
    if len < 1e-6 {
        return None;
    }
    let out = [d[1] / len, -d[0] / len];
    let moved = [p[0] + out[0] * by, p[1] + out[1] * by];
    let first = meet([before[0], before[1]], [p[0] - before[0], p[1] - before[1]], moved, d)?;
    let second = meet(moved, d, q, [after[0] - q[0], after[1] - q[1]])?;
    let mut shape = points.to_vec();
    *shape.get_mut(i)? = first;
    *shape.get_mut((i + 1) % n)? = second;
    Some(shape)
}

/// Whether a shape is one a frame can be: convex, wound as frames are, no thinner than a cut may
/// leave, and on a canvas `width` × `height`.
fn fits(points: &[Point], width: u32, height: u32) -> bool {
    let turns = || {
        let next = points.iter().copied().cycle().skip(1);
        let after = points.iter().copied().cycle().skip(2);
        points
            .iter()
            .copied()
            .zip(next)
            .zip(after)
            .map(|((p, q), r)| (q[0] - p[0]) * (r[1] - q[1]) - (q[1] - p[1]) * (r[0] - q[0]))
    };
    let (w, h) = (width as f32 + 0.5, height as f32 + 0.5);
    points.len() >= 3
        && turns().all(|t| t >= -1e-3)
        && twice_area(points) > 0.0
        && least_across(points) >= LEAST_ACROSS
        && points.iter().all(|p| (-0.5..=w).contains(&p[0]) && (-0.5..=h).contains(&p[1]))
}

/// A gutter as it is dragged: the frames' edges along one line on each side of it, moved together
/// so the gap between them stays.
#[derive(Clone, Debug, PartialEq)]
pub struct Gutter {
    /// The way it moves: out of the edge it was taken by.
    pub normal: Point,
    /// The edges moved: each frame, its edge's index, and 1 for an edge facing the way the gutter
    /// moves (it moves out as the gutter does), -1 for one facing back.
    pub edges: Vec<(u64, usize, f32)>,
}

/// The gutter at `p` among `frames` (each frame and its shape): the edge nearest `p`, within
/// `reach` (canvas pixels), with every edge along the same line facing the same way, and the
/// edges facing them across the nearest gap, no wider than `most_gap`. None if no edge is in reach.
pub fn gutter_at(frames: &[(u64, Vec<Point>)], p: Point, reach: f32, most_gap: f32) -> Option<Gutter> {
    // each edge: its frame, index, a corner, the unit normal out, and how far p is from it
    let all: Vec<(u64, usize, Point, Point, f32)> = frames
        .iter()
        .flat_map(|(id, shape)| {
            sides(shape).enumerate().filter_map(move |(k, (a, b))| {
                let d = [b[0] - a[0], b[1] - a[1]];
                let len2 = d[0] * d[0] + d[1] * d[1];
                if len2 < 1e-9 {
                    return None;
                }
                let t = (((p[0] - a[0]) * d[0] + (p[1] - a[1]) * d[1]) / len2).clamp(0.0, 1.0);
                let near = [a[0] + d[0] * t, a[1] + d[1] * t];
                let len = len2.sqrt();
                Some((*id, k, a, [d[1] / len, -d[0] / len], (p[0] - near[0]).hypot(p[1] - near[1])))
            })
        })
        .collect();
    let (_, _, a, normal, far) = *all.iter().min_by(|x, y| x.4.total_cmp(&y.4))?;
    if far > reach {
        return None;
    }
    let along = |c: Point| c[0] * normal[0] + c[1] * normal[1];
    let line = along(a);
    let facing = |m: Point| m[0] * normal[0] + m[1] * normal[1];
    let mut edges: Vec<(u64, usize, f32)> = all
        .iter()
        .filter(|e| facing(e.3) > 0.9995 && (along(e.2) - line).abs() < 1.0)
        .map(|e| (e.0, e.1, 1.0))
        .collect();
    // across the gap: the nearest edges facing back, beyond this line
    let back = |e: &&(u64, usize, Point, Point, f32)| {
        let gap = along(e.2) - line;
        facing(e.3) < -0.9995 && gap > 0.0 && gap <= most_gap
    };
    if let Some(nearest) = all.iter().filter(back).map(|e| along(e.2)).min_by(f32::total_cmp) {
        edges.extend(all.iter().filter(back).filter(|e| (along(e.2) - nearest).abs() < 1.0).map(|e| (e.0, e.1, -1.0)));
    }
    Some(Gutter { normal, edges })
}

impl Gutter {
    /// The frames it moves, as they are with it moved `by` canvas pixels along its normal; none if
    /// a frame would no longer fit (see [`fits`]) on a canvas `width` × `height`.
    fn moved_exactly(
        &self,
        frames: &[(u64, Vec<Point>)],
        by: f32,
        width: u32,
        height: u32,
    ) -> Option<Vec<(u64, Vec<Point>)>> {
        let mut out: Vec<(u64, Vec<Point>)> = Vec::new();
        for &(id, edge, way) in &self.edges {
            let shape = match out.iter().position(|(o, _)| *o == id) {
                Some(i) => out.swap_remove(i).1,
                None => frames.iter().find(|(f, _)| *f == id)?.1.clone(),
            };
            let shape = move_edge(&shape, edge, way * by)?;
            if !fits(&shape, width, height) {
                return None;
            }
            out.push((id, shape));
        }
        Some(out)
    }

    /// The frames it moves (from `frames`, as they were when it was taken) with it moved as near
    /// `by` canvas pixels along its normal as they fit on a canvas `width` × `height`, and how far
    /// that is.
    pub fn moved(
        &self,
        frames: &[(u64, Vec<Point>)],
        by: f32,
        width: u32,
        height: u32,
    ) -> (f32, Vec<(u64, Vec<Point>)>) {
        if let Some(shapes) = self.moved_exactly(frames, by, width, height) {
            return (by, shapes);
        }
        // as far as it goes: halved towards where it was
        let (mut fit, mut over) = (0.0f32, 1.0f32);
        for _ in 0..20 {
            let mid = (fit + over) / 2.0;
            if self.moved_exactly(frames, by * mid, width, height).is_some() { fit = mid } else { over = mid }
        }
        let shapes = self.moved_exactly(frames, by * fit, width, height).unwrap_or_default();
        (by * fit, shapes)
    }
}

/// The widest a frame's border is drawn (canvas pixels), as the app's outline style allows.
const MOST_BORDER: f32 = 100.0;

#[cfg(test)]
mod tests {
    use super::*;

    fn square(s: f32) -> Vec<Point> {
        vec![[0.0, 0.0], [s, 0.0], [s, s], [0.0, s]]
    }

    fn area(points: &[Point]) -> f32 {
        twice_area(points) / 2.0
    }

    #[test]
    fn a_cut_down_the_middle_leaves_two_halves_a_gap_apart() {
        let (one, other) = cut(&square(100.0), [50.0, -10.0], [50.0, 110.0], 10.0).unwrap();
        assert!((area(&one) - 4500.0).abs() < 0.01 && (area(&other) - 4500.0).abs() < 0.01);
        let xs = |s: &[Point]| s.iter().map(|p| p[0]).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)));
        let (a, b) = (xs(&one), xs(&other));
        let (left, right) = if a.0 < b.0 { (a, b) } else { (b, a) };
        assert_eq!((left.1, right.0), (45.0, 55.0));
        assert!(twice_area(&one) > 0.0 && twice_area(&other) > 0.0);
    }

    #[test]
    fn the_larger_half_comes_first() {
        let (keep, other) = cut(&square(100.0), [-5.0, 30.0], [105.0, 30.0], 0.0).unwrap();
        assert!(area(&keep) > area(&other));
        assert!((area(&other) - 3000.0).abs() < 0.01);
    }

    #[test]
    fn a_cut_too_near_an_edge_or_missing_the_shape_makes_nothing() {
        assert!(cut(&square(100.0), [3.0, 0.0], [3.0, 100.0], 0.0).is_none());
        assert!(cut(&square(100.0), [200.0, 0.0], [200.0, 100.0], 0.0).is_none());
        assert!(!crosses(&square(100.0), [150.0, 0.0], [150.0, 100.0]));
        assert!(crosses(&square(100.0), [50.0, 40.0], [50.0, 60.0]));
        assert!(crosses(&square(100.0), [-50.0, 50.0], [150.0, 50.0]));
    }

    #[test]
    fn a_nearly_straight_line_is_made_straight_about_its_middle() {
        let (a, b) = snapped([0.0, 0.0], [100.0, 5.0]);
        assert!((a[1] - 2.5).abs() < 1e-3 && (b[1] - 2.5).abs() < 1e-3);
        assert!(((b[0] - a[0]) - 100.0f32.hypot(5.0)).abs() < 1e-3);
        let slant = snapped([0.0, 0.0], [100.0, 30.0]);
        assert_eq!(slant, ([0.0, 0.0], [100.0, 30.0]));
        assert_eq!(gap_for([0.0, 0.0], [0.0, 10.0], 1.0, 2.0), 1.0);
        assert_eq!(gap_for([0.0, 0.0], [10.0, 1.0], 1.0, 2.0), 2.0);
    }

    #[test]
    fn a_slanted_cut_keeps_both_halves_convex_and_wound_alike() {
        let (one, other) = cut(&square(100.0), [0.0, 20.0], [100.0, 80.0], 6.0).unwrap();
        for s in [&one, &other] {
            assert!(twice_area(s) > 0.0);
            // convex: every corner turns the same way
            let n = s.len();
            for i in 0..n {
                let (p, q, r) = (s[i], s[(i + 1) % n], s[(i + 2) % n]);
                let turn = (q[0] - p[0]) * (r[1] - q[1]) - (q[1] - p[1]) * (r[0] - q[0]);
                assert!(turn >= -1e-3, "{s:?}");
            }
        }
    }

    /// A 2 × 2 page on a 1000 square: cut down the middle (a 20 gap), then across (a 30 gap).
    fn grid() -> ((), Vec<(u64, Vec<Point>)>) {
        let page = page_shape(1000, 1000, PageMeasures::for_canvas(1000, 1000).margin);
        let (left, right) = cut(&page, [500.0, 0.0], [500.0, 1000.0], 20.0).unwrap();
        let mut frames = Vec::new();
        for half in [left, right] {
            let (top, bottom) = cut(&half, [0.0, 500.0], [1000.0, 500.0], 30.0).unwrap();
            frames.push(top);
            frames.push(bottom);
        }
        ((), frames.into_iter().enumerate().map(|(k, f)| (k as u64 + 1, f)).collect())
    }

    fn xs(shape: &[Point]) -> (f32, f32) {
        shape.iter().map(|p| p[0]).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x)))
    }

    #[test]
    fn a_gutter_taken_from_either_side_moves_both_columns_edges_and_keeps_its_gap() {
        let (_, frames) = grid();
        assert_eq!(frames.len(), 4);
        for at in [[489.0, 200.0], [511.0, 800.0], [500.0, 300.0]] {
            let gutter = gutter_at(&frames, at, 15.0, 250.0).unwrap();
            assert_eq!(gutter.edges.len(), 4, "{at:?}");
            let (by, moved) = gutter.moved(&frames, 100.0 * gutter.normal[0].signum(), 1000, 1000);
            assert_eq!(by.abs(), 100.0);
            assert_eq!(moved.len(), 4);
            let mut sides: Vec<(f32, f32)> = moved.iter().map(|(_, s)| xs(s)).collect();
            sides.sort_by(|a, b| a.0.total_cmp(&b.0));
            // the left column's right edge and the right column's left edge, still 20 apart
            assert_eq!((sides[0].1, sides[2].0), (590.0, 610.0));
        }
        assert!(gutter_at(&frames, [300.0, 300.0], 15.0, 250.0).is_none());
    }

    #[test]
    fn a_gutter_stops_where_a_panel_would_get_too_thin() {
        let (_, frames) = grid();
        let gutter = gutter_at(&frames, [500.0, 300.0], 15.0, 250.0).unwrap();
        let (by, moved) = gutter.moved(&frames, 5000.0 * gutter.normal[0].signum(), 1000, 1000);
        assert!(by.abs() < 500.0 && by.abs() > 300.0, "{by}");
        assert!(moved.iter().all(|(_, s)| least_across(s) >= LEAST_ACROSS - 0.01));
    }

    #[test]
    fn the_page_s_edge_moves_alone_as_its_margin() {
        let (_, frames) = grid();
        let gutter = gutter_at(&frames, [70.0, 200.0], 15.0, 250.0).unwrap();
        // both left panels' left edges, nothing across from them
        assert_eq!(gutter.edges.len(), 2);
        assert!(gutter.edges.iter().all(|e| e.2 == 1.0));
        let (_, moved) = gutter.moved(&frames, -40.0 * gutter.normal[0], 1000, 1000);
        // out along its normal: further left, the margin narrower
        assert!(moved.iter().all(|(_, s)| xs(s).0 == 30.0));
    }

    #[test]
    fn a_slanted_edge_moves_with_its_neighbours_kept_on_their_lines() {
        let (one, _) = cut(&square(100.0), [0.0, 20.0], [100.0, 80.0], 0.0).unwrap();
        let slant = sides(&one).position(|(p, q)| (p[0] - q[0]).abs() > 1.0 && (p[1] - q[1]).abs() > 1.0).unwrap();
        let moved = move_edge(&one, slant, 5.0).unwrap();
        assert!(twice_area(&moved) > twice_area(&one));
        // the other corners stay where they were
        assert_eq!(moved.iter().filter(|p| one.contains(p)).count(), one.len() - 2);
    }
}

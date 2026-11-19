//! Comic panels: a page cut into panels as a comic's or a manga's is. Each panel is a group: at
//! its bottom its frame, a locked layer keeping the panel's shape (a convex polygon), drawn as
//! white paper with its border as the layer's outline (its style, so its width and colour are
//! changed as any outline's are); above the frame the panel's art, clipped to it. The shape is
//! kept, not only drawn, so a panel can be cut again: a line dragged across panels cuts each in
//! two, with a gap (the gutter) between the halves.

use crate::document::{Document, LayerEffect, LayerId, LayerStyle};
use crate::geom::IRect;

/// A point on the canvas, canvas pixels.
pub type Point = [f32; 2];

/// A panel's frame: the shape its art is clipped to.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PanelFrame {
    /// The shape's corners, canvas pixels, a convex polygon wound so its area (shoelace) is
    /// positive.
    pub points: Vec<Point>,
}

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
            border: (side * 0.003).clamp(1.0, crate::document::STYLE_MOST_SIZE).round(),
        }
    }
}

/// The style a frame's border is: an outline, black, `width` canvas pixels, crisp.
pub fn border_style(width: f32) -> LayerStyle {
    LayerStyle {
        outline: LayerEffect {
            on: true,
            color: [0.0; 3],
            opacity: 1.0,
            size: width.clamp(0.0, crate::document::STYLE_MOST_SIZE),
            softness: 0.0,
            ..LayerEffect::default()
        },
        ..LayerStyle::default()
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

/// How far `r` is inside the edges (negative outside): the nearest edge's distance, so the
/// frame's corners stay sharp.
fn inside(edges: &[(Point, Point)], r: Point) -> f32 {
    edges.iter().map(|(p, n)| (r[0] - p[0]) * n[0] + (r[1] - p[1]) * n[1]).fold(f32::INFINITY, f32::min)
}

/// Whether point `r` is inside the shape.
pub fn contains(points: &[Point], r: Point) -> bool {
    points.len() >= 3 && inside(&edges(points), r) >= 0.0
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

/// The pixels the shape covers, all of each it touches, on a canvas `width` × `height`; none if it
/// is off it.
pub fn shape_rect(points: &[Point], width: u32, height: u32) -> Option<IRect> {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in points {
        (x0, y0, x1, y1) = (x0.min(p[0]), y0.min(p[1]), x1.max(p[0]), y1.max(p[1]));
    }
    if !(x0.is_finite() && y0.is_finite() && x1.is_finite() && y1.is_finite()) {
        return None;
    }
    let rect = IRect::new(x0.floor() as i32 - 1, y0.floor() as i32 - 1, x1.ceil() as i32 + 1, y1.ceil() as i32 + 1)
        .intersect(IRect::from_size(width, height));
    (!rect.is_empty()).then_some(rect)
}

/// The frame drawn within `rect`: white where the shape is, its edge smoothed over a pixel,
/// clear elsewhere (premultiplied RGBA8, `rect`'s rows).
pub fn draw_frame(points: &[Point], rect: IRect) -> Vec<u8> {
    let (w, h) = (rect.width() as usize, rect.height() as usize);
    let mut out = vec![0u8; w * h * 4];
    if points.len() < 3 {
        return out;
    }
    let edges = edges(points);
    for (row, line) in out.chunks_exact_mut(w * 4).enumerate() {
        let y = (rect.y0 + row as i32) as f32 + 0.5;
        for (col, px) in line.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = (rect.x0 + col as i32) as f32 + 0.5;
            let cover = (inside(&edges, [x, y]) + 0.5).clamp(0.0, 1.0);
            px.fill((cover * 255.0).round() as u8);
        }
    }
    out
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
    pub edges: Vec<(LayerId, usize, f32)>,
}

/// The gutter at `p` among `frames` (each frame and its shape): the edge nearest `p`, within
/// `reach` (canvas pixels), with every edge along the same line facing the same way, and the
/// edges facing them across the nearest gap, no wider than `most_gap`. None if no edge is in reach.
pub fn gutter_at(frames: &[(LayerId, Vec<Point>)], p: Point, reach: f32, most_gap: f32) -> Option<Gutter> {
    // each edge: its frame, index, a corner, the unit normal out, and how far p is from it
    let all: Vec<(LayerId, usize, Point, Point, f32)> = frames
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
    let mut edges: Vec<(LayerId, usize, f32)> = all
        .iter()
        .filter(|e| facing(e.3) > 0.9995 && (along(e.2) - line).abs() < 1.0)
        .map(|e| (e.0, e.1, 1.0))
        .collect();
    // across the gap: the nearest edges facing back, beyond this line
    let back = |e: &&(LayerId, usize, Point, Point, f32)| {
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
        frames: &[(LayerId, Vec<Point>)],
        by: f32,
        width: u32,
        height: u32,
    ) -> Option<Vec<(LayerId, Vec<Point>)>> {
        let mut out: Vec<(LayerId, Vec<Point>)> = Vec::new();
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
        frames: &[(LayerId, Vec<Point>)],
        by: f32,
        width: u32,
        height: u32,
    ) -> (f32, Vec<(LayerId, Vec<Point>)>) {
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

/// A cut made by [`Document::cut_panels`]: the frame cut, its shape before, and the new panel's
/// frame made of the other half.
#[derive(Clone, Debug, PartialEq)]
pub struct PanelCut {
    pub frame: LayerId,
    pub was: Vec<Point>,
    pub made: LayerId,
}

/// A frame to cut: its id, its shape, the half it keeps and the other.
type Plan = (LayerId, Vec<Point>, Vec<Point>, Vec<Point>);

/// The layers a new panel is made of.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NewPanel {
    pub group: LayerId,
    pub frame: LayerId,
    pub art: LayerId,
}

impl Document {
    /// The panels' frames, bottom first, with their shapes.
    pub fn panel_frames(&self) -> Vec<(LayerId, &PanelFrame)> {
        self.layers().iter().filter_map(|l| l.panel.as_ref().map(|p| (l.id, p))).collect()
    }

    /// Gives frame `id` the shape `points` (its pixels are the caller's to draw); false if it isn't a
    /// frame.
    pub fn set_panel_shape(&mut self, id: LayerId, points: Vec<Point>) -> bool {
        let Some(panel) = self.layer_mut(id).and_then(|l| l.panel.as_mut()) else { return false };
        panel.points = tidied(points);
        true
    }

    /// The frame of panel `group` (its bottom layer, keeping the panel's shape), if it is a panel.
    pub fn frame_of_panel(&self, group: LayerId) -> Option<LayerId> {
        let first = *self.members(Some(group)).first()?;
        self.layer(first).filter(|l| l.panel.is_some()).map(|l| l.id)
    }

    /// The panel `id` is in: the group directly holding it, if that is a panel.
    pub fn panel_of(&self, id: LayerId) -> Option<LayerId> {
        let group = self.group_of(id)?;
        self.frame_of_panel(group).map(|_| group)
    }

    /// A panel of `shape` above the current layer (in its group), named `name`, its border
    /// `style`: a group holding the frame and an empty layer clipped to it, which becomes current.
    fn add_panel(&mut self, shape: Vec<Point>, style: LayerStyle, name: &str) -> Option<NewPanel> {
        let frame = self.add_layer();
        self.rename(frame, name);
        let group = self.wrap_in_group(frame, "Frame")?;
        if let Some(l) = self.layer_mut(frame) {
            l.panel = Some(PanelFrame { points: tidied(shape) });
            l.style = Some(style);
        }
        self.set_current(frame);
        // clipped to the frame as it is added (see Document::add_layer)
        let art = self.add_layer();
        self.rename(art, "Art");
        if let Some(l) = self.layer_mut(frame) {
            l.locked = true;
        }
        Some(NewPanel { group, frame, art })
    }

    /// Makes the canvas a comic page: one panel inside a margin, on top of the stack, its art
    /// layer current. None if it has panels already or no room for them.
    pub fn add_comic_page(&mut self) -> Option<NewPanel> {
        if !self.panel_frames().is_empty() || !self.has_room(2) {
            return None;
        }
        let measures = PageMeasures::for_canvas(self.width(), self.height());
        let top = *self.members(None).last()?;
        self.set_current(top);
        let shape = page_shape(self.width(), self.height(), measures.margin);
        self.add_panel(shape, border_style(measures.border), "Panel 1")
    }

    /// Cuts every panel the line from `a` to `b` passes through in two along it (made straight
    /// across or down when near it), `column_gap` apart for a cut down the page and `row_gap` for
    /// one across. Each keeps its larger half, its art and its place; a new panel (with an empty
    /// layer, the same border) takes the other just above it. The current layer stays. The cuts
    /// made, bottom first; none for a line through no panel, or no room for the new ones.
    pub fn cut_panels(&mut self, a: Point, b: Point, column_gap: f32, row_gap: f32) -> Vec<PanelCut> {
        let (a, b) = snapped(a, b);
        let gap = gap_for(a, b, column_gap, row_gap);
        let plans: Vec<Plan> = self
            .panel_frames()
            .into_iter()
            .filter(|(id, frame)| self.panel_of(*id).is_some() && crosses(&frame.points, a, b))
            .filter_map(|(id, frame)| {
                let (keep, other) = cut(&frame.points, a, b, gap)?;
                Some((id, frame.points.clone(), keep, other))
            })
            .collect();
        if plans.is_empty() || !self.has_room(2 * plans.len()) {
            return Vec::new();
        }
        let current = self.current();
        let mut count = self.panel_frames().len();
        let mut cuts = Vec::with_capacity(plans.len());
        for (frame, was, keep, other) in plans {
            let Some(group) = self.panel_of(frame) else { continue };
            let style = self.layer(frame).and_then(|l| l.style).unwrap_or_else(|| border_style(1.0));
            if let Some(p) = self.layer_mut(frame).and_then(|l| l.panel.as_mut()) {
                p.points = keep;
            }
            count += 1;
            self.set_current(group);
            if let Some(made) = self.add_panel(other, style, &format!("Panel {count}")) {
                cuts.push(PanelCut { frame, was, made: made.frame });
            }
        }
        self.set_current(current);
        cuts
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LayerKind;

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

    #[test]
    fn a_frame_is_drawn_white_inside_with_a_smoothed_edge() {
        let shape = vec![[2.0, 2.0], [6.5, 2.0], [6.5, 6.0], [2.0, 6.0]];
        let rect = shape_rect(&shape, 10, 10).unwrap();
        assert_eq!(rect, IRect::new(1, 1, 8, 7));
        let px = draw_frame(&shape, rect);
        let at = |x: i32, y: i32| px[((y - 1) as usize * 7 + (x - 1) as usize) * 4];
        assert_eq!((at(3, 3), at(1, 1), at(7, 3)), (255, 0, 0));
        // the column half covered
        assert_eq!(at(6, 3), 128);
    }

    #[test]
    fn a_page_is_one_panel_whose_art_is_clipped_to_its_frame() {
        let mut doc = Document::new(1000, 1500);
        let base = doc.current();
        let page = doc.add_comic_page().unwrap();
        assert!(doc.add_comic_page().is_none());
        assert_eq!(doc.current(), page.art);
        assert_eq!(doc.layer(page.group).unwrap().kind, LayerKind::Group);
        assert_eq!(doc.members(Some(page.group)), vec![page.frame, page.art]);
        assert_eq!(doc.members(None), vec![base, page.group]);
        assert_eq!(doc.clip_base(page.art), Some(page.frame));
        let frame = doc.layer(page.frame).unwrap();
        assert!(frame.locked && frame.shown_style().is_some());
        assert_eq!(frame.panel.as_ref().unwrap().points[0], [70.0, 70.0]);
        // a layer added in the panel is clipped to its frame too
        let more = doc.add_layer();
        assert_eq!((doc.panel_of(more), doc.clip_base(more)), (Some(page.group), Some(page.frame)));
    }

    #[test]
    fn a_line_across_a_page_cuts_its_panel_and_then_both_of_the_new_ones() {
        let mut doc = Document::new(1000, 1000);
        let page = doc.add_comic_page().unwrap();
        // across the page: a row gap
        let cuts = doc.cut_panels([0.0, 400.0], [1000.0, 401.0], 20.0, 30.0);
        assert_eq!(cuts.len(), 1);
        assert_eq!(cuts[0].frame, page.frame);
        assert_eq!(doc.current(), page.art);
        let made = cuts[0].made;
        let new_group = doc.panel_of(made).unwrap();
        assert_eq!(doc.members(Some(new_group)).len(), 2);
        let ys = |id: LayerId, doc: &Document| {
            let p = &doc.layer(id).unwrap().panel.as_ref().unwrap().points;
            p.iter().map(|p| p[1]).fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)))
        };
        // the line made straight about its middle (400.5), the gap split round it
        assert_eq!(ys(page.frame, &doc), (415.5, 930.0));
        assert_eq!(ys(made, &doc), (70.0, 385.5));
        // down the whole page: both panels cut, with the column gap
        let cuts = doc.cut_panels([500.0, 0.0], [500.0, 1000.0], 20.0, 30.0);
        assert_eq!(cuts.len(), 2);
        assert_eq!(doc.panel_frames().len(), 4);
        // a line through no panel cuts nothing
        assert!(doc.cut_panels([1.0, 1.0], [2.0, 900.0], 20.0, 30.0).is_empty());
    }

    /// A 2 × 2 page on a 1000 square: cut down the middle (a 20 gap), then across (a 30 gap).
    fn grid() -> (Document, Vec<(LayerId, Vec<Point>)>) {
        let mut doc = Document::new(1000, 1000);
        doc.add_comic_page().unwrap();
        doc.cut_panels([500.0, 0.0], [500.0, 1000.0], 20.0, 30.0);
        doc.cut_panels([0.0, 500.0], [1000.0, 500.0], 20.0, 30.0);
        let frames = doc.panel_frames().into_iter().map(|(id, f)| (id, f.points.clone())).collect();
        (doc, frames)
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

    #[test]
    fn panels_are_kept_in_the_file() {
        let mut doc = Document::new(400, 400);
        let page = doc.add_comic_page().unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.layer(page.frame).unwrap().panel, doc.layer(page.frame).unwrap().panel);
    }
}

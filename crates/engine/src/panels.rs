//! Comic panels as the document keeps them: each panel is a group, at its bottom its frame, a
//! locked layer keeping the panel's shape (a convex polygon), drawn as white paper with its border
//! as the layer's outline (its style, so its width and colour are changed as any outline's are);
//! above the frame the panel's art, clipped to it. The shape is kept, not only drawn, so a plugin's
//! tool can cut and move panels again (Comic Panels; see `plugin_tools.rs` in the core): here is
//! how a frame is drawn and how a panel is added; where cuts and gutters go is the plugin's.

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

    /// A panel of `shape` above layer `above` (in its group; none: on top of the stack), named
    /// `name`, its border `style`: a group holding the frame and an empty layer clipped to it, which
    /// becomes current. None if there is no room for it, or no such layer.
    pub fn add_panel(
        &mut self,
        above: Option<LayerId>,
        shape: Vec<Point>,
        style: LayerStyle,
        name: &str,
    ) -> Option<NewPanel> {
        let above = match above {
            Some(id) => self.layer(id).map(|l| l.id)?,
            None => *self.members(None).last()?,
        };
        if !self.has_room(2) {
            return None;
        }
        self.set_current(above);
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::LayerKind;

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

    fn square(at: f32, side: f32) -> Vec<Point> {
        vec![[at, at], [at + side, at], [at + side, at + side], [at, at + side]]
    }

    #[test]
    fn a_panel_is_a_group_whose_art_is_clipped_to_its_frame() {
        let mut doc = Document::new(1000, 1500);
        let base = doc.current();
        let page = doc.add_panel(None, square(70.0, 500.0), border_style(4.0), "Panel 1").unwrap();
        assert_eq!(doc.current(), page.art);
        assert_eq!(doc.layer(page.group).unwrap().kind, LayerKind::Group);
        assert_eq!(doc.members(Some(page.group)), vec![page.frame, page.art]);
        assert_eq!(doc.members(None), vec![base, page.group]);
        assert_eq!(doc.clip_base(page.art), Some(page.frame));
        let frame = doc.layer(page.frame).unwrap();
        assert!(frame.locked && frame.shown_style().is_some());
        assert_eq!(frame.panel.as_ref().unwrap().points[0], [70.0, 70.0]);
        assert_eq!(doc.panel_frames().len(), 1);
        // a layer added in the panel is clipped to its frame too
        let more = doc.add_layer();
        assert_eq!((doc.panel_of(more), doc.clip_base(more)), (Some(page.group), Some(page.frame)));
        // another above it, in the stack beside it
        let next = doc.add_panel(Some(page.group), square(600.0, 100.0), border_style(4.0), "Panel 2").unwrap();
        assert_eq!(doc.members(None), vec![base, page.group, next.group]);
        assert!(doc.add_panel(Some(LayerId(9999)), square(0.0, 10.0), border_style(1.0), "None").is_none());
        // a shape given is wound as frames are
        assert!(doc.set_panel_shape(next.frame, vec![[0.0, 0.0], [0.0, 10.0], [10.0, 10.0], [10.0, 0.0]]));
        assert!(twice_area(&doc.layer(next.frame).unwrap().panel.as_ref().unwrap().points) > 0.0);
        assert!(!doc.set_panel_shape(next.art, square(0.0, 10.0)));
    }

    #[test]
    fn panels_are_kept_in_the_file() {
        let mut doc = Document::new(400, 400);
        let page = doc.add_panel(None, square(28.0, 344.0), border_style(1.0), "Panel 1").unwrap();
        let json = serde_json::to_string(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(back.layer(page.frame).unwrap().panel, doc.layer(page.frame).unwrap().panel);
    }
}

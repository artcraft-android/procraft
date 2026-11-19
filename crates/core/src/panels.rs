//! Comic panels (see [`procraft_engine::panels`]): a canvas made a comic page, one panel inside its
//! margin, and panels cut in two by a line dragged across them, each change one undo step with the
//! frames' pixels drawn from their shapes.

use procraft_engine::document::{Document, LayerId};
use procraft_engine::geom::{Affine, IRect, Vec2};
use procraft_engine::panels::{self, PageMeasures, Point};
use procraft_engine::view::View;

use crate::CanvasSession;

/// A panel as the panel tool shows it: its group, its frame (the layer keeping its shape) and the
/// shape's corners, canvas pixels, x and y by turns.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct ComicPanel {
    pub group: u64,
    pub frame: u64,
    pub points: Vec<f32>,
}

/// The gaps between panels a cut leaves, canvas pixels: between panels side by side (a cut down
/// the page) and between rows (a cut across it).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct PanelGaps {
    pub column: f32,
    pub row: f32,
}

/// The frame `points` drawn over `rect` (where it was, and where it is now), on a canvas `width` ×
/// `height`.
fn frame_part(points: &[Point], was: Option<&[Point]>, width: u32, height: u32) -> Option<(IRect, Vec<u8>)> {
    let now = panels::shape_rect(points, width, height);
    let before = was.and_then(|w| panels::shape_rect(w, width, height));
    let rect = match (now, before) {
        (Some(a), Some(b)) => a.union(b),
        (a, b) => a.or(b)?,
    };
    Some((rect, panels::draw_frame(points, rect)))
}

/// The shape of frame `id` in `doc`.
fn shape(doc: &Document, id: LayerId) -> Option<Vec<Point>> {
    doc.layer(id)?.panel.as_ref().map(|p| p.points.clone())
}

/// How far from a gutter, window pixels, a touch takes it.
const GUTTER_REACH: f32 = 36.0;

/// A gutter being dragged: what it moves, where the drag began (canvas pixels), the frames it
/// moves as they were, the document as it was, and the frames as last drawn.
pub(crate) struct GutterDrag {
    gutter: panels::Gutter,
    start: Point,
    was: Vec<(LayerId, Vec<Point>)>,
    before: Document,
    shown: Vec<(LayerId, Vec<Point>)>,
}

/// Where a frame changed going from shape `from` to `to` (on a canvas `width` × `height`): around
/// the corners that moved, as an edge moved takes only the two it joins. None if none did.
fn changed_rect(from: &[Point], to: &[Point], width: u32, height: u32) -> Option<IRect> {
    let corners: Vec<Point> = if from.len() == to.len() {
        from.iter().zip(to).filter(|(a, b)| a != b).flat_map(|(a, b)| [*a, *b]).collect()
    } else {
        from.iter().chain(to).copied().collect()
    };
    if corners.is_empty() {
        return None;
    }
    panels::shape_rect(&corners, width, height)
}

/// The shape of `id` among `shapes`.
fn shape_of(shapes: &[(LayerId, Vec<Point>)], id: LayerId) -> Option<&[Point]> {
    shapes.iter().find(|(f, _)| *f == id).map(|(_, s)| s.as_slice())
}

#[uniffi::export]
impl CanvasSession {
    /// Makes the canvas a comic page, as one undo step: one panel inside a margin on top of the
    /// stack (a group of its frame, white with a black border, and a layer clipped to it, which
    /// becomes current). The layer's id; none if the canvas has panels already, is animated, or
    /// has no room.
    pub fn add_comic_page(&self) -> Option<u64> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.doc.timeline.is_some() || st.doc.animation.is_some() {
            return None;
        }
        st.finish_stroke();
        st.settle();
        let before = st.doc.clone();
        let page = st.doc.add_comic_page()?;
        let (w, h) = (st.doc.width(), st.doc.height());
        let part = shape(&st.doc, page.frame).and_then(|s| frame_part(&s, None, w, h));
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("comic page");
            let parts = part.map(|(rect, px)| (page.frame, rect, px)).into_iter().collect();
            canvas.write_layer_parts_and_document(parts, before);
        }
        st.revision += 1;
        self.shared.frame(st);
        Some(page.art.0)
    }

    /// The canvas's panels, bottom first.
    pub fn comic_panels(&self) -> Vec<ComicPanel> {
        let st = self.shared.lock();
        let doc = &st.doc;
        doc.panel_frames()
            .into_iter()
            .filter_map(|(frame, p)| {
                let group = doc.panel_of(frame)?;
                let points = p.points.iter().flatten().copied().collect();
                Some(ComicPanel { group: group.0, frame: frame.0, points })
            })
            .collect()
    }

    /// The gaps cuts leave on this canvas unless the app is told others: about 2% of its shorter
    /// side between columns, 3.5% between rows.
    pub fn panel_gaps(&self) -> PanelGaps {
        let st = self.shared.lock();
        let m = PageMeasures::for_canvas(st.doc.width(), st.doc.height());
        PanelGaps { column: m.column_gap, row: m.row_gap }
    }

    /// Fits the whole canvas between `left`, `top`, `right` and `bottom` (window pixels), as the
    /// panel tool shows the page under its header.
    pub fn panel_tool_fit(&self, left: f32, top: f32, right: f32, bottom: f32) {
        if ![left, top, right, bottom].iter().all(|v| v.is_finite()) {
            return;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.view_motion = None;
        let size = ((right - left).max(1.0) as u32, (bottom - top).max(1.0) as u32);
        let fitted = View::fit((st.doc.width(), st.doc.height()), size);
        st.view = Some(View::from_affine(fitted.to_screen().then(Affine::translate(Vec2::new(left, top)))));
        self.shared.frame(st);
    }

    /// The line from (`ax`, `ay`) to (`bx`, `by`) (window pixels) as a cut would make it: straight
    /// across or down the canvas when near it. Its ends on the screen, x and y by turns, for the
    /// panel tool to show; none off the canvas's view.
    pub fn panel_cut_on_screen(&self, ax: f32, ay: f32, bx: f32, by: f32) -> Vec<f32> {
        let st = self.shared.lock();
        let Some(view) = st.view else { return Vec::new() };
        let canvas = |x: f32, y: f32| view.screen_to_canvas(Vec2::new(x, y)).filter(|p| p.is_finite());
        let (Some(a), Some(b)) = (canvas(ax, ay), canvas(bx, by)) else { return Vec::new() };
        let (a, b) = panels::snapped([a.x, a.y], [b.x, b.y]);
        let to_screen = view.to_screen();
        let (a, b) = (to_screen.apply(Vec2::new(a[0], a[1])), to_screen.apply(Vec2::new(b[0], b[1])));
        vec![a.x, a.y, b.x, b.y]
    }

    /// Takes the gutter at (`x`, `y`) (window pixels) to drag, with the panels' edges along it on
    /// both sides, or the page's edge there (its margin); false if none is in reach.
    pub fn gutter_begin(&self, x: f32, y: f32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let Some(view) = st.view else { return false };
        let Some(p) = view.screen_to_canvas(Vec2::new(x, y)).filter(|p| p.is_finite()) else { return false };
        let scale = view.to_screen().scale_factor().max(1e-6);
        let doc = &st.doc;
        let frames: Vec<(LayerId, Vec<Point>)> = doc
            .panel_frames()
            .into_iter()
            .filter(|(id, _)| doc.panel_of(*id).is_some())
            .map(|(id, f)| (id, f.points.clone()))
            .collect();
        let most_gap = doc.width().max(doc.height()) as f32 / 4.0;
        let Some(gutter) = panels::gutter_at(&frames, [p.x, p.y], GUTTER_REACH / scale, most_gap) else { return false };
        let was: Vec<(LayerId, Vec<Point>)> =
            frames.into_iter().filter(|(id, _)| gutter.edges.iter().any(|e| e.0 == *id)).collect();
        st.gutter_drag =
            Some(GutterDrag { gutter, start: [p.x, p.y], before: st.doc.clone(), shown: was.clone(), was });
        true
    }

    /// Moves the gutter taken to follow (`x`, `y`) (window pixels) across it, as far as its panels
    /// fit: their frames change at once, the drag one undo step when it ends. False with none
    /// taken.
    pub fn gutter_drag(&self, x: f32, y: f32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(mut drag) = st.gutter_drag.take() else { return false };
        let at = st.view.and_then(|v| v.screen_to_canvas(Vec2::new(x, y))).filter(|p| p.is_finite());
        if let Some(p) = at {
            let n = drag.gutter.normal;
            let by = (p.x - drag.start[0]) * n[0] + (p.y - drag.start[1]) * n[1];
            let (w, h) = (st.doc.width(), st.doc.height());
            let (_, shapes) = drag.gutter.moved(&drag.was, by, w, h);
            for (id, shape) in &shapes {
                st.doc.set_panel_shape(*id, shape.clone());
                let rect = shape_of(&drag.shown, *id).and_then(|prev| changed_rect(prev, shape, w, h));
                if let (Some(rect), Some(canvas)) = (rect, st.canvas.as_mut()) {
                    let px = panels::draw_frame(shape, rect);
                    canvas.put_layer_region(*id, rect.x0 as u32, rect.y0 as u32, rect.width(), rect.height(), px);
                }
            }
            if !shapes.is_empty() {
                drag.shown = shapes;
            }
        }
        st.gutter_drag = Some(drag);
        self.shared.frame(st);
        true
    }

    /// Ends dragging a gutter: `keep` makes the move one undo step; otherwise the panels go back.
    pub fn gutter_end(&self, keep: bool) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(drag) = st.gutter_drag.take() else { return };
        let (w, h) = (st.doc.width(), st.doc.height());
        let keep = keep && drag.shown != drag.was;
        let mut parts = Vec::new();
        if let Some(canvas) = st.canvas.as_mut() {
            for (id, was) in &drag.was {
                let Some(now) = shape_of(&drag.shown, *id) else { continue };
                let Some(rect) = changed_rect(was, now, w, h) else { continue };
                // the pixels as they were, for the step to go back to (or to stay)
                let px = panels::draw_frame(was, rect);
                canvas.put_layer_region(*id, rect.x0 as u32, rect.y0 as u32, rect.width(), rect.height(), px);
                if keep {
                    parts.push((*id, rect, panels::draw_frame(now, rect)));
                }
            }
            if keep {
                canvas.name_next_step("move gutter");
                canvas.write_layer_parts_and_document(parts, drag.before);
            } else {
                canvas.invalidate();
            }
        }
        if keep {
            st.revision += 1;
        } else {
            for (id, was) in drag.was {
                st.doc.set_panel_shape(id, was);
            }
        }
        self.shared.frame(st);
    }

    /// [`Self::cut_panels`] along a line dragged on the screen (window pixels), with this canvas's
    /// gaps ([`Self::panel_gaps`]).
    pub fn cut_panels_on_screen(&self, ax: f32, ay: f32, bx: f32, by: f32) -> u32 {
        let ends = {
            let st = self.shared.lock();
            let Some(view) = st.view else { return 0 };
            let canvas = |x: f32, y: f32| view.screen_to_canvas(Vec2::new(x, y)).filter(|p| p.is_finite());
            canvas(ax, ay).zip(canvas(bx, by))
        };
        let Some((a, b)) = ends else { return 0 };
        let gaps = self.panel_gaps();
        self.cut_panels(a.x, a.y, b.x, b.y, gaps.column, gaps.row)
    }

    /// Cuts every panel the line from (`ax`, `ay`) to (`bx`, `by`) (canvas pixels) passes through
    /// in two, as one undo step: `column_gap` apart for a cut down the page, `row_gap` for one
    /// across (see [`Self::panel_gaps`]). Each panel keeps its larger half and its art; a new
    /// panel takes the other. How many were cut.
    pub fn cut_panels(&self, ax: f32, ay: f32, bx: f32, by: f32, column_gap: f32, row_gap: f32) -> u32 {
        let finite = [ax, ay, bx, by, column_gap, row_gap].iter().all(|v| v.is_finite());
        if !finite {
            return 0;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let before = st.doc.clone();
        let (w, h) = (st.doc.width(), st.doc.height());
        let most = w.max(h) as f32 / 4.0;
        let cuts = st.doc.cut_panels([ax, ay], [bx, by], column_gap.clamp(0.0, most), row_gap.clamp(0.0, most));
        if cuts.is_empty() {
            return 0;
        }
        let mut parts = Vec::with_capacity(cuts.len() * 2);
        for cut in &cuts {
            let kept = shape(&st.doc, cut.frame).and_then(|s| frame_part(&s, Some(&cut.was), w, h));
            let made = shape(&st.doc, cut.made).and_then(|s| frame_part(&s, None, w, h));
            parts.extend(kept.map(|(r, px)| (cut.frame, r, px)));
            parts.extend(made.map(|(r, px)| (cut.made, r, px)));
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step("cut panels");
            canvas.write_layer_parts_and_document(parts, before);
        }
        st.revision += 1;
        self.shared.frame(st);
        u32::try_from(cuts.len()).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use procraft_engine::view::View;
    use procraft_gpu::Gpu;

    use crate::CanvasSession;

    fn session(side: u32) -> Option<std::sync::Arc<CanvasSession>> {
        let gpu = match Gpu::headless() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipped: {e}");
                return None;
            }
        };
        let s = CanvasSession::new(side, side);
        {
            let mut st = s.shared.lock();
            st.window_size = (side, side);
            st.view = Some(View::fit((side, side), (side, side)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        Some(s)
    }

    fn pixel(s: &CanvasSession, x: u32, y: u32) -> [u8; 4] {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        let px = canvas.read_composite(gpu).unwrap();
        let i = ((y * st.doc.width() + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    fn layer_pixel(s: &CanvasSession, id: u64, x: u32, y: u32) -> [u8; 4] {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.render(gpu, &mut st.doc, None);
        let px = canvas.read_layer(gpu, procraft_engine::document::LayerId(id)).unwrap();
        let i = ((y * st.doc.width() + x) * 4) as usize;
        [px[i], px[i + 1], px[i + 2], px[i + 3]]
    }

    #[test]
    fn a_page_s_panel_is_white_with_a_black_border_and_cuts_undo_in_one_step() {
        let Some(s) = session(400) else { return };
        // the background shown grey, so the panel's white tells apart
        s.set_background_color(0.5, 0.5, 0.5, true);
        let art = s.add_comic_page().unwrap();
        assert!(s.add_comic_page().is_none());
        let panels = s.comic_panels();
        assert_eq!(panels.len(), 1);
        assert_eq!(panels[0].points[..2], [28.0, 28.0]);
        assert_eq!(s.layers().iter().find(|l| l.current).map(|l| l.id), Some(art));
        // inside white, the border just outside the frame black, the margin the background
        assert_eq!(pixel(&s, 200, 200)[..3], [255, 255, 255]);
        let edge = pixel(&s, 27, 200);
        assert!(edge[0] < 100, "{edge:?}");
        assert!(pixel(&s, 10, 200)[0].abs_diff(128) < 4);
        // a cut down the middle: a gap of the background between two panels
        let gaps = s.panel_gaps();
        assert_eq!(s.cut_panels(200.0, 0.0, 201.0, 400.0, gaps.column, gaps.row), 1);
        let panels = s.comic_panels();
        assert_eq!(panels.len(), 2);
        assert!(pixel(&s, 200, 200)[0].abs_diff(128) < 4);
        assert_eq!(pixel(&s, 100, 200)[..3], [255, 255, 255]);
        assert_eq!(pixel(&s, 300, 200)[..3], [255, 255, 255]);
        // the cut frame cleared where it no longer is
        assert_eq!(layer_pixel(&s, panels[0].frame, 200, 200)[3], 0);
        // missing every panel cuts nothing
        assert_eq!(s.cut_panels(1.0, 1.0, 2.0, 2.0, 8.0, 8.0), 0);
        s.undo();
        assert_eq!(s.comic_panels().len(), 1);
        assert_eq!(pixel(&s, 200, 200)[..3], [255, 255, 255]);
        s.undo();
        assert!(s.comic_panels().is_empty());
        assert!(pixel(&s, 200, 200)[0].abs_diff(128) < 4);
    }

    #[test]
    fn the_tool_works_in_window_pixels_and_straightens_its_line() {
        let Some(s) = session(300) else { return };
        assert_eq!(s.cut_panels_on_screen(150.0, 0.0, 150.0, 300.0), 0, "no page yet");
        s.add_comic_page().unwrap();
        let line = s.panel_cut_on_screen(10.0, 100.0, 290.0, 104.0);
        assert!((line[1] - line[3]).abs() < 0.01, "{line:?}");
        assert_eq!(s.cut_panels_on_screen(10.0, 100.0, 290.0, 104.0), 1);
        assert_eq!(s.comic_panels().len(), 2);
    }

    /// Canvas point (`x`, `y`) on the screen.
    fn on_screen(s: &CanvasSession, x: f32, y: f32) -> (f32, f32) {
        let view = s.shared.lock().view.unwrap();
        let p = view.to_screen().apply(procraft_engine::geom::Vec2::new(x, y));
        (p.x, p.y)
    }

    #[test]
    fn a_gutter_dragged_moves_both_panels_edges_live_and_undoes_in_one_step() {
        let Some(s) = session(300) else { return };
        s.set_background_color(0.5, 0.5, 0.5, true);
        s.add_comic_page().unwrap();
        // two panels, 21..147 and 153..279 across
        let gaps = s.panel_gaps();
        assert_eq!(s.cut_panels(150.0, 0.0, 150.0, 300.0, gaps.column, gaps.row), 1);
        let grey = |px: [u8; 4]| px[0].abs_diff(128) < 4;
        assert!(grey(pixel(&s, 150, 150)));
        // nothing to take in the middle of a panel
        let (mx, my) = on_screen(&s, 80.0, 150.0);
        assert!(!s.gutter_begin(mx, my));
        let (x0, y0) = on_screen(&s, 150.0, 150.0);
        let (x1, _) = on_screen(&s, 200.0, 150.0);
        assert!(s.gutter_begin(x0, y0));
        assert!(s.gutter_drag(x1, y0));
        // shown as it moves, the gap now at 200
        assert_eq!(pixel(&s, 150, 150)[..3], [255, 255, 255]);
        assert!(grey(pixel(&s, 200, 150)));
        s.gutter_end(true);
        let edges: Vec<(f32, f32)> = s
            .comic_panels()
            .iter()
            .map(|p| p.points.chunks(2).map(|c| c[0]).fold((f32::MAX, f32::MIN), |(lo, hi), x| (lo.min(x), hi.max(x))))
            .collect();
        assert!(edges.iter().any(|e| (e.1 - 197.0).abs() < 0.5), "{edges:?}");
        assert!(edges.iter().any(|e| (e.0 - 203.0).abs() < 0.5), "{edges:?}");
        assert!(grey(pixel(&s, 200, 150)));
        assert_eq!(pixel(&s, 150, 150)[..3], [255, 255, 255]);
        s.undo();
        assert!(grey(pixel(&s, 150, 150)));
        assert_eq!(pixel(&s, 200, 150)[..3], [255, 255, 255]);
        s.redo();
        assert!(grey(pixel(&s, 200, 150)));
        // a drag let go of without keeping it goes back
        assert!(s.gutter_begin(on_screen(&s, 200.0, 150.0).0, y0));
        s.gutter_drag(x0, y0);
        s.gutter_end(false);
        assert!(grey(pixel(&s, 200, 150)));
        assert_eq!(pixel(&s, 150, 150)[..3], [255, 255, 255]);
    }

    #[test]
    fn art_in_a_panel_shows_only_inside_its_frame() {
        let Some(s) = session(200) else { return };
        let art = s.add_comic_page().unwrap();
        // the whole art layer filled red: only the panel shows it
        s.set_color(1.0, 0.0, 0.0);
        assert!(s.fill_layer(art));
        let inside = pixel(&s, 100, 100);
        assert!(inside[0] > 240 && inside[1] < 20, "{inside:?}");
        let margin = pixel(&s, 3, 100);
        assert!(margin[0] > 240 && margin[1] > 240, "{margin:?}");
    }
}

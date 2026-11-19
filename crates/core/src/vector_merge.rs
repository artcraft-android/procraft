//! Vector layers merged as vector layers: Merge Down, and Merge with layers picked, keep the
//! strokes and fills of plain vector layers as strokes and fills on the layer they merge into
//! (see [`VectorContent::merged`]), instead of drawing them into its pixels.
//!
//! Only layers whose look is all in what they hold merge so: vector layers in the Normal blend
//! mode, with no mask and no layer style, none above the lowest clipped. Any other merge draws
//! them into pixels as before.

use procraft_engine::document::{BlendMode, Document, LayerId, LayerKind};
use procraft_engine::vector::{Merging, VectorContent};

use crate::State;

impl State {
    /// What vector layer `lower` holds with `uppers` (bottom first) merged into it, if all of them
    /// can merge as vector layers; none if any can't, when the merge draws them into pixels. A
    /// hidden layer gives nothing (the merge keeps what showed).
    pub(crate) fn vector_merged(&self, lower: LayerId, uppers: &[LayerId]) -> Option<VectorContent> {
        let doc = &self.doc;
        let plain = |id: LayerId, lowest: bool| {
            doc.layer(id).is_some_and(|l| {
                l.kind == LayerKind::Paint
                    && l.vector.is_some()
                    && l.text.is_none()
                    && l.plugin.is_none()
                    && !l.is_adjustment()
                    && l.blend == BlendMode::Normal
                    && (lowest || !l.clipped)
                    && l.style.as_ref().is_none_or(|s| !s.shows())
                    && doc.mask_of(id).is_none()
            })
        };
        if uppers.is_empty() || !plain(lower, true) || !uppers.iter().all(|u| plain(*u, false)) {
            return None;
        }
        let empty = VectorContent::default();
        let layers: Vec<Merging<'_>> = std::iter::once(lower)
            .chain(uppers.iter().copied())
            .filter_map(|id| doc.layer(id))
            .map(|l| match l.vector.as_deref() {
                Some(content) if l.visible => Merging { content, opacity: l.opacity },
                _ => Merging { content: &empty, opacity: 1.0 },
            })
            .collect();
        Some(VectorContent::merged(&layers))
    }

    /// A merge into vector layer `lower` kept as one undo step called `name`: `finish` takes the
    /// merged layers out of the document (as the merge of pixels does), `lower` is left holding
    /// `merged`, and its pixels are drawn again where they changed. `before` is the document as
    /// undo puts it back.
    pub(crate) fn keep_vector_merge(
        &mut self,
        name: &str,
        lower: LayerId,
        merged: VectorContent,
        before: Document,
        finish: impl FnOnce(&mut Document),
    ) {
        let was = self.doc.layer(lower).and_then(|l| l.vector.clone()).unwrap_or_default();
        // a hidden one shows, and gave up what it held: all of that is drawn again too
        let was =
            if self.doc.layer(lower).is_some_and(|l| l.visible) { (*was).clone() } else { VectorContent::default() };
        finish(&mut self.doc);
        // (finishing a merge makes the layer an ordinary one; this one stays a vector layer)
        self.doc.set_vector(lower, merged);
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
            canvas.name_next_step(name);
        }
        if was.is_empty()
            && let Some(canvas) = self.canvas.as_mut()
        {
            canvas.clear_layer(lower);
        }
        self.redraw_vector_changed(lower, &was);
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.record_document(before);
            canvas.collapse_steps();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use procraft_engine::document::LayerId;
    use procraft_engine::view::View;
    use procraft_gpu::Gpu;

    use crate::vector_layer::VectorEraser;
    use crate::{CanvasSession, PenSample, PointerKind, ToolKind};

    /// A session 300 × 300 shown a pixel a point, with a GPU to draw on; none where there is no
    /// GPU (the test is skipped).
    fn session() -> Option<Arc<CanvasSession>> {
        let gpu = match Gpu::headless() {
            Ok(gpu) => gpu,
            Err(e) => {
                eprintln!("skipped: {e}");
                return None;
            }
        };
        let s = CanvasSession::new(300, 300);
        {
            let mut st = s.shared.lock();
            st.window_size = (300, 300);
            st.view = Some(View::fit((300, 300), (300, 300)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        Some(s)
    }

    fn straight(s: &CanvasSession, from: (f32, f32), to: (f32, f32)) {
        let at = |i: u32| {
            let t = i as f32 / 30.0;
            PenSample {
                x: from.0 + (to.0 - from.0) * t,
                y: from.1 + (to.1 - from.1) * t,
                pressure: 0.8,
                tilt: 0.0,
                orientation: 0.0,
                time_ms: f64::from(i) * 8.0,
                roll: 0.0,
            }
        };
        s.stroke_begin(PointerKind::Stylus, vec![at(0)]);
        s.stroke_add((1..30).map(at).collect());
        s.stroke_end(vec![at(30)]);
    }

    /// A box of four strokes crossing at its corners, from `a` to `b` both ways.
    fn boxed(s: &CanvasSession, a: f32, b: f32) {
        straight(s, (a - 10.0, a), (b + 10.0, a));
        straight(s, (b, a - 10.0), (b, b + 10.0));
        straight(s, (b + 10.0, b), (a - 10.0, b));
        straight(s, (a, b + 10.0), (a, a - 10.0));
    }

    fn pixels(s: &CanvasSession, layer: u64) -> Vec<u8> {
        s.shared.lock().layer_pixels(LayerId(layer)).unwrap()
    }

    /// The canvas as composited (premultiplied RGBA8), everything queued drawn first.
    fn composite(s: &CanvasSession) -> Vec<u8> {
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        st.flush();
        let (canvas, gpu) = (st.canvas.as_mut().unwrap(), st.gpu.as_ref().unwrap());
        canvas.read_composite_without(gpu, &mut st.doc, &[]).unwrap()
    }

    fn pixel(rgba: &[u8], x: usize, y: usize) -> [u8; 4] {
        let at = (y * 300 + x) * 4;
        [rgba[at], rgba[at + 1], rgba[at + 2], rgba[at + 3]]
    }

    fn furthest(a: &[u8], b: &[u8]) -> u8 {
        assert_eq!(a.len(), b.len());
        a.iter().zip(b).map(|(x, y)| x.abs_diff(*y)).max().unwrap_or(0)
    }

    /// The most any pixel differs between two pictures, away from where the upper eraser of
    /// [`two_layers`] went.
    fn apart_from_eraser(a: &[u8], b: &[u8]) -> u8 {
        let mut most = 0;
        for y in 0..300 {
            for x in 0..300 {
                if (120..=160).contains(&x) && (100..=180).contains(&y) {
                    continue;
                }
                let (p, q) = (pixel(a, x, y), pixel(b, x, y));
                most = p.iter().zip(q).map(|(m, n)| m.abs_diff(n)).fold(most, u8::max);
            }
        }
        most
    }

    /// Two vector layers: below, a black box filled red; above, a blue box filled green that
    /// overlaps it, the upper's line across erased in the middle (through the red fill too).
    fn two_layers(s: &CanvasSession) -> (u64, u64) {
        let lower = s.add_vector_layer();
        s.set_color(0.0, 0.0, 0.0);
        s.set_brush_size(0.1);
        boxed(s, 40.0, 160.0);
        assert!(s.pour_drop(100.0, 100.0, 1.0, 0.0, 0.0, 0.5));
        let upper = s.add_vector_layer();
        s.set_color(0.0, 0.0, 1.0);
        boxed(s, 120.0, 260.0);
        assert!(s.pour_drop(200.0, 200.0, 0.0, 1.0, 0.0, 0.5));
        straight(s, (20.0, 140.0), (280.0, 140.0));
        s.set_tool(ToolKind::Erase);
        s.set_vector_eraser(VectorEraser::Subtract);
        s.set_brush_size(0.2);
        straight(s, (140.0, 120.0), (140.0, 160.0));
        s.set_tool(ToolKind::Paint);
        (lower, upper)
    }

    #[test]
    fn merge_down_keeps_vector_layers_strokes_and_fills_and_shows_as_they_did() {
        let Some(s) = session() else { return };
        let (lower, upper) = two_layers(&s);
        let shown = composite(&s);
        let layers = s.layers().len();
        let (strokes, fills) = (
            s.vector_stroke_count(lower) + s.vector_stroke_count(upper),
            s.vector_fill_count(lower) + s.vector_fill_count(upper),
        );
        // the eraser took the upper line's middle, not the red under it
        assert!(pixel(&pixels(&s, lower), 140, 140)[3] == 255);

        assert!(s.merge_down(upper));
        assert_eq!(s.layers().len(), layers - 1);
        assert!(s.is_vector_layer(lower));
        assert_eq!(s.vector_stroke_count(lower), strokes);
        assert_eq!(s.vector_fill_count(lower), fills);
        // it shows as the two did: the upper's fill over the red, the eraser's gap showing red
        let merged = composite(&s);
        assert!(apart_from_eraser(&merged, &shown) <= 2, "{}", apart_from_eraser(&merged, &shown));
        // where the upper eraser crossed the red below, off the upper's lines, the same too: it
        // took from its own layer alone. (On them, a soft eraser's marks build up a little on
        // each stroke and fill it took from, so the lines show there a little more than they did.)
        for (x, y) in [(140, 130), (136, 130), (140, 150), (144, 155)] {
            assert_eq!(pixel(&merged, x, y), pixel(&shown, x, y), "at {x}, {y}");
        }
        let on = pixels(&s, lower);
        assert_eq!(pixel(&on, 200, 200), [0, 255, 0, 255]);
        assert!(pixel(&on, 150, 150)[1] > 240, "the green over the red");
        // and is what its strokes and fills draw
        assert!(s.redraw_vector_layer(lower));
        assert!(furthest(&pixels(&s, lower), &on) <= 1);
        s.undo();

        // one undo puts the two layers back as they were
        assert_eq!(s.undo().as_deref(), Some("merge down"));
        assert_eq!(s.layers().len(), layers);
        assert_eq!(s.vector_stroke_count(upper) + s.vector_stroke_count(lower), strokes);
        assert_eq!(furthest(&composite(&s), &shown), 0);
        s.redo();
        assert_eq!(s.vector_fill_count(lower), fills);
        assert!(furthest(&composite(&s), &merged) <= 1);
    }

    #[test]
    fn layers_picked_merge_as_vectors_with_their_opacity_and_others_as_pixels() {
        let Some(s) = session() else { return };
        let (lower, upper) = two_layers(&s);
        let third = s.add_vector_layer();
        straight(&s, (20.0, 280.0), (280.0, 20.0));
        assert!(s.set_layer_opacity(third, 0.5, true));
        let shown = composite(&s);
        let strokes: u32 = [lower, upper, third].iter().map(|l| s.vector_stroke_count(*l)).sum();
        assert!(s.merge_between(lower, third));
        assert!(s.is_vector_layer(lower));
        assert_eq!(s.vector_stroke_count(lower), strokes);
        // the half-opaque line is half as strong where it crosses nothing
        let merged = composite(&s);
        assert!(apart_from_eraser(&merged, &shown) <= 3, "{}", apart_from_eraser(&merged, &shown));
        assert_eq!(s.undo().as_deref(), Some("merge layers"));
        assert!(s.is_vector_layer(third));

        // a layer that isn't a vector layer is merged into pixels, as before
        assert!(s.select_layer(third));
        let paint = s.add_layer();
        straight(&s, (20.0, 20.0), (280.0, 280.0));
        assert!(s.merge_down(paint));
        assert!(!s.is_vector_layer(third));
    }
}

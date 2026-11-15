//! Vector layers merged into one that is still a vector layer: every stroke and fill kept as it
//! was made, each layer's on top of the one below's, so that the merged layer draws what the
//! layers showed apart and stays as editable as they were.

use std::collections::HashMap;
use std::sync::Arc;

use super::{FillMode, Object, StrokeId, VectorContent, reaches_meet};

/// One layer's part in a merge: what it holds, and the opacity it showed at.
#[derive(Clone, Copy, Debug)]
pub struct Merging<'a> {
    pub content: &'a VectorContent,
    pub opacity: f32,
}

impl VectorContent {
    /// `layers` (bottom first) made one: each one's strokes and fills drawn after all of those
    /// below, in the order they were made. The bottom one's keep their names; the others' are
    /// given new ones past them, and what names them (an eraser's marks, a fill's walls) follows.
    ///
    /// What each layer did to itself alone, it still does to itself alone:
    /// - an eraser that took from all made before it on its layer names those, so that it doesn't
    ///   take from the layers below as well (one with nothing before it had nothing to take, and
    ///   goes);
    /// - a fill laid behind its layer's strokes is laid first among them, over the layers below,
    ///   instead of going under those too.
    ///
    /// Each layer's opacity goes into its strokes' and fills' own (an eraser's take as much as
    /// they did). Where a layer's own strokes overlap, that shows them a little darker than the
    /// layer did at that opacity; at full opacity the merged layer is what the layers showed.
    pub fn merged(layers: &[Merging<'_>]) -> VectorContent {
        let mut objects: Vec<Object> = Vec::new();
        let mut next: u64 = 0;
        for (at, layer) in layers.iter().enumerate() {
            let own = if at == 0 { layer.content.objects.clone() } else { layer.content.kept_to_itself(&objects) };
            // (the bottom layer keeps its names: those below it in the stack are none)
            let base = next;
            let renamed = |id: StrokeId| StrokeId(base.saturating_add(id.0));
            let names: HashMap<StrokeId, StrokeId> = own.iter().map(|o| (o.id(), renamed(o.id()))).collect();
            let rename = |id: &StrokeId| names.get(id).copied().unwrap_or(*id);
            for mut object in own {
                // (only what changes is copied: what is left as it was is the same thing, and
                // needn't be drawn again)
                if base > 0 {
                    object.set_id(renamed(object.id()));
                    if let Some(ids) = object.takes_from() {
                        let ids = ids.iter().map(rename).collect();
                        object.set_takes_from(Some(ids));
                    }
                }
                match &mut object {
                    Object::Stroke(stroke) => {
                        if !stroke.ink.erases && layer.opacity < 1.0 {
                            Arc::make_mut(stroke).ink.opacity *= layer.opacity.clamp(0.0, 1.0);
                        }
                    }
                    Object::Fill(fill) => {
                        if fill.mode != FillMode::Erase && layer.opacity < 1.0 {
                            Arc::make_mut(fill).opacity *= layer.opacity.clamp(0.0, 1.0);
                        }
                        if base > 0
                            && let Some(traced) = Arc::make_mut(fill).traced.as_mut()
                        {
                            traced.sources = traced.sources.iter().map(rename).collect();
                        }
                    }
                }
                objects.push(object);
            }
            next = base.saturating_add(layer.content.next);
        }
        VectorContent::restored(objects, next)
    }

    /// This layer's strokes and fills, ordered and named so that drawn over `below` (what the
    /// layers under it hold, merged) they do to themselves alone what they did here: see
    /// [`VectorContent::merged`]. An eraser that took from all made before it goes on doing so
    /// where nothing it would wrongly take from lies under it (drawn so, it erases just as it
    /// did); elsewhere it names what it took from.
    fn kept_to_itself(&self, below: &[Object]) -> Vec<Object> {
        let mut behind: Vec<Object> = Vec::new();
        let mut rest: Vec<Object> = Vec::new();
        // what paints here, made so far
        let mut made: Vec<StrokeId> = Vec::new();
        for (at, object) in self.objects.iter().enumerate() {
            let mut object = object.clone();
            if object.erases() {
                if object.takes_from().is_none() {
                    if made.is_empty() {
                        continue;
                    }
                    // what it would take from that it didn't: the layers below, and this one's
                    // fills laid behind after it (which go before it now)
                    let reach = object.reach();
                    let meets = |o: &Object| match (reach, o.reach()) {
                        (Some(a), Some(b)) => reaches_meet(a, b),
                        _ => false,
                    };
                    let wrongly = below.iter().any(meets)
                        || self
                            .objects
                            .iter()
                            .skip(at + 1)
                            .any(|o| matches!(o, Object::Fill(fill) if fill.mode == FillMode::Behind) && meets(o));
                    if wrongly {
                        object.set_takes_from(Some(made.clone()));
                    }
                }
                rest.push(object);
                continue;
            }
            made.push(object.id());
            match &mut object {
                Object::Fill(fill) if fill.mode == FillMode::Behind => {
                    // under all of this layer made before it: first of all, laid over what is below
                    Arc::make_mut(fill).mode = FillMode::Over;
                    behind.insert(0, object);
                }
                _ => rest.push(object),
            }
        }
        behind.extend(rest);
        behind
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::brush::Brush;
    use crate::geom::Vec2;
    use crate::input::{Pointer, Sample};
    use crate::vector::{Ink, Setup, TracedFrom, VectorFill, VectorStroke};

    fn ink(erases: bool) -> Ink {
        Ink {
            colour: [0.1, 0.2, 0.3],
            secondary: [1.0, 1.0, 1.0],
            opacity: 1.0,
            erases,
            preserve_alpha: false,
            wetness: 1.0,
        }
    }

    fn straight(content: &mut VectorContent, from: (f32, f32), to: (f32, f32), erases: bool) -> StrokeId {
        let brush = Brush { streamline: 0.0, ..Brush::round_pen() };
        let setup = Setup::new(Arc::new(brush), 12.0, Pointer::Finger, 7);
        let mut stroke = VectorStroke::begin(content.next_id(), setup, ink(erases));
        let (a, b) = (Vec2::new(from.0, from.1), Vec2::new(to.0, to.1));
        let samples: Vec<Sample> =
            (0..=20).map(|i| Sample::at(a.lerp(b, i as f32 / 20.0), 1.0, f64::from(i) * 8.0)).collect();
        stroke.record(&samples);
        let id = stroke.id;
        content.push(stroke);
        id
    }

    fn square(content: &mut VectorContent, at: f32, mode: FillMode, sources: Vec<StrokeId>) -> StrokeId {
        let id = content.next_id();
        content.push_fill(VectorFill {
            id,
            loops: vec![vec![
                Vec2::new(at, at),
                Vec2::new(at + 50.0, at),
                Vec2::new(at + 50.0, at + 50.0),
                Vec2::new(at, at + 50.0),
            ]],
            colour: [0.9, 0.5, 0.1],
            opacity: 1.0,
            mode,
            hard: false,
            traced: (!sources.is_empty()).then(|| TracedFrom {
                seed: Vec2::new(at + 25.0, at + 25.0),
                factor: 0.5,
                sources,
            }),
            takes_from: None,
        });
        id
    }

    fn fill_of(content: &VectorContent, index: usize) -> Arc<VectorFill> {
        match content.objects().get(index) {
            Some(Object::Fill(fill)) => Arc::clone(fill),
            other => panic!("no fill at {index}: {other:?}"),
        }
    }

    fn stroke_of(content: &VectorContent, index: usize) -> Arc<VectorStroke> {
        match content.objects().get(index) {
            Some(Object::Stroke(stroke)) => Arc::clone(stroke),
            other => panic!("no stroke at {index}: {other:?}"),
        }
    }

    #[test]
    fn the_layers_strokes_and_fills_stack_bottom_first_under_names_of_their_own() {
        let mut lower = VectorContent::default();
        let line = straight(&mut lower, (10.0, 10.0), (90.0, 10.0), false);
        square(&mut lower, 0.0, FillMode::Over, Vec::new());
        let mut upper = VectorContent::default();
        straight(&mut upper, (10.0, 40.0), (90.0, 40.0), false);
        square(&mut upper, 20.0, FillMode::Over, Vec::new());

        let merged = VectorContent::merged(&[
            Merging { content: &lower, opacity: 1.0 },
            Merging { content: &upper, opacity: 1.0 },
        ]);
        assert_eq!(merged.len(), 4);
        // the bottom layer's keep their names, and are the same things (nothing to draw again)
        assert_eq!(merged.objects().first(), lower.objects().first());
        assert_eq!(stroke_of(&merged, 0).id, line);
        let upper_reach = upper.objects().iter().fold(crate::geom::IRect::EMPTY, |all, o| all.union(o.bounds()));
        assert_eq!(merged.changed_from(&lower), upper_reach);
        // the upper's come after, each named apart from all others
        let ids: Vec<StrokeId> = merged.objects().iter().map(Object::id).collect();
        let mut unique = ids.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), ids.len());
        assert_eq!(stroke_of(&merged, 2).line(), stroke_of(&upper, 0).line());
        assert_eq!(fill_of(&merged, 3).loops, fill_of(&upper, 1).loops);
        // and what is made next is named past them all
        assert!(ids.iter().all(|id| *id < merged.next_id()));
    }

    #[test]
    fn an_upper_layer_s_eraser_takes_from_its_own_layer_alone() {
        let mut lower = VectorContent::default();
        let below = straight(&mut lower, (10.0, 50.0), (90.0, 50.0), false);
        let mut upper = VectorContent::default();
        // an eraser with nothing before it on its layer erased nothing there
        straight(&mut upper, (50.0, 0.0), (50.0, 100.0), true);
        let own = straight(&mut upper, (10.0, 60.0), (90.0, 60.0), false);
        let eraser = straight(&mut upper, (50.0, 0.0), (50.0, 100.0), true);
        let after = straight(&mut upper, (10.0, 70.0), (90.0, 70.0), false);

        let merged = VectorContent::merged(&[
            Merging { content: &lower, opacity: 1.0 },
            Merging { content: &upper, opacity: 1.0 },
        ]);
        // the first eraser went: there was nothing for it to take from
        assert_eq!(merged.len(), 4);
        let renamed = |id: StrokeId| StrokeId(lower.next_id().0 + id.0);
        let erasing = stroke_of(&merged, 2);
        assert_eq!(erasing.id, renamed(eraser));
        assert_eq!(erasing.takes_from.as_deref(), Some([renamed(own)].as_slice()));
        assert!(stroke_of(&merged, 3).id == renamed(after));
        // so the line below is whole where the upper eraser crossed it, the upper one is not
        assert!(!merged.erased(0, below, Vec2::new(50.0, 50.0)));
        assert!(merged.erased(1, renamed(own), Vec2::new(50.0, 60.0)));
        assert!(!merged.erased(3, renamed(after), Vec2::new(50.0, 70.0)));
        // the bottom layer's own eraser still takes from all made before it there
        let mut lower_erased = lower.clone();
        straight(&mut lower_erased, (50.0, 0.0), (50.0, 100.0), true);
        let merged = VectorContent::merged(&[
            Merging { content: &lower_erased, opacity: 1.0 },
            Merging { content: &upper, opacity: 1.0 },
        ]);
        assert!(stroke_of(&merged, 1).takes_from.is_none());
        assert!(merged.erased(0, below, Vec2::new(50.0, 50.0)));

        // an eraser over nothing of the layers below goes on taking from all made before it,
        // which there is its own layer's alone: drawn so, it erases exactly as it did
        let mut apart = VectorContent::default();
        let line = straight(&mut apart, (200.0, 200.0), (300.0, 200.0), false);
        straight(&mut apart, (250.0, 150.0), (250.0, 250.0), true);
        let merged = VectorContent::merged(&[
            Merging { content: &lower, opacity: 1.0 },
            Merging { content: &apart, opacity: 1.0 },
        ]);
        assert!(stroke_of(&merged, 2).takes_from.is_none());
        assert!(merged.erased(1, renamed(line), Vec2::new(250.0, 200.0)));
        // unless a fill laid behind after it would come under it, as it now goes first
        square(&mut apart, 220.0, FillMode::Behind, Vec::new());
        let merged = VectorContent::merged(&[
            Merging { content: &lower, opacity: 1.0 },
            Merging { content: &apart, opacity: 1.0 },
        ]);
        assert_eq!(stroke_of(&merged, 3).takes_from.as_deref(), Some([renamed(line)].as_slice()));
    }

    #[test]
    fn a_fill_behind_an_upper_layer_s_lines_stays_over_the_layer_below() {
        let mut lower = VectorContent::default();
        straight(&mut lower, (0.0, 0.0), (100.0, 100.0), false);
        let lower_fill = square(&mut lower, 10.0, FillMode::Behind, Vec::new());
        let mut upper = VectorContent::default();
        let a = straight(&mut upper, (20.0, 20.0), (70.0, 20.0), false);
        let b = straight(&mut upper, (20.0, 70.0), (70.0, 70.0), false);
        let first = square(&mut upper, 20.0, FillMode::Behind, vec![a, b]);
        let c = straight(&mut upper, (20.0, 20.0), (20.0, 70.0), false);
        let second = square(&mut upper, 30.0, FillMode::Behind, vec![c]);

        let merged = VectorContent::merged(&[
            Merging { content: &lower, opacity: 1.0 },
            Merging { content: &upper, opacity: 1.0 },
        ]);
        let renamed = |id: StrokeId| StrokeId(lower.next_id().0 + id.0);
        let order: Vec<StrokeId> = merged.objects().iter().map(Object::id).collect();
        // the bottom layer's fill is still laid behind its own lines
        assert_eq!(fill_of(&merged, 1).id, lower_fill);
        assert_eq!(fill_of(&merged, 1).mode, FillMode::Behind);
        // the upper's come first of its own, the later under the earlier, laid over the layer below
        assert_eq!(
            order.get(2..),
            Some([renamed(second), renamed(first), renamed(a), renamed(b), renamed(c)].as_slice())
        );
        assert_eq!(fill_of(&merged, 2).mode, FillMode::Over);
        assert_eq!(fill_of(&merged, 3).mode, FillMode::Over);
        // its walls are the renamed lines
        let traced = fill_of(&merged, 3).traced.clone();
        assert_eq!(traced.map(|t| t.sources), Some(vec![renamed(a), renamed(b)]));
    }

    #[test]
    fn an_eraser_that_named_its_strokes_goes_on_naming_them_renamed() {
        let mut upper = VectorContent::default();
        let line = straight(&mut upper, (10.0, 10.0), (90.0, 10.0), false);
        straight(&mut upper, (50.0, 0.0), (50.0, 20.0), true);
        // carried, the eraser names what it was made on
        upper.place(
            Some(&[line]),
            crate::geom::Homography::from_affine(crate::geom::Affine::translate(Vec2::new(5.0, 5.0))),
        );
        let named = upper.objects().get(1).and_then(Object::takes_from).map(<[StrokeId]>::to_vec);
        assert_eq!(named, Some(vec![line]));
        let mut lower = VectorContent::default();
        straight(&mut lower, (0.0, 0.0), (10.0, 10.0), false);
        straight(&mut lower, (0.0, 10.0), (10.0, 0.0), false);

        let merged = VectorContent::merged(&[
            Merging { content: &lower, opacity: 1.0 },
            Merging { content: &upper, opacity: 1.0 },
        ]);
        let renamed = StrokeId(lower.next_id().0 + line.0);
        assert_eq!(merged.erasers_of(renamed).len(), 1);
        assert!(merged.erasers_of(StrokeId(0)).is_empty());
    }

    #[test]
    fn a_layer_s_opacity_goes_into_what_paints_on_it_not_into_its_erasers() {
        let mut lower = VectorContent::default();
        straight(&mut lower, (10.0, 10.0), (90.0, 10.0), false);
        let mut upper = VectorContent::default();
        straight(&mut upper, (10.0, 40.0), (90.0, 40.0), false);
        square(&mut upper, 20.0, FillMode::Over, Vec::new());
        straight(&mut upper, (50.0, 0.0), (50.0, 100.0), true);
        let mut lasso = upper.clone();
        square(&mut lasso, 0.0, FillMode::Erase, Vec::new());

        let merged = VectorContent::merged(&[
            Merging { content: &lower, opacity: 0.5 },
            Merging { content: &lasso, opacity: 0.25 },
        ]);
        assert_eq!(stroke_of(&merged, 0).ink.opacity, 0.5);
        assert_eq!(stroke_of(&merged, 1).ink.opacity, 0.25);
        assert_eq!(fill_of(&merged, 2).opacity, 0.25);
        assert_eq!(stroke_of(&merged, 3).ink.opacity, 1.0);
        assert_eq!(fill_of(&merged, 4).opacity, 1.0);
        // the lasso's shape takes from the upper layer's alone
        assert_eq!(fill_of(&merged, 4).takes_from.as_ref().map(Vec::len), Some(2));
    }
}

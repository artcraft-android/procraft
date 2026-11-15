//! Adjustment layers beyond a still canvas: a timeline clip whose drawing is one, its values keyed
//! over time, and a copy of the document that shows only what one redraws (for a file that can't
//! keep it as an adjustment).

use crate::document::{Document, LayerId, LayerKind};
use crate::filter::Filter;
use crate::timeline::{ClipContent, ClipId, Track};

impl Document {
    /// Marks each timeline clip whose drawing is an adjustment layer as an adjustment clip (and
    /// the others as not), so its adjustment keys are drawn. True if any changed.
    pub fn mark_adjustment_clips(&mut self) -> bool {
        let Some(mut tl) = self.timeline.take() else { return false };
        let changed = mark(&mut tl.tracks, self);
        self.timeline = Some(tl);
        changed
    }

    /// The adjustment layer clip `clip` draws, if it is an adjustment clip.
    pub fn clip_adjustment_layer(&self, clip: ClipId) -> Option<LayerId> {
        let c = self.timeline.as_ref()?.clip(clip)?;
        let ClipContent::Drawing(d) = &c.content else { return None };
        self.layer(d.root).filter(|l| l.is_adjustment()).map(|l| l.id)
    }

    /// Clip `clip`'s adjustment at its own frame `local`: as its keys ease it, else its layer's.
    /// None unless it is an adjustment clip.
    pub fn clip_adjustment_at(&self, clip: ClipId, local: f32) -> Option<Filter> {
        let own = self.layer(self.clip_adjustment_layer(clip)?)?.adjustment.as_ref()?.filter;
        let keyed = self.timeline.as_ref()?.clip(clip)?.keys.adjustment.value_at(local);
        Some(keyed.filter(|k| k.same_kind(&own)).unwrap_or(own))
    }

    /// A copy showing only what adjustment layer `id` draws, at its full strength and unmasked:
    /// what is below it (in its group, if the group composites on its own) redrawn through it,
    /// with nothing above it. Its pixels are what a file that has no such adjustment keeps in its
    /// place: a layer of them, at its opacity and through its mask, looks as it did.
    pub fn adjustment_alone(&self, id: LayerId) -> Option<Document> {
        let layer = self.layer(id).filter(|l| l.is_adjustment())?;
        let at = self.index_of(id)?;
        let mut doc = self.clone();
        let ancestors: Vec<LayerId> =
            std::iter::successors(layer.parent, |g| self.layer(*g).and_then(|l| l.parent)).collect();
        // the nearest group that composites on its own: what is outside it doesn't reach the layer
        let isolated = ancestors.iter().copied().find(|g| self.layer(*g).is_some_and(|l| !l.passes_through()));
        for (i, l) in self.layers().iter().enumerate() {
            let outside = isolated.is_some_and(|g| l.id != g && !self.is_inside(l.id, g));
            let above = i > at && !ancestors.contains(&l.id);
            // its own mask is left off: the layer made of it carries the mask
            let own_mask = l.kind == LayerKind::Mask && l.parent == Some(id);
            if (outside && !ancestors.contains(&l.id)) || above || own_mask {
                doc.set_visible(l.id, false);
            }
        }
        for g in &ancestors {
            if let Some(l) = doc.layer_mut(*g) {
                (l.visible, l.opacity, l.blend) = (true, 1.0, crate::document::BlendMode::Normal);
            }
            if let Some(m) = self.mask_of(*g) {
                doc.set_visible(m, false);
            }
        }
        if let Some(l) = doc.layer_mut(id) {
            (l.visible, l.opacity, l.blend) = (true, 1.0, crate::document::BlendMode::Normal);
        }
        if isolated.is_some() {
            doc.background_visible = false;
        }
        doc.solo = None;
        doc.timeline = None;
        doc.animation = None;
        Some(doc)
    }
}

fn mark(tracks: &mut [Track], doc: &Document) -> bool {
    let mut changed = false;
    for clip in tracks.iter_mut().flat_map(|t| t.clips.iter_mut()) {
        let adjustment = match &mut clip.content {
            ClipContent::Drawing(d) => doc.layer(d.root).is_some_and(|l| l.is_adjustment()),
            ClipContent::Group(g) => {
                changed |= mark(&mut g.tracks, doc);
                false
            }
            _ => false,
        };
        if clip.adjustment != adjustment {
            clip.adjustment = adjustment;
            changed = true;
        }
    }
    changed
}

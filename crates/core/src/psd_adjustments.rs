//! Adjustment layers in a PSD: as Photoshop's own adjustment layers where it has one like them
//! (hue/saturation, curves, colour balance), and otherwise flattened into a layer of pixels that
//! looks as the adjustment did.

use std::collections::HashMap;

use procraft_engine::document::{Document, LayerId};
use procraft_engine::formats;

use crate::project::straight_alpha;
use crate::{CanvasSession, MobileError};

/// The pixels (straight RGBA8) each adjustment layer a PSD can't keep as one is flattened into.
pub(crate) type Flattened = HashMap<LayerId, Vec<u8>>;

/// The shown masks of the adjustment layers of `doc` a PSD keeps as its own kind: read ahead of
/// the file being written, where their records want them.
pub(crate) fn kept_masks(doc: &Document) -> Vec<LayerId> {
    doc.layers()
        .iter()
        .filter(|l| l.adjustment.as_ref().is_some_and(|a| formats::psd_adjustment(&a.filter, &a.table).is_some()))
        .filter_map(|l| doc.mask_of(l.id).filter(|m| doc.layer(*m).is_some_and(|m| m.visible)))
        .collect()
}

impl CanvasSession {
    /// Each adjustment layer of `doc` a PSD has no kind for, as it draws alone (see
    /// [`procraft_gpu::Canvas::read_adjustment_alone`]).
    pub(crate) fn flattened_adjustments(&self, doc: &Document) -> Result<Flattened, MobileError> {
        let wanted: Vec<LayerId> = doc
            .layers()
            .iter()
            .filter(|l| l.adjustment.as_ref().is_some_and(|a| formats::psd_adjustment(&a.filter, &a.table).is_none()))
            .map(|l| l.id)
            .collect();
        let mut out = Flattened::new();
        if wanted.is_empty() {
            return Ok(out);
        }
        let mut guard = self.shared.lock_finished();
        let st = &mut *guard;
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return Ok(out) };
        for id in wanted {
            let rgba = canvas
                .read_adjustment_alone(gpu, &mut st.doc, id)
                .map_err(|e| crate::project::file_error("reading an adjustment layer", e))?;
            out.insert(id, straight_alpha(&rgba));
        }
        Ok(out)
    }
}

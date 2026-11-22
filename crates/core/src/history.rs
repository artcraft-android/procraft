//! The colours painted with lately, newest first, the same in every app: the app keeps them (they
//! outlast an artwork) and hands them to each canvas, whose paint strokes add their colour as they
//! end. Colours are 0xRRGGBB.
//!
//! It keeps the previous colour too, which holding the colour button goes back to: the colour
//! before the last one chosen (with the eyedropper, the colour panel closed on a new one, or the
//! hold itself), the brush's and a mask's apart, as masks paint with a colour of their own.

use std::sync::{Arc, Mutex};

/// How many colours the history keeps.
pub const HISTORY_SIZE: usize = 10;

/// How long (milliseconds) the colour button is held still before it goes to the previous colour.
pub const COLOUR_HOLD_MS: u32 = 300;

/// [`COLOUR_HOLD_MS`], for the apps.
#[uniffi::export]
pub fn colour_hold_ms() -> u32 {
    COLOUR_HOLD_MS
}

#[derive(Default, uniffi::Object)]
pub struct ColourHistory {
    colours: Mutex<Vec<u32>>,
    previous: Mutex<Previous>,
}

/// The previous colours, the brush's and a mask's, and the colour the colour panel opened on (and
/// whether it was a mask's) while it is open.
#[derive(Default)]
struct Previous {
    colour: [Option<u32>; 2],
    opened: Option<(u32, bool)>,
}

#[uniffi::export]
impl ColourHistory {
    #[uniffi::constructor]
    pub fn new() -> Arc<ColourHistory> {
        Arc::new(ColourHistory::default())
    }

    /// Newest first.
    pub fn colours(&self) -> Vec<u32> {
        self.kept().clone()
    }

    pub fn clear(&self) {
        self.kept().clear();
    }

    /// The last colour painted with other than `current`, to go back to.
    pub fn previous(&self, current: u32) -> Option<u32> {
        let current = current & 0xFF_FFFF;
        self.kept().iter().copied().find(|&c| c != current)
    }

    /// The colour became `to` from `from` by a choice of the user's outside the colour panel (the
    /// eyedropper; `mask`: a mask's colour): `from` is the previous colour, unless they are the same.
    pub fn chose(&self, from: u32, to: u32, mask: bool) {
        let (from, to) = (from & 0xFF_FFFF, to & 0xFF_FFFF);
        if from != to
            && let Some(slot) = self.previous_kept().colour.get_mut(usize::from(mask))
        {
            *slot = Some(from);
        }
    }

    /// The colour panel shows (`shown`) or doesn't, the colour now `current` (a mask's with
    /// `mask`); told whenever that may have changed (on every frame will do). As it opens, the
    /// colour it opens on is noted; closed on another, that one is the previous colour.
    pub fn panel_shown(&self, shown: bool, current: u32, mask: bool) {
        let current = current & 0xFF_FFFF;
        let mut kept = self.previous_kept();
        match (shown, kept.opened) {
            (true, None) => kept.opened = Some((current, mask)),
            (false, Some((opened, was_mask))) => {
                kept.opened = None;
                if opened != current
                    && was_mask == mask
                    && let Some(slot) = kept.colour.get_mut(usize::from(mask))
                {
                    *slot = Some(opened);
                }
            }
            _ => {}
        }
    }

    /// The colour button held (or the quick menu's Previous colour) with the colour `current` (a
    /// mask's with `mask`): the colour to go to, the previous one (before any, the last painted with
    /// other than `current`; a mask's has none), and `current` becomes the previous colour, so
    /// another hold comes back to it. None, and nothing changed, if there is none to go to.
    pub fn swap_previous(&self, current: u32, mask: bool) -> Option<u32> {
        let current = current & 0xFF_FFFF;
        let kept = self.previous_kept().colour.get(usize::from(mask)).copied().flatten();
        let before = kept.or_else(|| if mask { None } else { self.previous(current) })?;
        if before == current {
            return None;
        }
        if let Some(slot) = self.previous_kept().colour.get_mut(usize::from(mask)) {
            *slot = Some(current);
        }
        Some(before)
    }
}

impl ColourHistory {
    fn kept(&self) -> std::sync::MutexGuard<'_, Vec<u32>> {
        self.colours.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn previous_kept(&self) -> std::sync::MutexGuard<'_, Previous> {
        self.previous.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// A stroke was painted in `rgb`: it goes to the front.
    pub(crate) fn painted(&self, rgb: u32) {
        let rgb = rgb & 0xFF_FFFF;
        let mut kept = self.kept();
        kept.retain(|&c| c != rgb);
        kept.insert(0, rgb);
        kept.truncate(HISTORY_SIZE);
    }
}

/// An sRGB colour (0..1) as 0xRRGGBB.
pub(crate) fn hex([r, g, b]: [f32; 3]) -> u32 {
    let byte = |c: f32| (c.clamp(0.0, 1.0) * 255.0).round() as u32;
    byte(r) << 16 | byte(g) << 8 | byte(b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{CanvasSession, PenSample, ToolKind};

    /// The colour button's hold: the colour and the previous one trade places, so a second hold
    /// comes back; the previous colour is the one before the last chosen with the eyedropper or
    /// the colour panel (closed on another, not each colour it passed), the brush's and a mask's
    /// apart; before any choice, the last painted with.
    #[test]
    fn holding_the_colour_button_trades_the_colour_with_the_previous_one() {
        let (red, green, blue, grey) = (0xFF0000, 0x00FF00, 0x0000FF, 0x808080);
        let h = ColourHistory::new();
        // nothing chosen or painted: nothing to go to, nothing changed
        assert_eq!(h.swap_previous(red, false), None);
        // only painted: the last painted with, then back
        h.painted(green);
        assert_eq!(h.swap_previous(red, false), Some(green));
        assert_eq!(h.swap_previous(green, false), Some(red));
        assert_eq!(h.swap_previous(red, false), Some(green));
        // red picked with the eyedropper over green: green is the previous colour, held twice back
        h.chose(green, red, false);
        assert_eq!(h.swap_previous(red, false), Some(green));
        assert_eq!(h.swap_previous(green, false), Some(red));
        // the panel opened on red, dragged through many colours to blue and closed: red is the
        // previous colour, not one it passed; told every frame, it notes only the edges
        h.panel_shown(true, red, false);
        h.panel_shown(true, 0x123456, false);
        h.panel_shown(true, blue, false);
        h.panel_shown(false, blue, false);
        h.panel_shown(false, blue, false);
        assert_eq!(h.swap_previous(blue, false), Some(red));
        assert_eq!(h.swap_previous(red, false), Some(blue));
        // opened and closed on the same colour, or the eyedropper picking the same: no change
        h.panel_shown(true, blue, false);
        h.panel_shown(false, blue, false);
        h.chose(blue, blue, false);
        assert_eq!(h.swap_previous(blue, false), Some(red));
        // a mask's colours apart: none yet (painted colours aren't a mask's), then its own
        assert_eq!(h.swap_previous(grey, true), None);
        h.chose(0x000000, grey, true);
        assert_eq!(h.swap_previous(grey, true), Some(0x000000));
        assert_eq!(h.swap_previous(red, false), Some(blue));
        // the panel opened on the brush's colour and closed on a mask's: neither changes
        h.panel_shown(true, blue, false);
        h.panel_shown(false, grey, true);
        assert_eq!(h.swap_previous(blue, false), Some(red));
        assert_eq!(COLOUR_HOLD_MS, 300);
    }

    #[test]
    fn paint_strokes_add_their_colour_newest_first() {
        let h = ColourHistory::new();
        for c in 0..12 {
            h.painted(c);
        }
        h.painted(5);
        assert_eq!(h.colours().len(), HISTORY_SIZE);
        assert_eq!(h.colours().first(), Some(&5));
        assert_eq!(h.colours().get(1), Some(&11));
        assert_eq!(h.previous(5), Some(11));
        h.clear();
        assert_eq!(h.previous(5), None);

        // a canvas given it adds a paint stroke's colour as the stroke ends; erasing adds none
        let s = CanvasSession::new(64, 64);
        s.use_colour_history(h.clone());
        let at =
            |ms| vec![PenSample { x: 1.0, y: 1.0, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: ms, roll: 0.0 }];
        s.set_color(1.0, 0.0, 0.0);
        s.stroke_end(at(0.0));
        assert_eq!(h.colours(), vec![0xFF0000]);
        s.set_tool(ToolKind::Erase);
        s.set_color(0.0, 0.0, 1.0);
        s.stroke_end(at(10.0));
        assert_eq!(h.colours(), vec![0xFF0000]);
    }
}

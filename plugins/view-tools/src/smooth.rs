//! Smooth when zoomed: the canvas shows smoothed between its pixels however far it is zoomed in,
//! for those who would rather not see each pixel as a square of its own colour, which the app shows
//! from 100% on. It's the screen alone: what is saved, exported and picked stays as it is. Off, the
//! canvas shows as the app has it.
//!
//! The app asks the smoothing hook whether to show it smoothed.

use crate::{CONTROLS, SMOOTH_ZOOM as FEATURE};

/// Whether the canvas shows smoothed however far it is zoomed in (1) or as the app has it (0).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_view_smooth() -> i32 {
    i32::from(CONTROLS.on(FEATURE))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::procraft_control;

    #[test]
    fn smoothed_while_its_switch_is_on_and_as_the_app_has_it_while_off() {
        let _turn = crate::fresh(FEATURE);
        assert_eq!(procraft_view_smooth(), 1);
        procraft_control(FEATURE as i32, 0.0);
        assert_eq!(procraft_view_smooth(), 0);
        procraft_control(FEATURE as i32, 1.0);
        assert_eq!(procraft_view_smooth(), 1);
    }

    #[test]
    fn the_other_features_leave_it_as_it_is() {
        let _turn = crate::fresh(FEATURE);
        procraft_control(FEATURE as i32, 0.0);
        // the app tells the copy for another feature this one's switch as off: it answers 0 there
        for other in [crate::VALUE_CHECK, crate::REFERENCE_WINDOWS, crate::ROTATION_RESET] {
            procraft_control(other as i32, 1.0);
            assert_eq!(procraft_view_smooth(), 0, "{other}");
            procraft_control(other as i32, 0.0);
        }
    }
}

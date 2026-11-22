//! Rotation reset: Reset rotation, a row of the Actions panel's Canvas tab (and Alt+R), turns the
//! canvas back upright, or to the right angle nearest, about the middle of the window, its zoom
//! and what is there kept; a switch keeps two fingers from turning the canvas as they pinch, so a
//! pinch near the canvas's edges doesn't turn it by accident; and another keeps a quick pinch in
//! from fitting the canvas to the screen, so it zooms out as any pinch does. It's a plugin, not
//! part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The action reads how far the canvas is turned and turns it (the `view` permission); the switches
//! answer the gesture lock's hook. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

use core::f32::consts::FRAC_PI_2;

/// The controls by their place, as the manifest starts them: where Reset rotation turns to (0
/// upright, 1 the nearest right angle), whether pinching keeps the turn, and whether a quick pinch
/// keeps from fitting the canvas.
static CONTROLS: kit::Controls<3> = kit::Controls::new([0.0, 0.0, 0.0]);
kit::controls!(CONTROLS);
const TO: usize = 0;
const LOCK: usize = 1;
const NO_FIT: usize = 2;

/// The parts of the view the gesture lock's hook asks about: the turn, and the fit a quick pinch
/// in makes.
const TURN: i32 = 0;
const FIT: i32 = 1;

/// Where a canvas turned `turn` radians goes: upright, or to the right angle `nearest` it (half
/// way between two, the one further round).
fn reset(turn: f32, nearest: bool) -> f32 {
    if !nearest || !turn.is_finite() {
        return 0.0;
    }
    let quarters = turn / FRAC_PI_2;
    let whole = (quarters + if quarters >= 0.0 { 0.5 } else { -0.5 }) as i32;
    whole as f32 * FRAC_PI_2
}

/// The app's functions the `view` permission gives.
#[cfg(target_arch = "wasm32")]
mod view {
    #[link(wasm_import_module = "procraft")]
    unsafe extern "C" {
        pub fn view_turn() -> f32;
        pub fn turn_view(radians: f32);
    }
}

/// Reset rotation tapped (its one action): the canvas turned as [`reset`] says.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_action(_index: i32) -> i32 {
    #[cfg(target_arch = "wasm32")]
    // SAFETY: the app's own functions, as the `view` permission gives them; they take and give
    // plain numbers
    unsafe {
        view::turn_view(reset(view::view_turn(), CONTROLS.on(TO)));
    }
    0
}

/// Whether two fingers leave part `part` of the view as it is: the turn, or the fit, while its
/// switch is on.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_gesture_lock(part: i32) -> i32 {
    let locked = match part {
        TURN => CONTROLS.on(LOCK),
        FIT => CONTROLS.on(NO_FIT),
        _ => false,
    };
    i32::from(locked)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_canvas_goes_upright_or_to_the_right_angle_nearest() {
        let degrees = |d: f32| d.to_radians();
        // upright, from any turn
        for turn in [0.0, 30.0, -100.0, 179.0] {
            assert_eq!(reset(degrees(turn), false), 0.0);
        }
        // the nearest right angle
        for (turn, to) in [(10.0, 0.0), (50.0, 90.0), (-50.0, -90.0), (100.0, 90.0), (170.0, 180.0), (-179.0, -180.0)] {
            assert!((reset(degrees(turn), true) - degrees(to)).abs() < 1e-5, "{turn} goes to {to}");
        }
        // a turn that isn't a number goes upright
        assert_eq!(reset(f32::NAN, true), 0.0);
    }

    #[test]
    fn pinching_keeps_the_turn_or_the_fit_while_its_switch_is_on() {
        let parts = || (procraft_gesture_lock(TURN), procraft_gesture_lock(FIT), procraft_gesture_lock(-1));
        assert_eq!(parts(), (0, 0, 0));
        procraft_control(LOCK as i32, 1.0);
        assert_eq!((parts(), procraft_gesture_lock(2)), ((1, 0, 0), 0));
        procraft_control(NO_FIT as i32, 1.0);
        assert_eq!((parts(), procraft_gesture_lock(2)), ((1, 1, 0), 0));
        procraft_control(LOCK as i32, 0.0);
        assert_eq!(parts(), (0, 1, 0));
        procraft_control(NO_FIT as i32, 0.0);
        assert_eq!(parts(), (0, 0, 0));
    }
}

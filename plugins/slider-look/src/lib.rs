//! Slider look: the side bar's size and opacity sliders as see-through, frosted glass, the canvas
//! behind them blurred and showing through as much as its Opacity says. It's a plugin, not part of
//! the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! It answers the `slider_look` hook. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

/// The controls by their place, as the manifest starts them: how opaque the tracks are (0 to 1),
/// and whether what is behind them is frosted.
static CONTROLS: kit::Controls<2> = kit::Controls::new([0.35, 1.0]);
kit::controls!(CONTROLS);
const OPACITY: usize = 0;
const FROSTED: usize = 1;

/// What the slider look's hook asks: the tracks' opacity, and whether what is behind is frosted.
const ASKED_OPACITY: i32 = 0;
const ASKED_FROSTED: i32 = 1;

/// The tracks' look, `what` of it (0 to 1); below 0 for what it doesn't know, left as it is.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_slider_look(what: i32) -> f32 {
    match what {
        ASKED_OPACITY => {
            let v = CONTROLS.get(OPACITY);
            if v.is_nan() { -1.0 } else { v.clamp(0.0, 1.0) }
        }
        ASKED_FROSTED => {
            if CONTROLS.on(FROSTED) {
                1.0
            } else {
                0.0
            }
        }
        _ => -1.0,
    }
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
    fn the_tracks_show_through_as_its_controls_say_and_what_it_doesn_t_know_is_left() {
        // as it starts: a third opaque, frosted
        assert_eq!(procraft_slider_look(ASKED_OPACITY), 0.35);
        assert_eq!(procraft_slider_look(ASKED_FROSTED), 1.0);
        for what in [2, -1, 99] {
            assert!(procraft_slider_look(what) < 0.0, "{what}");
        }
        procraft_control(OPACITY as i32, 2.0);
        assert_eq!(procraft_slider_look(ASKED_OPACITY), 1.0, "kept to its range");
        procraft_control(OPACITY as i32, 0.1);
        procraft_control(FROSTED as i32, 0.0);
        assert_eq!((procraft_slider_look(ASKED_OPACITY), procraft_slider_look(ASKED_FROSTED)), (0.1, 0.0));
    }
}

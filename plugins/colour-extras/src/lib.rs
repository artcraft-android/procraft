//! Colour extras: two ways round the colour panel. Its sliders step one unit at a tap on either end
//! (a degree of hue, a percent of saturation or brightness, a level of red, green or blue), and its
//! swatch shows the colour as the panel opened beside the colour now, a tap on the first going back
//! to it. Each has a switch of its own. It's a plugin, not part of the app: `tools/pack_plugin.py`
//! makes a `.procraft-plugin` file of it.
//!
//! It answers the `colour_panel` hook. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

/// The controls by their place, as the manifest starts them: both off.
static CONTROLS: kit::Controls<2> = kit::Controls::new([0.0, 0.0]);
kit::controls!(CONTROLS);
const STEPS: usize = 0;
const COMPARE: usize = 1;

/// What the colour panel's hook asks: whether its sliders step at their ends, and whether its
/// swatch shows the colour before beside the colour now.
const ASKED_STEPS: i32 = 0;
const ASKED_COMPARE: i32 = 1;

/// Whether the colour panel does `part` of what it may (not 0 for yes).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_colour_panel(part: i32) -> i32 {
    let on = match part {
        ASKED_STEPS => CONTROLS.on(STEPS),
        ASKED_COMPARE => CONTROLS.on(COMPARE),
        _ => false,
    };
    i32::from(on)
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
    fn each_part_is_as_its_switch_says_and_what_it_doesn_t_know_is_no() {
        // as it starts: neither
        assert_eq!((procraft_colour_panel(ASKED_STEPS), procraft_colour_panel(ASKED_COMPARE)), (0, 0));
        procraft_control(STEPS as i32, 1.0);
        assert_eq!((procraft_colour_panel(ASKED_STEPS), procraft_colour_panel(ASKED_COMPARE)), (1, 0));
        procraft_control(COMPARE as i32, 1.0);
        procraft_control(STEPS as i32, 0.0);
        assert_eq!((procraft_colour_panel(ASKED_STEPS), procraft_colour_panel(ASKED_COMPARE)), (0, 1));
        for part in [2, -1, 99] {
            assert_eq!(procraft_colour_panel(part), 0, "{part}");
        }
    }
}

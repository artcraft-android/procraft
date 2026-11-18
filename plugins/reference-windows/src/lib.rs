//! Reference windows: more than one Reference window open at once. While it's on, + on a window's
//! top bar opens another a little below it, showing the canvas or a picture imported into it; each
//! is moved, sized, zoomed and closed on its own and kept with the artwork, as the first is. It's a
//! plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks the reference windows' hook how many may be open, and does the rest. The interface
//! is written down at the top of `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

/// The controls by their place, as the manifest starts them: how many windows at once.
static CONTROLS: kit::Controls<1> = kit::Controls::new([3.0]);
kit::controls!(CONTROLS);
const MOST: usize = 0;

/// How many Reference windows may be open at once: as the control says, 2 to 8.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_reference_windows() -> i32 {
    let most = CONTROLS.get(MOST);
    if most.is_finite() { ((most + 0.5) as i32).clamp(2, 8) } else { 2 }
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
    fn as_many_windows_as_the_control_says_within_two_to_eight() {
        assert_eq!(procraft_reference_windows(), 3);
        for (set, said) in [(5.0, 5), (2.0, 2), (8.0, 8), (20.0, 8), (0.0, 2), (f32::NAN, 2)] {
            procraft_control(MOST as i32, set);
            assert_eq!(procraft_reference_windows(), said, "{set}");
        }
    }
}

//! Floating panels: the Layers panel and the brush library each torn off into a window of its own,
//! dragged out by its grabber, while its switch is on. The window stays open while one draws, is
//! moved by its grabber, sized from its corner and closed by its Close, and the app keeps where it
//! was left. It's a plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin`
//! file of it.
//!
//! The app asks whether each panel may float, by its number, and does the rest: the windows, what
//! is in them (the panels as they hang from their buttons) and keeping them. The interface is
//! written down at the top of `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

/// The controls by their place, as the manifest starts them.
static CONTROLS: kit::Controls<2> = kit::Controls::new([1.0, 1.0]);
kit::controls!(CONTROLS);
const LAYERS: usize = 0;
const BRUSHES: usize = 1;

/// The panels the app asks about, by their number: the Layers panel, then the brush library.
const PANELS: [usize; 2] = [LAYERS, BRUSHES];

/// Whether panel `panel` may be torn off into a window of its own: as its switch says; a panel it
/// doesn't know stays where it hangs.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_floating_panel(panel: i32) -> i32 {
    let switch = usize::try_from(panel).ok().and_then(|at| PANELS.get(at).copied());
    i32::from(switch.is_some_and(|switch| CONTROLS.on(switch)))
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
    fn each_panel_floats_while_its_switch_is_on() {
        let said = || (procraft_floating_panel(0), procraft_floating_panel(1));
        // both start on
        assert_eq!(said(), (1, 1));
        procraft_control(LAYERS as i32, 0.0);
        assert_eq!(said(), (0, 1));
        procraft_control(BRUSHES as i32, 0.0);
        assert_eq!(said(), (0, 0));
        procraft_control(LAYERS as i32, 1.0);
        assert_eq!(said(), (1, 0));
        // a panel it doesn't know, and a control it doesn't have
        procraft_control(BRUSHES as i32, 1.0);
        assert_eq!(
            (procraft_floating_panel(-1), procraft_floating_panel(2), procraft_floating_panel(i32::MAX)),
            (0, 0, 0)
        );
        procraft_control(2, 0.0);
        assert_eq!(said(), (1, 1));
    }
}

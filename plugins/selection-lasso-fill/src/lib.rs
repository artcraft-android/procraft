//! Lasso Fill: a shape drawn with a lasso is filled (or erased) on the current layer, as one step,
//! instead of being selected. It offers Color Fill's Lasso Fill switch, and the Paint and Erase
//! tools' own lassos, taken up by holding the tool's button while Lasso Fill is on. It's a plugin,
//! not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question and gives it nothing else (the plugin kit answers it). Drawing the
//! shapes and filling them stay the app's: this only says they're offered. The interface is written
//! down at the top of `crates/core/src/plugin_run.rs`.

#![no_std]

// always, while the plugin is on
kit::yes!(procraft_lasso_fill);

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

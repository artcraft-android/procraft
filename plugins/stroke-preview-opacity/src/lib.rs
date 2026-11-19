//! Stroke preview opacity: a stroke shows fainter while it is drawn, as far as its slider says
//! (5% to 100% of how it will be kept), so what is under it can still be seen; once the pen
//! lifts it is kept at its own opacity, as if this weren't on. The slider is live: moved while a
//! stroke is being drawn, that stroke follows it. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks one question, with the stroke's tool, and gives it nothing else. Drawing and
//! keeping the stroke stay the app's: this only says how strongly it shows meanwhile. The
//! interface is written down at the top of `crates/core/src/plugin_run.rs`.

#![no_std]

/// Its slider (how strongly a stroke shows while drawn) and its switch (erasing and smudging too).
static CONTROLS: kit::Controls<2> = kit::Controls::new([0.5, 0.0]);
kit::controls!(CONTROLS);

/// The tools the app tells of: painting and the light tool always show fainter, the eraser and
/// the smudge only with the switch on.
const ERASE: i32 = 1;
const SMUDGE: i32 = 2;

/// How strongly a stroke of `tool` shows while it is drawn, 0 to 1 of how it will be kept.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_stroke_preview_opacity(tool: i32) -> f32 {
    if matches!(tool, ERASE | SMUDGE) && !CONTROLS.on(1) {
        return 1.0;
    }
    let shown = CONTROLS.get(0);
    // never gone altogether: a stroke nobody can see can't be drawn
    if shown >= 0.05 { shown.min(1.0) } else { 0.05 }
}

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

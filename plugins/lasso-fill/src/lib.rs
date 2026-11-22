//! Lasso fill: strokes fill what they go round, closed by a straight line back to where they
//! began, rather than painting their path. Every stroke does while its Lasso fill switch is on (in
//! Actions > Canvas, or on Alt+F), and a brush's own strokes do whenever its Fill row in the brush
//! studio says so, so a fill brush sits beside the pens. The fill shows as it grows, or as a plain
//! stroke filled as the pen lifts; the eraser takes away what it goes round only while Eraser too
//! is on. It's a plugin, not part of the app: `tools/pack_plugin.py` makes a `.procraft-plugin`
//! file of it.
//!
//! The app asks the stroke-fill hook as each stroke begins, after telling the brush's rows; how a
//! region is filled (the winding rule, the colour, the paper, the selection) is the app's. The
//! interface is written down at the top of `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

use core::sync::atomic::{AtomicU32, Ordering};

/// The controls by their place, as the manifest starts them: every stroke fills, the fill shows
/// as it grows, and the eraser fills too.
static CONTROLS: kit::Controls<3> = kit::Controls::new([0.0, 1.0, 0.0]);
kit::controls!(CONTROLS);
const EVERY: usize = 0;
const GROWING: usize = 1;
const ERASER: usize = 2;

/// How the app numbers the tools it asks about.
const PAINT: i32 = 0;
const ERASE: i32 = 1;

/// The current brush's Fill row: its choice's place (0 Off, 1 Fill what it encloses).
static ROW: AtomicU32 = AtomicU32::new(0);

/// The current brush's rows, by their place in the manifest.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_control(index: i32, value: f32) {
    if index == 0 {
        ROW.store(value.to_bits(), Ordering::Relaxed);
    }
}

/// Whether a stroke beginning with `tool` fills what it goes round: 1 shown as it grows, 2 shown
/// plainly and filled as it ends, 0 painted as usual.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_stroke_fill(tool: i32, _pointer: i32) -> i32 {
    let brush = f32::from_bits(ROW.load(Ordering::Relaxed)) >= 0.5;
    let fills = match tool {
        PAINT => CONTROLS.on(EVERY) || brush,
        ERASE => CONTROLS.on(ERASER) && (CONTROLS.on(EVERY) || brush),
        _ => false,
    };
    match (fills, CONTROLS.on(GROWING)) {
        (false, _) => 0,
        (true, true) => 1,
        (true, false) => 2,
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

    fn set(controls: [f32; 3], row: f32) {
        for (i, v) in controls.into_iter().enumerate() {
            procraft_control(i as i32, v);
        }
        procraft_brush_control(0, row);
    }

    #[test]
    fn strokes_fill_while_the_switch_or_the_brush_says_and_show_as_its_switch_says() {
        // as it starts: nothing fills
        set([0.0, 1.0, 0.0], 0.0);
        assert_eq!((procraft_stroke_fill(PAINT, 1), procraft_stroke_fill(ERASE, 1)), (0, 0));
        // every stroke, shown as it grows; the eraser only with its switch
        set([1.0, 1.0, 0.0], 0.0);
        assert_eq!((procraft_stroke_fill(PAINT, 0), procraft_stroke_fill(ERASE, 0)), (1, 0));
        set([1.0, 1.0, 1.0], 0.0);
        assert_eq!(procraft_stroke_fill(ERASE, 2), 1);
        // a brush's own Fill row, shown plainly and filled at the lift
        set([0.0, 0.0, 0.0], 1.0);
        assert_eq!((procraft_stroke_fill(PAINT, 1), procraft_stroke_fill(ERASE, 1)), (2, 0));
        // a smudge, or a tool it doesn't know, never
        set([1.0, 1.0, 1.0], 1.0);
        assert_eq!((procraft_stroke_fill(2, 1), procraft_stroke_fill(7, 1)), (0, 0));
    }
}

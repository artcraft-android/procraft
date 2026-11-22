//! Coloured brush previews: the brush library's sample strokes drawn in the colour being painted
//! with, or in a colour of the plugin's own set by its Hue, Saturation and Brightness, rather than
//! white; the brush in use, on the accent, too if its switch says. It's a plugin, not part of the
//! app: `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! It answers the `brush_preview_colour` hook. The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

/// The controls by their place, as the manifest starts them: whether it draws in a colour of its
/// own, that colour's hue (degrees), saturation and brightness (0 to 1), and whether the brush in
/// use is drawn so too.
static CONTROLS: kit::Controls<5> = kit::Controls::new([0.0, 200.0, 0.6, 1.0, 0.0]);
kit::controls!(CONTROLS);
const OWN: usize = 0;
const HUE: usize = 1;
const SATURATION: usize = 2;
const BRIGHTNESS: usize = 3;
const CHOSEN: usize = 4;

/// `hue` (degrees), `saturation` and `brightness` (0 to 1) as 0xRRGGBB.
fn rgb(hue: f32, saturation: f32, brightness: f32) -> i32 {
    let clamp = |v: f32, most: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, most) };
    let (s, v) = (clamp(saturation, 1.0), clamp(brightness, 1.0));
    // the hue in sixths of the wheel, 0 to 6 (360° the same as 0°)
    let sixths = clamp(hue, 360.0) / 60.0;
    let sector = (sixths as i32).min(5);
    let within = sixths - sector as f32;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * within), v * (1.0 - s * (1.0 - within)));
    let (r, g, b) = match sector {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    let byte = |c: f32| (c * 255.0 + 0.5) as i32;
    (byte(r) << 16) | (byte(g) << 8) | byte(b)
}

/// The colour (0xRRGGBB) a brush's sample stroke is drawn in, the current colour `current`: its
/// own, or the current; -1 (as the app draws it) for the brush in use (`chosen` not 0) unless the
/// switch says.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_brush_preview_colour(current: i32, chosen: i32) -> i32 {
    if chosen != 0 && !CONTROLS.on(CHOSEN) {
        return -1;
    }
    if CONTROLS.on(OWN) {
        rgb(CONTROLS.get(HUE), CONTROLS.get(SATURATION), CONTROLS.get(BRIGHTNESS))
    } else {
        current & 0xFF_FFFF
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
    fn a_hue_saturation_and_brightness_make_their_colour() {
        for (hue, s, v, want) in [
            (0.0, 1.0, 1.0, 0xFF0000),
            (60.0, 1.0, 1.0, 0xFFFF00),
            (120.0, 1.0, 1.0, 0x00FF00),
            (180.0, 1.0, 1.0, 0x00FFFF),
            (240.0, 1.0, 1.0, 0x0000FF),
            (300.0, 1.0, 1.0, 0xFF00FF),
            (360.0, 1.0, 1.0, 0xFF0000),
            (30.0, 1.0, 1.0, 0xFF8000),
            (0.0, 0.0, 1.0, 0xFFFFFF),
            (200.0, 0.0, 0.5, 0x808080),
            (123.0, 0.7, 0.0, 0x000000),
        ] {
            assert_eq!(rgb(hue, s, v), want, "{hue} {s} {v}");
        }
        // what isn't in range, or isn't a number, is taken to the nearest it can be
        assert_eq!(rgb(-30.0, 2.0, 7.0), 0xFF0000);
        assert_eq!(rgb(f32::NAN, f32::NAN, f32::NAN), 0x000000);
    }

    #[test]
    fn previews_go_in_the_current_colour_or_its_own_the_brush_in_use_as_its_switch_says() {
        let current = 0x12_3456;
        // as it starts: the current colour, the brush in use as the app draws it
        assert_eq!(procraft_brush_preview_colour(current, 0), current);
        assert_eq!(procraft_brush_preview_colour(current, 1), -1);
        // the current colour's bits alone, whatever comes above them
        assert_eq!(procraft_brush_preview_colour(-1, 0), 0xFF_FFFF);
        // a colour of its own, from its sliders
        procraft_control(OWN as i32, 1.0);
        procraft_control(HUE as i32, 120.0);
        procraft_control(SATURATION as i32, 1.0);
        procraft_control(BRIGHTNESS as i32, 0.5);
        assert_eq!(procraft_brush_preview_colour(current, 0), 0x008000);
        // the brush in use too
        procraft_control(CHOSEN as i32, 1.0);
        assert_eq!(procraft_brush_preview_colour(current, 1), 0x008000);
        procraft_control(OWN as i32, 0.0);
        assert_eq!(procraft_brush_preview_colour(current, 1), current);
        procraft_control(CHOSEN as i32, 0.0);
    }
}

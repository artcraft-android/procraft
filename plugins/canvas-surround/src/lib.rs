//! Canvas surround: the area round the canvas in a colour of the plugin's own, set by its Hue,
//! Saturation and Brightness (black at no brightness), or a picture someone picks, in place of the
//! interface's grey. It's a plugin, not part of the app: `tools/pack_plugin.py` makes a
//! `.procraft-plugin` file of it.
//!
//! It answers the `interface_colour` hook for the surround (role 1), leaving every other role (the
//! accent) as it is, and the `surround_picture` hook with its Picture control while its switch is
//! on: the app reads the picture someone picked and shows it covering the window, the colour where
//! it is see-through. The interface is written down at the top of `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

/// The controls by their place, as the manifest starts them: the colour's hue (degrees),
/// saturation and brightness (0 to 1), whether a picture shows, and whether one is picked.
static CONTROLS: kit::Controls<5> = kit::Controls::new([30.0, 0.1, 0.3, 0.0, 0.0]);
kit::controls!(CONTROLS);
const HUE: usize = 0;
const SATURATION: usize = 1;
const BRIGHTNESS: usize = 2;
const OWN: usize = 3;
const PICTURE: usize = 4;

/// The colour hook's role that is the area round the canvas.
const SURROUND: i32 = 1;

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

/// The colour (0xRRGGBB) the interface draws `role` in: the surround in the sliders' colour; -1 for
/// any other role, left as it is.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_interface_colour(role: i32) -> i32 {
    if role != SURROUND {
        return -1;
    }
    rgb(CONTROLS.get(HUE), CONTROLS.get(SATURATION), CONTROLS.get(BRIGHTNESS))
}

/// The place of the control whose picture shows round the canvas: Picture's while the switch is on
/// and one is picked, else none (-1).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_surround_picture() -> i32 {
    if CONTROLS.on(OWN) && CONTROLS.on(PICTURE) { PICTURE as i32 } else { -1 }
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
    fn the_surround_goes_in_the_sliders_colour_or_the_picture_and_every_other_role_as_it_is() {
        // as it starts: a dark warm grey, no picture
        assert_eq!(procraft_interface_colour(SURROUND), rgb(30.0, 0.1, 0.3));
        assert_eq!(rgb(30.0, 0.1, 0.3), 0x4D4945);
        assert_eq!(procraft_surround_picture(), -1);
        for role in [0, 2, -1] {
            assert_eq!(procraft_interface_colour(role), -1, "{role}");
        }
        procraft_control(HUE as i32, 240.0);
        procraft_control(SATURATION as i32, 1.0);
        procraft_control(BRIGHTNESS as i32, 1.0);
        assert_eq!(procraft_interface_colour(SURROUND), 0x0000FF);
        procraft_control(BRIGHTNESS as i32, 0.0);
        assert_eq!(procraft_interface_colour(SURROUND), 0x000000, "black");
        assert_eq!(procraft_interface_colour(0), -1, "the accent stays the app's");
        // a picture: only with its switch on and one picked
        procraft_control(OWN as i32, 1.0);
        assert_eq!(procraft_surround_picture(), -1, "none picked");
        procraft_control(PICTURE as i32, 1.0);
        assert_eq!(procraft_surround_picture(), PICTURE as i32);
        procraft_control(OWN as i32, 0.0);
        assert_eq!(procraft_surround_picture(), -1);
    }

    #[test]
    fn a_hue_saturation_and_brightness_make_their_colour() {
        for (hue, s, v, want) in [
            (0.0, 1.0, 1.0, 0xFF0000),
            (120.0, 1.0, 1.0, 0x00FF00),
            (360.0, 1.0, 1.0, 0xFF0000),
            (0.0, 0.0, 1.0, 0xFFFFFF),
            (200.0, 0.0, 0.5, 0x808080),
        ] {
            assert_eq!(rgb(hue, s, v), want, "{hue} {s} {v}");
        }
        assert_eq!(rgb(f32::NAN, f32::NAN, f32::NAN), 0x000000);
    }
}

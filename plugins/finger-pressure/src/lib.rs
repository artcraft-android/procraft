//! Finger pressure: a finger, which has no pressure of its own, draws with one. The faster it
//! moves the lighter it presses, from the most pressure when it goes slowly towards the least as
//! it flies, and each of its strokes tapers in from nothing and out to nothing. A pen, its eraser
//! end and a mouse draw as they always do. It's a plugin, not part of the app:
//! `tools/pack_plugin.py` makes a `.procraft-plugin` file of it.
//!
//! The app asks the pressure hook for each sample's pressure as a stroke goes, and for the
//! stroke's tapers as it begins; the brush's own pressure settings then say what the pressure does
//! (its size, its opacity). The interface is written down at the top of
//! `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

use core::sync::atomic::{AtomicU32, Ordering};

/// The controls by their place, as the manifest starts them: how much speed takes the pressure
/// down (0..1), the start and end tapers (window points), and the least and most pressure.
static CONTROLS: kit::Controls<5> = kit::Controls::new([0.7, 20.0, 40.0, 0.1, 1.0]);
kit::controls!(CONTROLS);
const SPEED: usize = 0;
const START: usize = 1;
const END: usize = 2;
const LEAST: usize = 3;
const MOST: usize = 4;

/// How the app numbers a finger.
const FINGER: i32 = 0;
/// The speed (window points a second) at which speed has taken half of what it can, and how
/// sharply it goes from none to all: slow, careful lines keep nearly all their pressure.
const HALF_SPEED: f32 = 1000.0;
/// How long (milliseconds) the speed takes to follow the finger: the time a finger's jitter
/// between samples is evened out over.
const SPEED_MS: f32 = 40.0;
/// The fastest a finger is taken to move, window points a second: a sample far from the last
/// within a moment is a jump, not speed.
const FASTEST: f32 = 20_000.0;
/// The longest taper the app takes, window points.
const LONGEST_TAPER: f32 = 1000.0;

/// What the stroke's samples so far leave: where and when the last came, the way gone since the
/// last that came later than the one before it (samples a platform hands over at one time), and
/// the speed as it is followed.
static LAST_X: AtomicU32 = AtomicU32::new(0);
static LAST_Y: AtomicU32 = AtomicU32::new(0);
static LAST_MS: AtomicU32 = AtomicU32::new(0);
static GONE: AtomicU32 = AtomicU32::new(0);
static FOLLOWED: AtomicU32 = AtomicU32::new(0);

fn get(slot: &AtomicU32) -> f32 {
    f32::from_bits(slot.load(Ordering::Relaxed))
}

fn set(slot: &AtomicU32, value: f32) {
    slot.store(value.to_bits(), Ordering::Relaxed);
}

/// The square root of `v` (0 for none), without the standard library: halving its exponent for a
/// first guess, then three of Newton's steps.
fn root(v: f32) -> f32 {
    if v.is_nan() || v <= 0.0 {
        return 0.0;
    }
    let mut r = f32::from_bits((v.to_bits() >> 1) + 0x1fbd_1df5);
    for _ in 0..3 {
        r = 0.5 * (r + v / r);
    }
    r
}

/// How much of what speed can take away a finger moving `speed` window points a second takes:
/// next to none when slow, half at [`HALF_SPEED`], nearly all when it flies.
fn speed_share(speed: f32) -> f32 {
    let s = (speed / HALF_SPEED).max(0.0);
    s * s / (1.0 + s * s)
}

/// The pressure a finger moving `speed` window points a second presses with, by the controls.
fn pressure_at(speed: f32) -> f32 {
    let (least, most) = (CONTROLS.get(LEAST).clamp(0.0, 1.0), CONTROLS.get(MOST).clamp(0.0, 1.0));
    let influence = CONTROLS.get(SPEED).clamp(0.0, 1.0);
    (most - (most - least) * influence * speed_share(speed)).clamp(0.0, 1.0)
}

/// The speed followed on to a sample at (`x`, `y`), `ms` after the stroke's first (`first`: it is
/// the first).
fn follow(first: bool, x: f32, y: f32, ms: f32) -> f32 {
    if first {
        for (slot, value) in [(&LAST_X, x), (&LAST_Y, y), (&LAST_MS, ms), (&GONE, 0.0), (&FOLLOWED, 0.0)] {
            set(slot, value);
        }
        return 0.0;
    }
    let (dx, dy) = (x - get(&LAST_X), y - get(&LAST_Y));
    let gone = get(&GONE) + root(dx * dx + dy * dy);
    set(&LAST_X, x);
    set(&LAST_Y, y);
    let dt = ms - get(&LAST_MS);
    let followed = get(&FOLLOWED);
    // samples that came at one time go together into the next that came later
    if dt.is_nan() || dt <= 0.0 {
        set(&GONE, gone);
        return followed;
    }
    set(&LAST_MS, ms);
    set(&GONE, 0.0);
    let now = (gone * 1000.0 / dt).min(FASTEST);
    // followed over [`SPEED_MS`], each sample as far as the time it covers
    let followed = followed + (now - followed) * dt / (dt + SPEED_MS);
    set(&FOLLOWED, followed);
    followed
}

/// The pressure a stroke's sample draws with: for a finger, as fast as it is going; anything else
/// as it came (below 0).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_pressure(pointer: i32, first: i32, x: f32, y: f32, ms: f32, _pressure: f32) -> f32 {
    if pointer != FINGER {
        return -1.0;
    }
    pressure_at(follow(first != 0, x, y, ms))
}

/// How far (window points) a finger's stroke's pressure grows from nothing at its start (`end` 0)
/// and falls to nothing at its end (1): the tapers' controls. Nothing for anything else.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_pressure_taper(pointer: i32, end: i32) -> f32 {
    let control = match end {
        0 => START,
        1 => END,
        _ => return 0.0,
    };
    if pointer != FINGER {
        return 0.0;
    }
    CONTROLS.get(control).clamp(0.0, LONGEST_TAPER)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A finger moving across at `speed` window points a second, a sample every `every` ms, for
    /// `ms`: the pressure of its last sample.
    fn steady(speed: f32, every: f32, ms: f32) -> f32 {
        let mut last = procraft_pressure(FINGER, 1, 0.0, 0.0, 0.0, 1.0);
        let mut t = every;
        while t <= ms {
            last = procraft_pressure(FINGER, 0, speed * t / 1000.0, 0.0, t, 1.0);
            t += every;
        }
        last
    }

    #[test]
    fn the_root_is_the_standard_library_s() {
        for v in [1e-6_f32, 0.01, 0.5, 1.0, 2.0, 9.0, 1234.5, 4e6, 1e12] {
            assert!((root(v) - v.sqrt()).abs() <= v.sqrt() * 1e-6, "{v}: {} {}", root(v), v.sqrt());
        }
        assert_eq!((root(0.0), root(-4.0), root(f32::NAN)), (0.0, 0.0, 0.0));
    }

    #[test]
    fn the_faster_a_finger_goes_the_lighter_it_presses_and_only_a_finger() {
        // (one test: the controls and what a stroke leaves are the module's)
        let at = |speed: f32| steady(speed, 1000.0 / 120.0, 400.0);
        let (slow, middling, fast) = (at(200.0), at(1000.0), at(3000.0));
        println!("at 200, 1000 and 3000 points a second: {slow:.3}, {middling:.3}, {fast:.3}");
        assert!((slow - 0.976).abs() < 0.005, "{slow}");
        assert!((middling - 0.685).abs() < 0.005, "{middling}");
        assert!((fast - 0.433).abs() < 0.005, "{fast}");
        // still: the most; at its first sample too
        assert_eq!(at(0.0), 1.0);
        assert_eq!(procraft_pressure(FINGER, 1, 5.0, 5.0, 0.0, 0.3), 1.0);
        // a pen, its eraser end and a mouse are left as they came, with no tapers
        for pointer in [1, 2, 3] {
            assert!(procraft_pressure(pointer, 1, 0.0, 0.0, 0.0, 0.4) < 0.0);
            assert!(procraft_pressure(pointer, 0, 10.0, 0.0, 8.0, 0.4) < 0.0);
            assert_eq!((procraft_pressure_taper(pointer, 0), procraft_pressure_taper(pointer, 1)), (0.0, 0.0));
        }
        assert_eq!((procraft_pressure_taper(FINGER, 0), procraft_pressure_taper(FINGER, 1)), (20.0, 40.0));
        assert_eq!(procraft_pressure_taper(FINGER, 2), 0.0);
        // samples handed over at one time count with the next that came later
        let batched = {
            procraft_pressure(FINGER, 1, 0.0, 0.0, 0.0, 1.0);
            let mut last = 0.0;
            for i in 1..=48 {
                // two samples at each time, a step apart
                let t = (i / 2 * 8) as f32;
                last = procraft_pressure(FINGER, 0, i as f32 * 4.0, 0.0, t, 1.0);
            }
            last
        };
        assert!((batched - at(1000.0)).abs() < 0.05, "{batched}");
        // the controls: no influence, none at all; the range is the least and the most
        procraft_control(SPEED as i32, 0.0);
        assert_eq!(at(3000.0), 1.0);
        procraft_control(SPEED as i32, 1.0);
        procraft_control(LEAST as i32, 0.25);
        procraft_control(MOST as i32, 0.75);
        assert!((at(100_000.0) - 0.25).abs() < 0.01 && (at(0.0) - 0.75).abs() < 1e-6);
        procraft_control(END as i32, 0.0);
        assert_eq!(procraft_pressure_taper(FINGER, 1), 0.0);
        procraft_control(START as i32, 5000.0);
        assert_eq!(procraft_pressure_taper(FINGER, 0), LONGEST_TAPER);
    }
}

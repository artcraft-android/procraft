//! Paper: a paper for each artwork, chosen in the Actions panel's Canvas tab. Every brush takes its
//! grain as it paints, a grain fixed to the canvas on top of the brush's own: where the paper is
//! low its hollows take a share of the paint (Grain says how much). The paper shows under the art,
//! in a colour of its own or the artwork's background colour, its hollows a little darker. Each
//! artwork keeps its own paper. The papers are drawn here, a tile that repeats seamlessly: no
//! picture of anyone's. It's a plugin, not part of the app: `tools/pack_plugin.py` makes a
//! `.procraft-plugin` file of it.
//!
//! The app asks the paper's hook for the tile and how it is taken. The interface is written down
//! at the top of `crates/core/src/plugin_run.rs`.

#![cfg_attr(not(test), no_std)]

use core::cell::UnsafeCell;
use core::sync::atomic::{AtomicU32, Ordering};

/// The controls by their place, as the manifest starts them.
static CONTROLS: kit::Controls<5> = kit::Controls::new([0.0, 0.0, 0.6, 1.0, 0.3]);
kit::controls!(CONTROLS);
const PAPER: usize = 0;
const COLOUR: usize = 1;
const GRAIN: usize = 2;
const SIZE: usize = 3;
const SHOWN: usize = 4;

/// The tile's side: what the app asks for.
const SIDE: usize = 256;

/// The papers, by the Paper control's steps (0 is none).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    ColdPress,
    HotPress,
    Rough,
    Canvas,
    Laid,
}

impl Kind {
    fn of(step: f32) -> Option<Kind> {
        match (step + 0.5) as i32 {
            1 => Some(Kind::ColdPress),
            2 => Some(Kind::HotPress),
            3 => Some(Kind::Rough),
            4 => Some(Kind::Canvas),
            5 => Some(Kind::Laid),
            _ => None,
        }
    }
}

/// The colours under the art, by the Paper colour control's steps from White (0xRRGGBB); the
/// first step is the artwork's own background colour.
const COLOURS: [i32; 6] = [0xFFFFFF, 0xF4ECD8, 0xF8F4E6, 0xC9C9C6, 0xC2A27A, 0x1C1C1C];

/// A number from a lattice point and a seed, 0 to 1.
fn hash(x: u32, y: u32, seed: u32) -> f32 {
    let mut h = x.wrapping_mul(0x8DA6_B343) ^ y.wrapping_mul(0xD816_3841) ^ seed.wrapping_mul(0xCB1A_B31F);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5BD1_E995);
    h ^= h >> 15;
    (h & 0xFFFF) as f32 / 65535.0
}

/// The most lattice points across a tile: its finest cells are 2 pixels.
const MOST_CELLS: usize = SIDE / 2;

/// Smooth noise on a lattice of cells `cell` pixels a side, repeating every [`SIDE`] pixels (`cell`
/// divides it, 2 at the least), `strength` times as strong, added to `sum` (a tile, rows from the
/// top). Each row is worked out down the lattice's columns first, then across: two hashes a
/// column a row, and a few steps a pixel.
fn add_noise(sum: &mut [f32; SIDE * SIDE], cell: usize, seed: u32, strength: f32) {
    let cell = cell.clamp(2, SIDE);
    let n = SIDE / cell;
    let ease = |t: f32| t * t * (3.0 - 2.0 * t);
    // how far across its cell each column is, eased
    let mut across = [0.0_f32; SIDE];
    for (x, f) in across.iter_mut().enumerate() {
        *f = ease((x % cell) as f32 / cell as f32);
    }
    let mut column = [0.0_f32; MOST_CELLS + 1];
    for (y, row) in sum.chunks_exact_mut(SIDE).enumerate() {
        let (cy, fy) = (y / cell, ease((y % cell) as f32 / cell as f32));
        let (j0, j1) = ((cy % n) as u32, ((cy + 1) % n) as u32);
        for (i, v) in column.iter_mut().take(n + 1).enumerate() {
            let i = (i % n) as u32;
            let (top, bottom) = (hash(i, j0, seed), hash(i, j1, seed));
            *v = top + (bottom - top) * fy;
        }
        for (x, (pixel, fx)) in row.iter_mut().zip(across).enumerate() {
            let cx = x / cell;
            let (left, right) = (column.get(cx).copied().unwrap_or(0.0), column.get(cx + 1).copied().unwrap_or(0.0));
            *pixel += (left + (right - left) * fx) * strength;
        }
    }
}

/// Octaves of noise from cells `cells` pixels a side, each half as strong as the last, as a tile:
/// 0 to 1.
fn fractal(out: &mut [f32; SIDE * SIDE], cells: &[usize], seed: u32) {
    out.fill(0.0);
    let (mut weight, mut strength) = (0.0, 1.0);
    for (i, &cell) in cells.iter().enumerate() {
        add_noise(out, cell, seed.wrapping_add(i as u32 * 101), strength);
        weight += strength;
        strength *= 0.5;
    }
    for v in out.iter_mut() {
        *v /= weight;
    }
}

/// `v` pushed away from 0.5 by `k` (1 keeps it), within 0 to 1.
fn contrast(v: f32, k: f32) -> f32 {
    ((v - 0.5) * k + 0.5).clamp(0.0, 1.0)
}

/// A triangle wave over `period` pixels at `at`: 0 at its ends, 1 in its middle.
fn ridge(at: usize, period: usize) -> f32 {
    let t = (at % period) as f32 / period as f32;
    let d = 2.0 * t - 1.0;
    1.0 - if d < 0.0 { -d } else { d }
}

/// How high paper `kind` stands at each pixel of its tile, 0 (a hollow) to 1 (a peak), into `out`.
fn heights(kind: Kind, out: &mut [f32; SIDE * SIDE]) {
    let each = |out: &mut [f32; SIDE * SIDE], f: &dyn Fn(usize, usize, f32) -> f32| {
        for (i, v) in out.iter_mut().enumerate() {
            *v = f(i % SIDE, i / SIDE, *v);
        }
    };
    match kind {
        // medium tooth, blotchy: the hollows a strong brush still fills
        Kind::ColdPress => {
            fractal(out, &[32, 16, 8, 4], 11);
            each(out, &|_, _, v| contrast(v, 1.8));
        }
        // smooth, the tooth fine and shallow
        Kind::HotPress => {
            fractal(out, &[8, 4, 2], 23);
            each(out, &|_, _, v| 0.75 + 0.25 * contrast(v, 1.4));
        }
        // coarse, deep tooth
        Kind::Rough => {
            fractal(out, &[64, 32, 16, 8, 4], 37);
            each(out, &|_, _, v| contrast(v, 2.4));
        }
        // threads over and under each other, eight pixels apart, a little uneven
        Kind::Canvas => {
            fractal(out, &[16, 4], 41);
            each(out, &|x, y, v| {
                let over = (x / 8 + y / 8) % 2 == 0;
                let thread = if over { ridge(y, 8) } else { ridge(x, 8) };
                (0.15 + 0.75 * thread + 0.1 * v).clamp(0.0, 1.0)
            });
        }
        // fine laid lines across, chain lines down every 64 pixels, a light tooth over both
        Kind::Laid => {
            fractal(out, &[16, 8], 53);
            each(out, &|x, y, v| {
                let chain = if x % 64 < 3 { 0.35 } else { 0.0 };
                (0.35 + 0.45 * ridge(y, 4) - chain + 0.2 * v).clamp(0.0, 1.0)
            });
        }
    }
}

/// Memory of the module's own, written by one call at a time (a plugin's module runs on one thread).
struct Owned<T>(UnsafeCell<T>);
// SAFETY: a plugin's module runs on one thread at a time
unsafe impl<T> Sync for Owned<T> {}

/// The tile as last drawn, its heights as they were worked out, and which paper that was (its
/// step, 0 for none yet).
static TILE: Owned<[u8; SIDE * SIDE]> = Owned(UnsafeCell::new([0; SIDE * SIDE]));
static HEIGHTS: Owned<[f32; SIDE * SIDE]> = Owned(UnsafeCell::new([0.0; SIDE * SIDE]));
static DRAWN: AtomicU32 = AtomicU32::new(0);

/// The tile of paper `kind`, drawn again only when the paper changes.
fn tile(kind: Kind) -> &'static [u8; SIDE * SIDE] {
    let step = kind as u32 + 1;
    // SAFETY: only this function writes them, never while another call reads them (one thread)
    let (pixels, worked) = unsafe { (&mut *TILE.0.get(), &mut *HEIGHTS.0.get()) };
    if DRAWN.load(Ordering::Relaxed) != step {
        heights(kind, worked);
        for (pixel, h) in pixels.iter_mut().zip(worked.iter()) {
            *pixel = (h * 255.0 + 0.5) as u8;
        }
        DRAWN.store(step, Ordering::Relaxed);
    }
    pixels
}

/// Where the paper's tile is, `side` square (the app asks for [`SIDE`]); 0 for no paper.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_paper(side: i32) -> i32 {
    match Kind::of(CONTROLS.get(PAPER)) {
        Some(kind) if side == SIDE as i32 => tile(kind).as_ptr() as i32,
        _ => 0,
    }
}

/// How the paper is taken: how strongly its hollows take paint away (0), how many canvas pixels a
/// pixel of its tile covers (1), how deep it shows under the art (2).
#[unsafe(no_mangle)]
pub extern "C" fn procraft_paper_look(what: i32) -> f32 {
    match what {
        0 => CONTROLS.get(GRAIN).clamp(0.0, 1.0),
        1 => CONTROLS.get(SIZE).clamp(0.25, 16.0),
        2 => CONTROLS.get(SHOWN).clamp(0.0, 1.0),
        _ => 0.0,
    }
}

/// The colour under the art, or -1 for the artwork's own.
#[unsafe(no_mangle)]
pub extern "C" fn procraft_paper_colour() -> i32 {
    let step = (CONTROLS.get(COLOUR) + 0.5) as usize;
    step.checked_sub(1).and_then(|at| COLOURS.get(at)).copied().unwrap_or(-1)
}

#[cfg(not(test))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    core::arch::wasm32::unreachable()
}

#[cfg(test)]
mod tests {
    use super::*;

    const KINDS: [Kind; 5] = [Kind::ColdPress, Kind::HotPress, Kind::Rough, Kind::Canvas, Kind::Laid];

    /// Paper `kind`'s heights, worked out as its tile is.
    fn worked(kind: Kind) -> Box<[f32; SIDE * SIDE]> {
        let mut out: Box<[f32; SIDE * SIDE]> = vec![0.0; SIDE * SIDE].into_boxed_slice().try_into().unwrap();
        heights(kind, &mut out);
        out
    }

    #[test]
    fn each_paper_repeats_without_a_seam_and_has_a_tooth() {
        for kind in KINDS {
            let all = worked(kind);
            let at = |x: usize, y: usize| all[(y % SIDE) * SIDE + x % SIDE];
            // across the tile's edge it goes on as it does inside: no jump bigger than inside
            let step = |a: f32, b: f32| (a - b).abs();
            let inside: f32 = (1..SIDE).map(|x| step(at(x, 7), at(x - 1, 7))).fold(0.0, f32::max);
            assert!(step(at(0, 7), at(SIDE - 1, 7)) <= inside + 1e-6, "{kind:?} across");
            let inside: f32 = (1..SIDE).map(|y| step(at(7, y), at(7, y - 1))).fold(0.0, f32::max);
            assert!(step(at(7, 0), at(7, SIDE - 1)) <= inside + 1e-6, "{kind:?} down");
            // hollows and peaks, all within 0 to 1
            let all: Vec<f32> = (0..SIDE * SIDE).map(|i| at(i % SIDE, i / SIDE)).collect();
            let (low, high) = all.iter().fold((1.0_f32, 0.0_f32), |(l, h), &v| (l.min(v), h.max(v)));
            assert!(low >= 0.0 && high <= 1.0 && high - low > 0.2, "{kind:?}: {low} to {high}");
        }
        // each paper its own
        let first: Vec<u8> = KINDS.iter().map(|&k| (worked(k)[50 * SIDE + 30] * 255.0) as u8).collect();
        assert!(first.windows(2).any(|w| w[0] != w[1]));
    }

    #[test]
    fn the_app_is_given_the_paper_chosen_and_none_for_none() {
        procraft_control(PAPER as i32, 0.0);
        assert_eq!(procraft_paper(SIDE as i32), 0);
        procraft_control(PAPER as i32, 3.0);
        assert_ne!(procraft_paper(SIDE as i32), 0);
        // a side it doesn't draw
        assert_eq!(procraft_paper(128), 0);
        assert_eq!(tile(Kind::Rough)[5], (worked(Kind::Rough)[5] * 255.0 + 0.5) as u8);
        // how it's taken, as the controls say, within the app's ranges
        procraft_control(GRAIN as i32, 0.4);
        procraft_control(SIZE as i32, 2.0);
        assert_eq!((procraft_paper_look(0), procraft_paper_look(1), procraft_paper_look(2)), (0.4, 2.0, 0.3));
        assert_eq!(procraft_paper_look(9), 0.0);
        // the artwork's background colour, then each of its own
        procraft_control(COLOUR as i32, 0.0);
        assert_eq!(procraft_paper_colour(), -1);
        procraft_control(COLOUR as i32, 1.0);
        assert_eq!(procraft_paper_colour(), 0xFFFFFF);
        procraft_control(COLOUR as i32, 6.0);
        assert_eq!(procraft_paper_colour(), 0x1C1C1C);
    }
}

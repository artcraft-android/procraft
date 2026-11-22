//! Our shapes and grains drawn bigger than brushes hold them ([`TEXTURE_SIZE`]), the same pictures
//! sharper, for a stamp or a grain's tile bigger than that on the canvas: each drawn the first time
//! a brush wants it at a size, off the thread that draws (the stroke that asked draws with the
//! smaller one meanwhile; choosing the brush or its size asks ahead), and kept while it is among
//! the last few wanted, within [`HELD_BYTES`]. Brush files' own big images go their own way (see
//! `grain_store.rs`).

use std::collections::HashSet;
use std::sync::{Arc, LazyLock, Mutex, PoisonError};

use procraft_engine::brush::{Brush, GrainMode, Pixels};
use procraft_engine::textures::{self, SHARPEST, TEXTURE_SIZE};

/// How much a stamp or tile may be magnified before it is drawn bigger: at most this many canvas
/// pixels to each of its texels (a quarter more is hardly seen, and keeps the sizes just past
/// [`TEXTURE_SIZE`] from each drawing a picture of their own).
const MAGNIFIED_MOST: f32 = 1.25;

/// How many bytes of bigger pictures are kept at once (two shapes and two grains at 2048 a side,
/// or many more smaller ones).
const HELD_BYTES: usize = 16 << 20;

/// What drawing one `side` pixels across holds for a while beyond the picture itself, bytes a
/// pixel (a stand-in's values in floats, their blurs and their order): a system short of that
/// draws a smaller one.
const WORK_PER_PIXEL: u64 = 32;

/// One picture of ours wanted bigger: what tells it apart, its side, and how it is drawn.
#[derive(Clone)]
pub(crate) struct Wanted {
    pub(crate) id: u64,
    pub(crate) side: u32,
    draw: Draw,
}

#[derive(Clone)]
enum Draw {
    Shape(u32),
    Grain(u32),
    Source(Pixels),
}

impl Wanted {
    fn drawn(&self) -> Option<Vec<u8>> {
        let side = self.side as usize;
        match &self.draw {
            Draw::Shape(i) => textures::shape_at(*i, side),
            Draw::Grain(i) => textures::grain_at(*i, side),
            Draw::Source(pixels) => pixels.sharper(side),
        }
    }
}

#[derive(Default)]
struct Held {
    /// Drawn, the latest wanted last.
    ready: Vec<(u64, Arc<Vec<u8>>)>,
    /// Being drawn.
    drawing: HashSet<u64>,
}

static HELD: LazyLock<Mutex<Held>> = LazyLock::new(Default::default);

#[cfg(test)]
std::thread_local! {
    /// None drawn bigger for this thread's strokes (a test's, to set beside them).
    pub(crate) static OFF: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// The side a stamp or tile `pixels` canvas pixels across wants its picture drawn at, up to `most`
/// (and [`SHARPEST`]): the least power of two that magnifies it no more than [`MAGNIFIED_MOST`];
/// none where [`TEXTURE_SIZE`] does (or the system is short of memory for even the next size up).
pub(crate) fn side_for(pixels: f32, most: usize) -> Option<u32> {
    if !pixels.is_finite() || pixels <= TEXTURE_SIZE as f32 * MAGNIFIED_MOST {
        return None;
    }
    let wanted = ((pixels / MAGNIFIED_MOST).ceil() as usize).next_power_of_two();
    let mut side = wanted.min(most).min(SHARPEST);
    while side > TEXTURE_SIZE && crate::device::short_of_memory((side * side) as u64 * WORK_PER_PIXEL) {
        side /= 2;
    }
    (side > TEXTURE_SIZE).then_some(side as u32)
}

/// `brush`'s shape wanted drawn bigger for a stamp `diameter` canvas pixels across, if it is one
/// of ours that shows more so: none for the round tip, a brush file's own image, a picture of the
/// user's own, or one soft all through.
pub(crate) fn shape_wanted(brush: &Brush, diameter: f32) -> Option<Wanted> {
    if brush.shape_file_now().is_some() {
        return None;
    }
    let (draw, most) = match (&brush.shape.source, brush.shape.image) {
        (Some(source), _) => (Draw::Source(source.pixels.clone()), source.pixels.most_side()),
        (None, Some(i)) if textures::shape_sharpens(i) => (Draw::Shape(i), SHARPEST),
        _ => return None,
    };
    let side = side_for(diameter, most)?;
    let what = match &draw {
        Draw::Source(_) => brush.shape.source.as_ref().map_or(0, |s| s.id),
        Draw::Shape(i) | Draw::Grain(i) => u64::from(*i),
    };
    Some(Wanted { id: id_of(0, what, side), side, draw })
}

/// `brush`'s grain wanted drawn bigger for a stamp `diameter` canvas pixels across (its tile on the
/// canvas from that, moving with the stamps, or fixed to the canvas), as [`shape_wanted`] says.
pub(crate) fn grain_wanted(brush: &Brush, diameter: f32) -> Option<Wanted> {
    if brush.grain_file_now().is_some_and(|f| f.full.is_some()) || brush.grain.depth <= 0.0 {
        return None;
    }
    let (draw, most) = match (&brush.grain.source, brush.grain.image) {
        (Some(source), _) => (Draw::Source(source.pixels.clone()), source.pixels.most_side()),
        (None, Some(i)) if textures::grain_sharpens(i) => (Draw::Grain(i), SHARPEST),
        _ => return None,
    };
    let moving = brush.grain.mode == GrainMode::Moving || brush.grain_each_stamp();
    let tile = if moving {
        let square = diameter / if brush.round_tip() { procraft_engine::brushfile::ROUND_TIP_SHARE } else { 1.0 };
        brush.grain.moving_tile(square, brush.setting("grain_zoom"), 1.0)
    } else {
        brush.grain_tile()
    };
    let side = side_for(tile, most)?;
    let what = match &draw {
        Draw::Source(_) => brush.grain.source.as_ref().map_or(0, |s| s.id),
        Draw::Shape(i) | Draw::Grain(i) => u64::from(*i),
    };
    Some(Wanted { id: id_of(1, what, side), side, draw })
}

/// What tells a picture drawn bigger apart: FNV-1a of what it is (a shape or a grain), which (our
/// picture's place, or a stand-in's id) and its side.
fn id_of(kind: u64, what: u64, side: u32) -> u64 {
    [0x5348_4152_5045_5200 | kind, what, u64::from(side)]
        .into_iter()
        .flat_map(u64::to_le_bytes)
        .fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3))
}

/// `wanted`'s pixels if they are drawn; else starts drawing them off this thread (unless that is
/// under way already) and gives none, or with `now`, draws them here and gives them.
pub(crate) fn pixels(wanted: &Wanted, now: bool) -> Option<Arc<Vec<u8>>> {
    #[cfg(test)]
    if OFF.with(std::cell::Cell::get) {
        return None;
    }
    {
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(at) = held.ready.iter().position(|(id, _)| *id == wanted.id) {
            let entry = held.ready.remove(at);
            let pixels = Arc::clone(&entry.1);
            held.ready.push(entry);
            return Some(pixels);
        }
        if !now && !held.drawing.insert(wanted.id) {
            return None;
        }
    }
    if now {
        let drawn = Arc::new(wanted.drawn()?);
        keep(wanted.id, Arc::clone(&drawn));
        return Some(drawn);
    }
    let job = wanted.clone();
    let thread = std::thread::Builder::new().name("procraft-sharper".to_owned());
    let started = thread.spawn(move || {
        let drawn = job.drawn();
        let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
        held.drawing.remove(&job.id);
        drop(held);
        if let Some(pixels) = drawn {
            keep(job.id, Arc::new(pixels));
        }
    });
    if let Err(e) = started {
        log::warn!("drawing a brush's picture sharper: {e}");
        HELD.lock().unwrap_or_else(PoisonError::into_inner).drawing.remove(&wanted.id);
    }
    None
}

/// Keeps `pixels` as `id`'s, letting go of the longest unwanted past [`HELD_BYTES`].
fn keep(id: u64, pixels: Arc<Vec<u8>>) {
    let mut held = HELD.lock().unwrap_or_else(PoisonError::into_inner);
    held.ready.retain(|(other, _)| *other != id);
    held.ready.push((id, pixels));
    while held.ready.len() > 1 && held.ready.iter().map(|(_, p)| p.len()).sum::<usize>() > HELD_BYTES {
        held.ready.remove(0);
    }
}

/// Lets go of every picture drawn bigger (the system short of memory): drawn again when next
/// wanted.
pub(crate) fn let_go() {
    HELD.lock().unwrap_or_else(PoisonError::into_inner).ready.clear();
}

/// How many bytes of pictures drawn bigger are kept now.
#[cfg(test)]
pub(crate) fn held_bytes() -> usize {
    HELD.lock().unwrap_or_else(PoisonError::into_inner).ready.iter().map(|(_, p)| p.len()).sum()
}

#[cfg(test)]
mod tests {
    use procraft_engine::brush::{Brush, Source, Taper};
    use procraft_engine::textures::{SHARPEST, TEXTURE_SIZE, grain, shape};

    use super::*;
    use crate::{CanvasSession, PenSample, PointerKind};

    #[test]
    fn a_stamp_wants_the_least_power_of_two_that_magnifies_it_a_quarter_at_most() {
        assert_eq!(side_for(200.0, SHARPEST), None);
        assert_eq!(side_for(320.0, SHARPEST), None);
        assert_eq!(side_for(321.0, SHARPEST), Some(512));
        assert_eq!(side_for(500.0, SHARPEST), Some(512));
        assert_eq!(side_for(640.0, SHARPEST), Some(512));
        assert_eq!(side_for(641.0, SHARPEST), Some(1024));
        assert_eq!(side_for(2560.0, SHARPEST), Some(2048));
        assert_eq!(side_for(9000.0, SHARPEST), Some(SHARPEST as u32));
        // no bigger than the picture goes, nor anything for one that goes no bigger
        assert_eq!(side_for(2000.0, 1024), Some(1024));
        assert_eq!(side_for(2000.0, TEXTURE_SIZE), None);
        assert_eq!(side_for(f32::NAN, SHARPEST), None);
        // a system short of memory draws a smaller one (what drawing 2048 holds for a while, 128 MB,
        // leaves it short; 1024's 32 MB doesn't)
        crate::device::MEMORY_FREE.with(|m| m.set(Some(600 << 20)));
        assert_eq!(side_for(2560.0, SHARPEST), Some(1024));
        crate::device::MEMORY_FREE.with(|m| m.set(Some(100 << 20)));
        assert_eq!(side_for(2560.0, SHARPEST), Some(512));
        crate::device::MEMORY_FREE.with(|m| m.set(None));
    }

    #[test]
    fn only_our_pictures_that_show_more_are_wanted_bigger() {
        let with_shape = |image: Option<u32>| {
            let mut b = Brush::round_pen();
            b.shape.image = image;
            b
        };
        // the round tip is drawn as it is at any size; a soft round shows nothing more
        assert!(shape_wanted(&with_shape(None), 1000.0).is_none());
        assert!(shape_wanted(&with_shape(Some(shape::SOFT_ROUND)), 1000.0).is_none());
        // a block does, from a stamp past 320 pixels
        assert!(shape_wanted(&with_shape(Some(shape::BLOCK)), 300.0).is_none());
        let block = shape_wanted(&with_shape(Some(shape::BLOCK)), 1000.0).unwrap();
        assert_eq!(block.side, 1024);
        // a picture of the user's own goes no bigger than it is
        let mut own = with_shape(None);
        own.shape.source = Source::new(vec![255; TEXTURE_SIZE * TEXTURE_SIZE]);
        assert!(shape_wanted(&own, 1000.0).is_none());
        // a grain by its tile: fixed to the canvas, its scale
        let mut b = Brush::round_pen();
        b.grain.image = Some(grain::GRUNGE);
        b.grain.depth = 1.0;
        b.grain.scale = 1000.0;
        assert_eq!(grain_wanted(&b, 50.0).map(|w| w.side), Some(1024));
        b.grain.scale = 200.0;
        assert!(grain_wanted(&b, 50.0).is_none());
        // none for a grain that isn't there
        b.grain.scale = 1000.0;
        b.grain.depth = 0.0;
        assert!(grain_wanted(&b, 50.0).is_none());
        // each picture at each size told apart
        let other = shape_wanted(&with_shape(Some(shape::STAR)), 1000.0).unwrap();
        let smaller = shape_wanted(&with_shape(Some(shape::BLOCK)), 500.0).unwrap();
        assert!(block.id != other.id && block.id != smaller.id);
    }

    /// Wanted, a picture is drawn off the thread that asked (which draws with the smaller one
    /// meanwhile), and is there once drawn; the pictures kept never pass their bytes.
    #[test]
    fn a_picture_wanted_bigger_is_drawn_aside_and_kept_within_bounds() {
        let mut b = Brush::round_pen();
        b.shape.image = Some(shape::HEART);
        let wanted = shape_wanted(&b, 1500.0).unwrap();
        assert_eq!(wanted.side, 2048);
        let started = std::time::Instant::now();
        let first = pixels(&wanted, false);
        let asked = started.elapsed();
        // (another test may have drawn it already)
        if first.is_none() {
            assert!(asked < std::time::Duration::from_millis(100), "asking took {asked:?}");
        }
        let drawn = loop {
            if let Some(p) = pixels(&wanted, false) {
                break p;
            }
            assert!(started.elapsed() < std::time::Duration::from_secs(60), "not drawn");
            std::thread::sleep(std::time::Duration::from_millis(20));
        };
        assert_eq!(drawn.len(), 2048 * 2048);
        // as drawn here
        assert_eq!(*drawn, textures::shape_at(shape::HEART, 2048).unwrap());
        // many more kept: never past the bytes, the latest kept
        for k in 0..8u64 {
            keep(u64::MAX - k, Arc::new(vec![0; 2048 * 2048]));
            assert!(held_bytes() <= HELD_BYTES, "{} bytes kept", held_bytes());
        }
        assert!(HELD.lock().unwrap().ready.iter().any(|(id, _)| *id == u64::MAX - 7));
        let_go();
    }

    /// Choosing a brush or its size asks for its pictures bigger ahead, off the thread that asks:
    /// a big block's shape (and its second's), none for a small one.
    #[test]
    fn a_big_brush_s_pictures_are_asked_for_ahead() {
        let s = CanvasSession::new(64, 64);
        {
            let mut st = s.shared.lock();
            let tool = st.tool;
            let brush = st.brush_mut(tool).unwrap();
            let mut block = Brush { name: "Block".to_owned(), spacing: 1.0, ..Brush::round_pen() };
            block.shape.image = Some(shape::TRIANGLE);
            (block.min_size, block.max_size) = (100.0, 900.0);
            let mut second = block.clone();
            second.shape.image = Some(shape::DIAMOND);
            block.secondary = Some(Box::new(second));
            *brush = block;
        }
        s.set_brush_size(0.1);
        assert!(s.shared.lock().ask_sharper().is_empty());
        s.set_brush_size(1.0);
        let wanted = s.shared.lock().ask_sharper();
        assert_eq!(wanted.iter().map(|w| w.side).collect::<Vec<_>>(), vec![1024, 1024]);
        let started = std::time::Instant::now();
        while wanted.iter().any(|w| pixels(w, false).is_none()) {
            assert!(started.elapsed() < std::time::Duration::from_secs(60), "not drawn");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }

    /// The side of the canvas [`one_stamp`] draws on.
    const CANVAS: usize = 1024;

    /// One stamp of `brush` in white at full size (`diameter` pixels across, its least and most
    /// sizes), in the middle of a canvas [`CANVAS`] square, its pictures as brushes hold them or
    /// (`sharper`) drawn bigger: the layer's alpha (0..1), row by row; none without a GPU.
    fn one_stamp(mut brush: Brush, diameter: f32, sharper: bool) -> Option<Vec<f32>> {
        let gpu = procraft_gpu::Gpu::headless().ok()?;
        let side = CANVAS as u32;
        let s = CanvasSession::new(side, side);
        {
            let mut st = s.shared.lock();
            st.window_size = (side, side);
            st.view = Some(procraft_engine::view::View::fit((side, side), (side, side)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
            (brush.pen_taper, brush.touch_taper) = (Taper::NONE, Taper::NONE);
            brush.streamline = 0.0;
            // far apart, so that it can be big (one stamp is drawn)
            brush.spacing = 1.0;
            (brush.min_size, brush.max_size) = (diameter, diameter);
            let tool = st.tool;
            *st.brush_mut(tool)? = brush;
        }
        s.set_brush_size(1.0);
        s.set_color(1.0, 1.0, 1.0);
        OFF.with(|off| off.set(!sharper));
        let middle = side as f32 / 2.0;
        let at =
            PenSample { x: middle, y: middle, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: 0.0, roll: 0.0 };
        s.stroke_begin(PointerKind::Finger, vec![at]);
        s.stroke_end(vec![]);
        OFF.with(|off| off.set(false));
        let mut guard = s.shared.lock();
        let st = &mut *guard;
        let id = st.doc.current();
        let (canvas, gpu) = (st.canvas.as_mut()?, st.gpu.as_ref()?);
        canvas.render_finished(gpu, &mut st.doc);
        let rgba = canvas.read_layer(gpu, id).ok()?;
        Some(rgba.chunks(4).map(|p| f32::from(p[3]) / 255.0).collect())
    }

    /// The width of the left edge of a block stamp `diameter` pixels across, from a tenth of full to
    /// nine tenths (canvas pixels, the median over the rows through its middle), drawn with our
    /// shape as brushes hold it or (`sharper`) drawn bigger; none without a GPU.
    fn block_edge(diameter: f32, sharper: bool) -> Option<f32> {
        let mut block = Brush { name: "Block".to_owned(), ..Brush::round_pen() };
        block.shape.image = Some(shape::BLOCK);
        let alpha = one_stamp(block, diameter, sharper)?;
        let a = |x: usize, y: usize| alpha.get(y * CANVAS + x).copied().unwrap_or(0.0);
        // where along a row its value first passes `level`, between pixels
        let crossing = |y: usize, level: f32| {
            (1..CANVAS / 2).find(|&x| a(x, y) >= level).map(|x| {
                let (before, now) = (a(x - 1, y), a(x, y));
                x as f32 - 1.0 + (level - before) / (now - before).max(1e-6)
            })
        };
        let mut widths: Vec<f32> = (480..545).filter_map(|y| Some(crossing(y, 0.9)? - crossing(y, 0.1)?)).collect();
        widths.sort_by(f32::total_cmp);
        widths.get(widths.len() / 2).copied()
    }

    /// The issue's case: a block 500 pixels across. Its edge, a ramp of a few texels, was magnified
    /// twice over (some 5.6 pixels); drawn bigger it is as crisp as at the size brushes hold (some
    /// 3, a ramp of 2.6 texels read between texels), and stays so however big.
    #[test]
    fn a_block_500_pixels_across_has_a_crisp_edge() {
        let (Some(before), Some(after)) = (block_edge(500.0, false), block_edge(500.0, true)) else { return };
        eprintln!("the block's edge at 500 px: {before:.2} px as held, {after:.2} px drawn sharper");
        assert!(before > 4.5, "{before}");
        assert!(after < 3.5 && after < before * 0.6, "{after} against {before}");
        // and at 1400 pixels, as crisp still (drawn at the most side)
        let Some(big) = block_edge(1400.0, true) else { return };
        eprintln!("at 1400 px: {big:.2} px");
        assert!(big < 3.5, "{big}");
    }

    /// How steep `alpha` (a canvas [`CANVAS`] square) is from pixel to pixel over the 256 square in
    /// its middle: the mean of each step squared.
    fn steepness(alpha: &[f32]) -> f64 {
        let a = |x: usize, y: usize| f64::from(alpha.get(y * CANVAS + x).copied().unwrap_or(0.0));
        let (from, to) = (CANVAS / 2 - 128, CANVAS / 2 + 128);
        let mut sum = 0.0;
        for y in from..to {
            for x in from..to {
                sum += (a(x + 1, y) - a(x, y)).powi(2) + (a(x, y + 1) - a(x, y)).powi(2);
            }
        }
        sum / ((to - from) * (to - from)) as f64
    }

    /// The issue's other case: the Grunge grain on a big tile (1000 pixels, fixed to the canvas)
    /// under a round stamp 800 pixels across. As held it was magnified four times over, its flecks
    /// soft blobs; drawn bigger it has its finer noise and crisp flecks, and it is as dark.
    #[test]
    fn grunge_on_a_big_tile_is_sharper() {
        let grunge = || {
            let mut b = Brush { name: "Grunge".to_owned(), ..Brush::round_pen() };
            b.hardness = 1.0;
            b.grain.image = Some(grain::GRUNGE);
            b.grain.depth = 1.0;
            b.grain.scale = 1000.0;
            b
        };
        let (Some(before), Some(after)) = (one_stamp(grunge(), 800.0, false), one_stamp(grunge(), 800.0, true)) else {
            return;
        };
        let mean = |a: &[f32]| a.iter().map(|&v| f64::from(v)).sum::<f64>() / a.len() as f64;
        let (steep_before, steep_after) = (steepness(&before), steepness(&after));
        let times = steep_after / steep_before;
        let (ink_before, ink_after) = (mean(&before), mean(&after));
        eprintln!("grunge on a 1000 px tile: steepness {steep_before:.5} as held, {steep_after:.5} drawn sharper");
        eprintln!("({times:.1} times as steep); ink {ink_before:.4} and {ink_after:.4}");
        assert!(times > 2.0, "{steep_after} against {steep_before}");
        assert!((ink_after / ink_before - 1.0).abs() < 0.05, "{ink_after} against {ink_before}");
    }

    /// A stock brush's stand-in for a stock tip (ours, its tones, outline and marks the tip's) is
    /// drawn bigger as the same picture: averaged back down, within a few levels of the one it holds,
    /// and no bigger than the tip itself is.
    #[test]
    fn a_stand_in_drawn_bigger_is_the_same_picture() {
        let sets = crate::stock_library::shipped();
        let mut tried = 0;
        for brush in sets.iter().flat_map(|s| &s.brushes) {
            let Some(source) = brush.shape.source.as_ref() else { continue };
            let most = source.pixels.most_side();
            if most < 1024 || brush.shape.image != Some(shape::STOCK_GRAINY_SQUARE) || tried >= 3 {
                continue;
            }
            let key = source.stand_in_for.unwrap();
            assert!(most <= textures::stock_shape_side(key).unwrap_or(SHARPEST), "{}", brush.name);
            let big = source.pixels.sharper(1024).unwrap();
            let small = source.pixels.shared();
            let mut apart = 0.0f64;
            for y in 0..TEXTURE_SIZE {
                for x in 0..TEXTURE_SIZE {
                    let sum: u32 = (0..16).map(|k| u32::from(big[(y * 4 + k / 4) * 1024 + x * 4 + k % 4])).sum();
                    apart += (f64::from(sum) / 16.0 - f64::from(small[y * TEXTURE_SIZE + x])).abs();
                }
            }
            let apart = apart / (TEXTURE_SIZE * TEXTURE_SIZE) as f64;
            eprintln!("{}: drawn at 1024, averaged down {apart:.2} levels from its own", brush.name);
            assert!(apart < 12.0, "{}: {apart}", brush.name);
            tried += 1;
        }
        assert_eq!(tried, 3);
    }
}

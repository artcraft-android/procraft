//! Ribbon brushes on a real GPU (skipped where the machine has none): the shape image stretched
//! along the stroke as one strip.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use make2d_engine::brush::{Brush, Shape, Source, Taper};
use make2d_engine::document::Document;
use make2d_engine::geom::Vec2;
use make2d_engine::input::{Pointer, Sample};
use make2d_engine::stroke::{Stamp, StrokeBuilder};
use make2d_engine::textures::TEXTURE_SIZE;
use make2d_gpu::{Canvas, Gpu, StrokeStyle, StrokeTool};

fn gpu() -> Option<Gpu> {
    match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipped: {e}");
            None
        }
    }
}

/// Fails the test if the GPU reported validation errors by its end.
struct NoErrors<'a>(&'a Gpu);

impl Drop for NoErrors<'_> {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            assert_eq!(self.0.errors(), 0, "GPU validation errors");
        }
    }
}

fn alpha(px: &[u8], width: u32, x: u32, y: u32) -> u8 {
    px[((y * width + x) * 4 + 3) as usize]
}

const WIDTH: u32 = 260;
const HEIGHT: u32 = 100;

/// A shape whose left half is paint, all the way down, and whose right half is bare: as a ribbon,
/// dashes along the strip.
fn half_shape() -> Source {
    let side = TEXTURE_SIZE;
    Source::new((0..side * side).map(|i| if i % side < side / 2 { 255 } else { 0 }).collect()).unwrap()
}

/// A plain brush with `half_shape`, drawn as a ribbon if `ribbon`.
fn brush(ribbon: bool) -> Brush {
    let mut b = Brush {
        streamline: 0.0,
        pressure_smoothing: 0.0,
        min_size: 0.0,
        pen_taper: Taper::NONE,
        touch_taper: Taper::NONE,
        shape: Shape { source: Some(half_shape()), ..Shape::default() },
        ..Brush::round_pen()
    };
    b.set_setting("ribbon", if ribbon { 1.0 } else { 0.0 });
    b
}

/// A line along y = 50 from x = 20 to 240 at full pressure.
fn line() -> Vec<Sample> {
    (0..=22).map(|i| Sample::at(Vec2::new(20.0 + 10.0 * i as f32, 50.0), 1.0, i as f64 * 4.0)).collect()
}

/// The layer after drawing `stamps` in lots of `lot` (all at once for 0) with `brush`'s style.
fn drawn(gpu: &Gpu, brush: &Brush, stamps: &[Stamp], lot: usize) -> Vec<u8> {
    let mut doc = Document::new(WIDTH, HEIGHT);
    let mut canvas = Canvas::new(gpu, &doc).unwrap();
    let mut style = StrokeStyle::for_brush(brush, StrokeTool::Paint, [0.0; 3], 1.0, false);
    canvas.use_sources(gpu, &mut style, brush);
    canvas.begin_stroke(doc.current(), style);
    if lot == 0 {
        canvas.add_stamps(stamps);
    } else {
        for lot in stamps.chunks(lot) {
            canvas.add_stamps(lot);
            canvas.render(gpu, &mut doc, None);
        }
    }
    canvas.end_stroke();
    canvas.render(gpu, &mut doc, None);
    canvas.read_layer(gpu, doc.current()).unwrap()
}

fn stroke(brush: &Brush, diameter: f32, samples: &[Sample]) -> Vec<Stamp> {
    let mut s = StrokeBuilder::new(brush.clone(), diameter, Pointer::Stylus);
    s.add(samples);
    s.finish()
}

#[test]
fn a_ribbon_tiles_its_shape_along_the_stroke() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let b = brush(true);
    assert!(b.is_ribbon());
    let layer = drawn(&gpu, &b, &stroke(&b, 20.0, &line()), 0);
    // a tile as long as the strip is wide (20 pixels), its first half paint: solid from side to
    // edge of the strip, none between
    for tile in 0..10 {
        let x0 = 20 + 20 * tile;
        for y in 43..58 {
            assert_eq!(alpha(&layer, WIDTH, x0 + 5, y), 255, "paint at {} {y}", x0 + 5);
            assert_eq!(alpha(&layer, WIDTH, x0 + 15, y), 0, "a gap at {} {y}", x0 + 15);
        }
    }
    // nothing past the strip's sides, nor before its start
    for x in 20..240 {
        assert_eq!(alpha(&layer, WIDTH, x, 38), 0, "above at {x}");
        assert_eq!(alpha(&layer, WIDTH, x, 62), 0, "below at {x}");
    }
    assert_eq!(alpha(&layer, WIDTH, 15, 50), 0);
    // stamped, the same brush fills the line in: its stamps overlap
    let stamped = brush(false);
    let layer = drawn(&gpu, &stamped, &stroke(&stamped, 20.0, &line()), 0);
    assert!((40..220).all(|x| alpha(&layer, WIDTH, x, 50) > 200));
}

#[test]
fn a_ribbon_drawn_a_little_at_a_time_is_the_ribbon_drawn_at_once() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let b = brush(true);
    let wavy: Vec<Sample> = (0..=22)
        .map(|i| {
            Sample::at(Vec2::new(20.0 + 10.0 * i as f32, 50.0 + 20.0 * (i as f32 * 0.4).sin()), 1.0, i as f64 * 4.0)
        })
        .collect();
    let stamps = stroke(&b, 16.0, &wavy);
    let at_once = drawn(&gpu, &b, &stamps, 0);
    assert!(at_once.iter().skip(3).step_by(4).filter(|&&a| a == 255).count() > 1000);
    for lot in [1, 7] {
        assert!(drawn(&gpu, &b, &stamps, lot) == at_once, "in lots of {lot}");
    }
}

#[test]
fn a_ribbon_is_as_wide_as_the_pressure_makes_it() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut b = brush(true);
    b.pressure_size = 1.0;
    // pressure rising from a fifth to full along the line
    let pressed: Vec<Sample> =
        line().into_iter().enumerate().map(|(i, s)| Sample { pressure: 0.2 + 0.8 * i as f32 / 22.0, ..s }).collect();
    let layer = drawn(&gpu, &b, &stroke(&b, 40.0, &pressed), 0);
    // the strip's height across a dash near each end
    let across = |x: u32| (0..HEIGHT).filter(|&y| alpha(&layer, WIDTH, x, y) > 127).count();
    let (start, end) = ((25..45).map(across).max().unwrap(), (195..235).map(across).max().unwrap());
    // (the pressure followed a little behind the pen)
    assert!(start >= 6 && end >= 24 && end * 2 > 5 * start, "{start} {end}");
}

#[test]
fn a_ribbon_goes_on_from_where_it_was_after_its_tip_and_starts_again_when_redrawn() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let b = brush(true);
    let stamps = stroke(&b, 20.0, &line());
    let at_once = drawn(&gpu, &b, &stamps, 0);
    let (first, rest) = stamps.split_at(40);
    for redrawn in [false, true] {
        let mut doc = Document::new(WIDTH, HEIGHT);
        let mut canvas = Canvas::new(&gpu, &doc).unwrap();
        let mut style = StrokeStyle::for_brush(&b, StrokeTool::Paint, [0.0; 3], 1.0, false);
        canvas.use_sources(&gpu, &mut style, &b);
        canvas.begin_stroke(doc.current(), style);
        canvas.add_stamps(first);
        // a tip shown ahead, then taken off by the stamps that come
        canvas.set_tip(&rest[..10]);
        canvas.render(&gpu, &mut doc, None);
        canvas.add_stamps(rest);
        canvas.render(&gpu, &mut doc, None);
        // the whole stroke again from scratch, as a stroke's end redraws it
        if redrawn {
            canvas.replace_stamps(&stamps);
        }
        canvas.end_stroke();
        canvas.render(&gpu, &mut doc, None);
        assert!(canvas.read_layer(&gpu, doc.current()).unwrap() == at_once, "redrawn: {redrawn}");
    }
}

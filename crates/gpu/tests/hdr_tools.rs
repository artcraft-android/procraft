//! The tools that once stopped at an HDR canvas, on a real GPU (skipped where the machine has
//! none): the smudge, plugin filters and layers parked to save the GPU's memory, each keeping
//! light above white.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use make2d_engine::document::{Document, DynamicRange, LayerId};
use make2d_engine::geom::Vec2;
use make2d_engine::hdr;
use make2d_engine::stroke::Stamp;
use make2d_gpu::{Canvas, FilterRun, Gpu, ReadyToPark, StrokeStyle, StrokeTool, read_filter};

const W: u32 = 32;

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

fn hdr_document() -> Document {
    let mut doc = Document::new(W, W);
    doc.range = DynamicRange::High;
    doc
}

/// A hard round stamp of `colour` (sRGB-encoded, past 1 for light above white) at `at`.
fn paint(canvas: &mut Canvas, layer: LayerId, colour: [f32; 3], at: Vec2, radius: f32) {
    let mut style = StrokeStyle::new(StrokeTool::Paint, colour, 1.0);
    style.hardness = 1.0;
    canvas.begin_stroke(layer, style);
    canvas.add_stamps(&[Stamp::round(at, radius, 1.0)]);
    canvas.end_stroke();
}

/// Pixel (x, y) of half-float pixels, as floats.
fn wide_at(half: &[u8], x: u32, y: u32) -> [f32; 4] {
    let i = ((y * W + x) as usize) * hdr::HDR_PIXEL;
    let v: Vec<f32> = hdr::floats(&half[i..i + hdr::HDR_PIXEL]).collect();
    [v[0], v[1], v[2], v[3]]
}

/// Grey everywhere, with light three times white's on the left of x = 16.
fn bright_on_the_left(gpu: &Gpu) -> (Canvas, Document) {
    let mut doc = hdr_document();
    let mut canvas = Canvas::new(gpu, &doc).unwrap();
    let layer = doc.current();
    paint(&mut canvas, layer, [0.2, 0.2, 0.2], Vec2::new(16.0, 16.0), 100.0);
    paint(&mut canvas, layer, [3.0, 2.0, 1.5], Vec2::new(-84.0, 16.0), 100.0);
    canvas.render(gpu, &mut doc, None);
    (canvas, doc)
}

#[test]
fn a_smudge_drags_light_above_white_on_an_hdr_canvas() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (mut canvas, mut doc) = bright_on_the_left(&gpu);
    let layer = doc.current();
    let before = canvas.read_layer_wide(&gpu, layer).unwrap();
    assert!(wide_at(&before, 22, 16)[0] < 0.3);
    // from the light into the grey
    let stamps: Vec<Stamp> = (0..40).map(|i| Stamp::round(Vec2::new(10.0 + i as f32 * 0.4, 16.0), 4.0, 1.0)).collect();
    canvas.begin_stroke(layer, StrokeStyle::new(StrokeTool::Smudge, [0.0; 3], 1.0));
    canvas.add_stamps(&stamps);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    assert!(!canvas.take_smudge_refused());
    let smudged = canvas.read_layer_wide(&gpu, layer).unwrap();
    // carried past white, where 8 bits would have cut it at 1
    let [r, g, _, a] = wide_at(&smudged, 20, 16);
    assert!(r > 1.2 && g > r * 0.5 && a == 1.0, "{r} {g} {a}");
    // what the brush never reached stays as it was
    assert_eq!(wide_at(&smudged, 28, 2), wide_at(&before, 28, 2));
    assert_eq!(wide_at(&smudged, 4, 2), wide_at(&before, 4, 2));
    // and an undo brings it all back
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(canvas.read_layer_wide(&gpu, layer).unwrap(), before);
}

#[test]
fn wet_paint_picks_up_light_above_white_on_an_hdr_canvas() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (mut canvas, mut doc) = bright_on_the_left(&gpu);
    let layer = doc.current();
    let before = canvas.read_layer_wide(&gpu, layer).unwrap();
    // white, wet with a strong pull, from the light into the grey: it carries the light past white
    let wet = StrokeStyle {
        wet_pull: Some(0.85),
        dilution: 0.5,
        load: 0.1,
        ..StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0)
    };
    canvas.begin_stroke(layer, wet);
    canvas.add_stamps(
        &(0..40)
            .map(|i| Stamp {
                time_ms: f64::from(i),
                distance: i as f32 * 0.4,
                ..Stamp::round(Vec2::new(8.0 + i as f32 * 0.4, 16.0), 4.0, 0.5)
            })
            .collect::<Vec<_>>(),
    );
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    assert!(!canvas.take_smudge_refused());
    let after = canvas.read_layer_wide(&gpu, layer).unwrap();
    let [r, ..] = wide_at(&after, 20, 16);
    assert!(r > 1.1, "{r}");
    assert_eq!(wide_at(&after, 28, 2), wide_at(&before, 28, 2));
    assert_eq!(wide_at(&after, 4, 2), wide_at(&before, 4, 2));
}

#[test]
fn a_plugin_filter_runs_on_an_hdr_canvas_keeping_the_light_it_leaves_alone() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (mut canvas, mut doc) = bright_on_the_left(&gpu);
    let layer = doc.current();
    let before = canvas.read_layer_wide(&gpu, layer).unwrap();
    // inverts the bottom half, leaves the top as it is
    let text = "fn pixel(colour: vec4<f32>, at: vec2<f32>) -> vec4<f32> {
        if (at.y < 16.0) { return colour; }
        return vec4<f32>(1.0 - colour.rgb, colour.a);
    }";
    let code = || read_filter(text, &[]).unwrap();
    let program = gpu.make_filter_for(code(), DynamicRange::High).unwrap();
    canvas.begin_transform(layer);
    canvas.set_plugin_filter(&program, &[], false);
    canvas.render(&gpu, &mut doc, None);
    assert!(matches!(canvas.take_filter_run(), Some(FilterRun::Drew(_))));
    canvas.end_transform(true);
    canvas.render(&gpu, &mut doc, None);
    let after = canvas.read_layer_wide(&gpu, layer).unwrap();
    // left alone: the light above white as it was
    assert_eq!(wide_at(&after, 4, 4), wide_at(&before, 4, 4));
    assert!(wide_at(&after, 4, 4)[0] > 2.9);
    // filtered: the grey inverted; the light given to the filter cut at white, so it inverts to
    // black
    let [r, ..] = wide_at(&after, 24, 24);
    assert!((r - 0.8).abs() < 0.01, "{r}");
    let [r, g, b, a] = wide_at(&after, 4, 24);
    assert!(r < 0.01 && g < 0.01 && b < 0.01 && a == 1.0, "{r} {g} {b} {a}");
    // undone, as it was
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(canvas.read_layer_wide(&gpu, layer).unwrap(), before);

    // one made for a standard canvas says so, and changes nothing
    let standard = gpu.make_filter(code()).unwrap();
    canvas.begin_transform(layer);
    canvas.set_plugin_filter(&standard, &[], false);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(canvas.take_filter_run(), Some(FilterRun::OtherRange));
    canvas.end_transform(false);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(canvas.read_layer_wide(&gpu, layer).unwrap(), before);
}

#[test]
fn an_hdr_canvas_parks_layers_as_half_floats_and_brings_them_back() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = hdr_document();
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let base = doc.current();
    paint(&mut canvas, base, [0.3, 0.3, 0.3], Vec2::new(16.0, 16.0), 100.0);
    let mut ids = vec![base];
    for i in 0..9u8 {
        let id = doc.add_layer();
        let at = Vec2::new(f32::from(i % 4) * 8.0 + 4.0, f32::from(i / 4) * 8.0 + 4.0);
        paint(&mut canvas, id, [1.0 + f32::from(i) * 0.25, 1.5, 0.5], at, 3.0);
        ids.push(id);
    }
    canvas.render(&gpu, &mut doc, None);
    let layers: Vec<Vec<u8>> = ids.iter().map(|id| canvas.read_layer_wide(&gpu, *id).unwrap()).collect();
    let picture = canvas.read_composite_wide(&gpu).unwrap();
    assert!(hdr::brightest(&picture) > 3.0);

    // room for three layers' half floats only: the rest are parked
    canvas.set_resident_budget(3 * u64::from(W * W) * hdr::HDR_PIXEL as u64);
    assert!(!canvas.has_room());
    canvas.invalidate();
    for _ in 0..20 {
        canvas.render(&gpu, &mut doc, None);
    }
    assert!(canvas.parked_layers() >= 6, "parked: {}", canvas.parked_layers());
    assert_eq!(canvas.read_composite_wide(&gpu).unwrap(), picture);
    for (id, was) in ids.iter().zip(&layers) {
        assert_eq!(&canvas.read_layer_wide(&gpu, *id).unwrap(), was, "{id:?}");
    }
    // a parked layer painted on again: back with its light, the new paint on it
    let parked = *ids.iter().skip(1).find(|id| canvas.is_parked(**id)).unwrap();
    let k = ids.iter().position(|id| *id == parked).unwrap();
    paint(&mut canvas, parked, [2.0, 0.0, 0.0], Vec2::new(30.0, 30.0), 1.5);
    canvas.render(&gpu, &mut doc, None);
    let painted = canvas.read_layer_wide(&gpu, parked).unwrap();
    assert!(wide_at(&painted, 30, 30)[0] > 1.9);
    assert_eq!(wide_at(&painted, 4, 4), wide_at(&layers[k], 4, 4));
    let lit = (0..W * W).map(|i| wide_at(&layers[k], i % W, i / W)[1]).fold(0.0f32, f32::max);
    let kept = (0..W * W).filter(|i| (i % W) < 28).map(|i| wide_at(&painted, i % W, i / W)[1]).fold(0.0f32, f32::max);
    assert!(lit > 1.0 && (kept - lit).abs() < 1e-3, "{lit} {kept}");

    // an artwork's layer parked as it opens, as its file keeps it
    let mut opened = Canvas::new(&gpu, &doc).unwrap();
    for (id, px) in ids.iter().zip(&layers) {
        opened.park_ready(*id, ReadyToPark::of_wide(px, W, W));
    }
    assert_eq!(opened.parked_layers(), ids.len());
    opened.render(&gpu, &mut doc, None);
    assert_eq!(opened.read_layer_wide(&gpu, base).unwrap(), layers[0]);
    let top = *ids.last().unwrap();
    assert_eq!(opened.read_layer_wide(&gpu, top).unwrap(), layers[ids.len() - 1]);
}

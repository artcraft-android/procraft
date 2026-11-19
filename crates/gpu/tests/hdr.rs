//! HDR documents on a real GPU (skipped where the machine has none): a standard document's canvas
//! drawing as it always has, and an HDR one keeping light above white through painting, blending,
//! adjustments, the light tool and what is read back for export.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::document::{BlendMode, Document, DynamicRange, LayerAdjustment, LayerId};
use procraft_engine::geom::Vec2;
use procraft_engine::hdr;
use procraft_engine::stroke::Stamp;
use procraft_gpu::{Canvas, Filter, Gpu, LightMode, LightTool, StrokeStyle, StrokeTool};

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

fn document(range: DynamicRange) -> Document {
    let mut doc = Document::new(W, W);
    doc.range = range;
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

/// Every pixel.
fn fill(canvas: &mut Canvas, layer: LayerId, colour: [f32; 3]) {
    paint(canvas, layer, colour, Vec2::new(16.0, 16.0), 100.0);
}

/// Pixel (x, y) of half-float pixels, as floats.
fn wide_at(half: &[u8], x: u32, y: u32) -> [f32; 4] {
    let i = ((y * W + x) as usize) * hdr::HDR_PIXEL;
    let v: Vec<f32> = hdr::floats(&half[i..i + hdr::HDR_PIXEL]).collect();
    [v[0], v[1], v[2], v[3]]
}

fn at(px: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// Layers in blend modes that can go past white (on an HDR canvas, which keeps it).
const BRIGHTENING: [(BlendMode, [f32; 3]); 5] = [
    (BlendMode::Screen, [0.2, 0.6, 0.9]),
    (BlendMode::Add, [0.5, 0.4, 0.1]),
    (BlendMode::ColorDodge, [0.3, 0.2, 0.7]),
    (BlendMode::LinearLight, [0.8, 0.5, 0.2]),
    (BlendMode::Multiply, [0.9, 0.8, 0.4]),
];

/// Layers in blend modes that never go past white.
const WITHIN: [(BlendMode, [f32; 3]); 5] = [
    (BlendMode::Screen, [0.2, 0.6, 0.9]),
    (BlendMode::Overlay, [0.5, 0.4, 0.1]),
    (BlendMode::Darken, [0.3, 0.2, 0.7]),
    (BlendMode::Color, [0.8, 0.5, 0.2]),
    (BlendMode::Multiply, [0.9, 0.8, 0.4]),
];

/// A picture that uses most of what blends and adjusts: a base, a layer in each of `modes` over
/// it, some see-through, and an adjustment layer over them all. Colours below white, as a
/// standard canvas takes them.
fn scene(gpu: &Gpu, range: DynamicRange, modes: [(BlendMode, [f32; 3]); 5]) -> (Canvas, Document) {
    let mut doc = document(range);
    let mut canvas = Canvas::new(gpu, &doc).unwrap();
    let base = doc.current();
    fill(&mut canvas, base, [0.55, 0.3, 0.2]);
    for (k, (mode, colour)) in modes.into_iter().enumerate() {
        let id = doc.add_layer();
        doc.set_blend(id, mode);
        doc.set_opacity(id, 0.5 + 0.1 * k as f32);
        paint(&mut canvas, id, colour, Vec2::new(4.0 + 6.0 * k as f32, 16.0), 7.0);
    }
    let adjust = LayerAdjustment {
        filter: Filter::Hsb { hue: 0.3, saturation: 1.2, brightness: 0.05 },
        table: Vec::new(),
        settings: serde_json::Value::Null,
    };
    doc.add_adjustment_layer("Hue".to_owned(), adjust);
    canvas.render(gpu, &mut doc, None);
    (canvas, doc)
}

/// FNV-1a: a fingerprint of a picture's bytes.
fn fingerprint(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf2_9ce4_8422_2325, |h, &b| (h ^ u64::from(b)).wrapping_mul(0x100_0000_01b3))
}

/// The scene's composite on a standard canvas as it was drawn before HDR documents came (on
/// Apple's GPUs, where this was taken: other GPUs round some blends otherwise).
const STANDARD_SCENE: u64 = 0x10f5_b85b_b68e_51d1;

#[test]
fn a_standard_document_draws_as_it_always_has() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (canvas, doc) = scene(&gpu, DynamicRange::Standard, BRIGHTENING);
    assert!(!canvas.is_hdr());
    let px = canvas.read_composite(&gpu).unwrap();
    assert_eq!(px.len(), (W * W * 4) as usize);
    // read wide, a standard canvas's pixels are its 8-bit ones
    assert_eq!(canvas.read_composite_wide(&gpu).unwrap(), px);
    assert_eq!(canvas.read_layer_wide(&gpu, doc.current()).unwrap(), canvas.read_layer(&gpu, doc.current()).unwrap());
    let apple = gpu.adapter.get_info().backend == procraft_gpu::wgpu::Backend::Metal;
    if apple {
        assert_eq!(fingerprint(&px), STANDARD_SCENE, "a standard canvas's picture changed");
    }
}

#[test]
fn an_hdr_document_with_nothing_past_white_draws_as_a_standard_one_does() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (standard, _) = scene(&gpu, DynamicRange::Standard, WITHIN);
    let (high, _) = scene(&gpu, DynamicRange::High, WITHIN);
    assert!(high.is_hdr());
    let (a, b) = (standard.read_composite(&gpu).unwrap(), high.read_composite(&gpu).unwrap());
    // the same but for rounding: an HDR canvas keeps its in-between results finer
    let worst = a.iter().zip(&b).map(|(a, b)| a.abs_diff(*b)).max().unwrap();
    assert!(worst <= 3, "{worst}");
}

#[test]
fn an_hdr_document_keeps_light_above_white_through_painting_and_blending() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = document(DynamicRange::High);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let base = doc.current();
    // a colour three times white's red, a half again its green
    fill(&mut canvas, base, [3.0, 1.5, 0.5]);
    canvas.render(&gpu, &mut doc, None);
    let layer = canvas.read_layer_wide(&gpu, base).unwrap();
    assert_eq!(layer.len(), (W * W) as usize * hdr::HDR_PIXEL);
    let [r, g, b, a] = wide_at(&layer, 16, 16);
    assert!((r - 3.0).abs() < 0.02 && (g - 1.5).abs() < 0.02 && (b - 0.5).abs() < 0.01 && a == 1.0, "{r} {g} {b} {a}");
    // read as 8 bits, cut off at white
    assert_eq!(at(&canvas.read_layer(&gpu, base).unwrap(), 16, 16), [255, 255, 128, 255]);

    // half a layer of grey over it, Normal: half the light comes through
    let over = doc.add_layer();
    doc.set_opacity(over, 0.5);
    fill(&mut canvas, over, [0.2, 0.2, 0.2]);
    // a layer added on in its own place: its light adds to what is there, past white
    let added = doc.add_layer();
    doc.set_blend(added, BlendMode::Add);
    paint(&mut canvas, added, [0.75, 0.75, 0.75], Vec2::new(8.0, 8.0), 4.0);
    canvas.render(&gpu, &mut doc, None);
    let picture = canvas.read_composite_wide(&gpu).unwrap();
    let [r, g, _, _] = wide_at(&picture, 24, 24);
    assert!((r - 1.6).abs() < 0.02 && (g - 0.85).abs() < 0.02, "{r} {g}");
    let [r, g, b, _] = wide_at(&picture, 8, 8);
    assert!((r - 2.35).abs() < 0.03 && (g - 1.6).abs() < 0.03 && (b - 1.1).abs() < 0.03, "{r} {g} {b}");

    // an adjustment over it all keeps what is past white
    let adjust = LayerAdjustment {
        filter: Filter::Hsb { hue: 0.0, saturation: 1.0, brightness: 0.0 },
        table: Vec::new(),
        settings: serde_json::Value::Null,
    };
    doc.add_adjustment_layer("Unchanged".to_owned(), adjust);
    canvas.render(&gpu, &mut doc, None);
    let [r, _, _, _] = wide_at(&canvas.read_composite_wide(&gpu).unwrap(), 24, 24);
    assert!(r > 1.5, "{r}");
}

#[test]
fn the_light_tool_brightens_past_white_up_to_its_limit_in_an_hdr_document() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = document(DynamicRange::High);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    fill(&mut canvas, layer, [0.8, 0.5, 0.3]);
    canvas.render(&gpu, &mut doc, None);
    let lit = |canvas: &mut Canvas, doc: &mut Document, ceiling: f32| {
        let light = LightTool { ceiling, ..LightTool::new(LightMode::Brighten, 1.0) };
        let style = StrokeStyle { light, ..StrokeStyle::new(StrokeTool::Light, [0.0; 3], 1.0) };
        for _ in 0..4 {
            canvas.begin_stroke(doc.current(), style);
            canvas.add_stamps(&[Stamp::round(Vec2::new(16.0, 16.0), 60.0, 1.0)]);
            canvas.end_stroke();
        }
        canvas.render(&gpu, doc, None);
        wide_at(&canvas.read_layer_wide(&gpu, doc.current()).unwrap(), 16, 16)
    };
    // a ceiling of two stops: the red reaches four times white's light and no further
    let [r, g, b, _] = lit(&mut canvas, &mut doc, 4.0);
    let red = hdr::linear(r);
    assert!((red - 4.0).abs() < 0.1, "{r} ({red})");
    // its hue kept: green and blue scaled alike in linear light
    let before = [0.8f32, 0.5, 0.3].map(hdr::linear);
    assert!((hdr::linear(g) / red - before[1] / before[0]).abs() < 0.02);
    assert!((hdr::linear(b) / red - before[2] / before[0]).abs() < 0.02);
    // and tamed at full strength, drawn back to white
    let style = StrokeStyle {
        light: LightTool::new(LightMode::Tame, 1.0),
        ..StrokeStyle::new(StrokeTool::Light, [0.0; 3], 1.0)
    };
    canvas.begin_stroke(layer, style);
    canvas.add_stamps(&[Stamp::round(Vec2::new(16.0, 16.0), 60.0, 1.0)]);
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    let [tamed, _, _, _] = wide_at(&canvas.read_layer_wide(&gpu, layer).unwrap(), 16, 16);
    assert!((tamed - 1.0).abs() < 0.01, "{r} -> {tamed}");
}

#[test]
fn an_hdr_document_exports_its_light_above_white_and_tone_maps_to_a_standard_picture() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let mut doc = document(DynamicRange::High);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let layer = doc.current();
    fill(&mut canvas, layer, [0.4, 0.4, 0.4]);
    paint(&mut canvas, layer, [2.5, 2.0, 1.2], Vec2::new(8.0, 8.0), 4.0);
    canvas.render(&gpu, &mut doc, None);
    let picture = canvas.read_composite_wide(&gpu).unwrap();
    assert!(hdr::brightest(&picture) > 8.0);
    // the EXR keeps it, in linear light
    let exr = hdr::exr(&picture, W, W, false);
    let pixels_at = exr.len() - (W * W) as usize * hdr::HDR_PIXEL - 8 * W as usize;
    assert!(pixels_at > 0);
    let values: Vec<f32> = hdr::floats(&exr[pixels_at..]).collect();
    assert!(values.iter().any(|v| *v > 8.0));
    // tone-mapped, the highlight rolls off below white, keeping its hue, and the rest stays
    let toned = hdr::tone_mapped(&picture);
    let [r, g, b, a] = at(&toned, 8, 8);
    assert!(r > 240 && g < r && b < g && a == 255, "{:?}", [r, g, b, a]);
    assert_eq!(at(&toned, 24, 24), at(&hdr::narrow(&picture), 24, 24));
    // a layer saved wide comes back as it was
    let saved = canvas.read_layer_wide(&gpu, layer).unwrap();
    let mut opened = Canvas::new(&gpu, &doc).unwrap();
    opened.upload_layer_wide(&gpu, layer, &saved).unwrap();
    opened.render(&gpu, &mut doc, None);
    assert_eq!(opened.read_layer_wide(&gpu, layer).unwrap(), saved);
    assert_eq!(opened.read_composite_wide(&gpu).unwrap(), picture);
}

#[test]
fn a_standard_canvas_takes_no_wide_pixels() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let doc = document(DynamicRange::Standard);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let wide = vec![0u8; (W * W) as usize * hdr::HDR_PIXEL];
    assert!(canvas.upload_layer_wide(&gpu, doc.current(), &wide).is_err());
}

/// An adjustment layer of curves (straight) with an exposure of `stops`, starting `above`.
fn exposure_scene(gpu: &Gpu, range: DynamicRange, stops: f32, above: f32) -> (Canvas, Document) {
    curves_scene(gpu, range, Filter::CURVES.with_values(&[stops, above, 0.0]))
}

/// An adjustment layer of curves (straight) as `filter` says, over a colour.
fn curves_scene(gpu: &Gpu, range: DynamicRange, filter: Filter) -> (Canvas, Document) {
    let mut doc = document(range);
    let mut canvas = Canvas::new(gpu, &doc).unwrap();
    let base = doc.current();
    fill(&mut canvas, base, [0.8, 0.4, 0.2]);
    let table: Vec<u8> = (0..=255u8).flat_map(|v| [v, v, v, v]).collect();
    doc.add_adjustment_layer("Curves".to_owned(), LayerAdjustment { filter, table, settings: serde_json::Value::Null });
    canvas.render(gpu, &mut doc, None);
    (canvas, doc)
}

#[test]
fn curves_exposure_lifts_light_past_white_on_an_hdr_canvas_keeping_its_hue() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (canvas, _doc) = exposure_scene(&gpu, DynamicRange::High, 2.0, 0.0);
    let [r, g, b, a] = wide_at(&canvas.read_composite_wide(&gpu).unwrap(), 16, 16);
    // 0.8 is 0.604 of white's light; four times that is 2.42, encoded 1.47
    assert!((r - 1.468).abs() < 0.03 && a > 0.99, "{r} {g} {b} {a}");
    // each channel's light four times what it was: the hue holds
    assert!((g - 0.73).abs() < 0.03 && (b - 0.39).abs() < 0.03, "{r} {g} {b}");
}

#[test]
fn curves_exposure_on_a_standard_canvas_stops_at_white_keeping_its_hue() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (canvas, _doc) = exposure_scene(&gpu, DynamicRange::Standard, 2.0, 0.0);
    let [r, g, _, _] = at(&canvas.read_composite(&gpu).unwrap(), 16, 16);
    // scaled back as one value: red at white, green as far below it as it was
    assert!(r >= 254 && (125..=134).contains(&g), "{r} {g}");
    // only what is brighter than 0.9 is lifted: this colour is not
    let (canvas, _doc) = exposure_scene(&gpu, DynamicRange::Standard, 2.0, 0.9);
    let [r, g, b, _] = at(&canvas.read_composite(&gpu).unwrap(), 16, 16);
    assert!(r.abs_diff(204) <= 1 && g.abs_diff(102) <= 1 && b.abs_diff(51) <= 1, "{r} {g} {b}");
}

#[test]
fn curves_limit_eases_light_past_its_peak_back_keeping_its_hue() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // lifted two stops (red 1.47, green 0.73), then limited to white from 0.8 of it
    let (canvas, _doc) = curves_scene(&gpu, DynamicRange::High, Filter::CURVES.with_values(&[2.0, 0.0, 0.0, 0.0, 0.8]));
    let [r, g, b, _] = wide_at(&canvas.read_composite_wide(&gpu).unwrap(), 16, 16);
    assert!(r > 0.98 && r < 1.0, "{r} {g} {b}");
    // the others scaled with it
    assert!((g - 0.73 * r / 1.468).abs() < 0.02 && (b - 0.39 * r / 1.468).abs() < 0.02, "{r} {g} {b}");
    // off (its peak at LIMIT_OFF) it leaves the light as it was
    let off = Filter::CURVES.with_values(&[2.0, 0.0, 0.0, procraft_engine::filter::LIMIT_OFF, 0.8]);
    let (canvas, _doc) = curves_scene(&gpu, DynamicRange::High, off);
    let [r, _, _, _] = wide_at(&canvas.read_composite_wide(&gpu).unwrap(), 16, 16);
    assert!((r - 1.468).abs() < 0.03, "{r}");
}

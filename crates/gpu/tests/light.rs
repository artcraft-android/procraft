//! The light tool on a real GPU (skipped where the machine has none): brightening, dimming and
//! taming the paint under the brush without changing its hue.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::document::Document;
use procraft_engine::geom::Vec2;
use procraft_engine::stroke::Stamp;
use procraft_gpu::{Canvas, Gpu, LightMode, LightTool, StrokeStyle, StrokeTool};

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

const SIDE: u32 = 64;

fn pixel(px: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * SIDE + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

/// sRGB (0..255) to linear light.
fn linear(v: u8) -> f32 {
    let v = f32::from(v) / 255.0;
    if v > 0.04045 { ((v + 0.055) / 1.055).powf(2.4) } else { v / 12.92 }
}

/// A canvas whose layer is `colour` (sRGB, 0..1) over its left half, bare on its right.
fn painted(gpu: &Gpu, colour: [f32; 3]) -> (Canvas, Document) {
    let mut doc = Document::new(SIDE, SIDE);
    let mut canvas = Canvas::new(gpu, &doc).unwrap();
    canvas.begin_stroke(doc.current(), StrokeStyle::new(StrokeTool::Paint, colour, 1.0));
    canvas
        .add_stamps(&[Stamp::round(Vec2::new(16.0, 16.0), 20.0, 1.0), Stamp::round(Vec2::new(16.0, 48.0), 20.0, 1.0)]);
    canvas.end_stroke();
    canvas.render(gpu, &mut doc, None);
    (canvas, doc)
}

/// A light stroke over the whole canvas at `opacity`.
fn light(gpu: &Gpu, canvas: &mut Canvas, doc: &mut Document, light: LightTool, opacity: f32) -> Vec<u8> {
    let style = StrokeStyle { light, ..StrokeStyle::new(StrokeTool::Light, [0.0; 3], opacity) };
    canvas.begin_stroke(doc.current(), style);
    canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 60.0, 1.0)]);
    canvas.end_stroke();
    canvas.render(gpu, doc, None);
    canvas.read_layer(gpu, doc.current()).unwrap()
}

/// The pixel's channels in linear light over its strongest one: its hue and saturation.
fn proportions(p: [u8; 4]) -> [f32; 3] {
    let l = [linear(p[0]), linear(p[1]), linear(p[2])];
    let peak = l[0].max(l[1]).max(l[2]);
    l.map(|c| c / peak)
}

fn close(a: [f32; 3], b: [f32; 3], within: f32) -> bool {
    a.iter().zip(b).all(|(a, b)| (a - b).abs() <= within)
}

#[test]
fn brightening_doubles_the_light_for_each_stop_and_keeps_the_hue() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let colour = [0.4, 0.25, 0.1];
    let (mut canvas, mut doc) = painted(&gpu, colour);
    let before = pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 16, 16);
    // a full stroke at half strength: one stop
    let after = light(&gpu, &mut canvas, &mut doc, LightTool::new(LightMode::Brighten, 0.5), 1.0);
    let lit = pixel(&after, 16, 16);
    for c in 0..3 {
        let ratio = linear(lit[c]) / linear(before[c]);
        assert!((ratio - 2.0).abs() < 0.12, "channel {c}: {before:?} -> {lit:?} ({ratio})");
    }
    assert!(close(proportions(lit), proportions(before), 0.03), "{before:?} {lit:?}");
    assert_eq!(lit[3], 255);
    // where there was no paint there is still none
    assert_eq!(pixel(&after, 60, 32), [0, 0, 0, 0]);
    // and undone, the paint as it was
    canvas.undo();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 16, 16), before);
}

#[test]
fn dimming_halves_the_light_and_a_stroke_half_on_moves_it_half_as_far() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let colour = [0.8, 0.5, 0.3];
    let (mut canvas, mut doc) = painted(&gpu, colour);
    let before = pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 16, 16);
    let dimmed = pixel(&light(&gpu, &mut canvas, &mut doc, LightTool::new(LightMode::Dim, 0.5), 1.0), 16, 16);
    for c in 0..3 {
        let ratio = linear(dimmed[c]) / linear(before[c]);
        assert!((ratio - 0.5).abs() < 0.04, "channel {c}: {before:?} -> {dimmed:?} ({ratio})");
    }
    assert!(close(proportions(dimmed), proportions(before), 0.03));
    // a stroke half on: half a stop
    let (mut canvas, mut doc) = painted(&gpu, colour);
    let half = pixel(&light(&gpu, &mut canvas, &mut doc, LightTool::new(LightMode::Dim, 0.5), 0.5), 16, 16);
    let ratio = linear(half[0]) / linear(before[0]);
    assert!((ratio - 0.5f32.sqrt()).abs() < 0.04, "{ratio}");
}

#[test]
fn brightening_stops_at_white_without_changing_the_hue() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    // a strong orange: its red at white after a little brightening
    let (mut canvas, mut doc) = painted(&gpu, [0.9, 0.45, 0.2]);
    let before = pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 16, 16);
    let lit = pixel(&light(&gpu, &mut canvas, &mut doc, LightTool::new(LightMode::Brighten, 1.0), 1.0), 16, 16);
    assert_eq!(lit[0], 255, "{lit:?}");
    assert!(lit[1] > before[1] && lit[1] < 250, "{before:?} {lit:?}");
    assert!(close(proportions(lit), proportions(before), 0.03), "{before:?} {lit:?}");
    // white and black stay as they are: white has no room, black no light
    for colour in [[1.0; 3], [0.0; 3]] {
        let (mut canvas, mut doc) = painted(&gpu, colour);
        let before = pixel(&canvas.read_layer(&gpu, doc.current()).unwrap(), 16, 16);
        let after = pixel(&light(&gpu, &mut canvas, &mut doc, LightTool::new(LightMode::Brighten, 1.0), 1.0), 16, 16);
        assert_eq!(before, after);
    }
}

#[test]
fn taming_leaves_a_picture_with_nothing_past_white_as_it_is() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (mut canvas, mut doc) = painted(&gpu, [0.9, 0.6, 0.3]);
    let before = canvas.read_layer(&gpu, doc.current()).unwrap();
    let after = light(&gpu, &mut canvas, &mut doc, LightTool::new(LightMode::Tame, 1.0), 1.0);
    assert!(before == after);
}

#[test]
fn the_light_shows_as_it_is_drawn() {
    let Some(gpu) = gpu() else { return };
    let _check = NoErrors(&gpu);
    let (mut canvas, mut doc) = painted(&gpu, [0.4, 0.25, 0.1]);
    canvas.render(&gpu, &mut doc, None);
    let shown = pixel(&canvas.read_composite(&gpu).unwrap(), 16, 16);
    let style = StrokeStyle {
        light: LightTool::new(LightMode::Brighten, 0.5),
        ..StrokeStyle::new(StrokeTool::Light, [0.0; 3], 1.0)
    };
    canvas.begin_stroke(doc.current(), style);
    canvas.add_stamps(&[Stamp::round(Vec2::new(32.0, 32.0), 60.0, 1.0)]);
    canvas.render(&gpu, &mut doc, None);
    // before the stroke ends, the composite shows it lighter; the layer is as it was
    let live = pixel(&canvas.read_composite(&gpu).unwrap(), 16, 16);
    assert!(live[0] > shown[0] + 20, "{shown:?} {live:?}");
    canvas.end_stroke();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(pixel(&canvas.read_composite(&gpu).unwrap(), 16, 16), live);
}

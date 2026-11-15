//! Adjustment layers, the pixelate, dither and vignette adjustments, and layer styles on a real GPU
//! (skipped where the machine has none).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use make2d_engine::document::{BlendMode, Document, LayerAdjustment, LayerId, LayerStyle};
use make2d_engine::geom::Vec2;
use make2d_engine::stroke::Stamp;
use make2d_gpu::{Canvas, Filter, Gpu, StrokeStyle, StrokeTool};

const W: u32 = 32;

fn setup() -> Option<(Gpu, Document, Canvas)> {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let doc = Document::new(W, W);
    let canvas = Canvas::new(&gpu, &doc).unwrap();
    Some((gpu, doc, canvas))
}

fn paint(canvas: &mut Canvas, layer: LayerId, rgb: [u8; 3], at: Vec2, radius: f32) {
    let color = rgb.map(|c| f32::from(c) / 255.0);
    let mut style = StrokeStyle::new(StrokeTool::Paint, color, 1.0);
    style.hardness = 1.0;
    canvas.begin_stroke(layer, style);
    canvas.add_stamps(&[Stamp::round(at, radius, 1.0)]);
    canvas.end_stroke();
}

/// Covers the whole canvas.
fn fill(canvas: &mut Canvas, layer: LayerId, rgb: [u8; 3]) {
    paint(canvas, layer, rgb, Vec2::new(16.0, 16.0), 100.0);
}

fn composite(gpu: &Gpu, doc: &mut Document, canvas: &mut Canvas) -> Vec<u8> {
    canvas.render(gpu, doc, None);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
    canvas.read_composite(gpu).unwrap()
}

fn at(px: &[u8], x: u32, y: u32) -> [u8; 4] {
    let i = ((y * W + x) * 4) as usize;
    [px[i], px[i + 1], px[i + 2], px[i + 3]]
}

fn adjustment(filter: Filter) -> LayerAdjustment {
    LayerAdjustment { filter, table: Vec::new(), settings: serde_json::Value::Null }
}

/// No chroma left: a grey as light as the colour.
const GREY: Filter = Filter::Hsb { hue: 0.0, saturation: 0.0, brightness: 0.0 };

fn is_grey([r, g, b, _]: [u8; 4]) -> bool {
    r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2
}

#[test]
fn an_adjustment_layer_changes_what_is_below_it_and_nothing_of_its_own() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [255, 0, 0]);
    let a = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert!(is_grey(at(&px, 16, 16)) && at(&px, 16, 16)[0].abs_diff(76) <= 2, "{:?}", at(&px, 16, 16));
    // the layer below is as it was
    assert_eq!(at(&canvas.read_layer(&gpu, base).unwrap(), 16, 16), [255, 0, 0, 255]);
    // half its opacity: half way back to red
    doc.set_opacity(a, 0.5);
    canvas.invalidate();
    let [r, g, b, _] = at(&composite(&gpu, &mut doc, &mut canvas), 16, 16);
    assert!(r.abs_diff(166) <= 3 && g.abs_diff(38) <= 3 && g.abs_diff(b) <= 2, "{r} {g} {b}");
    // hidden, it does nothing
    doc.set_opacity(a, 1.0);
    doc.set_visible(a, false);
    canvas.invalidate();
    assert_eq!(at(&composite(&gpu, &mut doc, &mut canvas), 16, 16), [255, 0, 0, 255]);
    // and a layer above it isn't changed
    doc.set_visible(a, true);
    let top = doc.add_layer();
    paint(&mut canvas, top, [0, 0, 255], Vec2::new(4.0, 4.0), 3.0);
    canvas.invalidate();
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(at(&px, 4, 4), [0, 0, 255, 255]);
    assert!(is_grey(at(&px, 20, 20)));
}

#[test]
fn an_adjustment_layer_shows_through_its_mask() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [255, 0, 0]);
    let a = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    let mask = doc.add_mask(a).unwrap();
    // white on the right half only
    paint(&mut canvas, mask, [255, 255, 255], Vec2::new(32.0, 16.0), 12.0);
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(at(&px, 2, 16), [255, 0, 0, 255], "hidden by its mask");
    assert!(is_grey(at(&px, 30, 16)), "{:?}", at(&px, 30, 16));
    // in another mode: the same, blended
    doc.set_blend(a, BlendMode::Multiply);
    canvas.invalidate();
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(at(&px, 2, 16), [255, 0, 0, 255]);
    let [r, g, b, _] = at(&px, 30, 16);
    assert!(r.abs_diff(76) <= 3 && g <= 2 && b <= 2, "red times its grey: {r} {g} {b}");
}

#[test]
fn in_a_group_of_its_own_it_changes_only_what_is_below_it_there() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [0, 0, 255]);
    let inner = doc.add_layer();
    paint(&mut canvas, inner, [255, 0, 0], Vec2::new(8.0, 16.0), 6.0);
    let adjust = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    doc.toggle_pick(inner);
    assert_eq!(doc.current(), adjust);
    let group = doc.group_chosen().unwrap();
    // composited on its own (not passing through)
    doc.set_opacity(group, 0.999);
    canvas.invalidate();
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert!(is_grey(at(&px, 8, 16)), "the red in the group: {:?}", at(&px, 8, 16));
    let [r, g, b, _] = at(&px, 26, 16);
    assert!(b > 250 && r < 3 && g < 3, "the blue below the group: {r} {g} {b}");
    // passing through, it changes all below it, as if ungrouped
    doc.set_opacity(group, 1.0);
    canvas.invalidate();
    assert!(is_grey(at(&composite(&gpu, &mut doc, &mut canvas), 26, 16)));
}

#[test]
fn a_blurring_adjustment_layer_keeps_up_with_what_is_painted_below() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    doc.background_visible = false;
    let base = doc.current();
    paint(&mut canvas, base, [255, 255, 255], Vec2::new(8.0, 8.0), 4.0);
    doc.add_adjustment_layer("Gaussian Blur".to_owned(), adjustment(Filter::Blur { sigma: 3.0 }));
    let px = composite(&gpu, &mut doc, &mut canvas);
    // spread past its edge, softer in its middle
    assert!(at(&px, 8, 14)[3] > 5, "{:?}", at(&px, 8, 14));
    assert!(at(&px, 8, 8)[3] < 250);
    // a dot painted far off is blurred as it is drawn, and the first stays blurred
    paint(&mut canvas, base, [255, 255, 255], Vec2::new(24.0, 24.0), 2.0);
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert!(at(&px, 24, 28)[3] > 5 && at(&px, 24, 24)[3] < 250, "{:?}", at(&px, 24, 24));
    assert!(at(&px, 8, 14)[3] > 5 && at(&px, 8, 8)[3] < 250);
}

#[test]
fn a_curves_adjustment_layer_reads_its_own_table() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [200, 100, 50]);
    // every channel inverted
    let table: Vec<u8> = (0..=255u8).flat_map(|v| [255 - v, 255 - v, 255 - v, v]).collect();
    let a = doc.add_adjustment_layer(
        "Curves".to_owned(),
        LayerAdjustment { filter: Filter::Curves, table, settings: serde_json::Value::Null },
    );
    let [r, g, b, _] = at(&composite(&gpu, &mut doc, &mut canvas), 16, 16);
    assert!(r.abs_diff(55) <= 2 && g.abs_diff(155) <= 2 && b.abs_diff(205) <= 2, "{r} {g} {b}");
    // a new table goes up as it changes
    let same: Vec<u8> = (0..=255u8).flat_map(|v| [v, v, v, v]).collect();
    doc.set_adjustment(a, LayerAdjustment { filter: Filter::Curves, table: same, settings: serde_json::Value::Null });
    canvas.invalidate();
    let [r, g, b, _] = at(&composite(&gpu, &mut doc, &mut canvas), 16, 16);
    assert!(r.abs_diff(200) <= 2 && g.abs_diff(100) <= 2 && b.abs_diff(50) <= 2, "{r} {g} {b}");
}

/// The layer's pixels drawn through `filter` (destructively, as the Adjustments menu does).
fn filtered(gpu: &Gpu, doc: &mut Document, canvas: &mut Canvas, layer: LayerId, filter: Filter) -> Vec<u8> {
    canvas.begin_transform(layer);
    canvas.set_filter(filter);
    canvas.render(gpu, doc, None);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
    let px = canvas.read_layer(gpu, layer).unwrap();
    canvas.end_transform(false);
    canvas.render(gpu, doc, None);
    px
}

#[test]
fn pixelate_makes_each_cell_one_colour() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let layer = doc.current();
    // a hard disc: cells across its edge come out a part of the way
    paint(&mut canvas, layer, [255, 255, 255], Vec2::new(16.0, 16.0), 14.0);
    let px = filtered(&gpu, &mut doc, &mut canvas, layer, Filter::Pixelate { size: 8.0 });
    for cell in [(0, 0), (8, 0), (16, 8), (24, 24), (0, 16)] {
        let first = at(&px, cell.0, cell.1);
        for y in cell.1..cell.1 + 8 {
            for x in cell.0..cell.0 + 8 {
                let p = at(&px, x, y);
                assert!(p.iter().zip(&first).all(|(a, b)| a.abs_diff(*b) <= 1), "cell {cell:?} at {x},{y}: {p:?}");
            }
        }
    }
    assert_eq!(at(&px, 16, 16), [255, 255, 255, 255]);
    let edge = at(&px, 0, 0)[3];
    assert!(edge > 10 && edge < 245, "a cell across the edge: {edge}");
}

#[test]
fn dither_breaks_a_grey_into_its_two_nearest_steps() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let layer = doc.current();
    fill(&mut canvas, layer, [128, 128, 128]);
    for kind in [0, 1] {
        let dither = Filter::Dither { amount: 1.0, levels: 2.0, scale: 1.0, kind, mono: true };
        let px = filtered(&gpu, &mut doc, &mut canvas, layer, dither);
        let mut white = 0;
        for y in 0..W {
            for x in 0..W {
                let [r, g, b, a] = at(&px, x, y);
                assert!((r == 0 || r == 255) && r == g && g == b && a == 255, "kind {kind} at {x},{y}: {r}");
                white += usize::from(r == 255);
            }
        }
        let share = white as f32 / (W * W) as f32;
        assert!((share - 0.5).abs() < 0.1, "kind {kind}: {share} white");
    }
    // none of it: as it was
    let none = Filter::Dither { amount: 0.0, levels: 2.0, scale: 1.0, kind: 0, mono: false };
    assert_eq!(at(&filtered(&gpu, &mut doc, &mut canvas, layer, none), 5, 5), [128, 128, 128, 255]);
}

#[test]
fn a_vignette_darkens_the_corners_and_not_the_middle() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let layer = doc.current();
    fill(&mut canvas, layer, [200, 200, 200]);
    let dark = Filter::Vignette { amount: 1.0, size: 0.3, softness: 0.5, light: false };
    let px = filtered(&gpu, &mut doc, &mut canvas, layer, dark);
    assert_eq!(at(&px, 16, 16), [200, 200, 200, 255]);
    assert!(at(&px, 0, 0)[0] < 60, "{:?}", at(&px, 0, 0));
    assert!(at(&px, 0, 16)[0] < at(&px, 8, 16)[0], "darker further out");
    let light = Filter::Vignette { amount: 1.0, size: 0.3, softness: 0.5, light: true };
    let px = filtered(&gpu, &mut doc, &mut canvas, layer, light);
    assert!(at(&px, 0, 0)[0] > 240 && at(&px, 0, 0)[3] == 255, "{:?}", at(&px, 0, 0));
}

#[test]
fn the_new_adjustments_run_as_adjustment_layers_too() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [200, 200, 200]);
    let a = doc.add_adjustment_layer(
        "Vignette".to_owned(),
        adjustment(Filter::Vignette { amount: 1.0, size: 0.3, softness: 0.5, light: false }),
    );
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert!(at(&px, 0, 0)[0] < 60 && at(&px, 16, 16)[0] == 200);
    doc.set_adjustment(a, adjustment(Filter::Pixelate { size: 16.0 }));
    paint(&mut canvas, base, [0, 0, 0], Vec2::new(4.0, 4.0), 3.0);
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(at(&px, 0, 0), at(&px, 15, 15), "one cell");
    assert!(at(&px, 0, 0)[0] < 200 && at(&px, 0, 0)[0] > 100, "{:?}", at(&px, 0, 0));
    assert_eq!(at(&px, 16, 16), [200, 200, 200, 255]);
}

fn styled(style: impl FnOnce(&mut LayerStyle)) -> LayerStyle {
    let mut s = LayerStyle::default();
    style(&mut s);
    s
}

#[test]
fn an_outline_goes_round_what_the_layer_shows() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let layer = doc.current();
    paint(&mut canvas, layer, [255, 0, 0], Vec2::new(16.0, 16.0), 6.0);
    let style = styled(|s| {
        s.outline.on = true;
        s.outline.color = [0.0, 0.0, 1.0];
        s.outline.size = 3.0;
    });
    assert!(doc.set_style(layer, Some(style)));
    canvas.invalidate();
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(at(&px, 16, 16), [255, 0, 0, 255], "the layer over it");
    assert_eq!(at(&px, 16, 24), [0, 0, 255, 255], "two out from its edge");
    assert_eq!(at(&px, 16, 27), [255, 255, 255, 255], "beyond it, the background");
    // the layer's pixels are as they were
    assert_eq!(at(&canvas.read_layer(&gpu, layer).unwrap(), 16, 24)[3], 0);
    // a dot painted later is outlined as it is drawn
    paint(&mut canvas, layer, [255, 0, 0], Vec2::new(4.0, 4.0), 1.5);
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(at(&px, 4, 7), [0, 0, 255, 255], "{:?}", at(&px, 4, 7));
    // off: none
    assert!(doc.set_style(layer, Some(LayerStyle::default())));
    canvas.invalidate();
    assert_eq!(at(&composite(&gpu, &mut doc, &mut canvas), 16, 24), [255, 255, 255, 255]);
}

#[test]
fn a_drop_shadow_falls_by_its_distance_and_angle() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let layer = doc.current();
    paint(&mut canvas, layer, [255, 255, 255], Vec2::new(12.0, 12.0), 5.0);
    let style = styled(|s| {
        s.shadow.on = true;
        s.shadow.opacity = 1.0;
        s.shadow.size = 1.0;
        s.shadow.softness = 0.0;
        s.shadow.distance = 8.0;
        s.shadow.angle = 0.0;
    });
    doc.background = [0.5, 0.5, 0.5];
    doc.set_style(layer, Some(style));
    canvas.invalidate();
    let px = composite(&gpu, &mut doc, &mut canvas);
    // to the right, beyond the layer's edge: black
    assert_eq!(at(&px, 22, 12), [0, 0, 0, 255], "{:?}", at(&px, 22, 12));
    assert_eq!(at(&px, 12, 12), [255, 255, 255, 255]);
    assert_eq!(at(&px, 4, 12), [128, 128, 128, 255], "none to the left");
    // down
    let mut down = style;
    down.shadow.angle = 90.0;
    doc.set_style(layer, Some(down));
    canvas.invalidate();
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(at(&px, 12, 22), [0, 0, 0, 255]);
    assert_eq!(at(&px, 22, 12), [128, 128, 128, 255]);
}

#[test]
fn glows_fade_out_from_the_edge_and_in_from_it() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    doc.background = [0.0, 0.0, 0.0];
    let layer = doc.current();
    paint(&mut canvas, layer, [0, 0, 255], Vec2::new(16.0, 16.0), 8.0);
    let style = styled(|s| {
        s.outer_glow.on = true;
        s.outer_glow.color = [1.0, 1.0, 0.0];
        s.outer_glow.opacity = 1.0;
        s.outer_glow.size = 6.0;
        s.inner_glow.on = true;
        s.inner_glow.color = [1.0, 0.0, 0.0];
        s.inner_glow.opacity = 1.0;
        s.inner_glow.size = 4.0;
    });
    doc.set_style(layer, Some(style));
    canvas.invalidate();
    let px = composite(&gpu, &mut doc, &mut canvas);
    // outside: yellow, fading out
    let (near, far) = (at(&px, 16, 25)[0], at(&px, 16, 29)[0]);
    assert!(near > far && near > 100, "{near} {far}");
    assert_eq!(at(&px, 16, 31)[0], 0, "beyond its size");
    // inside: red at the edge, blue in the middle
    let edge = at(&px, 16, 23);
    assert!(edge[0] > 150 && edge[2] < 100, "{edge:?}");
    assert_eq!(at(&px, 16, 16), [0, 0, 255, 255]);
}

#[test]
fn styles_go_with_what_is_clipped_to_the_layer() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    paint(&mut canvas, base, [255, 0, 0], Vec2::new(16.0, 16.0), 6.0);
    let clipped = doc.add_layer();
    doc.set_clipped(clipped, true);
    fill(&mut canvas, clipped, [0, 255, 0]);
    let style = styled(|s| {
        s.outline.on = true;
        s.outline.color = [0.0, 0.0, 1.0];
        s.outline.size = 3.0;
    });
    doc.set_style(base, Some(style));
    canvas.invalidate();
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(at(&px, 16, 16), [0, 255, 0, 255], "clipped paint over the base");
    assert_eq!(at(&px, 16, 24), [0, 0, 255, 255], "the base's outline round both");
}

/// The composite as the canvas shows it now, then with everything composited afresh.
fn as_shown_and_afresh(gpu: &Gpu, doc: &mut Document, canvas: &mut Canvas) -> (Vec<u8>, Vec<u8>) {
    let shown = composite(gpu, doc, canvas);
    canvas.invalidate();
    (shown, composite(gpu, doc, canvas))
}

fn close(a: &[u8], b: &[u8], by: u8) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= by)
}

#[test]
fn merging_bakes_an_adjustment_layer_into_the_merged_pixels() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [255, 0, 0]);
    let a = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    let mask = doc.add_mask(a).unwrap();
    paint(&mut canvas, mask, [255, 255, 255], Vec2::new(32.0, 16.0), 12.0);
    let top = doc.add_layer();
    paint(&mut canvas, top, [0, 0, 255], Vec2::new(16.0, 4.0), 3.0);
    let before_px = composite(&gpu, &mut doc, &mut canvas);
    // the three picked: the adjustment goes into the red through its mask, the blue over that
    doc.set_current(base);
    doc.toggle_pick(a);
    doc.toggle_pick(top);
    let (lower, uppers) = doc.merge_chosen_plan().unwrap();
    assert_eq!((lower, uppers.clone()), (base, vec![a, top]));
    let before = doc.clone();
    canvas.merge_layers(uppers.clone(), lower, before, None);
    doc.finish_merge_chosen(lower, &uppers);
    assert!(doc.layer(a).is_none() && doc.layer(mask).is_none());
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert!(close(&px, &before_px, 2), "looks as it did");
    let layer = canvas.read_layer(&gpu, base).unwrap();
    assert_eq!(at(&layer, 2, 16), [255, 0, 0, 255], "kept out by the mask");
    assert!(is_grey(at(&layer, 30, 16)), "{:?}", at(&layer, 30, 16));
    // undone, all of it back
    canvas.undo();
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert!(doc.layer(a).is_some());
    assert!(close(&px, &before_px, 2));
    assert_eq!(at(&canvas.read_layer(&gpu, base).unwrap(), 30, 16), [255, 0, 0, 255]);
}

#[test]
fn merge_down_bakes_an_adjustment_layer_and_styles() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [255, 0, 0]);
    // an adjustment merged down into the layer below
    let a = doc.add_adjustment_layer("Blur".to_owned(), adjustment(Filter::Blur { sigma: 2.0 }));
    paint(&mut canvas, base, [0, 0, 255], Vec2::new(16.0, 16.0), 4.0);
    let before_px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(doc.merge_target(a), Some(base));
    canvas.merge_down(a, base, doc.clone(), None);
    doc.finish_merge_down(a);
    assert!(close(&composite(&gpu, &mut doc, &mut canvas), &before_px, 2));
    // a styled layer merged down: its outline goes into the pixels, and the styles of the one
    // below go too
    let top = doc.add_layer();
    paint(&mut canvas, top, [0, 255, 0], Vec2::new(8.0, 8.0), 3.0);
    let outline = styled(|s| {
        s.outline.on = true;
        s.outline.color = [1.0, 1.0, 0.0];
        s.outline.size = 2.0;
    });
    doc.set_style(top, Some(outline));
    canvas.invalidate();
    let before_px = composite(&gpu, &mut doc, &mut canvas);
    assert_eq!(at(&before_px, 8, 12), [255, 255, 0, 255], "the outline");
    canvas.merge_down(top, base, doc.clone(), Some(make2d_engine::geom::IRect::new(5, 5, 11, 11)));
    doc.finish_merge_down(top);
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert!(close(&px, &before_px, 2), "looks as it did");
    assert_eq!(at(&canvas.read_layer(&gpu, base).unwrap(), 8, 12), [255, 255, 0, 255], "in the pixels");
    assert!(doc.layer(base).unwrap().style.is_none());
}

#[test]
fn a_styled_layer_below_keeps_its_look_when_merged_into() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    doc.background = [0.0, 0.0, 0.0];
    let base = doc.current();
    paint(&mut canvas, base, [255, 255, 255], Vec2::new(12.0, 12.0), 4.0);
    let shadow = styled(|s| {
        s.shadow.on = true;
        s.shadow.opacity = 1.0;
        s.shadow.color = [0.0, 0.0, 1.0];
        s.shadow.size = 1.0;
        s.shadow.distance = 6.0;
    });
    doc.set_style(base, Some(shadow));
    let top = doc.add_layer();
    paint(&mut canvas, top, [255, 0, 0], Vec2::new(26.0, 26.0), 2.0);
    let before_px = composite(&gpu, &mut doc, &mut canvas);
    canvas.merge_down(top, base, doc.clone(), Some(make2d_engine::geom::IRect::new(23, 23, 29, 29)));
    doc.finish_merge_down(top);
    assert!(close(&composite(&gpu, &mut doc, &mut canvas), &before_px, 2));
}

#[test]
fn painting_an_adjustment_layers_mask_shows_as_it_goes() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [255, 0, 0]);
    let a = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    let mask = doc.add_mask(a).unwrap();
    // a black mask: nothing of it shows
    paint(&mut canvas, mask, [0, 0, 0], Vec2::new(16.0, 16.0), 100.0);
    assert_eq!(at(&composite(&gpu, &mut doc, &mut canvas), 24, 16), [255, 0, 0, 255]);
    let mut style = StrokeStyle::new(StrokeTool::Paint, [1.0, 1.0, 1.0], 1.0);
    style.hardness = 1.0;
    canvas.begin_stroke(mask, style);
    canvas.add_stamps(&[Stamp::round(Vec2::new(24.0, 16.0), 5.0, 1.0)]);
    // (the caches round what is worked on made first)
    canvas.render_finished(&gpu, &mut doc);
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert!(is_grey(at(&px, 24, 16)), "before the stroke ends: {:?}", at(&px, 24, 16));
    assert_eq!(at(&px, 4, 16), [255, 0, 0, 255]);
    canvas.end_stroke();
    let px = composite(&gpu, &mut doc, &mut canvas);
    assert!(is_grey(at(&px, 24, 16)));
}

#[test]
fn a_blur_layer_redraws_only_round_a_stroke_and_matches_a_whole_redraw() {
    let Some((gpu, _, _)) = setup() else { return };
    let mut doc = Document::new(256, 256);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    let base = doc.current();
    let mut style = StrokeStyle::new(StrokeTool::Paint, [0.0, 0.0, 1.0], 1.0);
    style.hardness = 1.0;
    canvas.begin_stroke(base, style);
    canvas.add_stamps(&[Stamp::round(Vec2::new(40.0, 40.0), 20.0, 1.0)]);
    canvas.end_stroke();
    doc.add_adjustment_layer("Blur".to_owned(), adjustment(Filter::Blur { sigma: 2.0 }));
    composite(&gpu, &mut doc, &mut canvas);
    // a dot far off, then another overlapping the first's blur
    for (x, y) in [(200.0, 200.0), (60.0, 40.0)] {
        canvas.begin_stroke(base, style);
        canvas.add_stamps(&[Stamp::round(Vec2::new(x, y), 6.0, 1.0)]);
        canvas.end_stroke();
        let (shown, afresh) = as_shown_and_afresh(&gpu, &mut doc, &mut canvas);
        assert!(close(&shown, &afresh, 1), "a stroke at {x},{y} drawn as a whole redraw would");
    }
}

#[test]
fn an_adjustment_clips_keys_ease_its_values_frame_to_frame() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [255, 0, 0]);
    let a = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    assert!(doc.start_timeline());
    let tl = doc.timeline.as_mut().unwrap();
    tl.duration = 3;
    for track in &mut tl.tracks {
        for clip in &mut track.clips {
            clip.duration = 3;
        }
    }
    let clip = tl
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| matches!(&c.content, make2d_engine::timeline::ClipContent::Drawing(d) if d.root == a))
        .map(|c| c.id)
        .unwrap();
    doc.mark_adjustment_clips();
    let keys = &mut doc.timeline.as_mut().unwrap().clip_mut(clip).unwrap().keys.adjustment;
    let linear = make2d_engine::timeline::Easing::Linear;
    keys.set(0, Filter::Hsb { hue: 0.0, saturation: 1.0, brightness: 0.0 }, linear.clone());
    keys.set(2, GREY, linear);
    canvas.invalidate();
    let mut reds = Vec::new();
    for frame in 0..3 {
        canvas.set_timeline_frame(frame);
        canvas.invalidate();
        reds.push(at(&composite(&gpu, &mut doc, &mut canvas), 16, 16));
    }
    assert_eq!(reds[0], [255, 0, 0, 255], "as it was at the first key");
    assert!(is_grey(reds[2]), "grey at the last: {:?}", reds[2]);
    assert!(reds[1][1] > reds[0][1] && reds[1][1] < reds[2][1] && !is_grey(reds[1]), "half way: {:?}", reds[1]);
}

#[test]
fn an_adjustment_layer_drawn_alone_is_what_it_redraws() {
    let Some((gpu, mut doc, mut canvas)) = setup() else { return };
    let base = doc.current();
    fill(&mut canvas, base, [255, 0, 0]);
    let a = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    let mask = doc.add_mask(a).unwrap();
    paint(&mut canvas, mask, [0, 0, 0], Vec2::new(16.0, 16.0), 100.0);
    let top = doc.add_layer();
    fill(&mut canvas, top, [0, 0, 255]);
    let shown = composite(&gpu, &mut doc, &mut canvas);
    let alone = canvas.read_adjustment_alone(&gpu, &mut doc, a).unwrap();
    // at its full strength, unmasked, with nothing above it
    assert!(is_grey(at(&alone, 16, 16)), "{:?}", at(&alone, 16, 16));
    // and the canvas shows as it did
    assert_eq!(composite(&gpu, &mut doc, &mut canvas), shown);
}

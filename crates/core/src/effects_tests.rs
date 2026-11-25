//! Adjustment layers through the session's own calls: merged with the layers below, keyed on a
//! timeline's Keys panel, and written into a PSD.

use procraft_engine::document::{LayerAdjustment, LayerId};
use procraft_engine::filter::Filter;
use procraft_engine::timeline::{ClipContent, ClipId};
use procraft_engine::view::View;
use procraft_gpu::Gpu;

use crate::timeline::{TimelineChannel, TimelineEasing};
use crate::{CanvasSession, PenSample, PointerKind};

fn sample(x: f32, y: f32) -> PenSample {
    PenSample { x, y, pressure: 1.0, tilt: 0.0, orientation: 0.0, time_ms: 0.0, roll: 0.0 }
}

fn session(side: u32) -> Option<std::sync::Arc<CanvasSession>> {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    let s = CanvasSession::new(side, side);
    {
        let mut st = s.shared.lock();
        st.window_size = (side, side);
        st.view = Some(View::fit((side, side), (side, side)));
        st.gpu = Some(gpu);
        st.ensure_canvas();
    }
    Some(s)
}

const GREY: Filter = Filter::Hsb { hue: 0.0, saturation: 0.0, brightness: 0.0 };

/// A layer painted red all over, with an adjustment layer of `filter` above it.
fn red_under(s: &CanvasSession, filter: Filter) -> (LayerId, LayerId) {
    s.set_brush_size(1.0);
    s.set_color(1.0, 0.0, 0.0);
    for y in [4.0, 12.0, 20.0, 28.0, 36.0] {
        s.stroke_begin(PointerKind::Finger, vec![sample(0.0, y), sample(40.0, y)]);
        s.stroke_end(vec![]);
    }
    let mut st = s.shared.lock();
    let base = st.doc.current();
    let a = st.doc.add_adjustment_layer(
        "Adjustment".to_owned(),
        LayerAdjustment { filter, table: Vec::new(), settings: serde_json::Value::Null },
    );
    if let Some(c) = st.canvas.as_mut() {
        c.invalidate();
    }
    (base, a)
}

fn pixel(s: &CanvasSession, layer: Option<LayerId>, x: u32, y: u32) -> [u8; 4] {
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return [0; 4] };
    canvas.render_finished(gpu, &mut st.doc);
    let px = match layer {
        Some(l) => canvas.read_layer(gpu, l),
        None => canvas.read_composite(gpu),
    }
    .unwrap_or_default();
    let i = ((y * st.doc.width() + x) * 4) as usize;
    px.get(i..i + 4).and_then(|p| p.try_into().ok()).unwrap_or([0; 4])
}

#[test]
fn merging_picked_layers_with_an_adjustment_layer_bakes_it_in() {
    let Some(s) = session(40) else { return };
    let (base, a) = red_under(&s, GREY);
    let shown = pixel(&s, None, 20, 20);
    {
        let mut st = s.shared.lock();
        st.doc.set_current(base);
        st.doc.toggle_pick(a);
    }
    assert!(s.merge_picked(), "merged, not refused");
    let st = s.shared.lock();
    assert!(st.doc.layer(a).is_none());
    drop(st);
    let [r, g, b, _] = pixel(&s, Some(base), 20, 20);
    assert!(r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2, "grey in the layer: {r} {g} {b}");
    assert_eq!(pixel(&s, None, 20, 20), shown, "looks as it did");
}

#[test]
fn an_adjustment_clips_values_are_keyed_from_the_keys_panel() {
    let Some(s) = session(40) else { return };
    let (_, a) = red_under(&s, Filter::Hsb { hue: 0.0, saturation: 1.0, brightness: 0.0 });
    assert!(s.start_timeline());
    let clip = {
        let st = s.shared.lock();
        let tl = st.doc.timeline.as_ref().unwrap();
        tl.tracks
            .iter()
            .flat_map(|t| &t.clips)
            .find(|c| matches!(&c.content, ClipContent::Drawing(d) if d.root == a))
            .map(|c| c.id)
    };
    let Some(ClipId(clip)) = clip else { panic!("no clip for the adjustment layer") };
    let values = s.timeline_adjustment_values(clip);
    assert_eq!(values.iter().map(|v| v.name.as_str()).collect::<Vec<_>>(), ["Hue", "Saturation", "Brightness"]);
    assert_eq!(values[1].value, 1.0);
    // saturation to none, keyed at the playhead: grey, and a key there
    assert!(s.set_timeline_adjustment_value(clip, 1, 0.0, TimelineEasing::Linear, true));
    assert_eq!(s.timeline_adjustment_values(clip)[1].value, 0.0);
    let keys = s.timeline_keys(clip);
    assert!(keys.iter().any(|k| k.channel == TimelineChannel::Adjustment && k.frame == 0));
    let [r, g, b, _] = pixel(&s, None, 20, 20);
    assert!(r.abs_diff(g) <= 2 && g.abs_diff(b) <= 2, "grey: {r} {g} {b}");
    // a plain drawing has none
    let other = s.timeline_tracks().iter().flat_map(|t| t.clips.clone()).map(|c| c.id).find(|c| *c != clip);
    assert!(other.is_some_and(|o| s.timeline_adjustment_values(o).is_empty()));
    // taken away, as it was
    assert!(s.remove_timeline_keys(clip));
    assert_eq!(pixel(&s, None, 20, 20), [255, 0, 0, 255]);
}

#[test]
fn a_psd_keeps_hue_as_an_adjustment_layer_and_a_blur_as_pixels() {
    let Some(s) = session(40) else { return };
    let (_, hue) = red_under(&s, GREY);
    let blur = {
        let mut st = s.shared.lock();
        let id = st.doc.add_adjustment_layer(
            "Soft".to_owned(),
            LayerAdjustment {
                filter: Filter::Blur { sigma: 2.0 },
                table: Vec::new(),
                settings: serde_json::Value::Null,
            },
        );
        if let Some(c) = st.canvas.as_mut() {
            c.invalidate();
        }
        id
    };
    let dir = std::env::temp_dir().join(format!("procraft-effects-psd-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    let path = dir.join("out.psd");
    assert!(s.export_psd(path.to_string_lossy().into_owned()).is_ok());
    let bytes = std::fs::read(&path).unwrap_or_default();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(bytes.windows(8).any(|w| w == b"8BIMhue2"), "the hue layer as Photoshop's own");
    // the blur came as a layer of pixels: what it drew (grey, from the hue below it)
    let read = procraft_engine::psd::read(&bytes).unwrap_or_else(|e| panic!("{e:?}"));
    let soft = read.document.layers().iter().find(|l| l.name == "Soft").map(|l| l.id);
    let px = soft.and_then(|id| read.pixels.iter().find(|(l, _)| *l == id)).map(|(_, p)| p.clone());
    let at = px.as_ref().and_then(|p| p.get((20 * 40 + 20) * 4..(20 * 40 + 20) * 4 + 4)).unwrap_or(&[0; 4]);
    assert!(at[3] == 255 && at[0].abs_diff(at[1]) <= 2, "{at:?}");
    assert!(read.document.layers().iter().all(|l| l.name != "Adjustment"), "no pixels: the reader leaves it out");
    let _ = (hue, blur);
}

#[test]
fn curves_exposure_is_set_from_its_panel_and_kept_by_its_layer() {
    let Some(s) = session(40) else { return };
    let (base, placeholder) = red_under(&s, GREY);
    {
        let mut st = s.shared.lock();
        st.doc.delete_layer(placeholder);
        st.doc.set_current(base);
    }
    assert!(s.adjustment_layer_add(crate::Adjustment::Curves));
    // the exposure's own place among the channels, its sliders set there
    s.adjustment_curve_channel(4);
    s.adjustment_exposure(0, -1.0);
    s.adjustment_exposure(2, 9.0);
    let shown = s.adjustment_shown();
    assert_eq!((shown.curve_channel, shown.exposure.clone()), (4, vec![-1.0, 0.0, 1.0]));
    assert!(shown.changed);
    assert!(s.adjustment_end(true));
    let a = s.shared.lock().doc.current();
    let filter = s.shared.lock().doc.layer(a).and_then(|l| l.adjustment.clone()).map(|a| a.filter);
    assert_eq!(filter, Some(Filter::CURVES.with_values(&[-1.0, 0.0, 1.0])));
    // a stop down: half the light (red 1.0 to 0.5, encoded 0.735)
    let [r, g, b, _] = pixel(&s, None, 20, 20);
    assert!(r.abs_diff(188) <= 2 && g == 0 && b == 0, "{r} {g} {b}");
    // opened again, its panel shows the exposure as it was left
    assert_eq!(s.adjustment_layer_edit(a.0), Some(crate::Adjustment::Curves));
    assert_eq!(s.adjustment_shown().exposure, vec![-1.0, 0.0, 1.0]);
    // its limit, the next channel: off until its peak is brought down
    assert_eq!(s.adjustment_shown().limit, vec![procraft_engine::filter::LIMIT_OFF, 0.8]);
    s.adjustment_curve_channel(5);
    s.adjustment_limit(0, 1.5);
    s.adjustment_limit(1, 2.0);
    let shown = s.adjustment_shown();
    assert_eq!((shown.curve_channel, shown.limit), (5, vec![1.5, 0.95]));
    assert!(s.adjustment_end(true));
    let filter = s.shared.lock().doc.layer(a).and_then(|l| l.adjustment.clone()).map(|a| a.filter);
    assert_eq!(filter, Some(Filter::CURVES.with_values(&[-1.0, 0.0, 1.0, 1.5, 0.95])));
}

#[test]
fn the_texture_packer_finds_drawings_and_shares_each_as_a_png() {
    let Some(s) = session(40) else { return };
    // two blobs apart, on a white background the packer leaves out
    s.set_brush_size(0.2);
    s.set_color(1.0, 0.0, 0.0);
    s.stroke_begin(PointerKind::Finger, vec![sample(6.0, 6.0), sample(8.0, 6.0)]);
    s.stroke_end(vec![]);
    s.stroke_begin(PointerKind::Finger, vec![sample(30.0, 30.0), sample(32.0, 30.0)]);
    s.stroke_end(vec![]);
    assert_eq!(s.texture_packer_open().ok(), Some(2));
    let boxes = s.texture_packer_boxes(6, 0);
    assert_eq!(boxes.len(), 2);
    assert!(boxes[0].y < boxes[1].y, "reading order: {boxes:?}");
    // far enough apart to join only past their gap
    assert_eq!(s.texture_packer_boxes(40, 0).len(), 1);
    let padded = s.texture_packer_boxes(6, 2);
    assert_eq!(padded[1].width, boxes[1].width + 4);
    let hit = s.texture_packer_hit(boxes.clone(), boxes[1].x as f32 + 1.0, boxes[1].y as f32 + 1.0);
    assert_eq!(hit, Some(1));
    assert!(!s.texture_packer_thumbnail(boxes[0], 56).is_empty());
    let dir = std::env::temp_dir().join(format!("packer-{}", std::process::id()));
    let sprites = vec![
        crate::texture_packer::PackerSprite { place: boxes[0], index: 0, name: "dot/top".to_owned() },
        crate::texture_packer::PackerSprite { place: boxes[1], index: 1, name: String::new() },
    ];
    let paths = s.texture_packer_export(dir.to_string_lossy().into_owned(), sprites).unwrap_or_default();
    let names: Vec<String> = paths
        .iter()
        .filter_map(|p| std::path::Path::new(p).file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    assert_eq!(names, ["dot-top.png", "Sprite_02.png"]);
    // a PNG the box's size, red where drawn and clear round it
    let png = std::fs::read(&paths[1]).unwrap_or_default();
    let decoder = png::Decoder::new(std::io::Cursor::new(png));
    let mut reader = decoder.read_info().ok();
    let info = reader.as_ref().map(|r| (r.info().width, r.info().height));
    assert_eq!(info, Some((boxes[1].width, boxes[1].height)));
    let mut px = vec![0; reader.as_ref().map_or(0, |r| r.output_buffer_size().unwrap_or(0))];
    reader.as_mut().and_then(|r| r.next_frame(&mut px).ok());
    // (a blob this small is a pixel or two of all but full cover)
    assert!(px.chunks(4).any(|p| p[0] > 200 && p[1] < 40 && p[3] >= 250), "red in it");
    s.texture_packer_close();
    assert!(s.texture_packer_boxes(6, 0).is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

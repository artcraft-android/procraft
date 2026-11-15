//! Adjustment layers merged, in PSDs and keyed on a timeline; filters' values as panels list them.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use crate::document::{BlendMode, Document, LayerAdjustment, LayerKind, LayerStyle};
use crate::filter::{Filter, ParamUnit};
use crate::formats::{self, PsdKind, PsdLayer};
use crate::timeline::{Channel, ClipContent, Easing, Keyed};

fn adjustment(filter: Filter) -> LayerAdjustment {
    LayerAdjustment { filter, table: Vec::new(), settings: serde_json::Value::Null }
}

const GREY: Filter = Filter::Hsb { hue: 0.0, saturation: 0.0, brightness: 0.0 };

#[test]
fn every_kind_lists_a_name_and_range_for_each_smooth_value() {
    let kinds = [
        GREY,
        Filter::Blur { sigma: 4.0 },
        Filter::Sharpen { amount: 0.5 },
        Filter::Noise { amount: 0.5, frequency: 0.1, octaves: 3, turbulence: 0.2, kind: 1, multi: true, added: false },
        Filter::Motion { along: [10.0, -4.0] },
        Filter::Zoom { centre: [5.0, 6.0], amount: 0.1, toward: [0.0, 1.0] },
        Filter::Chromatic {
            centre: [1.0, 2.0],
            amount: 0.01,
            displace: true,
            offset: [3.0, 4.0],
            blur: 0.5,
            falloff: 0.2,
            transparency: 0.1,
        },
        Filter::Halftone { pitch: 8.0, kind: 2 },
        Filter::Bloom { sigma: 9.0, amount: 0.5, threshold: 0.5, transition: 0.3, burn: 0.2 },
        Filter::GradientMap { amount: 0.4 },
        Filter::ColorBalance { shadows: [0.1, 0.2, 0.3], midtones: [0.0; 3], highlights: [-0.1, 0.0, 0.4] },
        Filter::Glitch { kind: 2, amount: 0.5, values: [0.1, 0.2, 0.3], zoom: 0.4, seed: 0.5 },
        Filter::Pixelate { size: 8.0 },
        Filter::Dither { amount: 1.0, levels: 4.0, scale: 2.0, kind: 1, mono: true },
        Filter::Vignette { amount: 0.6, size: 0.3, softness: 0.5, light: true },
    ];
    for f in kinds {
        let params = f.params();
        assert_eq!(params.len(), f.values().len(), "{f:?}");
        assert!(!params.is_empty(), "{f:?}");
        assert!(params.iter().all(|p| p.low < p.high && !p.name.is_empty()), "{f:?}");
        // given its own values back it is itself, choices and all
        assert_eq!(f.with_values(&f.values()), f);
    }
    // curves' own keyed values are its exposure's; its curves are its table
    assert_eq!(Filter::CURVES.params()[0].unit, ParamUnit::Stops);
    assert_eq!(GREY.params()[0].unit, ParamUnit::Degrees);
    // a value set is kept in its range, and the others stay
    let d = Filter::Dither { amount: 1.0, levels: 4.0, scale: 2.0, kind: 1, mono: true };
    assert_eq!(d.with_param(1, 99.0), Filter::Dither { amount: 1.0, levels: 16.0, scale: 2.0, kind: 1, mono: true });
    assert_eq!(d.with_param(7, 0.5), d, "no such value");
}

#[test]
fn keyed_filters_ease_value_by_value_and_keep_their_choices() {
    let a = Filter::Vignette { amount: 0.0, size: 0.2, softness: 0.4, light: true };
    let b = Filter::Vignette { amount: 1.0, size: 0.6, softness: 0.4, light: true };
    let mut c: Channel<Filter> = Channel::default();
    c.set(0, a, Easing::Linear);
    c.set(10, b, Easing::Linear);
    let Some(Filter::Vignette { amount, size, softness, light }) = c.value_at(5.0) else { panic!() };
    assert!((amount - 0.5).abs() < 1e-5 && (size - 0.4).abs() < 1e-5 && (softness - 0.4).abs() < 1e-5 && light);
    // of another kind, they don't mix
    assert_eq!(a.plus(Filter::Blur { sigma: 3.0 }), a);
    // held, it keeps the first key's
    let mut held: Channel<Filter> = Channel::default();
    held.set(0, a, Easing::Hold);
    held.set(10, b, Easing::Linear);
    assert_eq!(held.value_at(9.0), Some(a));
}

#[test]
fn reach_is_how_far_a_change_below_spreads() {
    assert_eq!(GREY.reach(), Some(0));
    assert_eq!(Filter::Blur { sigma: 0.0 }.reach(), Some(0), "a blur of none changes nothing");
    assert!(Filter::Blur { sigma: 10.0 }.reach() > Filter::Blur { sigma: 2.0 }.reach());
    assert_eq!(Filter::Sharpen { amount: 1.0 }.reach(), Some(1));
    assert_eq!(Filter::Pixelate { size: 8.0 }.reach(), Some(9));
    assert_eq!(Filter::Zoom { centre: [0.0; 2], amount: 0.1, toward: [0.0; 2] }.reach(), None);
}

#[test]
fn merging_picks_takes_adjustment_layers_into_the_lowest_paint_layer() {
    let mut doc = Document::new(8, 8);
    let low_adjust = doc.add_adjustment_layer("Below".to_owned(), adjustment(GREY));
    let base = doc.add_layer();
    let adjust = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    let top = doc.add_layer();
    doc.set_current(low_adjust);
    for id in [base, adjust, top] {
        doc.toggle_pick(id);
    }
    // the adjustment at the bottom has nothing among them to redraw: it stays
    assert_eq!(doc.merge_chosen_plan(), Some((base, vec![adjust, top])));
    // merging down an adjustment layer takes it into the paint layer below; none below it, none
    assert_eq!(doc.merge_target(adjust), Some(base));
    assert_eq!(doc.merge_target(base), None, "an adjustment layer takes no pixels");
}

#[test]
fn merged_into_a_layer_loses_its_styles_which_went_into_its_pixels() {
    let mut doc = Document::new(8, 8);
    let base = doc.current();
    let mut style = LayerStyle::default();
    style.outline.on = true;
    doc.set_style(base, Some(style));
    let top = doc.add_layer();
    assert!(doc.finish_merge_down(top));
    assert!(doc.layer(base).unwrap().style.is_none());
}

#[test]
fn an_adjustment_alone_shows_only_what_it_redraws() {
    let mut doc = Document::new(8, 8);
    let outside = doc.current();
    let inner = doc.add_layer();
    let adjust = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    let mask = doc.add_mask(adjust).unwrap();
    doc.set_opacity(adjust, 0.5);
    doc.set_blend(adjust, BlendMode::Multiply);
    doc.set_current(adjust);
    let above = doc.add_layer();
    // passing through: everything below shows, nothing above, its mask left off, at full
    let alone = doc.adjustment_alone(adjust).unwrap();
    let shown = |d: &Document, id| d.layer(id).unwrap().visible;
    assert!(shown(&alone, outside) && shown(&alone, inner) && !shown(&alone, above) && !shown(&alone, mask));
    let a = alone.layer(adjust).unwrap();
    assert_eq!((a.opacity, a.blend), (1.0, BlendMode::Normal));
    assert!(alone.background_visible);
    // in a group of its own: only what is in the group, on nothing
    doc.set_current(adjust);
    doc.toggle_pick(inner);
    let group = doc.group_chosen().unwrap();
    doc.set_opacity(group, 0.5);
    let alone = doc.adjustment_alone(adjust).unwrap();
    assert!(!shown(&alone, outside) && shown(&alone, inner) && shown(&alone, group));
    assert_eq!(alone.layer(group).unwrap().opacity, 1.0);
    assert!(!alone.background_visible);
    assert_eq!(doc.adjustment_alone(inner), None, "not an adjustment layer");
}

#[test]
fn a_psd_keeps_the_adjustments_it_has_kinds_for() {
    let hsb = Filter::Hsb { hue: std::f32::consts::FRAC_PI_2, saturation: 0.5, brightness: 0.25 };
    let (key, data) = formats::psd_adjustment(&hsb, &[]).unwrap();
    assert_eq!((&key, data.len()), (b"hue2", 100));
    let word = |i: usize| i16::from_be_bytes([data[i], data[i + 1]]);
    assert_eq!((word(0), word(10), word(12), word(14)), (2, 90, -50, 25));
    let table: Vec<u8> = (0..=255u8).flat_map(|v| [255 - v, v, v, v / 2]).collect();
    // with an exposure, which Photoshop's curves can't say, it is flattened
    assert!(formats::psd_adjustment(&Filter::Curves { exposure: 1.0, above: 0.0, soft: 0.5 }, &table).is_none());
    let (key, data) = formats::psd_adjustment(&Filter::CURVES, &table).unwrap();
    assert_eq!(&key, b"curv");
    assert_eq!(data.len() % 2, 0);
    // all four curves; the one for all three first, its last point (output, input) at 255
    assert_eq!(u32::from_be_bytes([data[3], data[4], data[5], data[6]]), 0b1111);
    let all_last = 7 + 2 + 16 * 4;
    assert_eq!(&data[all_last..all_last + 4], &[0, 127, 0, 255]);
    let red_first = 7 + 2 + 17 * 4 + 2;
    assert_eq!(&data[red_first..red_first + 4], &[0, 255, 0, 0], "red inverted");
    let balance = Filter::ColorBalance { shadows: [0.5, 0.0, 0.0], midtones: [0.0; 3], highlights: [0.0, 0.0, -1.0] };
    let (key, data) = formats::psd_adjustment(&balance, &[]).unwrap();
    assert_eq!((&key, data.len(), data[1], i16::from_be_bytes([data[16], data[17]])), (b"blnc", 20, 50, -100));
    assert_eq!(formats::psd_adjustment(&Filter::Blur { sigma: 3.0 }, &[]), None, "flattened instead");
}

#[test]
fn a_psd_adjustment_layer_is_written_with_its_block_and_mask() {
    let (w, h) = (4u32, 4u32);
    // not all one colour (a bottom layer that is comes back as the background)
    let mut red: Vec<u8> = [255, 0, 0, 255].repeat(16);
    red[4..8].copy_from_slice(&[0, 0, 255, 255]);
    let mask = vec![255u8; 16];
    let (key, data) = formats::psd_adjustment(&GREY, &[]).unwrap();
    let layers = [
        PsdLayer {
            name: "Hue",
            kind: PsdKind::Adjustment { key, data, mask: Some(&mask) },
            opacity: 0.5,
            visible: true,
            blend: BlendMode::Normal,
            clipped: false,
        },
        PsdLayer {
            name: "Paint",
            kind: PsdKind::Pixels { rgba: &red, mask: None },
            opacity: 1.0,
            visible: true,
            blend: BlendMode::Normal,
            clipped: false,
        },
    ];
    let bytes = formats::psd(w, h, 72.0, &[], &layers, &red, None);
    assert!(bytes.windows(8).any(|x| x == b"8BIMhue2"), "the adjustment's block");
    // our own reader leaves adjustments out and keeps the rest
    let read = crate::psd::read(&bytes).unwrap();
    let paint: Vec<_> = read.document.layers().iter().filter(|l| l.kind == LayerKind::Paint).collect();
    assert_eq!(paint.len(), 1);
    assert_eq!(paint[0].name, "Paint");
}

#[test]
fn an_adjustment_clip_carries_its_keyed_values_into_the_plan() {
    let mut doc = Document::new(8, 8);
    let adjust = doc.add_adjustment_layer("HSB".to_owned(), adjustment(GREY));
    assert!(doc.start_timeline());
    let clip = doc
        .timeline
        .as_ref()
        .unwrap()
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| matches!(&c.content, ClipContent::Drawing(d) if d.root == adjust))
        .unwrap()
        .id;
    assert!(doc.mark_adjustment_clips());
    assert!(!doc.mark_adjustment_clips(), "marked already");
    assert_eq!(doc.clip_adjustment_layer(clip), Some(adjust));
    // drawn, with no keys: the layer's own
    let item = |d: &Document| d.timeline.as_ref().unwrap().plan(0).items.into_iter().find(|i| i.clip == clip).unwrap();
    assert_eq!(item(&doc).adjustment, None);
    assert_eq!(doc.clip_adjustment_at(clip, 0.0), Some(GREY));
    let keyed = Filter::Hsb { hue: 0.0, saturation: 0.5, brightness: 0.0 };
    doc.timeline.as_mut().unwrap().clip_mut(clip).unwrap().keys.adjustment.set(0, keyed, Easing::Linear);
    assert_eq!(item(&doc).adjustment, Some(keyed));
    assert_eq!(doc.clip_adjustment_at(clip, 0.0), Some(keyed));
    // a key of another kind than the layer's is left out
    doc.set_adjustment(adjust, adjustment(Filter::Blur { sigma: 2.0 }));
    assert_eq!(doc.clip_adjustment_at(clip, 0.0), Some(Filter::Blur { sigma: 2.0 }));
}

#[test]
fn a_file_with_the_old_effect_keys_still_opens() {
    let mut doc = Document::new(8, 8);
    assert!(doc.start_timeline());
    let mut json = serde_json::to_value(&doc).unwrap();
    let keys = &mut json["timeline"]["tracks"][0]["clips"][0]["keys"];
    keys.as_object_mut().unwrap().remove("adjustment");
    keys["effects"] = serde_json::json!({ "3": { "keys": [] } });
    let back: Document = serde_json::from_value(json).unwrap();
    assert!(back.timeline.unwrap().tracks[0].clips[0].keys.adjustment.keys.is_empty());
}

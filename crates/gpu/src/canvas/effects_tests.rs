//! How far a change composites again round adjustment layers that read their neighbours.

use make2d_engine::document::{Document, LayerAdjustment};
use make2d_engine::filter::Filter;
use make2d_engine::geom::IRect;

use super::{band_strips, composited_rects};

fn with(filter: Filter) -> Document {
    let mut doc = Document::new(1000, 1000);
    doc.add_adjustment_layer(
        "Adjustment".to_owned(),
        LayerAdjustment { filter, table: Vec::new(), settings: serde_json::Value::Null },
    );
    doc
}

const FULL: IRect = IRect { x0: 0, y0: 0, x1: 1000, y1: 1000 };
const STROKE: IRect = IRect { x0: 400, y0: 400, x1: 420, y1: 420 };

#[test]
fn a_blur_layer_composites_round_a_stroke_by_its_reach_not_the_whole_canvas() {
    let doc = with(Filter::Blur { sigma: 4.0 });
    let r = Filter::Blur { sigma: 4.0 }.reach().unwrap() as i32;
    let (changed, composited) = composited_rects(&doc, STROKE, FULL);
    assert_eq!(changed, IRect::new(400 - r, 400 - r, 420 + r, 420 + r));
    assert_eq!(composited, IRect::new(400 - 2 * r, 400 - 2 * r, 420 + 2 * r, 420 + 2 * r));
    // the band between, in four strips that cover it once
    let strips = band_strips(composited, changed);
    let area: i64 = strips.iter().map(|s| i64::from(s.width()) * i64::from(s.height())).sum();
    let whole = |s: IRect| i64::from(s.width()) * i64::from(s.height());
    assert_eq!(area, whole(composited) - whole(changed));
    assert!(strips.iter().all(|s| s.intersect(changed).is_empty()));
}

#[test]
fn colour_changes_spread_nothing_and_a_zoom_redraws_everything() {
    let (changed, composited) =
        composited_rects(&with(Filter::Hsb { hue: 0.2, saturation: 1.0, brightness: 0.0 }), STROKE, FULL);
    assert_eq!((changed, composited), (STROKE, STROKE));
    let zoom = with(Filter::Zoom { centre: [500.0, 500.0], amount: 0.1, toward: [0.0; 2] });
    assert_eq!(composited_rects(&zoom, STROKE, FULL), (FULL, FULL));
    // on a timeline its keys can change it: all of it
    let mut timed = with(Filter::Blur { sigma: 4.0 });
    assert!(timed.start_timeline());
    assert_eq!(composited_rects(&timed, STROKE, FULL), (FULL, FULL));
}

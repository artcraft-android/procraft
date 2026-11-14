//! A canvas on a timeline draws just as it did with Animation Assist: every frame played, and
//! every frame drawn on with its onion skins (skipped where the machine has no GPU).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use make2d_engine::animation::{self, Animation};
use make2d_engine::document::{Document, LayerId};
use make2d_gpu::{Canvas, Gpu};

const SIDE: u32 = 8;

fn gpu() -> Option<Gpu> {
    match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipped: {e}");
            None
        }
    }
}

/// Layer `i`'s pixels (premultiplied): its own column opaque, and a half-covered column every
/// layer shares, so order, tints and fades all show.
fn pixels(i: usize) -> Vec<u8> {
    let colour = [[220, 40, 40], [40, 200, 60], [50, 80, 230], [230, 200, 30], [160, 60, 200]][i % 5];
    let mut px = vec![0_u8; (SIDE * SIDE * 4) as usize];
    for y in 0..SIDE {
        for x in 0..SIDE {
            let at = ((y * SIDE + x) * 4) as usize;
            if x as usize == i {
                px[at..at + 4].copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
            } else if x == SIDE - 1 {
                px[at..at + 4].copy_from_slice(&[colour[0] / 2, colour[1] / 2, colour[2] / 2, 128]);
            }
        }
    }
    px
}

/// A canvas of five frames, the third a group holding its layer.
fn canvas_doc() -> (Document, Vec<LayerId>) {
    let mut doc = Document::new(SIDE, SIDE);
    let mut layers = vec![doc.current()];
    for _ in 1..5 {
        layers.push(doc.add_layer());
    }
    doc.set_current(layers[2]);
    doc.group_chosen().unwrap();
    (doc, layers)
}

fn render(canvas: &mut Canvas, gpu: &Gpu, doc: &mut Document) -> Vec<u8> {
    canvas.invalidate();
    canvas.render_finished(gpu, doc);
    canvas.read_composite(gpu).unwrap()
}

fn canvas_for(gpu: &Gpu, doc: &Document, layers: &[LayerId]) -> Canvas {
    let mut canvas = Canvas::new(gpu, doc).unwrap();
    for (i, id) in layers.iter().enumerate() {
        canvas.upload_layer(gpu, *id, &pixels(i)).unwrap();
    }
    canvas
}

/// Every frame of `doc` (with Animation Assist) against the same canvas put on a timeline.
fn draws_the_same(gpu: &Gpu, doc: &Document, layers: &[LayerId]) {
    let a = doc.animation.clone().unwrap();
    let mut assist = doc.clone();
    let mut timeline = doc.clone();
    assert!(timeline.start_timeline());
    let mut before = canvas_for(gpu, &assist, layers);
    let mut after = canvas_for(gpu, &timeline, layers);
    let frames = animation::frames(&assist);
    // playing: each place in the sequence
    let mut place = 0_usize;
    let mut seen = std::collections::HashSet::new();
    for (i, frame) in frames.iter().enumerate() {
        for _ in 0..=a.hold(*frame) {
            before.set_play_frame(Some(i));
            after.set_play_frame(Some(place));
            let shown = render(&mut before, gpu, &mut assist);
            assert_eq!(shown, render(&mut after, gpu, &mut timeline), "playing {place}");
            seen.insert(shown);
            place += 1;
        }
    }
    // the frames do differ (unless a background and foreground cover all but one)
    assert!(seen.len() > 1 || frames.len() < 3, "every frame drew the same");
    // drawing on each frame, its onion skins around it
    before.set_play_frame(None);
    after.set_play_frame(None);
    let mut at = 0;
    for (i, frame) in frames.iter().enumerate() {
        let layer = assist.members(Some(*frame)).first().copied().unwrap_or(*frame);
        assist.set_current(layer);
        after.set_timeline_frame(at);
        assert_eq!(render(&mut before, gpu, &mut assist), render(&mut after, gpu, &mut timeline), "drawing on {i}");
        at += 1 + a.hold(*frame);
    }
}

#[test]
fn a_timeline_draws_as_animation_assist_did() {
    let Some(gpu) = gpu() else { return };
    let (mut doc, layers) = canvas_doc();
    let frames = {
        doc.animation = Some(Animation::default());
        animation::frames(&doc)
    };
    let tinted = Animation {
        onion_frames: 2,
        onion_opacity: 0.8,
        before: [1.0, 0.0, 0.0],
        after: [0.0, 0.0, 1.0],
        ..Animation::default()
    };
    let mut held = tinted.clone();
    held.set_hold(frames[1], 2);
    held.set_hold(frames[2], 1);
    let cases = [
        Animation::default(),
        tinted.clone(),
        held.clone(),
        Animation { background: true, ..held.clone() },
        Animation { foreground: true, ..held.clone() },
        Animation { background: true, foreground: true, ..tinted },
    ];
    for a in cases {
        doc.animation = Some(a);
        draws_the_same(&gpu, &doc, &layers);
    }
    // a hidden layer isn't a frame, and stays hidden on its own track
    doc.set_visible(frames[3], false);
    draws_the_same(&gpu, &doc, &layers);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
}

#[test]
fn a_timeline_fades_a_drawing_by_its_clip() {
    let Some(gpu) = gpu() else { return };
    let (mut doc, layers) = canvas_doc();
    doc.animation = Some(Animation::default());
    assert!(doc.start_timeline());
    let mut canvas = canvas_for(&gpu, &doc, &layers);
    canvas.set_play_frame(Some(1));
    let full = render(&mut canvas, &gpu, &mut doc);
    doc.timeline.as_mut().unwrap().tracks[0].clips[1].opacity = 0.5;
    let half = render(&mut canvas, &gpu, &mut doc);
    // frame 1 is layer 1, in column 1 over the white background
    let px = |p: &[u8]| p[((SIDE + 1) * 4) as usize..((SIDE + 1) * 4 + 4) as usize].to_vec();
    assert_eq!(px(&full)[..3], [40, 200, 60]);
    let faded = px(&half);
    assert!(faded[0] > 120 && faded[1] > 200 && faded[2] > 130, "{faded:?}");
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
}

#[test]
fn a_drawing_moves_turns_and_fades_by_its_keys() {
    use make2d_engine::timeline::{Channel, Easing, Key};
    let Some(gpu) = gpu() else { return };
    let mut doc = Document::new(SIDE, SIDE);
    let layer = doc.current();
    doc.animation = Some(Animation::default());
    assert!(doc.start_timeline());
    let mut canvas = canvas_for(&gpu, &doc, &[layer]);
    canvas.set_play_frame(Some(0));
    let px = |p: &[u8], x: u32, y: u32| p[((y * SIDE + x) * 4) as usize..((y * SIDE + x) * 4 + 3) as usize].to_vec();
    let still = render(&mut canvas, &gpu, &mut doc);
    assert_eq!(px(&still, 0, 2), [220, 40, 40], "column 0 is the layer's red");
    // moved 3 to the right: the red column is now column 3, column 0 the white background
    first_clip(&mut doc).keys.position =
        Channel { keys: vec![Key { frame: 0, value: [3.0, 0.0], easing: Easing::Linear, path: None }] };
    let moved = render(&mut canvas, &gpu, &mut doc);
    assert_eq!(px(&moved, 3, 2), [220, 40, 40], "{:?}", px(&moved, 3, 2));
    assert_eq!(px(&moved, 0, 2), [255, 255, 255]);
    // a half turn about the middle: column 0 lands on column 7
    first_clip(&mut doc).keys.position = Channel::default();
    first_clip(&mut doc).keys.pivot = Some([SIDE as f32 / 2.0, SIDE as f32 / 2.0]);
    first_clip(&mut doc).keys.rotation =
        Channel { keys: vec![Key { frame: 0, value: std::f32::consts::PI, easing: Easing::Linear, path: None }] };
    let turned = render(&mut canvas, &gpu, &mut doc);
    assert_eq!(px(&turned, 7, 2), [220, 40, 40], "{:?}", px(&turned, 7, 2));
    // faded to half by an opacity key, unmoved: half way to white
    first_clip(&mut doc).keys = Default::default();
    first_clip(&mut doc).keys.opacity =
        Channel { keys: vec![Key { frame: 0, value: 0.5, easing: Easing::Linear, path: None }] };
    let faded = render(&mut canvas, &gpu, &mut doc);
    let red = px(&faded, 0, 2);
    assert!((red[0] as i32 - 237).abs() <= 2 && (red[1] as i32 - 147).abs() <= 2, "{red:?}");
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
}

fn first_clip(doc: &mut Document) -> &mut make2d_engine::timeline::Clip {
    &mut doc.timeline.as_mut().unwrap().tracks[0].clips[0]
}

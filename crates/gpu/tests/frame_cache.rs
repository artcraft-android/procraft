//! Timeline frames kept and shown again: after each kind of edit a frame from the cache is the
//! same, pixel for pixel, as one composited afresh, and only the frames an edit touches are
//! composited again (skipped where the machine has no GPU).
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use std::time::{Duration, Instant};

use make2d_engine::animation::{self, Animation};
use make2d_engine::document::{Document, LayerId};
use make2d_engine::geom::Vec2;
use make2d_engine::stroke::Stamp;
use make2d_engine::timeline::{CameraPose, Channel, ClipId, Easing, Key, MaskMode};
use make2d_gpu::{Canvas, Gpu, StrokeStyle, StrokeTool};

const SIDE: u32 = 16;

fn gpu() -> Option<Gpu> {
    match Gpu::headless() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipped: {e}");
            None
        }
    }
}

/// Layer `i`'s pixels (premultiplied), `w` × `h`: a band of its own colour, and a half-covered
/// stripe every layer shares, so order, tints and fades all show.
fn pixels(i: usize, w: u32, h: u32) -> Vec<u8> {
    let colour = [[220, 40, 40], [40, 200, 60], [50, 80, 230], [230, 200, 30], [160, 60, 200], [30, 190, 190]][i % 6];
    let band = (w / 8).max(1);
    let mut px = vec![0_u8; (w * h * 4) as usize];
    for (n, p) in px.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        let x = n as u32 % w;
        if (x / band) as usize % 8 == i % 8 {
            p.copy_from_slice(&[colour[0], colour[1], colour[2], 255]);
        } else if x.is_multiple_of(5) {
            p.copy_from_slice(&[colour[0] / 2, colour[1] / 2, colour[2] / 2, 128]);
        }
    }
    px
}

#[derive(Clone, Copy, Debug)]
enum Mode {
    /// Drawn on, the onion skins about it (scrubbing).
    Drawing,
    Playing,
    /// Playing through the camera.
    Camera,
}

const MODES: [Mode; 3] = [Mode::Drawing, Mode::Playing, Mode::Camera];

fn show(canvas: &mut Canvas, mode: Mode, frame: u32) {
    match mode {
        Mode::Drawing => {
            canvas.set_play_frame(None);
            canvas.set_camera_view(false);
            canvas.set_timeline_frame(frame);
        }
        Mode::Playing | Mode::Camera => {
            canvas.set_camera_view(matches!(mode, Mode::Camera));
            canvas.set_play_frame(Some(frame as usize));
        }
    }
}

/// The frame shown, and whether it came from the cache.
fn frame(canvas: &mut Canvas, gpu: &Gpu, doc: &mut Document) -> (Vec<u8>, bool) {
    let hits = canvas.frame_cache_stats().hits;
    canvas.invalidate();
    canvas.render_finished(gpu, doc);
    (canvas.read_composite(gpu).unwrap(), canvas.frame_cache_stats().hits > hits)
}

/// The same document on two canvases: one keeping frames, one compositing every one afresh.
struct Pair {
    cached: Canvas,
    fresh: Canvas,
}

impl Pair {
    fn both(&mut self, f: impl Fn(&mut Canvas)) {
        f(&mut self.cached);
        f(&mut self.fresh);
    }

    /// Every frame in every mode on both, which must match pixel for pixel: which frames each
    /// mode took from the cache.
    fn sweep(&mut self, gpu: &Gpu, doc: &mut Document, what: &str) -> Vec<Vec<bool>> {
        let duration = doc.timeline.as_ref().unwrap().duration;
        let mut hits = Vec::new();
        for mode in MODES {
            let mut mode_hits = Vec::new();
            for f in 0..duration {
                show(&mut self.cached, mode, f);
                show(&mut self.fresh, mode, f);
                let (kept, hit) = frame(&mut self.cached, gpu, doc);
                let (afresh, fresh_hit) = frame(&mut self.fresh, gpu, doc);
                assert!(!fresh_hit, "the canvas with no budget keeps nothing");
                assert!(kept == afresh, "{what}: {mode:?} frame {f} differs from one composited afresh");
                mode_hits.push(hit);
            }
            hits.push(mode_hits);
        }
        assert_eq!(gpu.errors(), 0, "GPU validation errors");
        hits
    }
}

/// The drawings frame `f` draws in `mode` (onion skins and mattes too).
fn draws(doc: &Document, mode: Mode, f: u32) -> Vec<LayerId> {
    let tl = doc.timeline.as_ref().unwrap();
    let playing = !matches!(mode, Mode::Drawing);
    let mut roots = tl.plan(f).roots();
    for t in &tl.tracks {
        let (before, after) = tl.onions_on(t, f, playing);
        roots.extend(before.iter().chain(&after).map(|s| s.root));
    }
    roots
}

/// After an edit touching `touched` (with `before` the document as it was): every frame that
/// draws none of them, before or after, comes from the cache, and a played frame that does is
/// composited again.
fn only_touched(hits: &[Vec<bool>], before: &Document, after: &Document, touched: &[LayerId], what: &str) {
    for (m, mode) in MODES.iter().enumerate() {
        for (f, hit) in hits[m].iter().enumerate() {
            let f32_ = f as u32;
            let drawn =
                draws(before, *mode, f32_).iter().chain(&draws(after, *mode, f32_)).any(|l| touched.contains(l));
            if !drawn {
                assert!(*hit, "{what}: {mode:?} frame {f} doesn't draw what changed, but was composited again");
            } else if !matches!(mode, Mode::Drawing) {
                assert!(!hit, "{what}: {mode:?} frame {f} draws what changed, but came from the cache");
            }
        }
    }
}

fn paint(canvas: &mut Canvas, layer: LayerId) {
    canvas.begin_stroke(layer, StrokeStyle::new(StrokeTool::Paint, [0.1, 0.2, 0.9], 1.0));
    canvas.add_stamps(&[Stamp::round(Vec2::new(5.0, 5.0), 3.0, 1.0)]);
    canvas.end_stroke();
}

/// The clip that draws `root`.
fn clip_of(doc: &Document, root: LayerId) -> ClipId {
    let tl = doc.timeline.as_ref().unwrap();
    tl.tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find(|c| matches!(c.content, make2d_engine::timeline::ClipContent::Drawing(ref d) if d.root == root))
        .unwrap()
        .id
}

/// Six layers: the first a background under the whole timeline, the others drawings in turn
/// (two of them held), one onion skin each side; nine frames.
fn setup(gpu: &Gpu) -> (Document, Vec<LayerId>, Pair) {
    let mut doc = Document::new(SIDE, SIDE);
    let mut layers = vec![doc.current()];
    for _ in 1..6 {
        layers.push(doc.add_layer());
    }
    let mut a = Animation {
        onion_frames: 1,
        onion_opacity: 0.6,
        before: [1.0, 0.0, 0.0],
        after: [0.0, 0.0, 1.0],
        background: true,
        ..Animation::default()
    };
    doc.animation = Some(a.clone());
    let frames = animation::frames(&doc);
    a.set_hold(frames[1], 2);
    a.set_hold(frames[3], 1);
    doc.animation = Some(a);
    assert!(doc.start_timeline());
    // the background keeps its own place in the sequence too
    assert_eq!(doc.timeline.as_ref().unwrap().duration, 9);
    let make = |budget| {
        let mut c = Canvas::new(gpu, &doc).unwrap();
        for (i, id) in layers.iter().enumerate() {
            c.upload_layer(gpu, *id, &pixels(i, SIDE, SIDE)).unwrap();
        }
        c.set_frame_budget(budget);
        c
    };
    let pair = Pair { cached: make(None), fresh: make(Some(0)) };
    (doc, layers, pair)
}

#[test]
fn kept_frames_match_fresh_ones_after_every_kind_of_edit() {
    let Some(gpu) = gpu() else { return };
    let (mut doc, layers, mut pair) = setup(&gpu);
    let none = |hits: &[Vec<bool>]| hits.iter().flatten().all(|h| !h);
    let all = |hits: &[Vec<bool>]| hits.iter().flatten().all(|h| *h);
    assert!(none(&pair.sweep(&gpu, &mut doc, "first")), "nothing kept yet");
    assert!(all(&pair.sweep(&gpu, &mut doc, "again")), "every frame kept");

    // painting a drawing: the frames that show it (or skin it)
    let before = doc.clone();
    pair.both(|c| paint(c, layers[2]));
    let hits = pair.sweep(&gpu, &mut doc, "painted");
    only_touched(&hits, &before, &doc, &[layers[2]], "painted");
    // and undoing it
    pair.both(Canvas::undo);
    let hits = pair.sweep(&gpu, &mut doc, "undone");
    only_touched(&hits, &before, &doc, &[layers[2]], "undone");

    // a clip's opacity
    let before = doc.clone();
    let clip = clip_of(&doc, layers[3]);
    doc.timeline.as_mut().unwrap().clip_mut(clip).unwrap().opacity = 0.5;
    let hits = pair.sweep(&gpu, &mut doc, "clip opacity");
    only_touched(&hits, &before, &doc, &[layers[3]], "clip opacity");

    // a key moving a clip
    let before = doc.clone();
    let clip = clip_of(&doc, layers[4]);
    doc.timeline.as_mut().unwrap().clip_mut(clip).unwrap().keys.position =
        Channel { keys: vec![Key { frame: 0, value: [3.0, 1.0], easing: Easing::Linear, path: None }] };
    let hits = pair.sweep(&gpu, &mut doc, "key");
    only_touched(&hits, &before, &doc, &[layers[4]], "key");

    // a matte: the background masks the last drawing
    let before = doc.clone();
    let clip = clip_of(&doc, layers[5]);
    doc.timeline.as_mut().unwrap().clip_mut(clip).unwrap().mask = MaskMode::Alpha;
    let hits = pair.sweep(&gpu, &mut doc, "matte");
    only_touched(&hits, &before, &doc, &[layers[5]], "matte");

    // the first two drawings grouped
    let before = doc.clone();
    let ids = [clip_of(&doc, layers[1]), clip_of(&doc, layers[2])];
    let group = doc.timeline.as_mut().unwrap().group(&ids, "Group").unwrap();
    let hits = pair.sweep(&gpu, &mut doc, "grouped");
    only_touched(&hits, &before, &doc, &layers[1..3], "grouped");
    doc.timeline.as_mut().unwrap().clip_mut(group).unwrap().opacity = 0.7;
    let hits = pair.sweep(&gpu, &mut doc, "group opacity");
    only_touched(&hits, &before, &doc, &layers[1..3], "group opacity");

    // camera keys: only frames played through the camera
    let side = SIDE as f32;
    let looking = CameraPose { center: [side / 2.0 - 3.0, side / 2.0], zoom: 1.2, rotation: 0.3 };
    let tl = doc.timeline.as_mut().unwrap();
    tl.camera.set(0, CameraPose::still(side, side), Easing::Linear);
    tl.camera.set(8, looking, Easing::Linear);
    let hits = pair.sweep(&gpu, &mut doc, "camera");
    assert!(all(&hits[..2]), "drawing and playing without the camera are as they were");
    assert!(hits[2].iter().all(|h| !h), "every frame through the camera moved");
    // the last key moved: the first frame, on the first key, stays
    doc.timeline.as_mut().unwrap().camera.set(8, CameraPose { zoom: 0.8, ..looking }, Easing::Linear);
    let hits = pair.sweep(&gpu, &mut doc, "camera key");
    assert!(all(&hits[..2]));
    assert!(hits[2][0] && hits[2][1..].iter().all(|h| !h), "{:?}", hits[2]);

    // settings: the onion skins' opacity touches only frames drawn on
    doc.timeline.as_mut().unwrap().onion.opacity = 0.3;
    let hits = pair.sweep(&gpu, &mut doc, "onion skins");
    assert!(none(&hits[..1]) && all(&hits[1..]), "{hits:?}");

    // a layer's own setting: every frame that draws it (the background, all of them)
    doc.set_visible(layers[3], false);
    let hits = pair.sweep(&gpu, &mut doc, "layer hidden");
    only_touched(&hits, &doc.clone(), &doc, &[layers[3]], "layer hidden");
    doc.set_visible(layers[0], false);
    assert!(none(&pair.sweep(&gpu, &mut doc, "background hidden")));
    doc.background = [0.2, 0.3, 0.4];
    assert!(none(&pair.sweep(&gpu, &mut doc, "background colour")));

    // something the cache can't see: everything let go
    pair.cached.forget_frames();
    assert_eq!(pair.cached.frame_cache_stats().frames, 0);
    assert!(none(&pair.sweep(&gpu, &mut doc, "forgotten")));
}

#[test]
fn the_frames_used_least_lately_go_past_the_budget() {
    let Some(gpu) = gpu() else { return };
    let (mut doc, _, mut pair) = setup(&gpu);
    let canvas = &mut pair.cached;
    canvas.set_frame_budget(Some(3 * u64::from(SIDE * SIDE * 4)));
    for f in 0..9 {
        show(canvas, Mode::Playing, f);
        assert!(!frame(canvas, &gpu, &mut doc).1);
    }
    let stats = canvas.frame_cache_stats();
    assert_eq!((stats.frames, stats.bytes), (3, u64::from(3 * SIDE * SIDE * 4)));
    // the last three are kept; frame 6 used again outlives 7 and 8
    for f in [6, 7, 8, 6] {
        show(canvas, Mode::Playing, f);
        assert!(frame(canvas, &gpu, &mut doc).1, "frame {f}");
    }
    show(canvas, Mode::Playing, 0);
    assert!(!frame(canvas, &gpu, &mut doc).1);
    for (f, kept) in [(6, true), (8, true), (7, false)] {
        show(canvas, Mode::Playing, f);
        assert_eq!(frame(canvas, &gpu, &mut doc).1, kept, "frame {f}");
    }
    // no budget, nothing kept
    canvas.set_frame_budget(Some(0));
    assert_eq!(canvas.frame_cache_stats().frames, 0);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
}

/// Scrubbing a 120-frame, 1920 × 1080 timeline frame by frame (24 drawings on fives, drawn on,
/// two onion skins each side), with and without kept frames. Run on its own, in release, for the numbers in
/// `docs/design/timeline.md`:
/// `cargo test --release -p make2d-gpu --test frame_cache -- --ignored --nocapture`.
#[test]
#[ignore = "timing: slow, and takes a gigabyte of GPU memory"]
fn scrubbing_120_frames() {
    let Some(gpu) = gpu() else { return };
    let (w, h) = (1920, 1080);
    let mut doc = Document::new(w, h);
    let mut layers = vec![doc.current()];
    for _ in 1..24 {
        layers.push(doc.add_layer());
    }
    let mut a = Animation { onion_frames: 2, ..Animation::default() };
    doc.animation = Some(a.clone());
    for f in &animation::frames(&doc) {
        a.set_hold(*f, 4);
    }
    doc.animation = Some(a);
    assert!(doc.start_timeline());
    let duration = doc.timeline.as_ref().unwrap().duration;
    assert_eq!(duration, 120);
    let make = |budget| {
        let mut c = Canvas::new(&gpu, &doc).unwrap();
        for (i, id) in layers.iter().enumerate() {
            c.upload_layer(&gpu, *id, &pixels(i, w, h)).unwrap();
        }
        c.set_frame_budget(budget);
        c
    };
    let scrub = |canvas: &mut Canvas, doc: &mut Document| -> Duration {
        let start = Instant::now();
        for f in 0..duration {
            canvas.set_timeline_frame(f);
            canvas.render_finished(&gpu, doc);
            gpu.flush();
        }
        start.elapsed()
    };
    let mut fresh = make(Some(0));
    let mut cached = make(Some(2 << 30));
    for c in [&mut fresh, &mut cached] {
        c.set_timeline_frame(duration - 1);
        c.render_finished(&gpu, &mut doc);
        gpu.flush();
    }
    let uncached = scrub(&mut fresh, &mut doc);
    let filling = scrub(&mut cached, &mut doc);
    let hits = cached.frame_cache_stats().hits;
    let kept = scrub(&mut cached, &mut doc);
    let per = |d: Duration| d.as_secs_f64() * 1000.0 / f64::from(duration);
    println!(
        "scrubbing {duration} frames at {w}x{h}: uncached {:.2} ms a frame, filling the cache {:.2} ms, from the cache {:.2} ms ({:?})",
        per(uncached),
        per(filling),
        per(kept),
        cached.frame_cache_stats()
    );
    assert_eq!(cached.frame_cache_stats().hits - hits, u64::from(duration));
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
}

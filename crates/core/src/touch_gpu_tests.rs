//! The touches send the GPU nothing from the app's own thread: what they do to the canvas is queued
//! and laid by the frames, on the render thread. A touch that kept a snapped shape laid a heavy
//! brush's stroke whole on the app's thread (some seconds of the GPU's work, sent piece by piece),
//! and a phone's Vulkan driver crashed recording the barriers of one of those pieces.

use std::time::{Duration, Instant};

use procraft_gpu::work_sent_here;

use crate::quickshape_tests::Hand;
use crate::tests::{READY, run_loop, session, settled};
use crate::{CanvasSession, PenSample, PointerKind, TouchCommand, TouchMove};

/// The app's own thread: each call made through it is looked at for whether it sent the GPU
/// anything (see [`work_sent_here`]).
#[derive(Default)]
struct AppThread {
    /// The calls that sent something, and how much.
    sent: Vec<(String, u64)>,
    calls: usize,
    /// The longest call, and which.
    longest: (Duration, String),
    /// The longest waits on the canvas within a call: how long, in which call, and where.
    waits: Vec<(Duration, String, String)>,
}

impl AppThread {
    fn call<T>(&mut self, name: &str, call: impl FnOnce() -> T) -> T {
        let (before, began) = (work_sent_here(), Instant::now());
        crate::held::take_longest_wait();
        let out = call();
        let sent = work_sent_here().wrapping_sub(before);
        if let Some((waited, at)) = crate::held::take_longest_wait() {
            self.waits.push((waited, name.to_owned(), crate::held::place(at)));
            self.waits.sort_by_key(|w| std::cmp::Reverse(w.0));
            self.waits.truncate(6);
        }
        if began.elapsed() > self.longest.0 {
            self.longest = (began.elapsed(), name.to_owned());
        }
        self.calls += 1;
        if sent > 0 {
            self.sent.push((name.to_owned(), sent));
        }
        out
    }

    /// A tick at `at` (ms), and those the app is then asked for while input is kept for later (the
    /// canvas held by a frame), until all is taken: what the touches asked of the app.
    fn tick(&mut self, s: &CanvasSession, name: &str, at: f64) -> Vec<TouchCommand> {
        let mut out = self.call(name, || s.touch_tick(at));
        while let Some(due) = s.kept_input_due() {
            std::thread::sleep(Duration::from_millis(2));
            out.extend(self.call(name, || s.touch_tick(due.max(at))));
        }
        out
    }
}

fn pen(x: f32, y: f32, time_ms: f64) -> Vec<PenSample> {
    vec![PenSample { x, y, pressure: 0.7, tilt: 0.0, orientation: 0.0, time_ms, roll: 0.0 }]
}

/// Picks the stock brush `name` for painting, at `size`; false if there is none so named.
fn use_brush(s: &CanvasSession, name: &str, size: f32) -> bool {
    let at = {
        let st = s.shared.lock();
        st.library.iter().enumerate().find_map(|(set, entry)| {
            let at = entry.brushes.iter().position(|b| b.name == name)?;
            Some((set as u32, at as u32))
        })
    };
    let Some((set, index)) = at else { return false };
    if !s.select_brush(set, index) {
        return false;
    }
    // its big pictures made (off this thread), as they would be by the time a hand draws (made
    // aside, once their making has begun)
    if crate::big_pictures::ASIDE.with(std::cell::Cell::get) {
        let asked = Instant::now();
        while crate::big_pictures::big_pictures_left() == 0 && asked.elapsed() < Duration::from_secs(1) {
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    crate::big_pictures::wait_for_big_pictures(Duration::from_secs(60));
    s.set_brush_size(size);
    true
}

/// What the app's thread does in a while of drawing through the touches, starting at `t` (ms): a
/// pen stroke; a loop held until it snaps, dragged out by the pen still holding it and let go;
/// (`edit`) the shape edited, a handle of it dragged and a tap elsewhere keeping it; a new pen
/// stroke, which keeps the shape if it is still open; a two-finger pinch; the ticks between.
/// `frame` stands for the render thread meanwhile. When it ended (ms).
fn draw_through_touches(
    s: &CanvasSession,
    app: &mut AppThread,
    frame: &mut dyn FnMut(&CanvasSession),
    t: f64,
    edit: bool,
) -> f64 {
    let mut out: Vec<TouchCommand> = Vec::new();
    // a pen stroke
    out.extend(app.call("touch_down (pen)", || s.touch_down(1, PointerKind::Stylus, pen(200.0, 200.0, t))));
    for i in 1..=30 {
        let (x, ms) = (200.0 + i as f32 * 12.0, t + f64::from(i) * 8.0);
        out.extend(app.call("touch_moves (pen)", || {
            s.touch_moves(vec![TouchMove { id: 1, samples: pen(x, 200.0 + (x / 40.0).sin() * 30.0, ms) }])
        }));
        if i % 4 == 0 {
            frame(s);
        }
    }
    out.extend(app.call("touch_up (pen)", || s.touch_up(1, pen(560.0, 200.0, t + 250.0))));
    frame(s);
    // a loop held until it snaps, through the touches as the app hands them over
    let t = t + 400.0;
    let mut hand = Hand::small(1.04);
    hand.centre = (500.0, 550.0);
    hand.rx = 220.0;
    hand.ry = 160.0;
    let samples: Vec<PenSample> =
        hand.samples(700.0).into_iter().map(|p| PenSample { time_ms: p.time_ms + t, ..p }).collect();
    let Some(first) = samples.first().copied() else { return t };
    out.extend(app.call("touch_down (pen, loop)", || s.touch_down(1, PointerKind::Stylus, vec![first])));
    for (i, chunk) in samples[1..].chunks(4).enumerate() {
        out.extend(app.call("touch_move (pen, loop)", || s.touch_move(1, chunk.to_vec())));
        if i % 4 == 0 {
            frame(s);
        }
    }
    let last = *samples.last().unwrap_or(&first);
    let snapped = app.tick(s, "touch_tick (the hold)", last.time_ms + 700.0);
    let snapped_ok = snapped.contains(&TouchCommand::ShapeSnapped);
    if !snapped_ok {
        let (deadline, kept) = (s.touch_deadline(), s.kept_input_due());
        let st = s.shared.lock();
        let tail: Vec<_> = out.iter().rev().take(4).collect();
        panic!(
            "the loop never snapped at {} (zoom {:?}): a stroke {}, a shape {}, {} of {} drawn, pen input waiting {}, \
             the touches' deadline {deadline:?}, input kept {kept:?}; asked {snapped:?} by the tick, {tail:?} last",
            last.time_ms + 700.0,
            st.view.map(|v| v.zoom()),
            st.stroke.is_some(),
            st.shape.is_some(),
            st.drawn.len(),
            samples.len(),
            s.shared.input_waiting(),
        );
    }
    out.extend(snapped);
    frame(s);
    let mut t = last.time_ms + 720.0;
    if snapped_ok {
        // dragged out by the pen still holding it, frames between
        for k in 1..=12 {
            let at = (last.x + k as f32 * 6.0, last.y + k as f32 * 4.0);
            out.extend(app.call("touch_move (pen, holding the shape)", || {
                s.touch_move(1, pen(at.0, at.1, t + f64::from(k) * 8.0))
            }));
            frame(s);
        }
        t += 120.0;
        out.extend(
            app.call("touch_up (pen, letting the shape go)", || s.touch_up(1, pen(last.x + 72.0, last.y + 48.0, t))),
        );
        frame(s);
        // the app's Edit Shape: a handle dragged by the pen, then a tap elsewhere keeps the shape
        if edit {
            s.set_shape_editing(true);
            if let Some(info) = app.call("quickshape_info", || s.quickshape_info())
                && let (Some(&hx), Some(&hy)) = (info.handles.first(), info.handles.get(1))
            {
                out.extend(app.call("touch_down (pen, on a handle)", || {
                    s.touch_down(2, PointerKind::Stylus, pen(hx, hy, t + 100.0))
                }));
                for k in 1..=10 {
                    out.extend(app.call("touch_move (pen, the handle)", || {
                        s.touch_move(2, pen(hx + k as f32 * 5.0, hy, t + 100.0 + f64::from(k) * 10.0))
                    }));
                    frame(s);
                }
                out.extend(app.call("touch_up (pen, the handle)", || s.touch_up(2, pen(hx + 50.0, hy, t + 220.0))));
                frame(s);
                t += 240.0;
            }
            out.extend(app.call("touch_down (pen, a tap keeping the shape)", || {
                s.touch_down(2, PointerKind::Stylus, pen(80.0, 80.0, t + 100.0))
            }));
            out.extend(
                app.call("touch_up (pen, a tap keeping the shape)", || s.touch_up(2, pen(80.0, 80.0, t + 140.0))),
            );
            frame(s);
            t += 160.0;
        }
    }
    // a new pen stroke elsewhere keeps the shape (if still open), and draws on
    let t = t + 300.0;
    out.extend(
        app.call("touch_down (pen, keeping the shape)", || s.touch_down(3, PointerKind::Stylus, pen(150.0, 900.0, t))),
    );
    for i in 1..=20 {
        let (x, ms) = (150.0 + i as f32 * 15.0, t + f64::from(i) * 8.0);
        out.extend(app.call("touch_move (pen, after the shape)", || s.touch_move(3, pen(x, 900.0, ms))));
        if i % 5 == 0 {
            frame(s);
        }
    }
    out.extend(app.call("touch_up (pen, after the shape)", || s.touch_up(3, pen(450.0, 900.0, t + 170.0))));
    frame(s);
    // two fingers pinch the view larger (and with `edit`, smaller again)
    let t = t + 400.0;
    let (from, to) = if edit { (220.0, 100.0) } else { (100.0, 220.0) };
    out.extend(app.call("touch_down (fingers)", || s.touch_down(4, PointerKind::Finger, pen(500.0 - from, 500.0, t))));
    out.extend(
        app.call("touch_down (fingers)", || s.touch_down(5, PointerKind::Finger, pen(500.0 + from, 500.0, t + 4.0))),
    );
    for i in 1..=20 {
        let (spread, ms) = (from + (to - from) * i as f32 / 20.0, t + 4.0 + f64::from(i) * 8.0);
        out.extend(app.call("touch_moves (fingers)", || {
            s.touch_moves(vec![
                TouchMove { id: 4, samples: pen(500.0 - spread, 500.0, ms) },
                TouchMove { id: 5, samples: pen(500.0 + spread, 500.0, ms) },
            ])
        }));
        if i % 5 == 0 {
            frame(s);
        }
    }
    out.extend(app.call("touch_up (fingers)", || s.touch_up(5, pen(500.0 + to, 500.0, t + 180.0))));
    out.extend(app.call("touch_up (fingers)", || s.touch_up(4, pen(500.0 - to, 500.0, t + 184.0))));
    let t = t + 200.0;
    if let Some(due) = app.call("touch_deadline", || s.touch_deadline()) {
        out.extend(app.tick(s, "touch_tick", due.max(t)));
    }
    frame(s);
    t + 100.0
}

/// A pen's drag through the touches (pointer `id`) from (`x`, `y`) `dx` across and `dy` down in
/// `steps` moves, from `t` (ms), frames between; named `what` among the app's calls. When it
/// ended (ms).
fn drag(
    s: &CanvasSession,
    app: &mut AppThread,
    frame: &mut dyn FnMut(&CanvasSession),
    what: &str,
    (id, kind): (u64, PointerKind),
    (x, y, dx, dy): (f32, f32, f32, f32),
    (t, steps): (f64, u32),
) -> f64 {
    app.call(&format!("touch_down ({what})"), || s.touch_down(id, kind, pen(x, y, t)));
    for k in 1..=steps {
        let f = k as f32 / steps as f32;
        let at = (x + dx * f, y + dy * f + (f * 9.0).sin() * 12.0);
        app.call(&format!("touch_move ({what})"), || s.touch_move(id, pen(at.0, at.1, t + f64::from(k) * 8.0)));
        if k % 4 == 0 {
            frame(s);
        }
    }
    let end = t + f64::from(steps + 1) * 8.0;
    app.call(&format!("touch_up ({what})"), || s.touch_up(id, pen(x + dx, y + dy, end)));
    frame(s);
    end + 100.0
}

/// The other tools' touches, from `t` (ms), as the app's thread takes them, with what the app
/// calls to open and close each: a smudge's and an eraser's strokes; a freehand selection drawn
/// and the tool closed; the layer transformed, its box dragged, and kept; Liquify pushed about
/// and kept; the layer cleared; Continue filling's tap; a pen's stroke before each of the last three
/// opens, its end still being laid. `frame` stands for the render thread. When it ended (ms).
fn other_tools_through_touches(
    s: &CanvasSession,
    app: &mut AppThread,
    frame: &mut dyn FnMut(&CanvasSession),
    t: f64,
) -> f64 {
    use crate::touch::{Fill, SelectKind, SelectTouch, TransformTouch};
    let pen_at = (1, PointerKind::Stylus);
    let mut t = t;
    for (tool, name) in [(crate::ToolKind::Smudge, "smudge"), (crate::ToolKind::Erase, "eraser")] {
        app.call("set_tool", || s.set_tool(tool));
        t = drag(s, app, frame, name, pen_at, (200.0, 400.0, 500.0, 60.0), (t, 30));
    }
    app.call("set_tool", || s.set_tool(crate::ToolKind::Paint));
    // a freehand selection, the tool closed
    let how = SelectTouch { kind: SelectKind::Freehand, join: crate::SelectionJoin::Add, threshold: 0.3 };
    app.call("set_touch_selecting", || s.set_touch_selecting(Some(how)));
    t = drag(s, app, frame, "selection", pen_at, (150.0, 150.0, 600.0, 500.0), (t, 30));
    app.call("set_touch_selecting (closed)", || s.set_touch_selecting(None));
    app.call("select_none", || s.select_none());
    frame(s);
    // the layer transformed (a stroke just drawn, its end still being laid): its box dragged from
    // its middle, and kept
    t = drag(s, app, frame, "pen", pen_at, (200.0, 700.0, 500.0, -80.0), (t, 30));
    if let Some(shown) = app.call("transform_begin", || s.transform_begin()) {
        app.call("set_touch_transforming", || s.set_touch_transforming(Some(TransformTouch { advanced_mesh: false })));
        let corners = shown.corners.as_chunks::<2>().0;
        let n = corners.len().max(1) as f32;
        let (cx, cy) = corners.iter().fold((0.0, 0.0), |(x, y), [a, b]| (x + a / n, y + b / n));
        t = drag(s, app, frame, "transform", (2, PointerKind::Finger), (cx, cy, 60.0, 40.0), (t, 20));
        app.call("transform_end", || s.transform_end(true));
        app.call("set_touch_transforming (closed)", || s.set_touch_transforming(None));
    }
    frame(s);
    // Liquify (a stroke just drawn) pushed about by the pen, and kept
    t = drag(s, app, frame, "pen", pen_at, (200.0, 800.0, 500.0, -80.0), (t, 30));
    if app.call("adjustment_start (Liquify)", || s.adjustment_start(crate::Adjustment::Liquify)) {
        t = drag(s, app, frame, "liquify", pen_at, (300.0, 300.0, 300.0, 200.0), (t, 30));
        app.call("adjustment_end (Liquify)", || s.adjustment_end(true));
    }
    frame(s);
    // the layer cleared, a stroke just drawn
    t = drag(s, app, frame, "pen", pen_at, (200.0, 850.0, 500.0, -80.0), (t, 30));
    app.call("clear_layer", || s.clear_layer());
    frame(s);
    // Continue filling's tap
    let fill = Fill { r: 0.9, g: 0.5, b: 0.1, threshold: 0.3 };
    app.call("set_touch_filling", || s.set_touch_filling(Some(fill)));
    app.call("touch_down (fill)", || s.touch_down(3, PointerKind::Finger, pen(80.0, 900.0, t)));
    app.call("touch_up (fill)", || s.touch_up(3, pen(80.0, 900.0, t + 40.0)));
    app.call("set_touch_filling (closed)", || s.set_touch_filling(None));
    frame(s);
    t + 200.0
}

/// Big pictures made aside as the app makes them, off its thread, while this is held (tests make
/// them at once where they are wanted, so that their strokes come out the same each time).
struct MadeAside;

impl MadeAside {
    fn on() -> MadeAside {
        crate::big_pictures::ASIDE.with(|a| a.set(true));
        MadeAside
    }
}

impl Drop for MadeAside {
    fn drop(&mut self) {
        crate::big_pictures::wait_for_big_pictures(Duration::from_secs(120));
        crate::big_pictures::ASIDE.with(|a| a.set(false));
    }
}

/// The canvas the touches draw on, the tablets' way with their input, shown in a 1024 window.
fn tablet() -> Option<std::sync::Arc<CanvasSession>> {
    let s = session(1024, 1024)?;
    s.shared.set_touch_mail_on(true);
    s.set_display_scale(2.0);
    Some(s)
}

/// No call the touches take from the app's thread sends the GPU anything: not a pen's stroke, nor
/// a loop held until it snaps, dragged and let go, nor a handle of it dragged, nor the tap or the
/// new stroke that keeps it (which laid all of a heavy brush's shape at once there before, and
/// crashed a phone's Vulkan driver), nor a pinch, for a plain brush and heavy ones, big (their big pictures)
/// or not. The frames (drawn on this thread between the calls, as the render thread would) lay
/// it all, with no GPU errors. Printed: the calls that sent something.
#[test]
fn the_touches_send_the_gpu_nothing_from_the_apps_thread() {
    let mut app = AppThread::default();
    let mut drawn = 0usize;
    let mut made = Vec::new();
    for (brush, size) in [("", 0.3), ("Quicksilver", 1.0), ("Gouache", 1.0), ("Wet Acrylic", 0.5)] {
        let Some(s) = tablet() else { return };
        if !brush.is_empty() && !use_brush(&s, brush, size) {
            println!("no {brush}: left out");
            continue;
        }
        let (view, _target) = {
            let st = s.shared.lock();
            let Some(gpu) = st.gpu.as_ref() else { return };
            let target = gpu.device.create_texture(&procraft_gpu::wgpu::TextureDescriptor {
                label: Some("window"),
                size: procraft_gpu::wgpu::Extent3d { width: 1024, height: 1024, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: procraft_gpu::wgpu::TextureDimension::D2,
                format: procraft_gpu::wgpu::TextureFormat::Rgba8Unorm,
                usage: procraft_gpu::wgpu::TextureUsages::RENDER_ATTACHMENT,
                view_formats: &[],
            });
            (target.create_view(&Default::default()), target)
        };
        let started = Instant::now();
        let mut frame = |s: &CanvasSession| {
            let mut st = s.shared.lock();
            s.shared.take_queued_input(&mut st);
            st.draw_frame(
                &view,
                procraft_gpu::wgpu::TextureFormat::Rgba8Unorm,
                (1024, 1024),
                procraft_engine::profile::Profile::Srgb,
                started,
            );
            drawn += 1;
        };
        let before = app.sent.len();
        let t = draw_through_touches(&s, &mut app, &mut frame, 1000.0, false);
        draw_through_touches(&s, &mut app, &mut frame, t, true);
        // the frames lay what is left
        for _ in 0..2000 {
            let more = {
                let st = s.shared.lock();
                st.needs_frame || s.shared.touch_input_kept() || st.canvas.as_ref().is_some_and(|c| c.busy())
            };
            if !more {
                break;
            }
            frame(&s);
        }
        let st = s.shared.lock();
        let gpu = st.gpu.as_ref().unwrap();
        gpu.flush();
        assert_eq!(gpu.errors(), 0, "{brush}: GPU errors");
        assert!(st.shape.is_none(), "{brush}: the shape still open");
        // the pens' strokes made, the shapes among them, twice over
        made.push((brush, st.info.strokes, st.canvas.as_ref().map_or(0, |c| c.steps_made())));
        let sent: Vec<_> = app.sent[before..].iter().map(|(n, k)| format!("{n} ({k})")).collect();
        // (a big brush's strokes read its big pictures: put on the GPU by the frames)
        let held = st.canvas.as_ref().map(|c| c.full_images_held());
        println!(
            "{brush:>12} at {size}: {} of the app's calls sent the GPU something: {sent:?}; shapes and grains at their \
             own size held {held:?}",
            sent.len()
        );
    }
    println!("{} calls, {drawn} frames; strokes and steps made: {made:?}", app.calls);
    for (brush, strokes, steps) in &made {
        assert!(*strokes >= 6 && *steps >= 6, "{brush}: {strokes} strokes, {steps} steps");
    }
    assert!(app.sent.is_empty(), "the app's thread sent the GPU work: {:?}", app.sent);
}

/// The app's thread drawing through the touches (strokes, loops snapped and dragged, handles
/// dragged, new strokes keeping the shapes, pinches) for rounds on end as the render loop draws
/// its frames on a thread of its own, the tablets' way: the GPU sees no errors and isn't lost,
/// every shape is kept, and the app's thread sends it nothing. For a heavy brush, big. Run on
/// OpenGL too (`WGPU_BACKEND=gl`). Printed: the rounds, the frames drawn, and the app's longest
/// call.
#[test]
fn quickshapes_kept_by_new_touches_as_the_render_loop_draws_give_the_gpu_no_errors() {
    let rounds: usize = std::env::var("TOUCH_GPU_ROUNDS").ok().and_then(|r| r.parse().ok()).unwrap_or(6);
    // (the brushes' big pictures made as the app makes them)
    let _aside = MadeAside::on();
    for (brush, size) in [("", 0.3), ("Quicksilver", 1.0)] {
        let Some(s) = tablet() else { return };
        if !brush.is_empty() && !use_brush(&s, brush, size) {
            println!("no {brush}: left out");
            continue;
        }
        let backend = s.shared.lock().gpu.as_ref().map(|g| format!("{:?}", g.adapter.get_info().backend));
        let running = run_loop(&s, READY);
        let mut app = AppThread::default();
        let started = Instant::now();
        let mut t = 1000.0;
        for round in 0..rounds {
            // (the render loop draws the frames: here the app's thread only lets time go by)
            let mut frame = |_: &CanvasSession| std::thread::sleep(Duration::from_millis(4));
            t = draw_through_touches(&s, &mut app, &mut frame, t, round % 2 == 1);
            // the input kept while a frame held the canvas, taken by the ticks the app is asked for
            app.tick(&s, "touch_tick (kept input)", t);
        }
        settled(&s);
        let frames = s.activity().frames;
        drop(running);
        let st = s.shared.lock();
        let gpu = st.gpu.as_ref().unwrap();
        gpu.flush();
        println!(
            "{brush:>12} on {backend:?}: {rounds} rounds, {} calls in {:.1} s, {frames} frames, the longest call \
             {:.0} ms ({}); sent from the app's thread: {:?}",
            app.calls,
            started.elapsed().as_secs_f64(),
            app.longest.0.as_secs_f64() * 1000.0,
            app.longest.1,
            app.sent
        );
        let waits: Vec<String> = app
            .waits
            .iter()
            .map(|(d, call, at)| format!("{:.0} ms in {call} at {at}", d.as_secs_f64() * 1000.0))
            .collect();
        println!("{brush:>12}: the app's longest waits on the canvas: {waits:?}");
        assert_eq!(gpu.errors(), 0, "{brush}: GPU errors");
        assert!(!gpu.is_lost(), "{brush}: the GPU lost");
        assert!(st.shape.is_none(), "{brush}: a shape still open");
        assert!(st.info.strokes >= 3 * rounds as u64, "{brush}: {} strokes", st.info.strokes);
        assert!(app.sent.is_empty(), "{brush}: the app's thread sent the GPU work: {:?}", app.sent);
    }
}

/// The canvas as it is once the render loop has laid everything: its composite.
fn laid_picture(s: &CanvasSession) -> Vec<u8> {
    settled(s);
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else { return Vec::new() };
    canvas.render_finished(gpu, &mut st.doc);
    canvas.read_composite(gpu).unwrap_or_default()
}

/// A canvas laying strokes as a slow GPU does: a little of their paint at each frame (as fast as
/// `rate` says, work a millisecond), so a heavy brush's stroke takes many frames after the pen lifts.
fn slow_gpu(s: &CanvasSession, rate: f64) {
    let mut st = s.shared.lock();
    if let Some(canvas) = st.canvas.as_mut() {
        canvas.set_dry_paced(true, Some(rate));
        canvas.set_in_place_rate(Some(rate));
        // (a stroke laid again as it ends: a piece of its work a frame)
        canvas.set_relay_at_once(Duration::ZERO);
    }
}

/// An undo as the frames (on a thread of their own) still lay a heavy brush's stroke a share at a
/// time, and a redo after it, are queued behind that work: the app's thread sends the GPU nothing
/// and waits on no laying, the history reads at once as it will be, and once the frames have laid
/// it all the canvas is as with the undo (and redo) made after they had, pixel for pixel. Before,
/// each laid the rest of the stroke at once on the app's thread: a heavy brush's stroke held it for
/// seconds, and a phone's Vulkan driver crashed under such work. Printed: how long the calls took.
#[test]
fn an_undo_as_the_frames_still_lay_a_heavy_stroke_waits_behind_it() {
    // the picture once all is laid after a heavy stroke and an undo (and a redo), made as the
    // frames still lay the stroke (`early`) or after; the undo's and redo's calls, and whether the
    // frames were laying as the undo came
    let run = |early: bool, redo: bool| -> Option<(Vec<u8>, AppThread, bool)> {
        let s = tablet()?;
        if !use_brush(&s, "Quicksilver", 1.0) {
            return None;
        }
        // (the least each frame lays, a stamp of the stroke's at a time)
        slow_gpu(&s, 1.0);
        if let Some(canvas) = s.shared.lock().canvas.as_mut() {
            canvas.set_stamps_at_once(1);
        }
        let running = run_loop(&s, READY);
        let mut touches = AppThread::default();
        let mut frame = |_: &CanvasSession| std::thread::sleep(Duration::from_millis(4));
        let pen_at = (1, PointerKind::Stylus);
        drag(&s, &mut touches, &mut frame, "pen", pen_at, (150.0, 300.0, 700.0, 400.0), (1000.0, 40));
        touches.tick(&s, "touch_tick (kept input)", 2000.0);
        if !early {
            settled(&s);
        }
        let laying = s.shared.lock().paced_work_waiting();
        let unshown = |s: &CanvasSession| s.shared.lock().canvas.as_ref().map_or(0, |c| c.laid_unshown());
        let before = unshown(&s);
        let mut app = AppThread::default();
        app.call("undo", || s.undo());
        let info = s.info();
        assert!(info.can_redo && !info.can_undo, "after the undo: {info:?}");
        if redo {
            app.call("redo", || s.redo());
            let info = s.info();
            assert!(info.can_undo && !info.can_redo, "after the redo: {info:?}");
        }
        settled(&s);
        if early {
            assert_eq!(unshown(&s), before, "a render that showed nothing laid the stroke at once");
        }
        let picture = laid_picture(&s);
        drop(running);
        let gpu = s.shared.lock().gpu.clone()?;
        gpu.flush();
        assert_eq!(gpu.errors(), 0, "GPU errors");
        Some((picture, app, laying))
    };
    // the most two pictures' channels differ by, and how many differ
    let differ = |a: &[u8], b: &[u8]| {
        a.iter().zip(b).fold((0u8, 0usize), |(most, n), (x, y)| (most.max(x.abs_diff(*y)), n + usize::from(x != y)))
    };
    for redo in [false, true] {
        let (Some((early, app, laying)), Some((late, _, _))) = (run(true, redo), run(false, redo)) else { return };
        // (the stroke laid after the frames again: how far apart two such come out)
        let Some((again, _, _)) = run(false, redo) else { return };
        let ms = |d: Duration| d.as_secs_f64() * 1000.0;
        let what = if redo { "undo and redo" } else { "undo" };
        let (apart, twice) = (differ(&early, &late), differ(&late, &again));
        println!(
            "{what} as the frames still lay the stroke ({laying}): the longest call {:.1} ms ({}); sent {:?}; \
             against after the frames {} channels apart by {} at most (made after them twice: {} by {})",
            ms(app.longest.0),
            app.longest.1,
            app.sent,
            apart.1,
            apart.0,
            twice.1,
            twice.0
        );
        assert!(laying, "the frames had laid the stroke before the {what} came: no test of it");
        assert!(app.sent.is_empty(), "the app's thread sent the GPU work: {:?}", app.sent);
        assert!(app.longest.0 < Duration::from_millis(100), "{} took {:?}", app.longest.1, app.longest.0);
        // pixel for pixel, but for a level in a few places: this brush's stroke laid at a crawl came
        // out so against it laid at once, once, under the other tests' load (a stroke undone, or left
        // out, would be a stroke's worth apart)
        assert!(!early.is_empty() && early.len() == late.len(), "{what}: no picture");
        assert!(
            apart.0 <= 1 && apart.1 <= early.len() / 1000,
            "{what}: laid otherwise than after the frames: {apart:?}"
        );
    }
}

/// The other tools as the app's thread takes their touches (a smudge's and an eraser's strokes, a
/// freehand selection, a transform's drag, Liquify, Continue filling's tap), each opened and
/// closed as the app does, while the render loop draws its frames on a thread of their own, a
/// heavy brush's stroke still being laid as each opens: the touches send the GPU nothing, no render
/// that shows nothing lays a stroke at once (the app's calls that run the canvas's work wait for the
/// frames to lay it first), and the GPU sees no errors. Printed: which of the app's calls sent the
/// GPU something, and the longest.
#[test]
fn the_other_tools_touches_as_the_render_loop_draws() {
    let _aside = MadeAside::on();
    let Some(s) = tablet() else { return };
    if !use_brush(&s, "Quicksilver", 0.6) {
        return;
    }
    // (strokes laid the least a frame lays: their ends still laid as the tools open)
    slow_gpu(&s, 1.0);
    let unshown = |s: &CanvasSession| s.shared.lock().canvas.as_ref().map_or(0, |c| c.laid_unshown());
    let laid_unshown = unshown(&s);
    let running = run_loop(&s, READY);
    let mut app = AppThread::default();
    let mut frame = |_: &CanvasSession| std::thread::sleep(Duration::from_millis(4));
    let t = draw_through_touches(&s, &mut app, &mut frame, 1000.0, false);
    let t = other_tools_through_touches(&s, &mut app, &mut frame, t);
    app.tick(&s, "touch_tick (kept input)", t);
    settled(&s);
    drop(running);
    let laid_unshown = unshown(&s) - laid_unshown;
    let gpu = s.shared.lock().gpu.clone().unwrap();
    gpu.flush();
    let sent: Vec<String> = app.sent.iter().map(|(n, k)| format!("{n} ({k})")).collect();
    println!(
        "{} calls, the longest {:.0} ms ({}); {laid_unshown} stamps laid by renders that showed nothing; sent \
         from the app's thread: {sent:?}",
        app.calls,
        app.longest.0.as_secs_f64() * 1000.0,
        app.longest.1
    );
    assert_eq!(gpu.errors(), 0, "GPU errors");
    assert_eq!(laid_unshown, 0, "a render that showed nothing laid a stroke's work at once");
    let touches: Vec<&(String, u64)> = app.sent.iter().filter(|(n, _)| n.starts_with("touch_")).collect();
    assert!(touches.is_empty(), "the touches sent the GPU work from the app's thread: {touches:?}");
}

/// A big brush's stroke begun after its big pictures left memory (kept in their files: let go for
/// other brushes', or as the system ran short) reads them back before it holds the canvas, and
/// draws with them as with them held: the frames go on meanwhile. Under the canvas, the reading
/// (a 2048 square picture decoded and its smaller levels made, for its shape and its grain) held
/// the frames up. Printed: how long the pen's touch took, the pictures held and read.
#[test]
fn a_big_brush_stroke_reads_its_pictures_back_before_it_holds_the_canvas() {
    let _aside = MadeAside::on();
    let dir = std::env::temp_dir().join(format!("procraft-touch-read-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    crate::big_pictures::use_folder(&dir);
    let begun = |let_go: bool| -> Option<(Duration, (usize, usize))> {
        let s = tablet()?;
        if !use_brush(&s, "Quicksilver", 1.0) {
            return None;
        }
        if let_go {
            crate::big_pictures::let_go();
        }
        let mut app = AppThread::default();
        let mut frame = |s: &CanvasSession| {
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            if let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
                canvas.render(gpu, &mut st.doc, None);
            }
        };
        drag(&s, &mut app, &mut frame, "pen", (1, PointerKind::Stylus), (200.0, 300.0, 500.0, 300.0), (1000.0, 20));
        let held = s.shared.lock().canvas.as_ref().map(|c| c.full_images_held())?;
        let took = app.longest.0;
        Some((took, held))
    };
    let (Some((held_took, held)), Some((read_took, read))) = (begun(false), begun(true)) else { return };
    let _ = std::fs::remove_dir_all(&dir);
    let ms = |d: Duration| d.as_secs_f64() * 1000.0;
    println!(
        "the pen's longest call: {:.1} ms with the pictures held {held:?}, {:.1} ms read back {read:?}",
        ms(held_took),
        ms(read_took)
    );
    assert!(held.0 + held.1 > 0, "the stroke read no big picture");
    assert_eq!(read, held, "drawn otherwise with its pictures read back");
}

/// QuickShape on the last stroke as the frames (on a thread of their own) still lay that heavy
/// brush's stroke a share at a time: the stroke's undo and its shape are queued behind that work,
/// the app's thread sends the GPU nothing, no render that shows nothing lays the stroke at once, and
/// once all is laid the shape kept is as with QuickShape made after the frames had laid the stroke,
/// pixel for pixel. Before, it laid the rest of the stroke at once on the app's thread.
#[test]
fn quickshape_on_the_last_stroke_as_the_frames_still_lay_it_waits_behind_it() {
    let run = |early: bool| -> Option<(Vec<u8>, AppThread, bool)> {
        let s = tablet()?;
        if !use_brush(&s, "Quicksilver", 1.0) {
            return None;
        }
        slow_gpu(&s, 1.0);
        if let Some(canvas) = s.shared.lock().canvas.as_mut() {
            canvas.set_stamps_at_once(1);
        }
        let running = run_loop(&s, READY);
        let mut touches = AppThread::default();
        let mut hand = Hand::small(1.04);
        hand.centre = (500.0, 500.0);
        hand.rx = 260.0;
        hand.ry = 180.0;
        let samples = hand.samples(0.0);
        let first = *samples.first()?;
        touches.call("touch_down", || s.touch_down(1, PointerKind::Stylus, vec![first]));
        for chunk in samples[1..].chunks(4) {
            touches.call("touch_move", || s.touch_move(1, chunk.to_vec()));
            std::thread::sleep(Duration::from_millis(2));
        }
        let last = *samples.last()?;
        touches.call("touch_up", || s.touch_up(1, vec![last]));
        touches.tick(&s, "touch_tick (kept input)", last.time_ms + 50.0);
        if !early {
            settled(&s);
        }
        let laying = s.shared.lock().paced_work_waiting();
        let unshown = |s: &CanvasSession| s.shared.lock().canvas.as_ref().map_or(0, |c| c.laid_unshown());
        let before = unshown(&s);
        let mut app = AppThread::default();
        let shape = app.call("quickshape_last", || s.quickshape_last());
        assert!(shape.is_some(), "the last stroke snapped to no shape");
        app.call("quickshape_done", || s.quickshape_done());
        settled(&s);
        if early {
            assert_eq!(unshown(&s), before, "a render that showed nothing laid the stroke at once");
        }
        let picture = laid_picture(&s);
        drop(running);
        let gpu = s.shared.lock().gpu.clone()?;
        gpu.flush();
        assert_eq!(gpu.errors(), 0, "GPU errors");
        Some((picture, app, laying))
    };
    let (Some((early, app, laying)), Some((late, _, _))) = (run(true), run(false)) else { return };
    println!(
        "QuickShape on the last stroke as the frames still lay it ({laying}): the longest call {:.1} ms ({}); sent \
         {:?}",
        app.longest.0.as_secs_f64() * 1000.0,
        app.longest.1,
        app.sent
    );
    assert!(laying, "the frames had laid the stroke before QuickShape came: no test of it");
    assert!(app.sent.is_empty(), "the app's thread sent the GPU work: {:?}", app.sent);
    assert!(!early.is_empty() && early == late, "kept otherwise than after the frames");
}

/// A transform begun as the frames (on a thread of their own) still lay a heavy brush's stroke a
/// share at a time waits for them to lay it, the canvas let go meanwhile, and only then reads the
/// layer: no render that shows nothing lays the stroke at once (before, the app's thread laid the
/// rest of it at once there, and nested its pieces' renders past the stack). Printed: the wait.
#[test]
fn a_transform_begun_as_the_frames_still_lay_a_stroke_waits_for_them() {
    let Some(s) = tablet() else { return };
    if !use_brush(&s, "Quicksilver", 1.0) {
        return;
    }
    // (a stamp a frame at the least pace, so that the frames still lay it as the transform begins;
    // once it waits, as much a frame as there is, so that the next frames lay the rest well within
    // the bound: at the GPU's own pace a debug build's frames took past it)
    slow_gpu(&s, 1.0);
    if let Some(canvas) = s.shared.lock().canvas.as_mut() {
        canvas.set_stamps_at_once(1);
    }
    let running = run_loop(&s, READY);
    let mut touches = AppThread::default();
    let mut frame = |_: &CanvasSession| std::thread::sleep(Duration::from_millis(4));
    drag(&s, &mut touches, &mut frame, "pen", (1, PointerKind::Stylus), (150.0, 300.0, 700.0, 400.0), (1000.0, 40));
    touches.tick(&s, "touch_tick (kept input)", 2000.0);
    let laying = s.shared.lock().paced_work_waiting();
    let unshown = |s: &CanvasSession| s.shared.lock().canvas.as_ref().map_or(0, |c| c.laid_unshown());
    let before = unshown(&s);
    let mut app = AppThread::default();
    let shown = std::thread::scope(|scope| {
        scope.spawn(|| {
            std::thread::sleep(Duration::from_millis(100));
            if let Some(canvas) = s.shared.lock().canvas.as_mut() {
                canvas.set_dry_paced(true, Some(1e12));
                canvas.set_in_place_rate(Some(1e12));
                canvas.set_relay_at_once(Duration::from_secs(10));
                canvas.set_stamps_at_once(1 << 20);
            }
        });
        app.call("transform_begin", || s.transform_begin())
    });
    let laid_after = unshown(&s) - before;
    app.call("transform_end", || s.transform_end(true));
    settled(&s);
    drop(running);
    let gpu = s.shared.lock().gpu.clone().unwrap();
    gpu.flush();
    println!(
        "a transform begun as the frames still lay a stroke ({laying}): waited {:.0} ms; {laid_after} stamps laid by \
         renders that showed nothing",
        app.longest.0.as_secs_f64() * 1000.0
    );
    assert!(laying, "the frames had laid the stroke before the transform began: no test of it");
    assert!(shown.is_some(), "no transform began");
    assert_eq!(laid_after, 0, "a render that showed nothing laid the stroke at once");
    // (the frames sped up after 100 ms)
    assert!(app.longest.0 >= Duration::from_millis(90), "didn't wait for the frames: {:?}", app.longest.0);
    assert!(app.longest.0 < crate::FRAMES_LAY_MOST, "waited past the frames' bound: {:?}", app.longest.0);
    assert_eq!(gpu.errors(), 0, "GPU errors");
}

//! What each library brush costs to draw: the same stroke with every one, at one size in canvas
//! pixels, each frame drawn into a window and the GPU waited for after it, so a frame's time is
//! what the GPU took for it. A bench, run by hand with `--ignored --nocapture`; `WGPU_BACKEND=gl`
//! runs it on OpenGL (a stand-in for a weak device), `AUDIT_LIMITS=small` holds the GPU to a budget
//! tablet's limits, `AUDIT_BUDGET_MB` to its memory. Writes a row a brush (and a row a variant of
//! it, see [`Variant`]) to `AUDIT_OUT`. Beside it, what making the stamps costs the CPU alone
//! ([`brush_tip_cpu`]) and a digest of every stamp each brush makes ([`brush_stamp_digests`]).

use std::io::Write;
use std::time::Instant;

use procraft_engine::brush::{Brush, GrainMode, Rendering};
use procraft_engine::view::View;
use procraft_gpu::{Gpu, wgpu};

use crate::{CanvasSession, PenSample, PointerKind, ToolKind};

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1000.0
}

fn env(name: &str, default: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| default.to_owned())
}

/// The `k`th sample of the stroke (a pen at 240 Hz, 6 screen pixels a sample): back and forth
/// across the canvas fitted in a `width` × `height` window, a slow wave up and down, pressure
/// rising and falling.
fn point(k: usize, width: f32, height: f32) -> PenSample {
    let side = height;
    let left = (width - side) / 2.0 + 80.0;
    let span = side - 160.0;
    let along = k as f32 * 6.0;
    let line = (along / span) as usize;
    let into = along % span;
    let x = if line.is_multiple_of(2) { left + into } else { left + span - into };
    let y = 160.0 + (line % 5) as f32 * 260.0 + (along * 0.01).sin() * 90.0;
    PenSample {
        x,
        y,
        pressure: 0.35 + 0.6 * (k as f32 * 0.02).sin().abs(),
        tilt: 0.0,
        orientation: 0.0,
        time_ms: k as f64 * 4.1667,
        roll: 0.0,
    }
}

/// A brush with one of its costs taken away, to tell what that cost: the look changes, so these
/// are for the bench alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Variant {
    /// As it is.
    Base,
    /// A dual brush without its second.
    Single,
    /// Wet mix off (the stroke built up in the stroke's texture, unless it lays in place dry).
    Dry,
    /// No grain, nor its second's.
    NoGrain,
    /// Its stamps (and its second's) twice as far apart.
    Spaced,
    /// Its shapes and grains read from the pictures brushes hold, never the big ones.
    SmallPictures,
    /// No tip shown ahead of a dual brush's or a wet brush's stroke as it is drawn (the stroke's
    /// end laid by its own stamps only once they are kept).
    NoTips,
}

impl Variant {
    fn name(self) -> &'static str {
        match self {
            Variant::Base => "base",
            Variant::Single => "single",
            Variant::Dry => "dry",
            Variant::NoGrain => "no_grain",
            Variant::Spaced => "spaced",
            Variant::SmallPictures => "small_pictures",
            Variant::NoTips => "no_tips",
        }
    }

    fn by_name(name: &str) -> Option<Variant> {
        [Variant::Single, Variant::Dry, Variant::NoGrain, Variant::Spaced, Variant::SmallPictures, Variant::NoTips]
            .into_iter()
            .find(|v| v.name() == name)
    }

    /// Whether it takes anything away from `brush`.
    fn applies(self, brush: &Brush) -> bool {
        let grained = |b: &Brush| b.grain.image.is_some() && b.grain.depth > 0.0;
        match self {
            Variant::Base => true,
            Variant::Single => brush.secondary.is_some(),
            Variant::Dry => brush.mixes_wet(),
            Variant::NoGrain => grained(brush) || brush.secondary.as_deref().is_some_and(grained),
            Variant::Spaced | Variant::SmallPictures => true,
            Variant::NoTips => brush.secondary.is_some() || brush.mixes_wet() || brush.laid_in_place(),
        }
    }

    /// `brush` with it taken away.
    fn apply(self, brush: &mut Brush) {
        match self {
            Variant::Base | Variant::SmallPictures | Variant::NoTips => {}
            Variant::Single => brush.secondary = None,
            Variant::Dry => {
                brush.extra.insert("wet_dilution".to_owned(), 0.0);
            }
            Variant::NoGrain => {
                brush.grain.depth = 0.0;
                if let Some(second) = brush.secondary.as_deref_mut() {
                    second.grain.depth = 0.0;
                }
            }
            Variant::Spaced => {
                brush.spacing *= 2.0;
                if let Some(second) = brush.secondary.as_deref_mut() {
                    second.spacing *= 2.0;
                }
            }
        }
    }
}

/// What a brush is made of, as the audit groups it.
struct Kinds {
    dual: bool,
    dual_mode: f32,
    wet: bool,
    in_place: bool,
    laid_in_place: bool,
    rendering: Rendering,
    second_rendering: Option<Rendering>,
    grain: &'static str,
    second_grain: &'static str,
    spacing: f32,
    second_spacing: f32,
    second_diameter: f32,
    /// The pictures' sides each part reads: its shape's and its grain's (0: none).
    shape_px: u32,
    grain_px: u32,
    second_shape_px: u32,
    second_grain_px: u32,
    /// The biggest pictures each part has (the brush file's, or ours big), whether read or not.
    shape_side: u32,
    grain_side: u32,
}

fn grain_kind(b: &Brush) -> &'static str {
    match (b.grain.image.is_some() && b.grain.depth > 0.0, b.grain.mode) {
        (false, _) => "none",
        (true, GrainMode::Moving) => "moving",
        (true, GrainMode::Texturized) if b.grain_each_stamp() => "texturized_each",
        (true, GrainMode::Texturized) => "texturized",
    }
}

fn shape_side(b: &Brush) -> u32 {
    b.shape_file_now().map(|f| f.full.side).or_else(|| crate::big_pictures::shape_of(b).map(|p| p.side)).unwrap_or(0)
}

fn grain_side(b: &Brush) -> u32 {
    b.grain_file_now()
        .and_then(|f| f.full.as_ref().map(|g| g.side))
        .or_else(|| crate::big_pictures::grain_of(b).map(|p| p.side))
        .unwrap_or(0)
}

/// The side of the picture a style reads its shape or grain from: its own (`full`), else the
/// pictures brushes hold (`held`), or none.
fn read_side(full: Option<u64>, own: u32, held: bool) -> u32 {
    match (full, held) {
        (Some(_), _) => own.max(procraft_engine::textures::TEXTURE_SIZE as u32),
        (None, true) => procraft_engine::textures::TEXTURE_SIZE as u32,
        (None, false) => 0,
    }
}

/// The current paint brush's kinds and the pictures its stroke reads, as a stroke would take it.
fn kinds(s: &CanvasSession) -> Kinds {
    let mut st = s.shared.lock();
    let current = st.doc.current();
    let (brush, style, _) = st.stroke_brush(ToolKind::Paint, current);
    let second = st.second_brush(ToolKind::Paint, current, &brush, &style);
    let (second_brush, second_style, second_diameter) = match &second {
        Some((b, stroke, d)) => (Some(b.clone()), Some(stroke.style), *d),
        None => (None, None, 0.0),
    };
    Kinds {
        dual: second.is_some(),
        dual_mode: second.as_ref().map_or(-1.0, |(_, stroke, _)| stroke.mode as f32),
        wet: style.wet_pull.is_some(),
        in_place: style.in_place(),
        laid_in_place: style.laid_in_place,
        rendering: style.rendering,
        second_rendering: second_style.map(|s| s.rendering),
        grain: grain_kind(&brush),
        second_grain: second_brush.as_ref().map_or("", grain_kind),
        spacing: brush.spacing,
        second_spacing: second_brush.as_ref().map_or(0.0, |b| b.spacing),
        second_diameter,
        shape_px: read_side(style.full_shape, shape_side(&brush), style.shape.is_some()),
        grain_px: if style.grain.is_some() { style.grain_side as u32 } else { 0 },
        second_shape_px: second_style
            .map_or(0, |ss| read_side(ss.full_shape, second_brush.as_ref().map_or(0, shape_side), ss.shape.is_some())),
        second_grain_px: second_style.map_or(0, |ss| if ss.grain.is_some() { ss.grain_side as u32 } else { 0 }),
        shape_side: shape_side(&brush).max(second_brush.as_ref().map_or(0, shape_side)),
        grain_side: grain_side(&brush).max(second_brush.as_ref().map_or(0, grain_side)),
    }
}

/// The size slider that makes `brush` `diameter` canvas pixels across, or its most.
fn slider_for(brush: &Brush, diameter: f32) -> f32 {
    if brush.diameter(1.0) <= diameter {
        return 1.0;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    for _ in 0..40 {
        let mid = 0.5 * (lo + hi);
        if brush.diameter(mid) < diameter { lo = mid } else { hi = mid }
    }
    hi
}

/// One stroke's costs.
#[derive(Clone, Copy, Default)]
struct Run {
    /// Everything from the pen down to the last frame its end needed: input, recording, the GPU.
    total: f64,
    /// Of that, the GPU waited for after each frame, and the frames' recording.
    gpu: f64,
    cpu: f64,
    /// The pen's input (the stroke's samples turned into stamps).
    input: f64,
    /// The stroke's end: its frames and their time (the stroke laid again, where it is).
    lift: f64,
    lift_frames: usize,
    frames: usize,
    /// The slowest frame (its input, recording and GPU), and the median one's.
    worst: f64,
    median: f64,
    /// Stamps queued (and of them the second's), and of them those queued as the stroke ended.
    stamps: u64,
    second_stamps: u64,
    lift_stamps: u64,
    /// Compute dispatches and pieces sent.
    dispatches: u64,
    pieces: u64,
    /// The stroke's own stamps (and its second's) as it ended, before its end taper: what laying
    /// it once would take; the rest of `stamps` are its tips' and what its end laid again.
    kept: u64,
    second_kept: u64,
    /// Of the stamps dual and wet brushes' tips were given, those laid.
    tips_given: u64,
    tips_laid: u64,
}

/// The audit's canvas, window and the GPU's name.
struct Bench {
    s: std::sync::Arc<CanvasSession>,
    gpu: Gpu,
    target: wgpu::TextureView,
    width: u32,
    height: u32,
    started: Instant,
}

/// The GPU the audit runs on: as the platform makes one (`WGPU_BACKEND` choosing), or with
/// `AUDIT_LIMITS=small` held to a budget tablet's limits (an Adreno 610's: 128 MB a storage
/// binding, 32 storage buffers a stage).
fn audit_gpu() -> Option<Gpu> {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return None;
        }
    };
    if env("AUDIT_LIMITS", "") != "small" {
        return Some(gpu);
    }
    let limits = wgpu::Limits {
        max_storage_buffer_binding_size: 128 << 20,
        max_storage_buffers_per_shader_stage: 32,
        max_texture_dimension_2d: 16384,
        ..gpu.adapter.limits()
    };
    drop(gpu);
    Gpu::headless_limited(limits).ok()
}

impl Bench {
    fn new(side: u32) -> Option<Bench> {
        let gpu = audit_gpu()?;
        // a device of little memory, as a budget tablet's (`AUDIT_BUDGET_MB`: what its layers may
        // take): its canvas keeps less to go back to and carries no part from lot to lot
        if let Ok(mb) = env("AUDIT_BUDGET_MB", "").parse::<u64>() {
            crate::layers::set_resident_budget(mb << 20);
        }
        let (width, height) = (2560u32, 1600u32);
        let s = CanvasSession::new(side, side);
        {
            let mut st = s.shared.lock();
            st.window_size = (width, height);
            st.view = Some(View::fit((side, side), (width, height)));
            st.gpu = Some(gpu.clone());
            st.ensure_canvas();
        }
        s.resize_window(width, height);
        s.set_display_scale(2.0);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("window"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        });
        let target = texture.create_view(&Default::default());
        s.shared.lock().view = Some(View::fit((side, side), (width, height)));
        Some(Bench { s, gpu, target, width, height, started: Instant::now() })
    }

    /// A frame: its recording (and sending), then the GPU's work for it waited for; and whether
    /// another is wanted.
    fn frame(&self) -> (f64, f64, bool) {
        let t = Instant::now();
        let more = {
            let mut st = self.s.shared.lock();
            let format = wgpu::TextureFormat::Rgba8Unorm;
            st.draw_frame(
                &self.target,
                format,
                (self.width, self.height),
                procraft_engine::profile::Profile::Srgb,
                self.started,
            );
            st.needs_frame || st.canvas.as_ref().is_some_and(|c| c.lays_over_frames())
        };
        let cpu = ms(t);
        let t = Instant::now();
        self.gpu.flush();
        (cpu, ms(t), more)
    }

    fn counts(&self) -> (u64, u64, u64, u64) {
        let st = self.s.shared.lock();
        st.canvas.as_ref().map_or((0, 0, 0, 0), |c| {
            (c.stamps_queued(), c.second_stamps_queued(), c.compute_dispatches(), c.pieces_sent())
        })
    }

    fn tips(&self) -> (u64, u64) {
        self.s.shared.lock().canvas.as_ref().map_or((0, 0), |c| c.tip_work().1)
    }

    fn set_tips(&self, on: bool) {
        if let Some(c) = self.s.shared.lock().canvas.as_mut() {
            c.set_tips_ahead(on);
        }
    }

    fn pt(&self, k: usize) -> PenSample {
        point(k, self.width as f32, self.height as f32)
    }

    /// Paint over the canvas for wet brushes to mix into.
    fn underpaint(&self) {
        let library = self.s.brush_library();
        let Some((set, index)) = library.iter().enumerate().find_map(|(i, set)| {
            set.brushes.iter().position(|b| b.name == "Soft Airbrush").map(|j| (i as u32, j as u32))
        }) else {
            return;
        };
        self.s.select_brush(set, index);
        self.s.set_brush_size(1.0);
        for k in 0..4 {
            self.s.set_color(k as f32 / 3.0, 0.4, 1.0 - k as f32 / 3.0);
            self.s.stroke_begin(PointerKind::Stylus, vec![self.pt(k * 300)]);
            self.s.stroke_add((1..80).map(|i| self.pt(k * 300 + i * 3)).collect());
            self.s.stroke_end(vec![]);
            self.frame();
        }
    }

    /// The stroke, `frames` frames of two samples each, then its end until no frame is wanted.
    fn stroke(&self, frames: usize) -> Run {
        let (stamps0, second0, dispatches0, pieces0) = self.counts();
        let tips0 = self.tips();
        let mut whole = Vec::with_capacity(frames + 8);
        let (mut run, mut input) = (Run::default(), 0.0);
        let t = Instant::now();
        self.s.stroke_begin(PointerKind::Stylus, vec![self.pt(0)]);
        input += ms(t);
        let (cpu, gpu, _) = self.frame();
        run.cpu += cpu;
        run.gpu += gpu;
        whole.push(input + cpu + gpu);
        for f in 1..=frames {
            let t = Instant::now();
            self.s.stroke_add(vec![self.pt(2 * f - 1), self.pt(2 * f)]);
            let add = ms(t);
            input += add;
            let (cpu, gpu, _) = self.frame();
            run.cpu += cpu;
            run.gpu += gpu;
            whole.push(add + cpu + gpu);
        }
        let before_lift = self.counts().0;
        {
            let st = self.s.shared.lock();
            run.kept = st.stroke.as_ref().map_or(0, |b| b.stamp_count() as u64);
            run.second_kept = st.second.as_ref().map_or(0, |b| b.stamp_count() as u64);
        }
        let t = Instant::now();
        self.s.stroke_end(vec![]);
        let end = ms(t);
        input += end;
        let mut lift = end;
        let mut first = true;
        loop {
            let (cpu, gpu, more) = self.frame();
            run.cpu += cpu;
            run.gpu += gpu;
            lift += cpu + gpu;
            whole.push(cpu + gpu + if first { end } else { 0.0 });
            first = false;
            run.lift_frames += 1;
            if !more || run.lift_frames >= 400 {
                break;
            }
        }
        let (stamps, second, dispatches, pieces) = self.counts();
        run.input = input;
        run.lift = lift;
        run.frames = whole.len();
        run.total = whole.iter().sum();
        run.worst = whole.iter().copied().fold(0.0, f64::max);
        let mut sorted = whole.clone();
        sorted.sort_by(f64::total_cmp);
        run.median = sorted.get(sorted.len() / 2).copied().unwrap_or(0.0);
        run.stamps = stamps - stamps0;
        run.second_stamps = second - second0;
        run.lift_stamps = stamps - before_lift;
        run.dispatches = dispatches - dispatches0;
        run.pieces = pieces - pieces0;
        let tips = self.tips();
        (run.tips_given, run.tips_laid) = (tips.0 - tips0.0, tips.1 - tips0.1);
        run
    }
}

/// Every library brush (or the sets `AUDIT_SETS`, the brushes `AUDIT_BRUSHES` names, `,` between)
/// drawn in a stroke `AUDIT_FRAMES` (120) frames long at `AUDIT_DIAMETER` (100) canvas pixels (or
/// its most, if less; `AUDIT_SLIDER` sets the size slider instead) on an `AUDIT_CANVAS` (2048)
/// square canvas fitted in a 2560 × 1600 window, the GPU waited for after each frame; the best of
/// `AUDIT_ROUNDS` (1) strokes. With `AUDIT_VARIANTS` (`,` between: single, dry, no_grain, spaced,
/// small_pictures, no_tips) each brush again with that taken away, where it has it. A row each to
/// `AUDIT_OUT` (brush-cost.csv).
#[test]
#[ignore]
fn brush_cost_audit() {
    let frames: usize = env("AUDIT_FRAMES", "120").parse().unwrap_or(120);
    let side: u32 = env("AUDIT_CANVAS", "2048").parse().unwrap_or(2048);
    let diameter: f32 = env("AUDIT_DIAMETER", "100").parse().unwrap_or(100.0);
    let slider: Option<f32> = env("AUDIT_SLIDER", "").parse().ok();
    let rounds: usize = env("AUDIT_ROUNDS", "1").parse().unwrap_or(1).max(1);
    let sets = env("AUDIT_SETS", "");
    let names = env("AUDIT_BRUSHES", "");
    let variants: Vec<Variant> = std::iter::once(Variant::Base)
        .chain(env("AUDIT_VARIANTS", "").split(',').filter_map(Variant::by_name))
        .collect();
    // the big pictures kept between runs (drawing them all takes a while)
    let pictures = env("AUDIT_PICTURES", "");
    if !pictures.is_empty() {
        crate::big_pictures::use_folder(std::path::Path::new(&pictures));
    }
    let Some(bench) = Bench::new(side) else { return };
    let info = bench.gpu.adapter.get_info();
    let backend = format!("{:?}", info.backend);
    let limits = env("AUDIT_LIMITS", "full");
    println!(
        "{} ({backend}), limits {limits}, window {}x{}, canvas {side} square, stroke {frames} frames, {}",
        info.name,
        bench.width,
        bench.height,
        slider.map_or_else(|| format!("{diameter} px"), |s| format!("slider {s}"))
    );
    let mut out = std::fs::File::create(env("AUDIT_OUT", "brush-cost.csv")).unwrap();
    writeln!(
        out,
        "backend,limits,canvas,set,brush,variant,diameter,second_diameter,spacing,second_spacing,dual,dual_mode,wet,in_place,laid_in_place,rendering,second_rendering,grain,second_grain,shape_px,grain_px,second_shape_px,second_grain_px,shape_side,grain_side,total_ms,gpu_ms,cpu_ms,input_ms,lift_ms,worst_ms,median_ms,frames,lift_frames,stamps,second_stamps,lift_stamps,dispatches,pieces,kept,second_kept,tips_given,tips_laid,us_per_stamp"
    )
    .unwrap();
    bench.underpaint();
    bench.s.set_color(0.8, 0.2, 0.1);
    let library = bench.s.brush_library();
    for set in &library {
        if !sets.is_empty() && !sets.split(',').any(|n| n == set.name) {
            continue;
        }
        for b in &set.brushes {
            if !names.is_empty() && !names.split(',').any(|n| n == b.name) {
                continue;
            }
            if !bench.s.select_brush(b.set, b.index) {
                continue;
            }
            let original = bench.s.shared.lock().brush(ToolKind::Paint).clone();
            for &variant in &variants {
                if !variant.applies(&original) {
                    continue;
                }
                {
                    let mut st = bench.s.shared.lock();
                    if let Some(brush) = st.brush_mut(ToolKind::Paint) {
                        *brush = original.clone();
                        variant.apply(brush);
                    }
                    st.load_brush_pictures();
                }
                crate::big_pictures::OFF.with(|off| off.set(variant == Variant::SmallPictures));
                crate::big_pictures::wait_for_big_pictures(std::time::Duration::from_secs(120));
                let brush = bench.s.shared.lock().brush(ToolKind::Paint).clone();
                bench.s.set_brush_size(slider.unwrap_or_else(|| slider_for(&brush, diameter)));
                let k = kinds(&bench.s);
                let drawn = brush.diameter(bench.s.shared.lock().sliders(ToolKind::Paint).size);
                let mut best: Option<Run> = None;
                bench.set_tips(variant != Variant::NoTips);
                for _ in 0..rounds {
                    let run = bench.stroke(frames);
                    bench.s.undo();
                    bench.frame();
                    if best.is_none_or(|b| run.total < b.total) {
                        best = Some(run);
                    }
                }
                bench.set_tips(true);
                let r = best.unwrap_or_default();
                let row = format!(
                    "{backend},{limits},{side},{},{},{},{drawn:.1},{:.1},{:.4},{:.4},{},{:.0},{},{},{},{:?},{},{},{},{},{},{},{},{},{},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{:.2},{},{},{},{},{},{},{},{},{},{},{},{:.2}",
                    set.name.replace(',', " "),
                    b.name.replace(',', " "),
                    variant.name(),
                    k.second_diameter,
                    k.spacing,
                    k.second_spacing,
                    u8::from(k.dual),
                    k.dual_mode,
                    u8::from(k.wet),
                    u8::from(k.in_place),
                    u8::from(k.laid_in_place),
                    k.rendering,
                    k.second_rendering.map_or_else(String::new, |r| format!("{r:?}")),
                    k.grain,
                    k.second_grain,
                    k.shape_px,
                    k.grain_px,
                    k.second_shape_px,
                    k.second_grain_px,
                    k.shape_side,
                    k.grain_side,
                    r.total,
                    r.gpu,
                    r.cpu,
                    r.input,
                    r.lift,
                    r.worst,
                    r.median,
                    r.frames,
                    r.lift_frames,
                    r.stamps,
                    r.second_stamps,
                    r.lift_stamps,
                    r.dispatches,
                    r.pieces,
                    r.kept,
                    r.second_kept,
                    r.tips_given,
                    r.tips_laid,
                    r.total * 1000.0 / r.stamps.max(1) as f64,
                );
                writeln!(out, "{row}").unwrap();
                println!("{row}");
            }
            // the brush as it was
            let mut st = bench.s.shared.lock();
            if let Some(brush) = st.brush_mut(ToolKind::Paint) {
                *brush = original;
            }
        }
    }
    crate::big_pictures::OFF.with(|off| off.set(false));
    assert_eq!(bench.gpu.errors(), 0, "GPU validation errors");
}

/// What the CPU spends making a brush's stamps, frame by frame, without a GPU: the audit's stroke
/// (`AUDIT_FRAMES` frames, two pen samples each) at `AUDIT_DIAMETER` canvas pixels on an
/// `AUDIT_CANVAS` square canvas, for the brushes `AUDIT_BRUSHES` names, made as a stroke on the
/// canvas makes them: the stamps the samples add (and a dual brush's second's), then what runs
/// ahead of them each frame (a stroke's tip, or a dual brush's path on to the input). Printed: the
/// milliseconds each took in all, and the stamps each made.
#[test]
#[ignore]
fn brush_tip_cpu() {
    use procraft_engine::input::Pointer;
    use procraft_engine::stroke::StrokeBuilder;
    let frames: usize = env("AUDIT_FRAMES", "120").parse().unwrap_or(120);
    let side: u32 = env("AUDIT_CANVAS", "2048").parse().unwrap_or(2048);
    let diameter: f32 = env("AUDIT_DIAMETER", "100").parse().unwrap_or(100.0);
    let names = env("AUDIT_BRUSHES", "Pierrot");
    let (width, height) = (2560u32, 1600u32);
    let s = CanvasSession::new(side, side);
    {
        let mut st = s.shared.lock();
        st.window_size = (width, height);
        st.view = Some(View::fit((side, side), (width, height)));
    }
    s.set_display_scale(2.0);
    let pt = |k: usize| point(k, width as f32, height as f32);
    println!("brush,kind,add_ms,ahead_ms,clone_ms,added,ahead_stamps,ahead_most");
    for set in &s.brush_library() {
        for b in &set.brushes {
            if !names.split(',').any(|n| n == b.name) || !s.select_brush(b.set, b.index) {
                continue;
            }
            let brush = s.shared.lock().brush(ToolKind::Paint).clone();
            s.set_brush_size(slider_for(&brush, diameter));
            let mut guard = s.shared.lock();
            let st = &mut *guard;
            let current = st.doc.current();
            let (brush, style, d) = st.stroke_brush(ToolKind::Paint, current);
            let second = st.second_brush(ToolKind::Paint, current, &brush, &style);
            let zoom = st.view.map_or(1.0, |v| v.zoom()) / st.display_scale;
            let scale = st.brush_scale();
            let make = |brush: Brush, d: f32, seed: u32| {
                StrokeBuilder::with_seed(brush, d * scale, Pointer::Stylus, seed)
                    .scaled(scale)
                    .with_input(st.input.clone())
                    .seen_at(zoom)
            };
            let mut first = make(brush.clone(), d, 7);
            let mut second = second.map(|(b, _, d2)| make(b, d2, 7 ^ 0x5eed_0002));
            let tipped = second.is_none() && !style.in_place() && first.shows_tip();
            let kind = match (&second, tipped) {
                (Some(_), _) => "dual",
                (None, true) => "tip",
                (None, false) => "plain",
            };
            let (mut add_ms, mut ahead_ms, mut added, mut ahead, mut most) = (0.0, 0.0, 0usize, 0usize, 0usize);
            // (what copying the builders takes, as each look ahead copies them)
            let mut clone_ms = 0.0;
            let start = st.to_canvas(&[pt(0)]);
            let _ = first.add(&start);
            if let Some(second) = second.as_mut() {
                let _ = second.add(&start);
            }
            for f in 1..=frames {
                let samples = st.to_canvas(&[pt(2 * f - 1), pt(2 * f)]);
                let t = Instant::now();
                if let Some(second) = second.as_mut() {
                    added += second.add(&samples).len();
                }
                added += if tipped { first.add_kept(&samples).len() } else { first.add(&samples).len() };
                add_ms += ms(t);
                let t = Instant::now();
                let n = if tipped {
                    first.tip_now().len()
                } else {
                    let mut tip = first.held_back();
                    tip.extend(first.preview_to_input(&[]));
                    tip.len()
                        + second.as_mut().map_or(0, |second| {
                            let mut tip = second.held_back();
                            tip.extend(second.preview_to_input(&[]));
                            tip.len()
                        })
                };
                ahead_ms += ms(t);
                let t = Instant::now();
                let copies = (first.clone(), second.clone());
                clone_ms += ms(t);
                drop(copies);
                ahead += n;
                most = most.max(n);
            }
            println!("{},{kind},{add_ms:.2},{ahead_ms:.2},{clone_ms:.2},{added},{ahead},{most}", b.name);
        }
    }
}

/// A digest of every stamp the library's brushes make for the audit's stroke without a GPU (at
/// the size slider's 0.3 and at `AUDIT_DIAMETER` canvas pixels): the samples' stamps, what runs
/// ahead of them each frame (a tip, or a dual brush's path on to the input, its second's too) and
/// the stroke's end, each stamp's every field. Printed a line a brush, to set beside another
/// build's: the same lines, the same stamps bit for bit.
#[test]
#[ignore]
fn brush_stamp_digests() {
    use procraft_engine::input::Pointer;
    use procraft_engine::stroke::{Stamp, StrokeBuilder};
    let frames: usize = env("AUDIT_FRAMES", "120").parse().unwrap_or(120);
    let side: u32 = env("AUDIT_CANVAS", "2048").parse().unwrap_or(2048);
    let diameter: f32 = env("AUDIT_DIAMETER", "100").parse().unwrap_or(100.0);
    let (width, height) = (2560u32, 1600u32);
    let s = CanvasSession::new(side, side);
    {
        let mut st = s.shared.lock();
        st.window_size = (width, height);
        st.view = Some(View::fit((side, side), (width, height)));
    }
    s.set_display_scale(2.0);
    let pt = |k: usize| point(k, width as f32, height as f32);
    // FNV-1a over each stamp's fields as Debug writes them (every float to its last bit)
    let fold = |hash: &mut u64, stamps: &[Stamp]| {
        for stamp in stamps {
            for byte in format!("{stamp:?}").bytes() {
                *hash = (*hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3);
            }
        }
    };
    let (mut all, mut count) = (0xcbf2_9ce4_8422_2325u64, 0usize);
    for set in &s.brush_library() {
        for b in &set.brushes {
            if !s.select_brush(b.set, b.index) {
                continue;
            }
            let brush = s.shared.lock().brush(ToolKind::Paint).clone();
            for slider in [0.3, slider_for(&brush, diameter)] {
                s.set_brush_size(slider);
                let mut guard = s.shared.lock();
                let st = &mut *guard;
                let current = st.doc.current();
                let (brush, style, d) = st.stroke_brush(ToolKind::Paint, current);
                let second = st.second_brush(ToolKind::Paint, current, &brush, &style);
                let zoom = st.view.map_or(1.0, |v| v.zoom()) / st.display_scale;
                let scale = st.brush_scale();
                let make = |brush: Brush, d: f32, seed: u32| {
                    StrokeBuilder::with_seed(brush, d * scale, Pointer::Stylus, seed)
                        .scaled(scale)
                        .with_input(st.input.clone())
                        .seen_at(zoom)
                };
                let mut first = make(brush.clone(), d, 7);
                let mut second = second.map(|(b, _, d2)| make(b, d2, 7 ^ 0x5eed_0002));
                let tipped = second.is_none() && !style.in_place() && first.shows_tip();
                let mut hash = 0xcbf2_9ce4_8422_2325u64;
                let mut n = 0usize;
                let start = st.to_canvas(&[pt(0)]);
                let mut made = first.add(&start);
                if let Some(second) = second.as_mut() {
                    made.extend(second.add(&start));
                }
                for f in 1..=frames {
                    let samples = st.to_canvas(&[pt(2 * f - 1), pt(2 * f)]);
                    if let Some(second) = second.as_mut() {
                        made.extend(second.add(&samples));
                    }
                    made.extend(if tipped { first.add_kept(&samples) } else { first.add(&samples) });
                    if tipped {
                        made.extend(first.tip_now());
                    } else {
                        made.extend(first.held_back());
                        made.extend(first.preview_to_input(&[]));
                        if let Some(second) = second.as_mut() {
                            made.extend(second.held_back());
                            made.extend(second.preview_to_input(&[]));
                        }
                    }
                    fold(&mut hash, &made);
                    n += made.len();
                    made.clear();
                }
                made.extend(first.finish());
                if let Some(second) = second.as_mut() {
                    made.extend(second.finish());
                }
                fold(&mut hash, &made);
                n += made.len();
                println!("digest,{},{},{slider:.4},{n},{hash:016x}", set.name, b.name);
                all ^= hash.rotate_left((count % 64) as u32);
                count += 1;
            }
        }
    }
    println!("digest of all {count} strokes: {all:016x}");
}

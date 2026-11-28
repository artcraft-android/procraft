//! Artworks on GPUs that offer less than most: an old Mali's limits (#412).

use procraft_engine::brush::{Brush, Rendering};
use procraft_gpu::{Gpu, wgpu};

use crate::tests::sample;
use crate::{CanvasSession, PenSample, PointerKind, ToolKind};

/// What a Mali-G72 (Bifrost, the Galaxy Tab S6 Lite's Exynos 9611) gives wgpu on Vulkan 1.1, at the
/// least: textures of 8192 pixels a side and buffers of 1 GB, as a Bifrost Mali-G52's driver of the
/// same years told a user's log (which gave 35 storage buffers a stage, workgroups of 384 and 32 KB
/// shared besides); held lower where old Mali drivers may report less: 8 storage buffers a stage,
/// 4 colour targets, offsets of 256 bytes, and the rest at wgpu's downlevel least (16 textures
/// and samplers a stage, 4 bind groups, 4 storage textures, 128 MB a storage binding, workgroups
/// of 256 with 16 KB shared).
fn mali_g72_limits() -> wgpu::Limits {
    wgpu::Limits {
        max_texture_dimension_1d: 8192,
        max_texture_dimension_2d: 8192,
        max_buffer_size: 1 << 30,
        max_storage_buffers_per_shader_stage: 8,
        max_color_attachments: 4,
        min_uniform_buffer_offset_alignment: 256,
        min_storage_buffer_offset_alignment: 256,
        ..wgpu::Limits::downlevel_defaults()
    }
}

/// Which brushes of the library are of a kind.
type Wanted = Box<dyn Fn(&Brush) -> bool>;

/// The first brush of the library `wanted` takes, by its set and place.
fn library_brush(s: &CanvasSession, wanted: impl Fn(&Brush) -> bool) -> Option<(u32, u32)> {
    let st = s.shared.lock();
    st.library.iter().enumerate().find_map(|(set, entry)| {
        let at = entry.brushes.iter().position(&wanted)?;
        Some((set as u32, at as u32))
    })
}

/// A stroke across the canvas `width` × `height`, a band down at `y` (0..1).
fn stroke(s: &CanvasSession, width: u32, height: u32, y: f32) {
    let at = |k: usize| PenSample {
        pressure: 0.3 + 0.6 * (k as f32 * 0.2).sin().abs(),
        ..sample(
            width as f32 * (0.1 + 0.02 * k as f32),
            height as f32 * (y + 0.03 * (k as f32 * 0.3).sin()),
            k as f64 * 8.0,
        )
    };
    s.stroke_begin(PointerKind::Stylus, vec![at(0)]);
    for k in 1..40 {
        s.stroke_add(vec![at(k)]);
    }
    s.stroke_end(vec![]);
    render(s);
}

/// Draws what is queued, and waits for it.
fn render(s: &CanvasSession) {
    let mut guard = s.shared.lock();
    let st = &mut *guard;
    if let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) {
        canvas.render_finished(gpu, &mut st.doc);
        let _ = gpu.wait();
    }
}

/// Every layer's pixels, as the canvas has them.
fn contents(s: &CanvasSession) -> Vec<Vec<u8>> {
    render(s);
    s.layers().into_iter().map(|l| s.copy_layer_contents(l.id).map(|c| c.rgba).unwrap_or_default()).collect()
}

/// #412: on a GPU held to a Mali-G72's limits, the canvases' pipelines are made without an error
/// and the compute passes run, as there; and artworks the size of the tablet's screen, of an A4
/// page at 300 dpi and of 4096 pixels square are made, painted on two layers with a dry, a wet, a
/// dual brush built up by its rule, a brush laid in place and a smudge, saved, and opened again
/// (their canvas made as an opened artwork's is) as they were, then painted on: with no GPU error
/// anywhere. The time each took is printed.
#[test]
fn an_artwork_opens_and_paints_on_a_gpu_held_to_an_old_mali_s_limits() {
    let gpu = match Gpu::headless_limited(mali_g72_limits()) {
        Ok(gpu) => gpu,
        Err(e) => return eprintln!("skipped: {e}"),
    };
    let l = gpu.device.limits();
    assert!(l.max_texture_dimension_2d <= 8192 && l.max_storage_buffers_per_shader_stage <= 8);
    assert!(l.max_color_attachments <= 4 && l.max_sampled_textures_per_shader_stage <= 16);
    let started = std::time::Instant::now();
    gpu.prepare_canvases();
    assert_eq!(gpu.errors(), 0, "the canvases' pipelines");
    assert!(gpu.computes(), "the compute passes run, as on the Mali");
    eprintln!("the pipelines made in {} ms", started.elapsed().as_millis());
    let by_rule = |b: &Brush| matches!(b.rendering, Rendering::Gradual | Rendering::Capped);
    let wanted: [(&str, Wanted); 4] = [
        ("dry", Box::new(|b: &Brush| b.secondary.is_none() && !b.mixes_wet() && !b.laid_in_place())),
        ("wet", Box::new(|b: &Brush| b.mixes_wet())),
        (
            "dual by its rule",
            Box::new(move |b: &Brush| {
                !b.mixes_wet() && !b.laid_in_place() && b.secondary.as_deref().is_some_and(|s| by_rule(s) || by_rule(b))
            }),
        ),
        ("laid in place", Box::new(|b: &Brush| b.laid_in_place() && !b.mixes_wet())),
    ];
    for (w, h) in [(2000, 1200), (2480, 3508), (4096, 4096)] {
        let started = std::time::Instant::now();
        let dir = std::env::temp_dir().join(format!("procraft-mali-g72-{w}x{h}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.to_string_lossy().into_owned();
        let s = CanvasSession::new(w, h);
        s.use_gpu(gpu.clone());
        s.show_in(w, h);
        assert!(s.shared.lock().canvas.is_some(), "{w}×{h}: a new canvas");
        s.add_layer();
        for (k, (kind, wanted)) in wanted.iter().enumerate() {
            let Some((set, index)) = library_brush(&s, wanted) else { panic!("no {kind} brush") };
            assert!(s.select_brush(set, index));
            s.set_brush_size(0.3);
            s.set_color(0.2, 0.4, 0.8);
            stroke(&s, w, h, 0.15 + 0.15 * k as f32);
            assert_eq!(gpu.errors(), 0, "{w}×{h}: a {kind} stroke");
        }
        s.set_tool(ToolKind::Smudge);
        stroke(&s, w, h, 0.4);
        assert_eq!(gpu.errors(), 0, "{w}×{h}: a smudge");
        s.save(path.clone()).unwrap();
        let painted = contents(&s);
        assert!(painted.iter().any(|px| px.iter().any(|&v| v != 0)), "{w}×{h}: painted");
        drop(s);

        let opened = CanvasSession::open(path).unwrap();
        opened.use_gpu(gpu.clone());
        opened.show_in(w, h);
        {
            let st = opened.shared.lock();
            assert!(st.canvas.is_some(), "{w}×{h}: opened, its canvas made: {:?}", st.error);
            assert!(st.error.is_none());
        }
        assert_eq!(contents(&opened), painted, "{w}×{h}: opened as saved");
        opened.set_tool(ToolKind::Paint);
        stroke(&opened, w, h, 0.8);
        assert_eq!(gpu.errors(), 0, "{w}×{h}: opened and painted on");
        drop(opened);
        let _ = std::fs::remove_dir_all(&dir);
        eprintln!("{w}×{h}: made, painted, saved and opened again in {} ms", started.elapsed().as_millis());
    }
}

/// A window-sized picture for frames to be drawn into, as a window's are.
fn window(gpu: &Gpu, (width, height): (u32, u32)) -> wgpu::TextureView {
    gpu.device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some("window"),
            size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            view_formats: &[],
        })
        .create_view(&Default::default())
}

/// #442, #427: a layer's opacity dragged as the apps drive it, on a GPU held to a Mali-G72's
/// limits, the Layers panel's glass over the canvas: its slider (each change shown at once and
/// refreshing the app, which asks for a time-lapse frame; a frame drawn after each), on the layer
/// worked on and on one below it, over layers that all fit on the GPU and over more than fit (most
/// kept off it, the composite made around pictures of the rest), then Layer Opacity's drag across
/// the canvas. No GPU error and the device never lost; never more than one time-lapse picture
/// waiting to be kept, none taken while the slider moves (each a copy coming back from the GPU in a
/// buffer of its own: taken a frame each, faster than they were kept, hundreds piled up until a
/// tablet's GPU had no memory left and lost its device); and the composite's caches kept through
/// each drag, so its frames composite
/// only the layers worked on and the dragged one (let go at each change, over layers kept off the
/// GPU their remake started again each frame and never finished: the drag showed nothing till it
/// was let go). The time each drag took is printed.
#[test]
fn a_layer_s_opacity_dragged_on_an_old_mali_keeps_its_device_and_its_memory() {
    let gpu = match Gpu::headless_limited(mali_g72_limits()) {
        Ok(gpu) => gpu,
        Err(e) => return eprintln!("skipped: {e}"),
    };
    let (w, h) = (1536, 1152);
    let size = (1200, 900);
    let view = window(&gpu, size);
    let started = std::time::Instant::now();
    let draw = |s: &CanvasSession| {
        let format = wgpu::TextureFormat::Rgba8Unorm;
        s.draw(&view, format, size, procraft_engine::profile::Profile::Srgb, started);
    };
    let dir = std::env::temp_dir().join(format!("procraft-opacity-lapse-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let lapse = crate::timelapse::TimeLapse::new(dir.to_string_lossy().into_owned());
    // the layers' textures all on the GPU, or room for three of the six
    for fit in [64, 3] {
        let s = CanvasSession::new(w, h);
        s.use_gpu(gpu.clone());
        s.show_in(size.0, size.1);
        {
            let mut st = s.shared.lock();
            let per = u64::from(w) * u64::from(h) * 4;
            st.canvas.as_mut().unwrap().set_resident_budget(per * fit);
        }
        for k in 0..6 {
            if k > 0 {
                s.add_layer();
            }
            stroke(&s, w, h, 0.1 + 0.13 * k as f32);
        }
        // the Layers panel's glass, at the right
        s.set_glass(vec![780.0, 40.0, 1180.0, 860.0, 16.0, 1.0], Vec::new());
        draw(&s);
        let ids: Vec<u64> = s.layers().iter().map(|l| l.id).collect();
        // the layer worked on (the top one), then one below it
        for (dragged, which) in [(ids[0], "the current layer"), (ids[3], "a layer below it")] {
            let t = std::time::Instant::now();
            let mut most_waiting = 0;
            for k in 0..=60 {
                assert!(s.set_layer_opacity(dragged, 1.0 - k as f32 / 60.0, false));
                s.record_time_lapse(std::sync::Arc::clone(&lapse));
                draw(&s);
                most_waiting = most_waiting.max(s.shared.lock().lapse_keeper.waiting());
            }
            let (busy, cached) = {
                let st = s.shared.lock();
                let canvas = st.canvas.as_ref().unwrap();
                (canvas.busy(), canvas.composites_cached())
            };
            assert!(s.set_layer_opacity(dragged, 0.4, true));
            draw(&s);
            eprintln!("fit {fit}, {which}: dragged in {} ms", t.elapsed().as_millis());
            assert_eq!(gpu.errors(), 0, "fit {fit}, {which}");
            assert!(!gpu.is_lost(), "fit {fit}, {which}");
            assert!(most_waiting <= 1, "fit {fit}, {which}: {most_waiting} time-lapse pictures waiting");
            assert!(cached && !busy, "fit {fit}, {which}: caches kept through the drag ({cached}, busy {busy})");
            let opacity = s.layers().iter().find(|l| l.id == dragged).map(|l| l.opacity);
            assert_eq!(opacity, Some(0.4));
        }
        // Layer Opacity: a drag across the canvas
        s.set_touch_layer_opacity(Some(ids[0]));
        let at = |x: f32, t: f64| vec![PenSample { pressure: 0.5, ..sample(x, 450.0, t) }];
        s.touch_down(1, PointerKind::Finger, at(900.0, 0.0));
        for k in 1..=40 {
            s.touch_move(1, at(900.0 - k as f32 * 15.0, f64::from(k) * 8.0));
            draw(&s);
        }
        s.touch_up(1, at(300.0, 400.0));
        s.set_touch_layer_opacity(None);
        draw(&s);
        let opacity = s.layers().first().map(|l| l.opacity).unwrap_or(1.0);
        assert!(opacity < 0.4, "fit {fit}: the drag lowered it to {opacity}");
        assert_eq!(gpu.errors(), 0, "fit {fit}: Layer Opacity");
        assert!(!gpu.is_lost(), "fit {fit}: Layer Opacity");
        let keeper = s.shared.lock().lapse_keeper.clone();
        keeper.settle(crate::timelapse::KEEP_MOST);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

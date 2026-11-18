//! A big artwork of many layers (A4 at 300 dpi, 39 layers) opens and exports within the budgets
//! the app sets on a device of 4 GB: what the heap and the GPU hold at their fullest, while the
//! artwork is opened and shown and while it is exported in each format, from the open canvas and
//! from the gallery (opened only to be exported). The files are the same as those written with
//! every layer on the GPU. (A test of its own, as it counts every allocation the program makes.)
//! `BIG_ARTWORK_DIR=folder` keeps the artwork there, to be used again.

// (the helpers too are the test's)
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Duration, Instant};

use procraft_core::CanvasSession;
use procraft_engine::document::Document;
use procraft_gpu::{Gpu, wgpu};

#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

/// The artwork: A4 at 300 dpi, of 39 layers.
const WIDTH: u32 = 2480;
const HEIGHT: u32 = 3508;
const LAYERS: usize = 39;

/// A device of 4 GB: the budgets the app sets for it (half its memory for layers, a quarter for
/// their textures).
const RAM: u64 = 4 << 30;

/// The window the canvas is shown in (the tablet's screen).
const SCREEN: (u32, u32) = (1200, 2000);

/// The GPU's memory at its fullest (what its allocator holds), sampled while something is
/// measured.
static GPU_PEAK: AtomicU64 = AtomicU64::new(0);
/// The heap and the GPU together at their fullest, as sampled.
static BOTH_PEAK: AtomicU64 = AtomicU64::new(0);

const MB: f64 = (1u64 << 20) as f64;

/// One layer's pixels, bytes.
const LAYER_BYTES: u64 = WIDTH as u64 * HEIGHT as u64 * 4;

/// The most opening and showing the artwork may hold beyond the textures' budget, in layers'
/// worth: the layers kept compressed off the GPU (what of them doesn't pack), the few being read
/// and the buffers they go up through. (All of them at once, as before, came to 26 more.)
const OPENING_MORE: u64 = 18;

/// The most an export may hold, in layers' worth: the picture, the layer being written, what is
/// made of it and its copy on its way back from the GPU. (Every layer at once came to 141.)
const EXPORTING: u64 = 6;

/// What one step held at its fullest beyond what was held before it.
#[derive(Clone, Copy, Debug)]
struct Peak {
    heap: u64,
    gpu: u64,
    both: u64,
}

impl Peak {
    fn total(&self) -> u64 {
        self.both.max(self.heap.max(self.gpu))
    }
}

/// Samples the GPU's allocator every 2 ms, and the heap with it, until `stop`.
fn sampler(gpu: Gpu, stop: Arc<AtomicBool>) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        while !stop.load(Ordering::Relaxed) {
            let now = gpu.device.generate_allocator_report().map_or(0, |r| r.total_reserved_bytes);
            GPU_PEAK.fetch_max(now, Ordering::Relaxed);
            let heap = dhat::HeapStats::get().curr_bytes as u64;
            BOTH_PEAK.fetch_max(heap + now, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(2));
        }
    })
}

/// Runs `step` and returns what it held at its fullest beyond what was held when it began (the
/// heap's own count, and the GPU's as sampled).
fn measure<T>(gpu: &Gpu, step: impl FnOnce() -> T) -> (T, Peak) {
    let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
    let gpu_before = gpu.device.generate_allocator_report().map_or(0, |r| r.total_reserved_bytes);
    GPU_PEAK.store(gpu_before, Ordering::Relaxed);
    BOTH_PEAK.store(gpu_before, Ordering::Relaxed);
    // the heap counted from here: the profiler counts what is allocated after its start
    let profiler = dhat::Profiler::builder().testing().build();
    let stop = Arc::new(AtomicBool::new(false));
    let thread = sampler(gpu.clone(), stop.clone());
    let out = step();
    stop.store(true, Ordering::Relaxed);
    let _ = thread.join();
    let heap_peak = dhat::HeapStats::get().max_bytes as u64;
    drop(profiler);
    let gpu_peak = GPU_PEAK.load(Ordering::Relaxed).saturating_sub(gpu_before);
    let both = BOTH_PEAK.load(Ordering::Relaxed).saturating_sub(gpu_before);
    (out, Peak { heap: heap_peak, gpu: gpu_peak, both })
}

/// A layer's pixels, premultiplied: soft blobs of colour across the whole canvas with a little
/// grain, as painted layers are (each a different picture).
fn layer_pixels(seed: u64) -> Vec<u8> {
    let (w, h) = (WIDTH as usize, HEIGHT as usize);
    let mut px = vec![0u8; w * h * 4];
    let mut x = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x
    };
    // the first layer is painted all over; the others in blobs spread over the canvas
    let blobs = if seed == 0 { 1 } else { 5 };
    for b in 0..blobs {
        let (cx, cy, rx, ry) = if seed == 0 {
            (w as f32 / 2.0, h as f32 / 2.0, w as f32, h as f32)
        } else {
            let r = next();
            let cx = (r % w as u64) as f32;
            let cy = ((r >> 20) % h as u64) as f32;
            let rx = 150.0 + ((r >> 40) % 700) as f32;
            let ry = 150.0 + ((r >> 50) % 900) as f32;
            (cx, cy, rx, ry)
        };
        let c = next();
        let colour = [(c & 0xFF) as f32, ((c >> 8) & 0xFF) as f32, ((c >> 16) & 0xFF) as f32];
        let alpha = if seed == 0 { 255.0 } else { 120.0 + ((c >> 24) % 136) as f32 };
        let y0 = (cy - ry).max(0.0) as usize;
        let y1 = ((cy + ry) as usize).min(h);
        let x0 = (cx - rx).max(0.0) as usize;
        let x1 = ((cx + rx) as usize).min(w);
        for y in y0..y1 {
            let dy = (y as f32 - cy) / ry;
            for xx in x0..x1 {
                let dx = (xx as f32 - cx) / rx;
                let d = dx * dx + dy * dy;
                if d >= 1.0 {
                    continue;
                }
                // soft at the edge, and a little grain
                let grain = (next() & 0x1F) as f32 - 16.0;
                let a = (alpha * (1.0 - d * d) + grain * 0.5).clamp(0.0, 255.0) / 255.0;
                let shade = 1.0 - 0.3 * (xx as f32 / w as f32) + (b as f32) * 0.01;
                let i = (y * w + xx) * 4;
                let p = &mut px[i..i + 4];
                // over what is there
                let keep = 1.0 - a;
                for k in 0..3 {
                    let v = (colour[k] * shade).clamp(0.0, 255.0) * a + f32::from(p[k]) * keep;
                    p[k] = v.round().clamp(0.0, 255.0) as u8;
                }
                p[3] = (a * 255.0 + f32::from(p[3]) * keep).round().clamp(0.0, 255.0) as u8;
                for k in 0..3 {
                    p[k] = p[k].min(p[3]);
                }
            }
        }
    }
    px
}

/// The PNG an artwork's folder keeps a layer in (its pixels as they are).
fn png(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, WIDTH, HEIGHT);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fast);
    let mut writer = encoder.write_header().unwrap();
    writer.write_image_data(rgba).unwrap();
    writer.finish().unwrap();
    out
}

/// Writes the artwork's folder into `dir`: its file and a PNG for each layer.
fn write_artwork(dir: &Path) {
    let mut doc = Document::new(WIDTH, HEIGHT);
    let mut ids = vec![doc.current()];
    for _ in 1..LAYERS {
        ids.push(doc.add_layer());
    }
    std::fs::create_dir_all(dir.join("layers")).unwrap();
    let file = serde_json::json!({
        "format": "procraft-artwork",
        "version": 1,
        "name": "Big",
        "document": doc,
    });
    std::fs::write(dir.join("artwork.json"), serde_json::to_vec(&file).unwrap()).unwrap();
    // a few at a time
    let next = std::sync::atomic::AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..6 {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(id) = ids.get(i) else { break };
                    let bytes = png(&layer_pixels(i as u64));
                    std::fs::write(dir.join("layers").join(format!("{}.png", id.0)), bytes).unwrap();
                }
            });
        }
    });
}

/// Shows the session as the screen would, a frame at a time, until it wants no more.
fn show(s: &CanvasSession, gpu: &Gpu) {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screen"),
        size: wgpu::Extent3d { width: SCREEN.0, height: SCREEN.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    s.show_in(SCREEN.0, SCREEN.1);
    for _ in 0..20_000 {
        let again = s.draw(
            &view,
            wgpu::TextureFormat::Rgba8Unorm,
            SCREEN,
            procraft_engine::profile::Profile::Srgb,
            Instant::now(),
        );
        let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
        if again.is_none_or(|d| d > Duration::from_millis(100)) && !s.wants_frame() {
            return;
        }
    }
}

/// The formats exported, by name, each written by the session into a file in `dir`.
const FORMATS: [&str; 6] = ["PNG", "JPEG", "PSD", "PDF", "TIFF", "package"];

/// Those exported from the gallery too: the flattened picture and the layered files (what the
/// others hold more is told from the canvas).
const GALLERY_FORMATS: [&str; 3] = ["PNG", "PSD", "package"];

fn export(s: &CanvasSession, format: &str, dir: &Path) -> PathBuf {
    let path = dir.join(format!("Big.{format}"));
    let p = path.to_string_lossy().into_owned();
    match format {
        "PNG" => s.export_png(p),
        "JPEG" => s.export_jpeg(p),
        "PSD" => s.export_psd(p),
        "PDF" => s.export_pdf(p, false, 100),
        "TIFF" => s.export_tiff(p),
        _ => s.export_package(p, 7, None),
    }
    .unwrap_or_else(|e| panic!("{format}: {e}"));
    path
}

fn mb(b: u64) -> f64 {
    b as f64 / MB
}

fn line(what: &str, p: Peak) {
    eprintln!(
        "{what:<26} heap {:7.0} MB | GPU {:7.0} MB | together {:7.0} MB ({:4.1} layers)",
        mb(p.heap),
        mb(p.gpu),
        mb(p.both),
        p.total() as f64 / LAYER_BYTES as f64
    );
}

#[test]
fn a_big_artwork_opens_and_exports_within_its_budget() {
    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("skipped: {e}");
            return;
        }
    };
    if gpu.device.generate_allocator_report().is_none() {
        eprintln!("skipped: the GPU's memory can't be told");
        return;
    }
    // offscreen exports use this device too
    procraft_core::keep_device(&gpu);
    let root = match std::env::var("BIG_ARTWORK_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => std::env::temp_dir().join(format!("procraft-big-artwork-{}", std::process::id())),
    };
    let art = root.join("Big");
    if !art.join("artwork.json").exists() {
        let t = Instant::now();
        write_artwork(&art);
        eprintln!("artwork written in {:.1} s", t.elapsed().as_secs_f64());
    }
    let out = root.join("out");
    let _ = std::fs::remove_dir_all(&out);
    std::fs::create_dir_all(&out).unwrap();
    let art_path = art.to_string_lossy().into_owned();

    procraft_core::set_layer_budget(RAM / 2);
    procraft_core::set_resident_budget(RAM / 4);
    eprintln!("budgets: layers {:.0} MB, their textures {:.0} MB", mb(RAM / 2), mb(RAM / 4));
    let mut peaks = Vec::new();

    // opened and shown, as tapping it in the gallery does
    let t = Instant::now();
    let (s, open) = measure(&gpu, || {
        let s = CanvasSession::open(art_path.clone()).unwrap();
        s.use_gpu(gpu.clone());
        assert!(s.canvas_made());
        show(&s, &gpu);
        s
    });
    line(&format!("opened ({:.1} s)", t.elapsed().as_secs_f64()), open);
    peaks.push(("opened".to_owned(), open));
    let mut shown_files = Vec::new();
    for format in FORMATS {
        let t = Instant::now();
        let (path, p) = measure(&gpu, || export(&s, format, &out));
        line(&format!("{format} ({:.1} s)", t.elapsed().as_secs_f64()), p);
        peaks.push((format!("{format} from the canvas"), p));
        shown_files.push(std::fs::read(&path).unwrap());
        let _ = std::fs::remove_file(&path);
    }
    drop(s);

    // from the gallery: opened only to be exported
    let mut gallery_files = Vec::new();
    for format in GALLERY_FORMATS {
        let t = Instant::now();
        let (path, p) = measure(&gpu, || {
            let s = CanvasSession::open(art_path.clone()).unwrap();
            assert!(s.prepare_offscreen());
            export(&s, format, &out)
        });
        line(&format!("gallery {format} ({:.1} s)", t.elapsed().as_secs_f64()), p);
        peaks.push((format!("{format} from the gallery"), p));
        gallery_files.push(std::fs::read(&path).unwrap());
        let _ = std::fs::remove_file(&path);
    }

    // with no budget (every layer on the GPU), for the files as they should be
    procraft_core::set_resident_budget(0);
    let s = CanvasSession::open(art_path.clone()).unwrap();
    s.use_gpu(gpu.clone());
    show(&s, &gpu);
    for (i, format) in FORMATS.iter().enumerate() {
        let path = export(&s, format, &out);
        let whole = std::fs::read(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert!(shown_files[i] == whole, "{format} from the canvas differs from the file with every layer on the GPU");
        if let Some(g) = GALLERY_FORMATS.iter().position(|f| f == format) {
            assert!(
                gallery_files[g] == whole,
                "{format} from the gallery differs from the file with every layer on the GPU"
            );
        }
    }
    drop(s);
    procraft_core::set_resident_budget(RAM / 4);
    if std::env::var_os("BIG_ARTWORK_DIR").is_none() {
        let _ = std::fs::remove_dir_all(&root);
    }
    let errors = gpu.errors();
    assert_eq!(errors, 0, "GPU validation errors");
    // opening keeps to the textures' budget and a few layers more; an export holds a few layers
    // whatever the artwork's number of them; from the gallery, the two together
    let opening = RAM / 4 + OPENING_MORE * LAYER_BYTES;
    for (what, p) in &peaks {
        let most = match what.as_str() {
            "opened" => opening,
            w if w.ends_with("from the gallery") => opening + EXPORTING * LAYER_BYTES,
            _ => EXPORTING * LAYER_BYTES,
        };
        assert!(p.total() <= most, "{what}: {:.0} MB at the fullest, past {:.0} MB", mb(p.total()), mb(most));
    }
}

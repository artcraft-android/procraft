//! How long a layer kept compressed off the GPU (parked) takes to come back, as on a tablet whose
//! textures' budget holds fewer layers than the artwork has: a 4096-square artwork of three
//! softly painted layers opened with room on the GPU for one, each layer chosen in turn and the
//! canvas drawn until it wants no more frames (the layers its caches are remade from brought
//! back, and the one let go compressed if it has no copy yet), then each parked layer's pixels
//! read as copying it does, and checked against what was painted. Each step's time is printed,
//! with what the canvas's log says it spent bringing layers back. A bench, run by hand in a
//! release build:
//! `cargo test --release -p procraft-core --test parked_layer_back -- --ignored --nocapture`.

// (the helpers too are the test's)
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use procraft_core::CanvasSession;
use procraft_engine::document::Document;
use procraft_gpu::{Gpu, wgpu};

const SIDE: u32 = 4096;
const LAYERS: usize = 3;
const SCREEN: (u32, u32) = (2560, 1600);
const TILE: usize = 256;

/// The log's lines, kept to be read: the canvas tells how long it spent bringing layers back.
struct Kept(Mutex<Vec<String>>);

impl log::Log for Kept {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info
    }

    fn log(&self, r: &log::Record) {
        if self.enabled(r.metadata()) {
            self.0.lock().unwrap().push(r.args().to_string());
        }
    }

    fn flush(&self) {}
}

static KEPT: Kept = Kept(Mutex::new(Vec::new()));

/// The log's lines since last asked.
fn logged() -> Vec<String> {
    std::mem::take(&mut *KEPT.0.lock().unwrap())
}

struct Rng(u64);

impl Rng {
    fn f(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
}

/// A softly painted tile, `TILE` square: an opaque gradient under a dozen airbrushed blobs.
fn soft_tile(seed: u64) -> Vec<u8> {
    let mut r = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let mut t = vec![0u8; TILE * TILE * 4];
    let put = |t: &mut [u8], x: usize, y: usize, rgb: [f32; 3], a: f32| {
        let i = (y * TILE + x) * 4;
        let a = a.clamp(0.0, 1.0);
        for c in 0..3 {
            let d = f32::from(t[i + c]) / 255.0;
            t[i + c] = ((rgb[c] * a + d * (1.0 - a)) * 255.0 + 0.5) as u8;
        }
        let da = f32::from(t[i + 3]) / 255.0;
        t[i + 3] = ((a + da * (1.0 - a)) * 255.0 + 0.5) as u8;
    };
    for y in 0..TILE {
        for x in 0..TILE {
            let (u, v) = (x as f32 / 255.0, y as f32 / 255.0);
            put(&mut t, x, y, [0.55 + 0.3 * u, 0.4 + 0.2 * v, 0.35 + 0.1 * (u - v)], 1.0);
        }
    }
    for _ in 0..12 {
        let (cx, cy, rad) = (r.f() * 256.0, r.f() * 256.0, 20.0 + r.f() * 90.0);
        let col = [r.f(), r.f(), r.f()];
        let op = 0.2 + r.f() * 0.4;
        for y in 0..TILE {
            for x in 0..TILE {
                let d = ((x as f32 - cx).powi(2) + (y as f32 - cy).powi(2)).sqrt() / rad;
                if d < 1.0 {
                    put(&mut t, x, y, col, op * (1.0 - d * d).powi(2));
                }
            }
        }
    }
    t
}

/// A layer's pixels (opaque, so premultiplied as they are): tiles of soft paint, each its own.
fn layer_pixels(seed: u64) -> Vec<u8> {
    let (side, tiles) = (SIDE as usize, SIDE as usize / TILE);
    let mut out = vec![0u8; side * side * 4];
    for ty in 0..tiles {
        for tx in 0..tiles {
            let t = soft_tile(seed * 1000 + (ty * tiles + tx) as u64);
            for r in 0..TILE {
                let at = ((ty * TILE + r) * side + tx * TILE) * 4;
                out[at..at + TILE * 4].copy_from_slice(&t[r * TILE * 4..(r + 1) * TILE * 4]);
            }
        }
    }
    out
}

/// Writes the artwork's folder: its file and a PNG for each layer; the layers' pixels.
fn write_artwork(dir: &Path) -> Vec<Vec<u8>> {
    let mut doc = Document::new(SIDE, SIDE);
    let mut ids = vec![doc.current()];
    for _ in 1..LAYERS {
        ids.push(doc.add_layer());
    }
    std::fs::create_dir_all(dir.join("layers")).unwrap();
    let file = serde_json::json!({ "format": "procraft-artwork", "version": 1, "name": "Parked", "document": doc });
    std::fs::write(dir.join("artwork.json"), serde_json::to_vec(&file).unwrap()).unwrap();
    let mut layers = Vec::new();
    for (i, id) in ids.iter().enumerate() {
        let px = layer_pixels(i as u64 + 1);
        let mut bytes = Vec::new();
        let mut encoder = png::Encoder::new(&mut bytes, SIDE, SIDE);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&px).unwrap();
        writer.finish().unwrap();
        std::fs::write(dir.join("layers").join(format!("{}.png", id.0)), bytes).unwrap();
        layers.push(px);
    }
    layers
}

/// Draws the session as the screen would, a frame at a time, until it wants no more.
fn show(s: &CanvasSession, gpu: &Gpu, view: &wgpu::TextureView) {
    for _ in 0..20_000 {
        let again = s.draw(
            view,
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

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

#[test]
#[ignore]
fn a_parked_layer_comes_back() {
    if cfg!(debug_assertions) {
        eprintln!("(a debug build: its times say little)");
    }
    let _ = log::set_logger(&KEPT);
    log::set_max_level(log::LevelFilter::Info);
    let gpu = match Gpu::headless() {
        Ok(gpu) => gpu,
        Err(e) => {
            eprintln!("skipped: {e}");
            return;
        }
    };
    procraft_core::keep_device(&gpu);
    // room for the canvas's own textures (seven layers' worth) and one layer
    let per = u64::from(SIDE) * u64::from(SIDE) * 4;
    procraft_core::set_layer_budget(8 << 30);
    procraft_core::set_resident_budget(8 * per);
    let dir = std::env::temp_dir().join(format!("procraft-parked-layer-back-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let painted = write_artwork(&dir);

    let t = Instant::now();
    let s = CanvasSession::open(dir.to_string_lossy().into_owned()).unwrap();
    s.use_gpu(gpu.clone());
    assert!(s.canvas_made());
    let screen = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("screen"),
        size: wgpu::Extent3d { width: SCREEN.0, height: SCREEN.1, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = screen.create_view(&wgpu::TextureViewDescriptor::default());
    s.show_in(SCREEN.0, SCREEN.1);
    show(&s, &gpu, &view);
    let opened = logged();
    let made = opened.iter().find(|l| l.starts_with("canvas made")).cloned().unwrap_or_default();
    eprintln!("opened and shown in {:.0} ms ({made})", ms(t.elapsed()));
    assert!(made.contains(&format!("{} kept compressed", LAYERS - 1)), "not parked as meant: {made}");

    // each chosen in turn, twice round (the first time round, the one let go has no compressed
    // copy yet, so it is read back and compressed too)
    let ids: Vec<u64> = s.layers().iter().map(|l| l.id).collect();
    assert_eq!(ids.len(), LAYERS);
    let mut back_ms = Vec::new();
    for round in 1..=2 {
        for &id in ids.iter().rev() {
            logged();
            let t = Instant::now();
            assert!(s.select_layer(id));
            show(&s, &gpu, &view);
            let took = t.elapsed();
            let lines = logged();
            let remade = lines.iter().find(|l| l.starts_with("caches remade")).cloned().unwrap_or_default();
            // "... N layers brought back in M ms"
            let brought = remade
                .split(" layers brought back in ")
                .nth(1)
                .and_then(|r| r.trim_end_matches(" ms").parse::<f64>().ok());
            let count = remade
                .rsplit_once(" layers brought back")
                .and_then(|(head, _)| head.rsplit(' ').next())
                .and_then(|n| n.parse::<f64>().ok());
            if let (Some(b), Some(n)) = (brought, count)
                && n > 0.0
            {
                back_ms.push(b / n);
            }
            eprintln!("round {round}, layer {id} chosen: settled in {:.0} ms ({remade})", ms(took));
        }
    }

    // a parked layer's pixels read as copying it does: decoded and laid out whole; as painted
    let current = s.layers().iter().find(|l| l.current).map(|l| l.id);
    let mut copy_ms = Vec::new();
    for (i, &id) in s.layers().iter().rev().map(|l| l.id).collect::<Vec<_>>().iter().enumerate() {
        if Some(id) == current {
            continue;
        }
        let t = Instant::now();
        let copied = s.copy_layer_contents(id).unwrap();
        copy_ms.push(ms(t.elapsed()));
        assert!(copied.rgba == painted[i], "layer {id} read back otherwise than painted");
    }
    let best = |v: &[f64]| v.iter().copied().fold(f64::MAX, f64::min);
    eprintln!(
        "a parked layer brought back for the caches: {:.0} ms at best ({} times); its pixels read: {:.0} ms at best \
         ({:?} ms)",
        best(&back_ms),
        back_ms.len(),
        best(&copy_ms),
        copy_ms.iter().map(|m| m.round()).collect::<Vec<_>>()
    );
    drop(s);
    let _ = std::fs::remove_dir_all(&dir);
}

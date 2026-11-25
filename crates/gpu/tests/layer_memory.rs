//! How many layers a canvas holds when those not worked on are kept compressed off the GPU: by
//! what they take, not each at its full size (skipped where the machine has no GPU). On its own
//! as it sets the layers' budget, which the whole process shares.
#![allow(clippy::unwrap_used, clippy::indexing_slicing)]

use procraft_engine::document::{Document, MOST_LAYERS, set_layer_budget};
use procraft_gpu::{Canvas, Gpu};

/// 4 MiB a layer's texture.
const W: u32 = 1024;
const FULL: u64 = W as u64 * W as u64 * 4;

/// A layer's worth of pixels that don't compress.
fn noise(seed: u32) -> Vec<u8> {
    let mut x = seed.wrapping_mul(2_654_435_761).max(1);
    let mut px = vec![0u8; FULL as usize];
    for p in px.as_chunks_mut::<4>().0 {
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        p[..3].copy_from_slice(&x.to_le_bytes()[..3]);
        p[3] = 255;
    }
    px
}

#[test]
fn layers_kept_compressed_count_by_what_they_take() {
    let gpu = match Gpu::headless() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipped: {e}");
            return;
        }
    };
    // 256 MiB for layers: four layers' worth kept back, room for 60 each its full size
    set_layer_budget(256 << 20);
    let mut doc = Document::new(W, W);
    let mut canvas = Canvas::new(&gpu, &doc).unwrap();
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(doc.layer_memory(), None, "every texture kept: nothing measured");
    assert_eq!(doc.layer_cap(), 60);

    // textures for three layers only: the rest are kept compressed, and empty ones take little
    canvas.set_resident_budget(3 * FULL);
    for _ in 0..80 {
        doc.add_layer();
    }
    canvas.render(&gpu, &mut doc, None);
    canvas.render(&gpu, &mut doc, None);
    assert_eq!(gpu.errors(), 0, "GPU validation errors");
    let measured = doc.layer_memory().expect("measured as it renders");
    assert_eq!((measured.resident, measured.layers), (3, 81));
    assert_eq!(doc.layer_cap(), MOST_LAYERS, "81 empty layers, far past the 60 full ones");
    assert!(doc.has_room(500));

    // layers full of what doesn't compress take nearly their full size: fewer fit
    let ids: Vec<_> = doc.pixel_layer_ids().collect();
    for (n, id) in ids.iter().take(50).enumerate() {
        canvas.park_pixels(*id, &noise(n as u32 + 1)).unwrap();
    }
    canvas.render(&gpu, &mut doc, None);
    let cap = doc.layer_cap();
    assert!(cap > 81 && cap < MOST_LAYERS, "room for some more: {cap}");
    // past the room: no more
    for (n, id) in ids.iter().skip(50).take(8).enumerate() {
        canvas.park_pixels(*id, &noise(n as u32 + 100)).unwrap();
    }
    canvas.render(&gpu, &mut doc, None);
    assert!(!doc.has_room(1), "58 layers that don't compress fill 256 MiB: {}", doc.layer_cap());
}

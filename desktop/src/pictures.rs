//! The store's pictures: fetched once by address on a thread, decoded once, kept as textures.

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use procraft_core::Store;

/// The largest a picture may be before it is dropped: the card and the band never show more.
const PICTURE_BYTES: usize = 4 * 1024 * 1024;

/// Every picture the store has asked for, by the address it came from. An address answered with
/// nothing is kept as such, so it is not asked again.
#[derive(Default)]
pub struct Pictures {
    kept: HashMap<String, Option<egui::TextureHandle>>,
    waiting: Vec<(String, Receiver<procraft_core::Fetched>)>,
}

impl Pictures {
    /// The texture for `address`, asking for it the first time. Nothing while it comes, and
    /// nothing where the address gives nothing.
    pub fn get(&mut self, ctx: &egui::Context, store: &Arc<Store>, address: &str) -> Option<egui::TextureHandle> {
        if address.is_empty() {
            return None;
        }
        if let Some(kept) = self.kept.get(address) {
            return kept.clone();
        }
        if self.waiting.iter().any(|(asked, _)| asked == address) {
            return None;
        }
        // four at a time is plenty for a screenful, and the rest ask again next frame
        if self.waiting.len() >= 4 {
            ctx.request_repaint();
            return None;
        }
        let (sender, done) = std::sync::mpsc::channel();
        let store = store.clone();
        let asked = address.to_owned();
        std::thread::spawn(move || {
            let _ = sender.send(store.fetch_picture(asked));
        });
        self.waiting.push((address.to_owned(), done));
        ctx.request_repaint();
        None
    }

    /// Takes in whatever came back, one frame's worth.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let mut done = Vec::new();
        let mut still = Vec::new();
        for (address, waiting) in self.waiting.drain(..) {
            match waiting.try_recv() {
                Ok(fetched) => done.push((address, fetched)),
                Err(TryRecvError::Empty) => still.push((address, waiting)),
                Err(TryRecvError::Disconnected) => {
                    self.kept.insert(address, None);
                }
            };
        }
        self.waiting = still;
        for (address, fetched) in done {
            let kept = if fetched.bytes.len() <= PICTURE_BYTES { decode(ctx, &address, &fetched.bytes) } else { None };
            self.kept.insert(address, kept);
        }
        if !self.waiting.is_empty() {
            ctx.request_repaint();
        }
    }
}

/// The bytes as a texture, or nothing when they are not a picture this app reads.
fn decode(ctx: &egui::Context, address: &str, bytes: &[u8]) -> Option<egui::TextureHandle> {
    let picture = image::load_from_memory(bytes).ok()?.to_rgba8();
    let size = [picture.width() as usize, picture.height() as usize];
    let colour = egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw());
    Some(ctx.load_texture(format!("store picture {address}"), colour, egui::TextureOptions::LINEAR))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 2 by 2 PNG, made here so the test needs nothing on disk.
    fn png() -> Vec<u8> {
        let picture = image::RgbaImage::from_pixel(2, 2, image::Rgba([200, 40, 40, 255]));
        let mut bytes = Vec::new();
        image::write_buffer_with_format(
            &mut std::io::Cursor::new(&mut bytes),
            picture.as_raw(),
            2,
            2,
            image::ExtendedColorType::Rgba8,
            image::ImageFormat::Png,
        )
        .ok();
        bytes
    }

    #[test]
    fn a_png_becomes_a_texture_and_rubbish_does_not() {
        let ctx = egui::Context::default();
        let bytes = png();
        assert!(!bytes.is_empty(), "the test made a png");
        assert!(decode(&ctx, "test", &bytes).is_some());
        assert!(decode(&ctx, "rubbish", b"not a picture").is_none());
    }
}

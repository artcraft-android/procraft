//! The store's pictures: fetched once by address on a thread, decoded once, kept as textures.

use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError};

use procraft_core::{Store, StoreItem, StorePreview};

const KEPT_PICTURES: usize = 32;
#[cfg(test)]
const PICTURE_EDGE: u32 = 800;

/// Every picture the store has asked for, by the address it came from. An address answered with
/// nothing is kept as such, so it is not asked again.
#[derive(Default)]
pub struct Pictures {
    kept: HashMap<String, Option<egui::TextureHandle>>,
    recent: VecDeque<String>,
    waiting: Vec<(String, Receiver<Option<egui::ColorImage>>)>,
}

impl Pictures {
    /// The cover for a listing, including generated artwork for plugins without a static image.
    pub fn cover(&mut self, ctx: &egui::Context, store: &Arc<Store>, item: &StoreItem) -> Option<egui::TextureHandle> {
        self.artwork(ctx, store, item, false)
    }

    pub fn icon(&mut self, ctx: &egui::Context, store: &Arc<Store>, item: &StoreItem) -> Option<egui::TextureHandle> {
        if item.has_icon() {
            let loaded = self.artwork(ctx, store, item, true);
            if loaded.is_some() || !self.kept.contains_key(&item.icon_key()) {
                return loaded;
            }
        }
        self.cover(ctx, store, item)
    }

    fn artwork(
        &mut self,
        ctx: &egui::Context,
        store: &Arc<Store>,
        item: &StoreItem,
        icon: bool,
    ) -> Option<egui::TextureHandle> {
        if !icon && item.preview.is_empty() && item.kind != procraft_core::ItemKind::Plugin {
            return None;
        }
        let key = if icon { item.icon_key() } else { item.cover_key() };
        if let Some(kept) = self.kept.get(&key).cloned() {
            self.used(&key);
            return kept;
        }
        if self.waiting.iter().any(|(asked, _)| *asked == key) {
            return None;
        }
        if self.waiting.len() >= 4 {
            ctx.request_repaint();
            return None;
        }
        let (sender, done) = std::sync::mpsc::channel();
        let store = store.clone();
        let item = item.clone();
        std::thread::spawn(move || {
            let picture = if icon { store.fetch_store_icon(item) } else { store.fetch_store_cover(item) };
            let _ = sender.send(picture.as_ref().map(colour_image));
        });
        self.waiting.push((key, done));
        ctx.request_repaint();
        None
    }

    /// Takes in whatever came back, one frame's worth.
    pub fn poll(&mut self, ctx: &egui::Context) {
        let mut done = Vec::new();
        self.waiting.retain(|(address, waiting)| match waiting.try_recv() {
            Ok(picture) => {
                done.push((address.clone(), picture));
                false
            }
            Err(TryRecvError::Empty) => true,
            Err(TryRecvError::Disconnected) => {
                done.push((address.clone(), None));
                false
            }
        });
        for (address, picture) in done {
            let texture = picture.map(|picture| {
                ctx.load_texture(format!("store picture {address}"), picture, egui::TextureOptions::LINEAR)
            });
            self.used(&address);
            self.kept.insert(address, texture);
        }
        if !self.waiting.is_empty() {
            ctx.request_repaint();
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    fn used(&mut self, address: &str) {
        self.recent.retain(|one| one != address);
        self.recent.push_back(address.to_owned());
        if self.recent.len() > KEPT_PICTURES
            && let Some(old) = self.recent.pop_front()
        {
            self.kept.remove(&old);
        }
    }
}

pub fn colour_image(preview: &StorePreview) -> egui::ColorImage {
    egui::ColorImage::from_rgba_unmultiplied([preview.width as usize, preview.height as usize], &preview.rgba)
}

#[cfg(test)]
fn pixels(bytes: &[u8]) -> Option<egui::ColorImage> {
    let picture = procraft_core::store_picture(bytes, PICTURE_EDGE)?;
    let size = [picture.width() as usize, picture.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, picture.as_raw()))
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
        let bytes = png();
        assert!(!bytes.is_empty(), "the test made a png");
        assert!(pixels(&bytes).is_some());
        assert!(pixels(b"not a picture").is_none());
    }
}

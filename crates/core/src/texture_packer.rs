//! The Texture Packer: the separate drawings on the canvas, as it shows now with
//! no background, found as boxes (`procraft_engine::sprite_scan`) to pick, name and share each as a
//! PNG of its own, in sRGB (what game engines take a PNG's colours to be).

use std::fs;
use std::path::Path;

use procraft_engine::cms::{Lut, builtin};
use procraft_engine::profile::Profile;
use procraft_engine::sprite_scan::{self, SpriteBox};

use crate::project::{file_error, png_bytes, straight_alpha, thumbnail, write_atomically};
use crate::{CanvasSession, MobileError};

/// The picture the packer cuts from: straight RGBA in sRGB, and the drawings found on it before
/// any are joined.
pub(crate) struct Packer {
    width: u32,
    height: u32,
    rgba: Vec<u8>,
    components: Vec<SpriteBox>,
}

/// A sprite's box on the canvas (canvas pixels).
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct PackerBox {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl From<SpriteBox> for PackerBox {
    fn from(b: SpriteBox) -> PackerBox {
        PackerBox { x: b.x, y: b.y, width: b.width, height: b.height }
    }
}

impl From<PackerBox> for SpriteBox {
    fn from(b: PackerBox) -> SpriteBox {
        SpriteBox { x: b.x, y: b.y, width: b.width, height: b.height }
    }
}

/// A sprite to share: its box, its place in reading order (for its default name) and its name
/// (empty: the default).
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct PackerSprite {
    pub place: PackerBox,
    pub index: u32,
    pub name: String,
}

#[uniffi::export]
impl CanvasSession {
    /// Opens the Texture Packer on the canvas as it shows now (the frame drawn on, if it is an
    /// animation), without its background: how many separate drawings it holds (none: nothing to
    /// pack).
    pub fn texture_packer_open(&self) -> Result<u32, MobileError> {
        let (width, height, rgba, convert) = {
            let mut guard = self.shared.lock_finished();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let mut doc = st.doc.clone();
            doc.background_visible = false;
            let convert = (doc.icc.is_some() || doc.profile != Profile::Srgb).then(|| doc.rgb_profile());
            let (w, h) = (doc.width(), doc.height());
            let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else {
                return Err(file_error("reading the picture", "the canvas is not shown yet"));
            };
            canvas.invalidate();
            canvas.render_finished(gpu, &mut doc);
            // an HDR canvas's light above white rolled off into the standard picture
            let rgba = if canvas.is_hdr() {
                canvas.read_composite_wide(gpu).map(|half| procraft_engine::hdr::tone_mapped(&half))
            } else {
                canvas.read_composite(gpu)
            };
            canvas.invalidate();
            canvas.render(gpu, &mut st.doc, None);
            self.shared.frame(st);
            (w, h, rgba.map_err(|e| file_error("reading the picture", e))?, convert)
        };
        let mut straight = straight_alpha(&rgba);
        if let Some(from) = convert {
            Lut::build(&from, &builtin(Profile::Srgb), None, 33).apply_rgba(&mut straight, false);
        }
        let components = sprite_scan::components(&straight, width, height);
        let count = u32::try_from(components.len()).unwrap_or(u32::MAX);
        self.shared.lock().packer = Some(Packer { width, height, rgba: straight, components });
        Ok(count)
    }

    /// The sprites' boxes, in reading order: drawings fewer than `separation` clear pixels apart
    /// (1 to 40; at 1 only touching ones) taken as one, each grown by `padding` pixels (0 to 32).
    pub fn texture_packer_boxes(&self, separation: u32, padding: u32) -> Vec<PackerBox> {
        let st = self.shared.lock();
        let Some(p) = st.packer.as_ref() else { return Vec::new() };
        let joined = sprite_scan::merged(&p.components, separation.clamp(1, 40));
        sprite_scan::padded(&joined, padding.min(32), p.width, p.height).into_iter().map(PackerBox::from).collect()
    }

    /// The box (an index into `boxes`) a tap at canvas point (`x`, `y`) picks: the smallest under
    /// it.
    pub fn texture_packer_hit(&self, boxes: Vec<PackerBox>, x: f32, y: f32) -> Option<u32> {
        let boxes: Vec<SpriteBox> = boxes.into_iter().map(SpriteBox::from).collect();
        sprite_scan::hit(&boxes, x, y).and_then(|i| u32::try_from(i).ok())
    }

    /// The name sprite `index` (in reading order) shares under until it is renamed.
    pub fn texture_packer_default_name(&self, index: u32) -> String {
        sprite_scan::default_name(index as usize)
    }

    /// Box `place` cut out as a PNG at most `side` pixels across, to show.
    pub fn texture_packer_thumbnail(&self, place: PackerBox, side: u32) -> Vec<u8> {
        let st = self.shared.lock();
        let Some(p) = st.packer.as_ref() else { return Vec::new() };
        let Some(b) = kept(p, place) else { return Vec::new() };
        let cut = sprite_scan::crop(&p.rgba, p.width, &b);
        let (w, h, small) = thumbnail(&cut, b.width, b.height, side.max(1));
        png_bytes(w, h, &small, &[]).unwrap_or_default()
    }

    /// Writes each of `sprites` as its own PNG (sRGB, straight alpha) in folder `dir` (made if
    /// missing), named as it is named (made safe, and none written over another); their paths.
    pub fn texture_packer_export(&self, dir: String, sprites: Vec<PackerSprite>) -> Result<Vec<String>, MobileError> {
        let dir = Path::new(&dir);
        fs::create_dir_all(dir).map_err(|e| file_error("saving", e))?;
        // the boxes off the picture left out before naming, so every name has its sprite
        let (cuts, named): (Vec<Cut>, Vec<(usize, String)>) = {
            let st = self.shared.lock();
            let Some(p) = st.packer.as_ref() else { return Err(file_error("saving", "the Texture Packer isn't open")) };
            sprites
                .iter()
                .filter_map(|s| kept(p, s.place).map(|b| (s, b)))
                .map(|(s, b)| ((b, sprite_scan::crop(&p.rgba, p.width, &b)), (s.index as usize, s.name.clone())))
                .unzip()
        };
        let names = sprite_scan::file_names(&named);
        self.exported(0.0);
        let mut paths = Vec::with_capacity(cuts.len());
        for (k, ((b, cut), name)) in cuts.iter().zip(&names).enumerate() {
            let png = png_bytes(b.width, b.height, cut, &[])?;
            let path = dir.join(format!("{name}.png"));
            write_atomically(&path, |f| fs::write(f, &png).map_err(|e| file_error("saving", e)))?;
            paths.push(path.to_string_lossy().into_owned());
            self.exported((k + 1) as f32 / cuts.len().max(1) as f32);
        }
        Ok(paths)
    }

    /// Closes the Texture Packer, letting its picture go.
    pub fn texture_packer_close(&self) {
        self.shared.lock().packer = None;
    }
}

/// A sprite's box and its pixels cut out.
type Cut = (SpriteBox, Vec<u8>);

/// `place` kept to the packer's picture; none if nothing of it is on it.
fn kept(p: &Packer, place: PackerBox) -> Option<SpriteBox> {
    let x = place.x.min(p.width);
    let y = place.y.min(p.height);
    let width = place.width.min(p.width - x);
    let height = place.height.min(p.height - y);
    (width > 0 && height > 0).then_some(SpriteBox { x, y, width, height })
}

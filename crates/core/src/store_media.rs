//! Store cover policy, gallery playback, and independent limits for demo media.

use crate::store_decode::{StoreDecodeKind, decode_bytes, end, read_u32, write_u32};
use std::io::{self, Cursor, Read, Write};
use std::path::Path;

use image::codecs::{gif::GifDecoder, png::PngDecoder, webp::WebPDecoder};
use image::{AnimationDecoder as _, ImageDecoder as _, ImageFormat, RgbaImage};

use crate::{ItemKind, Store, StoreItem, StorePreview, store_picture};

pub(crate) const PICTURE_BYTES: usize = 4 * 1024 * 1024;
const GALLERY_BYTES: usize = 12 * 1024 * 1024;
pub(crate) const VIDEO_BYTES: usize = 25 * 1024 * 1024;
const ANIMATION_FRAMES: usize = 120;
const ANIMATION_MS: u32 = 12_000;
const ANIMATION_PIXELS: u64 = 160_000_000;
const ANIMATION_EDGE: u32 = 320;
const PICTURE_EDGE: u32 = 800;

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum StoreVideo {
    Webm,
    Mp4,
}

impl StoreVideo {
    pub fn extension(self) -> &'static str {
        match self {
            Self::Webm => "webm",
            Self::Mp4 => "mp4",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct StoreGalleryEntry {
    pub source: String,
    pub cover: bool,
    pub url: String,
    pub video: Option<StoreVideo>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct StoreGalleryFrame {
    pub image: StorePreview,
    pub delay_ms: u32,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct StoreGalleryImage {
    pub frames: Vec<StoreGalleryFrame>,
    pub failure: Option<String>,
}

fn extension(address: &str) -> &str {
    address
        .split(['?', '#'])
        .next()
        .unwrap_or_default()
        .rsplit('/')
        .next()
        .unwrap_or_default()
        .rsplit_once('.')
        .map_or("", |(_, ext)| ext)
}

fn video_of(address: &str) -> Option<StoreVideo> {
    let ext = extension(address);
    if ext.eq_ignore_ascii_case("webm") {
        Some(StoreVideo::Webm)
    } else if ext.eq_ignore_ascii_case("mp4") {
        Some(StoreVideo::Mp4)
    } else {
        None
    }
}

fn gallery_only_url(address: &str) -> bool {
    let ext = extension(address);
    video_of(address).is_some() || ext.eq_ignore_ascii_case("gif") || ext.eq_ignore_ascii_case("apng")
}

impl StoreItem {
    pub fn gallery_entries(&self) -> Vec<StoreGalleryEntry> {
        let mut entries = Vec::new();
        if self.kind == ItemKind::Plugin || !self.preview.is_empty() {
            entries.push(StoreGalleryEntry {
                source: self.source.clone(),
                cover: true,
                url: self.preview.clone(),
                video: None,
            });
        }
        for url in &self.previews {
            if entries.iter().any(|entry| entry.url == *url && (!entry.cover || !gallery_only_url(url))) {
                continue;
            }
            if entries.len() == crate::store::PREVIEWS_MOST {
                break;
            }
            entries.push(StoreGalleryEntry {
                source: self.source.clone(),
                cover: false,
                url: url.clone(),
                video: video_of(url),
            });
        }
        entries
    }

    pub fn cover_key(&self) -> String {
        if self.kind == ItemKind::Plugin {
            format!(
                "plugin-cover:{}:{}:{}:{}:{}:{}",
                self.source, self.id, self.name, self.publisher, self.category, self.preview
            )
        } else {
            format!("cover:{}:{}", self.source, self.preview)
        }
    }

    pub fn icon_key(&self) -> String {
        format!("icon:{}:{}", self.cover_key(), self.icon)
    }

    pub fn has_icon(&self) -> bool {
        !self.icon.is_empty() || self.kind == ItemKind::Plugin
    }
}

#[uniffi::export]
pub fn store_gallery_entries(item: StoreItem) -> Vec<StoreGalleryEntry> {
    item.gallery_entries()
}

#[uniffi::export]
pub fn store_cover_key(item: StoreItem) -> String {
    item.cover_key()
}

#[uniffi::export]
pub fn store_icon_key(item: StoreItem) -> String {
    item.icon_key()
}

#[uniffi::export]
impl Store {
    /// Static publisher art when it is readable. A plugin otherwise gets its own title card.
    pub fn fetch_store_cover(&self, item: StoreItem) -> Option<StorePreview> {
        let supplied = if item.preview.is_empty() || gallery_only_url(&item.preview) {
            None
        } else {
            self.decoded_store_image(&item.source, &item.preview, StoreDecodeKind::Cover, PICTURE_BYTES)
                .ok()
                .and_then(|frames| frames.into_iter().next())
                .map(|frame| frame.image)
        };
        supplied.or_else(|| {
            (item.kind == ItemKind::Plugin).then(|| {
                let art = crate::store_previews::plugin_cover(&item);
                preview(image::imageops::resize(
                    &art,
                    PICTURE_EDGE,
                    PICTURE_EDGE * 3 / 4,
                    image::imageops::FilterType::Triangle,
                ))
            })
        })
    }

    /// Square publisher artwork or the plugin's square template.
    pub fn fetch_store_icon(&self, item: StoreItem) -> Option<StorePreview> {
        if !item.icon.is_empty()
            && !gallery_only_url(&item.icon)
            && let Ok(frames) =
                self.decoded_store_image(&item.source, &item.icon, StoreDecodeKind::Cover, PICTURE_BYTES)
            && let Some(frame) = frames.into_iter().next()
        {
            let image = frame.image;
            if image.width == image.height {
                return Some(image);
            }
            return RgbaImage::from_raw(image.width, image.height, image.rgba)
                .map(|image| preview(crate::store_package::square_store_art(&image, PICTURE_EDGE)));
        }
        (item.kind == ItemKind::Plugin).then(|| {
            let art = crate::store_previews::plugin_icon(&item);
            preview(image::imageops::resize(&art, PICTURE_EDGE, PICTURE_EDGE, image::imageops::FilterType::Triangle))
        })
    }

    /// Fetches and decodes the selected gallery image, including its frames when it animates.
    pub fn fetch_gallery_image(&self, address: String) -> StoreGalleryImage {
        self.fetch_gallery_image_from(String::new(), address)
    }

    pub fn fetch_gallery_image_from(&self, source: String, address: String) -> StoreGalleryImage {
        let result = self.decoded_store_image(&source, &address, StoreDecodeKind::Gallery, GALLERY_BYTES);
        match result {
            Ok(frames) => StoreGalleryImage { frames, failure: None },
            Err(reason) => StoreGalleryImage { frames: Vec::new(), failure: Some(reason) },
        }
    }

    /// Stages a gallery clip on disk after the user asks to play it.
    pub fn download_store_video(&self, address: String, path: String) -> Option<String> {
        self.download_store_video_from(String::new(), address, path)
    }

    pub fn download_store_video_from(&self, source: String, address: String, path: String) -> Option<String> {
        let Some(kind) = video_of(&address) else {
            return Some("Choose a WebM or MP4 gallery video.".to_owned());
        };
        let path = Path::new(&path);
        let result = (|| {
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(path)
                .map_err(|e| format!("The video couldn't be staged: {e}"))?;
            let written = self
                .copy_media(&source, &address, VIDEO_BYTES, &mut file)
                .and_then(|_| file.flush().map_err(|e| e.to_string()));
            drop(file);
            let result = written.and_then(|_| {
                let kind = match kind {
                    StoreVideo::Webm => StoreDecodeKind::Webm,
                    StoreVideo::Mp4 => StoreDecodeKind::Mp4,
                };
                let mut checked = crate::store_decode::decode(kind, path)?;
                end(&mut checked).map_err(|error| error.to_string())
            });
            if result.is_err() {
                let _ = std::fs::remove_file(path);
            }
            result
        })();
        result.err()
    }
}

impl Store {
    fn decoded_store_image(
        &self,
        source: &str,
        address: &str,
        kind: StoreDecodeKind,
        cap: usize,
    ) -> Result<Vec<StoreGalleryFrame>, String> {
        let bytes = self.fetch_media_bytes(source, address, cap)?;
        let mut input = decode_bytes(kind, &bytes)?;
        read_decoded_images(kind, &mut input).map_err(|_| "The Store decoder returned invalid image data.".to_owned())
    }
}

pub(crate) fn decode_images(kind: StoreDecodeKind, input: impl Read, out: &mut impl Write) -> Result<(), String> {
    let cover = matches!(kind, StoreDecodeKind::Cover);
    let cap = if cover { PICTURE_BYTES } else { GALLERY_BYTES };
    let mut bytes = Vec::new();
    input.take(cap as u64 + 1).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > cap {
        return Err("The Store image exceeds its size limit.".to_owned());
    }
    let frames = if cover {
        let image = static_cover(&bytes).ok_or("The cover must be a readable static image.")?;
        vec![StoreGalleryFrame { image: preview(image), delay_ms: 0 }]
    } else {
        gallery_image(&bytes)?
    };
    let write = (|| -> io::Result<()> {
        write_u32(out, frames.len() as u32)?;
        for frame in frames {
            write_u32(out, frame.image.width)?;
            write_u32(out, frame.image.height)?;
            write_u32(out, frame.delay_ms)?;
            out.write_all(&frame.image.rgba)?;
        }
        Ok(())
    })();
    write.map_err(|e| e.to_string())
}

fn read_decoded_images(kind: StoreDecodeKind, input: &mut impl Read) -> io::Result<Vec<StoreGalleryFrame>> {
    let bad = || io::Error::other("Invalid decoded Store image.");
    let cover = matches!(kind, StoreDecodeKind::Cover);
    let count = read_u32(input)? as usize;
    if count == 0 || count > ANIMATION_FRAMES || (cover && count != 1) {
        return Err(bad());
    }
    let edge = if count > 1 { ANIMATION_EDGE } else { PICTURE_EDGE };
    let mut frames = Vec::with_capacity(count);
    let mut duration = 0;
    for _ in 0..count {
        let (width, height, delay_ms) = (read_u32(input)?, read_u32(input)?, read_u32(input)?);
        duration += u64::from(delay_ms);
        if !(1..=edge).contains(&width)
            || !(1..=edge).contains(&height)
            || duration > u64::from(ANIMATION_MS)
            || (count > 1 && delay_ms == 0)
        {
            return Err(bad());
        }
        let mut rgba = vec![0; (width * height * 4) as usize];
        input.read_exact(&mut rgba)?;
        frames.push(StoreGalleryFrame { image: StorePreview { width, height, rgba }, delay_ms });
    }
    end(input)?;
    Ok(frames)
}

fn preview(picture: RgbaImage) -> StorePreview {
    StorePreview { width: picture.width(), height: picture.height(), rgba: picture.into_raw() }
}

fn static_cover(bytes: &[u8]) -> Option<RgbaImage> {
    match image::guess_format(bytes).ok()? {
        ImageFormat::Jpeg => {}
        ImageFormat::Png if !PngDecoder::with_limits(Cursor::new(bytes), limits(4096)).ok()?.is_apng().ok()? => {}
        ImageFormat::WebP if !WebPDecoder::new(Cursor::new(bytes)).ok()?.has_animation() => {}
        _ => return None,
    }
    store_picture(bytes, PICTURE_EDGE)
}

fn limits(edge: u32) -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(edge);
    limits.max_image_height = Some(edge);
    limits.max_alloc = Some(64 * 1024 * 1024);
    limits
}

fn gallery_image(bytes: &[u8]) -> Result<Vec<StoreGalleryFrame>, String> {
    let decode_error = |_: image::ImageError| "The gallery image couldn't be read.".to_owned();
    let safe = limits(2048);
    match image::guess_format(bytes).map_err(decode_error)? {
        ImageFormat::Gif => {
            let mut decoder = GifDecoder::new(Cursor::new(bytes)).map_err(decode_error)?;
            decoder.set_limits(safe).map_err(decode_error)?;
            animation(decoder.into_frames())
        }
        ImageFormat::Png => {
            let mut decoder = PngDecoder::with_limits(Cursor::new(bytes), limits(4096)).map_err(decode_error)?;
            if decoder.is_apng().map_err(decode_error)? {
                decoder.set_limits(safe).map_err(decode_error)?;
                animation(decoder.apng().map_err(decode_error)?.into_frames())
            } else {
                still(bytes)
            }
        }
        ImageFormat::WebP => {
            let mut decoder = WebPDecoder::new(Cursor::new(bytes)).map_err(decode_error)?;
            if decoder.has_animation() {
                decoder.set_limits(safe).map_err(decode_error)?;
                animation(decoder.into_frames())
            } else {
                still(bytes)
            }
        }
        ImageFormat::Jpeg => still(bytes),
        _ => Err("Choose a PNG, JPEG, GIF, APNG or WebP gallery image.".to_owned()),
    }
}

fn still(bytes: &[u8]) -> Result<Vec<StoreGalleryFrame>, String> {
    let picture = store_picture(bytes, PICTURE_EDGE).ok_or("The gallery image couldn't be read.")?;
    Ok(vec![StoreGalleryFrame { image: preview(picture), delay_ms: 0 }])
}

fn animation(frames: image::Frames<'_>) -> Result<Vec<StoreGalleryFrame>, String> {
    let mut clip = Vec::new();
    let mut duration = 0;
    let mut pixels = 0;
    for frame in frames {
        let frame = frame.map_err(|_| "A gallery animation frame couldn't be read.")?;
        let (n, d) = frame.delay().numer_denom_ms();
        let delay_ms = (n / d.max(1)).max(20);
        duration = u32::saturating_add(duration, delay_ms);
        let picture = frame.into_buffer();
        pixels += u64::from(picture.width()) * u64::from(picture.height());
        if clip.len() >= ANIMATION_FRAMES || duration > ANIMATION_MS || pixels > ANIMATION_PIXELS {
            return Err("The animation is too long or detailed. Use a shorter clip or submit a WebM or MP4.".to_owned());
        }
        let picture = if picture.width().max(picture.height()) > ANIMATION_EDGE {
            image::DynamicImage::ImageRgba8(picture)
                .resize(ANIMATION_EDGE, ANIMATION_EDGE, image::imageops::FilterType::Triangle)
                .into_rgba8()
        } else {
            picture
        };
        clip.push(StoreGalleryFrame { image: preview(picture), delay_ms });
    }
    if clip.is_empty() {
        return Err("The animation has no frames.".to_owned());
    }
    Ok(clip)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gif_and_apng_play_in_the_gallery_but_are_rejected_as_covers() {
        let red = RgbaImage::from_pixel(6, 4, image::Rgba([231, 27, 62, 255]));
        let blue = RgbaImage::from_pixel(6, 4, image::Rgba([26, 62, 231, 255]));
        let mut gif = Vec::new();
        image::codecs::gif::GifEncoder::new(&mut gif)
            .encode_frames(
                [red.clone(), blue.clone()]
                    .map(|frame| image::Frame::from_parts(frame, 0, 0, image::Delay::from_numer_denom_ms(120, 1))),
            )
            .unwrap();
        let mut apng = Vec::new();
        {
            let mut png = png::Encoder::new(&mut apng, 6, 4);
            png.set_color(png::ColorType::Rgba);
            png.set_animated(2, 0).unwrap();
            let mut writer = png.write_header().unwrap();
            writer.write_image_data(red.as_raw()).unwrap();
            writer.write_image_data(blue.as_raw()).unwrap();
            writer.finish().unwrap();
        }
        for bytes in [gif, apng] {
            assert!(static_cover(&bytes).is_none());
            let frames = gallery_image(&bytes).unwrap();
            assert_eq!(frames.len(), 2);
            assert_ne!(frames[0].image.rgba, frames[1].image.rgba);
        }
    }

    #[test]
    fn plugin_gallery_begins_with_a_cover_and_preserves_demo_order() {
        let mut item = crate::store::bundled_plugin_for_cover_test().unwrap();
        item.previews =
            vec!["https://example.com/demo.APNG".to_owned(), "https://example.com/demo.webm?v=1".to_owned()];
        let gallery = item.gallery_entries();
        assert!(gallery[0].cover);
        assert_eq!(gallery[1].video, None);
        assert_eq!(gallery[2].video, Some(StoreVideo::Webm));
    }
}

//! HDR artworks (see [`procraft_engine::document::DynamicRange`]): choosing it as a canvas is made,
//! the light tool's limit above white, the screen's headroom, layers saved with their light above
//! white, and HDR exports.
//!
//! An HDR artwork's layer files are 16-bit PNGs whose samples are the layer's half floats as they
//! are (premultiplied, sRGB-encoded, past 1 for light above white): read only by ProCraft, which
//! takes an 8-bit layer file as it always has.

use std::fs;
use std::path::Path;
use std::sync::Arc;

use procraft_engine::document::DynamicRange;
use procraft_engine::hdr::{self as pixels, HDR_PIXEL};

use crate::project::{file_error, write_atomically};
use crate::{CanvasSession, MobileError};

/// How far above white the light tool brightens an HDR canvas at first, stops (four times
/// white's light).
pub(crate) const LIGHT_LIMIT: f32 = 2.0;
/// The most it may be set to.
pub(crate) const LIGHT_LIMIT_MOST: f32 = 5.0;
/// The most stops above white the paint colour may be lit.
pub(crate) const COLOR_STOPS_MOST: f32 = 5.0;

impl crate::State {
    /// The colour strokes paint with: on an HDR canvas the colour chosen lit its stops above
    /// white (see [`CanvasSession::set_color_stops`]), else as it is.
    pub(crate) fn paint_colour(&self) -> [f32; 3] {
        if self.doc.range.is_high() { pixels::lit_above_white(self.color, self.color_stops) } else { self.color }
    }
}

/// The light tool's ceiling (linear light) for a limit of `stops` above white.
pub(crate) fn ceiling(stops: f32) -> f32 {
    let stops = if stops.is_finite() { stops.clamp(0.0, LIGHT_LIMIT_MOST) } else { LIGHT_LIMIT };
    stops.exp2()
}

/// A layer `width` × `height` read back as it is saved: an HDR artwork's as half floats, a
/// standard one's RGBA8.
pub(crate) fn layer_pixels(
    read: procraft_gpu::Readback,
    hdr: bool,
    (width, height): (u32, u32),
) -> Result<Vec<u8>, procraft_gpu::GpuError> {
    let rgba = if hdr { read.wide_pixels()? } else { read.pixels()? };
    Ok(as_saved(&rgba, hdr, (width, height)))
}

/// Pixels as a layer `width` × `height` of an HDR artwork (`hdr`) is saved: 8-bit ones (a layer
/// not yet on the GPU) widened, half floats as they are.
pub(crate) fn as_saved(rgba: &[u8], hdr: bool, (width, height): (u32, u32)) -> Vec<u8> {
    let narrow = width as usize * height as usize * 4;
    if hdr && rgba.len() == narrow { pixels::widen(rgba) } else { rgba.to_vec() }
}

/// An HDR layer's file: its half floats as 16-bit samples.
pub(crate) fn write_half_png(path: &Path, width: u32, height: u32, half: &[u8]) -> Result<(), MobileError> {
    let mut info = png::Info::with_size(width, height);
    info.color_type = png::ColorType::Rgba;
    info.bit_depth = png::BitDepth::Sixteen;
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::with_info(&mut out, info).map_err(|e| file_error("saving", e))?;
        encoder.set_compression(png::Compression::Fast);
        let mut writer = encoder.write_header().map_err(|e| file_error("saving", e))?;
        // PNG's samples are big-endian
        let samples: Vec<u8> = half.as_chunks::<2>().0.iter().flat_map(|&[lo, hi]| [hi, lo]).collect();
        writer.write_image_data(&samples).map_err(|e| file_error("saving", e))?;
        writer.finish().map_err(|e| file_error("saving", e))?;
    }
    fs::write(path, out).map_err(|e| file_error("saving", e))
}

/// An HDR layer's half floats from its file (see [`write_half_png`]), which must be `width` ×
/// `height`.
pub(crate) fn read_half_png<R: std::io::BufRead + std::io::Seek>(
    mut reader: png::Reader<R>,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, MobileError> {
    let size = width as usize * height as usize * HDR_PIXEL;
    let mut samples = vec![0; reader.output_buffer_size().unwrap_or(size).max(size)];
    reader.next_frame(&mut samples).map_err(|e| file_error("opening a layer", e))?;
    samples.truncate(size);
    Ok(samples.as_chunks::<2>().0.iter().flat_map(|&[hi, lo]| [lo, hi]).collect())
}

/// An HDR artwork's picture with its light above white: half floats (premultiplied RGBA,
/// sRGB-encoded and carried on past white, rows top to bottom), for the app to write as an HDR
/// image.
#[derive(Clone, Debug, uniffi::Record)]
pub struct HdrPicture {
    pub width: u32,
    pub height: u32,
    pub half: Vec<u8>,
    /// In Display P3's primaries (else sRGB's).
    pub p3: bool,
    /// Its brightest light, linear (1: white).
    pub brightest: f32,
}

#[uniffi::export]
impl CanvasSession {
    /// Makes a canvas just made an HDR one (`hdr`, keeping light above white) or a standard one.
    /// Called before anything is drawn on it: what its canvas held is made again empty.
    pub fn set_new_hdr(&self, hdr: bool) {
        let range = if hdr { DynamicRange::High } else { DynamicRange::Standard };
        let mut st = self.shared.lock();
        if st.doc.range == range {
            return;
        }
        st.doc.range = range;
        // its layers' textures are of the other format: made again, and the window's surface
        // too (an HDR canvas is shown in the screen's extended range)
        if st.canvas.take().is_some() {
            st.ensure_canvas();
        }
        st.reconfigure = true;
        self.shared.frame(&mut st);
    }

    /// Whether the artwork is an HDR one.
    pub fn is_hdr(&self) -> bool {
        self.shared.lock().doc.range.is_high()
    }

    /// How far above white (stops, 0 to 5) the light tool brightens an HDR canvas.
    pub fn set_light_limit(&self, stops: f32) {
        let stops = if stops.is_finite() { stops.clamp(0.0, LIGHT_LIMIT_MOST) } else { LIGHT_LIMIT };
        self.shared.lock().light_limit = stops;
    }

    pub fn light_limit(&self) -> f32 {
        self.shared.lock().light_limit
    }

    /// How many stops above white (0 to [`COLOR_STOPS_MOST`]) an HDR canvas's paint colour is lit:
    /// the colour chosen, as bright as that. A standard canvas paints the colour as it is.
    pub fn set_color_stops(&self, stops: f32) {
        let stops = if stops.is_finite() { stops.clamp(0.0, COLOR_STOPS_MOST) } else { 0.0 };
        self.shared.lock().color_stops = stops;
    }

    pub fn color_stops(&self) -> f32 {
        self.shared.lock().color_stops
    }

    /// The screen's headroom now: how many times white's light the brightest it shows is (the
    /// iPad's `currentEDRHeadroom`, which changes with its brightness). An HDR canvas's light
    /// above white is rolled off to it.
    pub fn set_display_headroom(&self, headroom: f32) {
        let peak = if headroom.is_finite() { headroom.max(1.0) } else { 1.0 };
        let mut st = self.shared.lock();
        if (st.display_peak - peak).abs() > 0.01 {
            st.display_peak = peak;
            if st.doc.range.is_high() {
                self.shared.frame(&mut st);
            }
        }
    }

    /// The flattened picture of an HDR artwork with its light above white (see [`HdrPicture`]);
    /// a standard one's is its 8-bit picture widened.
    pub fn hdr_picture(&self) -> Result<HdrPicture, MobileError> {
        let mut guard = self.shared.lock_finished();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        let (width, height, p3) = (st.doc.width(), st.doc.height(), st.doc.profile.is_p3());
        let (Some(canvas), Some(gpu)) = (st.canvas.as_mut(), st.gpu.as_ref()) else {
            return Err(file_error("reading the picture", "the canvas is not shown yet"));
        };
        canvas.render_finished(gpu, &mut st.doc);
        let half = canvas.read_composite_wide(gpu).map_err(|e| file_error("reading the picture", e))?;
        let half = if canvas.is_hdr() { half } else { pixels::widen(&half) };
        let brightest = pixels::brightest(&half);
        Ok(HdrPicture { width, height, half, p3, brightest })
    }

    /// Frame `index` of the animation (as [`CanvasSession::animation_frame`] counts them), full
    /// size, with its light above white: for an HDR video. None past the last.
    pub fn animation_frame_hdr(&self, index: u32) -> Option<HdrPicture> {
        let shot = self.shot_at(index)?;
        let (width, height, half) = self.render_shot_full(shot, false, |_| {}, true).ok()?;
        let p3 = self.shared.lock().doc.profile.is_p3();
        let half = if half.len() == width as usize * height as usize * 4 { pixels::widen(&half) } else { half };
        let brightest = pixels::brightest(&half);
        Some(HdrPicture { width, height, half, p3, brightest })
    }

    /// Frame `index` of the animation as a frame of a 10-bit HLG video `width` × `height` (see
    /// [`procraft_engine::hdr::hlg_frame`]): half floats, BT.2020, HLG-encoded. None past the last.
    pub fn animation_frame_hlg(&self, index: u32, width: u32, height: u32) -> Option<Vec<u8>> {
        let frame = self.animation_frame_hdr(index)?;
        Some(pixels::hlg_frame(&frame.half, (frame.width, frame.height), frame.p3, (width, height)))
    }

    /// Writes the flattened picture to `path` as an OpenEXR file: half floats in linear light,
    /// premultiplied, an HDR artwork's light above white kept.
    pub fn export_exr(self: Arc<Self>, path: String) -> Result<(), MobileError> {
        self.exported(0.0);
        let picture = self.hdr_picture()?;
        self.exported(0.7);
        let bytes = pixels::exr(&picture.half, picture.width, picture.height, picture.p3);
        self.exported(0.95);
        write_atomically(Path::new(&path), |p| fs::write(p, &bytes).map_err(|e| file_error("saving", e)))?;
        self.exported(1.0);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_light_limit_is_stops_above_white() {
        assert_eq!(ceiling(0.0), 1.0);
        assert_eq!(ceiling(2.0), 4.0);
        assert_eq!(ceiling(9.0), 32.0);
        assert_eq!(ceiling(f32::NAN), 4.0);
    }

    /// A line across window y = 100 of a 100 × 100 canvas shown 200 across, in `colour` lit `stops`
    /// above white, on an HDR canvas or a standard one; then what the eyedropper finds on it.
    fn picked(hdr: bool, colour: [f32; 3], stops: f32) -> Option<crate::pick::Pick> {
        let gpu = match procraft_gpu::Gpu::headless() {
            Ok(g) => g,
            Err(e) => {
                eprintln!("skipped: {e}");
                return None;
            }
        };
        let s = CanvasSession::new(100, 100);
        s.set_new_hdr(hdr);
        {
            let mut st = s.shared.lock();
            st.window_size = (200, 200);
            st.view = Some(procraft_engine::view::View::fit((100, 100), (200, 200)));
            st.gpu = Some(gpu);
            st.ensure_canvas();
        }
        let [r, g, b] = colour;
        s.set_color(r, g, b);
        s.set_color_stops(stops);
        let at = |x: f32, t: f64| crate::PenSample {
            x,
            y: 100.0,
            pressure: 1.0,
            tilt: 0.0,
            orientation: 0.0,
            time_ms: t,
            roll: 0.0,
        };
        s.stroke_begin(crate::PointerKind::Stylus, vec![at(20.0, 0.0)]);
        s.stroke_add((1..40).map(|i| at(20.0 + i as f32 * 4.0, f64::from(i) * 8.0)).collect());
        s.stroke_end(vec![at(180.0, 330.0)]);
        s.pick(100.0, 100.0, 21)
    }

    #[test]
    fn an_hdr_canvas_paints_and_picks_light_above_white() {
        let Some(lit) = picked(true, [1.0, 0.5, 0.25], 2.0) else { return };
        // the colour as chosen, and its light: two stops above white
        assert!((lit.stops - 2.0).abs() < 0.05, "{}", lit.stops);
        let rgb = lit.rgb.unwrap();
        assert_eq!(rgb >> 16, 0xFF);
        assert!(((rgb >> 8) & 0xFF).abs_diff(0x80) <= 2 && (rgb & 0xFF).abs_diff(0x40) <= 2, "{rgb:06x}");
        // within white, as it was
        let Some(plain) = picked(true, [1.0, 0.5, 0.25], 0.0) else { return };
        assert_eq!(plain.stops, 0.0);
        assert_eq!(plain.rgb, lit.rgb);
        // a standard canvas paints the colour as it is
        let Some(standard) = picked(false, [1.0, 0.5, 0.25], 2.0) else { return };
        assert_eq!(standard.stops, 0.0);
        assert_eq!(standard.rgb, lit.rgb);
    }

    #[test]
    fn an_hdr_layer_file_holds_its_half_floats() {
        let dir = std::env::temp_dir().join(format!("procraft-hdr-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("layer.png");
        let half = pixels::halves([3.0, 1.5, 0.25, 1.0, 0.0, 0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 0.5]);
        write_half_png(&path, 3, 1, &half).unwrap();
        let reader = png::Decoder::new(std::io::BufReader::new(fs::File::open(&path).unwrap())).read_info().unwrap();
        assert_eq!(read_half_png(reader, 3, 1).unwrap(), half);
        fs::remove_dir_all(&dir).ok();
    }
}

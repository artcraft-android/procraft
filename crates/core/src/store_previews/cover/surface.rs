use std::sync::OnceLock;

use image::{GrayImage, RgbaImage, imageops};
use procraft_engine::zip::ZipReader;
use tiny_skia::{
    BlendMode, Color, FillRule, FilterQuality, IntSize, Mask, Paint, Path, PathBuilder, Pattern, Pixmap, PixmapPaint,
    Rect, SpreadMode, Stroke, Transform,
};

use super::super::{
    MOST_PICTURES, PREVIEW_WIDTH, blit, face_ready, folder_of, is_first_picture, is_picture, is_thumbnail, picture_of,
    words,
};
use super::ink::Mark;

pub(super) const W: f32 = 840.0;
pub(super) const H: f32 = 630.0;
const SCALE: f32 = PREVIEW_WIDTH as f32 / W;
pub(super) type Point = (f32, f32);
pub(super) type Bounds = (f32, f32, f32, f32);

fn color(rgb: u32, opacity: f32) -> Color {
    Color::from_rgba8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, (opacity * 255.0).round() as u8)
}

#[allow(clippy::expect_used, reason = "bounds are authored in code with positive dimensions")]
fn rect((x, y, w, h): Bounds) -> Rect {
    Rect::from_xywh(x, y, w, h).expect("the authored layout has positive bounds")
}

pub(super) fn rectangle(bounds: Bounds) -> Path {
    PathBuilder::from_rect(rect(bounds))
}

pub(super) fn polygon(points: &[Point]) -> Path {
    polyline(points, true)
}

#[allow(clippy::expect_used, reason = "every authored path supplies a nonempty list of points")]
pub(super) fn polyline(points: &[Point], close: bool) -> Path {
    let mut builder = PathBuilder::new();
    let (&(x, y), rest) = points.split_first().expect("the authored path has points");
    builder.move_to(x, y);
    for &(x, y) in rest {
        builder.line_to(x, y);
    }
    if close {
        builder.close();
    }
    builder.finish().expect("the authored path has points")
}

#[allow(clippy::expect_used, reason = "radii are authored in code with positive dimensions")]
pub(super) fn oval((x, y): Point, rx: f32, ry: f32) -> Path {
    PathBuilder::from_oval(rect((x - rx, y - ry, rx * 2.0, ry * 2.0))).expect("the authored oval has positive radii")
}

pub(super) fn rotate((x, y): Point, (cx, cy): Point, angle: f32) -> Point {
    let (sin, cos) = angle.to_radians().sin_cos();
    (cx + cos * x - sin * y, cy + sin * x + cos * y)
}

pub(super) fn curve(a: Point, b: Point, c: Point, d: Point) -> impl Iterator<Item = Point> {
    (0..=36).map(move |index| {
        let t = index as f32 / 36.0;
        let u = 1.0 - t;
        let weights = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
        [a, b, c, d].into_iter().zip(weights).fold((0.0, 0.0), |(x, y), (p, f)| (x + p.0 * f, y + p.1 * f))
    })
}

#[allow(clippy::expect_used, reason = "cover dimensions are fixed and nonzero")]
pub(super) fn pixmap(height: u32) -> Pixmap {
    Pixmap::new(PREVIEW_WIDTH, height).expect("fixed cover dimensions")
}

pub(super) struct CoverArt<'a> {
    pixels: Pixmap,
    title: Option<&'a str>,
    height: u32,
}

impl<'a> CoverArt<'a> {
    pub(super) fn new(title: Option<&'a str>, height: u32) -> Self {
        Self { pixels: pixmap(height), title, height }
    }

    pub(super) fn plate(&self) -> Self {
        Self::new(None, self.height)
    }

    fn vertical_scale(&self) -> f32 {
        self.height as f32 / H
    }

    fn transform(&self) -> Transform {
        Transform::from_scale(SCALE, self.vertical_scale())
    }

    pub(super) fn finish(self, substrate: Option<&Pixmap>) -> Vec<u8> {
        let Some(substrate) = substrate else { return self.pixels.take() };
        let mut printed = substrate.clone();
        // The design includes the colored stock so fibers continue through the background,
        // ink impressions, type, and shapes without a break.
        let ink = PixmapPaint { blend_mode: BlendMode::Multiply, ..PixmapPaint::default() };
        printed.draw_pixmap(0, 0, self.pixels.as_ref(), &ink, Transform::identity(), None);
        printed.take()
    }

    pub(super) fn titled(&self) -> bool {
        self.title.is_some()
    }

    pub(super) fn stock(&mut self, rgb: u32) {
        self.pixels.fill(color(rgb, 1.0));
    }

    pub(super) fn fill(&mut self, shape: &Path, rgb: u32, alpha: f32) {
        let mut paint = Paint::default();
        paint.set_color(color(rgb, alpha));
        self.pixels.fill_path(shape, &paint, FillRule::Winding, self.transform(), None);
    }

    pub(super) fn line(&mut self, shape: &Path, rgb: u32, width: f32, alpha: f32) {
        let mut paint = Paint::default();
        paint.set_color(color(rgb, alpha));
        let stroke = Stroke { width, line_cap: tiny_skia::LineCap::Round, ..Stroke::default() };
        self.pixels.stroke_path(shape, &paint, &stroke, self.transform(), None);
    }

    pub(super) fn shadow(&mut self, shape: &Path, offset: Point, opacity: f32) {
        let bounds = shape.bounds();
        let sigma = 4.2 * SCALE;
        let padding = (sigma * 3.0).ceil();
        let start = |edge: f32, shift: f32, scale: f32| ((edge + shift) * scale - padding).floor().max(0.0) as i32;
        let end = |edge: f32, shift: f32, limit: u32, scale: f32| {
            ((edge + shift) * scale + padding).ceil().min(limit as f32) as i32
        };
        let x = start(bounds.left(), offset.0, SCALE);
        let y = start(bounds.top(), offset.1, self.vertical_scale());
        let width = (end(bounds.right(), offset.0, PREVIEW_WIDTH, SCALE) - x).max(0) as u32;
        let height = (end(bounds.bottom(), offset.1, self.height, self.vertical_scale()) - y).max(0) as u32;
        let Some(mut mask) = Mask::new(width, height) else { return };
        let shifted =
            self.transform().post_translate(offset.0 * SCALE - x as f32, offset.1 * self.vertical_scale() - y as f32);
        mask.fill_path(shape, FillRule::Winding, true, shifted);
        let Some(mask) = GrayImage::from_raw(width, height, mask.data().to_vec()) else { return };
        let softened = imageops::fast_blur(&mask, sigma);
        let Some(mut shade) = Pixmap::new(width, height) else { return };
        for (pixel, alpha) in shade.data_mut().as_chunks_mut::<4>().0.iter_mut().zip(softened.as_raw()) {
            let a = f32::from(*alpha) * opacity;
            *pixel = [(a * 0.13).round() as u8, (a * 0.11).round() as u8, (a * 0.10).round() as u8, a.round() as u8];
        }
        self.pixels.draw_pixmap(x, y, shade.as_ref(), &PixmapPaint::default(), Transform::identity(), None);
    }

    /// Clips another cover layer in design coordinates, with optional offset registration.
    pub(super) fn layer(&mut self, layer: &Self, shape: &Path, offset: Point) {
        let Some(mut clip) = Mask::new(PREVIEW_WIDTH, self.height) else { return };
        clip.fill_path(shape, FillRule::Winding, true, self.transform());
        self.pixels.draw_pixmap(
            0,
            0,
            layer.pixels.as_ref(),
            &PixmapPaint::default(),
            Transform::from_translate(offset.0 * SCALE, offset.1 * self.vertical_scale()),
            Some(&clip),
        );
    }

    /// Transforms a rendered ink impression while keeping its proportions.
    pub(super) fn mark(&mut self, mark: Option<&Mark>, area: Bounds, angle: f32, rgb: u32, opacity: f32) {
        self.print(mark, area, angle, rgb, opacity, false);
    }

    pub(super) fn print(
        &mut self,
        mark: Option<&Mark>,
        (cx, cy, width, height): Bounds,
        angle: f32,
        rgb: u32,
        opacity: f32,
        flip: bool,
    ) {
        let Some(source) = mark.and_then(|mark| mark.mask(width * SCALE, height * self.vertical_scale())) else {
            return;
        };
        let (sw, sh) = (source.width() as f32, source.height() as f32);
        let scale = (width * SCALE / sw).min(height * self.vertical_scale() / sh);
        let mut ink = source.clone();
        let rgb = color(rgb, 1.0);
        for pixel in ink.data_mut().as_chunks_mut::<4>().0 {
            let alpha = f32::from(pixel[3]);
            *pixel = [
                (rgb.red() * alpha).round() as u8,
                (rgb.green() * alpha).round() as u8,
                (rgb.blue() * alpha).round() as u8,
                pixel[3],
            ];
        }
        let matrix = Transform::from_translate(-sw * 0.5, -sh * 0.5)
            .post_scale(if flip { -scale } else { scale }, scale)
            .post_rotate(angle)
            .post_translate(cx * SCALE, cy * self.vertical_scale());
        let paint = PixmapPaint { opacity, quality: FilterQuality::Bilinear, ..PixmapPaint::default() };
        self.pixels.draw_pixmap(0, 0, ink.as_ref(), &paint, matrix, None);
    }

    /// Samples a whole-sheet view or a closer detail. Position changes the crop, while folds
    /// and lighting change only the brightness of the source.
    pub(super) fn cut(&mut self, scan: &Scan, shape: &Path, crop: Crop, opacity: f32, shade: f32) {
        let bounds = shape.bounds();
        let (width, height) = (scan.image.width() as f32, scan.image.height() as f32);
        let ratio = self.vertical_scale() / SCALE;
        let scale = (bounds.width() / width).max(bounds.height() * ratio / height) * crop.zoom;
        let scale_y = scale / ratio;
        let x = bounds.left() + (bounds.width() - width * scale) * crop.focus.0;
        let y = bounds.top() + (bounds.height() - height * scale_y) * crop.focus.1;
        let paint = Paint {
            shader: Pattern::new(
                scan.image.as_ref(),
                SpreadMode::Pad,
                FilterQuality::Bilinear,
                opacity,
                Transform::from_scale(scale, scale_y).post_translate(x, y),
            ),
            ..Paint::default()
        };
        self.pixels.fill_path(shape, &paint, FillRule::Winding, self.transform(), None);
        if shade != 0.0 {
            self.fill(shape, if shade > 0.0 { 0xfff7e5 } else { 0x221a17 }, shade.abs());
        }
    }

    pub(super) fn title(&mut self, bounds: Bounds, size: f32, rgb: u32, bold: bool) {
        self.write_title(bounds, size, rgb, bold, false);
    }

    pub(super) fn scan_title(&mut self, bounds: Bounds, size: f32, rgb: u32, bold: bool) {
        self.write_title(bounds, size, rgb, bold, true);
    }

    fn write_title(&mut self, bounds: Bounds, size: f32, rgb: u32, bold: bool, scanned: bool) {
        if let Some(title) = self.title {
            self.write_words(title, bounds, size, rgb, bold, scanned);
        }
    }

    pub(super) fn label(&mut self, text: &str, bounds: Bounds, size: f32, rgb: u32) {
        self.write_words(text, bounds, size, rgb, false, false);
    }

    fn write_words(&mut self, title: &str, bounds: Bounds, size: f32, rgb: u32, bold: bool, scanned: bool) {
        if !face_ready() {
            return;
        }
        if bold {
            static BOLD: OnceLock<()> = OnceLock::new();
            BOLD.get_or_init(|| {
                const FACE: &[u8] = include_bytes!("../../../../../android/app/src/main/assets/fonts/Inter-Bold.ttf");
                crate::add_text_font(FACE.to_vec());
            });
        }
        let (x, top, width, height) = bounds;
        let colour = color(rgb, 1.0);
        let colour = [colour.red(), colour.green(), colour.blue()];
        let title = title.split_whitespace().collect::<Vec<_>>().join(" ");
        let text = |line: &str, size: f32| {
            let mut info = words(line, size, colour, 2.0, size * 0.86 + 2.0, false);
            if bold {
                info.style = "Bold".to_owned();
                info.tracking = -0.023;
            }
            info
        };
        let measure = |line: &str, size| crate::text_box(text(line, size)).get(2).copied().unwrap_or(0.0);
        let width = width * SCALE;
        let height = height * self.vertical_scale();
        let mut size = size * SCALE;
        let lines = loop {
            let mut lines = Vec::new();
            let mut line = String::new();
            for word in title.split_whitespace() {
                let next = if line.is_empty() { word.to_owned() } else { format!("{line} {word}") };
                if !line.is_empty() && measure(&next, size) > width {
                    lines.push(std::mem::take(&mut line));
                    line.push_str(word);
                } else {
                    line = next;
                }
            }
            lines.push(line);
            if (lines.len() as f32 * size * 1.12 <= height && lines.iter().all(|line| measure(line, size) <= width))
                || size <= 6.0
            {
                break lines;
            }
            size *= 0.90;
        };
        let Some(mut pixels) = RgbaImage::from_raw(PREVIEW_WIDTH, self.height, self.pixels.data().to_vec()) else {
            return;
        };
        let light = colour.iter().sum::<f32>() > 1.5;
        let material = (scanned && light).then(|| {
            let detail = imageops::grayscale(&pixels);
            let tone = imageops::fast_blur(&detail, 3.0);
            (detail, tone)
        });
        for (index, line) in lines.into_iter().enumerate() {
            let strip_w = (measure(&line, size).min(width).ceil() as u32).max(1) + 4;
            let strip_h = (size * 1.55).ceil() as u32 + 4;
            let mut draw = crate::draw_text(text(&line, size), strip_w, strip_h);
            let y = top * self.vertical_scale() + index as f32 * size * 1.12;
            if y + size > top * self.vertical_scale() + height + 2.0 {
                break;
            }
            let origin = ((x * SCALE).round() as i32 - 2, y.round() as i32 - 2);
            if scanned {
                multiply_scan_ink(&mut draw, strip_w, origin, &pixels, material.as_ref());
            }
            blit(&mut pixels, strip_w, strip_h, &draw, origin.0, origin.1);
        }
        self.pixels.data_mut().copy_from_slice(pixels.as_raw());
    }
}

/// Dark ink multiplies directly on the scan. Pale lettering multiplies with the local texture
/// after removing the stock's base tone, which keeps it legible over dark paper.
fn multiply_scan_ink(
    ink: &mut [u8],
    width: u32,
    origin: (i32, i32),
    paper: &RgbaImage,
    material: Option<&(GrayImage, GrayImage)>,
) {
    for (i, [r, g, b, a]) in ink.as_chunks_mut::<4>().0.iter_mut().enumerate() {
        if *a == 0 {
            continue;
        }
        let x = origin.0 + (i % width as usize) as i32;
        let y = origin.1 + (i / width as usize) as i32;
        if x < 0 || y < 0 || x as u32 >= paper.width() || y as u32 >= paper.height() {
            continue;
        }
        let (x, y) = (x as u32, y as u32);
        let factors = if let Some((detail, tone)) = material {
            let sampled = f32::from(detail.get_pixel(x, y).0[0]);
            let mean = f32::from(tone.get_pixel(x, y).0[0]);
            [(1.0 + (sampled - mean) * 0.018).clamp(0.65, 1.0); 3]
        } else {
            let [red, green, blue, _] = paper.get_pixel(x, y).0;
            [red, green, blue].map(|channel| f32::from(channel) / 255.0)
        };
        for (channel, factor) in [r, g, b].into_iter().zip(factors) {
            *channel = (f32::from(*channel) * factor).round() as u8;
        }
    }
}

pub(super) struct Crop {
    zoom: f32,
    focus: Point,
}

impl Crop {
    pub(super) const WHOLE: Self = Self { zoom: 1.0, focus: (0.5, 0.5) };

    pub(super) fn detail(index: usize) -> Self {
        let points = [(0.28, 0.33), (0.72, 0.52), (0.48, 0.76), (0.34, 0.59), (0.74, 0.28)];
        Self {
            zoom: 1.2 + (index % 4) as f32 * 0.29,
            focus: points.get(index % points.len()).copied().unwrap_or((0.5, 0.5)),
        }
    }
}

pub(super) struct Scan {
    image: Pixmap,
    average: [f32; 3],
}

impl Scan {
    pub(super) fn of(image: RgbaImage) -> Option<Self> {
        let (width, height) = image.dimensions();
        let scale = (1800.0 / width.max(height) as f32).min(1.0);
        let image = if scale < 1.0 {
            imageops::resize(
                &image,
                (width as f32 * scale).round().max(1.0) as u32,
                (height as f32 * scale).round().max(1.0) as u32,
                imageops::FilterType::Triangle,
            )
        } else {
            image
        };
        let sample = image.pixels().step_by(117);
        let (mut average, mut count) = ([0.0; 3], 0.0);
        for [red, green, blue, _] in sample.map(|pixel| pixel.0).filter(|pixel| pixel[3] >= 64) {
            for (sum, channel) in average.iter_mut().zip([red, green, blue]) {
                *sum += f32::from(channel);
            }
            count += 1.0;
        }
        if count == 0.0 {
            return None;
        }
        average.iter_mut().for_each(|value| *value /= count);
        let size = IntSize::from_wh(image.width(), image.height())?;
        let mut bytes = image.into_raw();
        for [r, g, b, a] in bytes.as_chunks_mut::<4>().0 {
            let alpha = u16::from(*a);
            for channel in [r, g, b] {
                *channel = ((u16::from(*channel) * alpha + 127) / 255) as u8;
            }
        }
        Some(Self { image: Pixmap::from_vec(bytes, size)?, average })
    }
}

/// Read the original color layer when present. Thumbnail-only archives still have a source, and
/// a bad entry in one asset does not prevent using the next asset's scan.
pub(super) fn pack_scans(zip: &ZipReader<'_>) -> Vec<Scan> {
    use std::collections::BTreeMap;
    let mut assets = BTreeMap::<_, Vec<_>>::new();
    for name in zip.names().filter(|name| is_picture(name)) {
        let rank = if is_first_picture(name) {
            0
        } else if is_thumbnail(name) {
            1
        } else {
            2
        };
        assets.entry(folder_of(name)).or_default().push((rank, name));
    }
    assets
        .into_values()
        .filter_map(|mut choices| {
            choices.sort_unstable();
            choices.into_iter().find_map(|(_, name)| picture_of(zip, name).and_then(Scan::of))
        })
        .take(MOST_PICTURES)
        .collect()
}

pub(super) fn varied(scans: &[Scan]) -> bool {
    let Some(first) = scans.first() else { return false };
    scans
        .iter()
        .skip(1)
        .any(|other| first.average.iter().zip(other.average).map(|(a, b)| (a - b).abs()).sum::<f32>() > 38.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_solid_ink_shape_multiplies_the_texture_beneath_it() {
        let mut paper = pixmap(super::super::PREVIEW_HEIGHT);
        paper.fill(color(0xf0f0f0, 1.0));
        let mut shade = Paint::default();
        shade.set_color(color(0xa0a0a0, 1.0));
        paper.fill_rect(
            rect((0.0, 0.0, PREVIEW_WIDTH as f32 * 0.5, super::super::PREVIEW_HEIGHT as f32)),
            &shade,
            Transform::identity(),
            None,
        );
        let mut art = CoverArt::new(None, super::super::PREVIEW_HEIGHT);
        art.stock(0x1c253d);
        art.fill(&rectangle((210.0, 157.5, 420.0, 315.0)), 0x804020, 1.0);
        let printed =
            RgbaImage::from_raw(PREVIEW_WIDTH, super::super::PREVIEW_HEIGHT, art.finish(Some(&paper))).unwrap();
        assert_eq!(printed.get_pixel(450, 450).0, [80, 40, 20, 255]);
        assert_eq!(printed.get_pixel(750, 450).0, [120, 60, 30, 255]);
    }
}

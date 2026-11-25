//! Package artwork made from the thumbnails already stored in native share files.
use std::collections::HashSet;
use std::path::Path;

use image::{Rgba, RgbaImage, imageops};
use procraft_engine::zip::ZipReader;
use serde::Deserialize;

use super::{
    FOLDER_MARK, PREVIEW_HEIGHT as HEIGHT, PREVIEW_WIDTH as WIDTH, blit, face_ready, words, words_width, write_words,
};

const PER_PAGE: usize = 9;
const MAX_ITEMS: usize = PER_PAGE * crate::store::STORE_CONTENTS_LIMIT;
const THUMB_BYTES: usize = 8 << 20;
const NESTED_BRUSH_BYTES: usize = 128 << 20;
const THUMBNAIL: &str = "QuickLook/Thumbnail.png";

#[derive(Clone, Copy)]
pub(crate) enum ShareKind {
    Brush,
    BrushSet,
    BrushLibrary,
    Asset,
    AssetSet,
    AssetLibrary,
}

impl ShareKind {
    pub(crate) fn extension(self) -> &'static str {
        match self {
            Self::Brush => "brush",
            Self::BrushSet => "brushset",
            Self::BrushLibrary => "brushlibrary",
            Self::Asset => "asset",
            Self::AssetSet => "assetset",
            Self::AssetLibrary => "assetlibrary",
        }
    }

    pub(crate) fn listing_kind(self) -> &'static str {
        if self.is_brush() { self.extension() } else { "asset pack" }
    }

    pub(crate) fn is_brush(self) -> bool {
        matches!(self, Self::Brush | Self::BrushSet | Self::BrushLibrary)
    }

    pub(crate) fn from_path(path: &Path) -> Option<Self> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        [Self::Brush, Self::BrushSet, Self::BrushLibrary, Self::Asset, Self::AssetSet, Self::AssetLibrary]
            .into_iter()
            .find(|kind| kind.extension() == ext)
    }
}

struct Entry {
    name: String,
    group: String,
    thumbnail: String,
    nested: bool,
}

struct Tile {
    name: String,
    group: String,
    image: Option<RgbaImage>,
}

#[derive(Deserialize)]
struct Manifest {
    format: String,
    version: u32,
    kind: String,
    name: String,
    libraries: Vec<Library>,
}

#[derive(Deserialize)]
struct Library {
    folders: Vec<Folder>,
}

#[derive(Deserialize)]
struct Folder {
    name: String,
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    id: String,
    name: String,
}

pub(crate) struct SharedFile {
    pub kind: ShareKind,
    pub name: String,
    entries: Vec<Entry>,
    zip: ZipReader<'static>,
}

impl SharedFile {
    pub(crate) fn open(path: &Path, display_name: Option<&str>) -> Result<Self, String> {
        let invalid =
            || "Choose a ProCraft brush, brush set, brush library, asset, asset set, or asset library.".to_owned();
        let kind = ShareKind::from_path(path).ok_or_else(invalid)?;
        let zip = ZipReader::open(path).ok_or_else(invalid)?;
        let stem = display_name
            .map(Path::new)
            .unwrap_or(path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(str::trim)
            .filter(|stem| !stem.is_empty())
            .unwrap_or("Untitled");
        let (name, entries) = match kind {
            ShareKind::Brush => {
                let name = brush_name(&zip, "").ok_or_else(invalid)?;
                (
                    name.clone(),
                    vec![Entry { name, group: String::new(), thumbnail: THUMBNAIL.to_owned(), nested: false }],
                )
            }
            ShareKind::BrushSet => {
                let (name, folders) = (zip.size("brushset.plist").is_some_and(|size| size <= 1 << 20))
                    .then(|| procraft_engine::brushfile::set_folders(&zip))
                    .flatten()
                    .ok_or_else(invalid)?;
                check_size(folders.len())?;
                let entries = folders
                    .into_iter()
                    .enumerate()
                    .filter_map(|(i, folder)| {
                        let prefix = format!("{folder}/");
                        let name = brush_name(&zip, &prefix).unwrap_or_else(|| format!("Brush {}", i + 1));
                        (!name.starts_with(FOLDER_MARK)).then(|| Entry {
                            name,
                            group: String::new(),
                            thumbnail: format!("{prefix}{THUMBNAIL}"),
                            nested: false,
                        })
                    })
                    .collect();
                (name, entries)
            }
            ShareKind::BrushLibrary => {
                let groups = procraft_engine::brushlibrary::brush_paths(&zip).ok_or_else(invalid)?;
                let entries = groups
                    .into_iter()
                    .flat_map(|(group, paths)| {
                        paths.into_iter().filter_map(move |thumbnail| {
                            let name = Path::new(&thumbnail).file_stem().and_then(|s| s.to_str()).unwrap_or("Brush");
                            if name.starts_with(FOLDER_MARK) {
                                return None;
                            }
                            Some(Entry { name: name.to_owned(), group: group.clone(), thumbnail, nested: true })
                        })
                    })
                    .collect();
                (stem.to_owned(), entries)
            }
            _ => assets(&zip, kind).ok_or_else(invalid)?,
        };
        if entries.is_empty() {
            return Err("The file doesn't list any items to preview.".to_owned());
        }
        check_size(entries.len())?;
        Ok(Self { kind, name: clean(&name), entries, zip })
    }

    pub(crate) fn pages<'a>(&'a self, title: &'a str) -> impl Iterator<Item = RgbaImage> + 'a {
        let total = self.entries.len().div_ceil(PER_PAGE);
        let count = count_label(self.kind, self.entries.len());
        self.entries.chunks(PER_PAGE).enumerate().map(move |(page, entries)| {
            let tiles: Vec<_> = entries.iter().map(|entry| self.tile(entry)).collect();
            contents_page(&tiles, title, &count, (page + 1, total))
        })
    }

    fn tile(&self, entry: &Entry) -> Tile {
        let picture = if entry.nested {
            limited_entry(&self.zip, &entry.thumbnail, NESTED_BRUSH_BYTES)
                .and_then(|bytes| picture(&ZipReader::new(&bytes)?, THUMBNAIL))
        } else {
            picture(&self.zip, &entry.thumbnail)
        };
        let image = picture
            .and_then(crop_visible)
            .map(|image| if self.kind.is_brush() { display_brush_mask(image) } else { image });
        Tile { name: clean(&entry.name), group: clean(&entry.group), image }
    }
}

fn check_size(count: usize) -> Result<(), String> {
    if count <= MAX_ITEMS {
        Ok(())
    } else {
        Err(format!("A Store Package can show up to {MAX_ITEMS} items. Choose a smaller collection."))
    }
}

fn assets(zip: &ZipReader<'_>, kind: ShareKind) -> Option<(String, Vec<Entry>)> {
    let bytes = limited_entry(zip, "assets.json", 1 << 20)?;
    let manifest: Manifest = serde_json::from_slice(&bytes).ok()?;
    let expected = match kind {
        ShareKind::Asset => "asset",
        ShareKind::AssetSet => "set",
        ShareKind::AssetLibrary => "library",
        _ => return None,
    };
    if manifest.format != "procraft-assets" || manifest.version != 1 || manifest.kind != expected {
        return None;
    }
    let mut seen = HashSet::new();
    let mut entries = Vec::new();
    for library in manifest.libraries {
        for folder in library.folders {
            for asset in folder.assets {
                let safe_id = !asset.id.is_empty()
                    && asset.id.len() <= 128
                    && asset.id.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'));
                if !safe_id || !seen.insert(asset.id.clone()) {
                    continue;
                }
                let group = if matches!(kind, ShareKind::AssetLibrary) { folder.name.clone() } else { String::new() };
                entries.push(Entry {
                    name: asset.name,
                    group,
                    thumbnail: format!("items/{}/thumbnail.png", asset.id),
                    nested: false,
                });
            }
        }
    }
    if matches!(kind, ShareKind::Asset) && entries.len() != 1 {
        return None;
    }
    Some((manifest.name, entries))
}

fn brush_name(zip: &ZipReader<'_>, prefix: &str) -> Option<String> {
    let bytes = limited_entry(zip, &format!("{prefix}Brush.archive"), 1 << 20)?;
    let archive = procraft_engine::keyed::Archive::read(&bytes)?;
    let root = archive.root()?;
    matches!(root.class(), Some("SilicaBrush" | "ValkyrieBrush")).then_some(())?;
    Some(
        root.raw("name")
            .and_then(procraft_engine::plist::Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| "Untitled Brush".to_owned()),
    )
}

fn limited_entry(zip: &ZipReader<'_>, path: &str, cap: usize) -> Option<Vec<u8>> {
    (zip.size(path)? <= cap).then(|| zip.read(path)).flatten()
}

fn picture(zip: &ZipReader<'_>, path: &str) -> Option<RgbaImage> {
    crate::store_picture(&limited_entry(zip, path, THUMB_BYTES)?, 800)
}

fn clean(name: &str) -> String {
    name.chars().filter(|c| !c.is_control()).take(160).collect::<String>().trim().to_owned()
}

fn crop_visible(image: RgbaImage) -> Option<RgbaImage> {
    let (w, h) = image.dimensions();
    let (mut left, mut top, mut right, mut bottom) = (w, h, 0, 0);
    for (x, y, _) in image.enumerate_pixels().filter(|(_, _, pixel)| pixel.0[3] >= 12) {
        left = left.min(x);
        right = right.max(x + 1);
        top = top.min(y);
        bottom = bottom.max(y + 1);
    }
    (left < right && top < bottom).then(|| imageops::crop_imm(&image, left, top, right - left, bottom - top).to_image())
}

const INK: [f32; 3] = [0.10, 0.16, 0.14];
const MUTED: [f32; 3] = [0.31, 0.38, 0.35];
const PAPER: [u8; 4] = [245, 243, 233, 255];
const MARGIN: u32 = 64;

fn count_label(kind: ShareKind, count: usize) -> String {
    let noun = match (kind.is_brush(), count) {
        (true, 1) => "brush",
        (true, _) => "brushes",
        (false, 1) => "asset",
        (false, _) => "assets",
    };
    format!("{count} {noun}")
}

fn paper_stock() -> RgbaImage {
    RgbaImage::from_fn(WIDTH, HEIGHT, |x, y| {
        let grit = (x.wrapping_mul(1_934_963) ^ y.wrapping_mul(83_492_791)).wrapping_mul(2_654_435_761);
        let noise = ((grit >> 29) as i16 - 3) / 2;
        let [r, g, b, a] = PAPER;
        Rgba([(i16::from(r) + noise) as u8, (i16::from(g) + noise) as u8, (i16::from(b) + noise) as u8, a])
    })
}

/// Most Procreate Quick Looks store a white mask for the brush UI to tint. Preserve its
/// edges, opacity and texture and print that mask as ink on the light cover stock.
fn display_brush_mask(mut image: RgbaImage) -> RgbaImage {
    let sample = image.pixels().step_by(7).filter(|pixel| pixel.0[3] >= 100);
    let (mut pale, mut other) = (0, 0);
    for pixel in sample {
        let [r, g, b, _] = pixel.0;
        let low = r.min(g).min(b);
        let high = r.max(g).max(b);
        if low >= 197 && high - low <= 12 {
            pale += 1;
        } else {
            other += 1;
        }
    }
    if pale > other * 4 {
        for pixel in image.pixels_mut() {
            let [r, g, b, a] = pixel.0;
            let low = r.min(g).min(b);
            let high = r.max(g).max(b);
            if low >= 160 && high - low <= 20 {
                pixel.0 = [26, 58, 48, a];
            }
        }
    }
    image
}

fn matte(image: &RgbaImage) -> Option<[u8; 4]> {
    let mut pale = 0;
    let mut dark = 0;
    let mut clear = 0;
    for [r, g, b, a] in image.as_raw().as_chunks::<4>().0.iter().step_by(7).copied() {
        if a < 100 {
            clear += 1;
        } else if u16::from(r) + u16::from(g) + u16::from(b) > 635 {
            pale += 1;
        } else {
            dark += 1;
        }
    }
    (clear > 0).then_some(if pale > dark * 3 { [152, 170, 152, 255] } else { [244, 238, 219, 255] })
}

fn fitted(image: &RgbaImage, width: u32, height: u32) -> RgbaImage {
    let scale = (f64::from(width) / f64::from(image.width())).min(f64::from(height) / f64::from(image.height()));
    let w = (f64::from(image.width()) * scale).round().max(1.0) as u32;
    let h = (f64::from(image.height()) * scale).round().max(1.0) as u32;
    imageops::resize(image, w, h, imageops::FilterType::Lanczos3)
}

fn contents_page(tiles: &[Tile], title: &str, count: &str, (page, pages): (usize, usize)) -> RgbaImage {
    let mut out = paper_stock();
    headline(&mut out, title, 53.0, INK, MARGIN as f32, 38.0, 860.0);
    let page = format!("{page} / {pages}");
    let page_width = words_width(&page, 24.0, true);
    write_words(&mut out, &page, 24.0, MUTED, (WIDTH - MARGIN) as f32 - page_width, 64.0, true, 170.0);
    bar(&mut out, MARGIN, 112, WIDTH - 2 * MARGIN, 2, [100, 127, 108, 255]);
    bar(&mut out, MARGIN, 826, WIDTH - 2 * MARGIN, 1, [161, 170, 153, 255]);
    write_words(&mut out, count, 20.0, MUTED, MARGIN as f32, 846.0, true, 350.0);
    let columns = tiles.len().clamp(1, 3) as u32;
    let gap = 31;
    let cell = (WIDTH - 2 * MARGIN - (columns - 1) * gap) / columns;
    let rows = tiles.len().div_ceil(columns as usize) as u32;
    let max_row = if tiles.len() == 1 { 676 } else { 420 };
    let row_height = ((676 - 16 * (rows - 1)) / rows).min(max_row);
    for (row, entries) in tiles.chunks(columns as usize).enumerate() {
        for (column, tile) in entries.iter().enumerate() {
            let x = MARGIN + column as u32 * (cell + gap);
            let y = 132 + row as u32 * (row_height + 16);
            let art_h = row_height - 58;
            if let Some(image) = &tile.image {
                let shown = fitted(image, cell - 14, art_h - 12);
                let ox = x + (cell - shown.width()) / 2;
                let oy = y + (art_h - shown.height()) / 2;
                if let Some(mat) = matte(image).filter(|rgb| rgb[0] < 200) {
                    bar(&mut out, ox - 6, oy - 6, shown.width() + 12, shown.height() + 12, mat);
                }
                imageops::overlay(&mut out, &shown, i64::from(ox), i64::from(oy));
            } else {
                write_words(
                    &mut out,
                    "Preview unavailable",
                    19.0,
                    MUTED,
                    x as f32,
                    (y + art_h / 2) as f32,
                    false,
                    cell as f32,
                );
            }
            let cap = y + art_h;
            bar(&mut out, x, cap, cell, 1, [166, 175, 158, 255]);
            headline(&mut out, &tile.name, 24.0, INK, x as f32, (cap + 8) as f32, cell as f32);
            write_words(&mut out, &tile.group, 15.0, MUTED, x as f32, (cap + 36) as f32, true, cell as f32);
        }
    }
    out
}

fn bar(out: &mut RgbaImage, x: u32, y: u32, width: u32, height: u32, color: [u8; 4]) {
    imageops::replace(out, &RgbaImage::from_pixel(width, height, Rgba(color)), i64::from(x), i64::from(y));
}

fn headline(out: &mut RgbaImage, title: &str, size: f32, color: [f32; 3], x: f32, y: f32, most: f32) {
    use std::sync::OnceLock;
    if title.is_empty() || !face_ready() {
        return;
    }
    static BOLD: OnceLock<()> = OnceLock::new();
    BOLD.get_or_init(|| {
        crate::add_text_font(include_bytes!("../../../../android/app/src/main/assets/fonts/Inter-Bold.ttf").to_vec());
    });
    let style = |title: &str, size: f32| {
        let mut info = words(title, size, color, 2.0, size * 0.86 + 2.0, false);
        info.style = "Bold".to_owned();
        info.tracking = -0.033;
        info
    };
    let measured = |text: &str, size: f32| crate::text_box(style(text, size)).get(2).copied().unwrap_or_default();
    let minimum = size * 0.69;
    let mut size = size;
    let mut title = title.to_owned();
    let mut width = measured(&title, size);
    while width > most && size > minimum {
        size *= 0.90;
        width = measured(&title, size);
    }
    if width > most {
        while !title.is_empty() && measured(&format!("{title}..."), size) > most {
            title.pop();
        }
        title.push_str("...");
        width = measured(&title, size);
    }
    let w = width.ceil() as u32 + 4;
    let h = (size * 1.60).ceil() as u32 + 4;
    let drawn = crate::draw_text(style(&title, size), w, h);
    blit(out, w, h, &drawn, x.round() as i32 - 2, y.round() as i32 - 2);
}

#[cfg(test)]
mod tests {
    use super::*;
    use procraft_engine::brush::Brush;
    use procraft_engine::brushfile::Pictures;
    use procraft_engine::zip::ZipWriter;
    use serde_json::json;

    fn png(picture: &RgbaImage) -> Vec<u8> {
        crate::project::rgba_png(picture.width(), picture.height(), picture.as_raw())
    }

    fn dump_folder() -> Option<std::path::PathBuf> {
        let folder = std::path::PathBuf::from(std::env::var_os("PROCRAFT_CONTENTS_DUMP")?);
        std::fs::create_dir_all(&folder).unwrap();
        Some(folder)
    }

    #[test]
    fn a_single_asset_fills_a_contents_page_using_its_composite() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dialogue.asset");
        let mut zip = ZipWriter::new();
        zip.add(
            "assets.json",
            &serde_json::to_vec(&json!({
                "format": "procraft-assets", "version": 1, "kind": "asset", "name": "Dialogue Bubble",
                "libraries": [{"name": "Comic tools", "folders": [{"name": "Balloons",
                    "assets": [{"name": "Dialogue Bubble", "id": "bubble", "width": 480, "height": 480}]}]}],
            }))
            .unwrap(),
        );
        let mut vector = tiny_skia::PathBuilder::new();
        vector.move_to(414.0, 341.0);
        vector.cubic_to(170.0, 337.0, 82.0, 271.0, 82.0, 196.0);
        vector.cubic_to(82.0, 91.0, 225.0, 31.0, 447.0, 31.0);
        vector.cubic_to(680.0, 31.0, 820.0, 103.0, 820.0, 194.0);
        vector.cubic_to(820.0, 273.0, 720.0, 329.0, 557.0, 341.0);
        vector.quad_to(493.0, 428.0, 385.0, 463.0);
        vector.quad_to(420.0, 409.0, 414.0, 341.0);
        vector.close();
        let path_data = vector.finish().unwrap();
        let mut canvas = tiny_skia::Pixmap::new(960, 960).unwrap();
        let mut paint = tiny_skia::Paint::default();
        paint.set_color_rgba8(254, 253, 249, 255);
        canvas.fill_path(&path_data, &paint, tiny_skia::FillRule::Winding, tiny_skia::Transform::identity(), None);
        paint.set_color_rgba8(39, 56, 54, 255);
        let outline = tiny_skia::Stroke { width: 15.0, line_join: tiny_skia::LineJoin::Round, ..Default::default() };
        canvas.stroke_path(&path_data, &paint, &outline, tiny_skia::Transform::identity(), None);
        let bubble = RgbaImage::from_raw(960, 960, canvas.take()).unwrap();
        zip.add("items/bubble/thumbnail.png", &png(&bubble));
        zip.add("items/bubble/layers/1.png", &png(&RgbaImage::from_pixel(16, 16, Rgba([255, 0, 0, 255]))));
        zip.add("items/bubble/artwork.json", br#"{"layers":[{"blend":"Multiply"},{"opacity":0.5}]}"#);
        std::fs::write(&path, zip.finish()).unwrap();
        let file = SharedFile::open(&path, None).unwrap();
        let mut pages = file.pages(&file.name);
        let picture = pages.next().unwrap();
        assert!(pages.next().is_none());
        let dark: Vec<_> = picture.enumerate_pixels().filter(|(_, _, p)| p.0 == [39, 56, 54, 255]).collect();
        assert!(dark.iter().map(|(x, _, _)| x).min().unwrap() < &290);
        assert!(dark.iter().map(|(x, _, _)| x).max().unwrap() > &910);
        assert!(picture.pixels().all(|pixel| pixel.0 != [255, 0, 0, 255]));
        if let Some(folder) = dump_folder() {
            picture.save(folder.join("dialogue-bubble.png")).unwrap();
        }
    }

    #[test]
    fn the_asset_library_follows_manifest_order_and_keeps_items_with_missing_previews() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("props.assetlibrary");
        let mut zip = ZipWriter::new();
        zip.add(
            "assets.json",
            &serde_json::to_vec(&json!({
                "format": "procraft-assets", "version": 1, "kind": "library", "name": "Studio Props",
                "libraries": [{"name": "Studio Props", "folders": [
                    {"name": "Comic Panels", "assets": [{"name": "First", "id": "z"}, {"name": "Second", "id": "a"}]},
                    {"name": "Scrapbook", "assets": [{"name": "Third", "id": "b"}, {"name": "Duplicate", "id": "z"}]}
                ]}],
            }))
            .unwrap(),
        );
        let sample = png(&RgbaImage::from_pixel(8, 6, Rgba([84, 153, 165, 255])));
        zip.add("items/a/thumbnail.png", &sample);
        zip.add("items/z/thumbnail.png", &sample);
        std::fs::write(&path, zip.finish()).unwrap();
        let file = SharedFile::open(&path, None).unwrap();
        assert_eq!(
            file.entries.iter().map(|entry| entry.name.as_str()).collect::<Vec<_>>(),
            ["First", "Second", "Third"]
        );
        assert_eq!(file.entries[2].group, "Scrapbook");
        assert!(file.tile(&file.entries[2]).image.is_none());
        assert_eq!(file.pages(&file.name).count(), 1);

        if let (Some(folder), Some(paper)) = (dump_folder(), std::env::var_os("PROCRAFT_PAPER_SAMPLE")) {
            let paper = SharedFile::open(Path::new(&paper), None).unwrap();
            paper.pages(&paper.name).next().unwrap().save(folder.join("paper-contents.png")).unwrap();
        }
    }

    #[test]
    fn fifty_brushes_fill_six_pages_using_only_their_quick_looks() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("fifty.brushlibrary");
        let heads = [("Dry Media", None), ("Wet Media", None)];
        let mut writer = procraft_engine::brushlibrary::LibraryWriter::begin(
            std::fs::File::create(&path).unwrap(),
            &heads,
            [None; 3],
            |i| format!("{i:032x}"),
            0,
        );
        for index in 0..50u8 {
            let mut pixels = RgbaImage::new(330, 142);
            for y in 10..132 {
                for x in 14..316 {
                    let centre = 69.0 + ((f64::from(x) + f64::from(index) * 4.0) * 0.018).sin() * 27.0;
                    let half = (f64::from(330 - x) * 0.07 + f64::from(index % 6)).max(4.0);
                    let texture = (x * 37 + y * 79 + u32::from(index)) % 21;
                    if (f64::from(y) - centre).abs() < half && texture > u32::from(index % 5) {
                        pixels.put_pixel(x, y, Rgba([35 + index * 2, 65 + index, 104, 225]));
                    }
                }
            }
            let preview = png(&pixels);
            let brush = Brush {
                name: format!("{} {:02}", if index < 25 { "Dry Stroke" } else { "Wet Stroke" }, index + 1),
                ..Brush::round_pen()
            };
            writer.add(
                usize::from(index >= 25),
                &brush,
                "",
                &Pictures {
                    thumbnail_png: &preview,
                    shape_png: &[],
                    grain_png: None,
                    metallic_png: None,
                    roughness_png: None,
                    author_png: None,
                    signature_png: None,
                    secondary: None,
                },
            );
        }
        writer.finish().unwrap();
        let file = SharedFile::open(&path, Some("Studio Brush Library.brushlibrary")).unwrap();
        assert_eq!(file.name, "Studio Brush Library");
        assert_eq!(file.entries.len(), 50);
        assert_eq!(file.entries[0].name, "Dry Stroke 01");
        assert_eq!(file.entries[49].name, "Wet Stroke 50");
        assert!(file.entries.iter().all(|entry| file.tile(entry).image.is_some()));
        let pages: Vec<_> = file.pages(&file.name).collect();
        assert_eq!(pages.len(), 6);
        assert_ne!(pages.first(), pages.last());
        if let (Some(folder), Some(library)) = (dump_folder(), std::env::var_os("PROCRAFT_BRUSH_LIBRARY_SAMPLE")) {
            let file = SharedFile::open(Path::new(&library), None).unwrap();
            assert_eq!(file.entries.len(), 50);
            assert!(file.entries.iter().all(|entry| file.tile(entry).image.is_some()));
            let mut pages = 0;
            for (index, page) in file.pages(&file.name).enumerate() {
                page.save(folder.join(format!("brush-library-page-{}.png", index + 1))).unwrap();
                pages += 1;
            }
            assert_eq!(pages, 6);
        }
    }
}

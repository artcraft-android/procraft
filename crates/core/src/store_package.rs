//! The local Store Package export shared by desktop and Android.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use image::RgbaImage;

use crate::store_previews::quick_look::{ShareKind, SharedFile};
use crate::{CanvasSession, CoverTemplate, StoreCover, StorePreview, store};

/// The listing fields a maker enters on either device.
#[derive(uniffi::Record)]
pub struct StorePackageDetails {
    pub name: String,
    pub author: String,
    pub description: String,
    pub category: String,
    pub licence: String,
    pub tags: String,
}

/// A prepared cover and the templates it supports, used by the Android picker.
#[derive(uniffi::Object)]
pub struct StorePackageCover(StoreCover);

#[uniffi::export]
impl StorePackageCover {
    pub fn templates(&self) -> Vec<String> {
        self.0.templates().into_iter().map(|template| template.name().to_owned()).collect()
    }

    pub fn recommendation(&self) -> Option<String> {
        self.0.recommended().map(|template| template.name().to_owned())
    }

    /// PNG pixels ready to show or write into the package.
    pub fn render_png(&self, template: Option<String>, title: Option<String>) -> Vec<u8> {
        self.png(template, title, false)
    }

    pub fn render_square_png(&self, template: Option<String>, title: Option<String>) -> Vec<u8> {
        self.png(template, title, true)
    }
}

impl StorePackageCover {
    fn png(&self, template: Option<String>, title: Option<String>, square: bool) -> Vec<u8> {
        let template = template
            .and_then(|name| self.0.templates().into_iter().find(|template: &CoverTemplate| template.name() == name));
        let preview = if square {
            self.0.render_square(template, title.as_deref())
        } else {
            self.0.render(template, title.as_deref())
        };
        let mut png = Vec::new();
        if let Some(picture) = pixels(preview) {
            let _ = png_to(&mut std::io::Cursor::new(&mut png), &picture);
        }
        png
    }
}

#[uniffi::export]
pub fn open_asset_package_cover(path: String) -> Option<Arc<StorePackageCover>> {
    crate::asset_pack_store_cover(Path::new(&path)).map(StorePackageCover).map(Arc::new)
}

/// Identity for the package picker. The original display name matters when Android stages a file.
#[derive(uniffi::Record)]
pub struct StoreSharedFile {
    pub name: String,
    pub brushes: bool,
}

#[uniffi::export]
pub fn store_shared_file(path: String, display_name: Option<String>) -> Option<StoreSharedFile> {
    let file = SharedFile::open(Path::new(&path), display_name.as_deref()).ok()?;
    Some(StoreSharedFile { name: file.name, brushes: file.kind.is_brush() })
}

/// The exact artwork choices made in the composer.
#[derive(uniffi::Record)]
pub struct StorePackageImageChoices {
    pub cover_png: Vec<u8>,
    pub icon_png: Vec<u8>,
    pub removed_pages: Vec<u32>,
    pub maker: Vec<String>,
}

#[derive(Default)]
pub struct StorePackageArtwork<'a> {
    pub cover: Option<&'a RgbaImage>,
    pub icon: Option<&'a RgbaImage>,
    pub removed_pages: &'a [u32],
    pub maker: &'a [RgbaImage],
}

#[uniffi::export]
impl CanvasSession {
    pub fn prepare_store_package_cover(&self, library: u32, set: u32, name: String) -> Option<Arc<StorePackageCover>> {
        self.brush_set_store_cover(library, set, &name).map(StorePackageCover).map(Arc::new)
    }

    /// Writes the finished package into a new folder and returns its path. The PNG is the one
    /// the maker previewed, and additional images follow the generated contents pages.
    pub fn export_store_package(
        &self,
        folder: String,
        library: u32,
        set: u32,
        asset: Option<String>,
        details: StorePackageDetails,
        cover_png: Vec<u8>,
        maker: Vec<String>,
    ) -> Result<String, crate::MobileError> {
        self.export_store_package_artwork(
            folder,
            library,
            set,
            asset,
            details,
            StorePackageImageChoices { cover_png, icon_png: Vec::new(), removed_pages: Vec::new(), maker },
        )
    }

    /// Thumbnails of the exact automatic pages that are eligible to be included on export.
    pub fn store_package_gallery(
        &self,
        library: u32,
        set: u32,
        asset: Option<String>,
        title: String,
    ) -> Result<Vec<StorePreview>, crate::MobileError> {
        store_package_gallery(self, library, set, asset.as_deref().map(Path::new), &title)
            .map_err(|error| crate::project::file_error("Preparing the Store gallery", error))
    }

    pub fn export_store_package_artwork(
        &self,
        folder: String,
        library: u32,
        set: u32,
        asset: Option<String>,
        details: StorePackageDetails,
        choices: StorePackageImageChoices,
    ) -> Result<String, crate::MobileError> {
        let optional = |bytes: &[u8], label| {
            if bytes.is_empty() {
                return Ok(None);
            }
            store::store_picture(bytes, 1600)
                .map(Some)
                .ok_or_else(|| crate::project::file_error(label, "invalid picture"))
        };
        let cover = optional(&choices.cover_png, "Reading the cover")?;
        let icon = optional(&choices.icon_png, "Reading the square icon")?;
        let images = choices
            .maker
            .iter()
            .take(store::STORE_MAKER_IMAGE_LIMIT)
            .map(|path| {
                read_store_maker_image(Path::new(path))
                    .ok_or_else(|| crate::project::file_error("Reading a gallery image", "invalid picture"))
            })
            .collect::<Result<Vec<_>, _>>()?;
        write_store_package_artwork(
            self,
            Path::new(&folder),
            library,
            set,
            asset.as_deref().map(Path::new),
            &details,
            StorePackageArtwork {
                cover: cover.as_ref(),
                icon: icon.as_ref(),
                removed_pages: &choices.removed_pages,
                maker: &images,
            },
        )
        .map(|path| path.to_string_lossy().into_owned())
        .map_err(|error| crate::project::file_error("Exporting the Store Package", error))
    }
}

/// A bounded read of a user-selected image, with the package's preview resolution.
pub fn read_store_maker_image(path: &Path) -> Option<RgbaImage> {
    use std::io::Read as _;
    const LIMIT: u64 = 32 * 1024 * 1024;
    let mut bytes = Vec::new();
    std::fs::File::open(path).ok()?.take(LIMIT + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= LIMIT).then(|| store::store_picture(&bytes, 1600)).flatten()
}

pub fn square_store_art(picture: &RgbaImage, edge: u32) -> RgbaImage {
    let fit = image::DynamicImage::ImageRgba8(picture.clone()).thumbnail(edge, edge).into_rgba8();
    let mut square = RgbaImage::from_pixel(edge, edge, image::Rgba([33, 33, 36, 255]));
    let (x, y) = ((edge - fit.width()) / 2, (edge - fit.height()) / 2);
    image::imageops::overlay(&mut square, &fit, i64::from(x), i64::from(y));
    square
}

#[uniffi::export]
pub fn store_square_png(bytes: Vec<u8>) -> Vec<u8> {
    let Some(picture) = store::store_picture(&bytes, 1200) else { return Vec::new() };
    let square = square_store_art(&picture, 1200);
    let mut png = Vec::new();
    if png_to(&mut png, &square).is_err() {
        png.clear();
    }
    png
}

pub fn store_package_gallery(
    session: &CanvasSession,
    library: u32,
    set: u32,
    asset: Option<&Path>,
    title: &str,
) -> Result<Vec<StorePreview>, String> {
    let temporary;
    let path = if let Some(asset) = asset {
        asset.to_owned()
    } else {
        temporary = tempfile::tempdir().map_err(|error| error.to_string())?;
        let path = temporary.path().join("gallery.brushset");
        write_live_brush_set(session, library, set, &path)?;
        path
    };
    let shared = SharedFile::open(&path, None)?;
    Ok(shared
        .pages(title)
        .map(|page| {
            let picture = image::imageops::resize(&page, 320, 240, image::imageops::FilterType::Triangle);
            StorePreview { width: picture.width(), height: picture.height(), rgba: picture.into_raw() }
        })
        .collect())
}

fn write_live_brush_set(session: &CanvasSession, library: u32, set: u32, path: &Path) -> Result<(), String> {
    if session.brush_libraries().iter().position(|entry| entry.current) != Some(library as usize) {
        return Err("The source brush library is no longer open.".to_owned());
    }
    session
        .write_brush_set_file(set, path.to_string_lossy().into_owned(), None)
        .map_err(|error| format!("The brush set couldn't be written: {error}"))
}

/// Package export publishes a complete new folder. A same-name export gets a numbered folder,
/// preserving any prior package and all of its images.
#[allow(clippy::too_many_arguments)]
pub fn write_store_package(
    session: &CanvasSession,
    folder: &Path,
    library: u32,
    set: u32,
    asset: Option<&Path>,
    details: &StorePackageDetails,
    cover: &RgbaImage,
    maker: &[RgbaImage],
) -> Result<PathBuf, String> {
    write_store_package_with_cover(session, folder, library, set, asset, details, Some(cover), maker)
}

#[allow(clippy::too_many_arguments)]
pub fn write_store_package_with_cover(
    session: &CanvasSession,
    folder: &Path,
    library: u32,
    set: u32,
    asset: Option<&Path>,
    details: &StorePackageDetails,
    cover: Option<&RgbaImage>,
    maker: &[RgbaImage],
) -> Result<PathBuf, String> {
    write_store_package_artwork(
        session,
        folder,
        library,
        set,
        asset,
        details,
        StorePackageArtwork { cover, maker, ..StorePackageArtwork::default() },
    )
}

pub fn write_store_package_artwork(
    session: &CanvasSession,
    folder: &Path,
    library: u32,
    set: u32,
    asset: Option<&Path>,
    details: &StorePackageDetails,
    artwork: StorePackageArtwork<'_>,
) -> Result<PathBuf, String> {
    let name = details.name.trim();
    let tags: Vec<_> =
        details.tags.split(',').map(str::trim).filter(|tag| !tag.is_empty()).map(str::to_owned).collect();
    let stage = tempfile::Builder::new()
        .prefix(".store-package-")
        .tempdir_in(folder)
        .map_err(|error| format!("The package folder couldn't be created: {error}"))?;
    let dir = stage.path();
    let payload = if let Some(source) = asset {
        let kind = ShareKind::from_path(source).ok_or("Choose a ProCraft brush or asset file.")?;
        let target = dir.join(format!("file.{}", kind.extension()));
        std::fs::copy(source, &target).map_err(|error| format!("The shared file couldn't be copied: {error}"))?;
        target
    } else {
        let target = dir.join("file.brushset");
        write_live_brush_set(session, library, set, &target)?;
        target
    };
    let file = SharedFile::open(&payload, None)?;
    let kind = file.kind.listing_kind();
    let pictures = write_gallery(dir, artwork.cover, file.pages(name), artwork.maker, artwork.removed_pages)
        .map_err(|error| format!("A gallery image couldn't be written: {error}"))?;
    let icon = if let Some(picture) = artwork.icon {
        let normalized;
        let picture = if picture.width() != picture.height() {
            normalized = square_store_art(picture, 1200);
            &normalized
        } else {
            picture
        };
        save_artwork(&dir.join("icon.png"), picture, true).map_err(|error| error.to_string())?;
        "icon.png"
    } else {
        ""
    };
    drop(file); // release the staged payload before the directory rename on Windows
    let exported = store::iso(store::now());
    let metadata = store::package_details_with_cover(
        kind,
        name,
        details.author.trim(),
        details.description.trim(),
        &details.category,
        &details.licence,
        &tags,
        &pictures,
        artwork.cover.map_or("", |_| "preview.png"),
        icon,
        &exported,
    );
    std::fs::write(dir.join("details.json"), metadata)
        .map_err(|error| format!("The listing details couldn't be written: {error}"))?;
    let base = folder_name(name);
    for index in 1..u64::MAX {
        let name = if index == 1 { base.clone() } else { format!("{base} {index}") };
        let target = folder.join(name);
        if target.exists() {
            continue;
        }
        match std::fs::rename(dir, &target) {
            Ok(()) => return Ok(target),
            Err(_) if target.exists() => {}
            Err(error) => return Err(format!("The finished package couldn't be moved into place: {error}")),
        }
    }
    Err("A new folder name couldn't be found for the package.".to_owned())
}

fn folder_name(name: &str) -> String {
    let clean: String = name
        .chars()
        .take(64)
        .map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_') { c } else { '-' })
        .collect();
    let clean = clean.trim();
    let reserved = [
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1",
        "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    if clean.is_empty() {
        "Untitled Set".to_owned()
    } else if reserved.iter().any(|part| clean.eq_ignore_ascii_case(part)) {
        format!("{clean} Package")
    } else {
        clean.to_owned()
    }
}

fn write_gallery(
    dir: &Path,
    cover: Option<&RgbaImage>,
    contents: impl IntoIterator<Item = RgbaImage>,
    maker: &[RgbaImage],
    removed_pages: &[u32],
) -> std::io::Result<Vec<String>> {
    let mut names = Vec::new();
    let mut save = |name: String, picture: &RgbaImage| {
        save_artwork(&dir.join(&name), picture, name == "preview.png")?;
        names.push(name);
        Ok::<(), std::io::Error>(())
    };
    if let Some(cover) = cover {
        save("preview.png".to_owned(), cover)?;
    }
    for (at, picture) in contents.into_iter().enumerate() {
        if !removed_pages.contains(&(at as u32)) {
            save(format!("contents-{}.png", at + 1), &picture)?;
        }
    }
    for (at, picture) in maker.iter().take(store::STORE_MAKER_IMAGE_LIMIT).enumerate() {
        save(format!("picture-{}.png", at + 1), picture)?;
    }
    Ok(names)
}

fn save_artwork(path: &Path, picture: &RgbaImage, storefront: bool) -> std::io::Result<()> {
    if storefront {
        // A highly detailed image can exceed the catalogue's static-image cap even after normalization.
        // One smaller encoding preserves the aspect and has less than four MiB of raw RGBA pixels.
        let mut png = Vec::new();
        png_to(&mut png, picture)?;
        if png.len() > crate::store_media::PICTURE_BYTES {
            let fitted = image::DynamicImage::ImageRgba8(picture.clone()).thumbnail(1000, 1000).into_rgba8();
            png.clear();
            png_to(&mut png, &fitted)?;
        }
        return std::fs::write(path, png);
    }
    let mut out = std::io::BufWriter::new(std::fs::File::create(path)?);
    png_to(&mut out, picture)?;
    std::io::Write::flush(&mut out)
}

fn pixels(preview: StorePreview) -> Option<RgbaImage> {
    RgbaImage::from_raw(preview.width, preview.height, preview.rgba)
}

fn png_to(out: impl std::io::Write, picture: &RgbaImage) -> std::io::Result<()> {
    use image::ImageEncoder as _;
    image::codecs::png::PngEncoder::new(out)
        .write_image(picture.as_raw(), picture.width(), picture.height(), image::ExtendedColorType::Rgba8)
        .map_err(std::io::Error::other)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn contents_are_between_the_cover_and_maker_images() {
        let dir = tempfile::tempdir().unwrap();
        let cover = RgbaImage::from_pixel(8, 6, image::Rgba([204, 63, 44, 255]));
        let specimen = RgbaImage::from_pixel(8, 6, image::Rgba([29, 39, 36, 255]));
        let maker = RgbaImage::from_pixel(8, 6, image::Rgba([60, 75, 120, 255]));
        let names = write_gallery(dir.path(), Some(&cover), vec![specimen], &[maker], &[]).unwrap();
        assert_eq!(names, ["preview.png", "contents-1.png", "picture-1.png"]);
        assert_eq!(image::open(dir.path().join("preview.png")).unwrap().into_rgba8(), cover);
    }

    #[test]
    fn package_keeps_custom_artwork_and_leaves_removed_contents_out() {
        let root = tempfile::tempdir().unwrap();
        let session = CanvasSession::new(32, 32);
        let source = root.path().join("ink.brush");
        session.write_brush_file(0, 0, source.to_string_lossy().into_owned()).unwrap();
        let pages = store_package_gallery(&session, 0, 0, Some(&source), "Ink").unwrap();
        assert_eq!(pages.len(), 1);
        assert_eq!((pages[0].width, pages[0].height), (320, 240));
        let cover = RgbaImage::from_pixel(72, 48, image::Rgba([145, 90, 45, 255]));
        let icon = RgbaImage::from_pixel(36, 36, image::Rgba([12, 85, 120, 255]));
        let gallery = RgbaImage::from_pixel(40, 60, image::Rgba([103, 85, 42, 255]));
        let dir = write_store_package_artwork(
            &session,
            root.path(),
            0,
            0,
            Some(&source),
            &StorePackageDetails {
                name: "Ink".to_owned(),
                author: "Painter".to_owned(),
                description: "A brush".to_owned(),
                category: "Brushes".to_owned(),
                licence: String::new(),
                tags: "ink".to_owned(),
            },
            StorePackageArtwork {
                cover: Some(&cover),
                icon: Some(&icon),
                removed_pages: &[0],
                maker: std::slice::from_ref(&gallery),
            },
        )
        .unwrap();
        assert_eq!(image::open(dir.join("preview.png")).unwrap().into_rgba8(), cover);
        assert_eq!(image::open(dir.join("icon.png")).unwrap().into_rgba8(), icon);
        assert_eq!(image::open(dir.join("picture-1.png")).unwrap().into_rgba8(), gallery);
        assert!(!dir.join("contents-1.png").exists());
        let details: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("details.json")).unwrap()).unwrap();
        assert_eq!(details["preview"], "preview.png");
        assert_eq!(details["icon"], "icon.png");
        assert_eq!(details["pictures"], serde_json::json!(["preview.png", "picture-1.png"]));
    }

    #[test]
    fn a_saved_brush_generates_its_contents_without_generating_a_cover() {
        let root = tempfile::tempdir().unwrap();
        let session = CanvasSession::new(32, 32);
        let source = root.path().join("one.brush");
        session.write_brush_file(0, 0, source.to_string_lossy().into_owned()).unwrap();
        let exported = session
            .export_store_package(
                root.path().to_string_lossy().into_owned(),
                0,
                0,
                Some(source.to_string_lossy().into_owned()),
                StorePackageDetails {
                    name: "One brush".to_owned(),
                    author: "Painter".to_owned(),
                    description: "A brush".to_owned(),
                    category: "Brushes".to_owned(),
                    licence: "CC BY 4.0".to_owned(),
                    tags: "ink".to_owned(),
                },
                Vec::new(),
                Vec::new(),
            )
            .unwrap();
        let dir = Path::new(&exported);
        assert_eq!(std::fs::read(dir.join("file.brush")).unwrap(), std::fs::read(source).unwrap());
        assert!(dir.join("contents-1.png").is_file());
        assert!(!dir.join("preview.png").exists());
        let details: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.join("details.json")).unwrap()).unwrap();
        assert_eq!(details["preview"], "");
        assert_eq!(details["pictures"], serde_json::json!(["contents-1.png"]));
    }

    #[test]
    fn the_package_and_all_three_normal_share_files_reopen() {
        let root = tempfile::tempdir().unwrap();
        let session = CanvasSession::new(32, 32);
        let at = |name: &str| root.path().join(name).to_string_lossy().into_owned();
        session.write_brush_file(0, 0, at("single.brush")).unwrap();
        session.write_brush_set_file(0, at("group.brushset"), None).unwrap();
        session.write_brush_library_file(at("whole.brushlibrary"), None).unwrap();
        for (name, is_library) in [("single.brush", false), ("group.brushset", false), ("whole.brushlibrary", true)] {
            let read = crate::read_brushes_file(at(name), Some(name.to_owned()), None).expect(name);
            assert_eq!(read.is_library(), is_library);
        }

        let cover = session.prepare_store_package_cover(0, 0, "Inks".to_owned()).unwrap();
        let png = cover.render_png(None, None);
        assert!(!png.is_empty());
        let details = || StorePackageDetails {
            name: "Inks".to_owned(),
            author: "Painter".to_owned(),
            description: "Dry inks".to_owned(),
            category: "Brushes".to_owned(),
            licence: "CC BY 4.0".to_owned(),
            tags: "ink, dry".to_owned(),
        };
        let maker_path = at("maker.png");
        let maker = RgbaImage::from_pixel(8, 6, image::Rgba([60, 75, 120, 255]));
        png_to(std::fs::File::create(&maker_path).unwrap(), &maker).unwrap();
        let first = session.export_store_package(at(""), 0, 0, None, details(), png.clone(), vec![maker_path]).unwrap();
        let second = session.export_store_package(at(""), 0, 0, None, details(), png.clone(), Vec::new()).unwrap();
        assert_ne!(first, second);
        let first = Path::new(&first);
        let second = Path::new(&second);
        let exported = std::fs::read(first.join("preview.png")).unwrap();
        assert_eq!(
            image::load_from_memory(&png).unwrap().into_rgba8(),
            image::load_from_memory(&exported).unwrap().into_rgba8()
        );
        let metadata = std::fs::read_to_string(first.join("details.json")).unwrap();
        let details_json: serde_json::Value = serde_json::from_str(&metadata).unwrap();
        let pictures = details_json["pictures"].as_array().unwrap();
        assert_eq!(pictures[0], "preview.png");
        assert_eq!(pictures.last().unwrap(), "picture-1.png");
        assert_eq!(image::open(first.join("picture-1.png")).unwrap().into_rgba8(), maker);
        assert!(
            root.path().read_dir().unwrap().all(|entry| !entry.unwrap().file_name().to_string_lossy().starts_with('.'))
        );
        for folder in [first, second] {
            let file = folder.join("file.brushset");
            assert!(crate::read_brushes_file(file.to_string_lossy().into_owned(), None, None).is_some());
        }
    }
}

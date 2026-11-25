//! The store package: one brush set leaving the app as the folder a store reads - the set's own
//! file and a details file saying what a listing needs, written where a person picks.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Vec2};
use procraft_core::{CanvasSession, CoverTemplate, StoreCover, StorePreview};

use crate::look::{Look, Weight, argb, text, text_width, wrapped};
use crate::{icons, ui};

/// The sheet, its bar, and how far its content sits in from its sides.
const WIDTH: f32 = 520.0;
const HEIGHT: f32 = 620.0;
const CORNER: f32 = 14.0;
const BAR: f32 = 56.0;
const INSET: f32 = 16.0;
const ROW_HEIGHT: f32 = GROUP_ROW;
const THUMB_W: f32 = 108.0;
const THUMB_H: f32 = 81.0;
/// Maker pictures are laid out four across.
const PER_ROW: usize = 4;
const GAP: f32 = 8.0;
const MOST_OWN: usize = procraft_core::STORE_MAKER_IMAGE_LIMIT;
const COVER_ROW: f32 = GROUP_ROW;
const SECTION_HEAD: f32 = GROUP_HEAD + GROUP_LINE + GROUP_HEAD_BELOW;
const COVER_MENU: usize = 3;
const COVER_SOURCE: usize = 4;
const ICON_SOURCE: usize = 5;
const GALLERY_SOURCE: usize = 6;
const ICON_MENU: usize = 7;

use crate::store::{
    ACCENT, BG, GROUP_HEAD, GROUP_HEAD_BELOW, GROUP_LINE, GROUP_ROW, HINT, IDLE, ROW_INK, SEP, TITLE, group_head,
};

/// The categories a listing may carry, as the store's own list.
const CATEGORIES: [&str; 8] =
    ["Brushes", "Papers and Textures", "Colour", "Layers", "Interface", "Effects", "Canvas", "Other"];
/// The licences a maker may pick from, the common ones written out.
const LICENCES: [&str; 5] = ["CC0-1.0", "CC BY 4.0", "CC BY-SA 4.0", "MIT", "All rights reserved"];
/// What is being shared: the set the sheet opened from, or a pack of its own file.
const KINDS: [&str; 2] = ["Brush set", "Shared file"];
const BRUSHES: usize = 0;
const ASSET_PACK: usize = 1;
/// The files a pack comes as: the asset library's own share file.
const SHARE_FILES: [&str; 6] = ["assetset", "assetlibrary", "asset", "brush", "brushset", "brushlibrary"];

/// Which field has the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Name,
    Author,
    Description,
    Tags,
}

/// The store package sheet: what it is making, and what a listing will say about it.
#[derive(Default)]
pub struct StorePackage {
    pub open: bool,
    share: Option<ShareChooser>,
    copied: Option<(arboard::Clipboard, tempfile::TempDir)>,
    /// The set it is making, its name as the library has it, and its icon.
    set: u32,
    icon: usize,
    name: String,
    /// The name the set brought, which a pack picked for the same sheet may take the place of.
    set_name: String,
    /// The name the picture carries, so that a listing renamed under it is drawn again.
    drawn_name: String,
    author: String,
    description: String,
    tags: String,
    category: usize,
    licence: usize,
    typing: Option<Field>,
    scroll: f32,
    /// The picker open under a row: which row asked, the row itself, and the panel it opened.
    picker: Option<(usize, Rect, Rect)>,
    picker_scroll: f32,
    /// The library the set is in, the picture the app made for it, and its handle on the GPU.
    library: u32,
    picture: Option<image::RgbaImage>,
    texture: Option<egui::TextureHandle>,
    brush_cover: CoverInput,
    cover_template: Option<CoverTemplate>,
    icon_template: Option<CoverTemplate>,
    hide_cover_title: bool,
    /// The pictures they took themselves.
    pictures: Vec<Shot>,
    custom_cover: Option<Shot>,
    custom_icon: Option<Shot>,
    square: Option<Shot>,
    gallery: Vec<egui::TextureHandle>,
    removed_pages: Vec<u32>,
    gallery_job: Option<Receiver<Result<Vec<StorePreview>, String>>>,
    gallery_started: bool,
    gallery_ready: bool,
    gallery_error: Option<String>,
    /// Whether it is a brush set or a pack, and the pack's own file when it is one.
    kind: usize,
    pack: Option<Pack>,
    /// Whether the picture has to be made again, the kind changing under it.
    redraw: bool,
}

/// The asset pack the sheet is making: its file, and the file a listing takes.
struct Pack {
    path: std::path::PathBuf,
    name: String,
    brushes: bool,
    cover: CoverInput,
    _scratch: Option<tempfile::TempDir>,
}

#[derive(Clone, Copy)]
pub(crate) enum BrushShare {
    Brush(u32, u32),
    Set(u32, usize),
    Library,
}

impl BrushShare {
    fn format(self) -> (&'static str, &'static str) {
        match self {
            Self::Brush(..) => ("Brush", "brush"),
            Self::Set(..) => ("Brush set", "brushset"),
            Self::Library => ("Brush library", "brushlibrary"),
        }
    }

    fn write(self, session: &CanvasSession, path: &std::path::Path) -> Result<(), String> {
        let path = path.to_string_lossy().into_owned();
        match self {
            Self::Brush(set, index) => session.write_brush_file(set, index, path),
            Self::Set(set, _) => session.write_brush_set_file(set, path, None),
            Self::Library => session.write_brush_library_file(path, None),
        }
        .map_err(|error| error.to_string())
    }
}

struct ShareChooser {
    target: BrushShare,
    name: String,
    thumbnail: Option<egui::TextureHandle>,
    job: Option<(Destination, Receiver<Result<SharedFile, String>>)>,
    error: Option<String>,
}

#[derive(Clone, Copy)]
enum Destination {
    Store,
    Save,
    Copy,
}

struct SharedFile {
    path: std::path::PathBuf,
    folder: Option<tempfile::TempDir>,
}

#[derive(Default)]
enum CoverInput {
    #[default]
    Empty,
    Preparing(Receiver<PreparedCover>),
    Ready(StoreCover),
}

struct PreparedCover {
    cover: StoreCover,
    preview: StorePreview,
    square: StorePreview,
    title: Option<String>,
}

impl CoverInput {
    fn prepare(
        ctx: &egui::Context,
        title: Option<String>,
        artwork: impl FnOnce() -> Option<StoreCover> + Send + 'static,
    ) -> Self {
        let (send, receive) = channel();
        let ctx = ctx.clone();
        let spawn = std::thread::Builder::new().name("store cover".to_owned()).spawn(move || {
            if let Some(cover) = artwork() {
                let preview = cover.render(None, title.as_deref());
                let square = cover.render_square(None, title.as_deref());
                let _ = send.send(PreparedCover { cover, preview, square, title });
            } else {
                drop(send);
            }
            ctx.request_repaint();
        });
        if let Err(error) = spawn {
            log::warn!("the store cover worker could not start: {error}");
            return Self::Empty;
        }
        Self::Preparing(receive)
    }

    fn poll(&mut self) -> Option<(StorePreview, StorePreview, Option<String>)> {
        let Self::Preparing(receiver) = self else { return None };
        match receiver.try_recv() {
            Ok(prepared) => {
                *self = Self::Ready(prepared.cover);
                Some((prepared.preview, prepared.square, prepared.title))
            }
            Err(TryRecvError::Disconnected) => {
                *self = Self::Empty;
                None
            }
            Err(TryRecvError::Empty) => None,
        }
    }
}

/// One of their own pictures: its pixels, and its handle on the GPU.
struct Shot {
    image: image::RgbaImage,
    texture: egui::TextureHandle,
}

impl StorePackage {
    pub(crate) fn share(&mut self, target: BrushShare, name: String, thumbnail: Option<egui::TextureHandle>) {
        self.share = Some(ShareChooser { target, name, thumbnail, job: None, error: None });
        self.open = true;
    }

    fn show_share(&mut self, ctx: &egui::Context, look: &mut Look, session: &Arc<CanvasSession>) {
        let Some(mut chooser) = self.share.take() else { return };
        if let Some((destination, receive)) = &chooser.job {
            let result = match receive.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Disconnected) => Some(Err("The shared file couldn't be prepared.".to_owned())),
                Err(TryRecvError::Empty) => None,
            };
            if let Some(result) = result {
                let destination = *destination;
                chooser.job = None;
                match result {
                    Ok(file) => {
                        match destination {
                            Destination::Store => {
                                self.reset(&chooser.name, 0);
                                self.kind = ASSET_PACK;
                                self.pack = Some(Pack {
                                    path: file.path,
                                    name: chooser.name,
                                    brushes: true,
                                    cover: CoverInput::Empty,
                                    _scratch: file.folder,
                                });
                            }
                            Destination::Copy => {
                                let copy = arboard::Clipboard::new().and_then(|mut clipboard| {
                                    clipboard.set().file_list(&[&file.path])?;
                                    Ok(clipboard)
                                });
                                if let (Ok(clipboard), Some(folder)) = (copy, file.folder) {
                                    self.copied = Some((clipboard, folder));
                                    crate::files::say("File copied.");
                                    self.open = false;
                                } else {
                                    chooser.error = Some("The file couldn't be copied to the clipboard.".to_owned());
                                    self.share = Some(chooser);
                                }
                            }
                            Destination::Save => self.open = false,
                        }
                        return;
                    }
                    Err(error) => chooser.error = Some(error),
                }
            }
        }
        let busy = chooser.job.is_some();
        if !busy && ui::escape(ctx) {
            self.open = false;
            return;
        }
        let screen = ctx.content_rect();
        let sheet = Rect::from_center_size(screen.center(), Vec2::new(400.0_f32.min(screen.width() - 32.0), 350.0));
        let mut close = false;
        egui::Area::new(Id::unique("share scrim")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |area| {
            close = area.allocate_rect(screen, Sense::click()).clicked() && !busy;
            area.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(120));
        });
        let mut choice = None;
        egui::Area::new(Id::unique("brush share chooser")).fixed_pos(sheet.min).order(Order::Debug).show(ctx, |area| {
            area.allocate_rect(sheet, Sense::click());
            let painter = area.painter();
            painter.add(
                egui::epaint::Shadow { offset: [0, 0], blur: 46, spread: 0, color: Color32::from_black_alpha(130) }
                    .as_shape(sheet, CORNER),
            );
            painter.rect_filled(sheet, CORNER, argb(BG));
            let bar = Rect::from_min_size(sheet.min, Vec2::new(sheet.width(), BAR));
            painter.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, argb(SEP)));
            text(painter, bar.center(), Align2::CENTER_CENTER, "Share", 17.0, Weight::SemiBold, argb(TITLE));
            let cancel = Rect::from_min_size(bar.min, Vec2::new(100.0, BAR));
            text(
                painter,
                egui::pos2(bar.left() + INSET, bar.center().y),
                Align2::LEFT_CENTER,
                "Cancel",
                17.0,
                Weight::Regular,
                argb(if busy { IDLE } else { ACCENT }),
            );
            close |= !busy && area.interact(cancel, area.scope_id().with("share cancel"), Sense::click()).clicked();
            let picture =
                Rect::from_min_size(egui::pos2(sheet.left() + INSET, bar.bottom() + INSET), Vec2::splat(56.0));
            painter.rect_filled(picture, 13.0, argb(crate::store::TILE_FILL));
            if let Some(texture) = &chooser.thumbnail {
                let fit = texture.size_vec2() * (picture.shrink(5.0).size() / texture.size_vec2()).min_elem();
                let image = Rect::from_center_size(picture.center(), fit);
                let uv = Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0));
                painter.image(texture.id(), image, uv, argb(TITLE));
            } else {
                let icon = match chooser.target {
                    BrushShare::Brush(..) => &icons::BRUSH,
                    BrushShare::Set(_, icon) => icons::SET_ICONS.get(icon).unwrap_or(&icons::BRUSH_SET),
                    BrushShare::Library => &icons::MENU_LIBRARIES,
                };
                look.icon_on(area, painter, icon, picture.center(), 28.0, argb(IDLE));
            }
            let x = picture.right() + 12.0;
            crate::store::ellipsized(
                painter,
                egui::pos2(x, picture.top() + 19.0),
                &chooser.name,
                sheet.right() - INSET - x,
                17.0,
                Weight::SemiBold,
                argb(TITLE),
            );
            text(
                painter,
                egui::pos2(x, picture.top() + 39.0),
                Align2::LEFT_CENTER,
                chooser.target.format().0,
                15.0,
                Weight::Regular,
                argb(ROW_INK),
            );
            let group = Rect::from_min_size(
                egui::pos2(picture.left(), picture.bottom() + INSET),
                Vec2::new(sheet.width() - INSET * 2.0, ROW_HEIGHT * 3.0),
            );
            painter.rect_filled(group, 10.0, argb(crate::store::GROUP_FILL));
            let mut y = group.top();
            for (destination, label, height) in [
                (Destination::Store, "Store", ROW_HEIGHT),
                (Destination::Save, "Save to Files", ROW_HEIGHT),
                (Destination::Copy, "Copy File", ROW_HEIGHT),
            ] {
                let row = Rect::from_min_size(egui::pos2(group.left(), y), Vec2::new(group.width(), height));
                let store = matches!(destination, Destination::Store);
                let center = row.center().y;
                text(
                    painter,
                    egui::pos2(row.left() + INSET, center - if store { 9.0 } else { 0.0 }),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    argb(if busy { IDLE } else { TITLE }),
                );
                if store {
                    text(
                        painter,
                        egui::pos2(row.left() + INSET, center + 12.0),
                        Align2::LEFT_CENTER,
                        "Package, artwork, and listing details",
                        15.0,
                        Weight::Regular,
                        argb(ROW_INK),
                    );
                } else {
                    form_rule(painter, group, row.top());
                }
                look.icon_on(
                    area,
                    painter,
                    &icons::CHEVRON_RIGHT,
                    egui::pos2(row.right() - INSET, center),
                    12.0,
                    argb(IDLE),
                );
                let action = area.interact(row, area.scope_id().with(label), Sense::click());
                action.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, !busy, label));
                if !busy && action.clicked() {
                    choice = Some(destination);
                }
                y += height;
            }
            let (status, color) = if let Some(error) = &chooser.error {
                (error.as_str(), argb(crate::store::DANGER))
            } else if busy {
                ("Preparing file…", argb(ROW_INK))
            } else {
                ("", argb(ROW_INK))
            };
            crate::store::ellipsized(
                painter,
                egui::pos2(group.left(), group.bottom() + 18.0),
                status,
                group.width(),
                13.0,
                Weight::Regular,
                color,
            );
        });
        if close {
            self.open = false;
            return;
        }
        if let Some(destination) = choice {
            if let (Destination::Store, BrushShare::Set(set, icon)) = (destination, chooser.target) {
                self.begin(ctx, session, set, &chooser.name, icon);
                return;
            }
            let target = chooser.target;
            let (kind, extension) = target.format();
            let save = if matches!(destination, Destination::Save) {
                let Some(path) = crate::files::pick_share_path(&chooser.name, extension, kind) else {
                    self.share = Some(chooser);
                    return;
                };
                Some(path)
            } else {
                None
            };
            let name = crate::files::share_file_name(&chooser.name, extension);
            let (send, receive) = channel();
            let session = Arc::clone(session);
            let ctx = ctx.clone();
            let spawn = std::thread::Builder::new().name("write brush share".to_owned()).spawn(move || {
                let result = (|| {
                    let (path, folder) = if let Some(path) = save {
                        (path, None)
                    } else {
                        let folder = tempfile::tempdir().map_err(|error| error.to_string())?;
                        (folder.path().join(name), Some(folder))
                    };
                    target.write(&session, &path)?;
                    Ok(SharedFile { path, folder })
                })();
                let _ = send.send(result);
                ctx.request_repaint();
            });
            match spawn {
                Ok(_) => {
                    chooser.error = None;
                    chooser.job = Some((destination, receive));
                }
                Err(error) => chooser.error = Some(error.to_string()),
            }
        }
        self.share = Some(chooser);
    }

    /// Begins a package for `set`, in the library in use, drawing its picture from the set itself.
    pub fn begin(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>, set: u32, name: &str, icon: usize) {
        self.reset(name, icon);
        self.set = set;
        self.library = session.brush_libraries().iter().position(|library| library.current).unwrap_or(0) as u32;
        let (library, session, seed_key) = (self.library, Arc::clone(session), name.to_owned());
        self.brush_cover = CoverInput::prepare(ctx, self.cover_title().map(str::to_owned), move || {
            session.brush_set_store_cover(library, set, &seed_key)
        });
    }

    fn contents_only(&self) -> bool {
        self.kind == ASSET_PACK && self.pack.as_ref().is_some_and(|pack| pack.brushes)
    }

    fn cover_input(&self) -> Option<&CoverInput> {
        if self.kind == ASSET_PACK { self.pack.as_ref().map(|pack| &pack.cover) } else { Some(&self.brush_cover) }
    }

    fn cover(&self) -> Option<&StoreCover> {
        match self.cover_input()? {
            CoverInput::Ready(cover) => Some(cover),
            _ => None,
        }
    }

    fn cover_title(&self) -> Option<&str> {
        let name = self.name.trim();
        (!self.hide_cover_title && !name.is_empty()).then_some(name)
    }

    fn poll_cover(&mut self, ctx: &egui::Context) {
        let input = if self.kind == ASSET_PACK {
            let Some(pack) = self.pack.as_mut() else { return };
            &mut pack.cover
        } else {
            &mut self.brush_cover
        };
        let Some((preview, square, title)) = input.poll() else { return };
        if self.cover_title() == title.as_deref() {
            self.picture = preview_image(preview);
            self.square = preview_image(square).map(|image| Shot::new(ctx, image));
            self.drawn_name = self.name.trim().to_owned();
            self.set_texture(ctx);
        } else {
            self.renew_picture(ctx);
        }
    }

    /// The picture's handle on the GPU.
    fn set_texture(&mut self, ctx: &egui::Context) {
        self.texture = self.picture.as_ref().map(|picture| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [picture.width() as usize, picture.height() as usize],
                picture.as_raw(),
            );
            ctx.load_texture("store package picture", image, egui::TextureOptions::LINEAR)
        });
    }

    /// These pixels are used both for the on-screen cover and for preview.png.
    fn renew_picture(&mut self, ctx: &egui::Context) {
        let title = self.cover_title();
        let picture = self.cover().map(|cover| cover.render(self.cover_template, title)).and_then(preview_image);
        let square = self
            .cover()
            .map(|cover| cover.render_square(self.icon_template.or(self.cover_template), title))
            .and_then(preview_image)
            .map(|image| Shot::new(ctx, image));
        self.picture = picture;
        self.square = square;
        self.drawn_name = self.name.trim().to_owned();
        self.set_texture(ctx);
    }

    fn cover_label(&self) -> String {
        if let Some(template) = self.cover_template {
            template.name().to_owned()
        } else {
            self.cover()
                .and_then(StoreCover::recommended)
                .map_or_else(|| "Auto".to_owned(), |template| format!("Auto · {}", template.name()))
        }
    }

    /// The sheet's fields, ready for a new package.
    fn reset(&mut self, name: &str, icon: usize) {
        self.open = true;
        self.share = None;
        self.set = 0;
        self.icon = icon;
        self.name = name.to_owned();
        self.set_name = name.to_owned();
        self.drawn_name = name.trim().to_owned();
        self.author.clear();
        self.description.clear();
        self.tags.clear();
        self.category = 0;
        self.licence = 1;
        self.typing = None;
        self.scroll = 0.0;
        self.picker = None;
        self.picker_scroll = 0.0;
        self.picture = None;
        self.texture = None;
        self.brush_cover = CoverInput::Empty;
        self.cover_template = None;
        self.icon_template = None;
        self.hide_cover_title = false;
        self.pictures.clear();
        self.custom_cover = None;
        self.custom_icon = None;
        self.square = None;
        self.reset_gallery();
        self.kind = BRUSHES;
        self.pack = None;
        self.redraw = false;
    }

    fn reset_gallery(&mut self) {
        self.gallery.clear();
        self.removed_pages.clear();
        self.gallery_job = None;
        self.gallery_started = false;
        self.gallery_ready = false;
        self.gallery_error = None;
    }

    fn prepare_gallery(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>) {
        if !self.gallery_started {
            let asset = if self.kind == ASSET_PACK {
                let Some(pack) = &self.pack else { return };
                Some(pack.path.clone())
            } else {
                None
            };
            let (send, receive) = channel();
            let (session, ctx, name) = (Arc::clone(session), ctx.clone(), self.name.clone());
            let (library, set) = (self.library, self.set);
            let job = std::thread::Builder::new().name("Store gallery".to_owned()).spawn(move || {
                let _ =
                    send.send(procraft_core::store_package_gallery(&session, library, set, asset.as_deref(), &name));
                ctx.request_repaint();
            });
            self.gallery_started = true;
            match job {
                Ok(_) => self.gallery_job = Some(receive),
                Err(error) => self.gallery_error = Some(error.to_string()),
            }
        }
        let Some(waiting) = &self.gallery_job else { return };
        match waiting.try_recv() {
            Ok(Ok(pages)) => {
                self.gallery = pages
                    .iter()
                    .map(|page| {
                        ctx.load_texture(
                            "Store contents page",
                            crate::pictures::colour_image(page),
                            egui::TextureOptions::LINEAR,
                        )
                    })
                    .collect();
                self.gallery_ready = true;
            }
            Ok(Err(error)) => self.gallery_error = Some(error),
            Err(TryRecvError::Disconnected) => {
                self.gallery_error = Some("Could not prepare contents pages.".to_owned())
            }
            Err(TryRecvError::Empty) => return,
        }
        self.gallery_job = None;
    }

    fn import_artwork(&mut self, ctx: &egui::Context, session: &CanvasSession, target: usize, source: usize) {
        if source == 3 {
            match target {
                COVER_SOURCE => self.custom_cover = None,
                ICON_SOURCE => self.custom_icon = None,
                _ => self.removed_pages.clear(),
            }
            return;
        }
        let room = if target == GALLERY_SOURCE { MOST_OWN.saturating_sub(self.pictures.len()) } else { 1 };
        if room == 0 {
            return;
        }
        let images = match source {
            0 => {
                let picker = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]);
                let paths = if target == GALLERY_SOURCE {
                    picker.pick_files()
                } else {
                    picker.pick_file().map(|path| vec![path])
                };
                let Some(paths) = paths else { return };
                paths
                    .iter()
                    .take(room)
                    .filter_map(|path| procraft_core::read_store_maker_image(path))
                    .collect::<Vec<_>>()
            }
            1 => clipboard_picture().into_iter().collect(),
            _ => session.canvas_thumbnail(1600).and_then(|picture| canvas_picture(&picture)).into_iter().collect(),
        };
        if images.is_empty() {
            crate::files::tell("Store", "Couldn't read an image from that source.");
            return;
        }
        for image in images {
            match target {
                COVER_SOURCE => {
                    self.custom_cover = Some(Shot::new(ctx, image));
                }
                ICON_SOURCE => {
                    self.custom_icon = Some(Shot::new(ctx, procraft_core::square_store_art(&image, 1200)));
                }
                _ => self.pictures.push(Shot::new(ctx, image)),
            }
        }
    }

    /// Takes the pack's own file: its picture is composed from the pictures the archive holds.
    fn pick_pack(&mut self, ctx: &egui::Context) {
        let Some(path) = rfd::FileDialog::new().add_filter("ProCraft brush and asset files", &SHARE_FILES).pick_file()
        else {
            return;
        };
        let at = path.to_string_lossy().into_owned();
        let Some(shared) = procraft_core::store_shared_file(at, None) else {
            crate::files::tell("Store", "Choose a ProCraft brush or asset share file with items to preview.");
            return;
        };
        let name = shared.name;
        // the sheet keeps the set's name only until a pack brings one of its own
        if self.name.trim().is_empty()
            || self.name == self.set_name
            || self.pack.as_ref().is_some_and(|pack| self.name == pack.name)
        {
            self.name = name.clone();
        }
        let cover = if shared.brushes {
            CoverInput::Empty
        } else {
            let source = path.clone();
            CoverInput::prepare(ctx, self.cover_title().map(str::to_owned), move || {
                procraft_core::asset_pack_store_cover(&source)
            })
        };
        self.pack = Some(Pack { path, name, brushes: shared.brushes, cover, _scratch: None });
        self.reset_gallery();
        self.cover_template = None;
        self.icon_template = None;
        self.renew_picture(ctx);
        if shared.brushes && self.category == 1 {
            self.category = 0;
        } else if !shared.brushes && self.category == 0 {
            self.category = 1;
        }
    }

    /// Choices listed under the row that asked for them.
    fn pick_menu(&mut self, ui: &egui::Ui, sheet: Rect, which: usize, anchor: Rect, session: &CanvasSession) {
        let templates = if matches!(which, COVER_MENU | ICON_MENU) {
            self.cover().map(StoreCover::templates).unwrap_or_default()
        } else {
            Vec::new()
        };
        let (options, chosen) = match which {
            0 => (CATEGORIES.to_vec(), self.category),
            1 => (LICENCES.to_vec(), self.licence),
            2 => (KINDS.to_vec(), self.kind),
            COVER_SOURCE | ICON_SOURCE | GALLERY_SOURCE => {
                let mut sources = vec!["File", "Clipboard", "Current Project"];
                match which {
                    COVER_SOURCE if self.custom_cover.is_some() => {
                        sources.push(if self.contents_only() { "Remove custom cover" } else { "Use automatic cover" })
                    }
                    ICON_SOURCE if self.custom_icon.is_some() => sources.push("Use automatic icon"),
                    _ => {}
                }
                (sources, usize::MAX)
            }
            _ => {
                let icon = which == ICON_MENU;
                let mut choices = vec![if icon { "Match cover layout" } else { "Auto" }];
                choices.extend(templates.iter().map(|template| template.name()));
                let selected = if icon { self.icon_template } else { self.cover_template };
                let chosen = selected.and_then(|template| templates.iter().position(|&t| t == template));
                (choices, chosen.map_or(0, |at| at + 1))
            }
        };
        let width = 262.0_f32.min(sheet.width() - INSET * 2.0);
        let content = options.len() as f32 * COVER_ROW;
        let height = (content + 12.0).min(364.0).min(sheet.height() - 16.0);
        let below = anchor.bottom() + 6.0;
        let top = if below + height > sheet.bottom() - 8.0 {
            (anchor.top() - height - 6.0).max(sheet.top() + 8.0)
        } else {
            below
        };
        let panel = Rect::from_min_size(Pos2::new(anchor.right() - width, top), Vec2::new(width, height));
        self.picker = Some((which, anchor, panel));
        ui.painter().add(
            egui::epaint::Shadow { offset: [0, 0], blur: 40, spread: 0, color: Color32::from_black_alpha(120) }
                .as_shape(panel, 22.0),
        );
        ui.painter().rect_filled(panel, 22.0, argb(BG));
        ui.interact(panel, ui.scope_id().with("package pick panel"), Sense::click_and_drag());
        let body = panel.shrink2(Vec2::new(0.0, 6.0));
        let paint = ui.painter().with_clip_rect(body);
        let over = ui.input(|i| i.pointer.latest_pos()).is_some_and(|position| panel.contains(position));
        let delta = if over { ui.input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
        let most = (content - body.height()).max(0.0);
        self.picker_scroll = (self.picker_scroll - delta).clamp(0.0, most);
        let mut y = body.top() - self.picker_scroll;
        for (at, option) in options.iter().enumerate() {
            let row = Rect::from_min_size(Pos2::new(panel.left(), y), Vec2::new(width, COVER_ROW));
            y += COVER_ROW;
            if !body.intersects(row) {
                continue;
            }
            let on = at == chosen;
            text(
                &paint,
                Pos2::new(row.left() + INSET, row.center().y),
                Align2::LEFT_CENTER,
                option,
                17.0,
                Weight::Regular,
                argb(if on || matches!(which, COVER_SOURCE | ICON_SOURCE | GALLERY_SOURCE) { TITLE } else { ROW_INK }),
            );
            let radio = Pos2::new(row.right() - 27.0, row.center().y);
            if !matches!(which, COVER_SOURCE | ICON_SOURCE | GALLERY_SOURCE) {
                paint.circle_stroke(radio, 8.5, Stroke::new(1.5, argb(if on { ACCENT } else { IDLE })));
            }
            if on {
                paint.circle_filled(radio, 4.5, argb(ACCENT));
            }
            if ui.interact(row.intersect(body), ui.scope_id().with(("package pick row", at)), Sense::click()).clicked()
            {
                match which {
                    0 => self.category = at,
                    1 => self.licence = at,
                    2 => {
                        if self.kind != at {
                            if let Some(pack) = &self.pack {
                                if at == ASSET_PACK && self.name == self.set_name {
                                    self.name.clone_from(&pack.name);
                                } else if at == BRUSHES && self.name == pack.name {
                                    self.name.clone_from(&self.set_name);
                                }
                            }
                            self.kind = at;
                            self.reset_gallery();
                            self.cover_template = None;
                            self.icon_template = None;
                            self.redraw = true;
                        }
                    }
                    COVER_SOURCE | ICON_SOURCE | GALLERY_SOURCE => self.import_artwork(ui.ctx(), session, which, at),
                    _ => {
                        let selected = at.checked_sub(1).and_then(|at| templates.get(at)).copied();
                        if which == ICON_MENU {
                            self.icon_template = selected;
                        } else {
                            self.cover_template = selected;
                        }
                        self.redraw = true;
                    }
                }
                self.picker = None;
            }
        }
        if most > 0.0 {
            let thumb = body.height() * body.height() / content;
            let top = body.top() + self.picker_scroll / most * (body.height() - thumb);
            let x = body.right() - 4.0;
            paint.line_segment([Pos2::new(x, top), Pos2::new(x, top + thumb)], Stroke::new(2.0, argb(IDLE)));
        }
    }

    /// The three things a listing cannot do without: a name, a description and at least one tag;
    /// and a pack, when it is one.
    fn ready(&self) -> bool {
        !self.name.trim().is_empty()
            && !self.description.trim().is_empty()
            && !self.tags.trim().is_empty()
            && (self.kind == BRUSHES || self.pack.is_some())
    }

    /// Draws the sheet over everything it opened from. Closes on Cancel, on Escape, or on a press
    /// beside it.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &Arc<CanvasSession>) {
        if !self.open {
            return;
        }
        if self.share.is_some() {
            self.show_share(ctx, look, session);
            return;
        }
        self.poll_cover(ctx);
        self.prepare_gallery(ctx, session);
        if ui::escape(ctx) && self.picker.take().is_none() && self.typing.take().is_none() {
            self.open = false;
            return;
        }
        let screen = ctx.content_rect();
        let size = Vec2::new(WIDTH.min(screen.width() - 40.0), HEIGHT.min(screen.height() - 60.0));
        let sheet = Rect::from_center_size(screen.center(), size);
        // the work behind it, dimmed, taking the presses that land outside the sheet
        egui::Area::new(Id::unique("package scrim")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |area| {
            let beside = area.allocate_rect(screen, Sense::click());
            area.painter().rect_filled(screen, 0.0, Color32::from_black_alpha(120));
            if beside.clicked() {
                self.open = false;
            }
        });
        if !self.open {
            return;
        }
        let had_picker = self.picker.is_some();
        egui::Area::new(Id::unique("package sheet")).fixed_pos(sheet.min).order(Order::Debug).show(ctx, |area| {
            area.allocate_rect(sheet, Sense::click_and_drag());
            area.painter().add(
                egui::epaint::Shadow { offset: [0, 0], blur: 46, spread: 0, color: Color32::from_black_alpha(130) }
                    .as_shape(sheet, CORNER),
            );
            area.painter().with_clip_rect(sheet).rect_filled(sheet, CORNER, argb(BG));
            self.read_keys(ctx);
            if self.drawn_name != self.name.trim() {
                if self.hide_cover_title {
                    self.drawn_name = self.name.trim().to_owned();
                } else {
                    self.renew_picture(ctx);
                }
            }
            // the bar: Cancel, the sheet's name, and the Export that is the signal when it cannot be taken
            let bar = Rect::from_min_size(sheet.min, Vec2::new(sheet.width(), BAR));
            area.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, argb(SEP)));
            text(
                area.painter(),
                Pos2::new(bar.left() + INSET, bar.center().y),
                Align2::LEFT_CENTER,
                "Cancel",
                17.0,
                Weight::Regular,
                argb(ACCENT),
            );
            text(
                area.painter(),
                Pos2::new(bar.center().x, bar.center().y),
                Align2::CENTER_CENTER,
                "Store Details",
                17.0,
                Weight::SemiBold,
                argb(TITLE),
            );
            let ready = self.ready()
                && self.gallery_ready
                && (self.custom_cover.is_some() || self.contents_only() || self.picture.is_some());
            text(
                area.painter(),
                Pos2::new(bar.right() - INSET, bar.center().y),
                Align2::RIGHT_CENTER,
                "Export",
                17.0,
                Weight::SemiBold,
                argb(if ready { ACCENT } else { HINT }),
            );
            for (at, rect) in [
                Rect::from_min_max(Pos2::new(bar.left(), bar.top()), Pos2::new(bar.left() + 100.0, bar.bottom())),
                Rect::from_min_max(Pos2::new(bar.right() - 100.0, bar.top()), Pos2::new(bar.right(), bar.bottom())),
            ]
            .iter()
            .enumerate()
            {
                if area.interact(*rect, area.scope_id().with(("package bar", at)), Sense::click()).clicked() {
                    if at == 0 {
                        self.open = false;
                    } else if ready {
                        self.export(session);
                    }
                }
            }
            // the body, scrolling inside the sheet so the sheet never grows
            let body = Rect::from_min_max(
                Pos2::new(sheet.left() + INSET, sheet.top() + BAR),
                Pos2::new(sheet.right() - INSET, sheet.bottom()),
            );
            let height = self.height(body.width());
            let most = (height - body.height()).max(0.0);
            let over = area.input(|i| i.pointer.latest_pos()).is_some_and(|p| body.contains(p));
            let delta = if over && self.picker.is_none() { area.input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
            self.scroll = (self.scroll - delta).clamp(0.0, most);
            let painter = area.painter().with_clip_rect(body.intersect(area.clip_rect()));
            let mut y = body.top() - self.scroll;
            y = self.show_cover(area, &painter, look, body, y);
            y = self.show_gallery(area, &painter, look, body, y);
            y = group_head(&painter, body, y, "DETAILS", argb(ROW_INK));
            let group = Rect::from_min_size(
                Pos2::new(body.left(), y),
                Vec2::new(body.width(), ROW_HEIGHT * if self.kind == ASSET_PACK { 6.0 } else { 5.0 }),
            );
            painter.rect_filled(group, 10.0, argb(crate::store::GROUP_FILL));
            let fields = [(Field::Name, "Name", self.name.clone()), (Field::Author, "Author", self.author.clone())];
            for (index, (field, label, value)) in fields.into_iter().enumerate() {
                let row = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), ROW_HEIGHT));
                if index != 0 {
                    form_rule(&painter, group, row.top());
                }
                text(
                    &painter,
                    Pos2::new(row.left() + 16.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    argb(TITLE),
                );
                let at = Pos2::new(row.right() - 16.0, row.center().y);
                if self.typing == Some(field) {
                    text(&painter, at, Align2::RIGHT_CENTER, &value, 17.0, Weight::Regular, argb(TITLE));
                    let width = text_width(&painter, &value, 17.0, Weight::Regular);
                    circle_caret(&painter, at.x - width - 3.0, at.y);
                } else if value.is_empty() {
                    text(&painter, at, Align2::RIGHT_CENTER, " ", 17.0, Weight::Regular, argb(ROW_INK));
                } else {
                    text(&painter, at, Align2::RIGHT_CENTER, &value, 17.0, Weight::Regular, argb(ROW_INK));
                }
                if area
                    .interact(row.intersect(body), area.scope_id().with(("package row", label)), Sense::click())
                    .clicked()
                {
                    self.typing = Some(field);
                }
                y += ROW_HEIGHT;
            }
            // Kind is what is shared; the category and the licence are the maker's to say
            let line = |at: usize, list: &[&str], fallback: &str| list.get(at).copied().unwrap_or(fallback).to_owned();
            for (menu, label, value) in [
                (2, "Kind", line(self.kind, &KINDS, "Brush set")),
                (0, "Category", line(self.category, &CATEGORIES, "Brushes")),
                (1, "Licence", line(self.licence, &LICENCES, "CC BY 4.0")),
            ] {
                let row = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), ROW_HEIGHT));
                form_rule(&painter, group, row.top());
                text(
                    &painter,
                    Pos2::new(row.left() + 16.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    argb(TITLE),
                );
                text(
                    &painter,
                    Pos2::new(row.right() - 16.0 - 12.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    &value,
                    17.0,
                    Weight::Regular,
                    argb(ROW_INK),
                );
                look.icon_on(
                    area,
                    &painter,
                    &icons::CHEVRON_RIGHT,
                    Pos2::new(row.right() - 14.0, row.center().y),
                    13.0,
                    argb(IDLE),
                );
                if !(menu == 2 && self.pack.as_ref().is_some_and(|pack| pack._scratch.is_some()))
                    && area
                        .interact(row.intersect(body), area.scope_id().with(("package pick", label)), Sense::click())
                        .clicked()
                {
                    // Kind, Category and Licence list what they may say under the row that was tapped
                    self.picker = Some((menu, row, Rect::NOTHING));
                    self.picker_scroll = 0.0;
                }
                y += ROW_HEIGHT;
            }
            // a pack brings its own file: its pictures are what its card shows
            if self.kind == ASSET_PACK {
                let row = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), ROW_HEIGHT));
                form_rule(&painter, group, row.top());
                text(
                    &painter,
                    Pos2::new(row.left() + 16.0, row.center().y),
                    Align2::LEFT_CENTER,
                    "Shared file",
                    17.0,
                    Weight::Regular,
                    argb(TITLE),
                );
                let shown = self.pack.as_ref().map(|pack| {
                    pack.path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default()
                });
                text(
                    &painter,
                    Pos2::new(row.right() - 16.0 - 12.0, row.center().y),
                    Align2::RIGHT_CENTER,
                    shown.as_deref().unwrap_or("Choose a file"),
                    17.0,
                    Weight::Regular,
                    argb(if shown.is_some() { ROW_INK } else { HINT }),
                );
                look.icon_on(
                    area,
                    &painter,
                    &icons::CHEVRON_RIGHT,
                    Pos2::new(row.right() - 14.0, row.center().y),
                    13.0,
                    argb(IDLE),
                );
                if area
                    .interact(row.intersect(body), area.scope_id().with("package pick pack"), Sense::click())
                    .clicked()
                {
                    self.pick_pack(ctx);
                }
                y += ROW_HEIGHT;
            }
            y += 8.0;
            text(
                &painter,
                Pos2::new(body.left(), y),
                Align2::LEFT_TOP,
                "Kind and category are read from the file. Change them if they are wrong.",
                13.0,
                Weight::Regular,
                argb(ROW_INK),
            );
            y += 32.0;
            // the words, and the tags that make it findable
            for (field, heading_text, hint) in [
                (Field::Description, "DESCRIPTION", "One sentence on what the set does"),
                (Field::Tags, "TAGS", "Comma separated"),
            ] {
                y = group_head(&painter, body, y, heading_text, argb(ROW_INK));
                let row = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), ROW_HEIGHT));
                painter.rect_filled(row, 10.0, argb(crate::store::GROUP_FILL));
                let value = match field {
                    Field::Description => self.description.clone(),
                    Field::Tags => self.tags.clone(),
                    _ => String::new(),
                };
                let left = Pos2::new(row.left() + 16.0, row.center().y);
                let empty = value.is_empty();
                let shown = if empty { hint.to_owned() } else { value.clone() };
                let colour = if empty { argb(HINT) } else { argb(TITLE) };
                text(&painter, left, Align2::LEFT_CENTER, &shown, 17.0, Weight::Regular, colour);
                if self.typing == Some(field) {
                    let width = text_width(&painter, &shown, 17.0, Weight::Regular).min(row.width() - 32.0);
                    circle_caret(&painter, left.x + width + 2.0, left.y);
                }
                if area
                    .interact(
                        row.intersect(body),
                        area.scope_id().with(("package field", heading_text)),
                        Sense::click(),
                    )
                    .clicked()
                {
                    self.typing = Some(field);
                }
                y += ROW_HEIGHT + 20.0;
            }
            // the three things a listing cannot do without, named once
            let needed = if self.kind == ASSET_PACK && self.pack.is_none() {
                "A name, a description, one tag and the pack's file are needed to export."
            } else {
                "A name, a description and one tag are needed to export."
            };
            let galley = wrapped(&painter, needed, 13.0, Weight::Regular, argb(ROW_INK), body.width());
            y += galley.size().y;
            painter.galley(Pos2::new(body.left(), y - galley.size().y), galley, argb(ROW_INK));
            // the choices a tapped row listed, over the sheet
            if let Some((which, anchor, _)) = self.picker {
                self.pick_menu(area, sheet, which, anchor, session);
            }
        });
        if std::mem::take(&mut self.redraw) {
            self.renew_picture(ctx);
            ctx.request_repaint();
        }
        // a press anywhere else closes the picker
        if had_picker
            && ctx.input(|i| i.pointer.any_click())
            && let Some(position) = ctx.input(|i| i.pointer.interact_pos())
        {
            let inside =
                self.picker.is_some_and(|(_, anchor, panel)| panel.contains(position) || anchor.contains(position));
            if !inside {
                self.picker = None;
            }
        }
    }

    fn show_cover(&mut self, ui: &egui::Ui, painter: &egui::Painter, look: &mut Look, body: Rect, y: f32) -> f32 {
        let y = group_head(painter, body, y, "ARTWORK", argb(ROW_INK));
        let edge = Self::artwork_edge(body.width());
        let cover = Rect::from_min_size(egui::pos2(body.left(), y), Vec2::new(edge * 4.0 / 3.0, edge));
        let icon = Rect::from_min_size(egui::pos2(cover.right() + 12.0, y), Vec2::splat(edge));
        let wide = self.custom_cover.as_ref().map(|shot| &shot.texture).or(self.texture.as_ref());
        let square =
            self.custom_icon.as_ref().or(self.square.as_ref()).or(self.custom_cover.as_ref()).map(|shot| &shot.texture);
        for (rect, label, texture) in [(cover, "Cover", wide), (icon, "Square icon", square)] {
            painter.rect_filled(rect, 10.0, argb(crate::store::TILE_FILL));
            if let Some(texture) = texture {
                crate::store_gallery::paint_picture(painter, rect, texture);
            } else if matches!(self.cover_input(), Some(CoverInput::Preparing(_))) {
                text(painter, rect.center(), Align2::CENTER_CENTER, "Preparing…", 15.0, Weight::Regular, argb(ROW_INK));
            }
            text(
                painter,
                rect.left_bottom() + egui::vec2(0.0, 8.0),
                Align2::LEFT_TOP,
                label,
                13.0,
                Weight::Regular,
                argb(ROW_INK),
            );
        }
        let controls = cover.bottom() + 32.0;
        let group = Rect::from_min_size(
            egui::pos2(body.left(), controls),
            Vec2::new(body.width(), ROW_HEIGHT * if self.contents_only() { 2.0 } else { 5.0 }),
        );
        painter.rect_filled(group, 10.0, argb(crate::store::GROUP_FILL));
        let cover_label = self.cover_label();
        for (at, (menu, label, value)) in [
            (COVER_SOURCE, "Cover image", if self.custom_cover.is_some() { "Custom" } else { "Automatic" }),
            (ICON_SOURCE, "Square icon", if self.custom_icon.is_some() { "Custom" } else { "Automatic" }),
            (COVER_MENU, "Cover layout", cover_label.as_str()),
            (ICON_MENU, "Icon layout", self.icon_template.map_or("Match cover layout", CoverTemplate::name)),
        ]
        .into_iter()
        .take(if self.contents_only() { 2 } else { 4 })
        .enumerate()
        {
            let row = group_row(group, at);
            if at > 0 {
                form_rule(painter, group, row.top());
            }
            let automatic = self.cover().is_some_and(|cover| cover.templates().len() > 1);
            let enabled = match menu {
                COVER_MENU => automatic && self.custom_cover.is_none(),
                ICON_MENU => automatic && self.custom_icon.is_none(),
                _ => true,
            };
            self.artwork_row(ui, painter, look, body, row, menu, label, value, enabled);
        }
        if !self.contents_only() {
            let row = group_row(group, 4);
            form_rule(painter, group, row.top());
            let enabled = self.cover().is_some();
            text(
                painter,
                row.left_center() + egui::vec2(INSET, 0.0),
                Align2::LEFT_CENTER,
                "Show title on cover",
                17.0,
                Weight::Regular,
                argb(if enabled { TITLE } else { IDLE }),
            );
            let on = enabled && !self.hide_cover_title;
            let toggle = Rect::from_center_size(row.right_center() - egui::vec2(42.0, 0.0), egui::vec2(51.0, 31.0));
            painter.rect_filled(toggle, 16.0, argb(if on { ACCENT } else { SEP }));
            let knob = toggle.center() + egui::vec2(if on { 10.0 } else { -10.0 }, 0.0);
            painter.circle_filled(knob, 12.0, Color32::WHITE);
            if enabled && ui.interact(row.intersect(body), ui.scope_id().with("cover title"), Sense::click()).clicked()
            {
                self.hide_cover_title = !self.hide_cover_title;
                self.redraw = true;
            }
        }
        group.bottom()
    }

    fn show_gallery(&mut self, ui: &egui::Ui, painter: &egui::Painter, look: &mut Look, body: Rect, y: f32) -> f32 {
        let mut y = group_head(painter, body, y, "GALLERY IMAGES", argb(ROW_INK));
        let width = (body.width() - GAP * (PER_ROW - 1) as f32) / PER_ROW as f32;
        let height = width * THUMB_H / THUMB_W;
        let mut remove = None;
        let visible = self
            .gallery
            .iter()
            .enumerate()
            .filter(|(at, _)| !self.removed_pages.contains(&(*at as u32)))
            .map(|(at, texture)| (Some(at as u32), at, texture))
            .chain(self.pictures.iter().enumerate().map(|(at, shot)| (None, at, &shot.texture)));
        for (cell, (page, at, texture)) in visible.enumerate() {
            let rect = Rect::from_min_size(
                egui::pos2(
                    body.left() + (width + GAP) * (cell % PER_ROW) as f32,
                    y + (height + GAP) * (cell / PER_ROW) as f32,
                ),
                Vec2::new(width, height),
            );
            painter.rect_filled(rect, 10.0, argb(crate::store::TILE_FILL));
            crate::store_gallery::paint_picture(painter, rect, texture);
            let corner = rect.right_top() + egui::vec2(-12.0, 12.0);
            painter.circle_filled(corner, 10.0, Color32::from_black_alpha(175));
            cross(painter, corner, 4.5, Color32::WHITE);
            let button = Rect::from_center_size(corner, Vec2::splat(22.0)).intersect(body);
            if ui
                .interact(button, ui.scope_id().with(("remove gallery image", page, at)), Sense::click())
                .on_hover_text(if page.is_some() { "Remove contents page" } else { "Remove gallery image" })
                .clicked()
            {
                remove = Some((page, at));
            }
        }
        y += self.gallery_rows() as f32 * (height + GAP);
        if let Some((page, at)) = remove {
            if let Some(page) = page {
                self.removed_pages.push(page);
            } else {
                self.pictures.remove(at);
            }
        }
        if !self.gallery_ready && self.gallery_started {
            let note = self.gallery_error.as_deref().unwrap_or("Preparing the contents pages…");
            form_note(painter, body, y + 15.0, note);
            y += GROUP_ROW / 2.0;
        }
        let restore = !self.removed_pages.is_empty();
        let group = Rect::from_min_size(
            egui::pos2(body.left(), y),
            Vec2::new(body.width(), ROW_HEIGHT * if restore { 2.0 } else { 1.0 }),
        );
        painter.rect_filled(group, 10.0, argb(crate::store::GROUP_FILL));
        self.artwork_row(
            ui,
            painter,
            look,
            body,
            group_row(group, 0),
            GALLERY_SOURCE,
            "Add gallery images",
            "",
            self.pictures.len() < MOST_OWN,
        );
        if restore {
            let row = group_row(group, 1);
            form_rule(painter, group, row.top());
            text(
                painter,
                row.left_center() + egui::vec2(INSET, 0.0),
                Align2::LEFT_CENTER,
                "Restore contents pages",
                17.0,
                Weight::Regular,
                argb(ACCENT),
            );
            if ui.interact(row.intersect(body), ui.scope_id().with("restore contents"), Sense::click()).clicked() {
                self.removed_pages.clear();
            }
        }
        group.bottom()
    }

    fn artwork_row(
        &mut self,
        ui: &egui::Ui,
        painter: &egui::Painter,
        look: &mut Look,
        body: Rect,
        row: Rect,
        target: usize,
        label: &str,
        value: &str,
        enabled: bool,
    ) {
        text(
            painter,
            row.left_center() + egui::vec2(INSET, 0.0),
            Align2::LEFT_CENTER,
            label,
            17.0,
            Weight::Regular,
            argb(if enabled { TITLE } else { IDLE }),
        );
        text(
            painter,
            row.right_center() - egui::vec2(INSET + 20.0, 0.0),
            Align2::RIGHT_CENTER,
            value,
            17.0,
            Weight::Regular,
            argb(ROW_INK),
        );
        look.icon_on(
            ui,
            painter,
            &icons::CHEVRON_RIGHT,
            row.right_center() - egui::vec2(INSET + 6.5, 0.0),
            13.0,
            argb(IDLE),
        );
        if enabled
            && ui
                .interact(row.intersect(body), ui.scope_id().with(("artwork source", target)), Sense::click())
                .clicked()
        {
            self.typing = None;
            self.picker = Some((target, row, Rect::NOTHING));
            self.picker_scroll = 0.0;
        }
    }

    fn artwork_edge(width: f32) -> f32 {
        ((width - 12.0) * 3.0 / 7.0).max(1.0)
    }

    fn gallery_rows(&self) -> usize {
        (self.gallery.len().saturating_sub(self.removed_pages.len()) + self.pictures.len()).div_ceil(PER_ROW)
    }

    /// Writes it out: its file, the store's pictures, and a details file saying what a listing
    /// needs.
    fn export(&mut self, session: &Arc<CanvasSession>) {
        let Some(folder) = rfd::FileDialog::new().set_title("Where the set goes").pick_folder() else { return };
        match self.write_package(&folder, session) {
            Ok(dir) => {
                self.open = false;
                crate::files::say(&format!("Store Package saved to {}", dir.display()));
            }
            Err(message) => crate::files::tell("Store", &message),
        }
    }

    fn write_package(&self, folder: &std::path::Path, session: &CanvasSession) -> Result<std::path::PathBuf, String> {
        let cover = if let Some(custom) = &self.custom_cover {
            Some(&custom.image)
        } else if self.contents_only() {
            None
        } else {
            Some(self.picture.as_ref().ok_or("The cover could not be made.")?)
        };
        let asset = if self.kind == ASSET_PACK {
            Some(self.pack.as_ref().ok_or("The pack's file is needed first.")?.path.as_path())
        } else {
            None
        };
        let details = procraft_core::StorePackageDetails {
            name: self.name.clone(),
            author: self.author.clone(),
            description: self.description.clone(),
            category: CATEGORIES.get(self.category).copied().unwrap_or("Brushes").to_owned(),
            licence: LICENCES.get(self.licence).copied().unwrap_or("CC BY 4.0").to_owned(),
            tags: self.tags.clone(),
        };
        let images: Vec<_> = self.pictures.iter().map(|shot| shot.image.clone()).collect();
        let icon =
            self.custom_icon.as_ref().or(self.square.as_ref()).or(self.custom_cover.as_ref()).map(|shot| &shot.image);
        procraft_core::write_store_package_artwork(
            session,
            folder,
            self.library,
            self.set,
            asset,
            &details,
            procraft_core::StorePackageArtwork { cover, icon, removed_pages: &self.removed_pages, maker: &images },
        )
    }

    /// How tall the sheet's content is, worked out from what it holds.
    fn height(&self, width: f32) -> f32 {
        let artwork =
            SECTION_HEAD + Self::artwork_edge(width) + 32.0 + ROW_HEIGHT * if self.contents_only() { 2.0 } else { 5.0 };
        let thumb = (width - GAP * (PER_ROW - 1) as f32) / PER_ROW as f32 * THUMB_H / THUMB_W;
        let gallery = SECTION_HEAD
            + ROW_HEIGHT
            + self.gallery_rows() as f32 * (thumb + GAP)
            + if self.removed_pages.is_empty() { 0.0 } else { ROW_HEIGHT }
            + if !self.gallery_ready && self.gallery_started { ROW_HEIGHT / 2.0 } else { 0.0 };
        let pack = if self.kind == ASSET_PACK { ROW_HEIGHT } else { 0.0 };
        artwork
            + gallery
            + SECTION_HEAD
            + ROW_HEIGHT * 5.0
            + pack
            + 40.0
            + (SECTION_HEAD + ROW_HEIGHT + 20.0) * 2.0
            + 40.0
    }

    /// What the focused field takes.
    fn read_keys(&mut self, ctx: &egui::Context) {
        let Some(field) = self.typing else { return };
        let mut into = match field {
            Field::Name => std::mem::take(&mut self.name),
            Field::Author => std::mem::take(&mut self.author),
            Field::Description => std::mem::take(&mut self.description),
            Field::Tags => std::mem::take(&mut self.tags),
        };
        for event in ctx.input(|i| i.events.clone()) {
            match event {
                egui::Event::Text(typed) => into.push_str(&typed),
                egui::Event::Key { key: egui::Key::Backspace, pressed: true, .. } => {
                    into.pop();
                }
                egui::Event::Key { key: egui::Key::Escape, pressed: true, .. } => self.typing = None,
                _ => {}
            }
        }
        match field {
            Field::Name => self.name = into,
            Field::Author => self.author = into,
            Field::Description => self.description = into,
            Field::Tags => self.tags = into,
        }
    }
}

impl Shot {
    fn new(ctx: &egui::Context, image: image::RgbaImage) -> Self {
        let colour =
            egui::ColorImage::from_rgba_unmultiplied([image.width() as usize, image.height() as usize], image.as_raw());
        let texture = ctx.load_texture("Store artwork", colour, egui::TextureOptions::LINEAR);
        Self { image, texture }
    }
}

fn canvas_picture(picture: &procraft_core::ThumbnailImage) -> Option<image::RgbaImage> {
    let picture = procraft_core::copy_straight(picture);
    image::RgbaImage::from_raw(picture.width, picture.height, picture.rgba)
}

fn clipboard_picture() -> Option<image::RgbaImage> {
    if let Some(picture) = crate::system_clipboard::own_copy_unless_replaced() {
        let picture = canvas_picture(&picture)?;
        return Some(image::DynamicImage::ImageRgba8(picture).thumbnail(1600, 1600).into_rgba8());
    }
    let mut clipboard = arboard::Clipboard::new().ok()?;
    if let Ok(picture) = clipboard.get_image() {
        let (w, h) = (u32::try_from(picture.width).ok()?, u32::try_from(picture.height).ok()?);
        let rgba = image::RgbaImage::from_raw(w, h, picture.bytes.into_owned())?;
        return Some(image::DynamicImage::ImageRgba8(rgba).thumbnail(1600, 1600).into_rgba8());
    }
    clipboard.get().file_list().ok()?.iter().find_map(|path| procraft_core::read_store_maker_image(path))
}

fn group_row(group: Rect, at: usize) -> Rect {
    Rect::from_min_size(group.left_top() + Vec2::new(0.0, at as f32 * ROW_HEIGHT), Vec2::new(group.width(), ROW_HEIGHT))
}

fn form_note(painter: &egui::Painter, body: Rect, y: f32, words: &str) {
    crate::store::ellipsized(
        painter,
        egui::pos2(body.left() + INSET, y),
        words,
        body.width() - 2.0 * INSET,
        13.0,
        Weight::Regular,
        argb(ROW_INK),
    );
}

fn preview_image(preview: StorePreview) -> Option<image::RgbaImage> {
    image::RgbaImage::from_raw(preview.width, preview.height, preview.rgba)
}

/// A heading over a group of the sheet, small caps at its leading edge.
fn form_rule(painter: &egui::Painter, group: Rect, y: f32) {
    painter.line_segment(
        [egui::pos2(group.left() + INSET, y), egui::pos2(group.right() - INSET, y)],
        Stroke::new(1.0, argb(SEP)),
    );
}

/// The caret at the end of what a focused field holds.
fn circle_caret(painter: &egui::Painter, x: f32, y: f32) {
    painter.line_segment([Pos2::new(x, y - 10.0), Pos2::new(x, y + 10.0)], Stroke::new(2.0, argb(ACCENT)));
}

/// The cross that takes a picture away again.
fn cross(painter: &egui::Painter, centre: Pos2, size: f32, colour: Color32) {
    let stroke = Stroke::new(1.6, colour);
    painter.line_segment(
        [Pos2::new(centre.x - size, centre.y - size), Pos2::new(centre.x + size, centre.y + size)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x + size, centre.y - size), Pos2::new(centre.x - size, centre.y + size)],
        stroke,
    );
}

/// A picture as the bytes of a PNG file.
#[cfg(test)]
fn png_bytes(picture: &image::RgbaImage) -> Option<Vec<u8>> {
    let mut png = Vec::new();
    image::write_buffer_with_format(
        &mut std::io::Cursor::new(&mut png),
        picture.as_raw(),
        picture.width(),
        picture.height(),
        image::ExtendedColorType::Rgba8,
        image::ImageFormat::Png,
    )
    .ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_share_choice_writes_the_native_file_for_its_target() {
        let session = CanvasSession::new(64, 64);
        let set = session.new_brush_set();
        let brush = session.new_brush(set).unwrap();
        let destination = tempfile::tempdir().unwrap();
        for target in [BrushShare::Brush(set, brush), BrushShare::Set(set, 0), BrushShare::Library] {
            let (_, extension) = target.format();
            let path = destination.path().join(format!("Studio.{extension}"));
            target.write(&session, &path).unwrap();
            let listed = procraft_core::store_shared_file(path.to_string_lossy().into_owned(), None).unwrap();
            assert!(listed.brushes, "{extension}");
            assert!(std::fs::metadata(&path).unwrap().len() > 0);
        }
    }

    #[test]
    fn the_three_things_a_listing_needs_are_what_makes_an_export_ready() {
        let mut package = StorePackage::default();
        package.reset("Dry Ink", 3);
        assert!(!package.ready(), "a new package has no description and no tags");
        package.description = "Six brushes.".into();
        assert!(!package.ready());
        package.tags = " ink , rough ".into();
        assert!(package.ready());
        package.name.clear();
        assert!(!package.ready());
    }

    #[test]
    fn package_export_uses_the_selected_untitled_cover_and_keeps_the_catalogue_references() {
        let dir = std::env::temp_dir().join(format!("procraft-package-cover-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let scan = image::RgbaImage::from_pixel(144, 192, image::Rgba([209, 166, 115, 255]));
        let mut zip = procraft_engine::zip::ZipWriter::new();
        zip.add("assets.json", br#"{"format":"procraft-assets","version":1,"kind":"set","name":"Amber Paper","libraries":[{"folders":[{"name":"Papers","assets":[{"id":"first","name":"Amber"}]}]}]}"#);
        zip.add("items/first/thumbnail.png", &png_bytes(&scan).unwrap());
        let path = dir.join("amber.assetset");
        let file = zip.finish();
        std::fs::write(&path, &file).unwrap();
        let cover = procraft_core::asset_pack_store_cover(&path).unwrap();
        let ctx = egui::Context::default();
        let mut package = StorePackage {
            name: "Amber Paper".to_owned(),
            category: 1,
            kind: ASSET_PACK,
            cover_template: Some(CoverTemplate::PaperPoster),
            hide_cover_title: true,
            pack: Some(Pack {
                path,
                name: "Amber Paper".to_owned(),
                brushes: false,
                cover: CoverInput::Ready(cover),
                _scratch: None,
            }),
            ..StorePackage::default()
        };
        package.renew_picture(&ctx);
        let session = CanvasSession::new(16, 16);
        let exported = package.write_package(&dir, &session).unwrap();
        let details = std::fs::read_to_string(exported.join("details.json")).unwrap();
        assert!(details.contains(r#""preview":"preview.png""#));
        assert!(details.contains(r#""pictures":["preview.png","contents-1.png"]"#));
        assert_eq!(std::fs::read(exported.join("file.assetset")).unwrap(), file);
        let saved = image::open(exported.join("preview.png")).unwrap().to_rgba8();
        assert_eq!(&saved, package.picture.as_ref().unwrap());
        assert_ne!(saved.as_raw(), &package.cover().unwrap().render(None, Some("Amber Paper")).rgba);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

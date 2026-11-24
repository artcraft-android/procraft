//! The store package: one brush set leaving the app as the folder a store reads - the set's own
//! file and a details file saying what a listing needs, written where a person picks.

use std::sync::Arc;

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Vec2};
use procraft_core::CanvasSession;

use crate::look::{Look, Weight, argb, text, text_width, wrapped};
use crate::{icons, ui};

/// The sheet, its bar, and how far its content sits in from its sides.
const WIDTH: f32 = 520.0;
const HEIGHT: f32 = 620.0;
const CORNER: f32 = 14.0;
const BAR: f32 = 56.0;
const INSET: f32 = 16.0;
const ROW_HEIGHT: f32 = 56.0;
const THUMB_W: f32 = 108.0;
const THUMB_H: f32 = 81.0;
/// The strip lays its pictures four across, and holds four of their own after the drawn one.
const PER_ROW: usize = 4;
const GAP: f32 = 8.0;
const MOST_OWN: usize = 4;

use crate::store::{ACCENT, BG, HINT, IDLE, ROW_INK, SEP, TITLE};

/// The categories a listing may carry, as the store's own list.
const CATEGORIES: [&str; 8] =
    ["Brushes", "Papers and Textures", "Colour", "Layers", "Interface", "Effects", "Canvas", "Other"];
/// The licences a maker may pick from, the common ones written out.
const LICENCES: [&str; 5] = ["CC0-1.0", "CC BY 4.0", "CC BY-SA 4.0", "MIT", "All rights reserved"];

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
    /// The set it is making, its name as the library has it, and its icon.
    set: u32,
    icon: usize,
    name: String,
    author: String,
    description: String,
    tags: String,
    category: usize,
    licence: usize,
    typing: Option<Field>,
    scroll: f32,
    /// The picker open under a row: which row asked, the row itself, and the panel it opened.
    picker: Option<(usize, Rect, Rect)>,
    /// The library the set is in, the picture the app drew for it, and its handle on the GPU.
    library: u32,
    picture: Option<image::RgbaImage>,
    texture: Option<egui::TextureHandle>,
    /// The pictures they took themselves.
    pictures: Vec<Shot>,
}

/// One of their own pictures: its pixels, and its handle on the GPU.
struct Shot {
    image: image::RgbaImage,
    texture: egui::TextureHandle,
}

impl StorePackage {
    /// Begins a package for `set`, in the library in use, drawing its picture from the set itself.
    pub fn begin(
        &mut self,
        ctx: &egui::Context,
        session: &CanvasSession,
        set: u32,
        name: &str,
        icon: usize,
    ) {
        self.reset(name, icon);
        self.set = set;
        self.library = session.brush_libraries().iter().position(|library| library.current).unwrap_or(0) as u32;
        self.picture = drawn_picture(session, self.library, set);
        let texture = self.picture.as_ref().map(|picture| {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [picture.width() as usize, picture.height() as usize],
                picture.as_raw(),
            );
            ctx.load_texture("store package picture", image, egui::TextureOptions::LINEAR)
        });
        self.texture = texture;
    }

    /// The sheet's fields, ready for a new package.
    fn reset(&mut self, name: &str, icon: usize) {
        self.open = true;
        self.set = 0;
        self.icon = icon;
        self.name = name.to_owned();
        self.author.clear();
        self.description.clear();
        self.tags.clear();
        self.category = 0;
        self.licence = 1;
        self.typing = None;
        self.scroll = 0.0;
        self.picker = None;
        self.picture = None;
        self.texture = None;
        self.pictures.clear();
    }

    /// Their own pictures, taken from the files they pick: four at most after the drawn one.
    fn pick_pictures(&mut self, ctx: &egui::Context) {
        let room = MOST_OWN.saturating_sub(self.pictures.len());
        if room == 0 {
            return;
        }
        let Some(paths) = rfd::FileDialog::new().add_filter("Pictures", &["png", "jpg", "jpeg", "webp"]).pick_files()
        else {
            return;
        };
        for path in paths.into_iter().take(room) {
            let Ok(picture) = image::open(&path) else {
                crate::files::tell("Store", &format!("{} could not be read.", path.display()));
                continue;
            };
            let image = picture.to_rgba8();
            let colour = egui::ColorImage::from_rgba_unmultiplied(
                [image.width() as usize, image.height() as usize],
                image.as_raw(),
            );
            let texture = ctx.load_texture(
                format!("store package picture {}", self.pictures.len()),
                colour,
                egui::TextureOptions::LINEAR,
            );
            self.pictures.push(Shot { image, texture });
        }
    }

    /// The categories or the licences, listed under the row that asked for one.
    fn pick_menu(&mut self, ui: &egui::Ui, sheet: Rect, which: usize, anchor: Rect) {
        let (options, chosen) =
            if which == 0 { (CATEGORIES.to_vec(), self.category) } else { (LICENCES.to_vec(), self.licence) };
        let width = 262.0;
        let height = options.len() as f32 * 44.0 + 12.0;
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
        let mut y = panel.top() + 6.0;
        for (at, option) in options.iter().enumerate() {
            let row = Rect::from_min_size(Pos2::new(panel.left(), y), Vec2::new(width, 44.0));
            let on = at == chosen;
            text(ui.painter(), Pos2::new(row.left() + 18.0, row.center().y), Align2::LEFT_CENTER, option, 16.0,
                Weight::Regular, argb(if on { TITLE } else { ROW_INK }));
            let radio = Pos2::new(row.right() - 27.0, row.center().y);
            ui.painter().circle_stroke(radio, 8.5, Stroke::new(1.5, argb(if on { ACCENT } else { IDLE })));
            if on {
                ui.painter().circle_filled(radio, 4.5, argb(ACCENT));
            }
            if ui.interact(row, ui.scope_id().with(("package pick row", at)), Sense::click()).clicked() {
                if which == 0 {
                    self.category = at;
                } else {
                    self.licence = at;
                }
                self.picker = None;
            }
            y += 44.0;
        }
    }

    /// The three things a listing cannot do without: a name, a description and at least one tag.
    fn ready(&self) -> bool {
        !self.name.trim().is_empty() && !self.description.trim().is_empty() && !self.tags.trim().is_empty()
    }

    /// Draws the sheet over everything it opened from. Closes on Cancel, on Escape, or on a press
    /// beside it.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &Arc<CanvasSession>) {
        if !self.open {
            return;
        }
        if ui::escape(ctx) {
            match self.typing.take() {
                Some(_) => {}
                None => {
                    self.open = false;
                    return;
                }
            }
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
            // the bar: Cancel, the sheet's name, and the Export that is the signal when it cannot be taken
            let bar = Rect::from_min_size(sheet.min, Vec2::new(sheet.width(), BAR));
            area.painter().line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, argb(SEP)));
            text(area.painter(), Pos2::new(bar.left() + INSET, bar.center().y), Align2::LEFT_CENTER, "Cancel",
                17.0, Weight::Regular, argb(ACCENT));
            text(area.painter(), Pos2::new(bar.center().x, bar.center().y), Align2::CENTER_CENTER, "Store Details",
                17.0, Weight::SemiBold, argb(TITLE));
            let ready = self.ready();
            text(area.painter(), Pos2::new(bar.right() - INSET, bar.center().y), Align2::RIGHT_CENTER, "Export",
                17.0, Weight::SemiBold, argb(if ready { ACCENT } else { HINT }));
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
            let height = self.height();
            let most = (height - body.height()).max(0.0);
            let over = area.input(|i| i.pointer.latest_pos()).is_some_and(|p| body.contains(p));
            let delta = if over { area.input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
            self.scroll = (self.scroll - delta).clamp(0.0, most);
            let painter = area.painter().with_clip_rect(body.intersect(area.clip_rect()));
            let mut y = body.top() + 16.0 - self.scroll;
            // the pictures: the one the app drew from the set, then their own, then the way to add
            heading(&painter, body, y, "SCREENSHOTS");
            y += 18.0;
            let mut cells = 1 + self.pictures.len();
            if self.pictures.len() < MOST_OWN {
                cells += 1;
            }
            let mut drop: Option<usize> = None;
            let mut take = false;
            for at in 0..cells {
                let left = body.left() + (THUMB_W + GAP) * (at % PER_ROW) as f32;
                let top = y + (THUMB_H + GAP) * (at / PER_ROW) as f32;
                let thumb = Rect::from_min_size(Pos2::new(left, top), Vec2::new(THUMB_W, THUMB_H));
                let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
                if at == 0 {
                    painter.rect_filled(thumb, 10.0, argb(crate::store::PAPER));
                    match &self.texture {
                        Some(texture) => {
                            painter.image(texture.id(), thumb, uv, Color32::WHITE);
                        }
                        None => {
                            let icon = icons::SET_ICONS.get(self.icon).unwrap_or(&icons::BRUSH_SET);
                            look.icon_on(area, &painter, icon, thumb.center(), 30.0, argb(IDLE));
                        }
                    }
                } else if at <= self.pictures.len() {
                    painter.rect_filled(thumb, 10.0, argb(crate::store::PAPER));
                    if let Some(shot) = self.pictures.get(at - 1) {
                        painter.image(shot.texture.id(), thumb, uv, Color32::WHITE);
                    }
                    // a tap on its corner takes one of their own away again
                    let corner = Pos2::new(thumb.right() - 4.0, thumb.top() + 4.0);
                    painter.circle_filled(corner, 9.0, Color32::from_black_alpha(150));
                    cross(&painter, corner, 4.5, Color32::WHITE);
                    let corner = Rect::from_center_size(corner, Vec2::splat(20.0));
                    if area.interact(corner, area.scope_id().with(("package drop", at)), Sense::click()).clicked() {
                        drop = at.checked_sub(1);
                    }
                } else {
                    painter.rect_stroke(thumb, 10.0, Stroke::new(2.0, argb(SEP)), egui::StrokeKind::Inside);
                    look.icon_on(area, &painter, &icons::PLUS_MEDIUM, thumb.center(), 22.0, argb(ROW_INK));
                    if area.interact(thumb, area.scope_id().with("package add"), Sense::click()).clicked() {
                        take = true;
                    }
                }
            }
            if let Some(at) = drop {
                self.pictures.remove(at);
            }
            if take {
                self.pick_pictures(ctx);
            }
            y += (THUMB_H + GAP) * cells.div_ceil(PER_ROW) as f32;
            text(&painter, Pos2::new(body.left(), y), Align2::LEFT_TOP,
                "The app drew the first one from the set.", 13.0, Weight::Regular, argb(ROW_INK));
            y += 28.0;
            // the details, one row each, the two pickers reading the file's own
            heading(&painter, body, y, "DETAILS");
            y += 18.0;
            let fields = [
                (Field::Name, "Name", self.name.clone()),
                (Field::Author, "Author", self.author.clone()),
            ];
            for (field, label, value) in fields {
                let row = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), ROW_HEIGHT));
                painter.rect_filled(row, 10.0, argb(crate::store::GROUP_FILL));
                text(&painter, Pos2::new(row.left() + 16.0, row.center().y), Align2::LEFT_CENTER, label, 17.0,
                    Weight::Regular, argb(TITLE));
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
                if area.interact(row, area.scope_id().with(("package row", label)), Sense::click()).clicked() {
                    self.typing = Some(field);
                }
                y += ROW_HEIGHT;
            }
            // Kind is what the set is; the category and the licence are the maker's to say
            let line = |at: usize, list: &[&str], fallback: &str| list.get(at).copied().unwrap_or(fallback).to_owned();
            for (at, (label, value)) in [("Kind", "Brush set".to_owned()),
                ("Category", line(self.category, &CATEGORIES, "Brushes")),
                ("Licence", line(self.licence, &LICENCES, "CC BY 4.0"))]
                .into_iter()
                .enumerate()
            {
                let row = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), ROW_HEIGHT));
                painter.rect_filled(row, 10.0, argb(crate::store::GROUP_FILL));
                text(&painter, Pos2::new(row.left() + 16.0, row.center().y), Align2::LEFT_CENTER, label, 17.0,
                    Weight::Regular, argb(TITLE));
                text(&painter, Pos2::new(row.right() - 16.0 - 12.0, row.center().y), Align2::RIGHT_CENTER, &value,
                    17.0, Weight::Regular, argb(ROW_INK));
                look.icon_on(area, &painter, &icons::CHEVRON_LEFT, Pos2::new(row.right() - 14.0, row.center().y), 13.0,
                    argb(IDLE));
                if area.interact(row, area.scope_id().with(("package pick", at)), Sense::click()).clicked()
                    && label != "Kind"
                {
                    // Category and Licence list what they may say under the row that was tapped
                    self.picker = Some((at.saturating_sub(1), row, Rect::NOTHING));
                }
                y += ROW_HEIGHT;
            }
            y += 8.0;
            text(&painter, Pos2::new(body.left(), y), Align2::LEFT_TOP,
                "Kind and category are read from the file. Change them if they are wrong.", 13.0, Weight::Regular,
                argb(ROW_INK));
            y += 32.0;
            // the words, and the tags that make it findable
            for (field, heading_text, hint) in [
                (Field::Description, "DESCRIPTION", "One sentence on what the set does"),
                (Field::Tags, "TAGS", "Comma separated"),
            ] {
                heading(&painter, body, y, heading_text);
                y += 18.0;
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
                if area.interact(row, area.scope_id().with(("package field", heading_text)), Sense::click()).clicked() {
                    self.typing = Some(field);
                }
                y += ROW_HEIGHT + 20.0;
            }
            // the three things a listing cannot do without, named once
            let galley = wrapped(&painter, "A name, a description and one tag are needed to export.", 13.0,
                Weight::Regular, argb(ROW_INK), body.width());
            y += galley.size().y;
            painter.galley(Pos2::new(body.left(), y - galley.size().y), galley, argb(ROW_INK));
            // the choices a tapped row listed, over the sheet
            if let Some((which, anchor, _)) = self.picker {
                self.pick_menu(area, sheet, which, anchor);
            }
        });
        // a press anywhere else closes the picker
        if had_picker
            && ctx.input(|i| i.pointer.any_click())
            && let Some(position) = ctx.input(|i| i.pointer.interact_pos())
        {
            let inside = self
                .picker
                .is_some_and(|(_, anchor, panel)| panel.contains(position) || anchor.contains(position));
            if !inside {
                self.picker = None;
            }
        }
    }

    /// Writes the set out: its file, and a details file saying what a listing needs.
    fn export(&mut self, session: &Arc<CanvasSession>) {
        let Some(folder) = rfd::FileDialog::new().set_title("Where the set goes").pick_folder() else { return };
        let Some(bytes) = session.brush_set_as_file(self.set) else {
            crate::files::tell("Store", "The set could not be written.");
            return;
        };
        let name = self.name.trim().to_owned();
        let dir = folder.join(folder_of(&name));
        if let Err(e) = std::fs::create_dir_all(&dir) {
            crate::files::tell("Store", &format!("The folder could not be made: {e}"));
            return;
        }
        if let Err(e) = std::fs::write(dir.join("file.brushset"), &bytes) {
            crate::files::tell("Store", &format!("The set could not be written: {e}"));
            return;
        }
        // the drawn picture first, then their own, so the store shows them in that order
        let mut pictures = Vec::new();
        match self.picture.as_ref().and_then(png_bytes).map(|png| std::fs::write(dir.join("preview.png"), png)) {
            Some(Err(e)) => {
                crate::files::tell("Store", &format!("The picture could not be written: {e}"));
                return;
            }
            Some(Ok(())) => pictures.push("preview.png".to_owned()),
            None => {}
        }
        for (at, shot) in self.pictures.iter().enumerate() {
            let Some(png) = png_bytes(&shot.image) else { continue };
            let named = format!("picture-{}.png", at + 1);
            if let Err(e) = std::fs::write(dir.join(&named), png) {
                crate::files::tell("Store", &format!("A picture could not be written: {e}"));
                return;
            }
            pictures.push(named);
        }
        let tags = tags_of(&self.tags);
        let category = CATEGORIES.get(self.category).copied().unwrap_or("Brushes");
        let licence = LICENCES.get(self.licence).copied().unwrap_or("CC BY 4.0");
        let details = procraft_core::package_details(
            "brushset",
            &name,
            self.author.trim(),
            self.description.trim(),
            category,
            licence,
            &tags,
            &pictures,
            &crate::store::date_text(crate::store::now_secs(), true),
        );
        if let Err(e) = std::fs::write(dir.join("details.json"), details) {
            crate::files::tell("Store", &format!("The details could not be written: {e}"));
            return;
        }
        self.open = false;
        crate::files::tell("Store", &format!("Written to {}", dir.display()));
    }

    /// How tall the sheet's content is, worked out from what it holds.
    fn height(&self) -> f32 {
        let cells = 1 + self.pictures.len() + usize::from(self.pictures.len() < MOST_OWN);
        let rows = cells.div_ceil(PER_ROW) as f32;
        16.0 + 18.0 + (THUMB_H + GAP) * rows + 28.0
            + 18.0
            + ROW_HEIGHT * 5.0
            + 8.0
            + 32.0
            + (18.0 + ROW_HEIGHT + 20.0) * 2.0
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

/// A heading over a group of the sheet, small caps at its leading edge.
fn heading(painter: &egui::Painter, body: Rect, y: f32, words: &str) {
    text(painter, Pos2::new(body.left(), y), Align2::LEFT_TOP, words, 13.0, Weight::Regular, argb(ROW_INK));
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

/// The tags a typed line holds: split on commas, trimmed, each one kept.
fn tags_of(line: &str) -> Vec<String> {
    line.split(',').map(str::trim).filter(|tag| !tag.is_empty()).map(str::to_owned).collect()
}

/// The folder a set makes: its name, with anything a file system would refuse left out.
fn folder_of(name: &str) -> String {
    let kept: String =
        name.chars().map(|c| if c.is_alphanumeric() || c == ' ' || c == '-' || c == '_' { c } else { '-' }).collect();
    let trimmed = kept.trim().to_owned();
    if trimmed.is_empty() {
        "Untitled Set".to_owned()
    } else {
        trimmed
    }
}

/// The picture the app draws for a set: its first brush's sample stroke on warm paper, in the 4:3
/// frame the store shows, eight per cent kept clear as a margin.
fn drawn_picture(session: &CanvasSession, library: u32, set: u32) -> Option<image::RgbaImage> {
    let margin = margin();
    let sample = session.brush_library_preview(library, set, 0, PICTURE_W - margin * 2, PICTURE_H - margin * 2)?;
    Some(picture_of(&sample))
}

/// The store's 4:3 frame, and how much of it stays clear around the sample.
const PICTURE_W: u32 = 520;
const PICTURE_H: u32 = 377;

fn margin() -> u32 {
    (PICTURE_W as f32 * 0.08).round() as u32
}

/// A sample stroke laid on warm paper, a margin kept clear around it.
fn picture_of(sample: &procraft_core::ThumbnailImage) -> image::RgbaImage {
    let margin = margin();
    let stroke = image::RgbaImage::from_raw(sample.width, sample.height, sample.rgba.clone());
    let mut paper = image::RgbaImage::from_pixel(PICTURE_W, PICTURE_H, image::Rgba([0xFB, 0xF9, 0xF4, 0xFF]));
    if let Some(stroke) = stroke {
        image::imageops::overlay(&mut paper, &stroke, i64::from(margin), i64::from(margin));
    }
    paper
}

/// A picture as the bytes of a PNG file.
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
    fn the_drawn_picture_keeps_its_margin_and_lays_the_sample_on_paper() {
        // a block of red across the sample, so where it lands in the frame shows
        let sample = procraft_core::ThumbnailImage { width: 8, height: 6, rgba: [255u8, 0, 0, 255].repeat(48) };
        let picture = picture_of(&sample);
        assert_eq!((picture.width(), picture.height()), (PICTURE_W, PICTURE_H));
        assert_eq!(picture.get_pixel(0, 0).0, [0xFB, 0xF9, 0xF4, 0xFF], "the corner is paper");
        let margin = margin();
        assert_eq!(picture.get_pixel(margin - 1, margin - 1).0, [0xFB, 0xF9, 0xF4, 0xFF], "the margin is clear");
        assert_eq!(picture.get_pixel(margin + 4, margin + 3).0, [255, 0, 0, 255], "the sample is laid in");
        assert_eq!(picture.get_pixel(PICTURE_W - 1, PICTURE_H - 1).0, [0xFB, 0xF9, 0xF4, 0xFF]);
    }

    #[test]
    fn tags_are_split_and_the_folder_keeps_only_what_a_file_system_will_take() {
        assert_eq!(tags_of(" ink , rough,, paper "), vec!["ink", "rough", "paper"]);
        assert_eq!(folder_of("Dry Ink: Six/Sets"), "Dry Ink- Six-Sets");
        assert_eq!(folder_of("  "), "Untitled Set");
    }
}

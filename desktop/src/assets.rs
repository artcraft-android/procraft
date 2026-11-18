//! The asset library: a global shelf of reusable materials, shared by every artwork. The library
//! in use's folders on the left, the shown folder's assets as thumbnails on the right; the title
//! opens every library's card. Tapping an asset puts it on the canvas ready to transform. The plus
//! captures what is on the canvas or brings pictures, PSDs and whole projects in; a new asset is
//! named right away.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use egui::text::{CCursor, CCursorRange, LayoutJob, TextFormat, TextWrapping};
use egui::{
    Align2, Color32, Pos2, Rect, ScrollArea, Sense, Stroke, TextureHandle, TextureOptions, Ui, UiBuilder, pos2, vec2,
};
use procraft_core::{AssetFolderInfo, AssetInfo, AssetLibraryInfo, AssetScope, CanvasSession, PsdImportMode};

use crate::alert::{Alert, Answer, Asking};
use crate::icons;
use crate::look::{Look, Shade, Themed, Weight, argb, pc, text};
use crate::menu::{self, Entry, Menu};
use crate::panel::{self, PANEL_GLYPH, PANEL_TITLE};

const ASSETS_WIDTH: f32 = 440.0;
const RAIL_WIDTH: f32 = 168.0;
const HEADER_HEIGHT: f32 = 52.0;
/// The panel ends this far above the window's bottom edge.
const BOTTOM_MARGIN: f32 = 25.0;
const ROW_HEIGHT: f32 = 50.0;
const RAIL_COLUMN: Themed = Themed(0x00000000, 0xFFE6E6E6);
const RAIL_CELL: Themed = Themed(0x2E5B5B5B, 0xFFD7D7D7);
const GRID_COLUMN: Themed = Themed(0x26000000, 0xFFE0E0E0);
const CELL_FILL: u32 = 0x14FFFFFF;
const CELL_GAP: f32 = 8.0;
const CELL_PAD: f32 = 12.0;
/// The name's strip under a thumbnail.
const NAME_AREA: f32 = 24.0;
const COLUMNS: usize = 2;
/// A library's card: its size, the gap between cards, its corner, its folders' rows and its
/// assets' rows.
const CARD_HEIGHT: f32 = 210.0;
const CARD_GAP: f32 = 8.0;
const CARD_CORNER: u8 = 16;
const CARD_FOLDER_ROW: f32 = 19.0;
const CARD_FOLDER_GAP: f32 = 3.0;
const CARD_ASSET: f32 = 52.0;
/// A folder's row on a card, not the one shown.
const CARD_FOLDER_FILL: Themed = Themed(0x59000000, 0x14000000);
/// Menus stay below the top bar.
const MENU_TOP: f32 = 57.0;
const MENU_GLASS: menu::Glass =
    menu::Glass { fill: menu::GLASS, edge: Some(menu::EDGE), frost: true, corner: menu::LIBRARY_CORNER, shadow: false };
/// A press moved this far (points) is the list's, to scroll; held still this long, its menu opens.
const SLOP: f32 = 8.0;
const HOLD: Duration = Duration::from_millis(400);
/// Thumbnails drawn on a thread of their own, a few at a time, and how often they are looked for.
const PREVIEW_SIDE: u32 = 256;
const PREVIEWS_AT_A_TIME: usize = 4;
const PREVIEW_LOOK: Duration = Duration::from_millis(16);

/// What Delete asks about.
enum Deleting {
    Library(u32),
    Folder(u32, u32),
    Asset(String),
}

/// A press on an asset, followed until let go: where and when it went down, how far the grid was
/// scrolled, and whether it turned into a scroll or was held for its menu.
#[derive(Clone)]
struct Press {
    id: String,
    origin: Pos2,
    since: Instant,
    list: f32,
    scrolling: bool,
    held: bool,
}

/// A press on a folder row, followed until let go: where and when it went down, how far the rail
/// was scrolled, and whether it turned into a scroll or was held for its menu.
#[derive(Clone, Copy)]
struct RailPress {
    row: usize,
    origin: Pos2,
    since: Instant,
    list: f32,
    scrolling: bool,
    held: bool,
}

/// A press on a library's card, followed until let go: where and when it went down, and whether
/// it turned into a scroll or was held for its menu.
#[derive(Clone, Copy)]
struct CardPress {
    card: usize,
    origin: Pos2,
    since: Instant,
    scrolling: bool,
    held: bool,
}

/// A file brought in on a thread of its own.
enum ImportJob {
    Image(Vec<u8>, String),
    Psd(Vec<u8>, String, PsdImportMode),
    Project(String, String),
}

/// An import waited on: where it lands, and the folder's asset ids before it, so the new one can
/// be found and named once it is kept.
struct ImportWait {
    done: std::sync::mpsc::Receiver<bool>,
    library: u32,
    folder: u32,
    before: Vec<String>,
}

/// A shared asset file waited on: where it was aimed, and the shelf's shape before it, so where
/// it landed can be shown once it is kept.
struct FileWait {
    done: std::sync::mpsc::Receiver<bool>,
    library: u32,
    folder: u32,
    libraries: usize,
    folders: usize,
}

#[derive(Default)]
pub struct Assets {
    /// The folder shown on the right, in the library in use.
    shown_folder: usize,
    /// The library in use last frame, to put the first folder back when it changes.
    last_library: Option<usize>,
    plus_menu: Menu,
    /// A folder row's menu, and which row it is for.
    row_menu: Menu,
    row_menu_for: Option<usize>,
    /// An asset's menu, and the asset it is for.
    asset_menu: Menu,
    asset_menu_for: Option<String>,
    /// The folders an asset can be moved to, and the asset being moved.
    move_menu: Menu,
    move_for: Option<String>,
    /// The ways an asset's thumbnail can be set, and the asset they are for.
    thumb_menu: Menu,
    thumb_for: Option<String>,
    /// Every library's card shows rather than the one in use; the title's menu; the plus's menu
    /// over the cards; a card's menu and for which; where the cards were last drawn.
    showing_libraries: bool,
    title_menu: Menu,
    libraries_plus: Menu,
    card_menu: Menu,
    card_menu_for: Option<usize>,
    card_rects: Vec<(usize, Rect)>,
    /// A library, folder or asset renamed in place: which, the name as typed, whether it has been
    /// focused.
    renaming_library: Option<(usize, String, bool)>,
    renaming_folder: Option<(usize, String, bool)>,
    renaming_asset: Option<(String, String, bool)>,
    deleting: Alert,
    delete_what: Option<Deleting>,
    /// A PSD picked, waiting to be asked how its layers come in.
    psd_alert: Alert,
    psd_pending: Option<(Vec<u8>, String)>,
    /// Each asset's thumbnail by id (none: it can't be read), those still to draw, and the thread
    /// drawing them.
    previews: HashMap<String, Option<TextureHandle>>,
    wanted: Vec<String>,
    drawing: Option<std::sync::mpsc::Receiver<(String, Option<egui::ColorImage>)>>,
    /// Files being brought in on threads of their own.
    importing: Vec<ImportWait>,
    /// Shared asset files being brought in on threads of their own.
    file_importing: Vec<FileWait>,
    press: Option<Press>,
    rail_press: Option<RailPress>,
    card_press: Option<CardPress>,
    /// How far the grid and the rail are scrolled.
    grid_at: f32,
    rail_at: f32,
    /// Where the cells and rail rows were last drawn, for the menus.
    cell_rects: Vec<(String, Rect)>,
    rail_rects: Vec<(usize, Rect)>,
    /// An asset was placed: the app transforms it.
    pub placed: bool,
    /// Something to say about a capture or an import.
    pub notice: Option<(String, bool)>,
}

impl Assets {
    /// A menu or question of the panel's is up: a press elsewhere only closes it.
    pub fn modal(&self) -> bool {
        self.plus_menu.is_open()
            || self.row_menu.is_open()
            || self.asset_menu.is_open()
            || self.move_menu.is_open()
            || self.thumb_menu.is_open()
            || self.title_menu.is_open()
            || self.libraries_plus.is_open()
            || self.card_menu.is_open()
            || self.deleting.is_open()
            || self.psd_alert.is_open()
    }

    /// The artwork changed: the panel's transient state goes, its shelf and thumbnails stay.
    pub fn forget(&mut self) {
        self.press = None;
        self.rail_press = None;
        self.card_press = None;
        self.placed = false;
        self.notice = None;
        self.plus_menu.close();
        self.row_menu.close();
        self.asset_menu.close();
        self.move_menu.close();
        self.thumb_menu.close();
        self.title_menu.close();
        self.libraries_plus.close();
        self.card_menu.close();
        self.row_menu_for = None;
        self.asset_menu_for = None;
        self.move_for = None;
        self.thumb_for = None;
        self.card_menu_for = None;
        self.renaming_library = None;
        self.renaming_folder = None;
        self.renaming_asset = None;
        self.deleting = Alert::default();
        self.psd_alert = Alert::default();
        self.psd_pending = None;
    }

    /// Shows the panel hanging from `button`; returns where it is (points).
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &Arc<CanvasSession>,
        button: Rect,
        backdrops: &mut Vec<f32>,
    ) -> Rect {
        let screen = ctx.content_rect();
        let (left, arrow_x) = panel::place(ctx, ASSETS_WIDTH, button.center().x);
        let top = screen.top() + pc::BAR_HEIGHT;
        let rect = Rect::from_min_size(
            pos2(left, top),
            vec2(ASSETS_WIDTH, (screen.bottom() - BOTTOM_MARGIN - top).max(200.0)),
        );
        let libraries = session.asset_libraries();
        let current = libraries.iter().position(|l| l.current).unwrap_or(0);
        if self.last_library != Some(current) {
            self.last_library = Some(current);
            self.shown_folder = 0;
        }
        let folders = libraries.get(current).map(|l| l.folders.as_slice()).unwrap_or(&[]);
        self.shown_folder = self.shown_folder.min(folders.len().saturating_sub(1));
        let shown = self.shown_folder;
        let assets = folders.get(shown).map(|f| f.assets.as_slice()).unwrap_or(&[]);
        // thumbnails of assets that are gone are let go of
        let ids: HashSet<&str> =
            libraries.iter().flat_map(|l| &l.folders).flat_map(|f| &f.assets).map(|a| a.id.as_str()).collect();
        self.previews.retain(|id, _| ids.contains(id.as_str()));
        self.wanted.clear();

        let mut rail_rects = Vec::new();
        let mut cell_rects = Vec::new();
        let mut plus = Rect::NOTHING;
        let mut title_row = Rect::NOTHING;
        let mut folder_tapped = None;
        let mut row_menu_open = None;
        let mut asset_menu_open = None;
        egui::Area::new(egui::Id::unique("asset library")).fixed_pos(rect.min).order(egui::Order::Foreground).show(
            ctx,
            |ui| {
                // nothing under the panel takes a touch
                ui.allocate_rect(rect, Sense::click());
                panel::frame(ui, look, "assets frame", rect, arrow_x, backdrops);
                let body = panel::body(rect);
                // the library in use's name (typed over while it is renamed), or every library's
                let name = libraries.get(current).map_or("Assets", |l| l.name.as_str());
                let renaming =
                    !self.showing_libraries && self.renaming_library.as_ref().is_some_and(|r| r.0 == current);
                let heading = match (self.showing_libraries, renaming) {
                    (true, _) => "Asset libraries",
                    (false, true) => "",
                    (false, false) => name,
                };
                let (tapped, at) = panel::header_title(
                    ui,
                    look,
                    body,
                    heading,
                    20.0,
                    !self.showing_libraries,
                    self.title_menu.is_open(),
                );
                title_row = at;
                if tapped && !self.showing_libraries {
                    self.title_menu.open();
                }
                if renaming {
                    let field = Rect::from_min_size(pos2(body.left() + 20.0, body.top() + 17.0), vec2(300.0, 30.0));
                    self.rename_library_field(ui, session, current, field, 24.0);
                }
                plus = Rect::from_min_size(pos2(body.right() - 12.0 - 41.0, body.top() + 12.0), vec2(41.0, 40.0));
                look.icon(ui, &icons::PLUS_MEDIUM, plus.center(), 41.0, argb(PANEL_GLYPH));
                if ui.interact(plus, ui.scope_id().with("assets plus"), Sense::click()).clicked() {
                    if self.showing_libraries {
                        self.libraries_plus.open();
                    } else {
                        self.plus_menu.open();
                    }
                }

                let lists = Rect::from_min_max(pos2(body.left(), body.top() + HEADER_HEIGHT), body.max);
                if self.showing_libraries {
                    self.libraries_body(ui, look, session, &libraries, lists, shown);
                    return;
                }
                if folders.iter().all(|f| f.assets.is_empty()) {
                    let middle = pos2(lists.center().x, lists.top() + 60.0);
                    text(
                        ui.painter(),
                        middle,
                        Align2::CENTER_TOP,
                        "This library has no assets yet",
                        17.0,
                        Weight::SemiBold,
                        argb(PANEL_TITLE),
                    );
                    text(
                        ui.painter(),
                        middle + vec2(0.0, 30.0),
                        Align2::CENTER_TOP,
                        "Capture or import with + to add some.",
                        14.0,
                        Weight::Regular,
                        argb(PANEL_GLYPH),
                    );
                    return;
                }
                let rail = Rect::from_min_size(lists.min, vec2(RAIL_WIDTH, lists.height()));
                let grid = Rect::from_min_max(pos2(lists.left() + RAIL_WIDTH, lists.top()), lists.max);
                if RAIL_COLUMN.value() >> 24 != 0 {
                    ui.painter().rect_filled(rail, 0, argb(RAIL_COLUMN));
                }
                let corner = panel::POPOVER_CORNER;
                panel::rounded(ui, look, "assets grid", grid, [0.0, 0.0, corner, 0.0], argb(GRID_COLUMN));

                ui.scope_builder(UiBuilder::new().max_rect(rail), |ui| {
                    // the rail follows a press itself, and carries on when flicked; a hold for the
                    // menu keeps it still
                    let held = self.rail_press.as_ref().is_some_and(|p| p.held);
                    let held_at = self.rail_press.as_ref().filter(|p| !p.scrolling).map(|p| p.list);
                    let mut area = ScrollArea::vertical()
                        .id_salt("assets rail")
                        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                        .scroll_source(if held {
                            egui::scroll_area::ScrollSource::NONE
                        } else {
                            egui::scroll_area::ScrollSource::ALL
                        });
                    if let Some(at) = held_at {
                        area = area.vertical_scroll_offset(at);
                    }
                    let out = area.show(ui, |ui| {
                        ui.add_space(12.0);
                        let width = ui.available_width();
                        for (f, folder) in folders.iter().enumerate() {
                            let (response, at) =
                                self.folder_row(ui, look, session, current, f, folder, f == shown, width);
                            rail_rects.push((f, at));
                            if self.rail_press.is_none()
                                && response.is_pointer_button_down_on()
                                && let Some(origin) = response.interact_pointer_pos()
                            {
                                self.rail_press = Some(RailPress {
                                    row: f,
                                    origin,
                                    since: Instant::now(),
                                    list: self.rail_at,
                                    scrolling: false,
                                    held: false,
                                });
                            }
                            // a tap shows it; a scroll or a hold for the menu does not
                            if response.clicked() && self.rail_press.as_ref().is_none_or(|p| !p.scrolling && !p.held) {
                                folder_tapped = Some(f);
                            }
                            if response.secondary_clicked() {
                                row_menu_open = Some(f);
                            }
                            ui.add_space(8.0);
                        }
                    });
                    self.rail_at = out.state.clamped_offset().y;
                });

                ui.scope_builder(UiBuilder::new().max_rect(grid), |ui| {
                    self.asset_grid(ui, session, assets, grid, &mut cell_rects, &mut asset_menu_open);
                });
            },
        );
        self.rail_rects = rail_rects;
        self.cell_rects = cell_rects;
        self.draw_previews(ctx, session);
        self.imports_done(ctx, session);
        self.file_imports_done(ctx, session);
        self.follow_press(ctx);
        self.follow_rail_press(ctx);
        self.follow_card_press(ctx);

        if let Some(f) = folder_tapped {
            self.shown_folder = f;
        }
        if let Some(f) = row_menu_open {
            self.row_menu_for = Some(f);
            self.row_menu.open();
        }
        if let Some(id) = asset_menu_open {
            self.asset_menu_for = Some(id);
            self.asset_menu.open();
        }

        self.plus_menu(ctx, look, session, plus, current as u32, shown as u32, backdrops);
        self.row_menu(ctx, look, session, &libraries, current, backdrops);
        self.title_menu(ctx, look, session, title_row, &libraries, backdrops);
        self.libraries_plus_menu(ctx, look, session, plus, backdrops);
        self.card_menu(ctx, look, session, &libraries, backdrops);
        self.asset_menu(ctx, look, session, assets, backdrops);
        self.move_menu(ctx, look, session, &libraries, current, backdrops);
        self.thumb_menu(ctx, look, session, backdrops);
        if self.deleting.show(ctx, look) == Some(Answer::Action) {
            match self.delete_what.take() {
                Some(Deleting::Library(i)) => {
                    session.delete_asset_library(i);
                }
                Some(Deleting::Folder(lib, f)) => {
                    session.delete_asset_folder(lib, f);
                }
                Some(Deleting::Asset(id)) => {
                    session.delete_asset(id);
                }
                None => {}
            }
        }
        if let Some(answer) = self.psd_alert.show(ctx, look)
            && let Some((bytes, name)) = self.psd_pending.take()
        {
            let mode = match answer {
                Answer::Extra => Some(PsdImportMode::Explode),
                Answer::Action => Some(PsdImportMode::Group),
                _ => None,
            };
            if let Some(mode) = mode {
                self.import(session, ImportJob::Psd(bytes, name, mode), current as u32, shown as u32);
            }
        }
        rect
    }

    /// Every library's card in the panel's lists' place, two to a row.
    fn libraries_body(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &Arc<CanvasSession>,
        libraries: &[AssetLibraryInfo],
        lists: Rect,
        shown: usize,
    ) {
        let mut cards = Vec::new();
        let mut opened = None;
        let mut menu_for = None;
        let held = self.card_press.as_ref().is_some_and(|p| p.held);
        ui.scope_builder(UiBuilder::new().max_rect(lists), |ui| {
            ScrollArea::vertical()
                .id_salt("asset libraries")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                .scroll_source(if held {
                    egui::scroll_area::ScrollSource::NONE
                } else {
                    egui::scroll_area::ScrollSource::ALL
                })
                .show(ui, |ui| {
                    let width = (lists.width() - 24.0 - CARD_GAP) / 2.0;
                    let rows = libraries.len().div_ceil(2);
                    let height = rows as f32 * (CARD_HEIGHT + CARD_GAP) - CARD_GAP + 12.0;
                    let (grid, _) = ui.allocate_exact_size(vec2(lists.width(), height), Sense::hover());
                    for (i, library) in libraries.iter().enumerate() {
                        let (col, row) = ((i % 2) as f32, (i / 2) as f32);
                        let card = Rect::from_min_size(
                            pos2(
                                grid.left() + 12.0 + col * (width + CARD_GAP),
                                grid.top() + row * (CARD_HEIGHT + CARD_GAP),
                            ),
                            vec2(width, CARD_HEIGHT),
                        );
                        cards.push((i, card));
                        let response = ui.interact(card, ui.scope_id().with(("asset library card", i)), Sense::click());
                        self.library_card(ui, look, session, i, library, card, shown);
                        if self.card_press.is_none()
                            && response.is_pointer_button_down_on()
                            && let Some(origin) = response.interact_pointer_pos()
                        {
                            self.card_press = Some(CardPress {
                                card: i,
                                origin,
                                since: Instant::now(),
                                scrolling: false,
                                held: false,
                            });
                        }
                        // a tap opens it; a scroll or a hold for the menu does not
                        if response.secondary_clicked() {
                            menu_for = Some(i);
                        } else if response.clicked() && self.card_press.as_ref().is_none_or(|p| !p.scrolling && !p.held)
                        {
                            opened = Some(i);
                        }
                    }
                });
        });
        self.card_rects = cards;
        if let Some(i) = menu_for {
            self.card_menu_for = Some(i);
            self.card_menu.open();
        } else if let Some(i) = opened {
            self.open_asset_library(session, libraries, i);
        }
    }

    /// Brings library `i` into use and shows its folders.
    fn open_asset_library(&mut self, session: &CanvasSession, libraries: &[AssetLibraryInfo], i: usize) {
        self.showing_libraries = false;
        if libraries.get(i).is_some_and(|l| l.current) {
            return;
        }
        if session.open_asset_library(i as u32) {
            self.shown_folder = 0;
        } else {
            log::error!("asset library {i} couldn't be opened");
        }
    }

    /// One library's card: its name over its folders down the left and a few assets of one of them
    /// beside them; the one in use outlined.
    #[allow(clippy::too_many_arguments)]
    fn library_card(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &CanvasSession,
        i: usize,
        library: &AssetLibraryInfo,
        card: Rect,
        shown: usize,
    ) {
        let painter = ui.painter_at(card);
        painter.rect_filled(card, CARD_CORNER, argb(RAIL_CELL));
        if library.current {
            painter.rect_stroke(card, CARD_CORNER, Stroke::new(2.0, argb(pc::ACCENT)), egui::StrokeKind::Inside);
        }
        let name_at = Rect::from_min_size(card.min + vec2(10.0, 10.0), vec2(card.width() - 20.0, 18.0));
        if !self.rename_library_field(ui, session, i, name_at, 14.0) {
            let name = one_line(ui, &library.name, 14.0, Weight::SemiBold, argb(PANEL_TITLE), name_at.width());
            painter.galley(name_at.min, name, argb(PANEL_TITLE));
        }
        // the one in use's shown folder; another's first with an asset in it
        let chosen = if library.current {
            shown
        } else {
            library.folders.iter().position(|f| !f.assets.is_empty()).unwrap_or(0)
        };
        let folders_width = (card.width() - 20.0) * 0.45;
        let mut y = card.top() + 38.0;
        for (f, folder) in library.folders.iter().enumerate() {
            if y + CARD_FOLDER_ROW > card.bottom() - 8.0 {
                break;
            }
            let row = Rect::from_min_size(pos2(card.left() + 10.0, y), vec2(folders_width, CARD_FOLDER_ROW));
            let selected = f == chosen;
            painter.rect_filled(row, 5, panel::row_fill(selected, CARD_FOLDER_FILL));
            let ink = if selected { Color32::WHITE } else { argb(PANEL_TITLE) };
            look.icon(ui, &icons::FOLDER, pos2(row.left() + 10.0, row.center().y), 14.0, ink);
            let name = one_line(ui, &folder.name, 8.0, Weight::Regular, ink, row.width() - 22.0);
            painter.galley(pos2(row.left() + 20.0, row.center().y - name.size().y / 2.0), name, ink);
            y += CARD_FOLDER_ROW + CARD_FOLDER_GAP;
        }
        let left = card.left() + 10.0 + folders_width + 6.0;
        let width = card.right() - 10.0 - left;
        let mut y = card.top() + 38.0;
        for asset in library.folders.get(chosen).map(|f| f.assets.as_slice()).unwrap_or_default() {
            if y + CARD_ASSET > card.bottom() - 8.0 {
                break;
            }
            let row = Rect::from_min_size(pos2(left, y), vec2(width, CARD_ASSET));
            if !self.previews.contains_key(&asset.id) && !self.wanted.contains(&asset.id) {
                self.wanted.push(asset.id.clone());
            }
            let thumb = Rect::from_min_size(row.min + vec2(0.0, 4.0), vec2(CARD_ASSET - 8.0, CARD_ASSET - 8.0));
            painter.rect_filled(thumb, 6, argb(CELL_FILL));
            if let Some(Some(texture)) = self.previews.get(&asset.id) {
                let fitted = fit_rect(thumb.shrink(3.0), asset.width, asset.height);
                let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                painter.image(texture.id(), fitted, uv, Color32::WHITE);
            }
            let name = one_line(ui, &asset.name, 9.0, Weight::Regular, argb(PANEL_TITLE), width - CARD_ASSET - 2.0);
            painter.galley(pos2(thumb.right() + 5.0, row.center().y - name.size().y / 2.0), name, argb(PANEL_TITLE));
            y += CARD_ASSET + 2.0;
        }
    }

    /// While library `i` is renamed, its name typed in place at `at`; true if it is being.
    fn rename_library_field(&mut self, ui: &mut Ui, session: &CanvasSession, i: usize, at: Rect, size: f32) -> bool {
        let Some((_, typed, focused)) = self.renaming_library.as_mut().filter(|(r, _, _)| *r == i) else {
            return false;
        };
        let mut output = egui::TextEdit::singleline(typed)
            .font(Weight::SemiBold.font(size))
            .text_color(argb(PANEL_TITLE))
            .frame(egui::Frame::NONE)
            .margin(vec2(0.0, 0.0))
            .desired_width(at.width())
            .show(&mut ui.new_child(UiBuilder::new().max_rect(at)));
        if !*focused {
            pick_all(&mut output, ui.ctx(), typed.chars().count());
            *focused = true;
        } else if output.response.lost_focus() {
            let name = typed.clone();
            self.renaming_library = None;
            session.rename_asset_library(i as u32, name);
        }
        true
    }

    /// The title's menu, hanging under it: Rename or Share the library in use, or go back to every
    /// library.
    fn title_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        title: Rect,
        libraries: &[AssetLibraryInfo],
        backdrops: &mut Vec<f32>,
    ) {
        let ink = menu::INK;
        let entries = vec![
            Entry::Item(&icons::MENU_RENAME, "Rename", ink),
            Entry::Item(&icons::MENU_SHARE, "Share...", ink),
            Entry::Rule(11.0),
            Entry::Item(&icons::MENU_LIBRARIES, "Back to libraries", ink),
        ];
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let rect = Rect::from_min_size(pos2(title.left(), title.bottom()), size);
        let mut tapped = None;
        self.title_menu.show(ctx, "assets title menu", rect, rect.left_top(), menu::OVER_A_PANEL, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        let Some(Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.title_menu.close();
        match *label {
            "Rename" => {
                let place = libraries.iter().position(|l| l.current).unwrap_or(0);
                let name = libraries.get(place).map(|l| l.name.clone()).unwrap_or_default();
                self.renaming_library = Some((place, name, false));
            }
            "Share..." => {
                let place = libraries.iter().position(|l| l.current).unwrap_or(0);
                let name = libraries.get(place).map(|l| l.name.clone()).unwrap_or_default();
                self.share_library(session, place as u32, &name);
            }
            _ => self.showing_libraries = true,
        }
    }

    /// The plus's menu with every library shown: a new library.
    fn libraries_plus_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        plus: Rect,
        backdrops: &mut Vec<f32>,
    ) {
        let entries = [Entry::Item(&icons::MENU_NEW_LIBRARY, "Create new library", menu::INK)];
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let screen = ctx.content_rect();
        let top = plus.bottom().min(screen.bottom() - size.y - 8.0).max(screen.top() + MENU_TOP);
        let rect = Rect::from_min_size(pos2(plus.right() - size.x, top), size);
        let mut tapped = None;
        self.libraries_plus.show(
            ctx,
            "asset libraries plus menu",
            rect,
            rect.right_top(),
            menu::OVER_A_PANEL,
            backdrops,
            |ui| {
                tapped = menu::entries(ui, look, rect, &entries);
            },
        );
        let Some(tapped) = tapped else { return };
        self.libraries_plus.close();
        if tapped == 0
            && let Some(i) = session.new_asset_library()
        {
            let name = session.asset_libraries().get(i as usize).map(|l| l.name.clone()).unwrap_or_default();
            self.renaming_library = Some((i as usize, name, false));
        }
    }

    /// A card's menu, over its upper part: Rename, Share, and Delete while there are others.
    fn card_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        libraries: &[AssetLibraryInfo],
        backdrops: &mut Vec<f32>,
    ) {
        let Some(i) = self.card_menu_for else { return };
        let Some(card) = self.card_rects.iter().find(|(c, _)| *c == i).map(|(_, r)| *r) else { return };
        let ink = menu::INK;
        let mut entries =
            vec![Entry::Item(&icons::MENU_RENAME, "Rename", ink), Entry::Item(&icons::MENU_SHARE, "Share...", ink)];
        if libraries.len() > 1 {
            entries.push(menu::DIVIDER);
            entries.push(Entry::Item(&icons::MENU_TRASH, "Delete", menu::DANGER));
        }
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let rect = Rect::from_min_size(pos2(card.center().x - size.x / 2.0, card.top() + card.height() / 4.0), size);
        let mut tapped = None;
        self.card_menu.show(ctx, "asset card menu", rect, rect.center_top(), menu::OVER_A_PANEL, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        if !self.card_menu.is_open() {
            self.card_menu_for = None;
        }
        let Some(Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.card_menu.close();
        match *label {
            "Rename" => {
                let name = libraries.get(i).map(|l| l.name.clone()).unwrap_or_default();
                self.renaming_library = Some((i, name, false));
            }
            "Share..." => {
                let name = libraries.get(i).map(|l| l.name.clone()).unwrap_or_default();
                self.share_library(session, i as u32, &name);
            }
            _ => self.ask_delete(Deleting::Library(i as u32)),
        }
    }

    /// A folder of the library in use on the rail: its mark and its name, or a field renaming it.
    #[allow(clippy::too_many_arguments)]
    fn folder_row(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        session: &CanvasSession,
        library: usize,
        f: usize,
        folder: &AssetFolderInfo,
        selected: bool,
        width: f32,
    ) -> (egui::Response, Rect) {
        let (outer, response) = ui.allocate_exact_size(vec2(width, ROW_HEIGHT), Sense::click());
        let rect = outer.shrink2(vec2(12.0, 0.0));
        ui.painter().rect_filled(rect, 12, panel::row_fill(selected, RAIL_CELL));
        let ink = if selected { Color32::WHITE } else { argb(PANEL_TITLE) };
        look.icon(ui, &icons::FOLDER, pos2(rect.left() + 24.0, rect.center().y), 28.0, ink);
        let left = rect.left() + 4.0 + 40.0 + 1.0;
        if let Some((_, typed, focused)) = self.renaming_folder.as_mut().filter(|(r, _, _)| *r == f) {
            let field = Rect::from_min_max(
                pos2(left, rect.center().y - 10.0),
                pos2(rect.right() - 8.0, rect.center().y + 10.0),
            );
            let mut output = egui::TextEdit::singleline(typed)
                .font(Weight::SemiBold.font(14.0))
                .text_color(ink)
                .frame(egui::Frame::NONE)
                .margin(vec2(0.0, 0.0))
                .desired_width(field.width())
                .show(&mut ui.new_child(UiBuilder::new().max_rect(field)));
            if !*focused {
                pick_all(&mut output, ui.ctx(), typed.chars().count());
                *focused = true;
            } else if output.response.lost_focus() {
                let name = typed.clone();
                self.renaming_folder = None;
                session.rename_asset_folder(library as u32, f as u32, name);
            }
            return (response, rect);
        }
        let name = two_lines(ui, &folder.name, 14.0, Weight::SemiBold, ink, rect.right() - 8.0 - left);
        let at = Align2::LEFT_CENTER.anchor_size(pos2(left, rect.center().y), name.size());
        ui.painter().galley(at.min, name, ink);
        (response, rect)
    }

    /// The shown folder's assets, two to a row: each a thumbnail over its name.
    fn asset_grid(
        &mut self,
        ui: &mut Ui,
        session: &CanvasSession,
        assets: &[AssetInfo],
        grid: Rect,
        cell_rects: &mut Vec<(String, Rect)>,
        asset_menu_open: &mut Option<String>,
    ) {
        let cell_w = (grid.width() - CELL_PAD * 2.0 - CELL_GAP) / COLUMNS as f32;
        let thumb_h = cell_w * 0.8;
        let cell_h = thumb_h + NAME_AREA;
        let rows = assets.len().div_ceil(COLUMNS);
        let total = rows as f32 * (cell_h + CELL_GAP) + CELL_PAD;
        if assets.is_empty() {
            text(
                ui.painter(),
                pos2(grid.center().x, grid.top() + 28.0),
                Align2::CENTER_TOP,
                "Nothing here yet",
                15.0,
                Weight::Medium,
                argb(PANEL_TITLE),
            );
            return;
        }
        // the grid follows a press itself, and carries on when flicked; a hold for the menu keeps
        // it still
        let held = self.press.as_ref().is_some_and(|p| p.held);
        let held_at = self.press.as_ref().filter(|p| !p.scrolling).map(|p| p.list);
        let mut area = ScrollArea::vertical()
            .id_salt(("assets grid", self.shown_folder))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
            .scroll_source(if held {
                egui::scroll_area::ScrollSource::NONE
            } else {
                egui::scroll_area::ScrollSource::ALL
            });
        if let Some(at) = held_at {
            area = area.vertical_scroll_offset(at);
        }
        let out = area.show(ui, |ui| {
            let (all, _) = ui.allocate_exact_size(vec2(grid.width(), total), Sense::hover());
            for (i, asset) in assets.iter().enumerate() {
                let (row, col) = (i / COLUMNS, i % COLUMNS);
                let cell = Rect::from_min_size(
                    all.min
                        + vec2(
                            CELL_PAD + col as f32 * (cell_w + CELL_GAP),
                            CELL_PAD + row as f32 * (cell_h + CELL_GAP),
                        ),
                    vec2(cell_w, cell_h),
                );
                cell_rects.push((asset.id.clone(), cell));
                // one being renamed is drawn even out of sight, so its field can scroll in
                let renaming = self.renaming_asset.as_ref().is_some_and(|(id, _, _)| *id == asset.id);
                if !ui.is_rect_visible(cell) && !renaming {
                    continue;
                }
                let frame = Rect::from_min_size(cell.min, vec2(cell_w, thumb_h));
                let painter = ui.painter_at(cell);
                painter.rect_filled(frame, 10, argb(CELL_FILL));
                if !self.previews.contains_key(&asset.id) {
                    self.wanted.push(asset.id.clone());
                }
                if let Some(Some(texture)) = self.previews.get(&asset.id) {
                    let fitted = fit_rect(frame.shrink(6.0), asset.width, asset.height);
                    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    painter.image(texture.id(), fitted, uv, Color32::WHITE);
                }
                let ink = argb(PANEL_TITLE);
                if let Some((_, typed, focused)) = self.renaming_asset.as_mut().filter(|(id, _, _)| *id == asset.id) {
                    let field =
                        Rect::from_min_size(pos2(cell.left() + 4.0, frame.bottom() + 3.0), vec2(cell_w - 8.0, 18.0));
                    let mut output = egui::TextEdit::singleline(typed)
                        .font(Weight::SemiBold.font(13.0))
                        .text_color(ink)
                        .frame(egui::Frame::NONE)
                        .margin(vec2(0.0, 0.0))
                        .desired_width(field.width())
                        .show(&mut ui.new_child(UiBuilder::new().max_rect(field)));
                    if !*focused {
                        ui.scroll_to_rect(cell, Some(egui::Align::Center));
                        pick_all(&mut output, ui.ctx(), typed.chars().count());
                        *focused = true;
                    } else if output.response.lost_focus() {
                        let name = typed.clone();
                        self.renaming_asset = None;
                        session.rename_asset(asset.id.clone(), name);
                    }
                } else {
                    let name = one_line(ui, &asset.name, 13.0, Weight::SemiBold, ink, cell_w - 8.0);
                    let at = pos2(cell.center().x - name.size().x / 2.0, frame.bottom() + 5.0);
                    painter.galley(at, name, ink);
                }
                let response = ui.interact(cell, ui.scope_id().with(("asset", &asset.id)), Sense::click());
                if response.secondary_clicked() {
                    *asset_menu_open = Some(asset.id.clone());
                }
                if response.is_pointer_button_down_on()
                    && self.press.is_none()
                    && let Some(origin) = ui.input(|inp| inp.pointer.interact_pos())
                {
                    self.press = Some(Press {
                        id: asset.id.clone(),
                        origin,
                        since: Instant::now(),
                        list: self.grid_at,
                        scrolling: false,
                        held: false,
                    });
                }
                // a tap places it; a scroll or a hold for the menu does not
                if response.clicked() && self.press.as_ref().is_none_or(|p| !p.scrolling && !p.held) {
                    self.place(session, &asset.id);
                }
            }
        });
        self.grid_at = out.state.clamped_offset().y;
    }

    /// Takes the thumbnails drawn so far, and has those wanted drawn on a thread of their own.
    fn draw_previews(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>) {
        if let Some(drawing) = &self.drawing {
            loop {
                match drawing.try_recv() {
                    Ok((id, image)) => {
                        let texture =
                            image.map(|img| ctx.load_texture(format!("asset {id}"), img, TextureOptions::LINEAR));
                        self.previews.insert(id, texture);
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => break,
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        self.drawing = None;
                        break;
                    }
                }
            }
        }
        let previews = &self.previews;
        self.wanted.retain(|id| !previews.contains_key(id));
        if self.drawing.is_none() && !self.wanted.is_empty() {
            let (done, drawing) = std::sync::mpsc::channel();
            let batch: Vec<String> = self.wanted.drain(..self.wanted.len().min(PREVIEWS_AT_A_TIME)).collect();
            let session = Arc::clone(session);
            let started = std::thread::Builder::new().name("asset previews".into()).spawn(move || {
                for id in batch {
                    // the preview comes back with straight alpha
                    let image = session.asset_preview(id.clone(), PREVIEW_SIDE).map(|t| {
                        let size = [t.width as usize, t.height as usize];
                        egui::ColorImage::from_rgba_unmultiplied(size, &t.rgba)
                    });
                    if done.send((id, image)).is_err() {
                        return;
                    }
                }
            });
            match started {
                Ok(_) => self.drawing = Some(drawing),
                Err(e) => log::warn!("the assets' thumbnails aren't drawn: {e}"),
            }
        }
        if self.drawing.is_some() {
            ctx.request_repaint_after(PREVIEW_LOOK);
        }
    }

    /// Follows the press on an asset: moved past the slop, the grid takes it and scrolls; held
    /// still, its menu opens. A plain tap is placed where its cell is drawn.
    fn follow_press(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.press.take() else { return };
        let (at, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        let at = at.unwrap_or(p.origin);
        if down {
            if !p.scrolling && !p.held {
                if at.distance(p.origin) > SLOP {
                    p.scrolling = true;
                } else if p.since.elapsed() >= HOLD {
                    p.held = true;
                    self.asset_menu_for = Some(p.id.clone());
                    self.asset_menu.open();
                } else {
                    ctx.request_repaint_after(HOLD.saturating_sub(p.since.elapsed()));
                }
            }
            self.press = Some(p);
        }
    }

    /// Follows the press on a folder row: moved past the slop, the rail takes it and scrolls; held
    /// still, its menu opens. A plain tap shows that folder.
    fn follow_rail_press(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.rail_press.take() else { return };
        let (at, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        let at = at.unwrap_or(p.origin);
        if down {
            if !p.scrolling && !p.held {
                if at.distance(p.origin) > SLOP {
                    p.scrolling = true;
                } else if p.since.elapsed() >= HOLD {
                    p.held = true;
                    self.row_menu_for = Some(p.row);
                    self.row_menu.open();
                } else {
                    ctx.request_repaint_after(HOLD.saturating_sub(p.since.elapsed()));
                }
            }
            self.rail_press = Some(p);
        }
    }

    /// Follows the press on a library's card: moved past the slop, the list takes it and scrolls;
    /// held still, its menu opens. A plain tap opens that library.
    fn follow_card_press(&mut self, ctx: &egui::Context) {
        let Some(mut p) = self.card_press.take() else { return };
        let (at, down) = ctx.input(|i| (i.pointer.interact_pos(), i.pointer.primary_down()));
        let at = at.unwrap_or(p.origin);
        if down {
            if !p.scrolling && !p.held {
                if at.distance(p.origin) > SLOP {
                    p.scrolling = true;
                } else if p.since.elapsed() >= HOLD {
                    p.held = true;
                    self.card_menu_for = Some(p.card);
                    self.card_menu.open();
                } else {
                    ctx.request_repaint_after(HOLD.saturating_sub(p.since.elapsed()));
                }
            }
            self.card_press = Some(p);
        }
    }

    /// Puts the asset on the canvas; the app transforms it once it lands.
    fn place(&mut self, session: &CanvasSession, id: &str) {
        if session.place_asset(id.to_owned()).is_some() {
            self.placed = true;
        }
    }

    /// The plus's menu: captures of what is on the canvas, the camera and imports, and new
    /// libraries and folders.
    #[allow(clippy::too_many_arguments)]
    fn plus_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &Arc<CanvasSession>,
        plus: Rect,
        library: u32,
        folder: u32,
        backdrops: &mut Vec<f32>,
    ) {
        let ink = menu::INK;
        let entries = vec![
            Entry::Item(&icons::MENU_CAMERA, "Capture current layer", ink),
            Entry::Item(&icons::MENU_GROUP, "Capture group", ink),
            Entry::Item(&icons::MENU_SELECT_ALL, "Capture picked layers", ink),
            Entry::Item(&icons::MENU_PHOTO, "Capture canvas", ink),
            Entry::Item(&icons::MENU_CAMERA, "Capture selection", ink),
            menu::DIVIDER,
            Entry::Item(&icons::MENU_CAMERA, "Camera", ink),
            Entry::Item(&icons::MENU_PHOTO, "Import file...", ink),
            Entry::Item(&icons::MENU_LIBRARIES, "Import from project...", ink),
            menu::DIVIDER,
            Entry::Item(&icons::MENU_NEW_LIBRARY, "New library", ink),
            Entry::Item(&icons::MENU_NEW_SET, "New folder", ink),
        ];
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let screen = ctx.content_rect();
        let top = plus.bottom().min(screen.bottom() - size.y - 8.0).max(screen.top() + MENU_TOP);
        let rect = Rect::from_min_size(pos2(plus.right() - size.x, top), size);
        let mut tapped = None;
        self.plus_menu.show(ctx, "assets plus menu", rect, rect.right_top(), menu::OVER_A_PANEL, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        let Some(Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.plus_menu.close();
        match *label {
            "Capture current layer" => self.capture(session, AssetScope::CurrentLayer, library, folder),
            "Capture group" => self.capture(session, AssetScope::Group, library, folder),
            "Capture picked layers" => self.capture(session, AssetScope::Picked, library, folder),
            "Capture canvas" => self.capture(session, AssetScope::Canvas, library, folder),
            "Capture selection" => self.capture(session, AssetScope::Selection, library, folder),
            "Camera" => self.import_image(session, library, folder, "Camera"),
            "Import file..." => self.import_file(session, library, folder),
            "Import from project..." => self.import_project(session, library, folder),
            "New library" => {
                if let Some(i) = session.new_asset_library() {
                    session.open_asset_library(i);
                    self.shown_folder = 0;
                    let name = session.asset_libraries().get(i as usize).map(|l| l.name.clone()).unwrap_or_default();
                    self.renaming_library = Some((i as usize, name, false));
                }
            }
            "New folder" => {
                if let Some(f) = session.new_asset_folder(library) {
                    self.shown_folder = f as usize;
                    let name = session
                        .asset_libraries()
                        .get(library as usize)
                        .and_then(|l| l.folders.get(f as usize))
                        .map(|fo| fo.name.clone())
                        .unwrap_or_default();
                    self.renaming_folder = Some((f as usize, name, false));
                }
            }
            _ => {}
        }
    }

    /// Takes `scope` of the open artwork into the shown folder, its new asset named right away.
    fn capture(&mut self, session: &CanvasSession, scope: AssetScope, library: u32, folder: u32) {
        match session.capture_asset(String::new(), scope, library, folder) {
            Some(id) => self.name_new(session, id, library, folder),
            None => self.notice = Some(("Nothing to capture.".into(), false)),
        }
    }

    /// One file picked and brought in as whatever it is: a shared asset file, a PSD or a plain
    /// picture.
    fn import_file(&mut self, session: &Arc<CanvasSession>, library: u32, folder: u32) {
        let Some(path) = crate::files::dialog()
            .set_title("Import file")
            .add_filter(
                "Pictures, Photoshop & assets",
                &["png", "jpg", "jpeg", "psd", "psb", "asset", "assetset", "assetlibrary", "zip"],
            )
            .pick_file()
        else {
            return;
        };
        let ext = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        if matches!(ext.as_str(), "asset" | "assetset" | "assetlibrary")
            || procraft_core::zip_file_kind(path.to_string_lossy().into_owned()) == procraft_core::ZipKind::Assets
        {
            self.import_asset_file(session, &path, library, folder);
        } else if matches!(ext.as_str(), "psd" | "psb") {
            self.import_psd(&path);
        } else {
            self.import_image_file(session, &path, library, folder);
        }
    }

    /// A picture picked and brought in as a single-image asset. The camera picks one too: there
    /// is no webcam on desktop.
    fn import_image(&mut self, session: &Arc<CanvasSession>, library: u32, folder: u32, title: &str) {
        let Some(path) =
            crate::files::dialog().set_title(title).add_filter("Pictures", &["png", "jpg", "jpeg"]).pick_file()
        else {
            return;
        };
        self.import_image_file(session, &path, library, folder);
    }

    /// The picture at `path` read and brought in as a single-image asset.
    fn import_image_file(&mut self, session: &Arc<CanvasSession>, path: &Path, library: u32, folder: u32) {
        let Ok(bytes) = std::fs::read(path) else {
            self.notice = Some(("This file could not be read.".into(), true));
            return;
        };
        let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.import(session, ImportJob::Image(bytes, name), library, folder);
    }

    /// A PSD read, then asked how its layers come in before it is brought in.
    fn import_psd(&mut self, path: &Path) {
        let Ok(bytes) = std::fs::read(path) else {
            self.notice = Some(("This file could not be read.".into(), true));
            return;
        };
        let name = path.file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.psd_pending = Some((bytes, name));
        self.psd_alert.ask(Asking {
            title: "Import PSD".into(),
            message: "Bring the layers in as one group, or each as its own asset?".into(),
            action: "Group".into(),
            icon: Some(&icons::MENU_FOLDER),
            destructive: false,
            extra: Some("Explode".into()),
            first: None,
            alone: false,
        });
    }

    /// Another artwork's folder picked, its layers brought in as one asset.
    fn import_project(&mut self, session: &Arc<CanvasSession>, library: u32, folder: u32) {
        let Some(dir) = crate::files::dialog().set_title("Import from project").pick_folder() else { return };
        let name = dir.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        self.import(session, ImportJob::Project(dir.to_string_lossy().into_owned(), name), library, folder);
    }

    /// A shared asset file (.asset, .assetset, .assetlibrary, or a zip holding one) read and
    /// brought in.
    fn import_asset_file(&mut self, session: &Arc<CanvasSession>, path: &Path, library: u32, folder: u32) {
        let Ok(bytes) = std::fs::read(path) else {
            self.notice = Some(("This file could not be read.".into(), true));
            return;
        };
        self.import_asset_bytes(session, bytes, library, folder);
    }

    /// Shared asset files (dropped on the canvas, or kept from before one opened) brought in where
    /// the panel shows.
    pub fn import_files(&mut self, session: &Arc<CanvasSession>, files: Vec<Vec<u8>>) {
        if files.is_empty() {
            return;
        }
        let libraries = session.asset_libraries();
        let current = libraries.iter().position(|l| l.current).unwrap_or(0) as u32;
        let folder = self.shown_folder as u32;
        for bytes in files {
            self.import_asset_bytes(session, bytes, current, folder);
        }
    }

    /// A shared asset file brought in on a thread of its own (a whole library takes a while); where
    /// it landed is shown once it is kept.
    fn import_asset_bytes(&mut self, session: &Arc<CanvasSession>, bytes: Vec<u8>, library: u32, folder: u32) {
        let before = session.asset_libraries();
        let libraries = before.len();
        let folders = before.get(library as usize).map_or(0, |l| l.folders.len());
        let (done, importing) = std::sync::mpsc::channel();
        let session = Arc::clone(session);
        std::thread::spawn(move || {
            let kept = session.import_asset_file(bytes, library, folder).is_some();
            let _ = done.send(kept);
        });
        self.file_importing.push(FileWait { done: importing, library, folder, libraries, folders });
    }

    /// Shared asset files finished since the last frame looked at: a failure told of, and where
    /// they landed shown - a library file's new library opened, a set's new folder, or the folder
    /// a single asset went to.
    fn file_imports_done(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let mut failed = false;
        let mut kept = Vec::new();
        self.file_importing.retain(|wait| match wait.done.try_recv() {
            Ok(good) => {
                if good {
                    kept.push((wait.library, wait.folder, wait.libraries, wait.folders));
                } else {
                    failed = true;
                }
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        if failed {
            self.notice = Some(("The file could not be brought in.".into(), true));
        }
        for (library, folder, libraries, folders) in kept {
            self.showing_libraries = false;
            let after = session.asset_libraries();
            if after.len() > libraries {
                // a library file makes its own: the first new one opened
                session.open_asset_library(libraries as u32);
                self.shown_folder = 0;
            } else if let Some(l) = after.get(library as usize)
                && l.folders.len() > folders
            {
                self.shown_folder = l.folders.len() - 1;
            } else {
                self.shown_folder = folder as usize;
            }
        }
        if !self.file_importing.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    /// A file brought in on a thread of its own (a big one takes a while); the shelf reads itself
    /// again each frame, so what comes in shows once it is kept.
    fn import(&mut self, session: &Arc<CanvasSession>, job: ImportJob, library: u32, folder: u32) {
        let before = session
            .asset_libraries()
            .get(library as usize)
            .and_then(|l| l.folders.get(folder as usize))
            .map(|f| f.assets.iter().map(|a| a.id.clone()).collect())
            .unwrap_or_default();
        let (done, importing) = std::sync::mpsc::channel();
        let session = Arc::clone(session);
        std::thread::spawn(move || {
            let kept = match job {
                ImportJob::Image(bytes, name) => session.import_asset_image(bytes, name, library, folder).is_some(),
                ImportJob::Psd(bytes, name, mode) => {
                    session.import_asset_psd(bytes, name, mode, library, folder).is_some()
                }
                ImportJob::Project(dir, name) => {
                    session.import_asset_from_project(dir, name, library, folder).is_some()
                }
            };
            let _ = done.send(kept);
        });
        self.importing.push(ImportWait { done: importing, library, folder, before });
    }

    /// Imports finished since the last frame looked at; a failure is told of, and each new asset
    /// named right away.
    fn imports_done(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let mut failed = false;
        let mut kept = Vec::new();
        self.importing.retain(|wait| match wait.done.try_recv() {
            Ok(good) => {
                if good {
                    kept.push((wait.library, wait.folder, wait.before.clone()));
                } else {
                    failed = true;
                }
                false
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => true,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => false,
        });
        if failed {
            self.notice = Some(("The file could not be brought in.".into(), true));
        }
        for (library, folder, before) in kept {
            // the first asset the folder didn't have before is the new one (an exploded PSD's
            // first layer, when several came in)
            let libraries = session.asset_libraries();
            let new = libraries
                .get(library as usize)
                .and_then(|l| l.folders.get(folder as usize))
                .and_then(|f| f.assets.iter().find(|a| !before.contains(&a.id)));
            if let Some(asset) = new {
                self.name_new(session, asset.id.clone(), library, folder);
            }
        }
        if !self.importing.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(50));
        }
    }

    /// A new asset named right away: its folder shown, its whole name chosen to type over.
    fn name_new(&mut self, session: &CanvasSession, id: String, library: u32, folder: u32) {
        self.showing_libraries = false;
        let libraries = session.asset_libraries();
        if libraries.iter().position(|l| l.current) != Some(library as usize) {
            session.open_asset_library(library);
        }
        self.shown_folder = folder as usize;
        let name = libraries
            .get(library as usize)
            .and_then(|l| l.folders.get(folder as usize))
            .and_then(|f| f.assets.iter().find(|a| a.id == id))
            .map(|a| a.name.clone())
            .unwrap_or_default();
        self.renaming_asset = Some((id, name, false));
    }

    /// A folder row's menu: Rename, Share, and Delete when there is another to fall back to.
    fn row_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        libraries: &[AssetLibraryInfo],
        current: usize,
        backdrops: &mut Vec<f32>,
    ) {
        let Some(f) = self.row_menu_for else { return };
        let Some(row_rect) = self.rail_rects.iter().find(|(r, _)| *r == f).map(|(_, r)| *r) else { return };
        let ink = menu::INK;
        let mut entries =
            vec![Entry::Item(&icons::MENU_RENAME, "Rename", ink), Entry::Item(&icons::MENU_SHARE, "Share...", ink)];
        if libraries.get(current).map_or(0, |l| l.folders.len()) > 1 {
            entries.push(menu::DIVIDER);
            entries.push(Entry::Item(&icons::MENU_TRASH, "Delete", menu::DANGER));
        }
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let screen = ctx.content_rect();
        let left = row_rect.left().min(screen.right() - size.x - 8.0).max(screen.left() + 8.0);
        let top = (row_rect.bottom() + 4.0).min(screen.bottom() - size.y - 8.0).max(screen.top() + MENU_TOP);
        let rect = Rect::from_min_size(pos2(left, top), size);
        let mut tapped = None;
        self.row_menu.show(ctx, "assets row menu", rect, rect.left_top(), MENU_GLASS, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        if !self.row_menu.is_open() {
            self.row_menu_for = None;
        }
        let Some(Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.row_menu.close();
        match *label {
            "Rename" => {
                let name =
                    libraries.get(current).and_then(|l| l.folders.get(f)).map(|fo| fo.name.clone()).unwrap_or_default();
                self.renaming_folder = Some((f, name, false));
            }
            "Share..." => {
                let name =
                    libraries.get(current).and_then(|l| l.folders.get(f)).map(|fo| fo.name.clone()).unwrap_or_default();
                self.share_folder(session, current as u32, f as u32, &name);
            }
            _ => self.ask_delete(Deleting::Folder(current as u32, f as u32)),
        }
    }

    /// An asset's menu: Rename, Duplicate, Move to folder, Set thumbnail, Share, Delete.
    fn asset_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        assets: &[AssetInfo],
        backdrops: &mut Vec<f32>,
    ) {
        let Some(id) = self.asset_menu_for.clone() else { return };
        let Some(cell) = self.cell_rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r) else { return };
        let ink = menu::INK;
        let entries = vec![
            Entry::Item(&icons::MENU_RENAME, "Rename", ink),
            Entry::Item(&icons::MENU_DUPLICATE, "Duplicate", ink),
            Entry::Item(&icons::MENU_FOLDER, "Move to folder", ink),
            Entry::Item(&icons::MENU_PHOTO, "Set thumbnail", ink),
            Entry::Item(&icons::MENU_SHARE, "Share...", ink),
            menu::DIVIDER,
            Entry::Item(&icons::MENU_TRASH, "Delete", menu::DANGER),
        ];
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let screen = ctx.content_rect();
        let left = cell.left().min(screen.right() - size.x - 8.0).max(screen.left() + 8.0);
        let top = (cell.center().y - size.y / 2.0)
            .clamp(screen.top() + MENU_TOP, (screen.bottom() - size.y - 8.0).max(screen.top() + MENU_TOP));
        let rect = Rect::from_min_size(pos2(left, top), size);
        let mut tapped = None;
        self.asset_menu.show(ctx, "assets asset menu", rect, rect.left_center(), MENU_GLASS, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        if !self.asset_menu.is_open() {
            self.asset_menu_for = None;
        }
        let Some(Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.asset_menu.close();
        match *label {
            "Rename" => {
                let name = assets.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_default();
                self.renaming_asset = Some((id, name, false));
            }
            "Duplicate" => {
                session.duplicate_asset(id);
            }
            "Move to folder" => {
                self.asset_menu_for = None;
                self.move_for = Some(id);
                self.move_menu.open();
            }
            "Set thumbnail" => {
                self.asset_menu_for = None;
                self.thumb_for = Some(id);
                self.thumb_menu.open();
            }
            "Share..." => {
                let name = assets.iter().find(|a| a.id == id).map(|a| a.name.clone()).unwrap_or_default();
                self.share_asset(session, id, &name);
            }
            _ => self.ask_delete(Deleting::Asset(id)),
        }
    }

    /// The asset written out as a .asset where the user picks.
    fn share_asset(&mut self, session: &CanvasSession, id: String, name: &str) {
        let Some(path) = share_path("Share asset", "Asset", "asset", name) else { return };
        if !session.export_asset(id, path.to_string_lossy().into_owned()) {
            self.notice = Some(("The asset could not be shared.".into(), true));
        }
    }

    /// The folder written out as a .assetset where the user picks.
    fn share_folder(&mut self, session: &CanvasSession, library: u32, folder: u32, name: &str) {
        let Some(path) = share_path("Share folder", "Asset set", "assetset", name) else { return };
        if !session.export_asset_folder(library, folder, path.to_string_lossy().into_owned()) {
            self.notice = Some(("The folder could not be shared.".into(), true));
        }
    }

    /// The library written out as a .assetlibrary where the user picks.
    fn share_library(&mut self, session: &CanvasSession, library: u32, name: &str) {
        let Some(path) = share_path("Share library", "Asset library", "assetlibrary", name) else { return };
        if !session.export_asset_library(library, path.to_string_lossy().into_owned()) {
            self.notice = Some(("The library could not be shared.".into(), true));
        }
    }

    /// A picture picked to stand in for the asset's own thumbnail; the cached preview is let go of
    /// so it is drawn again from the new one.
    fn set_thumbnail(&mut self, session: &CanvasSession, id: String) {
        let Some(path) = crate::files::dialog()
            .set_title("Set thumbnail")
            .add_filter("Pictures", &["png", "jpg", "jpeg"])
            .pick_file()
        else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            self.notice = Some(("This file could not be read.".into(), true));
            return;
        };
        if session.set_asset_thumbnail(id.clone(), bytes) {
            self.previews.remove(&id);
        } else {
            self.notice = Some(("The thumbnail could not be set.".into(), true));
        }
    }

    /// The ways an asset's thumbnail can be set, beside its cell as the move menu is.
    fn thumb_menu(&mut self, ctx: &egui::Context, look: &mut Look, session: &CanvasSession, backdrops: &mut Vec<f32>) {
        let Some(id) = self.thumb_for.clone() else { return };
        let Some(cell) = self.cell_rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r) else { return };
        let ink = menu::INK;
        let entries = vec![
            Entry::Item(&icons::MENU_PHOTO, "From canvas", ink),
            Entry::Item(&icons::MENU_CAMERA, "From current layer", ink),
            Entry::Item(&icons::PASTE_DOWN, "From clipboard", ink),
            Entry::Item(&icons::MENU_FOLDER, "From file...", ink),
        ];
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let screen = ctx.content_rect();
        let left = cell.left().min(screen.right() - size.x - 8.0).max(screen.left() + 8.0);
        let top = (cell.center().y - size.y / 2.0)
            .clamp(screen.top() + MENU_TOP, (screen.bottom() - size.y - 8.0).max(screen.top() + MENU_TOP));
        let rect = Rect::from_min_size(pos2(left, top), size);
        let mut tapped = None;
        self.thumb_menu.show(ctx, "assets thumbnail menu", rect, rect.left_center(), MENU_GLASS, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        if !self.thumb_menu.is_open() {
            self.thumb_for = None;
        }
        let Some(Entry::Item(_, label, _)) = tapped.and_then(|t| entries.get(t)) else { return };
        self.thumb_menu.close();
        self.thumb_for = None;
        match *label {
            "From canvas" => self.thumbnail_kept(id.clone(), session.set_asset_thumbnail_from_canvas(id)),
            "From current layer" => self.thumbnail_kept(id.clone(), session.set_asset_thumbnail_from_layer(id)),
            "From clipboard" => self.thumbnail_from_clipboard(session, id),
            _ => self.set_thumbnail(session, id),
        }
    }

    /// A thumbnail set (or not): the cached preview let go of so it is drawn again from the new one.
    fn thumbnail_kept(&mut self, id: String, kept: bool) {
        if kept {
            self.previews.remove(&id);
        } else {
            self.notice = Some(("The thumbnail could not be set.".into(), true));
        }
    }

    /// The picture on the system's clipboard as the asset's thumbnail; none there is told of.
    fn thumbnail_from_clipboard(&mut self, session: &CanvasSession, id: String) {
        let image = arboard::Clipboard::new().ok().and_then(|mut c| c.get_image().ok());
        let Some(image) = image else {
            self.notice = Some(("There is no picture on the clipboard.".into(), true));
            return;
        };
        let Some(png) = image::RgbaImage::from_raw(image.width as u32, image.height as u32, image.bytes.into_owned())
        else {
            self.notice = Some(("The thumbnail could not be set.".into(), true));
            return;
        };
        let mut bytes = Vec::new();
        if png.write_to(&mut std::io::Cursor::new(&mut bytes), image::ImageFormat::Png).is_err() {
            self.notice = Some(("The thumbnail could not be set.".into(), true));
            return;
        }
        self.thumbnail_kept(id.clone(), session.set_asset_thumbnail(id, bytes));
    }

    /// The folders an asset can be moved to, across the libraries; the one it is in left out.
    fn move_menu(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &CanvasSession,
        libraries: &[AssetLibraryInfo],
        current: usize,
        backdrops: &mut Vec<f32>,
    ) {
        let Some(id) = self.move_for.clone() else { return };
        let Some(cell) = self.cell_rects.iter().find(|(i, _)| *i == id).map(|(_, r)| *r) else { return };
        let many = libraries.len() > 1;
        let mut labels = Vec::new();
        let mut targets = Vec::new();
        for (li, lib) in libraries.iter().enumerate() {
            for (fi, folder) in lib.folders.iter().enumerate() {
                if li == current && fi == self.shown_folder {
                    continue;
                }
                labels.push(if many { format!("{} / {}", lib.name, folder.name) } else { folder.name.clone() });
                targets.push((li, fi));
            }
        }
        if labels.is_empty() {
            self.move_menu.close();
            self.move_for = None;
            return;
        }
        let entries: Vec<Entry> = labels.iter().map(|l| Entry::Item(&icons::FOLDER, l.as_str(), menu::INK)).collect();
        let size = vec2(menu::LIBRARY_WIDTH, menu::entries_height(&entries));
        let screen = ctx.content_rect();
        let left = cell.left().min(screen.right() - size.x - 8.0).max(screen.left() + 8.0);
        let top = (cell.center().y - size.y / 2.0)
            .clamp(screen.top() + MENU_TOP, (screen.bottom() - size.y - 8.0).max(screen.top() + MENU_TOP));
        let rect = Rect::from_min_size(pos2(left, top), size);
        let mut tapped = None;
        self.move_menu.show(ctx, "assets move menu", rect, rect.left_center(), MENU_GLASS, backdrops, |ui| {
            tapped = menu::entries(ui, look, rect, &entries);
        });
        if !self.move_menu.is_open() {
            self.move_for = None;
        }
        let Some(&(li, fi)) = tapped.and_then(|t| targets.get(t)) else { return };
        self.move_menu.close();
        self.move_for = None;
        session.move_asset(id, li as u32, fi as u32);
    }

    /// Asks before deleting, as the brush library does.
    fn ask_delete(&mut self, what: Deleting) {
        let (title, message) = match what {
            Deleting::Library(_) => ("Delete this library?", "Its folders and assets go with it."),
            Deleting::Folder(..) => ("Delete this folder?", "It can't be brought back."),
            Deleting::Asset(_) => ("Delete this asset?", "It can't be brought back."),
        };
        self.delete_what = Some(what);
        self.deleting.open(title, message, "Delete", Some(&icons::TRASH));
    }
}

/// Where a share goes: a save dialog titled for it, the name made safe for a file with the
/// extension put on it.
fn share_path(title: &str, kind: &str, extension: &str, name: &str) -> Option<std::path::PathBuf> {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || " _-".contains(c) { c } else { '_' }).collect();
    let safe = if safe.trim().is_empty() { "Asset".to_owned() } else { safe };
    crate::files::dialog()
        .set_title(title)
        .add_filter(kind, &[extension])
        .set_file_name(format!("{safe}.{extension}"))
        .save_file()
}

/// A rename field's whole name chosen, to type over.
fn pick_all(output: &mut egui::widgets::text_edit::TextEditOutput, ctx: &egui::Context, len: usize) {
    output.response.request_focus();
    let all = CCursorRange::two(CCursor::new(0), CCursor::new(len));
    output.state.cursor.set_char_range(Some(all));
    std::mem::take(&mut output.state).store(ctx, output.response.id);
}

/// A picture as large as fits `frame`, centred, keeping its shape.
fn fit_rect(frame: Rect, width: u32, height: u32) -> Rect {
    let ratio = if width > 0 && height > 0 { width as f32 / height as f32 } else { 1.0 };
    let size = if ratio <= frame.width() / frame.height() {
        vec2(frame.height() * ratio, frame.height())
    } else {
        vec2(frame.width(), frame.width() / ratio)
    };
    Rect::from_center_size(frame.center(), size)
}

fn format(size: f32, weight: Weight, colour: Color32) -> TextFormat {
    TextFormat { font_id: weight.font(size), color: colour, extra_letter_spacing: -0.045 * size, ..Default::default() }
}

/// Text on one line, cut short with an ellipsis past `max_width`.
fn one_line(ui: &Ui, text: &str, size: f32, weight: Weight, colour: Color32, max_width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, format(size, weight, colour));
    job.wrap = TextWrapping { max_width, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

/// Text on two lines at most, cut short with an ellipsis past `max_width`.
fn two_lines(ui: &Ui, text: &str, size: f32, weight: Weight, colour: Color32, max_width: f32) -> Arc<egui::Galley> {
    let mut job = LayoutJob::default();
    job.append(text, 0.0, format(size, weight, colour));
    job.wrap = TextWrapping { max_width, max_rows: 2, break_anywhere: false, overflow_character: Some('…') };
    ui.painter().layout_job(job)
}

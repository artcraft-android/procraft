//! The store: a sheet over the work, listing what the catalogues offer, one item's own page, and
//! Settings with the sources it reads. Drawn from the store's data (`procraft_core::Store`), at the
//! measurements of the board the design was drawn on.

use std::sync::Arc;
use std::time::Instant;

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, Vec2};

use crate::alert::{Alert, Answer};
use crate::look::{Look, Themed, Weight, argb, text, text_width, wrapped};
use crate::{icons, ui};

/// The sheet's own size, and how far its content sits in from its sides.
const WIDTH: f32 = 960.0;
const HEIGHT: f32 = 780.0;
const CORNER: f32 = 22.0;
const INSET: f32 = 40.0;
/// The bar over the sheet, and the room the large title keeps under it.
const BAR: f32 = 56.0;
const TITLE_SIZE: f32 = 34.0;
const TITLE_LINE: f32 = 41.0;
const SEARCH_HEIGHT: f32 = 36.0;
const PILL_HEIGHT: f32 = 30.0;
/// One row: a 42 picture, a name over one more line, and the state at the end.
const ROW_HEIGHT: f32 = 67.0;
/// How many rows a section shows before its See all.
const ROWS_SHOWN: usize = 4;
/// How a shelf sits: four cards across, and the room under each picture for its name, its line and
/// its state.
const SHELF_CARDS: usize = 4;
const SHELF_GAP: f32 = 13.0;
/// The band the board gives the first thing listed, at the top of the list.
const BAND_HEIGHT: f32 = 260.0;
const BAND_GAP: f32 = 24.0;
const TILE: f32 = 42.0;
const PILL_STATE: f32 = 28.0;
const HEADING_ABOVE: f32 = 40.0;
const HEADING_BELOW: f32 = 16.0;
const SECTION: f32 = 22.0;
/// The item page's own pieces.
const ICON: f32 = 120.0;
const PROSE: f32 = 620.0;
const CHIP: f32 = 28.0;
/// Settings: a group's heading, its card of rows, and the line that explains it.
const GROUP_ROW: f32 = 56.0;
const GROUP_HEAD: f32 = 16.0;
const GROUP_FOOT: f32 = 26.0;
// a grouped list's header rhythm: the header's line, and the 8 between it and what it labels
const GROUP_LINE: f32 = 17.0;
// a grouped list's separator: inset 16 in from both content edges
const GROUP_RULE_INSET: f32 = 16.0;
const GROUP_HEAD_BELOW: f32 = 8.0;

/// The same colours the board writes, as the interface's pairs: dark first, light second.
pub(crate) const BG: Themed = Themed(0xFF212121, 0xFFF0F0F0);
pub(crate) const TITLE: Themed = Themed(0xFFFFFFFF, 0xFF111111);
pub(crate) const ROW_INK: Themed = Themed(0x99EBEBF5, 0x993C3C43);
pub(crate) const IDLE: Themed = Themed(0x80EBEBF5, 0x733C3C43);
pub(crate) const HINT: Themed = Themed(0x61EBEBF5, 0x613C3C43);
const SEARCH_FILL: Themed = Themed(0x2E767680, 0x1F767680);
pub(crate) const SEP: Themed = Themed(0x99545458, 0x4A3C3C43);
pub(crate) const TILE_FILL: Themed = Themed(0x14FFFFFF, 0x14000000);
pub(crate) const GROUP_FILL: Themed = Themed(0x0EFFFFFF, 0xFFF7F7F7);
/// The warm paper a drawn picture stands on.
pub(crate) const PAPER: Themed = Themed(0xFFFBF9F4, 0xFFFBF9F4);
const PILL_FILL: Themed = Themed(0x38767680, 0x1F767680);
const PILL_STATE_FILL: Themed = Themed(0x47787880, 0x47787880);
const PILL_ON: Themed = Themed(0xFFEDEDED, 0xFF1B1B1B);
const PILL_ON_INK: Themed = Themed(0xFF1B1B1B, 0xFFEDEDED);
const CHIP_FILL: Themed = Themed(0x14FFFFFF, 0x14000000);
const SWITCH_OFF: Themed = Themed(0x52787880, 0x52787880);
pub(crate) const ACCENT: Themed = Themed(0xFF0A84FF, 0xFF007AFF);
/// The white a filled accent pill writes in.
const ACCENT_ON: Themed = Themed(0xFFFFFFFF, 0xFFFFFFFF);
const DANGER: Themed = Themed(0xFFFF453A, 0xFFD32B21);
const DISABLED: Themed = Themed(0x42EBEBF5, 0x42424242);

/// How long the sheet takes to come in, and how dark the work behind it goes.
const IN_S: f32 = 0.2;
const DIM: f32 = 0.7;

/// Which page of the store is showing.
#[derive(Clone, Debug, Default, PartialEq)]
enum View {
    #[default]
    Browse,
    /// One item's own page, by its catalogue id and the source that listed it.
    Item {
        source: String,
        id: String,
    },
    Settings,
    /// One source's own page, by its id.
    Source {
        id: String,
    },
    /// One kind's whole list, as a section's See all opens it.
    SeeAll(procraft_core::ItemKind),
    Downloads,
}

/// Which choice menu is open.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuKind {
    Filters,
    Sort,
}

/// What a wall of cards is ordered by.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum SortBy {
    #[default]
    Title,
    Author,
    Size,
}

/// Which field has the keys.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Field {
    Search,
    Add,
}

/// What a worker was asked to do.
enum JobWhat {
    Adding,
    Refreshing,
    Installing(Box<procraft_core::StoreItem>),
}

/// What it brought back.
enum Finished {
    Added(Option<String>),
    Refreshed(Option<String>),
    Fetched(procraft_core::Fetched),
}

/// A read on a thread of its own: the sheet keeps drawing while a web address answers.
struct Job {
    done: std::sync::mpsc::Receiver<Finished>,
    what: JobWhat,
}

/// The store's sheet: what it is showing, and the store it is showing.
pub struct StoreSheet {
    pub open: bool,
    store: Option<Arc<procraft_core::Store>>,
    /// What has been typed into the search field, and whether it has the keys.
    query: String,
    typing: Option<Field>,
    /// Which category is chosen: 0 is All, then the store's own list.
    category: usize,
    view: View,
    /// The pages this one was opened from, latest last: Back walks them.
    history: Vec<View>,
    /// How far the page has been scrolled, and how much of it there is above the fold.
    scroll: f32,
    height: f32,
    opened: Option<Instant>,
    /// Adding a source: its sheet, its field and the line saying why it was not taken.
    adding: bool,
    address: String,
    refusal: String,
    /// The question a removal asks, and the source it is about.
    alert: Alert,
    removing: Option<String>,
    /// The item waiting on a file to be chosen, while the store cannot fetch one.
    installing: Option<procraft_core::StoreItem>,
    /// The read in flight, if one is.
    job: Option<Job>,
    /// Whether the debug install hook already ran.
    shot_installed: bool,
    /// The reason the last try at an item did not install it, kept for its button.
    problem: Option<(String, String)>,
    /// The pictures the catalogues gave, fetched and kept by address.
    pictures: crate::pictures::Pictures,
    /// The filters the list is cut by, and the menu that sets them.
    kind: Option<procraft_core::ItemKind>,
    from: Option<String>,
    menu: Option<MenuKind>,
    /// Where that menu hangs from, and what the wall sorts by.
    filters_at: Option<Rect>,
    sort_at: Option<Rect>,
    sort: SortBy,
    ascending: bool,
}

/// Opens the sheet on a page of someone's choosing, for screenshots: PROCRAFT_STORE_SHOT names the
/// page, PROCRAFT_STORE_SOURCE a catalogue to add first and PROCRAFT_STORE_ITEM the item page to
/// open. It runs every frame until the app closes, because the first frames come before the
/// gallery is up and the sheet would be thrown away with them. A debug build only.
#[cfg(debug_assertions)]
pub fn shot_setup(sheet: &mut StoreSheet) {
    let Ok(what) = std::env::var("PROCRAFT_STORE_SHOT") else { return };
    if !sheet.open {
        sheet.open();
    }
    let Some(store) = sheet.store.clone() else { return };
    if let Ok(path) = std::env::var("PROCRAFT_STORE_SOURCE")
        && !store.sources().iter().any(|source| source.address == path)
    {
        let _ = store.add_source(path);
    }
    if what == "browse" {
        return;
    }
    let items = store.items();
    match what.as_str() {
        "settings" => sheet.view = View::Settings,
        "downloads" => sheet.view = View::Downloads,
        "add" => {
            sheet.view = View::Settings;
            sheet.adding = true;
            sheet.address = "https://example.org/catalogue.json".to_owned();
        }
        "seeall" => {
            if let Some(item) = items.iter().find(|item| item.kind == procraft_core::ItemKind::BrushSet) {
                sheet.view = View::SeeAll(item.kind);
            }
        }
        "item" => {
            let named = std::env::var("PROCRAFT_STORE_ITEM").ok();
            let picked = named
                .and_then(|id| items.iter().find(|item| item.id == id).cloned())
                .or_else(|| items.iter().find(|item| item.file.is_empty()).cloned());
            if let Some(item) = picked {
                sheet.view = View::Item { source: item.source.clone(), id: item.id.clone() };
            }
        }
        "source" => {
            if let Some(listed) = store.sources().into_iter().find(|source| source.id != "procraft") {
                sheet.view = View::Source { id: listed.id };
            } else if let Some(listed) = store.sources().into_iter().next() {
                sheet.view = View::Source { id: listed.id };
            }
        }
        _ => sheet.view = View::Browse,
    }
    sheet.history.clear();
}

impl Default for StoreSheet {
    fn default() -> Self {
        StoreSheet {
            open: false,
            store: None,
            query: String::new(),
            typing: None,
            category: 0,
            view: View::Browse,
            history: Vec::new(),
            scroll: 0.0,
            height: 0.0,
            opened: None,
            adding: false,
            address: String::new(),
            refusal: String::new(),
            alert: Alert::default(),
            removing: None,
            installing: None,
            job: None,
            shot_installed: false,
            problem: None,
            pictures: crate::pictures::Pictures::default(),
            kind: None,
            from: None,
            menu: None,
            filters_at: None,
            sort_at: None,
            sort: SortBy::default(),
            ascending: true,
        }
    }
}

impl StoreSheet {
    /// Opens the sheet, reading the store's sources the first time.
    pub fn open(&mut self) {
        if self.store.is_none() {
            let dir = crate::files::app_dir().join("store").to_string_lossy().into_owned();
            self.store = Some(procraft_core::Store::open(dir));
        }
        self.open = true;
        self.opened = Some(Instant::now());
        self.view = View::Browse;
        self.history.clear();
        self.scroll = 0.0;
        self.typing = None;
        self.adding = false;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.typing = None;
        self.adding = false;
        self.view = View::Browse;
        self.history.clear();
    }

    /// A step back, to the page this one was opened from; with nothing behind it, the store closes.
    fn back(&mut self) {
        if self.adding {
            self.adding = false;
            self.typing = None;
            return;
        }
        self.menu = None;
        self.typing = None;
        match self.history.pop() {
            Some(previous) => {
                self.view = previous;
                self.scroll = 0.0;
            }
            None if self.view == View::Browse => self.close(),
            None => {
                self.view = View::Browse;
                self.scroll = 0.0;
            }
        }
    }

    /// Goes to `view`, keeping the page it leaves behind for Back.
    fn go(&mut self, view: View) {
        self.history.push(std::mem::replace(&mut self.view, view));
        self.scroll = 0.0;
        self.typing = None;
    }

    /// Draws the sheet over everything, taking the presses it needs. Closes on Escape, on Back or
    /// on a press beside it.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &Arc<procraft_core::CanvasSession>,
        plugins: &Arc<procraft_core::Plugins>,
    ) {
        let Some(store) = self.store.clone() else {
            self.close();
            return;
        };
        // what a worker read comes back here, one answer a frame
        self.poll(ctx, session, plugins, &store);
        self.pictures.poll(ctx);
        let screen = ctx.content_rect();
        let shown = self.opened.map_or(1.0, |at| (at.elapsed().as_secs_f32() / IN_S).clamp(0.0, 1.0));
        if shown < 1.0 {
            ctx.request_repaint();
        }
        // an alert over the sheet takes Escape itself, so the sheet leaves it alone
        if !self.alert.is_open() && ui::escape(ctx) {
            match self.typing.take() {
                Some(_) => {}
                None => {
                    self.back();
                    return;
                }
            }
        }
        let size = Vec2::new(WIDTH.min(screen.width() - 40.0), HEIGHT.min(screen.height() - 96.0));
        let sheet = Rect::from_center_size(screen.center(), size);
        // the work behind it, dimmed, taking the presses that land outside the sheet
        egui::Area::new(Id::unique("store scrim")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |area| {
            let beside = area.allocate_rect(screen, Sense::click());
            area.painter().rect_filled(screen, 0.0, Color32::from_black_alpha((DIM * shown * 255.0) as u8));
            if beside.clicked() && !self.alert.is_open() {
                self.close();
            }
        });
        if !self.open {
            return;
        }
        // the page that is showing, found before the sheet is drawn
        let item = match &self.view {
            View::Item { source, id } => store.items().into_iter().find(|i| &i.id == id && &i.source == source),
            _ => None,
        };
        let source = match &self.view {
            View::Source { id } => store.source(id.clone()),
            _ => None,
        };
        // a press anywhere but the menu closes it
        let had_menu = self.menu.is_some();
        egui::Area::new(Id::unique("store sheet")).fixed_pos(sheet.min).order(Order::Debug).show(ctx, |area| {
            area.allocate_rect(sheet, Sense::click_and_drag());
            area.painter().add(
                egui::epaint::Shadow { offset: [0, 0], blur: 48, spread: 0, color: Color32::from_black_alpha(130) }
                    .as_shape(sheet, CORNER),
            );
            area.painter().with_clip_rect(sheet).rect_filled(sheet, CORNER, argb(BG));
            let ink = move |colour: Themed| argb(colour).gamma_multiply(shown);
            let back = Rect::from_min_size(Pos2::new(sheet.left() + INSET, sheet.top() + 11.0), Vec2::new(44.0, 34.0));
            look.icon(area, &icons::CHEVRON_LEFT, back.center(), 22.0, ink(TITLE));
            // the bar says which page it is: an item, a source, a section, or Settings
            let centre = match (&item, &source, self.view.clone()) {
                (Some(item), _, _) => Some(item.name.clone()),
                (_, Some(source), _) => Some(source.name.clone()),
                (_, _, View::Settings) => Some("Settings".to_owned()),
                (_, _, View::SeeAll(kind)) => Some(section_title(kind).to_owned()),
                (_, _, View::Downloads) => Some("Downloads".to_owned()),
                _ => None,
            };
            if let Some(name) = centre {
                text(
                    area.painter(),
                    Pos2::new(sheet.center().x, back.center().y),
                    Align2::CENTER_CENTER,
                    &name,
                    17.0,
                    Weight::SemiBold,
                    ink(TITLE),
                );
            }
            // the trailing end: the page's own action hard on the inset, then the downloads one
            // step in from it, as the board orders the two
            let mut right = sheet.right() - INSET;
            if matches!(self.view, View::Browse) && !self.adding {
                let gear = Rect::from_center_size(Pos2::new(right - 11.0, back.center().y), Vec2::splat(32.0));
                look.icon(area, &icons::GEAR, gear.center(), 22.0, ink(TITLE));
                if area.interact(gear, area.scope_id().with("store settings"), Sense::click()).clicked() {
                    self.go(View::Settings);
                }
                right -= 40.0;
            }
            if self.view != View::Downloads && !self.adding {
                let downloads = Rect::from_center_size(Pos2::new(right - 11.0, back.center().y), Vec2::splat(32.0));
                download_glyph(area.painter(), downloads.center(), 22.0, ink(TITLE));
                if area.interact(downloads, area.scope_id().with("store downloads"), Sense::click()).clicked() {
                    self.go(View::Downloads);
                }
                right -= 40.0;
            }
            match self.view.clone() {
                View::Browse if !self.adding => {}
                View::SeeAll(_) => {
                    let sort = Rect::from_center_size(Pos2::new(right - 16.0, back.center().y), Vec2::splat(32.0));
                    self.sort_at = Some(sort);
                    sort_glyph(area.painter(), sort.center(), 22.0, ink(TITLE));
                    if area.interact(sort, area.scope_id().with("store sort"), Sense::click()).clicked() {
                        self.menu = if self.menu == Some(MenuKind::Sort) { None } else { Some(MenuKind::Sort) };
                    }
                }
                _ => {}
            }
            let body = Rect::from_min_max(
                Pos2::new(sheet.left() + INSET, sheet.top() + BAR),
                Pos2::new(sheet.right() - INSET, sheet.bottom() - 20.0),
            );
            match (&item, &source) {
                (Some(item), _) => self.item_page(area, look, &store, body, ink, item),
                (_, Some(source)) => self.source_page(area, &store, body, ink, source.clone()),
                _ if self.view == View::Settings => self.settings_page(area, look, &store, body, ink),
                _ if self.view == View::Downloads => self.downloads_page(area, look, &store, body, ink),
                _ if matches!(self.view, View::SeeAll(_)) => self.see_all_page(area, look, &store, body, ink),
                _ => self.page(area, look, &store, body, ink),
            }
            if area.interact(back, area.scope_id().with("store back"), Sense::click()).clicked() {
                self.back();
            }
            if self.adding {
                self.add_sheet(area, &store, sheet, ink);
            }
            if let Some(menu) = self.menu {
                self.menu_panel(area, &store, ink, menu, sheet);
            }
            if had_menu && area.input(|i| i.pointer.any_click()) {
                self.menu = None;
            }
        });
        // the question a removal asks, over the sheet
        if self.alert.is_open() {
            match self.alert.show(ctx, look) {
                Some(Answer::Action) => {
                    if let Some(id) = self.removing.take() {
                        store.remove_source(id);
                        self.back();
                    } else if let Some(item) = self.installing.take() {
                        self.install_from_file(session, plugins, &store, item);
                    }
                }
                Some(_) => {
                    self.removing = None;
                    self.installing = None;
                }
                None => {}
            }
        }
    }

    /// Takes an item's file from this machine, checks it against what the catalogue says, and
    /// installs it. The fetch, when it arrives, hands the same bytes to the same place.
    fn install_from_file(
        &mut self,
        session: &Arc<procraft_core::CanvasSession>,
        plugins: &Arc<procraft_core::Plugins>,
        store: &Arc<procraft_core::Store>,
        item: procraft_core::StoreItem,
    ) {
        let Some(path) = rfd::FileDialog::new().set_title("The file the catalogue lists").pick_file() else {
            return;
        };
        let Ok(bytes) = std::fs::read(&path) else {
            self.alert.tell("Not installed", &format!("{} could not be read.", path.display()), None);
            return;
        };
        self.take_bytes(session, plugins, store, item, &bytes);
    }

    /// Checks the bytes against what the catalogue said, installs them, and records what was taken.
    fn take_bytes(
        &mut self,
        session: &Arc<procraft_core::CanvasSession>,
        plugins: &Arc<procraft_core::Plugins>,
        store: &Arc<procraft_core::Store>,
        item: procraft_core::StoreItem,
        bytes: &[u8],
    ) {
        match crate::install::take(session, plugins, &item, bytes) {
            // the pill itself says it is installed: no message, and no window over the work
            Ok(()) => {
                store.keep_installed(item.id.clone(), procraft_core::digest_of(bytes));
            }
            Err(why) => self.problem = Some((item.id.clone(), why)),
        }
    }

    /// Picks up what a worker brought back, one answer a frame.
    fn poll(
        &mut self,
        ctx: &egui::Context,
        session: &Arc<procraft_core::CanvasSession>,
        plugins: &Arc<procraft_core::Plugins>,
        store: &Arc<procraft_core::Store>,
    ) {
        let Some(job) = &self.job else { return };
        // a read may take half a minute: the sheet keeps asking rather than holding the frame
        ctx.request_repaint();
        let answer = match job.done.try_recv() {
            Ok(answer) => answer,
            Err(std::sync::mpsc::TryRecvError::Empty) => return,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                self.job = None;
                return;
            }
        };
        let Some(what) = self.job.take().map(|job| job.what) else { return };
        match (what, answer) {
            (JobWhat::Adding, Finished::Added(refusal)) => match refusal {
                None => {
                    self.adding = false;
                    self.typing = None;
                    self.address.clear();
                    self.refusal.clear();
                }
                Some(why) => self.refusal = format!("Not added: {why}"),
            },
            (JobWhat::Refreshing, Finished::Refreshed(Some(why))) => {
                self.alert.tell("Not read", &why, None);
            }
            (JobWhat::Refreshing, Finished::Refreshed(None)) => {}
            (JobWhat::Installing(item), Finished::Fetched(fetched)) => {
                if fetched.failure.is_empty() {
                    self.take_bytes(session, plugins, store, *item, &fetched.bytes);
                } else {
                    // no fetch of it worked, so the file can be chosen by hand instead
                    self.installing = Some(*item);
                    self.alert.ask(crate::alert::Asking {
                        title: "Not installed".to_owned(),
                        message: format!("{}. You can choose the file yourself instead.", fetched.failure),
                        action: "Choose File".to_owned(),
                        icon: Some(&icons::FOLDER_FILL),
                        destructive: false,
                        extra: None,
                        first: None,
                        alone: false,
                    });
                }
            }
            _ => {}
        }
    }

    /// Reads the address the add sheet holds, on a thread of its own.
    fn start_add(&mut self, store: &Arc<procraft_core::Store>) {
        if self.job.is_some() {
            return;
        }
        let (sender, done) = std::sync::mpsc::channel();
        let store = store.clone();
        let address = self.address.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Finished::Added(store.add_source(address)));
        });
        self.job = Some(Job { done, what: JobWhat::Adding });
    }

    /// Reads a source again, on a thread of its own.
    fn start_refresh(&mut self, store: &Arc<procraft_core::Store>, id: String) {
        if self.job.is_some() {
            return;
        }
        let (sender, done) = std::sync::mpsc::channel();
        let store = store.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Finished::Refreshed(store.refresh(id)));
        });
        self.job = Some(Job { done, what: JobWhat::Refreshing });
    }

    /// Fetches an item's file, on a thread of its own.
    /// A debug hook: `PROCRAFT_STORE_INSTALL=<id>` installs that item once, so a capture can show
    /// the installed state without a hand on the mouse.
    #[cfg(debug_assertions)]
    pub fn shot_install(&mut self) {
        if self.shot_installed {
            return;
        }
        let Ok(id) = std::env::var("PROCRAFT_STORE_INSTALL") else {
            return;
        };
        let Some(store) = self.store.clone() else {
            return;
        };
        let Some(item) = store.items().into_iter().find(|item| item.id == id) else {
            return;
        };
        self.shot_installed = true;
        self.start_install(&store, item);
    }

    fn start_install(&mut self, store: &Arc<procraft_core::Store>, item: procraft_core::StoreItem) {
        if self.job.is_some() {
            return;
        }
        let (sender, done) = std::sync::mpsc::channel();
        let store = store.clone();
        let id = item.id.clone();
        std::thread::spawn(move || {
            let _ = sender.send(Finished::Fetched(store.fetch_item(id)));
        });
        self.job = Some(Job { done, what: JobWhat::Installing(Box::new(item)) });
    }

    /// Whether the add sheet's read is in flight.
    fn adding_now(&self) -> bool {
        matches!(self.job.as_ref().map(|job| &job.what), Some(JobWhat::Adding))
    }

    /// Whether this item's file is being fetched.
    fn installing_now(&self, id: &str) -> bool {
        matches!(
            self.job.as_ref().map(|job| &job.what),
            Some(JobWhat::Installing(item)) if item.id == id
        )
    }

    /// The page under the bar: the large title, the search field, the categories, then the list.
    fn page(
        &mut self,
        ui: &egui::Ui,
        look: &mut Look,
        store: &Arc<procraft_core::Store>,
        body: Rect,
        ink: impl Fn(Themed) -> Color32 + Copy,
    ) {
        let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
        let title = Rect::from_min_size(Pos2::new(body.left(), body.top() + 12.0), Vec2::new(body.width(), TITLE_LINE));
        text(
            &painter,
            Pos2::new(title.left(), title.center().y),
            Align2::LEFT_CENTER,
            "Store",
            TITLE_SIZE,
            Weight::Bold,
            ink(TITLE),
        );
        // the search field, with the drawn magnifier the design gives it
        let search =
            Rect::from_min_size(Pos2::new(body.left(), title.bottom() + 16.0), Vec2::new(body.width(), SEARCH_HEIGHT));
        painter.rect_filled(search, 10.0, ink(SEARCH_FILL));
        let mag = Pos2::new(search.left() + 14.0, search.center().y);
        painter.circle_stroke(mag, 5.0, Stroke::new(1.6, ink(HINT)));
        painter.line_segment(
            [Pos2::new(mag.x + 3.6, mag.y + 3.6), Pos2::new(mag.x + 7.0, mag.y + 7.0)],
            Stroke::new(1.6, ink(HINT)),
        );
        let typed = Rect::from_min_max(
            Pos2::new(search.left() + 30.0, search.top()),
            Pos2::new(search.right() - 10.0, search.bottom()),
        );
        if self.query.is_empty() {
            text(
                &painter,
                Pos2::new(typed.left(), typed.center().y),
                Align2::LEFT_CENTER,
                "Search",
                17.0,
                Weight::Regular,
                ink(HINT),
            );
        } else {
            text(
                &painter,
                Pos2::new(typed.left(), typed.center().y),
                Align2::LEFT_CENTER,
                &self.query,
                17.0,
                Weight::Regular,
                ink(TITLE),
            );
            let width = text_width(&painter, &self.query, 17.0, Weight::Regular).min(typed.width());
            painter.rect_filled(
                Rect::from_min_size(
                    Pos2::new(typed.left() + width + 1.0, typed.center().y - 10.0),
                    Vec2::new(2.0, 20.0),
                ),
                1.0,
                ink(ACCENT),
            );
        }
        if ui.interact(search, ui.scope_id().with("store search"), Sense::click()).clicked() {
            self.typing = Some(Field::Search);
        } else if ui.input(|i| i.pointer.any_click()) {
            let at = ui.input(|i| i.pointer.interact_pos()).unwrap_or(Pos2::ZERO);
            if !search.contains(at) && self.typing == Some(Field::Search) {
                self.typing = None;
            }
        }
        self.read_keys(ui.ctx());
        // the categories: All, then the store's own list, with the Filters pill at the trailing end
        let names = store.categories();
        let mut x = body.left();
        let pills =
            Rect::from_min_size(Pos2::new(body.left(), search.bottom() + 16.0), Vec2::new(body.width(), PILL_HEIGHT));
        let filters = Rect::from_min_size(Pos2::new(pills.right() - 76.0, pills.top()), Vec2::new(76.0, PILL_HEIGHT));
        for (at, name) in std::iter::once("All".to_owned()).chain(names.iter().cloned()).enumerate() {
            let width = text_width(&painter, &name, 13.0, Weight::Regular) + 24.0;
            let pill = Rect::from_min_size(Pos2::new(x, pills.top()), Vec2::new(width, PILL_HEIGHT));
            if pill.right() > filters.left() - 8.0 {
                break;
            }
            let on = self.category == at;
            painter.rect_filled(pill, PILL_HEIGHT / 2.0, ink(if on { PILL_ON } else { PILL_FILL }));
            text(
                &painter,
                pill.center(),
                Align2::CENTER_CENTER,
                &name,
                13.0,
                if on { Weight::SemiBold } else { Weight::Regular },
                ink(if on { PILL_ON_INK } else { TITLE }),
            );
            if ui.interact(pill, ui.scope_id().with(("store pill", at)), Sense::click()).clicked() {
                self.category = at;
                self.scroll = 0.0;
            }
            x = pill.right() + 6.0;
        }
        // the two axes the category row does not carry: kind and source
        self.filters_at = Some(filters);
        let filtered = self.kind.is_some() || self.from.is_some();
        painter.rect_filled(filters, PILL_HEIGHT / 2.0, ink(PILL_FILL));
        text(
            &painter,
            filters.center(),
            Align2::CENTER_CENTER,
            "Filters",
            13.0,
            Weight::Regular,
            ink(if filtered { ACCENT } else { TITLE }),
        );
        if ui.interact(filters, ui.scope_id().with("store filters"), Sense::click()).clicked() {
            self.menu = if self.menu == Some(MenuKind::Filters) { None } else { Some(MenuKind::Filters) };
        }
        // the list, under the categories, scrolling; a source that could not be read says so once
        // above it, with the day of its copy
        let stale = store.sources().into_iter().find(|source| !source.failure.is_empty() && source.read > 0);
        let stale_room = if stale.is_some() { 26.0 } else { 0.0 };
        let list = Rect::from_min_max(Pos2::new(body.left(), pills.bottom() + 12.0 + stale_room), body.max);
        if let Some(source) = &stale {
            let said = format!(
                "{} could not be refreshed. Showing what it had on {}.",
                source.name,
                date_text(source.read, false)
            );
            let middle = pills.bottom() + 25.0;
            let info = Pos2::new(body.left() + 6.0, middle);
            painter.circle_stroke(info, 6.5, Stroke::new(1.2, ink(IDLE)));
            painter.line_segment(
                [Pos2::new(info.x, info.y - 3.0), Pos2::new(info.x, info.y + 3.5)],
                Stroke::new(1.2, ink(IDLE)),
            );
            painter.circle_filled(Pos2::new(info.x, info.y - 5.0), 0.8, ink(IDLE));
            text(
                &painter,
                Pos2::new(body.left() + 20.0, middle),
                Align2::LEFT_CENTER,
                &said,
                13.0,
                Weight::Regular,
                ink(ROW_INK),
            );
        }
        let chosen = self.category.checked_sub(1).and_then(|at| names.get(at).cloned());
        let query = self.query.trim().to_lowercase();
        let mut items = if query.is_empty() { store.items() } else { store.search(query.clone()) };
        if let Some(category) = &chosen {
            items.retain(|item| &item.category == category);
        }
        if let Some(kind) = self.kind {
            items.retain(|item| item.kind == kind);
        }
        if let Some(from) = &self.from {
            items.retain(|item| &item.source == from);
        }
        let empty = if items.is_empty() {
            match (query.is_empty(), chosen.is_some()) {
                (false, _) => format!("No results for {}.", self.query.trim()),
                (true, true) => "Nothing here in this category.".to_owned(),
                (true, false) => "Every source is off. Turn one on in Sources.".to_owned(),
            }
        } else {
            String::new()
        };
        // a source that could not be read says so once, above the list, with the day of its copy
        let groups =
            [procraft_core::ItemKind::BrushSet, procraft_core::ItemKind::Plugin, procraft_core::ItemKind::AssetPack];
        let content = if empty.is_empty() {
            let mut content = 40.0;
            // the band comes first, and only on the page as it opens: cut by a search, a category
            // or a filter it would be showing something the list below does not
            let banded =
                self.query.trim().is_empty() && self.category == 0 && self.kind.is_none() && self.from.is_none();
            if banded && !items.is_empty() {
                content += BAND_HEIGHT + BAND_GAP;
            }
            // what has changed since it was installed sits above the kinds, in rows as the board has it
            let updated = items.iter().filter(|item| needs_update(store, item)).count();
            if updated > 0 {
                content += HEADING_ABOVE + 30.0 + HEADING_BELOW + updated.min(ROWS_SHOWN) as f32 * ROW_HEIGHT;
            }
            // and what the store took sits under them
            let took = store.installed().iter().filter(|one| items.iter().any(|item| item.id == one.id)).count();
            if took > 0 {
                content += HEADING_ABOVE + 30.0 + HEADING_BELOW + shelf_height(list.width());
            }
            for kind in groups {
                let count = items.iter().filter(|i| i.kind == kind).count();
                if count == 0 {
                    continue;
                }
                content += HEADING_ABOVE + 30.0 + HEADING_BELOW;
                content += if kind == procraft_core::ItemKind::Plugin {
                    count.min(ROWS_SHOWN) as f32 * ROW_HEIGHT
                } else {
                    shelf_height(list.width())
                };
            }
            content
        } else {
            0.0
        };
        self.height = content;
        self.scroll = self.scrolled(ui, list);
        let clip = list.intersect(ui.clip_rect());
        let painter = ui.painter().with_clip_rect(clip);
        if !empty.is_empty() {
            text(
                &painter,
                Pos2::new(list.center().x, list.top() + 48.0),
                Align2::CENTER_TOP,
                &empty,
                15.0,
                Weight::Medium,
                ink(ROW_INK),
            );
            return;
        }
        let mut y = list.top() - self.scroll;
        // the band, then what has changed since it was installed, as the board has them
        let banded = self.query.trim().is_empty() && self.category == 0 && self.kind.is_none() && self.from.is_none();
        if banded && !items.is_empty() {
            // an item with a picture the publisher gave comes first, as the band is a picture
            let featured = items.iter().find(|item| !item.preview.is_empty()).or_else(|| items.first());
            self.band(&painter, look, ui, store, featured, list, y, ink);
            y += BAND_HEIGHT + BAND_GAP;
        }
        let updated: Vec<&procraft_core::StoreItem> = items.iter().filter(|item| needs_update(store, item)).collect();
        if !updated.is_empty() {
            let head = Rect::from_min_size(
                Pos2::new(list.left() + 16.0, y + HEADING_ABOVE),
                Vec2::new(list.width() - 16.0, 30.0),
            );
            text(&painter, head.left_top(), Align2::LEFT_TOP, "Updates", SECTION, Weight::Bold, ink(TITLE));
            y += HEADING_ABOVE + 30.0 + HEADING_BELOW;
            for (at, item) in updated.iter().take(ROWS_SHOWN).enumerate() {
                let row = Rect::from_min_size(Pos2::new(list.left(), y), Vec2::new(list.width(), ROW_HEIGHT));
                self.row(&painter, look, ui, store, item, row, at, ink);
                y += ROW_HEIGHT;
            }
        }
        for kind in groups {
            let group: Vec<&procraft_core::StoreItem> = items.iter().filter(|i| i.kind == kind).collect();
            if group.is_empty() {
                continue;
            }
            let head = Rect::from_min_size(
                Pos2::new(list.left() + 16.0, y + HEADING_ABOVE),
                Vec2::new(list.width() - 16.0, 30.0),
            );
            text(&painter, head.left_top(), Align2::LEFT_TOP, section_title(kind), SECTION, Weight::Bold, ink(TITLE));
            // a section that hides anything says so, and opens as its own wall
            if group.len() > ROWS_SHOWN {
                let see = Rect::from_min_max(Pos2::new(head.right() - 92.0, head.top()), head.right_top());
                text(
                    &painter,
                    Pos2::new(see.right() - 14.0, see.center().y),
                    Align2::RIGHT_CENTER,
                    "See all",
                    15.0,
                    Weight::Regular,
                    ink(ACCENT),
                );
                look.icon_on(
                    ui,
                    &painter,
                    &icons::CHEVRON_RIGHT,
                    Pos2::new(see.right() - 3.0, see.center().y),
                    11.0,
                    ink(ACCENT),
                );
                if ui.interact(see, ui.scope_id().with(("store see all", kind.text())), Sense::click()).clicked() {
                    self.go(View::SeeAll(kind));
                }
            }
            y += HEADING_ABOVE + 30.0 + HEADING_BELOW;
            if kind == procraft_core::ItemKind::Plugin {
                for (at, item) in group.iter().take(ROWS_SHOWN).enumerate() {
                    let row = Rect::from_min_size(Pos2::new(list.left(), y), Vec2::new(list.width(), ROW_HEIGHT));
                    if row.bottom() > clip.top() && row.top() < clip.bottom() {
                        self.row(&painter, look, ui, store, item, row, at, ink);
                    }
                    y += ROW_HEIGHT;
                }
            } else {
                // brushes and asset packs are a shelf of pictures, as the board draws them
                self.shelf(&painter, look, ui, store, &group, list, y, ink);
                y += shelf_height(list.width());
            }
        }
        // what the store took, at the foot, as the board puts it
        let took: Vec<&procraft_core::StoreItem> =
            store.installed().iter().filter_map(|one| items.iter().find(|item| item.id == one.id)).collect();
        if !took.is_empty() {
            let head = Rect::from_min_size(
                Pos2::new(list.left() + 16.0, y + HEADING_ABOVE),
                Vec2::new(list.width() - 16.0, 30.0),
            );
            text(&painter, head.left_top(), Align2::LEFT_TOP, "Installed", SECTION, Weight::Bold, ink(TITLE));
            y += HEADING_ABOVE + 30.0 + HEADING_BELOW;
            self.shelf(&painter, look, ui, store, &took, list, y, ink);
        }
        // the list's top edge dissolves under the pills rather than being cut level with them; at
        // the top of the page there is nothing to dissolve
        if self.scroll > 1.0 {
            let fade =
                Rect::from_min_max(Pos2::new(list.left(), list.top()), Pos2::new(list.right(), list.top() + 22.0));
            fade_down(&painter, fade, ink(BG), 255, 0);
        }
    }

    /// The band: the first thing the store lists, drawn large over the list, and opening it when
    /// tapped. The picture the catalogue gave would fill it where there is one.
    fn band(
        &mut self,
        painter: &egui::Painter,
        look: &mut Look,
        ui: &egui::Ui,
        store: &Arc<procraft_core::Store>,
        item: Option<&procraft_core::StoreItem>,
        area: Rect,
        y: f32,
        ink: impl Fn(Themed) -> Color32 + Copy,
    ) {
        let Some(item) = item else { return };
        let band = Rect::from_min_size(Pos2::new(area.left(), y), Vec2::new(area.width(), BAND_HEIGHT));
        painter.rect_filled(band, 20.0, ink(TILE_FILL));
        painter.rect_stroke(band, 20.0, Stroke::new(1.0, ink(SEP)), egui::StrokeKind::Inside);
        // the picture the catalogue gave fills it, cropped, under a scrim that keeps the words legible
        match self.pictures.get(ui.ctx(), store, &item.preview) {
            Some(picture) => {
                picture_rounded(painter, &picture, band, 20.0, fill_uv(picture.size_vec2(), band.size()));
                // a scrim that deepens toward the foot: its vertex colours interpolate across the
                // mesh, so the words hold on any picture with no step to see
                let scrim = Rect::from_min_max(Pos2::new(band.left(), band.top() + BAND_HEIGHT * 0.2), band.max);
                scrim_rounded(painter, scrim, 20.0, 8, 208);
            }
            None => look.icon_on(
                ui,
                painter,
                kind_icon(item.kind),
                Pos2::new(band.right() - 110.0, band.top() + 84.0),
                120.0,
                ink(IDLE).gamma_multiply(0.35),
            ),
        }
        let left = band.left() + 20.0;
        // the words stack above the foot: the app row, then one sentence, the name, the kicker
        let row = band.bottom() - 28.0;
        let sentence_top = band.bottom() - 78.0;
        let name_top = band.bottom() - 116.0;
        let kicker_top = band.bottom() - 138.0;
        let kicker = item.category.to_uppercase();
        text(painter, Pos2::new(left, kicker_top), Align2::LEFT_TOP, &kicker, 12.0, Weight::SemiBold, ink(ROW_INK));
        text(painter, Pos2::new(left, name_top), Align2::LEFT_TOP, &item.name, 30.0, Weight::Bold, ink(TITLE));
        let sentence = one_line(&item.description, band.width() - 40.0, painter);
        text(painter, Pos2::new(left, sentence_top), Align2::LEFT_TOP, &sentence, 15.0, Weight::Regular, ink(ROW_INK));
        // the app row: its icon, the publisher, and the state at the end
        let tile = Rect::from_center_size(Pos2::new(left + 12.0, row), Vec2::splat(24.0));
        painter.rect_filled(tile, 6.0, ink(PILL_FILL));
        look.icon_on(ui, painter, kind_icon(item.kind), tile.center(), 14.0, ink(TITLE));
        text(
            painter,
            Pos2::new(tile.right() + 8.0, tile.center().y),
            Align2::LEFT_CENTER,
            &item.publisher,
            13.0,
            Weight::Regular,
            ink(ROW_INK),
        );
        state_pill(
            painter,
            look,
            ui,
            installed(store, item),
            needs_update(store, item),
            band.right() - 20.0,
            tile.center().y,
            ink,
        );
        if ui.interact(band, ui.scope_id().with(("store band", item.id.clone())), Sense::click()).clicked() {
            self.go(View::Item { source: item.source.clone(), id: item.id.clone() });
        }
    }

    /// A shelf of cards, four across: the picture, the name, one line under it and the state.
    fn shelf(
        &mut self,
        painter: &egui::Painter,
        look: &mut Look,
        ui: &egui::Ui,
        store: &Arc<procraft_core::Store>,
        group: &[&procraft_core::StoreItem],
        area: Rect,
        y: f32,
        ink: impl Fn(Themed) -> Color32 + Copy,
    ) {
        let each = card_width(area.width());
        let art_height = each * 0.75;
        for (at, item) in group.iter().take(SHELF_CARDS).enumerate() {
            let left = area.left() + (each + SHELF_GAP) * at as f32;
            let art = Rect::from_min_size(Pos2::new(left, y), Vec2::new(each, art_height));
            painter.rect_filled(art, 16.0, ink(TILE_FILL));
            match self.pictures.get(ui.ctx(), store, &item.preview) {
                Some(picture) => {
                    painter.image(picture.id(), art, fill_uv(picture.size_vec2(), art.size()), Color32::WHITE);
                }
                None => look.icon_on(ui, painter, kind_icon(item.kind), art.center(), 36.0, ink(IDLE)),
            }
            text(
                painter,
                Pos2::new(left, art.bottom() + 10.0),
                Align2::LEFT_TOP,
                &item.name,
                15.0,
                Weight::SemiBold,
                ink(TITLE),
            );
            text(
                painter,
                Pos2::new(left, art.bottom() + 32.0),
                Align2::LEFT_TOP,
                &second_line(item),
                13.0,
                Weight::Regular,
                ink(ROW_INK),
            );
            state_pill(
                painter,
                look,
                ui,
                installed(store, item),
                needs_update(store, item),
                left + each,
                art.bottom() + 66.0,
                ink,
            );
            if ui.interact(art, ui.scope_id().with(("shelf art", item.id.clone())), Sense::click()).clicked() {
                self.go(View::Item { source: item.source.clone(), id: item.id.clone() });
            }
        }
    }

    /// One row: its kind's tile, the name, one line under it, and the state at the trailing end.
    fn row(
        &mut self,
        painter: &egui::Painter,
        look: &mut Look,
        ui: &egui::Ui,
        store: &Arc<procraft_core::Store>,
        item: &procraft_core::StoreItem,
        row: Rect,
        at: usize,
        ink: impl Fn(Themed) -> Color32 + Copy,
    ) {
        let tile = Rect::from_min_size(Pos2::new(row.left(), row.center().y - TILE / 2.0), Vec2::splat(TILE));
        painter.rect_filled(tile, 10.0, ink(TILE_FILL));
        match self.pictures.get(ui.ctx(), store, &item.preview) {
            Some(picture) => {
                painter.image(picture.id(), tile, fill_uv(picture.size_vec2(), tile.size()), Color32::WHITE);
            }
            None => look.icon_on(ui, painter, kind_icon(item.kind), tile.center(), 24.0, ink(IDLE)),
        }
        let name = Pos2::new(tile.right() + 12.0, row.center().y - 11.0);
        text(painter, name, Align2::LEFT_CENTER, &item.name, 17.0, Weight::Regular, ink(TITLE));
        text(
            painter,
            Pos2::new(name.x, row.center().y + 11.0),
            Align2::LEFT_CENTER,
            &second_line(item),
            15.0,
            Weight::Regular,
            ink(ROW_INK),
        );
        state_pill(
            painter,
            look,
            ui,
            installed(store, item),
            needs_update(store, item),
            row.right(),
            row.center().y,
            ink,
        );
        // every row but a group's first carries the rule above it
        if at > 0 {
            painter.line_segment(
                [
                    Pos2::new(row.left() + GROUP_RULE_INSET, row.top()),
                    Pos2::new(row.right() - GROUP_RULE_INSET, row.top()),
                ],
                Stroke::new(1.0, ink(SEP)),
            );
        }
        if ui.interact(row, ui.scope_id().with(("store row", item.id.clone())), Sense::click()).clicked() {
            self.go(View::Item { source: item.source.clone(), id: item.id.clone() });
        }
    }

    /// One item's own page: the head, its facts, its words, the details and the maker's other work.
    fn item_page(
        &mut self,
        ui: &egui::Ui,
        look: &mut Look,
        store: &Arc<procraft_core::Store>,
        body: Rect,
        ink: impl Fn(Themed) -> Color32 + Copy,
        item: &procraft_core::StoreItem,
    ) {
        let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
        let mut y = body.top() + 24.0 - self.scroll;
        // the head: the icon, the name with its kind, the publisher, and the one action
        let icon = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::splat(ICON));
        painter.rect_filled(icon, 27.0, ink(TILE_FILL));
        look.icon_on(ui, &painter, kind_icon(item.kind), icon.center(), 56.0, ink(IDLE));
        let mut x = icon.right() + 20.0;
        let name_y = icon.top() + 34.0;
        text(&painter, Pos2::new(x, name_y), Align2::LEFT_CENTER, &item.name, 30.0, Weight::Bold, ink(TITLE));
        x += text_width(&painter, &item.name, 30.0, Weight::Bold) + 10.0;
        let kind = item.kind.text().to_uppercase();
        let chip = Rect::from_min_size(
            Pos2::new(x, name_y - 10.0),
            Vec2::new(text_width(&painter, &kind, 11.0, Weight::SemiBold) + 14.0, 20.0),
        );
        painter.rect_filled(chip, 6.0, ink(TILE_FILL));
        text(&painter, chip.center(), Align2::CENTER_CENTER, &kind, 11.0, Weight::SemiBold, ink(ROW_INK));
        text(
            &painter,
            Pos2::new(icon.right() + 20.0, name_y + 30.0),
            Align2::LEFT_CENTER,
            &item.publisher,
            15.0,
            Weight::Regular,
            ink(ACCENT),
        );
        // the action, at the trailing end: what came with the app is already in, what the
        // catalogue has changed since is an update, and what is on its way says so
        let is_in = installed(store, item);
        let update = needs_update(store, item);
        let fetching = self.installing_now(&item.id);
        // the last try at this item left a problem: the button says so, and keeps the reason until
        // it is asked for
        let problem = self.problem.as_ref().filter(|(id, _)| *id == item.id).map(|(_, why)| why.clone());
        let label = if fetching {
            "Installing"
        } else if problem.is_some() {
            "Not installed"
        } else if update {
            "Update"
        } else if is_in {
            "Installed"
        } else {
            "Install"
        };
        let width = text_width(&painter, label, 15.0, Weight::SemiBold) + 36.0;
        let button =
            Rect::from_center_size(Pos2::new(body.right() - width / 2.0, name_y + 4.0), Vec2::new(width, 30.0));
        let filled = problem.is_none() && (!is_in || update || fetching);
        painter.rect_filled(button, 15.0, ink(if filled { ACCENT } else { PILL_STATE_FILL }));
        text(
            &painter,
            button.center(),
            Align2::CENTER_CENTER,
            label,
            15.0,
            Weight::SemiBold,
            ink(if filled { ACCENT_ON } else { ROW_INK }),
        );
        // taking something from a catalogue: its file is fetched, and when no fetch of it works
        // the alert offers to take the file from this machine instead
        if (problem.is_some() || (!is_in || update))
            && !fetching
            && ui.interact(button, ui.scope_id().with("store install"), Sense::click()).clicked()
        {
            match problem {
                // the reason only when it is asked for, in the sheet, never in a window of the system's
                Some(why) => {
                    self.problem = None;
                    self.alert.tell("Not installed", &why, None);
                }
                None => self.start_install(store, item.clone()),
            }
        }
        y = icon.bottom() + 16.0;
        // what taking it means, in words, once per kind
        let caveat = caveat(item.kind);
        let galley = wrapped(&painter, caveat, 15.0, Weight::Regular, ink(ROW_INK), PROSE - 24.0);
        let info = Pos2::new(body.left() + 8.0, y + 10.0);
        painter.circle_stroke(info, 7.0, Stroke::new(1.4, ink(IDLE)));
        painter.line_segment(
            [Pos2::new(info.x, info.y - 3.0), Pos2::new(info.x, info.y + 4.0)],
            Stroke::new(1.4, ink(IDLE)),
        );
        painter.circle_filled(Pos2::new(info.x, info.y - 5.5), 0.9, ink(IDLE));
        let height = galley.size().y;
        painter.galley(Pos2::new(body.left() + 24.0, y), galley, ink(ROW_INK));
        y += height + 20.0;
        // the first picture the catalogue gave, in the store's own frame
        if let Some(picture) = item.previews.first().and_then(|address| self.pictures.get(ui.ctx(), store, address)) {
            let art = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(PROSE, PROSE * 377.0 / 520.0));
            painter.image(picture.id(), art, fill_uv(picture.size_vec2(), art.size()), Color32::WHITE);
            y = art.bottom() + 20.0;
        }
        // the facts: a column each, ruled above and below, at the width of the page
        let facts = facts_of(item);
        if !facts.is_empty() {
            let strip = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), 44.0 + 24.0));
            painter.line_segment([strip.left_top(), strip.right_top()], Stroke::new(1.0, ink(SEP)));
            painter.line_segment([strip.left_bottom(), strip.right_bottom()], Stroke::new(1.0, ink(SEP)));
            let column = strip.width() / facts.len() as f32;
            for (at, (label, value)) in facts.iter().enumerate() {
                let left = strip.left() + column * at as f32;
                if at > 0 {
                    painter.line_segment(
                        [Pos2::new(left, strip.top() + 12.0), Pos2::new(left, strip.bottom() - 12.0)],
                        Stroke::new(1.0, ink(SEP)),
                    );
                }
                let at_x = left + if at == 0 { 16.0 } else { 10.0 };
                // a header's rhythm here too: 16 over the label, 8 between it and its value
                text(
                    &painter,
                    Pos2::new(at_x, strip.top() + GROUP_HEAD),
                    Align2::LEFT_TOP,
                    label,
                    11.0,
                    Weight::SemiBold,
                    ink(IDLE),
                );
                text(
                    &painter,
                    Pos2::new(at_x, strip.top() + GROUP_HEAD + GROUP_LINE + GROUP_HEAD_BELOW),
                    Align2::LEFT_TOP,
                    value,
                    16.0,
                    Weight::SemiBold,
                    ink(TITLE),
                );
            }
            y = strip.bottom() + 24.0;
        }
        // the words, then the details, then the maker's other work
        let words = if item.body.is_empty() { item.description.as_str() } else { item.body.as_str() };
        if !words.is_empty() {
            let galley = wrapped(&painter, words, 16.0, Weight::Regular, ink(ROW_INK), PROSE);
            y += galley.size().y;
            painter.galley(Pos2::new(body.left(), y - galley.size().y), galley, ink(ROW_INK));
            y += 40.0;
        }
        text(
            &painter,
            Pos2::new(body.left() + 16.0, y),
            Align2::LEFT_TOP,
            "Details",
            SECTION,
            Weight::Bold,
            ink(TITLE),
        );
        y += 30.0 + HEADING_BELOW;
        let rows: Vec<(String, String)> =
            vec![("Category".to_owned(), item.category.clone()), ("Published by".to_owned(), item.publisher.clone())];
        // the tags keep a row of their own, chips at the trailing end: three at most, then a count
        let tags_height = if item.tags.is_empty() { 0.0 } else { GROUP_ROW };
        let card = Rect::from_min_size(
            Pos2::new(body.left(), y),
            Vec2::new(body.width(), rows.len() as f32 * GROUP_ROW + tags_height),
        );
        painter.rect_filled(card, 10.0, ink(GROUP_FILL));
        for (at, (label, value)) in rows.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(card.left(), card.top() + at as f32 * GROUP_ROW),
                Vec2::new(card.width(), GROUP_ROW),
            );
            if at > 0 {
                painter.line_segment(
                    [
                        Pos2::new(row.left() + GROUP_RULE_INSET, row.top()),
                        Pos2::new(row.right() - GROUP_RULE_INSET, row.top()),
                    ],
                    Stroke::new(1.0, ink(SEP)),
                );
            }
            text(
                &painter,
                Pos2::new(row.left() + 16.0, row.center().y),
                Align2::LEFT_CENTER,
                label,
                17.0,
                Weight::Regular,
                ink(TITLE),
            );
            text(
                &painter,
                Pos2::new(row.right() - 16.0, row.center().y),
                Align2::RIGHT_CENTER,
                value,
                17.0,
                Weight::Regular,
                ink(ROW_INK),
            );
        }
        if !item.tags.is_empty() {
            let row = Rect::from_min_size(
                Pos2::new(card.left(), card.top() + rows.len() as f32 * GROUP_ROW),
                Vec2::new(card.width(), GROUP_ROW),
            );
            painter.line_segment(
                [
                    Pos2::new(row.left() + GROUP_RULE_INSET, row.top()),
                    Pos2::new(row.right() - GROUP_RULE_INSET, row.top()),
                ],
                Stroke::new(1.0, ink(SEP)),
            );
            text(
                &painter,
                Pos2::new(row.left() + 16.0, row.center().y),
                Align2::LEFT_CENTER,
                "Tags",
                17.0,
                Weight::Regular,
                ink(TITLE),
            );
            let mut chips: Vec<String> = item.tags.iter().take(3).cloned().collect();
            let more = item.tags.len().saturating_sub(3);
            if more > 0 {
                chips.push(format!("+{more}"));
            }
            let widths: Vec<f32> =
                chips.iter().map(|chip| text_width(&painter, chip, 13.0, Weight::Medium) + 24.0).collect();
            let total: f32 = widths.iter().sum::<f32>() + 8.0 * chips.len().saturating_sub(1) as f32;
            let mut chip_x = row.right() - 16.0 - total;
            for (chip, width) in chips.iter().zip(widths.iter()) {
                let at =
                    Rect::from_center_size(Pos2::new(chip_x + width / 2.0, row.center().y), Vec2::new(*width, CHIP));
                painter.rect_filled(at, CHIP / 2.0, ink(CHIP_FILL));
                text(&painter, at.center(), Align2::CENTER_CENTER, chip, 13.0, Weight::Medium, ink(ROW_INK));
                chip_x += width + 8.0;
            }
        }
        y = card.bottom() + 40.0;
        // more from the same maker, three across
        let more: Vec<procraft_core::StoreItem> = store
            .items()
            .into_iter()
            .filter(|other| other.publisher == item.publisher && other.id != item.id)
            .take(3)
            .collect();
        if !more.is_empty() {
            text(
                &painter,
                Pos2::new(body.left() + 16.0, y),
                Align2::LEFT_TOP,
                &format!("More from {}", item.publisher),
                SECTION,
                Weight::Bold,
                ink(TITLE),
            );
            y += 30.0 + HEADING_BELOW;
            let gap = 16.0;
            let each = (body.width() - gap * 2.0) / 3.0;
            for (at, other) in more.iter().enumerate() {
                let left = body.left() + (each + gap) * at as f32;
                let art = Rect::from_min_size(Pos2::new(left, y), Vec2::new(each, each * 0.75));
                painter.rect_filled(art, 16.0, ink(TILE_FILL));
                match self.pictures.get(ui.ctx(), store, &other.preview) {
                    Some(picture) => {
                        painter.image(picture.id(), art, fill_uv(picture.size_vec2(), art.size()), Color32::WHITE);
                    }
                    None => look.icon_on(ui, &painter, kind_icon(other.kind), art.center(), 36.0, ink(IDLE)),
                }
                text(
                    &painter,
                    Pos2::new(left, art.bottom() + 12.0),
                    Align2::LEFT_TOP,
                    &other.name,
                    16.0,
                    Weight::SemiBold,
                    ink(TITLE),
                );
                text(
                    &painter,
                    Pos2::new(left, art.bottom() + 34.0),
                    Align2::LEFT_TOP,
                    &other.publisher,
                    14.0,
                    Weight::Regular,
                    ink(ROW_INK),
                );
                state_pill(
                    &painter,
                    look,
                    ui,
                    installed(store, other),
                    needs_update(store, other),
                    left + each,
                    art.bottom() + 34.0,
                    ink,
                );
                if ui.interact(art, ui.scope_id().with(("more art", other.id.clone())), Sense::click()).clicked() {
                    self.go(View::Item { source: other.source.clone(), id: other.id.clone() });
                }
            }
            y += each * 0.75 + 34.0 + 22.0;
        }
        self.height = (y + self.scroll) - body.top();
        self.scroll = self.scrolled(ui, body);
    }

    /// Settings: one scroll, its groups drawn as the board draws them - a small-caps heading, a card
    /// of rows, and the line that says what the group means.
    fn settings_page(
        &mut self,
        ui: &egui::Ui,
        look: &mut Look,
        store: &Arc<procraft_core::Store>,
        body: Rect,
        ink: impl Fn(Themed) -> Color32 + Copy,
    ) {
        let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
        let title = Rect::from_min_size(Pos2::new(body.left(), body.top() + 12.0), Vec2::new(body.width(), TITLE_LINE));
        text(
            &painter,
            Pos2::new(title.left(), title.center().y),
            Align2::LEFT_CENTER,
            "Settings",
            TITLE_SIZE,
            Weight::Bold,
            ink(TITLE),
        );
        let mut y = title.bottom() + 24.0 - self.scroll;
        // Sources: one row each, and the way to add one
        let sources = store.sources();
        y = group_head(&painter, body, y, "SOURCES", ink(ROW_INK));
        let rows = sources.len() + 1;
        let card = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), rows as f32 * GROUP_ROW));
        painter.rect_filled(card, 10.0, ink(GROUP_FILL));
        for (at, source) in sources.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(card.left(), card.top() + at as f32 * GROUP_ROW),
                Vec2::new(card.width(), GROUP_ROW),
            );
            if at > 0 {
                painter.line_segment(
                    [
                        Pos2::new(row.left() + GROUP_RULE_INSET, row.top()),
                        Pos2::new(row.right() - GROUP_RULE_INSET, row.top()),
                    ],
                    Stroke::new(1.0, ink(SEP)),
                );
            }
            let name = Pos2::new(row.left() + 16.0, row.center().y - 11.0);
            text(
                &painter,
                name,
                Align2::LEFT_CENTER,
                &source.name,
                17.0,
                Weight::Regular,
                ink(if source.on { TITLE } else { DISABLED }),
            );
            let line = source_line(source);
            text(
                &painter,
                Pos2::new(name.x, row.center().y + 11.0),
                Align2::LEFT_CENTER,
                &line,
                15.0,
                Weight::Regular,
                ink(ROW_INK),
            );
            // a disclosure indicator: the same chevron at the HIG's 13, its trailing edge 16 in
            // from the row's edge, its middle on the row's
            let chev = Pos2::new(row.right() - 16.0 - 6.5, row.center().y);
            look.icon_on(ui, &painter, &icons::CHEVRON_RIGHT, chev, 13.0, ink(IDLE));
            if ui.interact(row, ui.scope_id().with(("store source", source.id.clone())), Sense::click()).clicked() {
                self.go(View::Source { id: source.id.clone() });
            }
        }
        // the last row adds one
        let add = Rect::from_min_size(
            Pos2::new(card.left(), card.top() + sources.len() as f32 * GROUP_ROW),
            Vec2::new(card.width(), GROUP_ROW),
        );
        if !sources.is_empty() {
            painter.line_segment(
                [Pos2::new(add.left(), add.top()), Pos2::new(add.right(), add.top())],
                Stroke::new(1.0, ink(SEP)),
            );
        }
        text(
            &painter,
            Pos2::new(add.left() + 16.0, add.center().y),
            Align2::LEFT_CENTER,
            "Add Source...",
            17.0,
            Weight::Regular,
            ink(ACCENT),
        );
        if ui.interact(add, ui.scope_id().with("store add source"), Sense::click()).clicked() {
            self.adding = true;
            self.address.clear();
            self.refusal.clear();
            self.typing = Some(Field::Add);
        }
        y = card.bottom() + GROUP_HEAD;
        text(
            &painter,
            Pos2::new(body.left() + 16.0, y),
            Align2::LEFT_TOP,
            "The store reads these when it opens. Turning one off hides its items; nothing is uninstalled.",
            13.0,
            Weight::Regular,
            ink(ROW_INK),
        );
        y += GROUP_FOOT + 26.0;
        // Updates: when the sources were last read, which is the only checking there is so far
        let checked = store.sources().iter().map(|source| source.read).max().unwrap_or(0);
        y = group_head(&painter, body, y, "UPDATES", ink(ROW_INK));
        let updates = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), GROUP_ROW));
        painter.rect_filled(updates, 10.0, ink(GROUP_FILL));
        text(
            &painter,
            Pos2::new(updates.left() + 16.0, updates.center().y),
            Align2::LEFT_CENTER,
            "Last Checked",
            17.0,
            Weight::Regular,
            ink(TITLE),
        );
        let when = if checked == 0 { "Never".to_owned() } else { date_text(checked, true) };
        text(
            &painter,
            Pos2::new(updates.right() - 16.0, updates.center().y),
            Align2::RIGHT_CENTER,
            &when,
            17.0,
            Weight::Regular,
            ink(ROW_INK),
        );
        y = updates.bottom() + GROUP_HEAD;
        text(
            &painter,
            Pos2::new(body.left() + 16.0, y),
            Align2::LEFT_TOP,
            "The store reads its sources when it opens. Nothing is checked in the background.",
            13.0,
            Weight::Regular,
            ink(ROW_INK),
        );
        y += GROUP_FOOT + 26.0;
        // Storage: what came with the app, and what the store installed
        let items = store.items();
        let came = items.iter().filter(|item| item.file.is_empty()).count() + store.installed().len();
        y = group_head(&painter, body, y, "STORAGE", ink(ROW_INK));
        let storage = Rect::from_min_size(Pos2::new(body.left(), y), Vec2::new(body.width(), GROUP_ROW * 2.0));
        painter.rect_filled(storage, 10.0, ink(GROUP_FILL));
        for (at, (label, value)) in
            [("Installed Items", format!("{came} items")), ("Downloads", "0 in progress".to_owned())].iter().enumerate()
        {
            let row = Rect::from_min_size(
                Pos2::new(storage.left(), storage.top() + at as f32 * GROUP_ROW),
                Vec2::new(storage.width(), GROUP_ROW),
            );
            if at > 0 {
                painter.line_segment(
                    [
                        Pos2::new(row.left() + GROUP_RULE_INSET, row.top()),
                        Pos2::new(row.right() - GROUP_RULE_INSET, row.top()),
                    ],
                    Stroke::new(1.0, ink(SEP)),
                );
            }
            text(
                &painter,
                Pos2::new(row.left() + 16.0, row.center().y),
                Align2::LEFT_CENTER,
                label,
                17.0,
                Weight::Regular,
                ink(TITLE),
            );
            text(
                &painter,
                Pos2::new(row.right() - 16.0, row.center().y),
                Align2::RIGHT_CENTER,
                value,
                17.0,
                Weight::Regular,
                ink(ROW_INK),
            );
        }
        y = storage.bottom() + GROUP_HEAD;
        text(
            &painter,
            Pos2::new(body.left() + 16.0, y),
            Align2::LEFT_TOP,
            "Downloads stop when the store closes. Installed items live in your project.",
            13.0,
            Weight::Regular,
            ink(ROW_INK),
        );
        y += GROUP_FOOT;
        self.height = (y + self.scroll) - body.top();
        self.scroll = self.scrolled(ui, body);
    }

    /// One source's page: its switch, where it came from, when it was last read, and what taking it
    /// away would mean. The one that ships with the app only has the switch.
    fn source_page(
        &mut self,
        ui: &egui::Ui,
        store: &Arc<procraft_core::Store>,
        body: Rect,
        ink: impl Fn(Themed) -> Color32 + Copy,
        source: procraft_core::SourceInfo,
    ) {
        let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
        let mut y = body.top() + 24.0 - self.scroll;
        let rows: Vec<(String, String)> = vec![
            (
                "Address".to_owned(),
                if source.address.is_empty() { "Bundled with ProCraft".to_owned() } else { source.address.clone() },
            ),
            (
                "Last Refreshed".to_owned(),
                if source.read == 0 { "Never".to_owned() } else { date_text(source.read, true) },
            ),
        ];
        let card = Rect::from_min_size(
            Pos2::new(body.left(), y),
            Vec2::new(body.width(), (rows.len() + 1) as f32 * GROUP_ROW),
        );
        painter.rect_filled(card, 10.0, ink(GROUP_FILL));
        // Enabled, with the switch at the trailing end
        let enabled = Rect::from_min_size(Pos2::new(card.left(), card.top()), Vec2::new(card.width(), GROUP_ROW));
        text(
            &painter,
            Pos2::new(enabled.left() + 16.0, enabled.center().y),
            Align2::LEFT_CENTER,
            "Enabled",
            17.0,
            Weight::Regular,
            ink(TITLE),
        );
        let track =
            Rect::from_center_size(Pos2::new(enabled.right() - 16.0 - 25.5, enabled.center().y), Vec2::new(51.0, 31.0));
        painter.rect_filled(track, 16.0, ink(if source.on { ACCENT } else { SWITCH_OFF }));
        let knob = if source.on { track.right() - 2.0 - 13.5 } else { track.left() + 2.0 + 13.5 };
        painter.circle_filled(Pos2::new(knob, track.center().y), 13.5, Color32::WHITE);
        if ui.interact(enabled, ui.scope_id().with("store source switch"), Sense::click()).clicked() {
            store.set_on(source.id.clone(), !source.on);
        }
        for (at, (label, value)) in rows.iter().enumerate() {
            let row = Rect::from_min_size(
                Pos2::new(card.left(), card.top() + (at + 1) as f32 * GROUP_ROW),
                Vec2::new(card.width(), GROUP_ROW),
            );
            painter.line_segment(
                [
                    Pos2::new(row.left() + GROUP_RULE_INSET, row.top()),
                    Pos2::new(row.right() - GROUP_RULE_INSET, row.top()),
                ],
                Stroke::new(1.0, ink(SEP)),
            );
            text(
                &painter,
                Pos2::new(row.left() + 16.0, row.center().y),
                Align2::LEFT_CENTER,
                label,
                17.0,
                Weight::Regular,
                ink(TITLE),
            );
            text(
                &painter,
                Pos2::new(row.right() - 16.0, row.center().y),
                Align2::RIGHT_CENTER,
                value,
                17.0,
                Weight::Regular,
                ink(ROW_INK),
            );
        }
        y = card.bottom() + GROUP_HEAD;
        text(
            &painter,
            Pos2::new(body.left() + 16.0, y),
            Align2::LEFT_TOP,
            "Off hides this source's items from the store. Nothing is uninstalled.",
            13.0,
            Weight::Regular,
            ink(ROW_INK),
        );
        y += GROUP_FOOT + 26.0;
        // what can be done with it, one row each: refreshing a catalogue on this machine, and
        // taking it away. A web address has no Refresh Now: this build cannot read one.
        let shipped = source.id == "procraft";
        let local = !source.address.is_empty() && !source.address.starts_with("http");
        let mut actions: Vec<(&str, Themed)> = Vec::new();
        if !shipped {
            if local {
                actions.push(("Refresh Now", ACCENT));
            }
            actions.push(("Remove This Source", DANGER));
        }
        if !actions.is_empty() {
            let card = Rect::from_min_size(
                Pos2::new(body.left(), y),
                Vec2::new(body.width(), actions.len() as f32 * GROUP_ROW),
            );
            painter.rect_filled(card, 10.0, ink(GROUP_FILL));
            for (at, (label, colour)) in actions.iter().enumerate() {
                let row = Rect::from_min_size(
                    Pos2::new(card.left(), card.top() + at as f32 * GROUP_ROW),
                    Vec2::new(card.width(), GROUP_ROW),
                );
                if at > 0 {
                    painter.line_segment(
                        [
                            Pos2::new(row.left() + GROUP_RULE_INSET, row.top()),
                            Pos2::new(row.right() - GROUP_RULE_INSET, row.top()),
                        ],
                        Stroke::new(1.0, ink(SEP)),
                    );
                }
                text(
                    &painter,
                    Pos2::new(row.left() + 16.0, row.center().y),
                    Align2::LEFT_CENTER,
                    label,
                    17.0,
                    Weight::Regular,
                    ink(*colour),
                );
                if ui.interact(row, ui.scope_id().with(("store source do", *label)), Sense::click()).clicked() {
                    if *label == "Refresh Now" {
                        self.start_refresh(store, source.id.clone());
                    } else {
                        self.removing = Some(source.id.clone());
                        self.alert.open(
                            "Remove this source?",
                            &format!(
                                "\"{}\" is forgotten. Its items leave the store, and things installed from it stay installed.",
                                source.name
                            ),
                            "Remove",
                            Some(&icons::CROSS),
                        );
                    }
                }
            }
            y = card.bottom() + GROUP_HEAD;
            text(
                &painter,
                Pos2::new(body.left() + 16.0, y),
                Align2::LEFT_TOP,
                "Items you installed from this source stay installed, and their cards keep saying where they came from.",
                13.0,
                Weight::Regular,
                ink(ROW_INK),
            );
            y += GROUP_FOOT;
        }
        self.height = (y + self.scroll) - body.top();
        self.scroll = self.scrolled(ui, body);
    }

    /// Adding a source: one field on a sheet over Settings, as the board draws it - Cancel and Add in
    /// its bar, the address under its own heading, and the line saying why it was not taken.
    fn add_sheet(
        &mut self,
        ui: &egui::Ui,
        store: &Arc<procraft_core::Store>,
        sheet: Rect,
        ink: impl Fn(Themed) -> Color32 + Copy,
    ) {
        let painter = ui.painter();
        painter.rect_filled(sheet, CORNER, Color32::from_black_alpha((0.4 * 255.0) as u8));
        let size = Vec2::new(540.0, 300.0).min(sheet.size() - Vec2::splat(40.0));
        let panel = Rect::from_center_size(sheet.center(), size);
        painter.rect_filled(panel, 14.0, ink(BG));
        // its bar: Cancel, the title, and Add
        let bar = Rect::from_min_size(panel.min, Vec2::new(panel.width(), BAR));
        painter.line_segment([bar.left_bottom(), bar.right_bottom()], Stroke::new(1.0, ink(SEP)));
        text(
            painter,
            Pos2::new(bar.left() + 16.0, bar.center().y),
            Align2::LEFT_CENTER,
            "Cancel",
            17.0,
            Weight::Regular,
            ink(ACCENT),
        );
        text(
            painter,
            Pos2::new(bar.center().x, bar.center().y),
            Align2::CENTER_CENTER,
            "Add Source",
            17.0,
            Weight::SemiBold,
            ink(TITLE),
        );
        let ready = !self.address.trim().is_empty();
        text(
            painter,
            Pos2::new(bar.right() - 16.0, bar.center().y),
            Align2::RIGHT_CENTER,
            "Add",
            17.0,
            Weight::SemiBold,
            ink(if ready { ACCENT } else { DISABLED }),
        );
        for (at, rect) in [
            Rect::from_min_max(Pos2::new(bar.left(), bar.top()), Pos2::new(bar.left() + 100.0, bar.bottom())),
            Rect::from_min_max(Pos2::new(bar.right() - 100.0, bar.top()), Pos2::new(bar.right(), bar.bottom())),
        ]
        .iter()
        .enumerate()
        {
            if ui.interact(*rect, ui.scope_id().with(("add bar", at)), Sense::click()).clicked() {
                if at == 0 {
                    self.adding = false;
                    self.typing = None;
                } else if ready && !self.adding_now() {
                    self.start_add(store);
                }
            }
        }
        // the field, under its own heading: a grouped list's 16 over the header, 8 under it, both
        // starting where the field's own text does
        let at = Pos2::new(panel.left() + 32.0, bar.bottom() + GROUP_HEAD);
        text(painter, at, Align2::LEFT_TOP, "ADDRESS", 13.0, Weight::Regular, ink(ROW_INK));
        let field = Rect::from_min_size(
            Pos2::new(panel.left() + 16.0, at.y + GROUP_LINE + GROUP_HEAD_BELOW),
            Vec2::new(panel.width() - 32.0, 56.0),
        );
        painter.rect_filled(field, 10.0, ink(GROUP_FILL));
        let text_at = Pos2::new(field.left() + 16.0, field.center().y);
        if self.address.is_empty() {
            text(painter, text_at, Align2::LEFT_CENTER, "https://", 17.0, Weight::Regular, ink(HINT));
        } else {
            text(painter, text_at, Align2::LEFT_CENTER, &self.address, 17.0, Weight::Regular, ink(TITLE));
            let width = text_width(painter, &self.address, 17.0, Weight::Regular).min(field.width() - 32.0);
            painter.rect_filled(
                Rect::from_min_size(Pos2::new(text_at.x + width + 2.0, field.center().y - 10.0), Vec2::new(2.0, 20.0)),
                1.0,
                ink(ACCENT),
            );
        }
        if ui.interact(field, ui.scope_id().with("add field"), Sense::click()).clicked() {
            self.typing = Some(Field::Add);
        }
        self.read_keys(ui.ctx());
        if self.typing == Some(Field::Add) && ui.input(|i| i.key_pressed(egui::Key::Enter)) && ready {
            self.start_add(store);
        }
        // what it says under the field: why it was not taken, or what to paste
        let line = if self.refusal.is_empty() {
            "A catalogue is a JSON file. Its address can be a web address, or the path to a file on this machine."
                .to_owned()
        } else {
            self.refusal.clone()
        };
        let colour = if self.refusal.is_empty() { ROW_INK } else { DANGER };
        let galley = wrapped(painter, &line, 13.0, Weight::Regular, ink(colour), field.width() - 32.0);
        painter.galley(Pos2::new(field.left() + 16.0, field.bottom() + 12.0), galley, ink(colour));
    }

    /// One kind's whole list: a wall of its cards three across, the sort in the bar.
    fn see_all_page(
        &mut self,
        ui: &egui::Ui,
        look: &mut Look,
        store: &Arc<procraft_core::Store>,
        body: Rect,
        ink: impl Fn(Themed) -> Color32 + Copy,
    ) {
        let View::SeeAll(kind) = &self.view else { return };
        let kind = *kind;
        let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
        let title = Rect::from_min_size(Pos2::new(body.left(), body.top() + 12.0), Vec2::new(body.width(), TITLE_LINE));
        text(
            &painter,
            Pos2::new(title.left(), title.center().y),
            Align2::LEFT_CENTER,
            section_title(kind),
            TITLE_SIZE,
            Weight::Bold,
            ink(TITLE),
        );
        // the same cuts the list has, applied to one kind, then the order the bar asked for
        let mut items: Vec<procraft_core::StoreItem> = store.items().into_iter().filter(|i| i.kind == kind).collect();
        if let Some(from) = &self.from {
            items.retain(|item| &item.source == from);
        }
        let query = self.query.trim().to_lowercase();
        if !query.is_empty() {
            items.retain(|item| {
                item.name.to_lowercase().contains(&query)
                    || item.publisher.to_lowercase().contains(&query)
                    || item.tags.iter().any(|tag| tag.to_lowercase().contains(&query))
            });
        }
        self.sorted(&mut items);
        if items.is_empty() {
            text(
                &painter,
                Pos2::new(body.center().x, title.bottom() + 48.0),
                Align2::CENTER_TOP,
                "Nothing here yet.",
                15.0,
                Weight::Medium,
                ink(ROW_INK),
            );
            self.height = 0.0;
            return;
        }
        let gap = 20.0;
        let each = (body.width() - gap * 2.0) / 3.0;
        let card = each * 0.75 + 66.0;
        let rows = items.len().div_ceil(3);
        self.height = (title.bottom() + 24.0 + rows as f32 * (card + 24.0)) - body.top();
        self.scroll = self.scrolled(ui, body);
        let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
        let mut y = title.bottom() + 24.0 - self.scroll;
        for (at, item) in items.iter().enumerate() {
            let column = at % 3;
            if column == 0 && at > 0 {
                y += card + 24.0;
            }
            let left = body.left() + (each + gap) * column as f32;
            let art = Rect::from_min_size(Pos2::new(left, y), Vec2::new(each, each * 0.75));
            painter.rect_filled(art, 16.0, ink(TILE_FILL));
            match self.pictures.get(ui.ctx(), store, &item.preview) {
                Some(picture) => {
                    painter.image(picture.id(), art, fill_uv(picture.size_vec2(), art.size()), Color32::WHITE);
                }
                None => look.icon_on(ui, &painter, kind_icon(item.kind), art.center(), 40.0, ink(IDLE)),
            }
            text(
                &painter,
                Pos2::new(left, art.bottom() + 12.0),
                Align2::LEFT_TOP,
                &item.name,
                16.0,
                Weight::SemiBold,
                ink(TITLE),
            );
            text(
                &painter,
                Pos2::new(left, art.bottom() + 34.0),
                Align2::LEFT_TOP,
                &second_line(item),
                14.0,
                Weight::Regular,
                ink(ROW_INK),
            );
            state_pill(
                &painter,
                look,
                ui,
                installed(store, item),
                needs_update(store, item),
                left + each,
                art.bottom() + 43.0,
                ink,
            );
            if ui.interact(art, ui.scope_id().with(("wall art", item.id.clone())), Sense::click()).clicked() {
                self.go(View::Item { source: item.source.clone(), id: item.id.clone() });
            }
        }
    }

    /// The downloads page: what is arriving, what arrived, and what has a newer file waiting. Only
    /// the first is ever empty in this build: nothing transfers over a web address yet.
    fn downloads_page(
        &mut self,
        ui: &egui::Ui,
        look: &mut Look,
        store: &Arc<procraft_core::Store>,
        body: Rect,
        ink: impl Fn(Themed) -> Color32 + Copy,
    ) {
        let painter = ui.painter().with_clip_rect(body.intersect(ui.clip_rect()));
        let title = Rect::from_min_size(Pos2::new(body.left(), body.top() + 12.0), Vec2::new(body.width(), TITLE_LINE));
        text(
            &painter,
            Pos2::new(title.left(), title.center().y),
            Align2::LEFT_CENTER,
            "Downloads",
            TITLE_SIZE,
            Weight::Bold,
            ink(TITLE),
        );
        let records = store.installed();
        let items = store.items();
        let waiting: Vec<&procraft_core::Installed> = records
            .iter()
            .filter(|one| items.iter().any(|item| item.id == one.id && changes(&one.digest, item)))
            .collect();
        let mut content = (title.bottom() + 8.0) - body.top();
        content += GROUP_HEAD + 30.0;
        if !waiting.is_empty() {
            content += GROUP_HEAD + waiting.len() as f32 * ROW_HEIGHT + 24.0;
        }
        if !records.is_empty() {
            content += GROUP_HEAD + records.len() as f32 * ROW_HEIGHT + 24.0;
        }
        self.height = content;
        self.scroll = self.scrolled(ui, body);
        let mut y = title.bottom() + 8.0 - self.scroll;
        // Downloading: nothing, until the fetch exists
        y = group_head(&painter, body, y, "DOWNLOADING", ink(ROW_INK));
        text(
            &painter,
            Pos2::new(body.left() + 16.0, y),
            Align2::LEFT_TOP,
            "Nothing downloading.",
            15.0,
            Weight::Medium,
            ink(ROW_INK),
        );
        y += 30.0;
        // Waiting for You: what the catalogue has moved on from
        if !waiting.is_empty() {
            y = group_head(&painter, body, y, "WAITING FOR YOU", ink(ROW_INK));
            let card = Rect::from_min_size(
                Pos2::new(body.left(), y),
                Vec2::new(body.width(), waiting.len() as f32 * ROW_HEIGHT),
            );
            painter.rect_filled(card, 10.0, ink(GROUP_FILL));
            for (at, one) in waiting.iter().enumerate() {
                self.record_row(&painter, look, ui, store, one, card, at, true, ink);
            }
            y = card.bottom() + 24.0;
        }
        // Finished: what the store took, newest first
        if !records.is_empty() {
            y = group_head(&painter, body, y, "FINISHED", ink(ROW_INK));
            let card = Rect::from_min_size(
                Pos2::new(body.left(), y),
                Vec2::new(body.width(), records.len() as f32 * ROW_HEIGHT),
            );
            painter.rect_filled(card, 10.0, ink(GROUP_FILL));
            for (at, one) in records.iter().enumerate() {
                self.record_row(&painter, look, ui, store, one, card, at, false, ink);
            }
        }
    }

    /// One row of that page: the kind's tile, the name over its version and when it came, and what
    /// state it is in. A row whose item is still listed opens it.
    fn record_row(
        &mut self,
        painter: &egui::Painter,
        look: &mut Look,
        ui: &egui::Ui,
        store: &Arc<procraft_core::Store>,
        one: &procraft_core::Installed,
        card: Rect,
        at: usize,
        update: bool,
        ink: impl Fn(Themed) -> Color32 + Copy,
    ) {
        let row = Rect::from_min_size(
            Pos2::new(card.left(), card.top() + at as f32 * ROW_HEIGHT),
            Vec2::new(card.width(), ROW_HEIGHT),
        );
        if at > 0 {
            painter.line_segment(
                [
                    Pos2::new(row.left() + GROUP_RULE_INSET, row.top()),
                    Pos2::new(row.right() - GROUP_RULE_INSET, row.top()),
                ],
                Stroke::new(1.0, ink(SEP)),
            );
        }
        let tile = Rect::from_min_size(Pos2::new(row.left() + 16.0, row.center().y - 17.0), Vec2::splat(34.0));
        painter.rect_filled(tile, 8.0, ink(TILE_FILL));
        look.icon_on(ui, painter, kind_icon(one.kind), tile.center(), 20.0, ink(IDLE));
        let name = Pos2::new(tile.right() + 12.0, row.center().y - 10.0);
        text(painter, name, Align2::LEFT_CENTER, &one.name, 17.0, Weight::Regular, ink(TITLE));
        let line = format!("Version {} - {}", one.version, date_text(one.at, false));
        text(
            painter,
            Pos2::new(name.x, row.center().y + 10.0),
            Align2::LEFT_CENTER,
            &line,
            15.0,
            Weight::Regular,
            ink(ROW_INK),
        );
        state_pill(painter, look, ui, true, update, row.right() - 16.0, row.center().y, ink);
        if ui.interact(row, ui.scope_id().with(("download row", one.id.clone())), Sense::click()).clicked()
            && let Some(item) = store.items().into_iter().find(|item| item.id == one.id)
        {
            self.go(View::Item { source: item.source.clone(), id: item.id.clone() });
        }
    }

    /// A choice menu, hanging from the control that opened it: the filters the list is cut by, or
    /// what the wall sorts by, one radio each.
    fn menu_panel(
        &mut self,
        ui: &egui::Ui,
        store: &Arc<procraft_core::Store>,
        ink: impl Fn(Themed) -> Color32 + Copy,
        menu: MenuKind,
        sheet: Rect,
    ) {
        let mut choices: Vec<Choice> = Vec::new();
        match menu {
            MenuKind::Filters => {
                choices.push(Choice::Head("Kind"));
                choices.push(Choice::Pick {
                    label: "All kinds".to_owned(),
                    on: self.kind.is_none(),
                    set: Set::Kind(None),
                });
                for kind in [
                    procraft_core::ItemKind::Plugin,
                    procraft_core::ItemKind::BrushSet,
                    procraft_core::ItemKind::AssetPack,
                ] {
                    choices.push(Choice::Pick {
                        label: filter_label(kind).to_owned(),
                        on: self.kind == Some(kind),
                        set: Set::Kind(Some(kind)),
                    });
                }
                choices.push(Choice::Head("Source"));
                choices.push(Choice::Pick {
                    label: "All sources".to_owned(),
                    on: self.from.is_none(),
                    set: Set::Source(None),
                });
                for source in store.sources() {
                    let on = self.from.as_deref() == Some(source.id.as_str());
                    choices.push(Choice::Pick { label: source.name, on, set: Set::Source(Some(source.id)) });
                }
            }
            MenuKind::Sort => {
                choices.push(Choice::Head("Sort by"));
                for (label, by) in [("Title", SortBy::Title), ("Author", SortBy::Author), ("Size", SortBy::Size)] {
                    choices.push(Choice::Pick { label: label.to_owned(), on: self.sort == by, set: Set::Sort(by) });
                }
                choices.push(Choice::Head("Order"));
                choices.push(Choice::Pick { label: "Ascending".to_owned(), on: self.ascending, set: Set::Order(true) });
                choices.push(Choice::Pick {
                    label: "Descending".to_owned(),
                    on: !self.ascending,
                    set: Set::Order(false),
                });
            }
        }
        let anchor = match menu {
            MenuKind::Filters => self.filters_at,
            MenuKind::Sort => self.sort_at,
        };
        let Some(anchor) = anchor else { return };
        let width = 262.0;
        let height: f32 = choices
            .iter()
            .map(|choice| match choice {
                Choice::Head(_) => 30.0,
                Choice::Pick { .. } => 44.0,
            })
            .sum::<f32>()
            + 12.0;
        let left = (anchor.right() - width).max(sheet.left() + 8.0);
        let below = anchor.bottom() + 6.0;
        let top = if below + height > sheet.bottom() - 8.0 {
            (anchor.top() - height - 6.0).max(sheet.top() + 8.0)
        } else {
            below
        };
        let panel = Rect::from_min_size(Pos2::new(left, top), Vec2::new(width, height));
        ui.painter().add(
            egui::epaint::Shadow { offset: [0, 0], blur: 40, spread: 0, color: Color32::from_black_alpha(120) }
                .as_shape(panel, 22.0),
        );
        ui.painter().rect_filled(panel, 22.0, ink(BG));
        ui.interact(panel, ui.scope_id().with("store menu"), Sense::click_and_drag());
        let mut y = panel.top() + 6.0;
        for (at, choice) in choices.iter().enumerate() {
            match choice {
                Choice::Head(title) => {
                    text(
                        ui.painter(),
                        Pos2::new(panel.left() + 18.0, y + 8.0),
                        Align2::LEFT_TOP,
                        title,
                        13.0,
                        Weight::Regular,
                        ink(IDLE),
                    );
                    y += 30.0;
                }
                Choice::Pick { label, on, set } => {
                    let row = Rect::from_min_size(Pos2::new(panel.left(), y), Vec2::new(width, 44.0));
                    text(
                        ui.painter(),
                        Pos2::new(row.left() + 18.0, row.center().y),
                        Align2::LEFT_CENTER,
                        label,
                        16.0,
                        Weight::Regular,
                        ink(if *on { TITLE } else { ROW_INK }),
                    );
                    let radio = Pos2::new(row.right() - 18.0 - 9.0, row.center().y);
                    ui.painter().circle_stroke(radio, 8.5, Stroke::new(1.5, ink(if *on { ACCENT } else { IDLE })));
                    if *on {
                        ui.painter().circle_filled(radio, 4.5, ink(ACCENT));
                    }
                    if ui.interact(row, ui.scope_id().with(("menu row", at)), Sense::click()).clicked() {
                        match set {
                            Set::Kind(kind) => self.kind = *kind,
                            Set::Source(from) => self.from = from.clone(),
                            Set::Sort(by) => self.sort = *by,
                            Set::Order(up) => self.ascending = *up,
                        }
                        self.menu = None;
                        self.scroll = 0.0;
                    }
                    y += 44.0;
                }
            }
        }
    }

    /// The items in the order the wall asks for.
    fn sorted(&self, items: &mut [procraft_core::StoreItem]) {
        items.sort_by(|a, b| match self.sort {
            SortBy::Title => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            SortBy::Author => a
                .publisher
                .to_lowercase()
                .cmp(&b.publisher.to_lowercase())
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())),
            SortBy::Size => a.bytes.cmp(&b.bytes),
        });
        if !self.ascending {
            items.reverse();
        }
    }

    /// A wheel over the page moves it, kept inside what there is to show.
    fn scrolled(&mut self, ui: &egui::Ui, area: Rect) -> f32 {
        let most = (self.height - area.height()).max(0.0);
        let over = ui.input(|i| i.pointer.latest_pos()).is_some_and(|p| area.contains(p));
        let delta = if over { ui.input(|i| i.smooth_scroll_delta.y) } else { 0.0 };
        (self.scroll - delta).clamp(0.0, most)
    }

    /// What a field takes while it has the keys.
    fn read_keys(&mut self, ctx: &egui::Context) {
        let Some(field) = self.typing else { return };
        let events = ctx.input(|i| i.events.clone());
        let mut into = match field {
            Field::Search => std::mem::take(&mut self.query),
            Field::Add => std::mem::take(&mut self.address),
        };
        for event in events {
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
            Field::Search => self.query = into,
            Field::Add => self.address = into,
        }
    }
}

/// Whether an item is in the app already: the ones that came with it, and anything the store took.
fn installed(store: &procraft_core::Store, item: &procraft_core::StoreItem) -> bool {
    item.file.is_empty() || store.installed_one(item.id.clone()).is_some()
}

/// Whether the catalogue now lists other bytes than the ones that were installed.
fn changes(digest: &str, item: &procraft_core::StoreItem) -> bool {
    !item.sha256.is_empty() && digest != item.sha256
}

/// Whether an installed item has a newer file waiting for it.
fn needs_update(store: &procraft_core::Store, item: &procraft_core::StoreItem) -> bool {
    store.installed_one(item.id.clone()).is_some_and(|one| changes(&one.digest, item))
}

/// The state one item is in, at the trailing end of a line: the app's own are already in, and
/// anything a catalogue lists waits to be fetched.
fn state_pill(
    painter: &egui::Painter,
    look: &mut Look,
    ui: &egui::Ui,
    installed: bool,
    update: bool,
    right: f32,
    middle: f32,
    ink: impl Fn(Themed) -> Color32,
) {
    let label = if update {
        "Update"
    } else if installed {
        "Installed"
    } else {
        "Get"
    };
    let tick = installed && !update;
    let width = text_width(painter, label, 15.0, Weight::SemiBold) + 28.0 + if tick { 21.0 } else { 0.0 };
    let pill = Rect::from_center_size(Pos2::new(right - width / 2.0, middle), Vec2::new(width, PILL_STATE));
    painter.rect_filled(pill, PILL_STATE / 2.0, ink(if update { ACCENT } else { PILL_STATE_FILL }));
    let mut x = pill.left() + 14.0;
    if tick {
        look.icon_on(ui, painter, &icons::TICK, Pos2::new(x + 7.0, pill.center().y), 15.0, ink(ROW_INK));
        x += 21.0;
    }
    let colour = if update {
        ACCENT_ON
    } else if installed {
        ROW_INK
    } else {
        ACCENT
    };
    text(painter, Pos2::new(x, pill.center().y), Align2::LEFT_CENTER, label, 15.0, Weight::SemiBold, ink(colour));
}

/// One line of a choice menu: a head, or a row that is one choice.
#[derive(Clone)]
enum Choice {
    Head(&'static str),
    Pick { label: String, on: bool, set: Set },
}

/// What picking a row sets.
#[derive(Clone)]
enum Set {
    Kind(Option<procraft_core::ItemKind>),
    Source(Option<String>),
    Sort(SortBy),
    Order(bool),
}

/// What taking something of each kind means, in the words its item page uses.
fn caveat(kind: procraft_core::ItemKind) -> &'static str {
    match kind {
        procraft_core::ItemKind::Plugin => {
            "Runs code on your hooks. You can turn it off in Plugins, and nothing it draws or stores survives."
        }
        procraft_core::ItemKind::BrushSet => "Adds to the brush library.",
        procraft_core::ItemKind::AssetPack => "Adds to the asset library.",
    }
}

/// A rounded rect as a triangle fan, so a picture or a scrim can follow it. Egui's own picture
/// drawing is square-cornered; this is the small mesh that rounds it. The colour comes per vertex,
/// so a mesh can carry a gradient with nothing to see between one band and the next.
fn rounded_mesh(
    rect: Rect,
    corner: f32,
    uv: Rect,
    colour: impl Fn(Pos2) -> Color32,
    texture: egui::TextureId,
) -> egui::Mesh {
    let radius = corner.min(rect.width().min(rect.height()) * 0.5).max(0.0);
    let uv_of = |p: Pos2| {
        Pos2::new(
            uv.left() + uv.width() * ((p.x - rect.left()) / rect.width()),
            uv.top() + uv.height() * ((p.y - rect.top()) / rect.height()),
        )
    };
    let mut shape = egui::Mesh::with_texture(texture);
    let centre = rect.center();
    shape.vertices.push(egui::epaint::Vertex { pos: centre, uv: uv_of(centre), color: colour(centre) });
    // the ring: four quarter arcs, clockwise from the top left
    let arcs = [
        (Pos2::new(rect.left() + radius, rect.top() + radius), std::f32::consts::PI),
        (Pos2::new(rect.right() - radius, rect.top() + radius), 1.5 * std::f32::consts::PI),
        (Pos2::new(rect.right() - radius, rect.bottom() - radius), 0.0),
        (Pos2::new(rect.left() + radius, rect.bottom() - radius), 0.5 * std::f32::consts::PI),
    ];
    const STEPS: usize = 6;
    let mut ring: Vec<Pos2> = Vec::new();
    for (centre, from) in arcs {
        for step in 0..=STEPS {
            let angle = from + (std::f32::consts::FRAC_PI_2) * (step as f32 / STEPS as f32);
            ring.push(Pos2::new(centre.x + radius * angle.cos(), centre.y + radius * angle.sin()));
        }
    }
    for point in &ring {
        shape.vertices.push(egui::epaint::Vertex { pos: *point, uv: uv_of(*point), color: colour(*point) });
    }
    let count = ring.len() as u32;
    for step in 0..count {
        shape.indices.extend_from_slice(&[0, step + 1, (step + 1) % count + 1]);
    }
    shape
}

/// The font atlas's one white texel. A mesh of one colour draws with this uv, sampling a white
/// texel, never the whole atlas - which streaks the shape with whatever glyphs sit at its corner.
fn white_uv() -> Rect {
    Rect::from_min_max(egui::epaint::WHITE_UV, egui::epaint::WHITE_UV)
}

/// A grouped list's head, the only way the store draws one: 16 under whatever came before it, its
/// own 17 line in 13, and 8 between it and the card or field it labels. Returns where the next
/// block starts.
fn group_head(painter: &egui::Painter, body: Rect, y: f32, label: &str, colour: Color32) -> f32 {
    text(
        painter,
        Pos2::new(body.left() + 16.0, y + GROUP_HEAD),
        Align2::LEFT_TOP,
        label,
        13.0,
        Weight::Regular,
        colour,
    );
    y + GROUP_HEAD + GROUP_LINE + GROUP_HEAD_BELOW
}

/// A picture drawn inside a rounded rect.
fn picture_rounded(painter: &egui::Painter, texture: &egui::TextureHandle, rect: Rect, corner: f32, uv: Rect) {
    let mesh = rounded_mesh(rect, corner, uv, |_| Color32::WHITE, texture.id());
    painter.add(egui::Shape::mesh(mesh));
}

/// A vertical fade of one colour: `top` alpha at its first row, `bottom` at its last. The vertex
/// colours interpolate across the mesh, so there is no step to see.
fn fade_down(painter: &egui::Painter, rect: Rect, colour: Color32, top: u8, bottom: u8) {
    let (from, to) = (f32::from(top), f32::from(bottom));
    let (r, g, b) = (colour.r(), colour.g(), colour.b());
    let color = move |p: Pos2| {
        let down = ((p.y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
        Color32::from_rgba_unmultiplied(r, g, b, (from + (to - from) * down) as u8)
    };
    let mesh = rounded_mesh(rect, 0.0, white_uv(), color, egui::TextureId::default());
    painter.add(egui::Shape::mesh(mesh));
}

/// A scrim from `top` alpha at its first row to `bottom` at its last, following a rounded rect.
fn scrim_rounded(painter: &egui::Painter, rect: Rect, corner: f32, top: u8, bottom: u8) {
    let (from, to) = (f32::from(top), f32::from(bottom));
    let color = move |p: Pos2| {
        let down = ((p.y - rect.top()) / rect.height().max(1.0)).clamp(0.0, 1.0);
        Color32::from_black_alpha((from + (to - from) * down) as u8)
    };
    let mesh = rounded_mesh(rect, corner, white_uv(), color, egui::TextureId::default());
    painter.add(egui::Shape::mesh(mesh));
}

/// The part of a picture to show so it fills `area` without stretching it out of shape.
fn fill_uv(picture: Vec2, area: Vec2) -> Rect {
    let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
    if picture.x <= 0.0 || picture.y <= 0.0 || area.x <= 0.0 || area.y <= 0.0 {
        return whole;
    }
    let (want, has) = (area.x / area.y, picture.x / picture.y);
    let (mut u0, mut v0) = (0.0, 0.0);
    let (mut u1, mut v1) = (1.0, 1.0);
    if has > want {
        let cover = want / has;
        u0 = (1.0 - cover) / 2.0;
        u1 = 1.0 - u0;
    } else {
        let cover = has / want;
        v0 = (1.0 - cover) / 2.0;
        v1 = 1.0 - v0;
    }
    Rect::from_min_max(Pos2::new(u0, v0), Pos2::new(u1, v1))
}

/// As much of a sentence as fits `width` on one line, with an ellipsis where it was cut.
fn one_line(sentence: &str, width: f32, painter: &egui::Painter) -> String {
    let mut kept = String::new();
    for word in sentence.split_whitespace() {
        let trying = if kept.is_empty() { word.to_owned() } else { format!("{kept} {word}") };
        if text_width(painter, &format!("{trying} ..."), 15.0, Weight::Regular) > width {
            return if kept.is_empty() { String::new() } else { format!("{kept} ...") };
        }
        kept = trying;
    }
    kept
}

/// One card's width in a shelf, and how tall the shelf is with its labels under the pictures.
fn card_width(area: f32) -> f32 {
    (area - SHELF_GAP * (SHELF_CARDS as f32 - 1.0)) / SHELF_CARDS as f32
}

fn shelf_height(area: f32) -> f32 {
    card_width(area) * 0.75 + 84.0
}

/// The heading a kind's section carries, and the title its wall takes.
fn section_title(kind: procraft_core::ItemKind) -> &'static str {
    match kind {
        procraft_core::ItemKind::Plugin => "Plugins",
        procraft_core::ItemKind::BrushSet => "Brushes",
        procraft_core::ItemKind::AssetPack => "Asset packs",
    }
}

/// What the Filters menu calls a kind.
fn filter_label(kind: procraft_core::ItemKind) -> &'static str {
    match kind {
        procraft_core::ItemKind::Plugin => "Plugins",
        procraft_core::ItemKind::BrushSet => "Brush sets",
        procraft_core::ItemKind::AssetPack => "Asset packs",
    }
}

/// The downloads arrow and its tray, drawn where a page carries one in its bar.
fn download_glyph(painter: &egui::Painter, centre: Pos2, size: f32, colour: Color32) {
    let d = size / 2.0;
    let stroke = Stroke::new(1.8, colour);
    painter.line_segment([Pos2::new(centre.x, centre.y - d * 0.8), Pos2::new(centre.x, centre.y + d * 0.1)], stroke);
    painter.line_segment(
        [Pos2::new(centre.x - d * 0.42, centre.y - d * 0.3), Pos2::new(centre.x, centre.y + d * 0.1)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x + d * 0.42, centre.y - d * 0.3), Pos2::new(centre.x, centre.y + d * 0.1)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(centre.x - d * 0.8, centre.y + d * 0.7), Pos2::new(centre.x + d * 0.8, centre.y + d * 0.7)],
        stroke,
    );
}

/// The two arrows the sort button carries: one up, one down.
fn sort_glyph(painter: &egui::Painter, centre: Pos2, size: f32, colour: Color32) {
    let d = size / 2.0;
    let stroke = Stroke::new(1.8, colour);
    let (left, right) = (centre.x - d * 0.45, centre.x + d * 0.45);
    painter.line_segment([Pos2::new(left, centre.y + d * 0.6), Pos2::new(left, centre.y - d * 0.6)], stroke);
    painter.line_segment([Pos2::new(left - d * 0.3, centre.y - d * 0.25), Pos2::new(left, centre.y - d * 0.6)], stroke);
    painter.line_segment([Pos2::new(right, centre.y - d * 0.6), Pos2::new(right, centre.y + d * 0.6)], stroke);
    painter
        .line_segment([Pos2::new(right + d * 0.3, centre.y + d * 0.25), Pos2::new(right, centre.y + d * 0.6)], stroke);
}

/// The icon a kind's tile draws, as the design's tile does.
fn kind_icon(kind: procraft_core::ItemKind) -> &'static icons::Icon {
    match kind {
        procraft_core::ItemKind::Plugin => &icons::ACTIONS,
        procraft_core::ItemKind::BrushSet => &icons::BRUSH,
        procraft_core::ItemKind::AssetPack => &icons::FOLDER_FILL,
    }
}

/// The line under a name: the publisher and the size, or that it came with the app.
fn second_line(item: &procraft_core::StoreItem) -> String {
    if item.file.is_empty() {
        return "Bundled with ProCraft".to_owned();
    }
    format!("{} - {}", item.publisher, size_of(item.bytes))
}

/// A source's second line: whose it is and when it was read, or why it is showing a stale copy.
fn source_line(source: &procraft_core::SourceInfo) -> String {
    if !source.failure.is_empty() {
        return source.failure.clone();
    }
    if source.address.is_empty() {
        return "Bundled with ProCraft".to_owned();
    }
    let who = if source.by.is_empty() { source.address.clone() } else { source.by.clone() };
    if source.read == 0 {
        return format!("{who} - not read yet");
    }
    format!("{who} - {}", date_text(source.read, false))
}

fn size_of(bytes: u64) -> String {
    if bytes >= 1 << 20 {
        format!("{:.1} MB", bytes as f64 / f64::from(1 << 20))
    } else {
        format!("{} KB", bytes.div_ceil(1024))
    }
}

/// Now, in seconds since the epoch.
pub(crate) fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs())
}

/// A moment in local time, as a row shows it: "18 Nov", or "18 Nov 2026" on a page of its own.
pub(crate) fn date_text(secs: u64, with_year: bool) -> String {
    use chrono::TimeZone;
    let millis = i64::try_from(secs).unwrap_or(i64::MAX).saturating_mul(1000);
    chrono::Local.timestamp_millis_opt(millis).single().map_or_else(String::new, |when| {
        if with_year { when.format("%-d %b %Y").to_string() } else { when.format("%-d %b").to_string() }
    })
}

/// The four facts a person checks before taking something, always four: the size, the version, the
/// licence and the tags, an absent licence or tag list said as much, then the requirement after them
/// when the catalogue gives one.
fn facts_of(item: &procraft_core::StoreItem) -> Vec<(String, String)> {
    let licence = if item.licence.is_empty() { "Not given".to_owned() } else { item.licence.clone() };
    let tags = if item.tags.is_empty() { "None".to_owned() } else { item.tags.join(", ") };
    let mut facts = vec![
        ("Size".to_owned(), size_of(item.bytes)),
        ("Version".to_owned(), if item.version.is_empty() { "Not given".to_owned() } else { item.version.clone() }),
        ("Licence".to_owned(), licence),
        ("Tags".to_owned(), tags),
    ];
    if !item.requires.is_empty() {
        facts.push(("Requires".to_owned(), item.requires.clone()));
    }
    facts
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(file: &str, bytes: u64) -> procraft_core::StoreItem {
        procraft_core::StoreItem {
            id: "org.example.dry-ink".into(),
            kind: procraft_core::ItemKind::BrushSet,
            name: "Dry Ink".into(),
            version: "2.1.0".into(),
            publisher: "R. Halloway".into(),
            description: "Six brushes.".into(),
            licence: "CC BY 4.0".into(),
            requires: "Interface 2".into(),
            category: "Brushes".into(),
            tags: Vec::new(),
            preview: String::new(),
            previews: Vec::new(),
            body: String::new(),
            file: file.into(),
            bytes,
            sha256: String::new(),
            source: "a1b2c3".into(),
            source_name: "Ink and Grain".into(),
        }
    }

    fn source(address: &str, by: &str, read: u64, failure: &str) -> procraft_core::SourceInfo {
        procraft_core::SourceInfo {
            id: "a1b2c3".into(),
            name: "Ink and Grain".into(),
            by: by.into(),
            address: address.into(),
            on: true,
            read,
            failure: failure.into(),
        }
    }

    #[test]
    fn a_row_s_second_line_is_the_publisher_and_the_size() {
        assert_eq!(second_line(&item("https://example.org/x.brushset", 1844221)), "R. Halloway - 1.8 MB");
        assert_eq!(second_line(&item("https://example.org/x.brushset", 48213)), "R. Halloway - 48 KB");
        assert_eq!(second_line(&item("", 0)), "Bundled with ProCraft");
    }

    #[test]
    fn back_walks_the_pages_it_came_from_and_then_closes_the_store() {
        let mut sheet = StoreSheet { open: true, ..Default::default() };
        sheet.go(View::Settings);
        sheet.go(View::Source { id: "a1b2c3".to_owned() });
        sheet.back();
        assert_eq!(sheet.view, View::Settings, "a source goes back to Settings");
        sheet.back();
        assert_eq!(sheet.view, View::Browse, "Settings goes back to the list");
        sheet.back();
        assert!(!sheet.open, "the list closes the store");
    }

    #[test]
    fn a_picture_fills_its_frame_by_cropping_and_never_stretching() {
        // 4:3 picture into the band's 880 by 260: the top and bottom go, the sides stay
        let band = fill_uv(Vec2::new(800.0, 600.0), Vec2::new(880.0, 260.0));
        assert_eq!((band.left(), band.right()), (0.0, 1.0), "the full width stays");
        assert!((band.top() - 0.303).abs() < 0.01, "about a third off each end: {}", band.top());
        assert!((band.bottom() - 0.697).abs() < 0.01);
        // a picture wider than its frame loses the sides instead
        let card = fill_uv(Vec2::new(2000.0, 400.0), Vec2::new(400.0, 400.0));
        assert_eq!((card.top(), card.bottom()), (0.0, 1.0), "the full height stays");
        assert!((card.left() - 0.4).abs() < 0.01, "the sides go: {}", card.left());
        // the same shape as its frame is drawn whole, and nothing at all is the whole picture
        let whole = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
        assert_eq!(fill_uv(Vec2::new(880.0, 260.0), Vec2::new(880.0, 260.0)), whole);
        assert_eq!(fill_uv(Vec2::ZERO, Vec2::new(10.0, 10.0)), whole);
    }

    #[test]
    fn a_catalogue_listing_other_bytes_makes_an_installed_item_an_update() {
        let mut listed = item("https://example.org/x.brushset", 5);
        listed.sha256 = "a".repeat(64);
        assert!(changes(&"b".repeat(64), &listed), "other bytes are an update");
        assert!(!changes(&"a".repeat(64), &listed), "the same bytes are up to date");
        listed.sha256.clear();
        assert!(!changes(&"b".repeat(64), &listed), "nothing listed is nothing to compare");
    }

    #[test]
    fn the_facts_strip_always_shows_four_and_says_what_was_not_given() {
        let all = facts_of(&item("https://example.org/x.brushset", 1844221));
        assert_eq!(
            all,
            vec![
                ("Size".to_owned(), "1.8 MB".to_owned()),
                ("Version".to_owned(), "2.1.0".to_owned()),
                ("Licence".to_owned(), "CC BY 4.0".to_owned()),
                ("Tags".to_owned(), "None".to_owned()),
                ("Requires".to_owned(), "Interface 2".to_owned()),
            ]
        );
        let mut bare = item("https://example.org/x.brushset", 0);
        bare.licence.clear();
        bare.requires.clear();
        bare.tags = vec!["ink".into(), "grain".into()];
        assert_eq!(
            facts_of(&bare),
            vec![
                ("Size".to_owned(), "0 KB".to_owned()),
                ("Version".to_owned(), "2.1.0".to_owned()),
                ("Licence".to_owned(), "Not given".to_owned()),
                ("Tags".to_owned(), "ink, grain".to_owned()),
            ]
        );
    }

    #[test]
    fn a_source_s_second_line_says_whose_it_is_or_why_it_is_stale() {
        assert_eq!(source_line(&source("", "", 0, "")), "Bundled with ProCraft");
        assert_eq!(
            source_line(&source("https://example.org/c.json", "R. Halloway", 0, "")),
            "R. Halloway - not read yet"
        );
        let stale = source("https://example.org/c.json", "R. Halloway", 1_772_000_000, "it could not be read");
        assert_eq!(source_line(&stale), "it could not be read");
        let read = source("https://example.org/c.json", "R. Halloway", 1_772_000_000, "");
        assert!(source_line(&read).starts_with("R. Halloway - "));
    }

    /// The list scrolls under its clip, and the glyphs in it are masked at the list's edge as the
    /// words beside them are: none of them paints above the edge, however far the list has gone.
    /// The sheet is drawn whole with a store of its own, then what it painted is read back: every
    /// picture of a glyph over the edge carries the list's clip, not the sheet's.
    #[test]
    fn a_glyph_scrolled_over_the_list_s_edge_is_masked_by_it() {
        let dir = std::env::temp_dir().join(format!("procraft-store-glyphs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.to_string_lossy().into_owned();
        let store = procraft_core::Store::open(path.clone());
        let session = Arc::new(procraft_core::CanvasSession::new(64, 48));
        let plugins = procraft_core::Plugins::with_store(dir.join("plugins").to_string_lossy().into_owned());
        let mut sheet = StoreSheet { store: Some(store), open: true, ..StoreSheet::default() };
        let mut look = Look::default();
        // the window it is drawn in, and where the page puts the list's top edge in it: under the
        // title, the search field and the categories
        let screen = Rect::from_min_size(Pos2::ZERO, Vec2::new(1200.0, 900.0));
        let size = Vec2::new(WIDTH.min(screen.width() - 40.0), HEIGHT.min(screen.height() - 96.0));
        let sheet_rect = Rect::from_center_size(screen.center(), size);
        let body_top = sheet_rect.top() + BAR;
        let list_top = body_top + 12.0 + TITLE_LINE + 16.0 + SEARCH_HEIGHT + 16.0 + PILL_HEIGHT + 12.0;
        let above = Rect::from_min_max(
            Pos2::new(sheet_rect.left() + INSET, body_top),
            Pos2::new(sheet_rect.right() - INSET, list_top),
        );
        let ctx = egui::Context::default();
        // the fonts' texture is the first egui makes, and it carries the white pixel every solid
        // shape draws from: laid out once here, as the interface's own words lay it out first in
        // the app, so the sheet's glyphs take textures of their own
        let warm = egui::RawInput { screen_rect: Some(screen), ..egui::RawInput::default() };
        let mut warmed = ctx.run_ui(warm, |ui| {
            ui.label("warm");
        });
        warmed.textures_delta.clear();
        /// What a frame at `scroll` painted of the glyphs that reached above the list: each glyph's
        /// own place, and the clip its painter gave it. A glyph is a mesh the size of a glyph: the
        /// sheet's shadow, its fades and its scrims are meshes too, and are the sheet's own.
        fn glyphs_above(
            ctx: &egui::Context,
            sheet: &mut StoreSheet,
            look: &mut Look,
            session: &Arc<procraft_core::CanvasSession>,
            plugins: &Arc<procraft_core::Plugins>,
            screen: Rect,
            above: Rect,
            scroll: f32,
        ) -> Vec<(Rect, Rect)> {
            sheet.scroll = scroll;
            let input = egui::RawInput { screen_rect: Some(screen), ..egui::RawInput::default() };
            let mut output = ctx.run_pass(input, |ctx| sheet.show(ctx, look, session, plugins));
            output.textures_delta.clear();
            output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    egui::Shape::Mesh(mesh) => {
                        let bounds = mesh.calc_bounds();
                        (bounds.width() <= 300.0 && bounds.height() <= 300.0 && bounds.intersects(above))
                            .then_some((bounds, clipped.clip_rect))
                    }
                    _ => None,
                })
                .collect()
        }
        // at rest nothing of the list reaches above it
        let still = glyphs_above(&ctx, &mut sheet, &mut look, &session, &plugins, screen, above, 0.0);
        assert!(still.is_empty(), "at rest the list's glyphs stay under its top edge: {still:?}");
        // scrolled, the band's glyph crosses that edge (the scroll stops where the list ends): it
        // must still be clipped by the list, as its words are
        let scrolled = glyphs_above(&ctx, &mut sheet, &mut look, &session, &plugins, screen, above, 420.0);
        assert!(!scrolled.is_empty(), "the scroll brings a glyph over the list's top edge");
        assert!(sheet.scroll > 24.0, "the list has room to scroll: {}", sheet.scroll);
        for (bounds, clip) in &scrolled {
            assert!(
                clip.top() >= list_top - 0.5,
                "a glyph over the edge is clipped to the list, not to the sheet: {bounds:?} clipped {clip:?}"
            );
        }
        let _ = std::fs::remove_dir_all(&dir);
    }
}

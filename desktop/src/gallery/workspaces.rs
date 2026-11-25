//! The gallery's workspaces, as the iPad app's (GalleryWorkspaces.swift): galleries of their own,
//! the one showing named on a capsule in the middle of the title row. A tap on it hangs their list
//! from it: a tap on one shows it (the one showing is ticked), and a row slid left (or clicked with
//! the other button) shows Rename and Delete, but for the first, which stays as it is. Under them,
//! the row that makes a new one. What each is, and their folders, are the core's (workspaces.rs).

use std::sync::Arc;

use egui::{Align2, Color32, Id, Order, Rect, ScrollArea, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use procraft_core::{ArtworkStore, Workspace, Workspaces};

use super::{ACTIONS_GROUP, ACTIONS_SEPARATOR, CAPSULE, CAPSULE_EDGE, Gallery, PANEL_TITLE, POPOVER, TEXT};
use crate::icons;
use crate::look::{Look, Themed, Weight, argb, pc, text};
use crate::panel;

const PANEL_WIDTH: f32 = 300.0;
const ROW: f32 = 44.0;
/// The rows scroll past this many, the last showing half so it is seen there are more.
const MOST_ROWS: f32 = 7.5;
const BUTTON_WIDTH: f32 = 74.0;
/// How far a row slides to show Rename and Delete.
const REVEAL: f32 = BUTTON_WIDTH * 2.0 + 3.0 + 16.0;
const RENAME_FILL: Themed = Themed(0xFF3A3A3A, 0xFFC6C6C6);
const RENAME_INK: Themed = Themed(0xFFC7C7C7, 0xFF575757);
const DELETE_FILL: u32 = 0xFFFE604D;

/// What the workspaces' alert asks about.
pub(super) enum Asked {
    New,
    Rename(Workspace),
    Delete(Workspace),
}

/// The workspaces: where they are kept, the one showing, their list open, the row slid open to
/// show Rename and Delete (by folder) or being slid (and how far), and what the alert asks about.
#[derive(Default)]
pub(super) struct WorkspacesShown {
    pub(super) kept: Option<Arc<Workspaces>>,
    pub(super) showing: Option<Workspace>,
    pub(super) open: bool,
    row_open: Option<String>,
    row_drag: Option<(String, f32)>,
    pub(super) alert: crate::alert::Alert,
    pub(super) asked: Option<Asked>,
    /// Where its button was last drawn.
    pub(super) button: Option<Rect>,
}

/// The widest the workspace's button is in a window `width` across: what is left in the middle
/// between the app's name and the buttons on the right, the same each side.
fn button_room(width: f32) -> f32 {
    (width - 584.0).clamp(90.0, 300.0)
}

/// `name` cut in its middle (with an ellipsis) to fit `most` across at `size`.
fn cut_to(ui: &Ui, name: &str, size: f32, most: f32) -> String {
    let wide = |s: &str| ui.painter().layout_no_wrap(s.to_owned(), Weight::Regular.font(size), Color32::WHITE).size().x;
    if wide(name) <= most {
        return name.to_owned();
    }
    let chars: Vec<char> = name.chars().collect();
    let mut keep = chars.len();
    while keep > 1 {
        keep -= 1;
        let head = chars.iter().take(keep.div_ceil(2));
        let tail = chars.iter().skip(chars.len() - keep / 2);
        let cut: String = head.chain(std::iter::once(&'…')).chain(tail).collect();
        if wide(&cut) <= most {
            return cut;
        }
    }
    "…".to_owned()
}

impl Gallery {
    /// The gallery of the workspace showing, its workspaces kept in `default_root` (the first,
    /// the gallery as it has always been) and folders of their own in `others`.
    pub(super) fn with_workspaces(workspaces: Arc<Workspaces>) -> Gallery {
        let showing = workspaces.selected();
        let mut gallery = Gallery::with_store(ArtworkStore::open(showing.root.clone()));
        gallery.workspaces.kept = Some(workspaces);
        gallery.workspaces.showing = Some(showing);
        gallery
    }

    /// The workspace picked shown if it isn't the one showing (another was picked, or this one
    /// has a new name, or is gone): its own artworks, from its top, no stack open.
    pub(super) fn read_workspaces(&mut self) {
        let Some(kept) = self.workspaces.kept.clone() else { return };
        let picked = kept.selected();
        if self.workspaces.showing.as_ref() == Some(&picked) {
            return;
        }
        self.store = ArtworkStore::open(picked.root.clone());
        self.workspaces.showing = Some(picked);
        self.stack = None;
        self.opening_stack = None;
        self.leaving_stack = None;
        self.selecting = false;
        self.picked.clear();
        self.swiped = None;
        self.more_open = false;
        self.thumbs.clear();
        self.refresh();
    }

    /// With the Workspaces plugin off, the gallery is the first workspace's, where the app has
    /// always kept its artworks (as on the iPad); the others wait, as they were, for it to be on
    /// again.
    pub(super) fn follow_workspaces_plugin(&mut self) {
        if !self.hidden.contains(&procraft_core::Feature::Workspaces) {
            return;
        }
        self.workspaces.open = false;
        let Some(kept) = self.workspaces.kept.clone() else { return };
        if self.workspaces.showing.as_ref().is_none_or(|w| w.is_default) {
            return;
        }
        let Some(first) = kept.list().into_iter().next() else { return };
        kept.select(first);
        self.read_workspaces();
    }

    /// The capsule in the middle of the title row (`top` its top), on a `screen` this wide: the
    /// workspace showing, and a chevron for the list a tap on it opens, while the Workspaces plugin
    /// is on. Not in a stack, where the middle is the stack's name's.
    pub(super) fn workspace_button(&mut self, ui: &Ui, look: &mut Look, screen: Rect, top: f32) {
        let Some(showing) = self.workspaces.showing.as_ref() else { return };
        if self.stack.is_some() || self.hidden.contains(&procraft_core::Feature::Workspaces) {
            return;
        }
        let ink = argb(TEXT);
        let name = cut_to(ui, &showing.name, 16.0, button_room(screen.width()) - 49.0);
        let wide = ui.painter().layout_no_wrap(name.clone(), Weight::Regular.font(16.0), ink).size().x;
        let width = 16.0 + wide + 5.0 + 12.0 + 16.0;
        let button = Rect::from_center_size(pos2(screen.center().x, top + 45.0 / 2.0), vec2(width, 45.0));
        let painter = ui.painter();
        painter.rect_filled(button, 22.5, argb(CAPSULE));
        painter.rect_stroke(button, 22.5, Stroke::new(1.0, argb(CAPSULE_EDGE)), egui::StrokeKind::Inside);
        text(
            painter,
            pos2(button.left() + 16.0, button.center().y),
            Align2::LEFT_CENTER,
            &name,
            16.0,
            Weight::Regular,
            ink,
        );
        look.icon(ui, &icons::CHEVRON_DOWN, pos2(button.right() - 16.0 - 6.0, button.center().y), 12.0, ink);
        if ui.interact(button, ui.scope_id().with("workspaces"), Sense::click()).clicked() {
            self.workspaces.open = !self.workspaces.open;
            self.workspaces.row_open = None;
            self.more_open = false;
            self.new_open = false;
        }
        self.workspaces.button = Some(button);
    }

    /// The workspaces, hanging from their button, while open; then the alert asking a new one's
    /// name, another name, or before one is deleted, as it shows.
    pub(super) fn workspaces_panel(&mut self, ctx: &egui::Context, look: &mut Look, screen: Rect) {
        if self.workspaces.open && self.stack.is_none() {
            self.workspaces_list(ctx, look, screen);
        }
        self.workspace_answered(ctx, look);
    }

    fn workspaces_list(&mut self, ctx: &egui::Context, look: &mut Look, screen: Rect) {
        let Some(kept) = self.workspaces.kept.clone() else { return };
        let list = kept.list();
        let showing = self.workspaces.showing.clone();
        let count = list.len() as f32;
        let tall = count.min(MOST_ROWS) * (ROW + 1.0) - if count > MOST_ROWS { 0.0 } else { 1.0 };
        let height = panel::ARROW_HEIGHT + 14.0 + 29.0 + 14.0 + tall + 8.0 + ROW + 12.0;
        let left =
            (screen.center().x - PANEL_WIDTH / 2.0).clamp(screen.left() + 4.0, screen.right() - 4.0 - PANEL_WIDTH);
        let rect = Rect::from_min_size(pos2(left, screen.top() + 57.0), vec2(PANEL_WIDTH, height));
        let arrow_x = self.workspaces.button.map_or(screen.center().x, |b| b.center().x) - left;
        let mut close = false;
        // what was asked of a row: picked, renamed or deleted; or a new one
        let mut picked = None;
        let mut renamed = None;
        let mut deleted = None;
        let mut new = false;
        egui::Area::new(Id::unique("workspaces")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            close = ui.allocate_rect(screen, Sense::click()).clicked() || crate::ui::escape(ui.ctx());
            ui.interact(rect, ui.scope_id().with("panel"), Sense::click_and_drag());
            let body = panel::solid_frame(ui, look, "workspaces", rect, arrow_x, false, panel::POPOVER_CORNER, POPOVER);
            let ink = argb(PANEL_TITLE);
            let title = pos2(body.left() + 24.0, body.top() + 14.0);
            text(ui.painter(), title, Align2::LEFT_TOP, "Workspaces", 24.0, Weight::Bold, ink);
            let rows = Rect::from_min_size(pos2(body.left() + 10.0, body.top() + 57.0), vec2(PANEL_WIDTH - 20.0, tall));
            ui.painter().rect_filled(rows, 18.0, argb(ACTIONS_GROUP));
            ui.scope_builder(UiBuilder::new().max_rect(rows), |ui| {
                ui.set_clip_rect(rows.intersect(ui.clip_rect()));
                ScrollArea::vertical()
                    .id_salt("workspaces")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysHidden)
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                        for (i, w) in list.iter().enumerate() {
                            if i > 0 {
                                let (line, _) = ui.allocate_exact_size(vec2(rows.width(), 1.0), Sense::hover());
                                ui.painter().rect_filled(line, 0, argb(ACTIONS_SEPARATOR));
                            }
                            match self.workspace_row(ui, look, w, showing.as_ref() == Some(w), rows.width()) {
                                Some(RowAsked::Pick) => picked = Some(w.clone()),
                                Some(RowAsked::Rename) => renamed = Some(w.clone()),
                                Some(RowAsked::Delete) => deleted = Some(w.clone()),
                                None => {}
                            }
                        }
                    });
            });
            let new_row = Rect::from_min_size(pos2(rows.left(), rows.bottom() + 8.0), vec2(rows.width(), ROW));
            ui.painter().rect_filled(new_row, 18.0, argb(ACTIONS_GROUP));
            let at = pos2(new_row.left() + 16.0, new_row.center().y);
            text(ui.painter(), at, Align2::LEFT_CENTER, "New Workspace", 17.0, Weight::Regular, ink);
            look.icon(ui, &icons::ADD, pos2(new_row.right() - 8.0 - 15.0, new_row.center().y), 30.0, ink);
            new = ui.interact(new_row, ui.scope_id().with("new workspace"), Sense::click()).clicked();
        });
        if close {
            self.workspaces.open = false;
        }
        if let Some(w) = picked {
            self.workspaces.open = false;
            kept.select(w);
            self.read_workspaces();
        } else if new {
            self.workspaces.open = false;
            self.workspaces.asked = Some(Asked::New);
            self.workspaces.alert.ask_name("New Workspace", "", "Create");
        } else if let Some(w) = renamed {
            self.workspaces.open = false;
            self.workspaces.alert.ask_name("Rename Workspace", &w.name, "Rename");
            self.workspaces.asked = Some(Asked::Rename(w));
        } else if let Some(w) = deleted {
            self.workspaces.open = false;
            let count = kept.artwork_count(w.clone());
            let with = match count {
                0 => String::new(),
                1 => "1 artwork is deleted with it. ".to_owned(),
                n => format!("{n} artworks are deleted with it. "),
            };
            let title = format!("Delete “{}”?", w.name);
            self.workspaces.alert.open(&title, &format!("{with}This can't be undone."), "Delete", Some(&icons::TRASH));
            self.workspaces.asked = Some(Asked::Delete(w));
        }
        if !self.workspaces.open {
            self.workspaces.row_open = None;
            self.workspaces.row_drag = None;
        }
    }

    /// Workspace `w`'s row, `width` across: its name, ticked if `showing`. One that can be renamed
    /// or deleted slides left under a drag (or opens with the other button) to show the buttons for
    /// it; a tap puts one slid open back, else picks it.
    fn workspace_row(
        &mut self,
        ui: &mut Ui,
        look: &mut Look,
        w: &Workspace,
        showing: bool,
        width: f32,
    ) -> Option<RowAsked> {
        let (row, response) = ui.allocate_exact_size(vec2(width, ROW), Sense::click_and_drag());
        let slides = !w.is_default;
        let open = self.workspaces.row_open.as_deref() == Some(w.root.as_str());
        let x = match &self.workspaces.row_drag {
            Some((root, x)) if *root == w.root => *x,
            _ if open => -REVEAL,
            _ => 0.0,
        };
        if slides && response.drag_started() {
            self.workspaces.row_drag = Some((w.root.clone(), x));
        }
        if let Some((root, x)) = self.workspaces.row_drag.as_mut()
            && *root == w.root
        {
            *x = (*x + response.drag_delta().x).clamp(-REVEAL, 0.0);
        }
        if response.drag_stopped() && self.workspaces.row_drag.as_ref().is_some_and(|(root, _)| *root == w.root) {
            self.workspaces.row_open = (x < -REVEAL / 2.0).then(|| w.root.clone());
            self.workspaces.row_drag = None;
        }
        if slides && response.secondary_clicked() {
            self.workspaces.row_open = if open { None } else { Some(w.root.clone()) };
        }
        let mut asked = None;
        if x < 0.0 {
            let delete = Rect::from_min_size(
                pos2(row.right() - 8.0 - BUTTON_WIDTH, row.center().y - 16.5),
                vec2(BUTTON_WIDTH, 33.0),
            );
            let rename = delete.translate(vec2(-(BUTTON_WIDTH + 3.0), 0.0));
            for (button, label, fill, ink, does) in [
                (rename, "Rename", argb(RENAME_FILL), argb(RENAME_INK), RowAsked::Rename),
                (delete, "Delete", argb(DELETE_FILL), Color32::WHITE, RowAsked::Delete),
            ] {
                ui.painter().rect_filled(button, 7.0, fill);
                text(ui.painter(), button.center(), Align2::CENTER_CENTER, label, 15.0, Weight::SemiBold, ink);
                if ui.interact(button, ui.scope_id().with(("workspace", label, &w.root)), Sense::click()).clicked() {
                    asked = Some(does);
                }
            }
        }
        // it draws in from the right, its name staying where it is
        let shown = Rect::from_min_max(row.min, pos2(row.right() + x, row.bottom()));
        if x < 0.0 {
            ui.painter().rect_filled(shown, 0, argb(POPOVER));
            ui.painter().rect_filled(shown, 0, argb(ACTIONS_GROUP));
        }
        let ink = argb(PANEL_TITLE);
        let tick = showing && x == 0.0;
        let room = shown.width() - 16.0 - 14.0 - if tick { 22.0 + 8.0 } else { 0.0 };
        let name = cut_to(ui, &w.name, 17.0, room.max(1.0));
        text(
            ui.painter(),
            pos2(shown.left() + 16.0, shown.center().y),
            Align2::LEFT_CENTER,
            &name,
            17.0,
            Weight::Regular,
            ink,
        );
        // drawn back, the room is its name's
        if tick {
            look.icon(
                ui,
                &icons::MENU_TICK,
                pos2(shown.right() - 14.0 - 11.0, shown.center().y),
                22.0,
                argb(pc::ACCENT),
            );
        }
        if asked.is_some() {
            self.workspaces.row_open = None;
            return asked;
        }
        if response.clicked() {
            if self.workspaces.row_open.is_some() {
                self.workspaces.row_open = None;
            } else {
                return Some(RowAsked::Pick);
            }
        }
        None
    }

    /// The workspaces' alert, as it shows; answered, what it asked done: a new one made and shown,
    /// one renamed, or one deleted with its artworks (the first showing if it was showing).
    fn workspace_answered(&mut self, ctx: &egui::Context, look: &mut Look) {
        let Some(answer) = self.workspaces.alert.show(ctx, look) else { return };
        let typed = self.workspaces.alert.typed().unwrap_or_default().to_owned();
        let (Some(asked), Some(kept)) = (self.workspaces.asked.take(), self.workspaces.kept.clone()) else { return };
        if answer != crate::alert::Answer::Action {
            return;
        }
        let failed = match asked {
            Asked::New => match kept.create(typed) {
                Some(made) => {
                    kept.select(made);
                    None
                }
                None => Some("This workspace can't be made."),
            },
            Asked::Rename(w) => kept.rename(w, typed).is_none().then_some("This workspace can't be renamed."),
            Asked::Delete(w) => {
                kept.delete(w);
                None
            }
        };
        self.read_workspaces();
        if let Some(why) = failed {
            self.workspaces.alert.tell("Workspaces", why, None);
        }
    }
}

/// What a workspace's row asked.
enum RowAsked {
    Pick,
    Rename,
    Delete,
}

#[cfg(test)]
mod tests {
    use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, vec2};

    use super::*;

    /// The whole gallery, its panels and alerts too, shown a frame at a time.
    struct Shown {
        ctx: egui::Context,
        look: Look,
        gallery: Gallery,
        colours: crate::colours::Colours,
        time: f64,
    }

    impl Shown {
        fn frame(&mut self, events: Vec<Event>) {
            self.time += 1.0 / 60.0;
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(1200.0, 900.0))),
                time: Some(self.time),
                events,
                ..RawInput::default()
            };
            let Shown { ctx, look, gallery, colours, .. } = self;
            let mut output = ctx.clone().run_ui(raw, |ui| {
                gallery.show(ui.ctx(), look, colours);
            });
            output.textures_delta.clear();
        }

        fn click(&mut self, at: Pos2) {
            let button = |pressed| Event::PointerButton {
                pos: at,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            };
            self.frame(vec![Event::PointerMoved(at)]);
            self.frame(vec![button(true)]);
            self.frame(vec![button(false)]);
        }

        /// Frames enough for an alert to come or go.
        fn settle(&mut self) {
            for _ in 0..3 {
                self.frame(Vec::new());
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
            self.frame(Vec::new());
        }

        /// The workspaces' panel's rows (its list's, then New Workspace's), as laid out now.
        fn rows(&self, n: usize) -> (Vec<Rect>, Rect) {
            let rect = self.ctx.memory(|m| m.area_rect(Id::unique("workspaces"))).expect("the workspaces' panel");
            // (its area is the window; the panel hangs from 57 down, centred)
            let left = rect.center().x - PANEL_WIDTH / 2.0 + 10.0;
            let top = 57.0 + panel::ARROW_HEIGHT + 57.0;
            let rows = (0..n).map(|i| Rect::from_min_size(pos2(left, top + (ROW + 1.0) * i as f32), vec2(280.0, ROW)));
            let tall = n as f32 * (ROW + 1.0) - 1.0;
            (rows.collect(), Rect::from_min_size(pos2(left, top + tall + 8.0), vec2(280.0, ROW)))
        }
    }

    /// From the button in the middle of the title row: New Workspace asks its name and shows it,
    /// empty; the list then has it, ticked; the first picked again shows its artworks.
    #[test]
    fn a_new_workspace_is_named_and_shown_and_the_first_comes_back() {
        let root = std::env::temp_dir().join(format!("procraft-desk-workspaces-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let (first, others) = (root.join("artworks"), root.join("workspaces"));
        let store = ArtworkStore::open(first.to_string_lossy().into_owned());
        let dir = store.new_folder(None);
        std::fs::write(
            std::path::Path::new(&dir).join("artwork.json"),
            r#"{"name":"A","document":{"width":8,"height":8}}"#,
        )
        .unwrap();
        let workspaces = Workspaces::open(first.to_string_lossy().into_owned(), others.to_string_lossy().into_owned());
        let mut shown = Shown {
            ctx: egui::Context::default(),
            look: Look::default(),
            gallery: Gallery::with_workspaces(workspaces),
            colours: crate::colours::Colours::default(),
            time: 0.0,
        };
        crate::look::install_fonts(&shown.ctx);
        shown.frame(Vec::new());
        assert_eq!(shown.gallery.entries.len(), 1);
        let button = shown.gallery.workspaces.button.expect("the workspace's button");
        assert!((button.center().x - 600.0).abs() < 0.5, "in the middle: {button:?}");
        shown.click(button.center());
        assert!(shown.gallery.workspaces.open);
        shown.frame(Vec::new());
        let (_, new) = shown.rows(1);
        shown.click(new.center());
        assert!(!shown.gallery.workspaces.open);
        assert!(shown.gallery.workspaces.alert.is_open(), "its name asked");
        shown.settle();
        shown.frame(vec![Event::Text("Inks".into())]);
        shown.frame(vec![Event::Key {
            key: egui::Key::Enter,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        shown.frame(Vec::new());
        let showing = shown.gallery.workspaces.showing.clone().expect("a workspace showing");
        assert_eq!(showing.name, "Inks");
        assert!(shown.gallery.entries.is_empty(), "a new one is empty");
        assert_eq!(shown.gallery.store.root(), showing.root);
        // the list has both, the first first; it picked again shows its artwork
        shown.settle();
        shown.click(shown.gallery.workspaces.button.unwrap().center());
        shown.frame(Vec::new());
        let (rows, _) = shown.rows(2);
        shown.click(rows[0].center());
        let showing = shown.gallery.workspaces.showing.clone().unwrap();
        assert!(showing.is_default, "{showing:?}");
        assert_eq!(shown.gallery.entries.len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }
}

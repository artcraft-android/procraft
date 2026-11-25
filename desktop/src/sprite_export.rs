//! Sharing the frames as a sprite sheet, as the iPad's (AnimatedExport.swift `SpriteSheetExport`):
//! the sheet as the settings make it, the settings (the size, the layout, padding, trimming, held
//! frames, the JSON and the background), Cancel, and the round Done that saves the sheet where the
//! person picks, its JSON beside it. The sheet is made again (off the window's thread) a moment
//! after the settings stop changing, to show it and its size; while Done writes it, the export
//! card shows how far it has come.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Align2, Color32, ColorImage, Id, Order, Rect, Sense, Stroke, TextureHandle, pos2, vec2};
use procraft_core::{CanvasSession, SheetLayout, SpriteSheetChoices, SpriteSheetMade};

use crate::icons;
use crate::look::{Look, Themed, Weight, argb, pc, text};

const SCREEN_FILL: Themed = Themed(0xFF1F1F1F, 0xFFF2F2F2);
const TITLE_INK: Themed = Themed(0xFFAAAAAA, 0xFF3D3D3D);
const ROW_INK: Themed = Themed(0xFFC1C1C1, 0xFF595959);
const SIZE_INK: Themed = Themed(0xFF8E8E8E, 0xFF3F3F3F);
const PANEL_FILL: Themed = Themed(0xFF262626, 0xFFEBEBEB);
const PANEL_EDGE: Themed = Themed(0xFF363636, 0xFFDADADA);
const SEGMENT_TRACK: Themed = Themed(0xFF3C3C3C, 0xFFD5D5D5);
const CAPSULE_FILL: Themed = Themed(0xFF1A1A1A, 0xFFFBFBFB);
const CAPSULE_EDGE: Themed = Themed(0xFF373737, 0xFFE0E0E0);
const CANCEL_INK: u32 = 0xFFA5A5A5;
const DONE_BLUE: u32 = 0xFF0190FF;
const PANEL_WIDTH: f32 = 400.0;
/// Web Ready's longest side for a frame, as the animated exports'.
const WEB_READY_SIDE: u32 = 480;
/// The most columns, rows or pixels of padding.
const MOST_COUNT: u32 = 64;
/// How long the settings rest before the sheet is made again.
const SETTLE: Duration = Duration::from_millis(250);
/// The preview's longest side at most: a sheet of many large frames is shown smaller.
const PREVIEW_MOST: u32 = 4096;
/// The layouts, as their segments name them.
const LAYOUTS: [(SheetLayout, &str); 3] =
    [(SheetLayout::Grid, "Grid"), (SheetLayout::Across, "Row"), (SheetLayout::Down, "Column")];

/// As the screen starts: a near-square grid of the frames full size, each held frame once, with
/// its JSON and no background.
pub fn start_choices() -> SpriteSheetChoices {
    SpriteSheetChoices {
        layout: SheetLayout::Grid,
        columns: 0,
        rows: 0,
        padding: 0,
        trim: false,
        held_once: true,
        json: true,
        max_side: 0,
        transparent: true,
    }
}

/// The sheet's file name for an artwork named `name`: "Name Sprite Sheet.png".
pub fn file_name(name: String) -> String {
    format!("{} Sprite Sheet.png", procraft_core::export_file_name(name, false))
}

/// One sheet made at a time: the preview's and Done's write through the same renderer, and the
/// export card follows the one under way.
static MAKING: Mutex<()> = Mutex::new(());

/// The sheet as `choices` ask, written in `folder` as `name` (a PNG), and its JSON beside it (the
/// same name ending ".json") if asked for: what was made.
pub fn sheet_files(
    session: &CanvasSession,
    folder: &Path,
    name: &str,
    choices: SpriteSheetChoices,
) -> Result<SpriteSheetMade, String> {
    let _one = MAKING.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let path = folder.join(name).to_string_lossy().into_owned();
    session.export_sprite_sheet(path, choices).map_err(|e| e.to_string())
}

/// The files `made` wrote, a PNG at `png` and its JSON if any.
fn files_of(png: &Path, made: &SpriteSheetMade) -> Vec<PathBuf> {
    std::iter::once(png.to_path_buf()).chain(made.json_path.as_ref().map(PathBuf::from)).collect()
}

/// A sheet made for the preview: what it is, and its picture (fitted to [`PREVIEW_MOST`]).
type Preview = (SpriteSheetMade, ColorImage);

/// The screen while it is up, and the one that just ended, kept a frame: its sheet's texture was
/// drawn in the frame it ended in, and a texture let go of is gone before that frame is.
#[derive(Default)]
pub struct SpriteSheet {
    open: Option<SpriteSheetExport>,
    ended: Option<SpriteSheetExport>,
}

impl SpriteSheet {
    /// Opens the screen, as it starts.
    pub fn open(&mut self) {
        self.open = Some(SpriteSheetExport::new());
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// A new frame: the screen that ended in the last is let go of.
    pub fn frame_started(&mut self) {
        self.ended = None;
    }

    /// Shows the screen over everything while it is up.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &Arc<CanvasSession>) {
        let Some(screen) = self.open.as_mut() else { return };
        if screen.show(ctx, look, session) {
            self.ended = self.open.take();
        }
    }
}

pub struct SpriteSheetExport {
    choices: SpriteSheetChoices,
    /// When the settings last changed: the sheet is made once they rest.
    changed: Instant,
    /// The sheet shown and what it is, the settings it was made with; the one being made.
    sheet: Option<TextureHandle>,
    made: Option<(SpriteSheetChoices, SpriteSheetMade)>,
    making: Option<(SpriteSheetChoices, Receiver<Option<Preview>>)>,
    /// The settings a sheet couldn't be made with (not tried again).
    failed: Option<SpriteSheetChoices>,
    /// Upright, the settings panel is open.
    settings_open: bool,
    card: crate::export_card::ExportCard,
}

impl SpriteSheetExport {
    fn new() -> SpriteSheetExport {
        SpriteSheetExport {
            choices: start_choices(),
            // made at once as it opens
            changed: Instant::now().checked_sub(SETTLE).unwrap_or_else(Instant::now),
            sheet: None,
            made: None,
            making: None,
            failed: None,
            settings_open: false,
            card: crate::export_card::ExportCard::default(),
        }
    }

    /// The sheet for the settings now, made again (off the window's thread) once they have rested
    /// a moment; one at a time.
    fn make_sheet(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>) {
        if let Some((choices, rx)) = &self.making {
            match rx.try_recv() {
                Ok(Some((made, picture))) => {
                    self.sheet = Some(ctx.load_texture("sprite sheet", picture, egui::TextureOptions::NEAREST));
                    self.made = Some((*choices, made));
                }
                Ok(None) | Err(TryRecvError::Disconnected) => self.failed = Some(*choices),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(50));
                    return;
                }
            }
            self.making = None;
        }
        let wanted = self.choices;
        if self.made.as_ref().is_some_and(|(c, _)| *c == wanted) || self.failed == Some(wanted) {
            return;
        }
        let resting = self.changed.elapsed();
        if resting < SETTLE {
            ctx.request_repaint_after(SETTLE - resting);
            return;
        }
        let (tx, rx) = channel();
        let session = Arc::clone(session);
        // drawn from the artwork open, as an export is (on OpenGL nothing else draws meanwhile)
        crate::files::write_meanwhile(true, move || {
            let folder = std::env::temp_dir().join("procraft-exports");
            let name = file_name(session.name());
            let preview = match sheet_files(&session, &folder, &name, wanted) {
                Ok(made) => preview_picture(&folder.join(&name)).map(|picture| (made, picture)),
                Err(e) => {
                    log::warn!("making a sprite sheet: {e}");
                    None
                }
            };
            let _ = tx.send(preview);
        });
        self.making = Some((wanted, rx));
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    /// The settings changed to `choices`.
    fn change(&mut self, choices: SpriteSheetChoices) {
        if choices != self.choices {
            self.choices = choices;
            self.changed = Instant::now();
        }
    }

    /// Shows the screen over everything: true once it has ended (Cancel, or the sheet saved).
    /// Wide, the settings stay in a 400-wide panel on the left and the artwork's name and the
    /// sheet's size sit over the sheet, which fills the rest (41 in at the sides, 110 down).
    /// Upright, "Export Settings" opens the panel over a square box (24 in), a tap beside it
    /// closing it again. Cancel and the round Done are at the top right.
    fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &Arc<CanvasSession>) -> bool {
        self.make_sheet(ctx, session);
        let screen = ctx.content_rect();
        let wide = screen.width() > screen.height();
        let left = if wide { PANEL_WIDTH + 10.0 } else { 0.0 };
        let inset = if wide { 41.0 } else { 24.0 };
        let box_width = screen.width() - left - inset * 2.0;
        let box_height = if wide { box_width.min(screen.height() - 140.0) } else { box_width };
        let shown = Rect::from_min_size(screen.min + vec2(left + inset, 110.0), vec2(box_width, box_height));
        let mut cancel = false;
        let mut done = false;
        let mut choices = self.choices;
        egui::Area::new(Id::unique("sprite sheet export")).fixed_pos(screen.min).order(Order::Foreground).show(
            ctx,
            |ui| {
                // the whole window: nothing under it takes a press
                let beside = ui.allocate_rect(screen, Sense::click_and_drag());
                ui.painter().rect_filled(screen, 0, argb(SCREEN_FILL));
                // the sheet, fitted to the box, its edge drawn
                if let Some(t) = &self.sheet {
                    let [w, h] = t.size().map(|v| v.max(1) as f32);
                    let k = (shown.width() / w).min(shown.height() / h);
                    let picture = Rect::from_center_size(shown.center(), vec2(w * k, h * k));
                    let all = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                    ui.painter().image(t.id(), picture, all, Color32::WHITE);
                    ui.painter().rect_stroke(picture, 0, Stroke::new(1.0, argb(PANEL_EDGE)), egui::StrokeKind::Inside);
                }
                if wide {
                    // the name and what the sheet is, centred between the panel and the buttons
                    let middle = screen.left() + left + 24.0 + (screen.width() - left - 24.0 - 224.0) / 2.0;
                    let name = title_of(session);
                    let at = pos2(middle, screen.top() + 23.0 + 15.0);
                    text(ui.painter(), at, Align2::CENTER_CENTER, &name, 20.0, Weight::Bold, argb(TITLE_INK));
                    if let Some((_, made)) = &self.made {
                        let at = pos2(middle, screen.top() + 23.0 + 30.0 + 10.0);
                        let said = said(made);
                        text(ui.painter(), at, Align2::CENTER_CENTER, &said, 13.0, Weight::Regular, argb(SIZE_INK));
                    }
                } else {
                    let button = Rect::from_min_size(screen.min + vec2(14.0, 20.0), vec2(60.0 + 110.0, 58.0));
                    let chevron = pos2(button.left() + 30.0, button.center().y);
                    look.icon(ui, &icons::CHEVRON_LEFT, chevron, 28.0, argb(TITLE_INK));
                    let at = pos2(button.left() + 60.0, button.center().y - 14.5);
                    text(ui.painter(), at, Align2::LEFT_CENTER, "Export", 24.0, Weight::Bold, argb(TITLE_INK));
                    let at = pos2(button.left() + 60.0, button.center().y + 14.5);
                    text(ui.painter(), at, Align2::LEFT_CENTER, "Settings", 24.0, Weight::Bold, argb(TITLE_INK));
                    if ui.interact(button, ui.scope_id().with("export settings"), Sense::click()).clicked() {
                        self.settings_open = !self.settings_open;
                    }
                }
                // Cancel and Done at the top right
                let done_rect =
                    Rect::from_min_size(pos2(screen.right() - 16.0 - 44.0, screen.top() + 16.0), vec2(44.0, 45.0));
                let galley =
                    ui.painter().layout_no_wrap("Cancel".to_owned(), Weight::Regular.font(17.0), argb(CANCEL_INK));
                let capsule = Rect::from_min_size(
                    pos2(done_rect.left() - 13.0 - galley.size().x - 26.0, done_rect.center().y - 21.5),
                    vec2(galley.size().x + 26.0, 43.0),
                );
                ui.painter().rect_filled(capsule, 21.5, argb(CAPSULE_FILL));
                ui.painter().rect_stroke(capsule, 21.5, Stroke::new(1.0, argb(CAPSULE_EDGE)), egui::StrokeKind::Inside);
                ui.painter().galley(capsule.center() - galley.size() / 2.0, galley, argb(CANCEL_INK));
                cancel = ui.interact(capsule, ui.scope_id().with("cancel"), Sense::click()).clicked();
                ui.painter().circle_filled(done_rect.center(), 22.0, argb(DONE_BLUE));
                look.icon(ui, &icons::DONE_TICK, done_rect.center(), 40.0, Color32::WHITE);
                if !self.card.busy() {
                    done = ui.interact(done_rect, ui.scope_id().with("done"), Sense::click()).clicked();
                }
                if wide || self.settings_open {
                    let panel = Rect::from_min_max(
                        screen.min + vec2(10.0, 10.0),
                        pos2(screen.left() + 10.0 + PANEL_WIDTH, screen.bottom() - 10.0),
                    );
                    // upright, a tap outside the panel closes it
                    if !wide && beside.clicked() && beside.interact_pointer_pos().is_some_and(|p| !panel.contains(p)) {
                        self.settings_open = false;
                    }
                    ui.interact(panel, ui.scope_id().with("panel"), Sense::click_and_drag());
                    ui.painter().rect_filled(panel, 20, argb(PANEL_FILL));
                    ui.painter().rect_stroke(panel, 20, Stroke::new(1.0, argb(PANEL_EDGE)), egui::StrokeKind::Inside);
                    choices = settings(ui, panel, self.choices);
                }
            },
        );
        self.change(choices);
        if cancel || (crate::ui::escape(ctx) && !self.card.busy()) {
            return true;
        }
        if done {
            self.save(session);
        }
        // saved: once the card has shown its check, the screen goes
        self.card.show(ctx, look) == Some(crate::export_card::Ended::Made)
    }

    /// Done: the sheet as the settings make it written where the person picks, its JSON beside
    /// it, the export card showing how far it has come.
    fn save(&mut self, session: &Arc<CanvasSession>) {
        let Some(path) = crate::files::dialog()
            .set_title("Share as Sprite Sheet")
            .set_file_name(file_name(session.name()))
            .add_filter("PNG", &["png"])
            .save_file()
        else {
            return;
        };
        let path = if path.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
            path
        } else {
            path.with_extension("png")
        };
        let (Some(folder), Some(name)) = (path.parent().map(Path::to_path_buf), path.file_name()) else { return };
        let name = name.to_string_lossy().into_owned();
        let choices = self.choices;
        crate::harmony_export::write_placed(session, &mut self.card, folder, move |session, part| {
            let made = sheet_files(session, part, &name, choices)?;
            Ok(files_of(&part.join(&name), &made))
        });
    }
}

/// The settings in `panel`, top down: the title, Max Resolution or Web Ready, the layout (a grid's
/// columns and rows), the padding, and the switches. What they are now.
fn settings(ui: &egui::Ui, panel: Rect, choices: SpriteSheetChoices) -> SpriteSheetChoices {
    let mut c = choices;
    let at = pos2(panel.left() + 24.0, panel.top() + 19.0 + 18.0);
    text(ui.painter(), at, Align2::LEFT_CENTER, "Sprite Sheet", 24.0, Weight::Bold, argb(TITLE_INK));
    let mut y = panel.top() + 19.0 + 36.0 + 11.5;
    let sizes = segments(ui, panel, y, &["Max Resolution", "Web Ready"], usize::from(c.max_side != 0), "size");
    if let Some(i) = sizes {
        c.max_side = if i == 1 { WEB_READY_SIDE } else { 0 };
    }
    y += 34.0 + 12.0;
    let labels = LAYOUTS.map(|(_, label)| label);
    let chosen = LAYOUTS.iter().position(|(l, _)| *l == c.layout).unwrap_or(0);
    if let Some((layout, _)) = segments(ui, panel, y, &labels, chosen, "layout").and_then(|i| LAYOUTS.get(i)) {
        c.layout = *layout;
    }
    y += 34.0 + 6.0;
    if c.layout == SheetLayout::Grid {
        c.columns = count_line(ui, panel, y, "Columns", c.columns, "Auto");
        y += 52.0;
        c.rows = count_line(ui, panel, y, "Rows", c.rows, "Auto");
        y += 52.0;
    }
    c.padding = count_line(ui, panel, y, "Padding", c.padding, "None");
    y += 52.0;
    let switches: [(&str, &mut bool); 4] = [
        ("Trim empty space", &mut c.trim),
        ("Held frames once", &mut c.held_once),
        ("JSON data", &mut c.json),
        ("Transparent background", &mut c.transparent),
    ];
    for (label, on) in switches {
        let row = Rect::from_min_size(pos2(panel.left(), y), vec2(PANEL_WIDTH, 52.0));
        let at = pos2(row.left() + 16.0, row.center().y);
        text(ui.painter(), at, Align2::LEFT_CENTER, label, 17.0, Weight::Regular, argb(ROW_INK));
        crate::transform::switch(ui, pos2(row.left() + 16.0 + 307.0, row.center().y - 14.0), *on, label);
        if ui.interact(row, ui.scope_id().with(label), Sense::click()).clicked() {
            *on = !*on;
        }
        y += 52.0;
    }
    c
}

/// Choices side by side in a grey track 384 wide from `top`, 12 apart, the `chosen` one blue: the
/// one pressed, if one was.
fn segments(ui: &egui::Ui, panel: Rect, top: f32, labels: &[&str], chosen: usize, id: &str) -> Option<usize> {
    let track = Rect::from_min_size(pos2(panel.left() + 7.5, top), vec2(384.0, 34.0));
    ui.painter().rect_filled(track, 17, argb(SEGMENT_TRACK));
    let n = labels.len().max(1) as f32;
    let item = (track.width() - 4.0 - 12.0 * (n - 1.0)) / n;
    let mut pressed = None;
    for (i, label) in labels.iter().enumerate() {
        let cell = Rect::from_min_size(track.min + vec2(2.0 + (item + 12.0) * i as f32, 2.0), vec2(item, 30.0));
        if i == chosen {
            ui.painter().rect_filled(cell, 15, argb(pc::ACCENT));
        }
        let ink = if i == chosen { Color32::WHITE } else { argb(ROW_INK) };
        text(ui.painter(), cell.center(), Align2::CENTER_CENTER, label, 15.0, Weight::Regular, ink);
        if ui.interact(cell, ui.scope_id().with((id, i)), Sense::click()).clicked() {
            pressed = Some(i);
        }
    }
    pressed
}

/// A setting that counts, 52 tall from `top`: its name, its value (0 is `none`), and − and + in a
/// grey capsule 16 in from the right. The value, stepped if one was pressed.
fn count_line(ui: &egui::Ui, panel: Rect, top: f32, label: &str, value: u32, none: &str) -> u32 {
    let row = Rect::from_min_size(pos2(panel.left(), top), vec2(PANEL_WIDTH, 52.0));
    let ink = argb(ROW_INK);
    text(ui.painter(), pos2(row.left() + 16.0, row.center().y), Align2::LEFT_CENTER, label, 17.0, Weight::Regular, ink);
    let capsule = Rect::from_min_size(pos2(row.right() - 16.0 - 89.0, row.center().y - 16.0), vec2(89.0, 32.0));
    let shown = if value == 0 { none.to_owned() } else { value.to_string() };
    let at = pos2(capsule.left() - 12.0, row.center().y);
    text(ui.painter(), at, Align2::RIGHT_CENTER, &shown, 17.0, Weight::Regular, ink);
    ui.painter().rect_filled(capsule, 16, argb(SEGMENT_TRACK));
    let middle = capsule.center().x;
    let rule = Rect::from_center_size(pos2(middle, capsule.center().y), vec2(1.0, 18.0));
    ui.painter().rect_filled(rule, 0, argb(PANEL_EDGE));
    let mut out = value;
    for (sign, half, step_ok) in [
        ("−", Rect::from_min_max(capsule.min, pos2(middle, capsule.bottom())), value > 0),
        ("+", Rect::from_min_max(pos2(middle, capsule.top()), capsule.max), value < MOST_COUNT),
    ] {
        let ink = if step_ok { ink } else { ink.gamma_multiply(0.35) };
        text(ui.painter(), half.center(), Align2::CENTER_CENTER, sign, 20.0, Weight::Medium, ink);
        if step_ok && ui.interact(half, ui.scope_id().with((label, sign)), Sense::click()).clicked() {
            out = if sign == "+" { value + 1 } else { value - 1 };
        }
    }
    out
}

/// What the sheet is: its size and how many pictures are on it.
fn said(made: &SpriteSheetMade) -> String {
    let pictures = if made.cells == 1 { "1 frame".to_owned() } else { format!("{} frames", made.cells) };
    format!("{} × {} px · {pictures}", made.width, made.height)
}

/// The sheet's PNG at `path` as the preview shows it, fitted to [`PREVIEW_MOST`].
fn preview_picture(path: &Path) -> Option<ColorImage> {
    let picture = image::open(path).ok()?.to_rgba8();
    let (w, h) = picture.dimensions();
    let picture = if w.max(h) > PREVIEW_MOST {
        let k = PREVIEW_MOST as f32 / w.max(h) as f32;
        let (pw, ph) = (((w as f32 * k).round() as u32).max(1), ((h as f32 * k).round() as u32).max(1));
        image::imageops::resize(&picture, pw, ph, image::imageops::FilterType::Triangle)
    } else {
        picture
    };
    let size = [picture.width() as usize, picture.height() as usize];
    Some(ColorImage::from_rgba_unmultiplied(size, picture.as_raw()))
}

/// The artwork's name, or "Untitled Artwork".
fn title_of(session: &CanvasSession) -> String {
    let name = session.name();
    if name.trim().is_empty() { "Untitled Artwork".to_owned() } else { name.trim().to_owned() }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 40 × 40 canvas drawn offscreen, three layers filled red, green and blue, with Animation
    /// Assist on: three frames. None where there is no GPU.
    fn three_frames() -> Option<Arc<CanvasSession>> {
        let s = CanvasSession::new(40, 40);
        if !s.prepare_offscreen() {
            eprintln!("skipped: no GPU device");
            return None;
        }
        s.set_name("Dots walk".into());
        let first = s.layers().first()?.id;
        let ids = [first, s.add_layer(), s.add_layer()];
        for (id, [r, g, b]) in ids.into_iter().zip([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]) {
            s.set_color(r, g, b);
            assert!(s.fill_layer(id));
        }
        assert!(s.set_animation_assist(true));
        Some(s)
    }

    fn folder(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("procraft-desktop-sheet-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn the_sheet_is_saved_with_its_json_beside_it_as_done_writes_it() {
        let Some(s) = three_frames() else { return };
        let to = folder("done");
        std::fs::create_dir_all(&to).unwrap();
        let name = file_name(s.name());
        assert_eq!(name, "Dots walk Sprite Sheet.png");
        // as Done writes it: in a part folder beside where it goes, then put in place
        let part = crate::harmony_export::part_folder(&to);
        let made = sheet_files(&s, &part, &name, start_choices()).unwrap();
        let files = files_of(&part.join(&name), &made);
        crate::harmony_export::place(&to, &part, &files).unwrap();
        assert!(!part.exists(), "the part folder goes");
        // three frames on a near-square grid: two across, two down, the fourth cell empty
        assert_eq!((made.width, made.height, made.cells, made.frames), (80, 80, 3, 3));
        assert_eq!(said(&made), "80 × 80 px · 3 frames");
        let sheet = image::open(to.join(&name)).unwrap().to_rgba8();
        assert_eq!(sheet.dimensions(), (80, 80));
        assert_eq!(sheet.get_pixel(20, 20).0, [255, 0, 0, 255]);
        assert_eq!(sheet.get_pixel(60, 20).0, [0, 255, 0, 255]);
        assert_eq!(sheet.get_pixel(20, 60).0, [0, 0, 255, 255]);
        assert_eq!(sheet.get_pixel(60, 60).0[3], 0, "the empty cell is clear");
        // the JSON names the picture beside it and where each frame is
        let json = std::fs::read_to_string(to.join("Dots walk Sprite Sheet.json")).unwrap();
        assert!(json.contains("\"Dots walk Sprite Sheet.png\""), "{json}");
        assert_eq!(json.matches("\"frame\"").count(), 3, "{json}");
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn a_row_at_web_size_without_its_json_is_the_picture_alone() {
        let Some(s) = three_frames() else { return };
        let to = folder("row");
        let choices = SpriteSheetChoices {
            layout: SheetLayout::Across,
            padding: 2,
            json: false,
            max_side: 20,
            ..start_choices()
        };
        let made = sheet_files(&s, &to, "row.png", choices).unwrap();
        // three 20-pixel frames in a row, 2 apart
        assert_eq!((made.width, made.height, made.cells), (64, 20, 3));
        assert_eq!(made.json_path, None);
        assert_eq!(files_of(&to.join("row.png"), &made), [to.join("row.png")]);
        assert!(!to.join("row.json").exists());
        let picture = preview_picture(&to.join("row.png")).unwrap();
        assert_eq!(picture.size, [64, 20]);
        let _ = std::fs::remove_dir_all(&to);
    }

    #[test]
    fn the_screen_shows_the_sheet_and_makes_it_again_as_the_layout_changes() {
        use egui::{Event, Modifiers, PointerButton, RawInput};
        let Some(s) = three_frames() else { return };
        let (ctx, mut look) = (egui::Context::default(), Look::default());
        crate::look::install_fonts(&ctx);
        let mut sheet = SpriteSheet::default();
        sheet.open();
        // wide: the panel at 10 in; the layout's segments under the size's, "Row" the middle one
        let row =
            pos2(10.0 + 7.5 + 2.0 + (384.0 - 4.0 - 24.0) / 3.0 * 1.5 + 12.0, 10.0 + 19.0 + 36.0 + 11.5 + 46.0 + 17.0);
        let press = |pressed| Event::PointerButton {
            pos: row,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut frame = |sheet: &mut SpriteSheet, events: Vec<Event>, step: u32| {
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))),
                time: Some(f64::from(step) / 60.0),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.clone().run_ui(raw, |ui| sheet.show(ui.ctx(), &mut look, &s));
            output.textures_delta.clear();
            sheet.frame_started();
        };
        let made = |sheet: &SpriteSheet| {
            sheet
                .open
                .as_ref()
                .and_then(|e| e.made.as_ref().filter(|_| e.sheet.is_some()).map(|(c, m)| (*c, m.clone())))
        };
        let mut step = 0;
        while made(&sheet).is_none() && step < 500 {
            frame(&mut sheet, Vec::new(), step);
            std::thread::sleep(Duration::from_millis(10));
            step += 1;
        }
        let (choices, first) = made(&sheet).unwrap();
        assert_eq!(choices.layout, SheetLayout::Grid);
        assert_eq!((first.width, first.height, first.cells), (80, 80, 3));
        // Row pressed: once the settings rest, the three frames in a row
        frame(&mut sheet, vec![Event::PointerMoved(row)], step);
        frame(&mut sheet, vec![press(true)], step + 1);
        frame(&mut sheet, vec![press(false)], step + 2);
        step += 3;
        assert_eq!(sheet.open.as_ref().map(|e| e.choices.layout), Some(SheetLayout::Across));
        while made(&sheet).is_none_or(|(c, _)| c.layout != SheetLayout::Across) && step < 1000 {
            frame(&mut sheet, Vec::new(), step);
            std::thread::sleep(Duration::from_millis(10));
            step += 1;
        }
        let (_, across) = made(&sheet).unwrap();
        assert_eq!((across.width, across.height, across.cells), (120, 40, 3));
        assert!(sheet.is_open());
    }
}

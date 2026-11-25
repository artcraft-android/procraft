//! Sharing what a plugin makes of the animation (interface 23: Sprite Sheet and Harmony Scene come
//! from the Sprite Sheet and Harmony plugin), as the iPad's (AnimatedExport.swift
//! `PluginShareScreen`). One with settings opens its screen: what it makes as they stand, the
//! settings as its plugin lists them (a slider with names as choices side by side, another as a
//! count with − and +, a switch as a switch), Cancel, and the round Done that saves its files in
//! the folder the person picks. What it makes is made again (off the window's thread) a moment
//! after the settings stop changing, to show it and what the plugin says of it. One without
//! settings is made as soon as the folder is picked. While Done writes, the export card shows how
//! far it has come. The settings are the plugin's own, kept with it as on its card.
//!
//! A sound the export takes that it can't read as it is (an m4a, an mp3) is copied as WAV first by
//! the system's own converter (a Mac's afconvert, elsewhere ffmpeg where it is installed); a sound
//! none can copy is left out.
//!
//! And how an export of several files (a sheet and its JSON) is written: in a folder of its own
//! beside where they go, then moved into place once all are whole.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use egui::{Align2, Color32, ColorImage, Id, Order, Rect, Sense, Stroke, TextureHandle, pos2, vec2};
use procraft_core::{
    CanvasSession, PluginControl, PluginShare, PluginShared, PluginSlider, Plugins, plugin_slider_choice,
    plugin_slider_chosen, plugin_slider_shown,
};

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
/// How long the settings rest before what it makes is made again.
const SETTLE: Duration = Duration::from_millis(250);
/// The preview's longest side at most: a sheet of many large frames is shown smaller.
const PREVIEW_MOST: u32 = 4096;

/// One made at a time: the preview's and Done's go through the same renderer, and the export card
/// follows the one under way.
static MAKING: Mutex<()> = Mutex::new(());

/// The files `share` of `plugins` makes of the animation, written in `folder`, and what the plugin
/// says of them. A sound it takes that it can't read as it is goes over as a WAV copy. Slow: off
/// the window's thread.
pub fn share_files(
    session: &CanvasSession,
    plugins: &Arc<Plugins>,
    key: &str,
    folder: &Path,
) -> Result<PluginShared, String> {
    let _one = MAKING.lock().unwrap_or_else(|e| e.into_inner());
    std::fs::create_dir_all(folder).map_err(|e| e.to_string())?;
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    let sounds = std::env::temp_dir().join(format!("procraft-share-sounds-{}-{nanos}", std::process::id()));
    let mut wav = HashMap::new();
    for source in session.plugin_share_sounds(Arc::clone(plugins), key.to_owned()) {
        let out = sounds.join(format!("{}.wav", wav.len()));
        if wav_copy(Path::new(&source), &out) {
            wav.insert(source, out.to_string_lossy().into_owned());
        }
    }
    let made = session
        .plugin_share(Arc::clone(plugins), key.to_owned(), folder.to_string_lossy().into_owned(), wav)
        .map_err(|e| e.to_string());
    let _ = std::fs::remove_dir_all(&sounds);
    made
}

/// The sound at `source` written at `to` as 16-bit WAV, its rate and channels as they are, by the
/// system's converter: a Mac's afconvert, elsewhere ffmpeg if it is installed. False if it can't
/// be (nothing is left at `to`).
pub fn wav_copy(source: &Path, to: &Path) -> bool {
    if to.parent().is_some_and(|p| std::fs::create_dir_all(p).is_err()) {
        return false;
    }
    let mut command = if cfg!(target_os = "macos") {
        let mut c = std::process::Command::new("afconvert");
        c.args(["-f", "WAVE", "-d", "LEI16"]).arg(source).arg(to);
        c
    } else {
        let mut c = std::process::Command::new("ffmpeg");
        c.args(["-nostdin", "-y", "-loglevel", "error", "-i"]).arg(source).args(["-vn", "-c:a", "pcm_s16le"]).arg(to);
        c
    };
    // (no console window flashing up on Windows)
    #[cfg(windows)]
    std::os::windows::process::CommandExt::creation_flags(&mut command, 0x0800_0000);
    let ran = command.stdin(std::process::Stdio::null()).output();
    // a WAV's header alone is 44 bytes: more is a sound
    let copied = ran.is_ok_and(|o| o.status.success()) && std::fs::metadata(to).is_ok_and(|m| m.len() > 44);
    if !copied {
        let _ = std::fs::remove_file(to);
    }
    copied
}

/// Writes an export on a thread of its own (among those the app waits for as it ends), `card`
/// showing how far the core says it has come. `write` makes its files in the folder it is handed,
/// a part folder of its own beside `to`, so what is there stays until the new files are whole;
/// they are then moved into `to`. Cancelled or failed, the part folder goes and nothing in `to`
/// changes; a failure is told.
pub(crate) fn write_placed(
    session: &Arc<CanvasSession>,
    card: &mut crate::export_card::ExportCard,
    to: PathBuf,
    write: impl FnOnce(&CanvasSession, &Path) -> Result<Vec<PathBuf>, String> + Send + 'static,
) {
    if card.busy() {
        return;
    }
    session.reset_export_progress();
    let following = Arc::clone(session);
    let exporting = card.start(move || following.export_progress());
    let session = Arc::clone(session);
    crate::files::write_meanwhile(true, move || {
        let part = part_folder(&to);
        // a panic on the way (the GPU's driver, say) is said as any failure, the card let go
        let written = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| write(&session, &part)))
            .unwrap_or_else(|panic| Err(format!("it stopped: {}", procraft_gpu::panic_message(panic.as_ref()))));
        let placed = written
            .and_then(|files| if exporting.cancelled() { Ok(false) } else { place(&to, &part, &files).map(|()| true) });
        let _ = std::fs::remove_dir_all(&part);
        match placed {
            Ok(made) => exporting.ended(made),
            Err(e) => {
                exporting.ended(false);
                if !exporting.cancelled() {
                    crate::files::tell_later("Can't share", &e);
                }
            }
        }
    });
}

/// A new part folder in `to` for an export's files while they are written.
pub(crate) fn part_folder(to: &Path) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    to.join(format!(".procraft-export-{nanos}.part"))
}

/// `files`, written in the part folder `part`, moved into `to` under their own names (over any
/// there of the same name), and `part` gone.
pub(crate) fn place(to: &Path, part: &Path, files: &[PathBuf]) -> Result<(), String> {
    for f in files {
        let Some(name) = f.file_name() else { continue };
        std::fs::rename(f, to.join(name)).map_err(|e| e.to_string())?;
    }
    let _ = std::fs::remove_dir_all(part);
    Ok(())
}

/// `share`'s files saved in the folder the person picks, the export card showing how far it has
/// come.
fn save(
    session: &Arc<CanvasSession>,
    plugins: &Arc<Plugins>,
    share: &PluginShare,
    card: &mut crate::export_card::ExportCard,
) {
    if card.busy() {
        return;
    }
    let Some(folder) = crate::files::dialog().set_title(format!("Share as {}", share.label)).pick_folder() else {
        return;
    };
    let (plugins, key) = (Arc::clone(plugins), share.key.clone());
    write_placed(session, card, folder, move |session, part| {
        let made = share_files(session, &plugins, &key, part)?;
        Ok(made.files.into_iter().map(PathBuf::from).collect())
    });
}

/// What a share made for the preview: what the plugin said, and its first picture (fitted to
/// [`PREVIEW_MOST`]), if it made one.
type Preview = (Option<String>, Option<ColorImage>);

/// The screen while it is up, and the one that just ended, kept a frame: its picture's texture was
/// drawn in the frame it ended in, and a texture let go of is gone before that frame is.
#[derive(Default)]
pub struct PluginShares {
    open: Option<ShareScreen>,
    ended: Option<ShareScreen>,
}

impl PluginShares {
    /// Share's `share`: its screen if it has settings; otherwise saved at once where the person
    /// picks, `card` showing how far it has come.
    pub fn open(
        &mut self,
        share: PluginShare,
        session: &Arc<CanvasSession>,
        plugins: &Arc<Plugins>,
        card: &mut crate::export_card::ExportCard,
    ) {
        if share.controls.is_empty() {
            save(session, plugins, &share, card);
        } else {
            self.open = Some(ShareScreen::new(share));
        }
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// A new frame: the screen that ended in the last is let go of.
    pub fn frame_started(&mut self) {
        self.ended = None;
    }

    /// Shows the screen over everything while it is up.
    pub fn show(&mut self, ctx: &egui::Context, look: &mut Look, session: &Arc<CanvasSession>, plugins: &Arc<Plugins>) {
        let Some(screen) = self.open.as_mut() else { return };
        if screen.show(ctx, look, session, plugins) {
            self.ended = self.open.take();
        }
    }
}

struct ShareScreen {
    share: PluginShare,
    /// When the settings last changed: what it makes is made again once they rest.
    changed: Instant,
    /// The settings as last seen (see [`asked`]).
    asked: String,
    /// The picture shown and what the plugin said, the settings they were made with; the one being
    /// made.
    picture: Option<TextureHandle>,
    made: Option<(String, Option<String>)>,
    making: Option<(String, Receiver<Option<Preview>>)>,
    /// The settings it couldn't be made with (not tried again).
    failed: Option<String>,
    /// Upright, the settings panel is open.
    settings_open: bool,
    card: crate::export_card::ExportCard,
}

/// The settings of `share` as a key, so what it makes is made again as they change.
fn asked(share: &PluginShare) -> String {
    let values = share.controls.iter().map(|control| match control {
        PluginControl::Switch { control } => control.on.to_string(),
        PluginControl::Slider { control } => control.value.to_string(),
        PluginControl::File { .. } | PluginControl::Texture { .. } | PluginControl::Folder { .. } => String::new(),
    });
    values.collect::<Vec<_>>().join("-")
}

impl ShareScreen {
    fn new(share: PluginShare) -> ShareScreen {
        ShareScreen {
            asked: asked(&share),
            share,
            // made at once as it opens
            changed: Instant::now().checked_sub(SETTLE).unwrap_or_else(Instant::now),
            picture: None,
            made: None,
            making: None,
            failed: None,
            settings_open: false,
            card: crate::export_card::ExportCard::default(),
        }
    }

    /// What it makes as the settings stand, made again (off the window's thread) once they have
    /// rested a moment; one at a time.
    fn make(&mut self, ctx: &egui::Context, session: &Arc<CanvasSession>, plugins: &Arc<Plugins>) {
        if let Some((asked, rx)) = &self.making {
            match rx.try_recv() {
                Ok(Some((said, picture))) => {
                    self.picture = picture.map(|p| ctx.load_texture("plugin share", p, egui::TextureOptions::NEAREST));
                    self.made = Some((asked.clone(), said));
                }
                Ok(None) | Err(TryRecvError::Disconnected) => self.failed = Some(asked.clone()),
                Err(TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(50));
                    return;
                }
            }
            self.making = None;
        }
        let wanted = self.asked.clone();
        if self.made.as_ref().is_some_and(|(a, _)| *a == wanted) || self.failed.as_ref() == Some(&wanted) {
            return;
        }
        let resting = self.changed.elapsed();
        if resting < SETTLE {
            ctx.request_repaint_after(SETTLE - resting);
            return;
        }
        let (tx, rx) = channel();
        let (session, plugins, key) = (Arc::clone(session), Arc::clone(plugins), self.share.key.clone());
        // drawn from the artwork open, as an export is (on OpenGL nothing else draws meanwhile)
        crate::files::write_meanwhile(true, move || {
            let folder = part_folder(&std::env::temp_dir().join("procraft-exports"));
            let preview = match share_files(&session, &plugins, &key, &folder) {
                Ok(made) => {
                    let png = made.files.iter().find(|f| f.to_ascii_lowercase().ends_with(".png"));
                    Some((made.said, png.and_then(|p| preview_picture(Path::new(p)))))
                }
                Err(e) => {
                    log::warn!("making a plugin's share: {e}");
                    None
                }
            };
            let _ = std::fs::remove_dir_all(&folder);
            let _ = tx.send(preview);
        });
        self.making = Some((wanted, rx));
        ctx.request_repaint_after(Duration::from_millis(50));
    }

    /// Shows the screen over everything: true once it has ended (Cancel, the files saved, or its
    /// plugin turned off). Wide, the settings stay in a 400-wide panel on the left and the
    /// artwork's name and what the plugin says sit over the picture, which fills the rest (41 in at
    /// the sides, 110 down). Upright, "Export Settings" opens the panel over a square box (24 in),
    /// a tap beside it closing it again. Cancel and the round Done are at the top right.
    fn show(
        &mut self,
        ctx: &egui::Context,
        look: &mut Look,
        session: &Arc<CanvasSession>,
        plugins: &Arc<Plugins>,
    ) -> bool {
        // as its controls are set now; gone with its plugin
        let Some(current) = plugins.shares().into_iter().find(|s| s.key == self.share.key) else { return true };
        let now = asked(&current);
        if now != self.asked {
            self.asked = now;
            self.changed = Instant::now();
        }
        self.share = current;
        self.make(ctx, session, plugins);
        let screen = ctx.content_rect();
        let wide = screen.width() > screen.height();
        let left = if wide { PANEL_WIDTH + 10.0 } else { 0.0 };
        let inset = if wide { 41.0 } else { 24.0 };
        let box_width = screen.width() - left - inset * 2.0;
        let box_height = if wide { box_width.min(screen.height() - 140.0) } else { box_width };
        let shown = Rect::from_min_size(screen.min + vec2(left + inset, 110.0), vec2(box_width, box_height));
        let mut cancel = false;
        let mut done = false;
        egui::Area::new(Id::unique("plugin share")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            // the whole window: nothing under it takes a press
            let beside = ui.allocate_rect(screen, Sense::click_and_drag());
            ui.painter().rect_filled(screen, 0, argb(SCREEN_FILL));
            // what it makes, fitted to the box, its edge drawn
            if let Some(t) = &self.picture {
                let [w, h] = t.size().map(|v| v.max(1) as f32);
                let k = (shown.width() / w).min(shown.height() / h);
                let picture = Rect::from_center_size(shown.center(), vec2(w * k, h * k));
                let all = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
                ui.painter().image(t.id(), picture, all, Color32::WHITE);
                ui.painter().rect_stroke(picture, 0, Stroke::new(1.0, argb(PANEL_EDGE)), egui::StrokeKind::Inside);
            }
            if wide {
                // the name and what the plugin says, centred between the panel and the buttons
                let middle = screen.left() + left + 24.0 + (screen.width() - left - 24.0 - 224.0) / 2.0;
                let name = title_of(session);
                let at = pos2(middle, screen.top() + 23.0 + 15.0);
                text(ui.painter(), at, Align2::CENTER_CENTER, &name, 20.0, Weight::Bold, argb(TITLE_INK));
                if let Some((_, Some(said))) = &self.made {
                    let at = pos2(middle, screen.top() + 23.0 + 30.0 + 10.0);
                    text(ui.painter(), at, Align2::CENTER_CENTER, said, 13.0, Weight::Regular, argb(SIZE_INK));
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
            let galley = ui.painter().layout_no_wrap("Cancel".to_owned(), Weight::Regular.font(17.0), argb(CANCEL_INK));
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
                settings(ui, panel, &self.share, plugins);
            }
        });
        if cancel || (crate::ui::escape(ctx) && !self.card.busy()) {
            return true;
        }
        if done {
            save(session, plugins, &self.share, &mut self.card);
        }
        // saved: once the card has shown its check, the screen goes
        self.card.show(ctx, look) == Some(crate::export_card::Ended::Made)
    }
}

/// The settings in `panel`, top down: the export's name, then each of its plugin's controls (a
/// slider with names as segments under its name, another as a count, a switch as a switch). A
/// change goes straight to the plugin.
fn settings(ui: &egui::Ui, panel: Rect, share: &PluginShare, plugins: &Plugins) {
    let at = pos2(panel.left() + 24.0, panel.top() + 19.0 + 18.0);
    text(ui.painter(), at, Align2::LEFT_CENTER, &share.label, 24.0, Weight::Bold, argb(TITLE_INK));
    let mut y = panel.top() + 19.0 + 36.0 + 11.5;
    for control in &share.controls {
        match control {
            PluginControl::Switch { control } => {
                let row = Rect::from_min_size(pos2(panel.left(), y), vec2(PANEL_WIDTH, 52.0));
                let at = pos2(row.left() + 16.0, row.center().y);
                text(ui.painter(), at, Align2::LEFT_CENTER, &control.label, 17.0, Weight::Regular, argb(ROW_INK));
                crate::transform::switch(
                    ui,
                    pos2(row.left() + 16.0 + 307.0, row.center().y - 14.0),
                    control.on,
                    &control.key,
                );
                if ui.interact(row, ui.scope_id().with(&control.key), Sense::click()).clicked() {
                    plugins.set_switch(control.key.clone(), !control.on);
                }
                y += 52.0;
            }
            PluginControl::Slider { control } if !control.names.is_empty() => {
                let at = pos2(panel.left() + 16.0, y + 10.0 + 9.0);
                let ink = argb(ROW_INK).gamma_multiply(0.7);
                text(ui.painter(), at, Align2::LEFT_CENTER, &control.label, 15.0, Weight::Regular, ink);
                y += 10.0 + 18.0 + 6.0;
                let labels: Vec<&str> = control.names.iter().map(String::as_str).collect();
                let chosen = plugin_slider_chosen(control.clone(), control.value) as usize;
                if let Some(i) = segments(ui, panel, y, &labels, chosen, &control.key) {
                    plugins.set_slider(control.key.clone(), plugin_slider_choice(control.clone(), i as u32));
                }
                y += 34.0 + 6.0;
            }
            PluginControl::Slider { control } => {
                if let Some(to) = count_line(ui, panel, y, control) {
                    plugins.set_slider(control.key.clone(), to);
                }
                y += 52.0;
            }
            PluginControl::File { .. } | PluginControl::Texture { .. } | PluginControl::Folder { .. } => {}
        }
    }
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

/// A slider as a count, 52 tall from `top`: its name, its value as the plugin shows it, and − and +
/// a step each in a grey capsule 16 in from the right. Its new value, if one was pressed.
fn count_line(ui: &egui::Ui, panel: Rect, top: f32, slider: &PluginSlider) -> Option<f32> {
    let row = Rect::from_min_size(pos2(panel.left(), top), vec2(PANEL_WIDTH, 52.0));
    let ink = argb(ROW_INK);
    let at = pos2(row.left() + 16.0, row.center().y);
    text(ui.painter(), at, Align2::LEFT_CENTER, &slider.label, 17.0, Weight::Regular, ink);
    let capsule = Rect::from_min_size(pos2(row.right() - 16.0 - 89.0, row.center().y - 16.0), vec2(89.0, 32.0));
    let shown = plugin_slider_shown(slider.clone(), slider.value);
    let at = pos2(capsule.left() - 12.0, row.center().y);
    text(ui.painter(), at, Align2::RIGHT_CENTER, &shown, 17.0, Weight::Regular, ink);
    ui.painter().rect_filled(capsule, 16, argb(SEGMENT_TRACK));
    let middle = capsule.center().x;
    let rule = Rect::from_center_size(pos2(middle, capsule.center().y), vec2(1.0, 18.0));
    ui.painter().rect_filled(rule, 0, argb(PANEL_EDGE));
    let mut out = None;
    for (sign, half, step_ok, to) in [
        (
            "−",
            Rect::from_min_max(capsule.min, pos2(middle, capsule.bottom())),
            slider.value > slider.least,
            (slider.value - slider.step).max(slider.least),
        ),
        (
            "+",
            Rect::from_min_max(pos2(middle, capsule.top()), capsule.max),
            slider.value < slider.most,
            (slider.value + slider.step).min(slider.most),
        ),
    ] {
        let ink = if step_ok { ink } else { ink.gamma_multiply(0.35) };
        text(ui.painter(), half.center(), Align2::CENTER_CENTER, sign, 20.0, Weight::Medium, ink);
        if step_ok && ui.interact(half, ui.scope_id().with((&slider.key, sign)), Sense::click()).clicked() {
            out = Some(to);
        }
    }
    out
}

/// The PNG at `path` as the preview shows it, fitted to [`PREVIEW_MOST`].
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

    /// The Sprite Sheet and Harmony plugin.
    const EXPORTS: &str = "io.github.artcraftandroid.frame-exports";

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
        let dir = std::env::temp_dir().join(format!("procraft-desktop-share-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The app's plugins in `dir`, the Sprite Sheet and Harmony plugin turned on.
    fn exports_on(dir: &Path) -> Arc<Plugins> {
        let plugins = Plugins::with_bundled(dir.join("plugins").to_string_lossy().into_owned());
        assert!(plugins.set_on(EXPORTS.to_owned(), true));
        plugins
    }

    #[test]
    fn a_sheet_and_its_json_are_written_in_a_part_folder_then_put_in_place() {
        let Some(s) = three_frames() else { return };
        let dir = folder("done");
        let plugins = exports_on(&dir);
        let to = dir.join("to");
        std::fs::create_dir_all(&to).unwrap();
        // as Done writes it: in a part folder beside where it goes, then put in place
        let part = part_folder(&to);
        let made = share_files(&s, &plugins, &format!("{EXPORTS}.sprite_sheet"), &part).unwrap();
        let files: Vec<PathBuf> = made.files.iter().map(PathBuf::from).collect();
        place(&to, &part, &files).unwrap();
        assert!(!part.exists(), "the part folder goes");
        let mut names: Vec<String> =
            std::fs::read_dir(&to).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["Dots_walk.json", "Dots_walk.png"]);
        assert!(made.said.is_some_and(|s| s.contains("80 × 80")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn harmony_is_one_zip() {
        let Some(s) = three_frames() else { return };
        let dir = folder("harmony");
        let plugins = exports_on(&dir);
        let made = share_files(&s, &plugins, &format!("{EXPORTS}.harmony"), &dir.join("out")).unwrap();
        assert_eq!(made.files.len(), 1);
        assert!(made.files[0].ends_with(".zip"), "{:?}", made.files);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_screen_shows_the_sheet_and_makes_it_again_as_the_layout_changes() {
        use egui::{Event, Modifiers, PointerButton, RawInput};
        let Some(s) = three_frames() else { return };
        let dir = folder("screen");
        let plugins = exports_on(&dir);
        let share = plugins.shares().into_iter().find(|p| p.label == "Sprite Sheet").unwrap();
        let (ctx, mut look) = (egui::Context::default(), Look::default());
        crate::look::install_fonts(&ctx);
        let mut shares = PluginShares::default();
        let mut card = crate::export_card::ExportCard::default();
        shares.open(share, &s, &plugins, &mut card);
        assert!(shares.is_open(), "a share with settings opens its screen");
        // wide: the panel at 10 in; under the Longest side's count, Layout's name and its
        // segments, "Row" the middle one
        let row = pos2(
            10.0 + 7.5 + 2.0 + (384.0 - 4.0 - 24.0) / 3.0 * 1.5 + 12.0,
            10.0 + 19.0 + 36.0 + 11.5 + 52.0 + 34.0 + 17.0,
        );
        let press = |pressed| Event::PointerButton {
            pos: row,
            button: PointerButton::Primary,
            pressed,
            modifiers: Modifiers::NONE,
        };
        let mut frame = |shares: &mut PluginShares, events: Vec<Event>, step: u32| {
            let raw = RawInput {
                screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(1200.0, 800.0))),
                time: Some(f64::from(step) / 60.0),
                events,
                ..RawInput::default()
            };
            let mut output = ctx.clone().run_ui(raw, |ui| shares.show(ui.ctx(), &mut look, &s, &plugins));
            output.textures_delta.clear();
            shares.frame_started();
        };
        let made = |shares: &PluginShares| {
            shares.open.as_ref().and_then(|e| {
                let size = e.picture.as_ref()?.size();
                e.made.as_ref().map(|(asked, _)| (asked.clone(), size))
            })
        };
        let mut step = 0;
        while made(&shares).is_none() && step < 500 {
            frame(&mut shares, Vec::new(), step);
            std::thread::sleep(Duration::from_millis(10));
            step += 1;
        }
        let (first, size) = made(&shares).unwrap();
        // three frames, the green held once, on a near-square grid of 40-pixel cells
        assert_eq!(size, [80, 80]);
        // Row pressed: once the settings rest, the three frames in a row
        frame(&mut shares, vec![Event::PointerMoved(row)], step);
        frame(&mut shares, vec![press(true)], step + 1);
        frame(&mut shares, vec![press(false)], step + 2);
        step += 3;
        while made(&shares).is_none_or(|(asked, _)| asked == first) && step < 1000 {
            frame(&mut shares, Vec::new(), step);
            std::thread::sleep(Duration::from_millis(10));
            step += 1;
        }
        let (_, across) = made(&shares).unwrap();
        assert_eq!(across, [120, 40]);
        assert!(shares.is_open());
        let _ = std::fs::remove_dir_all(&dir);
    }
}

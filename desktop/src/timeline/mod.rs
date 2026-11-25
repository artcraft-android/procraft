//! The timeline at the bottom of the canvas, once the artwork is on one, as the iPad shows it
//! (ios/ProCraft/TimelineGrid.swift and the files beside it): a grid of frame cells, a row a track,
//! under a ruler of frame numbers, the playhead standing still in the middle of the bar while the
//! frames slide under it. Its header plays the frames the grid shows, steps a frame (held, a
//! drawing) back or on, and adds a track or a frame; its settings button asks for the settings'
//! card ([`TimelineGrid::settings_asked`]).
//!
//! What it reads comes from the core in one call (`timeline_panel`), read again only when the
//! artwork or the playhead has changed; each drawing's picture is drawn again only when its
//! version changes. Drawing on the canvas paints the drawing under the playhead on the chosen
//! track (a stroke on an empty frame makes one), and the core draws the onion skins: the grid only
//! moves the playhead and the track drawn on.
//!
//! Editing the frames, as the iPad's grid does (ios/ProCraft/TimelineGridCells.swift,
//! TimelineGridMenus.swift, TimelineGridMove.swift, TimelineGroupRows.swift):
//!
//! - **Lasting longer** (`edges.rs`): the frame under the playhead, tapped, is outlined with a
//!   handle on each edge; a handle dragged shows what it would do (`timeline_lengthen_preview`)
//!   and is one step when let go; held at a limit, a line stands down the tracks there.
//! - **The lasso** (the header's lasso): strokes pick the frames they cross; the last of them in
//!   time has the handles, which lengthen them all; one of them held (or right-clicked) opens
//!   their menu.
//! - **Moving** (`moving.rs`): a block held and dragged goes along by frames and rows, dropped
//!   where it is let go, onto a new track over the line between two rows.
//! - **Menus** (`menus.rs`), on a hold or a right-click: a frame's, a block's (a group, a sound,
//!   a video), an empty frame's, a track's (on its name), the picked frames', the end flap's (it
//!   is tapped), and Add new (`+` held).
//! - **Groups** (`rows.rs`): an expanded group's tracks share rows under its track, inside an
//!   outline; collapsed, each of its frames shows the drawings in it.
//! - **Keys** (`keys.rs`): a keyed track's rows of keys under its row (rows of the same
//!   [`rows::GridRows`], a shared row's tracks sharing them), their marks picked, dragged,
//!   stretched and eased; a block's Keys card (`keys_card.rs`), which a frame's or a group's
//!   menu's **Keys** opens through [`TimelineGrid::keys_card_for`]; Animate in the header, keying
//!   with the transform tool, and Perform (`animate.rs`); the motion path over the canvas
//!   (`motion_path.rs`).
//!
//! Not here (the iPad has them): zoom, and what only sounds and videos have.

mod actions;
mod animate;
mod edges;
mod grid;
mod grid_icons;
mod keys;
mod keys_card;
mod menus;
mod motion_path;
mod moving;
mod rows;

use std::collections::{BTreeSet, HashMap};
use std::sync::Arc;

use egui::TextureHandle;
use procraft_core::{CanvasSession, LayerInfo, TimelineClipKind, TimelineInfo, TimelinePanel};

/// How far past the timeline's last frame the playhead may rest (the core's `PAST_END`): the grid
/// shows that many empty frames after it.
const PAST_END: u32 = 64;
/// A drawing's picture, this many pixels on its long side.
const PICTURE_SIDE: u32 = 96;
/// The most pictures drawn in one frame of the interface: the rest come in the frames after.
const PICTURES_A_FRAME: usize = 12;

/// The timeline grid: what it last read of the timeline, the drawings' pictures, and how the frames
/// under the playhead are moving.
#[derive(Default)]
pub struct TimelineGrid {
    /// The settings button (or Actions → Timeline Settings) was tapped: the settings' card is
    /// wanted. The app takes it.
    pub settings_asked: bool,
    /// Where the settings button was last drawn (window points): the card stands over it.
    pub settings_button: Option<egui::Rect>,
    /// The block (a clip id) whose Keys card is open: a frame's menu's Keys sets it.
    pub keys_card_for: Option<u64>,
    /// Animate tapped (or the autokey badge's Turn Off): autokey wanted on or off. The app takes
    /// it, since it opens and ends the transform tool.
    pub animate_asked: Option<bool>,
    /// Settings → Timeline → Perform smoothing (0..1), for a take's keys.
    pub perform_smoothing: f32,
    /// The Timeline plugin's Performed moves is off: no record button, nothing armed.
    pub perform_hidden: bool,
    /// The Timeline plugin's My curves is off: the curves kept aren't offered.
    pub my_curves_hidden: bool,
    /// The canvas it read (its address), the number of the panel's last reading, and the edits
    /// and timeline it was read at.
    session: usize,
    known: u64,
    read_at: Option<(u64, TimelineInfo)>,
    panel: Option<TimelinePanel>,
    /// Each drawing's picture, by its layer, with the version it was drawn at (none: it couldn't
    /// be drawn then).
    pictures: HashMap<u64, (u64, Option<TextureHandle>)>,
    motion: grid::Motion,
    playing: Option<Playing>,
    /// The rows as last read, top first.
    rows: Arc<rows::GridRows>,
    /// The frame under the playhead tapped: its handles out.
    picked: Option<u64>,
    /// The lasso on: the frames picked together.
    selecting: Option<BTreeSet<u64>>,
    /// An edge being dragged, the lasso's stroke, a block being moved.
    edit: edges::Edit,
    stroke: grid::Lasso,
    moving: Option<moving::Move>,
    /// The menu standing on the bar, and where its cards were last drawn.
    menu: Option<menus::Menu>,
    cards: Vec<egui::Rect>,
    /// The timeline's length as the end flap is dragged, before it is let go.
    length_draft: Option<u32>,
    /// What to tell (a warning, or not).
    said: Option<(String, bool)>,
    pub(crate) keys: keys::Keys,
    pub(crate) animate: animate::Animate,
    card: keys_card::Card,
    path: motion_path::PathDrag,
}

/// Playing: round the frames `first..end` (those the grid showed as it began), the frame showing,
/// the way it goes, when the next is due (the interface's seconds), and the frame the grid holds
/// still at meanwhile.
struct Playing {
    first: u32,
    end: u32,
    at: u32,
    step: i32,
    due: f64,
    held: f32,
}

/// How far over the window's foot the grid's bar reaches this frame (0 while none shows): the
/// toolbars and the adjustments' panels at the foot stand over it, as on the iPad.
pub fn lift(ctx: &egui::Context) -> f32 {
    let frame = ctx.cumulative_frame_nr();
    ctx.data(|d| d.get_temp::<(f32, u64)>(egui::Id::unique("timeline grid lift")))
        .filter(|(_, at)| at + 1 >= frame)
        .map_or(0.0, |(lift, _)| lift)
}

/// The top of the timeline's bar (window points), where it showed this frame or the last: what
/// stands at the window's foot stands over it (see [`lift`]).
#[cfg(test)]
pub fn bar_top(ctx: &egui::Context) -> Option<f32> {
    let lift = lift(ctx);
    (lift > 0.0).then(|| ctx.content_rect().bottom() - lift)
}

/// Whether layer `l` is listed in the layers panel: off a timeline (`root` none), every layer; on
/// one, those of drawing `root` (itself and everything inside it), the drawing at the playhead on
/// the chosen track (the core's `timeline_drawing_shown`).
pub fn listed(layers: &[LayerInfo], l: &LayerInfo, root: Option<u64>) -> bool {
    let Some(root) = root else { return true };
    let mut at = Some(l.id);
    for _ in 0..=layers.len() {
        let Some(id) = at else { return false };
        if id == root {
            return true;
        }
        at = layers.iter().find(|x| x.id == id).and_then(|x| x.parent);
    }
    false
}

impl TimelineGrid {
    /// Whether the artwork is on a timeline.
    pub fn on(session: &CanvasSession) -> bool {
        session.timeline_info().is_some()
    }

    /// The artwork put on a timeline (Animation Assist's frames becoming its drawings), or taken
    /// off it (the timeline kept for when it goes back on): one undo step.
    pub fn set_on(&mut self, session: &CanvasSession, on: bool) {
        self.stop(session, false);
        if on {
            session.start_timeline();
        } else {
            session.stop_timeline();
        }
        self.read_at = None;
    }

    /// Playing stops (a screen covers the bar, or the canvas goes).
    pub fn reset(&mut self, session: &CanvasSession) {
        self.stop(session, false);
        self.motion.let_go();
        self.let_go_edits();
        self.keys.cancel_drag();
        self.keys.menu = None;
        self.animate.armed = false;
    }

    /// Its menu's keypad or a name being typed takes the keyboard's keys.
    pub fn takes_keys(&self) -> bool {
        self.menu.as_ref().is_some_and(menus::Menu::takes_keys)
    }

    /// Everything being dragged or open let go: the menu, an edge, the stroke, a block moving.
    fn let_go_edits(&mut self) {
        self.menu = None;
        self.cards.clear();
        self.edit = edges::Edit::default();
        self.stroke = grid::Lasso::default();
        self.moving = None;
        self.length_draft = None;
    }

    /// The rows laid from the panel as last read, with the tracks' rows of keys out now.
    fn lay_rows(&mut self) {
        self.rows = Arc::new(match self.panel.as_ref() {
            Some(panel) => rows::GridRows::new(panel, &self.keys.out),
            None => rows::GridRows::default(),
        });
    }

    /// What is picked, or has a menu, that the timeline no longer has, let go.
    fn forget_gone(&mut self) {
        if let Some(p) = self.picked
            && self.clip(p).is_none()
        {
            self.picked = None;
        }
        let rows = Arc::clone(&self.rows);
        let has = |id: u64| rows.tracks().any(|(_, s)| s.track().clips.iter().any(|c| c.id == id));
        if let Some(chosen) = self.selecting.as_mut() {
            chosen.retain(|id| has(*id));
        }
        let gone = match self.menu.as_ref().map(|m| &m.kind) {
            Some(menus::Kind::Frame { clip } | menus::Kind::Block { clip }) => !has(*clip),
            Some(menus::Kind::Track { track } | menus::Kind::Empty { track, .. }) => rows.segment_of(*track).is_none(),
            Some(menus::Kind::Picked) => self.selecting.as_ref().is_none_or(BTreeSet::is_empty),
            _ => false,
        };
        if gone {
            self.menu = None;
        }
    }

    /// The timeline as last read: none off one.
    fn info(&self) -> Option<&TimelineInfo> {
        self.panel.as_ref()?.info.as_ref()
    }

    /// The frame in the middle: the one playing, else the playhead's.
    pub fn frame(&self) -> u32 {
        self.playing.as_ref().map(|p| p.at).or_else(|| self.info().map(|i| i.frame)).unwrap_or(0)
    }

    #[cfg(test)]
    pub(crate) fn is_playing(&self) -> bool {
        self.playing.is_some()
    }

    /// Reads the timeline again where it has changed (the artwork's edits, the playhead or the
    /// chosen track): false off a timeline, everything it showed let go.
    fn read(&mut self, ctx: &egui::Context, session: &CanvasSession) -> bool {
        let address = std::ptr::from_ref(session) as usize;
        if address != self.session {
            let curves = self.keys.curves.take();
            // another canvas: nothing read of the last one holds
            *self = TimelineGrid {
                settings_asked: self.settings_asked,
                session: address,
                perform_smoothing: self.perform_smoothing,
                perform_hidden: self.perform_hidden,
                my_curves_hidden: self.my_curves_hidden,
                ..TimelineGrid::default()
            };
            self.keys.curves = curves;
        }
        let Some(info) = session.timeline_info() else {
            if self.panel.is_some() || self.playing.is_some() {
                self.stop(session, false);
                self.panel = None;
                self.rows = Arc::default();
                self.pictures.clear();
                self.let_go_edits();
                self.read_at = None;
                self.motion.let_go();
                self.keys.picks.clear();
                self.keys.cancel_drag();
                self.keys.menu = None;
                self.keys_card_for = None;
                self.animate.armed = false;
            }
            return false;
        };
        let edits = session.edits();
        if self.read_at.as_ref().is_none_or(|(e, i)| *e != edits || *i != info) {
            if let Some(panel) = session.timeline_panel(self.known) {
                self.known = panel.generation;
                self.keys.refreshed(&panel);
                self.panel = Some(panel);
                self.lay_rows();
                self.forget_gone();
            }
            self.read_at = Some((edits, info));
        }
        // (a track's rows of keys put out or away: the rows laid again)
        if self.rows.keys_out != self.keys.out {
            self.lay_rows();
        }
        self.read_pictures(ctx, session);
        self.panel.as_ref().is_some_and(|p| p.info.is_some())
    }

    /// Draws again the pictures of the drawings whose version has changed since theirs was drawn
    /// (or that have none yet), a few a frame; the others are let go.
    fn read_pictures(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let Some(panel) = self.panel.as_ref() else { return };
        self.pictures.retain(|root, _| panel.pictures.iter().any(|p| p.root == *root));
        let stale: Vec<_> = panel
            .pictures
            .iter()
            .filter(|p| self.pictures.get(&p.root).is_none_or(|(v, _)| *v != p.version))
            .copied()
            .collect();
        for p in stale.iter().take(PICTURES_A_FRAME) {
            let texture = session.frame_thumbnail(p.root, PICTURE_SIDE).map(|t| {
                let size = [t.width as usize, t.height as usize];
                let image = egui::ColorImage::from_rgba_premultiplied(size, &t.rgba);
                ctx.load_texture(format!("timeline drawing {}", p.root), image, egui::TextureOptions::LINEAR)
            });
            self.pictures.insert(p.root, (p.version, texture));
        }
        if stale.len() > PICTURES_A_FRAME {
            ctx.request_repaint();
        }
    }

    /// The playhead on `frame` (an empty frame a little past the end too): a layer of the drawing
    /// there on the chosen track becomes current. Playing stops.
    fn go_to(&mut self, session: &CanvasSession, frame: u32) {
        self.stop(session, false);
        session.rest_timeline_frame(frame);
    }

    /// One frame on or back from the playhead (an empty one past the end too).
    fn step_frame(&mut self, session: &CanvasSession, by: i64) {
        let Some(info) = self.info() else { return };
        let last = i64::from(info.duration.saturating_sub(1) + PAST_END);
        let to = (i64::from(self.frame()) + by).clamp(0, last);
        if to != i64::from(self.frame()) {
            self.go_to(session, u32::try_from(to).unwrap_or(0));
        }
    }

    /// The playhead to the start of the drawing before or after it on the chosen track (a group
    /// counts as a drawing; sounds and videos don't).
    fn step_drawing(&mut self, session: &CanvasSession, by: i64) {
        let Some(panel) = self.panel.as_ref() else { return };
        let Some(info) = panel.info.as_ref() else { return };
        let Some(row) = panel.rows.iter().find(|r| Some(r.track.id) == info.track).or_else(|| panel.rows.first())
        else {
            return;
        };
        let at = self.frame();
        let starts = row
            .track
            .clips
            .iter()
            .filter(|c| c.kind != TimelineClipKind::Audio && c.kind != TimelineClipKind::Video)
            .map(|c| c.start);
        let next = if by > 0 { starts.filter(|s| *s > at).min() } else { starts.filter(|s| *s < at).max() };
        if let Some(next) = next {
            self.go_to(session, next);
        }
    }

    /// The chosen track drawn on from now on.
    fn select_track(&mut self, session: &CanvasSession, id: u64) {
        if self.info().is_some_and(|i| i.track != Some(id)) {
            session.select_timeline_track(id);
        }
    }

    /// A new frame (a new drawing) at the playhead on the chosen track, the frames after it moving
    /// along: what to tell when it can't be made.
    fn add_frame(&mut self, session: &CanvasSession) -> Option<&'static str> {
        self.stop(session, false);
        if session.add_timeline_drawing() != 0 {
            return None;
        }
        let locked = self.panel.as_ref().is_some_and(|p| {
            let track = p.info.as_ref().and_then(|i| i.track);
            p.rows.iter().any(|r| Some(r.track.id) == track && r.track.locked)
        });
        Some(if locked { "The track is locked." } else { "There's no room for another layer." })
    }

    /// A new empty track on top, drawn on.
    fn add_track(&mut self, session: &CanvasSession) {
        self.stop(session, false);
        session.add_timeline_track();
    }

    /// Plays round frames `first..end` from the playhead's (from the first, where it is outside
    /// them), the grid holding still at `held`.
    fn play(&mut self, ctx: &egui::Context, session: &CanvasSession, first: u32, end: u32, held: f32) {
        let Some(info) = self.info() else { return };
        if info.duration == 0 || first >= end {
            return;
        }
        let fps = f64::from(info.fps.max(1));
        let here = info.frame;
        let at = if (first..end).contains(&here) { here } else { first };
        let now = ctx.input(|i| i.time);
        session.set_timeline_playing(Some(at));
        self.playing = Some(Playing { first, end, at, step: 1, due: now + 1.0 / fps, held });
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(1.0 / fps));
    }

    /// Playing goes on: the next frame shows when it is due; a single pass ended, it stops on its
    /// last frame.
    fn play_on(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        let fps = f64::from(self.info().map_or(1, |i| i.fps.max(1)));
        let Some(p) = self.playing.as_mut() else { return };
        let now = ctx.input(|i| i.time);
        if now < p.due {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(p.due - now));
            return;
        }
        match session.timeline_next_step_within(p.at, p.step, p.first, p.end) {
            Some(next) => {
                p.at = next.at;
                p.step = next.step;
                // on time by the clock, unless it has fallen a frame behind
                p.due = if now - p.due > 1.0 / fps { now + 1.0 / fps } else { p.due + 1.0 / fps };
                session.set_timeline_playing(Some(p.at));
                ctx.request_repaint_after(std::time::Duration::from_secs_f64((p.due - now).max(0.0)));
            }
            None => self.stop(session, true),
        }
    }

    /// Playing stops; with `stay`, the playhead stays on the frame it stopped on.
    pub(crate) fn stop(&mut self, session: &CanvasSession, stay: bool) {
        let Some(p) = self.playing.take() else { return };
        session.set_timeline_playing(None);
        if stay {
            session.rest_timeline_frame(p.at);
        }
        self.motion.stopped_playing(p.held);
    }
}

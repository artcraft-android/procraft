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
//! Keys (keys.rs): a track's rows of keys under it, their marks picked, dragged, stretched and
//! eased; a block's Keys card (keys_card.rs); Animate in the header, keying with the transform tool,
//! and Perform (animate.rs); the motion path over the canvas (motion_path.rs).
//!
//! Not here yet (the iPad has them): the lasso, Add new, the menus on a hold, the frame picked's
//! lengthening handles, moving blocks and zoom.

mod animate;
mod grid;
mod keys;
mod keys_card;
mod motion_path;

pub use animate::bar_top;

use std::collections::HashMap;

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
        self.keys.cancel_drag();
        self.keys.menu = None;
        self.animate.armed = false;
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
                ..TimelineGrid::default()
            };
            self.keys.curves = curves;
        }
        let Some(info) = session.timeline_info() else {
            if self.panel.is_some() || self.playing.is_some() {
                self.stop(session, false);
                self.panel = None;
                self.pictures.clear();
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
            }
            self.read_at = Some((edits, info));
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

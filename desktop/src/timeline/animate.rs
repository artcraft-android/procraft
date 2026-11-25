//! Animate and Perform, as the iPad's (TimelineAutokey.swift, `GridPerformOverlay` in
//! TimelineGridKeys.swift).
//!
//! **Animate** in the header is autokey: with it on, the transform tool on a timeline keys the
//! pose of what is under the playhead on the chosen track rather than moving its pixels (the
//! core's `timeline::autokey`). The app opens the transform as it goes on and ends it as it goes
//! off ([`TimelineGrid::animate_asked`]); the transform's toolbar stands above the timeline's bar
//! ([`bar_top`]). While the transform keys a pose, a red badge over the canvas says the frame being
//! keyed; clicked ("Turn Off"), autokey goes off. Moving the playhead lays the transform's box
//! again over what is there ([`TimelineGrid::autokey_box`]). New keys ease as the Keys card (or a
//! key's menu) last chose.
//!
//! **Perform**: with Animate on, a record button (a red dot, ringed while armed) after Play arms a
//! take. The next press on the canvas starts it: the frames play on from the playhead, one at a
//! time at the timeline's rate, while the block under the playhead on the chosen track follows the
//! pointer, keyed on each frame (`perform_timeline_pose`); letting go (or the block's last frame)
//! ends it, evened out by Settings → Timeline → Perform smoothing and thinned to the keys that keep
//! its path (`perform_timeline_end`), all one "Perform" undo step, and the track's rows of keys
//! come out. The iPad's second finger (bigger or smaller, and turned) is **Shift** held during the
//! take: the block stays where it is while the pointer, gone round its middle or nearer to it or
//! farther, turns it and sizes it as a second finger would; let go of Shift, it moves on from there.
//! A trackpad's pinch and turn (or two fingers on a touch screen) size and turn it as on the iPad.

use egui::{Align2, Color32, Id, Order, Pos2, Rect, Sense, Stroke, pos2, vec2};
use procraft_core::{CanvasSession, TimelineClipKind, TimelinePose, TransformBox};

use super::TimelineGrid;
use super::grid::Place;
use crate::look::{Weight, text, text_width};

/// Autokey's colour, a recording light's.
pub(super) const RED: Color32 = Color32::from_rgb(0xFF, 0x45, 0x3A);

/// Animate's state: autokey on, a take armed, the take under way, and the frame the transform's
/// box was last laid at.
#[derive(Default)]
pub(crate) struct Animate {
    pub(crate) autokey: bool,
    pub(crate) armed: bool,
    take: Option<Take>,
    followed: Option<u32>,
}

/// A take under way: the block, how it was as the press began, the frames it has keyed so far and
/// the last it may key, the frame it is on and when the next is due; how far the pointer has moved
/// it (canvas pixels), and how many times as big and how far turned (degrees) it has made it.
struct Take {
    clip: u64,
    pose: TimelinePose,
    first: u32,
    last: u32,
    end: u32,
    at: u32,
    due: f64,
    moved: [f32; 2],
    scale: Option<f32>,
    turn: Option<f32>,
    /// Where the pointer was last frame.
    pointer: Pos2,
    /// With Shift down: the block's middle (window points), and the pointer's distance from it and
    /// angle about it as Shift went down, with the size and turn it had then.
    shift: Option<(Pos2, f32, f32, f32, f32)>,
}

impl Animate {
    pub(crate) fn performing(&self) -> bool {
        self.take.is_some()
    }
}

fn bar_top_id() -> Id {
    Id::unique("timeline bar top")
}

/// The timeline's bar was drawn this frame with its top at `top`.
pub(super) fn keep_bar_top(ctx: &egui::Context, top: f32) {
    let frame = ctx.cumulative_frame_nr();
    ctx.data_mut(|d| d.insert_temp(bar_top_id(), (top, frame)));
}

/// The top of the timeline's bar (window points), where it showed this frame or the last: the
/// toolbars at the window's foot stand above it.
pub fn bar_top(ctx: &egui::Context) -> Option<f32> {
    let frame = ctx.cumulative_frame_nr();
    let (top, at) = ctx.data(|d| d.get_temp::<(f32, u64)>(bar_top_id()))?;
    (at + 1 >= frame).then_some(top)
}

/// The record button's glyph: a red dot, ringed while a take is armed.
pub(super) fn record_glyph(painter: &egui::Painter, at: Pos2, armed: bool) {
    painter.circle_filled(at, 7.5, RED);
    if armed {
        painter.circle_stroke(at, 12.5, Stroke::new(2.0, RED));
    }
}

/// Animate's glyph: a key's diamond with a smaller one trailing it, 27 points across (filled
/// while on).
pub(super) fn animate_glyph(painter: &egui::Painter, at: Pos2, ink: Color32, on: bool) {
    let diamond = |c: Pos2, d: f32| vec![c - vec2(0.0, d), c + vec2(d, 0.0), c + vec2(0.0, d), c - vec2(d, 0.0)];
    let big = diamond(at + vec2(3.0, 0.0), 8.0);
    let fill = if on { ink } else { Color32::TRANSPARENT };
    painter.add(egui::Shape::convex_polygon(big, fill, Stroke::new(1.8, ink)));
    let small = diamond(at - vec2(9.0, 0.0), 3.5);
    painter.add(egui::Shape::closed_line(small, Stroke::new(1.5, ink)));
}

impl TimelineGrid {
    /// Autokey on or off for this canvas (its new keys easing as the Keys card chose): a take
    /// armed is let go, playing stops. The app opens or ends the transform tool.
    pub fn set_autokey(&mut self, session: &CanvasSession, on: bool) {
        self.stop(session, false);
        self.animate.autokey = on;
        self.animate.armed = false;
        self.animate.followed = None;
        session.set_timeline_autokey(on, self.keys.easing);
    }

    /// While the transform keys a pose: the box laid again over what the playhead is on, once each
    /// time it moves (the app gives it to the transform).
    pub fn autokey_box(&mut self, session: &CanvasSession) -> Option<TransformBox> {
        if !session.timeline_autokey_posing() {
            self.animate.followed = None;
            return None;
        }
        let frame = session.timeline_info()?.frame;
        if self.animate.followed == Some(frame) {
            return None;
        }
        self.animate.followed = Some(frame);
        session.timeline_autokey_box()
    }

    /// While the transform keys a pose with autokey on: the badge over the canvas saying the frame
    /// it keys; clicked, autokey goes off.
    pub(super) fn autokey_badge(&mut self, ctx: &egui::Context, session: &CanvasSession) {
        if !self.animate.autokey || !session.timeline_autokey_posing() {
            return;
        }
        let said = format!("Keying frame {}", self.frame() + 1);
        let screen = ctx.content_rect();
        let top = crate::panel::bar_shown(ctx) + 10.0;
        egui::Area::new(Id::unique("autokey badge")).fixed_pos(screen.min).order(Order::Foreground).show(ctx, |ui| {
            let words = text_width(ui.painter(), &said, 14.0, Weight::Bold);
            let off = text_width(ui.painter(), "Turn Off", 14.0, Weight::Regular);
            let width = 12.0 + 7.0 + 7.0 + words + 14.0 + off + 12.0;
            let badge = Rect::from_min_size(pos2(screen.center().x - width / 2.0, top), vec2(width, 28.0));
            let response = ui.allocate_rect(badge, Sense::click());
            crate::screen::claim(ui, badge);
            let painter = ui.painter();
            painter.rect_filled(badge, 14, RED);
            let y = badge.center().y;
            painter.circle_filled(pos2(badge.left() + 15.5, y), 3.5, Color32::WHITE);
            let x = badge.left() + 26.0;
            text(painter, pos2(x, y), Align2::LEFT_CENTER, &said, 14.0, Weight::Bold, Color32::WHITE);
            let rule = x + words + 7.0;
            painter.line_segment(
                [pos2(rule, y - 7.0), pos2(rule, y + 7.0)],
                Stroke::new(1.0, Color32::from_white_alpha(128)),
            );
            text(painter, pos2(rule + 7.0, y), Align2::LEFT_CENTER, "Turn Off", 14.0, Weight::Regular, Color32::WHITE);
            if response.clicked() {
                self.animate_asked = Some(false);
            }
        });
    }

    /// The block on the chosen track at the playhead (a group, a sound or a video too).
    pub(super) fn current_clip(&self) -> Option<&procraft_core::TimelineClipInfo> {
        let panel = self.panel.as_ref()?;
        let info = panel.info.as_ref()?;
        let row = panel.rows.iter().find(|r| Some(r.track.id) == info.track)?;
        let at = info.frame;
        row.track.clips.iter().find(|c| c.start <= at && at < c.start + c.duration)
    }

    // MARK: Perform

    /// While a take is armed: the canvas takes the press that starts it, and the take plays on
    /// until it is let go. Returns what to tell.
    pub(super) fn perform(
        &mut self,
        ctx: &egui::Context,
        session: &CanvasSession,
        place: &Place,
    ) -> Option<&'static str> {
        if !self.animate.armed || !self.animate.autokey {
            if let Some(take) = self.animate.take.take() {
                self.end_take(session, take);
            }
            return None;
        }
        let screen = ctx.content_rect();
        let canvas = Rect::from_min_max(
            pos2(screen.left(), screen.top() + crate::panel::bar_shown(ctx)),
            pos2(screen.right(), place.bar.top() - 4.0),
        );
        if crate::ui::escape(ctx) && self.animate.take.is_none() {
            self.animate.armed = false;
            return None;
        }
        let mut said = None;
        let now = ctx.input(|i| i.time);
        egui::Area::new(Id::unique("timeline perform")).fixed_pos(canvas.min).order(Order::Middle).show(ctx, |ui| {
            let response = ui.allocate_rect(canvas, Sense::click_and_drag());
            crate::screen::claim(ui, canvas);
            let (pointer, shift, pinch) =
                ui.input(|i| (i.pointer.latest_pos(), i.modifiers.shift, (i.zoom_delta(), i.rotation_delta())));
            let down = response.is_pointer_button_down_on();
            if down && self.animate.take.is_none() {
                said = self.begin_take(session, pointer.unwrap_or(canvas.center()), now);
            }
            if let Some(mut take) = self.animate.take.take() {
                if let Some(p) = pointer {
                    self.follow(ctx, session, &mut take, p, shift, pinch);
                }
                let ended = !down || self.key_take(ctx, session, &mut take, now);
                if ended {
                    self.end_take(session, take);
                } else {
                    self.animate.take = Some(take);
                    ui.ctx().request_repaint();
                }
            }
            // what to do, over the canvas's top
            let words = if self.animate.take.is_some() {
                "Performing…"
            } else {
                "Drag on the canvas to perform the move (Shift: scale and turn)"
            };
            let painter = ui.painter();
            let width = text_width(painter, words, 14.0, Weight::Bold) + 24.0;
            let top = canvas.top() + 46.0;
            let hint = Rect::from_min_size(pos2(canvas.center().x - width / 2.0, top), vec2(width, 28.0));
            painter.rect_filled(hint, 14, RED);
            text(painter, hint.center(), Align2::CENTER_CENTER, words, 14.0, Weight::Bold, Color32::WHITE);
        });
        said
    }

    /// The press that starts a take, at `at`: the block under the playhead on the chosen track,
    /// as it is there now. Returns what to tell when there is none.
    fn begin_take(&mut self, session: &CanvasSession, at: Pos2, now: f64) -> Option<&'static str> {
        let clip = self.current_clip().filter(|c| c.kind != TimelineClipKind::Audio).cloned();
        let Some((clip, pose)) = clip.and_then(|c| session.timeline_pose(c.id).map(|p| (c, p))) else {
            self.animate.armed = false;
            return Some("Put the playhead on the frame to move first.");
        };
        self.stop(session, false);
        let first = self.frame();
        self.animate.take = Some(Take {
            clip: clip.id,
            pose,
            first,
            last: first,
            end: clip.start + clip.duration.max(1) - 1,
            at: first,
            due: now,
            moved: [0.0, 0.0],
            scale: None,
            turn: None,
            pointer: at,
            shift: None,
        });
        None
    }

    /// The pointer at `p` (window points): with Shift up, the block moves as far as it has; with
    /// it down, the block is sized and turned about its middle as the pointer goes. A pinch and
    /// turn (`pinch`: this frame's zoom and turn, radians) sizes and turns it too.
    fn follow(
        &self,
        ctx: &egui::Context,
        session: &CanvasSession,
        take: &mut Take,
        p: Pos2,
        shift: bool,
        pinch: (f32, f32),
    ) {
        let ppp = ctx.pixels_per_point();
        let m = session.view_matrix();
        let get = |i: usize| m.get(i).copied().unwrap_or(0.0);
        let (a, b, c, d, e, f) = (get(0), get(1), get(2), get(3), get(4), get(5));
        let det = a * d - b * c;
        let (zoom, turn) = pinch;
        if (zoom - 1.0).abs() > 1e-4 && zoom.is_finite() {
            take.scale = Some(take.scale.unwrap_or(1.0) * zoom);
        }
        if turn.abs() > 1e-5 && turn.is_finite() {
            take.turn = Some(take.turn.unwrap_or(0.0) + turn.to_degrees());
        }
        if shift {
            if take.shift.is_none() {
                // its middle: where its position key is on the frame keyed last, on the window
                let path = session.timeline_motion_path(take.clip);
                let key = path.and_then(|p| p.keys.into_iter().min_by_key(|k| k.frame.abs_diff(take.last)));
                let Some(key) = key else { return };
                let middle = pos2(a * key.x + c * key.y + e, b * key.x + d * key.y + f) / ppp;
                let from = p - middle;
                take.shift = Some((
                    middle,
                    from.length().max(8.0),
                    from.y.atan2(from.x),
                    take.scale.unwrap_or(1.0),
                    take.turn.unwrap_or(0.0),
                ));
            }
            if let Some((middle, spread, angle, scale, turn)) = take.shift {
                let now = p - middle;
                take.scale = Some(scale * now.length().max(1.0) / spread);
                let mut step = (now.y.atan2(now.x) - angle).to_degrees();
                while step > 180.0 {
                    step -= 360.0;
                }
                while step < -180.0 {
                    step += 360.0;
                }
                take.turn = Some(turn + step);
            }
        } else {
            take.shift = None;
            if det.abs() > f32::EPSILON {
                let (dx, dy) = ((p.x - take.pointer.x) * ppp, (p.y - take.pointer.y) * ppp);
                take.moved[0] += (d * dx - c * dy) / det;
                take.moved[1] += (-b * dx + a * dy) / det;
            }
        }
        take.pointer = p;
    }

    /// The take's frame keyed when it is due, and the next one shown: true when it has reached the
    /// block's last frame.
    fn key_take(&self, ctx: &egui::Context, session: &CanvasSession, take: &mut Take, now: f64) -> bool {
        let fps = f64::from(self.info().map_or(12, |i| i.fps.max(1)));
        if now < take.due {
            ctx.request_repaint_after(std::time::Duration::from_secs_f64(take.due - now));
            return false;
        }
        let p = &take.pose;
        session.perform_timeline_pose(
            take.clip,
            take.at,
            p.x + take.moved[0],
            p.y + take.moved[1],
            take.scale.map(|s| p.scale_x * s),
            take.turn.map(|t| p.rotation + t),
        );
        take.last = take.at;
        session.set_timeline_playing(Some(take.at));
        if take.at >= take.end {
            return true;
        }
        take.at += 1;
        take.due = if now - take.due > 1.0 / fps { now + 1.0 / fps } else { take.due + 1.0 / fps };
        false
    }

    /// The take ends: evened out and thinned, one "Perform" undo step; the playhead rests on its
    /// last frame, and the track's rows of keys come out.
    fn end_take(&mut self, session: &CanvasSession, take: Take) {
        let info = session.info();
        let tolerance = info.width.max(info.height) as f32 / 500.0;
        let smoothing = self.perform_smoothing.clamp(0.0, 1.0);
        session.perform_timeline_end(take.clip, take.first, take.last, tolerance, smoothing);
        session.set_timeline_playing(None);
        session.rest_timeline_frame(take.last);
        self.animate.armed = false;
        let track = self
            .panel
            .as_ref()
            .and_then(|p| p.rows.iter().find(|r| r.track.clips.iter().any(|c| c.id == take.clip)).map(|r| r.track.id));
        if let Some(t) = track {
            self.keys.out.insert(t);
        }
        self.read_at = None;
    }
}

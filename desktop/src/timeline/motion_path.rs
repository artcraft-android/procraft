//! The motion path over the canvas, as the iPad's (`MotionPathOverlay` in TimelineBar.swift): for
//! the block under the playhead on the chosen track, while Animate is on or that track's rows of
//! keys are out, the path its pivot takes, a dot at each position key, and a bent key's two handles
//! on lines to it.
//!
//! A key's dot clicked goes to its frame; dragged, the key moves there (`timeline_motion_delta`
//! counting the move through the groups it is in), one "Key" undo step let go. A handle's dot
//! dragged bends the path, the other mirroring it (`set_timeline_path_handles`), one "Motion Path"
//! step. Only the dots take presses: the canvas keeps the rest. On desktop a key's dot
//! double-clicked bends the path there (handles a sixth of the way to its neighbours) or, bent,
//! makes it a corner again: the iPad has no way to bend a key outside a take.

use egui::{Color32, Id, LayerId, Order, Pos2, Rect, Sense, Shape, Stroke, pos2, vec2};
use procraft_core::{CanvasSession, MotionPath, PathKey, TimelineChannel, TimelinePose};

use super::TimelineGrid;
use super::grid::Place;
use crate::look::{argb, pc};

/// A dot's reach (points across).
const REACH: f32 = 40.0;

/// A drag on the path under way: the path as it changes it, and what is dragged.
#[derive(Default)]
pub(super) struct PathDrag {
    live: Option<MotionPath>,
    dragging: Option<Dragging>,
}

/// A dot dragged: its key's frame, which dot (none: the key's own; 0 or 1: a handle), where the
/// press began (canvas pixels), and how the block was then.
#[derive(Clone)]
struct Dragging {
    frame: u32,
    handle: Option<usize>,
    start: [f32; 2],
    pose: Option<TimelinePose>,
}

/// The view's matrix as a function from canvas pixels to window points, and back.
struct View {
    m: [f32; 6],
    ppp: f32,
}

impl View {
    fn of(ctx: &egui::Context, session: &CanvasSession) -> View {
        let m = session.view_matrix();
        let get = |i: usize| m.get(i).copied().unwrap_or(0.0);
        View { m: [get(0), get(1), get(2), get(3), get(4), get(5)], ppp: ctx.pixels_per_point() }
    }

    fn window(&self, x: f32, y: f32) -> Pos2 {
        let [a, b, c, d, e, f] = self.m;
        pos2(a * x + c * y + e, b * x + d * y + f) / self.ppp
    }

    fn canvas(&self, p: Pos2) -> Option<[f32; 2]> {
        let [a, b, c, d, e, f] = self.m;
        let det = a * d - b * c;
        if det.abs() <= f32::EPSILON {
            return None;
        }
        let (x, y) = (p.x * self.ppp - e, p.y * self.ppp - f);
        Some([(d * x - c * y) / det, (-b * x + a * y) / det])
    }
}

/// A white dot ringed in the accent colour (filled with it on the playhead's key).
fn dot(painter: &egui::Painter, at: Pos2, r: f32, current: bool) {
    painter.circle_filled(at, r, argb(pc::ACCENT));
    painter.circle_filled(at, r - 2.0, if current { argb(pc::ACCENT) } else { Color32::WHITE });
}

impl TimelineGrid {
    /// The block whose path shows: the one under the playhead on the chosen track, while Animate
    /// is on or that track's rows of keys are out (not while playing or performing).
    fn path_clip(&self) -> Option<u64> {
        if self.playing.is_some() || self.animate.performing() {
            return None;
        }
        let track = self.info()?.track?;
        if !(self.animate.autokey || self.keys.out.contains(&track)) {
            return None;
        }
        self.current_clip().map(|c| c.id)
    }

    /// The path over the canvas, its dots taking presses.
    pub(super) fn motion_path(&mut self, ctx: &egui::Context, session: &CanvasSession, place: &Place) {
        let Some(id) = self.path_clip() else {
            self.path = PathDrag::default();
            return;
        };
        let Some(path) = self.path.live.clone().or_else(|| session.timeline_motion_path(id)) else { return };
        if path.keys.is_empty() {
            return;
        }
        let view = View::of(ctx, session);
        let here = self.frame();
        // (kept off the timeline's bar)
        let screen = ctx.content_rect();
        let clip = Rect::from_min_max(screen.min, pos2(screen.right(), place.bar.top()));
        let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::unique("motion path"))).with_clip_rect(clip);
        let line: Vec<Pos2> = path.points.as_chunks::<2>().0.iter().map(|[x, y]| view.window(*x, *y)).collect();
        painter.add(Shape::line(line.clone(), Stroke::new(4.0, Color32::from_white_alpha(178))));
        painter.add(Shape::line(line, Stroke::new(2.0, argb(pc::ACCENT))));
        for key in &path.keys {
            let at = view.window(key.x, key.y);
            if let Some([a, b, c, d]) = key.handles.as_deref().and_then(|h| <[f32; 4]>::try_from(h).ok()) {
                for (dx, dy) in [(a, b), (c, d)] {
                    let end = view.window(key.x + dx, key.y + dy);
                    painter.line_segment([at, end], Stroke::new(1.5, argb(pc::ACCENT)));
                    dot(&painter, end, 5.0, false);
                }
            }
            dot(&painter, at, 6.5, key.frame == here);
        }
        // a take armed takes the canvas's presses, these among them
        if self.animate.armed {
            return;
        }
        for key in &path.keys {
            let at = view.window(key.x, key.y);
            if let Some([a, b, c, d]) = key.handles.as_deref().and_then(|h| <[f32; 4]>::try_from(h).ok()) {
                for (handle, (dx, dy)) in [(a, b), (c, d)].into_iter().enumerate() {
                    let end = view.window(key.x + dx, key.y + dy);
                    if let Some((p, done, _, _)) = grip(ctx, clip, end, ("handle", key.frame, handle)) {
                        self.handle_dragged(session, &view, id, key, handle, p, done);
                    }
                }
            }
            if let Some((p, done, clicked, twice)) = grip(ctx, clip, at, ("key", key.frame, 2)) {
                if twice {
                    self.bend(session, id, &path, key.frame);
                } else if clicked {
                    if key.frame != self.frame() {
                        self.go_to(session, key.frame);
                        self.read_at = None;
                    }
                } else {
                    self.key_dragged(session, &view, id, key, p, done);
                }
            }
        }
    }

    /// Key `key`'s dot dragged to window point `p`: the playhead goes to its frame, and its
    /// position key moves as far as the drag has gone; one undo step when `done`.
    #[allow(clippy::too_many_arguments)]
    fn key_dragged(&mut self, session: &CanvasSession, view: &View, id: u64, key: &PathKey, p: Pos2, done: bool) {
        let Some(at) = view.canvas(p) else { return };
        if self.path.dragging.is_none() {
            if key.frame != self.frame() {
                self.go_to(session, key.frame);
            }
            let pose = session.timeline_pose(id);
            self.path.dragging = Some(Dragging { frame: key.frame, handle: None, start: at, pose });
        }
        let Some(Dragging { frame, handle: None, start, pose: Some(pose) }) = self.path.dragging.clone() else {
            return;
        };
        let by = session.timeline_motion_delta(id, frame, at[0] - start[0], at[1] - start[1]);
        let values = vec![pose.x + by.first().copied().unwrap_or(0.0), pose.y + by.get(1).copied().unwrap_or(0.0)];
        let easing = self.easing_at(id, frame);
        session.set_timeline_key(id, TimelineChannel::Position, values, easing, done);
        self.settle(session, id, done);
    }

    /// Handle `handle` (0 toward the key before, 1 toward the key after) of key `key` dragged to
    /// window point `p`; the other mirrors it. One undo step when `done`.
    #[allow(clippy::too_many_arguments)]
    fn handle_dragged(
        &mut self,
        session: &CanvasSession,
        view: &View,
        id: u64,
        key: &PathKey,
        handle: usize,
        p: Pos2,
        done: bool,
    ) {
        let Some(at) = view.canvas(p) else { return };
        if self.path.dragging.is_none() {
            self.path.dragging = Some(Dragging { frame: key.frame, handle: Some(handle), start: at, pose: None });
        }
        let (dx, dy) = (at[0] - key.x, at[1] - key.y);
        let handles = if handle == 0 { vec![dx, dy, -dx, -dy] } else { vec![-dx, -dy, dx, dy] };
        session.set_timeline_path_handles(id, key.frame, Some(handles), done);
        self.settle(session, id, done);
    }

    /// Key `frame` of the path bent (handles a sixth of the way toward the keys either side), or
    /// made a corner again where it was bent: one undo step.
    fn bend(&mut self, session: &CanvasSession, id: u64, path: &MotionPath, frame: u32) {
        let keys = &path.keys;
        let Some(i) = keys.iter().position(|k| k.frame == frame) else { return };
        let bent = keys.get(i).is_some_and(|k| k.handles.is_some());
        let handles = if bent {
            None
        } else {
            let (Some(before), Some(after)) = (keys.get(i.saturating_sub(1)), keys.get((i + 1).min(keys.len() - 1)))
            else {
                return;
            };
            let (mut dx, mut dy) = ((after.x - before.x) / 6.0, (after.y - before.y) / 6.0);
            if dx == 0.0 && dy == 0.0 {
                // a lone key: handles across, an eighth of the canvas
                dx = session.info().width as f32 / 8.0;
                dy = 0.0;
            }
            Some(vec![-dx, -dy, dx, dy])
        };
        session.set_timeline_path_handles(id, frame, handles, true);
        self.path = PathDrag::default();
        self.read_at = None;
    }

    /// How clip `id`'s position key at `frame` eases (kept as it is moved).
    fn easing_at(&self, id: u64, frame: u32) -> procraft_core::TimelineEasing {
        let keys = self.panel.as_ref().and_then(|p| p.keys.iter().find(|k| k.id == id));
        keys.and_then(|k| k.keys.iter().find(|k| k.frame == frame && k.channel == TimelineChannel::Position))
            .map_or(self.keys.easing, |k| k.easing)
    }

    /// The path read again as a drag changes it; let go, back to the timeline's own.
    fn settle(&mut self, session: &CanvasSession, id: u64, done: bool) {
        if done {
            self.path = PathDrag::default();
            self.read_at = None;
        } else {
            self.path.live = session.timeline_motion_path(id);
        }
    }
}

/// A dot's press area at `at` (kept within `clip`): where a press on it is (window points), and
/// whether it was let go, clicked, or double-clicked.
fn grip(ctx: &egui::Context, clip: Rect, at: Pos2, id: (&str, u32, usize)) -> Option<(Pos2, bool, bool, bool)> {
    let rect = Rect::from_center_size(at, vec2(REACH, REACH));
    if !clip.intersects(rect) {
        return None;
    }
    let mut out = None;
    egui::Area::new(Id::unique("motion path dot").with(id)).fixed_pos(rect.min).order(Order::Middle).show(ctx, |ui| {
        let response = ui.allocate_rect(rect, Sense::click_and_drag());
        crate::screen::claim(ui, rect);
        let p = response.interact_pointer_pos().or(ui.input(|i| i.pointer.latest_pos())).unwrap_or(at);
        if response.double_clicked() {
            out = Some((p, true, false, true));
        } else if response.clicked() {
            out = Some((p, true, true, false));
        } else if response.drag_stopped() {
            out = Some((p, true, false, false));
        } else if response.dragged() {
            out = Some((p, false, false, false));
        }
    });
    out
}

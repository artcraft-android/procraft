//! How the brush cursor's outline turns, and how big it is while painting: as the stamps it
//! stands for.
//!
//! While a stroke is painted it is the newest stamp's footprint, as big as that stamp is laid (its
//! pressure and start taper) and turned as it is (the stroke's way for a brush that follows it, the
//! pen's azimuth and barrel for one turned by the pen, a random turn or scatter included). Hovering, it turns as a stamp laid there would: the way the
//! pointer has been going, smoothed, stands for the stroke's way, so a brush that follows the stroke
//! shows which way it would lie as the pen moves over the canvas; and as the pen is held.
//!
//! Both are read without waiting on the canvas (a frame laying a stroke's paint holds it): the
//! newest stamp's are noted as the stamps are laid, the hover's turn as the pointer moves.
//!
//! Hovering, the cursor fades: its outline and its fill show only once the pen has hovered a
//! moment, fading in over the next, so a pen lifted between strokes leaves nothing in the way of
//! the next one; they fade back out while it moves fast, and near the top of its reach.

use std::sync::atomic::{AtomicU32, Ordering};

use procraft_engine::stroke::{HeldPen, hover_angle};

use crate::{CanvasSession, Shared, State};

/// A hovering pointer's move this long (points) or shorter turns its way only as far as the move
/// agrees with it; each [`HEADING_REACH`] more pulls it further, up to [`HEADING_MOST`].
const HEADING_STILL: f32 = 3.0;
const HEADING_REACH: f32 = 8.0;
const HEADING_MOST: f32 = 0.3;
/// A move agreeing with the way pulls it this much more (a move against it, none): a pen held
/// near still, its moves this way and that, doesn't swing it about.
const HEADING_AGREEING: f32 = 0.1;
/// How fast that falls off as the move turns from the way (per unit of their difference).
const HEADING_DISAGREEING: f32 = 0.75;

/// A hover settles from 0 as it begins to 1 a second on (a 120th at each of 120 hover events a
/// second); the cursor shows once it has settled this far, and fully this much further on.
const HOVER_SHOWS_S: f32 = 0.1;
const HOVER_FADES_IN_S: f32 = 0.125;
/// Hovering faster than this (points a second: 30 points between two of 120 hover events a
/// second), it settles back, the faster the quicker.
const HOVER_RACING: f32 = 3600.0;
/// The cursor fades out over the top third of the height the pen is sensed at.
const HOVER_HEIGHT_FADE: f32 = 3.0;
/// A step between hover events counts this long at most (seconds): the pen out of sight meanwhile.
const HOVER_STEP_MOST_S: f32 = 1.0 / 30.0;

/// How far a hover has settled (0..1) from `settled` after a step of `seconds` moving at `speed`
/// points a second: on with the time, back while it races, the faster the quicker. Every app's hover
/// cursor fades by it.
pub(crate) fn hover_settled(settled: f32, seconds: f32, speed: f32) -> f32 {
    let step = if seconds.is_finite() { seconds.clamp(0.0, HOVER_STEP_MOST_S) } else { 0.0 };
    let gained = if speed > HOVER_RACING { -step * speed / HOVER_RACING } else { step };
    (settled + gained).clamp(0.0, 1.0)
}

/// How strongly the hover cursor shows (0..1), settled `settled`, the pen `height` up (a share of
/// the height it is sensed at; 0 where the platform doesn't say): none until it has hovered a
/// moment, then fading in; fainter over the top third of the height.
pub(crate) fn hover_shown(settled: f32, height: f32) -> f32 {
    let shown = ((settled - HOVER_SHOWS_S) / HOVER_FADES_IN_S).clamp(0.0, 1.0);
    let low = if height > 0.0 { ((1.0 - height) * HOVER_HEIGHT_FADE).clamp(0.0, 1.0) } else { 1.0 };
    shown * low
}

/// A pen held over the canvas: how it is turned (radians), its lean's way as its samples' azimuth
/// (orientation) and its barrel (0 for a pen that doesn't say).
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct CursorPen {
    pub azimuth: f32,
    pub roll: f32,
}

/// The way a hovering pointer has been going.
#[derive(Debug, Default)]
struct Heading {
    /// Where it last was (points); none since it left.
    last: Option<(f32, f32)>,
    /// The way it goes, smoothed (zero before it has gone anywhere).
    way: (f32, f32),
}

impl Heading {
    /// It moved on to `at` (points).
    fn moved(&mut self, at: (f32, f32)) {
        if !(at.0.is_finite() && at.1.is_finite()) {
            return;
        }
        let Some(from) = self.last.replace(at) else { return };
        let (dx, dy) = (at.0 - from.0, at.1 - from.1);
        let far = dx.hypot(dy);
        // a still pointer keeps its way
        if far.is_nan() || far <= 1e-4 {
            return;
        }
        let (ux, uy) = (dx / far, dy / far);
        let (ox, oy) = (ux - self.way.0, uy - self.way.1);
        let agrees = (1.0 - HEADING_DISAGREEING * ox.hypot(oy)).max(0.0);
        let pull = ((far - HEADING_STILL) / HEADING_REACH).clamp(0.0, 1.0) * HEADING_MOST;
        let pull = (pull + HEADING_AGREEING * agrees).min(1.0);
        self.way = (self.way.0 + pull * ox, self.way.1 + pull * oy);
    }

    /// The way (radians on the screen, clockwise from the right), none before it has gone anywhere.
    fn angle(&self) -> Option<f32> {
        let (x, y) = self.way;
        (x.hypot(y) > 1e-4).then(|| y.atan2(x))
    }
}

/// How far a hover has settled (see [`hover_settled`]), for a pointer told of read without
/// waiting on the canvas ([`CanvasSession::cursor_hover_at`]).
#[derive(Debug, Default)]
struct HoverFade {
    /// 0 as the hover begins, 1 once it has gone on long enough.
    settled: f32,
    /// Where (points) and when (seconds) the pointer last was; none since the hover began.
    last: Option<((f32, f32), f64)>,
}

impl HoverFade {
    /// The pointer hovers at `at` (points) at `now` (seconds): how far it has settled now.
    fn moved(&mut self, at: (f32, f32), now: f64) -> f32 {
        if !(at.0.is_finite() && at.1.is_finite() && now.is_finite()) {
            return self.settled;
        }
        if let Some((from, then)) = self.last {
            // (a clock gone back counts as no time)
            let seconds = ((now - then) as f32).max(0.0);
            let far = (at.0 - from.0).hypot(at.1 - from.1);
            let speed = if seconds > 0.0 && far.is_finite() { far / seconds } else { 0.0 };
            self.settled = hover_settled(self.settled, seconds, speed);
        }
        self.last = Some((at, now));
        self.settled
    }

    /// How strongly the cursor shows (0..1), the pen `height` up.
    fn shown(&self, height: f32) -> f32 {
        hover_shown(self.settled, height)
    }
}

/// How the brush cursor shows where the pointer hovers ([`CanvasSession::cursor_hover_at`]): how
/// far its outline turns (see [`CanvasSession::cursor_turn`]), and how strongly its outline and its
/// fill show (0..1) as the hover fades in.
#[derive(Clone, Copy, Debug, PartialEq, uniffi::Record)]
pub struct CursorHover {
    pub turn: f32,
    pub outline: f32,
    pub fill: f32,
}

/// The brush cursor's turns, kept beside the canvas's lock.
pub(crate) struct CursorTurns {
    /// The newest stamp's while a stroke is painted (an f32's bits; NaN before it has one), and
    /// its radius on the screen (window pixels).
    painted: AtomicU32,
    painted_radius: AtomicU32,
    /// As last worked out where the pointer hovers.
    hovered: AtomicU32,
    heading: parking_lot::Mutex<Heading>,
    /// How far the hover's fade has come.
    fade: parking_lot::Mutex<HoverFade>,
    /// Window pixels a point, as last seen.
    points: AtomicU32,
    /// The plugins strokes go through, for where the cursor shows (see
    /// [`CanvasSession::cursor_point`]): kept beside the canvas's lock, so it never waits on it.
    pub(crate) hooks: parking_lot::Mutex<Option<std::sync::Arc<crate::hooks::Hooks>>>,
}

impl Default for CursorTurns {
    fn default() -> CursorTurns {
        CursorTurns {
            painted: AtomicU32::new(f32::NAN.to_bits()),
            painted_radius: AtomicU32::new(f32::NAN.to_bits()),
            hovered: AtomicU32::new(0f32.to_bits()),
            heading: parking_lot::Mutex::new(Heading::default()),
            fade: parking_lot::Mutex::new(HoverFade::default()),
            points: AtomicU32::new(1f32.to_bits()),
            hooks: parking_lot::Mutex::new(None),
        }
    }
}

fn load(a: &AtomicU32) -> f32 {
    f32::from_bits(a.load(Ordering::Relaxed))
}

fn store(a: &AtomicU32, v: f32) {
    a.store(v.to_bits(), Ordering::Relaxed);
}

impl State {
    /// The canvas's turn on the screen (radians, clockwise).
    fn view_turn(&self) -> f32 {
        self.view.map_or(0.0, |v| v.rotation())
    }

    /// How far the cursor's outline turns past the current brush's own rotation (which
    /// [`CanvasSession::brush_outline`] has turned it by already) for a stamp turned `angle` on the
    /// canvas: on the screen, with the canvas.
    fn cursor_turn_for(&self, angle: f32) -> f32 {
        let turn = angle - self.brush(self.tool).shape.rotation + self.view_turn();
        if turn.is_finite() { turn } else { 0.0 }
    }

    /// The cursor's turn and radius on the screen (window pixels) for the newest stamp of the
    /// stroke painted, none before it has one.
    fn painted_stamp(&self) -> Option<(f32, f32)> {
        let stamp = self.stroke.as_ref()?.newest_stamp()?;
        let zoom = self.view.map_or(1.0, |v| v.zoom());
        let radius = stamp.radius * zoom;
        Some((self.cursor_turn_for(stamp.angle), if radius.is_finite() { radius.max(0.0) } else { f32::NAN }))
    }

    /// The cursor's turn hovering, going `heading` (radians on the screen; none before it has
    /// moved), the pen held as `pen` says (none: a pointer that doesn't tell).
    fn hover_turn(&self, heading: Option<f32>, pen: Option<CursorPen>) -> f32 {
        let view = self.view_turn();
        let pen = pen.map(|p| HeldPen { azimuth: p.azimuth, roll: p.roll });
        // a way on the screen is that much less turned on the canvas
        let angle = hover_angle(self.brush(self.tool), heading.map(|h| h - view), pen, view);
        self.cursor_turn_for(angle)
    }
}

impl Shared {
    /// Notes the newest stamp's turn and size for the cursor, the stroke's stamps laid (as `st`
    /// now is).
    pub(crate) fn note_painted_turn(&self, st: &State) {
        let (turn, radius) = st.painted_stamp().unwrap_or((f32::NAN, f32::NAN));
        store(&self.cursor.painted, turn);
        store(&self.cursor.painted_radius, radius);
    }

    /// No stroke is painted: the cursor turns as it hovers, as big as the brush.
    pub(crate) fn forget_painted_turn(&self) {
        store(&self.cursor.painted, f32::NAN);
        store(&self.cursor.painted_radius, f32::NAN);
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The pen, or a mouse, hovers over the canvas at (`x`, `y`), window pixels, held as `pen` says
    /// (none for a pointer that doesn't tell how it is turned). The way it goes is noted; returns how
    /// far the brush cursor's outline turns there (see [`CanvasSession::cursor_turn`]).
    pub fn cursor_hovers(&self, x: f32, y: f32, pen: Option<CursorPen>) -> f32 {
        let turns = &self.shared.cursor;
        // (the canvas held by a frame: the scale and the turn as last seen)
        let st = self.shared.try_lock();
        if let Some(st) = &st {
            store(&turns.points, st.display_scale.max(1e-3));
        }
        let points = load(&turns.points).max(1e-3);
        let heading = {
            let mut h = turns.heading.lock();
            h.moved((x / points, y / points));
            h.angle()
        };
        if let Some(st) = &st {
            store(&turns.hovered, st.hover_turn(heading, pen));
        }
        load(&turns.hovered)
    }

    /// The pen, or a mouse, hovers over the canvas at (`x`, `y`), window pixels, at `seconds` (any
    /// steady clock), held as `pen` says, `height` up (a share of the height it is sensed at, 0 on
    /// the glass; none for a pointer that doesn't tell): how its brush cursor shows there. It turns
    /// as [`CanvasSession::cursor_hovers`] says; its outline and fill show once the hover has gone
    /// on a moment (none as it begins), fading in, back out while the pointer races, and fainter
    /// high above the glass, as [`CanvasSession::pen_hover_cursor`] fades. Read without waiting on
    /// the canvas.
    pub fn cursor_hover_at(
        &self,
        x: f32,
        y: f32,
        pen: Option<CursorPen>,
        seconds: f64,
        height: Option<f32>,
    ) -> CursorHover {
        let turn = self.cursor_hovers(x, y, pen);
        let points = load(&self.shared.cursor.points).max(1e-3);
        let mut fade = self.shared.cursor.fade.lock();
        fade.moved((x / points, y / points), seconds);
        let shown = fade.shown(height.unwrap_or(0.0));
        CursorHover { turn, outline: shown, fill: shown }
    }

    /// The hovering pointer left the canvas, or touched it: its next move begins afresh from
    /// where it comes back (the way it was going is kept), its cursor faded in again from none.
    pub fn cursor_hover_gone(&self) {
        self.shared.cursor.heading.lock().last = None;
        *self.shared.cursor.fade.lock() = HoverFade::default();
    }

    /// How far the brush cursor's outline turns on the screen past the brush's own rotation (radians,
    /// clockwise), as the stamps it stands for: while a stroke is painted, as its newest stamp is;
    /// else as one laid where the pointer last hovered would be. Read without waiting on the canvas.
    pub fn cursor_turn(&self) -> f32 {
        let painted = load(&self.shared.cursor.painted);
        if painted.is_finite() { painted } else { load(&self.shared.cursor.hovered) }
    }

    /// Where the brush cursor shows for a pointer at (`x`, `y`), window pixels: where a stroke's
    /// sample there lands, as the plugins on the `position` hook move it (there, with none on), so
    /// the cursor stands where the stroke goes, hovering and painting alike. Read without waiting on
    /// the canvas.
    pub fn cursor_point(&self, x: f32, y: f32) -> Vec<f32> {
        let hooks = self.shared.cursor.hooks.lock().clone();
        let (x, y) = hooks.map_or((x, y), |hooks| hooks.position(x, y));
        vec![x, y]
    }

    /// How big the brush cursor's outline is while a stroke is painted: its newest stamp's radius
    /// on the screen (window pixels), as it is laid (its pressure and start taper in); none before
    /// it has one, or not painting (the brush's own size then). Read without waiting on the canvas.
    pub fn cursor_radius(&self) -> Option<f32> {
        Some(load(&self.shared.cursor.painted_radius)).filter(|r| r.is_finite())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hovering_pointer_s_way_follows_its_moves_and_holds_when_it_stops() {
        let mut h = Heading::default();
        assert_eq!(h.angle(), None);
        // across the screen, a few points at a time: it comes round to the right
        for i in 0..40 {
            h.moved((i as f32 * 6.0, 100.0));
        }
        let right = h.angle().unwrap_or(f32::NAN);
        assert!(right.abs() < 0.02, "{right}");
        // then down: it turns to follow, not at once
        h.moved((240.0, 106.0));
        let first = h.angle().unwrap_or(f32::NAN);
        assert!(first > 0.0 && first < 1.0, "{first}");
        for i in 2..40 {
            h.moved((240.0, 100.0 + i as f32 * 6.0));
        }
        let down = h.angle().unwrap_or(f32::NAN);
        assert!((down - std::f32::consts::FRAC_PI_2).abs() < 0.02, "{down}");
        // held still, and wobbling back and forth by a point: it stays as it was
        for _ in 0..20 {
            h.moved((240.0, 334.0));
        }
        for i in 0..20 {
            h.moved((240.0 + if i % 2 == 0 { 1.0 } else { -1.0 }, 334.0));
        }
        let held = h.angle().unwrap_or(f32::NAN);
        assert!((held - down).abs() < 0.2, "{down} -> {held}");
        // gone and back elsewhere: the jump there is no move
        h.last = None;
        h.moved((900.0, 0.0));
        assert_eq!(h.angle(), Some(held));
    }

    /// A hover at 120 events a second, `n` of them, each `step` points on from the last: how much
    /// the cursor shows after each.
    fn hovered(fade: &mut HoverFade, from: f64, n: usize, step: f32) -> Vec<f32> {
        (0..n)
            .map(|i| {
                fade.moved((step * i as f32, 0.0), from + i as f64 / 120.0);
                fade.shown(0.0)
            })
            .collect()
    }

    /// Where `fade` stands after a move to `at` at `now`.
    fn shown_after(fade: &mut HoverFade, at: (f32, f32), now: f64) -> f32 {
        fade.moved(at, now);
        fade.shown(0.0)
    }

    #[test]
    fn a_hover_shows_its_cursor_only_after_a_moment_fading_in() {
        let mut fade = HoverFade::default();
        let shown = hovered(&mut fade, 10.0, 40, 2.0);
        // as it begins and for a tenth of a second on: nothing in the way of the next stroke
        assert!(shown[..=12].iter().all(|&s| s < 1e-3), "{shown:?}");
        // then fading in over an eighth of a second, steadily
        assert!(shown[13] > 0.03 && shown[13] < 0.2, "{shown:?}");
        assert!(shown[13..=27].windows(2).all(|w| w[1] >= w[0]), "{shown:?}");
        assert!(shown[26] < 1.0 && shown[27] >= 0.999, "{shown:?}");
        assert!(shown[27..].iter().all(|&s| s >= 0.999), "{shown:?}");
        eprintln!("hover fade at 120 Hz: {shown:?}");
    }

    #[test]
    fn a_hover_fades_back_out_while_it_moves_fast_and_in_again_when_it_slows() {
        let mut fade = HoverFade::default();
        // hovered slowly over a second: whole, and all the way along
        hovered(&mut fade, 0.0, 150, 1.0);
        assert_eq!(fade.shown(0.0), 1.0);
        // 60 points between events at 120 a second (7200 a second, twice as fast as it takes):
        // it goes back two 120ths each event, so it shows whole a while yet, then fades out
        let (x, t) = (149.0, 149.0 / 120.0);
        let fast: Vec<f32> =
            (1..=80).map(|i| shown_after(&mut fade, (x + 60.0 * i as f32, 0.0), t + f64::from(i) / 120.0)).collect();
        assert!(fast[..40].iter().all(|&s| s == 1.0), "{fast:?}");
        assert!(fast[40..].windows(2).all(|w| w[1] <= w[0]) && fast[50] < 1.0, "{fast:?}");
        assert_eq!(fast[79], 0.0, "{fast:?}");
        // slower than 30 points an event: back on its way in
        let (x, t) = (x + 4800.0, t + 80.0 / 120.0);
        let slow: Vec<f32> =
            (1..60).map(|i| shown_after(&mut fade, (x + i as f32, 0.0), t + f64::from(i) / 120.0)).collect();
        assert!(slow[10] < 1e-3 && slow[58] >= 0.999, "{slow:?}");
        eprintln!("fast: {fast:?}");
    }

    #[test]
    fn a_hover_counts_time_not_events_and_a_pen_high_up_shows_fainter() {
        // at 240 events a second it fades in as at 120: by the time, not the count
        let mut fade = HoverFade::default();
        let shown: Vec<f32> = (0..60).map(|i| shown_after(&mut fade, (i as f32, 0.0), i as f64 / 240.0)).collect();
        assert!(shown[23] < 1e-3 && shown[24] < 1e-2, "{shown:?}");
        assert!(shown[54] >= 0.999 && shown[52] < 1.0, "{shown:?}");
        // a step counts a 30th of a second at most (the pen out of sight meanwhile)
        let mut fade = HoverFade::default();
        fade.moved((0.0, 0.0), 0.0);
        assert!((fade.moved((1.0, 0.0), 2.0) - 1.0 / 30.0).abs() < 1e-6);
        // a clock gone back, or not a number, changes nothing
        let settled = fade.settled;
        assert_eq!(fade.moved((2.0, 0.0), 1.0), settled);
        assert_eq!(fade.moved((f32::NAN, 0.0), 3.0), settled);
        // up to two thirds of the height it is sensed at whole, then fainter to none at the top;
        // none said, whole
        assert_eq!(hover_shown(1.0, 0.0), 1.0);
        assert_eq!(hover_shown(1.0, 0.5), 1.0);
        assert!((hover_shown(1.0, 0.9) - 0.3).abs() < 1e-5);
        assert_eq!(hover_shown(1.0, 1.0), 0.0);
        assert_eq!(hover_shown(1.0, f32::NAN), 1.0);
    }
}

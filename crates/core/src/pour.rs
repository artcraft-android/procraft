//! Dropping a colour on the canvas: the colour poured into the area joined to a point whose colour
//! is near the point's own, measured on the reference layer if there is one. While the finger stays
//! down the threshold can change and the pour is redone from the layer as it was; it ends as one
//! undo step. Recolor keeps a pour open while its point moves and its colour changes. A plugin on
//! the fill hook has its say as a pour is let go, on a thread of its own: the pour stays open, its
//! layer held, until it has.

use std::sync::{Arc, Condvar, Mutex};

use make2d_engine::document::LayerId;
use make2d_engine::fill::{self, Flood};
use make2d_engine::geom::{IRect, Vec2};
use make2d_engine::selection::Mask;

use crate::hooks::Hooks;
use crate::plugin_run::MOST_FILL;
use crate::reveal::{Reveal, Shake};
use crate::{CanvasSession, Shared, State};

/// What is said of a fill with more in it than can go through a plugin.
const TOO_BIG: &str = "This fill is too big for a plugin: it stays as it is.";

/// Continuous filling: a pixel no more opaque than this (of 255) is unfilled.
const UNFILLED: u8 = 8;
/// Continuous filling: a place this near (canvas pixels) where a fill last came to nothing isn't
/// tried again.
const TRAIL_MISS: f32 = 12.0;

/// Continue filling's Continuous drag: each area the touch crosses where nothing is filled yet is
/// filled (kept at once, so a plugin on the fill hook closes it before the next is measured), and
/// the whole drag is one undo step.
#[derive(Default)]
pub(crate) struct Trail {
    /// The current layer's pixels as the drag's last fill left them, and what fills are measured
    /// on (none inside: the layer itself), so neither is read back for each fill.
    layer: Option<(LayerId, Vec<u8>)>,
    measured: Option<Option<Vec<u8>>>,
    /// Where the drag's fills reached, a byte a canvas pixel: none of it is filled again.
    covered: Vec<u8>,
    /// How many fills it kept (on a vector layer its one drop holds them all, and isn't counted).
    kept: u32,
    /// Where a fill last came to nothing (canvas pixels).
    missed: Option<Vec2>,
}

impl Trail {
    fn cover(&mut self, mask: &Mask) {
        if self.covered.len() != mask.data.len() {
            self.covered = vec![0; mask.data.len()];
        }
        for (c, &m) in self.covered.iter_mut().zip(&mask.data) {
            *c |= u8::from(m > 0);
        }
    }

    fn cover_pixel(&mut self, i: usize, pixels: usize) {
        if self.covered.len() != pixels {
            self.covered = vec![0; pixels];
        }
        if let Some(c) = self.covered.get_mut(i) {
            *c = 1;
        }
    }
}

/// What a place on a Continuous drag did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrailStep {
    /// It was filled.
    Filled,
    /// Something is filled there already (or it is off the canvas): nothing to do.
    Passed,
    /// The last fill is still being finished by a plugin: it is to be tried again.
    Busy,
}

/// A pour a plugin is finishing: when it has, and whoever is waiting for that.
#[derive(Default)]
pub(crate) struct Finishing {
    done: Mutex<bool>,
    ended: Condvar,
}

impl Finishing {
    fn wait(&self) {
        let mut done = self.done.lock().unwrap_or_else(|e| e.into_inner());
        while !*done {
            done = self.ended.wait(done).unwrap_or_else(|e| e.into_inner());
        }
    }

    fn end(&self) {
        *self.done.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.ended.notify_all();
    }
}

/// A pour under way.
pub(crate) struct Pour {
    layer: LayerId,
    /// The layer's pixels as the pour began.
    before: Vec<u8>,
    /// The area measured from the point; none while the point is off the canvas.
    flood: Option<Flood>,
    /// The reference layer's pixels when measured on it, so poured over this one.
    elsewhere: Option<Vec<u8>>,
    colour: [f32; 3],
    selection: Option<Mask>,
    alpha_lock: bool,
    /// The threshold last poured, in thousandths, and where the layer was last poured into (x, y,
    /// width, height): outside it the layer is as it was.
    shown: i32,
    shown_region: Option<[u32; 4]>,
    /// What it last poured, as the flood reached (within the selection).
    shown_mask: Option<Mask>,
    /// The flood as a plugin left it, once one has.
    closed: Option<Mask>,
    /// Being finished by a plugin: nothing more changes it.
    finishing: Option<Arc<Finishing>>,
}

impl Pour {
    /// The colour poured at `threshold`, as the rectangle of the layer it changes now joined with
    /// the one it changed last (put back as it was where it no longer pours): that rectangle (x, y,
    /// width, height) and its pixels; none if neither changes anything. With `same_flood` (only
    /// the threshold changed since it was shown) only the rectangle where what it pours changed.
    /// Remembered as shown.
    fn pour_region(&mut self, width: u32, threshold: f32, same_flood: bool) -> Option<([u32; 4], Vec<u8>)> {
        let mask = self.flood.as_ref().map(|flood| {
            let mut mask = self.closed.clone().unwrap_or_else(|| flood.mask(threshold));
            // poured into what is see-through, it goes on under the line it stopped at
            if flood.colour[3] == 0 {
                fill::under_edges(&mut mask, self.elsewhere.as_deref().unwrap_or(&self.before));
            }
            if let Some(sel) = &self.selection {
                for (m, &s) in mask.data.iter_mut().zip(&sel.data) {
                    *m = ((u16::from(*m) * u16::from(s) + 127) / 255) as u8;
                }
            }
            mask
        });
        let now = mask.as_ref().and_then(Mask::bounds);
        // the same colour poured from the same seed colour: the pixels change only where the
        // flood reaches differently (a threshold dragged over a large fill changes its edges)
        let changed = match (&mask, &self.shown_mask) {
            (Some(m), Some(old)) if same_flood && m.data.len() == old.data.len() => Some(differ(m, old)),
            _ => None,
        };
        let region = match (changed, now, self.shown_region) {
            (Some(Some(r)), ..) => r,
            (Some(None), ..) => {
                self.shown_region = now;
                self.shown_mask = mask;
                return None;
            }
            (None, Some(a), Some(b)) => join(a, b),
            (None, a, b) => a.or(b)?,
        };
        self.shown_region = now;
        let [x, y, w, h] = region.map(|v| v as usize);
        let stride = width as usize;
        // the layer as it was there, and what of the flood falls there (a row at a time)
        let cut = |all: &[u8], px: usize| -> Vec<u8> {
            let mut out = Vec::with_capacity(w * h * px);
            for r in y..y + h {
                out.extend_from_slice(all.get((r * stride + x) * px..(r * stride + x + w) * px).unwrap_or(&[]));
            }
            out
        };
        let mut pixels = cut(&self.before, 4);
        if let (Some(mask), Some(flood)) = (&mask, &self.flood) {
            let data = cut(&mask.data, 1);
            let part = Mask { width: region[2], height: region[3], data };
            if self.elsewhere.is_some() {
                fill::pour_over(&mut pixels, &part, self.colour, self.alpha_lock);
            } else {
                fill::pour(&mut pixels, &part, flood.colour, self.colour, self.alpha_lock);
            }
        }
        self.shown_mask = mask;
        Some((region, pixels))
    }

    /// The layer's pixels with the pour as it now is put in, what it was measured on, and what of
    /// the canvas it reached (before the selection): it ends.
    fn settled(mut self, width: u32) -> (Vec<u8>, Option<Vec<u8>>, Option<Mask>) {
        let threshold = self.shown as f32 / 1000.0;
        let reached = self.flood.as_ref().map(|flood| self.closed.clone().unwrap_or_else(|| flood.mask(threshold)));
        // (nothing shown yet: the whole of it comes back)
        self.shown_region = None;
        self.shown_mask = None;
        let region = self.pour_region(width, threshold, false);
        let mut pixels = std::mem::take(&mut self.before);
        if let Some(([x, y, w, _], part)) = region {
            let (stride, row) = (width as usize * 4, w as usize * 4);
            for (r, from) in part.chunks_exact(row.max(1)).enumerate() {
                let at = (y as usize + r) * stride + x as usize * 4;
                if let Some(to) = pixels.get_mut(at..at + row) {
                    to.copy_from_slice(from);
                }
            }
        }
        (pixels, self.elsewhere.take(), reached)
    }

    /// The area measured again from canvas point `seed`, on the reference layer or the layer as
    /// it was.
    fn measure(&mut self, width: u32, height: u32, seed: (u32, u32)) {
        let sample = self.elsewhere.as_deref().unwrap_or(&self.before);
        self.flood = Flood::new(width, height, sample, seed);
    }

    /// The point moved to `seed`: true if the pour stays as it is, the new point being of the
    /// same colour inside what it poured (the area joined to it then the same), so only the seed
    /// moves; otherwise measured again from it, and false.
    fn moved(&mut self, width: u32, height: u32, seed: (u32, u32)) -> bool {
        let sample = self.elsewhere.as_deref().unwrap_or(&self.before);
        let i = seed.1 as usize * width as usize + seed.0 as usize;
        let colour: Option<[u8; 4]> = sample.get(i * 4..i * 4 + 4).and_then(|c| c.try_into().ok());
        let inside = self.shown_mask.as_ref().and_then(|m| m.data.get(i)).is_some_and(|&v| v > 0);
        if let Some(flood) = self.flood.as_mut()
            && inside
            && colour == Some(flood.colour)
            && flood.reseed(seed)
        {
            return true;
        }
        self.measure(width, height, seed);
        false
    }

    /// The pour as it now is, put on its layer in `canvas` where it changed (`same_flood`: only
    /// its threshold changed since it was last shown).
    fn show(&mut self, width: u32, canvas: Option<&mut make2d_gpu::Canvas>, same_flood: bool) {
        let threshold = self.shown as f32 / 1000.0;
        let Some(([x, y, w, h], pixels)) = self.pour_region(width, threshold, same_flood) else { return };
        if let Some(canvas) = canvas {
            canvas.put_layer_region(self.layer, x, y, w, h, pixels);
        }
    }
}

/// `mask`, a fill from `seed`, as the plugins on the fill hook leave it: its box goes through with
/// a pixel round it, which, in the picture, is a pixel of no fill, and past the picture's edge
/// nothing. None if they change nothing, or it is too big to go through. Takes a while: nothing
/// is held while it does.
pub(crate) fn closed_by(hooks: &Hooks, mut mask: Mask, seed: (u32, u32)) -> Option<Mask> {
    let [x, y, w, h] = mask.bounds()?;
    let (left, top) = (x.saturating_sub(1), y.saturating_sub(1));
    let (right, bottom) = ((x + w + 1).min(mask.width), (y + h + 1).min(mask.height));
    let (width, height) = ((right - left) as usize, (bottom - top) as usize);
    if (width * height) as u64 > MOST_FILL {
        hooks.say(TOO_BIG);
        return None;
    }
    let (stride, words) = (mask.width as usize, width.div_ceil(64));
    let mut rows = vec![0_u64; words * height];
    for (from, to) in mask.data.chunks_exact(stride).skip(top as usize).zip(rows.chunks_exact_mut(words)) {
        for (i, &v) in from.iter().skip(left as usize).take(width).enumerate() {
            if let Some(word) = to.get_mut(i / 64).filter(|_| v > 0) {
                *word |= 1 << (i % 64);
            }
        }
    }
    let from = (seed.0.checked_sub(left)? as usize, seed.1.checked_sub(top)? as usize);
    if !hooks.fill_rows(&mut rows, width, height, from) {
        return None;
    }
    for (to, left_of) in mask.data.chunks_exact_mut(stride).skip(top as usize).zip(rows.chunks_exact(words)) {
        for (i, v) in to.iter_mut().skip(left as usize).take(width).enumerate() {
            if left_of.get(i / 64).is_some_and(|word| word >> (i % 64) & 1 == 0) {
                *v = 0;
            }
        }
    }
    Some(mask)
}

/// Ends the open pour: `keep` makes it one undo step, called `name` if given (else "ColorDrop");
/// otherwise (or with nothing poured) the layer goes back to how it was.
fn end_pour(shared: &Shared, st: &mut State, keep: bool, name: Option<&str>) {
    let Some(pour) = st.pour.take() else { return };
    let keep = keep && pour.flood.is_some();
    // a Continuous drag goes on from the layer as this leaves it
    if st.pour_trail.is_some() {
        let (width, layer) = (st.doc.width(), pour.layer);
        let (pixels, measured, reached) = if keep { pour.settled(width) } else { (pour.before, pour.elsewhere, None) };
        if let Some(trail) = st.pour_trail.as_mut() {
            trail.layer = Some((layer, pixels));
            if trail.measured.is_none() {
                trail.measured = Some(measured);
            }
            if let Some(reached) = &reached {
                trail.cover(reached);
            }
            trail.kept += u32::from(keep);
        }
    }
    if let Some(canvas) = st.canvas.as_mut() {
        if keep {
            canvas.name_next_step(name.unwrap_or("ColorDrop"));
        }
        canvas.release_layer(keep, None);
    }
    st.revision += 1;
    shared.frame(st);
}

impl State {
    /// A dropped colour's fill over `region` (x, y, width, height, canvas pixels), the colour
    /// landed on window point `point`, starts spreading in: the canvas as it was is kept to show
    /// over the fill but for a hole that grows. Not with Reduce Motion, nor on a model.
    fn start_reveal(&mut self, point: Vec2, region: [u32; 4]) {
        self.end_reveal();
        if self.reduce_motion || self.scene_shown() || self.sphere.is_some() {
            return;
        }
        let Some(view) = self.view else { return };
        let [x, y, w, h] = region.map(|v| i32::try_from(v).unwrap_or(i32::MAX));
        let rect = IRect::new(x, y, x.saturating_add(w), y.saturating_add(h));
        let mut shake = Shake::from_clock();
        let Some(reveal) = Reveal::new(point, rect, &view, self.window_size, self.display_scale, &mut shake) else {
            return;
        };
        // only what the window shows of it is veiled, at no more pixels than it is shown with
        let (ww, wh) = (self.window_size.0 as f32, self.window_size.1 as f32);
        let seen = [(0.0, 0.0), (ww, 0.0), (0.0, wh), (ww, wh)].map(|(x, y)| view.screen_to_canvas(Vec2::new(x, y)));
        let seen = seen.iter().flatten().fold(None, |b: Option<(Vec2, Vec2)>, &q| {
            let (lo, hi) = b.unwrap_or((q, q));
            Some((Vec2::new(lo.x.min(q.x), lo.y.min(q.y)), Vec2::new(hi.x.max(q.x), hi.y.max(q.y))))
        });
        let Some((lo, hi)) = seen.filter(|(lo, hi)| lo.is_finite() && hi.is_finite()) else { return };
        let seen = IRect::new(
            lo.x.floor() as i32 - 2,
            lo.y.floor() as i32 - 2,
            hi.x.ceil() as i32 + 2,
            hi.y.ceil() as i32 + 2,
        );
        let scale = view.to_screen().scale_factor();
        if let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) {
            canvas.veil_begin(gpu, rect.intersect(seen), scale);
            if canvas.veil_rect().is_some() {
                self.reveal = Some(reveal);
            }
        }
    }

    /// A fill spreading in shows all of itself now.
    pub(crate) fn end_reveal(&mut self) {
        if self.reveal.take().is_some() {
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.veil_end();
            }
            self.needs_frame = true;
        }
    }
}

/// The smallest rectangle (x, y, width, height) holding every pixel where masks `a` and `b` (of
/// the same size) differ; none where they don't.
fn differ(a: &Mask, b: &Mask) -> Option<[u32; 4]> {
    let w = (a.width as usize).max(1);
    let (mut top, mut bottom, mut left, mut right) = (None, 0, usize::MAX, 0);
    for (y, (p, q)) in a.data.chunks(w).zip(b.data.chunks(w)).enumerate() {
        if p == q {
            continue;
        }
        let Some(first) = p.iter().zip(q).position(|(u, v)| u != v) else { continue };
        let last = p.iter().zip(q).rposition(|(u, v)| u != v).unwrap_or(first);
        top.get_or_insert(y);
        bottom = y;
        left = left.min(first);
        right = right.max(last);
    }
    let top = top?;
    let n = |v: usize| u32::try_from(v).unwrap_or(u32::MAX);
    Some([n(left), n(top), n(right - left + 1), n(bottom - top + 1)])
}

/// The smallest rectangle (x, y, width, height) holding both `a` and `b`.
fn join(a: [u32; 4], b: [u32; 4]) -> [u32; 4] {
    let [ax, ay, aw, ah] = a;
    let [bx, by, bw, bh] = b;
    let (x, y) = (ax.min(bx), ay.min(by));
    [x, y, (ax + aw).max(bx + bw) - x, (ay + ah).max(by + bh) - y]
}

fn thousandths(threshold: f32) -> i32 {
    (threshold.clamp(0.0, 1.0) * 1000.0).round() as i32
}

fn unit(c: f32) -> f32 {
    if c.is_finite() { c.clamp(0.0, 1.0) } else { 0.0 }
}

/// The canvas pixel under window point (`x`, `y`), if it is on the canvas.
fn seed_at(st: &crate::State, x: f32, y: f32) -> Option<(u32, u32)> {
    st.window_texel(x, y)
}

#[uniffi::export]
impl CanvasSession {
    /// Whether window point (`x`, `y`) is on the artwork, where a dragged colour can come to rest.
    pub fn pour_lands(&self, x: f32, y: f32) -> bool {
        seed_at(&self.shared.lock(), x, y).is_some()
    }

    /// Pours colour (`r`, `g`, `b`, 0..1) at window point (`x`, `y`) with `threshold` (0..1) on
    /// the current layer, within the selection if there is one. False if nothing was poured (off
    /// the canvas, or the layer locked or text). The pour stays open for
    /// [`CanvasSession::pour_threshold`] until [`CanvasSession::pour_end`].
    pub fn pour_begin(&self, x: f32, y: f32, r: f32, g: f32, b: f32, threshold: f32) -> bool {
        self.begin_pour(x, y, [r, g, b], threshold, false)
    }

    /// Pours as [`CanvasSession::pour_begin`] does a colour dragged out and let go, or come to
    /// rest, at window point (`x`, `y`): the fill is made at once, and shows spreading in from the
    /// point over a moment (at once with Reduce Motion).
    pub fn pour_drop(&self, x: f32, y: f32, r: f32, g: f32, b: f32, threshold: f32) -> bool {
        self.begin_pour(x, y, [r, g, b], threshold, true)
    }

    /// The open pour redone from window point (`x`, `y`) instead, from the layer as it was: off
    /// the canvas it pours nothing until it comes back. False if no pour is open.
    pub fn pour_move(&self, x: f32, y: f32) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let seed = seed_at(st, x, y);
        if st.vector_pour_move(seed.and_then(|_| st.window_to_canvas(x, y))) {
            self.shared.frame(st);
            return true;
        }
        let (w, h) = (st.doc.width(), st.doc.height());
        let Some(pour) = st.pour.as_mut().filter(|p| p.finishing.is_none()) else { return false };
        let same = match seed {
            Some(seed) => pour.moved(w, h, seed),
            None => {
                pour.flood = None;
                false
            }
        };
        // moved within what it filled, from the same colour: nothing changes
        if !same {
            pour.show(w, st.canvas.as_mut(), false);
            self.shared.frame(st);
        }
        true
    }

    /// The open pour redone with colour (`r`, `g`, `b`, 0..1).
    pub fn pour_colour(&self, r: f32, g: f32, b: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.vector_pour_colour([r, g, b].map(unit)) {
            self.shared.frame(st);
            return;
        }
        let Some(pour) = st.pour.as_mut().filter(|p| p.finishing.is_none()) else { return };
        let colour = [r, g, b].map(unit);
        if colour == pour.colour {
            return;
        }
        pour.colour = colour;
        let w = st.doc.width();
        pour.show(w, st.canvas.as_mut(), false);
        self.shared.frame(st);
    }

    /// The open pour redone at `threshold` (0..1).
    pub fn pour_threshold(&self, threshold: f32) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.vector_pour_threshold(threshold) {
            self.shared.frame(st);
            return;
        }
        let Some(pour) = st.pour.as_mut().filter(|p| p.finishing.is_none()) else { return };
        let t = thousandths(threshold);
        if t == pour.shown {
            return;
        }
        pour.shown = t;
        let w = st.doc.width();
        pour.show(w, st.canvas.as_mut(), true);
        self.shared.frame(st);
    }

    /// Whether a dropped colour measures the visible layers together, as they show without the
    /// background (All Layers), instead of the current layer (or the reference layer). Off at
    /// first; it holds from the next pour.
    pub fn set_pour_all_layers(&self, on: bool) {
        self.shared.lock().pour_all_layers = on;
    }

    /// See [`CanvasSession::set_pour_all_layers`].
    pub fn pour_all_layers(&self) -> bool {
        self.shared.lock().pour_all_layers
    }

    /// Ends the open pour: `keep` makes it one undo step, called `name` if given (else
    /// "ColorDrop"); otherwise (or with nothing poured) the layer goes back to how it was. Kept,
    /// with a plugin on the fill hook, it ends when that has had its say, on a thread of its own:
    /// [`CanvasSession::pour_finishing`] until then.
    pub fn pour_end(&self, keep: bool, name: Option<String>) {
        self.await_pour();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if st.vector_pour_end(keep, name.as_deref()) {
            self.shared.frame(st);
            return;
        }
        if keep && self.finish_pour(st, &name) {
            return;
        }
        end_pour(&self.shared, st, keep, name.as_deref());
    }

    /// Whether a fill let go is still being finished by a plugin: nothing should be started until
    /// it is. The pour's own calls wait for it.
    pub fn pour_finishing(&self) -> bool {
        self.shared.lock().pour.as_ref().is_some_and(|pour| pour.finishing.is_some())
    }
}

impl State {
    /// Whether a pour measures what every visible layer shows (All Layers, or a plugin saying
    /// so), which changes as the current layer is filled.
    fn pour_measures_all(&self) -> bool {
        self.pour_all_layers || self.hooks.as_ref().is_some_and(|hooks| hooks.fill_all_layers())
    }

    /// What a pour on layer `current` is measured on, when not the layer itself: every visible
    /// layer together (All Layers), every layer seen while a plugin says so, or else the
    /// reference layer when another layer is it.
    fn pour_measured_on(&mut self, current: LayerId) -> Option<Vec<u8>> {
        if self.pour_all_layers {
            self.layers_composite()
        } else if self.hooks.as_ref().is_some_and(|hooks| hooks.fill_all_layers()) {
            self.composite_pixels()
        } else {
            self.doc.reference().filter(|&r| r != current).and_then(|r| self.layer_pixels(r))
        }
    }

    /// Whether canvas pixel `seed` of layer `current` is somewhere a Continuous drag fills: not
    /// reached by it yet, and nothing there, on the layer or on what it is measured on.
    fn trail_unfilled(&mut self, current: LayerId, seed: (u32, u32)) -> bool {
        let i = seed.1 as usize * self.doc.width() as usize + seed.0 as usize;
        let Some(trail) = self.pour_trail.as_ref() else { return false };
        if trail.covered.get(i).is_some_and(|&c| c > 0) {
            return false;
        }
        let (read_layer, read_measured) =
            (trail.layer.as_ref().is_none_or(|(l, _)| *l != current), trail.measured.is_none());
        if read_layer {
            let pixels = self.current_pixels();
            if let Some(trail) = self.pour_trail.as_mut() {
                trail.layer = pixels.map(|p| (current, p));
            }
        }
        if read_measured {
            let measured = self.pour_measured_on(current);
            if let Some(trail) = self.pour_trail.as_mut() {
                trail.measured = Some(measured);
            }
        }
        let Some(trail) = self.pour_trail.as_ref() else { return false };
        let clear = |p: &[u8]| p.get(i * 4 + 3).is_some_and(|&a| a <= UNFILLED);
        trail.layer.as_ref().is_some_and(|(_, p)| clear(p))
            && trail.measured.as_ref().is_none_or(|m| m.as_deref().is_none_or(clear))
    }

    /// The visible layers composited together without the background, everything queued drawn
    /// first: what a pour measures with All Layers.
    fn layers_composite(&mut self) -> Option<Vec<u8>> {
        self.finish_stroke();
        self.settle();
        self.flush();
        let (Some(canvas), Some(gpu)) = (self.canvas.as_mut(), self.gpu.as_ref()) else { return None };
        canvas.read_layers_composite(gpu, &mut self.doc).ok()
    }
}

impl CanvasSession {
    /// Pours `colour` at window point (`x`, `y`) with `threshold` (see
    /// [`CanvasSession::pour_begin`]); `spread`: shown spreading in from the point.
    fn begin_pour(&self, x: f32, y: f32, colour: [f32; 3], threshold: f32, spread: bool) -> bool {
        self.pour_end(true, None);
        self.await_pour();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        // a fill still spreading in shows all of itself
        st.end_reveal();
        let [r, g, b] = colour;
        let Some(seed) = seed_at(st, x, y) else { return false };
        let (w, h) = (st.doc.width(), st.doc.height());
        let current = st.doc.current();
        let Some(layer) = st.doc.layer(current).cloned() else { return false };
        // on a vector layer the fill is traced from the strokes' lines and kept as a shape; its
        // threshold is how wide an opening between them it gets out through
        if layer.vector.is_some() {
            if st.doc.is_locked(current) {
                return false;
            }
            let Some(at) = st.window_to_canvas(x, y) else { return false };
            // dropped on what is picked, or on a stroke's ink, the colour is theirs
            st.finish_stroke();
            st.settle();
            if st.vector_recolour_at(current, at, [r, g, b].map(unit)) {
                self.shared.frame(st);
                return true;
            }
            // (the fill is kept whole: what was selected is let go of first)
            st.let_go_of_selection();
            let poured = st.vector_pour_begin(current, at, [r, g, b].map(unit), threshold);
            if poured {
                self.shared.frame(st);
            }
            return poured;
        }
        // a plugin's layer that takes a dropped colour makes of it what it will, as one step
        if layer.plugin.is_some() && !st.doc.is_locked(current) && st.plugin_layer_fills(current) {
            let at = Vec2::new(seed.0 as f32 + 0.5, seed.1 as f32 + 0.5);
            let done = st.fill_plugin_layer(current, at, [r, g, b], threshold);
            if done {
                self.shared.frame(st);
            }
            return done;
        }
        if st.doc.is_locked(current) || layer.drawn_from_data() {
            return false;
        }
        // (a Continuous drag knows the layer as its last fill left it, and what that was measured
        // on unless every layer is, which that fill changed)
        let trail = st.pour_trail.as_mut();
        let known = trail.and_then(|t| t.layer.take()).filter(|(l, _)| *l == current).map(|(_, p)| p);
        let Some(before) = known.or_else(|| st.current_pixels()) else { return false };
        let within = st.reach_selection();
        let all = st.pour_measures_all();
        let measured = st.pour_trail.as_ref().filter(|t| !all || t.kept == 0).and_then(|t| t.measured.clone());
        let elsewhere = match measured {
            Some(measured) => measured,
            None => st.pour_measured_on(current),
        };
        let mut pour = Pour {
            layer: current,
            before,
            flood: None,
            elsewhere,
            colour: [r, g, b].map(unit),
            selection: within,
            alpha_lock: layer.alpha_lock,
            shown: thousandths(threshold),
            shown_region: None,
            shown_mask: None,
            closed: None,
            finishing: None,
        };
        pour.measure(w, h, seed);
        if pour.flood.is_none() {
            return false;
        }
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.hold_layer(current);
        }
        pour.show(w, st.canvas.as_mut(), false);
        let region = pour.shown_region;
        st.pour = Some(pour);
        if let Some(region) = region.filter(|_| spread) {
            st.start_reveal(Vec2::new(x, y), region);
        }
        self.shared.frame(st);
        true
    }

    /// Continue filling's Continuous drag begins: what [`CanvasSession::pour_trail_at`] fills
    /// from here is one undo step, made by [`CanvasSession::pour_trail_end`].
    pub(crate) fn pour_trail_begin(&self) {
        self.pour_trail_end(true);
        self.pour_end(true, None);
        self.await_pour();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        st.pour_trail = Some(Trail::default());
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.mark_steps();
        }
    }

    /// Whether a Continuous drag is under way.
    pub(crate) fn pour_trailing(&self) -> bool {
        self.shared.lock().pour_trail.is_some()
    }

    /// The Continuous drag at window point (`x`, `y`): where nothing is filled yet, on the canvas
    /// and not reached by the drag already, `colour` (0..1) is poured with `threshold` and kept.
    pub(crate) fn pour_trail_at(&self, x: f32, y: f32, colour: [f32; 3], threshold: f32) -> TrailStep {
        if self.pour_finishing() {
            return TrailStep::Busy;
        }
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let (Some(seed), Some(at)) = (seed_at(st, x, y), st.window_to_canvas(x, y)) else { return TrailStep::Passed };
        let Some(trail) = st.pour_trail.as_ref() else { return TrailStep::Passed };
        if trail.missed.is_some_and(|m| (m.x - at.x).hypot(m.y - at.y) < TRAIL_MISS) {
            return TrailStep::Passed;
        }
        let current = st.doc.current();
        let Some(layer) = st.doc.layer(current) else { return TrailStep::Passed };
        // (text takes no colour until it is rasterized)
        let (vector, text) = (layer.vector.is_some(), layer.text.is_some());
        if st.doc.is_locked(current) || text {
            return TrailStep::Passed;
        }
        // on a vector layer one drop holds every fill the drag makes
        let filled = if vector {
            if !st.vector_unfilled(current, at) {
                return TrailStep::Passed;
            }
            let filled = st.vector_pour_more(current, at, colour.map(unit), threshold);
            self.shared.frame(st);
            filled
        } else {
            if !st.trail_unfilled(current, seed) {
                return TrailStep::Passed;
            }
            drop(guard);
            let filled = self.begin_pour(x, y, colour, threshold, false);
            if filled {
                self.pour_end(true, None);
            }
            guard = self.shared.lock();
            let st = &mut *guard;
            let (w, h) = (st.doc.width() as usize, st.doc.height() as usize);
            if let Some(trail) = st.pour_trail.as_mut() {
                trail.cover_pixel(seed.1 as usize * w + seed.0 as usize, w * h);
            }
            filled
        };
        if !filled && let Some(trail) = guard.pour_trail.as_mut() {
            trail.missed = Some(at);
        }
        if filled { TrailStep::Filled } else { TrailStep::Passed }
    }

    /// The Continuous drag ends: `keep` makes all it filled one undo step; otherwise it is all
    /// taken back, with no step left of it.
    pub(crate) fn pour_trail_end(&self, keep: bool) {
        if !self.pour_trailing() {
            return;
        }
        self.pour_end(keep, None);
        self.await_pour();
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(trail) = st.pour_trail.take() else { return };
        let Some(canvas) = st.canvas.as_mut() else { return };
        canvas.collapse_steps();
        if !keep && trail.kept > 0 {
            canvas.undo();
            st.flush();
            if let Some(canvas) = st.canvas.as_mut() {
                canvas.forget_redo();
            }
        }
        st.revision += 1;
        self.shared.frame(st);
    }

    /// Waits for a fill a plugin is finishing, if there is one.
    pub(crate) fn await_pour(&self) {
        let finishing = self.shared.lock().pour.as_ref().and_then(|pour| pour.finishing.clone());
        if let Some(finishing) = finishing {
            finishing.wait();
        }
    }

    /// Hands the open pour to a thread of its own, to end when the plugins on the fill hook have
    /// had their say; false if none wants to, and it is to end now.
    fn finish_pour(&self, st: &mut State, name: &Option<String>) -> bool {
        let Some(hooks) = st.hooks.clone().filter(|hooks| hooks.fill_mask()) else { return false };
        let Some(pour) = st.pour.as_mut() else { return false };
        let threshold = pour.shown as f32 / 1000.0;
        let Some(flood) = pour.flood.as_ref() else { return false };
        let (mask, seed) = (flood.mask(threshold), flood.seed());
        let finishing = Arc::new(Finishing::default());
        let (shared, ended, name) = (Arc::clone(&self.shared), Arc::clone(&finishing), name.clone());
        let work = move || {
            let closed = closed_by(&hooks, mask, seed);
            let mut guard = shared.lock();
            let st = &mut *guard;
            let width = st.doc.width();
            if let (Some(pour), Some(closed)) = (st.pour.as_mut(), closed) {
                pour.closed = Some(closed);
                pour.show(width, st.canvas.as_mut(), false);
            }
            end_pour(&shared, st, true, name.as_deref());
            drop(guard);
            ended.end();
        };
        if std::thread::Builder::new().name("fill".to_owned()).spawn(work).is_err() {
            return false;
        }
        pour.finishing = Some(finishing);
        true
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A mask filled all over.
    pub(crate) fn mask_of_box(width: u32, height: u32) -> Mask {
        Mask { width, height, data: vec![255; width as usize * height as usize] }
    }

    #[test]
    fn a_fill_goes_through_a_row_of_bits_and_comes_back_by_where_it_was() {
        use crate::plugin_run::tests::{FILL_AT, fill_module};
        use crate::plugin_run::{Hook, Running};
        // takes out the fifth pixel of every row it is handed
        let out = "(i64.store (i32.const 65536) (i64.const 16))";
        let hooks = Hooks::default();
        let module = fill_module(FILL_AT, "(i32.const 1)", out);
        hooks.put_on("fill".to_owned(), Running::start(&module, &[]).unwrap(), &[Hook::FillMask]);
        // a block of 10 x 3 at (20, 5) in a mask of 100 x 50, with soft values
        let mut mask = Mask { width: 100, height: 50, data: vec![0; 100 * 50] };
        for y in 5..8 {
            mask.data[y * 100 + 20..y * 100 + 30].fill(90);
        }
        let closed = closed_by(&hooks, mask.clone(), (22, 6)).unwrap();
        // its box and a pixel round it start at (19, 4): the fifth pixel is at 23
        for y in 5..8 {
            assert_eq!(closed.data[y * 100 + 23], 0, "{y}");
            assert_eq!(closed.data[y * 100 + 22], 90, "what stays keeps its value");
            assert_eq!(closed.data[y * 100 + 24], 90);
        }
        assert_eq!(closed.data.iter().filter(|&&v| v > 0).count(), 27);
    }

    #[test]
    fn a_box_at_the_picture_s_edge_has_nothing_past_it() {
        use crate::plugin_run::tests::fill_module;
        use crate::plugin_run::{Hook, Running};
        // it must be told 3 wide and 2 tall (nothing left or above, a pixel right and below) and
        // the seed at (1, 0), or it crashes
        let begin = "(if (i32.or (i32.ne (local.get 0) (i32.const 3)) (i32.or (i32.ne (local.get 1) (i32.const 2))
                (i32.or (i32.ne (local.get 2) (i32.const 1)) (i32.ne (local.get 3) (i32.const 0)))))
                (then (unreachable)))
             (i32.const 65536)";
        let hooks = Hooks::default();
        let module = fill_module(begin, "(i32.const 0)", "");
        hooks.put_on("fill".to_owned(), Running::start(&module, &[]).unwrap(), &[Hook::FillMask]);
        let mut mask = Mask { width: 100, height: 50, data: vec![0; 100 * 50] };
        mask.data[0..2].fill(255);
        assert!(closed_by(&hooks, mask, (1, 0)).is_none());
        assert_eq!(hooks.take_said(), None);
    }
}

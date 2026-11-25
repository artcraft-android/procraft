//! Transforming a layer's content: the box drawn around it, and how its handles, the toolbar's
//! actions and undo change where the content goes.
//!
//! The content's place is the quad its source rectangle's corners are carried to; the box is
//! usually that quad, but turning the box alone (the node below it) leaves the content where it is
//! and fits the box around it at the new angle. Every change to the box carries the content with
//! it through the map from the old box to the new one.
//!
//! A warp lays a 4 × 4 mesh (`warp.rs`) over the content; once there is one it places the content,
//! and the box is its corners.
//!
//! A puppet (`puppet.rs`) lays a mesh of triangles over the painted pixels and bends it by pins.
//! It bends the content where it is in the source rectangle, before the box places it: the box's
//! handles still move, turn and scale the puppet as a whole.

use std::sync::{Arc, Mutex};

use crate::geom::{Affine, Homography, Vec2};
use crate::puppet::{self, PuppetFrame, PuppetMesh, Solver};
use crate::warp::{self, Mesh};

/// How the box's handles change the content.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformMode {
    /// Corners and edges stretch the box along its own axes.
    Freeform,
    /// Corners and edges scale the box, keeping its proportions.
    Uniform,
    /// Corners move on their own; edges move both their corners.
    Distort,
    /// The content is bent through a mesh: its points, or the surface between them, are dragged.
    Warp,
    /// The content is bent by pins put on a mesh over it: a pin is dragged, or turned by its ring.
    Puppet,
}

/// What a drag holds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handle {
    /// A corner node: 0 top left, 1 top right, 2 bottom right, 3 bottom left.
    Corner(u8),
    /// The node in the middle of an edge: 0 top, 1 right, 2 bottom, 3 left.
    Edge(u8),
    /// The node above the box: turns the content about the box's centre.
    Rotate,
    /// The node below the box: turns the box alone, which then fits the content again.
    TurnBox,
    /// Anywhere else: moves the content (in a warp, drags the mesh's surface under the touch).
    Move,
    /// One of a warp's 16 control points, row by row.
    Mesh(u8),
    /// A puppet's pin, by its place among them: moves it.
    Pin(u32),
    /// The ring around a puppet's pin: turns the mesh about it.
    PinRing(u32),
    /// A puppet's mesh away from its pins: a tap puts a pin there.
    OnMesh,
}

/// A puppet's pin as a transform keeps it: where it is on the mesh at rest, where that point has
/// been taken, and the turn it holds there (none: it turns as the mesh bends), all in source
/// coordinates. Kept by place, not by triangle, so the mesh can be laid again under it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PuppetPin {
    pub rest: Vec2,
    pub at: Vec2,
    pub angle: Option<f32>,
}

/// Four corners: top left, top right, bottom right, bottom left.
pub type Quad = [Vec2; 4];

#[derive(Clone, Debug, PartialEq)]
struct Placement {
    content: Quad,
    frame: Quad,
    warp: Option<Mesh>,
    /// A puppet's pins; none (or none of them) leaves the content unbent.
    pins: Option<Arc<[PuppetPin]>>,
}

impl Placement {
    fn pins(&self) -> &[PuppetPin] {
        self.pins.as_deref().unwrap_or_default()
    }

    fn with_pins(&self, pins: Vec<PuppetPin>) -> Placement {
        Placement { pins: (!pins.is_empty()).then(|| pins.into()), ..self.clone() }
    }
}

/// How the content is bent, when it is: through a warp's mesh laid over the source rectangle, or
/// a puppet's. Two are alike when they bend alike (a puppet's when it is the same frame).
#[derive(Clone, Debug)]
pub enum Bend {
    Grid { mesh: Mesh, source: Quad },
    Puppet(Arc<PuppetFrame>),
}

impl PartialEq for Bend {
    fn eq(&self, other: &Bend) -> bool {
        match (self, other) {
            (Bend::Grid { mesh: a, source: s }, Bend::Grid { mesh: b, source: t }) => a == b && s == t,
            (Bend::Puppet(a), Bend::Puppet(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Bend {
    /// Where canvas point `p` of the content as it began goes.
    pub fn point(&self, p: Vec2) -> Vec2 {
        match self {
            Bend::Grid { mesh, source } => {
                let [top_left, top_right, _, bottom_left] = *source;
                let (w, h) = (top_right.x - top_left.x, bottom_left.y - top_left.y);
                if !(w.abs() > 1e-6 && h.abs() > 1e-6) {
                    return p;
                }
                warp::point(mesh, (p.x - top_left.x) / w, (p.y - top_left.y) / h)
            }
            Bend::Puppet(frame) => frame.point(p),
        }
    }
}

/// An open puppet: its mesh, whether new pins turn as it bends, the pin chosen, and what was last
/// worked out for it (shared by the transform's copies, keyed by what it was worked out for).
#[derive(Clone, Debug)]
struct PuppetRig {
    mesh: Arc<PuppetMesh>,
    auto_rotate: bool,
    selected: Option<usize>,
    worked: Arc<Mutex<Worked>>,
}

#[derive(Debug, Default)]
struct Worked {
    solver: Option<Arc<Solver>>,
    /// The pins and content quad the frame is for, the frame, and each pin as shown (canvas
    /// point and angle).
    frame: Option<(Vec<PuppetPin>, Quad, Arc<PuppetFrame>, Vec<(Vec2, f32)>)>,
}

/// An open transform.
#[derive(Clone, Debug)]
pub struct Transform {
    source: Quad,
    /// The convex hull of the content's pixels, in source coordinates.
    hull: Vec<Vec2>,
    now: Placement,
    history: Vec<Placement>,
    /// The handle held, where the drag started and the placement then.
    drag: Option<(Handle, Vec2, Placement)>,
    /// Lines a moved box snaps onto besides the canvas's own (canvas pixels): other layers'
    /// edges and middles, across (x) and down (y).
    snap_lines: (Vec<f32>, Vec<f32>),
    /// The lines (an x and a y, canvas pixels) the box held in a move has snapped onto now.
    snapped: [Option<f32>; 2],
    /// Pixel art: the content keeps to whole pixels however it is moved or sized ([`on_pixels`]).
    pixelated: bool,
    puppet: Option<PuppetRig>,
    /// A pin's ring held: where the pin is shown (canvas pixels) and the angle it held (source).
    ring: Option<(Vec2, f32)>,
}

/// What a drag is pulled towards.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Guides {
    /// Moves keep to 15° directions once past `lock_after`, and turns to 15° steps.
    pub magnetics: bool,
    /// How far (canvas pixels) a move goes before its direction locks.
    pub lock_after: f32,
    /// A moved box's edges and centre this near (canvas pixels) the canvas's edges or centre go
    /// onto them; none for no snapping.
    pub snap: Option<f32>,
    /// The canvas's size.
    pub canvas: Vec2,
}

/// Smallest scale a handle can bring the box to.
const MIN_SCALE: f32 = 0.01;

/// How much one pinch can scale what the box holds, from where the fingers took it: down to
/// 1/1024 and up to 1024 times.
const PINCH_SCALE: (f32, f32) = (1.0 / 1024.0, 1024.0);

/// The step magnetics keeps directions and turns to.
const MAGNETIC_STEP: f32 = std::f32::consts::PI / 12.0;

impl Transform {
    /// A transform of the content inside the rectangle (`x`, `y`, `width`, `height`, canvas
    /// pixels) whose pixels' convex hull is `hull`.
    pub fn new(x: f32, y: f32, width: f32, height: f32, hull: Vec<Vec2>) -> Transform {
        let source =
            [Vec2::new(x, y), Vec2::new(x + width, y), Vec2::new(x + width, y + height), Vec2::new(x, y + height)];
        let now = Placement { content: source, frame: source, warp: None, pins: None };
        Transform {
            source,
            hull,
            now,
            history: Vec::new(),
            drag: None,
            snap_lines: (Vec::new(), Vec::new()),
            snapped: [None; 2],
            pixelated: false,
            puppet: None,
            ring: None,
        }
    }

    /// Pixel art on or off: on, the content keeps to whole pixels from now on (and goes onto them
    /// now, which is no step of its own): moved by whole pixels, its edges on whole pixels when it
    /// is sized.
    pub fn set_pixelated(&mut self, on: bool) {
        self.pixelated = on;
        if on && self.drag.is_none() {
            self.now = on_pixels(&self.now);
        }
    }

    /// Whether the content lies along the canvas's own axes (unturned, or turned by quarter
    /// turns) with no warp: then its pixels stay whole pixels wherever it goes.
    pub fn is_upright(&self) -> bool {
        self.now.warp.is_none() && self.now.pins().is_empty() && upright(self.now.content)
    }

    /// The placement as it is kept: on whole pixels, for pixel art.
    fn kept(&self, p: Placement) -> Placement {
        if self.pixelated { on_pixels(&p) } else { p }
    }

    /// Other things a moved box snaps onto: their bounds (`x`, `y`, `width`, `height`, canvas
    /// pixels), by their edges and middles.
    pub fn set_snap_targets(&mut self, bounds: &[[f32; 4]]) {
        let ok = |v: &f32| v.is_finite();
        self.snap_lines = (
            bounds.iter().flat_map(|&[x, _, w, _]| [x, x + w / 2.0, x + w]).filter(ok).collect(),
            bounds.iter().flat_map(|&[_, y, _, h]| [y, y + h / 2.0, y + h]).filter(ok).collect(),
        );
    }

    /// The source rectangle's corners.
    pub fn source(&self) -> Quad {
        self.source
    }

    /// Where the source rectangle's corners are now.
    pub fn content(&self) -> Quad {
        self.now.content
    }

    /// Where the source rectangle's corners go as the content's pixels are drawn: where the
    /// content is, except that content only moved, turned by quarter turns or mirrored (its size
    /// and shape as they were, no warp) goes the nearest whole number of pixels, halves away from
    /// zero. Every pixel then lands on a pixel and is drawn as it was, whatever the filter, so
    /// moving a drawing never softens it.
    pub fn placed(&self) -> Quad {
        match self.now.warp {
            Some(_) => self.now.content,
            None => whole_pixel_move(self.source, self.now.content).unwrap_or(self.now.content),
        }
    }

    /// The box shown around the content.
    pub fn frame(&self) -> Quad {
        self.now.frame
    }

    /// The warp's mesh, once there is one.
    pub fn warp(&self) -> Option<Mesh> {
        self.now.warp
    }

    pub fn can_undo(&self) -> bool {
        !self.history.is_empty()
    }

    /// Lays a warp's mesh evenly over the content, unless there is one already (not a step of its
    /// own: nothing moves).
    pub fn start_warp(&mut self) {
        // a puppet and a warp don't bend together: its pins go, as a step of their own
        if !self.now.pins().is_empty() {
            self.change(|p| Some(p.with_pins(Vec::new())));
        }
        if self.now.warp.is_none() {
            self.now.warp = Some(warp::over_quad(self.now.content));
            self.now.frame = self.now.content;
        }
    }

    /// A drag of `handle` starts at canvas point `at`.
    pub fn grab(&mut self, handle: Handle, at: Vec2) {
        if let (Handle::Pin(i) | Handle::PinRing(i), Some(rig)) = (handle, self.puppet.as_mut()) {
            rig.selected = Some(i as usize);
        }
        // a ring turns the pin from the angle it shows: a free pin's, the turn the bend gives it
        self.ring = match handle {
            Handle::PinRing(i) => self.puppet_frame().and_then(|(_, shown)| {
                let (centre, _) = *shown.get(i as usize)?;
                let pin = self.now.pins().get(i as usize)?;
                Some((centre, pin.angle.or_else(|| self.source_angle(shown.get(i as usize)?.1))?))
            }),
            _ => None,
        };
        self.drag = Some((handle, at, self.now.clone()));
    }

    /// The drag reached canvas point `to`, in `mode`, pulled by `guides`. Returns whether
    /// anything moved.
    pub fn drag(&mut self, mode: TransformMode, to: Vec2, guides: &Guides) -> bool {
        let Some((handle, from, start)) = self.drag.clone() else { return false };
        if !to.is_finite() {
            return false;
        }
        let next = self.dragged(handle, mode, from, to, &start, guides).unwrap_or(start);
        let next = self.kept(next);
        let moved = next != self.now;
        let frame = next.frame;
        self.now = next;
        // a moved box sitting on a line it snaps to shows that line while held
        self.snapped = match (handle, guides.snap) {
            (Handle::Move, Some(_)) => lines_under(frame, guides.canvas, &self.snap_lines),
            _ => [None; 2],
        };
        moved
    }

    /// Two fingers held the content from `from` and have reached `to` (canvas points, each pair
    /// the same two fingers): it goes with them, moved, turned and scaled as a whole from where
    /// [`Transform::grab`] took it, the box with it (a box stays the shape it is, whatever the
    /// mode). Returns whether anything moved.
    pub fn pinch(&mut self, from: [Vec2; 2], to: [Vec2; 2]) -> bool {
        let Some((_, _, start)) = &self.drag else { return false };
        let (was, now) = (from[1] - from[0], to[1] - to[0]);
        if !(from.iter().chain(&to).all(|p| p.is_finite()) && was.length() > 1e-3 && now.length() > 1e-3) {
            return false;
        }
        let (middle_was, middle) = (from[0].lerp(from[1], 0.5), to[0].lerp(to[1], 0.5));
        let spread = now.length() / was.length();
        let scale = if spread.is_nan() { PINCH_SCALE.0 } else { spread.clamp(PINCH_SCALE.0, PINCH_SCALE.1) };
        let m = Affine::translate(Vec2::ZERO - middle_was)
            .then(Affine::scale(scale))
            .then(Affine::rotate(angle(now) - angle(was)))
            .then(Affine::translate(middle));
            let next = self.kept(map_placement(start, m));
            // pinched again and again, the content could go past what can be drawn (to a point, or out
            // past any number): it stays where the last step that could be drawn left it
            if !self.drawable(&next) {
                return false;
            }
        let moved = next != self.now;
        self.now = next;
        moved
    }

    /// Whether content placed as in `p` can be drawn: every point a number, the box's sides too,
    /// and its pixels found again from where they go (the map from the source to its place
    /// undoes).
    fn drawable(&self, p: &Placement) -> bool {
        let finite = p.content.iter().chain(&p.frame).chain(p.warp.iter().flatten()).all(|v| v.is_finite());
        let [a, b, _, d] = p.frame;
        finite
            && a.distance(b).is_finite()
            && a.distance(d).is_finite()
            && Homography::quad_to_quad(p.content, self.source).is_some()
    }

    /// The lines (x across, y down, canvas pixels) the box held in a move has snapped onto: shown
    /// as guides until it is let go.
    pub fn snapped(&self) -> [Option<f32>; 2] {
        self.snapped
    }

    /// The drag ends; a change is kept as an undo step.
    pub fn release(&mut self) {
        self.snapped = [None; 2];
        if let Some((_, _, start)) = self.drag.take()
            && start != self.now
        {
            self.history.push(start);
        }
    }

    /// The drag is abandoned (a second finger came down): the box goes back.
    pub fn cancel_drag(&mut self) {
        self.snapped = [None; 2];
        if let Some((_, _, start)) = self.drag.take() {
            self.now = start;
        }
    }

    /// Mirrors the content across the box's upright axis.
    pub fn flip_horizontal(&mut self) {
        let [a, b, c, d] = self.now.frame;
        self.change(|p| {
            let content = carry(p, [b, a, d, c])?;
            // a mesh mirrors by taking its columns in the other order
            let warp =
                p.warp.map(|m| std::array::from_fn(|k| m.get(k / 4 * 4 + 3 - k % 4).copied().unwrap_or_default()));
            Some(Placement { content, frame: p.frame, warp, pins: p.pins.clone() })
        });
    }

    /// Mirrors the content across the box's level axis.
    pub fn flip_vertical(&mut self) {
        let [a, b, c, d] = self.now.frame;
        self.change(|p| {
            let content = carry(p, [d, c, b, a])?;
            let warp =
                p.warp.map(|m| std::array::from_fn(|k| m.get((3 - k / 4) * 4 + k % 4).copied().unwrap_or_default()));
            Some(Placement { content, frame: p.frame, warp, pins: p.pins.clone() })
        });
    }

    /// Turns the box and content by `radians` (clockwise on screen) about the box's centre.
    pub fn rotate(&mut self, radians: f32) {
        let c = centre(self.now.frame);
        let m = Affine::translate(Vec2::ZERO - c).then(Affine::rotate(radians)).then(Affine::translate(c));
        self.change(|p| Some(map_placement(p, m)));
    }

    /// The box's width and height (canvas pixels), along its top and left edges.
    pub fn size(&self) -> (f32, f32) {
        let [a, b, _, d] = self.now.frame;
        (a.distance(b), a.distance(d))
    }

    /// How far the box is turned from upright (radians, clockwise on screen).
    pub fn angle(&self) -> f32 {
        let [a, b, ..] = self.now.frame;
        angle(b - a)
    }

    /// Makes the box `width` × `height` (canvas pixels) about its centre, turned as it is: a
    /// size typed in. A negative size mirrors the content that way.
    pub fn set_size(&mut self, width: f32, height: f32) {
        let (w, h) = self.size();
        if !(width.is_finite() && height.is_finite()) || width.abs() < 1.0 || height.abs() < 1.0 || w <= 0.0 || h <= 0.0
        {
            return;
        }
        let c = centre(self.now.frame);
        let turn = self.angle();
        let stretch = Affine { a: width.abs() / w, d: height.abs() / h, ..Affine::IDENTITY };
        let m = Affine::translate(Vec2::ZERO - c)
            .then(Affine::rotate(-turn))
            .then(stretch)
            .then(Affine::rotate(turn))
            .then(Affine::translate(c));
        self.change(|p| Some(map_placement(p, m)));
        if width < 0.0 {
            self.flip_horizontal();
        }
        if height < 0.0 {
            self.flip_vertical();
        }
    }

    /// Turns the box to `radians` from upright (clockwise on screen) about its centre: an angle
    /// typed in.
    pub fn set_angle(&mut self, radians: f32) {
        if radians.is_finite() {
            self.rotate(radians - self.angle());
        }
    }

    /// Scales the box and content alike, keeping their proportions, their turn and any flip, so
    /// that the box as it is turned (the upright rectangle around it, a warp's mesh too) just fits
    /// a `width` × `height` canvas, centred on it.
    pub fn fit(&mut self, width: f32, height: f32) {
        let mut mesh = self.now.warp.map(|m| m.to_vec()).unwrap_or_default();
        // a puppet's mesh as it is bent, too
        if let Some((frame, _)) = self.puppet_frame() {
            mesh.extend(frame.edge_lines().as_chunks::<2>().0.iter().map(|&[x, y]| Vec2::new(x, y)));
        }
        let (mut low, mut high) = (Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN));
        for p in self.now.frame.iter().chain(&mesh) {
            low = Vec2::new(low.x.min(p.x), low.y.min(p.y));
            high = Vec2::new(high.x.max(p.x), high.y.max(p.y));
        }
        let (w, h) = (high.x - low.x, high.y - low.y);
        if !(w > 0.0 && h > 0.0) {
            return;
        }
        let k = (width / w).min(height / h);
        let middle = Vec2::new((low.x + high.x) / 2.0, (low.y + high.y) / 2.0);
        let m = Affine::translate(Vec2::ZERO - middle)
            .then(Affine { a: k, d: k, ..Affine::IDENTITY })
            .then(Affine::translate(Vec2::new(width / 2.0, height / 2.0)));
        self.change(|p| Some(map_placement(p, m)));
    }

    /// Moves the content one canvas pixel towards `toward`, along whichever axis it is further
    /// along from the box's centre.
    pub fn nudge(&mut self, toward: Vec2) {
        let d = toward - centre(self.now.frame);
        if !d.is_finite() || d == Vec2::ZERO {
            return;
        }
        let step = if d.x.abs() >= d.y.abs() { Vec2::new(d.x.signum(), 0.0) } else { Vec2::new(0.0, d.y.signum()) };
        self.change(|p| Some(map_placement(p, Affine::translate(step))));
    }

    /// Puts the content back where it started.
    pub fn reset(&mut self) {
        let source = self.source;
        self.change(|_| Some(Placement { content: source, frame: source, warp: None, pins: None }));
    }

    /// Takes back the last change. Returns whether there was one.
    pub fn undo(&mut self) -> bool {
        self.drag = None;
        match self.history.pop() {
            Some(p) => {
                self.now = p;
                true
            }
            None => false,
        }
    }

    /// Applies `f` to the placement, kept as an undo step when it changes anything.
    fn change(&mut self, f: impl FnOnce(&Placement) -> Option<Placement>) {
        self.drag = None;
        let finite = |n: &Placement| {
            n.content.iter().chain(&n.frame).chain(n.warp.iter().flatten()).all(|v| v.is_finite())
                && n.pins().iter().all(|p| p.rest.is_finite() && p.at.is_finite() && p.angle.is_none_or(f32::is_finite))
        };
        if let Some(next) = f(&self.now).filter(finite).map(|next| self.kept(next))
            && next != self.now
        {
            let was = std::mem::replace(&mut self.now, next);
            self.history.push(was);
        }
    }

    fn dragged(
        &self,
        handle: Handle,
        mode: TransformMode,
        from: Vec2,
        to: Vec2,
        start: &Placement,
        guides: &Guides,
    ) -> Option<Placement> {
        let f = start.frame;
        let delta = to - from;
        if mode == TransformMode::Warp
            && let Some(mesh) = start.warp
        {
            return Some(warped(mesh, handle, from, delta, start.pins.clone()));
        }
        match handle {
            Handle::Pin(i) => return self.pin_dragged(start, i as usize, from, to),
            Handle::PinRing(i) => return self.pin_turned(start, i as usize, from, to, guides),
            Handle::OnMesh => return Some(start.clone()),
            _ => {}
        }
        let distort = mode == TransformMode::Distort;
        let next = match handle {
            Handle::Move => {
                let mut delta = delta;
                if guides.magnetics && delta.length() > guides.lock_after {
                    // the direction keeps to the 15° step at or below it
                    let step = (angle(delta) / MAGNETIC_STEP).floor() * MAGNETIC_STEP;
                    let (sin, cos) = step.sin_cos();
                    let along = delta.x * cos + delta.y * sin;
                    delta = Vec2::new(cos, sin) * along;
                }
                if let Some(reach) = guides.snap {
                    delta = delta + snap_offset(f.map(|p| p + delta), guides.canvas, &self.snap_lines, reach);
                }
                map_placement(start, Affine::translate(delta))
            }
            Handle::Rotate => {
                let c = centre(f);
                let mut turn = angle(to - c) - angle(from - c);
                if guides.magnetics {
                    let [a, b, ..] = f;
                    let was = angle(b - a);
                    turn = ((was + turn) / MAGNETIC_STEP).round() * MAGNETIC_STEP - was;
                }
                map_placement(
                    start,
                    Affine::translate(Vec2::ZERO - c).then(Affine::rotate(turn)).then(Affine::translate(c)),
                )
            }
            Handle::TurnBox => {
                let c = centre(f);
                let turn = angle(to - c) - angle(from - c);
                let [a, b, ..] = f;
                let frame = self.fit_box(&start, angle(b - a) + turn)?;
                Placement { frame, ..start.clone() }
            }
            Handle::Mesh(_) | Handle::Pin(_) | Handle::PinRing(_) | Handle::OnMesh => start.clone(),
            Handle::Corner(i) => {
                let i = usize::from(i % 4);
                let (corner, anchor) = (at(f, i), at(f, i + 2));
                if distort {
                    let mut frame = f;
                    set(&mut frame, i, corner + delta);
                    return carry_all(start, frame);
                }
                let target = corner + delta;
                let m = if mode == TransformMode::Uniform {
                    scale_about(anchor, projected_scale(target, corner, anchor))
                } else {
                    // the target in the box's own axes, against the corner's
                    let basis = basis(f)?.inverse()?;
                    let (t, c) = (basis.apply(target - anchor), basis.apply(corner - anchor));
                    let sx = if c.x.abs() > 1e-6 { (t.x / c.x).max(MIN_SCALE) } else { 1.0 };
                    let sy = if c.y.abs() > 1e-6 { (t.y / c.y).max(MIN_SCALE) } else { 1.0 };
                    axes_scale_about(f, anchor, sx, sy)?
                };
                map_placement(start, m)
            }
            Handle::Edge(k) => {
                let k = usize::from(k % 4);
                if distort {
                    let mut frame = f;
                    set(&mut frame, k, at(f, k) + delta);
                    set(&mut frame, k + 1, at(f, k + 1) + delta);
                    return carry_all(start, frame);
                }
                let mid = at(f, k).lerp(at(f, k + 1), 0.5);
                let anchor = at(f, k + 2).lerp(at(f, k + 3), 0.5);
                let s = projected_scale(mid + delta, mid, anchor);
                let m = if mode == TransformMode::Uniform {
                    scale_about(anchor, s)
                } else if k % 2 == 0 {
                    axes_scale_about(f, anchor, 1.0, s)?
                } else {
                    axes_scale_about(f, anchor, s, 1.0)?
                };
                map_placement(start, m)
            }
        };
        Some(next)
    }

    /// The box at `radians` that just holds the content placed as in `p`.
    fn fit_box(&self, p: &Placement, radians: f32) -> Option<Quad> {
        let to_content = Homography::quad_to_quad(self.source, p.content)?;
        let [s0, s1, _, s3] = self.source;
        let (w, h) = (s1.x - s0.x, s3.y - s0.y);
        let place = |q: Vec2| match p.warp {
            Some(m) if w > 0.0 && h > 0.0 => Some(warp::point(&m, (q.x - s0.x) / w, (q.y - s0.y) / h)),
            _ => to_content.apply(q),
        };
        let points: Vec<Vec2> = if self.hull.is_empty() {
            p.warp.map_or(p.content.to_vec(), |m| m.to_vec())
        } else {
            self.hull.iter().filter_map(|&q| place(q)).collect()
        };
        let (sin, cos) = radians.sin_cos();
        let (u, v) = (Vec2::new(cos, sin), Vec2::new(-sin, cos));
        let dot = |a: Vec2, b: Vec2| a.x * b.x + a.y * b.y;
        let (mut u0, mut u1, mut v0, mut v1) = (f32::MAX, f32::MIN, f32::MAX, f32::MIN);
        for q in &points {
            let (pu, pv) = (dot(*q, u), dot(*q, v));
            u0 = u0.min(pu);
            u1 = u1.max(pu);
            v0 = v0.min(pv);
            v1 = v1.max(pv);
        }
        (u0 <= u1 && v0 <= v1).then(|| [u * u0 + v * v0, u * u1 + v * v0, u * u1 + v * v1, u * u0 + v * v1])
    }
}

/// The convex hull of `points`, without repeats.
pub fn convex_hull(points: &[Vec2]) -> Vec<Vec2> {
    let mut pts: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    pts.sort_by(|a, b| a.x.total_cmp(&b.x).then(a.y.total_cmp(&b.y)));
    pts.dedup();
    if pts.len() < 3 {
        return pts;
    }
    let mut lower = half_hull(pts.iter());
    let mut upper = half_hull(pts.iter().rev());
    // each half ends where the other starts
    lower.pop();
    upper.pop();
    lower.append(&mut upper);
    lower
}

/// One side of the hull of points sorted along x (monotone chain).
fn half_hull<'a>(points: impl Iterator<Item = &'a Vec2>) -> Vec<Vec2> {
    let cross = |o: Vec2, a: Vec2, b: Vec2| (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x);
    let mut chain: Vec<Vec2> = Vec::new();
    for &p in points {
        while let [.., a, b] = *chain.as_slice() {
            if cross(a, b, p) > 0.0 {
                break;
            }
            chain.pop();
        }
        chain.push(p);
    }
    chain
}

/// How far to shift `frame` so that the nearest of its left, middle and right (and top, middle and
/// bottom) lands on a line, the canvas's edges and middle or one of `others` (across, down),
/// when one is within `reach`.
fn snap_offset(frame: Quad, canvas: Vec2, others: &(Vec<f32>, Vec<f32>), reach: f32) -> Vec2 {
    let axis = |values: [f32; 4], size: f32, others: &[f32]| {
        let (lo, hi) = values.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let features = [lo, (lo + hi) / 2.0, hi];
        let lines = [0.0, size / 2.0, size];
        let mut best: Option<f32> = None;
        for f in features {
            for &l in lines.iter().chain(others) {
                let d = l - f;
                if d.abs() <= reach && best.is_none_or(|b| d.abs() < b.abs()) {
                    best = Some(d);
                }
            }
        }
        best.unwrap_or(0.0)
    };
    Vec2::new(axis(frame.map(|p| p.x), canvas.x, &others.0), axis(frame.map(|p| p.y), canvas.y, &others.1))
}

/// The snapping lines (the canvas's edges and middle, then `others`) a box's edges or centre lie
/// on, across (x) and down (y).
fn lines_under(frame: Quad, canvas: Vec2, others: &(Vec<f32>, Vec<f32>)) -> [Option<f32>; 2] {
    let axis = |values: [f32; 4], size: f32, others: &[f32]| {
        let (lo, hi) = values.iter().fold((f32::MAX, f32::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
        let lines = [0.0, size / 2.0, size];
        lines.iter().chain(others).copied().find(|&l| [lo, (lo + hi) / 2.0, hi].iter().any(|f| (l - f).abs() < 0.01))
    };
    [axis(frame.map(|p| p.x), canvas.x, &others.0), axis(frame.map(|p| p.y), canvas.y, &others.1)]
}

fn at(q: Quad, i: usize) -> Vec2 {
    q.get(i % 4).copied().unwrap_or_default()
}

fn set(q: &mut Quad, i: usize, v: Vec2) {
    if let Some(p) = q.get_mut(i % 4) {
        *p = v;
    }
}

fn centre(q: Quad) -> Vec2 {
    let [a, b, c, d] = q;
    (a + b + c + d) * 0.25
}

fn angle(v: Vec2) -> f32 {
    v.y.atan2(v.x)
}

/// The scale about `anchor` that brings `point` nearest `target` along the line through them.
fn projected_scale(target: Vec2, point: Vec2, anchor: Vec2) -> f32 {
    let (t, p) = (target - anchor, point - anchor);
    let len2 = p.x * p.x + p.y * p.y;
    if len2 <= 1e-12 {
        return 1.0;
    }
    ((t.x * p.x + t.y * p.y) / len2).max(MIN_SCALE)
}

fn scale_about(anchor: Vec2, s: f32) -> Affine {
    Affine::translate(Vec2::ZERO - anchor).then(Affine::scale(s)).then(Affine::translate(anchor))
}

/// The box's own axes (its top and left edges) as a linear map from unit axes.
fn basis(f: Quad) -> Option<Affine> {
    let [a, b, _, d] = f;
    let (e1, e2) = (b - a, d - a);
    let m = Affine { a: e1.x, b: e1.y, c: e2.x, d: e2.y, e: 0.0, f: 0.0 };
    (m.determinant().abs() > 1e-9).then_some(m)
}

/// Scaling by `sx` along the box's top edge and `sy` along its left edge, about `anchor`.
fn axes_scale_about(f: Quad, anchor: Vec2, sx: f32, sy: f32) -> Option<Affine> {
    let b = basis(f)?;
    let s = Affine { a: sx, d: sy, ..Affine::IDENTITY };
    Some(Affine::translate(Vec2::ZERO - anchor).then(b.inverse()?).then(s).then(b).then(Affine::translate(anchor)))
}

/// Whether a quad lies along the canvas's own axes: a rectangle unturned, or turned by quarter turns.
fn upright(q: Quad) -> bool {
    let [a, b, c, d] = q;
    let near = |x: f32, y: f32| (x - y).abs() <= 1e-3;
    (near(a.y, b.y) && near(b.x, c.x) && near(c.y, d.y) && near(d.x, a.x))
        || (near(a.x, b.x) && near(b.y, c.y) && near(c.x, d.x) && near(d.y, a.y))
}

/// A stretch from `lo` to `hi` on whole pixels: a whole number of them long (one at least), the
/// end nearer a whole pixel staying where it is, so a stretch already on them moves as a whole
/// and one being sized keeps its other end.
fn whole_span(lo: f32, hi: f32) -> (f32, f32) {
    let size = (hi - lo).round().max(1.0);
    let off = |v: f32| (v - v.round()).abs();
    if off(hi) < off(lo) { (hi.round() - size, hi.round()) } else { (lo.round(), lo.round() + size) }
}

/// A placement kept to whole pixels, for pixel art. Content lying along the canvas's own axes has
/// its edges brought onto whole pixels, so it moves and is sized by whole pixels and each of its
/// pixels lands on whole ones; content turned any other way has its middle brought onto a pixel's
/// corner or middle, so it moves by half pixels at most. A warp is left as it is.
fn on_pixels(p: &Placement) -> Placement {
    if p.warp.is_some() {
        return p.clone();
    }
    if !upright(p.content) {
        let c = centre(p.content);
        let to = Vec2::new((c.x * 2.0).round() / 2.0, (c.y * 2.0).round() / 2.0);
        return map_placement(p, Affine::translate(to - c));
    }
    let (lo, hi) =
        p.content.iter().fold((Vec2::new(f32::MAX, f32::MAX), Vec2::new(f32::MIN, f32::MIN)), |(lo, hi), v| {
            (Vec2::new(lo.x.min(v.x), lo.y.min(v.y)), Vec2::new(hi.x.max(v.x), hi.y.max(v.y)))
        });
    let (w, h) = (hi.x - lo.x, hi.y - lo.y);
    if !(w > 1e-6 && h > 1e-6) {
        return p.clone();
    }
    let ((x0, x1), (y0, y1)) = (whole_span(lo.x, hi.x), whole_span(lo.y, hi.y));
    let m = Affine::translate(Vec2::ZERO - lo)
        .then(Affine { a: (x1 - x0) / w, d: (y1 - y0) / h, ..Affine::IDENTITY })
        .then(Affine::translate(Vec2::new(x0, y0)));
    map_placement(p, m)
}

fn map_placement(p: &Placement, m: Affine) -> Placement {
    Placement {
        content: p.content.map(|v| m.apply(v)),
        frame: p.frame.map(|v| m.apply(v)),
        warp: p.warp.map(|w| w.map(|v| m.apply(v))),
        pins: p.pins.clone(),
    }
}

/// Everything carried along as the box goes from where it is to `frame`.
fn carry_all(p: &Placement, frame: Quad) -> Option<Placement> {
    let h = Homography::quad_to_quad(p.frame, frame)?;
    let content = carry(p, frame)?;
    let warp = match p.warp {
        Some(m) => {
            let mut out = m;
            for v in &mut out {
                *v = h.apply(*v)?;
            }
            Some(out)
        }
        None => None,
    };
    Some(Placement { content, frame, warp, pins: p.pins.clone() })
}

/// A drag in a warp: a control point (a corner node is its control point), or the surface under the
/// touch; off the mesh, the whole mesh moves. The box follows the mesh's corners.
fn warped(mesh: Mesh, handle: Handle, from: Vec2, delta: Vec2, pins: Option<Arc<[PuppetPin]>>) -> Placement {
    let point = match handle {
        Handle::Mesh(k) => Some(usize::from(k % 16)),
        Handle::Corner(i) => [0, 3, 15, 12].get(usize::from(i % 4)).copied(),
        _ => None,
    };
    let next = match point {
        Some(k) => {
            let mut m = mesh;
            if let Some(p) = m.get_mut(k) {
                *p = *p + delta;
            }
            m
        }
        None => match warp::locate(&mesh, from) {
            Some((u, v)) => warp::drag_surface(&mesh, u, v, delta),
            None => mesh.map(|p| p + delta),
        },
    };
    let corners = [next[0], next[3], next[15], next[12]];
    Placement { content: corners, frame: corners, warp: Some(next), pins }
}

/// How far (canvas pixels) each corner may be from an exact move, quarter turn or mirror and still
/// count as one: more than the rounding of corners kept as `f32` on the biggest canvas, less than
/// any change a hand or a typed number makes.
const WHOLE_PIXEL_REACH: f32 = 0.01;

/// `to` as the upright rectangle `from` moved the nearest whole number of pixels: when `to` is
/// `from` moved, turned by quarter turns or mirrored, to within [`WHOLE_PIXEL_REACH`] at each
/// corner; none otherwise.
fn whole_pixel_move(from: Quad, to: Quad) -> Option<Quad> {
    let [a, b, _, d] = from;
    let (width, height) = (b.x - a.x, d.y - a.y);
    if !(width > 0.0 && height > 0.0 && b.y == a.y && d.x == a.x) {
        return None;
    }
    // where the rectangle's own axes went, each to the nearest of the four ways along the canvas's
    let [p, q, _, s] = to;
    let axis = |v: Vec2, length: f32| Vec2::new((v.x / length).round(), (v.y / length).round());
    let (across, down) = (axis(q - p, width), axis(s - p, height));
    let unit = |v: Vec2| v.x.abs() + v.y.abs() == 1.0;
    if !(unit(across) && unit(down) && across.dot(down) == 0.0) {
        return None;
    }
    let turned = |v: Vec2| across * v.x + down * v.y;
    // the move: what the corners say on average, which each must say to within the reach
    let offset = from.iter().zip(&to).fold(Vec2::ZERO, |sum, (f, t)| sum + (*t - turned(*f))) * 0.25;
    if !offset.is_finite() || from.iter().zip(&to).any(|(f, t)| t.distance(turned(*f) + offset) > WHOLE_PIXEL_REACH) {
        return None;
    }
    let whole = Vec2::new(offset.x.round(), offset.y.round());
    Some(from.map(|f| turned(f) + whole))
}

/// The content carried along as the box goes from where it is to `frame`.
fn carry(p: &Placement, frame: Quad) -> Option<Quad> {
    let h = Homography::quad_to_quad(p.frame, frame)?;
    let [a, b, c, d] = p.content;
    Some([h.apply(a)?, h.apply(b)?, h.apply(c)?, h.apply(d)?])
}

/// A puppet: its mesh laid over the content, pins put on it, moved and turned.
impl Transform {
    /// Lays a puppet's mesh over the content, its pins (if it has any) laid again on it where they
    /// are at rest. A warp goes, as a step of its own: the two don't bend together.
    pub fn start_puppet(&mut self, mesh: PuppetMesh) {
        if self.now.warp.is_some() {
            self.change(|p| Some(Placement { warp: None, ..p.clone() }));
        }
        let (auto_rotate, selected) = self.puppet.as_ref().map_or((true, None), |r| (r.auto_rotate, r.selected));
        self.puppet =
            Some(PuppetRig { mesh: Arc::new(mesh), auto_rotate, selected, worked: Arc::new(Mutex::new(Worked::default())) });
    }

    /// Whether a puppet's mesh is laid.
    pub fn has_puppet(&self) -> bool {
        self.puppet.is_some()
    }

    /// Whether new pins turn as the mesh bends (else each holds the turn it was put at).
    pub fn puppet_auto_rotate(&self) -> bool {
        self.puppet.as_ref().is_none_or(|r| r.auto_rotate)
    }

    /// Pins turn as the mesh bends from now on, or not; those there are freed, or held at the
    /// angle they show, as a step.
    pub fn set_puppet_auto_rotate(&mut self, on: bool) {
        let shown = self.puppet_frame().map(|(_, s)| s).unwrap_or_default();
        let held: Vec<Option<f32>> = shown.iter().map(|&(_, a)| self.source_angle(a)).collect();
        let Some(rig) = self.puppet.as_mut() else { return };
        rig.auto_rotate = on;
        let pins: Vec<PuppetPin> = self
            .now
            .pins()
            .iter()
            .enumerate()
            .map(|(k, p)| PuppetPin {
                angle: if on { None } else { p.angle.or_else(|| held.get(k).copied().flatten()).or(Some(0.0)) },
                ..*p
            })
            .collect();
        self.change(|p| Some(p.with_pins(pins)));
    }

    /// Puts a pin where canvas point `at` is on the mesh as it is bent, and chooses it. Whether
    /// one went (not off the mesh).
    pub fn add_pin(&mut self, at: Vec2) -> bool {
        let Some((frame, _)) = self.puppet_frame() else { return false };
        let Some(to_source) = frame.place.inverse() else { return false };
        let Some(at) = to_source.apply(at) else { return false };
        let Some((rest, turn)) = frame.find_bent(at) else { return false };
        let auto = self.puppet_auto_rotate();
        let mut pins = self.now.pins().to_vec();
        pins.push(PuppetPin { rest, at, angle: (!auto).then_some(turn) });
        let added = pins.len() - 1;
        self.change(|p| Some(p.with_pins(pins)));
        if let Some(rig) = self.puppet.as_mut() {
            rig.selected = Some(added);
        }
        true
    }

    /// Whether canvas point `at` is on the puppet's mesh as it is bent.
    pub fn on_puppet(&self, at: Vec2) -> bool {
        self.puppet_frame().is_some_and(|(frame, _)| {
            frame.place.inverse().and_then(|h| h.apply(at)).is_some_and(|p| frame.find_bent(p).is_some())
        })
    }

    /// Takes pin `i` away, as a step.
    pub fn remove_pin(&mut self, i: usize) {
        let mut pins = self.now.pins().to_vec();
        if i >= pins.len() {
            return;
        }
        pins.remove(i);
        self.change(|p| Some(p.with_pins(pins)));
        if let Some(rig) = self.puppet.as_mut() {
            rig.selected = None;
        }
    }

    /// The pin chosen, if any.
    pub fn selected_pin(&self) -> Option<usize> {
        self.puppet.as_ref().and_then(|r| r.selected).filter(|&i| i < self.now.pins().len())
    }

    pub fn select_pin(&mut self, pin: Option<usize>) {
        if let Some(rig) = self.puppet.as_mut() {
            rig.selected = pin;
        }
    }

    /// Each pin as it is shown: where (canvas pixels) and the angle it holds or the bend gives it
    /// (radians, clockwise on screen).
    pub fn puppet_pins(&self) -> Vec<(Vec2, f32)> {
        self.puppet_frame().map(|(_, shown)| shown).unwrap_or_default()
    }

    /// The puppet's mesh as it is shown: its edges (x0, y0, x1, y1 in turn, canvas pixels).
    pub fn puppet_edges(&self) -> Vec<f32> {
        self.puppet_frame().map(|(frame, _)| frame.edge_lines()).unwrap_or_default()
    }

    /// How the content is bent, if it is: by a warp, or by a puppet with pins.
    pub fn bend(&self) -> Option<Bend> {
        if let Some(mesh) = self.now.warp {
            return Some(Bend::Grid { mesh, source: self.source });
        }
        if self.now.pins().is_empty() {
            return None;
        }
        self.puppet_frame().map(|(frame, _)| Bend::Puppet(frame))
    }

    /// The puppet as it is now, worked out (or as it was last, when nothing has changed), and
    /// each pin as it is shown.
    fn puppet_frame(&self) -> Option<(Arc<PuppetFrame>, Vec<(Vec2, f32)>)> {
        let rig = self.puppet.as_ref()?;
        let (pins, content) = (self.now.pins(), self.now.content);
        let mut worked = rig.worked.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((p, q, frame, shown)) = &worked.frame
            && p.as_slice() == pins
            && *q == content
        {
            return Some((frame.clone(), shown.clone()));
        }
        let place = Homography::quad_to_quad(self.source, content)?;
        // each pin laid on the mesh where it is at rest (one the mesh no longer reaches is left out)
        let laid: Vec<(usize, puppet::Pin)> = pins
            .iter()
            .enumerate()
            .filter_map(|(k, p)| {
                let pin = puppet::Pin::new(&rig.mesh, p.rest, p.angle)?;
                Some((k, puppet::Pin { at: p.at, ..pin }))
            })
            .collect();
        let bound: Vec<puppet::Pin> = laid.iter().map(|(_, p)| *p).collect();
        let solver = match &worked.solver {
            Some(s) if s.fits(&bound) => s.clone(),
            _ => Arc::new(Solver::new(&rig.mesh, &bound)),
        };
        worked.solver = Some(solver.clone());
        let bent = solver.solve(&rig.mesh, &bound);
        let mut shown: Vec<(Vec2, f32)> =
            pins.iter().map(|p| (place.apply(p.at).unwrap_or(p.at), self.shown_angle(p.angle.unwrap_or(0.0)))).collect();
        for ((k, _), angle) in laid.iter().zip(&bent.angles) {
            if let Some(s) = shown.get_mut(*k) {
                s.1 = self.shown_angle(*angle);
            }
        }
        let frame = Arc::new(PuppetFrame { mesh: rig.mesh.clone(), bent, place });
        worked.frame = Some((pins.to_vec(), content, frame.clone(), shown.clone()));
        Some((frame, shown))
    }

    /// Whether the box has mirrored the content.
    fn mirrored(&self) -> bool {
        let [a, b, _, d] = self.now.content;
        (b - a).cross(d - a) < 0.0
    }

    /// A turn of the source as the box shows it (radians, clockwise on screen).
    fn shown_angle(&self, source: f32) -> f32 {
        let [a, b, ..] = self.now.content;
        angle(b - a) + if self.mirrored() { -source } else { source }
    }

    /// A turn as shown, back in the source; none if the box has flattened it.
    fn source_angle(&self, shown: f32) -> Option<f32> {
        let [a, b, ..] = self.now.content;
        let turn = shown - angle(b - a);
        turn.is_finite().then_some(if self.mirrored() { -turn } else { turn })
    }

    /// Pin `i` dragged from canvas point `from` to `to`.
    fn pin_dragged(&self, start: &Placement, i: usize, from: Vec2, to: Vec2) -> Option<Placement> {
        let to_source = Homography::quad_to_quad(start.content, self.source)?;
        let moved = to_source.apply(to)? - to_source.apply(from)?;
        let mut pins = start.pins().to_vec();
        let pin = pins.get_mut(i)?;
        pin.at = pin.at + moved;
        Some(start.with_pins(pins))
    }

    /// Pin `i`'s ring turned from canvas point `from` to `to`: it holds the turn it is given.
    fn pin_turned(&self, start: &Placement, i: usize, from: Vec2, to: Vec2, guides: &Guides) -> Option<Placement> {
        let (centre, held) = self.ring?;
        let mut turn = angle(to - centre) - angle(from - centre);
        if self.mirrored() {
            turn = -turn;
        }
        let mut next = held + turn;
        if guides.magnetics {
            next = (next / MAGNETIC_STEP).round() * MAGNETIC_STEP;
        }
        let mut pins = start.pins().to_vec();
        pins.get_mut(i)?.angle = Some(next);
        Some(start.with_pins(pins))
    }
}

/// For a transform that keys a pose rather than moving pixels (a timeline clip's, with autokey):
/// whether a drag is under way, and the content and box put where `m` carries the source.
impl Transform {
    /// Whether a handle is held.
    pub fn dragging(&self) -> bool {
        self.drag.is_some()
    }

    /// The content and its box go where `m` carries the source rectangle (no warp), as no step
    /// of its own: what the transform's undo takes back is kept.
    pub fn place(&mut self, m: Affine) {
        let source = Placement { content: self.source, frame: self.source, warp: None, pins: None };
        self.now = map_placement(&source, m);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: Vec2, b: Vec2) -> bool {
        a.distance(b) < 1e-3
    }

    fn quad_close(a: Quad, b: Quad) -> bool {
        a.iter().zip(&b).all(|(p, q)| close(*p, *q))
    }

    fn rect(x: f32, y: f32, w: f32, h: f32) -> Quad {
        [Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)]
    }

    const NONE: Guides = Guides { magnetics: false, lock_after: 0.0, snap: None, canvas: Vec2::ZERO };

    fn box_100x200() -> Transform {
        Transform::new(100.0, 100.0, 100.0, 200.0, Vec::new())
    }

    #[test]
    fn moving_carries_box_and_content_and_undoes() {
        let mut t = box_100x200();
        t.grab(Handle::Move, Vec2::new(150.0, 150.0));
        assert!(t.drag(TransformMode::Uniform, Vec2::new(160.0, 130.0), &NONE));
        t.release();
        assert!(quad_close(t.content(), rect(110.0, 80.0, 100.0, 200.0)));
        assert!(quad_close(t.frame(), t.content()));
        assert!(t.can_undo());
        assert!(t.undo());
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0, 200.0)));
        assert!(!t.undo());
    }

    #[test]
    fn uniform_corners_scale_about_the_opposite_corner() {
        let mut t = box_100x200();
        t.grab(Handle::Corner(2), Vec2::new(200.0, 300.0));
        // along the diagonal: twice the size
        t.drag(TransformMode::Uniform, Vec2::new(300.0, 500.0), &NONE);
        assert!(quad_close(t.content(), rect(100.0, 100.0, 200.0, 400.0)));
        // off the diagonal: the nearest point on it
        t.drag(TransformMode::Uniform, Vec2::new(300.0, 300.0), &NONE);
        let s = (200.0 * 100.0 + 200.0 * 200.0) / (100.0 * 100.0 + 200.0 * 200.0);
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0 * s, 200.0 * s)));
    }

    #[test]
    fn uniform_edges_scale_about_the_opposite_edge() {
        let mut t = box_100x200();
        t.grab(Handle::Edge(1), Vec2::new(200.0, 200.0));
        t.drag(TransformMode::Uniform, Vec2::new(250.0, 260.0), &NONE);
        // the left edge's middle (100, 200) stays
        assert!(quad_close(t.content(), rect(100.0, 50.0, 150.0, 300.0)));
    }

    #[test]
    fn freeform_stretches_along_the_box() {
        let mut t = box_100x200();
        t.grab(Handle::Corner(2), Vec2::new(200.0, 300.0));
        t.drag(TransformMode::Freeform, Vec2::new(300.0, 320.0), &NONE);
        assert!(quad_close(t.content(), rect(100.0, 100.0, 200.0, 220.0)));
        t.release();
        t.grab(Handle::Edge(0), Vec2::new(200.0, 100.0));
        t.drag(TransformMode::Freeform, Vec2::new(500.0, 210.0), &NONE);
        assert!(quad_close(t.content(), rect(100.0, 210.0, 200.0, 110.0)));
        // and along a turned box's own axes
        let mut t = box_100x200();
        t.rotate(std::f32::consts::FRAC_PI_2);
        let [_, _, br, _] = t.frame();
        t.grab(Handle::Corner(2), br);
        t.drag(TransformMode::Freeform, br + Vec2::new(-50.0, 0.0), &NONE);
        let [a, b, _, d] = t.frame();
        assert!((a.distance(b) - 100.0).abs() < 1e-3 && (a.distance(d) - 250.0).abs() < 1e-3);
    }

    #[test]
    fn distort_moves_corners_and_edges_on_their_own() {
        let mut t = box_100x200();
        t.grab(Handle::Corner(1), Vec2::new(200.0, 100.0));
        t.drag(TransformMode::Distort, Vec2::new(230.0, 80.0), &NONE);
        let [a, b, c, d] = t.content();
        assert!(close(a, Vec2::new(100.0, 100.0)) && close(b, Vec2::new(230.0, 80.0)));
        assert!(close(c, Vec2::new(200.0, 300.0)) && close(d, Vec2::new(100.0, 300.0)));
        t.release();
        t.grab(Handle::Edge(3), Vec2::new(100.0, 200.0));
        t.drag(TransformMode::Distort, Vec2::new(90.0, 200.0), &NONE);
        let [a, _, _, d] = t.content();
        assert!(close(a, Vec2::new(90.0, 100.0)) && close(d, Vec2::new(90.0, 300.0)));
    }

    #[test]
    fn rotating_turns_about_the_centre_and_a_cancelled_drag_goes_back() {
        let mut t = box_100x200();
        t.grab(Handle::Rotate, Vec2::new(150.0, 50.0));
        // from straight above the centre (150, 200) to straight right of it: a quarter turn
        t.drag(TransformMode::Uniform, Vec2::new(400.0, 200.0), &NONE);
        let [a, ..] = t.content();
        assert!(close(a, Vec2::new(250.0, 150.0)), "{a:?}");
        t.cancel_drag();
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0, 200.0)));
        assert!(!t.can_undo());
    }

    #[test]
    fn two_fingers_move_turn_and_scale_the_content_about_them_as_one_step() {
        let mut t = box_100x200();
        let (a, b) = (Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0));
        t.grab(Handle::Move, a);
        // spread to twice as far apart, a quarter turn clockwise, about the first finger
        assert!(t.pinch([a, b], [a, Vec2::new(100.0, 300.0)]));
        // the box's top left corner was under the first finger, and stays there
        let [tl, tr, ..] = t.frame();
        assert!(close(tl, a) && close(tr, Vec2::new(100.0, 300.0)), "{tl:?} {tr:?}");
        assert!(quad_close(t.content(), t.frame()));
        // measured from where they took it, not step by step
        assert!(t.pinch([a, b], [a, b]));
        assert!(quad_close(t.frame(), rect(100.0, 100.0, 100.0, 200.0)));
        assert!(t.pinch([a, b], [a + Vec2::new(10.0, 0.0), b + Vec2::new(10.0, 0.0)]));
        t.release();
        assert!(t.can_undo());
        t.undo();
        assert!(!t.can_undo());
        assert!(quad_close(t.frame(), rect(100.0, 100.0, 100.0, 200.0)));
    }

    #[test]
    fn a_pinch_needs_the_content_taken_and_two_fingers_apart() {
        let mut t = box_100x200();
        let (a, b) = (Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0));
        assert!(!t.pinch([a, b], [a, a + Vec2::new(0.0, 50.0)]));
        t.grab(Handle::Move, a);
        assert!(!t.pinch([a, a], [a, b]));
        assert!(!t.pinch([a, b], [a, a]));
        assert!(!t.pinch([a, b], [a, Vec2::new(f32::NAN, 0.0)]));
        // fingers brought together leave the box small, never turned inside out
        assert!(t.pinch([a, b], [a, a + Vec2::new(0.01, 0.0)]));
        let [tl, tr, ..] = t.frame();
        assert!(tr.x > tl.x);
        t.cancel_drag();
        assert!(quad_close(t.frame(), rect(100.0, 100.0, 100.0, 200.0)));
    }

    /// The width of the box, and whether what it holds can be drawn where it is.
    fn width_drawable(t: &Transform) -> (f32, bool) {
        let content = t.content();
        let drawable = content.iter().chain(&t.frame()).all(|p| p.is_finite())
            && Homography::quad_to_quad(content, t.source()).is_some();
        (t.size().0, drawable)
    }

    #[test]
    fn one_pinch_scales_1024_times_at_most_and_pinches_again_and_again_leave_it_drawable() {
        let mut t = box_100x200();
        let (a, b) = (Vec2::new(150.0, 200.0), Vec2::new(250.0, 200.0));
        // fingers 100 apart brought to a hair: a 1024th of the size, no less
        t.grab(Handle::Move, a);
        assert!(t.pinch([a, b], [Vec2::new(199.9994, 200.0), Vec2::new(200.0006, 200.0)]));
        t.release();
        assert!((t.size().0 - 100.0 / 1024.0).abs() < 1e-4, "{:?}", t.size());
        // a hair apart spread wide: 1024 times, no more
        let mut wide = box_100x200();
        wide.grab(Handle::Move, a);
        assert!(wide.pinch([Vec2::new(199.9994, 200.0), Vec2::new(200.0006, 200.0)], [a, b]));
        assert!((wide.size().0 - 100.0 * 1024.0).abs() < 1.0, "{:?}", wide.size());
        // pinched in quickly again and again, about wherever the fingers are: it gets as small as
        // can still be drawn, and stays drawable
        for k in 0..40 {
            let (a, b) = (Vec2::new(100.0 + k as f32, 300.0), Vec2::new(300.0 + k as f32, 300.0));
            t.grab(Handle::Move, a);
            t.pinch([a, b], [Vec2::new(199.9994, 300.0), Vec2::new(200.0006, 300.0)]);
            t.release();
            let (width, drawable) = width_drawable(&t);
            assert!(drawable && width > 0.0, "pinch {k}: {:?}", t.frame());
        }
        // and spread out again and again, it stays a number
        for k in 0..40 {
            let (a, b) = (Vec2::new(199.9994, 300.0), Vec2::new(200.0006, 300.0));
            wide.grab(Handle::Move, a);
            wide.pinch([a, b], [Vec2::new(100.0 - k as f32, 300.0), Vec2::new(300.0 + k as f32, 300.0)]);
            wide.release();
            let (width, drawable) = width_drawable(&wide);
            assert!(drawable && width.is_finite(), "pinch {k}: {:?}", wide.frame());
        }
        // every step back undoes, to where it started
        while t.undo() {}
        assert!(quad_close(t.frame(), rect(100.0, 100.0, 100.0, 200.0)));
    }

    #[test]
    fn turning_the_box_refits_it_and_leaves_the_content() {
        let hull = vec![Vec2::new(100.0, 100.0), Vec2::new(200.0, 100.0), Vec2::new(100.0, 300.0)];
        let mut t = Transform::new(100.0, 100.0, 100.0, 200.0, hull);
        // the node below the box, a quarter turn clockwise about the centre
        t.grab(Handle::TurnBox, Vec2::new(150.0, 350.0));
        t.drag(TransformMode::Uniform, Vec2::new(0.0, 200.0), &NONE);
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0, 200.0)));
        let [a, b, c, d] = t.frame();
        // a quarter turn: the box's top runs down the screen; it still just holds the triangle
        assert!(close(a, Vec2::new(200.0, 100.0)) && close(b, Vec2::new(200.0, 300.0)), "{a:?} {b:?}");
        assert!(close(c, Vec2::new(100.0, 300.0)) && close(d, Vec2::new(100.0, 100.0)), "{c:?} {d:?}");
    }

    #[test]
    fn flips_rotation_fit_and_reset() {
        let mut t = box_100x200();
        t.flip_horizontal();
        let [a, b, ..] = t.content();
        assert!(close(a, Vec2::new(200.0, 100.0)) && close(b, Vec2::new(100.0, 100.0)));
        assert!(quad_close(t.frame(), rect(100.0, 100.0, 100.0, 200.0)));
        t.flip_horizontal();
        t.flip_vertical();
        let [a, .., d] = t.content();
        assert!(close(a, Vec2::new(100.0, 300.0)) && close(d, Vec2::new(100.0, 100.0)));
        t.reset();
        t.rotate(std::f32::consts::FRAC_PI_4);
        assert!(close(centre(t.frame()), Vec2::new(150.0, 200.0)));
        let [a, b, ..] = t.frame();
        assert!((angle(b - a) - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
        // fitted turned as it is: the turned box's upright bounds fill the canvas's side
        t.fit(1000.0, 1000.0);
        let [a, b, ..] = t.frame();
        assert!((angle(b - a) - std::f32::consts::FRAC_PI_4).abs() < 1e-4);
        assert!(close(centre(t.frame()), Vec2::new(500.0, 500.0)));
        let (w, h) = t.size();
        let k = 1000.0 / (300.0 / std::f32::consts::SQRT_2);
        assert!((w - 100.0 * k).abs() < 1e-2 && (h - 200.0 * k).abs() < 1e-2, "{w} {h}");
        let xs = t.frame().map(|p| p.x);
        assert!(xs.iter().any(|x| x.abs() < 1e-2) && xs.iter().any(|x| (x - 1000.0).abs() < 1e-2), "{xs:?}");
        t.reset();
        assert!(quad_close(t.content(), rect(100.0, 100.0, 100.0, 200.0)));
        // every action was a step
        let mut steps = 0;
        while t.undo() {
            steps += 1;
        }
        assert_eq!(steps, 7);
        // a reset with nothing changed is not a step
        t.reset();
        assert!(!t.can_undo());
    }

    #[test]
    fn fit_to_canvas_keeps_flips_and_turns() {
        // flipped across: fitted, the content's left edge is still on the box's right
        let mut t = box_100x200();
        t.flip_horizontal();
        t.fit(1000.0, 1000.0);
        assert!(quad_close(t.frame(), rect(250.0, 0.0, 500.0, 1000.0)));
        let [a, b, c, d] = t.content();
        assert!(close(a, Vec2::new(750.0, 0.0)) && close(b, Vec2::new(250.0, 0.0)), "{a:?} {b:?}");
        assert!(close(c, Vec2::new(250.0, 1000.0)) && close(d, Vec2::new(750.0, 1000.0)), "{c:?} {d:?}");
        // flipped up and down: its top is at the bottom
        let mut t = box_100x200();
        t.flip_vertical();
        t.fit(1000.0, 1000.0);
        let [a, b, ..] = t.content();
        assert!(close(a, Vec2::new(250.0, 1000.0)) && close(b, Vec2::new(750.0, 1000.0)), "{a:?} {b:?}");
        // a quarter turn: the turned box is 200 across and 100 down, so it fits the width
        let mut t = box_100x200();
        t.rotate(std::f32::consts::FRAC_PI_2);
        t.fit(1000.0, 1000.0);
        let [a, b, c, d] = t.frame();
        assert!(close(a, Vec2::new(1000.0, 250.0)) && close(b, Vec2::new(1000.0, 750.0)), "{a:?} {b:?}");
        assert!(close(c, Vec2::new(0.0, 750.0)) && close(d, Vec2::new(0.0, 250.0)), "{c:?} {d:?}");
        assert!(quad_close(t.content(), t.frame()));
        // turned and flipped: both kept
        let mut t = box_100x200();
        t.flip_horizontal();
        t.rotate(std::f32::consts::FRAC_PI_2);
        t.fit(1000.0, 1000.0);
        let [fa, fb, fc, fd] = t.frame();
        assert!(quad_close(t.content(), [fb, fa, fd, fc]));
        assert!(close(fa, Vec2::new(1000.0, 250.0)));
        // undone in one step, flipped as before
        let before = t.content();
        t.fit(500.0, 300.0);
        assert!(t.undo());
        assert!(quad_close(t.content(), before));
    }

    #[test]
    fn magnetics_keep_moves_and_turns_to_15_degree_steps() {
        let magnetic = Guides { magnetics: true, lock_after: 4.0, ..NONE };
        let mut t = box_100x200();
        t.grab(Handle::Move, Vec2::new(150.0, 200.0));
        // a short wobble is free
        t.drag(TransformMode::Uniform, Vec2::new(152.0, 201.0), &magnetic);
        assert!(close(t.content()[0], Vec2::new(102.0, 101.0)));
        // 10° below level keeps to level (the step at or below it)
        let d = Vec2::new(100.0 * 10f32.to_radians().cos(), 100.0 * 10f32.to_radians().sin());
        t.drag(TransformMode::Uniform, Vec2::new(150.0, 200.0) + d, &magnetic);
        let [a, ..] = t.content();
        assert!((a.y - 100.0).abs() < 1e-3 && (a.x - (100.0 + d.x)).abs() < 1e-3, "{a:?}");
        t.cancel_drag();
        // a turn of 40° keeps to 45°
        let c = Vec2::new(150.0, 200.0);
        t.grab(Handle::Rotate, c + Vec2::new(0.0, -100.0));
        let turned = (-90f32 + 40.0).to_radians();
        t.drag(TransformMode::Uniform, c + Vec2::new(turned.cos(), turned.sin()) * 100.0, &magnetic);
        let [a, b, ..] = t.frame();
        assert!((angle(b - a).to_degrees() - 45.0).abs() < 1e-3);
    }

    #[test]
    fn snapping_pulls_a_moved_box_onto_the_canvas_lines() {
        let snap = Guides { snap: Some(10.0), canvas: Vec2::new(1000.0, 1000.0), ..NONE };
        let mut t = box_100x200();
        t.grab(Handle::Move, Vec2::new(150.0, 200.0));
        // the left edge 6 from the canvas's edge, the middle 4 from the canvas's middle: the nearer wins
        t.drag(TransformMode::Uniform, Vec2::new(56.0, 504.0), &snap);
        let [a, ..] = t.content();
        assert!(close(a, Vec2::new(0.0, 400.0)), "{a:?}");
        // the lines it snapped onto show while it is held: the canvas's left edge and its middle
        // (the box's middle, 400 + 100, on it)
        assert_eq!(t.snapped(), [Some(0.0), Some(500.0)]);
        // out of reach: free, no lines
        t.drag(TransformMode::Uniform, Vec2::new(80.0, 470.0), &snap);
        assert!(close(t.content()[0], Vec2::new(30.0, 370.0)));
        assert_eq!(t.snapped(), [None, None]);
        // another layer's right edge at 333 takes the box's left edge, 3 off
        t.set_snap_targets(&[[233.0, 700.0, 100.0, 100.0]]);
        t.drag(TransformMode::Uniform, Vec2::new(380.0, 470.0), &snap);
        assert!(close(t.content()[0], Vec2::new(333.0, 370.0)), "{:?}", t.content()[0]);
        assert_eq!(t.snapped(), [Some(333.0), None]);
        // let go, the lines go
        t.release();
        assert_eq!(t.snapped(), [None, None]);
    }

    #[test]
    fn a_nudge_moves_one_pixel_towards_the_tap() {
        let mut t = box_100x200();
        t.nudge(Vec2::new(500.0, 210.0));
        assert!(close(t.content()[0], Vec2::new(101.0, 100.0)));
        t.nudge(Vec2::new(151.0, 0.0));
        assert!(close(t.content()[0], Vec2::new(101.0, 99.0)));
        t.nudge(centre(t.frame()));
        assert!(t.undo() && t.undo() && !t.undo());
    }

    #[test]
    fn a_warp_bends_through_its_mesh_and_flips_and_resets() {
        let mut t = box_100x200();
        t.start_warp();
        let m = t.warp().unwrap();
        assert!(close(m[5], Vec2::new(100.0 + 100.0 / 3.0, 100.0 + 200.0 / 3.0)));
        // a corner node is its control point
        t.grab(Handle::Corner(2), Vec2::new(200.0, 300.0));
        t.drag(TransformMode::Warp, Vec2::new(230.0, 310.0), &NONE);
        t.release();
        assert!(close(t.warp().unwrap()[15], Vec2::new(230.0, 310.0)));
        assert!(close(t.frame()[2], Vec2::new(230.0, 310.0)));
        // the surface under the touch follows it
        let before = t.warp().unwrap();
        let at = warp::point(&before, 0.5, 0.5);
        t.grab(Handle::Move, at);
        t.drag(TransformMode::Warp, at + Vec2::new(20.0, 0.0), &NONE);
        assert!(close(warp::point(&t.warp().unwrap(), 0.5, 0.5), at + Vec2::new(20.0, 0.0)));
        // off the mesh: all of it moves
        t.grab(Handle::Move, Vec2::new(0.0, 0.0));
        t.drag(TransformMode::Warp, Vec2::new(5.0, 0.0), &NONE);
        assert!(close(t.warp().unwrap()[0], before[0] + Vec2::new(5.0, 0.0)));
        t.release();
        t.flip_horizontal();
        let flipped = t.warp().unwrap();
        assert!(close(flipped[0], before[3] + Vec2::new(5.0, 0.0)));
        t.rotate(std::f32::consts::FRAC_PI_2);
        t.reset();
        assert_eq!(t.warp(), None);
        assert!(t.undo() && t.warp().is_some());
    }

    #[test]
    fn hull_of_a_square_with_inner_points() {
        let pts = [
            Vec2::new(0.0, 0.0),
            Vec2::new(2.0, 0.0),
            Vec2::new(1.0, 1.0),
            Vec2::new(2.0, 2.0),
            Vec2::new(0.0, 2.0),
            Vec2::new(1.0, 0.0),
            Vec2::new(0.0, 0.0),
        ];
        let hull = convex_hull(&pts);
        assert_eq!(hull.len(), 4, "{hull:?}");
        for corner in [Vec2::new(0.0, 0.0), Vec2::new(2.0, 0.0), Vec2::new(2.0, 2.0), Vec2::new(0.0, 2.0)] {
            assert!(hull.contains(&corner));
        }
        assert_eq!(convex_hull(&[Vec2::new(1.0, 1.0)]).len(), 1);
    }

    #[test]
    fn a_size_and_an_angle_typed_in() {
        let mut t = box_100x200();
        let c = centre(t.frame());
        t.set_size(50.0, 400.0);
        let (w, h) = t.size();
        assert!((w - 50.0).abs() < 1e-3 && (h - 400.0).abs() < 1e-3, "{w} {h}");
        assert!(close(centre(t.frame()), c));
        t.set_angle(30f32.to_radians());
        assert!((t.angle().to_degrees() - 30.0).abs() < 1e-3);
        // turned, a size keeps the turn
        t.set_size(100.0, 100.0);
        assert!((t.angle().to_degrees() - 30.0).abs() < 1e-3);
        let (w, h) = t.size();
        assert!((w - 100.0).abs() < 1e-2 && (h - 100.0).abs() < 1e-2);
        // nothing for sizes under a pixel or not numbers
        t.set_size(0.5, 10.0);
        t.set_size(f32::NAN, 10.0);
        assert!((t.size().0 - 100.0).abs() < 1e-2);
        assert!(t.undo() && t.undo() && t.undo() && !t.undo());
    }

    #[test]
    fn pixel_art_keeps_the_content_on_whole_pixels() {
        let mut t = Transform::new(10.0, 20.0, 100.0, 200.0, Vec::new());
        t.set_pixelated(true);
        // moved: by whole pixels, the size it was
        t.grab(Handle::Move, Vec2::new(50.0, 50.0));
        t.drag(TransformMode::Freeform, Vec2::new(53.4, 48.7), &NONE);
        t.release();
        assert!(quad_close(t.content(), rect(13.0, 19.0, 100.0, 200.0)), "{:?}", t.content());
        // sized by its right edge: that edge onto a whole pixel, the left one staying
        t.grab(Handle::Edge(1), Vec2::new(113.0, 119.0));
        t.drag(TransformMode::Freeform, Vec2::new(120.6, 119.0), &NONE);
        t.release();
        assert!(quad_close(t.content(), rect(13.0, 19.0, 108.0, 200.0)), "{:?}", t.content());
        // and by its left edge: the right one staying
        t.grab(Handle::Edge(3), Vec2::new(13.0, 119.0));
        t.drag(TransformMode::Freeform, Vec2::new(20.3, 119.0), &NONE);
        t.release();
        assert!(quad_close(t.content(), rect(20.0, 19.0, 101.0, 200.0)), "{:?}", t.content());
        // never thinner than a pixel
        t.grab(Handle::Edge(1), Vec2::new(121.0, 119.0));
        t.drag(TransformMode::Freeform, Vec2::new(20.2, 119.0), &NONE);
        t.release();
        assert!(quad_close(t.content(), rect(20.0, 19.0, 1.0, 200.0)), "{:?}", t.content());
        assert!(t.undo());
        // a quarter turn is still along the canvas's axes, on whole pixels
        assert!(t.is_upright());
        t.rotate(std::f32::consts::FRAC_PI_2);
        assert!(t.is_upright());
        assert!(t.content().iter().all(|p| (p.x - p.x.round()).abs() < 1e-3 && (p.y - p.y.round()).abs() < 1e-3));
        // any other turn isn't: its middle goes onto a half pixel at most
        t.rotate(0.5);
        assert!(!t.is_upright());
        let c = centre(t.content());
        assert!(((c.x * 2.0).round() - c.x * 2.0).abs() < 1e-3 && ((c.y * 2.0).round() - c.y * 2.0).abs() < 1e-3);
        // off, it moves freely again
        t.set_pixelated(false);
        t.reset();
        t.grab(Handle::Move, Vec2::new(50.0, 50.0));
        t.drag(TransformMode::Freeform, Vec2::new(50.4, 50.0), &NONE);
        assert!(quad_close(t.content(), rect(10.4, 20.0, 100.0, 200.0)));
    }

    /// A transform of a bar 200 × 40 at (100, 100), its puppet's mesh laid.
    fn puppet() -> Transform {
        let mut t = Transform::new(100.0, 100.0, 200.0, 40.0, Vec::new());
        let mesh = PuppetMesh::build(&[255u8; 200 * 40], 200, Vec2::new(100.0, 100.0), puppet::MeshSettings::default());
        t.start_puppet(mesh.unwrap());
        t
    }

    #[test]
    fn pins_go_on_the_mesh_and_each_change_is_a_step() {
        let mut t = puppet();
        assert!(t.has_puppet() && t.bend().is_none() && !t.puppet_edges().is_empty());
        assert!(!t.add_pin(Vec2::new(50.0, 50.0)), "off the mesh");
        assert!(t.add_pin(Vec2::new(110.0, 120.0)) && t.add_pin(Vec2::new(290.0, 120.0)));
        assert_eq!(t.selected_pin(), Some(1));
        assert_eq!(t.puppet_pins().len(), 2);
        // the far pin dragged down: the bar bends, the near end stays
        t.grab(Handle::Pin(1), Vec2::new(290.0, 120.0));
        assert!(t.drag(TransformMode::Puppet, Vec2::new(270.0, 200.0), &NONE));
        t.release();
        let bend = t.bend().unwrap();
        assert!(close_to(bend.point(Vec2::new(290.0, 120.0)), Vec2::new(270.0, 200.0), 0.5));
        assert!(close_to(bend.point(Vec2::new(110.0, 120.0)), Vec2::new(110.0, 120.0), 0.5));
        assert!(close_to(t.puppet_pins()[1].0, Vec2::new(270.0, 200.0), 1e-3));
        // the ring turns the mesh about its pin
        let (centre, before) = t.puppet_pins()[1];
        t.grab(Handle::PinRing(1), centre + Vec2::new(30.0, 0.0));
        t.drag(TransformMode::Puppet, centre + Vec2::new(0.0, 30.0), &NONE);
        t.release();
        let after = t.puppet_pins()[1].1;
        assert!((after - before - std::f32::consts::FRAC_PI_2).abs() < 1e-3, "{before} {after}");
        // each was a step: undone, the pin is free and where it was taken, then back where it was put
        assert!(t.undo());
        assert!((t.puppet_pins()[1].1 - before).abs() < 1e-3);
        assert!(t.undo());
        assert!(close_to(t.puppet_pins()[1].0, Vec2::new(290.0, 120.0), 1e-3));
        t.remove_pin(0);
        assert_eq!(t.puppet_pins().len(), 1);
        assert!(t.undo() && t.puppet_pins().len() == 2);
        // a warp takes the pins away, as a step
        t.start_warp();
        assert!(matches!(t.bend(), Some(Bend::Grid { .. })));
        assert!(t.undo() && t.warp().is_none() && t.puppet_pins().len() == 2);
    }

    fn close_to(a: Vec2, b: Vec2, tolerance: f32) -> bool {
        a.distance(b) <= tolerance
    }

    #[test]
    fn a_puppet_bends_before_the_box_places_it() {
        let mut t = puppet();
        assert!(t.add_pin(Vec2::new(110.0, 120.0)) && t.add_pin(Vec2::new(290.0, 120.0)));
        // the box moved and turned a quarter: the pins go with it
        t.grab(Handle::Move, Vec2::ZERO);
        t.drag(TransformMode::Freeform, Vec2::new(50.0, 0.0), &NONE);
        t.release();
        assert!(close_to(t.puppet_pins()[0].0, Vec2::new(160.0, 120.0), 1e-3));
        t.rotate(std::f32::consts::FRAC_PI_2);
        let [(near, _), (far, _)] = [t.puppet_pins()[0], t.puppet_pins()[1]];
        assert!((near.x - far.x).abs() < 1e-2 && far.y > near.y, "{near:?} {far:?}");
        // a pin dragged on the canvas goes where it is dragged
        t.grab(Handle::Pin(1), far);
        t.drag(TransformMode::Puppet, far + Vec2::new(40.0, -10.0), &NONE);
        t.release();
        assert!(close_to(t.puppet_pins()[1].0, far + Vec2::new(40.0, -10.0), 1e-2));
        assert!(close_to(t.bend().unwrap().point(Vec2::new(290.0, 120.0)), far + Vec2::new(40.0, -10.0), 0.5));
        // a pin put on the bent mesh goes where it is put
        let middle = t.bend().unwrap().point(Vec2::new(200.0, 120.0));
        assert!(t.add_pin(middle));
        assert!(close_to(t.puppet_pins()[2].0, middle, 1e-2));
    }

    #[test]
    fn auto_rotate_off_holds_each_pin_at_the_turn_it_shows() {
        let mut t = puppet();
        assert!(t.puppet_auto_rotate());
        assert!(t.add_pin(Vec2::new(110.0, 120.0)) && t.add_pin(Vec2::new(290.0, 120.0)));
        t.grab(Handle::Pin(1), Vec2::new(290.0, 120.0));
        t.drag(TransformMode::Puppet, Vec2::new(250.0, 220.0), &NONE);
        t.release();
        let shown = t.puppet_pins();
        t.set_puppet_auto_rotate(false);
        assert!(!t.puppet_auto_rotate());
        for (a, b) in t.puppet_pins().iter().zip(&shown) {
            assert!((a.1 - b.1).abs() < 0.05, "{a:?} {b:?}");
        }
        // a pin put now holds the turn there
        assert!(t.add_pin(t.bend().unwrap().point(Vec2::new(200.0, 120.0))));
    }

    #[test]
    fn content_moved_lands_on_whole_pixels_halves_away_from_zero() {
        let mut t = box_100x200();
        assert_eq!(t.placed(), t.source());
        t.grab(Handle::Move, Vec2::new(150.0, 150.0));
        t.drag(TransformMode::Uniform, Vec2::new(150.5, 149.5), &NONE);
        // the box where it was dragged; the pixels half a pixel on, away from zero
        assert!(quad_close(t.content(), rect(100.5, 99.5, 100.0, 200.0)));
        assert_eq!(t.placed(), rect(101.0, 99.0, 100.0, 200.0));
        t.drag(TransformMode::Uniform, Vec2::new(153.3, 147.8), &NONE);
        assert_eq!(t.placed(), rect(103.0, 98.0, 100.0, 200.0));
        t.drag(TransformMode::Uniform, Vec2::new(149.6, 150.4), &NONE);
        assert_eq!(t.placed(), rect(100.0, 100.0, 100.0, 200.0));
        // far out on a big canvas, where corners kept as f32 are rounded: still a move
        let mut far = Transform::new(15000.0, 9000.0, 3.0, 2.0, Vec::new());
        far.grab(Handle::Move, Vec2::new(15001.0, 9001.0));
        far.drag(TransformMode::Uniform, Vec2::new(15001.3, 9000.6), &NONE);
        assert_eq!(far.placed(), rect(15000.0, 9000.0, 3.0, 2.0));
    }

    #[test]
    fn content_turned_by_quarters_or_mirrored_lands_on_whole_pixels() {
        let mut t = box_100x200();
        t.grab(Handle::Move, Vec2::new(150.0, 150.0));
        t.drag(TransformMode::Uniform, Vec2::new(150.3, 150.6), &NONE);
        t.release();
        let whole = |q: Quad| q.iter().all(|p| p.x.fract() == 0.0 && p.y.fract() == 0.0);
        // a quarter turn about the box's middle, (150.3, 200.6): the corners come within half a
        // pixel of where it puts them, each on a whole pixel
        t.rotate(std::f32::consts::FRAC_PI_2);
        let (placed, content) = (t.placed(), t.content());
        assert!(whole(placed), "{placed:?}");
        assert!(placed.iter().zip(&content).all(|(p, c)| p.distance(*c) <= 0.71), "{placed:?} {content:?}");
        // the source's top left corner went to the top right, its top edge down the right side
        assert_eq!(placed[1] - placed[0], Vec2::new(0.0, 100.0));
        assert_eq!(placed[3] - placed[0], Vec2::new(-200.0, 0.0));
        // two eighths of a turn make a quarter as well
        t.undo();
        t.rotate(std::f32::consts::FRAC_PI_4);
        t.rotate(std::f32::consts::FRAC_PI_4);
        assert!(whole(t.placed()), "{:?}", t.placed());
        t.flip_horizontal();
        t.flip_vertical();
        assert!(whole(t.placed()), "{:?}", t.placed());
    }

    #[test]
    fn content_scaled_turned_distorted_or_warped_lands_where_it_is() {
        let mut t = box_100x200();
        t.rotate(std::f32::consts::FRAC_PI_4);
        assert_eq!(t.placed(), t.content());
        t.reset();
        t.rotate(0.3);
        assert_eq!(t.placed(), t.content());
        // a turn too small to see goes on whole pixels
        t.reset();
        t.rotate(1e-6);
        assert_eq!(t.placed(), t.source());
        t.reset();
        t.set_size(100.4, 200.0);
        assert_eq!(t.placed(), t.content());
        t.reset();
        t.grab(Handle::Corner(2), Vec2::new(200.0, 300.0));
        t.drag(TransformMode::Distort, Vec2::new(203.0, 301.0), &NONE);
        t.release();
        assert_eq!(t.placed(), t.content());
        // a warp's mesh, moved as a whole by a fraction, is drawn through the mesh
        t.reset();
        t.start_warp();
        t.grab(Handle::Move, Vec2::new(-50.0, -50.0));
        t.drag(TransformMode::Warp, Vec2::new(-49.5, -50.0), &NONE);
        assert!(quad_close(t.content(), rect(100.5, 100.0, 100.0, 200.0)), "{:?}", t.content());
        assert_eq!(t.placed(), t.content());
    }
}

//! An adjustment open on the current layer, the same in every app: what it starts at, its values
//! as its bar and panels show them, and how each change redraws it (the filters themselves are in
//! adjust.rs). The app opens and ends it and shows its values; the touches drag its amount.
//! Noise, bloom, halftone, chromatic aberration and glitch keep their settings from one adjustment
//! to the next; the amount, HSB, colour balance and curves start afresh, and perspective blur
//! positional, its puck in the canvas's middle and its arrow up.

use std::sync::Arc;

use procraft_engine::filter::LIMIT_OFF;
use procraft_engine::geom::Vec2;
use procraft_engine::view::View;

use crate::CanvasSession;
use procraft_engine::document::{Document, LayerAdjustment, LayerId};

use crate::adjust::{
    ChromaticKind, DitherKind, GlitchKind, HalftoneKind, LiquifyBrush, LiquifyKind, MOTION_DRAG_PT, NoiseKind,
    PerspectiveKind, SwipeAdjustment, VignetteKind,
};
use crate::adjust_menu::{
    Adjustment, adjustment_groups, adjustment_live, adjustment_paintable, adjustment_pencil_amount,
};
use crate::gradients::Gradients;

/// Painted on, bloom starts smaller and burns more.
const PENCIL_BLOOM_SIZE: f32 = 0.335;
const PENCIL_BLOOM_BURN: f32 = 0.57;
/// How close (0..1) a curve's point may come to its neighbours.
const CURVE_GAP: f32 = 0.01;

/// Curves' channels: all, red, green, blue, its exposure and its limit.
const CURVE_CHANNELS: usize = 6;

/// Curves' exposure untouched: no stops, from the darkest, half soft.
const NO_EXPOSURE: [f32; 3] = [0.0, 0.0, 0.5];

/// Curves' limit untouched: none, its knee at 80%.
const NO_LIMIT: [f32; 2] = [LIMIT_OFF, 0.8];

/// Curves' limit kept to its ranges.
fn limit_kept([peak, knee]: [f32; 2]) -> [f32; 2] {
    [peak.clamp(0.0, LIMIT_OFF), knee.clamp(0.0, 0.95)]
}

/// Curves' exposure kept to its ranges.
fn exposure_kept([stops, above, soft]: [f32; 3]) -> [f32; 3] {
    [stops.clamp(-5.0, 5.0), above.clamp(0.0, 1.0), soft.clamp(0.0, 1.0)]
}
/// The clone source's ring (points from its centre), and how far off it a touch still takes it.
pub const CLONE_PUCK_RADIUS: f32 = 30.0;
pub(crate) const CLONE_PUCK_SLOP: f32 = 8.0;
/// Perspective blur's puck takes a drag that starts this near it (points): positional's moves it;
/// directional's within the inner ring moves it, and out to the reach turns the arrow when the drag
/// starts on the arrow's side (within a right angle of it) and moves the puck otherwise.
pub(crate) const POSITIONAL_GRAB: f32 = 50.0;
pub(crate) const DIRECTIONAL_INNER: f32 = 40.0;
pub(crate) const DIRECTIONAL_REACH: f32 = 110.0;
/// Perspective blur's arrow at first: up the window. The point along it given for drawing lies
/// this far from the puck (canvas pixels).
/// How many ways liquify has.
const LIQUIFY_KINDS: usize = 8;
const ARROW_UP: f32 = -std::f32::consts::FRAC_PI_2;
const ARROW_TIP: f32 = 100.0;

/// Each glitch kind's sliders as they start (the last its zoom).
fn glitch_defaults() -> [Vec<f32>; 4] {
    [vec![1.0, 0.09, 1.0], vec![0.2, 0.3, 1.0], vec![0.8, 0.19, 1.0], vec![0.98, -0.5, 0.85, 0.5]]
}

fn straight() -> Vec<(f32, f32)> {
    vec![(0.0, 0.0), (1.0, 1.0)]
}

/// What an open adjustment shows: its values as its bar and panels show them.
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct AdjustShown {
    pub adjustment: Option<Adjustment>,
    /// The amount (0..1) swiped, dragged or painted.
    pub amount: f32,
    /// Painted on with the brush (Pencil), and the layer shown as it was before.
    pub pencil: bool,
    pub preview: bool,
    /// Hue, saturation and brightness, each 0..1 (0.5 none).
    pub hsb: Vec<f32>,
    /// Colour balance: the tone shown (0 shadows, 1 midtones, 2 highlights) and its nine sliders,
    /// red, green and blue for each tone (0..1, 0.5 none).
    pub balance_tone: u32,
    pub balance: Vec<f32>,
    /// Curves: the channel shown (0 all, 1 red, 2 green, 3 blue, 4 exposure) and each channel's
    /// points as x, y pairs (0..1).
    pub curve_channel: u32,
    pub curves: Vec<Vec<f32>>,
    /// Curves' exposure: its stops (−5..5), and the brightness it starts above and how softly
    /// (0..1).
    pub exposure: Vec<f32>,
    /// Curves' limit: its peak (stops above white, 0..5; none from 6) and its knee (0..0.95).
    pub limit: Vec<f32>,
    /// The gradient mapped by (its place in the library).
    pub gradient: u32,
    pub noise_kind: NoiseKind,
    /// Noise's scale, octaves and turbulence (0..1); each channel its own; added or mixed in.
    pub noise: Vec<f32>,
    pub noise_multi: bool,
    pub noise_added: bool,
    /// Bloom's transition, size and burn (0..1).
    pub bloom: Vec<f32>,
    pub halftone_kind: HalftoneKind,
    pub chroma_kind: ChromaticKind,
    /// The chromatic kind shown's two sliders.
    pub chroma: Vec<f32>,
    pub glitch_kind: GlitchKind,
    /// The glitch kind shown's sliders (0..1, or −1..1 for diverge's shifts; the last its zoom).
    pub glitch: Vec<f32>,
    /// Liquify: its way, its size, pressure, distortion and momentum (0..1), how much of what it
    /// did shows (Adjust), and whether it has done anything yet.
    pub liquify_kind: LiquifyKind,
    pub liquify: Vec<f32>,
    pub liquify_amount: f32,
    pub liquified: bool,
    /// Cloning: where the source is in the window (x, y; none when not cloning or off the view).
    pub clone_puck: Option<Vec<f32>>,
    /// Perspective blur: all round or towards its arrow, and (on the layer, not painted on) its
    /// puck in the window and a point along its arrow there (x, y, then x, y; window pixels).
    pub perspective_kind: PerspectiveKind,
    pub perspective_puck: Option<Vec<f32>>,
    /// Anything was changed since it opened (or was last kept).
    pub changed: bool,
    /// A plugin's filter: its name, and its controls for one panel to draw.
    pub plugin: Option<crate::plugin_filter::PluginFilterShown>,
    /// Dither: its pattern, its levels and pattern size sliders (0..1), and whether it is in black
    /// and white.
    pub dither_kind: DitherKind,
    pub dither: Vec<f32>,
    pub dither_mono: bool,
    /// Vignette: darkening or lightening, and its size and softness sliders (0..1).
    pub vignette_kind: VignetteKind,
    pub vignette: Vec<f32>,
    /// It is an adjustment layer's: it changes what is below that layer, and is never painted on.
    pub on_layer: bool,
}

/// An adjustment layer open in the adjustment's panels: the layer, the document from before this
/// time it was opened (to keep as one undo step, or go back to), and from when its values last
/// started (undo and reset go back there); whether it was added just now, and whether it is shown
/// doing nothing for a moment (Preview).
pub(crate) struct LayerEdit {
    pub(crate) layer: LayerId,
    pub(crate) before: Document,
    pub(crate) start: Document,
    pub(crate) added: bool,
    pub(crate) preview: bool,
}

/// An adjustment layer's settings as its file keeps them: the values its panels show.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
struct LayerSettings {
    adjustment: String,
    amount: f32,
    hsb: Vec<f32>,
    balance_tone: u32,
    balance: Vec<f32>,
    curve_channel: u32,
    curves: Vec<Vec<f32>>,
    exposure: Vec<f32>,
    limit: Vec<f32>,
    gradient: u32,
    noise_kind: u32,
    noise: Vec<f32>,
    noise_multi: bool,
    noise_added: bool,
    bloom: Vec<f32>,
    halftone_kind: u32,
    chroma_kind: u32,
    chroma: Vec<Vec<f32>>,
    chroma_offset: [f32; 2],
    glitch_kind: u32,
    glitch: Vec<Vec<f32>>,
    perspective_kind: u32,
    perspective_origin: Option<[f32; 2]>,
    perspective_arrow: f32,
    dither_kind: u32,
    dither: Vec<f32>,
    dither_mono: bool,
    vignette_kind: u32,
    vignette: Vec<f32>,
}

/// The `i`th of `all`, or `or`.
fn nth<T: Copy>(all: &[T], i: u32, or: T) -> T {
    all.get(i as usize).copied().unwrap_or(or)
}

/// `values` into `into`, as many as both have, each finite one within `lo`..1.
fn fill<const N: usize>(into: &mut [f32; N], values: &[f32], lo: f32) {
    for (v, &x) in into.iter_mut().zip(values) {
        if x.is_finite() {
            *v = x.clamp(lo, 1.0);
        }
    }
}

/// The adjustment an adjustment layer's `settings` are for.
pub(crate) fn adjustment_of(settings: &serde_json::Value) -> Option<Adjustment> {
    let name = settings.get("adjustment")?.as_str()?;
    adjustment_named(name)
}

/// The adjustment named `name` (as `{:?}` writes it).
fn adjustment_named(name: &str) -> Option<Adjustment> {
    adjustment_groups().into_iter().flatten().find(|a| format!("{a:?}") == name)
}

pub(crate) struct Adjusting {
    open: Option<Adjustment>,
    amount: f32,
    pencil: bool,
    preview: bool,
    hsb: [f32; 3],
    balance_tone: usize,
    balance: [f32; 9],
    curve_channel: usize,
    curves: [Vec<(f32, f32)>; 4],
    exposure: [f32; 3],
    limit: [f32; 2],
    gradient: usize,
    gradients: Option<Arc<Gradients>>,
    noise_kind: NoiseKind,
    noise: [f32; 3],
    noise_multi: bool,
    noise_added: bool,
    bloom: [f32; 3],
    halftone_kind: HalftoneKind,
    chroma_kind: ChromaticKind,
    chroma: [[f32; 2]; 2],
    /// Displacing: how far the fringes moved (window pixels), and from where this drag began.
    chroma_offset: (f32, f32),
    chroma_from: (f32, f32),
    glitch_kind: GlitchKind,
    glitch: [Vec<f32>; 4],
    liquify_kind: LiquifyKind,
    /// Size, pressure, distortion and momentum; each way keeps its own distortion.
    liquify: [f32; 4],
    liquify_distortions: [f32; LIQUIFY_KINDS],
    liquify_amount: f32,
    liquified: bool,
    /// Changed since it opened (a slider, a choice, a liquify stroke): undo takes that back.
    touched: bool,
    /// Cloning: the source (canvas pixels), how far it is from the brush once a stroke fixed that,
    /// and where it was before each stroke kept and each undone (for undo and redo).
    puck: Option<Vec2>,
    puck_offset: Option<Vec2>,
    puck_before: Vec<Vec2>,
    puck_after: Vec<Vec2>,
    /// Perspective blur: its way, its puck and arrow, and where they were before each drag on the
    /// puck and each undone (for undo and redo).
    perspective_kind: PerspectiveKind,
    perspective: Perspective,
    perspective_before: Vec<Perspective>,
    perspective_after: Vec<Perspective>,
    dither_kind: DitherKind,
    dither: [f32; 2],
    dither_mono: bool,
    vignette_kind: VignetteKind,
    vignette: [f32; 2],
    /// The adjustment layer the adjustment open works on (kept through its undo and reset, which
    /// open it again); none, the current layer.
    layer: Option<LayerId>,
}

/// Perspective blur's puck: where it is (canvas pixels; none for the canvas's middle), and the way
/// its arrow points (a canvas angle, radians).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Perspective {
    origin: Option<Vec2>,
    arrow: f32,
}

impl Default for Perspective {
    fn default() -> Perspective {
        Perspective { origin: None, arrow: ARROW_UP }
    }
}

impl Default for Adjusting {
    fn default() -> Adjusting {
        Adjusting {
            open: None,
            amount: 0.0,
            pencil: false,
            preview: false,
            hsb: [0.5; 3],
            balance_tone: 1,
            balance: [0.5; 9],
            curve_channel: 0,
            curves: [straight(), straight(), straight(), straight()],
            exposure: NO_EXPOSURE,
            limit: NO_LIMIT,
            gradient: 0,
            gradients: None,
            noise_kind: NoiseKind::Clouds,
            noise: [0.0, 0.0, 0.7],
            noise_multi: false,
            noise_added: true,
            bloom: [1.0, 0.3, 0.35],
            halftone_kind: HalftoneKind::FullColor,
            chroma_kind: ChromaticKind::Perspective,
            chroma: [[0.0; 2]; 2],
            chroma_offset: (0.0, 0.0),
            chroma_from: (0.0, 0.0),
            glitch_kind: GlitchKind::Artifact,
            glitch: glitch_defaults(),
            liquify_kind: LiquifyKind::Push,
            liquify: [0.5, 1.0, 0.0, 0.0],
            liquify_distortions: [0.0; LIQUIFY_KINDS],
            liquify_amount: 1.0,
            liquified: false,
            touched: false,
            puck: None,
            puck_offset: None,
            puck_before: Vec::new(),
            puck_after: Vec::new(),
            perspective_kind: PerspectiveKind::Positional,
            perspective: Perspective::default(),
            perspective_before: Vec::new(),
            perspective_after: Vec::new(),
            dither_kind: DitherKind::Ordered,
            dither: [0.0, 0.0],
            dither_mono: false,
            vignette_kind: VignetteKind::Dark,
            vignette: [0.4, 0.6],
            layer: None,
        }
    }
}

impl Adjusting {
    /// The adjustment open, and whether it is painted on (Pencil).
    pub(crate) fn open(&self) -> Option<(Adjustment, bool)> {
        self.open.map(|a| (a, self.pencil))
    }

    /// One open as if begun (for the touches' tests, which have no canvas to begin it on).
    #[cfg(test)]
    pub(crate) fn opened(adjustment: Adjustment) -> Adjusting {
        Adjusting { open: Some(adjustment), ..Adjusting::default() }
    }

    /// Liquify's Adjust amount as an undo or redo left it, and whether it holds anything liquified.
    pub(crate) fn liquify_follows(&mut self, amount: f32, liquified: bool) {
        self.liquify_amount = amount;
        self.liquified = liquified;
    }

    /// Liquify opened again by an undo or a redo, as it was: its brush as it is set.
    pub(crate) fn liquify_open_again(&mut self) {
        self.open = Some(Adjustment::Liquify);
        self.pencil = false;
        self.preview = false;
        self.touched = true;
        self.puck = None;
        self.puck_offset = None;
    }

    /// The liquify brush as it is set.
    pub(crate) fn liquify_brush(&self) -> LiquifyBrush {
        let [size, pressure, distortion, _] = self.liquify;
        LiquifyBrush { kind: self.liquify_kind, size, pressure, distortion }
    }

    pub(crate) fn liquify_momentum(&self) -> f32 {
        self.liquify[3]
    }

    /// Cloning, undo puts the source back where it was before the last stroke, and redo where
    /// that stroke took it.
    pub(crate) fn puck_step(&mut self, undo: bool) {
        if self.open != Some(Adjustment::Clone) {
            return;
        }
        let (from, to) = if undo {
            (&mut self.puck_before, &mut self.puck_after)
        } else {
            (&mut self.puck_after, &mut self.puck_before)
        };
        if let Some(p) = from.pop() {
            to.extend(self.puck.replace(p));
        }
    }

    /// Perspective blur, undo puts its puck and arrow back as they were before the last drag on it,
    /// and redo as that drag left them. Returns whether there was one to step.
    fn perspective_step(&mut self, undo: bool) -> bool {
        if self.open != Some(Adjustment::PerspectiveBlur) {
            return false;
        }
        let (from, to) = if undo {
            (&mut self.perspective_before, &mut self.perspective_after)
        } else {
            (&mut self.perspective_after, &mut self.perspective_before)
        };
        let Some(p) = from.pop() else { return false };
        to.push(std::mem::replace(&mut self.perspective, p));
        true
    }

    /// Chromatic aberration displacing: a drag moves the fringes.
    pub(crate) fn displacing(&self) -> bool {
        self.open == Some(Adjustment::ChromaticAberration) && self.chroma_kind == ChromaticKind::Displace
    }

    /// A plugin's filter is open, as if just begun.
    pub(crate) fn open_plugin(&mut self) {
        self.open = Some(Adjustment::Plugin);
        self.pencil = false;
        self.preview = false;
        self.touched = false;
    }

    /// The plugin's filter open has stopped.
    pub(crate) fn close_plugin(&mut self) {
        self.open = None;
        self.preview = false;
    }

    /// Something open was changed.
    pub(crate) fn touch(&mut self) {
        self.touched = true;
    }

    /// Open, and changed since it opened: undo takes that back.
    pub(crate) fn touched(&self) -> bool {
        self.open.is_some() && self.touched
    }

    fn changed(&self) -> bool {
        match self.open {
            Some(Adjustment::Hsb) => self.hsb.iter().any(|&v| v != 0.5),
            Some(Adjustment::ColorBalance) => self.balance.iter().any(|&v| v != 0.5),
            Some(Adjustment::Curves) => {
                self.curves.iter().any(|c| *c != straight()) || self.exposure[0] != 0.0 || self.limit[0] < LIMIT_OFF
            }
            Some(Adjustment::GradientMap | Adjustment::Plugin) => true,
            Some(Adjustment::PerspectiveBlur) => self.amount > 0.0 || !self.perspective_before.is_empty(),
            _ => self.amount > 0.0,
        }
    }

    fn shown(&self, view: Option<View>, middle: Vec2) -> AdjustShown {
        AdjustShown {
            adjustment: self.open,
            amount: self.amount,
            pencil: self.pencil,
            preview: self.preview,
            hsb: self.hsb.to_vec(),
            balance_tone: self.balance_tone as u32,
            balance: self.balance.to_vec(),
            curve_channel: self.curve_channel as u32,
            curves: self.curves.iter().map(|c| c.iter().flat_map(|&(x, y)| [x, y]).collect()).collect(),
            exposure: self.exposure.to_vec(),
            limit: self.limit.to_vec(),
            gradient: self.gradient as u32,
            noise_kind: self.noise_kind,
            noise: self.noise.to_vec(),
            noise_multi: self.noise_multi,
            noise_added: self.noise_added,
            bloom: self.bloom.to_vec(),
            halftone_kind: self.halftone_kind,
            chroma_kind: self.chroma_kind,
            chroma: self.chroma.get(self.chroma_kind as usize).map_or_else(Vec::new, |c| c.to_vec()),
            glitch_kind: self.glitch_kind,
            glitch: self.glitch.get(self.glitch_kind as usize).cloned().unwrap_or_default(),
            liquify_kind: self.liquify_kind,
            liquify: self.liquify.to_vec(),
            liquify_amount: self.liquify_amount,
            liquified: self.liquified,
            clone_puck: self.puck.zip(view).map(|(p, v)| {
                let at = v.to_screen().apply(p);
                vec![at.x, at.y]
            }),
            perspective_kind: self.perspective_kind,
            perspective_puck: self.perspective_puck(view, middle),
            changed: self.changed(),
            plugin: None,
            dither_kind: self.dither_kind,
            dither: self.dither.to_vec(),
            dither_mono: self.dither_mono,
            vignette_kind: self.vignette_kind,
            vignette: self.vignette.to_vec(),
            on_layer: false,
        }
    }

    /// Its values as an adjustment layer keeps them.
    pub(crate) fn layer_settings(&self) -> serde_json::Value {
        let settings = LayerSettings {
            adjustment: self.open.map(|a| format!("{a:?}")).unwrap_or_default(),
            amount: self.amount,
            hsb: self.hsb.to_vec(),
            balance_tone: self.balance_tone as u32,
            balance: self.balance.to_vec(),
            curve_channel: self.curve_channel as u32,
            curves: self.curves.iter().map(|c| c.iter().flat_map(|&(x, y)| [x, y]).collect()).collect(),
            exposure: self.exposure.to_vec(),
            limit: self.limit.to_vec(),
            gradient: self.gradient as u32,
            noise_kind: self.noise_kind as u32,
            noise: self.noise.to_vec(),
            noise_multi: self.noise_multi,
            noise_added: self.noise_added,
            bloom: self.bloom.to_vec(),
            halftone_kind: self.halftone_kind as u32,
            chroma_kind: self.chroma_kind as u32,
            chroma: self.chroma.iter().map(|c| c.to_vec()).collect(),
            chroma_offset: [self.chroma_offset.0, self.chroma_offset.1],
            glitch_kind: self.glitch_kind as u32,
            glitch: self.glitch.to_vec(),
            perspective_kind: self.perspective_kind as u32,
            perspective_origin: self.perspective.origin.map(|o| [o.x, o.y]),
            perspective_arrow: self.perspective.arrow,
            dither_kind: self.dither_kind as u32,
            dither: self.dither.to_vec(),
            dither_mono: self.dither_mono,
            vignette_kind: self.vignette_kind as u32,
            vignette: self.vignette.to_vec(),
        };
        serde_json::to_value(settings).unwrap_or_default()
    }

    /// Its values set from an adjustment layer's `settings`: the adjustment they are for, if any.
    /// What they leave out stays as it is.
    fn load_layer_settings(&mut self, settings: &serde_json::Value) -> Option<Adjustment> {
        let s: LayerSettings = serde_json::from_value(settings.clone()).ok()?;
        let adjustment = adjustment_named(&s.adjustment).filter(|a| adjustment_live(*a))?;
        self.amount = if s.amount.is_finite() { s.amount.clamp(0.0, 1.0) } else { 0.0 };
        fill(&mut self.hsb, &s.hsb, 0.0);
        self.balance_tone = (s.balance_tone as usize).min(2);
        fill(&mut self.balance, &s.balance, 0.0);
        self.curve_channel = (s.curve_channel as usize).min(CURVE_CHANNELS - 1);
        for (curve, points) in self.curves.iter_mut().zip(&s.curves) {
            let pts: Vec<(f32, f32)> = points.as_chunks::<2>().0.iter().map(|&[x, y]| (x, y)).collect();
            if pts.len() >= 2 && pts.iter().all(|(x, y)| x.is_finite() && y.is_finite()) {
                *curve = pts;
            }
        }
        // (a layer from before Curves had its exposure: none)
        self.exposure = NO_EXPOSURE;
        for (v, &x) in self.exposure.iter_mut().zip(&s.exposure) {
            if x.is_finite() {
                *v = x;
            }
        }
        self.exposure = exposure_kept(self.exposure);
        self.limit = NO_LIMIT;
        for (v, &x) in self.limit.iter_mut().zip(&s.limit) {
            if x.is_finite() {
                *v = x;
            }
        }
        self.limit = limit_kept(self.limit);
        self.gradient = s.gradient as usize;
        use {ChromaticKind as C, DitherKind as D, GlitchKind as G, HalftoneKind as H, NoiseKind as N};
        self.noise_kind = nth(&[N::Clouds, N::Billows, N::Ridges], s.noise_kind, N::Clouds);
        fill(&mut self.noise, &s.noise, 0.0);
        self.noise_multi = s.noise_multi;
        self.noise_added = s.noise_added;
        fill(&mut self.bloom, &s.bloom, 0.0);
        self.halftone_kind = nth(&[H::FullColor, H::ScreenPrint, H::Newspaper], s.halftone_kind, H::FullColor);
        self.chroma_kind = nth(&[C::Perspective, C::Displace], s.chroma_kind, C::Perspective);
        for (c, v) in self.chroma.iter_mut().zip(&s.chroma) {
            fill(c, v, 0.0);
        }
        let [dx, dy] = s.chroma_offset;
        self.chroma_offset = if dx.is_finite() && dy.is_finite() { (dx, dy) } else { (0.0, 0.0) };
        self.glitch_kind = nth(&[G::Artifact, G::Wave, G::Signal, G::Diverge], s.glitch_kind, G::Artifact);
        for (g, v) in self.glitch.iter_mut().zip(&s.glitch) {
            if v.len() == g.len() && v.iter().all(|x| x.is_finite()) {
                g.clone_from(v);
            }
        }
        let p = [PerspectiveKind::Positional, PerspectiveKind::Directional];
        self.perspective_kind = nth(&p, s.perspective_kind, PerspectiveKind::Positional);
        self.perspective = Perspective {
            origin: s.perspective_origin.filter(|o| o.iter().all(|v| v.is_finite())).map(|[x, y]| Vec2::new(x, y)),
            arrow: if s.perspective_arrow.is_finite() { s.perspective_arrow } else { ARROW_UP },
        };
        self.dither_kind = nth(&[D::Ordered, D::Noise], s.dither_kind, D::Ordered);
        fill(&mut self.dither, &s.dither, 0.0);
        self.dither_mono = s.dither_mono;
        self.vignette_kind = nth(&[VignetteKind::Dark, VignetteKind::Light], s.vignette_kind, VignetteKind::Dark);
        fill(&mut self.vignette, &s.vignette, 0.0);
        Some(adjustment)
    }

    /// Perspective blur on the layer (not painted on): its puck and a point along its arrow in the
    /// window, its puck in the canvas's `middle` until moved.
    fn perspective_puck(&self, view: Option<View>, middle: Vec2) -> Option<Vec<f32>> {
        let v = view.filter(|_| self.open == Some(Adjustment::PerspectiveBlur) && !self.pencil)?;
        let origin = self.perspective.origin.unwrap_or(middle);
        let arrow = Vec2::new(self.perspective.arrow.cos(), self.perspective.arrow.sin());
        let (at, tip) = (v.to_screen().apply(origin), v.to_screen().apply(origin + arrow * ARROW_TIP));
        Some(vec![at.x, at.y, tip.x, tip.y])
    }
}

/// How an adjustment's values redraw it: the filter call to make, worked out under the lock and
/// made after it is let go.
enum Redraw {
    None,
    Swipe(SwipeAdjustment, f32),
    Hsb([f32; 3]),
    Balance([f32; 9]),
    Curves([Vec<f32>; 4], [f32; 3], [f32; 2]),
    Gradient(Vec<f32>, f32),
    Noise(NoiseKind, f32, [f32; 3], bool, bool),
    Bloom(f32, [f32; 3]),
    Halftone(HalftoneKind, f32),
    Chroma(ChromaticKind, f32, (f32, f32), [f32; 2]),
    Glitch(GlitchKind, f32, Vec<f32>),
    Perspective(PerspectiveKind, f32, Option<[f32; 2]>, f32),
    Pixelate(f32),
    Dither(DitherKind, f32, [f32; 2], bool),
    Vignette(VignetteKind, f32, [f32; 2]),
}

impl Adjusting {
    /// What the amount (or the values of the adjustment open) redraws.
    fn redraw(&self) -> Redraw {
        let a = self.amount;
        match self.open {
            Some(Adjustment::GaussianBlur) => Redraw::Swipe(SwipeAdjustment::GaussianBlur, a),
            Some(Adjustment::Sharpen) => Redraw::Swipe(SwipeAdjustment::Sharpen, a),
            Some(Adjustment::PerspectiveBlur) => Redraw::Perspective(
                self.perspective_kind,
                a,
                self.perspective.origin.map(|o| [o.x, o.y]),
                self.perspective.arrow,
            ),
            Some(Adjustment::Hsb) => Redraw::Hsb(self.hsb),
            Some(Adjustment::ColorBalance) => Redraw::Balance(self.balance),
            Some(Adjustment::Curves) => Redraw::Curves(
                self.curves.clone().map(|c| c.iter().flat_map(|&(x, y)| [x, y]).collect()),
                self.exposure,
                self.limit,
            ),
            Some(Adjustment::GradientMap) => {
                let list = self.gradients.as_ref().map(|g| g.list()).unwrap_or_default();
                match list.get(self.gradient) {
                    Some(g) => Redraw::Gradient(Gradients::map_stops(g), a),
                    None => Redraw::None,
                }
            }
            Some(Adjustment::Noise) => {
                Redraw::Noise(self.noise_kind, a, self.noise, self.noise_multi, self.noise_added)
            }
            Some(Adjustment::Bloom) => Redraw::Bloom(a, self.bloom),
            Some(Adjustment::Halftone) => Redraw::Halftone(self.halftone_kind, a),
            Some(Adjustment::ChromaticAberration) => {
                let offset = if self.chroma_kind == ChromaticKind::Displace { self.chroma_offset } else { (0.0, 0.0) };
                let values = self.chroma.get(self.chroma_kind as usize).copied().unwrap_or_default();
                Redraw::Chroma(self.chroma_kind, a, offset, values)
            }
            Some(Adjustment::Glitch) => Redraw::Glitch(
                self.glitch_kind,
                a,
                self.glitch.get(self.glitch_kind as usize).cloned().unwrap_or_default(),
            ),
            Some(Adjustment::Pixelate) => Redraw::Pixelate(a),
            Some(Adjustment::Dither) => Redraw::Dither(self.dither_kind, a, self.dither, self.dither_mono),
            Some(Adjustment::Vignette) => Redraw::Vignette(self.vignette_kind, a, self.vignette),
            _ => Redraw::None,
        }
    }
}

impl CanvasSession {
    /// Undo (`undo`) or redo with an adjustment open on the layer (not painted on): undo takes
    /// back what it changed since it opened and it stays open (named for the notice); with nothing
    /// changed, `step` is made and it opens again on what that leaves. Redo leaves changes made
    /// alone (they would be lost). None when no such adjustment is open.
    pub(crate) fn step_adjusting(
        &self,
        step: impl FnOnce(&Self) -> Option<String>,
        undo: bool,
    ) -> Option<Option<String>> {
        let (open, touched) = self.adjusting(|a, _| (a.open, a.touched));
        let open = open?;
        if self.shared.lock().pencil {
            return None;
        }
        // an adjustment layer: undo takes back what changed since its values last started; with
        // nothing changed it is closed and the step made as any
        if self.shared.lock().adjust_layer.is_some() {
            if self.layer_changed() {
                if !undo {
                    return Some(None);
                }
                self.layer_back();
                return Some(Some(crate::adjust_menu::adjustment_label(open)));
            }
            self.adjustment_end(true);
            return None;
        }
        // perspective blur's puck steps back (or on) a drag at a time
        if self.perspective_step(undo) {
            return Some(Some(crate::adjust_menu::adjustment_label(open)));
        }
        if touched {
            if !undo {
                return Some(None);
            }
            self.adjustment_again(open, false);
            return Some(Some(crate::adjust_menu::adjustment_label(open)));
        }
        let kept = self.adjusting(|a, _| (a.perspective_kind, a.perspective));
        self.adjustment_close(false);
        let name = step(self);
        self.adjustment_open(open);
        self.adjusting(|a, _| (a.perspective_kind, a.perspective) = kept);
        Some(name)
    }

    /// Perspective blur's puck and arrow stepped back (`undo`) or on a drag, and the blur redrawn.
    /// Returns whether there was one to step.
    fn perspective_step(&self, undo: bool) -> bool {
        let redraw = self.adjusting(|a, _| a.perspective_step(undo).then(|| a.redraw()));
        redraw.map(|r| self.redraw_adjustment(r)).is_some()
    }

    /// Opens the adjustment open again from nothing, or (`commit`) keeping what it did as one undo
    /// step. Perspective blur keeps its way, and committed, its puck and arrow.
    fn adjustment_again(&self, open: Adjustment, commit: bool) {
        // an adjustment layer goes on open, kept as it is or back to where its values started
        if self.shared.lock().adjust_layer.is_some() {
            if commit {
                self.layer_keep();
            } else {
                self.layer_back();
            }
            return;
        }
        let (kind, puck) = self.adjusting(|a, _| (a.perspective_kind, a.perspective));
        self.adjustment_close(commit);
        self.adjustment_open(open);
        self.adjusting(|a, _| {
            a.perspective_kind = kind;
            if commit {
                a.perspective = puck;
            }
        });
    }

    fn adjusting<R>(&self, f: impl FnOnce(&mut Adjusting, (u32, u32)) -> R) -> R {
        let mut st = self.shared.lock();
        let window = st.window_size;
        f(&mut st.adjust, window)
    }

    fn redraw_adjustment(&self, redraw: Redraw) {
        match redraw {
            Redraw::None => {}
            Redraw::Swipe(kind, amount) => self.adjust_swipe(kind, amount),
            Redraw::Hsb([h, s, b]) => self.adjust_hsb(h, s, b),
            Redraw::Balance(v) => {
                let v: Vec<f32> = v.iter().map(|x| (x - 0.5) * 2.0).collect();
                let tone = |k: usize| v.get(k * 3..k * 3 + 3).map(<[f32]>::to_vec).unwrap_or_default();
                self.adjust_color_balance(tone(0), tone(1), tone(2));
            }
            Redraw::Curves([all, red, green, blue], exposure, limit) => {
                self.adjust_curves(all, red, green, blue, exposure.to_vec(), limit.to_vec())
            }
            Redraw::Gradient(stops, amount) => self.adjust_gradient_map(stops, amount),
            Redraw::Noise(kind, amount, [scale, octaves, turbulence], multi, added) => {
                self.adjust_noise(kind, amount, scale, octaves, turbulence, multi, added);
            }
            Redraw::Bloom(amount, [transition, size, burn]) => self.adjust_bloom(amount, transition, size, burn),
            Redraw::Halftone(kind, amount) => self.adjust_halftone(kind, amount),
            Redraw::Chroma(kind, amount, (dx, dy), [a, b]) => self.adjust_chromatic(kind, amount, dx, dy, a, b),
            Redraw::Glitch(kind, amount, mut values) => {
                let zoom = values.pop().unwrap_or(1.0);
                self.adjust_glitch(kind, amount, values, zoom);
            }
            Redraw::Perspective(kind, amount, origin, arrow) => self.adjust_perspective(kind, amount, origin, arrow),
            Redraw::Pixelate(amount) => self.adjust_pixelate(amount),
            Redraw::Dither(kind, amount, [levels, scale], mono) => {
                self.adjust_dither(kind, amount, levels, scale, mono)
            }
            Redraw::Vignette(kind, amount, [size, softness]) => self.adjust_vignette(kind, amount, size, softness),
        }
    }

    /// Changes the open adjustment's values with `f` and redraws it.
    fn change_adjustment(&self, f: impl FnOnce(&mut Adjusting, (u32, u32))) {
        let redraw = self.adjusting(|a, window| {
            if a.open.is_none() {
                return Redraw::None;
            }
            a.touched = true;
            f(a, window);
            a.redraw()
        });
        self.redraw_adjustment(redraw);
    }
}

#[uniffi::export]
impl CanvasSession {
    /// The gradient library Gradient Map maps by.
    pub fn use_gradients(&self, gradients: Arc<Gradients>) {
        self.adjusting(|a, _| a.gradients = Some(gradients));
    }

    /// Opens `adjustment` on the current layer (one already open kept first); false when the layer
    /// can't be adjusted. The amount starts at none (a gradient map's at all of it), HSB, colour
    /// balance and curves as they were; the other adjustments keep their settings.
    pub fn adjustment_start(&self, adjustment: Adjustment) -> bool {
        // from the menu: on the current layer, not an adjustment layer open
        if self.adjusting(|a, _| a.layer.is_some()) {
            self.adjustment_end(true);
        }
        self.adjustment_open(adjustment)
    }

    /// Adds an adjustment layer of `adjustment` above the current layer and opens it, as the
    /// adjustment opens on a layer; false when it can't be one (Liquify, Clone, a plugin's
    /// filter), or the canvas has no room for another layer. Ended without keeping it, the layer
    /// goes again.
    pub fn adjustment_layer_add(&self, adjustment: Adjustment) -> bool {
        if !adjustment_live(adjustment) || !self.shared.lock().doc.has_room(1) {
            return false;
        }
        self.adjustment_end(true);
        let id = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            let before = st.doc.clone();
            let name = crate::adjust_menu::adjustment_label(adjustment);
            // nothing changed until its values are set (sharpening by none)
            let blank = LayerAdjustment {
                filter: procraft_gpu::Filter::Sharpen { amount: 0.0 },
                table: Vec::new(),
                settings: serde_json::Value::Null,
            };
            let id = st.doc.add_adjustment_layer(name, blank);
            st.adjust_layer = Some(LayerEdit { layer: id, start: st.doc.clone(), before, added: true, preview: false });
            st.revision += 1;
            id
        };
        // its values start as a new adjustment's do
        let up = self.shared.lock().canvas_angle(ARROW_UP);
        self.adjusting(|a, _| {
            a.layer = Some(id);
            a.hsb = [0.5; 3];
            a.balance = [0.5; 9];
            a.curves = [straight(), straight(), straight(), straight()];
            a.exposure = NO_EXPOSURE;
            a.limit = NO_LIMIT;
            a.chroma_offset = (0.0, 0.0);
            a.perspective_kind = PerspectiveKind::Positional;
            a.perspective = Perspective { origin: None, arrow: up };
            a.perspective_before.clear();
            a.perspective_after.clear();
        });
        self.adjustment_open(adjustment)
    }

    /// Opens adjustment layer `layer`'s adjustment with its values as it keeps them, to change
    /// them; the adjustment opened, or none when it isn't an adjustment layer (or is locked).
    pub fn adjustment_layer_edit(&self, layer: u64) -> Option<Adjustment> {
        let id = LayerId(layer);
        let settings = {
            let st = self.shared.lock();
            let l = st.doc.layer(id).filter(|_| !st.doc.is_locked(id))?;
            l.adjustment.as_ref()?.settings.clone()
        };
        self.adjustment_end(true);
        let adjustment = self.adjusting(|a, _| {
            let mut loaded = Adjusting { gradients: a.gradients.clone(), ..Adjusting::default() };
            let adjustment = loaded.load_layer_settings(&settings)?;
            *a = loaded;
            Some(adjustment)
        })?;
        {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            st.doc.set_current(id);
            let before = st.doc.clone();
            st.adjust_layer =
                Some(LayerEdit { layer: id, start: before.clone(), before, added: false, preview: false });
            st.adjusting = true;
            st.last_filter = st.doc.layer(id).and_then(|l| l.adjustment.as_ref()).map(|a| a.filter);
            st.revision += 1;
            self.shared.frame(st);
        }
        self.adjusting(|a, _| {
            a.open = Some(adjustment);
            a.layer = Some(id);
        });
        Some(adjustment)
    }

    /// Which adjustment layer `layer` is (none for any other layer).
    pub fn layer_adjustment(&self, layer: u64) -> Option<Adjustment> {
        let st = self.shared.lock();
        adjustment_of(&st.doc.layer(LayerId(layer))?.adjustment.as_ref()?.settings)
    }
}

impl CanvasSession {
    /// Whether the adjustment layer open has changed since its values last started.
    fn layer_changed(&self) -> bool {
        let st = self.shared.lock();
        st.adjust_layer.as_ref().is_some_and(|e| st.doc != e.start)
    }

    /// The adjustment layer open back to where its values last started, and its panel with it.
    fn layer_back(&self) {
        let settings = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            let Some(edit) = st.adjust_layer.as_mut() else { return };
            edit.preview = false;
            let (start, id) = (edit.start.clone(), edit.layer);
            st.doc = start;
            if let Some(canvas) = st.canvas.as_mut() {
                canvas.invalidate();
            }
            st.last_filter = st.doc.layer(id).and_then(|l| l.adjustment.as_ref()).map(|a| a.filter);
            self.shared.frame(st);
            st.doc.layer(id).and_then(|l| l.adjustment.as_ref()).map(|a| a.settings.clone())
        };
        self.adjusting(|a, _| {
            let open = a.open;
            if let Some(s) = &settings
                && !s.is_null()
            {
                a.load_layer_settings(s);
            } else {
                // just added: as it opened
                *a = Adjusting { gradients: a.gradients.clone(), layer: a.layer, open, ..Adjusting::default() };
            }
            a.touched = false;
        });
    }

    /// The adjustment layer open kept as it is, as one undo step, and goes on open from there.
    fn layer_keep(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let Some(edit) = st.adjust_layer.as_mut() else { return };
        if st.doc == edit.before {
            return;
        }
        let before = std::mem::replace(&mut edit.before, st.doc.clone());
        edit.start = st.doc.clone();
        let added = std::mem::replace(&mut edit.added, false);
        if let Some(canvas) = st.canvas.as_mut() {
            canvas.name_next_step(if added { "add adjustment layer" } else { "change adjustment layer" });
            canvas.record_document(before);
        }
        st.revision += 1;
        drop(guard);
        self.adjusting(|a, _| a.touched = false);
    }

    /// Opens `adjustment` on what it works on: the adjustment layer open (as it was set), or the
    /// current layer.
    fn adjustment_open(&self, adjustment: Adjustment) -> bool {
        if self.adjusting(|a, _| a.layer.is_some()) {
            let open = self.shared.lock().adjust_layer.is_some();
            if !open {
                return false;
            }
            {
                let mut st = self.shared.lock();
                st.adjusting = true;
            }
            let redraw = self.adjusting(|a, _| {
                a.open = Some(adjustment);
                a.amount = if adjustment == Adjustment::GradientMap { 1.0 } else { 0.0 };
                a.pencil = false;
                a.preview = false;
                a.touched = false;
                a.redraw()
            });
            self.redraw_adjustment(redraw);
            // its values as they start are where undo goes back to
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            if let Some(edit) = st.adjust_layer.as_mut() {
                edit.start = st.doc.clone();
            }
            return true;
        }
        // Liquify leaves an alpha locked layer alone; the others take one layer
        use crate::layers::Blocked;
        if matches!(self.why_not_adjusted(adjustment), Some(Blocked::AlphaLocked | Blocked::Group | Blocked::Several)) {
            return false;
        }
        self.adjustment_close(true);
        let begun = match adjustment {
            Adjustment::Liquify => self.liquify_begin(),
            Adjustment::Clone => self.clone_begin(),
            Adjustment::Plugin => return self.plugin_filter_again(),
            _ => self.adjust_begin(),
        };
        if !begun {
            return false;
        }
        // these work on the texture, not through the camera; the others keep to the meshes picked
        if matches!(adjustment, Adjustment::Liquify | Adjustment::Clone) {
            self.shared.lock().scene_flat_for_tool();
        } else {
            self.shared.lock().scene_selection_for_adjustment();
        }
        let up = self.shared.lock().canvas_angle(ARROW_UP);
        let redraw = self.adjusting(|a, _| {
            a.open = Some(adjustment);
            a.amount = if adjustment == Adjustment::GradientMap { 1.0 } else { 0.0 };
            a.pencil = adjustment == Adjustment::Clone;
            a.preview = false;
            a.hsb = [0.5; 3];
            a.balance = [0.5; 9];
            a.curves = [straight(), straight(), straight(), straight()];
            a.exposure = NO_EXPOSURE;
            a.limit = NO_LIMIT;
            a.chroma_offset = (0.0, 0.0);
            a.liquified = false;
            a.touched = false;
            a.liquify_amount = 1.0;
            a.puck = None;
            a.puck_offset = None;
            a.puck_before.clear();
            a.puck_after.clear();
            a.perspective_kind = PerspectiveKind::Positional;
            a.perspective = Perspective { origin: None, arrow: up };
            a.perspective_before.clear();
            a.perspective_after.clear();
            // a gradient map and a glitch show at once
            if matches!(adjustment, Adjustment::GradientMap | Adjustment::Glitch) { a.redraw() } else { Redraw::None }
        });
        self.redraw_adjustment(redraw);
        if adjustment == Adjustment::Clone {
            // the source starts in the middle of the window
            let (w, h) = self.shared.lock().window_size;
            self.adjustment_clone_move(w as f32 / 2.0, h as f32 / 2.0);
        }
        true
    }
}

impl CanvasSession {
    /// Ends the open adjustment as [`Self::adjustment_end`] does, keeping what it works on (an
    /// adjustment layer) for it to open again.
    fn adjustment_close(&self, commit: bool) -> bool {
        let open = self.adjusting(|a, _| {
            a.preview = false;
            a.pencil = false;
            a.puck = None;
            a.open.take().is_some()
        });
        if open {
            self.adjust_end(commit);
            let mut st = self.shared.lock();
            st.scene_selection_back();
            st.scene_back_from_tool();
        }
        open
    }
}

#[uniffi::export]
impl CanvasSession {
    /// Ends the open adjustment: `commit` keeps it as one undo step, otherwise the layer goes back.
    /// Returns whether one was open.
    pub fn adjustment_end(&self, commit: bool) -> bool {
        let open = self.adjustment_close(commit);
        self.adjusting(|a, _| a.layer = None);
        open
    }

    /// What the open adjustment shows.
    pub fn adjustment_shown(&self) -> AdjustShown {
        let st = self.shared.lock();
        let shown = st.adjust.shown(st.view, Vec2::new(st.doc.width() as f32 / 2.0, st.doc.height() as f32 / 2.0));
        AdjustShown { plugin: st.filters.shown(), on_layer: st.adjust_layer.is_some(), ..shown }
    }

    /// The amount moved by `step` (a swipe's), within 0..1.
    pub fn adjustment_amount_by(&self, step: f32) {
        self.change_adjustment(|a, _| a.amount = (a.amount + step).clamp(0.0, 1.0));
    }

    /// A motion blur's drag reached (`dx`, `dy`) window pixels from where it began: along it,
    /// longer the further it goes.
    pub fn adjustment_motion(&self, dx: f32, dy: f32) {
        if self.adjusting(|a, _| a.open == Some(Adjustment::MotionBlur)) {
            let amount = self.adjust_motion(dx, dy);
            self.adjusting(|a, _| a.amount = amount);
        }
    }

    pub fn adjustment_hsb(&self, hue: f32, saturation: f32, brightness: f32) {
        self.change_adjustment(|a, _| a.hsb = [hue, saturation, brightness].map(|v| v.clamp(0.0, 1.0)));
    }

    /// Colour balance's tone shown: 0 shadows, 1 midtones, 2 highlights.
    pub fn adjustment_balance_tone(&self, tone: u32) {
        self.adjusting(|a, _| a.balance_tone = (tone as usize).min(2));
    }

    /// Slider `k` (0 red, 1 green, 2 blue) of the tone shown moved to `value` (0..1).
    pub fn adjustment_balance(&self, k: u32, value: f32) {
        self.change_adjustment(|a, _| {
            let i = a.balance_tone * 3 + (k as usize).min(2);
            if let Some(v) = a.balance.get_mut(i) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// Curves' channel shown: 0 all, 1 red, 2 green, 3 blue, 4 exposure, 5 limit (their sliders in
    /// place of a curve).
    pub fn adjustment_curve_channel(&self, channel: u32) {
        self.adjusting(|a, _| a.curve_channel = (channel as usize).min(CURVE_CHANNELS - 1));
    }

    /// Curves' exposure slider `k` (0 its stops, −5..5; 1 the brightness it starts above, 2 how
    /// softly, 0..1) moved to `value`.
    pub fn adjustment_exposure(&self, k: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.exposure.get_mut(k as usize)
                && value.is_finite()
            {
                *v = value;
            }
            a.exposure = exposure_kept(a.exposure);
        });
    }

    /// Curves' limit slider `k` (0 its peak, stops above white, 0..5 and none from 6; 1 its knee,
    /// 0..0.95) moved to `value`.
    pub fn adjustment_limit(&self, k: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.limit.get_mut(k as usize)
                && value.is_finite()
            {
                *v = value;
            }
            a.limit = limit_kept(a.limit);
        });
    }

    /// Adds a point to the channel shown; returns its index.
    pub fn adjustment_curve_add(&self, x: f32, y: f32) -> u32 {
        let mut at = 0;
        self.change_adjustment(|a, _| {
            let Some(points) = a.curves.get_mut(a.curve_channel) else { return };
            let p = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
            at = points.partition_point(|q| q.0 <= p.0);
            points.insert(at, p);
        });
        at as u32
    }

    /// Moves point `index` of the channel shown to (`x`, `y`), kept between its neighbours.
    pub fn adjustment_curve_move(&self, index: u32, x: f32, y: f32) {
        self.change_adjustment(|a, _| {
            let Some(points) = a.curves.get_mut(a.curve_channel) else { return };
            let i = index as usize;
            if i >= points.len() {
                return;
            }
            let lo = if i == 0 { 0.0 } else { points.get(i - 1).map_or(0.0, |p| p.0 + CURVE_GAP) };
            let hi = points.get(i + 1).map_or(1.0, |p| p.0 - CURVE_GAP);
            if let Some(p) = points.get_mut(i) {
                *p = (x.clamp(lo, lo.max(hi)), y.clamp(0.0, 1.0));
            }
        });
    }

    /// Takes point `index` off the channel shown (a curve keeps two).
    pub fn adjustment_curve_delete(&self, index: u32) {
        self.change_adjustment(|a, _| {
            if let Some(points) = a.curves.get_mut(a.curve_channel)
                && points.len() > 2
                && (index as usize) < points.len()
            {
                points.remove(index as usize);
            }
        });
    }

    /// The gradient (by its place in the library) mapped by; also redraws after the library
    /// changed.
    pub fn adjustment_gradient(&self, index: u32) {
        self.change_adjustment(|a, _| {
            let count = a.gradients.as_ref().map_or(1, |g| g.list().len()).max(1);
            a.gradient = (index as usize).min(count - 1);
        });
    }

    pub fn adjustment_noise_kind(&self, kind: NoiseKind) {
        self.change_adjustment(|a, _| a.noise_kind = kind);
    }

    /// Noise's slider `index` (0 scale, 1 octaves, 2 turbulence) moved to `value` (0..1).
    pub fn adjustment_noise_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.noise.get_mut(index as usize) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// Noise each channel its own (`multi`) or one for all; added to the layer or mixed in.
    pub fn adjustment_noise_ways(&self, multi: bool, added: bool) {
        self.change_adjustment(|a, _| {
            a.noise_multi = multi;
            a.noise_added = added;
        });
    }

    /// Bloom's slider `index` (0 transition, 1 size, 2 burn) moved to `value` (0..1).
    pub fn adjustment_bloom_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.bloom.get_mut(index as usize) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    pub fn adjustment_halftone_kind(&self, kind: HalftoneKind) {
        self.change_adjustment(|a, _| a.halftone_kind = kind);
    }

    pub fn adjustment_chroma_kind(&self, kind: ChromaticKind) {
        self.change_adjustment(|a, _| a.chroma_kind = kind);
    }

    /// Chromatic aberration's slider `index` of the kind shown moved to `value` (0..1).
    pub fn adjustment_chroma_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.chroma.get_mut(a.chroma_kind as usize).and_then(|c| c.get_mut(index as usize)) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// Displacing: a drag (`dx`, `dy`) window pixels from where it began (`start` at its beginning)
    /// moves the fringes on; how far shows as the amount, a quarter of the window's short side all
    /// of it.
    pub fn adjustment_chroma_drag(&self, dx: f32, dy: f32, start: bool) {
        self.change_adjustment(|a, (w, h)| {
            if start {
                a.chroma_from = a.chroma_offset;
            }
            a.chroma_offset = (a.chroma_from.0 + dx, a.chroma_from.1 + dy);
            let whole = w.min(h) as f32 / 4.0;
            let far = a.chroma_offset.0.hypot(a.chroma_offset.1);
            a.amount = if whole > 0.0 { (far / whole).clamp(0.0, 1.0) } else { 0.0 };
        });
    }

    pub fn adjustment_glitch_kind(&self, kind: GlitchKind) {
        self.change_adjustment(|a, _| a.glitch_kind = kind);
    }

    /// Slider `index` of the glitch kind shown moved to `value` (0..1, or −1..1 for diverge's
    /// shifts).
    pub fn adjustment_glitch_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.glitch.get_mut(a.glitch_kind as usize).and_then(|g| g.get_mut(index as usize)) {
                *v = value.clamp(-1.0, 1.0);
            }
        });
    }

    pub fn adjustment_dither_kind(&self, kind: DitherKind) {
        self.change_adjustment(|a, _| a.dither_kind = kind);
    }

    /// Dither's slider `index` (0 levels, 1 pattern size) moved to `value` (0..1).
    pub fn adjustment_dither_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.dither.get_mut(index as usize) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// Dither in black and white, or in colour.
    pub fn adjustment_dither_mono(&self, mono: bool) {
        self.change_adjustment(|a, _| a.dither_mono = mono);
    }

    pub fn adjustment_vignette_kind(&self, kind: VignetteKind) {
        self.change_adjustment(|a, _| a.vignette_kind = kind);
    }

    /// Vignette's slider `index` (0 size, 1 softness) moved to `value` (0..1).
    pub fn adjustment_vignette_value(&self, index: u32, value: f32) {
        self.change_adjustment(|a, _| {
            if let Some(v) = a.vignette.get_mut(index as usize) {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// The layer shown as it was before the adjustment, or as adjusted.
    pub fn adjustment_preview(&self, original: bool) {
        if self.adjusting(|a, _| a.open.map(|_| a.preview = original)).is_some() {
            self.adjust_preview(original);
        }
    }

    /// Every value back to where the adjustment started.
    pub fn adjustment_reset(&self) {
        let Some(open) = self.adjusting(|a, _| a.open) else { return };
        // an adjustment layer: its values as a new one's
        if self.shared.lock().adjust_layer.is_some() {
            self.adjusting(|a, _| {
                *a = Adjusting { gradients: a.gradients.clone(), layer: a.layer, ..Adjusting::default() };
            });
            self.adjustment_open(open);
            self.adjusting(|a, _| a.touched = true);
            return;
        }
        self.adjustment_close(false);
        self.adjustment_open(open);
    }

    /// Keeps what the adjustment did as one undo step and starts it again from there.
    pub fn adjustment_apply(&self) {
        match self.adjusting(|a, _| a.open) {
            // it draws as it opens: opened again, it would go over what it did
            Some(Adjustment::Plugin) => {
                self.adjustment_close(true);
            }
            Some(open) => self.adjustment_again(open, true),
            None => {}
        }
    }

    /// Takes back the adjustment's changes (perspective blur's puck a drag at a time; Liquify's a
    /// step at a time), or with none, the last one kept.
    pub fn adjustment_undo(&self) {
        if self.step_liquify(true).is_some() {
            return;
        }
        let Some((open, changed)) = self.adjusting(|a, _| a.open.map(|o| (o, a.changed()))) else { return };
        if self.perspective_step(true) {
            return;
        }
        // an adjustment layer: back to where its values started, or with nothing changed, closed and
        // the last step undone
        if self.shared.lock().adjust_layer.is_some() {
            if self.layer_changed() {
                self.layer_back();
            } else {
                self.adjustment_end(true);
                self.undo();
            }
            return;
        }
        if changed {
            self.adjustment_again(open, false);
            return;
        }
        let kept = self.adjusting(|a, _| (a.perspective_kind, a.perspective));
        self.adjustment_close(false);
        self.undo();
        self.adjustment_open(open);
        self.adjusting(|a, _| (a.perspective_kind, a.perspective) = kept);
    }

    /// Layer or Pencil: painting the adjustment on starts it at its own amount, and only where the
    /// brush goes shows it; back to the layer starts it again.
    pub fn adjustment_pencil(&self, pencil: bool) {
        let Some((open, was)) = self.adjusting(|a, _| a.open.map(|o| (o, a.pencil))) else { return };
        // an adjustment layer shows through its mask instead
        if pencil == was || !adjustment_paintable(open) || self.shared.lock().adjust_layer.is_some() {
            return;
        }
        if !pencil {
            self.adjustment_reset();
            return;
        }
        self.adjust_pencil();
        self.adjusting(|a, _| a.pencil = true);
        let Some(amount) = adjustment_pencil_amount(open) else { return };
        if open == Adjustment::Bloom {
            self.adjusting(|a, _| {
                a.bloom[1] = PENCIL_BLOOM_SIZE;
                a.bloom[2] = PENCIL_BLOOM_BURN;
            });
        }
        if open == Adjustment::MotionBlur {
            // along the canvas, as long as a drag of that much
            let scale = self.shared.lock().display_scale;
            self.adjustment_motion(amount * MOTION_DRAG_PT * scale, 0.0);
        } else {
            self.change_adjustment(|a, _| a.amount = amount);
        }
    }

    /// Perspective blur all round its puck, or only towards its arrow.
    pub fn adjustment_perspective_kind(&self, kind: PerspectiveKind) {
        let redraw = self.adjusting(|a, _| {
            if a.open != Some(Adjustment::PerspectiveBlur) {
                return Redraw::None;
            }
            a.perspective_kind = kind;
            a.redraw()
        });
        self.redraw_adjustment(redraw);
    }

    /// Liquify's way.
    pub fn adjustment_liquify_kind(&self, kind: LiquifyKind) {
        self.adjusting(|a, _| {
            a.liquify_kind = kind;
            a.liquify[2] = a.liquify_distortions.get(kind as usize).copied().unwrap_or(0.0);
        });
    }

    /// Liquify's slider `index` (0 size, 1 pressure, 2 distortion, 3 momentum) moved to `value`.
    pub fn adjustment_liquify_value(&self, index: u32, value: f32) {
        self.adjusting(|a, _| {
            if let Some(v) = a.liquify.get_mut(index as usize) {
                *v = value.clamp(0.0, 1.0);
            }
            if index == 2
                && let Some(v) = a.liquify_distortions.get_mut(a.liquify_kind as usize)
            {
                *v = value.clamp(0.0, 1.0);
            }
        });
    }

    /// How much of what liquify did shows (0..1).
    pub fn adjustment_liquify_amount(&self, amount: f32) {
        let amount = amount.clamp(0.0, 1.0);
        self.adjusting(|a, _| a.liquify_amount = amount);
        self.liquify_amount(amount);
    }

    /// The finger let go of Adjust's slider: that drag is an undo step of its own.
    pub fn adjustment_liquify_amount_end(&self) {
        self.shared.lock().liquify_adjust_ends();
    }

    /// Everything liquify did undone.
    pub fn adjustment_liquify_reset(&self) {
        self.liquify_reset();
        self.adjusting(|a, _| {
            a.liquified = false;
            a.liquify_amount = 1.0;
        });
    }

    /// A liquify stroke's window points (x, y, pressure, milliseconds since the last, each), a
    /// pen's if `pen`; `start` begins it.
    pub fn adjustment_liquify_stroke(&self, points: Vec<f32>, start: bool, pen: bool) {
        let brush = self.adjusting(|a, _| {
            a.liquified = true;
            a.touched = true;
            a.liquify_brush()
        });
        self.liquify_stroke(points, start, pen, brush);
    }

    /// The liquify brush held still at window point (`x`, `y`) works on, from `from` to `to`
    /// seconds of being held. False if the kind of brush does nothing held.
    pub fn adjustment_liquify_held(&self, x: f32, y: f32, from: f32, to: f32) -> bool {
        let brush = self.adjusting(|a, _| a.liquify_brush());
        self.liquify_held(x, y, from, to, brush)
    }

    /// The pen's barrel rolled `turn` radians with the liquify brush at window point (`x`, `y`):
    /// a twirl turns with it.
    pub fn adjustment_liquify_rolled(&self, x: f32, y: f32, turn: f32) {
        let brush = self.adjusting(|a, _| a.liquify_brush());
        if self.liquify_rolled(x, y, turn, brush) {
            self.adjusting(|a, _| {
                a.liquified = true;
                a.touched = true;
            });
        }
    }

    /// The liquify brush lifts: true if it, or what it threw off, goes on.
    pub fn adjustment_liquify_lift(&self) -> bool {
        let (brush, momentum) = self.adjusting(|a, _| (a.liquify_brush(), a.liquify_momentum()));
        self.liquify_lift(momentum, brush)
    }

    /// `seconds` go by for what liquify set going: true if it goes on. Nothing does once it closes.
    pub fn adjustment_liquify_tick(&self, seconds: f32) -> bool {
        let (open, brush) = self.adjusting(|a, _| (a.open, a.liquify_brush()));
        open == Some(Adjustment::Liquify) && self.liquify_tick(seconds, brush)
    }

    /// Cloning, the source moved to window point (`x`, `y`): the next stroke fixes how far from it
    /// the brush is.
    pub fn adjustment_clone_move(&self, x: f32, y: f32) {
        {
            let mut st = self.shared.lock();
            let Some(at) = st.window_to_canvas(x, y) else { return };
            st.adjust.puck = Some(at);
            st.adjust.puck_offset = None;
        }
        self.clone_source(x, y);
    }

    /// The curve of `channel` as heights to draw, 256 across.
    pub fn adjustment_curve_shape(&self, channel: u32) -> Vec<f32> {
        let points = self.adjusting(|a, _| a.curves.get(channel as usize).cloned().unwrap_or_else(straight));
        self.curve_shape(points.iter().flat_map(|&(x, y)| [x, y]).collect())
    }
}

impl CanvasSession {
    /// Cloning, a stroke reached window point `at` (`start`: it begins there): the source follows
    /// the brush, as far from it as the first stroke after it moved put it.
    pub(crate) fn clone_stroke_at(&self, at: (f32, f32), start: bool) {
        let mut st = self.shared.lock();
        let Some(puck) = st.adjust.puck else { return };
        let Some(at) = st.window_to_canvas(at.0, at.1) else { return };
        let a = &mut st.adjust;
        if start {
            a.puck_before.push(puck);
            a.puck_after.clear();
        }
        let offset = *a.puck_offset.get_or_insert(puck - at);
        a.puck = Some(at + offset);
    }

    /// Cloning, where the source is in the window.
    pub(crate) fn clone_puck_at(&self) -> Option<(f32, f32)> {
        let st = self.shared.lock();
        let at = st.view?.to_screen().apply(st.adjust.puck?);
        Some((at.x, at.y))
    }

    /// Perspective blur on the layer (not painted on): its way, where its puck is in the window,
    /// and the way its arrow points there (radians).
    pub(crate) fn perspective_puck_at(&self) -> Option<(PerspectiveKind, (f32, f32), f32)> {
        let shown = self.adjustment_shown();
        let &[x, y, tx, ty] = shown.perspective_puck.as_deref()? else { return None };
        Some((shown.perspective_kind, (x, y), (ty - y).atan2(tx - x)))
    }

    /// A drag on perspective blur's puck begins: undo puts it back as it is now.
    pub(crate) fn perspective_grabbed(&self) {
        self.adjusting(|a, _| {
            if a.open == Some(Adjustment::PerspectiveBlur) {
                a.perspective_before.push(a.perspective);
                a.perspective_after.clear();
            }
        });
    }

    /// Perspective blur's puck dragged to window point (`x`, `y`).
    pub(crate) fn perspective_move(&self, x: f32, y: f32) {
        let Some(at) = self.shared.lock().window_to_canvas(x, y) else { return };
        self.change_adjustment(|a, _| a.perspective.origin = Some(at));
    }

    /// Perspective blur's arrow turned to point from its puck at window point (`x`, `y`).
    pub(crate) fn perspective_turn(&self, x: f32, y: f32) {
        let away = {
            let st = self.shared.lock();
            let middle = Vec2::new(st.doc.width() as f32 / 2.0, st.doc.height() as f32 / 2.0);
            st.window_to_canvas(x, y).map(|at| at - st.adjust.perspective.origin.unwrap_or(middle))
        };
        if let Some(away) = away.filter(|d| d.length() > 0.0) {
            self.change_adjustment(|a, _| a.perspective.arrow = away.y.atan2(away.x));
        }
    }
}

impl crate::State {
    /// The canvas angle (radians) of the way that points at `angle` in the window.
    fn canvas_angle(&self, angle: f32) -> f32 {
        let way = Vec2::new(angle.cos(), angle.sin());
        self.view
            .and_then(|v| v.to_screen().inverse())
            .map(|inv| inv.apply(way) - inv.apply(Vec2::new(0.0, 0.0)))
            .filter(|d| d.is_finite() && d.length() > 0.0)
            .map_or(angle, |d| d.y.atan2(d.x))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_start_as_the_app_ships_them() {
        let a = Adjusting::default();
        let shown = a.shown(None, Vec2::new(32.0, 32.0));
        assert_eq!(shown.adjustment, None);
        assert_eq!(shown.noise, vec![0.0, 0.0, 0.7]);
        assert_eq!(shown.bloom, vec![1.0, 0.3, 0.35]);
        assert_eq!(shown.glitch, vec![1.0, 0.09, 1.0]);
        assert_eq!(shown.curves[0], vec![0.0, 0.0, 1.0, 1.0]);
        assert!(!shown.changed);
    }

    #[test]
    fn what_counts_as_changed() {
        let mut a = Adjusting { open: Some(Adjustment::Hsb), ..Adjusting::default() };
        assert!(!a.changed());
        a.hsb[1] = 0.6;
        assert!(a.changed());
        a.open = Some(Adjustment::GradientMap);
        assert!(a.changed());
        a.open = Some(Adjustment::GaussianBlur);
        assert!(!a.changed());
        a.amount = 0.1;
        assert!(a.changed());
        assert!(matches!(a.redraw(), Redraw::Swipe(SwipeAdjustment::GaussianBlur, x) if x == 0.1));
    }
}

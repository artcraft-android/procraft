//! Vector layers shared with other programs: sent out as an SVG picture or a PDF page of shapes
//! (`procraft_engine::vector_export`), and an SVG picture taken in as a vector layer of strokes
//! and fills.

use std::sync::Arc;

use procraft_engine::brush::{Brush, Taper};
use procraft_engine::geom::Vec2;
use procraft_engine::input::{Pointer, Sample};
use procraft_engine::vector::{FillMode, Ink, Setup, VectorContent, VectorFill, VectorStroke};
use procraft_engine::vector_export::{self, Picture};

use crate::{CanvasSession, MobileError, State};

/// How far apart the input of a line read from a picture is laid along it (canvas pixels).
const INPUT_APART: f32 = 2.0;

/// The most strokes and fills taken from one picture.
const MOST_OBJECTS: usize = 20_000;

/// The pen a line read from a picture is drawn with: round and hard, one width from end to end
/// whatever the pen that draws it does, and no steadier than it was drawn.
fn plain_pen() -> Brush {
    let none = Taper { start: 0.0, end: 0.0, size: 0.0, opacity: 0.0 };
    Brush {
        name: "SVG Line".to_owned(),
        streamline: 0.0,
        hardness: 1.0,
        pressure_size: 0.0,
        pressure_opacity: 0.0,
        pen_taper: none.clone(),
        touch_taper: none,
        ..Brush::round_pen()
    }
}

/// What is being read out of a picture: where its own measure goes on the canvas, and what has
/// been made of it so far.
struct Reading {
    /// The picture's own measure to canvas pixels: x' = a x + c y + e, y' = b x + d y + f.
    fit: [f32; 6],
    pen: Arc<Brush>,
    content: VectorContent,
    /// Its colours (sRGB) in the canvas's profile; none if that is sRGB.
    colours: Option<ColourWay>,
}

impl Reading {
    /// Where a point of a path (in its own measure, carried by `by`) lies on the canvas.
    fn at(&self, by: usvg::Transform, x: f32, y: f32) -> Vec2 {
        let (px, py) = (by.sx * x + by.kx * y + by.tx, by.ky * x + by.sy * y + by.ty);
        let [a, b, c, d, e, f] = self.fit;
        Vec2::new(a * px + c * py + e, b * px + d * py + f)
    }

    /// How much bigger than its own measure `by` and the fit draw things.
    fn scale(&self, by: usvg::Transform) -> f32 {
        let [a, b, c, d, ..] = self.fit;
        ((by.sx * by.sy - by.kx * by.ky).abs() * (a * d - b * c).abs()).sqrt()
    }

    /// A path's lines, each a run of canvas points, with whether it closes: its curves as points
    /// along them.
    fn lines(&self, path: &usvg::Path) -> Vec<(Vec<Vec2>, bool)> {
        use usvg::tiny_skia_path::PathSegment;
        let by = path.abs_transform();
        let at = |p: usvg::tiny_skia_path::Point| self.at(by, p.x, p.y);
        let mut lines: Vec<(Vec<Vec2>, bool)> = Vec::new();
        let mut from = Vec2::ZERO;
        // (a curve as points along it, close enough together to pass for it)
        let pieces = |length: f32| (length / 1.5).ceil().clamp(2.0, 96.0) as usize;
        for segment in path.data().segments() {
            match segment {
                PathSegment::MoveTo(p) => {
                    from = at(p);
                    lines.push((vec![from], false));
                }
                PathSegment::LineTo(p) => {
                    from = at(p);
                    if let Some((line, _)) = lines.last_mut() {
                        line.push(from);
                    }
                }
                PathSegment::QuadTo(c, p) => {
                    let (c, to) = (at(c), at(p));
                    let n = pieces(from.distance(c) + c.distance(to));
                    if let Some((line, _)) = lines.last_mut() {
                        line.extend((1..=n).map(|k| {
                            let t = k as f32 / n as f32;
                            from.lerp(c, t).lerp(c.lerp(to, t), t)
                        }));
                    }
                    from = to;
                }
                PathSegment::CubicTo(c1, c2, p) => {
                    let (c1, c2, to) = (at(c1), at(c2), at(p));
                    let n = pieces(from.distance(c1) + c1.distance(c2) + c2.distance(to));
                    if let Some((line, _)) = lines.last_mut() {
                        line.extend((1..=n).map(|k| {
                            let t = k as f32 / n as f32;
                            let (ab, bc, cd) = (from.lerp(c1, t), c1.lerp(c2, t), c2.lerp(to, t));
                            ab.lerp(bc, t).lerp(bc.lerp(cd, t), t)
                        }));
                    }
                    from = to;
                }
                PathSegment::Close => {
                    if let Some((line, closed)) = lines.last_mut() {
                        *closed = true;
                        from = line.first().copied().unwrap_or(from);
                    }
                }
            }
        }
        lines.retain(|(line, _)| line.iter().all(|p| p.is_finite()) && !line.is_empty());
        lines
    }

    /// `colour` (sRGB, as SVG has it) as the canvas has its colours.
    fn colour(&self, colour: [f32; 3]) -> [f32; 3] {
        self.colours.as_ref().map_or(colour, |way| way.convert(colour))
    }

    /// Everything under `group`, lowest first, each shape as faint as `opacity` and its own make it.
    fn group(&mut self, group: &usvg::Group, opacity: f32) {
        let opacity = opacity * group.opacity().get();
        for node in group.children() {
            if self.content.len() >= MOST_OBJECTS {
                return;
            }
            match node {
                usvg::Node::Group(inner) => self.group(inner, opacity),
                // (a picture this app wrote has the canvas's background in it, which is no shape)
                usvg::Node::Path(path) if path.is_visible() && path.id() != vector_export::BACKGROUND => {
                    self.path(path, opacity);
                }
                _ => {}
            }
        }
    }

    /// One shape: what it is filled with as a fill, the line round it as strokes.
    fn path(&mut self, path: &usvg::Path, opacity: f32) {
        let lines = self.lines(path);
        if let Some(fill) = path.fill()
            && let Some(colour) = colour_of(fill.paint()).map(|c| self.colour(c))
        {
            let loops: Vec<Vec<Vec2>> =
                lines.iter().filter(|(line, _)| line.len() >= 3).map(|(l, _)| l.clone()).collect();
            if !loops.is_empty() {
                self.content.push_fill(VectorFill {
                    id: self.content.next_id(),
                    loops,
                    colour,
                    opacity: (opacity * fill.opacity().get()).clamp(0.0, 1.0),
                    mode: FillMode::Over,
                    hard: false,
                    traced: None,
                    takes_from: None,
                });
            }
        }
        let Some(stroke) = path.stroke() else { return };
        let Some(colour) = colour_of(stroke.paint()).map(|c| self.colour(c)) else { return };
        let width = stroke.width().get() * self.scale(path.abs_transform());
        if !(width.is_finite() && width > 0.0) {
            return;
        }
        for (line, closed) in lines {
            let mut points = line;
            if closed && let Some(first) = points.first().copied() {
                points.push(first);
            }
            // the pen's input along it: pressed evenly, a few pixels apart
            let mut samples: Vec<Sample> = Vec::new();
            for to in points {
                let before = samples.last().map(|s| s.pos);
                let pieces = before.map_or(1, |b| (b.distance(to) / INPUT_APART).ceil().clamp(1.0, 4096.0) as usize);
                for k in 1..=pieces {
                    let pos = before.map_or(to, |b| b.lerp(to, k as f32 / pieces as f32));
                    samples.push(Sample::at(pos, 1.0, samples.len() as f64 * 4.0));
                }
            }
            if samples.is_empty() {
                continue;
            }
            let id = self.content.next_id();
            let ink = Ink {
                colour,
                secondary: colour,
                opacity: (opacity * stroke.opacity().get()).clamp(0.0, 1.0),
                erases: false,
                preserve_alpha: false,
                wetness: 0.0,
            };
            let seed = (id.0 as u32).wrapping_mul(0x9e37_79b9);
            let mut kept =
                VectorStroke::begin(id, Setup::new(Arc::clone(&self.pen), width, Pointer::Stylus, seed), ink);
            kept.record(&samples);
            self.content.push(kept);
        }
    }
}

/// Colours moved from one profile to another.
struct ColourWay {
    from: Arc<procraft_engine::cms::IccProfile>,
    to: Arc<procraft_engine::cms::IccProfile>,
}

impl ColourWay {
    fn convert(&self, colour: [f32; 3]) -> [f32; 3] {
        let device = colour.map(|c| f64::from(c.clamp(0.0, 1.0)));
        let out = self.from.convert(&device, &self.to);
        std::array::from_fn(|i| out.get(i).map_or(colour[i], |&c| (c as f32).clamp(0.0, 1.0)))
    }
}

/// The colour a shape is painted with (one that shades from one colour to others: its first).
fn colour_of(paint: &usvg::Paint) -> Option<[f32; 3]> {
    let colour = match paint {
        usvg::Paint::Color(colour) => *colour,
        usvg::Paint::LinearGradient(gradient) => gradient.stops().first()?.color(),
        usvg::Paint::RadialGradient(gradient) => gradient.stops().first()?.color(),
        usvg::Paint::Pattern(_) => return None,
    };
    Some([colour.red, colour.green, colour.blue].map(|c| f32::from(c) / 255.0))
}

impl State {
    /// The artwork's vector layers as a picture of shapes: those that show and hold anything,
    /// lowest first, over the background if that shows. None if there are none.
    pub(crate) fn vector_picture(&self) -> Option<Picture> {
        let layers: Vec<vector_export::Group> = self
            .doc
            .layers()
            .iter()
            .filter(|l| self.doc.unseen(l.id).is_none())
            .filter_map(|l| {
                let content = l.vector.as_ref().filter(|content| !content.is_empty())?;
                Some(vector_export::layer(content, &l.name, l.opacity))
            })
            .collect();
        if layers.is_empty() {
            return None;
        }
        let mut picture = Picture {
            width: self.doc.width(),
            height: self.doc.height(),
            dpi: self.doc.dpi,
            background: self.doc.background_visible.then_some(self.doc.background),
            layers,
        };
        // (SVG and the PDF's colours are sRGB: a canvas in another profile has its own brought to it)
        if let Some(way) = self.colour_way(false) {
            picture.map_colours(&|c| way.convert(c));
        }
        Some(picture)
    }

    /// How colours go between the canvas's profile and sRGB: into the canvas's (`into`) or out
    /// of it. None if the canvas is in sRGB itself, so they stay as they are.
    fn colour_way(&self, into: bool) -> Option<ColourWay> {
        use procraft_engine::profile::Profile;
        if self.doc.icc.is_none() && self.doc.profile == Profile::Srgb {
            return None;
        }
        let (canvas, srgb) = (self.doc.rgb_profile(), procraft_engine::cms::builtin(Profile::Srgb));
        Some(if into { ColourWay { from: srgb, to: canvas } } else { ColourWay { from: canvas, to: srgb } })
    }

    /// What an SVG picture (`bytes`, its file) draws, as a vector layer would hold it on this
    /// canvas: fitted whole onto it, in the middle (a picture the canvas's own size lies as it
    /// is). Each filled shape a fill, each line round one a stroke of a plain round pen as wide
    /// as the line. None if it can't be read, or draws nothing of the kind.
    pub(crate) fn svg_content(&self, bytes: &[u8]) -> Option<VectorContent> {
        let tree = usvg::Tree::from_data(bytes, &usvg::Options::default()).ok()?;
        let (pw, ph) = (tree.size().width(), tree.size().height());
        let (cw, ch) = (self.doc.width() as f32, self.doc.height() as f32);
        if !(pw > 0.0 && ph > 0.0) {
            return None;
        }
        let by = (cw / pw).min(ch / ph);
        let fit = [by, 0.0, 0.0, by, (cw - pw * by) / 2.0, (ch - ph * by) / 2.0];
        let colours = self.colour_way(true);
        let mut reading = Reading { fit, pen: Arc::new(plain_pen()), content: VectorContent::default(), colours };
        reading.group(tree.root(), 1.0);
        (!reading.content.is_empty()).then_some(reading.content)
    }

    /// An SVG picture inserted as a new vector layer called `name` above the current one, one
    /// undo step: its id, or none if it can't be read, draws nothing, or there is no room.
    pub(crate) fn insert_svg(&mut self, bytes: &[u8], name: &str) -> Option<u64> {
        let content = self.svg_content(bytes)?;
        self.finish_stroke();
        self.settle();
        if !self.doc.has_room(1) || self.scene_shown() {
            return None;
        }
        // (a new layer is drawn whole: nothing picked elsewhere keeps it to itself)
        self.let_go_of_selection();
        let before = self.doc.clone();
        let layer = self.doc.add_vector_layer();
        if !name.trim().is_empty() {
            self.doc.rename(layer, name.trim());
        }
        self.doc.set_vector(layer, content.clone());
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.mark_steps();
            canvas.name_next_step("Insert SVG");
            canvas.record_document(before);
        }
        self.draw_vector(layer, &content, None);
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.collapse_steps();
        }
        self.revision += 1;
        Some(layer.0)
    }
}

fn reason(why: &str) -> MobileError {
    MobileError::File { reason: why.to_owned() }
}

#[uniffi::export]
impl CanvasSession {
    /// Whether the artwork has vector layers with something on them that shows: it shares as a
    /// PDF of shapes ([`CanvasSession::export_vector_pdf`]), and as SVG.
    pub fn exports_vectors(&self) -> bool {
        self.shared.lock().vector_picture().is_some()
    }

    /// The artwork's vector layers written to `path` as a one-page PDF of shapes (see
    /// `procraft_engine::vector_export`): the page the canvas's size, each stroke and fill a shape
    /// a program that edits such things can take hold of. Layers of pixels are not in it.
    pub fn export_vector_pdf(&self, path: String) -> Result<(), MobileError> {
        let picture = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            st.vector_picture()
        };
        let picture = picture.ok_or_else(|| reason("no vector layer with anything on it"))?;
        let bytes = vector_export::pdf(&picture);
        crate::project::write_atomically(std::path::Path::new(&path), |p| {
            std::fs::write(p, &bytes).map_err(|e| reason(&format!("writing the picture: {e}")))
        })
    }

    /// Inserts an SVG picture (`bytes`, its file) as a new vector layer called `name` above the
    /// current one, made current: its filled shapes fills, its lines strokes, fitted onto the
    /// canvas. 0 if it can't be read, draws nothing of the kind, or the canvas has no room for
    /// another layer.
    pub fn insert_svg(&self, bytes: Vec<u8>, name: String) -> u64 {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let layer = st.insert_svg(&bytes, &name).unwrap_or(0);
        if layer != 0 {
            self.shared.frame(st);
        }
        layer
    }
}

impl CanvasSession {
    /// The artwork's vector layers as an SVG picture's text, if it has any with something on them.
    pub(crate) fn vector_svg(&self) -> Option<String> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        st.vector_picture().map(|picture| vector_export::svg(&picture))
    }
}

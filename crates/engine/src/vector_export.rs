//! Vector layers as shapes other programs read: an SVG picture, or a PDF page.
//!
//! A stroke is kept as what the pen did and is drawn by its brush, stamp by stamp; nothing else
//! draws it that way. What goes out is its shape: a line as wide as the stroke where it keeps one
//! width, an outline around it where its width changes, in its colour and at its opacity. A brush's
//! grain, its edges' softness and how its paint mixes stay behind. A fill goes out as its outline.
//! What an eraser took is taken from the shapes it was made on, as a mask over them.

use std::fmt::Write as _;

use crate::geom::Vec2;
use crate::stroke::PathSpot;
use crate::vector::{FillMode, Object, VectorContent, VectorStroke};

/// A picture of shapes: the canvas's size in pixels, how many of them make an inch, the colour
/// behind everything (none: clear) and its layers, lowest first.
#[derive(Clone, Debug, PartialEq)]
pub struct Picture {
    pub width: u32,
    pub height: u32,
    pub dpi: f32,
    pub background: Option<[f32; 3]>,
    pub layers: Vec<Group>,
}

impl Picture {
    /// Every colour in it (its background's, every shape's and every eraser mark's) made `to`
    /// of itself: how a picture whose colours are in another profile is brought to sRGB.
    pub fn map_colours(&mut self, to: &impl Fn([f32; 3]) -> [f32; 3]) {
        fn group(g: &mut Group, to: &impl Fn([f32; 3]) -> [f32; 3]) {
            for shape in &mut g.taken {
                shape.colour = to(shape.colour);
            }
            for node in &mut g.nodes {
                match node {
                    Node::Shape(shape) => shape.colour = to(shape.colour),
                    Node::Group(inner) => group(inner, to),
                }
            }
        }
        if let Some(colour) = self.background.as_mut() {
            *colour = to(*colour);
        }
        for g in &mut self.layers {
            group(g, to);
        }
    }
}

/// Shapes drawn together, lowest first, as one: at `opacity`, and less what `taken` covers of them
/// (an eraser's marks, each taking as much as its own opacity says).
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    pub name: Option<String>,
    pub opacity: f32,
    pub nodes: Vec<Node>,
    pub taken: Vec<Shape>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Shape(Shape),
    Group(Group),
}

/// One shape in one colour (sRGB, 0..1).
#[derive(Clone, Debug, PartialEq)]
pub struct Shape {
    pub outline: Outline,
    pub colour: [f32; 3],
    pub opacity: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Outline {
    /// A line `width` across through `points`, round at its ends and where it turns.
    Line { points: Vec<Vec2>, width: f32 },
    /// What closed outlines hold: inside an odd number of them (`even_odd`), or inside any;
    /// `smooth`, they curve through their points rather than run straight between them.
    Area { loops: Vec<Vec<Vec2>>, even_odd: bool, smooth: bool },
}

/// How far a stroke's line may stray from the points kept of it, and its width from theirs (pixels).
const KEPT_WITHIN: f32 = 0.08;

/// What the canvas's background colour is called in an SVG picture: by it, it is known for what
/// it is when the picture is read back.
pub const BACKGROUND: &str = "background";

/// A turn sharper than this (radians) is a corner: the line does not curve through it.
const CORNER: f32 = 0.7;

/// What vector layer `content` draws, as a group called `name` at `opacity`: each stroke and fill
/// a shape, in the order they lie, each with what its erasers took.
pub fn layer(content: &VectorContent, name: &str, opacity: f32) -> Group {
    let mut nodes: Vec<Node> = Vec::new();
    for object in content.objects() {
        if object.erases() {
            // one that takes from all made before it takes from all drawn so far; one that names
            // what it takes from is with each of those
            if object.takes_from().is_none() {
                let taken = shapes_of(object);
                if !taken.is_empty() && !nodes.is_empty() {
                    let under = std::mem::take(&mut nodes);
                    nodes.push(Node::Group(Group { name: None, opacity: 1.0, nodes: under, taken }));
                }
            }
            continue;
        }
        let shapes = shapes_of(object);
        if shapes.is_empty() {
            continue;
        }
        let taken: Vec<Shape> = content.erasers_of(object.id()).iter().flat_map(shapes_of).collect();
        let mut own: Vec<Node> = shapes.into_iter().map(Node::Shape).collect();
        if !taken.is_empty() {
            own = vec![Node::Group(Group { name: None, opacity: 1.0, nodes: own, taken })];
        }
        // (a fill laid under what was there is under it)
        match object {
            Object::Fill(fill) if fill.mode == FillMode::Behind => {
                nodes.splice(0..0, own);
            }
            _ => nodes.extend(own),
        }
    }
    Group { name: Some(name.to_owned()), opacity: opacity.clamp(0.0, 1.0), nodes, taken: Vec::new() }
}

/// The shapes one stroke or fill draws (an eraser's: what it takes).
fn shapes_of(object: &Object) -> Vec<Shape> {
    match object {
        Object::Stroke(stroke) => stroke_shapes(stroke),
        Object::Fill(fill) => {
            let loops: Vec<Vec<Vec2>> = fill.loops.iter().filter(|outline| outline.len() >= 3).cloned().collect();
            if loops.is_empty() {
                return Vec::new();
            }
            vec![Shape {
                outline: Outline::Area { loops, even_odd: true, smooth: false },
                colour: fill.colour,
                opacity: fill.opacity.clamp(0.0, 1.0),
            }]
        }
    }
}

/// A stroke's shape, and that of each copy its guide's symmetry drew of it.
fn stroke_shapes(stroke: &VectorStroke) -> Vec<Shape> {
    let mut path = stroke.placed_path();
    // as wide at each place as the stamps the brush laid there (its path knows the pen's pressure
    // but not how the brush thins a stroke's ends), evened out over a few of them, and as much
    // narrower as the brush's edge is soft: a soft stamp's ink stops short of its rim
    let replay = stroke.replay();
    let mut laid: Vec<(f32, f32)> = replay.stamps.iter().map(|stamp| (stamp.distance, stamp.radius)).collect();
    laid.retain(|(along, radius)| along.is_finite() && radius.is_finite());
    laid.sort_by(|a, b| a.0.total_cmp(&b.0));
    let evened: Vec<(f32, f32)> = (0..laid.len())
        .map(|i| {
            let near = laid.get(i.saturating_sub(2)..(i + 3).min(laid.len())).unwrap_or_default();
            let mean = near.iter().map(|(_, radius)| radius).sum::<f32>() / near.len().max(1) as f32;
            (laid.get(i).map_or(0.0, |(along, _)| *along), mean)
        })
        .collect();
    let soft = 0.5 + 0.5 * stroke.setup.brush.hardness.clamp(0.0, 1.0);
    let own = stroke.path();
    for (spot, own) in path.iter_mut().zip(own.iter()) {
        // (the path lies where the stroke was placed, as much bigger as that made it)
        let placed = if own.radius > 1e-6 { spot.radius / own.radius } else { 1.0 };
        let after = evened.partition_point(|(along, _)| *along < own.distance);
        let radius = match (after.checked_sub(1).and_then(|i| evened.get(i)), evened.get(after)) {
            (Some(a), Some(b)) if b.0 > a.0 => a.1 + (b.1 - a.1) * ((own.distance - a.0) / (b.0 - a.0)).clamp(0.0, 1.0),
            (Some(only), None) | (None, Some(only)) | (Some(only), Some(_)) => only.1,
            (None, None) => own.radius,
        };
        spot.radius = radius * placed * soft;
    }
    let mut lines = vec![path.clone()];
    if let Some(guide) = stroke.guide.as_deref() {
        let copies = path.first().map_or(0, |spot| guide.copies(spot.pos).len());
        for k in 0..copies {
            lines.push(
                path.iter()
                    .filter_map(|spot| guide.copies(spot.pos).get(k).map(|copy| PathSpot { pos: copy.0, ..*spot }))
                    .collect(),
            );
        }
    }
    lines
        .iter()
        .filter_map(|line| outline_of(line))
        .map(|outline| Shape { outline, colour: stroke.ink.colour, opacity: stroke.ink.opacity.clamp(0.0, 1.0) })
        .collect()
}

/// The fewest of `path`'s spots that it stays within [`KEPT_WITHIN`] of, in place and in width.
fn simplified(path: &[PathSpot]) -> Vec<PathSpot> {
    let path: Vec<PathSpot> = path.iter().copied().filter(|s| s.pos.is_finite() && s.radius.is_finite()).collect();
    if path.len() < 3 {
        return path;
    }
    let mut keep = vec![false; path.len()];
    let mut spans = vec![(0usize, path.len() - 1)];
    while let Some((a, b)) = spans.pop() {
        let (Some(pa), Some(pb)) = (path.get(a), path.get(b)) else { continue };
        let along = pb.pos - pa.pos;
        let length2 = along.dot(along).max(1e-12);
        let mut worst = (KEPT_WITHIN, None);
        for (i, spot) in path.iter().enumerate().take(b).skip(a + 1) {
            let t = ((spot.pos - pa.pos).dot(along) / length2).clamp(0.0, 1.0);
            let off = spot.pos.distance(pa.pos + along * t);
            let wide = (spot.radius - (pa.radius + (pb.radius - pa.radius) * t)).abs();
            let off = off.max(wide);
            if off > worst.0 {
                worst = (off, Some(i));
            }
        }
        if let (_, Some(i)) = worst {
            if let Some(kept) = keep.get_mut(i) {
                *kept = true;
            }
            spans.push((a, i));
            spans.push((i, b));
        }
    }
    let last = path.len() - 1;
    path.iter()
        .zip(keep)
        .enumerate()
        .filter(|(i, (_, kept))| *kept || *i == 0 || *i == last)
        .map(|(_, (s, _))| *s)
        .collect()
}

/// A circle about `centre`, as points round it.
fn circle(centre: Vec2, radius: f32) -> Vec<Vec2> {
    (0..12)
        .map(|k| {
            let a = k as f32 / 12.0 * std::f32::consts::TAU;
            centre + Vec2::new(a.cos(), a.sin()) * radius
        })
        .collect()
}

/// The shape of a stroke along `path` (each spot with half its width there); none for no path.
fn outline_of(path: &[PathSpot]) -> Option<Outline> {
    let spots = simplified(path);
    let first = spots.first()?;
    let Some(last) = spots.last().filter(|_| spots.len() > 1) else {
        let loops = vec![circle(first.pos, first.radius.max(0.25))];
        return Some(Outline::Area { loops, even_odd: false, smooth: true });
    };
    let low = spots.iter().map(|s| s.radius).fold(f32::INFINITY, f32::min);
    let high = spots.iter().map(|s| s.radius).fold(0.0, f32::max);
    // one width all along: a line of that width
    if high - low <= (0.04 * high).max(0.1) {
        return Some(Outline::Line { points: spots.iter().map(|s| s.pos).collect(), width: (low + high).max(0.1) });
    }
    // else its two sides, each spot's half width out from it, joined round its ends: up one side,
    // round the end, back down the other, round the start
    let ways: Vec<Vec2> = (0..spots.len())
        .map(|i| {
            let before = spots.get(i.saturating_sub(1)).map_or(first.pos, |s| s.pos);
            let after = spots.get(i + 1).map_or(last.pos, |s| s.pos);
            let way = after - before;
            let length = way.length();
            if length > 1e-6 { way * (1.0 / length) } else { Vec2::new(1.0, 0.0) }
        })
        .collect();
    let side = |way: Vec2| Vec2::new(way.y, -way.x);
    let mut outline = Vec::with_capacity(spots.len() * 2 + 16);
    outline.extend(spots.iter().zip(&ways).map(|(s, way)| s.pos + side(*way) * s.radius));
    let cap = |outline: &mut Vec<Vec2>, at: &PathSpot, out: Vec2, way: Vec2| {
        for k in 1..6 {
            let a = k as f32 / 6.0 * std::f32::consts::PI;
            outline.push(at.pos + (out * a.cos() + way * a.sin()) * at.radius);
        }
    };
    if let Some(way) = ways.last() {
        cap(&mut outline, last, side(*way), *way);
    }
    outline.extend(spots.iter().zip(&ways).rev().map(|(s, way)| s.pos - side(*way) * s.radius));
    if let Some(way) = ways.first() {
        cap(&mut outline, first, side(*way) * -1.0, *way * -1.0);
    }
    // where it turns sharply its sides leave the outside of the turn open: a disc there closes it
    let mut loops = vec![outline];
    for i in 1..spots.len() - 1 {
        let (Some(a), Some(b), Some(c)) = (spots.get(i - 1), spots.get(i), spots.get(i + 1)) else { continue };
        if turn(b.pos - a.pos, c.pos - b.pos) > CORNER * 0.5 {
            loops.push(circle(b.pos, b.radius));
        }
    }
    Some(Outline::Area { loops, even_odd: false, smooth: true })
}

/// The angle between two ways (radians, 0..π); none of it where either is no way at all.
fn turn(a: Vec2, b: Vec2) -> f32 {
    let (la, lb) = (a.length(), b.length());
    if la < 1e-6 || lb < 1e-6 {
        return 0.0;
    }
    (a.dot(b) / (la * lb)).clamp(-1.0, 1.0).acos()
}

/// A piece of a path.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Piece {
    Move(Vec2),
    Line(Vec2),
    Curve(Vec2, Vec2, Vec2),
    Close,
}

/// The path through `points`: straight between them, or (`smooth`) curving through them, but for
/// corners; `closed`, back to the first.
fn pieces(points: &[Vec2], closed: bool, smooth: bool) -> Vec<Piece> {
    let points: Vec<Vec2> = points.iter().copied().filter(|p| p.is_finite()).collect();
    let n = points.len();
    let Some(first) = points.first().copied() else { return Vec::new() };
    let mut out = vec![Piece::Move(first)];
    let at = |i: usize| points.get(i % n).copied().unwrap_or(first);
    // the way the path goes through each point; none at a corner or an end
    let way = |i: usize| -> Option<Vec2> {
        if !smooth || n < 3 || (!closed && (i == 0 || i == n - 1)) {
            return None;
        }
        let (before, here, after) = (at(i + n - 1), at(i), at(i + 1));
        if turn(here - before, after - here) > CORNER {
            return None;
        }
        let way = after - before;
        let length = way.length();
        (length > 1e-6).then(|| way * (1.0 / length))
    };
    let stretches = if closed { n } else { n - 1 };
    for i in 0..stretches {
        let (from, to) = (at(i), at(i + 1));
        let third = from.distance(to) / 3.0;
        match (way(i), way((i + 1) % n)) {
            (None, None) => out.push(Piece::Line(to)),
            (a, b) => {
                out.push(Piece::Curve(from + a.unwrap_or(Vec2::ZERO) * third, to - b.unwrap_or(Vec2::ZERO) * third, to))
            }
        }
    }
    if closed {
        out.push(Piece::Close);
    }
    out
}

/// An outline's path.
fn outline_pieces(outline: &Outline) -> Vec<Piece> {
    match outline {
        Outline::Line { points, .. } => pieces(points, false, true),
        Outline::Area { loops, smooth, .. } => loops.iter().flat_map(|l| pieces(l, true, *smooth)).collect(),
    }
}

/// A number as short as it can be written to a hundredth.
fn number(v: f32) -> String {
    let v = if v.is_finite() { v } else { 0.0 };
    let text = format!("{v:.2}");
    let text = text.trim_end_matches('0').trim_end_matches('.');
    if text == "-0" || text.is_empty() { "0".to_owned() } else { text.to_owned() }
}

fn hex(colour: [f32; 3]) -> String {
    let [r, g, b] = colour.map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// The picture as an SVG file's text.
pub fn svg(picture: &Picture) -> String {
    let (w, h) = (picture.width, picture.height);
    let mut defs = String::new();
    let mut body = String::new();
    let mut masks = 0usize;
    if let Some(colour) = picture.background {
        let _ = writeln!(body, "<rect id=\"{BACKGROUND}\" width=\"{w}\" height=\"{h}\" fill=\"{}\"/>", hex(colour));
    }
    for (i, group) in picture.layers.iter().enumerate() {
        svg_group(group, Some(i), (w, h), &mut defs, &mut body, &mut masks);
    }
    let mut out = String::new();
    let _ = writeln!(out, "<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\">"
    );
    if !defs.is_empty() {
        let _ = writeln!(out, "<defs>\n{defs}</defs>");
    }
    out.push_str(&body);
    out.push_str("</svg>\n");
    out
}

fn svg_path(outline: &Outline) -> String {
    let mut d = String::new();
    let p = |v: Vec2| format!("{} {}", number(v.x), number(v.y));
    for piece in outline_pieces(outline) {
        match piece {
            Piece::Move(to) => {
                let _ = write!(d, "M{}", p(to));
            }
            Piece::Line(to) => {
                let _ = write!(d, "L{}", p(to));
            }
            Piece::Curve(a, b, to) => {
                let _ = write!(d, "C{} {} {}", p(a), p(b), p(to));
            }
            Piece::Close => d.push('Z'),
        }
    }
    d
}

fn svg_shape(shape: &Shape, colour: Option<[f32; 3]>, out: &mut String) {
    let colour = hex(colour.unwrap_or(shape.colour));
    let d = svg_path(&shape.outline);
    let faint = |what: &str| {
        if shape.opacity < 0.999 { format!(" {what}-opacity=\"{}\"", number(shape.opacity)) } else { String::new() }
    };
    match &shape.outline {
        Outline::Line { width, .. } => {
            let _ = writeln!(
                out,
                "<path d=\"{d}\" fill=\"none\" stroke=\"{colour}\" stroke-width=\"{}\" stroke-linecap=\"round\" stroke-linejoin=\"round\"{}/>",
                number(*width),
                faint("stroke")
            );
        }
        Outline::Area { even_odd, .. } => {
            let rule = if *even_odd { " fill-rule=\"evenodd\"" } else { "" };
            let _ = writeln!(out, "<path d=\"{d}\" fill=\"{colour}\"{rule}{}/>", faint("fill"));
        }
    }
}

fn svg_group(
    group: &Group,
    layer: Option<usize>,
    size: (u32, u32),
    defs: &mut String,
    out: &mut String,
    masks: &mut usize,
) {
    let mut open = String::from("<g");
    if let Some(i) = layer {
        let _ = write!(open, " id=\"layer{}\"", i + 1);
    }
    if group.opacity < 0.999 {
        let _ = write!(open, " opacity=\"{}\"", number(group.opacity));
    }
    if !group.taken.is_empty() {
        *masks += 1;
        let id = *masks;
        let (w, h) = size;
        let _ = writeln!(
            defs,
            "<mask id=\"taken{id}\" maskUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" width=\"{w}\" height=\"{h}\">"
        );
        let _ = writeln!(defs, "<rect width=\"{w}\" height=\"{h}\" fill=\"#ffffff\"/>");
        for shape in &group.taken {
            svg_shape(shape, Some([0.0; 3]), defs);
        }
        defs.push_str("</mask>\n");
        let _ = write!(open, " mask=\"url(#taken{id})\"");
    }
    out.push_str(&open);
    out.push_str(">\n");
    if let Some(name) = group.name.as_deref().filter(|name| !name.is_empty()) {
        let _ = writeln!(out, "<title>{}</title>", escaped(name));
    }
    for node in &group.nodes {
        match node {
            Node::Shape(shape) => svg_shape(shape, None, out),
            Node::Group(inner) => svg_group(inner, None, size, defs, out, masks),
        }
    }
    out.push_str("</g>\n");
}

/// A PDF being made: its objects by number (1 the catalog, 2 the pages, 3 the page, 4 what the
/// page and every group draw with, 5 the page's drawing), and what 4 lists.
struct Pdf {
    objects: Vec<Vec<u8>>,
    states: Vec<String>,
    forms: Vec<usize>,
    /// Points a canvas pixel, and the page's size in points. Everything is written in the page's
    /// own measure (from its bottom left, up), not laid over it by a matrix: a mask is measured by
    /// whatever matrix holds when it is set, and not every reader agrees what that is.
    scale: f32,
    page: (f32, f32),
}

impl Pdf {
    /// A dictionary and its bytes as a compressed stream object.
    fn stream(dict: &str, bytes: &[u8]) -> Vec<u8> {
        let packed = miniz_oxide::deflate::compress_to_vec_zlib(bytes, 6);
        let mut out = format!("<< {dict} /Filter /FlateDecode /Length {} >>\nstream\n", packed.len()).into_bytes();
        out.extend_from_slice(&packed);
        out.extend_from_slice(b"\nendstream");
        out
    }

    /// A graphics state, named by where it is in the list: the one already there if it is.
    fn state(&mut self, state: String) -> usize {
        match self.states.iter().position(|s| *s == state) {
            Some(at) => at,
            None => {
                self.states.push(state);
                self.states.len() - 1
            }
        }
    }

    /// Drawing kept as a group of its own, blended as one in `colours`; its number among the
    /// groups.
    fn form(&mut self, drawing: &str, colours: &str) -> usize {
        let (w, h) = self.page;
        let dict = format!(
            "/Type /XObject /Subtype /Form /BBox [0 0 {w:.2} {h:.2}] /Group << /S /Transparency /CS {colours} /I true >> /Resources 4 0 R"
        );
        self.objects.push(Pdf::stream(&dict, drawing.as_bytes()));
        self.forms.push(self.objects.len());
        self.forms.len() - 1
    }

    fn path(&self, outline: &Outline, out: &mut String) {
        let p = |v: Vec2| format!("{} {}", number(v.x * self.scale), number(self.page.1 - v.y * self.scale));
        for piece in outline_pieces(outline) {
            let _ = match piece {
                Piece::Move(to) => writeln!(out, "{} m", p(to)),
                Piece::Line(to) => writeln!(out, "{} l", p(to)),
                Piece::Curve(a, b, to) => writeln!(out, "{} {} {} c", p(a), p(b), p(to)),
                Piece::Close => writeln!(out, "h"),
            };
        }
    }

    fn shape(&mut self, shape: &Shape, colour: Option<[f32; 3]>, out: &mut String) {
        let [r, g, b] = colour.unwrap_or(shape.colour).map(|c| number(c.clamp(0.0, 1.0)));
        out.push_str("q\n");
        if shape.opacity < 0.999 {
            let a = number(shape.opacity);
            let state = self.state(format!("<< /Type /ExtGState /ca {a} /CA {a} >>"));
            let _ = writeln!(out, "/GS{state} gs");
        }
        match &shape.outline {
            Outline::Line { width, .. } => {
                let _ = writeln!(out, "{r} {g} {b} RG\n{} w\n1 J\n1 j", number(*width * self.scale));
                self.path(&shape.outline, out);
                out.push_str("S\n");
            }
            Outline::Area { even_odd, .. } => {
                let _ = writeln!(out, "{r} {g} {b} rg");
                self.path(&shape.outline, out);
                out.push_str(if *even_odd { "f*\n" } else { "f\n" });
            }
        }
        out.push_str("Q\n");
    }

    fn group(&mut self, group: &Group, out: &mut String) {
        let mut drawing = String::new();
        for node in &group.nodes {
            match node {
                Node::Shape(shape) => self.shape(shape, None, &mut drawing),
                Node::Group(inner) => self.group(inner, &mut drawing),
            }
        }
        // drawn as it is unless it is faint or has something taken from it: then as one, through
        // a mask that is white but for what was taken
        if group.taken.is_empty() && group.opacity >= 0.999 {
            out.push_str(&drawing);
            return;
        }
        let form = self.form(&drawing, "/DeviceRGB");
        let mask = (!group.taken.is_empty()).then(|| {
            let (w, h) = self.page;
            let mut left = format!("1 g\n0 0 {w:.2} {h:.2} re\nf\n");
            for shape in &group.taken {
                self.shape(shape, Some([0.0; 3]), &mut left);
            }
            let mask = self.form(&left, "/DeviceGray");
            self.forms.get(mask).copied().unwrap_or(0)
        });
        let a = number(group.opacity.clamp(0.0, 1.0));
        let masked = match mask {
            Some(object) => format!(" /SMask << /Type /Mask /S /Luminosity /G {object} 0 R >>"),
            None => String::new(),
        };
        let state = self.state(format!("<< /Type /ExtGState /ca {a} /CA {a}{masked} >>"));
        let _ = writeln!(out, "q\n/GS{state} gs\n/Fm{form} Do\nQ");
    }
}

/// The picture as a one-page PDF, the page the canvas's size at its pixels to the inch.
pub fn pdf(picture: &Picture) -> Vec<u8> {
    let (w, h) = (picture.width, picture.height);
    let scale = 72.0 / picture.dpi.clamp(1.0, 9999.0);
    let (pw, ph) = (w as f32 * scale, h as f32 * scale);
    let mut made = Pdf { objects: vec![Vec::new(); 5], states: Vec::new(), forms: Vec::new(), scale, page: (pw, ph) };
    let mut drawing = String::new();
    if let Some(colour) = picture.background {
        let [r, g, b] = colour.map(|c| number(c.clamp(0.0, 1.0)));
        let _ = writeln!(drawing, "{r} {g} {b} rg\n0 0 {pw:.2} {ph:.2} re\nf");
    }
    for group in &picture.layers {
        made.group(group, &mut drawing);
    }
    let states: String = made.states.iter().enumerate().map(|(i, s)| format!("/GS{i} {s} ")).collect();
    let forms: String = made.forms.iter().enumerate().map(|(i, object)| format!("/Fm{i} {object} 0 R ")).collect();
    let fixed = [
        b"<< /Type /Catalog /Pages 2 0 R >>".to_vec(),
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".to_vec(),
        format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {pw:.2} {ph:.2}] /Contents 5 0 R /Resources 4 0 R /Group << /S /Transparency /CS /DeviceRGB >> >>"
        )
        .into_bytes(),
        format!("<< /ExtGState << {states}>> /XObject << {forms}>> >>").into_bytes(),
        Pdf::stream("", drawing.as_bytes()),
    ];
    for (slot, object) in made.objects.iter_mut().zip(fixed) {
        *slot = object;
    }
    let mut out = b"%PDF-1.4\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(made.objects.len());
    for (i, body) in made.objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", i + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", made.objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", made.objects.len() + 1).as_bytes(),
    );
    out
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::brush::{Brush, Taper};
    use crate::input::{Pointer, Sample};
    use crate::vector::{Ink, Setup, StrokeId, VectorFill};

    fn ink(erases: bool) -> Ink {
        Ink { colour: [1.0, 0.0, 0.0], secondary: [0.0; 3], opacity: 1.0, erases, preserve_alpha: false, wetness: 0.0 }
    }

    /// A stroke across at row `y`, pressed as `pressure` says along it: by a pen of one width
    /// (`plain`), or one that thins at a stroke's ends and with the pen's pressure.
    fn across(content: &mut VectorContent, y: f32, erases: bool, plain: bool, pressure: impl Fn(f32) -> f32) {
        let none = Taper { start: 0.0, end: 0.0, size: 0.0, opacity: 0.0 };
        let brush = Brush { streamline: 0.0, hardness: 1.0, ..Brush::round_pen() };
        let brush = if plain {
            Brush { pressure_size: 0.0, pen_taper: none.clone(), touch_taper: none, ..brush }
        } else {
            brush
        };
        let mut stroke =
            VectorStroke::begin(content.next_id(), Setup::new(Arc::new(brush), 12.0, Pointer::Stylus, 7), ink(erases));
        let samples: Vec<Sample> = (0..=40)
            .map(|i| {
                let t = i as f32 / 40.0;
                Sample::at(Vec2::new(50.0 + 200.0 * t, y), pressure(t), f64::from(i) * 8.0)
            })
            .collect();
        stroke.record(&samples);
        content.push(stroke);
    }

    fn square(content: &mut VectorContent, mode: FillMode) -> StrokeId {
        let id = content.next_id();
        content.push_fill(VectorFill {
            id,
            loops: vec![vec![
                Vec2::new(60.0, 60.0),
                Vec2::new(140.0, 60.0),
                Vec2::new(140.0, 140.0),
                Vec2::new(60.0, 140.0),
            ]],
            colour: [0.0, 0.0, 1.0],
            opacity: 0.5,
            mode,
            hard: false,
            traced: None,
            takes_from: None,
        });
        id
    }

    fn count(text: &str, what: &str) -> usize {
        text.matches(what).count()
    }

    #[test]
    fn a_layer_s_strokes_and_fills_go_out_as_shapes_with_what_was_erased_taken_from_them() {
        let mut content = VectorContent::default();
        // a line of one width, a fill laid under it, an eraser over both, then a stroke that
        // swells and thins, made after the eraser and so whole
        across(&mut content, 100.0, false, true, |_| 1.0);
        square(&mut content, FillMode::Behind);
        across(&mut content, 100.0, true, true, |_| 1.0);
        across(&mut content, 180.0, false, false, |t| 0.15 + 0.85 * (t * std::f32::consts::PI).sin());
        let group = layer(&content, "Lines & <more>", 0.8);
        // the erased two are a group of their own, the fill under the line; the last is beside it
        assert_eq!(group.nodes.len(), 2);
        let Node::Group(erased) = &group.nodes[0] else { panic!("{:?}", group.nodes[0]) };
        assert_eq!((erased.nodes.len(), erased.taken.len()), (2, 1));
        assert!(matches!(&erased.nodes[0], Node::Shape(Shape { outline: Outline::Area { even_odd: true, .. }, .. })));
        let Node::Shape(Shape { outline: Outline::Line { points, width }, .. }) = &erased.nodes[1] else {
            panic!("{:?}", erased.nodes[1])
        };
        assert!(points.len() <= 4, "a straight line needs few points: {points:?}");
        assert!((*width - 12.0).abs() < 0.5, "{width}");
        // the one that swells is an outline, as wide in the middle as the brush and thin at its ends
        let Node::Shape(Shape { outline: Outline::Area { loops, even_odd: false, smooth: true }, .. }) =
            &group.nodes[1]
        else {
            panic!("{:?}", group.nodes[1])
        };
        let tall = |near: f32| {
            let ys: Vec<f32> = loops[0].iter().filter(|p| (p.x - near).abs() < 12.0).map(|p| p.y).collect();
            ys.iter().copied().fold(f32::NEG_INFINITY, f32::max) - ys.iter().copied().fold(f32::INFINITY, f32::min)
        };
        assert!(tall(150.0) > 9.0 && tall(150.0) < 13.0, "{}", tall(150.0));
        assert!(tall(60.0) < tall(150.0) * 0.7, "{} {}", tall(60.0), tall(150.0));
        let picture = Picture { width: 300, height: 300, dpi: 150.0, background: Some([1.0; 3]), layers: vec![group] };
        // as SVG: every tag closed, the layer named, its mask used
        let text = svg(&picture);
        assert!(text.starts_with("<?xml") && text.trim_end().ends_with("</svg>"));
        assert_eq!(count(&text, "<g"), count(&text, "</g>"));
        assert_eq!((count(&text, "<mask"), count(&text, "</mask>"), count(&text, "mask=\"url(#taken1)\"")), (1, 1, 1));
        assert!(text.contains("<title>Lines &amp; &lt;more&gt;</title>") && text.contains("opacity=\"0.8\""));
        assert!(text.contains("stroke-linecap=\"round\"") && text.contains("fill-rule=\"evenodd\""));
        assert!(text.contains("fill=\"#0000ff\"") && text.contains("fill-opacity=\"0.5\""));
        assert!(text.contains("<rect id=\"background\" width=\"300\" height=\"300\" fill=\"#ffffff\"/>"));
        // as PDF: every object where the table says it is, the mask and the faint layer there
        let file = pdf(&picture);
        assert!(file.starts_with(b"%PDF-1.4"));
        let text = String::from_utf8_lossy(&file);
        assert!(text.contains("/SMask") && text.contains("/ca 0.8") && text.contains("/MediaBox [0 0 144.00 144.00]"));
        let table = file.windows(6).rposition(|at| at == b"\nxref\n").unwrap();
        let offsets: Vec<usize> = String::from_utf8_lossy(&file[table..])
            .lines()
            .filter(|line| line.ends_with(" n "))
            .map(|line| line[..10].parse().unwrap())
            .collect();
        assert!(offsets.len() >= 7, "{offsets:?}");
        for (i, offset) in offsets.iter().enumerate() {
            assert!(file[*offset..].starts_with(format!("{} 0 obj\n", i + 1).as_bytes()), "object {}", i + 1);
        }
    }

    #[test]
    fn a_path_curves_through_its_points_but_for_corners() {
        // round a square: every point a corner, so straight lines
        let corners = [Vec2::new(0.0, 0.0), Vec2::new(10.0, 0.0), Vec2::new(10.0, 10.0), Vec2::new(0.0, 10.0)];
        let made = pieces(&corners, true, true);
        assert_eq!(made.len(), 6);
        assert!(made[1..5].iter().all(|p| matches!(p, Piece::Line(_))), "{made:?}");
        // round a circle: all curves, and no further out than the circle
        let made = pieces(&circle(Vec2::new(50.0, 50.0), 20.0), true, true);
        assert!(made[1..13].iter().all(|p| matches!(p, Piece::Curve(..))), "{made:?}");
        for piece in &made {
            if let Piece::Curve(a, b, _) = piece {
                assert!(a.distance(Vec2::new(50.0, 50.0)) < 21.5 && b.distance(Vec2::new(50.0, 50.0)) < 21.5);
            }
        }
        assert_eq!(number(12.0), "12");
        assert_eq!(number(0.504), "0.5");
        assert_eq!(number(-0.001), "0");
    }
}

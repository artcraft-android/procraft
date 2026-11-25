//! Comic Panels: a tool that makes the canvas a comic page and cuts it into panels. Opened on a
//! canvas with none, it makes one panel inside a margin; a line dragged across panels cuts each in
//! two, straight across or down when near it, with a gutter between; with Move chosen, a gutter
//! dragged moves the panels' edges on both sides of it (the page's edge alone, its margin). Each
//! panel is a group in the layers: its frame (the shape its art is clipped to, drawn white with
//! its border the frame's outline) and its art above it. Drawing the frames, and clipping the art
//! to them, is the app's, so a page shows as it was made with this plugin off; the shapes, and
//! where the cuts and gutters go, are here. `tools/pack_plugin.py` makes a `.procraft-plugin` file
//! of it.
//!
//! The interface is written down at the top of `crates/core/src/plugin_tools.rs`.

mod shapes;

use std::sync::Mutex;

use kit::layer::{Record, fields};

use crate::shapes::{Gutter, PageMeasures, Point};

/// Where records come in and go out.
static IN: kit::Place = kit::Place::new();
static OUT: kit::Place = kit::Place::new();

/// The touch under way.
static TOUCH: Mutex<Option<Touch>> = Mutex::new(None);

/// The tools, by their place in the manifest, and the panels tool's modes.
const PANELS: i32 = 0;
const CUT: u32 = 0;

/// How far from a gutter, points on the screen, a touch takes it.
const GUTTER_REACH: f32 = 18.0;

/// What the app tells, and what is answered (see `plugin_tools.rs`).
mod told {
    pub const WHAT: u16 = 1;
    pub const AT: u16 = 2;
    pub const MODE: u16 = 3;
    pub const PER_POINT: u16 = 4;
    pub const SIZE: u16 = 5;
    pub const ANIMATED: u16 = 6;
    pub const FRAME: u16 = 7;
    pub const FRAME_LAYER: u16 = 1;
    pub const FRAME_POINTS: u16 = 2;
    pub const OPENED: u32 = 0;
    pub const DOWN: u32 = 2;
    pub const MOVED: u32 = 3;
    pub const UP: u32 = 4;
    pub const CANCELLED: u32 = 5;
}

mod answer {
    pub const SHOW: u16 = 1;
    pub const EDIT: u16 = 2;
    pub const KEEP: u16 = 3;
    pub const STEP: u16 = 4;
    pub const SAID: u16 = 5;
    pub const CLOSE: u16 = 6;
    pub const SHOW_POINTS: u16 = 1;
    pub const SHOW_CLOSED: u16 = 2;
    pub const SHOW_LOOK: u16 = 3;
    pub const EDIT_KIND: u16 = 1;
    pub const EDIT_LAYER: u16 = 2;
    pub const EDIT_POINTS: u16 = 3;
    pub const EDIT_NAME: u16 = 4;
    pub const EDIT_LIKE: u16 = 5;
    pub const EDIT_BORDER: u16 = 6;
    pub const EDIT_CURRENT: u16 = 7;
    pub const SHAPE: u32 = 0;
    pub const PANEL: u32 = 1;
    pub const SHOWN: u32 = 0;
    pub const KEPT: u32 = 1;
    pub const BACK: u32 = 2;
    pub const GUIDE: u32 = 0;
    pub const DRAWN: u32 = 1;
}

/// What the app tells of the canvas and the touch.
#[derive(Clone, Debug, Default)]
struct Told {
    what: u32,
    at: Point,
    mode: u32,
    per_point: f32,
    size: (u32, u32),
    animated: bool,
    frames: Vec<(u64, Vec<Point>)>,
}

/// A touch under way: a cut being drawn from a point, or a gutter taken at one, with the frames it
/// moves as they were then.
enum Touch {
    Cut(Point),
    Gutter { gutter: Gutter, start: Point, was: Vec<(u64, Vec<Point>)> },
    Nothing,
}

/// An edit to the layers.
enum Edit {
    /// A frame's shape.
    Shape(u64, Vec<Point>),
    /// A new panel of a shape above the panel of a frame (none: on top of the stack), its border as
    /// that frame's (or of a width), named, its art current or not.
    Panel { above: Option<u64>, shape: Vec<Point>, like: Option<u64>, border: f32, name: String, current: bool },
}

/// An answer: what to show, the edits, what becomes of them, and what to say.
#[derive(Default)]
struct Answer {
    shown: Vec<(Vec<Point>, bool, u32)>,
    edits: Vec<Edit>,
    keep: u32,
    step: Option<String>,
    said: Option<String>,
    close: bool,
}

fn u32_of(bytes: &[u8]) -> u32 {
    bytes.get(..4).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes)
}

fn u64_of(bytes: &[u8]) -> u64 {
    bytes.get(..8).and_then(|b| b.try_into().ok()).map_or(0, u64::from_le_bytes)
}

fn f32s_of(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]])).collect()
}

fn points_of(bytes: &[u8]) -> Vec<Point> {
    f32s_of(bytes).chunks_exact(2).map(|p| [p[0], p[1]]).collect()
}

fn points(shape: &[Point]) -> Vec<u8> {
    shape.iter().flatten().flat_map(|v| v.to_le_bytes()).collect()
}

fn told(bytes: &[u8]) -> Told {
    let mut t = Told { per_point: 1.0, ..Told::default() };
    for (id, value) in fields(bytes) {
        match id {
            told::WHAT => t.what = u32_of(value),
            told::AT => {
                if let [x, y] = f32s_of(value)[..] {
                    t.at = [x, y];
                }
            }
            told::MODE => t.mode = u32_of(value),
            told::PER_POINT => t.per_point = f32s_of(value).first().copied().unwrap_or(1.0),
            told::SIZE => t.size = (u32_of(value), u32_of(value.get(4..).unwrap_or_default())),
            told::ANIMATED => t.animated = value.first().is_some_and(|b| *b != 0),
            told::FRAME => {
                let (mut layer, mut shape) = (0, Vec::new());
                for (id, v) in fields(value) {
                    match id {
                        told::FRAME_LAYER => layer = u64_of(v),
                        told::FRAME_POINTS => shape = points_of(v),
                        _ => {}
                    }
                }
                t.frames.push((layer, shape));
            }
            _ => {}
        }
    }
    t
}

fn record(a: Answer) -> Vec<u8> {
    let mut out = Record::default();
    for (shape, closed, look) in &a.shown {
        let mut r = Record::default();
        r.field(answer::SHOW_POINTS, &points(shape))
            .field(answer::SHOW_CLOSED, &[u8::from(*closed)])
            .field(answer::SHOW_LOOK, &look.to_le_bytes());
        out.field(answer::SHOW, &r.bytes());
    }
    for edit in &a.edits {
        let mut r = Record::default();
        match edit {
            Edit::Shape(frame, shape) => {
                r.field(answer::EDIT_KIND, &answer::SHAPE.to_le_bytes())
                    .field(answer::EDIT_LAYER, &frame.to_le_bytes())
                    .field(answer::EDIT_POINTS, &points(shape));
            }
            Edit::Panel { above, shape, like, border, name, current } => {
                r.field(answer::EDIT_KIND, &answer::PANEL.to_le_bytes())
                    .field(answer::EDIT_LAYER, &above.unwrap_or(0).to_le_bytes())
                    .field(answer::EDIT_POINTS, &points(shape))
                    .field(answer::EDIT_NAME, name.as_bytes())
                    .field(answer::EDIT_BORDER, &border.to_le_bytes())
                    .field(answer::EDIT_CURRENT, &[u8::from(*current)]);
                if let Some(like) = like {
                    r.field(answer::EDIT_LIKE, &like.to_le_bytes());
                }
            }
        }
        out.field(answer::EDIT, &r.bytes());
    }
    out.field(answer::KEEP, &a.keep.to_le_bytes());
    if let Some(step) = &a.step {
        out.field(answer::STEP, step.as_bytes());
    }
    if let Some(said) = &a.said {
        out.field(answer::SAID, said.as_bytes());
    }
    if a.close {
        out.field(answer::CLOSE, &[1]);
    }
    out.bytes()
}

/// The panels outlined, as `edits` leave them.
fn outlines(frames: &[(u64, Vec<Point>)], edits: &[Edit]) -> Vec<(Vec<Point>, bool, u32)> {
    let mut shapes: Vec<Vec<Point>> = frames
        .iter()
        .map(|(id, shape)| {
            let moved = edits.iter().rev().find_map(|e| match e {
                Edit::Shape(f, s) if f == id => Some(s.clone()),
                _ => None,
            });
            moved.unwrap_or_else(|| shape.clone())
        })
        .collect();
    shapes.extend(edits.iter().filter_map(|e| match e {
        Edit::Panel { shape, .. } => Some(shape.clone()),
        Edit::Shape(..) => None,
    }));
    shapes.into_iter().map(|s| (s, true, answer::GUIDE)).collect()
}

/// What the panels tool answers to `t`, with `touch` the touch under way.
fn answer(t: &Told, touch: &mut Option<Touch>) -> Answer {
    let (w, h) = t.size;
    let mut a = Answer::default();
    let touch_was = match touch.as_ref() {
        Some(Touch::Gutter { was, .. }) => Some(was.clone()),
        _ => None,
    };
    match t.what {
        told::OPENED if t.frames.is_empty() => {
            // the canvas made a page: one panel inside its margin, on top of the stack
            if t.animated || w == 0 || h == 0 {
                a.said = Some("Comic panels can't be added to this canvas.".into());
                a.close = true;
                return a;
            }
            let m = PageMeasures::for_canvas(w, h);
            let shape = shapes::page_shape(w, h, m.margin);
            a.edits.push(Edit::Panel {
                above: None,
                shape,
                like: None,
                border: m.border,
                name: "Panel 1".into(),
                current: true,
            });
            a.keep = answer::KEPT;
            a.step = Some("comic page".into());
        }
        told::DOWN => {
            *touch = Some(if t.mode == CUT {
                Touch::Cut(t.at)
            } else {
                let most_gap = w.max(h) as f32 / 4.0;
                let reach = GUTTER_REACH * t.per_point.max(1e-6);
                match shapes::gutter_at(&t.frames, t.at, reach, most_gap) {
                    Some(gutter) => {
                        let was: Vec<(u64, Vec<Point>)> = t
                            .frames
                            .iter()
                            .filter(|(id, _)| gutter.edges.iter().any(|e| e.0 == *id))
                            .cloned()
                            .collect();
                        Touch::Gutter { gutter, start: t.at, was }
                    }
                    None => Touch::Nothing,
                }
            });
        }
        told::MOVED | told::UP => match touch.as_ref() {
            Some(Touch::Cut(from)) => {
                let (a0, b0) = shapes::snapped(*from, t.at);
                if t.what == told::MOVED {
                    a.shown.push((vec![a0, b0], false, answer::DRAWN));
                } else {
                    cut(t, *from, &mut a);
                }
            }
            Some(Touch::Gutter { gutter, start, was }) => {
                let n = gutter.normal;
                let by = (t.at[0] - start[0]) * n[0] + (t.at[1] - start[1]) * n[1];
                let (_, moved) = gutter.moved(was, by, w, h);
                let changed = moved.iter().any(|(id, s)| was.iter().any(|(f, before)| f == id && before != s));
                a.edits.extend(moved.into_iter().map(|(id, s)| Edit::Shape(id, s)));
                a.keep = if t.what == told::MOVED {
                    answer::SHOWN
                } else if changed {
                    a.step = Some("move gutter".into());
                    answer::KEPT
                } else {
                    answer::BACK
                };
            }
            _ => {}
        },
        told::CANCELLED => {
            if matches!(touch, Some(Touch::Gutter { .. })) {
                a.keep = answer::BACK;
            }
            *touch = None;
        }
        _ => {}
    }
    if matches!(t.what, told::UP | told::CANCELLED) {
        *touch = None;
    }
    // as the edits leave them, or (going back) as they were before the touch
    let mut shown = match touch_was {
        Some(was) if a.keep == answer::BACK => {
            outlines(&t.frames, &was.into_iter().map(|(id, s)| Edit::Shape(id, s)).collect::<Vec<_>>())
        }
        _ => outlines(&t.frames, &a.edits),
    };
    shown.append(&mut a.shown);
    a.shown = shown;
    a
}

/// Cuts every panel the line from `from` to where the touch is crosses in two, as `a`'s edits:
/// each keeps its larger half and its art, a new panel the other just above it.
fn cut(t: &Told, from: Point, a: &mut Answer) {
    let (w, h) = t.size;
    let m = PageMeasures::for_canvas(w, h);
    let most = w.max(h) as f32 / 4.0;
    let (p, q) = shapes::snapped(from, t.at);
    let gap = shapes::gap_for(p, q, m.column_gap.clamp(0.0, most), m.row_gap.clamp(0.0, most));
    let mut count = t.frames.len();
    for (id, shape) in t.frames.iter().filter(|(_, s)| shapes::crosses(s, p, q)) {
        let Some((keep, other)) = shapes::cut(shape, p, q, gap) else { continue };
        count += 1;
        a.edits.push(Edit::Shape(*id, keep));
        let name = format!("Panel {count}");
        a.edits.push(Edit::Panel {
            above: Some(*id),
            shape: other,
            like: Some(*id),
            border: 1.0,
            name,
            current: false,
        });
    }
    if !a.edits.is_empty() {
        a.keep = answer::KEPT;
        a.step = Some("cut panels".into());
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_tool_in(len: i32) -> i32 {
    IN.make(len)
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_tool_out() -> i32 {
    OUT.with(|bytes| bytes.as_mut_ptr() as usize as i32)
}

#[unsafe(no_mangle)]
pub extern "C" fn procraft_tool_touch(tool: i32, len: i32) -> i32 {
    let bytes = IN.take();
    if tool != PANELS {
        return -1;
    }
    let t = told(bytes.get(..len.max(0) as usize).unwrap_or_default());
    let mut touch = TOUCH.lock().unwrap_or_else(|e| e.into_inner());
    let out = record(answer(&t, &mut touch));
    let len = out.len() as i32;
    OUT.put(out);
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    fn page(w: u32, h: u32) -> Told {
        let m = PageMeasures::for_canvas(w, h);
        Told { size: (w, h), per_point: 1.0, frames: vec![(7, shapes::page_shape(w, h, m.margin))], ..Told::default() }
    }

    fn at(t: &Told, what: u32, mode: u32, x: f32, y: f32) -> Told {
        Told { what, mode, at: [x, y], ..t.clone() }
    }

    #[test]
    fn opened_on_a_canvas_without_panels_it_makes_a_page_once() {
        let mut touch = None;
        let blank = Told { size: (1000, 1500), ..Told::default() };
        let a = answer(&blank, &mut touch);
        assert_eq!((a.keep, a.step.as_deref()), (answer::KEPT, Some("comic page")));
        let [Edit::Panel { above: None, shape, current: true, .. }] = &a.edits[..] else { panic!() };
        assert_eq!(shape[0], [70.0, 70.0]);
        assert_eq!(a.shown.len(), 1);
        // a page already: nothing made
        assert!(answer(&page(1000, 1500), &mut touch).edits.is_empty());
        // an animation: none made, and it says so
        let a = answer(&Told { animated: true, ..blank }, &mut touch);
        assert!(a.edits.is_empty() && a.close && a.said.is_some());
    }

    #[test]
    fn a_line_dragged_across_shows_straightened_and_cuts_as_it_is_let_go() {
        let mut touch = None;
        let t = page(1000, 1000);
        answer(&at(&t, told::DOWN, CUT, 0.0, 400.0), &mut touch);
        let moved = answer(&at(&t, told::MOVED, CUT, 1000.0, 401.0), &mut touch);
        assert!(moved.edits.is_empty());
        let (line, closed, look) = moved.shown.last().unwrap();
        assert!(!closed && *look == answer::DRAWN && (line[0][1] - line[1][1]).abs() < 1e-3);
        let up = answer(&at(&t, told::UP, CUT, 1000.0, 401.0), &mut touch);
        assert_eq!((up.keep, up.step.as_deref()), (answer::KEPT, Some("cut panels")));
        let [Edit::Shape(7, keep), Edit::Panel { above: Some(7), like: Some(7), shape: made, name, .. }] =
            &up.edits[..]
        else {
            panic!()
        };
        assert_eq!(name, "Panel 2");
        let ys = |s: &[Point]| s.iter().map(|p| p[1]).fold((f32::MAX, f32::MIN), |(lo, hi), y| (lo.min(y), hi.max(y)));
        // the line made straight about its middle (400.5), the row gap split round it
        assert_eq!((ys(keep), ys(made)), ((418.0, 930.0), (70.0, 383.0)));
        // a line through no panel cuts nothing
        answer(&at(&t, told::DOWN, CUT, 1.0, 1.0), &mut touch);
        assert!(answer(&at(&t, told::UP, CUT, 2.0, 900.0), &mut touch).edits.is_empty());
    }

    #[test]
    fn a_gutter_dragged_moves_live_keeps_as_it_is_let_go_and_goes_back_on_a_pinch() {
        let mut touch = None;
        let mut t = page(300, 300);
        let (l, r) = shapes::cut(&t.frames[0].1, [150.0, 0.0], [150.0, 300.0], 6.0).unwrap();
        t.frames = vec![(1, l), (2, r)];
        // nothing to take in the middle of a panel
        answer(&at(&t, told::DOWN, 1, 80.0, 150.0), &mut touch);
        let a = answer(&at(&t, told::MOVED, 1, 120.0, 150.0), &mut touch);
        assert!(a.edits.is_empty());
        answer(&at(&t, told::UP, 1, 120.0, 150.0), &mut touch);
        answer(&at(&t, told::DOWN, 1, 150.0, 150.0), &mut touch);
        let a = answer(&at(&t, told::MOVED, 1, 200.0, 150.0), &mut touch);
        assert_eq!((a.edits.len(), a.keep), (2, answer::SHOWN));
        let a = answer(&at(&t, told::UP, 1, 200.0, 150.0), &mut touch);
        assert_eq!((a.keep, a.step.as_deref()), (answer::KEPT, Some("move gutter")));
        answer(&at(&t, told::DOWN, 1, 150.0, 150.0), &mut touch);
        answer(&at(&t, told::MOVED, 1, 200.0, 150.0), &mut touch);
        assert_eq!(answer(&at(&t, told::CANCELLED, 1, 200.0, 150.0), &mut touch).keep, answer::BACK);
    }

    #[test]
    fn what_it_is_told_and_answers_goes_through_records() {
        let t = page(400, 300);
        let mut r = Record::default();
        let mut frame = Record::default();
        frame.field(told::FRAME_LAYER, &7_u64.to_le_bytes()).field(told::FRAME_POINTS, &points(&t.frames[0].1));
        r.field(told::WHAT, &told::DOWN.to_le_bytes())
            .field(told::AT, &points(&[[5.0, 6.0]]))
            .field(told::SIZE, &[400_u32.to_le_bytes(), 300_u32.to_le_bytes()].concat())
            .field(told::PER_POINT, &2.0_f32.to_le_bytes())
            .field(told::FRAME, &frame.bytes());
        let back = told(&r.bytes());
        assert_eq!((back.what, back.at, back.size, back.per_point), (told::DOWN, [5.0, 6.0], (400, 300), 2.0));
        assert_eq!(back.frames, t.frames);
        let out = record(Answer { keep: answer::KEPT, step: Some("x".into()), ..Answer::default() });
        assert_eq!(fields(&out).count(), 2);
    }
}

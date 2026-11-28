//! What the log says of what is being painted, so that a log someone shares explains a lag or a
//! crash without asking them: a canvas's size and dots per inch as it is made, the brush each time
//! another is chosen, and beside a slow frame's time the brush in use (its name, its size on the
//! canvas, whether it is dual and whether it mixes wet paint), the canvas's size, the zoom, the
//! layers visible and the stamps new to the frame.
//!
//! A frame notes these as it is drawn, while it holds the canvas anyway, under a lock of their own
//! held only to copy them, and that neither side waits for: a slow frame's line reads them without
//! the canvas's lock (the desktop's after its frame has let go of it), and never waits.

use std::sync::Arc;
use std::time::{Duration, Instant};

use procraft_engine::document::{Document, LayerId, LayerKind};

use crate::{CanvasSession, State, ToolKind};

/// The brush and the canvas, unchanged, are told in full again after this long; sooner, a slow
/// frame's line says they are as before, so a run of slow frames doesn't fill the log with them.
const TOLD_AGAIN_AFTER: Duration = Duration::from_secs(10);

/// The line telling an artwork's canvas made in `ms` milliseconds: `size` pixels at `dpi` dots per
/// inch, `layers` layers, `parked` of them kept compressed, and `copies_kb` KB of compressed copies
/// kept of the rest.
pub(crate) fn made_line(ms: u128, size: (u32, u32), dpi: f32, layers: usize, parked: usize, copies_kb: u64) -> String {
    let (w, h) = size;
    let dpi = (dpi * 100.0).round() / 100.0;
    format!(
        "canvas made in {ms} ms: {w}×{h} px, {dpi} dpi, {layers} layers, {parked} kept compressed, \
         {copies_kb} KB of copies of the rest"
    )
}

/// The line telling the brush `brush` chosen.
pub(crate) fn chosen_line(brush: &BrushNote) -> String {
    format!("brush chosen: {brush}")
}

/// The brush in use, as the log tells it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct BrushNote {
    pub(crate) tool: ToolKind,
    /// Its own name: the stock brush's, or the one it was given.
    pub(crate) name: Arc<str>,
    /// Across, canvas pixels: at its size slider, scaled as it paints.
    pub(crate) size: f32,
    /// Two brushes along one path.
    pub(crate) dual: bool,
    /// It mixes its paint wet with what is under it.
    pub(crate) wet: bool,
}

impl BrushNote {
    /// The current tool's brush in `st`, named by `kept` if that is its name already (so a frame
    /// makes nothing new while the brush stays).
    pub(crate) fn of(st: &State, kept: Option<&Arc<str>>) -> BrushNote {
        let brush = st.brush(st.tool);
        let name = match kept {
            Some(kept) if **kept == *brush.name => Arc::clone(kept),
            _ => Arc::from(brush.name.as_str()),
        };
        BrushNote {
            tool: st.tool,
            name,
            size: brush.diameter(st.shown_sliders().size) * st.brush_scale(),
            dual: brush.secondary.is_some(),
            wet: brush.mixes_wet(),
        }
    }
}

impl std::fmt::Display for BrushNote {
    /// `"6B Pencil" (paint) 24 px, dual, wet`: the name quoted as Rust quotes it, so that no name
    /// can break the line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let tool = match self.tool {
            ToolKind::Paint => "paint",
            ToolKind::Erase => "erase",
            ToolKind::Smudge => "smudge",
        };
        write!(f, "{:?} ({tool}) {} px", &*self.name, short(self.size))?;
        if self.dual {
            f.write_str(", dual")?;
        }
        if self.wet {
            f.write_str(", wet")?;
        }
        Ok(())
    }
}

/// What a frame was drawn with.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Painting {
    pub(crate) brush: BrushNote,
    /// The canvas, pixels.
    pub(crate) canvas: (u32, u32),
    /// Screen pixels to a canvas pixel.
    pub(crate) zoom: f32,
    /// The layers holding pixels that show (see [`layers_shown`]), and all that hold pixels.
    pub(crate) layers: (usize, usize),
}

impl Painting {
    /// What `st` draws with (the brush named by `kept` if that is its name already).
    fn of(st: &State, kept: Option<&Arc<str>>) -> Painting {
        Painting {
            brush: BrushNote::of(st, kept),
            canvas: (st.doc.width(), st.doc.height()),
            zoom: st.view.map_or(1.0, |v| v.zoom()),
            layers: layers_shown(&st.doc),
        }
    }
}

impl std::fmt::Display for Painting {
    /// `brush "6B Pencil" (paint) 24 px, dual, wet; canvas 4096×4096, zoom 37%, 12 of 14 layers
    /// visible`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (w, h) = self.canvas;
        let (shown, all) = self.layers;
        write!(
            f,
            "brush {}; canvas {w}×{h}, zoom {}%, {shown} of {all} layers visible",
            self.brush,
            short(self.zoom * 100.0)
        )
    }
}

/// `x` for the log: to a tenth under 10, whole from there.
fn short(x: f32) -> String {
    if x.abs() < 10.0 { format!("{:.1}", x) } else { format!("{:.0}", x) }
}

/// How many of `doc`'s layers holding pixels show (their own eye on, and every group they are in
/// shown; groups and masks aren't counted), and how many there are. One pass, from the top: a
/// group is above the layers in it, so whether it shows is known before they are met.
pub(crate) fn layers_shown(doc: &Document) -> (usize, usize) {
    let mut groups: Vec<(LayerId, bool)> = Vec::new();
    let (mut shown, mut all) = (0, 0);
    for layer in doc.layers().iter().rev() {
        let group_shows = layer.parent.and_then(|p| groups.iter().find(|(g, _)| *g == p)).is_none_or(|(_, s)| *s);
        let shows = layer.visible && group_shows;
        match layer.kind {
            LayerKind::Group => groups.push((layer.id, shows)),
            LayerKind::Paint => {
                all += 1;
                shown += usize::from(shows);
            }
            LayerKind::Mask => {}
        }
    }
    (shown, all)
}

/// What a slow frame's line says after its time (see [`FrameNotes::said`]): what it was drawn
/// with, `painting` (or that it is as last told, if `again`), and the stamps new to it.
fn slow_note(painting: &str, stamps: u64, again: bool) -> String {
    if again {
        format!("; brush and canvas as before; {stamps} new stamps")
    } else {
        format!("; {painting}; {stamps} new stamps")
    }
}

/// What the frames were last drawn with, for a slow frame's line (see the module's doc).
#[derive(Default)]
pub(crate) struct FrameNotes(parking_lot::Mutex<Notes>);

#[derive(Default)]
struct Notes {
    /// What the last frame noted was drawn with, and the stamps new to it.
    last: Option<(Painting, u64)>,
    /// The canvas's stamps queued so far, as the last frame was noted.
    queued: u64,
    /// What a slow frame's line last told in full, and when.
    told: Option<(String, Instant)>,
}

impl FrameNotes {
    /// Notes what the frame just drawn from `st` was drawn with. (Never waits: should a line be
    /// reading the notes just then, this frame goes unnoted, its stamps counted in the next.)
    pub(crate) fn note(&self, st: &State) {
        let Some(mut notes) = self.0.try_lock() else { return };
        let queued = st.canvas.as_deref().map_or(0, procraft_gpu::Canvas::stamps_queued);
        let painting = Painting::of(st, notes.last.as_ref().map(|(p, _)| &p.brush.name));
        // (a canvas made anew counts from nought)
        let stamps = queued.checked_sub(notes.queued).unwrap_or(queued);
        notes.queued = queued;
        notes.last = Some((painting, stamps));
    }

    /// What a slow frame's line says after its time at `now`: what the last frame noted was drawn
    /// with (or, told already less than [`TOLD_AGAIN_AFTER`] ago, that it is as before) and the
    /// stamps new to it. Nothing before a frame is noted (or, never so far, while one is).
    pub(crate) fn said(&self, now: Instant) -> String {
        let Some(mut notes) = self.0.try_lock() else { return String::new() };
        let Some((painting, stamps)) = notes.last.as_ref() else { return String::new() };
        let (painting, stamps) = (painting.to_string(), *stamps);
        let again = notes
            .told
            .as_ref()
            .is_some_and(|(told, at)| *told == painting && now.saturating_duration_since(*at) < TOLD_AGAIN_AFTER);
        let line = slow_note(&painting, stamps, again);
        if !again {
            notes.told = Some((painting, now));
        }
        line
    }
}

impl CanvasSession {
    /// What a slow frame's line in the log says after its time (`; brush "6B Pencil" (paint)
    /// 24 px; canvas 4096×4096, zoom 37%, 12 of 14 layers visible; 380 new stamps`), from the last
    /// frame drawn: read without the canvas's lock, so it never waits on a frame or a stroke
    /// holding it. Nothing before a frame is drawn.
    pub fn slow_frame_note(&self) -> String {
        self.shared.frame_notes.said(Instant::now())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pencil() -> BrushNote {
        BrushNote { tool: ToolKind::Paint, name: Arc::from("6B Pencil"), size: 24.4, dual: true, wet: true }
    }

    /// An artwork's canvas made says how big it is and its dots per inch, beside what it said
    /// before.
    #[test]
    fn a_canvas_made_says_its_size_and_dots_per_inch() {
        assert_eq!(
            made_line(812, (4096, 3072), 300.0, 14, 2, 1536),
            "canvas made in 812 ms: 4096×3072 px, 300 dpi, 14 layers, 2 kept compressed, 1536 KB of copies of the rest"
        );
        assert_eq!(
            made_line(5, (64, 64), 72.0001, 1, 0, 0),
            "canvas made in 5 ms: 64×64 px, 72 dpi, 1 layers, 0 kept compressed, 0 KB of copies of the rest"
        );
        assert!(made_line(5, (64, 64), 132.5, 1, 0, 0).contains(", 132.5 dpi,"));
    }

    /// A brush chosen says its name, its tool, its size on the canvas and whether it is dual or
    /// wet; a name with quotes or a line break in it stays on its line.
    #[test]
    fn a_brush_chosen_says_its_name_size_and_whether_it_is_dual_or_wet() {
        assert_eq!(chosen_line(&pencil()), r#"brush chosen: "6B Pencil" (paint) 24 px, dual, wet"#);
        let dry = BrushNote { tool: ToolKind::Erase, size: 2.34, dual: false, wet: false, ..pencil() };
        assert_eq!(chosen_line(&dry), r#"brush chosen: "6B Pencil" (erase) 2.3 px"#);
        let odd = BrushNote { tool: ToolKind::Smudge, name: Arc::from("say \"hi\"\nthere"), wet: false, ..pencil() };
        assert_eq!(chosen_line(&odd), r#"brush chosen: "say \"hi\"\nthere" (smudge) 24 px, dual"#);
    }

    /// A slow frame's line says the brush, the canvas, the zoom, the layers visible and the stamps
    /// new to it; within a while of saying them, an unchanged brush and canvas are as before, and
    /// said again once they change or the while is out.
    #[test]
    fn a_slow_frame_says_the_brush_canvas_zoom_layers_and_stamps_once_a_while() {
        let painting = Painting { brush: pencil(), canvas: (4096, 4096), zoom: 0.372, layers: (12, 14) };
        assert_eq!(
            painting.to_string(),
            r#"brush "6B Pencil" (paint) 24 px, dual, wet; canvas 4096×4096, zoom 37%, 12 of 14 layers visible"#
        );
        let far = Painting { zoom: 0.045, ..painting.clone() };
        assert!(far.to_string().contains(", zoom 4.5%, "), "{far}");
        let notes = FrameNotes::default();
        assert_eq!(notes.said(Instant::now()), "", "no frame noted");
        notes.0.lock().last = Some((painting.clone(), 380));
        let t = Instant::now();
        let full = r#"; brush "6B Pencil" (paint) 24 px, dual, wet; canvas 4096×4096, zoom 37%, 12 of 14 layers visible; 380 new stamps"#;
        assert_eq!(notes.said(t), full);
        notes.0.lock().last = Some((painting.clone(), 7));
        assert_eq!(notes.said(t + Duration::from_secs(3)), "; brush and canvas as before; 7 new stamps");
        // changed: said again
        let zoomed = Painting { zoom: 1.0, ..painting.clone() };
        notes.0.lock().last = Some((zoomed, 0));
        assert_eq!(
            notes.said(t + Duration::from_secs(4)),
            r#"; brush "6B Pencil" (paint) 24 px, dual, wet; canvas 4096×4096, zoom 100%, 12 of 14 layers visible; 0 new stamps"#
        );
        // (a zoom that rounds to the same isn't a change)
        notes.0.lock().last = Some((Painting { zoom: 1.0004, ..painting.clone() }, 1));
        assert_eq!(notes.said(t + Duration::from_secs(5)), "; brush and canvas as before; 1 new stamps");
        // the while out: said again, unchanged
        notes.0.lock().last = Some((Painting { zoom: 1.0, ..painting }, 2));
        assert!(notes.said(t + Duration::from_secs(5) + TOLD_AGAIN_AFTER).starts_with("; brush \"6B Pencil\""));
    }

    /// The layers counted visible are those holding pixels with their own eye on, in no hidden
    /// group (however deep); groups and masks aren't counted.
    #[test]
    fn the_layers_visible_leave_out_those_hidden_and_those_in_a_hidden_group() {
        // `ids` put in a group of their own
        let grouped = |doc: &mut Document, ids: &[LayerId]| {
            let (first, rest) = ids.split_first().expect("a layer");
            doc.set_current(*first);
            doc.clear_picks();
            for id in rest {
                doc.toggle_pick(*id);
            }
            doc.group_chosen().expect("a group")
        };
        let mut doc = Document::new(16, 16);
        let first = doc.current();
        let hidden = doc.add_layer();
        doc.set_visible(hidden, false);
        let inner = doc.add_layer();
        let deep = doc.add_layer();
        assert_eq!(layers_shown(&doc), (3, 4));
        let group = grouped(&mut doc, &[inner, deep]);
        assert_eq!(layers_shown(&doc), (3, 4), "a group shown");
        doc.set_visible(group, false);
        assert_eq!(layers_shown(&doc), (1, 4), "{:?}", doc.layers());
        let outer = grouped(&mut doc, &[group]);
        doc.set_visible(group, true);
        doc.set_visible(outer, false);
        assert_eq!(layers_shown(&doc), (1, 4), "the group round it hidden");
        doc.set_visible(outer, true);
        assert_eq!(layers_shown(&doc), (3, 4));
        assert!(doc.add_mask(first).is_some());
        assert_eq!(doc.layers().iter().filter(|l| l.kind == LayerKind::Mask).count(), 1);
        assert_eq!(layers_shown(&doc), (3, 4), "a mask not counted");
    }

    /// A session's frame notes what it draws with: the current tool's brush at its size, the
    /// canvas, the zoom and its layers, as the session itself says them.
    #[test]
    fn a_frame_notes_the_brush_and_canvas_the_session_has() {
        let s = CanvasSession::new(300, 200);
        s.set_tool(ToolKind::Erase);
        s.set_brush_size(0.5);
        let _ = s.add_layer();
        s.shared.frame_notes.note(&s.shared.lock());
        let info = s.info();
        let brush = s.current_brush();
        let dual = s.shared.lock().brush(ToolKind::Erase).secondary.is_some();
        let wet = s.shared.lock().brush(ToolKind::Erase).mixes_wet();
        let mut want = format!("; brush {:?} (erase) {} px", brush.name, short(info.brush_diameter));
        if dual {
            want += ", dual";
        }
        if wet {
            want += ", wet";
        }
        want += &format!("; canvas 300×200, zoom {}%, 2 of 2 layers visible; 0 new stamps", short(info.zoom * 100.0));
        assert_eq!(s.slow_frame_note(), want);
        // the brush chosen line says the same of the brush
        let chosen = chosen_line(&BrushNote::of(&s.shared.lock(), None));
        let said = chosen.strip_prefix("brush chosen: ").expect("the line's start");
        assert!(want.starts_with(&format!("; brush {said}; canvas ")), "{chosen} {want}");
    }

    /// A slow frame's line is told while another thread holds the canvas (a stroke laid, a frame
    /// drawn), at once: its note takes no lock of the canvas's.
    #[test]
    fn a_slow_frames_note_is_read_while_another_thread_holds_the_canvas() {
        let s = CanvasSession::new(64, 64);
        s.shared.frame_notes.note(&s.shared.lock());
        let free = s.slow_frame_note();
        assert!(free.starts_with("; brush "), "{free}");
        let (held, go) = (std::sync::mpsc::channel(), std::sync::mpsc::channel::<()>());
        let holder = {
            let (s, held) = (Arc::clone(&s), held.0);
            std::thread::spawn(move || {
                let _st = s.shared.lock();
                let _ = held.send(());
                let _ = go.1.recv_timeout(Duration::from_secs(5));
            })
        };
        held.1.recv_timeout(Duration::from_secs(5)).expect("the other thread holds the canvas");
        assert!(s.shared.try_lock().is_none(), "the canvas is held");
        let t = Instant::now();
        let busy = s.slow_frame_note();
        let quick = t.elapsed();
        let _ = go.0.send(());
        holder.join().expect("the holder ends");
        assert_eq!(busy, "; brush and canvas as before; 0 new stamps");
        assert!(quick < Duration::from_millis(20), "{quick:?}");
    }

    /// Noting a frame costs next to nothing beside the frame, with many layers in groups too: a
    /// few microseconds (in a debug build, where it is slowest), each frame.
    #[test]
    fn noting_a_frame_with_many_layers_in_groups_is_quick() {
        let s = CanvasSession::new(64, 64);
        {
            let mut st = s.shared.lock();
            let doc = &mut st.doc;
            // 40 groups of 6 layers, every other one hidden
            for i in 0..40 {
                let ids: Vec<LayerId> = (0..6).map(|_| doc.add_layer()).collect();
                let (first, rest) = ids.split_first().expect("layers");
                doc.set_current(*first);
                doc.clear_picks();
                for id in rest {
                    doc.toggle_pick(*id);
                }
                let group = doc.group_chosen().expect("a group");
                doc.set_visible(group, i % 2 == 0);
                doc.set_current(group);
            }
            assert_eq!(layers_shown(doc), (1 + 20 * 6, 241), "the layers made");
        }
        let st = s.shared.lock();
        let notes = FrameNotes::default();
        notes.note(&st);
        const FRAMES: u32 = 2000;
        let t = Instant::now();
        for _ in 0..FRAMES {
            notes.note(&st);
        }
        let each = t.elapsed() / FRAMES;
        eprintln!("a frame noted in {each:?} with {} layers in 40 groups", st.doc.layers().len());
        assert!(each < Duration::from_micros(200), "{each:?} a frame");
    }
}

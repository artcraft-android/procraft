// The timeline grid driven through the interface, with ui.rs's headless window (this file is
// included in its tests, as `timeline_grid`).

/// A frame of the window, as [`Window::frame`] runs it, giving what the interface drew.
fn drawn_frame(w: &mut Window) -> Vec<egui::epaint::ClippedShape> {
    w.time += 1.0 / 60.0;
    w.input.tick(&w.session);
    let raw = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, w.size)),
        time: Some(w.time),
        events: std::mem::take(&mut w.events),
        ..RawInput::default()
    };
    let Window { ctx, ui, input, session, .. } = &mut *w;
    let mut output = ctx.clone().run_ui(raw, |egui_ui| ui.show(egui_ui, session, input));
    output.textures_delta.clear();
    output.shapes
}

/// Every place `words` was written, exactly, in `shapes`.
fn written(shapes: &[egui::epaint::ClippedShape], words: &str) -> Vec<Pos2> {
    fn find(shape: &egui::Shape, words: &str, found: &mut Vec<Pos2>) {
        match shape {
            egui::Shape::Text(t) if t.galley.text() == words => found.push(t.pos),
            egui::Shape::Vec(all) => all.iter().for_each(|s| find(s, words, found)),
            _ => {}
        }
    }
    let mut found = Vec::new();
    shapes.iter().for_each(|c| find(&c.shape, words, &mut found));
    found
}

/// A window on a 400 x 300 canvas whose Actions open on the Canvas tab.
fn open() -> Option<Window> {
    let prefs = crate::prefs::Prefs { right_handed: true, actions_tab: 1, ..Default::default() };
    Window::open_canvas(vec2(1180.0, 1000.0), (400, 300), prefs)
}

/// Frames enough for whatever slides to settle.
fn settle(w: &mut Window) {
    for _ in 0..30 {
        w.frame();
    }
}

/// Actions → Canvas's row `label` clicked, and Actions closed again.
fn actions_row(w: &mut Window, label: &str) {
    w.ui.open = Some(Popover::Actions);
    w.frame();
    w.frame();
    let shapes = drawn_frame(w);
    let at = *written(&shapes, label).first().unwrap_or_else(|| panic!("no {label} row"));
    w.click(at + vec2(4.0, 8.0));
    settle(w);
    w.ui.open = None;
    settle(w);
}

/// The timeline turned on with Actions → Canvas → Timeline.
fn timeline_on(w: &mut Window) {
    actions_row(w, "Timeline");
    assert!(w.session.timeline_info().is_some(), "on a timeline");
}

fn info(w: &Window) -> procraft_core::TimelineInfo {
    w.session.timeline_info().expect("on a timeline")
}

/// The chosen track's clips' (start, duration), in time order.
fn clips(w: &Window) -> Vec<(u32, u32)> {
    let track = info(w).track;
    let tracks = w.session.timeline_tracks();
    let t = tracks.iter().find(|t| Some(t.id) == track).expect("the chosen track");
    t.clips.iter().map(|c| (c.start, c.duration)).collect()
}

/// A header button of the grid clicked.
fn button(w: &mut Window, name: &str) {
    let at = w.ui.timeline.button_centre(&w.ctx, name);
    w.click(at);
    settle(w);
}

/// The layers panel opened: the layers' names it lists, as drawn.
fn layers_listed(w: &mut Window, names: &[&str]) -> Vec<bool> {
    w.ui.open = Some(Popover::Layers);
    w.frame();
    w.frame();
    let shapes = drawn_frame(w);
    w.ui.open = None;
    w.frame();
    names.iter().map(|n| !written(&shapes, n).is_empty()).collect()
}

/// Actions → Canvas → Timeline puts the artwork on a timeline and shows the grid: its one layer
/// is the drawing on frame 1 of its track. A stroke on the empty frame after it (› in the header)
/// makes a drawing there, as long as a frame, and paints it; the layers panel lists only the
/// drawing at the playhead. Timeline again takes it off: the grid goes, and every layer is listed.
#[test]
fn the_timeline_comes_on_from_actions_and_a_stroke_on_an_empty_frame_makes_a_drawing() {
    let Some(mut w) = open() else { return };
    assert!(!w.ui.timeline.settings_asked);
    timeline_on(&mut w);
    // (named after the track's made, which the grid writes)
    let first = w.session.layers().into_iter().find(|l| l.current).expect("a layer").id;
    w.session.rename_layer(first, "Ink".into());
    assert!(w.ui.timeline.showing(), "the grid shows");
    assert!(w.area_shown(egui::Order::Middle, egui::Id::unique("timeline grid")).is_some());
    assert_eq!(clips(&w), vec![(0, 1)]);
    assert_eq!(info(&w).frame, 0);
    // › : the empty frame after it, past the timeline's end
    button(&mut w, "On");
    assert_eq!(info(&w).frame, 1);
    assert!((w.ui.timeline.shown_at() - 1.0).abs() < 0.01, "slid under the playhead");
    let paper = w.paper();
    w.mouse_stroke(paper.center() - vec2(60.0, 0.0), paper.center() + vec2(60.0, 20.0), 12);
    settle(&mut w);
    assert_eq!(clips(&w), vec![(0, 1), (1, 1)], "a drawing made on frame 2");
    assert_eq!(info(&w).duration, 2);
    assert!(w.painted() > 0, "the stroke painted it");
    let made = w.session.timeline_drawing_shown().expect("the drawing at the playhead");
    let made_name = w.session.layers().into_iter().find(|l| l.id == made).expect("its group").name;
    assert_ne!(made, first);
    assert_eq!(layers_listed(&mut w, &["Ink", &made_name]), vec![false, true]);
    // the first frame tapped: its drawing listed alone
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 0);
    w.click(at);
    settle(&mut w);
    assert_eq!(info(&w).frame, 0);
    assert_eq!(w.session.timeline_drawing_shown(), Some(first));
    assert_eq!(layers_listed(&mut w, &["Ink", &made_name]), vec![true, false]);
    // off again
    actions_row(&mut w, "Timeline");
    assert!(w.session.timeline_info().is_none());
    assert!(!w.ui.timeline.showing(), "the grid gone");
    assert_eq!(layers_listed(&mut w, &["Ink", &made_name]), vec![true, true]);
}

/// The header's + makes a new frame after the one at the playhead, which goes onto it; its new
/// track button makes a track on top, which is chosen (its row first, outlined). A tap on a frame
/// of the other track chooses it; a tap on a track's name does too.
#[test]
fn the_header_adds_a_frame_and_a_track_and_a_tap_chooses_a_track() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    let old = info(&w).track.expect("a track");
    button(&mut w, "Add");
    assert_eq!(clips(&w), vec![(0, 1), (1, 1)]);
    assert_eq!(info(&w).frame, 1);
    button(&mut w, "Track");
    let tracks = w.session.timeline_tracks();
    assert_eq!(tracks.len(), 2);
    let new = info(&w).track.expect("a track");
    assert_ne!(new, old);
    assert_eq!(tracks.last().map(|t| t.id), Some(new), "on top");
    assert!(clips(&w).is_empty());
    // the old track's row is the second: its frame 1 tapped
    let at = w.ui.timeline.cell_centre(&w.ctx, 1, 0);
    w.click(at);
    settle(&mut w);
    assert_eq!(info(&w).track, Some(old));
    assert_eq!(info(&w).frame, 0);
    // the new track's name tapped
    let at = w.ui.timeline.name_centre(&w.ctx, 0);
    w.click(at);
    settle(&mut w);
    assert_eq!(info(&w).track, Some(new));
    assert_eq!(info(&w).frame, 0, "the playhead stays");
    // the settings button asks for the settings, as Actions' Timeline Settings does
    button(&mut w, "Settings");
    assert!(w.ui.timeline.settings_asked);
    w.ui.timeline.settings_asked = false;
    actions_row(&mut w, "Timeline Settings");
    assert!(w.ui.timeline.settings_asked);
}

/// The frames dragged sideways scrub under the playhead (each frame shown as it passes, not yet
/// drawn on) and settle on the frame under it when let go; a tap on a frame or the ruler slides it
/// under the playhead. ‹ and › step a frame; held, they go to the drawing before or after.
#[test]
fn scrubbing_tapping_and_stepping_move_the_playhead() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    for _ in 0..4 {
        button(&mut w, "Add");
    }
    assert_eq!(clips(&w).len(), 5);
    assert_eq!(info(&w).frame, 4);
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 1);
    w.click(at);
    settle(&mut w);
    assert_eq!(info(&w).frame, 1);
    // dragged two frames' widths to the left: frame 4 comes under the playhead
    let from = w.ui.timeline.cell_centre(&w.ctx, 0, 6);
    let to = from - vec2(2.0 * 44.0, 0.0);
    w.hover(from);
    w.mouse_button(from, true);
    w.mouse_drag(from, to, 8);
    assert_eq!(info(&w).frame, 1, "not drawn on while scrubbing");
    assert!((w.ui.timeline.shown_at() - 3.0).abs() < 0.01, "{}", w.ui.timeline.shown_at());
    // still a moment, then let go
    for _ in 0..12 {
        w.hover(to);
    }
    w.mouse_button(to, false);
    settle(&mut w);
    assert_eq!(info(&w).frame, 3);
    assert!((w.ui.timeline.shown_at() - 3.0).abs() < 0.01);
    // the ruler over frame 1 tapped
    let at = w.ui.timeline.ruler_centre(&w.ctx, 0);
    w.click(at);
    settle(&mut w);
    assert_eq!(info(&w).frame, 0);
    button(&mut w, "On");
    assert_eq!(info(&w).frame, 1);
    button(&mut w, "Back");
    assert_eq!(info(&w).frame, 0);
    // the first drawing held for 3 frames: › held goes to the drawing after it
    let id = w.session.timeline_tracks()[0].clips[0].id;
    assert!(w.session.set_timeline_clip_duration(id, 3, true));
    settle(&mut w);
    let on = w.ui.timeline.button_centre(&w.ctx, "On");
    w.hover(on);
    w.mouse_button(on, true);
    for _ in 0..30 {
        w.hover(on);
    }
    w.mouse_button(on, false);
    settle(&mut w);
    assert_eq!(info(&w).frame, 3, "to the next drawing");
    let back = w.ui.timeline.button_centre(&w.ctx, "Back");
    w.hover(back);
    w.mouse_button(back, true);
    for _ in 0..30 {
        w.hover(back);
    }
    w.mouse_button(back, false);
    settle(&mut w);
    assert_eq!(info(&w).frame, 0, "to the drawing before");
}

/// Play goes round the frames the grid shows from the playhead's, at the timeline's frame rate,
/// the grid holding still; Pause stops on the frame it got to, which is drawn on.
#[test]
fn play_goes_round_the_frames_and_pause_stays_where_it_got_to() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    for _ in 0..3 {
        button(&mut w, "Add");
    }
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 0);
    w.click(at);
    settle(&mut w);
    assert_eq!(info(&w).frame, 0);
    let fps = info(&w).fps;
    button(&mut w, "Play");
    assert!(w.ui.timeline.is_playing());
    let mut seen = std::collections::BTreeSet::new();
    // two seconds: round the four frames several times
    for _ in 0..120 {
        w.frame();
        seen.insert(w.ui.timeline.frame());
        assert!((w.ui.timeline.shown_at() - 0.0).abs() < 0.01, "the grid holds still");
    }
    assert_eq!(seen.into_iter().collect::<Vec<_>>(), vec![0, 1, 2, 3], "at {fps} a second");
    assert_eq!(info(&w).frame, 0, "the playhead stays while it plays");
    let play = w.ui.timeline.button_centre(&w.ctx, "Play");
    w.click(play);
    let stopped = w.ui.timeline.frame();
    assert!(!w.ui.timeline.is_playing());
    settle(&mut w);
    assert_eq!(info(&w).frame, stopped);
    assert!((w.ui.timeline.shown_at() - stopped as f32).abs() < 0.01, "slid under the playhead");
    // a press on the frames stops it too
    button(&mut w, "Play");
    for _ in 0..10 {
        w.frame();
    }
    assert!(w.ui.timeline.is_playing());
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 2);
    w.click(at);
    assert!(!w.ui.timeline.is_playing());
}

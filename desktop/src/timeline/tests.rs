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
    // the settings button opens the settings' card, as Actions' Timeline Settings does
    button(&mut w, "Settings");
    assert!(w.ui.timeline_settings.is_open());
    w.ui.timeline_settings.close();
    actions_row(&mut w, "Timeline Settings");
    assert!(w.ui.timeline_settings.is_open());
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

// MARK: Editing the frames

/// The pointer pressed at `at` and held still there for `frames` frames (60 a second), not let go.
fn press_held(w: &mut Window, at: Pos2, frames: usize) {
    w.hover(at);
    w.mouse_button(at, true);
    for _ in 0..frames {
        w.hover(at);
    }
}

/// A press held still at `at` long enough to open its menu, then let go.
fn hold(w: &mut Window, at: Pos2) {
    press_held(w, at, 30);
    w.mouse_button(at, false);
    w.frame();
}

/// The mouse's other button clicked at `at`.
fn right_click(w: &mut Window, at: Pos2) {
    w.hover(at);
    for pressed in [true, false] {
        let button = egui::PointerButton::Secondary;
        w.events.push(Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE });
        w.frame();
    }
    w.frame();
}

/// Where `label` is written on the screen now (its first place), if it is.
fn label_at(w: &mut Window, label: &str) -> Option<Pos2> {
    let shapes = drawn_frame(w);
    written(&shapes, label).first().copied()
}

/// The menu's row (or button) `label` clicked.
fn menu_row(w: &mut Window, label: &str) {
    let at = label_at(w, label).unwrap_or_else(|| panic!("no {label} in the menu"));
    w.click(at + vec2(3.0, 6.0));
    settle(w);
}

/// A drag from `from` to `to`, a frame still first, held still at the end before letting go.
fn drag(w: &mut Window, from: Pos2, to: Pos2) {
    w.hover(from);
    w.mouse_button(from, true);
    w.mouse_drag(from, to, 10);
    for _ in 0..4 {
        w.hover(to);
    }
    w.mouse_button(to, false);
    settle(w);
}

/// Every track's (top first) clips' (start, duration), in time order.
fn all_clips(w: &Window) -> Vec<Vec<(u32, u32)>> {
    w.session.timeline_tracks().iter().rev().map(|t| t.clips.iter().map(|c| (c.start, c.duration)).collect()).collect()
}

/// The chosen track's clips' ids, in time order.
fn clip_ids(w: &Window) -> Vec<u64> {
    let track = info(w).track;
    let tracks = w.session.timeline_tracks();
    let t = tracks.iter().find(|t| Some(t.id) == track).expect("the chosen track");
    t.clips.iter().map(|c| c.id).collect()
}

/// The frame under the playhead tapped: its handles come out. Its end's handle dragged two frames
/// on makes it three frames long, the frame after it pushed along, one step when let go; its
/// start's dragged back where there is no room makes it longer at its end instead. Tapped again,
/// the handles go.
#[test]
fn a_frame_tapped_under_the_playhead_lasts_longer_by_its_handles() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    button(&mut w, "Add");
    assert_eq!(clips(&w), vec![(0, 1), (1, 1)]);
    let first = clip_ids(&w)[0];
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 0);
    w.click(at);
    settle(&mut w);
    assert_eq!(info(&w).frame, 0);
    assert_eq!(w.ui.timeline.picked_now(), None, "the first tap only goes there");
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 0);
    w.click(at);
    settle(&mut w);
    assert_eq!(w.ui.timeline.picked_now(), Some(first), "the handles out");
    let end = w.ui.timeline.handle_centre(&w.ctx, false).expect("its end's handle");
    // dragged two frames on: shown as it goes, done when let go
    w.hover(end);
    w.mouse_button(end, true);
    w.mouse_drag(end, end + vec2(2.0 * 44.0, 0.0), 10);
    assert_eq!(clips(&w), vec![(0, 1), (1, 1)], "nothing done before it is let go");
    w.mouse_button(end + vec2(2.0 * 44.0, 0.0), false);
    settle(&mut w);
    assert_eq!(clips(&w), vec![(0, 3), (3, 1)], "longer, the frame after it pushed along");
    assert!(w.steps().0, "an undo step");
    // its start dragged back with no room before it: as much longer at its end
    let start = w.ui.timeline.handle_centre(&w.ctx, true).expect("its start's handle");
    drag(&mut w, start, start - vec2(44.0, 0.0));
    assert_eq!(clips(&w), vec![(0, 4), (4, 1)]);
    // tapped again: the handles go
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 0);
    w.click(at);
    settle(&mut w);
    assert_eq!(w.ui.timeline.picked_now(), None);
}

/// The lasso picks the frames a stroke crosses, and a stroke over a picked frame lets it go. One
/// of them held opens their menu: Flip frames puts them in the opposite order, Duplicate puts
/// copies after them, picked in their place.
#[test]
fn the_lasso_picks_frames_whose_menu_flips_and_duplicates_them() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    button(&mut w, "Add");
    button(&mut w, "Add");
    let ids = clip_ids(&w);
    assert_eq!(ids.len(), 3);
    button(&mut w, "Lasso");
    assert_eq!(w.ui.timeline.chosen_now(), Some(Vec::new()));
    let (a, c) = (w.ui.timeline.cell_centre(&w.ctx, 0, 0), w.ui.timeline.cell_centre(&w.ctx, 0, 2));
    w.mouse_stroke(a, c, 12);
    settle(&mut w);
    let mut all = ids.clone();
    all.sort_unstable();
    assert_eq!(w.ui.timeline.chosen_now(), Some(all.clone()), "the three crossed");
    // a stroke over the middle one lets it go, another picks it again
    let b = w.ui.timeline.cell_centre(&w.ctx, 0, 1);
    w.mouse_stroke(b - vec2(12.0, 0.0), b + vec2(12.0, 4.0), 6);
    settle(&mut w);
    assert_eq!(w.ui.timeline.chosen_now().map(|c| c.len()), Some(2));
    assert!(!w.ui.timeline.chosen_now().unwrap_or_default().contains(&ids[1]));
    w.mouse_stroke(b - vec2(12.0, 0.0), b + vec2(12.0, 4.0), 6);
    settle(&mut w);
    assert_eq!(w.ui.timeline.chosen_now(), Some(all.clone()));
    // held: their menu
    hold(&mut w, a);
    assert!(w.ui.timeline.menu_open(), "the picked frames' menu");
    menu_row(&mut w, "Flip frames");
    assert!(!w.ui.timeline.menu_open());
    let flipped = clip_ids(&w);
    assert_eq!(flipped, ids.iter().rev().copied().collect::<Vec<_>>(), "in the opposite order");
    assert_eq!(w.ui.timeline.chosen_now(), Some(all.clone()), "still picked");
    hold(&mut w, a);
    menu_row(&mut w, "Duplicate");
    assert_eq!(clips(&w).len(), 6);
    let chosen = w.ui.timeline.chosen_now().unwrap_or_default();
    assert_eq!(chosen.len(), 3);
    assert!(chosen.iter().all(|id| !ids.contains(id)), "the copies picked");
    // the lasso off: nothing picked
    button(&mut w, "Lasso");
    assert_eq!(w.ui.timeline.chosen_now(), None);
}

/// A frame held and dragged goes along by frames and onto the row it is over, dropped where it is
/// let go; over the line between two rows it goes onto a new track made there.
#[test]
fn a_frame_held_and_dragged_moves_onto_another_row_and_onto_a_new_track() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    button(&mut w, "Add");
    button(&mut w, "Track");
    assert_eq!(all_clips(&w), vec![vec![], vec![(0, 1), (1, 1)]]);
    // the lower track's second frame held, then dragged two frames on and up a row
    let from = w.ui.timeline.cell_centre(&w.ctx, 1, 1);
    press_held(&mut w, from, 30);
    assert!(w.ui.timeline.menu_open(), "held: its menu");
    let to = w.ui.timeline.cell_centre(&w.ctx, 0, 3);
    w.mouse_drag(from, to, 12);
    assert!(!w.ui.timeline.menu_open(), "picked up: the menu goes");
    assert_eq!(all_clips(&w), vec![vec![], vec![(0, 1), (1, 1)]], "not yet dropped");
    w.mouse_button(to, false);
    settle(&mut w);
    assert_eq!(all_clips(&w), vec![vec![(3, 1)], vec![(0, 1)]]);
    // it, held and dragged over the line above the lower row: a new track there
    let from = w.ui.timeline.cell_centre(&w.ctx, 0, 3);
    press_held(&mut w, from, 30);
    let to = w.ui.timeline.row_line(&w.ctx, 1, 3);
    w.mouse_drag(from, to, 12);
    w.hover(to);
    w.mouse_button(to, false);
    settle(&mut w);
    assert_eq!(all_clips(&w), vec![vec![], vec![(3, 1)], vec![(0, 1)]], "on a track of its own between them");
}

/// A frame's menu (held, or right-clicked): Delete frame takes it off, leaving empty frames. An
/// empty frame's menu: its + makes the empty frames one longer, the frame after them moving
/// along; Remove empty section closes them up. Keys asks for the frame's keys' card.
#[test]
fn a_frame_s_and_an_empty_frame_s_menus() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    button(&mut w, "Add");
    button(&mut w, "Add");
    assert_eq!(clips(&w), vec![(0, 1), (1, 1), (2, 1)]);
    let middle = w.ui.timeline.cell_centre(&w.ctx, 0, 1);
    right_click(&mut w, middle);
    assert!(w.ui.timeline.menu_open());
    menu_row(&mut w, "Delete frame");
    assert_eq!(clips(&w), vec![(0, 1), (2, 1)], "an empty frame in its place");
    // the empty frame held: its menu
    hold(&mut w, middle);
    assert!(w.ui.timeline.menu_open(), "the empty frame's menu");
    assert!(label_at(&mut w, "Empty frames").is_some());
    menu_row(&mut w, "+");
    assert_eq!(clips(&w), vec![(0, 1), (3, 1)], "two empty frames");
    menu_row(&mut w, "Remove empty section");
    assert_eq!(clips(&w), vec![(0, 1), (1, 1)]);
    assert!(!w.ui.timeline.menu_open());
    // a frame's Keys: its keys' card asked for, the playhead brought onto it
    let id = clip_ids(&w)[1];
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 0);
    w.click(at);
    settle(&mut w);
    let second = w.ui.timeline.cell_centre(&w.ctx, 0, 1);
    right_click(&mut w, second);
    menu_row(&mut w, "Keys");
    assert_eq!(w.ui.timeline.keys_card_for, Some(id));
    assert_eq!(info(&w).frame, 1);
    assert!(!w.ui.timeline.menu_open());
}

/// A track's name held (or right-clicked) opens its menu: Lock track locks it, Highlight's colours
/// stand beside it and colour its name.
#[test]
fn a_track_s_menu_locks_and_highlights_it() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    let track = info(&w).track.expect("a track");
    let name = w.ui.timeline.name_centre(&w.ctx, 0);
    hold(&mut w, name);
    assert!(w.ui.timeline.menu_open(), "held: its menu");
    menu_row(&mut w, "Lock track");
    let t = |w: &Window| w.session.timeline_tracks().into_iter().find(|t| t.id == track).expect("the track");
    assert!(t(&w).locked);
    assert!(label_at(&mut w, "Unlock track").is_some(), "the menu stays, saying so");
    menu_row(&mut w, "Highlight");
    menu_row(&mut w, "Green");
    assert_eq!(t(&w).label, Some(0x30D158));
    // a press off the menu closes it
    w.click(pos2(40.0, 40.0));
    settle(&mut w);
    assert!(!w.ui.timeline.menu_open());
    right_click(&mut w, name);
    assert!(w.ui.timeline.menu_open(), "right-clicked: its menu");
    menu_row(&mut w, "Unlock track");
    assert!(!t(&w).locked);
}

/// The end flap dragged makes the timeline longer; tapped, its menu fits it to the frames and adds
/// frames at its end.
#[test]
fn the_end_flap_sets_the_timeline_s_length() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    button(&mut w, "Add");
    let before = info(&w).duration;
    assert_eq!(before, 2);
    let flap = w.ui.timeline.flap_centre(&w.ctx);
    drag(&mut w, flap, flap + vec2(3.0 * 44.0, 0.0));
    assert_eq!(info(&w).duration, before + 3);
    let flap = w.ui.timeline.flap_centre(&w.ctx);
    w.click(flap);
    settle(&mut w);
    assert!(w.ui.timeline.menu_open(), "tapped: its menu");
    menu_row(&mut w, "Fit to content");
    assert_eq!(info(&w).duration, 2);
    let flap = w.ui.timeline.flap_centre(&w.ctx);
    w.click(flap);
    settle(&mut w);
    let fps = info(&w).fps;
    menu_row(&mut w, "Add at end");
    assert_eq!(info(&w).duration, 2 + fps, "a second's worth");
}

/// Frames picked are made a group, a block with a chevron: tapped, its tracks' rows show under its
/// track; two groups on one track expanded share those rows. Its menu's Cycle repeats it. `+` held
/// opens Add new, whose picture comes in as a new frame.
#[test]
fn groups_are_made_expanded_into_shared_rows_and_cycled() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    for _ in 0..3 {
        button(&mut w, "Add");
    }
    assert_eq!(clips(&w).len(), 4);
    // frames 1 and 2 grouped, then 3 and 4
    for (a, b) in [(0, 1), (2, 3)] {
        button(&mut w, "Lasso");
        let (from, to) = (w.ui.timeline.cell_centre(&w.ctx, 0, a), w.ui.timeline.cell_centre(&w.ctx, 0, b));
        w.mouse_stroke(from, to, 10);
        settle(&mut w);
        hold(&mut w, from);
        menu_row(&mut w, "Group frames");
        assert_eq!(w.ui.timeline.chosen_now(), None, "the lasso goes off");
    }
    assert_eq!(clips(&w), vec![(0, 2), (2, 2)], "two groups");
    // made expanded: both groups' tracks share the row under the track
    assert_eq!(w.ui.timeline.row_count(), 2, "one row for both groups' tracks");
    assert_eq!(w.ui.timeline.tracks_on_row(1), 2);
    // the first one's chevron tapped: its tracks put away; again, out
    let chevron = w.ui.timeline.cell_centre(&w.ctx, 0, 0) - vec2(12.0, 0.0);
    w.click(chevron);
    settle(&mut w);
    assert_eq!(w.ui.timeline.tracks_on_row(1), 1);
    w.click(chevron);
    settle(&mut w);
    assert_eq!(w.ui.timeline.tracks_on_row(1), 2);
    // the second collapsed from its menu
    let second = w.ui.timeline.cell_centre(&w.ctx, 0, 3);
    right_click(&mut w, second);
    menu_row(&mut w, "Collapse group");
    w.click(pos2(40.0, 40.0));
    settle(&mut w);
    assert_eq!(w.ui.timeline.tracks_on_row(1), 1);
    // the first group's Cycle: Repeat
    let first = w.ui.timeline.cell_centre(&w.ctx, 0, 1);
    right_click(&mut w, first);
    menu_row(&mut w, "Cycle");
    menu_row(&mut w, "Repeat");
    let cycle = w.session.timeline_tracks().iter().flat_map(|t| t.clips.clone()).find(|c| c.start == 0).map(|c| c.cycle);
    assert_eq!(cycle, Some(2), "it plays its two frames over and over");
    w.click(pos2(40.0, 40.0));
    settle(&mut w);
    // `+` held: Add new; a picture from it is a new frame at the playhead
    let add = w.ui.timeline.button_centre(&w.ctx, "Add");
    press_held(&mut w, add, 15);
    w.mouse_button(add, false);
    settle(&mut w);
    assert!(w.ui.timeline.menu_open(), "Add new");
    assert!(label_at(&mut w, "Picture from a file…").is_some());
    let count = all_clips(&w).iter().map(Vec::len).sum::<usize>();
    let picture = procraft_core::ThumbnailImage { width: 2, height: 2, rgba: vec![255; 16] };
    w.ui.timeline.add_picture_for_test(&w.session, picture);
    settle(&mut w);
    assert_eq!(all_clips(&w).iter().map(Vec::len).sum::<usize>(), count + 1, "a new frame");
}

/// A repeating group's end handle sets how long it plays, a tag saying how many times as it is
/// dragged; its start has no handle.
#[test]
fn a_repeating_group_s_end_handle_sets_how_long_it_plays() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    button(&mut w, "Lasso");
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 0);
    w.mouse_stroke(at - vec2(12.0, 0.0), at + vec2(12.0, 0.0), 6);
    settle(&mut w);
    hold(&mut w, at);
    menu_row(&mut w, "Group frames");
    let group = clip_ids(&w)[0];
    assert_eq!(w.ui.timeline.picked_now(), Some(group), "the group made is picked");
    // (its tracks' row under it: the bar is taller)
    let at = w.ui.timeline.cell_centre(&w.ctx, 0, 0);
    right_click(&mut w, at + vec2(10.0, 0.0));
    menu_row(&mut w, "Cycle");
    menu_row(&mut w, "Repeat");
    w.click(pos2(40.0, 40.0));
    settle(&mut w);
    assert_eq!(w.session.timeline_tracks()[0].clips[0].cycle, 1);
    let end = w.ui.timeline.handle_centre(&w.ctx, false).expect("its end's handle");
    w.hover(end);
    w.mouse_button(end, true);
    w.mouse_drag(end, end + vec2(2.0 * 44.0, 0.0), 10);
    assert!(label_at(&mut w, "×3").is_some(), "the tag says three times");
    w.mouse_button(end + vec2(2.0 * 44.0, 0.0), false);
    settle(&mut w);
    assert_eq!(clips(&w), vec![(0, 3)]);
}

/// The frames picked together have the handles on the last of them: its end dragged two columns
/// on (a column for each picked frame on its track) makes every one of them a frame longer.
#[test]
fn the_picked_frames_handles_lengthen_them_all() {
    let Some(mut w) = open() else { return };
    timeline_on(&mut w);
    button(&mut w, "Add");
    button(&mut w, "Lasso");
    let (a, b) = (w.ui.timeline.cell_centre(&w.ctx, 0, 0), w.ui.timeline.cell_centre(&w.ctx, 0, 1));
    w.mouse_stroke(a, b, 8);
    settle(&mut w);
    assert_eq!(w.ui.timeline.chosen_now().map(|c| c.len()), Some(2));
    let end = w.ui.timeline.handle_centre(&w.ctx, false).expect("the last one's end handle");
    assert!((end.x - b.x) > 0.0, "on the last of them");
    drag(&mut w, end, end + vec2(2.0 * 44.0, 0.0));
    assert_eq!(clips(&w), vec![(0, 2), (2, 2)]);
    assert_eq!(w.ui.timeline.chosen_now().map(|c| c.len()), Some(2), "still picked");
}

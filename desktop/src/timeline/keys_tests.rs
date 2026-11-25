// Keys on the timeline grid driven through the interface, with ui.rs's headless window (this file
// is included in its tests, as `timeline_keys`): the Keys card, the rows of keys and their marks,
// the key's menu and the easing editor, stretching, Animate, Perform and the motion path.

use procraft_core::{TimelineChannel, TimelineEasing};

/// A frame of the window, as [`Window::frame`] runs it, giving what the interface drew.
fn drawn(w: &mut Window) -> Vec<egui::epaint::ClippedShape> {
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

/// Where `words` is written now (the first place), to click a little inside it.
fn word(w: &mut Window, words: &str) -> Pos2 {
    let shapes = drawn(w);
    *written(&shapes, words).first().unwrap_or_else(|| panic!("{words} isn't drawn")) + vec2(4.0, 8.0)
}

/// Frames enough for whatever slides to settle.
fn settle(w: &mut Window) {
    for _ in 0..30 {
        w.frame();
    }
}

/// A window on a 400 x 300 canvas, a stroke painted across its middle (corner to corner of a
/// square 60 pixels across), put on a timeline: its
/// drawing (the clip) lasting 10 frames on the one track. The clip and the track.
fn animated() -> Option<(Window, u64, u64)> {
    let prefs = crate::prefs::Prefs { right_handed: true, ..Default::default() };
    let mut w = Window::open_canvas(vec2(1180.0, 1000.0), (400, 300), prefs)?;
    let paper = w.paper();
    w.mouse_stroke(paper.center() - vec2(90.0, 90.0), paper.center() + vec2(90.0, 90.0), 12);
    assert!(w.session.start_timeline());
    settle(&mut w);
    let track = w.session.timeline_tracks().first().map(|t| t.id).expect("a track");
    let clip = w.session.timeline_tracks()[0].clips[0].id;
    assert!(w.session.set_timeline_clip_duration(clip, 10, true));
    settle(&mut w);
    Some((w, clip, track))
}

/// A position key on clip `clip` at frame `frame`, `x` across and `y` down (as the core sets it).
fn key_at(w: &mut Window, clip: u64, frame: u32, x: f32, y: f32) {
    w.session.rest_timeline_frame(frame);
    assert!(w.session.set_timeline_key(clip, TimelineChannel::Position, vec![x, y], TimelineEasing::Linear, true));
    settle(w);
}

/// The frames clip `clip` has keys of `channel` on.
fn keyed(w: &Window, clip: u64, channel: TimelineChannel) -> Vec<u32> {
    w.session.timeline_keys(clip).into_iter().filter(|k| k.channel == channel).map(|k| k.frame).collect()
}

fn frame(w: &Window) -> u32 {
    w.session.timeline_info().expect("a timeline").frame
}

/// The chevron after the first track's name clicked: its rows of keys out (or away).
fn rows_out(w: &mut Window) {
    let at = w.ui.timeline.chevron_centre(&w.ctx, 0).expect("a chevron: the track has keys");
    w.click(at);
    settle(w);
}

/// A header button of the grid clicked.
fn button(w: &mut Window, name: &str) {
    let at = w.ui.timeline.button_centre(&w.ctx, name);
    w.click(at);
    settle(w);
}

/// The mouse's right button clicked at `at`.
fn right_click(w: &mut Window, at: Pos2) {
    w.hover(at);
    for pressed in [true, false] {
        let button = egui::PointerButton::Secondary;
        w.events.push(Event::PointerButton { pos: at, button, pressed, modifiers: Modifiers::NONE });
        w.frame();
    }
    settle(w);
}

/// Canvas pixel `(x, y)` in the window.
fn on_window(w: &Window, x: f32, y: f32) -> Pos2 {
    let m = w.session.view_matrix();
    pos2(m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5])
}

/// The Keys card (a frame's menu's Keys) keys the block on the playhead's frame by slider, and
/// puts a key of a kind there by its row: the track then has a chevron, which puts its rows of
/// keys out under it, a mark on the keyed frame; a mark clicked is picked.
#[test]
fn the_keys_card_keys_the_playheads_frame_and_its_row_of_keys_shows_the_mark() {
    let Some((mut w, clip, _)) = animated() else { return };
    w.session.rest_timeline_frame(4);
    settle(&mut w);
    assert!(w.ui.timeline.chevron_centre(&w.ctx, 0).is_none(), "no keys, no chevron");
    w.ui.timeline.keys_card_for = Some(clip);
    settle(&mut w);
    let (card, _) = w.area_shown(egui::Order::Foreground, egui::Id::unique("timeline keys card")).expect("the card");
    assert!(!written(&drawn(&mut w), "Move across").is_empty());
    // Move across's bar: under the title (46), the three rows (40 each) and a gap (8), 36 into
    // its row; three quarters along it is 200 pixels to the right
    let y = card.top() + 46.0 + 3.0 * 40.0 + 8.0 + 36.0;
    let (left, right) = (card.left() + 18.0, card.right() - 18.0);
    w.click(pos2(left + (right - left) * 0.75, y));
    settle(&mut w);
    assert_eq!(keyed(&w, clip, TimelineChannel::Position), vec![4]);
    let pose = w.session.timeline_pose(clip).expect("its pose");
    assert!((pose.x - 200.0).abs() < 2.0, "{}", pose.x);
    assert_eq!(w.session.undo().as_deref(), Some("Key"), "one step");
    assert!(keyed(&w, clip, TimelineChannel::Position).is_empty());
    w.session.redo();
    settle(&mut w);
    // Key rotation here: the third row
    w.click(pos2(card.center().x, card.top() + 46.0 + 2.0 * 40.0 + 20.0));
    settle(&mut w);
    assert_eq!(keyed(&w, clip, TimelineChannel::Rotation), vec![4]);
    // the chevron: the track's rows of keys out under it
    rows_out(&mut w);
    assert_eq!(w.ui.timeline.key_rows(&w.ctx), vec![(0, "Position"), (0, "Rotation")]);
    let shapes = drawn(&mut w);
    assert!(!written(&shapes, "Position").is_empty() && !written(&shapes, "Rotation").is_empty());
    // its mark clicked: picked
    let mark = w.ui.timeline.key_centre(&w.ctx, 0, "Position", Some(4)).expect("its row");
    w.click(mark);
    settle(&mut w);
    assert_eq!(w.ui.timeline.keys.picks.len(), 1);
    assert_eq!(w.ui.timeline.keys.picks[0].frame, 4);
    // the card's × closes it; the chevron again puts the rows away
    w.ui.timeline.keys_card_for = None;
    rows_out(&mut w);
    assert!(w.ui.timeline.key_rows(&w.ctx).is_empty());
}

/// A mark clicked is picked and the playhead goes to it; dragged, it goes along by frames, shown
/// while dragged and one undo step let go.
#[test]
fn a_mark_is_picked_and_dragged_along_by_frames() {
    let Some((mut w, clip, _)) = animated() else { return };
    key_at(&mut w, clip, 2, 0.0, 0.0);
    key_at(&mut w, clip, 6, 80.0, 0.0);
    w.session.rest_timeline_frame(0);
    settle(&mut w);
    rows_out(&mut w);
    let mark = w.ui.timeline.key_centre(&w.ctx, 0, "Position", Some(2)).expect("its row");
    w.click(mark);
    settle(&mut w);
    assert_eq!(frame(&w), 2, "the playhead went to it");
    assert_eq!(w.ui.timeline.keys.picks.len(), 1);
    // dragged two frames on
    let from = w.ui.timeline.key_centre(&w.ctx, 0, "Position", Some(2)).expect("its row");
    let to = from + vec2(2.0 * 44.0, 0.0);
    w.hover(from);
    w.mouse_button(from, true);
    w.mouse_drag(from, to, 8);
    assert!(w.ui.timeline.keys.preview.is_some(), "shown while dragged");
    assert_eq!(keyed(&w, clip, TimelineChannel::Position), vec![2, 6], "not done yet");
    w.mouse_button(to, false);
    settle(&mut w);
    assert_eq!(keyed(&w, clip, TimelineChannel::Position), vec![4, 6]);
    assert_eq!(w.ui.timeline.keys.picks.iter().map(|p| p.frame).collect::<Vec<_>>(), vec![4], "still picked");
    assert_eq!(w.session.undo().as_deref(), Some("Move Keys"));
    assert_eq!(keyed(&w, clip, TimelineChannel::Position), vec![2, 6]);
    settle(&mut w);
    // held still on a mark: its menu, the mark taken alone; let go, nothing more
    let mark = w.ui.timeline.key_centre(&w.ctx, 0, "Position", Some(6)).expect("its row");
    w.hover(mark);
    w.mouse_button(mark, true);
    for _ in 0..30 {
        w.hover(mark);
    }
    assert!(w.ui.timeline.keys.menu.is_some(), "held: its menu");
    w.mouse_button(mark, false);
    settle(&mut w);
    assert!(w.ui.timeline.keys.menu.is_some(), "still open");
    assert_eq!(w.ui.timeline.keys.picks.iter().map(|p| p.frame).collect::<Vec<_>>(), vec![6]);
}

/// A mark right-clicked opens its menu: Easing's simple easings set how the key eases; Edit
/// Curve… opens the easing editor on it, whose kinds set it.
#[test]
fn the_easing_menu_and_the_easing_editor_set_how_a_key_eases() {
    let Some((mut w, clip, _)) = animated() else { return };
    key_at(&mut w, clip, 2, 0.0, 0.0);
    key_at(&mut w, clip, 6, 80.0, 0.0);
    rows_out(&mut w);
    let mark = w.ui.timeline.key_centre(&w.ctx, 0, "Position", Some(2)).expect("its row");
    right_click(&mut w, mark);
    assert!(w.ui.timeline.keys.menu.is_some(), "its menu");
    let easing = word(&mut w, "Easing");
    w.click(easing);
    settle(&mut w);
    let ease_in = word(&mut w, "Ease in");
    w.click(ease_in);
    settle(&mut w);
    let at = |w: &Window| w.session.timeline_keys(clip).into_iter().find(|k| k.frame == 2).map(|k| k.easing);
    assert_eq!(at(&w), Some(TimelineEasing::EaseIn));
    assert_eq!(w.ui.timeline.keys.easing, TimelineEasing::EaseIn, "new keys ease so too");
    let edit = word(&mut w, "Edit Curve…");
    w.click(edit);
    settle(&mut w);
    let bounce = word(&mut w, "Bounce");
    w.click(bounce);
    settle(&mut w);
    let spec = w.session.timeline_key_easing(clip, 2).expect("its easing");
    assert_eq!(spec.kind, procraft_core::EasingKind::Bounce);
    assert_eq!(w.session.undo().as_deref(), Some("Easing"));
    // Escape closes it
    w.events.push(Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    settle(&mut w);
    assert!(w.ui.timeline.keys.menu.is_none());
    // the menu's Delete Key takes it away
    let mark = w.ui.timeline.key_centre(&w.ctx, 0, "Position", Some(6)).expect("its row");
    right_click(&mut w, mark);
    let delete = word(&mut w, "Delete Key");
    w.click(delete);
    settle(&mut w);
    assert_eq!(keyed(&w, clip, TimelineChannel::Position), vec![2]);
}

/// A row's name clicked picks all its keys; on two frames or more, the grip after them dragged
/// stretches them about the first.
#[test]
fn keys_picked_on_two_frames_stretch_by_their_grip() {
    let Some((mut w, clip, _)) = animated() else { return };
    key_at(&mut w, clip, 2, 0.0, 0.0);
    key_at(&mut w, clip, 4, 80.0, 0.0);
    rows_out(&mut w);
    assert!(w.ui.timeline.grip_centre(&w.ctx, false).is_none(), "nothing picked, no grips");
    let name = w.ui.timeline.key_centre(&w.ctx, 0, "Position", None).expect("its row");
    w.click(name);
    settle(&mut w);
    assert_eq!(w.ui.timeline.keys.picks.len(), 2);
    let grip = w.ui.timeline.grip_centre(&w.ctx, false).expect("the grip after them");
    let to = grip + vec2(2.0 * 44.0, 0.0);
    w.hover(grip);
    w.mouse_button(grip, true);
    w.mouse_drag(grip, to, 8);
    w.mouse_button(to, false);
    settle(&mut w);
    assert_eq!(keyed(&w, clip, TimelineChannel::Position), vec![2, 6], "twice as far apart");
    assert_eq!(w.session.undo().as_deref(), Some("Stretch Keys"));
}

/// Animate turns autokey on and opens the transform tool, whose bar stands above the timeline's:
/// a drag on the box keys the drawing's position on the playhead's frame, and a badge says the
/// frame; its Turn Off turns autokey off, the transform ending with its keys kept.
#[test]
fn animate_keys_a_move_with_the_transform_tool() {
    let Some((mut w, clip, _)) = animated() else { return };
    w.session.rest_timeline_frame(3);
    settle(&mut w);
    button(&mut w, "Animate");
    assert!(w.ui.timeline.animate.autokey);
    assert!(w.ui.transform.on, "the transform tool open");
    assert!(w.session.timeline_autokey_posing(), "keying a pose");
    assert!(!written(&drawn(&mut w), "Keying frame 4").is_empty(), "the badge");
    let bar = crate::timeline::bar_top(&w.ctx).expect("the bar");
    let (x, top) = crate::transform::button_tip(&w.ctx, crate::transform::BUTTON_ROOMY, 0, 7);
    assert!(top < bar && x > 0.0, "the transform's bar stands above the timeline's");
    // (clear of its nodes, in its middle)
    let middle = w.paper().center();
    w.mouse_stroke(middle, middle + vec2(60.0, 0.0), 10);
    settle(&mut w);
    assert!(keyed(&w, clip, TimelineChannel::Position).contains(&3), "{:?}", w.session.timeline_keys(clip));
    let pose = w.session.timeline_pose(clip).expect("its pose");
    assert!(pose.x > 8.0 && pose.y.abs() < 2.0, "moved across: {pose:?}");
    // the playhead moved: the box laid again over the drawing as it is there
    let here = w.ui.transform.shown().0.map(|b| b.corners.clone()).expect("the box");
    button(&mut w, "Back");
    button(&mut w, "Back");
    assert_eq!(frame(&w), 1);
    let there = w.ui.transform.shown().0.map(|b| b.corners.clone()).expect("the box");
    let x = |c: &[f32]| c.first().copied().unwrap_or(0.0);
    assert!(x(&there) < x(&here) - 2.0, "{there:?} against {here:?}");
    let off = word(&mut w, "Turn Off");
    w.click(off);
    settle(&mut w);
    assert!(!w.ui.timeline.animate.autokey);
    assert!(!w.ui.transform.on && !w.session.timeline_autokey_posing());
    assert!(keyed(&w, clip, TimelineChannel::Position).contains(&3), "kept");
}

/// With Animate on, the record button arms a take: a drag on the canvas plays the frames on while
/// the drawing follows, keyed on each and thinned when let go, one "Perform" step; its rows of
/// keys come out. With Shift held, the pointer going round it turns it and sizes it.
#[test]
fn a_perform_take_keys_the_drawing_as_the_pointer_takes_it() {
    let Some((mut w, clip, _)) = animated() else { return };
    w.session.rest_timeline_frame(0);
    settle(&mut w);
    button(&mut w, "Animate");
    button(&mut w, "Record");
    assert!(w.ui.timeline.animate.armed);
    assert!(!written(&drawn(&mut w), "Drag on the canvas to perform the move (Shift: scale and turn)").is_empty());
    let from = w.paper().center();
    w.hover(from);
    w.mouse_button(from, true);
    for i in 1..=30 {
        w.hover(from + vec2(i as f32 * 3.0, i as f32));
    }
    w.mouse_button(from + vec2(90.0, 30.0), false);
    settle(&mut w);
    assert!(!w.ui.timeline.animate.armed, "the take ended");
    let keys = keyed(&w, clip, TimelineChannel::Position);
    assert!(keys.len() >= 2 && keys.iter().all(|f| *f <= 9), "{keys:?}");
    let last = *keys.last().expect("a key");
    w.session.rest_timeline_frame(last);
    settle(&mut w);
    let pose = w.session.timeline_pose(clip).expect("its pose");
    assert!(pose.x > 20.0 && pose.y > 5.0, "{pose:?}");
    assert!(w.ui.timeline.key_rows(&w.ctx).contains(&(0, "Position")), "its rows out");
    assert_eq!(w.session.undo().as_deref(), Some("Perform"), "one step");
    assert!(keyed(&w, clip, TimelineChannel::Position).is_empty());
    // again, with Shift held part of the way: turned and sized
    w.session.rest_timeline_frame(0);
    settle(&mut w);
    button(&mut w, "Record");
    let from = w.paper().center();
    w.hover(from);
    w.mouse_button(from, true);
    for i in 1..=6 {
        w.hover(from + vec2(i as f32 * 5.0, 0.0));
    }
    // round its middle, a quarter turn, and nearer
    let middle = from + vec2(30.0, 0.0);
    w.events.push(Event::ModifiersChanged(Modifiers::SHIFT));
    for i in 0..=20 {
        let a = std::f32::consts::FRAC_PI_2 * i as f32 / 20.0;
        let r = 60.0 - i as f32;
        w.hover(middle + vec2(a.cos(), a.sin()) * r);
    }
    w.events.push(Event::ModifiersChanged(Modifiers::NONE));
    w.mouse_button(middle + vec2(0.0, 40.0), false);
    settle(&mut w);
    assert!(!keyed(&w, clip, TimelineChannel::Rotation).is_empty(), "turned");
    assert!(!keyed(&w, clip, TimelineChannel::Scale).is_empty(), "sized");
}

/// The motion path shows over the canvas while the track's rows of keys are out: a key's dot
/// double-clicked bends the path there, and a handle's dot dragged bends it more, the other
/// handle mirroring it, one undo step.
#[test]
fn the_motion_paths_handles_are_dragged_to_bend_it() {
    let Some((mut w, clip, _)) = animated() else { return };
    key_at(&mut w, clip, 0, 0.0, 0.0);
    key_at(&mut w, clip, 4, 100.0, 0.0);
    key_at(&mut w, clip, 8, 100.0, 80.0);
    rows_out(&mut w);
    let path = w.session.timeline_motion_path(clip).expect("its path");
    let key = path.keys.iter().find(|k| k.frame == 4).expect("the key on frame 5").clone();
    assert!(key.handles.is_none());
    let dot = on_window(&w, key.x, key.y);
    w.click(dot);
    w.click(dot);
    settle(&mut w);
    let bent = w.session.timeline_motion_path(clip).expect("its path");
    let key = bent.keys.iter().find(|k| k.frame == 4).expect("the key").clone();
    let handles = key.handles.clone().expect("bent");
    // the handle toward the key after, dragged 40 down
    let from = on_window(&w, key.x + handles[2], key.y + handles[3]);
    w.mouse_stroke(from, from + vec2(0.0, 40.0), 8);
    settle(&mut w);
    let after = w.session.timeline_motion_path(clip).expect("its path");
    let h = after.keys.iter().find(|k| k.frame == 4).and_then(|k| k.handles.clone()).expect("bent");
    let scale = (on_window(&w, 1.0, 0.0) - on_window(&w, 0.0, 0.0)).length();
    assert!((h[3] - handles[3] - 40.0 / scale).abs() < 3.0, "{h:?} from {handles:?}");
    assert!((h[0] + h[2]).abs() < 0.01 && (h[1] + h[3]).abs() < 0.01, "mirrored: {h:?}");
    assert_eq!(w.session.undo().as_deref(), Some("Motion Path"));
}

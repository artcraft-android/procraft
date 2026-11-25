//! Autokey (a keyframe mode): with it on, the transform tool on a timeline moves, scales and
//! turns the drawing at the playhead by keying its clip's pose there, rather than moving its
//! pixels.
//!
//! The transform's box is laid around what the drawing paints, where its pose (and the groups it
//! is in) shows it at the playhead. Each step of a drag keys the channels the drag changed (a move
//! Position, a corner or edge Scale, the rotate node Rotation) at the playhead, shown at once; the
//! drag let go, or a toolbar change made, is one "Key" undo step. A channel's first key on a frame
//! after the clip's first also keys its first frame with the value it had, so what led up to the
//! playhead stays as it was. The first key that moves, scales or turns the clip sets what it turns
//! about to the middle of what it paints, as the Keys page does.
//!
//! With a group or video clip picked in the panel and playing at the playhead, it keys that clip's
//! pose instead: the box laid around what the group shows there (each drawing in it as its own
//! keys pose it) or the video's picture. With autokey on, the clip page's opacity keys the clip's
//! opacity at the playhead rather than setting its own. An adjustment clip's values have no
//! gesture of the transform's to key them by: they stay keyed from the Keys page.
//!
//! A video on the track drawn on, at the playhead, needs no picking: it is what is there to key
//! (a group there too). And a video has no pixels of the canvas's to move, so the transform tool
//! poses it with autokey off as well, for its whole length: a channel that isn't animated yet
//! (no key, or one) gets its one key on the clip's first frame, so the video sits where it was
//! put on every frame; a channel with two keys or more is keyed at the playhead, as with
//! autokey on.
//!
//! Distort and Warp act as Freeform (a pose can't bend), turning the box alone does nothing, and
//! Pixelated is let be. The transform's undo takes back the keys it made, one step at a time.
//! Ending the transform keeps them; cancelling it puts back every keyed clip's keys as they were
//! as it began, as one more step. With a selection, the transform moves the selected pixels as
//! ever.

use procraft_engine::animation::FrameLook;
use procraft_engine::document::LayerId;
use procraft_engine::geom::{Affine, IRect, Vec2};
use procraft_engine::timeline::{Channel, ClipContent, ClipId, ClipKeys, Easing, Timeline};
use procraft_engine::transform::{Transform, TransformMode};

use super::{TimelineEasing, easing_at};
use crate::transform::{TransformBox, corners_affine};
use crate::{CanvasSession, State};

/// The undo step a keyed drag makes.
const STEP: &str = "Key";
/// How much a channel must change to be keyed: canvas pixels moved, scale, radians turned.
const MOVED: f32 = 0.01;
const SCALED: f32 = 0.0005;
const TURNED: f32 = 0.0005;
/// The scales a key keeps within, either way.
const LEAST_SCALE: f32 = 0.01;
const MOST_SCALE: f32 = 100.0;

/// Autokey on or off, how its new keys ease, the clip picked in the panel, and the clip an open
/// transform keys.
pub(crate) struct Autokey {
    on: bool,
    easing: TimelineEasing,
    /// The clip picked in the panel: a group or video there is keyed rather than the drawing.
    picked: Option<ClipId>,
    posing: Option<Posing>,
    /// Whether a step was recorded since the apps last asked.
    keyed: bool,
}

impl Default for Autokey {
    fn default() -> Autokey {
        Autokey { on: false, easing: TimelineEasing::Smooth, picked: None, posing: None, keyed: false }
    }
}

/// The clip a transform keys, and what it was keyed from.
struct Posing {
    clip: ClipId,
    /// The clip's own frame keyed.
    local: i32,
    /// How the groups the clip is in place it on the canvas (identity in none).
    groups: Affine,
    /// What it scales and turns about.
    pivot: [f32; 2],
    /// Its keys before the drag under way.
    keys: ClipKeys,
    /// The turn last keyed or shown, which the next is unwound from.
    turn: f32,
    /// Each clip keyed in this transform, with its keys as it began.
    began: Vec<(ClipId, ClipKeys)>,
    /// Steps recorded in this transform, which its undo takes back.
    steps: u32,
    /// Placed for its whole length (a video's, with autokey off): a channel not yet animated
    /// keeps its one key on the clip's first frame.
    whole: bool,
}

/// The clip a transform keys at the playhead: the group or video picked, or the current layer's
/// drawing.
struct Target {
    clip: ClipId,
    what: What,
    local: i32,
}

/// What a keyed clip shows, for the box laid around it.
enum What {
    Drawing(LayerId),
    Group,
    /// Its picture's place on the canvas: left, top, width, height.
    Video([f32; 4]),
}

/// Why autokey can't key at the playhead.
enum Refused {
    Playing,
    NoDrawing,
    Locked,
}

/// A pose as keys hold it: moved, scaled, turned (radians).
#[derive(Clone, Copy, Debug, PartialEq)]
struct Pose {
    at: [f32; 2],
    scale: [f32; 2],
    turn: f32,
}

impl State {
    /// Whether the open transform keys a clip's pose.
    pub(crate) fn posing(&self) -> bool {
        self.autokey.posing.is_some()
    }

    /// How the box's handles act: Distort and Warp as Freeform while keying a pose.
    pub(crate) fn posed_mode(&self) -> TransformMode {
        match self.transform_mode {
            TransformMode::Distort | TransformMode::Warp | TransformMode::Puppet if self.posing() => {
                TransformMode::Freeform
            }
            m => m,
        }
    }

    /// With autokey on, on a timeline and nothing selected: the transform opened over the drawing at
    /// the playhead, to key its pose (its box; none if there is nothing there to key). With it
    /// off, the same over a video at the playhead on the track drawn on, to place it for its
    /// whole length. None when neither applies, and the pixels are transformed.
    pub(crate) fn autokey_begin(&mut self) -> Option<Option<TransformBox>> {
        if self.doc.timeline.is_none() || self.selection.is_some() {
            return None;
        }
        let target = match self.autokey_target() {
            Ok(t) => t,
            Err(_) if self.autokey.on => return Some(None),
            // (autokey off, and no video there to place: the pixels are transformed)
            Err(_) => return None,
        };
        // (a stroke just ended is on its layer before what it paints is measured)
        self.flush();
        let Some([x, y, w, h]) = self.target_box(&target) else { return Some(None) };
        let Some(posing) = self.posing_of(&target, [x + w / 2.0, y + h / 2.0], Vec::new(), 0) else {
            return Some(None);
        };
        self.transform_items = None;
        self.vector_transform = None;
        self.snap_pending = None;
        self.transform = Some(Transform::new(x, y, w, h, box_hull([x, y, w, h])));
        self.autokey.posing = Some(posing);
        self.place_box();
        Some(self.transform_box())
    }

    /// Whether the current layer's drawing shows at the playhead (always, off a timeline). Where
    /// the track drawn on has nothing at the playhead, the current layer is still the last
    /// frame's drawn on: a transform would move the pixels of a frame that isn't the one shown.
    pub(crate) fn current_shows_here(&self) -> bool {
        let Some(tl) = self.doc.timeline.as_ref() else { return true };
        let frame = self.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let drawing = self.doc.top_of(self.doc.current()).and_then(|root| tl.drawing_of(root));
        let Some((_, clip)) = drawing else { return true };
        let duration = tl.clip(clip).map_or(0, |c| c.duration);
        tl.local_frame(clip, frame).is_some_and(|l| (0..i64::from(duration)).contains(&l))
    }

    /// Why autokey opens no transform here, as the message says it; none if it isn't on, or would.
    pub(crate) fn autokey_refusal(&mut self) -> Option<String> {
        if !self.autokey.on || self.doc.timeline.is_none() || self.selection.is_some() {
            return None;
        }
        // (a stroke just ended is on its layer before what it paints is measured)
        self.flush();
        let why = match self.autokey_target() {
            Err(Refused::Playing) => "Stop playing to key",
            Err(Refused::NoDrawing) => "No drawing at the playhead to key",
            Err(Refused::Locked) => "The track is locked",
            Ok(t) if self.target_box(&t).is_none() => match t.what {
                What::Drawing(_) => "This drawing is empty",
                What::Group => "This group shows nothing here",
                What::Video(_) => "This video has no picture",
            },
            Ok(_) => return None,
        };
        Some(why.to_owned())
    }

    /// The group or video picked in the panel if it plays at the playhead, or the one on the
    /// track drawn on there; else the drawing at the playhead on the track drawn on, made
    /// current: as long as it can be keyed. With autokey off, only a video on the track drawn on.
    fn autokey_target(&mut self) -> Result<Target, Refused> {
        if self.canvas.as_ref().is_some_and(|c| c.play_frame().is_some()) {
            return Err(Refused::Playing);
        }
        let at = self.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let on = self.autokey.on;
        let picked = self.autokey.picked.filter(|_| on);
        if let Some(t) = picked.and_then(|id| picked_target(self.doc.timeline.as_ref()?, id, at)) {
            return t;
        }
        // the track drawn on shows a video at the playhead (with autokey on, a group too): that
        // is what is there to key, picked or not
        let there = self.drawn_track().and_then(|track| {
            let tl = self.doc.timeline.as_ref()?;
            picked_target(tl, super::clip_on(tl, track, at)?.id, at)
        });
        match there {
            Some(Ok(t)) if on || matches!(t.what, What::Video(_)) => return Ok(t),
            Some(Err(why)) if on => return Err(why),
            _ => {}
        }
        if !on {
            return Err(Refused::NoDrawing);
        }
        self.follow_playhead();
        let frame = self.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let tl = self.doc.timeline.as_ref().ok_or(Refused::NoDrawing)?;
        let root = self.doc.top_of(self.doc.current()).ok_or(Refused::NoDrawing)?;
        let (track, clip) = tl.drawing_of(root).ok_or(Refused::NoDrawing)?;
        if self.drawn_track() != Some(track) {
            return Err(Refused::NoDrawing);
        }
        let duration = tl.clip(clip).map_or(0, |c| c.duration);
        let local = tl.local_frame(clip, frame).filter(|l| (0..i64::from(duration)).contains(l));
        let local = local.and_then(|l| i32::try_from(l).ok()).ok_or(Refused::NoDrawing)?;
        if tl.track(track).is_some_and(|t| t.locked) {
            return Err(Refused::Locked);
        }
        Ok(Target { clip, what: What::Drawing(root), local })
    }

    /// What `target` shows at the frame keyed, in its own place (before its keys pose it): x, y,
    /// width, height in canvas pixels; none if nothing.
    fn target_box(&self, target: &Target) -> Option<[f32; 4]> {
        match target.what {
            What::Drawing(root) => self.drawing_box(root),
            What::Video(rect) => (rect[2] > 0.0 && rect[3] > 0.0).then_some(rect),
            What::Group => self.group_box(target.clip, target.local),
        }
    }

    /// What group clip `id` shows at its frame `local`: each drawing in it where its own keys
    /// (and the groups between) put it, taken back out of the group's own pose and those around
    /// it.
    fn group_box(&self, id: ClipId, local: i32) -> Option<[f32; 4]> {
        let tl = self.doc.timeline.as_ref()?;
        let frame = self.canvas.as_ref()?.timeline_frame();
        let group = tl.clip(id)?;
        let roots = group.roots();
        let own = group.keys.pose_at(local as f32).unwrap_or(Affine::IDENTITY);
        let around = tl.groups_pose(id, local as f32).unwrap_or(Affine::IDENTITY);
        let back = own.then(around).inverse()?;
        let mut corners = Vec::new();
        for s in tl.showing(frame, false) {
            if !roots.contains(&s.root) || !matches!(s.look, FrameLook::Shown) {
                continue;
            }
            let Some(b) = self.drawing_box(s.root) else { continue };
            let to = s.pose.unwrap_or(Affine::IDENTITY).then(back);
            corners.extend(box_hull(b).into_iter().map(|p| to.apply(p)));
        }
        bounds(&corners)
    }

    /// What drawing `root` paints: x, y, width, height in canvas pixels; none if nothing.
    fn drawing_box(&self, root: LayerId) -> Option<[f32; 4]> {
        let (canvas, gpu) = (self.canvas.as_ref()?, self.gpu.as_ref()?);
        let layers: Vec<LayerId> =
            self.doc.pixel_layer_ids().filter(|l| *l == root || self.doc.is_inside(*l, root)).collect();
        let b = canvas.painted_boxes(gpu, &layers).into_iter().flatten().reduce(IRect::union)?;
        (!b.is_empty()).then(|| [b.x0 as f32, b.y0 as f32, b.width() as f32, b.height() as f32])
    }

    /// Keying `target` from its keys now, turning about `pivot` if it has no pivot of its own.
    fn posing_of(
        &self,
        target: &Target,
        pivot: [f32; 2],
        mut began: Vec<(ClipId, ClipKeys)>,
        steps: u32,
    ) -> Option<Posing> {
        let tl = self.doc.timeline.as_ref()?;
        let keys = tl.clip(target.clip)?.keys.clone();
        let groups = tl.groups_pose(target.clip, target.local as f32).unwrap_or(Affine::IDENTITY);
        if !began.iter().any(|(c, _)| *c == target.clip) {
            began.push((target.clip, keys.clone()));
        }
        let turn = keys.rotation.value_at(target.local as f32).unwrap_or(0.0);
        Some(Posing {
            clip: target.clip,
            local: target.local,
            groups,
            pivot: keys.pivot.unwrap_or(pivot),
            keys,
            turn,
            began,
            steps,
            whole: !self.autokey.on,
        })
    }

    /// The box put where the clip's keys and groups show it at the frame keyed.
    fn place_box(&mut self) {
        let Some(p) = self.autokey.posing.as_ref() else { return };
        let keys = self.doc.timeline.as_ref().and_then(|tl| tl.clip(p.clip)).map(|c| &c.keys);
        let pose = keys.and_then(|k| k.pose_at(p.local as f32)).unwrap_or(Affine::IDENTITY);
        let shown = pose.then(p.groups);
        if let Some(t) = self.transform.as_mut() {
            t.place(shown);
        }
    }

    /// The box laid again over the drawing at the playhead, its keys as they are now (after the
    /// playhead moved, another drawing was made current, or an undo): unless a drag is under way,
    /// or there is nothing at the playhead to key.
    pub(crate) fn autokey_anchor(&mut self) {
        let Some(p) = self.autokey.posing.as_ref() else { return };
        if self.transform.as_ref().is_none_or(Transform::dragging) {
            return;
        }
        let (clip, began, steps, pivot) = (p.clip, p.began.clone(), p.steps, p.pivot);
        let Ok(target) = self.autokey_target() else { return };
        if target.clip != clip {
            // another clip: the box laid around what it shows
            self.flush();
            let Some([x, y, w, h]) = self.target_box(&target) else { return };
            let Some(posing) = self.posing_of(&target, [x + w / 2.0, y + h / 2.0], began, steps) else { return };
            self.transform = Some(Transform::new(x, y, w, h, box_hull([x, y, w, h])));
            self.autokey.posing = Some(posing);
        } else if let Some(posing) = self.posing_of(&target, pivot, began, steps) {
            self.autokey.posing = Some(posing);
        }
        self.place_box();
    }

    /// Keys the pose the box shows now at the frame keyed (the changed channels only), shown at
    /// once; with no drag under way, as an undo step.
    pub(crate) fn autokey_show(&mut self) {
        let (Some(t), Some(p)) = (self.transform.as_ref(), self.autokey.posing.as_mut()) else { return };
        let dragging = t.dragging();
        let Some(shown) = corners_affine(t.source(), t.content()) else { return };
        let Some(pose) = p.groups.inverse().map(|g| shown.then(g)) else { return };
        let Some(now) = decompose(pose, p.pivot, p.turn) else { return };
        p.turn = now.turn;
        let keys = keyed(&p.keys, p.local, p.pivot, now, self.autokey.easing.easing(), p.whole);
        let clip = p.clip;
        let Some(c) = self.doc.timeline.as_ref().and_then(|tl| tl.clip(clip)) else { return };
        if c.keys != keys {
            if self.pending_doc.is_none() {
                self.pending_doc = Some(self.doc.clone());
                self.pending_name = Some(STEP.to_owned());
            }
            if let Some(c) = self.doc.timeline.as_mut().and_then(|tl| tl.clip_mut(clip)) {
                c.keys = keys;
            }
            if let Some(canvas) = self.canvas.as_mut() {
                canvas.invalidate();
            }
            self.revision += 1;
        }
        // the box shows what was keyed (a pose doesn't skew)
        self.place_box();
        if !dragging {
            self.autokey_settle(true);
        }
    }

    /// The drag under way is over: what it keyed is kept as one undo step (`keep`), or put back.
    pub(crate) fn autokey_settle(&mut self, keep: bool) {
        let Some(before) = self.pending_doc.take() else { return };
        let name = self.pending_name.take();
        if !keep {
            self.doc = before;
            if let Some(c) = self.canvas.as_mut() {
                c.invalidate();
            }
            self.revision += 1;
            return;
        }
        let Some(p) = self.autokey.posing.as_mut() else { return };
        let now = self.doc.timeline.as_ref().and_then(|tl| tl.clip(p.clip)).map(|c| c.keys.clone());
        let Some(now) = now else { return };
        if now == p.keys {
            return;
        }
        p.keys = now;
        p.steps += 1;
        self.autokey.keyed = true;
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.name_next_step(name.as_deref().unwrap_or(STEP));
            canvas.record_document(before);
        }
    }

    /// The transform ends: what it keyed kept (`commit`), or every clip it keyed put back as it
    /// was as it began, as one more step.
    pub(crate) fn autokey_end(&mut self, commit: bool) {
        self.autokey_settle(commit);
        if let Some(t) = self.transform.as_mut() {
            t.cancel_drag();
        }
        self.transform = None;
        self.snap_pending = None;
        let Some(p) = self.autokey.posing.take() else { return };
        if commit || p.steps == 0 {
            return;
        }
        let before = self.doc.clone();
        let Some(tl) = self.doc.timeline.as_mut() else { return };
        let mut changed = false;
        for (clip, keys) in p.began {
            if let Some(c) = tl.clip_mut(clip)
                && c.keys != keys
            {
                c.keys = keys;
                changed = true;
            }
        }
        if changed && let Some(canvas) = self.canvas.as_mut() {
            canvas.name_next_step(STEP);
            canvas.record_document(before);
            canvas.invalidate();
            self.revision += 1;
            self.autokey.keyed = true;
        }
    }

    /// Turning the box alone does nothing to a pose.
    pub(crate) fn autokey_ignores(&self, handle: crate::TransformHandle) -> bool {
        self.posing() && handle == crate::TransformHandle::TurnBox
    }
}

/// `pose` (scaled and turned about `pivot`, then moved) taken apart into what keys hold, the turn
/// unwound to the one nearest `near`; none if it flattens everything.
fn decompose(pose: Affine, pivot: [f32; 2], near: f32) -> Option<Pose> {
    let (a, b, c, d) = (pose.a, pose.b, pose.c, pose.d);
    let sx = a.hypot(b);
    if !(sx > 1e-6 && sx.is_finite()) {
        return None;
    }
    let turn = b.atan2(a);
    let sy = -c * turn.sin() + d * turn.cos();
    // mirrored across, it is the same as turned half round and mirrored down: the one whose turn
    // is nearer the last is taken
    let tau = std::f32::consts::TAU;
    let unwound = |t: f32| t + ((near - t) / tau).round() * tau;
    let (one, other) = (unwound(turn), unwound(turn + std::f32::consts::PI));
    let (turn, sx, sy) = if (one - near).abs() <= (other - near).abs() { (one, sx, sy) } else { (other, -sx, -sy) };
    let p = Vec2::new(pivot[0], pivot[1]);
    let moved = pose.apply(p) - p;
    let within = |s: f32| s.signum() * s.abs().clamp(LEAST_SCALE, MOST_SCALE);
    Some(Pose { at: [moved.x, moved.y], scale: [within(sx), within(sy)], turn })
}

/// `keys` with the channels `now` changes keyed at clip frame `local` (new keys easing as
/// `easing`), each channel's first key after the clip's start keying its start too. With `whole`,
/// a channel with no key or one is given its one key on the clip's first frame instead, so the
/// clip shows as `now` for its whole length.
fn keyed(keys: &ClipKeys, local: i32, pivot: [f32; 2], now: Pose, easing: Easing, whole: bool) -> ClipKeys {
    let at = local as f32;
    let was = Pose {
        at: keys.position.position_at(at).unwrap_or([0.0; 2]),
        scale: keys.scale.value_at(at).unwrap_or([1.0; 2]),
        turn: keys.rotation.value_at(at).unwrap_or(0.0),
    };
    let far = |a: [f32; 2], b: [f32; 2], by: f32| (a[0] - b[0]).abs() > by || (a[1] - b[1]).abs() > by;
    let mut out = keys.clone();
    if far(now.at, was.at, MOVED) {
        place_channel(&mut out.position, local, was.at, now.at, &easing, whole);
    }
    if far(now.scale, was.scale, SCALED) {
        place_channel(&mut out.scale, local, was.scale, now.scale, &easing, whole);
    }
    if (now.turn - was.turn).abs() > TURNED {
        place_channel(&mut out.rotation, local, was.turn, now.turn, &easing, whole);
    }
    if out.pivot.is_none() && out.posed() {
        out.pivot = Some(pivot);
    }
    out
}

/// As [`key_channel`]; but with `whole`, a channel not yet animated (no key, or one) holds `now`
/// in one key on the clip's first frame.
fn place_channel<T: Clone>(ch: &mut Channel<T>, local: i32, was: T, now: T, easing: &Easing, whole: bool) {
    if whole && ch.keys.len() <= 1 {
        ch.keys.clear();
        ch.set(0, now, easing.clone());
    } else {
        key_channel(ch, local, was, now, easing);
    }
}

/// Channel `ch` keyed `now` at clip frame `local` (a key there keeps how it eases; a new one
/// eases as `easing`), its start keyed `was` too if this is its first key and not on the start.
fn key_channel<T: Clone>(ch: &mut Channel<T>, local: i32, was: T, now: T, easing: &Easing) {
    let e = easing_at(&ch.keys, local).unwrap_or_else(|| easing.clone());
    if ch.keys.is_empty() && local > 0 {
        ch.set(0, was, easing.clone());
    }
    ch.set(local, now, e);
}

/// Group or video clip `id`, picked in the panel, as autokey's target if it plays at timeline frame
/// `frame`; none if it isn't one, or isn't there (the drawing is keyed then).
fn picked_target(tl: &Timeline, id: ClipId, frame: u32) -> Option<Result<Target, Refused>> {
    let clip = tl.clip(id)?;
    let what = match &clip.content {
        ClipContent::Group(_) => What::Group,
        ClipContent::Video(v) => What::Video(v.rect),
        ClipContent::Drawing(_) | ClipContent::Audio(_) => return None,
    };
    let local = tl.local_frame(id, frame).filter(|l| (0..i64::from(clip.duration)).contains(l))?;
    let local = i32::try_from(local).ok()?;
    let track = tl.track_of_clip(id)?;
    if tl.track(track).is_none_or(|t| t.locked) {
        return Some(Err(Refused::Locked));
    }
    Some(Ok(Target { clip: id, what, local }))
}

/// Clip `id`'s own frame at timeline frame `frame`, if it is there and its track isn't locked.
fn clip_local(tl: &Timeline, id: ClipId, frame: u32) -> Option<i32> {
    let duration = tl.clip(id)?.duration;
    let local = tl.local_frame(id, frame).filter(|l| (0..i64::from(duration)).contains(l))?;
    let track = tl.track_of_clip(id)?;
    if tl.track(track).is_none_or(|t| t.locked) {
        return None;
    }
    i32::try_from(local).ok()
}

/// The box around `points`: x, y, width, height; none if they span nothing.
fn bounds(points: &[Vec2]) -> Option<[f32; 4]> {
    let first = points.first()?;
    let (mut x0, mut y0, mut x1, mut y1) = (first.x, first.y, first.x, first.y);
    for p in points {
        x0 = x0.min(p.x);
        y0 = y0.min(p.y);
        x1 = x1.max(p.x);
        y1 = y1.max(p.y);
    }
    let (w, h) = (x1 - x0, y1 - y0);
    (w > 0.0 && h > 0.0 && w.is_finite() && h.is_finite()).then_some([x0, y0, w, h])
}

/// The corners of the rectangle `r` (x, y, width, height).
fn box_hull([x, y, w, h]: [f32; 4]) -> Vec<Vec2> {
    vec![Vec2::new(x, y), Vec2::new(x + w, y), Vec2::new(x + w, y + h), Vec2::new(x, y + h)]
}

#[uniffi::export]
impl CanvasSession {
    /// Autokey on or off (see `timeline::autokey`): with it on, a transform opened on a timeline
    /// keys the pose of the drawing at the playhead, its new keys easing as `easing`. Turned off,
    /// an open transform keying a pose ends, keeping its keys.
    pub fn set_timeline_autokey(&self, on: bool, easing: TimelineEasing) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.autokey.on = on;
        st.autokey.easing = easing;
        // (a video being placed for its whole length is keyed at the playhead from here on)
        if on && let Some(p) = st.autokey.posing.as_mut() {
            p.whole = false;
        }
        if !on && st.posing() {
            st.end_transform(true);
            st.revision += 1;
            self.shared.frame(st);
        }
    }

    /// The clip picked in the panel (none: no clip): a group or video one, playing at the
    /// playhead, is what autokey keys rather than the drawing there; an open transform keying a
    /// pose is laid over it at once.
    pub fn set_timeline_autokey_clip(&self, id: Option<u64>) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let id = id.map(ClipId);
        if st.autokey.picked == id {
            return;
        }
        st.autokey.picked = id;
        if st.posing() {
            st.autokey_anchor();
            self.shared.frame(st);
        }
    }

    /// How autokey's new keys ease (the Keys page's choice), taken up at once.
    pub fn set_timeline_autokey_easing(&self, easing: TimelineEasing) {
        self.shared.lock().autokey.easing = easing;
    }

    /// With autokey on, clip `id`'s opacity (0..1) keyed at the playhead rather than set as its
    /// own: shown at once, one "Key Opacity" undo step when `done`. Its first key after the clip's
    /// start keys the start too, with the opacity it had. False if autokey is off, the clip isn't
    /// at the playhead, or its track is locked.
    pub fn autokey_timeline_clip_opacity(&self, id: u64, opacity: f32, done: bool) -> bool {
        let opacity = if opacity.is_finite() { opacity.clamp(0.0, 1.0) } else { 1.0 };
        let (frame, easing) = {
            let st = self.shared.lock();
            if !st.autokey.on {
                return false;
            }
            (st.canvas.as_ref().map_or(0, |c| c.timeline_frame()), st.autokey.easing.easing())
        };
        self.change_live("Key Opacity", done, |d| {
            let Some(tl) = d.timeline.as_mut() else { return false };
            let id = ClipId(id);
            let Some(local) = clip_local(tl, id, frame) else { return false };
            let Some(c) = tl.clip_mut(id) else { return false };
            let was = c.keys.opacity_at(local as f32).unwrap_or(c.opacity);
            let before = c.keys.opacity.clone();
            key_channel(&mut c.keys.opacity, local, was, opacity, &easing);
            c.keys.opacity != before
        })
    }

    /// Clip `id`'s opacity as it shows at the playhead (keyed, or its own); 1 if there is no such
    /// clip.
    pub fn timeline_clip_opacity_here(&self, id: u64) -> f32 {
        let st = self.shared.lock();
        let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(tl) = st.doc.timeline.as_ref() else { return 1.0 };
        let id = ClipId(id);
        let Some(c) = tl.clip(id) else { return 1.0 };
        let last = i64::from(c.duration.max(1)) - 1;
        let local = tl.local_frame(id, frame).unwrap_or(0).clamp(0, last);
        c.keys.opacity_at(local as f32).unwrap_or(c.opacity)
    }

    /// Whether the open transform keys a clip's pose rather than moving pixels.
    pub fn timeline_autokey_posing(&self) -> bool {
        self.shared.lock().posing()
    }

    /// The open transform's box laid again over the drawing at the playhead, as its keys now show
    /// it (after the playhead moved, or an undo); none if no transform keys a pose.
    pub fn timeline_autokey_box(&self) -> Option<TransformBox> {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if !st.posing() {
            return None;
        }
        st.autokey_anchor();
        self.shared.frame(st);
        st.transform_box()
    }

    /// Whether a transform keying a pose has made an undo step since this was last asked (the
    /// panel then reads its keys again).
    pub fn timeline_autokey_keyed(&self) -> bool {
        std::mem::take(&mut self.shared.lock().autokey.keyed)
    }
}

impl CanvasSession {
    /// The transform's undo while keying a pose: the last step it keyed taken back (none left,
    /// nothing), the box laid again. False if no transform keys a pose.
    pub(crate) fn autokey_undo(&self) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        if !st.posing() {
            return false;
        }
        st.autokey_settle(true);
        let steps = st.autokey.posing.as_ref().map_or(0, |p| p.steps);
        if steps > 0 {
            if let Some(c) = st.canvas.as_mut() {
                c.undo();
            }
            st.flush();
            if let Some(c) = st.canvas.as_mut() {
                c.invalidate();
            }
            if let Some(t) = st.transform.as_mut() {
                t.undo();
            }
            if let Some(p) = st.autokey.posing.as_mut() {
                p.steps -= 1;
            }
            st.revision += 1;
            st.autokey.keyed = true;
            st.autokey_anchor();
        }
        self.shared.frame(st);
        true
    }
}

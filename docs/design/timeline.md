# Timeline design

Status: proposed, Nov 14, 2026. The data model is in `crates/engine/src/timeline.rs` (compiles, tested, not yet part of `Document`).

Make2D's timeline is Koi Studio's Timeline v2 rebuilt on ProCraft's core. This document says what the model is, where it hooks into what already exists, and the order to build it in.

## Goals

- Tracks of clips, as in Koi: a drawing, a group, a video or a sound is a clip with a start and a length. Track order is draw order.
- Every clip carries its own keys (position, scale, rotation, opacity, effect parameters), counted from the clip's own first frame, so a clip can be moved or duplicated without breaking its motion.
- Koi's full easing set (edited in a small easing editor window, not a graph editor), motion paths with Bézier handles, a keyed camera, group clips that cycle, clip mattes, retiming and sync markers.
- A Make2D-native look throughout: ProCraft's visual language, not Koi's.
- Keep everything ProCraft does with pixels: textures, parking, undo, the file format, the brush engine.
- Open any existing ProCraft or Make2D canvas, including Animation Assist ones, without losing anything.

Not in this phase: adjustment layers (phase 4; effects stay ProCraft's own suite, with no per-layer effect stack), audio and video decoding (phase 3).

Left out on purpose (Nov 14, 2026 scope review): a timeline graph editor, stretching clips, scene split and splitting a project, vector in-betweening, and anything else niche. See the gap map for the full list.

## The one big decision: drawings are layers you already have

In Koi, each drawing (a cel) has its own little layer stack, separate from the document's layers. ProCraft has one layer tree, with pixels on the GPU keyed by `LayerId`, least-used layers parked compressed off the GPU, and undo that copies the document structure.

**Make2D keeps the one layer tree.** A drawing clip names a top-level layer or group in it (`Drawing::root`). The timeline only says when and how each one shows.

Why:

- Textures, parking, undo, saving, layer blend modes, clipping masks, alpha lock, text and vector layers all keep working on drawings with no new code.
- It's exactly how Animation Assist already works (top-level layers and groups are the frames), so migrating is a re-labelling, not a conversion.
- Parking is what makes long animations fit: 240 drawings at 4K is far beyond GPU memory, but only the few near the playhead need textures.

What changes for the layer tree:

- With a timeline, every top-level layer or group belongs to exactly one drawing clip. Nothing else is top-level.
- The layers panel shows the layers of the drawing under the playhead on the selected track, not the whole tree.
- New drawings are new top-level groups; deleting a clip deletes its group (one undo step).

The alternative, a separate per-drawing layer store like Koi's, would mean a second layer system next to ProCraft's, rewriting compositing, parking, undo and the file format for it. That's the single most expensive way to do this, for no gain the user would see.

## Model

`Document` gets one field, `timeline: Option<Timeline>` (serde default). `None` is a still canvas, exactly as today.

```
Timeline
├─ fps (1–120), duration (frames), exposure (frames per new drawing)
├─ simple: Option<TrackId>       the track shown as a flipbook; None, all tracks
├─ tracks: [Track]              bottom first, like layers
├─ camera: Channel<CameraPose>   center, zoom, rotation
├─ playback, loop_range, onion skins
├─ media: [Media]               sounds and videos in the package
└─ next_track, next_clip         id counters

Track: id, name, hidden, locked, opacity, clips (time order, no overlaps)

Clip: id, name, start, duration, opacity, blend, mask, adjustment, disabled, label,
      keys: ClipKeys, content:
        Drawing { root: LayerId }
        Group   { tracks, collapsed, cycle: { length, Repeat | ContinueAcross } }
        Video   { media, trim_start, rect }
        Audio   { media, trim_start, offset, volume, muted, volume_keys }

ClipKeys: position, scale, rotation, opacity (Channel each), pivot, effects
Channel<T>: keys sorted by frame;  Key { frame, value, easing, path handles }
Easing: Linear, Hold, Smooth, CubicBezier, Curve, Bounce, Elastic, Steps,
        ElasticSteps, Cyclic, Random, Fitted(easing, repeat), Reversed(easing)
```

Rules, all from Koi:

- **An exposure is a clip.** A drawing held for 3 frames is a drawing clip 3 frames long. An exposure sheet is a run of drawing clips on one track.
- **Keys are clip-relative.** Key 0 is the clip's first frame. Moving a clip moves nothing else; trimming its start shifts its keys so the motion stays put on screen.
- **Masks look one track down.** A clip's mask mode uses whatever clip is on the track directly below at the same frame. With no clip there, the mask does nothing, so moving the clip below never destroys anything. Alpha and luminance mattes stop their source from drawing on its own.
- **Groups have their own time.** Clips inside a group count frames from the group's start. A cycling group plays its first `length` frames again and again; its own keys either repeat each cycle or run on across.
- **Rotation never wraps** (4π is two full turns, so keyed spins play as authored). The camera's rotation does take the short way, since it comes from turning the view.
- **Edge drags** take one of three policies: clamp at the next clip, push everything after along, or trim into the neighbour.

## Sampling a frame

The renderer never walks the model directly. The engine turns a frame number into a `FramePlan` (`Timeline::plan`): a bottom-first list of what to draw.

```
FramePlan { items: [PlanItem] }
PlanItem {
  clip, track,
  shows: Drawing(LayerId)          the drawing's layer subtree
       | Group([PlanItem])         a group's own tracks at its own frame
       | Video { media, seconds },
  local,                           the clip's own frame
  opacity, blend,                  clip opacity times track opacity
  mask: Option<(mode, index)>,     the item on the track just below, when there is one
  matte_only: bool,                consumed by the clip above as a matte
}
Added in 1d: pose: Option<Affine> (None with no pose keys, the fast path), keyed opacity.
Phase 4: resolved effect parameters.
```

Groups stay nested in the plan, so the renderer can composite a group as one unit before blending it, with cycles already applied to their frame. Hidden tracks, disabled clips, adjustment clips (until phase 4) and audio draw nothing. This mirrors Koi's `TimelineSampler`, and the plan is what tests assert on: "at frame N, this is what's on screen".

The plan is pure engine code with no GPU, so the easing maths, groups, cycles and mattes are all unit-testable.

## Rendering

`gpu::Canvas` already composites a layer subtree (groups are composited on their own when they don't pass through). For a plan:

1. For each `Placed`, composite its subtree into a scratch texture (or use the layer texture directly for a single unposed layer).
2. Draw that with its pose, opacity and blend into the frame. Posed drawing reuses the affine sampling from `transform.wgsl`.
3. Mattes: render the source into a matte texture instead of the frame, then multiply the clip above by its alpha or luminance.
4. Apply the camera when exporting or when the camera view is on.

As built (1f): `Canvas::composite_timeline` walks the plan itself rather than a flattened list. Each depth of group clip has a *reel* of scratch textures (made only when first wanted): a pair a group's own tracks composite onto before the group is laid down as one (posed, faded and blended by its clip), a texture a masked clip is drawn alone on, and two matte textures (a masked clip can be the matte of the one above too). A matte source is drawn alone, at its opacity and pose, onto a matte texture; the clip above is drawn alone and then through it (the layer shader's clip alpha for clipping and alpha mattes, its mask lightness for luma mattes). With the camera on, the whole frame composites onto the top reel's pair first, then goes through the camera's affine onto the canvas, the background staying put under it.

The camera's frame is the canvas's size. `CameraPose::to_frame` maps the canvas into it (the center to the frame's middle, zoomed and turned about it); `Timeline::camera_at` eases its keys with the rotation unwound key to key, so it always turns the short way. A clip inside a group keeps its frames on the group's level: the core API reports and takes timeline frames (adding the groups' starts, `Timeline::origin`), and drawing at the playhead goes through each group's start and cycle (`Timeline::track_frame`, `local_frame`). Ungrouping lets go of the group's own keys and cycle.

Onion skins become "the drawings before and after on the selected track", reusing today's `FrameLook::Onion` tinting.

A per-frame composite cache (keyed by frame and document revision) makes scrubbing cheap. Koi's `StackCache` is the reference for invalidation.

## Editing and undo

`Document` is cloned for undo already, so timeline edits are undoable for free. Each user action is one step: moving a clip, setting a key, or adding a drawing (a new group plus its clip).

Drawing on the canvas paints into the drawing under the playhead on the selected track. If there isn't one there, the first stroke makes a new drawing clip with the document's exposure (frames per drawing: on ones, twos, threes and so on), as Koi does.

The core API for the apps (uniffi) is a thin layer: a `TimelineInfo` snapshot for the panel (tracks, clips, key frames), plus commands such as `set_playhead`, `add_track`, `add_drawing`, `move_clip(id, track, start, policy)`, `trim_clip`, `split_clip`, `duplicate_clip`, `set_key`, `remove_key`, `set_easing`, `set_camera_key`.

## Files

The timeline is saved inside the document's JSON as it is. Media goes in the package as `media/<id>.<ext>`.

ProCraft (upstream) ignores the field it doesn't know, so a Make2D animation opened in ProCraft shows its drawings as plain top-level groups. To keep that useful, Make2D can also write an Animation Assist block (one frame per drawing on the bottom track, holds from durations). That's a nice-to-have.

## Migrating Animation Assist

`Document::start_timeline` (built in 1a) turns a canvas with Animation Assist settings into a timeline:

- One track, "Animation". Each frame becomes a drawing clip, in order, with its hold as its duration.
- The Background and Foreground options become their own tracks below and above it, with one clip spanning the whole duration. The frame they name is taken off the Animation track, so at its own place in the sequence it shows just as before (behind or in front).
- A hidden top-level layer isn't a frame. It gets a hidden track of its own, so showing it later works.
- Tracks are ordered by where their layers were in the layer stack, so the drawing order is unchanged.
- fps, playback and onion settings (including "blend primary") carry over, and the canvas opens in simple mode.
- `Document::animation` is cleared once the timeline exists.
- Page Assist canvases aren't converted: pages aren't frames.

A canvas with no Animation Assist becomes a one-frame timeline with a track for each top-level layer or group (hidden ones on hidden tracks), so nothing disappears.

The tests convert canvases with holds, a background, a foreground, both, a hidden layer and a single frame, and check that every frame of the timeline shows exactly the drawings Animation Assist showed at that point in its sequence.

When a document is read, `Timeline::repaired` makes the timeline consistent with its layers: a drawing whose layer is gone is dropped (the layers stay), a clip that overlaps the one before it is dropped, no layer is drawn by two clips, and keys are put in frame order. A timeline that can't be read at all is left out rather than failing the document.

## Build order

| Step | What | Where | Done when |
|---|---|---|---|
| 1a ✓ | Model, `Document.timeline`, migration, `FramePlan` without poses | engine | Animation Assist canvases sample to the same frames they show today (done Nov 14, 2026) |
| 1b ✓ | Plan-driven compositing, onion skins, playback | gpu, core | A migrated canvas plays the same as before (done Nov 14, 2026: GPU tests compare every played and drawn-on frame pixel for pixel; exports now render each timeline frame as it plays, background and foreground included) |
| 1c ✓ | iPad timeline panel, Make2D-native look: tracks, clips, playhead, drag/trim/split; simple mode as today's frame strip (done Nov 14, 2026: Actions → Canvas → Timeline; the layers panel still lists every drawing's layers) | core API, `ios/Make2D` | Can block out an animation on iPad |
| 1d ✓ | Keys and poses: channels, Linear/Hold/Smooth/Bézier, posed compositing, pivot | engine, gpu | A clip can move, turn and fade (done Nov 14, 2026: keyed opacity and clip blend modes too; iPad Keys panel on the picked clip, keyed at the playhead) |
| 1e ✓ | Full easing library, a small easing editor window, motion paths | engine, iPad | Matches Koi's easing curves (done Nov 14, 2026: every Koi easing, checked against Koi's own numbers; Bézier motion paths timed by distance along them; on iPad the picked clip's path shows over the canvas while its keys are open, a key's dot tapped to go to it or dragged to move it, a handle dragged to bend the path, the other mirroring it) |
| 1f ✓ | Groups and cycles, mattes, keyed camera | engine, gpu, iPad | Koi's group, matte and camera features work (done Nov 14, 2026: clipping masks and alpha and luma mattes drawn on the GPU; group clips made from clips picked on one level, opened to edit their tracks, cycling with their keys repeating or running on, and composited as one; the camera keyed by dragging its frame over the canvas, exports and "Play Through Camera" seen through it) |
| 1g ✓ | Track clipboard, retime, sync markers (tap to drop one at the playhead while playing; new UI) | engine, iPad | Clips and keys copy and paste, the frame rate changes with the timing kept, markers time the drawings (done Nov 15, 2026: clips copied or cut with their keys and drawings, pasted at the playhead on the track drawn on and the tracks above it, each paste one undo step, the clipboard keeping its drawings' textures while nothing else does; keys copied from a frame or all of a clip's, pasted onto the clip at the playhead; Koi's "Keep Timing" retime, chosen in Settings, scaling clips, keys, cycles, markers and the camera, a sound keeping its start in seconds; markers saved with the timeline, dropped with Mark while playing, shown as amber flags on the ruler, dragged, held to delete, cleared in Settings; also from 1f, a camera lane of its keys under the ruler, and the layers panel listing only the drawing drawn on) |

Steps 1a and 1b change nothing anyone can see, which makes them safe to land first: the timeline drives everything, but a migrated canvas looks and plays exactly as it did.

## Decisions

Settled Nov 14, 2026:

- **Frame rate:** 1–120 fps, as in Koi (`MAX_FPS`).
- **Simple mode stays.** `Timeline::simple` names the one track a document shows as a flipbook strip, like Animation Assist today: one thumbnail per drawing, holds as widths, onion skins, play. `None` is the full track view. Both are views of the same document, so switching never changes it, and the other tracks (a background, say) still draw in simple mode. A migrated Animation Assist canvas opens in simple mode on its "Animation" track.
- **Effect keys** are stored as plain parameter lists by effect id (`Channel<Vec<f32>>`) until phase 4 defines the effect types.
- **Exposure is one setting per document** (`Timeline::exposure`, frames per new drawing, 1 to 12, default 1 for "on ones").

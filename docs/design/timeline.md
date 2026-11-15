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

Not in this phase: adjustment layers (phase 4; effects stay ProCraft's own suite, with no per-layer effect stack). Audio and video came in as phase 3 (see Media, below).

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

The timeline is saved inside the document's JSON as it is. Media goes in the artwork's folder as `media/<id>.<ext>` (see Media).

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

## Media (phase 3)

Built Nov 15, 2026: sounds and videos on tracks, and the animatic detector, ported from Koi Studio (`PaintCore/Audio`, `PaintCore/Video`) with a Make2D-native panel.

### Model

The engine keeps what places media in time (`crates/engine/src/media.rs`); the platform decodes and plays.

- `Timeline::media: [Media]`, each `{ id, name, kind (Audio | Video), seconds, extension, size }`. The id is a hash of the file's bytes (`MediaHasher`, FNV over 8-byte words), so bringing the same file in twice shares it. `extension` and `size` are new with serde defaults; a timeline written before media has no `media` list and opens as it was.
- A sound or video clip is an ordinary clip (frames, track, keys), its content `Audio { media, trim_start, offset, volume, muted, volume_keys }` or `Video { media, trim_start, rect }`. Unlike Koi, whose audio clips live apart from the tracks and are timed in seconds, a Make2D sound is a clip on a track like any other, so moving, grouping, cycling, hiding and the track rules all apply with no new code. `trim_start` (where it starts in its file) and a sound's `offset` (a start under a frame after its first frame, for sync finer than a frame) are seconds, so it stays in sync to the sample.
- A clip is never longer than what is left of its file from `trim_start` (`set_duration` clamps). `trim_media_start` drags the left edge (later or earlier in the file, the end kept, never before the file's start or the clip before it); `slip_media` plays another part of the file where the clip is.
- A new sound goes on a track of its own at the bottom (`add_sound`), a new video on one under the drawings, above the sound tracks, fitted into the canvas (`add_video`).
- `Timeline::sounds()` is every sound as it plays in timeline seconds (start, where in the file, how long, level, its volume keys): a group's sounds counted from its start and again on every cycle, cut where the group ends; a hidden track's are muted, a disabled clip's left out. `Sound::gain_at(frame)` is its keyed level.
- The plan's `Shows::Video { media, seconds, rect }` is what a video draws at a frame. It is in `PlanItem`, so `same_shot` (Harmony and sprite sheets) tells video frames apart.
- `Timeline::repaired` keeps the values sane (finite, non-negative trims, volume 0..1, a rect with some size). A clip whose file is missing stays: it plays and draws nothing until the file is back.

### Files

An artwork's folder keeps each file the document plays as `media/<16 hex id>.<ext>`, with a sound's waveform beside it as `<id>.peaks` (little-endian f32s, the loudest sample in each 50th of a second). A file brought in waits in the system temporary folder (`make2d-media/`, named the same way) until a save copies it in. A save moves the files the document no longer plays out to that folder rather than deleting them, because an undo can bring a clip back; `MediaStore` (core) looks in the artwork's folders it was opened from or saved to and then the waiting folder. Nothing about media is in the document's undo copies but the small `Media` records.

### Platform interface

The core never decodes media (`crates/core/src/media.rs`):

- `import_timeline_media(MediaImport { path, name, kind, seconds, width, height, peaks })`: the app reads the length, picture size and waveform; the core hashes and copies the file and makes the clip, one undo step ("Add Sound", "Add Video"), starting a timeline if there isn't one.
- `timeline_sounds(frame)`: the sounds to play, with their files' paths. The app schedules and mixes them.
- `VideoFrames` (a uniffi foreign trait the app implements): `picture(path, seconds, max_side) -> ThumbnailImage`. The GPU canvas (`gpu/src/canvas/video.rs`) holds one texture per video and asks for a new picture only when the time shown changes; it draws it into the clip's rect through the clip's pose, opacity and blend (the transform shader, as a posed drawing is). Exports and Harmony go through the same render, so video shows in them with no more code.
- The animatic detector: `animatic_scores(frames, w, h)` scores grey thumbnails the app decodes, `animatic_holds(scores, sensitivity, shortest)` cuts them, `make_timing_track(clip, holds)` lays the drawings.

### iPad

- `+ Media` in the panel's header: Sound… or Video… from Files (`TimelineMedia.swift`, `TimelineMediaViews.swift`).
- On its lane a sound shows its waveform (the part it plays, in a teal clip; dimmed when muted), a video a strip of film. Picked, the left edge trims into the file and the right edge as any clip's.
- A sound or video's Clip page: the whole file as a strip with the part played lit, dragged to slip; Level and Nudge (the sub-frame offset); Mute; for a video, Fit Canvas / Fill Canvas / Actual Size and Make Timing Track…; Play From the File's Start, Delete.
- Playing: `SoundPlayer` (AVAudioEngine) starts every sound together a moment ahead and the frames are paced by the clock from then, so they stay together; a loop or a jump starts the sounds again from there, and ping-pong's way back is silent. Scrubbing plays the slice of sound under each frame (at least 60 ms).
- MP4 and HEVC exports get the sounds mixed under them (`MediaExport.swift`, AVMutableComposition and an audio mix for the levels); at an export rate other than the timeline's, each sound starts where its frame is shown. GIF and PNG carry no sound.
- Video pictures come from `AVAssetImageGenerator` (within 10 ms of the time asked, scaled to the canvas's longest side).
- Debug scenes: `-screen timeline-sound`, `timeline-video`, `timeline-animatic` make a sound and an animatic on the spot.

### Animatic detector

Koi's `VideoDrawingDetector` finds where a video of held drawings (a storyboard or animatic export) changes drawing. Each frame the clip covers is decoded at the time it shows there, shrunk to a grey picture of at most 128 pixels (decoded at twice that and pooled 2×2 to average away compression noise), and compared with the frame before: its score is the share of pixels whose lightness moved by more than 16. Scores are worked out once; a cut is wherever a score reaches the threshold (sensitivity 0 to 1 mapped log-evenly from 0.3, cuts only, to 0.0005, every change), and a cut less than the shortest hold after the last is folded into it (so does a too-short last hold). Make Track lays a new, empty drawing for each hold on a track just above the video, to draw over with the video's timing. The iPad's sheet shows the frame looked at, the scores as bars with the threshold across them and a tick at each cut, Sensitivity and Shortest Hold; both are remembered.

Left out from Koi: putting the video's frame as a reference layer under each new drawing ("Include video drawings"); the video stays on its own track under them instead, which shows the same thing without copying pixels.

### Not built: desktop and Android

The engine, core and GPU parts are shared; each platform needs its own decoding and audio:

- **Desktop** (`desktop/`): a `VideoFrames` on a decoder (ffmpeg through a crate such as `ffmpeg-next`, or GStreamer), reading a file's length, size and waveform for `MediaImport`; sound out through `cpal` (or `rodio`), scheduling `timeline_sounds` and following the playhead as the iPad does; a file picker for `+ Media`; muxing the sounds into video exports (the desktop's video export would need to exist first).
- **Android**: `MediaExtractor`/`MediaCodec` or `MediaMetadataRetriever.getFrameAtTime` for `VideoFrames` and the detector's thumbnails, `MediaMetadataRetriever` for length and size, decoding to PCM for the waveform; `AudioTrack` or ExoPlayer clips for playing; `MediaMuxer` for exports.
- Neither has the panel's media UI yet; the core calls are the same.

Known gaps everywhere: keyed volume plays (the export mixes at the level at frame 0, and playing takes each sound's level when it starts); no recording from the microphone; splitting or duplicating a sound or video clip isn't offered (the engine's split and duplicate copy drawings).

## Build order

| Step | What | Where | Done when |
|---|---|---|---|
| 1a ✓ | Model, `Document.timeline`, migration, `FramePlan` without poses | engine | Animation Assist canvases sample to the same frames they show today (done Nov 14, 2026) |
| 1b ✓ | Plan-driven compositing, onion skins, playback | gpu, core | A migrated canvas plays the same as before (done Nov 14, 2026: GPU tests compare every played and drawn-on frame pixel for pixel; exports now render each timeline frame as it plays, background and foreground included) |
| 1c ✓ | iPad timeline panel, Make2D-native look: tracks, clips, playhead, drag/trim/split; simple mode as today's frame strip (done Nov 14, 2026: Actions → Canvas → Timeline; the layers panel still lists every drawing's layers) | core API, `ios/Make2D` | Can block out an animation on iPad |
| 1d ✓ | Keys and poses: channels, Linear/Hold/Smooth/Bézier, posed compositing, pivot | engine, gpu | A clip can move, turn and fade (done Nov 14, 2026: keyed opacity and clip blend modes too; iPad Keys panel on the picked clip, keyed at the playhead) |
| 1e ✓ | Full easing library, a small easing editor window, motion paths | engine, iPad | Matches Koi's easing curves (done Nov 14, 2026: every Koi easing, checked against Koi's own numbers; Bézier motion paths timed by distance along them; on iPad the picked clip's path shows over the canvas while its keys are open, a key's dot tapped to go to it or dragged to move it, a handle dragged to bend the path, the other mirroring it) |
| 1f ✓ | Groups and cycles, mattes, keyed camera | engine, gpu, iPad | Koi's group, matte and camera features work (done Nov 14, 2026: clipping masks and alpha and luma mattes drawn on the GPU; group clips made from clips picked on one level, opened to edit their tracks, cycling with their keys repeating or running on, and composited as one; the camera keyed by dragging its frame over the canvas, exports and "Play Through Camera" seen through it) |
| 1g | Track clipboard, retime, sync markers (tap to drop one at the playhead while playing; new UI) | engine, iPad | |
| 3 ✓ | Media: sounds and videos on tracks, waveforms, playing and scrubbing with sound, sound in video exports, the animatic detector | engine, core, gpu, iPad | A sound and an animatic can be brought in, timed and played, and drawn over (done Nov 15, 2026; desktop and Android not built, see Media) |

Steps 1a and 1b change nothing anyone can see, which makes them safe to land first: the timeline drives everything, but a migrated canvas looks and plays exactly as it did.

## Decisions

Settled Nov 14, 2026:

- **Frame rate:** 1–120 fps, as in Koi (`MAX_FPS`).
- **Simple mode stays.** `Timeline::simple` names the one track a document shows as a flipbook strip, like Animation Assist today: one thumbnail per drawing, holds as widths, onion skins, play. `None` is the full track view. Both are views of the same document, so switching never changes it, and the other tracks (a background, say) still draw in simple mode. A migrated Animation Assist canvas opens in simple mode on its "Animation" track.
- **Effect keys** are stored as plain parameter lists by effect id (`Channel<Vec<f32>>`) until phase 4 defines the effect types.
- **Exposure is one setting per document** (`Timeline::exposure`, frames per new drawing, 1 to 12, default 1 for "on ones").

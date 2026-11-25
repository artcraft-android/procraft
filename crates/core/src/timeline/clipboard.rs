//! The timeline clipboard: clips (with their keys and their drawings' layers) or keys on their
//! own, copied or cut and pasted at the playhead on the track drawn on, each paste one undo step.
//! There is one for every canvas, so what is copied on one pastes into another.
//!
//! The clipboard keeps the layers of the drawings it holds as they were when copied, and the
//! canvas copied from keeps their textures (a cut drawing's too, though its layers are gone), so a
//! paste there copies them as they are then: as they were cut, or as they are now if they are still
//! there. For pasting into another canvas, the clipboard also keeps the painted part of each of
//! their layers as it was copied (RGBA8, so an HDR canvas's light above white is cut to white),
//! put down about the middle of a canvas of another size, and the media their sounds and videos
//! play.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use procraft_engine::document::{Layer, LayerId, LayerKind};
use procraft_engine::geom::IRect;
use procraft_engine::timeline::{Clip, ClipContent, ClipId, ClipKeys, CopiedClips, Media};

use crate::{CanvasSession, State};

/// Clips copied, and what pasting them needs.
#[derive(Clone, Debug)]
pub(crate) struct CopiedTimelineClips {
    clips: CopiedClips,
    /// Their drawings' layers, by drawing (in the clips' order).
    layers: Vec<(LayerId, Vec<Layer>)>,
    /// Each pixel layer's painted part on the canvas copied from: the layer, where, its rows.
    pixels: Vec<(LayerId, IRect, Vec<u8>)>,
    /// The size of the canvas copied from.
    size: (u32, u32),
    /// The media their sounds and videos play, with where its file and waveform were.
    media: Vec<(Media, Option<PathBuf>, Option<PathBuf>)>,
}

/// What is copied, by whichever canvas copied it last.
#[derive(Clone, Default)]
struct Clipboard {
    /// Told apart from every other copy (see `State::timeline_copied`).
    copy: u64,
    clips: Option<Arc<CopiedTimelineClips>>,
    keys: Option<ClipKeys>,
}

/// Each copy's number.
static COPIES: AtomicU64 = AtomicU64::new(1);

#[cfg(not(test))]
static CLIPBOARD: std::sync::Mutex<Option<Clipboard>> = std::sync::Mutex::new(None);

#[cfg(not(test))]
fn clipboard() -> Clipboard {
    CLIPBOARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clone().unwrap_or_default()
}

#[cfg(not(test))]
fn set_clipboard(c: Clipboard) {
    *CLIPBOARD.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(c);
}

// Tests run side by side, each on a thread of its own: each has a clipboard of its own.
#[cfg(test)]
thread_local! {
    static CLIPBOARD: std::cell::RefCell<Clipboard> = std::cell::RefCell::new(Clipboard::default());
}

#[cfg(test)]
fn clipboard() -> Clipboard {
    CLIPBOARD.with(|c| c.borrow().clone())
}

#[cfg(test)]
fn set_clipboard(c: Clipboard) {
    CLIPBOARD.with(|b| *b.borrow_mut() = c);
}

/// What the timeline clipboard holds, for the apps' menus.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Record)]
pub struct TimelineClipboardInfo {
    /// How many clips (0: none).
    pub clips: u32,
    /// Keys are held.
    pub keys: bool,
}

#[uniffi::export]
impl CanvasSession {
    /// What the timeline clipboard holds (copied on this canvas or another).
    pub fn timeline_clipboard(&self) -> TimelineClipboardInfo {
        let c = clipboard();
        TimelineClipboardInfo {
            clips: c.clips.as_ref().map_or(0, |c| u32::try_from(c.clips.clips.len()).unwrap_or(u32::MAX)),
            keys: c.keys.is_some(),
        }
    }

    /// Clips `ids` (on tracks of one level) copied with their keys, in place of what the
    /// clipboard held. False if one isn't there or they aren't on one level.
    pub fn copy_timeline_clips(&self, ids: Vec<u64>) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.finish_stroke();
        st.settle();
        st.copy_clips(&ids.into_iter().map(ClipId).collect::<Vec<_>>())
    }

    /// Clips `ids` copied, then taken off their tracks with their drawings' layers, as one undo
    /// step.
    pub fn cut_timeline_clips(&self, ids: Vec<u64>) -> bool {
        let ids: Vec<ClipId> = ids.into_iter().map(ClipId).collect();
        {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            if !st.copy_clips(&ids) {
                return false;
            }
        }
        let cut = self
            .change("Cut Clips", |d| {
                let tl = d.timeline.as_mut()?;
                if ids.iter().any(|id| tl.track_of_clip(*id).and_then(|t| tl.track(t)).is_none_or(|t| t.locked)) {
                    return None;
                }
                let gone: Vec<_> = ids.iter().filter_map(|id| tl.remove(*id, false)).collect();
                for root in gone.iter().flat_map(|c| c.roots()) {
                    d.delete_layer(root);
                }
                Some(())
            })
            .is_some();
        if cut {
            self.follow();
        }
        cut
    }

    /// The clips copied (on this canvas or another) put down at the playhead, the lowest on the
    /// track drawn on and the others on the tracks above it (made, if there are none), each
    /// drawing a copy of its layers, as one undo step. The clips there move along to make room.
    /// Returns the clips pasted (none if there is nothing to paste, a track is locked or there is
    /// no room for the layers).
    pub fn paste_timeline_clips(&self) -> Vec<u64> {
        let made = {
            let mut guard = self.shared.lock();
            let st = &mut *guard;
            st.finish_stroke();
            st.settle();
            st.paste_clips()
        };
        if !made.is_empty() {
            self.follow();
        }
        made.into_iter().map(|c| c.0).collect()
    }

    /// Clip `id`'s keys copied (only those on the playhead's frame, if `here`), in place of what
    /// the clipboard held. False if it has none there.
    pub fn copy_timeline_keys(&self, id: u64, here: bool) -> bool {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(tl) = st.doc.timeline.as_ref() else { return false };
        let at = if here {
            match tl.local_frame(ClipId(id), frame).and_then(|f| i32::try_from(f).ok()) {
                Some(f) => Some(f),
                None => return false,
            }
        } else {
            None
        };
        let Some(keys) = tl.copy_keys(ClipId(id), at) else { return false };
        set_clipboard(Clipboard { copy: COPIES.fetch_add(1, Ordering::Relaxed), clips: None, keys: Some(keys) });
        st.timeline_copied = None;
        st.hold_clipboard(&[]);
        true
    }

    /// The keys copied (on this canvas or another) laid on the clip at the playhead on the track
    /// drawn on, the first on the playhead's frame, as one undo step. False with no keys copied or
    /// no clip there.
    pub fn paste_timeline_keys(&self) -> bool {
        let (target, at) = {
            let st = self.shared.lock();
            let frame = st.canvas.as_ref().map_or(0, |c| c.timeline_frame());
            let target = st.doc.timeline.as_ref().zip(st.drawn_track()).and_then(|(tl, t)| {
                let clip = super::clip_on(tl, t, frame)?.id;
                Some((clip, i32::try_from(tl.local_frame(clip, frame)?).ok()?))
            });
            let Some(target) = target else { return false };
            target
        };
        let Some(keys) = clipboard().keys else { return false };
        self.change("Paste Keys", |d| d.timeline.as_mut()?.paste_keys(target, &keys, at).then_some(())).is_some()
    }
}

impl CanvasSession {
    /// After clips come or go: the current layer follows the playhead.
    fn follow(&self) {
        let mut guard = self.shared.lock();
        let st = &mut *guard;
        st.follow_playhead();
        self.shared.frame(st);
    }
}

impl State {
    /// Clips `ids` into the clipboard; false if they can't be copied.
    fn copy_clips(&mut self, ids: &[ClipId]) -> bool {
        let Some(tl) = self.doc.timeline.as_ref() else { return false };
        let Some(clips) = tl.copy_clips(ids) else { return false };
        let layers: Vec<(LayerId, Vec<Layer>)> =
            clips.roots().into_iter().filter_map(|r| Some((r, self.doc.unit_layers(r)?))).collect();
        let mut used = Vec::new();
        for c in &clips.clips {
            media_of(&c.clip, &mut used);
        }
        let media = tl
            .media
            .iter()
            .filter(|m| used.contains(&m.id))
            .map(|m| (m.clone(), self.media.find(&m.file_name()), self.media.find(&m.peaks_name())))
            .collect();
        let units: Vec<LayerId> =
            layers.iter().flat_map(|(_, ls)| ls).filter(|l| l.kind != LayerKind::Group).map(|l| l.id).collect();
        let pixels = self.painted_parts(&units);
        let copy = COPIES.fetch_add(1, Ordering::Relaxed);
        let size = (self.doc.width(), self.doc.height());
        let clips = CopiedTimelineClips { clips, layers, pixels, size, media };
        set_clipboard(Clipboard { copy, clips: Some(Arc::new(clips)), keys: None });
        self.timeline_copied = Some(copy);
        self.hold_clipboard(&units);
        true
    }

    /// The canvas keeps the textures of `layers` (the clipboard's), letting go of those it kept.
    fn hold_clipboard(&mut self, layers: &[LayerId]) {
        if let Some(c) = self.canvas.as_mut() {
            c.hold_layers(layers.to_vec());
        }
    }

    /// Where each of `layers` is painted and its pixels there (none where it is clear), the
    /// canvas's work queued drawn first. Nothing without a GPU.
    fn painted_parts(&mut self, layers: &[LayerId]) -> Vec<(LayerId, IRect, Vec<u8>)> {
        self.flush();
        let whole = IRect::from_size(self.doc.width(), self.doc.height());
        let (Some(canvas), Some(gpu)) = (self.canvas.as_ref(), self.gpu.as_ref()) else { return Vec::new() };
        let boxes = canvas.painted_boxes_or_why(gpu, layers);
        layers
            .iter()
            .zip(boxes)
            .filter_map(|(id, painted)| {
                // (parked, the GPU can't tell: all of it)
                let rect = match painted {
                    Ok(painted) => painted?,
                    Err(_) => whole,
                };
                let part = canvas.read_layer_part(gpu, *id, rect).ok()?;
                Some((*id, rect, part))
            })
            .collect()
    }

    /// The clipboard's clips pasted (see [`CanvasSession::paste_timeline_clips`]).
    fn paste_clips(&mut self) -> Vec<ClipId> {
        let held = clipboard();
        let Some(copied) = held.clips else { return Vec::new() };
        let frame = self.canvas.as_ref().map_or(0, |c| c.timeline_frame());
        let Some(track) = self.drawn_track() else { return Vec::new() };
        let Some(level) = self.doc.timeline.as_ref().and_then(|tl| tl.level_start(track)) else { return Vec::new() };
        let units = copied.layers.iter().flat_map(|(_, ls)| ls).filter(|l| l.kind != LayerKind::Group).count();
        if !self.doc.has_room(units) {
            return Vec::new();
        }
        // copied here: the canvas still has the layers' textures
        let here = self.timeline_copied == Some(held.copy);
        let before = self.doc.clone();
        let mut roots = HashMap::new();
        let mut pairs = Vec::new();
        for (root, layers) in &copied.layers {
            if let Some((copy, p)) = self.doc.insert_copy(layers, *root) {
                roots.insert(*root, copy);
                pairs.extend(p);
            }
        }
        // from a canvas of another size, about the middle
        let (w, h) = (self.doc.width(), self.doc.height());
        let shift = if here {
            (0, 0)
        } else {
            let half = |to: u32, from: u32| ((i64::from(to) - i64::from(from)) / 2) as i32;
            (half(w, copied.size.0), half(h, copied.size.1))
        };
        let mut clips = copied.clips.clone();
        if shift != (0, 0) {
            for c in &mut clips.clips {
                shifted(&mut c.clip, shift.0 as f32, shift.1 as f32);
            }
        }
        if !here && let Some(tl) = self.doc.timeline.as_mut() {
            for (m, file, peaks) in &copied.media {
                tl.add_media(m.clone());
                if self.media.find(&m.file_name()).is_none()
                    && let Some(file) = file
                {
                    crate::media::bring_along(m, file, peaks.as_deref());
                }
            }
        }
        let at = frame.saturating_sub(level);
        let made = self.doc.timeline.as_mut().and_then(|tl| tl.paste_clips(&clips, track, at, &roots));
        let Some(made) = made else {
            self.doc = before;
            return Vec::new();
        };
        if let Some(canvas) = self.canvas.as_mut() {
            canvas.name_next_step("Paste Clips");
            if here {
                canvas.duplicate_layers(pairs, before);
            } else {
                canvas.record_document(before);
                if let Some(gpu) = self.gpu.as_ref() {
                    let full = IRect::from_size(w, h);
                    for (from, to) in pairs {
                        let Some((_, rect, part)) = copied.pixels.iter().find(|(id, ..)| *id == from) else { continue };
                        let moved =
                            IRect::new(rect.x0 + shift.0, rect.y0 + shift.1, rect.x1 + shift.0, rect.y1 + shift.1);
                        let kept = moved.intersect(full);
                        if kept.is_empty() {
                            continue;
                        }
                        let part = cropped(part, moved, kept);
                        if let Err(e) = canvas.upload_layer_part(gpu, to, kept, &part) {
                            log::warn!("pasting a drawing's pixels: {e}");
                        }
                    }
                }
                // the frame as cached went before the pixels came: composited again
                canvas.invalidate();
            }
        }
        if !here {
            self.media.know(&self.doc);
            self.attach_media();
        }
        self.revision += 1;
        made
    }
}

/// The media ids clip `clip` plays (a group's, at any depth), into `out`.
fn media_of(clip: &Clip, out: &mut Vec<procraft_engine::timeline::MediaId>) {
    match &clip.content {
        ClipContent::Audio(a) => out.push(a.media),
        ClipContent::Video(v) => {
            out.push(v.media);
            out.extend(v.sound.map(|s| s.media));
        }
        ClipContent::Group(g) => g.tracks.iter().flat_map(|t| &t.clips).for_each(|c| media_of(c, out)),
        ClipContent::Drawing(_) => {}
    }
}

/// Clip `clip` (and what is in it) moved `dx`, `dy` on the canvas: its pivot and a video's place.
fn shifted(clip: &mut Clip, dx: f32, dy: f32) {
    if let Some([x, y]) = clip.keys.pivot {
        clip.keys.pivot = Some([x + dx, y + dy]);
    }
    match &mut clip.content {
        ClipContent::Video(v) => {
            v.rect[0] += dx;
            v.rect[1] += dy;
        }
        ClipContent::Group(g) => g.tracks.iter_mut().flat_map(|t| &mut t.clips).for_each(|c| shifted(c, dx, dy)),
        ClipContent::Drawing(_) | ClipContent::Audio(_) => {}
    }
}

/// The rows of `part` (RGBA8 covering `rect`) inside `within` (part of `rect`).
fn cropped(part: &[u8], rect: IRect, within: IRect) -> Vec<u8> {
    let row = rect.width() as usize * 4;
    let (x0, x1) = ((within.x0 - rect.x0) as usize * 4, (within.x1 - rect.x0) as usize * 4);
    let mut out = Vec::with_capacity((x1 - x0) * within.height() as usize);
    for line in part.chunks_exact(row.max(1)).skip((within.y0 - rect.y0) as usize).take(within.height() as usize) {
        out.extend_from_slice(line.get(x0..x1).unwrap_or_default());
    }
    out
}

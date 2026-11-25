//! Timeline frames as they were composited, kept so that going back to one (scrubbing, playing
//! again) copies it into the composite instead of compositing it again.
//!
//! A frame is kept with its fingerprint: a hash of everything it was composited from that can
//! change (what its plan draws and how, its onion skins, the camera, the background, every
//! setting of every layer it draws, and those layers' pixel versions). A kept frame is used only
//! while its fingerprint still holds, so an edit throws away exactly the frames it changes: a
//! drawing painted, the frames that show it (or skin it); a clip, key, camera or settings edit,
//! the frames whose plan, skins or camera it changes. Painting drops those frames at once (see
//! [`FrameCache::forget_layer`]); other edits are found out when the frame is next wanted.
//! What the cache can't see (a video's picture, the canvas's size) keeps a frame out of it or
//! clears it all.

use std::collections::HashMap;
use std::fmt::Write as _;
use std::hash::{DefaultHasher, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};

use procraft_engine::document::{Document, LayerId};
use procraft_engine::timeline::{PlanItem, Shows, Timeline};

use super::{copy_at, extent, item_layers, items_key};
use crate::Gpu;

/// What a canvas's kept frames may take unless it is told otherwise, bytes (see
/// [`set_frame_cache_budget`]).
static BUDGET: AtomicU64 = AtomicU64::new(512 << 20);

/// Sets what each canvas's kept timeline frames may take, bytes: the app sets it from the
/// device's memory. 0 keeps none.
pub fn set_frame_cache_budget(bytes: u64) {
    BUDGET.store(bytes, Ordering::Relaxed);
}

/// What was set with [`set_frame_cache_budget`].
pub fn frame_cache_budget() -> u64 {
    BUDGET.load(Ordering::Relaxed)
}

/// How a canvas's kept frames have done.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FrameCacheStats {
    /// Frames shown from the cache.
    pub hits: u64,
    /// Frames composited that could have been kept (none was, or it no longer held).
    pub misses: u64,
    /// Frames kept now, and what they take, bytes.
    pub frames: usize,
    pub bytes: u64,
}

/// Which frame, as shown: drawn on (with onion skins) or played, through the camera or not.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) struct FrameKey {
    pub frame: u32,
    pub playing: bool,
    pub camera: bool,
}

/// A frame about to be composited, as the cache knows it.
pub(crate) struct Wanted {
    key: FrameKey,
    fingerprint: u64,
    layers: Vec<LayerId>,
}

struct Kept {
    texture: wgpu::Texture,
    fingerprint: u64,
    /// The layers it draws (onion skins and mattes too): painting one drops it.
    layers: Vec<LayerId>,
    /// When it was last used, counted in uses of the cache.
    used: u64,
}

#[derive(Default)]
pub(crate) struct FrameCache {
    kept: HashMap<FrameKey, Kept>,
    /// This canvas's own budget, if it has one (else the app's).
    budget: Option<u64>,
    /// The size and format the kept frames are (the composite's).
    shape: Option<(u32, u32, wgpu::TextureFormat)>,
    clock: u64,
    hits: u64,
    misses: u64,
}

impl FrameCache {
    pub fn set_budget(&mut self, bytes: Option<u64>) {
        self.budget = bytes;
        if self.budget() == 0 {
            self.forget();
        }
    }

    fn budget(&self) -> u64 {
        self.budget.unwrap_or_else(frame_cache_budget)
    }

    /// Every frame let go.
    pub fn forget(&mut self) {
        self.kept.clear();
    }

    /// The frames that draw layer `id` let go: its pixels are changing.
    pub fn forget_layer(&mut self, id: LayerId) {
        self.kept.retain(|_, k| !k.layers.contains(&id));
    }

    pub fn stats(&self) -> FrameCacheStats {
        let per = self.shape.map_or(0, |(w, h, f)| frame_bytes(w, h, f));
        FrameCacheStats {
            hits: self.hits,
            misses: self.misses,
            frames: self.kept.len(),
            bytes: per * self.kept.len() as u64,
        }
    }

    /// What the frame `key` of timeline `tl` will be composited from, if it can be kept (none for
    /// a frame showing a video, whose picture the cache can't see). `version` is a layer's
    /// pixels' version.
    pub fn wanted(
        &self,
        doc: &Document,
        tl: &Timeline,
        key: FrameKey,
        camera: bool,
        size: (u32, u32),
        version: impl Fn(LayerId) -> u64,
    ) -> Option<Wanted> {
        if self.budget() == 0 {
            return None;
        }
        let plan = tl.plan(key.frame);
        if shows_video(&plan.items) {
            return None;
        }
        let mut h = Fingerprint(DefaultHasher::new());
        // the same walk as the frame's composite: each track's skins below, its clips, its skins above
        let _ = write!(h, "{size:?} {key:?} {:?}", plan.items);
        let mut roots = plan.roots();
        for track in &tl.tracks {
            let (before, after) = tl.onions_on(track, key.frame, key.playing);
            if !before.is_empty() || !after.is_empty() {
                let _ = write!(h, "|{:?} {before:?} {after:?}", track.id);
                roots.extend(before.iter().chain(&after).map(|s| s.root));
            }
        }
        if camera {
            let _ = write!(h, "|camera {:?}", tl.camera_at(key.frame as f32));
        }
        roots.sort_unstable();
        roots.dedup();
        h.0.write_u64(items_key(doc, &roots, true));
        let mut layers: Vec<LayerId> = roots.iter().flat_map(|r| item_layers(doc, *r)).collect();
        layers.sort_unstable();
        layers.dedup();
        for id in &layers {
            let _ = write!(h, "|{id:?}@{}", version(*id));
        }
        Some(Wanted { key, fingerprint: h.0.finish(), layers })
    }

    /// Copies the kept frame `wanted` into `composite`, if it is kept and still holds; false
    /// (and the stale one let go) if it has to be composited.
    pub fn restore(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        wanted: &Wanted,
        composite: &wgpu::Texture,
    ) -> bool {
        self.clock += 1;
        if self.shape != Some(shape_of(composite)) {
            self.forget();
        }
        match self.kept.get_mut(&wanted.key) {
            Some(k) if k.fingerprint == wanted.fingerprint => {
                k.used = self.clock;
                gpu.copy_texture_to_texture(
                    encoder,
                    copy_at(&k.texture, 0, 0),
                    copy_at(composite, 0, 0),
                    extent_of(composite),
                );
                self.hits += 1;
                true
            }
            _ => {
                self.kept.remove(&wanted.key);
                self.misses += 1;
                false
            }
        }
    }

    /// Keeps a copy of `composite`, just composited as `wanted`: in a new texture while the
    /// budget has room, else in the one of the frame used least lately.
    pub fn keep(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        wanted: Wanted,
        composite: &wgpu::Texture,
    ) {
        let shape = shape_of(composite);
        let (width, height, format) = shape;
        let per = frame_bytes(width, height, format);
        let most = usize::try_from(self.budget() / per.max(1)).unwrap_or(usize::MAX);
        if most == 0 {
            return;
        }
        self.shape = Some(shape);
        // over the budget (it may have been lowered), the least used go; the last one's texture
        // is used again
        let mut spare = None;
        while self.kept.len() >= most {
            let Some(oldest) = self.kept.iter().min_by_key(|(_, k)| k.used).map(|(key, _)| *key) else { break };
            spare = self.kept.remove(&oldest).map(|k| k.texture);
        }
        let texture = spare.unwrap_or_else(|| {
            gpu.texture(&wgpu::TextureDescriptor {
                label: Some("kept frame"),
                size: extent(width, height),
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage: wgpu::TextureUsages::COPY_SRC | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            })
        });
        gpu.copy_texture_to_texture(encoder, copy_at(composite, 0, 0), copy_at(&texture, 0, 0), extent(width, height));
        self.kept.insert(
            wanted.key,
            Kept { texture, fingerprint: wanted.fingerprint, layers: wanted.layers, used: self.clock },
        );
    }
}

/// A hasher written to as text: a frame's parts hashed by how they print, every field of them.
struct Fingerprint(DefaultHasher);

impl std::fmt::Write for Fingerprint {
    fn write_str(&mut self, s: &str) -> std::fmt::Result {
        self.0.write(s.as_bytes());
        Ok(())
    }
}

fn shows_video(items: &[PlanItem]) -> bool {
    items.iter().any(|i| match &i.shows {
        Shows::Video { .. } => true,
        Shows::Group(inner) => shows_video(inner),
        Shows::Drawing(_) => false,
    })
}

fn shape_of(t: &wgpu::Texture) -> (u32, u32, wgpu::TextureFormat) {
    (t.width(), t.height(), t.format())
}

fn extent_of(t: &wgpu::Texture) -> wgpu::Extent3d {
    extent(t.width(), t.height())
}

fn frame_bytes(width: u32, height: u32, format: wgpu::TextureFormat) -> u64 {
    u64::from(width) * u64::from(height) * u64::from(format.block_copy_size(None).unwrap_or(4))
}

//! Video clips' pictures. The canvas doesn't decode video: a [`VideoSource`] the app gives it
//! hands over the picture a video shows at a time, which is kept on the GPU (one texture a
//! video, the size of its pictures) until the time shown changes. Each render first fetches the
//! pictures the frame shown wants; compositing then draws each into its clip's rect, posed,
//! faded and blended as any clip is.
//!
//! Reading a picture takes the decoder tens of milliseconds (a hundred for a large video, or one
//! with few key frames), far longer than a render may take with the app waiting on it. So once
//! [`Canvas::read_videos_aside`] is on, a render only asks for the pictures it lacks: a thread
//! of the canvas's own reads the newest time asked for in each video, the render draws the
//! picture it has meanwhile (the last one shown, a moment behind while scrubbing or playing),
//! and the one read shows on the first render after it comes ([`Canvas::videos_pending`] says
//! one is on its way). A render whose picture is read back ([`Canvas::render_finished`]: an
//! export's, a save's) still waits for each video's picture at its own time.

use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError};

use procraft_engine::document::Document;
use procraft_engine::geom::{Affine, IRect, Vec2};
use procraft_engine::timeline::MediaId;

use super::{
    Canvas, LAYER_FORMAT, LayerUniform, Look, Stack, TransformUniform, UniformData, render_pass, scissor, texture,
};
use crate::Gpu;

/// Where videos' pictures come from (the platform's decoder).
pub trait VideoSource: Send + Sync {
    /// The picture video `media` shows `seconds` into it, if it can be read: straight RGBA8, rows
    /// top to bottom. Any size; a large one is best scaled down to about the canvas's first.
    fn picture(&self, media: MediaId, seconds: f64) -> Option<VideoPicture>;
}

/// A video's picture.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoPicture {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// A video's picture as it is on the GPU, and the time in the video it is (milliseconds).
struct Held {
    at: i64,
    width: u32,
    height: u32,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
}

/// The videos' pictures the canvas has, and where it gets them.
#[derive(Default)]
pub(super) struct Videos {
    source: Option<Arc<dyn VideoSource>>,
    held: HashMap<MediaId, Held>,
    /// The thread reading pictures off the render's own, once asked for.
    aside: Option<Aside>,
    /// The render under way is read back: it waits for each picture at its own time.
    pub(super) exact: bool,
}

/// The thread that reads pictures aside, ended when this goes.
struct Aside {
    shared: Arc<AsideShared>,
}

#[derive(Default)]
struct AsideShared {
    state: Mutex<AsideState>,
    wake: Condvar,
}

#[derive(Default)]
struct AsideState {
    source: Option<Arc<dyn VideoSource>>,
    /// The time last asked for in each video and not yet read (milliseconds, and as asked): a
    /// newer time takes an older one's place, so the reader never falls behind what is shown.
    wanted: HashMap<MediaId, (i64, f64)>,
    /// What is being read now.
    reading: Option<(MediaId, i64)>,
    /// What has been read and the canvas hasn't taken yet.
    done: Vec<(MediaId, i64, VideoPicture)>,
    /// The time in a video that couldn't be read: not asked for again while it is the one wanted.
    failed: HashMap<MediaId, i64>,
    closed: bool,
}

impl AsideShared {
    fn state(&self) -> MutexGuard<'_, AsideState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Reads what is wanted, one picture at a time, until closed.
    fn read(&self) {
        loop {
            let (source, media, at, seconds) = {
                let mut st = self.state();
                loop {
                    if st.closed {
                        return;
                    }
                    let next = st.wanted.keys().next().copied().zip(st.source.clone());
                    if let Some((media, source)) = next
                        && let Some((at, seconds)) = st.wanted.remove(&media)
                    {
                        st.reading = Some((media, at));
                        break (source, media, at, seconds);
                    }
                    st = self.wake.wait(st).unwrap_or_else(PoisonError::into_inner);
                }
            };
            let picture = source.picture(media, seconds);
            let mut st = self.state();
            st.reading = None;
            match picture {
                Some(p) => {
                    st.failed.remove(&media);
                    st.done.retain(|(m, ..)| *m != media);
                    st.done.push((media, at, p));
                }
                None => _ = st.failed.insert(media, at),
            }
        }
    }
}

impl Drop for Aside {
    fn drop(&mut self) {
        self.shared.state().closed = true;
        self.shared.wake.notify_all();
    }
}

impl Canvas {
    /// Where videos' pictures come from; none, video clips draw nothing.
    pub fn set_video_source(&mut self, source: Option<Arc<dyn VideoSource>>) {
        if let Some(aside) = &self.videos.aside {
            let mut st = aside.shared.state();
            st.source.clone_from(&source);
            st.wanted.clear();
            st.done.clear();
            st.failed.clear();
        }
        self.videos.source = source;
        // (pictures shown come from the new source)
        if !self.videos.held.is_empty() {
            self.videos.held.clear();
            self.invalidate();
        }
    }

    /// Whether it has somewhere to get videos' pictures from.
    pub fn has_video_source(&self) -> bool {
        self.videos.source.is_some()
    }

    /// From now on a render doesn't wait for a video's picture: a thread of the canvas's own
    /// reads it, and it shows on the first render after it comes (see the module's notes). The
    /// app renders again while [`Canvas::videos_pending`].
    pub fn read_videos_aside(&mut self) {
        if self.videos.aside.is_some() {
            return;
        }
        let shared = Arc::new(AsideShared::default());
        shared.state().source.clone_from(&self.videos.source);
        let reader = shared.clone();
        // (left without a quality of service class of its own: all it does is wait on the
        // platform's decoder, whose threads run as "default", and a more urgent thread waiting
        // on those is what the system reports as a hang risk)
        match std::thread::Builder::new().name("video pictures".to_owned()).spawn(move || reader.read()) {
            Ok(_) => self.videos.aside = Some(Aside { shared }),
            // (no thread to be had: pictures go on being read as they are wanted)
            Err(e) => log::warn!("no thread for videos' pictures: {e}"),
        }
    }

    /// Whether a video's picture is being read aside, or waits to be taken: another render soon
    /// will show it.
    pub fn videos_pending(&self) -> bool {
        self.videos.aside.as_ref().is_some_and(|a| {
            let st = a.shared.state();
            !st.wanted.is_empty() || st.reading.is_some() || !st.done.is_empty()
        })
    }

    /// The pictures the timeline frame shown wants, where the time in a video changed: fetched
    /// now, or (read aside, unless this render is read back) asked for, those that came since
    /// the last render taken.
    pub(super) fn feed_videos(&mut self, gpu: &Gpu, doc: &Document) {
        let Some(tl) = doc.timeline.as_ref() else {
            self.videos.held.clear();
            return;
        };
        let Some(source) = self.videos.source.clone() else { return };
        let (frame, _) = self.timeline_at();
        let wanted = tl.videos_at(frame);
        let millis = |seconds: f64| (seconds * 1000.0).round() as i64;
        // pictures of videos the timeline no longer has go
        self.videos.held.retain(|id, _| tl.media.iter().any(|m| m.id == *id));
        // what was read aside since the last render: shown unless the video already has the
        // picture wanted now (a read that waited got it first), or no longer shows
        let aside = self.videos.aside.as_ref().map(|a| a.shared.clone());
        let read = aside.as_ref().map(|a| std::mem::take(&mut a.state().done)).unwrap_or_default();
        let mut came = false;
        for (media, at, p) in read {
            let Some(now) = wanted.iter().find(|(m, _)| *m == media).map(|(_, s)| millis(*s)) else { continue };
            if self.videos.held.get(&media).is_none_or(|h| h.at != now) {
                came |= self.hold_video(gpu, media, at, p);
            }
        }
        for (media, seconds) in wanted {
            let at = millis(seconds);
            if self.videos.held.get(&media).is_some_and(|h| h.at == at) {
                continue;
            }
            match aside.as_ref().filter(|_| !self.videos.exact) {
                Some(aside) => {
                    let mut st = aside.state();
                    let asked = st.reading == Some((media, at)) || st.wanted.get(&media).is_some_and(|w| w.0 == at);
                    if !asked && st.failed.get(&media) != Some(&at) {
                        st.wanted.insert(media, (at, seconds));
                        aside.wake.notify_one();
                    }
                }
                None => {
                    if let Some(p) = source.picture(media, seconds) {
                        self.hold_video(gpu, media, at, p);
                    }
                }
            }
        }
        // (a picture that came with nothing else changed is drawn by this render)
        if came {
            self.invalidate();
        }
    }

    /// Picture `p`, video `media`'s at `at` milliseconds, put on the GPU as the one it shows.
    /// False if it isn't a picture.
    fn hold_video(&mut self, gpu: &Gpu, media: MediaId, at: i64, p: VideoPicture) -> bool {
        let expected = p.width as usize * p.height as usize * 4;
        if p.width == 0 || p.height == 0 || p.rgba.len() != expected {
            return false;
        }
        let reuse = self.videos.held.remove(&media).filter(|h| h.width == p.width && h.height == p.height);
        let (tex, view) = match reuse {
            Some(h) => (h.texture, h.view),
            None => {
                let t = texture(gpu, "video picture", p.width, p.height, LAYER_FORMAT);
                let v = t.create_view(&wgpu::TextureViewDescriptor::default());
                (t, v)
            }
        };
        // a picture with see-through parts is premultiplied as the layers are
        let rgba = if p.rgba.as_chunks::<4>().0.iter().all(|&[.., a]| a == 255) { p.rgba } else { premultiplied(&p.rgba) };
        gpu.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &tex,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &rgba,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * p.width), rows_per_image: Some(p.height) },
            wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 },
        );
        self.videos.held.insert(media, Held { at, width: p.width, height: p.height, texture: tex, view });
        true
    }

    /// Video `media`'s picture drawn into `into` (left, top, width, height on the canvas) onto
    /// `stack`, posed, faded and blended by `look`. Nothing while it has no picture.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn composite_video(
        &self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        rect: IRect,
        media: MediaId,
        into: [f32; 4],
        look: Look,
        stack: &Stack<'_>,
        current: &mut usize,
    ) {
        let device = &gpu.device;
        let Some(held) = self.videos.held.get(&media) else { return };
        let [left, top, w, h] = into;
        // the picture's pixels onto the canvas, then as the clip is posed
        let placed = Affine { a: w / held.width as f32, d: h / held.height as f32, ..Affine::IDENTITY }
            .then(Affine::translate(Vec2::new(left, top)));
        let to_canvas = look.pose.map_or(placed, |p| placed.then(p));
        let Some(back) = to_canvas.inverse() else { return };
        let (_, posed) = self.posed.get(gpu);
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("video"),
            layout: &self.pipelines.transform_layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&held.view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&self.everything) },
            ],
        });
        // each canvas pixel from where it is in the picture, bilinear
        let offset = uniforms.push(&TransformUniform {
            r0: [back.a, back.c, back.e, 0.0],
            r1: [back.b, back.d, back.f, 0.0],
            r2: [0.0, 0.0, 1.0, 0.0],
            p: [1.0, held.width as f32, held.height as f32, 0.0],
        });
        {
            let mut pass = render_pass(encoder, "video", posed, wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT));
            scissor(&mut pass, rect);
            pass.set_pipeline(&self.pipelines.transform);
            pass.set_bind_group(0, &bind, &[]);
            pass.set_bind_group(1, &self.uniform_bind, &[offset]);
            pass.draw(0..3, 0..1);
        }
        let blend = look.blend.unwrap_or(procraft_engine::document::BlendMode::Normal);
        let uniform = LayerUniform { opacity: look.opacity, blend: blend.id(), tint: look.tint, ..Default::default() };
        self.draw_onto(gpu, encoder, uniforms, rect, posed, None, &self.stroke_view, uniform, blend, stack, current);
    }
}

fn premultiplied(rgba: &[u8]) -> Vec<u8> {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&[r, g, b, a]| {
            let [r, g, b, a] = [r, g, b, a].map(u16::from);
            [r * a / 255, g * a / 255, b * a / 255, a].map(|v| v as u8)
        })
        .collect()
}

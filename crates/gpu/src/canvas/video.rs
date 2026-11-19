//! Video clips' pictures. The canvas doesn't decode video: a [`VideoSource`] the app gives it
//! hands over the picture a video shows at a time, which is kept on the GPU (one texture a
//! video, the size of its pictures) until the time shown changes. Each render first fetches the
//! pictures the frame shown wants; compositing then draws each into its clip's rect, posed,
//! faded and blended as any clip is.

use std::collections::HashMap;
use std::sync::Arc;

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
}

impl Canvas {
    /// Where videos' pictures come from; none, video clips draw nothing.
    pub fn set_video_source(&mut self, source: Option<Arc<dyn VideoSource>>) {
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

    /// The pictures the timeline frame shown wants, fetched where the time in a video changed.
    pub(super) fn feed_videos(&mut self, gpu: &Gpu, doc: &Document) {
        let Some(tl) = doc.timeline.as_ref() else {
            self.videos.held.clear();
            return;
        };
        let Some(source) = self.videos.source.clone() else { return };
        let (frame, _) = self.timeline_at();
        let wanted = tl.videos_at(frame);
        // pictures of videos the timeline no longer has go
        self.videos.held.retain(|id, _| tl.media.iter().any(|m| m.id == *id));
        for (media, seconds) in wanted {
            let at = (seconds * 1000.0).round() as i64;
            if self.videos.held.get(&media).is_some_and(|h| h.at == at) {
                continue;
            }
            let Some(p) = source.picture(media, seconds) else { continue };
            let expected = p.width as usize * p.height as usize * 4;
            if p.width == 0 || p.height == 0 || p.rgba.len() != expected {
                continue;
            }
            let reuse = self.videos.held.remove(&media).filter(|h| h.width == p.width && h.height == p.height);
            let (tex, view) = match reuse {
                Some(h) => (h.texture, h.view),
                None => {
                    let t = texture(&gpu.device, "video picture", p.width, p.height, LAYER_FORMAT);
                    let v = t.create_view(&wgpu::TextureViewDescriptor::default());
                    (t, v)
                }
            };
            // a picture with see-through parts is premultiplied as the layers are
            let rgba =
                if p.rgba.as_chunks::<4>().0.iter().all(|&[.., a]| a == 255) { p.rgba } else { premultiplied(&p.rgba) };
            gpu.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &tex,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * p.width),
                    rows_per_image: Some(p.height),
                },
                wgpu::Extent3d { width: p.width, height: p.height, depth_or_array_layers: 1 },
            );
            self.videos.held.insert(media, Held { at, width: p.width, height: p.height, texture: tex, view });
        }
    }

    /// Video `media`'s picture drawn into `into` (left, top, width, height on the canvas) onto
    /// `stack`, posed, faded and blended by `look`. Nothing while it has no picture.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn composite_video(
        &self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: &mut UniformData,
        rect: IRect,
        media: MediaId,
        into: [f32; 4],
        look: Look,
        stack: &Stack<'_>,
        current: &mut usize,
    ) {
        let Some(held) = self.videos.held.get(&media) else { return };
        let [left, top, w, h] = into;
        // the picture's pixels onto the canvas, then as the clip is posed
        let placed = Affine { a: w / held.width as f32, d: h / held.height as f32, ..Affine::IDENTITY }
            .then(Affine::translate(Vec2::new(left, top)));
        let to_canvas = look.pose.map_or(placed, |p| placed.then(p));
        let Some(back) = to_canvas.inverse() else { return };
        let (_, posed) = self.posed.get(device);
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
        self.draw_onto(device, encoder, uniforms, rect, posed, None, &self.stroke_view, uniform, blend, stack, current);
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

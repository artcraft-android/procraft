//! A stroke shown while it is still moving (a QuickShape's, dragged about): laid each frame whole on
//! a canvas of its own a k-th of this one's size (strokes given in this canvas's pixels, see
//! [`Canvas::draw_scaled`]), over its layer as it was before the stroke, and shown in the layer as
//! the layer as it was plus what the stroke changed on the small canvas, drawn k times as big. Laid
//! whole at a k-th of the pixels, it keeps up with the drag where laying it again here at each move
//! fell behind; the stroke's own wet mixing and second are laid there as here. Nothing of it is
//! kept: the layer as it was is put back as it ends, and the stroke is then laid here once.

use procraft_engine::document::LayerId;
use procraft_engine::geom::IRect;

use super::{Canvas, LAYER_FORMAT, copy_at, extent, render_pass, scissor, texture};
use crate::Gpu;

/// What a stroke shown on a smaller canvas keeps (see the module).
pub(super) struct Provisional {
    pub(super) layer: LayerId,
    /// The layer as it was before the stroke, this canvas's size.
    before: wgpu::Texture,
    before_view: wgpu::TextureView,
    /// The small canvas's layer as it was before the stroke.
    small_before_view: wgpu::TextureView,
    /// How many of this canvas's pixels each of the small canvas's covers each way.
    k: wgpu::Buffer,
    /// Where the layer was last drawn otherwise than it was.
    pub(super) shown: IRect,
}

impl Canvas {
    /// Begins showing the stroke on `layer` laid on `small` (a canvas `k` times smaller each way,
    /// told to draw strokes so, see [`Canvas::draw_scaled`]) in its layer `small_layer`: that layer
    /// drawn from this one as it was before the open stroke (the stroke itself the caller cancels).
    /// The work is sent at once, before the small canvas's. False if it can't be.
    pub fn begin_provisional(
        &mut self,
        gpu: &Gpu,
        layer: LayerId,
        (small, small_layer): (&mut Canvas, LayerId),
        k: u32,
    ) -> bool {
        self.provisional = None;
        let Some(pipelines) = self.pipelines.provisional.as_ref() else { return false };
        let Some(target) = self.layers.get(&layer) else { return false };
        let Some(small_target) = small.layers.get(&small_layer) else { return false };
        let device = &gpu.device;
        let before = texture(gpu, "provisional before", self.width, self.height, LAYER_FORMAT);
        let (sw, sh) = (small.width, small.height);
        let small_before = texture(gpu, "provisional small before", sw, sh, LAYER_FORMAT);
        let k_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("provisional k"),
            size: 16,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        gpu.queue.write_buffer(&k_buffer, 0, bytemuck::cast_slice(&[k.max(1), 0u32, 0, 0]));
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("provisional") });
        // the layer as it was: the open stroke painting in place keeps it where it wrote
        let full = self.full();
        if self.smudge.as_ref().is_some_and(|s| s.layer == layer && s.into == layer) {
            self.smudge_before(gpu, &mut encoder, full, &before);
        } else {
            gpu.copy_texture_to_texture(
                &mut encoder,
                copy_at(&target.texture, 0, 0),
                copy_at(&before, 0, 0),
                extent(self.width, self.height),
            );
        }
        let before_view = before.create_view(&wgpu::TextureViewDescriptor::default());
        let small_before_view = small_before.create_view(&wgpu::TextureViewDescriptor::default());
        // the small canvas's layer as this one's was, a k by k mean a pixel; and kept so
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("provisional down"),
            layout: &pipelines.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&before_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&small_before_view) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&small_before_view) },
                wgpu::BindGroupEntry { binding: 3, resource: k_buffer.as_entire_binding() },
            ],
        });
        {
            let mut pass = render_pass(&mut encoder, "provisional down", &small_target.view, wgpu::LoadOp::Load);
            pass.set_pipeline(&pipelines.down);
            pass.set_bind_group(0, &bind, &[]);
            pass.draw(0..3, 0..1);
        }
        gpu.copy_texture_to_texture(
            &mut encoder,
            copy_at(&small_target.texture, 0, 0),
            copy_at(&small_before, 0, 0),
            extent(sw, sh),
        );
        gpu.queue.submit([encoder.finish()]);
        self.provisional = Some(Box::new(Provisional {
            layer,
            before,
            before_view,
            small_before_view,
            k: k_buffer,
            shown: IRect::EMPTY,
        }));
        true
    }

    /// Shows the stroke as the small canvas given to [`Canvas::begin_provisional`] has it now (its
    /// work sent before this canvas's next render), over `rect` of the layer (this canvas's pixels)
    /// and wherever it was shown before.
    pub fn show_provisional(&mut self, (small, small_layer): (&Canvas, LayerId), rect: IRect) {
        let Some(view) = small.layers.get(&small_layer).map(|l| l.view.clone()) else { return };
        if self.provisional.is_some() {
            self.ops.push(super::Op::ShowProvisional { small: view, rect: rect.intersect(self.full()) });
        }
    }

    /// Ends showing the stroke on a smaller canvas: the layer as it was put back where it was shown.
    pub fn end_provisional(&mut self) {
        if self.provisional.is_some() {
            self.ops.push(super::Op::EndProvisional);
        }
    }

    /// Whether a stroke on a smaller canvas is being shown (see [`Canvas::begin_provisional`]).
    pub fn shows_provisional(&self) -> bool {
        self.provisional.is_some()
    }

    /// Draws the stroke on the smaller canvas, its layer `small` as it is now, into the layer over
    /// `rect` and where it was shown before.
    pub(super) fn draw_provisional(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        small: &wgpu::TextureView,
        rect: IRect,
    ) {
        let Some(pipelines) = self.pipelines.provisional.as_ref() else { return };
        let Some(p) = self.provisional.as_mut() else { return };
        let Some(layer) = self.layers.get(&p.layer) else { return };
        let rect = rect.union(p.shown);
        if rect.is_empty() {
            return;
        }
        let bind = gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("provisional show"),
            layout: &pipelines.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&p.before_view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(small) },
                wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&p.small_before_view) },
                wgpu::BindGroupEntry { binding: 3, resource: p.k.as_entire_binding() },
            ],
        });
        let mut pass = render_pass(encoder, "provisional show", &layer.view, wgpu::LoadOp::Load);
        scissor(&mut pass, rect);
        pass.set_pipeline(&pipelines.show);
        pass.set_bind_group(0, &bind, &[]);
        pass.draw(0..3, 0..1);
        drop(pass);
        p.shown = rect;
        self.dirty = self.dirty.union(rect);
    }

    /// Puts the layer back as it was where the stroke on a smaller canvas was shown, and forgets it.
    pub(super) fn drop_provisional(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        let Some(p) = self.provisional.take() else { return };
        let Some(layer) = self.layers.get(&p.layer) else { return };
        let r = p.shown.intersect(self.full());
        if r.is_empty() {
            return;
        }
        gpu.copy_texture_to_texture(
            encoder,
            copy_at(&p.before, r.x0, r.y0),
            copy_at(&layer.texture, r.x0, r.y0),
            extent(r.width(), r.height()),
        );
        self.dirty = self.dirty.union(r);
    }
}

/// The passes a stroke on a smaller canvas is shown by (see the module).
pub(crate) struct ProvisionalPipelines {
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) down: wgpu::RenderPipeline,
    pub(super) show: wgpu::RenderPipeline,
}

impl ProvisionalPipelines {
    pub(super) fn new(device: &wgpu::Device, cache: Option<&wgpu::PipelineCache>) -> ProvisionalPipelines {
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: false },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("provisional"),
            entries: &[
                tex(0),
                tex(1),
                tex(2),
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("provisional"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("provisional"),
            source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/provisional.wgsl").into()),
        });
        let pass = |fs| {
            super::blended_pipeline(device, cache, &pipeline_layout, &module, fs, ("vs_full", fs), LAYER_FORMAT, None)
        };
        let (down, show) = (pass("fs_down"), pass("fs_show"));
        ProvisionalPipelines { layout, down, show }
    }
}

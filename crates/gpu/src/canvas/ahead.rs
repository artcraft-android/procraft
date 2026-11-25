//! Paint laid in place shown on to the pen: what the open stroke holds back for the rest of its
//! display frame, and its stamps from there to the pen (and where it is predicted to go), laid into
//! the layer for now as they would be were the stroke to go no further, and taken off again before
//! any more of the stroke is laid. What they go over is kept meanwhile: the layer there, what the
//! stroke keeps of it in its tiles (see `smudge.rs`), and the plots wet paint carries from pass to
//! pass.

use procraft_engine::geom::IRect;

use super::smudge::{tile_rect, tiles_of};
use super::{
    Canvas, Carry, Kept, LAYER_FORMAT, Paced, StampInstance, UniformData, WET_PLOT_MOST, copy_at, extent, texture,
    wet_reach,
};
use crate::Gpu;

/// Bytes of the plots wet paint carries over from pass to pass, at the start of what it carries.
const CARRIED_BYTES: u64 = 2 * (WET_PLOT_MOST as u64) * (WET_PLOT_MOST as u64) * 16;

/// Where what paint laid ahead goes over is kept: the layer, the stroke as laid, how much each
/// pixel has taken and how much of the paint shows (as the stroke keeps them), each as big as the
/// furthest it has reached; and wet paint's carried-over plots.
#[derive(Default)]
pub(super) struct Keep {
    layer: Option<wgpu::Texture>,
    grained: Option<wgpu::Texture>,
    amount: Option<wgpu::Texture>,
    shown: Option<wgpu::Texture>,
    carry: Option<wgpu::Buffer>,
    /// It couldn't be made: nothing more is laid ahead of this stroke.
    refused: bool,
}

impl Keep {
    pub(super) fn textures(&self) -> impl Iterator<Item = &wgpu::Texture> {
        self.layer.iter().chain(&self.grained).chain(&self.amount).chain(&self.shown)
    }
}

/// Paint laid ahead of a stroke: the pixels it changed, and the stroke's counts as they were (each
/// copy's carry's, and whose the stroke's carry held).
pub(super) struct Laid {
    changed: IRect,
    carries: Vec<Carry>,
    resident: Option<(usize, Kept)>,
    synthesized: IRect,
}

impl Canvas {
    /// Whether paint is laid ahead of the open stroke on `layer`: it is laid in place there.
    fn lays_ahead_onto(&self, layer: procraft_engine::document::LayerId) -> bool {
        self.lays_ahead() && self.smudge.as_ref().is_some_and(|s| s.layer == layer)
    }

    /// The most stamps laid ahead of the open stroke in a render that lays `now` more of it: those
    /// it holds back, those and what runs on to the pen.
    pub(super) fn ahead_most(&self, now: u64) -> u64 {
        if !self.lays_ahead() {
            return 0;
        }
        let held = self.smudge.as_ref().map_or(0, |s| s.pending.len() as u64);
        held + now + self.ahead.len() as u64
    }

    /// Lays paint laid in place ahead of its stroke for now: the stamps it holds back for the rest
    /// of their display frame, then those on to the pen ([`Canvas::set_ahead`]), as they would be
    /// laid were the stroke to go no further. What they go over is kept first, and comes back
    /// before any more of the stroke is laid ([`Canvas::take_back_ahead`]): what the stroke lays
    /// for good is as it would have been without.
    pub(super) fn lay_ahead(
        &mut self,
        gpu: &Gpu,
        encoder: &mut wgpu::CommandEncoder,
        uniforms: (&mut UniformData, &mut UniformData),
    ) {
        let Some(style) = self.active.filter(|a| self.lays_ahead_onto(a.layer)).map(|a| a.style) else { return };
        let full = self.full();
        let Some(smudge) = self.smudge.as_ref() else { return };
        if smudge.laid_ahead.is_some() || smudge.stalled || smudge.keep.refused || smudge.carries.len() > 1 {
            return;
        }
        let stamps: Vec<(StampInstance, [f32; 8], Paced)> = smudge.pending.iter().chain(&self.ahead).copied().collect();
        // a symmetry's copies take turns in the stroke's carry, putting theirs aside: what they put
        // aside isn't kept, so nothing is laid ahead of a stroke a symmetry copies
        if stamps.iter().any(|(_, _, p)| p.copy != 0) {
            return;
        }
        let changed = stamps.iter().fold(IRect::EMPTY, |r, (s, ..)| r.union(wet_reach(s))).intersect(full);
        if changed.is_empty() {
            return;
        }
        // the stroke's tiles there made first (the layer under them as it was before it): what it
        // keeps in them is kept from them, and put back into them
        if !self.smudge_keep(gpu, encoder, &[changed]) || !self.ahead_room(gpu, changed, &style) {
            return;
        }
        let Some(smudge) = self.smudge.as_mut() else { return };
        let Some(layer) = self.layers.get(&smudge.into) else { return };
        let size = extent(changed.width(), changed.height());
        if let Some(keep) = &smudge.keep.layer {
            gpu.copy_texture_to_texture(
                encoder,
                copy_at(&layer.texture, changed.x0, changed.y0),
                copy_at(keep, 0, 0),
                size,
            );
        }
        for at in tiles_of(changed, smudge.tile) {
            let Some(tile) = smudge.tiles.get(&at) else { continue };
            let t = tile_rect(at, smudge.tile);
            let r = t.intersect(changed);
            if r.is_empty() {
                continue;
            }
            let parts = [
                (&tile.grained, &smudge.keep.grained),
                (&tile.amount, &smudge.keep.amount),
                (&tile.shown, &smudge.keep.shown),
            ];
            for (from, into) in parts {
                if let (Some(from), Some(into)) = (from, into) {
                    gpu.copy_texture_to_texture(
                        encoder,
                        copy_at(from, r.x0 - t.x0, r.y0 - t.y0),
                        copy_at(into, r.x0 - changed.x0, r.y0 - changed.y0),
                        extent(r.width(), r.height()),
                    );
                }
            }
        }
        if let (Some(carry), Some(keep)) = (&smudge.carry, &smudge.keep.carry) {
            encoder.copy_buffer_to_buffer(carry, 0, keep, 0, CARRIED_BYTES);
        }
        smudge.laid_ahead = Some(Laid {
            changed,
            carries: smudge.carries.iter().map(Carry::plain).collect(),
            resident: smudge.resident,
            synthesized: smudge.synthesized,
        });
        self.lay_wet(gpu, encoder, uniforms, &stamps, &style);
    }

    /// Takes off what is laid ahead of the stroke (see [`Canvas::lay_ahead`]): what it went over
    /// comes back, and the stroke goes on from where it was.
    pub(super) fn take_back_ahead(&mut self, gpu: &Gpu, encoder: &mut wgpu::CommandEncoder) {
        let Some(smudge) = self.smudge.as_mut() else { return };
        let Some(laid) = smudge.laid_ahead.take() else { return };
        let Some(layer) = self.layers.get(&smudge.into) else { return };
        let changed = laid.changed;
        if let Some(keep) = &smudge.keep.layer {
            gpu.copy_texture_to_texture(
                encoder,
                copy_at(keep, 0, 0),
                copy_at(&layer.texture, changed.x0, changed.y0),
                extent(changed.width(), changed.height()),
            );
        }
        for at in tiles_of(changed, smudge.tile) {
            let Some(tile) = smudge.tiles.get(&at) else { continue };
            let t = tile_rect(at, smudge.tile);
            let r = t.intersect(changed);
            if r.is_empty() {
                continue;
            }
            let parts = [
                (&smudge.keep.grained, &tile.grained),
                (&smudge.keep.amount, &tile.amount),
                (&smudge.keep.shown, &tile.shown),
            ];
            for (from, into) in parts {
                if let (Some(from), Some(into)) = (from, into) {
                    gpu.copy_texture_to_texture(
                        encoder,
                        copy_at(from, r.x0 - changed.x0, r.y0 - changed.y0),
                        copy_at(into, r.x0 - t.x0, r.y0 - t.y0),
                        extent(r.width(), r.height()),
                    );
                }
            }
        }
        if let (Some(keep), Some(carry)) = (&smudge.keep.carry, &smudge.carry) {
            encoder.copy_buffer_to_buffer(keep, 0, carry, 0, CARRIED_BYTES);
        }
        smudge.carries.truncate(laid.carries.len());
        for (carry, was) in smudge.carries.iter_mut().zip(laid.carries) {
            *carry = Carry { aside: carry.aside.take(), unkept: carry.unkept, ..was };
        }
        smudge.resident = laid.resident;
        smudge.synthesized = laid.synthesized;
        self.dirty = self.dirty.union(changed);
    }

    /// Room to keep what paint laid ahead over `changed` goes over; false if it couldn't be made
    /// (nothing is laid ahead of this stroke from then on).
    fn ahead_room(&mut self, gpu: &Gpu, changed: IRect, style: &super::StrokeStyle) -> bool {
        let device = &gpu.device;
        let (most_w, most_h) = (self.width, self.height);
        let Some(smudge) = self.smudge.as_mut() else { return false };
        let Some(layer) = self.layers.get(&smudge.into) else { return false };
        let format = layer.texture.format();
        let keeps = smudge.keeps;
        let wet = style.wet_pull.is_some() && smudge.carry.is_some();
        let (width, height) = (changed.width(), changed.height());
        let keep = &mut smudge.keep;
        let roomy = |t: &Option<wgpu::Texture>| t.as_ref().is_some_and(|t| t.width() >= width && t.height() >= height);
        let wanted = [
            (true, roomy(&keep.layer)),
            (keeps.grained, roomy(&keep.grained)),
            (keeps.amount, roomy(&keep.amount)),
            (keeps.shown, roomy(&keep.shown)),
        ];
        if wanted.iter().all(|&(wanted, roomy)| !wanted || roomy) && (!wet || keep.carry.is_some()) {
            return true;
        }
        let memory = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
        let invalid = device.push_error_scope(wgpu::ErrorFilter::Validation);
        // grown in steps, so a stroke that reaches further doesn't make them every frame
        let side = |n: u32, had: Option<u32>, most: u32| n.max(had.unwrap_or(0)).next_multiple_of(256).min(most).max(n);
        let grow = |kept: &mut Option<wgpu::Texture>, label: &str, format: wgpu::TextureFormat| {
            if !roomy(kept) {
                let (w, h) = (kept.as_ref().map(wgpu::Texture::width), kept.as_ref().map(wgpu::Texture::height));
                *kept = Some(texture(gpu, label, side(width, w, most_w), side(height, h, most_h), format));
            }
        };
        grow(&mut keep.layer, "ahead layer", format);
        if keeps.grained {
            grow(&mut keep.grained, "ahead grained", format);
        }
        // (the amounts' bits as bytes, as the stroke's tiles keep them)
        if keeps.amount {
            grow(&mut keep.amount, "ahead amount", LAYER_FORMAT);
        }
        if keeps.shown {
            grow(&mut keep.shown, "ahead shown", LAYER_FORMAT);
        }
        if wet && keep.carry.is_none() {
            keep.carry = Some(device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("ahead carry"),
                size: CARRIED_BYTES,
                usage: wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            }));
        }
        if let Some(why) = pollster::block_on(invalid.pop()).or(pollster::block_on(memory.pop())) {
            log::error!("laying ahead: {why}");
            *keep = Keep { refused: true, ..Keep::default() };
            return false;
        }
        true
    }
}

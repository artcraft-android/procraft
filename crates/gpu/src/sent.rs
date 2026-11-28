//! The device's queue as wgpu's, with what each thread sends on it counted: a submission, a write to
//! a buffer or a texture (each records the GPU's commands, barriers among them, on the thread that
//! makes it), a frame shown. The app's own thread is to send nothing: its touches hand their work to
//! the frames, which the render thread draws (see `touch/mail.rs` in the core). A driver crashed in
//! a barrier recorded as a touch's call laid a heavy stroke whole on that thread.

use std::cell::Cell;

thread_local! {
    /// What this thread has sent to any GPU so far (see [`work_sent_here`]).
    // (already const: clippy 1.99 asks for it again when built for Android)
    #[allow(clippy::missing_const_for_thread_local)]
    static SENT_HERE: Cell<u64> = const { Cell::new(0) };
}

fn sent_here() {
    SENT_HERE.with(|sent| sent.set(sent.get().wrapping_add(1)));
}

/// How many times this thread has sent work to a GPU so far: submissions, writes to buffers and
/// textures, frames shown. Two looks around a call tell whether it sent any (for tests).
pub fn work_sent_here() -> u64 {
    SENT_HERE.with(Cell::get)
}

/// A device's queue (see the module's notes): wgpu's, each submission and write counted.
#[derive(Clone, Debug)]
pub struct Queue(wgpu::Queue);

impl Queue {
    pub(crate) fn new(queue: wgpu::Queue) -> Queue {
        Queue(queue)
    }

    /// [`wgpu::Queue::submit`], counted.
    pub fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(&self, command_buffers: I) -> wgpu::SubmissionIndex {
        sent_here();
        self.0.submit(command_buffers)
    }

    /// [`wgpu::Queue::write_buffer`], counted.
    pub fn write_buffer(&self, buffer: &wgpu::Buffer, offset: wgpu::BufferAddress, data: &[u8]) {
        sent_here();
        self.0.write_buffer(buffer, offset, data);
    }

    /// [`wgpu::Queue::write_texture`], counted.
    pub fn write_texture(
        &self,
        texture: wgpu::TexelCopyTextureInfo<'_>,
        data: &[u8],
        data_layout: wgpu::TexelCopyBufferLayout,
        size: wgpu::Extent3d,
    ) {
        sent_here();
        self.0.write_texture(texture, data, data_layout, size);
    }

    /// [`wgpu::Queue::present`], counted (a frame never drawn to is cleared as it is shown).
    pub fn present(&self, frame: wgpu::SurfaceTexture) {
        sent_here();
        self.0.present(frame);
    }
}

impl std::ops::Deref for Queue {
    type Target = wgpu::Queue;

    fn deref(&self) -> &wgpu::Queue {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::work_sent_here;

    /// Each submission and write is counted on the thread that makes it, and only there.
    #[test]
    fn what_a_thread_sends_is_counted_on_it_alone() {
        let Ok(gpu) = crate::Gpu::headless() else { return eprintln!("skipped: no GPU") };
        let buffer = gpu.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("counted"),
            size: 256,
            usage: wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let before = work_sent_here();
        gpu.queue.write_buffer(&buffer, 0, &[1; 16]);
        let encoder = gpu.device.create_command_encoder(&Default::default());
        gpu.queue.submit([encoder.finish()]);
        assert_eq!(work_sent_here() - before, 2);
        // another thread's work isn't this one's
        let elsewhere = {
            let (queue, buffer) = (gpu.queue.clone(), buffer.clone());
            std::thread::spawn(move || {
                let before = work_sent_here();
                queue.write_buffer(&buffer, 16, &[2; 16]);
                queue.submit(std::iter::empty());
                work_sent_here() - before
            })
            .join()
            .unwrap()
        };
        assert_eq!(elsewhere, 2);
        assert_eq!(work_sent_here() - before, 2);
        gpu.wait().unwrap();
        assert_eq!(gpu.errors(), 0);
    }
}

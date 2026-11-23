use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

const MAX_BYTES: u64 = 24 * 1024 * 1024;
const MAX_TEXTURES: usize = 32;

struct Entry {
    texture: wgpu::Texture,
    size: [u32; 2],
    render_format: wgpu::TextureFormat,
}

impl Entry {
    fn bytes(&self) -> u64 {
        bytes(self.size)
    }
}

struct Submitted {
    textures: Vec<Entry>,
    finished: Arc<AtomicBool>,
}

/// A single device's reusable RGBA render attachments. A texture can be
/// sampled by the outer canvas after rendering a nested layer into it.
#[derive(Default)]
pub(super) struct TransientTextures {
    available: Vec<Entry>,
    active: Vec<Entry>,
    submitted: VecDeque<Submitted>,
    resident_bytes: u64,
    resident_count: usize,
    pub(super) reuses: u64,
}

impl TransientTextures {
    pub(super) fn begin_frame(&mut self) {
        while self
            .submitted
            .front()
            .is_some_and(|batch| batch.finished.load(Ordering::Acquire))
        {
            if let Some(mut batch) = self.submitted.pop_front() {
                self.available.append(&mut batch.textures);
            }
        }
    }

    pub(super) fn acquire(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
    ) -> (wgpu::Texture, bool) {
        self.acquire_render_view(device, size, wgpu::TextureFormat::Rgba8Unorm)
    }

    pub(super) fn acquire_render_view(
        &mut self,
        device: &wgpu::Device,
        size: [u32; 2],
        render_format: wgpu::TextureFormat,
    ) -> (wgpu::Texture, bool) {
        if let Some(index) = self
            .available
            .iter()
            .position(|entry| entry.size == size && entry.render_format == render_format)
        {
            let entry = self.available.swap_remove(index);
            let texture = entry.texture.clone();
            self.active.push(entry);
            self.reuses += 1;
            return (texture, true);
        }
        let attachment =
            create_render_texture(device, size, render_format, "XRack transient RGBA target");
        let bytes = bytes(size);
        if bytes <= MAX_BYTES {
            while (self.resident_count == MAX_TEXTURES || self.resident_bytes + bytes > MAX_BYTES)
                && let Some(evicted) = self.available.pop()
            {
                self.resident_count -= 1;
                self.resident_bytes -= evicted.bytes();
            }
            if self.resident_count < MAX_TEXTURES && self.resident_bytes + bytes <= MAX_BYTES {
                self.resident_count += 1;
                self.resident_bytes += bytes;
                self.active.push(Entry {
                    texture: attachment.clone(),
                    size,
                    render_format,
                });
            }
        }
        (attachment, false)
    }

    pub(super) fn finish_frame(&mut self, queue: &wgpu::Queue) {
        if self.active.is_empty() {
            return;
        }
        let finished = Arc::new(AtomicBool::new(false));
        let signal = finished.clone();
        queue.on_submitted_work_done(move || signal.store(true, Ordering::Release));
        self.submitted.push_back(Submitted {
            textures: std::mem::take(&mut self.active),
            finished,
        });
    }
}

pub(super) fn create_render_texture(
    device: &wgpu::Device,
    size: [u32; 2],
    render_format: wgpu::TextureFormat,
    label: &str,
) -> wgpu::Texture {
    let format = wgpu::TextureFormat::Rgba8Unorm;
    // The outer 2D canvas samples the raw, encoded RGB stored in this texture.
    let render_formats = [render_format];
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: if render_format == format {
            &[]
        } else {
            &render_formats
        },
    })
}

pub(super) fn bytes([width, height]: [u32; 2]) -> u64 {
    u64::from(width)
        .saturating_mul(u64::from(height))
        .saturating_mul(4)
}

//! Resident particle surface sampling tables.
use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

use xrack_runtime::scene3d::ParticleMeshSurface;

use super::buffer;

const CACHE_BYTES: usize = 40 * 1024 * 1024;
const CACHE_ENTRIES: usize = 8;

pub(super) struct SurfaceBinding {
    pub key: usize,
    pub buffer: wgpu::Buffer,
}

struct Entry {
    source: Weak<ParticleMeshSurface>,
    buffer: wgpu::Buffer,
    bytes: usize,
}

#[derive(Default)]
pub(super) struct MeshSurfaceBuffers {
    entries: VecDeque<Entry>,
    pub uploads: u64,
}

impl MeshSurfaceBuffers {
    pub fn get(
        &mut self,
        device: &wgpu::Device,
        surface: &Arc<ParticleMeshSurface>,
    ) -> Option<SurfaceBinding> {
        let key = Arc::as_ptr(surface);
        let index = self
            .entries
            .iter()
            .position(|entry| entry.source.as_ptr() == key);
        if let Some(entry) = index.and_then(|index| self.entries.remove(index)) {
            let binding = SurfaceBinding {
                key: key as usize,
                buffer: entry.buffer.clone(),
            };
            self.entries.push_front(entry);
            return Some(binding);
        }
        let bytes = surface.bytes().len();
        let limits = device.limits();
        if limits.max_storage_buffers_per_shader_stage == 0
            || bytes as u64 > limits.max_storage_buffer_binding_size
            || bytes as u64 > limits.max_buffer_size
        {
            return None;
        }
        self.uploads = self.uploads.saturating_add(1);
        let entry = Entry {
            source: Arc::downgrade(surface),
            buffer: buffer(
                device,
                "XRack particle triangle sampler",
                surface.bytes(),
                wgpu::BufferUsages::STORAGE,
            ),
            bytes,
        };
        let binding = SurfaceBinding {
            key: key as usize,
            buffer: entry.buffer.clone(),
        };
        let mut bytes = entry.bytes;
        self.entries.retain(|entry| {
            if entry.source.strong_count() == 0 {
                return false;
            }
            bytes += entry.bytes;
            bytes <= CACHE_BYTES
        });
        self.entries.truncate(CACHE_ENTRIES - 1);
        self.entries.push_front(entry);
        Some(binding)
    }
}

//! Reuse immutable affector uniforms when only particle time changes.
use std::collections::VecDeque;

use xrack_runtime::scene3d::{MAX_PARTICLE_AFFECTORS, ParticleAffector};

use super::{append_floats, buffer};

const AFFECTOR_BYTES: usize = 64;
const CACHE_ENTRIES: usize = 4;

#[derive(Default)]
pub(super) struct AffectorBuffers {
    entries: VecDeque<(Vec<ParticleAffector>, wgpu::Buffer)>,
    pub uploads: u64,
}

impl AffectorBuffers {
    pub fn get(&mut self, device: &wgpu::Device, affectors: Vec<ParticleAffector>) -> wgpu::Buffer {
        let index = self
            .entries
            .iter()
            .position(|(cached, _)| *cached == affectors);
        let entry = index
            .and_then(|index| self.entries.remove(index))
            .unwrap_or_else(|| {
                let mut bytes = Vec::with_capacity(AFFECTOR_BYTES * MAX_PARTICLE_AFFECTORS);
                for affector in &affectors {
                    append_floats(&mut bytes, affector.position_age);
                    append_floats(&mut bytes, affector.vector_value);
                    append_floats(&mut bytes, affector.time_bounds);
                    for word in affector.control {
                        bytes.extend_from_slice(&word.to_ne_bytes());
                    }
                }
                bytes.resize(AFFECTOR_BYTES * MAX_PARTICLE_AFFECTORS, 0);
                let uniform = buffer(
                    device,
                    "XRack particle affectors",
                    &bytes,
                    wgpu::BufferUsages::UNIFORM,
                );
                self.uploads = self.uploads.saturating_add(1);
                (affectors, uniform)
            });
        let uniform = entry.1.clone();
        self.entries.truncate(CACHE_ENTRIES - 1);
        self.entries.push_front(entry);
        uniform
    }
}

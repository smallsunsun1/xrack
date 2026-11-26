//! Reuse immutable affector uniforms when only particle time changes.
use std::collections::VecDeque;

use xrack_runtime::scene3d::{MAX_PARTICLE_AFFECTORS, ParticleAffector};

use super::{append_floats, buffer};

const AFFECTOR_BYTES: usize = 160;
const WANDER_BYTES: usize = 32;
const CACHE_ENTRIES: usize = 4;

#[derive(Default)]
pub(super) struct AffectorBuffers {
    entries: VecDeque<(Vec<ParticleAffector>, wgpu::Buffer)>,
    pub uploads: u64,
}

impl AffectorBuffers {
    pub fn get(&mut self, device: &wgpu::Device, affectors: &[&ParticleAffector]) -> wgpu::Buffer {
        let index = self.entries.iter().position(|(cached, _)| {
            cached.len() == affectors.len()
                && cached
                    .iter()
                    .zip(affectors)
                    .all(|(a, b)| a.parameters_match(b))
        });
        let entry = index
            .and_then(|index| self.entries.remove(index))
            .unwrap_or_else(|| {
                let mut bytes =
                    Vec::with_capacity((AFFECTOR_BYTES + WANDER_BYTES) * MAX_PARTICLE_AFFECTORS);
                for affector in affectors {
                    append_floats(&mut bytes, affector.position_age);
                    append_floats(&mut bytes, affector.vector_value);
                    append_floats(&mut bytes, affector.time_bounds);
                    let mut control = affector.control;
                    control[3] = u32::from(affector.frame.is_some());
                    for word in control {
                        bytes.extend_from_slice(&word.to_ne_bytes());
                    }
                    if let Some(frame) = affector.frame {
                        append_floats(&mut bytes, frame.world_to_local.into_iter().flatten());
                        append_floats(&mut bytes, frame.local_to_world.into_iter().flatten());
                    } else {
                        bytes.resize(bytes.len() + 96, 0);
                    }
                }
                // Keep the common affector layout independent of the wave settings.
                bytes.resize(AFFECTOR_BYTES * MAX_PARTICLE_AFFECTORS, 0);
                for affector in affectors {
                    append_floats(&mut bytes, affector.wander.into_iter().flatten());
                }
                bytes.resize((AFFECTOR_BYTES + WANDER_BYTES) * MAX_PARTICLE_AFFECTORS, 0);
                let uniform = buffer(
                    device,
                    "XRack particle affectors",
                    &bytes,
                    wgpu::BufferUsages::UNIFORM,
                );
                self.uploads = self.uploads.saturating_add(1);
                (
                    affectors.iter().map(|&affector| affector.clone()).collect(),
                    uniform,
                )
            });
        let uniform = entry.1.clone();
        self.entries.truncate(CACHE_ENTRIES - 1);
        self.entries.push_front(entry);
        uniform
    }
}

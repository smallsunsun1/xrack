//! Reuse GPU keyframes while simulating and scrubbing particles.
use std::{borrow::Cow, collections::VecDeque};

use xrack_runtime::scene3d::ParticleEmissionTrack;

use super::{append_floats, buffer};

pub(super) const TRACK_BINDING: u32 = 10;
const CACHE_ENTRIES: usize = 12;

#[derive(Default)]
pub(super) struct EmissionTrackBuffers {
    entries: VecDeque<(ParticleEmissionTrack, wgpu::Buffer)>,
    pub uploads: u64,
}

impl EmissionTrackBuffers {
    pub fn get(
        &mut self,
        device: &wgpu::Device,
        track: &ParticleEmissionTrack,
    ) -> Option<wgpu::Buffer> {
        let bytes = std::mem::size_of_val(track.keys()) as u64;
        let limits = device.limits();
        if bytes == 0
            || bytes > limits.max_storage_buffer_binding_size
            || bytes > limits.max_buffer_size
        {
            return None;
        }
        let entry = self
            .entries
            .iter()
            .position(|(cached, _)| cached == track)
            .and_then(|index| self.entries.remove(index))
            .unwrap_or_else(|| {
                let mut data = Vec::with_capacity(bytes as usize);
                append_floats(&mut data, track.keys().iter().flatten().copied());
                let keys = buffer(
                    device,
                    "XRack particle birth keys",
                    &data,
                    wgpu::BufferUsages::STORAGE,
                );
                self.uploads = self.uploads.saturating_add(1);
                (track.clone(), keys)
            });
        let keys = entry.1.clone();
        self.entries.truncate(CACHE_ENTRIES - 1);
        self.entries.push_front(entry);
        Some(keys)
    }
}

pub(super) fn particles_shader(tracked: bool) -> Cow<'static, str> {
    let source = include_str!("particles.wgsl");
    if !tracked {
        return Cow::Borrowed(source);
    }
    let mut source = source.replacen(
        "// XRack particle birth track",
        "if motion.modes.w >= 2u { return tracked_particle_birth(particle, motion, origin, velocity); }",
        1,
    );
    source.push_str(include_str!("particle_birth_track.wgsl"));
    Cow::Owned(source)
}

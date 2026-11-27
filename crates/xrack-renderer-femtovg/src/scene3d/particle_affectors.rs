//! Reuse immutable affector uniforms and area-weighted target geometry.
use std::{
    collections::VecDeque,
    sync::{Arc, Weak},
};

use xrack_runtime::scene3d::{MAX_PARTICLE_AFFECTORS, ParticleAffector, ParticleMeshSurface};

use super::{
    append_floats, buffer, particle_follow::JOB_BINDING, particle_meshes::MeshSurfaceBuffers,
};

const AFFECTOR_BYTES: usize = 160;
const WANDER_BYTES: usize = 32;
const SHAPE_BYTES: usize = 80;
const CACHE_ENTRIES: usize = 4;
const MAX_COMBINED_MESH_BYTES: u64 = 40 * 1024 * 1024;
pub(super) const AFFECTOR_MESH_BINDING: u32 = JOB_BINDING + 1;

pub(super) struct AffectorBindings {
    uniform: wgpu::Buffer,
    triangles: Option<wgpu::Buffer>,
}

impl AffectorBindings {
    pub fn has_mesh(bindings: Option<&Self>) -> bool {
        bindings.is_some_and(|bindings| bindings.triangles.is_some())
    }

    pub fn entries(&self) -> impl Iterator<Item = wgpu::BindGroupEntry<'_>> {
        std::iter::once(wgpu::BindGroupEntry {
            binding: 6,
            resource: self.uniform.as_entire_binding(),
        })
        .chain(self.triangles.iter().map(|triangles| wgpu::BindGroupEntry {
            binding: AFFECTOR_MESH_BINDING,
            resource: triangles.as_entire_binding(),
        }))
    }
}

struct TargetBank {
    surfaces: Vec<Weak<ParticleMeshSurface>>,
    triangles: wgpu::Buffer,
}

#[derive(Default)]
pub(super) struct AffectorBuffers {
    entries: VecDeque<(Vec<ParticleAffector>, wgpu::Buffer)>,
    banks: VecDeque<TargetBank>,
    pub uploads: u64,
}

impl AffectorBuffers {
    pub fn get(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        resident: &mut MeshSurfaceBuffers,
        affectors: &[&ParticleAffector],
    ) -> AffectorBindings {
        let (triangles, offsets) = self.target_geometry(device, encoder, resident, affectors);
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
                let mut bytes = Vec::with_capacity(
                    (AFFECTOR_BYTES + WANDER_BYTES + SHAPE_BYTES) * MAX_PARTICLE_AFFECTORS,
                );
                for (index, affector) in affectors.iter().enumerate() {
                    append_floats(&mut bytes, affector.position_age);
                    append_floats(&mut bytes, affector.vector_value);
                    append_floats(&mut bytes, affector.time_bounds);
                    let mut control = affector.control;
                    let shaped = affector.shape.kind != 0;
                    control[3] = u32::from(affector.frame.is_some()) | (u32::from(shaped) << 1);
                    if affector.shape_mesh.is_some() && offsets[index][1] == 0 {
                        control[0] = 0;
                    }
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
                bytes.resize(AFFECTOR_BYTES * MAX_PARTICLE_AFFECTORS, 0);
                for affector in affectors {
                    append_floats(&mut bytes, affector.wander.into_iter().flatten());
                }
                bytes.resize((AFFECTOR_BYTES + WANDER_BYTES) * MAX_PARTICLE_AFFECTORS, 0);
                for (affector, [first, count]) in affectors.iter().zip(offsets) {
                    let shape = affector.shape;
                    append_floats(&mut bytes, shape.axes.into_iter().flatten());
                    append_floats(&mut bytes, shape.weights);
                    let count = affector
                        .shape_mesh
                        .as_ref()
                        .filter(|_| count != 0)
                        .map_or(count, |surface| surface.sample_count());
                    for word in [shape.kind, u32::from(shape.surface), first, count] {
                        bytes.extend_from_slice(&word.to_ne_bytes());
                    }
                }
                bytes.resize(
                    (AFFECTOR_BYTES + WANDER_BYTES + SHAPE_BYTES) * MAX_PARTICLE_AFFECTORS,
                    0,
                );
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
        AffectorBindings { uniform, triangles }
    }

    fn target_geometry(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        resident: &mut MeshSurfaceBuffers,
        affectors: &[&ParticleAffector],
    ) -> (Option<wgpu::Buffer>, [[u32; 2]; MAX_PARTICLE_AFFECTORS]) {
        let mut offsets = [[0, 0]; MAX_PARTICLE_AFFECTORS];
        if !affectors.iter().any(|field| field.shape_mesh.is_some()) {
            return (None, offsets);
        }
        let limits = device.limits();
        // The most demanding compute path can also have source mesh, birth
        // track, draw order and output buffers bound in the same stage.
        if limits.max_storage_buffers_per_shader_stage < 5 {
            return (None, offsets);
        }
        let budget = MAX_COMBINED_MESH_BYTES
            .min(limits.max_storage_buffer_binding_size)
            .min(limits.max_buffer_size);
        let mut bytes = 0;
        let mut count = 0;
        let mut sources = Vec::new();
        for (index, surface) in affectors
            .iter()
            .enumerate()
            .filter_map(|(index, field)| Some((index, field.shape_mesh.as_ref()?)))
        {
            if let Some((_, span)) = sources
                .iter()
                .find(|(source, _)| Arc::ptr_eq(source, surface))
            {
                offsets[index] = *span;
                continue;
            }
            let size = surface.bytes().len() as u64;
            if bytes + size > budget {
                continue;
            }
            let span = [count, surface.record_count()];
            count += span[1];
            bytes += size;
            sources.push((surface.clone(), span));
            offsets[index] = span;
        }
        let Some((first, rest)) = sources.split_first() else {
            return (None, offsets);
        };
        if rest.is_empty() {
            let buffer = resident.get(device, &first.0).map(|binding| binding.buffer);
            return (buffer, offsets);
        }
        let cached = self.banks.iter().position(|bank| {
            bank.surfaces.len() == sources.len()
                && bank
                    .surfaces
                    .iter()
                    .zip(&sources)
                    .all(|(weak, (strong, _))| weak.as_ptr() == Arc::as_ptr(strong))
        });
        let bank = cached
            .and_then(|index| self.banks.remove(index))
            .unwrap_or_else(|| {
                let triangles = device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("XRack mesh attractor triangle bank"),
                    size: bytes,
                    usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let mut offset = 0;
                for (surface, _) in &sources {
                    let size = surface.bytes().len() as u64;
                    if let Some(binding) = resident.get(device, surface) {
                        encoder.copy_buffer_to_buffer(&binding.buffer, 0, &triangles, offset, size);
                    }
                    offset += size;
                }
                TargetBank {
                    surfaces: sources
                        .iter()
                        .map(|(surface, _)| Arc::downgrade(surface))
                        .collect(),
                    triangles,
                }
            });
        let result = bank.triangles.clone();
        let mut used = bank.triangles.size();
        self.banks.retain(|entry| {
            if entry.surfaces.iter().any(|weak| weak.strong_count() == 0) {
                return false;
            }
            used += entry.triangles.size();
            used <= MAX_COMBINED_MESH_BYTES
        });
        self.banks.truncate(CACHE_ENTRIES - 1);
        self.banks.push_front(bank);
        (Some(result), offsets)
    }
}

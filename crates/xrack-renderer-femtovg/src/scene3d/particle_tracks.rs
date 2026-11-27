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

pub(super) fn particles_shader(
    tracked: bool,
    world_oriented: bool,
    sorted: bool,
    followed: bool,
    mesh_targets: bool,
) -> Cow<'static, str> {
    let mut source = Cow::Borrowed(include_str!("particles.wgsl"));
    if tracked {
        let mut tracked_source = source.replacen(
            "// XRack particle birth track",
            "if motion.modes.w >= 2u { return tracked_particle_birth(particle, motion, origin, velocity, normal); }",
            1,
        );
        tracked_source.push_str(include_str!("particle_birth_track.wgsl"));
        source = Cow::Owned(tracked_source);
    }
    if world_oriented {
        source = Cow::Owned(source.replacen(
            "// XRack oriented particle sprite",
            "if (emitter.indices.z & 1u) != 0u {
                let orientation = particle_orientation(source, poses.settings[source.particle.slot]);
                world = vec4<f32>(center + orientation * vec3<f32>(coordinates, 0.0), 1.0);
                offset = vec4<f32>(0.0);
            }",
            1,
        ).replacen(
            "// XRack oriented particle ribbon",
            "if (particle.emitter.indices.z & 1u) != 0u {\n                return world_trail_projection(particle, point, younger, older, start_velocity, side);\n            }",
            1,
        ).replacen(
            "let displaced = apply_affectors(source); // XRack sprite motion orientation",
            "let displaced = oriented_displacement(source);",
            1,
        ).replacen("@location(2) @interpolate(flat) depth", "@location(2) depth", 1));
    }
    if sorted {
        let mut ordered = source.replacen(
            "return draw_index; // XRack particle draw order",
            "return draw_order[draw_index].y;",
            1,
        );
        ordered.push_str(
            "\n@group(0) @binding(11) var<storage, read> draw_order: array<vec2<u32>>;\n",
        );
        source = Cow::Owned(ordered);
    }
    if followed {
        let mut anchored = source.replacen(
            "return birth; // XRack particle follow origin",
            "let offset = i32(birth.particle.emitter.appearance.w);
            let inherits = (birth.particle.emitter.indices.z & 256u) != 0u;
            let stride = select(1u, 3u, inherits);
            let address = u32(i32(particle_source_index(index)) + offset) * stride;
            var placed = birth;
            placed.origin += follow_origins[address].xyz;
            if inherits {
                let inherited_normal = follow_origins[address + 1u].xyz;
                let has_surface = any(inherited_normal != vec3<f32>(0.0));
                if shape_kind == 0u && has_surface {
                    placed.normal = inherited_normal;
                }
                if velocity_mode == 5u && (shape_kind != 0u || has_surface) {
                    let incoming = follow_origins[address + 2u].xyz;
                    if any(incoming != vec3<f32>(0.0)) {
                        placed.velocity = incoming * length(placed.velocity);
                    }
                }
            }
            return placed;",
            1,
        );
        use std::fmt::Write as _;
        let _ = writeln!(
            anchored,
            "\n@group(0) @binding({}) var<storage, read> follow_origins: array<vec4<f32>>;",
            super::particle_follow::FOLLOW_BINDING
        );
        source = Cow::Owned(anchored);
    }
    if mesh_targets {
        let mut targets = source.replacen(
            "// XRack particle target mesh",
            r#"if shape.modes.x == 5u {
                let count = select(shape.modes.w, min(shape.modes.w, limit), limit > 0u);
                let point = tick % max(count, 1u);
                return surface_point(target_triangles[shape.modes.z + point / 4u], point);
            }
            if shape.modes.x == 4u {
                let start = shape.modes.z;
                let count = shape.modes.w;
                let random = seed ^ 0x3cd9790du;
                let pick = random3(random);
                let column = min(u32(pick.x * f32(count)), count - 1u);
                let entry = target_triangles[start + column];
                let selected = select(entry.alternate, column, pick.y < entry.probability);
                return triangle_point(target_triangles[start + selected], pick.z, random);
            }"#,
            1,
        );
        use std::fmt::Write as _;
        let _ = writeln!(
            targets,
            "\n@group(0) @binding({}) var<storage, read> target_triangles: array<SurfaceTriangle>;",
            super::particle_affectors::AFFECTOR_MESH_BINDING
        );
        source = Cow::Owned(targets);
    }
    source
}

// Appended to particles.wgsl for model-instance compute pipelines.
struct ModelSettings {
    rotation: vec4<f32>,
    variation: vec4<f32>,
    spin: vec4<f32>,
    spin_variation: vec4<f32>,
}

struct ParticleModels {
    settings: array<ModelSettings, 16>,
}

@group(0) @binding(8) var<storage, read_write> instances: array<u32>;
@group(0) @binding(9) var<uniform> models: ParticleModels;

fn model_tint(source: ParticleCenter) -> vec4<f32> {
    let particle = source.particle;
    let size = vec2<f32>(textureDimensions(color_table));
    let row = floor(random01(particle.seed ^ 0x510e527fu) * size.y);
    let age = clamp(particle.seconds / particle.emitter.spread_lifetime.w, 0.0, 1.0);
    let uv = vec2<f32>(0.5 + age * (size.x - 1.0), row + 0.5) / size;
    let color = textureSampleLevel(color_table, sheet_sampler, uv, 0.0);
    return vec4<f32>(color.rgb / max(color.a, 1e-6), color.a);
}

fn instance_srgb(rgb: vec3<f32>) -> vec3<f32> {
    return select(12.92 * rgb, 1.055 * pow(rgb, vec3<f32>(1.0 / 2.4)) - vec3<f32>(0.055),
        rgb > vec3<f32>(0.0031308));
}

fn write_model(index: u32, source: ParticleCenter, visible: bool, tint: vec4<f32>) {
    let particle = source.particle;
    let emitter = particle.emitter;
    let settings = models.settings[particle.slot];
    let age = particle.seconds;
    let fraction = clamp(age / emitter.spread_lifetime.w, 0.0, 1.0);
    let alpha = select(0.0, base_alpha(emitter, age) * tint.a, visible && age < emitter.spread_lifetime.w);
    let scale = select(0.0, 2.0 * mix(emitter.position_radius.w, emitter.velocity_end_radius.w, fraction), alpha > 0.0);
    let variation = (2.0 * random3(particle.seed ^ 0x3141592du) - 1.0) * settings.variation.xyz;
    var spin = settings.spin.xyz;
    if any(settings.spin_variation.xyz != vec3<f32>(0.0)) {
        spin += (2.0 * random3(particle.seed ^ 0x75d77a21u) - 1.0) * settings.spin_variation.xyz;
    }
    let angles = fract((settings.rotation.xyz + variation + age * spin) / vec3<f32>(360.0)) * 6.28318530718;
    let c = cos(angles);
    let s = sin(angles);
    // X, then Y, then Z rotation, in the same order as authored model poses.
    let row_x = scale * vec3<f32>(c.z * c.y, c.z * s.y * s.x - s.z * c.x, c.z * s.y * c.x + s.z * s.x);
    let row_y = scale * vec3<f32>(s.z * c.y, s.z * s.y * s.x + c.z * c.x, s.z * s.y * c.x - c.z * s.x);
    let row_z = scale * vec3<f32>(-s.y, c.y * s.x, c.y * c.x);
    let color = instance_srgb(emitter.color.rgb * tint.rgb);
    // Match the 72-byte scene geometry instance stream exactly.
    let address = index * 18u;
    instances[address] = bitcast<u32>(row_x.x);
    instances[address + 1u] = bitcast<u32>(row_x.y);
    instances[address + 2u] = bitcast<u32>(row_x.z);
    instances[address + 3u] = bitcast<u32>(source.center.x);
    instances[address + 4u] = bitcast<u32>(row_y.x);
    instances[address + 5u] = bitcast<u32>(row_y.y);
    instances[address + 6u] = bitcast<u32>(row_y.z);
    instances[address + 7u] = bitcast<u32>(source.center.y);
    instances[address + 8u] = bitcast<u32>(row_z.x);
    instances[address + 9u] = bitcast<u32>(row_z.y);
    instances[address + 10u] = bitcast<u32>(row_z.z);
    instances[address + 11u] = bitcast<u32>(source.center.z);
    instances[address + 12u] = bitcast<u32>(color.x);
    instances[address + 13u] = bitcast<u32>(color.y);
    instances[address + 14u] = bitcast<u32>(color.z);
    instances[address + 15u] = bitcast<u32>(alpha);
    instances[address + 16u] = (index << 3u) | u32(settings.spin.w);
    instances[address + 17u] = 0u;
}

@compute @workgroup_size(64)
fn compute_vector(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(vector_source(gid.x));
        write_model(gid.x, source, true, vec4<f32>(1.0));
    }
}

@compute @workgroup_size(64)
fn compute_vector_tinted(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(vector_source(gid.x));
        write_model(gid.x, source, true, model_tint(source));
    }
}

@compute @workgroup_size(64)
fn compute_vector_affected(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(vector_source(gid.x));
        let displaced = apply_affectors(source);
        write_model(gid.x, displaced.source, displaced.visible, vec4<f32>(1.0));
    }
}

@compute @workgroup_size(64)
fn compute_vector_affected_tinted(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(vector_source(gid.x));
        let displaced = apply_affectors(source);
        write_model(gid.x, displaced.source, displaced.visible, model_tint(displaced.source));
    }
}

@compute @workgroup_size(64)
fn compute_spatial(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(spatial_source(gid.x));
        write_model(gid.x, source, true, vec4<f32>(1.0));
    }
}

@compute @workgroup_size(64)
fn compute_spatial_tinted(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(spatial_source(gid.x));
        write_model(gid.x, source, true, model_tint(source));
    }
}

@compute @workgroup_size(64)
fn compute_spatial_affected(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(spatial_source(gid.x));
        let displaced = apply_affectors(source);
        write_model(gid.x, displaced.source, displaced.visible, vec4<f32>(1.0));
    }
}

@compute @workgroup_size(64)
fn compute_spatial_affected_tinted(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(spatial_source(gid.x));
        let displaced = apply_affectors(source);
        write_model(gid.x, displaced.source, displaced.visible, model_tint(displaced.source));
    }
}

@compute @workgroup_size(64)
fn compute_mesh(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(mesh_source(gid.x));
        write_model(gid.x, source, true, vec4<f32>(1.0));
    }
}

@compute @workgroup_size(64)
fn compute_mesh_tinted(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(mesh_source(gid.x));
        write_model(gid.x, source, true, model_tint(source));
    }
}

@compute @workgroup_size(64)
fn compute_mesh_affected(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(mesh_source(gid.x));
        let displaced = apply_affectors(source);
        write_model(gid.x, displaced.source, displaced.visible, vec4<f32>(1.0));
    }
}

@compute @workgroup_size(64)
fn compute_mesh_affected_tinted(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(mesh_source(gid.x));
        let displaced = apply_affectors(source);
        write_model(gid.x, displaced.source, displaced.visible, model_tint(displaced.source));
    }
}


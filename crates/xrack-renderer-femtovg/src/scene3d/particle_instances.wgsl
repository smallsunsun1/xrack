// Appended to particles.wgsl for model-instance compute pipelines.
@group(0) @binding(8) var<storage, read_write> instances: array<u32>;

fn model_tint(source: ParticleCenter) -> vec4<f32> {
    let particle = source.particle;
    let size = vec2<f32>(textureDimensions(color_table));
    let row = floor(random01(particle.seed ^ 0x510e527fu) * size.y);
    let age = clamp(source.age / particle.emitter.spread_lifetime.w, 0.0, 1.0);
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
    let settings = poses.settings[particle.slot];
    let age = source.age;
    let fraction = clamp(age / emitter.spread_lifetime.w, 0.0, 1.0);
    let fade = particle_fade(emitter, age);
    let alpha = select(0.0, fade.x * tint.a, visible && particle.seconds < emitter.spread_lifetime.w && fade.y > 0.0);
    let scale = select(0.0, 2.0 * mix(emitter.position_radius.w, emitter.velocity_end_radius.w, fraction) * fade.y, alpha > 0.0);
    let rows = transpose(particle_orientation(source, settings));
    let row_x = scale * rows[0];
    let row_y = scale * rows[1];
    let row_z = scale * rows[2];
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
    instances[address + 16u] = (particle_source_index(index) << 3u) | u32(settings.spin.w);
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
        let displaced = oriented_displacement(source);
        write_model(gid.x, displaced.source, displaced.visible, vec4<f32>(1.0));
    }
}

@compute @workgroup_size(64)
fn compute_vector_affected_tinted(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(vector_source(gid.x));
        let displaced = oriented_displacement(source);
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
        let displaced = oriented_displacement(source);
        write_model(gid.x, displaced.source, displaced.visible, vec4<f32>(1.0));
    }
}

@compute @workgroup_size(64)
fn compute_spatial_affected_tinted(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(spatial_source(gid.x));
        let displaced = oriented_displacement(source);
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
        let displaced = oriented_displacement(source);
        write_model(gid.x, displaced.source, displaced.visible, vec4<f32>(1.0));
    }
}

@compute @workgroup_size(64)
fn compute_mesh_affected_tinted(@builtin(global_invocation_id) gid: vec3<u32>) {
    if gid.x < arrayLength(&instances) / 18u {
        let source = present_position(mesh_source(gid.x));
        let displaced = oriented_displacement(source);
        write_model(gid.x, displaced.source, displaced.visible, model_tint(displaced.source));
    }
}


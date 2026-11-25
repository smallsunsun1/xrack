struct TextureTransform {
    // First three lanes are the affine rows. W flags skip identity work.
    u: vec4<f32>,
    v: vec4<f32>,
}

fn texture_uv(transform: TextureTransform, uv: vec2<f32>) -> vec2<f32> {
    if transform.u.w == 0.0 {
        return uv;
    }
    let point = vec3<f32>(uv, 1.0);
    return vec2<f32>(dot(transform.u.xyz, point), dot(transform.v.xyz, point));
}

fn texture_gradient(transform: TextureTransform, gradient: vec2<f32>) -> vec2<f32> {
    if transform.v.w == 0.0 {
        return gradient;
    }
    return vec2<f32>(dot(transform.u.xy, gradient), dot(transform.v.xy, gradient));
}


fn base_texture_uses_alpha(channels: vec4<u32>) -> bool {
    // Scalar base-color selectors replicate a channel and use alpha one.
    return (channels.x & 15u) == 4u;
}

fn coverage_vertex_alpha(channels: vec4<u32>, vertex_alpha: f32) -> f32 {
    return select(vertex_alpha, 1.0, (channels.z & 0x80000000u) != 0u);
}

// Blend mode 2 takes opacity from the instance and tests the underlying
// material's mask separately. The other modes use the combined coverage.
fn material_coverage(mode: f32, surface_alpha: f32, tint_alpha: f32) -> vec2<f32> {
    let coverage = surface_alpha * tint_alpha;
    return select(vec2(coverage), vec2(tint_alpha, surface_alpha), mode > 1.5);
}

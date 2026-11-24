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

fn coverage_vertex_alpha(channels: vec4<u32>, alpha: vec2<f32>) -> f32 {
    // X includes the interpolated vertex alpha. Y carries only the instance alpha.
    return select(alpha.x, alpha.y, (channels.z & 0x80000000u) != 0u);
}

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

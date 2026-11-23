@group(0) @binding(3) var surfaces: XRACK_SCREEN_TEXTURE<f32>;
@group(0) @binding(4) var brdf: XRACK_SCREEN_TEXTURE<f32>;

fn surface_at(pixel: vec2<i32>, sample: u32) -> vec4<f32> {
    let value = textureLoad(surfaces, pixel, sample);
    if MULTISAMPLED_SURFACES && any(value != vec4<f32>(0.0)) {
        return vec4<f32>(normalize(value.xyz * 2.0 - 1.0), value.w);
    }
    return value;
}

fn response_at(pixel: vec2<i32>, sample: u32) -> vec3<f32> {
    let value = textureLoad(brdf, pixel, sample).rgb;
    return select(value, value * value, MULTISAMPLED_SURFACES);
}

fn indirect_delta(position: vec2<f32>, sample: u32) -> vec3<f32> {
    return vec3<f32>(0.0);
}

fn sample_headroom(pixel: vec2<i32>, sample: u32) -> f32 {
    return 65000.0;
}

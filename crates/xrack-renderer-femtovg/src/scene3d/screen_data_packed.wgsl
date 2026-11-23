@group(0) @binding(3) var surfaces: XRACK_SCREEN_TEXTURE<u32>;
@group(0) @binding(4) var factors: XRACK_SCREEN_TEXTURE<u32>;

fn surface_at(pixel: vec2<i32>, sample: u32) -> vec4<f32> {
    let data = textureLoad(surfaces, pixel, sample);
    if (data.z & 16384u) == 0u {
        return vec4<f32>(0.0);
    }
    return vec4<f32>(unpack_screen_normal(data.x), f32(data.z & 16383u) / 16383.0);
}

fn response_at(pixel: vec2<i32>, sample: u32) -> vec3<f32> {
    let encoded = vec3<f32>(textureLoad(factors, pixel, sample).rgb & vec3<u32>(255u)) / 255.0;
    return encoded * encoded;
}

fn indirect_normal(pixel: vec2<i32>, sample: u32) -> vec3<f32> {
    let data = textureLoad(surfaces, pixel, sample);
    if (data.z & 32768u) == 0u {
        return vec3<f32>(0.0);
    }
    return unpack_screen_normal(data.y);
}

fn diffuse_response(pixel: vec2<i32>, sample: u32) -> vec3<f32> {
    let encoded = vec3<f32>(textureLoad(factors, pixel, sample).rgb >> vec3<u32>(8u)) / 255.0;
    return encoded * encoded;
}

fn ambient_at(pixel: vec2<i32>, sample: u32) -> vec3<f32> {
    let low = textureLoad(surfaces, pixel, sample).w;
    let high = textureLoad(factors, pixel, sample).w;
    return unpack_ambient(low | (high << 16u));
}

// The fourth probe lane carries the original scene-linear color peak,
// which bounds signed additive blending into a half-float MSAA sample.
fn sample_headroom(pixel: vec2<i32>, sample: u32) -> f32 {
    return max(65000.0 - textureLoad(probe, pixel, sample).a, 0.0);
}

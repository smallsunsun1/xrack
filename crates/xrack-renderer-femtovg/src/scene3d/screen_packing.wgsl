// Keep both screen effects within the four-attachment forward pass.
fn fold_screen_normal(plane: vec2<f32>) -> vec2<f32> {
    return (1.0 - abs(plane.yx)) * select(vec2<f32>(-1.0), vec2<f32>(1.0), plane >= vec2<f32>(0.0));
}

fn encode_screen_normal(n: vec3<f32>) -> vec2<f32> {
    let plane = n.xy / dot(abs(n), vec3<f32>(1.0));
    return select(plane, fold_screen_normal(plane), n.z < 0.0);
}

fn decode_screen_normal(plane: vec2<f32>) -> vec3<f32> {
    let z = 1.0 - dot(abs(plane), vec2<f32>(1.0));
    return normalize(vec3<f32>(select(plane, fold_screen_normal(plane), z < 0.0), z));
}

fn pack_screen_normal(n: vec3<f32>) -> u32 {
    return pack4x8unorm(vec4<f32>(encode_screen_normal(n) * 0.5 + 0.5, 0.0, 0.0));
}

fn unpack_screen_normal(bits: u32) -> vec3<f32> {
    return decode_screen_normal(unpack4x8unorm(bits).xy * 2.0 - 1.0);
}

fn pack_ambient(rgb: vec3<f32>) -> u32 {
    let energy = max(rgb.r, max(rgb.g, rgb.b));
    if energy <= 1e-8 {
        return 0u;
    }
    let exponent = ceil(log2(energy));
    return pack4x8unorm(vec4<f32>(rgb * exp2(-exponent), (128.0 + exponent) / 255.0));
}

fn unpack_ambient(bits: u32) -> vec3<f32> {
    let rgbe = unpack4x8unorm(bits);
    return rgbe.rgb * exp2(round(rgbe.a * 255.0) - 128.0);
}

fn encode_srgb(linear: vec3<f32>) -> vec3<f32> {
    return select(1.055 * pow(max(linear, vec3<f32>(0.0)), vec3<f32>(1.0 / 2.4)) - 0.055,
                  linear * 12.92, linear <= vec3<f32>(0.0031308));
}

fn decode_srgb(encoded: vec3<f32>) -> vec3<f32> {
    return select(pow(max((encoded + 0.055) / 1.055, vec3<f32>(0.0)), vec3<f32>(2.4)),
                  encoded / 12.92, encoded <= vec3<f32>(0.04045));
}

fn ordered_dither(position: vec2<f32>) -> f32 {
    let tile = vec2<u32>(position) % vec2<u32>(4u);
    let bayer = array<f32, 16>(
        0.0, 8.0, 2.0, 10.0, 12.0, 4.0, 14.0, 6.0,
        3.0, 11.0, 1.0, 9.0, 15.0, 7.0, 13.0, 5.0);
    return ((bayer[tile.y * 4u + tile.x] + 0.5) / 16.0 - 0.5) / 255.0;
}

fn dither_display_rgb(
    encoded: vec3<f32>, alpha: f32, position: vec2<f32>, enabled: bool,
) -> vec3<f32> {
    if enabled {
        let noise = ordered_dither(position) * alpha;
        return clamp(encoded + vec3<f32>(noise), vec3<f32>(0.0), vec3<f32>(alpha));
    }
    return encoded;
}

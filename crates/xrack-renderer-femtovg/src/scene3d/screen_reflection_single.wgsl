const MULTISAMPLED_SURFACES: bool = false;
@group(0) @binding(2) var depth_buffer: texture_depth_2d;
@group(0) @binding(5) var probe: texture_2d<f32>;

fn scene_depth(pixel: vec2<i32>) -> f32 {
    return textureLoad(depth_buffer, pixel, 0);
}

fn sample_depth(pixel: vec2<i32>, sample: u32) -> f32 {
    return scene_depth(pixel);
}

fn nearest_sample(pixel: vec2<i32>) -> u32 {
    return 0u;
}

fn probe_at(pixel: vec2<i32>, sample: u32) -> vec3<f32> {
    return textureLoad(probe, pixel, 0).rgb;
}

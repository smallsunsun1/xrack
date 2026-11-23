const MULTISAMPLED_SURFACES: bool = true;
@group(0) @binding(2) var depth_buffer: texture_depth_multisampled_2d;
@group(0) @binding(5) var probe: texture_multisampled_2d<f32>;
@group(0) @binding(7) var<storage, read> opaque_depth: array<f32>;

fn scene_depth(pixel: vec2<i32>) -> f32 {
    return opaque_depth[u32(pixel.y) * u32(settings.extents.x) + u32(pixel.x)];
}

fn sample_depth(pixel: vec2<i32>, sample: u32) -> f32 {
    return textureLoad(depth_buffer, pixel, sample);
}

fn nearest_sample(pixel: vec2<i32>) -> u32 {
    var nearest = 0u;
    var depth = sample_depth(pixel, 0u);
    for (var sample = 1u; sample < textureNumSamples(depth_buffer); sample += 1u) {
        let z = sample_depth(pixel, sample);
        if z < depth {
            nearest = sample;
            depth = z;
        }
    }
    return nearest;
}

fn probe_at(pixel: vec2<i32>, sample: u32) -> vec3<f32> {
    return textureLoad(probe, pixel, sample).rgb;
}

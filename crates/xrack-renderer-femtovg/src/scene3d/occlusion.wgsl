struct Settings {
    depth_mapping: ClipDepthMapping,
    // Sampling radius and self-shadow bias.
    options: vec4<f32>,
}

@group(0) @binding(0) var depth_buffer: texture_depth_2d;
@group(0) @binding(1) var<uniform> settings: Settings;

struct OcclusionSamples {
    size: vec2<u32>,
    padding: vec2<u32>,
    samples: array<vec2<f32>>,
}

@group(0) @binding(2) var<storage, read_write> output_samples: OcclusionSamples;

fn orthographic() -> bool {
    return settings.depth_mapping.distances.z > 0.5;
}

fn view_point(pixel: vec2<i32>, size: vec2<i32>, depth: f32) -> vec3<f32> {
    let uv = (vec2<f32>(pixel) + 0.5) / vec2<f32>(size);
    return view_point_from_clip(settings.depth_mapping, uv, depth);
}

fn delta(pixel: vec2<i32>, size: vec2<i32>, center: vec3<f32>) -> vec3<f32> {
    let at = clamp(pixel, vec2<i32>(0), size - 1);
    let z = textureLoad(depth_buffer, at, 0);
    return view_point(at, size, z) - center;
}

fn evaluate(pixel: vec2<i32>, size: vec2<i32>) -> vec2<f32> {
    let depth = textureLoad(depth_buffer, pixel, 0);
    if is_clear_clip_depth(depth) {
        return vec2<f32>(0.0);
    }
    let center = view_point(pixel, size, depth);
    let left = delta(pixel - vec2<i32>(1, 0), size, center);
    let right = delta(pixel + vec2<i32>(1, 0), size, center);
    let up = delta(pixel - vec2<i32>(0, 1), size, center);
    let down = delta(pixel + vec2<i32>(0, 1), size, center);
    // Avoid using the far side of a depth discontinuity to build a normal.
    let dx = select(-left, right, dot(right, right) <= dot(left, left) || pixel.x == 0);
    let dy = select(-up, down, dot(down, down) <= dot(up, up) || pixel.y == 0);
    let facing = cross(dx, dy);
    if dot(facing, facing) <= 1e-20 {
        return vec2<f32>(0.0, center.z);
    }
    let n = normalize(facing);
    let radius = settings.options.x;
    var pixel_radius = radius * settings.depth_mapping.lens.xy * vec2<f32>(size) * 0.5
        / select(max(center.z, 1e-6), 1.0, orthographic());
    if MATRIX_CLIP_PROJECTION {
        let mapping = settings.depth_mapping;
        let clip = matrix_clip_from_view(mapping, center);
        let ndc = clip.xy / clip.w;
        let dx = mapping.view_to_clip[0].xyz - ndc.x * mapping.view_to_clip[3].xyz;
        let dy = mapping.view_to_clip[1].xyz - ndc.y * mapping.view_to_clip[3].xyz;
        pixel_radius = radius * vec2<f32>(length(dx), length(dy)) * vec2<f32>(size) * (0.5 / clip.w);
    }
    let extent = clamp(pixel_radius, vec2<f32>(1.0), vec2<f32>(64.0));
    // A small deterministic spiral covers the disk with a fixed sample cost.
    let angle = f32((u32(pixel.x) * 13u + u32(pixel.y) * 7u) & 7u) * 0.7853982;
    var obscurance = 0.0;
    for (var i = 0u; i < 16u; i += 1u) {
        let theta = angle + f32(i) * 2.3999632;
        let r = (0.22 + 0.78 * (f32(i) + 0.5) / 16.0) * extent;
        let step = vec2<i32>(round(vec2<f32>(cos(theta), sin(theta)) * r));
        let at = pixel + step;
        if all(step == vec2<i32>(0)) || any(at < vec2<i32>(0)) || any(at >= size) {
            continue;
        }
        let z = textureLoad(depth_buffer, at, 0);
        if is_clear_clip_depth(z) {
            continue;
        }
        let difference = (view_point(at, size, z) - center) / radius;
        let length_squared = dot(difference, difference);
        if length_squared <= 1e-12 || length_squared >= 1.0 {
            continue;
        }
        let hemisphere = max(dot(n, difference) - settings.options.y / radius, 0.0)
            * inverseSqrt(length_squared);
        let falloff = 1.0 - length_squared;
        obscurance += hemisphere * falloff * falloff;
    }
    return vec2<f32>(clamp(obscurance * (3.0 / 16.0), 0.0, 1.0), center.z);
}

@compute @workgroup_size(8, 8)
fn compute(@builtin(global_invocation_id) id: vec3<u32>) {
    let size = textureDimensions(depth_buffer);
    if any(id.xy >= size) {
        return;
    }
    if all(id.xy == vec2<u32>(0u)) {
        output_samples.size = size;
    }
    output_samples.samples[id.y * size.x + id.x] = evaluate(vec2<i32>(id.xy), vec2<i32>(size));
}

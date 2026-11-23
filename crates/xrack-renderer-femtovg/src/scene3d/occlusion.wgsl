struct Settings {
    // Horizontal and vertical clip scale, near and far distance.
    projection: vec4<f32>,
    // Sampling radius, self-shadow bias, orthographic flag.
    options: vec4<f32>,
    shift: vec4<f32>,
}

@group(0) @binding(0) var depth_buffer: texture_depth_2d;
@group(0) @binding(1) var<uniform> settings: Settings;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn orthographic() -> bool {
    return settings.options.z > 0.5;
}

fn view_point(pixel: vec2<i32>, size: vec2<i32>, depth: f32) -> vec3<f32> {
    let near = settings.projection.z;
    let far = settings.projection.w;
    let ndc = (vec2<f32>(pixel) + 0.5) / vec2<f32>(size) * 2.0 - 1.0;
    let plane = (ndc * vec2<f32>(1.0, -1.0) - settings.shift.xy) / settings.projection.xy;
    if orthographic() {
        return vec3<f32>(plane, mix(near, far, depth));
    }
    var ratio = 0.0;
    if far > 0.0 {
        ratio = near / far;
    }
    let z = near / max((1.0 - depth) + depth * ratio, 0.000000000001);
    return vec3<f32>(plane * z, z);
}

fn delta(pixel: vec2<i32>, size: vec2<i32>, center: vec3<f32>) -> vec3<f32> {
    let at = clamp(pixel, vec2<i32>(0), size - 1);
    let z = textureLoad(depth_buffer, at, 0);
    return view_point(at, size, z) - center;
}

@fragment
fn fragment(@builtin(position) point: vec4<f32>) -> @location(0) vec2<f32> {
    let size = vec2<i32>(textureDimensions(depth_buffer));
    let pixel = vec2<i32>(point.xy);
    let depth = textureLoad(depth_buffer, pixel, 0);
    if depth >= 1.0 {
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
    let pixel_radius = radius * settings.projection.xy * vec2<f32>(size) * 0.5
        / select(max(center.z, 1e-6), 1.0, orthographic());
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
        if z >= 1.0 {
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

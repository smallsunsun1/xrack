struct Camera {
    clip: array<vec4<f32>, 4>,
    inverse_extent: vec2<f32>,
    depth_offset: vec2<f32>,
}

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var scene_depth: texture_depth_2d;
@group(0) @binding(2) var multisample_depth: texture_depth_multisampled_2d;
override REVERSE_DEVICE_DEPTH: bool = false;

// Corner bits select the maximum extent along X, Y and Z.
const EDGES = array<u32, 24>(
    0, 1, 2, 3, 4, 5, 6, 7,
    0, 2, 1, 3, 4, 6, 5, 7,
    0, 4, 1, 5, 2, 6, 3, 7,
);

struct LineVertex {
    @builtin(position) position: vec4<f32>,
    @location(0) @interpolate(flat) marker: u32,
}

@vertex
fn vertex(
    @builtin(vertex_index) index: u32,
    @location(0) minimum: vec3<f32>,
    @location(1) maximum: vec3<f32>,
    @location(2) capture_origin: vec3<f32>,
) -> LineVertex {
    var position: vec3<f32>;
    let marker = index >= 24u;
    if marker {
        let axis = (index - 24u) / 2u;
        let extent = maximum - minimum;
        let radius = min(extent.x, min(extent.y, extent.z)) / 16.0;
        let direction = vec3(f32(axis == 0u), f32(axis == 1u), f32(axis == 2u));
        position = capture_origin + direction * select(-radius, radius, index % 2u == 1u);
    } else {
        let corner = EDGES[index];
        position = select(minimum, maximum, vec3(
            (corner & 1u) != 0u, (corner & 2u) != 0u, (corner & 4u) != 0u));
    }
    let world = vec4(position, 1.0);
    return LineVertex(vec4(dot(camera.clip[0], world), dot(camera.clip[1], world),
        dot(camera.clip[2], world), dot(camera.clip[3], world)), u32(marker));
}

fn depth_pixel(point: vec2<f32>, size: vec2<u32>) -> vec2<i32> {
    let uv = point * camera.inverse_extent + camera.depth_offset;
    return clamp(vec2<i32>(floor(uv * vec2<f32>(size))), vec2(0), vec2<i32>(size) - 1);
}

fn guide_color(input: LineVertex, occluder: f32) -> vec4<f32> {
    let epsilon = select(0.00001, max(occluder * 0.00001, 1e-12), REVERSE_DEVICE_DEPTH);
    let occluded = select(input.position.z > occluder + epsilon,
        input.position.z < occluder - epsilon, REVERSE_DEVICE_DEPTH);
    if occluded {
        discard;
    }
    let rgb = select(vec3(0.015, 0.86, 1.0), vec3(1.0, 0.52, 0.025), input.marker != 0u);
    return vec4(rgb, 1.0);
}

@fragment
fn single_sample(input: LineVertex) -> @location(0) vec4<f32> {
    let pixel = depth_pixel(input.position.xy, textureDimensions(scene_depth));
    return guide_color(input, textureLoad(scene_depth, pixel, 0));
}

@fragment
fn multisampled(input: LineVertex) -> @location(0) vec4<f32> {
    let pixel = depth_pixel(input.position.xy, textureDimensions(multisample_depth));
    var closest = select(1.0, 0.0, REVERSE_DEVICE_DEPTH);
    for (var sample = 0u; sample < textureNumSamples(multisample_depth); sample++) {
        let value = textureLoad(multisample_depth, pixel, i32(sample));
        closest = select(min(closest, value), max(closest, value), REVERSE_DEVICE_DEPTH);
    }
    return guide_color(input, closest);
}

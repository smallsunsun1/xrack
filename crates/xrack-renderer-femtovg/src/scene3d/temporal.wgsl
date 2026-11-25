@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var previous: texture_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let position = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u)) * 2.0 - 1.0;
    return vec4<f32>(position, 0.0, 1.0);
}

@fragment
fn initial(@builtin(position) position: vec4<f32>) -> @location(0) vec4<f32> {
    return textureLoad(source, vec2<i32>(position.xy), 0);
}

struct HistoryAndColor {
    @location(0) raw_sample: vec4<f32>,
    @location(1) filtered: vec4<f32>,
}

@fragment
fn blend(@builtin(position) position: vec4<f32>) -> HistoryAndColor {
    let pixel = vec2<i32>(position.xy);
    let current = textureLoad(source, pixel, 0);
    let history = textureLoad(previous, pixel, 0);
    return HistoryAndColor(current, mix(current, history, 0.5));
}

// The same storage buffer supplies the previous vertex transforms and the
// correspondence from this frame's instance IDs to the last frame's IDs.
struct PreviousInstance {
    x: vec4<f32>,
    y: vec4<f32>,
    z: vec4<f32>,
    previous_id: u32,
    index_and_enabled: u32,
    scale: f32,
    tint: u32,
    instance_data: vec4<f32>,
}

@group(0) @binding(2) var vectors: texture_2d<f32>;
@group(0) @binding(3) var ids: texture_2d<u32>;
@group(0) @binding(4) var previous_ids: texture_2d<u32>;
@group(0) @binding(5) var<storage, read> previous_instances: array<PreviousInstance>;

@fragment
fn reproject(@builtin(position) position: vec4<f32>) -> HistoryAndColor {
    let pixel = vec2<i32>(position.xy);
    let current = textureLoad(source, pixel, 0);
    let id = textureLoad(ids, pixel, 0).x;
    let vector = textureLoad(vectors, pixel, 0);
    let current_sample = vec4<f32>(current.rgb, vector.w);
    // History stores eye depth in its alpha slot. Only reproject radiance
    // where the current surface fully covers the underlay.
    if current.a < 1.0 || id == 0u || vector.z < -32.0 {
        return HistoryAndColor(current_sample, current);
    }
    let expected_id = previous_instances[id - 1u].previous_id;
    if expected_id == 0u {
        return HistoryAndColor(current_sample, current);
    }
    let size = textureDimensions(source);
    let history_pixel = vec2<f32>(pixel) + vector.xy * vec2<f32>(size);
    let base = vec2<i32>(floor(history_pixel));
    let fraction = fract(history_pixel);
    var history_sum = vec3<f32>(0.0);
    var valid_weight = 0.0;
    for (var i = 0; i < 4; i += 1) {
        let offset = vec2<i32>(i & 1, i >> 1);
        let at = base + offset;
        if any(at < vec2<i32>(0)) || any(at >= vec2<i32>(size)) {
            continue;
        }
        if textureLoad(previous_ids, at, 0).x != expected_id {
            continue;
        }
        let history = textureLoad(previous, at, 0);
        // Compare linear eye depths in log space so near and far surfaces use
        // the same relative tolerance, including at float16 precision.
        if abs(history.a - vector.z) > 0.025 {
            continue;
        }
        let weights = select(1.0 - fraction, fraction, offset != vec2<i32>(0));
        let weight = weights.x * weights.y;
        valid_weight += weight;
        history_sum += history.rgb * weight;
    }
    if valid_weight <= 0.0001 {
        return HistoryAndColor(current_sample, current);
    }
    // Constrain feedback to the current neighborhood to keep changes in
    // lighting and moving silhouettes from leaving an extended trail.
    var lo = current.rgb;
    var hi = current.rgb;
    let neighbors = array<vec2<i32>, 4>(vec2<i32>(-1, 0), vec2<i32>(1, 0), vec2<i32>(0, -1), vec2<i32>(0, 1));
    for (var i = 0; i < 4; i += 1) {
        let at = clamp(pixel + neighbors[i], vec2<i32>(0), vec2<i32>(size) - 1);
        let neighbor = textureLoad(source, at, 0).rgb;
        lo = min(lo, neighbor);
        hi = max(hi, neighbor);
    }
    let previous_color = clamp(history_sum / valid_weight, lo, hi);
    let filtered = mix(current.rgb, previous_color, valid_weight * 0.8);
    return HistoryAndColor(vec4<f32>(filtered, vector.w), vec4<f32>(filtered, current.a));
}

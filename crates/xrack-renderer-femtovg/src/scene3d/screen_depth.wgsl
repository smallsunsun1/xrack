override OBLIQUE_CLIP_DEPTH: bool = false;
override MATRIX_CLIP_PROJECTION: bool = false;
override REVERSE_DEVICE_DEPTH: bool = false;

fn clear_clip_depth() -> f32 {
    return select(1.0, 0.0, REVERSE_DEVICE_DEPTH);
}

fn is_clear_clip_depth(depth: f32) -> bool {
    return select(depth >= 1.0, depth <= 0.0, REVERSE_DEVICE_DEPTH);
}

fn closest_clip_depth(a: f32, b: f32) -> f32 {
    return select(min(a, b), max(a, b), REVERSE_DEVICE_DEPTH);
}

fn nearer_clip_depth(candidate: f32, baseline: f32) -> bool {
    if REVERSE_DEVICE_DEPTH {
        return candidate > baseline;
    }
    return candidate < baseline;
}

struct ClipDepthMapping {
    // Lateral scale and offset in the active raster target.
    lens: vec4<f32>,
    // Optical-axis near/far (zero means infinite), orthographic flag, near/far ratio.
    distances: vec4<f32>,
    // Depth slopes and conservative visible depth limits.
    tilt_limits: vec4<f32>,
    view_to_clip: array<vec4<f32>, 4>,
    clip_to_view: array<vec4<f32>, 4>,
    viewer: vec4<f32>,
}

fn screen_clip(uv: vec2<f32>, depth: f32) -> vec4<f32> {
    return vec4<f32>((uv * 2.0 - 1.0) * vec2<f32>(1.0, -1.0), depth, 1.0);
}

fn matrix_clip_from_view(mapping: ClipDepthMapping, point: vec3<f32>) -> vec4<f32> {
    let p = vec4<f32>(point, 1.0);
    return vec4<f32>(dot(mapping.view_to_clip[0], p), dot(mapping.view_to_clip[1], p),
        dot(mapping.view_to_clip[2], p), dot(mapping.view_to_clip[3], p));
}

fn view_plane(mapping: ClipDepthMapping, uv: vec2<f32>) -> vec2<f32> {
    let ndc = (uv * 2.0 - 1.0) * vec2<f32>(1.0, -1.0);
    return (ndc - mapping.lens.zw) / mapping.lens.xy;
}

fn depth_from_clip(mapping: ClipDepthMapping, clip: f32, offset: f32) -> f32 {
    let near = mapping.distances.x;
    let far = mapping.distances.y;
    if mapping.distances.z > 0.5 {
        if REVERSE_DEVICE_DEPTH {
            return mix(far, near, clip) - offset;
        }
        return mix(near, far, clip) - offset;
    }
    if REVERSE_DEVICE_DEPTH {
        return near / max(clip + (1.0 - clip) * mapping.distances.w + offset, 1e-12);
    }
    return near / max((1.0 - clip) + clip * mapping.distances.w + offset, 1e-12);
}

fn depth_at_pixel(
    mapping: ClipDepthMapping, pixel: vec2<i32>, size: vec2<f32>, clip: f32
) -> f32 {
    if MATRIX_CLIP_PROJECTION {
        let p = screen_clip((vec2<f32>(pixel) + 0.5) / size, clip);
        return dot(mapping.clip_to_view[2], p) / max(dot(mapping.clip_to_view[3], p), 1e-12);
    }
    var offset = 0.0;
    if OBLIQUE_CLIP_DEPTH {
        let uv = (vec2<f32>(pixel) + 0.5) / size;
        offset = dot(mapping.tilt_limits.xy, view_plane(mapping, uv));
    }
    return depth_from_clip(mapping, clip, offset);
}

fn view_point_from_clip(mapping: ClipDepthMapping, uv: vec2<f32>, clip: f32) -> vec3<f32> {
    if MATRIX_CLIP_PROJECTION {
        let p = screen_clip(uv, clip);
        let view = vec3<f32>(dot(mapping.clip_to_view[0], p), dot(mapping.clip_to_view[1], p),
                            dot(mapping.clip_to_view[2], p));
        return view / max(dot(mapping.clip_to_view[3], p), 1e-12);
    }
    let plane = view_plane(mapping, uv);
    var offset = 0.0;
    if OBLIQUE_CLIP_DEPTH {
        offset = dot(mapping.tilt_limits.xy, plane);
    }
    let depth = depth_from_clip(mapping, clip, offset);
    return vec3<f32>(plane * select(depth, 1.0, mapping.distances.z > 0.5), depth);
}

@group(0) @binding(0) var capture_color: texture_2d<f32>;
@group(0) @binding(1) var capture_depth: texture_2d<f32>;
@group(0) @binding(2) var<uniform> uv_offset: vec2<f32>;

override HAS_COLOR: bool = false;
override HAS_DEPTH: bool = false;
override REVERSE_DEVICE_DEPTH: bool = false;

struct Vertex {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn fullscreen(@builtin(vertex_index) index: u32) -> Vertex {
    let uv = vec2(f32((index << 1u) & 2u), f32(index & 2u));
    return Vertex(vec4(2.0 * uv.x - 1.0, 1.0 - 2.0 * uv.y, 0.0, 1.0), uv);
}

struct Composite {
    @location(0) color: vec4<f32>,
    @builtin(frag_depth) depth: f32,
}

fn pixel(uv: vec2<f32>, size: vec2<u32>) -> vec2<u32> {
    return min(vec2<u32>(uv * vec2<f32>(size)), size - 1u);
}

fn source_pixel(vertex: Vertex) -> Composite {
    // A capture uses the stable camera. Match a jittered viewport sample to
    // the same ray, without modifying the camera-space depth value.
    let uv = vertex.uv + uv_offset;
    if any(uv < vec2(0.0)) || any(uv >= vec2(1.0)) {
        discard;
    }
    let clear_depth = select(1.0, 0.0, REVERSE_DEVICE_DEPTH);
    var depth = clear_depth;
    if HAS_DEPTH {
        depth = xr_capture_depth_value(
            textureLoad(capture_depth, pixel(uv, textureDimensions(capture_depth)), 0).r,
        );
    }
    var color = vec4(0.0);
    if HAS_COLOR {
        color = textureLoad(capture_color, pixel(uv, textureDimensions(capture_color)), 0);
    }
    if depth == clear_depth && color.a == 0.0 {
        discard;
    }
    return Composite(color, depth);
}

@fragment
fn composite(vertex: Vertex) -> Composite {
    return source_pixel(vertex);
}

@fragment
fn composite_named_color(vertex: Vertex) -> @location(4) vec4<f32> {
    return source_pixel(vertex).color;
}

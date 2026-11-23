@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> options: vec4<f32>;

struct VertexOut {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> VertexOut {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return VertexOut(vec4<f32>(uv * vec2<f32>(2.0, -2.0) + vec2<f32>(-1.0, 1.0), 0.0, 1.0), uv);
}

fn radiance(uv: vec2<f32>) -> vec3<f32> {
    return textureSampleLevel(source, source_sampler, uv, 0.0).rgb;
}

fn highlight(color: vec3<f32>) -> vec3<f32> {
    let brightness = dot(color, vec3<f32>(0.2126, 0.7152, 0.0722));
    let threshold = options.x;
    let knee = max(threshold * 0.5, 0.0001);
    let soft = clamp(brightness - threshold + knee, 0.0, 2.0 * knee);
    let contribution = max(brightness - threshold, soft * soft / (4.0 * knee));
    return color * (contribution / max(brightness, 0.0001));
}

@fragment
fn extract(vertex: VertexOut) -> @location(0) vec4<f32> {
    let d = 0.5 / vec2<f32>(textureDimensions(source));
    let uv = vertex.uv;
    let color = highlight(radiance(uv - d)) + highlight(radiance(uv + d))
        + highlight(radiance(uv + vec2<f32>(d.x, -d.y)))
        + highlight(radiance(uv + vec2<f32>(-d.x, d.y)));
    return vec4<f32>(color * 0.25, 1.0);
}

@fragment
fn reduce(vertex: VertexOut) -> @location(0) vec4<f32> {
    let d = 0.75 / vec2<f32>(textureDimensions(source));
    let uv = vertex.uv;
    let corners = radiance(uv - d) + radiance(uv + d)
        + radiance(uv + vec2<f32>(d.x, -d.y)) + radiance(uv + vec2<f32>(-d.x, d.y));
    return vec4<f32>(corners * 0.1875 + radiance(uv) * 0.25, 1.0);
}

@fragment
fn expand(vertex: VertexOut) -> @location(0) vec4<f32> {
    let d = 1.0 / vec2<f32>(textureDimensions(source));
    let uv = vertex.uv;
    let x = vec2<f32>(d.x, 0.0);
    let y = vec2<f32>(0.0, d.y);
    let corners = radiance(uv - d) + radiance(uv + d) + radiance(uv + x - y) + radiance(uv - x + y);
    let edges = radiance(uv - x) + radiance(uv + x) + radiance(uv - y) + radiance(uv + y);
    let blurred = (corners + edges * 2.0 + radiance(uv) * 4.0) * 0.0625;
    // Mix with the target level at unit gain, including uniform highlights.
    return vec4<f32>(blurred * 0.67, 0.67);
}

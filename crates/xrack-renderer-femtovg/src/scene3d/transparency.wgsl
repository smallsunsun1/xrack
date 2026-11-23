@group(0) @binding(0) var accumulation: texture_2d<f32>;
@group(0) @binding(1) var revealage: texture_2d<f32>;
@group(0) @binding(2) var samples_accumulation: texture_multisampled_2d<f32>;
@group(0) @binding(3) var samples_revealage: texture_multisampled_2d<f32>;
@group(0) @binding(4) var samples_opaque: texture_multisampled_2d<f32>;

@vertex
fn vertex(@builtin(vertex_index) index: u32) -> @builtin(position) vec4<f32> {
    let uv = vec2<f32>(f32((index << 1u) & 2u), f32(index & 2u));
    return vec4<f32>(uv * 2.0 - 1.0, 0.0, 1.0);
}

fn compose(accum: vec4<f32>, reveal: f32) -> vec4<f32> {
    let alpha = 1.0 - clamp(reveal, 0.0, 1.0);
    if alpha <= 0.0 || accum.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let color = min(accum.rgb / accum.a, vec3<f32>(65000.0));
    return vec4<f32>(color * alpha, alpha);
}

@fragment
fn fragment(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(point.xy);
    return compose(textureLoad(accumulation, pixel, 0), textureLoad(revealage, pixel, 0).r);
}

@fragment
fn fragment_multisampled(@builtin(position) point: vec4<f32>) -> @location(0) vec4<f32> {
    let pixel = vec2<i32>(point.xy);
    // Resolve after combining each sample with the corresponding opaque
    // color. A single invocation works without sample-rate shading support.
    let samples = textureNumSamples(samples_opaque);
    var combined = vec4<f32>(0.0);
    for (var sample = 0u; sample < samples; sample += 1u) {
        let transparent = compose(textureLoad(samples_accumulation, pixel, sample),
                                  textureLoad(samples_revealage, pixel, sample).r);
        combined += transparent + textureLoad(samples_opaque, pixel, sample) * (1.0 - transparent.a);
    }
    return combined / f32(samples);
}

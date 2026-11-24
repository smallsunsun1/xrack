fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let current = scene_color(uv);
    let previous = scene_buffer_color(0u, uv);
    let decay = clamp(scene_effect_parameters().x, 0.0, 1.0);
    let trail = previous.a * decay * (1.0 - current.a);
    let coverage = current.a + trail;
    return vec4((current.rgb * current.a + previous.rgb * trail) / max(coverage, 0.000001), coverage);
}

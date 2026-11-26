fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let original = scene_color(uv);
    let layer = scene_buffer_color(0u, uv);
    return vec4<f32>(layer.g, 0.0, layer.g, max(original.a, layer.a));
}

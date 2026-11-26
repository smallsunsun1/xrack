fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    return vec4<f32>(scene_buffer_color(0u, uv).rgb * 0.25, 1.0);
}

fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let color = scene_color(uv);
    let first = scene_buffer_color(0u, uv).r;
    let second = scene_buffer_color(1u, uv).r;
    let third = scene_buffer_color(2u, uv).r / 131072.0;
    return vec4(first, second, third, color.a);
}

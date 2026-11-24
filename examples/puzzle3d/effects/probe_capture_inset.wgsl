fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if any(uv < vec2(0.74, 0.06)) || any(uv > vec2(0.96, 0.28)) {
        return scene_color(uv);
    }
    let signal = scene_buffer_color(0u, (uv - vec2(0.74, 0.06)) / 0.22);
    return vec4(mix(vec3(0.035, 0.055, 0.07), signal.rgb, signal.a), 1.0);
}

fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if any(uv < vec2(0.74, 0.06)) || any(uv > vec2(0.96, 0.28)) {
        return scene_color(uv);
    }
    let local_uv = (uv - vec2(0.74, 0.06)) / 0.22;
    let signal = scene_buffer_color(0u, local_uv);
    var shade = 1.0;
    if scene_has_buffer(1u) {
        shade = 0.65 + 0.35 * (1.0 - scene_buffer_color(1u, local_uv).r);
    }
    return vec4(mix(vec3(0.035, 0.055, 0.07), signal.rgb, signal.a) * shade, 1.0);
}

fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    if any(uv < vec2(0.425, 0.035)) || any(uv > vec2(0.575, 0.22)) {
        return scene_color(uv);
    }
    let feed = scene_buffer_color(0u, (uv - vec2(0.425, 0.035)) / vec2(0.15, 0.185));
    return vec4(mix(vec3(0.035, 0.055, 0.07), feed.rgb, feed.a), 1.0);
}

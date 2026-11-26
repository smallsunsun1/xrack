fn scene_effect(uv: vec2<f32>) -> vec4<f32> {
    let original = scene_color(uv);
    if uv.x < 0.5 || !scene_has_buffer(0u) {
        return original;
    }
    let input = select(0u, 1u, uv.x >= 0.58 && scene_has_buffer(1u));
    return vec4(scene_buffer_color(input, uv).rgb, original.a);
}
